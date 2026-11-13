//! Read-modify-write of the synced sidebar documents.

use nostr::Event;

use crate::{
    auth::signer::SignerHandle,
    error::{Error, Result},
    protocol::{
        http::HttpClient,
        sidebar::{self, Lane},
        sidebar_doc::{Edit, Tree},
    },
    service::messages::MessageService,
};

/// The relay's current readable head of `lane`, read from the writer.
pub async fn read_head(
    http: &HttpClient,
    signer: &SignerHandle,
    lane: Lane,
) -> Result<Option<(Event, Tree)>> {
    let me = signer.public_key();
    let events = http
        .query_raw(&[sidebar::head_filter(lane, &me.to_hex())])
        .await?;
    match sidebar::head(&events, lane, &me)? {
        Some(event) => {
            let (_, tree) = sidebar::decode_event(&event, signer).await?;
            Ok(Some((event, tree)))
        }
        None => Ok(None),
    }
}

/// Apply `apply` to the relay's head and publish the result once through the
/// outbox, then confirm it became the head. `Ok(None)`: nothing changed.
/// A head bzz cannot read is never overwritten.
pub async fn write(
    http: &HttpClient,
    signer: &SignerHandle,
    messages: &MessageService,
    lane: Lane,
    apply: impl FnOnce(&mut Edit<'_>, &Tree) -> Result<()>,
) -> Result<Option<(Event, Tree)>> {
    let head = read_head(http, signer, lane)
        .await?
        .map(|(event, tree)| (event.created_at.as_secs(), tree));
    let now_ms = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(i64::MAX);
    let Some(builder) = sidebar::edit(lane, head, signer, now_ms, apply).await? else {
        return Ok(None);
    };
    let event = messages.publish_builder(builder).await?;
    // The relay accepts a replaceable write that a newer head dominates.
    match read_head(http, signer, lane).await? {
        Some((head, tree)) if head.id == event.id => Ok(Some((event, tree))),
        _ => Err(Error::Protocol(
            "a newer sidebar change from another device won; try again".into(),
        )),
    }
}
