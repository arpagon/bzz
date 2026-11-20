//! The records of apps (v0.16, feature `apps`): one typed NIP-AR sync for
//! each open app, and create, revise and delete with content.
//!
//! This is the half of milestone M2 that knows the relay and nothing of Lua
//! or of drawing. `docs/app-records.md` is the interface in words;
//! `protocol::record` holds the pure parts (names, fence, mapping, checks).
//!
//! What it promises its caller:
//!
//! - **The fence** (rule 17): only the record types the app declared, only in
//!   the channel it is open in, for reads and for writes, whoever asks.
//! - **One set, never emptied.** The caller holds the set from the moment
//!   the app opens (first what the store kept, then what the relay says) and
//!   is told only real differences, also when the relay makes bzz fetch the
//!   whole set again.
//! - **No frame the relay would refuse**, as far as bzz can know: a refused
//!   frame costs the person's quota as an accepted one does.
//! - **An answer for every write**, one of `protocol::record::RecordError`.

use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use nostr::{Event, EventBuilder};
use serde_json::{Map, Value, json};
use tokio::{
    sync::{Notify, broadcast, mpsc},
    time::Instant,
};
use uuid::Uuid;

use crate::{
    auth::signer::SignerHandle,
    error::Error,
    protocol::{
        artifact::{self, Artifact, KIND_ARTIFACT, KIND_ARTIFACT_REMOVAL},
        http::HttpClient,
        record::{
            Change, Fence, MAX_LIVE_RECORDS, Record, RecordError, Refusal, Verdict, WriteBudget,
            difference, verdict,
        },
    },
    realtime::{
        admission::{self, ClosedReason, SubscriptionPriority},
        session::SessionEvent,
        supervisor::{SupervisorEvent, SupervisorHandle},
    },
    store::writer::StoreHandle,
};

/// How long a write that met a stale head waits for the app's own sync to
/// deliver the new one, before it asks the relay over HTTP. On the pinned
/// relay the head arrived 2 to 8 ms after the refusal.
pub const CONFLICT_SYNC_WAIT: Duration = Duration::from_millis(300);
/// The wait before a sync the relay ended with `resync:` is asked again. An
/// immediate retry met a second `resync:` in 2 of 30 rounds on the pinned
/// relay; 250 ms later it never did.
pub const RESYNC_DELAY: Duration = Duration::from_millis(250);
/// The longest wait between two attempts when the relay keeps ending the
/// sync before it completes.
pub const MAX_RESYNC_DELAY: Duration = Duration::from_secs(30);
/// The relay's bound on one sync, used when NIP-11 could not be read.
const DEFAULT_MAX_EVENTS: usize = 4096;
/// Own writes remembered so that their echo on the sync is not announced.
const OWN_WRITES: usize = 256;

/// Who signs a write and on which connection it is sent. The owner's is the
/// identity bzz is unlocked with. M7 makes one from a local agent's signer
/// and that agent's own connection: the relay counts an agent's writes
/// against the agent's quota, and accepts an event only on a connection
/// authenticated as its signer.
#[derive(Clone)]
pub struct Writer {
    signer: SignerHandle,
    supervisor: SupervisorHandle,
}

impl Writer {
    pub const fn new(signer: SignerHandle, supervisor: SupervisorHandle) -> Self {
        Self { signer, supervisor }
    }

    /// The identity records are signed with (public key, hex). It does not
    /// change for the life of the identity.
    pub fn pubkey(&self) -> String {
        self.signer.public_key().to_hex()
    }
}

/// Where the app's set stands. The caller is told every change of it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum SyncState {
    /// The first complete set has not arrived. What the store kept is shown.
    #[default]
    Loading,
    /// The set is complete and kept current.
    Live,
    /// The relay ended the sync (a deleted message, a removed member, a
    /// dropped connection) and bzz is asking again. The set is the last
    /// complete one; nothing is emptied.
    Resyncing,
    /// The app's types hold more live records in this channel than one sync
    /// returns (`max`). Final: asking again gives the same answer. Records
    /// can still be revised and deleted, which is the way out; none can be
    /// created.
    TooMany { max: usize },
    /// This identity cannot read the channel's records. Final: the app
    /// cannot be used here.
    Restricted,
    /// This relay cannot sync records, or refused the request.
    Unavailable { reason: String },
}

impl SyncState {
    /// Whether bzz will not ask again by itself.
    pub const fn is_final(&self) -> bool {
        matches!(
            self,
            Self::TooMany { .. } | Self::Restricted | Self::Unavailable { .. }
        )
    }
}

/// Why a batch of changes is told.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cause {
    /// One revision arrived as it happened.
    Live,
    /// bzz fetched the whole set (the first time, or again) and these are
    /// all the differences from what the caller held.
    Sync,
}

/// What an open app is told.
#[derive(Clone, Debug, PartialEq)]
pub enum RecordEvent {
    State(SyncState),
    /// Changes by other participants, each with who signed it. The caller's
    /// own writes are answered by their call and are not repeated here.
    Changes {
        changes: Vec<Change>,
        cause: Cause,
    },
}

/// The records of every app of one community.
#[derive(Clone)]
pub struct RecordsService {
    community_id: Uuid,
    http: HttpClient,
    store: StoreHandle,
    owner: Writer,
    /// Subscriptions of the apps that are open: one instance per channel.
    open: Arc<Mutex<HashSet<String>>>,
    /// What the relay's NIP-11 says of artifacts, once it was read.
    capabilities: Arc<Mutex<Option<artifact::Capabilities>>>,
    /// What the owner's apps may still write this minute: one budget for
    /// every app of this identity, because the relay has one for the person.
    budget: SharedBudget,
}

/// The budget of writes the open apps of one identity share
/// (`protocol::record::APP_WRITES_PER_MINUTE`). Whoever runs an app for the
/// owner (`apps::session`) takes a write out of it before it calls `create`,
/// `update` or `delete`.
pub type SharedBudget = Arc<Mutex<WriteBudget>>;

impl RecordsService {
    pub fn new(community_id: Uuid, http: HttpClient, store: StoreHandle, owner: Writer) -> Self {
        Self {
            community_id,
            http,
            store,
            owner,
            open: Arc::default(),
            capabilities: Arc::default(),
            budget: Arc::default(),
        }
    }

    /// The identity the owner's writes are signed with (public key, hex).
    pub fn owner_pubkey(&self) -> String {
        self.owner.pubkey()
    }

    /// The budget of writes every app of the owner shares.
    pub fn budget(&self) -> SharedBudget {
        self.budget.clone()
    }

    /// Whether `app` is open in `channel` now. An app that was just closed
    /// stays open until its sync has ended, a moment later.
    pub fn is_open(&self, channel: Uuid, app: &str) -> bool {
        lock(&self.open).contains(&Fence::subscription_of(app, channel))
    }

    /// Opens the records of `app` in `channel` for the short type names it
    /// declared. Returns at once with what the store kept; the relay's set
    /// follows on the receiver. Dropping every clone of the handle closes
    /// the sync.
    pub async fn open<S: AsRef<str>>(
        &self,
        channel: Uuid,
        app: &str,
        types: &[S],
    ) -> Result<(AppRecords, mpsc::UnboundedReceiver<RecordEvent>), RecordError> {
        let fence = Fence::new(app, channel, types)?;
        let subscription = fence.subscription_id();
        if !lock(&self.open).insert(subscription.clone()) {
            return Err(RecordError::Invalid {
                reason: format!("the app {app} is already open in this channel"),
            });
        }
        let registration = Registration {
            open: self.open.clone(),
            subscription: subscription.clone(),
        };
        let (community, long_types) = (self.community_id, fence.long_types());
        let stored = self
            .store
            .call(move |store| store.typed_artifacts(community, channel, &long_types))
            .await
            .map_err(|error| unavailable(format!("the local store failed: {error}")))?;
        let mut state = State::default();
        for head in stored {
            state.admit(&fence, head);
        }
        let shared = Arc::new(Shared {
            fence,
            state: Mutex::new(state),
            changed: Notify::new(),
        });
        let (events, receiver) = mpsc::unbounded_channel();
        let (commands, command_receiver) = mpsc::unbounded_channel();
        // Listen before the request is sent, so no head of it is missed.
        let relay_events = self.owner.supervisor.subscribe_events();
        tokio::spawn(
            SyncTask {
                shared: shared.clone(),
                service: self.clone(),
                events: events.clone(),
                subscription,
                max_events: DEFAULT_MAX_EVENTS,
                phase: Phase::Idle,
                attempts: 0,
                retry_at: None,
                _registration: registration,
            }
            .run(relay_events, command_receiver),
        );
        Ok((
            AppRecords {
                shared,
                service: self.clone(),
                events,
                commands,
            },
            receiver,
        ))
    }

    /// How many live artifacts of every type the channel holds, by the
    /// relay's count. Past the relay's `max_events` (4096) the channel-wide
    /// sync of the artifact panel fails for every client, whoever wrote the
    /// artifacts; an app that opens can warn before that.
    pub async fn channel_total(&self, channel: Uuid) -> Result<u64, RecordError> {
        let body =
            serde_json::to_vec(&json!([{"artifact": "current", "#h": [channel.to_string()]}]))
                .map_err(|error| unavailable(error.to_string()))?;
        let (status, bytes, _) = self
            .http
            .post_once_bound("count", body, 4096)
            .await
            .map_err(|_| unavailable("the relay's count could not be read"))?;
        serde_json::from_slice::<Value>(&bytes)
            .ok()
            .filter(|_| status == 200)
            .and_then(|value| value["count"].as_u64())
            .ok_or_else(|| unavailable("the relay's count could not be read"))
    }
}

/// Removes an app from the open set when its sync ends.
struct Registration {
    open: Arc<Mutex<HashSet<String>>>,
    subscription: String,
}

impl Drop for Registration {
    fn drop(&mut self) {
        lock(&self.open).remove(&self.subscription);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn unavailable(reason: impl Into<String>) -> RecordError {
    RecordError::Unavailable {
        reason: reason.into(),
    }
}

/// A write of the caller that the relay has or may have stored.
#[derive(Clone, Debug)]
struct OwnWrite {
    revision: String,
    record: Uuid,
    /// The head it was built on; `None` for a create.
    prev: Option<String>,
    deletion: bool,
    /// The sync request that was the latest when the relay acknowledged it.
    accepted: Option<u64>,
}

#[derive(Default)]
struct State {
    /// Live heads inside the fence that are records, by identity.
    heads: BTreeMap<Uuid, Artifact>,
    records: BTreeMap<Uuid, Record>,
    sync: SyncState,
    /// Creates on their way, counted against the limit.
    creating: usize,
    own: VecDeque<OwnWrite>,
    /// Counts the sync requests, sent or sent again by the supervisor. A
    /// write acknowledged under an earlier one is in every later cut.
    request: u64,
}

impl State {
    /// Takes a live head that is inside the fence. Returns whether it is a
    /// record.
    fn admit(&mut self, fence: &Fence, head: Artifact) -> bool {
        let id = head.id;
        match fence.record(&head) {
            Some(record) => {
                self.records.insert(id, record);
                self.heads.insert(id, head);
                true
            }
            None => {
                self.records.remove(&id);
                self.heads.remove(&id);
                false
            }
        }
    }

    fn forget(&mut self, id: Uuid) -> Option<Record> {
        self.heads.remove(&id);
        self.records.remove(&id)
    }

    fn live(&self) -> usize {
        self.records.len() + self.creating
    }

    fn remember(&mut self, write: OwnWrite) {
        if self.own.len() >= OWN_WRITES {
            self.own.pop_front();
        }
        self.own.push_back(write);
    }

    fn is_own(&self, revision: &str) -> bool {
        self.own.iter().any(|write| write.revision == revision)
    }

    /// A live head, from the sync or from the HTTP head view. Returns the
    /// change it makes to the set, if any.
    fn apply(&mut self, fence: &Fence, head: Artifact) -> Option<Change> {
        let id = head.id;
        let known = self.heads.get(&id);
        if known.is_some_and(|known| known.head == head.head) {
            return None;
        }
        // The revision the held head was built on: it arrived late.
        if known.is_some_and(|known| known.tag("prev").as_deref() == Some(head.head.as_str())) {
            return None;
        }
        if head.is_deleted() || head.home != fence.channel() {
            let by = Some(head.author.clone());
            return self.forget(id).map(|record| Change::Removed { record, by });
        }
        let author = head.author.clone();
        let previous = self.records.get(&id).cloned();
        if self.admit(fence, head) {
            self.records.get(&id).cloned().map(Change::Upsert)
        } else {
            // Revised into something that is no record of this contract.
            previous.map(|record| Change::Removed {
                record,
                by: Some(author),
            })
        }
    }
}

struct Shared {
    fence: Fence,
    state: Mutex<State>,
    /// Woken whenever the set changes; a write that met a stale head waits
    /// on it for the new one.
    changed: Notify,
}

/// The records of one open app. Cheap to clone; the sync ends when the last
/// clone is dropped.
#[derive(Clone)]
pub struct AppRecords {
    shared: Arc<Shared>,
    service: RecordsService,
    events: mpsc::UnboundedSender<RecordEvent>,
    /// Closing this channel is what ends the sync.
    commands: mpsc::UnboundedSender<Command>,
}

enum Command {
    Retry,
}

/// One create on its way, counted against the limit until it is answered.
struct Creating(Arc<Shared>);

impl Drop for Creating {
    fn drop(&mut self) {
        let mut state = lock(&self.0.state);
        state.creating = state.creating.saturating_sub(1);
    }
}

/// What a write does once it is signed.
enum Intent {
    Create,
    Update,
    Delete,
}

impl AppRecords {
    pub fn fence(&self) -> &Fence {
        &self.shared.fence
    }

    /// The identity the owner's writes are signed with (public key, hex).
    pub fn identity(&self) -> String {
        self.service.owner.pubkey()
    }

    pub fn state(&self) -> SyncState {
        lock(&self.shared.state).sync.clone()
    }

    /// The set as it stands, in the order of the records' ids. That order
    /// means nothing: a record carries its own order field.
    pub fn records(&self) -> Vec<Record> {
        lock(&self.shared.state).records.values().cloned().collect()
    }

    pub fn record(&self, id: Uuid) -> Option<Record> {
        lock(&self.shared.state).records.get(&id).cloned()
    }

    /// Asks for the set again after a final state, for example once records
    /// were deleted in a channel that held too many.
    pub fn retry(&self) {
        let _ = self.commands.send(Command::Retry);
    }

    /// Creates a record of a declared type. `content` is the app's own
    /// fields; the answer is the record as stored, with its revision.
    pub async fn create(
        &self,
        record_type: &str,
        content: Map<String, Value>,
    ) -> Result<Record, RecordError> {
        self.create_by(&self.service.owner.clone(), record_type, content, false)
            .await
    }

    /// Replaces a record's whole content. `revision` is the one the caller
    /// saw; if the record was revised since, the answer is `Conflict` with
    /// the record as it is now.
    pub async fn update(
        &self,
        id: Uuid,
        revision: &str,
        content: Map<String, Value>,
    ) -> Result<Record, RecordError> {
        self.revise_by(
            &self.service.owner.clone(),
            id,
            revision,
            Some(content),
            false,
        )
        .await
    }

    /// Deletes a record, under the same rule as `update`. The answer is the
    /// record's last state.
    pub async fn delete(&self, id: Uuid, revision: &str) -> Result<Record, RecordError> {
        self.revise_by(&self.service.owner.clone(), id, revision, None, false)
            .await
    }

    /// `create`, signed and sent by another writer of this bzz (M7: a local
    /// agent, with its own key and connection). The app is told of it as of
    /// any other participant's change.
    pub async fn create_as(
        &self,
        writer: &Writer,
        record_type: &str,
        content: Map<String, Value>,
    ) -> Result<Record, RecordError> {
        self.create_by(writer, record_type, content, true).await
    }

    /// `update` by another writer of this bzz.
    pub async fn update_as(
        &self,
        writer: &Writer,
        id: Uuid,
        revision: &str,
        content: Map<String, Value>,
    ) -> Result<Record, RecordError> {
        self.revise_by(writer, id, revision, Some(content), true)
            .await
    }

    /// `delete` by another writer of this bzz.
    pub async fn delete_as(
        &self,
        writer: &Writer,
        id: Uuid,
        revision: &str,
    ) -> Result<Record, RecordError> {
        self.revise_by(writer, id, revision, None, true).await
    }

    /// The states in which nothing can be written, before anything is built.
    fn writable(state: &State) -> Result<(), RecordError> {
        match &state.sync {
            SyncState::Restricted => Err(RecordError::Refused {
                refusal: Refusal::Unreadable,
                reason: "you cannot read this channel's records".into(),
            }),
            SyncState::Unavailable { reason } => Err(unavailable(reason.clone())),
            _ => Ok(()),
        }
    }

    async fn create_by(
        &self,
        writer: &Writer,
        record_type: &str,
        content: Map<String, Value>,
        announce: bool,
    ) -> Result<Record, RecordError> {
        let fence = &self.shared.fence;
        // The fence first: an undeclared type is refused whatever else holds.
        fence.long_type(record_type)?;
        let id = Uuid::new_v4();
        let builder = fence.create(record_type, id, &content)?;
        {
            let mut state = lock(&self.shared.state);
            Self::writable(&state)?;
            if matches!(state.sync, SyncState::TooMany { .. }) || state.live() >= MAX_LIVE_RECORDS {
                return Err(RecordError::Limit {
                    limit: MAX_LIVE_RECORDS,
                });
            }
            state.creating += 1;
        }
        // Released also when the caller gives up waiting.
        let _slot = Creating(self.shared.clone());
        self.publish(writer, builder, id, None, Intent::Create, announce)
            .await
    }

    async fn revise_by(
        &self,
        writer: &Writer,
        id: Uuid,
        revision: &str,
        content: Option<Map<String, Value>>,
        announce: bool,
    ) -> Result<Record, RecordError> {
        let fence = &self.shared.fence;
        let head = {
            let state = lock(&self.shared.state);
            Self::writable(&state)?;
            // Only what is in the app's own set can be named: a record of
            // another type or another channel is not there to be found.
            let Some(head) = state.heads.get(&id) else {
                return Err(RecordError::NotFound);
            };
            if head.head != revision {
                // Known without asking the relay, and without its quota.
                return Err(match state.records.get(&id) {
                    Some(current) => RecordError::Conflict {
                        current: Box::new(current.clone()),
                    },
                    None => RecordError::NotFound,
                });
            }
            head.clone()
        };
        let (builder, intent) = match &content {
            Some(content) => (fence.update(&head, content)?, Intent::Update),
            None => (fence.delete(&head)?, Intent::Delete),
        };
        self.publish(writer, builder, id, Some(head), intent, announce)
            .await
    }

    async fn publish(
        &self,
        writer: &Writer,
        builder: EventBuilder,
        id: Uuid,
        prev: Option<Artifact>,
        intent: Intent,
        announce: bool,
    ) -> Result<Record, RecordError> {
        let fence = &self.shared.fence;
        if let Some(reason) = writer.signer.refusal(KIND_ARTIFACT) {
            return Err(RecordError::Refused {
                refusal: Refusal::Signer,
                reason: reason.into(),
            });
        }
        let event = writer
            .signer
            .sign(builder)
            .await
            .map_err(|error| match error {
                Error::SignerRefused(reason) => RecordError::Refused {
                    refusal: Refusal::Signer,
                    reason,
                },
                Error::Locked(_) => unavailable("the identity is locked"),
                _ => unavailable("the record could not be signed"),
            })?;
        // The last look before a frame costs quota.
        fence.check(&event)?;
        let head = artifact::parse(&event).map_err(|_| RecordError::Invalid {
            reason: "the revision is not a valid artifact".into(),
        })?;
        let revision = head.head.clone();
        let stale = prev.as_ref().map(|prev| prev.head.clone());
        if !announce {
            lock(&self.shared.state).remember(OwnWrite {
                revision: revision.clone(),
                record: id,
                prev: stale.clone(),
                deletion: matches!(intent, Intent::Delete),
                accepted: None,
            });
        }
        let forget = |shared: &Shared| {
            lock(&shared.state)
                .own
                .retain(|write| write.revision != revision);
        };
        let ack = match writer.supervisor.publish_app(event).await {
            Ok(ack) => ack,
            Err(error) => {
                // Unknown whether it was stored: if it was, its head arrives
                // on the sync and is then told as a change.
                forget(&self.shared);
                return Err(transport(&error));
            }
        };
        if !ack.accepted {
            forget(&self.shared);
            return Err(match verdict(&ack.message) {
                Verdict::RateLimited { seconds } => RecordError::RateLimited { seconds },
                Verdict::HeadChanged => match stale {
                    Some(stale) => self.current_after_conflict(id, &stale).await,
                    None => RecordError::Refused {
                        refusal: Refusal::Other,
                        reason: "the relay reported a conflict for a new record".into(),
                    },
                },
                Verdict::Gone => RecordError::NotFound,
                Verdict::TooLarge => RecordError::TooLarge {
                    what: crate::protocol::record::Oversize::Content,
                    bytes: head.event.content.len(),
                    limit: crate::protocol::record::MAX_CONTENT_BYTES,
                },
                Verdict::Refused { refusal, reason } => RecordError::Refused { refusal, reason },
            });
        }
        self.stored(head, prev, intent, announce).await
    }

    /// The relay took the write: the set and the store follow at once, so
    /// that the caller's next write can build on it without waiting for the
    /// echo.
    async fn stored(
        &self,
        head: Artifact,
        prev: Option<Artifact>,
        intent: Intent,
        announce: bool,
    ) -> Result<Record, RecordError> {
        let fence = &self.shared.fence;
        let (community, channel, id) = (self.service.community_id, fence.channel(), head.id);
        let (answer, change) = {
            let mut state = lock(&self.shared.state);
            let request = state.request;
            if let Some(write) = state
                .own
                .iter_mut()
                .find(|write| write.revision == head.head)
            {
                write.accepted = Some(request);
            }
            let last = prev.as_ref().and_then(|prev| fence.record(prev));
            // The set moves only if it still stands where this write found
            // it: a later revision that arrived first is not undone.
            let stands = match (&prev, state.heads.get(&id)) {
                (None, None) => true,
                (Some(prev), Some(known)) => known.head == prev.head,
                _ => false,
            };
            let change = match intent {
                Intent::Delete => {
                    let removed = stands.then(|| state.forget(id)).flatten();
                    removed.map(|record| Change::Removed {
                        record,
                        by: Some(head.author.clone()),
                    })
                }
                Intent::Create | Intent::Update => {
                    if stands && state.admit(fence, head.clone()) {
                        state.records.get(&id).cloned().map(Change::Upsert)
                    } else {
                        None
                    }
                }
            };
            let answer = match intent {
                Intent::Delete => last,
                Intent::Create | Intent::Update => fence.record(&head),
            };
            (answer, change)
        };
        self.shared.changed.notify_waiters();
        if let Some(change) = change {
            let stored = match &change {
                Change::Upsert(_) => {
                    let head = head.clone();
                    self.service
                        .store
                        .call(move |store| store.apply_artifact_head(community, &head))
                        .await
                }
                Change::Removed { .. } => {
                    self.service
                        .store
                        .call(move |store| store.remove_artifact(community, id, channel))
                        .await
                }
            };
            if let Err(error) = stored {
                tracing::warn!(%error, "an app record was not written to the local store");
            }
            if announce {
                let _ = self.events.send(RecordEvent::Changes {
                    changes: vec![change],
                    cause: Cause::Live,
                });
            }
        }
        answer.ok_or_else(|| RecordError::Invalid {
            reason: "the stored revision is not a record".into(),
        })
    }

    /// The record as it is now, after the relay refused a stale head. The
    /// refusal does not carry it. The app's own sync delivers it within
    /// milliseconds; the HTTP head view is the fallback, and the only one
    /// that shows a deletion.
    async fn current_after_conflict(&self, id: Uuid, stale: &str) -> RecordError {
        let fence = &self.shared.fence;
        let look = |state: &State| match state.heads.get(&id) {
            Some(head) if head.head == stale => None,
            Some(_) => Some(match state.records.get(&id) {
                Some(current) => RecordError::Conflict {
                    current: Box::new(current.clone()),
                },
                None => RecordError::NotFound,
            }),
            None => Some(RecordError::NotFound),
        };
        let deadline = Instant::now() + CONFLICT_SYNC_WAIT;
        loop {
            let changed = self.shared.changed.notified();
            if let Some(answer) = look(&lock(&self.shared.state)) {
                return answer;
            }
            if tokio::time::timeout_at(deadline, changed).await.is_err() {
                break;
            }
        }
        let Ok(events) = self
            .service
            .http
            .query_raw(&[artifact::head_filter(id)])
            .await
        else {
            return unavailable("the record changed, and its current state could not be read");
        };
        let Some(head) = events
            .iter()
            .filter_map(|event| artifact::parse(event).ok())
            .find(|head| head.id == id)
        else {
            return RecordError::NotFound;
        };
        if head.head == stale {
            return unavailable("the record changed, and its current state could not be read");
        }
        // Outside the fence it is no record of this app, whatever it is now.
        if fence.long_types().contains(&head.artifact_type) {
            let change = lock(&self.shared.state).apply(fence, head.clone());
            if let Some(change) = change {
                self.shared.changed.notify_waiters();
                self.service.persist(fence, &head, &change).await;
                let _ = self.events.send(RecordEvent::Changes {
                    changes: vec![change],
                    cause: Cause::Live,
                });
            }
        }
        look(&lock(&self.shared.state)).unwrap_or(RecordError::NotFound)
    }
}

impl RecordsService {
    /// The relay's artifact capabilities, read once. `None` while the relay
    /// cannot be reached.
    async fn capabilities(&self) -> Option<artifact::Capabilities> {
        if let Some(known) = *lock(&self.capabilities) {
            return Some(known);
        }
        let caps = artifact::capabilities(&self.http.nip11().await.ok()?);
        *lock(&self.capabilities) = Some(caps);
        Some(caps)
    }

    async fn persist(&self, fence: &Fence, head: &Artifact, change: &Change) {
        let (community, channel, id) = (self.community_id, fence.channel(), head.id);
        let result = match change {
            Change::Upsert(_) => {
                let head = head.clone();
                self.store
                    .call(move |store| store.apply_artifact_head(community, &head))
                    .await
            }
            Change::Removed { .. } => {
                self.store
                    .call(move |store| store.remove_artifact(community, id, channel))
                    .await
            }
        };
        if let Err(error) = result {
            tracing::warn!(%error, "an app record was not written to the local store");
        }
    }
}

/// What a write that got no acknowledgement means to the caller.
fn transport(error: &Error) -> RecordError {
    match error {
        // bzz dropped it before the wire: nothing was sent.
        Error::Network(_) if admission::is_local_admission_error(error) => {
            unavailable("not sent: the connection to the relay dropped")
        }
        Error::Network(message) => match admission::rate_limit_retry_after(message) {
            Some(wait) => RecordError::RateLimited {
                seconds: wait.as_secs().max(1),
            },
            None if message == "session is offline" => unavailable("not sent: bzz is offline"),
            // On the wire when the connection went: only the sync can tell.
            None => unavailable(
                "the connection dropped before the relay acknowledged the write; it may still have been stored",
            ),
        },
        Error::Timeout(_) => {
            unavailable("the relay did not acknowledge the write; it may still have been stored")
        }
        Error::Access(_) | Error::Auth(_) => {
            unavailable("not sent: bzz is not signed in to the relay")
        }
        _ => unavailable("the write could not be sent"),
    }
}

/// Where the sync stands on the wire.
enum Phase {
    /// No request is open: before the first, between two, or after a final
    /// answer.
    Idle,
    /// Heads before `EOSE`: the set is complete only at its end.
    Collecting(Vec<Artifact>),
    /// After `EOSE`: every head is a change.
    Live,
}

/// The task of one open app's sync.
struct SyncTask {
    shared: Arc<Shared>,
    service: RecordsService,
    events: mpsc::UnboundedSender<RecordEvent>,
    subscription: String,
    max_events: usize,
    phase: Phase,
    /// Syncs ended by the relay since the last complete one.
    attempts: u32,
    retry_at: Option<Instant>,
    _registration: Registration,
}

impl SyncTask {
    async fn run(
        mut self,
        mut relay: broadcast::Receiver<SupervisorEvent>,
        mut commands: mpsc::UnboundedReceiver<Command>,
    ) {
        // A relay without the sync would answer the request with a notice
        // that names no subscription; ask what it can do first. A relay that
        // cannot be reached now is asked through the supervisor, which sends
        // the request when it connects.
        match self.service.capabilities().await {
            Some(caps) if !caps.sync => {
                self.set_state(SyncState::Unavailable {
                    reason: "this relay has no artifact sync".into(),
                });
            }
            Some(caps) => {
                self.max_events = caps.max_events;
                self.request().await;
            }
            None => self.request().await,
        }
        loop {
            let retry = self.retry_at;
            tokio::select! {
                command = commands.recv() => match command {
                    Some(Command::Retry) => {
                        if self.state().is_final() {
                            self.attempts = 0;
                            self.set_state(SyncState::Resyncing);
                            self.request().await;
                        }
                    }
                    // Every handle is gone: the app was closed.
                    None => break,
                },
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
                    Ok(SupervisorEvent::Session(event)) => self.session(event).await,
                    // A new connection, also one the person asked for: the
                    // supervisor says nothing of the old one in that case.
                    Ok(SupervisorEvent::Connecting) => self.reconnecting(),
                    Ok(_) => {}
                    // Frames were missed: only a whole new set is known to
                    // be whole.
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if !self.state().is_final() {
                            self.interrupted();
                            self.request().await;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                },
            }
        }
        let _ = self
            .service
            .owner
            .supervisor
            .close(self.subscription.clone())
            .await;
    }

    fn state(&self) -> SyncState {
        lock(&self.shared.state).sync.clone()
    }

    fn set_state(&self, next: SyncState) {
        {
            let mut state = lock(&self.shared.state);
            if state.sync == next {
                return;
            }
            state.sync = next.clone();
        }
        let _ = self.events.send(RecordEvent::State(next));
    }

    /// Sends the request, or sends it again under the same subscription:
    /// the relay replaces whatever held the id.
    async fn request(&mut self) {
        self.phase = Phase::Collecting(Vec::new());
        self.retry_at = None;
        lock(&self.shared.state).request += 1;
        let sent = self
            .service
            .owner
            .supervisor
            .subscribe_with_priority(
                self.subscription.clone(),
                vec![self.shared.fence.sync_filter()],
                SubscriptionPriority::Foreground,
            )
            .await;
        if sent.is_err() {
            self.phase = Phase::Idle;
            self.set_state(SyncState::Unavailable {
                reason: "bzz's connection to the relay has stopped".into(),
            });
        }
    }

    /// The set stopped being kept current, and will be asked again. What
    /// the caller holds stays as it is.
    fn interrupted(&mut self) {
        self.phase = Phase::Idle;
        if self.state() == SyncState::Live {
            self.set_state(SyncState::Resyncing);
        }
    }

    async fn session(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Event {
                subscription,
                event,
            } if subscription == self.subscription => self.head(event).await,
            SessionEvent::Eose(subscription) if subscription == self.subscription => {
                if matches!(self.phase, Phase::Collecting(_))
                    && let Phase::Collecting(buffer) =
                        std::mem::replace(&mut self.phase, Phase::Live)
                {
                    self.commit(buffer).await;
                }
            }
            SessionEvent::Closed {
                subscription,
                message,
            } if subscription == self.subscription => self.closed(&message),
            SessionEvent::Disconnected(_) => self.reconnecting(),
            _ => {}
        }
    }

    /// The connection dropped or is being made anew. The supervisor sends
    /// the request again once it is connected, and what follows is a whole
    /// set: heads are collected until its `EOSE`, never applied one by one
    /// as if the old sync went on.
    fn reconnecting(&mut self) {
        if self.state().is_final() {
            return;
        }
        let waiting = self.retry_at.is_some();
        self.interrupted();
        if !waiting {
            self.phase = Phase::Collecting(Vec::new());
            lock(&self.shared.state).request += 1;
        }
    }

    fn closed(&mut self, message: &str) {
        self.phase = Phase::Idle;
        match admission::closed_reason(message) {
            ClosedReason::Resync => {
                // Ordinary chat does this: a deleted message, a changed
                // topic, a removed member. Ask again shortly; wait longer
                // each time the relay ends the sync before it completes.
                let delay = RESYNC_DELAY
                    .saturating_mul(1_u32 << self.attempts.min(8))
                    .min(MAX_RESYNC_DELAY);
                self.attempts = self.attempts.saturating_add(1);
                self.retry_at = Some(Instant::now() + delay);
                if self.state() == SyncState::Live {
                    self.set_state(SyncState::Resyncing);
                }
            }
            ClosedReason::SyncOverflow => self.set_state(SyncState::TooMany {
                max: self.max_events,
            }),
            ClosedReason::Restricted => self.set_state(SyncState::Restricted),
            ClosedReason::Other => self.set_state(SyncState::Unavailable {
                reason: "the relay refused the request for this app's records".into(),
            }),
        }
    }

    async fn head(&mut self, event: Event) {
        let fence = &self.shared.fence;
        match event.kind.as_u16() {
            KIND_ARTIFACT => {
                // Signature and envelope, with the relay's own validator.
                let Ok(head) = artifact::parse(&event) else {
                    return;
                };
                // The fence for what comes in: the relay sends only what the
                // filter matches, and bzz does not take its word for it. A
                // head of a declared type homed elsewhere is a move away.
                if !fence.long_types().contains(&head.artifact_type) {
                    return;
                }
                match &mut self.phase {
                    Phase::Collecting(buffer) => {
                        if buffer.len() >= self.max_events {
                            // More than one sync may hold: never a part of a
                            // set as if it were the whole.
                            self.phase = Phase::Idle;
                            self.set_state(SyncState::TooMany {
                                max: self.max_events,
                            });
                        } else {
                            buffer.push(head);
                        }
                    }
                    Phase::Live => {
                        let (change, own) = {
                            let mut state = lock(&self.shared.state);
                            let own = state.is_own(&head.head);
                            (state.apply(fence, head.clone()), own)
                        };
                        if let Some(change) = change {
                            self.shared.changed.notify_waiters();
                            self.service.persist(fence, &head, &change).await;
                            if !own {
                                let _ = self.events.send(RecordEvent::Changes {
                                    changes: vec![change],
                                    cause: Cause::Live,
                                });
                            }
                        }
                    }
                    Phase::Idle => {}
                }
            }
            // The artifact moved to a channel this reader cannot read: the
            // relay says so itself, and only its pinned key is believed.
            KIND_ARTIFACT_REMOVAL if matches!(self.phase, Phase::Live) => {
                let community = self.service.community_id;
                let Ok(relay) = self
                    .service
                    .store
                    .call(move |store| store.pinned_relay_pubkey(community))
                    .await
                else {
                    return;
                };
                let Ok(removal) = artifact::parse_removal(&event, relay.as_deref()) else {
                    return;
                };
                if removal.source != fence.channel() {
                    return;
                }
                let removed = lock(&self.shared.state).forget(removal.id);
                if let Some(record) = removed {
                    self.shared.changed.notify_waiters();
                    let (id, channel) = (removal.id, removal.source);
                    let _ = self
                        .service
                        .store
                        .call(move |store| store.remove_artifact(community, id, channel))
                        .await;
                    let _ = self.events.send(RecordEvent::Changes {
                        changes: vec![Change::Removed { record, by: None }],
                        cause: Cause::Live,
                    });
                }
            }
            // Anything else on this subscription is not an artifact. The
            // pinned relay was seen to send one such frame after a resync.
            _ => {}
        }
    }

    /// `EOSE`: the heads collected are the whole set at one cut. The caller
    /// is told what differs from the set it holds, and nothing else.
    async fn commit(&mut self, buffer: Vec<Artifact>) {
        let fence = &self.shared.fence;
        let mut fresh = State::default();
        // In the order received: a later head of the same record wins.
        for head in buffer {
            if head.is_deleted() || head.home != fence.channel() {
                fresh.forget(head.id);
            } else {
                fresh.admit(fence, head);
            }
        }
        let (changes, heads) = {
            let mut state = lock(&self.shared.state);
            // A write of the caller that the relay acknowledged since this
            // request was sent may be later than its cut; if the cut still
            // shows what the write was built on, it is, and it arrives as a
            // live head next. Without this the caller would see its own new
            // record vanish and come back.
            let request = state.request;
            for write in state
                .own
                .iter()
                .filter(|write| write.accepted == Some(request))
            {
                let cut = fresh.heads.get(&write.record).map(|head| head.head.clone());
                if cut != write.prev {
                    continue;
                }
                if write.deletion {
                    fresh.forget(write.record);
                } else if let Some(head) = state
                    .heads
                    .get(&write.record)
                    .filter(|head| head.head == write.revision)
                {
                    fresh.admit(fence, head.clone());
                }
            }
            let changes = difference(&state.records, &fresh.records)
                .into_iter()
                .filter(|change| match change {
                    Change::Upsert(record) => !state.is_own(&record.revision),
                    Change::Removed { .. } => true,
                })
                .collect::<Vec<_>>();
            state.heads = fresh.heads;
            state.records = fresh.records;
            (changes, state.heads.values().cloned().collect::<Vec<_>>())
        };
        self.attempts = 0;
        self.shared.changed.notify_waiters();
        let (community, channel, types) = (
            self.service.community_id,
            fence.channel(),
            fence.long_types(),
        );
        let stored = self
            .service
            .store
            .call(move |store| store.replace_artifact_types(community, channel, &types, &heads))
            .await;
        if let Err(error) = stored {
            tracing::warn!(%error, "an app's records were not written to the local store");
        }
        if !changes.is_empty() {
            let _ = self.events.send(RecordEvent::Changes {
                changes,
                cause: Cause::Sync,
            });
        }
        self.set_state(SyncState::Live);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_without_an_acknowledgement_says_whether_it_was_sent() {
        let reason = |error: Error| match transport(&error) {
            RecordError::Unavailable { reason } => reason,
            other => panic!("{other:?}"),
        };
        // Never on the wire.
        for error in [
            admission::local_admission_error("session disconnected before wire send"),
            admission::local_admission_error("cancelled by reconnect before wire send"),
            Error::Network("session is offline".into()),
            Error::Access("session is not authenticated".into()),
        ] {
            assert!(reason(error).starts_with("not sent:"));
        }
        // On the wire, unanswered: it may have been stored.
        for error in [
            Error::Network("relay closed the connection".into()),
            Error::Timeout("relay acknowledgement".into()),
        ] {
            assert!(reason(error).ends_with("it may still have been stored"));
        }
        // A wait bzz or the relay asked for is a wait, whoever worded it.
        for (error, seconds) in [
            (
                admission::app_write_deferred(Duration::from_millis(39_200)),
                40,
            ),
            (
                Error::Network("rate-limited: quota exceeded; retry in 7s".into()),
                7,
            ),
        ] {
            assert_eq!(transport(&error), RecordError::RateLimited { seconds });
        }
    }
}
