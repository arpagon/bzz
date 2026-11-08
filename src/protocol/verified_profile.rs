//! NIP-FI verified profiles: relay-signed kind 30382 name claims that an
//! enterprise issuer authorized the relay to publish. Presentation only;
//! never authorization. Raw issuer claims never reach bzz, only these tags.

use nostr::Event;
use serde::{Deserialize, Serialize};

use crate::protocol::events::tag_values;

pub const KIND_VERIFIED_PROFILE: u16 = 30_382;
const MAX_NAME_CHARS: usize = 64;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct VerifiedName {
    pub display_name: String,
    pub username: String,
    pub full_name: Option<String>,
    /// Public issuer label chosen by the operator, never an issuer URI.
    pub issuer_label: String,
    pub verified_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedProfile {
    pub pubkey: String,
    /// `None` when the relay published `active=false` (verification ended).
    pub name: Option<VerifiedName>,
    pub event_id: String,
    pub created_at: u64,
}

fn one(event: &Event, name: &str) -> Option<String> {
    let values = tag_values(event, name);
    (values.len() == 1)
        .then(|| values.into_iter().next())
        .flatten()
}

fn bounded_text(value: String) -> Option<String> {
    let count = value.chars().count();
    (count > 0
        && count <= MAX_NAME_CHARS
        && value.trim() == value
        && !value.chars().any(char::is_control))
    .then_some(value)
}

/// Parse a 30382 event already verified and signed by `relay_pubkey`.
pub fn parse(event: &Event, relay_pubkey: &str) -> Option<VerifiedProfile> {
    if event.kind.as_u16() != KIND_VERIFIED_PROFILE || event.pubkey.to_hex() != relay_pubkey {
        return None;
    }
    let pubkey = one(event, "d")?;
    if one(event, "p")? != pubkey
        || pubkey.len() != 64
        || !pubkey
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    let name = match one(event, "active").as_deref() {
        Some("true") => {
            let username = one(event, "username")?;
            if username.is_empty()
                || username.len() > MAX_NAME_CHARS
                || !username.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
                })
            {
                return None;
            }
            Some(VerifiedName {
                display_name: bounded_text(one(event, "display_name")?)?,
                username,
                full_name: match tag_values(event, "full_name").as_slice() {
                    [] => None,
                    [value] => Some(bounded_text(value.clone())?),
                    _ => return None,
                },
                issuer_label: bounded_text(one(event, "issuer_label")?)?,
                verified_at: one(event, "verified_at")?.parse().ok()?,
            })
        }
        Some("false") => None,
        _ => return None,
    };
    Some(VerifiedProfile {
        pubkey,
        name,
        event_id: event.id.to_hex(),
        created_at: event.created_at.as_secs(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    fn event(relay: &Keys, tags: &[&[&str]]) -> Event {
        EventBuilder::new(Kind::Custom(KIND_VERIFIED_PROFILE), "")
            .tags(
                tags.iter()
                    .map(|tag| Tag::parse(tag.iter().copied()).unwrap()),
            )
            .sign_with_keys(relay)
            .unwrap()
    }

    #[test]
    fn accepts_only_well_formed_relay_signed_claims() {
        let relay = Keys::generate();
        let relay_hex = relay.public_key().to_hex();
        let person = "a".repeat(64);
        let good = event(
            &relay,
            &[
                &["d", &person],
                &["p", &person],
                &["active", "true"],
                &["display_name", "Wes Johnson"],
                &["username", "wes"],
                &["issuer_label", "Example IdP"],
                &["verified_at", "1790000000"],
            ],
        );
        let parsed = parse(&good, &relay_hex).unwrap();
        assert_eq!(parsed.name.unwrap().display_name, "Wes Johnson");
        assert!(
            parse(&good, &Keys::generate().public_key().to_hex()).is_none(),
            "other signer"
        );
        let ended = event(
            &relay,
            &[&["d", &person], &["p", &person], &["active", "false"]],
        );
        assert_eq!(parse(&ended, &relay_hex).unwrap().name, None);
        for bad in [
            vec![
                &["d", person.as_str()][..],
                &["p", &"b".repeat(64)],
                &["active", "true"],
            ],
            vec![&["d", "short"][..], &["p", "short"], &["active", "true"]],
        ] {
            assert!(parse(&event(&relay, &bad), &relay_hex).is_none());
        }
        let control = event(
            &relay,
            &[
                &["d", &person],
                &["p", &person],
                &["active", "true"],
                &["display_name", "bad\u{1b}[2J"],
                &["username", "wes"],
                &["issuer_label", "IdP"],
                &["verified_at", "1"],
            ],
        );
        assert!(parse(&control, &relay_hex).is_none(), "control characters");
    }
}
