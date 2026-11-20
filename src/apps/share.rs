//! An app offered in a channel, on the wire (plan v0.16.0 M6; ADR v0.16
//! apps, decision 9).
//!
//! **One artifact of type `bzz.app` per offered app**, homed in the
//! channel, with the client tag `["app", <name>]` and this content:
//!
//! ```json
//! {"v":1,
//!  "manifest":{"name":"board","title":"Board","api":1,"types":["card"],
//!              "script_sha256":"<hex of the script's SHA-256>"},
//!  "brief":"<the brief, text>",
//!  "script":"<the Lua source, text>"}
//! ```
//!
//! **An offer is one revision and its signer.** Any writer of the channel
//! can publish a new revision of an offer, delete it or restore it (M0,
//! question 3). Whoever signed the head revision is who offers what it
//! holds; a revision signed by someone else is an offer from that person.
//!
//! **Everything is checked on both sides.** Before a revision is sent
//! ([`content`], [`check`]): a script of at most [`SCRIPT_BYTES`] of text, a
//! brief of at most [`BRIEF_BYTES`], content the relay takes, and a frame
//! under the relay's limit after JSON escaping (past it the relay drops the
//! connection without an answer). On arrival ([`read`]): the same sizes, the
//! shape, an `api` this bzz knows, a name an app may have, the tag and the
//! manifest naming the same app, and **the script's hash** against the
//! manifest. What fails is refused with the reason, and is never run.
//!
//! This module is pure: events in, values out. `service::offers` talks to
//! the relay; `apps::store::offers` takes an offer in as a waiting version.

use nostr::{Event, EventBuilder, Kind, Tag};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use crate::apps::runtime::API_VERSION;
use crate::apps::store::Bundle;
use crate::apps::store::manifest::{
    BRIEF_BYTES, Manifest, SCRIPT_BYTES, check_brief, check_name, sha256,
};
use crate::protocol::{
    artifact::{self, APP_OFFERS_SUBSCRIPTION_PREFIX, Artifact, KIND_ARTIFACT},
    envelope,
    record::MAX_FRAME_BYTES,
};

/// The artifact type of an offered app.
pub const OFFER_TYPE: &str = crate::protocol::artifact::APP_OFFER_TYPE;
/// The number of the content's format.
pub const FORMAT: u64 = 1;
/// The client tag that names the app on every revision.
pub const APP_TAG: &str = "app";
/// The relay's limit on an event's content, at the pin.
pub const MAX_CONTENT_BYTES: usize = 262_144;

/// Why an offer is not taken, or not sent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// The content is not an offer of this format.
    NotTheShape(String),
    /// It says one thing and holds another: the script's hash is not the
    /// manifest's, or the tag names another app.
    Tampered(String),
    /// A script, a brief, the content or the frame over its limit.
    TooLarge(String),
    /// The contract number is one this bzz does not know.
    UnknownApi(i64),
    /// The name is not one an app may have.
    Name(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotTheShape(why) => write!(formatter, "it is not an app offer bzz reads: {why}"),
            Self::Tampered(why) => write!(formatter, "it was changed on the way: {why}"),
            Self::TooLarge(why) => write!(formatter, "it is too large: {why}"),
            Self::UnknownApi(api) => write!(
                formatter,
                "its script is written for api = {api}, and this bzz knows api = {API_VERSION}"
            ),
            Self::Name(why) => write!(formatter, "{why}"),
        }
    }
}

/// One revision of an offer, read and checked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Offer {
    /// The artifact (`d`).
    pub artifact: Uuid,
    pub channel: Uuid,
    /// The event id of this revision.
    pub revision: String,
    /// Who signed this revision (public key, hex).
    pub signer: String,
    /// The signer's clock, in seconds.
    pub signed_at: u64,
    /// Name, title, api and types, checked as a kept manifest is.
    pub manifest: Manifest,
    pub script: Vec<u8>,
    pub brief: String,
    /// The SHA-256 of the script, which the manifest names and the bytes
    /// were found to have.
    pub script_sha256: String,
}

impl Offer {
    pub fn brief_sha256(&self) -> String {
        sha256(self.brief.as_bytes())
    }
}

/// What a head of type `bzz.app` in a channel stands for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Head {
    Offer(Box<Offer>),
    /// Deleted, or moved out of the channel: no longer offered there.
    Withdrawn {
        artifact: Uuid,
        revision: String,
        /// Who signed the deletion.
        by: String,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    v: u64,
    manifest: WireManifest,
    brief: String,
    script: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireManifest {
    name: String,
    title: String,
    api: i64,
    types: Vec<String>,
    script_sha256: String,
}

/// The one sync of offers in a channel, apart from any app's records: a
/// resync of records then fetches no script.
pub fn sync_filter(channel: Uuid) -> Value {
    json!({"artifact": "current", "#h": [channel.to_string()], "#type": [OFFER_TYPE]})
}

/// The subscription of that sync. At most 64 bytes.
pub fn subscription_id(channel: Uuid) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"bzz-app-offers\0");
    hasher.update(channel.as_bytes());
    format!(
        "{APP_OFFERS_SUBSCRIPTION_PREFIX}{}",
        &hex::encode(hasher.finalize())[..24]
    )
}

/// Reads a head delivered for `channel`. A deletion, or a head homed in
/// another channel (moved away), is [`Head::Withdrawn`]; anything else is
/// an offer once every check holds.
pub fn read(head: &Artifact, channel: Uuid) -> Result<Head, Refusal> {
    if head.artifact_type != OFFER_TYPE {
        return Err(Refusal::NotTheShape(format!(
            "its type is {}, not {OFFER_TYPE}",
            artifact::display(&head.artifact_type, 40)
        )));
    }
    if head.is_deleted() || head.home != channel {
        return Ok(Head::Withdrawn {
            artifact: head.id,
            revision: head.head.clone(),
            by: head.author.clone(),
        });
    }
    let content = &head.event.content;
    if content.len() > MAX_CONTENT_BYTES {
        return Err(Refusal::TooLarge(format!(
            "its content is {} bytes; the relay takes {MAX_CONTENT_BYTES}",
            content.len()
        )));
    }
    let value: Value = serde_json::from_str(content)
        .map_err(|_| Refusal::NotTheShape("its content is not JSON".into()))?;
    match value.get("v").and_then(Value::as_u64) {
        Some(FORMAT) => {}
        Some(other) => {
            return Err(Refusal::NotTheShape(format!(
                "it is of format v = {other}; this bzz reads v = {FORMAT}"
            )));
        }
        None => return Err(Refusal::NotTheShape("it has no format number v".into())),
    }
    // The contract number first: a script of another contract is refused
    // for that, whatever else its manifest says.
    if let Some(api) = value.pointer("/manifest/api").and_then(Value::as_i64)
        && api != API_VERSION
    {
        return Err(Refusal::UnknownApi(api));
    }
    let wire: Wire =
        serde_json::from_value(value).map_err(|error| Refusal::NotTheShape(error.to_string()))?;
    check_name(&wire.manifest.name).map_err(Refusal::Name)?;
    let tagged = head.tag(APP_TAG);
    if tagged.as_deref() != Some(wire.manifest.name.as_str()) {
        return Err(Refusal::Tampered(format!(
            "its tag names the app {}, and its manifest {}",
            tagged.map_or_else(|| "nothing".to_owned(), |name| artifact::display(&name, 40)),
            wire.manifest.name
        )));
    }
    if wire.script.len() > SCRIPT_BYTES {
        return Err(Refusal::TooLarge(format!(
            "its script is {} bytes; a script may be {SCRIPT_BYTES}",
            wire.script.len()
        )));
    }
    if wire.brief.len() > BRIEF_BYTES {
        return Err(Refusal::TooLarge(format!(
            "its brief is {} bytes; a brief may be {BRIEF_BYTES}",
            wire.brief.len()
        )));
    }
    check_brief(wire.brief.as_bytes()).map_err(Refusal::NotTheShape)?;
    let found = sha256(wire.script.as_bytes());
    if !found.eq_ignore_ascii_case(&wire.manifest.script_sha256) {
        return Err(Refusal::Tampered(format!(
            "its manifest names the script {}…, and the script it holds is {}…",
            artifact::display(&wire.manifest.script_sha256, 12),
            &found[..12]
        )));
    }
    let manifest = Manifest {
        name: wire.manifest.name,
        title: wire.manifest.title,
        api: wire.manifest.api,
        types: wire.manifest.types,
    };
    // Every field as a kept manifest is held to: the title, the types.
    let manifest = Manifest::parse(&manifest.to_text()).map_err(Refusal::NotTheShape)?;
    Ok(Head::Offer(Box::new(Offer {
        artifact: head.id,
        channel,
        revision: head.head.clone(),
        signer: head.author.clone(),
        signed_at: head.created_at,
        manifest,
        script: wire.script.into_bytes(),
        brief: wire.brief,
        script_sha256: found,
    })))
}

/// The content of an offer of `bundle`, checked as the receiver will check
/// it: a script of text within its size, a brief within its size, and
/// content the relay takes.
pub fn content(bundle: &Bundle) -> Result<String, Refusal> {
    check_name(&bundle.manifest.name).map_err(Refusal::Name)?;
    if bundle.manifest.api != API_VERSION {
        return Err(Refusal::UnknownApi(bundle.manifest.api));
    }
    if bundle.script.len() > SCRIPT_BYTES {
        return Err(Refusal::TooLarge(format!(
            "the script is {} bytes; a script may be {SCRIPT_BYTES}",
            bundle.script.len()
        )));
    }
    if bundle.brief.len() > BRIEF_BYTES {
        return Err(Refusal::TooLarge(format!(
            "the brief is {} bytes; a brief may be {BRIEF_BYTES}",
            bundle.brief.len()
        )));
    }
    let script = std::str::from_utf8(&bundle.script).map_err(|_| {
        Refusal::NotTheShape("the script is not UTF-8 text, and only text travels".into())
    })?;
    let wire = Wire {
        v: FORMAT,
        manifest: WireManifest {
            name: bundle.manifest.name.clone(),
            title: bundle.manifest.title.clone(),
            api: bundle.manifest.api,
            types: bundle.manifest.types.clone(),
            script_sha256: sha256(&bundle.script),
        },
        brief: bundle.brief.clone(),
        script: script.to_owned(),
    };
    let json =
        serde_json::to_string(&wire).map_err(|error| Refusal::NotTheShape(error.to_string()))?;
    if json.len() > MAX_CONTENT_BYTES {
        return Err(Refusal::TooLarge(format!(
            "with its characters escaped the offer is {} bytes; the relay takes {MAX_CONTENT_BYTES}",
            json.len()
        )));
    }
    Ok(json)
}

fn tag(parts: &[&str]) -> Result<Tag, Refusal> {
    Tag::parse(parts.iter().copied()).map_err(|error| Refusal::NotTheShape(error.to_string()))
}

fn title(bundle: &Bundle) -> String {
    let title = bundle.manifest.title.trim();
    if title.is_empty() {
        bundle.manifest.name.clone()
    } else {
        title.to_owned()
    }
}

/// The first revision of a new offer of `bundle` in `channel`.
pub fn create(channel: Uuid, id: Uuid, bundle: &Bundle) -> Result<EventBuilder, Refusal> {
    let content = content(bundle)?;
    let tags = vec![
        tag(&["ar", "1"])?,
        tag(&["d", &id.to_string()])?,
        tag(&["h", &channel.to_string()])?,
        tag(&["type", OFFER_TYPE])?,
        tag(&["title", &title(bundle)])?,
        tag(&["op", "create"])?,
        tag(&[APP_TAG, &bundle.manifest.name])?,
    ];
    Ok(EventBuilder::new(Kind::Custom(KIND_ARTIFACT), content).tags(tags))
}

/// A later version over the live head of an offer: a whole new snapshot,
/// whoever signed the head.
pub fn revise(head: &Artifact, bundle: &Bundle) -> Result<EventBuilder, Refusal> {
    if head.artifact_type != OFFER_TYPE {
        return Err(Refusal::NotTheShape(
            "the artifact is not an app offer".into(),
        ));
    }
    if head.is_deleted() {
        return Err(Refusal::NotTheShape("the offer was withdrawn".into()));
    }
    let content = content(bundle)?;
    let mut tags = vec![
        tag(&["ar", "1"])?,
        tag(&["d", &head.id.to_string()])?,
        tag(&["h", &head.home.to_string()])?,
        tag(&["type", OFFER_TYPE])?,
        tag(&["title", &title(bundle)])?,
        tag(&["op", "update"])?,
    ];
    if let Some(root) = &head.root {
        tags.push(tag(&["root", root])?);
    }
    tags.push(tag(&["prev", &head.head])?);
    tags.push(tag(&[APP_TAG, &bundle.manifest.name])?);
    Ok(EventBuilder::new(Kind::Custom(KIND_ARTIFACT), content).tags(tags))
}

/// The withdrawal of an offer: a delete revision over its head.
pub fn withdraw(head: &Artifact) -> Result<EventBuilder, Refusal> {
    artifact::build(&artifact::Change::Delete, head, None)
        .map_err(|error| Refusal::NotTheShape(error.to_string()))
}

/// The last look at a signed revision before it is sent: the relay's own
/// envelope validator, its content and its escaped frame. A frame over the
/// limit would make the relay drop the connection with no answer.
pub fn check(event: &Event) -> Result<(), Refusal> {
    buzz_core::artifact::validate(event)
        .map_err(|error| Refusal::NotTheShape(format!("the relay would refuse it: {error}")))?;
    if event.content.len() > MAX_CONTENT_BYTES {
        return Err(Refusal::TooLarge(format!(
            "its content is {} bytes; the relay takes {MAX_CONTENT_BYTES}",
            event.content.len()
        )));
    }
    let frame = envelope::publish(event).len();
    if frame > MAX_FRAME_BYTES {
        return Err(Refusal::TooLarge(format!(
            "as it is sent, escaped, it is {frame} bytes; the relay takes {MAX_FRAME_BYTES} and drops the connection past that"
        )));
    }
    Ok(())
}

/// The message that tells the channel an app was shared: for people on
/// Buzz desktop it is all they see of it (M0 question 4). The brief goes
/// whole; its bytes are at most 16 KiB.
pub fn brief_message(bundle: &Bundle, version: u32) -> String {
    format!(
        "I shared the app {} ({}, version {version}) in this channel.\nIn bzz, :apps shows the offer and Ctrl-Y lets you read and approve it; nothing of it runs on your machine before you do. Its records are artifacts of the types bzz.app.{}.*.\n\n{}",
        title(bundle),
        bundle.manifest.name,
        bundle.manifest.name,
        bundle.brief.trim_end()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::artifact::parse;
    use nostr::Keys;

    fn bundle(script: &str, brief: &str) -> Bundle {
        let manifest = Manifest {
            name: "board".into(),
            title: "Board".into(),
            api: API_VERSION,
            types: vec!["card".into(), "column".into()],
        };
        Bundle {
            script: script.as_bytes().to_vec(),
            manifest_text: manifest.to_text(),
            manifest,
            brief: brief.into(),
        }
    }

    fn signed(builder: EventBuilder, keys: &Keys) -> Artifact {
        parse(&builder.sign_with_keys(keys).unwrap()).unwrap()
    }

    /// A revision with any content and tags, as another client could sign.
    fn raw(keys: &Keys, channel: Uuid, content: &str, app: Option<&str>) -> Artifact {
        let mut tags = vec![
            Tag::parse(["ar", "1"]).unwrap(),
            Tag::parse(["d", &Uuid::new_v4().to_string()]).unwrap(),
            Tag::parse(["h", &channel.to_string()]).unwrap(),
            Tag::parse(["type", OFFER_TYPE]).unwrap(),
            Tag::parse(["title", "Board"]).unwrap(),
            Tag::parse(["op", "create"]).unwrap(),
        ];
        if let Some(app) = app {
            tags.push(Tag::parse([APP_TAG, app]).unwrap());
        }
        signed(
            EventBuilder::new(Kind::Custom(KIND_ARTIFACT), content).tags(tags),
            keys,
        )
    }

    fn wire(script: &str, brief: &str) -> Value {
        json!({"v":1,"manifest":{"name":"board","title":"Board","api":1,"types":["card"],
               "script_sha256": sha256(script.as_bytes())},"brief":brief,"script":script})
    }

    #[test]
    fn an_offer_reads_back_as_it_was_sent_with_its_signer() {
        let keys = Keys::generate();
        let channel = Uuid::new_v4();
        let sent = bundle("local app = {}\nreturn app\n", "A board.\ncard: title\n");
        let head = signed(create(channel, Uuid::new_v4(), &sent).unwrap(), &keys);
        check(&head.event).unwrap();
        assert_eq!(head.tag(APP_TAG).as_deref(), Some("board"));
        let Head::Offer(offer) = read(&head, channel).unwrap() else {
            panic!("an offer")
        };
        assert_eq!(offer.manifest, sent.manifest);
        assert_eq!(offer.script, sent.script);
        assert_eq!(offer.brief, sent.brief);
        assert_eq!(offer.script_sha256, sha256(&sent.script));
        assert_eq!(offer.signer, keys.public_key().to_hex());
        assert_eq!(
            (offer.artifact, offer.revision.as_str()),
            (head.id, head.head.as_str())
        );

        // Anyone may sign the next revision: it is that person's offer.
        let other = Keys::generate();
        let later = bundle("return {}\n", "A board, later.\n");
        let revised = signed(revise(&head, &later).unwrap(), &other);
        check(&revised.event).unwrap();
        assert_eq!(revised.tag("prev"), Some(head.head.clone()));
        let Head::Offer(offer) = read(&revised, channel).unwrap() else {
            panic!("an offer")
        };
        assert_eq!(offer.signer, other.public_key().to_hex());
        assert_eq!(offer.artifact, head.id);
        assert_eq!(offer.brief, "A board, later.\n");

        // A deletion, and a head homed elsewhere, are no longer offers here.
        let gone = signed(withdraw(&revised).unwrap(), &keys);
        check(&gone.event).unwrap();
        assert_eq!(
            read(&gone, channel).unwrap(),
            Head::Withdrawn {
                artifact: head.id,
                revision: gone.head.clone(),
                by: keys.public_key().to_hex()
            }
        );
        assert!(matches!(
            read(&head, Uuid::new_v4()).unwrap(),
            Head::Withdrawn { .. }
        ));
        assert!(
            revise(&gone, &later).is_err(),
            "a withdrawn offer is shared anew"
        );
    }

    #[test]
    fn a_tampered_artifact_is_refused_for_what_is_wrong() {
        let keys = Keys::generate();
        let channel = Uuid::new_v4();
        let script = "return {}\n";
        let refuse = |content: &str, app: Option<&str>| {
            read(&raw(&keys, channel, content, app), channel).unwrap_err()
        };
        // The script changed after its hash was written.
        let mut value = wire(script, "A board.");
        value["script"] = json!("os.exit()\n");
        let error = refuse(&value.to_string(), Some("board"));
        assert!(matches!(error, Refusal::Tampered(_)), "{error}");
        assert!(error.to_string().contains("changed on the way"), "{error}");
        // The tag names another app than the manifest.
        let error = refuse(&wire(script, "A board.").to_string(), Some("poll"));
        assert!(matches!(error, Refusal::Tampered(_)), "{error}");
        let error = refuse(&wire(script, "A board.").to_string(), None);
        assert!(matches!(error, Refusal::Tampered(_)), "{error}");
        // Not the shape.
        for content in [
            "",
            "not json",
            "[1]",
            "{}",
            &json!({"v":2,"manifest":{},"brief":"","script":""}).to_string(),
            &{
                let mut value = wire(script, "A board.");
                value["network"] = json!(true);
                value.to_string()
            },
            &{
                let mut value = wire(script, "A board.");
                value["manifest"]["types"] = json!([]);
                value.to_string()
            },
            &wire(script, "a\u{1b}[2J").to_string(),
            &wire(script, "  ").to_string(),
        ] {
            let error = refuse(content, Some("board"));
            assert!(
                matches!(error, Refusal::NotTheShape(_)),
                "{content}: {error}"
            );
        }
        // An api bzz does not know, whatever else is there.
        let mut value = wire(script, "A board.");
        value["manifest"]["api"] = json!(2);
        value["manifest"]["extra"] = json!(1);
        let error = refuse(&value.to_string(), Some("board"));
        assert_eq!(error, Refusal::UnknownApi(2));
        assert!(error.to_string().contains("this bzz knows api = 1"));
        // A name that is no app's.
        for name in ["Board", "con", "a.b"] {
            let mut value = wire(script, "A board.");
            value["manifest"]["name"] = json!(name);
            let error = refuse(&value.to_string(), Some(name));
            assert!(matches!(error, Refusal::Name(_)), "{name}: {error}");
        }
    }

    #[test]
    fn size_is_checked_before_sending_and_on_arrival() {
        let keys = Keys::generate();
        let channel = Uuid::new_v4();
        // At the limits: a script of 128 KiB and a brief of 16 KiB travel.
        let script = format!("--{}\nreturn {{}}\n", "x".repeat(SCRIPT_BYTES - 13));
        assert_eq!(script.len(), SCRIPT_BYTES);
        let brief = "b".repeat(BRIEF_BYTES);
        let largest = bundle(&script, &brief);
        let head = signed(create(channel, Uuid::new_v4(), &largest).unwrap(), &keys);
        check(&head.event).unwrap();
        assert!(matches!(read(&head, channel).unwrap(), Head::Offer(_)));

        // One byte more of either is refused before anything is sent.
        let over = bundle(&format!("{script}-"), "A board.");
        assert!(matches!(content(&over), Err(Refusal::TooLarge(_))));
        assert!(matches!(
            create(
                channel,
                Uuid::new_v4(),
                &bundle("return {}", &format!("{brief}b"))
            ),
            Err(Refusal::TooLarge(_))
        ));
        // Escaping counts: a script of 128 KiB of newlines is 256 KiB of
        // JSON, over what the relay takes.
        let newlines = bundle(&"\n".repeat(SCRIPT_BYTES), "A board.");
        let error = content(&newlines).unwrap_err();
        assert!(error.to_string().contains("escaped"), "{error}");
        // A script that is not text does not travel.
        let mut binary = bundle("return {}", "A board.");
        binary.script = vec![0x1b, 0x4c, 0x75, 0x61, 0xff];
        assert!(matches!(content(&binary), Err(Refusal::NotTheShape(_))));

        // On arrival, from a client that does not check: the same limits.
        let big = format!("--{}\n", "x".repeat(SCRIPT_BYTES));
        let error = read(
            &raw(
                &keys,
                channel,
                &wire(&big, "A board.").to_string(),
                Some("board"),
            ),
            channel,
        )
        .unwrap_err();
        assert!(matches!(error, Refusal::TooLarge(_)), "{error}");
        let error = read(
            &raw(
                &keys,
                channel,
                &wire("return {}", &"b".repeat(BRIEF_BYTES + 1)).to_string(),
                Some("board"),
            ),
            channel,
        )
        .unwrap_err();
        assert!(matches!(error, Refusal::TooLarge(_)), "{error}");

        // The frame: content the relay takes can still escape past its
        // frame. Control characters are six bytes each in the frame.
        let noisy = raw(&keys, channel, &"\u{1}".repeat(100_000), Some("board"));
        let error = check(&noisy.event).unwrap_err();
        assert!(
            error.to_string().contains("drops the connection"),
            "{error}"
        );
    }

    #[test]
    fn the_sync_of_offers_is_its_own() {
        let channel = Uuid::new_v4();
        assert_eq!(
            sync_filter(channel),
            json!({"artifact":"current","#h":[channel.to_string()],"#type":["bzz.app"]})
        );
        let id = subscription_id(channel);
        assert!(id.starts_with(APP_OFFERS_SUBSCRIPTION_PREFIX) && id.len() <= 64);
        assert_ne!(id, subscription_id(Uuid::new_v4()));
        assert_ne!(
            id,
            crate::protocol::record::Fence::subscription_of("board", channel)
        );
    }

    #[test]
    fn the_brief_message_says_what_was_shared_and_holds_the_brief() {
        let text = brief_message(&bundle("return {}", "A board.\ncard: title\n\n"), 3);
        assert!(
            text.starts_with("I shared the app Board (board, version 3)"),
            "{text}"
        );
        assert!(text.ends_with("A board.\ncard: title"), "{text}");
        assert!(text.contains("bzz.app.board.*"));
    }
}
