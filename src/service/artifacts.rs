//! NIP-AR reads and envelope edits. Every edit names the stored head in
//! `prev`; on `conflict` the head is reloaded and the change is rebuilt on it
//! (never resubmitted with only `prev` replaced) or reported to the user.

use uuid::Uuid;

use crate::{
    auth::signer::SignerHandle,
    error::{Error, Result},
    protocol::{
        artifact::{self, Artifact, Change, Reconcile},
        http::HttpClient,
    },
    realtime::supervisor::SupervisorHandle,
    store::writer::StoreHandle,
};

#[derive(Clone)]
pub struct ArtifactService {
    community_id: Uuid,
    http: HttpClient,
    store: StoreHandle,
    signer: SignerHandle,
    supervisor: SupervisorHandle,
}

/// The result of an edit the relay accepted, or found already in effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Applied {
    Published(String),
    AlreadyCurrent,
}

impl ArtifactService {
    pub const fn new(
        community_id: Uuid,
        http: HttpClient,
        store: StoreHandle,
        signer: SignerHandle,
        supervisor: SupervisorHandle,
    ) -> Self {
        Self {
            community_id,
            http,
            store,
            signer,
            supervisor,
        }
    }

    pub async fn capabilities(&self) -> Result<artifact::Capabilities> {
        Ok(artifact::capabilities(&self.http.nip11().await?))
    }

    /// Current head of `id`, including a deletion; `None` when unknown or
    /// unreadable (the relay does not say which).
    pub async fn head(&self, id: Uuid) -> Result<Option<Artifact>> {
        let events = self.http.query_raw(&[artifact::head_filter(id)]).await?;
        let head = events
            .iter()
            .filter_map(|event| artifact::parse(event).ok())
            .find(|head| head.id == id);
        if let Some(head) = head.clone() {
            let community = self.community_id;
            self.store
                .call(move |store| store.apply_artifact_head(community, &head))
                .await?;
        }
        Ok(head)
    }

    /// Current `buzz.task` artifacts assigned to `pubkey` in every readable
    /// channel, stored as current heads. Returns how many arrived.
    pub async fn refresh_assigned(&self, pubkey: &str) -> Result<usize> {
        let events = self
            .http
            .query_raw(&[artifact::assigned_filter(pubkey)])
            .await?;
        let heads = events
            .iter()
            .filter_map(|event| artifact::parse(event).ok())
            .collect::<Vec<_>>();
        let count = heads.len();
        let community = self.community_id;
        self.store
            .call(move |store| {
                for head in &heads {
                    store.apply_artifact_head(community, head)?;
                }
                Ok(())
            })
            .await?;
        Ok(count)
    }

    /// Applies `change` to artifact `id` against its stored head.
    pub async fn apply(&self, id: Uuid, change: Change) -> Result<Applied> {
        let community = self.community_id;
        for _ in 0..artifact::MAX_RECONCILE {
            let stored = self
                .store
                .call(move |store| store.artifact(community, id))
                .await?
                .ok_or_else(|| Error::Protocol("artifact is not available locally".into()))?;
            let builder = artifact::build(&change, &stored.head, stored.live.as_ref())?;
            let event = self.signer.sign(builder).await?;
            let ack = self.supervisor.publish(event.clone()).await?;
            if ack.accepted {
                let head = artifact::parse(&event)?;
                self.store
                    .call(move |store| store.apply_artifact_head(community, &head))
                    .await?;
                return Ok(Applied::Published(event.id.to_hex()));
            }
            if !artifact::is_conflict(&ack.message) {
                return Err(Error::Protocol(ack.message));
            }
            // Someone else advanced the artifact: reload, then rebuild.
            let Some(head) = self.head(id).await? else {
                return Err(Error::Protocol(
                    "the artifact is no longer available to you".into(),
                ));
            };
            match artifact::reconcile(&change, &head) {
                Reconcile::Done => return Ok(Applied::AlreadyCurrent),
                Reconcile::Impossible(reason) => return Err(Error::Protocol(reason.into())),
                Reconcile::Retry => {}
            }
        }
        Err(Error::Protocol(
            "the artifact keeps changing; try again in a moment".into(),
        ))
    }
}
