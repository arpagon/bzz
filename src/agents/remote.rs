//! A moved agent's state and stop (plan v0.15.0, M5).
//!
//! Once an agent runs elsewhere, bzz has no channel to it but the relay, as
//! Buzz has none to a remote agent: its **presence** on the relay is its
//! status, and stopping it is a message, Buzz's own `!shutdown`, from its
//! owner.

use std::time::Duration;

use nostr::EventBuilder;
use serde_json::json;
use uuid::Uuid;

use crate::{
    agents::{
        manage::{OwnerRelay, Publisher as _},
        runtime::{admission::PlaceKind, relay::Published, service::relay_pubkey},
    },
    config::HostedAgentConfig,
    error::{Error, Result},
    protocol::presence::{self, Presence},
    store::writer::StoreHandle,
};

/// The body of Buzz's stop command, exactly.
pub const SHUTDOWN: &str = "!shutdown";

/// Whether `agent` is present on the relay, from the relay's own snapshot
/// of presence (kind 20001), which bzz reads for everyone. Absent from the
/// snapshot is offline. The relay forgets a presence after
/// [`presence::PRESENCE_TTL_SECONDS`], so a copy that crashed still shows as
/// present for up to that long.
pub async fn presence_of(relay: &OwnerRelay, store: &StoreHandle, agent: &str) -> Result<Presence> {
    let relay_key = relay_pubkey(store, relay.community, &relay.http).await?;
    let snapshot = relay
        .http
        .query_raw(&[json!({
            "kinds": [presence::KIND_PRESENCE], "authors": [agent], "limit": 1,
        })])
        .await?;
    Ok(snapshot
        .iter()
        .filter_map(|event| presence::observe(event, Some(&relay_key), |_| None))
        .find(|observed| observed.subjects.iter().any(|subject| subject == agent))
        .map_or(Presence::Offline, |observed| observed.presence))
}

/// What `bzz agent show` says of a moved agent's presence.
pub fn presence_line(presence: Presence) -> String {
    match presence {
        Presence::Online | Presence::Away => format!(
            "present ({}): a copy of it is running, or stopped without saying so less than {} s ago",
            presence.as_str(),
            presence::PRESENCE_TTL_SECONDS
        ),
        Presence::Offline => "not present: no copy of it is running".to_owned(),
    }
}

/// Buzz's stop command for `agent`, as `buzz-acp` at the pin accepts one
/// (`crates/buzz-acp/README.md`, "Inbound Author Gate"): a stream message
/// (kind 9) from the owner, whose body is exactly `!shutdown`, and which
/// mentions the agent with a `p` tag of its own. A name written into the
/// body would change the body, and the command would not be one.
pub fn shutdown_builder(channel: Uuid, agent: &str) -> Result<EventBuilder> {
    buzz_sdk::build_message(channel, SHUTDOWN, None, &[agent], false, &[], &[])
        .map_err(|error| Error::Protocol(error.to_string()))
}

/// What became of a stop command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Shutdown {
    pub channel: Uuid,
    pub channel_name: String,
    /// Its presence before the command.
    pub before: Presence,
    /// `Some(true)`: it was present and left. `Some(false)`: it is still
    /// present. `None`: it was not present before, so nothing can be told.
    pub left: Option<bool>,
}

/// Sends Buzz's stop command as the owner, in a channel the agent is a
/// member of, and watches its presence for up to `wait`.
pub async fn shutdown(
    relay: &OwnerRelay,
    store: &StoreHandle,
    agent: &HostedAgentConfig,
    channel: Option<Uuid>,
    wait: Duration,
) -> Result<Shutdown> {
    let is_member = |members: &[String]| {
        members
            .iter()
            .any(|member| member.eq_ignore_ascii_case(&agent.pubkey))
    };
    let (channel, channel_name) = match channel {
        Some(channel) => {
            let (facts, _) = relay.channel(store, channel).await?;
            if !is_member(&facts.members) {
                return Err(Error::Config(format!(
                    "{} is not a member of {}: a copy of it hears a command only where it is a member",
                    agent.label, facts.name
                )));
            }
            (channel, facts.name)
        }
        None => {
            // The most private place the two share: a direct message, a
            // Solo chat, then any channel.
            let mut places = Vec::new();
            for channel in relay.channels_of(&agent.pubkey).await? {
                if let Ok((facts, _)) = relay.channel(store, channel).await
                    && is_member(&facts.members)
                {
                    let rank = match facts.kind {
                        PlaceKind::Dm => 0,
                        PlaceKind::Solo => 1,
                        PlaceKind::Channel => 2,
                    };
                    places.push((rank, facts.name, channel));
                }
            }
            places.sort();
            let (_, name, channel) = places.into_iter().next().ok_or_else(|| {
                Error::Config(format!(
                    "{} is in no channel you can see, and a copy of it hears a command only where it is a member. Add it to one (`bzz agent add`), or stop it on its machine",
                    agent.label
                ))
            })?;
            (channel, name)
        }
    };
    let before = presence_of(relay, store, &agent.pubkey).await?;
    let command = relay
        .signer
        .sign(shutdown_builder(channel, &agent.pubkey)?)
        .await?;
    match relay.as_owner(command).await {
        Published::Accepted => {}
        Published::Refused(why) | Published::Later(why) => {
            return Err(Error::Access(format!(
                "the relay refused the command in {channel_name}: {why}"
            )));
        }
        Published::Unknown => {
            return Err(Error::Timeout(
                "the relay did not confirm the command".into(),
            ));
        }
    }
    let mut left = None;
    if before != Presence::Offline {
        let deadline = tokio::time::Instant::now() + wait;
        left = Some(false);
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if presence_of(relay, store, &agent.pubkey).await? == Presence::Offline {
                left = Some(true);
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                break;
            }
        }
    }
    Ok(Shutdown {
        channel,
        channel_name,
        before,
        left,
    })
}

#[cfg(test)]
mod tests {
    use nostr::Keys;

    use super::*;

    /// `buzz-acp`'s own test of an owner command at the pin
    /// (`crates/buzz-acp/src/lib.rs`, `is_owner_control_command` and
    /// `event_mentions_agent`); that the author is the owner is checked
    /// beside it.
    fn buzz_acp_takes_it_as(event: &nostr::Event, command: &str, agent: &str) -> bool {
        event.kind.as_u16() == 9
            && event.content.trim() == command
            && event.tags.iter().any(|tag| {
                let values = tag.as_slice();
                values.first().map(String::as_str) == Some("p")
                    && values.get(1).map(String::as_str) == Some(agent)
            })
    }

    #[test]
    fn the_stop_command_is_what_buzz_acp_takes_as_an_owner_command() {
        let (owner, agent) = (Keys::generate(), Keys::generate().public_key().to_hex());
        let channel = Uuid::new_v4();
        let command = shutdown_builder(channel, &agent)
            .unwrap()
            .sign_with_keys(&owner)
            .unwrap();
        assert_eq!(command.content, "!shutdown", "the body, exactly");
        assert!(buzz_acp_takes_it_as(&command, "!shutdown", &agent));
        assert_eq!(command.pubkey, owner.public_key(), "signed by the owner");
        assert_eq!(
            crate::protocol::events::channel_id(&command),
            Some(channel),
            "in the channel"
        );
        assert!(
            crate::protocol::events::event_references(&command).is_empty(),
            "not a reply: it stops the agent, not one conversation"
        );
        // What would not be a command: the agent named in the body, or no
        // mention at all.
        let named = buzz_sdk::build_message(
            channel,
            "@Scout !shutdown",
            None,
            &[&agent],
            false,
            &[],
            &[],
        )
        .unwrap()
        .sign_with_keys(&owner)
        .unwrap();
        assert!(!buzz_acp_takes_it_as(&named, "!shutdown", &agent));
        let unaddressed = buzz_sdk::build_message(channel, "!shutdown", None, &[], false, &[], &[])
            .unwrap()
            .sign_with_keys(&owner)
            .unwrap();
        assert!(!buzz_acp_takes_it_as(&unaddressed, "!shutdown", &agent));
        // Presence in words.
        assert!(presence_line(Presence::Online).starts_with("present (online)"));
        assert!(presence_line(Presence::Offline).starts_with("not present"));
    }
}
