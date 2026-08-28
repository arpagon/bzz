use std::collections::BTreeSet;

use nostr::{Event, EventBuilder, Kind, Tag};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    auth::signer::SignerHandle,
    error::{Error, Result},
    protocol::http::HttpClient,
    realtime::supervisor::SupervisorHandle,
    store::{models::OutboxState, writer::StoreHandle},
    sync::directory,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DmOpenResult {
    pub channel_id: Uuid,
    pub created: Option<bool>,
    pub visibility_confirmed: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DmAck {
    channel_id: String,
    #[serde(default)]
    created: Option<bool>,
}

#[derive(Clone)]
pub struct DmService {
    community_id: Uuid,
    signer: SignerHandle,
    http: HttpClient,
    store: StoreHandle,
    supervisor: SupervisorHandle,
}

impl DmService {
    pub const fn new(
        community_id: Uuid,
        signer: SignerHandle,
        http: HttpClient,
        store: StoreHandle,
        supervisor: SupervisorHandle,
    ) -> Self {
        Self {
            community_id,
            signer,
            http,
            store,
            supervisor,
        }
    }

    pub async fn open(&self, recipients: Vec<String>) -> Result<DmOpenResult> {
        let self_pubkey = self.signer.public_key().to_hex();
        let participants = normalize_participants(&self_pubkey, recipients)?;
        let others = participants
            .iter()
            .filter(|pubkey| pubkey.as_str() != self_pubkey)
            .map(String::as_str)
            .collect::<Vec<_>>();
        let builder =
            buzz_sdk::build_dm_open(&others).map_err(|error| Error::Config(error.to_string()))?;
        let event = self.signer.sign(builder).await?;
        match self.publish_command(event).await {
            Ok(message) => {
                if let Ok(ack) = parse_dm_ack(&message) {
                    let channel_id = Uuid::parse_str(&ack.channel_id).map_err(|_| {
                        Error::Protocol("DM response has an invalid channel ID".into())
                    })?;
                    self.confirm_participants(channel_id, &participants).await?;
                    return Ok(DmOpenResult {
                        channel_id,
                        created: ack.created,
                        visibility_confirmed: self.confirm_visible(channel_id).await?,
                    });
                }
                self.recover(&participants, None).await
            }
            Err(error) => self.recover(&participants, Some(error)).await,
        }
    }

    pub async fn add_member(&self, channel_id: Uuid, pubkey: String) -> Result<DmOpenResult> {
        let self_pubkey = self.signer.public_key().to_hex();
        let community_id = self.community_id;
        let current = self
            .store
            .call(move |store| store.dm_participants(community_id, channel_id))
            .await?;
        if current.is_empty() || !current.iter().any(|value| value == &self_pubkey) {
            return Err(Error::Access(
                "the active identity is not a member of this DM".into(),
            ));
        }
        let mut recipients = current
            .into_iter()
            .filter(|value| value != &self_pubkey)
            .collect::<Vec<_>>();
        recipients.push(pubkey.clone());
        let participants = normalize_participants(&self_pubkey, recipients)?;
        let builder = buzz_sdk::build_dm_add_member(channel_id, &pubkey)
            .map_err(|error| Error::Config(error.to_string()))?;
        let event = self.signer.sign(builder).await?;
        match self.publish_command(event).await {
            Ok(message) => {
                if let Ok(ack) = parse_dm_ack(&message) {
                    let new_channel = Uuid::parse_str(&ack.channel_id).map_err(|_| {
                        Error::Protocol("DM response has an invalid channel ID".into())
                    })?;
                    self.confirm_participants(new_channel, &participants)
                        .await?;
                    return Ok(DmOpenResult {
                        channel_id: new_channel,
                        created: ack.created,
                        visibility_confirmed: self.confirm_visible(new_channel).await?,
                    });
                }
                self.recover(&participants, None).await
            }
            Err(error) => self.recover(&participants, Some(error)).await,
        }
    }

    pub async fn hide(&self, channel_id: Uuid) -> Result<bool> {
        let builder = EventBuilder::new(Kind::Custom(41_012), "").tags([Tag::parse([
            "h",
            channel_id.to_string().as_str(),
        ])
        .map_err(|error| Error::Protocol(error.to_string()))?]);
        let event = self.signer.sign(builder).await?;
        let event_id = event.id.to_hex();
        let outcome = self.publish_command(event).await;
        let confirmed = self.confirm_hidden(channel_id).await?;
        if confirmed && outcome.is_err() {
            let community = self.community_id;
            self.store
                .call(move |store| {
                    store.set_outbox_state(community, &event_id, OutboxState::Delivered, None)
                })
                .await?;
            return Ok(true);
        }
        outcome?;
        Ok(confirmed)
    }

    async fn publish_command(&self, event: Event) -> Result<String> {
        let community = self.community_id;
        let stored = event.clone();
        self.store
            .call(move |store| store.insert_outbox(community, &stored))
            .await?;
        let event_id = event.id.to_hex();
        match self.supervisor.publish(event.clone()).await {
            Ok(ack) if ack.accepted => {
                let message = ack.message;
                let accepted = event;
                self.store
                    .call(move |store| {
                        store.apply_event(community, &accepted)?;
                        store.set_outbox_state(community, &event_id, OutboxState::Delivered, None)
                    })
                    .await?;
                Ok(message)
            }
            Ok(ack) => {
                let message = ack.message;
                let stored_message = message.clone();
                self.store
                    .call(move |store| {
                        store.set_outbox_state(
                            community,
                            &event_id,
                            OutboxState::Rejected,
                            Some(&stored_message),
                        )
                    })
                    .await?;
                Err(
                    if crate::realtime::admission::rate_limit_retry_after(&message).is_some() {
                        Error::Network(message)
                    } else {
                        Error::Access(message)
                    },
                )
            }
            Err(error) => {
                let state = if crate::realtime::admission::is_local_admission_error(&error) {
                    OutboxState::Rejected
                } else {
                    OutboxState::Unknown
                };
                let public = error.to_string();
                self.store
                    .call(move |store| {
                        store.set_outbox_state(community, &event_id, state, Some(&public))
                    })
                    .await?;
                Err(error)
            }
        }
    }

    async fn recover(
        &self,
        participants: &BTreeSet<String>,
        original: Option<Error>,
    ) -> Result<DmOpenResult> {
        let self_pubkey = self.signer.public_key().to_hex();
        for _ in 0..3 {
            let _ =
                directory::refresh(self.community_id, &self_pubkey, &self.http, &self.store).await;
            let expected = participants.clone();
            let community = self.community_id;
            if let Some(channel_id) = self
                .store
                .call(move |store| store.find_dm_by_participants(community, &expected))
                .await?
            {
                return Ok(DmOpenResult {
                    channel_id,
                    created: None,
                    visibility_confirmed: self.confirm_visible(channel_id).await?,
                });
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        Err(original.unwrap_or_else(|| {
            Error::Protocol("DM command was accepted but discovery state is unavailable".into())
        }))
    }

    async fn confirm_participants(
        &self,
        channel_id: Uuid,
        expected: &BTreeSet<String>,
    ) -> Result<()> {
        let self_pubkey = self.signer.public_key().to_hex();
        for _ in 0..3 {
            let _ =
                directory::refresh(self.community_id, &self_pubkey, &self.http, &self.store).await;
            let community = self.community_id;
            let members = self
                .store
                .call(move |store| store.dm_participants(community, channel_id))
                .await?
                .into_iter()
                .collect::<BTreeSet<_>>();
            if &members == expected {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        Err(Error::Protocol(
            "DM discovery participants do not match the command response".into(),
        ))
    }

    async fn confirm_hidden(&self, channel_id: Uuid) -> Result<bool> {
        let self_pubkey = self.signer.public_key().to_hex();
        for _ in 0..3 {
            let _ =
                directory::refresh(self.community_id, &self_pubkey, &self.http, &self.store).await;
            let identity = self_pubkey.clone();
            let community = self.community_id;
            if self
                .store
                .call(move |store| store.hidden_dms(community, &identity))
                .await?
                .contains(&channel_id)
            {
                return Ok(true);
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        Ok(false)
    }

    async fn confirm_visible(&self, channel_id: Uuid) -> Result<bool> {
        let self_pubkey = self.signer.public_key().to_hex();
        let identity = self_pubkey.clone();
        let community = self.community_id;
        if !self
            .store
            .call(move |store| store.hidden_dms(community, &identity))
            .await?
            .contains(&channel_id)
        {
            return Ok(true);
        }
        for _ in 0..3 {
            let _ =
                directory::refresh(self.community_id, &self_pubkey, &self.http, &self.store).await;
            let identity = self_pubkey.clone();
            if !self
                .store
                .call(move |store| store.hidden_dms(community, &identity))
                .await?
                .contains(&channel_id)
            {
                return Ok(true);
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        Ok(false)
    }
}

fn normalize_participants(self_pubkey: &str, recipients: Vec<String>) -> Result<BTreeSet<String>> {
    if recipients.is_empty() || recipients.len() > 8 {
        return Err(Error::Config("select 1-8 DM recipients".into()));
    }
    let self_pubkey = normalize_pubkey(self_pubkey)?;
    let mut participants = BTreeSet::from([self_pubkey.clone()]);
    for recipient in recipients {
        let recipient = normalize_pubkey(&recipient)?;
        if recipient == self_pubkey {
            return Err(Error::Config(
                "the active identity cannot be a DM recipient".into(),
            ));
        }
        if !participants.insert(recipient) {
            return Err(Error::Config("DM recipients must be unique".into()));
        }
    }
    if !(2..=9).contains(&participants.len()) {
        return Err(Error::Config(
            "a workspace DM requires 2-9 participants".into(),
        ));
    }
    Ok(participants)
}

fn normalize_pubkey(value: &str) -> Result<String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::Config(
            "a DM recipient pubkey must be 64 hexadecimal characters".into(),
        ));
    }
    Ok(value.to_ascii_lowercase())
}

fn parse_dm_ack(message: &str) -> Result<DmAck> {
    if message.len() > 8_192 {
        return Err(Error::Protocol("DM response exceeds the size limit".into()));
    }
    let json = message
        .strip_prefix("response:")
        .ok_or_else(|| Error::Protocol("relay returned a malformed DM response".into()))?;
    serde_json::from_str(json)
        .map_err(|_| Error::Protocol("relay returned a malformed DM response".into()))
}

#[cfg(test)]
mod tests {
    use super::{normalize_participants, parse_dm_ack};

    #[test]
    fn participant_sets_are_canonical_bounded_and_strict() {
        let own = "a".repeat(64);
        let other = "B".repeat(64);
        let values = normalize_participants(&own, vec![other]).unwrap();
        assert_eq!(
            values.into_iter().collect::<Vec<_>>(),
            vec![own, "b".repeat(64)]
        );
        assert!(normalize_participants(&"a".repeat(64), vec!["a".repeat(64)]).is_err());
    }

    #[test]
    fn command_responses_reject_unknown_fields() {
        let parsed = parse_dm_ack(
            r#"response:{"channel_id":"00000000-0000-0000-0000-000000000001","created":true}"#,
        )
        .unwrap();
        assert_eq!(parsed.created, Some(true));
        assert!(parse_dm_ack(r#"{"channel_id":"00000000-0000-0000-0000-000000000001"}"#).is_err());
        assert!(
            parse_dm_ack(
                r#"response:{"channel_id":"00000000-0000-0000-0000-000000000001","extra":1}"#
            )
            .is_err()
        );
    }
}
