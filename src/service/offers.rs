//! Apps offered in a channel (v0.16 M6, feature `apps`): sharing an app,
//! withdrawing it, and the sync that takes others' offers in.
//!
//! `apps::share` is the artifact on the wire; `apps::store::offers` takes a
//! revision in as a waiting version. This is the part that talks to the
//! relay.
//!
//! - **[`OffersService::share`]** publishes the version of an app that runs
//!   here (so one the owner approved, or wrote himself) as one artifact of
//!   type `bzz.app`, signed by the owner: a new artifact, or a new revision
//!   of the one shared before. Every size is checked before a frame goes.
//!   It then posts the brief to the channel as an ordinary message, once
//!   for each version shared: people on Buzz desktop see nothing else.
//! - **[`OffersService::unshare`]** publishes a delete revision.
//! - **[`OffersService::watch`]** keeps one sync of `bzz.app` for the
//!   channel the owner looks at, apart from the records of any app. Every
//!   head that arrives is taken in; nothing is fetched from anywhere else,
//!   and nothing runs. What came of it is told through the callback.

use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use nostr::Event;
use tokio::{
    sync::{broadcast, mpsc},
    time::Instant,
};
use uuid::Uuid;

use crate::{
    apps::{
        share::{self, Head},
        store::{
            Apps, Bundle,
            offers::{Arrival, Step, Told},
        },
    },
    auth::signer::SignerHandle,
    error::Error,
    protocol::{
        artifact::{self, Artifact, KIND_ARTIFACT},
        http::HttpClient,
        record::{Refusal, Verdict, verdict},
    },
    realtime::{
        admission::{self, ClosedReason, SubscriptionPriority},
        session::SessionEvent,
        supervisor::{SupervisorEvent, SupervisorHandle},
    },
    service::messages::MessageService,
    store::{app_offers::ShareRow, writer::StoreHandle},
};

/// The wait before a sync of offers the relay ended with `resync:` is asked
/// again, doubled each time it ends before it completes.
pub const RESYNC_DELAY: Duration = Duration::from_millis(250);
pub const MAX_RESYNC_DELAY: Duration = Duration::from_secs(30);
/// The most heads of offers one channel's sync is read for. Past it bzz
/// stops reading the channel's offers and says so: no partial set is taken
/// for the whole.
pub const MAX_OFFER_HEADS: usize = 256;

/// What the owner is told while the offers of a channel are watched.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OfferNews {
    /// What became of offers that arrived or went.
    Told { channel: Uuid, told: Vec<Told> },
    /// The whole set of the channel's offers was read: it is current.
    Synced { channel: Uuid },
    /// The offers of the channel cannot be read, and are not asked again.
    Stopped { channel: Uuid, why: String },
}

/// Where the owner is told.
pub type Tell = Arc<dyn Fn(OfferNews) + Send + Sync>;

/// What sharing did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Shared {
    pub name: String,
    pub version: u32,
    pub artifact: Uuid,
    /// The event id of the revision published.
    pub revision: String,
    /// A new revision of an artifact shared before, rather than a new one.
    pub revised: bool,
    /// The brief of this version was posted to the channel now.
    pub brief_posted: bool,
    /// This version, by these bytes, was the head already: nothing was
    /// published.
    pub unchanged: bool,
}

/// What withdrawing did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Unshared {
    pub name: String,
    /// It had been withdrawn already, by the signer of the deletion.
    pub already_by: Option<String>,
}

/// The channel watched, and the sender whose drop ends its sync.
type Watched = (Uuid, mpsc::UnboundedSender<()>);

/// The offers of one community's channels, for one identity.
#[derive(Clone)]
pub struct OffersService {
    community: Uuid,
    http: HttpClient,
    store: StoreHandle,
    signer: SignerHandle,
    supervisor: SupervisorHandle,
    messages: MessageService,
    apps_root: PathBuf,
    tell: Tell,
    /// The channel watched now, and the handle that ends its sync.
    watched: Arc<Mutex<Option<Watched>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// A relay's refusal of a write, in words for the owner.
fn refused(message: &str) -> String {
    match verdict(message) {
        Verdict::RateLimited { seconds } => {
            format!("too many writes; the relay asks to wait {seconds} s")
        }
        Verdict::Refused { refusal, reason } => match refusal {
            Refusal::Guest => {
                "you are a guest of this channel, and a guest cannot offer an app there".into()
            }
            Refusal::NotMember => "you are not a member of this channel".into(),
            Refusal::Archived => "this channel is archived; nothing can be offered in it".into(),
            _ => reason,
        },
        Verdict::TooLarge => "the relay refused it as too large".into(),
        Verdict::HeadChanged | Verdict::Gone => {
            "the offer changed in the channel meanwhile; try again".into()
        }
    }
}

impl OffersService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        community: Uuid,
        http: HttpClient,
        store: StoreHandle,
        signer: SignerHandle,
        supervisor: SupervisorHandle,
        messages: MessageService,
        apps_root: PathBuf,
        tell: Tell,
    ) -> Self {
        Self {
            community,
            http,
            store,
            signer,
            supervisor,
            messages,
            apps_root,
            tell,
            watched: Arc::default(),
        }
    }

    /// The identity offers are signed with.
    pub fn pubkey(&self) -> String {
        self.signer.public_key().to_hex()
    }

    /// The channel whose offers are watched now.
    pub fn watching(&self) -> Option<Uuid> {
        lock(&self.watched).as_ref().map(|(channel, _)| *channel)
    }

    /// Watches the offers of `channel` (`dm`: it is a direct message, where
    /// every offer is refused), or of none. The sync of the channel watched
    /// before ends.
    pub fn watch(&self, channel: Option<(Uuid, bool)>) {
        let mut watched = lock(&self.watched);
        if watched.as_ref().map(|(id, _)| *id) == channel.map(|(id, _)| id) {
            return;
        }
        // Dropping the sender ends the task of the channel before.
        *watched = None;
        let Some((channel, dm)) = channel else {
            return;
        };
        let (stop, stopped) = mpsc::unbounded_channel();
        *watched = Some((channel, stop));
        let relay = self.supervisor.subscribe_events();
        tokio::spawn(
            Watch {
                service: self.clone(),
                channel,
                dm,
                subscription: share::subscription_id(channel),
                collecting: Some(Vec::new()),
                attempts: 0,
                retry_at: None,
            }
            .run(relay, stopped),
        );
    }

    async fn head_of(&self, artifact: Uuid) -> Result<Option<Artifact>, String> {
        let events = self
            .http
            .query_raw(&[artifact::head_filter(artifact)])
            .await
            .map_err(|_| "the offer's current revision could not be read from the relay")?;
        Ok(events
            .iter()
            .filter_map(|event| artifact::parse(event).ok())
            .find(|head| head.id == artifact))
    }

    async fn send(&self, builder: nostr::EventBuilder) -> Result<Event, String> {
        let event = self
            .signer
            .sign(builder)
            .await
            .map_err(|error| match error {
                Error::Locked(_) => "the identity is locked".to_owned(),
                Error::SignerRefused(reason) => reason,
                _ => "the offer could not be signed".to_owned(),
            })?;
        share::check(&event).map_err(|refusal| format!("not sent: {refusal}"))?;
        let ack = self
            .supervisor
            .publish(event.clone())
            .await
            .map_err(|error| format!("not sent: {error}"))?;
        if !ack.accepted {
            return Err(if artifact::is_conflict(&ack.message) {
                "conflict".into()
            } else {
                refused(&ack.message)
            });
        }
        Ok(event)
    }

    /// Shares the version of `name` that runs here in `channel`.
    pub async fn share(&self, channel: Uuid, name: &str) -> Result<Shared, String> {
        let (community, root, wanted) = (self.community, self.apps_root.clone(), name.to_owned());
        let found = self
            .store
            .call(move |store| {
                let shared = store.app_share(community, channel, &wanted)?;
                let apps = Apps::at(store, root);
                // Only what runs here: approved by the owner, or his own.
                let bundle = apps.runnable(community, &wanted).and_then(|runnable| {
                    apps.bundle(community, &wanted, runnable.version())
                        .map(|bundle| (runnable.version(), bundle))
                });
                Ok((shared, bundle.map_err(|error| error.to_string())))
            })
            .await
            .map_err(|error| error.to_string())?;
        let (before, (version, bundle)) = (found.0, found.1?);
        let hash = crate::apps::store::manifest::sha256(&bundle.script);
        // Checked here first, so that nothing is signed that cannot go.
        share::content(&bundle).map_err(|refusal| format!("not shared: {refusal}"))?;
        let mut attempt = 0;
        let (event, revised, unchanged) = loop {
            attempt += 1;
            let head = match before.as_ref().filter(|row| row.is_shared()) {
                Some(row) => self.head_of(row.artifact).await?,
                None => None,
            };
            let live = head.filter(|head| !head.is_deleted() && head.home == channel);
            if let Some(head) = &live
                && head.author == self.pubkey()
                && matches!(share::read(head, channel), Ok(Head::Offer(offer)) if offer.script_sha256 == hash && offer.brief == bundle.brief)
            {
                break (head.event.clone(), false, true);
            }
            let builder = match &live {
                Some(head) => share::revise(head, &bundle),
                None => share::create(channel, Uuid::new_v4(), &bundle),
            }
            .map_err(|refusal| format!("not shared: {refusal}"))?;
            match self.send(builder).await {
                Ok(event) => break (event, live.is_some(), false),
                Err(why) if why == "conflict" && attempt < artifact::MAX_RECONCILE => {}
                Err(why) if why == "conflict" => {
                    return Err(
                        "the offer kept changing in the channel while it was shared; try again"
                            .into(),
                    );
                }
                Err(why) => return Err(why),
            }
        };
        let head = artifact::parse(&event).map_err(|error| error.to_string())?;
        let posted = before.as_ref().and_then(|row| row.brief_posted_version);
        let row = ShareRow {
            community: self.community,
            channel,
            name: name.to_owned(),
            artifact: head.id,
            version,
            script_sha256: hash,
            revision: head.head.clone(),
            brief_posted_version: posted,
            shared_at: now(),
            withdrawn_at: None,
        };
        let stored = row.clone();
        self.store
            .call(move |store| store.app_share_published(&stored))
            .await
            .map_err(|error| error.to_string())?;
        let brief_posted = if posted == Some(version) {
            false
        } else {
            self.post_brief(channel, name, &bundle, version).await?;
            true
        };
        Ok(Shared {
            name: name.to_owned(),
            version,
            artifact: head.id,
            revision: head.head,
            revised,
            brief_posted,
            unchanged,
        })
    }

    async fn post_brief(
        &self,
        channel: Uuid,
        name: &str,
        bundle: &Bundle,
        version: u32,
    ) -> Result<(), String> {
        self.messages
            .send(channel, &share::brief_message(bundle, version))
            .await
            .map_err(|error| {
                format!("the app is shared, and its brief could not be posted: {error}")
            })?;
        let (community, name) = (self.community, name.to_owned());
        self.store
            .call(move |store| store.app_share_brief_posted(community, channel, &name, version))
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    /// Withdraws the offer of `name` in `channel`: a delete revision.
    /// Copies already installed elsewhere stay.
    pub async fn unshare(&self, channel: Uuid, name: &str) -> Result<Unshared, String> {
        let (community, wanted) = (self.community, name.to_owned());
        let row = self
            .store
            .call(move |store| store.app_share(community, channel, &wanted))
            .await
            .map_err(|error| error.to_string())?
            .filter(ShareRow::is_shared)
            .ok_or_else(|| format!("{name} is not shared in this channel"))?;
        let mut attempt = 0;
        let (revision, already_by) = loop {
            attempt += 1;
            let Some(head) = self.head_of(row.artifact).await? else {
                break (row.revision.clone(), Some(String::new()));
            };
            if head.is_deleted() || head.home != channel {
                break (head.head.clone(), Some(head.author.clone()));
            }
            let builder = share::withdraw(&head).map_err(|refusal| refusal.to_string())?;
            match self.send(builder).await {
                Ok(event) => break (event.id.to_hex(), None),
                Err(why) if why == "conflict" && attempt < artifact::MAX_RECONCILE => {}
                Err(why) => return Err(why),
            }
        };
        let (community, wanted) = (self.community, name.to_owned());
        self.store
            .call(move |store| {
                store.app_share_withdrawn(community, channel, &wanted, &revision, now())
            })
            .await
            .map_err(|error| error.to_string())?;
        Ok(Unshared {
            name: name.to_owned(),
            already_by: already_by.filter(|by| !by.is_empty()),
        })
    }

    /// Takes one head in: the store's step, the script loaded apart off
    /// every thread that matters, the store's second step.
    async fn take_in(&self, head: Artifact, channel: Uuid, dm: bool) -> Vec<Told> {
        let arrival = Arrival::of(&head, channel, dm);
        let (community, root, me) = (self.community, self.apps_root.clone(), self.pubkey());
        let step = self
            .store
            .call(move |store| {
                Ok(Apps::at(store, root)
                    .offer_arrived(community, &me, arrival)
                    .map_err(|error| error.to_string()))
            })
            .await;
        let examine = match step {
            Ok(Ok(Step::Done(told))) => return told,
            Ok(Ok(Step::Examine(examine))) => examine,
            Ok(Err(error)) | Err(Error::Config(error)) => {
                tracing::warn!(%error, "an offered app could not be taken in");
                return Vec::new();
            }
            Err(error) => {
                tracing::warn!(%error, "an offered app could not be taken in");
                return Vec::new();
            }
        };
        let submission = examine.submission.clone();
        let Ok(Ok(examined)) =
            tokio::task::spawn_blocking(move || crate::apps::store::examine(submission)).await
        else {
            return Vec::new();
        };
        let root = self.apps_root.clone();
        match self
            .store
            .call(move |store| {
                Ok(Apps::at(store, root)
                    .offer_examined(*examine, examined)
                    .map_err(|error| error.to_string()))
            })
            .await
        {
            Ok(Ok(told)) => told,
            Ok(Err(error)) => {
                tracing::warn!(%error, "an offered app could not be recorded");
                Vec::new()
            }
            Err(error) => {
                tracing::warn!(%error, "an offered app could not be recorded");
                Vec::new()
            }
        }
    }
}

/// The sync of one channel's offers.
struct Watch {
    service: OffersService,
    channel: Uuid,
    dm: bool,
    subscription: String,
    /// Heads before `EOSE`; None once the set is live.
    collecting: Option<Vec<Artifact>>,
    attempts: u32,
    retry_at: Option<Instant>,
}

impl Watch {
    async fn run(
        mut self,
        mut relay: broadcast::Receiver<SupervisorEvent>,
        mut stopped: mpsc::UnboundedReceiver<()>,
    ) {
        self.request().await;
        loop {
            let retry = self.retry_at;
            tokio::select! {
                // The sender is dropped when another channel is watched.
                _ = stopped.recv() => break,
                () = async {
                    match retry {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                } => {
                    self.retry_at = None;
                    self.request().await;
                }
                event = relay.recv() => match event {
                    Ok(SupervisorEvent::Session(event)) => {
                        if !self.session(event).await {
                            break;
                        }
                    }
                    Ok(SupervisorEvent::Connecting) => self.collecting = Some(Vec::new()),
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => self.request().await,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
            }
        }
        let _ = self
            .service
            .supervisor
            .close(self.subscription.clone())
            .await;
    }

    async fn request(&mut self) {
        self.collecting = Some(Vec::new());
        let sent = self
            .service
            .supervisor
            .subscribe_with_priority(
                self.subscription.clone(),
                vec![share::sync_filter(self.channel)],
                SubscriptionPriority::Background,
            )
            .await;
        if sent.is_err() {
            self.collecting = None;
        }
    }

    fn tell(&self, news: OfferNews) {
        (self.service.tell)(news);
    }

    /// One frame of the relay. False: the sync is over for good.
    async fn session(&mut self, event: SessionEvent) -> bool {
        match event {
            SessionEvent::Event {
                subscription,
                event,
            } if subscription == self.subscription => {
                if event.kind.as_u16() != KIND_ARTIFACT {
                    return true;
                }
                let Ok(head) = artifact::parse(&event) else {
                    return true;
                };
                // The relay sends what the filter matches; bzz does not take
                // its word for it.
                if head.artifact_type != share::OFFER_TYPE {
                    return true;
                }
                match &mut self.collecting {
                    Some(buffer) if buffer.len() >= MAX_OFFER_HEADS => {
                        self.collecting = None;
                        self.tell(OfferNews::Stopped {
                            channel: self.channel,
                            why: format!(
                                "this channel holds more than {MAX_OFFER_HEADS} offers of apps; bzz does not read them"
                            ),
                        });
                        return false;
                    }
                    Some(buffer) => buffer.push(head),
                    None => {
                        let told = self.service.take_in(head, self.channel, self.dm).await;
                        if !told.is_empty() {
                            self.tell(OfferNews::Told {
                                channel: self.channel,
                                told,
                            });
                        }
                    }
                }
            }
            SessionEvent::Eose(subscription) if subscription == self.subscription => {
                if let Some(buffer) = self.collecting.take() {
                    self.commit(buffer).await;
                }
            }
            SessionEvent::Closed {
                subscription,
                message,
            } if subscription == self.subscription => {
                self.collecting = None;
                match admission::closed_reason(&message) {
                    ClosedReason::Resync => {
                        let delay = RESYNC_DELAY
                            .saturating_mul(1_u32 << self.attempts.min(8))
                            .min(MAX_RESYNC_DELAY);
                        self.attempts = self.attempts.saturating_add(1);
                        self.retry_at = Some(Instant::now() + delay);
                    }
                    reason => {
                        self.tell(OfferNews::Stopped {
                            channel: self.channel,
                            why: match reason {
                                ClosedReason::Restricted => {
                                    "you cannot read this channel's offers of apps".into()
                                }
                                ClosedReason::SyncOverflow => {
                                    "this channel holds too many artifacts to read its offers of apps"
                                        .into()
                                }
                                _ => "the relay refused to list this channel's offers of apps"
                                    .into(),
                            },
                        });
                        return false;
                    }
                }
            }
            // The supervisor asks again when it is connected; what follows
            // is a whole set.
            SessionEvent::Disconnected(_) => self.collecting = Some(Vec::new()),
            _ => {}
        }
        true
    }

    /// `EOSE`: the heads are the channel's whole set of offers.
    async fn commit(&mut self, buffer: Vec<Artifact>) {
        self.attempts = 0;
        let live: HashSet<String> = buffer
            .iter()
            .filter(|head| !head.is_deleted() && head.home == self.channel)
            .map(|head| head.head.clone())
            .collect();
        let mut told = Vec::new();
        for head in buffer {
            told.extend(self.service.take_in(head, self.channel, self.dm).await);
        }
        let (community, channel, root) = (
            self.service.community,
            self.channel,
            self.service.apps_root.clone(),
        );
        if let Ok(Ok(gone)) = self
            .service
            .store
            .call(move |store| {
                Ok(Apps::at(store, root)
                    .offers_in_set(community, channel, &live)
                    .map_err(|error| error.to_string()))
            })
            .await
        {
            told.extend(gone);
        }
        if !told.is_empty() {
            self.tell(OfferNews::Told {
                channel: self.channel,
                told,
            });
        }
        self.tell(OfferNews::Synced {
            channel: self.channel,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::refused;

    #[test]
    fn a_refused_offer_is_said_in_the_owners_terms() {
        assert!(
            refused("restricted: only members can post in this channel")
                .contains("a guest cannot offer")
        );
        assert_eq!(
            refused("restricted: not a channel member"),
            "you are not a member of this channel"
        );
        assert!(refused("invalid: channel is archived").contains("archived"));
        assert!(refused("rate-limited: quota exceeded; retry in 40s").contains("wait 40 s"));
        assert_eq!(refused("blocked: nope"), "blocked: nope");
    }
}
