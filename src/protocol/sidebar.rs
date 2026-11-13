//! Synced sidebar documents (kind 30078, NIP-44 encrypted to yourself):
//! channel sections (`d=channel-sections`) and per-group sort modes
//! (`d=channel-sort`), wire-compatible with Buzz desktop, mobile and the
//! `buzz sidebar` CLI. The register-tree codec is [`super::sidebar_doc`].
//!
//! Reading never overwrites: an unreadable head (not ours, bad ciphertext,
//! unknown `meta.v`) leaves bzz's own grouping in place, and a write refuses
//! to replace it.

use nostr::{Event, EventBuilder, Kind, Tag, Timestamp};
use serde_json::{Value, json};

use crate::{
    auth::signer::SignerHandle,
    error::{Error, Result},
    protocol::sidebar_doc::{self, Codec, Edit, Stamp, Tree},
};

pub const KIND: u16 = 30_078;

/// Relay event size ceiling shared with desktop's sidebar blobs.
const MAX_CIPHERTEXT_BYTES: usize = 65_535;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lane {
    Sections,
    Sort,
}

impl Lane {
    pub const ALL: [Self; 2] = [Self::Sections, Self::Sort];

    pub const fn d_tag(self) -> &'static str {
        match self {
            Self::Sections => "channel-sections",
            Self::Sort => "channel-sort",
        }
    }

    pub const fn codec(self) -> &'static Codec {
        match self {
            Self::Sections => &sidebar_doc::SECTIONS,
            Self::Sort => &sidebar_doc::SORT,
        }
    }

    pub fn from_d_tag(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|lane| lane.d_tag() == value)
    }

    /// The lane of a 30078 event, if it is a sidebar document.
    pub fn of(event: &Event) -> Option<Self> {
        if event.kind.as_u16() != KIND {
            return None;
        }
        event.tags.identifier().and_then(Self::from_d_tag)
    }
}

/// Fixed sort groups desktop stores; sections use `section:<id>`.
pub const FIXED_GROUPS: [&str; 4] = ["starred", "channels", "forums", "dms"];
pub const SECTION_PREFIX: &str = "section:";

/// Whether `candidate` replaces `current` as the head: newer `created_at`,
/// then the smaller event id (as the CLI's head selection).
pub fn newer(candidate: (u64, &str), current: Option<(u64, &str)>) -> bool {
    current.is_none_or(|(at, id)| candidate.0 > at || (candidate.0 == at && candidate.1 < id))
}

/// Decrypt and decode a self-authored sidebar event into its register tree.
/// A meta-less (mobile) payload is imported at the event's `created_at`.
pub async fn decode_event(event: &Event, signer: &SignerHandle) -> Result<(Lane, Tree)> {
    let lane = Lane::of(event).ok_or_else(|| Error::Protocol("not a sidebar document".into()))?;
    if event.pubkey != signer.public_key() {
        return Err(Error::Protocol("sidebar document is not yours".into()));
    }
    crate::protocol::events::verify(event)?;
    let plaintext = signer
        .decrypt_self(event.content.clone())
        .await
        .map_err(|_| Error::Protocol("sidebar document is not readable with your key".into()))?;
    let json: Value = serde_json::from_str(&plaintext)
        .map_err(|_| Error::Protocol("sidebar document is not JSON".into()))?;
    let legacy_v =
        i64::try_from(event.created_at.as_secs().saturating_mul(1_000)).unwrap_or(i64::MAX);
    let tree = sidebar_doc::decode(lane.codec(), &json, legacy_v).ok_or_else(|| {
        Error::Unsupported(
            "your sidebar document has a newer format; bzz leaves it unchanged".into(),
        )
    })?;
    Ok((lane, tree))
}

/// The relay's current head of `lane` for `author`: newest, then smallest id,
/// among verified self-authored events. `Err` when the relay returned events
/// but none is ours and valid, so a write never overwrites what it cannot read.
pub fn head(events: &[Event], lane: Lane, author: &nostr::PublicKey) -> Result<Option<Event>> {
    let mut best: Option<&Event> = None;
    for event in events {
        if event.pubkey != *author
            || Lane::of(event) != Some(lane)
            || crate::protocol::events::verify(event).is_err()
        {
            continue;
        }
        let id = event.id.to_hex();
        if newer(
            (event.created_at.as_secs(), &id),
            best.map(|best| (best.created_at.as_secs(), best.id.to_hex()))
                .as_ref()
                .map(|(at, id)| (*at, id.as_str())),
        ) {
            best = Some(event);
        }
    }
    if best.is_none() && !events.is_empty() {
        return Err(Error::Protocol(
            "the relay returned a sidebar document that is not yours or fails verification; bzz will not overwrite it".into(),
        ));
    }
    Ok(best.cloned())
}

/// The strong head query for `lane`.
pub fn head_filter(lane: Lane, author: &str) -> Value {
    json!({
        "kinds": [KIND],
        "authors": [author],
        "#d": [lane.d_tag()],
        "limit": 1,
        "consistency": "strong",
    })
}

/// Apply `apply` to `head` and build the replacement event, encrypted to
/// yourself and stamped one second after the head at the earliest.
/// `Ok(None)` when the edit changes nothing.
pub async fn edit(
    lane: Lane,
    head: Option<(u64, Tree)>,
    signer: &SignerHandle,
    now_ms: i64,
    apply: impl FnOnce(&mut Edit<'_>, &Tree) -> Result<()>,
) -> Result<Option<EventBuilder>> {
    let (head_created_at, head) = head.unwrap_or_default();
    let mut tree = head.clone();
    let stamp = Stamp {
        now_ms,
        dev: format!("{:016x}", rand::random::<u64>()),
    };
    apply(&mut Edit::new(&mut tree, &stamp), &head)?;
    if tree == head {
        return Ok(None);
    }
    match lane {
        Lane::Sections if !sidebar_doc::project(&tree).within_limits() => {
            return Err(Error::Config(format!(
                "the sidebar supports at most {} sections and {} channel assignments",
                sidebar_doc::MAX_SECTIONS,
                sidebar_doc::MAX_ASSIGNMENTS
            )));
        }
        Lane::Sort if sidebar_doc::sort_groups(&tree).len() > sidebar_doc::MAX_SORT_GROUPS => {
            return Err(Error::Config("too many sidebar sort groups".into()));
        }
        _ => {}
    }
    let plaintext = serde_json::to_string(&sidebar_doc::encode(lane.codec(), &tree))
        .map_err(|error| Error::Serialization(error.to_string()))?;
    if plaintext.len() > usize::from(u16::MAX) {
        return Err(Error::Config(
            "the sidebar document would be too large".into(),
        ));
    }
    let content = signer.encrypt_self(plaintext).await?;
    if content.len() > MAX_CIPHERTEXT_BYTES {
        return Err(Error::Config(
            "the sidebar document would be too large".into(),
        ));
    }
    let now = u64::try_from(now_ms / 1_000).unwrap_or(0);
    let created_at = now.max(head_created_at.saturating_add(1));
    Ok(Some(
        EventBuilder::new(Kind::Custom(KIND), content)
            .tags([Tag::identifier(lane.d_tag()), Tag::hashtag(lane.d_tag())])
            .custom_created_at(Timestamp::from(created_at)),
    ))
}

/// The kind 9007 command that creates a Solo chat, exactly as Buzz 1.0
/// emits it (upstream `docs/solo-mobile-contract.md`): a private stream with
/// the `buzz.solo` label. The relay derives the creator; bzz never sends `P`.
pub fn solo_creation(channel: uuid::Uuid, name: &str) -> Result<EventBuilder> {
    Ok(buzz_sdk::build_create_channel(
        channel,
        name,
        Some(buzz_sdk::Visibility::Private),
        Some(buzz_sdk::ChannelKind::Stream),
        None,
        None,
        false,
    )
    .map_err(|error| Error::Config(error.to_string()))?
    .tag(Tag::parse(["t", "buzz.solo"]).map_err(|error| Error::Protocol(error.to_string()))?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::Keys;

    async fn signed(keys: &Keys, lane: Lane, plaintext: &str, at: u64) -> Event {
        let signer = SignerHandle::spawn(keys.clone());
        let content = signer.encrypt_self(plaintext.into()).await.unwrap();
        EventBuilder::new(Kind::Custom(KIND), content)
            .tags([Tag::identifier(lane.d_tag())])
            .custom_created_at(Timestamp::from(at))
            .sign_with_keys(keys)
            .unwrap()
    }

    #[tokio::test]
    async fn a_mobile_head_decodes_and_an_edit_publishes_after_it() {
        let keys = Keys::generate();
        let signer = SignerHandle::spawn(keys.clone());
        let mobile =
            r#"{"version":1,"sections":[{"id":"x","name":"Team","order":0}],"assignments":{}}"#;
        let event = signed(&keys, Lane::Sections, mobile, 2_000_000_000).await;
        let (lane, tree) = decode_event(&event, &signer).await.unwrap();
        assert_eq!(lane, Lane::Sections);
        assert_eq!(sidebar_doc::project(&tree).sections[0].name, "Team");
        let channel = uuid::Uuid::new_v4().to_string();
        let builder = edit(
            Lane::Sections,
            Some((2_000_000_000, tree)),
            &signer,
            1_000,
            |edit, _| {
                edit.set(&["a", &channel], json!("x"));
                Ok(())
            },
        )
        .await
        .unwrap()
        .unwrap();
        let published = builder.sign_with_keys(&keys).unwrap();
        assert_eq!(
            published.created_at.as_secs(),
            2_000_000_001,
            "after the head"
        );
        assert_eq!(Lane::of(&published), Some(Lane::Sections));
        let (_, tree) = decode_event(&published, &signer).await.unwrap();
        assert_eq!(sidebar_doc::project(&tree).assignments[&channel], "x");
        // Writing what is already there publishes nothing.
        let unchanged = edit(
            Lane::Sections,
            Some((2_000_000_001, tree)),
            &signer,
            1_000,
            |edit, _| {
                edit.set(&["a", &channel], json!("x"));
                Ok(())
            },
        )
        .await
        .unwrap();
        assert!(unchanged.is_none());
    }

    #[tokio::test]
    async fn unreadable_and_foreign_heads_are_refused() {
        let keys = Keys::generate();
        let signer = SignerHandle::spawn(keys.clone());
        let future = signed(&keys, Lane::Sort, r#"{"version":1,"meta":{"v":2}}"#, 10).await;
        assert!(matches!(
            decode_event(&future, &signer).await,
            Err(Error::Unsupported(_))
        ));
        let other = Keys::generate();
        let foreign = signed(&other, Lane::Sort, r#"{"version":1,"groups":{}}"#, 10).await;
        assert!(decode_event(&foreign, &signer).await.is_err());
        assert!(head(&[foreign], Lane::Sort, &keys.public_key()).is_err());
        assert!(head(&[], Lane::Sort, &keys.public_key()).unwrap().is_none());
        let newest = signed(&keys, Lane::Sort, r#"{"version":1,"groups":{}}"#, 20).await;
        let picked = head(&[future, newest.clone()], Lane::Sort, &keys.public_key()).unwrap();
        assert_eq!(picked.map(|event| event.id), Some(newest.id));
    }

    /// The upstream fixture of a captured Buzz 1.0 Solo creation command.
    #[test]
    fn solo_creation_matches_the_buzz_1_0_command() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/buzz/solo-creation.json"))
                .unwrap();
        let channel = uuid::Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
        let event = solo_creation(channel, "Desktop Solo")
            .unwrap()
            .sign_with_keys(&Keys::generate())
            .unwrap();
        assert_eq!(
            u64::from(event.kind.as_u16()),
            fixture["creation"]["kind"].as_u64().unwrap()
        );
        assert_eq!(
            event.content,
            fixture["creation"]["content"].as_str().unwrap()
        );
        let tags = event
            .tags
            .iter()
            .map(|tag| tag.as_slice().to_vec())
            .collect::<Vec<_>>();
        let expected: Vec<Vec<String>> =
            serde_json::from_value(fixture["creation"]["tags"].clone()).unwrap();
        assert_eq!(tags, expected);
    }
}
