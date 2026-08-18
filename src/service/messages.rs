use std::collections::HashSet;

use nostr::{Event, EventId};
use uuid::Uuid;

use crate::{
    auth::signer::SignerHandle,
    error::{Error, Result},
    realtime::supervisor::SupervisorHandle,
    store::{models::OutboxState, writer::StoreHandle},
};

#[derive(Clone)]
pub struct MessageService {
    community_id: Uuid,
    signer: SignerHandle,
    store: StoreHandle,
    supervisor: SupervisorHandle,
}

impl MessageService {
    pub const fn new(
        community_id: Uuid,
        signer: SignerHandle,
        store: StoreHandle,
        supervisor: SupervisorHandle,
    ) -> Self {
        Self {
            community_id,
            signer,
            store,
            supervisor,
        }
    }
    pub async fn send(&self, channel: Uuid, content: &str) -> Result<Event> {
        self.send_with_media(channel, content, &[]).await
    }
    pub async fn send_with_media(
        &self,
        channel: Uuid,
        content: &str,
        attachments: &[crate::media::Attachment],
    ) -> Result<Event> {
        self.send_with_media_mentions(channel, content, attachments, &[])
            .await
    }

    pub async fn send_with_media_mentions(
        &self,
        channel: Uuid,
        content: &str,
        attachments: &[crate::media::Attachment],
        mentions: &[String],
    ) -> Result<Event> {
        let (content, tags) = message_media(content, attachments)?;
        let mentions = mention_references(mentions)?;
        self.send_builder(
            buzz_sdk::build_message(channel, &content, None, &mentions, false, &tags)
                .map_err(|error| Error::Protocol(error.to_string()))?,
        )
        .await
    }
    pub async fn reply(
        &self,
        channel: Uuid,
        root: &str,
        parent: &str,
        content: &str,
    ) -> Result<Event> {
        self.reply_with_media(channel, root, parent, content, &[])
            .await
    }
    pub async fn reply_with_media(
        &self,
        channel: Uuid,
        root: &str,
        parent: &str,
        content: &str,
        attachments: &[crate::media::Attachment],
    ) -> Result<Event> {
        self.reply_with_media_mentions(channel, root, parent, content, attachments, &[])
            .await
    }

    pub async fn reply_with_media_mentions(
        &self,
        channel: Uuid,
        root: &str,
        parent: &str,
        content: &str,
        attachments: &[crate::media::Attachment],
        mentions: &[String],
    ) -> Result<Event> {
        let thread = buzz_sdk::ThreadRef {
            root_event_id: EventId::from_hex(root)
                .map_err(|error| Error::Protocol(error.to_string()))?,
            parent_event_id: EventId::from_hex(parent)
                .map_err(|error| Error::Protocol(error.to_string()))?,
        };
        let (content, tags) = message_media(content, attachments)?;
        let mentions = mention_references(mentions)?;
        self.send_builder(
            buzz_sdk::build_message(channel, &content, Some(&thread), &mentions, false, &tags)
                .map_err(|error| Error::Protocol(error.to_string()))?,
        )
        .await
    }
    pub async fn react(&self, target: &str, emoji: &str) -> Result<Event> {
        let target =
            EventId::from_hex(target).map_err(|error| Error::Protocol(error.to_string()))?;
        self.send_builder(
            buzz_sdk::build_reaction(target, emoji)
                .map_err(|error| Error::Protocol(error.to_string()))?,
        )
        .await
    }
    pub async fn remove_reaction(&self, reaction: &str) -> Result<Event> {
        let reaction =
            EventId::from_hex(reaction).map_err(|error| Error::Protocol(error.to_string()))?;
        self.send_builder(
            buzz_sdk::build_remove_reaction(reaction)
                .map_err(|error| Error::Protocol(error.to_string()))?,
        )
        .await
    }
    pub async fn delete(&self, channel: Uuid, target: &str, author: &str) -> Result<Event> {
        if author != self.signer.public_key().to_hex() {
            return Err(Error::Access("only your own message can be deleted".into()));
        }
        let target =
            EventId::from_hex(target).map_err(|error| Error::Protocol(error.to_string()))?;
        self.send_builder(
            buzz_sdk::build_delete_compat(channel, target)
                .map_err(|error| Error::Protocol(error.to_string()))?,
        )
        .await
    }
    async fn send_builder(&self, builder: nostr::EventBuilder) -> Result<Event> {
        let event = self.signer.sign(builder).await?;
        let stored = event.clone();
        let community = self.community_id;
        self.store
            .call(move |store| store.insert_outbox(community, &stored))
            .await?;
        let id = event.id.to_hex();
        match self.supervisor.publish(event.clone()).await {
            Ok(ack) if ack.accepted => {
                let event_id = id;
                let accepted = event.clone();
                self.store
                    .call(move |store| {
                        store.apply_event(community, &accepted)?;
                        store.set_outbox_state(community, &event_id, OutboxState::Delivered, None)
                    })
                    .await?;
            }
            Ok(ack) => {
                let message = ack.message;
                let stored_message = message.clone();
                let event_id = id;
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
                return Err(Error::Access(message));
            }
            Err(error) => {
                let message = error.to_string();
                let event_id = id;
                self.store
                    .call(move |store| {
                        store.set_outbox_state(
                            community,
                            &event_id,
                            OutboxState::Unknown,
                            Some(&message),
                        )
                    })
                    .await?;
                return Err(error);
            }
        }
        Ok(event)
    }
}

fn mention_references(mentions: &[String]) -> Result<Vec<&str>> {
    if mentions.len() > crate::ui::composer::MENTION_CAP
        || mentions
            .iter()
            .any(|pubkey| !crate::ui::composer::valid_pubkey(pubkey))
    {
        return Err(Error::Protocol("invalid message mentions".into()));
    }
    let mut seen = HashSet::new();
    Ok(mentions
        .iter()
        .filter(|pubkey| seen.insert(pubkey.as_str()))
        .map(String::as_str)
        .collect())
}

fn message_media(
    content: &str,
    attachments: &[crate::media::Attachment],
) -> Result<(String, Vec<Vec<String>>)> {
    if attachments.len() > 8 {
        return Err(Error::Config(
            "a message can contain at most 8 attachments".into(),
        ));
    }
    let mut body = content.trim_end().to_owned();
    let mut tags = Vec::with_capacity(attachments.len());
    for (index, attachment) in attachments.iter().enumerate() {
        if !attachment.valid() {
            return Err(Error::Protocol(format!(
                "attachment {} has an invalid descriptor",
                index + 1
            )));
        }
        if !body.is_empty() {
            body.push('\n');
        }
        body.push_str(&attachment.markdown_line());
        tags.push(attachment.imeta_tag());
    }
    Ok((body, tags))
}

#[cfg(test)]
mod media_tests {
    use super::{mention_references, message_media};
    use crate::media::{Attachment, MediaKind};

    #[test]
    fn mention_references_are_deduplicated_and_canonical() {
        let key = "a".repeat(64);
        assert_eq!(
            mention_references(&[key.clone(), key.clone()]).unwrap(),
            vec![key.as_str()]
        );
        assert!(mention_references(&["A".repeat(64)]).is_err());
    }

    #[test]
    fn media_lines_and_tags_are_ordered() {
        let attachment = Attachment {
            index: 0,
            url: "https://buzz.example/media/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png".into(),
            mime: "image/png".into(),
            sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            size: 4,
            width: Some(1),
            height: Some(1),
            alt: None,
            blurhash: None,
            thumb: None,
            poster: None,
            filename: Some("safe.png".into()),
            duration_millis: None,
            kind: MediaKind::Image,
            spoiler: false,
            error: None,
        };
        let (content, tags) = message_media("hello", &[attachment]).unwrap();
        assert!(content.contains("![image](https://buzz.example/media/"));
        assert_eq!(tags[0][0], "imeta");
    }
}
