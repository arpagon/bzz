//! Records in memory, behind the same boundary the relay's service stands
//! behind ([`super::backend::Records`]). The headless driver runs an app
//! on it, and so do the tests of every platform: the session between the
//! script and this store is the one bzz's window uses.
//!
//! It keeps the promises of the contract and none it does not make: ids do
//! not sort by creation, a revision is an opaque string. It can also be
//! made to misbehave the way a relay does: a second participant, a
//! conflict, a quota, a refusal, a lost connection, a reply that comes
//! late, a set that had to be fetched again, and each state of a sync.
//!
//! What a call that changes the store returns ([`Told`]) is what the app's
//! session has not been told yet: give it to
//! [`Headless::deliver`](super::headless::Headless::deliver).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::backend::{Listing, Records, Told, Write};
use super::short_name;
use crate::apps::runtime::clock::SharedClock;
use crate::apps::runtime::records::{Record, Who, content_bytes};
use crate::protocol::record::{
    self, Change, MAX_CONTENT_BYTES, MAX_LIVE_RECORDS, Oversize, RecordError, Refusal, difference,
};
use crate::service::records::{Cause, SyncState};

/// The next update of a record first loses to another participant.
struct Armed {
    id: Uuid,
    who: String,
    content: Map<String, Value>,
}

pub struct MemoryStore {
    /// Who the app's own requests are signed by.
    local: Who,
    clock: SharedClock,
    /// Every live record, as the relay has it.
    records: BTreeMap<Uuid, record::Record>,
    /// The names of everyone who wrote here: the profiles of this world.
    names: HashMap<String, String>,
    counter: u64,
    /// Live records an app may have, and bytes of one record's content.
    pub live_limit: usize,
    pub content_limit: usize,
    /// Writes allowed in a window, when the relay's quota is set.
    quota: Option<(u32, Duration)>,
    writes: VecDeque<Duration>,
    available: bool,
    refusal: Option<(Refusal, String)>,
    fail_next: Vec<(String, RecordError)>,
    hold_next: Vec<String>,
    held: Vec<Told>,
    armed: Vec<Armed>,
    /// The set as the app last knew it, while the connection is lost.
    before_loss: Option<BTreeMap<Uuid, record::Record>>,
    counts: HashMap<&'static str, usize>,
    list_matches: Vec<String>,
    /// The record types the open app declared: nothing else is its own.
    declared: Vec<String>,
    sync: SyncState,
    /// What the session is told at its next look.
    outbox: VecDeque<Told>,
}

/// JSON that must be an object, as a record's content.
pub fn content(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

impl MemoryStore {
    pub fn new(local: Who, clock: SharedClock) -> Self {
        let names = HashMap::from([(local.id.clone(), local.name.clone())]);
        Self {
            local,
            clock,
            records: BTreeMap::new(),
            names,
            counter: 0,
            live_limit: MAX_LIVE_RECORDS,
            content_limit: MAX_CONTENT_BYTES,
            quota: None,
            writes: VecDeque::new(),
            available: true,
            refusal: None,
            fail_next: Vec::new(),
            hold_next: Vec::new(),
            held: Vec::new(),
            armed: Vec::new(),
            before_loss: None,
            counts: HashMap::new(),
            list_matches: Vec::new(),
            declared: Vec::new(),
            sync: SyncState::Live,
            outbox: VecDeque::new(),
        }
    }

    fn digest(&mut self, what: &str) -> [u8; 32] {
        self.counter += 1;
        Sha256::digest(format!("{what}:{}", self.counter)).into()
    }

    /// An id as the relay gives one: nothing in it says when it was made.
    fn new_id(&mut self) -> Uuid {
        let hash = self.digest("record");
        let mut bytes = [0_u8; 16];
        bytes.copy_from_slice(&hash[..16]);
        Uuid::from_bytes(bytes)
    }

    fn new_rev(&mut self) -> String {
        hex::encode(self.digest("rev"))
    }

    fn knows(&mut self, who: &Who) {
        self.names.insert(who.id.clone(), who.name.clone());
    }

    /// A record as a script would be handed it.
    fn shown(&self, record: &record::Record) -> Record {
        Record {
            id: record.id.to_string(),
            record_type: record.record_type.clone(),
            rev: record.revision.clone(),
            content: record.content.clone(),
            by: Who::new(
                record.author.clone(),
                self.names
                    .get(&record.author)
                    .cloned()
                    .unwrap_or_else(|| short_name(&record.author)),
            ),
        }
    }

    /// The set the app's side holds: the relay's, or while the connection
    /// is lost the one from before.
    fn held_set(&self) -> &BTreeMap<Uuid, record::Record> {
        self.before_loss.as_ref().unwrap_or(&self.records)
    }

    // ------------------------------------------------------------- reading

    /// Every live record, in no order.
    pub fn records(&self) -> Vec<Record> {
        self.records.values().map(|r| self.shown(r)).collect()
    }

    /// The first record whose content has the string `value` in `field`.
    pub fn find_by(&self, field: &str, value: &str) -> Option<Record> {
        self.records
            .values()
            .find(|r| r.content.get(field).and_then(Value::as_str) == Some(value))
            .map(|r| self.shown(r))
    }

    pub fn get(&self, record_id: &str) -> Option<Record> {
        let id = Uuid::parse_str(record_id).ok()?;
        self.records.get(&id).map(|r| self.shown(r))
    }

    /// How many requests of `op` (`list`, `create`, `update`, `delete`) of
    /// the app reached the store. A request the session answered by itself
    /// (an undeclared type, the app's own budget of writes) did not.
    pub fn count(&self, op: &str) -> usize {
        self.counts
            .iter()
            .find(|(name, _)| **name == op)
            .map_or(0, |(_, n)| *n)
    }

    /// The `match` of every list the app asked for, in order ("" for none).
    pub fn list_matches(&self) -> &[String] {
        &self.list_matches
    }

    /// The names of everyone who wrote here, by id.
    pub fn names(&self) -> &HashMap<String, String> {
        &self.names
    }

    // --------------------------------------------------- making it misbehave

    fn put(&mut self, record_type: &str, content: Map<String, Value>, by: &str) -> record::Record {
        let record = record::Record {
            id: self.new_id(),
            record_type: record_type.to_string(),
            revision: self.new_rev(),
            content,
            author: by.to_string(),
            signed_at: self.clock.wall().as_secs(),
        };
        self.records.insert(record.id, record.clone());
        record
    }

    /// A record that is simply there, written by `by`.
    pub fn seed(&mut self, record_type: &str, content: Map<String, Value>, by: &Who) -> Record {
        self.knows(by);
        let record = self.put(record_type, content, &by.id);
        self.shown(&record)
    }

    /// `writes` writes are taken by the relay in any `window`; the next is
    /// answered `rate_limited` with the seconds to wait. This is the
    /// relay's quota. The app's own budget is the session's.
    pub fn set_quota(&mut self, writes: u32, window: Duration) {
        self.quota = Some((writes, window));
    }

    /// Without a connection every write is answered `unavailable`. A list
    /// is still answered, from what the session holds.
    pub fn set_available(&mut self, available: bool) {
        self.available = available;
    }

    /// With a reason, every write is answered `refused` with it, as the
    /// relay refuses a guest. Reading still works.
    pub fn set_refusal(&mut self, reason: Option<&str>) {
        self.refusal = reason.map(|reason| (Refusal::Other, reason.to_string()));
    }

    /// The channel is archived, or no longer: the relay refuses every write
    /// and says so.
    pub fn set_archived(&mut self, archived: bool) {
        self.refusal = archived.then(|| {
            (
                Refusal::Archived,
                "this channel is archived; its records can be read, not changed".to_string(),
            )
        });
    }

    /// The next request of `op` fails with `error`.
    pub fn fail_next(&mut self, op: &str, error: RecordError) {
        self.fail_next.push((op.to_string(), error));
    }

    /// The reply to the next request of `op` is kept back until
    /// [`MemoryStore::release`]. The request itself is done at once, as a
    /// slow relay does it.
    pub fn hold_next(&mut self, op: &str) {
        self.hold_next.push(op.to_string());
    }

    /// The replies that were kept back, oldest first.
    pub fn release(&mut self) -> Vec<Told> {
        std::mem::take(&mut self.held)
    }

    /// The next update or delete of `record_id` by the app first loses to
    /// `who`, who writes `content`: the app gets that change as a push and
    /// then a conflict with the record as it is now.
    pub fn arm_conflict(&mut self, record_id: &str, who: &Who, content: Map<String, Value>) {
        self.knows(who);
        if let Ok(id) = Uuid::parse_str(record_id) {
            self.armed.push(Armed {
                id,
                who: who.id.clone(),
                content,
            });
        }
    }

    /// bzz does not know the name of `id` (yet): its records are shown with
    /// a short form of the id.
    pub fn forget_name(&mut self, id: &str) {
        self.names.remove(id);
    }

    /// bzz learns the name of `id`, or the name changed.
    pub fn set_name(&mut self, id: &str, name: &str) {
        self.names.insert(id.to_string(), name.to_string());
    }

    /// The app is opened before the relay has answered: of the records
    /// here it holds only those bzz `kept` from an earlier run, and the
    /// sync is `Loading`. [`MemoryStore::resync`] is the relay's answer.
    pub fn start_loading(&mut self, kept: impl Fn(&Record) -> bool) {
        let held = self
            .records
            .iter()
            .filter(|(_, record)| kept(&self.shown(record)))
            .map(|(id, record)| (*id, record.clone()))
            .collect();
        self.before_loss = Some(held);
        self.set_sync(SyncState::Loading);
    }

    /// Where the sync stands, as the service would say it: `Loading`,
    /// `TooMany`, `Restricted`, `Unavailable`. The session is told.
    pub fn set_sync(&mut self, state: SyncState) {
        if self.sync != state {
            self.sync = state.clone();
            self.outbox.push_back(Told::State(state));
        }
    }

    /// The connection is lost. Until [`MemoryStore::resync`] the app's
    /// writes are answered `unavailable` and it hears of no change; other
    /// participants go on writing.
    pub fn lose_connection(&mut self) {
        if self.before_loss.is_none() {
            self.before_loss = Some(self.records.clone());
        }
        self.available = false;
        self.set_sync(SyncState::Resyncing);
    }

    /// The set was fetched again. The app is told only what really differs
    /// from what it knew: nothing at all when nothing changed.
    pub fn resync(&mut self) -> Vec<Told> {
        self.available = true;
        let before = self
            .before_loss
            .take()
            .unwrap_or_else(|| self.records.clone());
        let changes = difference(&self.own(&before), &self.own(&self.records));
        self.set_sync(SyncState::Live);
        if changes.is_empty() {
            Vec::new()
        } else {
            vec![Told::Changes {
                changes,
                cause: Cause::Sync,
            }]
        }
    }

    /// The part of a set that is the open app's: its declared types.
    fn own(&self, set: &BTreeMap<Uuid, record::Record>) -> BTreeMap<Uuid, record::Record> {
        set.iter()
            .filter(|(_, record)| self.declared.contains(&record.record_type))
            .map(|(id, record)| (*id, record.clone()))
            .collect()
    }

    // ------------------------------------------------- the other participant

    fn told(&self, change: Change) -> Vec<Told> {
        if self.before_loss.is_some() {
            return Vec::new();
        }
        vec![Told::Changes {
            changes: vec![change],
            cause: Cause::Live,
        }]
    }

    /// `who` creates a record. Returns it, and what the app is told.
    pub fn remote_create(
        &mut self,
        who: &Who,
        record_type: &str,
        content: Map<String, Value>,
    ) -> (Record, Vec<Told>) {
        self.knows(who);
        let record = self.put(record_type, content, &who.id);
        (self.shown(&record), self.told(Change::Upsert(record)))
    }

    fn revise(
        &mut self,
        who: &str,
        id: Uuid,
        content: Map<String, Value>,
    ) -> Option<record::Record> {
        let revision = self.new_rev();
        let signed_at = self.clock.wall().as_secs();
        let record = self.records.get_mut(&id)?;
        record.content = content;
        record.revision = revision;
        record.author = who.to_string();
        record.signed_at = signed_at;
        Some(record.clone())
    }

    /// `who` replaces the content of a record, whatever its revision.
    pub fn remote_update(
        &mut self,
        who: &Who,
        record_id: &str,
        content: Map<String, Value>,
    ) -> Vec<Told> {
        self.knows(who);
        let Some(record) = Uuid::parse_str(record_id)
            .ok()
            .and_then(|id| self.revise(&who.id, id, content))
        else {
            return Vec::new();
        };
        self.told(Change::Upsert(record))
    }

    pub fn remote_delete(&mut self, who: &Who, record_id: &str) -> Vec<Told> {
        self.knows(who);
        let Some(record) = Uuid::parse_str(record_id)
            .ok()
            .and_then(|id| self.records.remove(&id))
        else {
            return Vec::new();
        };
        self.told(Change::Removed {
            record,
            by: Some(who.id.clone()),
        })
    }

    // ------------------------------------------------------ the app's requests

    fn failing(&mut self, op: &str) -> Option<RecordError> {
        let at = self.fail_next.iter().position(|(name, _)| name == op)?;
        Some(self.fail_next.remove(at).1)
    }

    fn holding(&mut self, op: &str) -> bool {
        match self.hold_next.iter().position(|held| held == op) {
            Some(at) => {
                self.hold_next.remove(at);
                true
            }
            None => false,
        }
    }

    fn answer(&mut self, write: &Write) -> Result<record::Record, RecordError> {
        if let Some(error) = self.failing(write.name()) {
            return Err(error);
        }
        match &self.sync {
            SyncState::Restricted => {
                return Err(RecordError::Refused {
                    refusal: Refusal::Unreadable,
                    reason: "you cannot read this channel's records".into(),
                });
            }
            SyncState::Unavailable { reason } => {
                return Err(RecordError::Unavailable {
                    reason: reason.clone(),
                });
            }
            _ => {}
        }
        if !self.available {
            return Err(RecordError::Unavailable {
                reason: "no connection to the relay".into(),
            });
        }
        // The fence, where the call is served (rule 17).
        if let Write::Create { record_type, .. } = write
            && !self.declared.contains(record_type)
        {
            return Err(RecordError::Fenced {
                reason: format!("the app did not declare the record type {record_type}"),
            });
        }
        if let Some((refusal, reason)) = &self.refusal {
            return Err(RecordError::Refused {
                refusal: *refusal,
                reason: reason.clone(),
            });
        }
        self.spend_a_write()?;
        let stored = self.store(write)?;
        // The app's side has its own write at once, as the service has,
        // also while it hears of nobody else's.
        if let Some(held) = &mut self.before_loss {
            match write {
                Write::Delete { id, .. } => {
                    held.remove(id);
                }
                _ => {
                    held.insert(stored.id, stored.clone());
                }
            }
        }
        Ok(stored)
    }

    fn store(&mut self, write: &Write) -> Result<record::Record, RecordError> {
        let by = self.local.id.clone();
        match write {
            Write::Create {
                record_type,
                content,
            } => {
                self.check_size(content)?;
                let live = self
                    .records
                    .values()
                    .filter(|record| self.declared.contains(&record.record_type))
                    .count();
                if matches!(self.sync, SyncState::TooMany { .. }) || live >= self.live_limit {
                    return Err(RecordError::Limit {
                        limit: self.live_limit,
                    });
                }
                Ok(self.put(record_type, content.clone(), &by))
            }
            Write::Update {
                id,
                revision,
                content,
            } => {
                self.check_size(content)?;
                self.lose_if_armed(*id);
                self.current(*id, revision)?;
                self.revise(&by, *id, content.clone())
                    .ok_or(RecordError::NotFound)
            }
            Write::Delete { id, revision } => {
                self.lose_if_armed(*id);
                self.current(*id, revision)?;
                self.records.remove(id).ok_or(RecordError::NotFound)
            }
        }
    }

    /// The record `id` of the open app, if `revision` is still its revision.
    fn current(&self, id: Uuid, revision: &str) -> Result<(), RecordError> {
        let record = self
            .records
            .get(&id)
            .filter(|record| self.declared.contains(&record.record_type))
            .ok_or(RecordError::NotFound)?;
        if record.revision != revision {
            return Err(RecordError::Conflict {
                current: Box::new(record.clone()),
            });
        }
        Ok(())
    }

    fn lose_if_armed(&mut self, id: Uuid) {
        if let Some(at) = self.armed.iter().position(|armed| armed.id == id) {
            let armed = self.armed.remove(at);
            if let Some(record) = self.revise(&armed.who, id, armed.content) {
                // The winner's revision is told as the change it is.
                let told = self.told(Change::Upsert(record));
                self.outbox.extend(told);
            }
        }
    }

    fn check_size(&self, content: &Map<String, Value>) -> Result<(), RecordError> {
        let bytes = content_bytes(content);
        if bytes > self.content_limit {
            return Err(RecordError::TooLarge {
                what: Oversize::Content,
                bytes,
                limit: self.content_limit,
            });
        }
        Ok(())
    }

    fn spend_a_write(&mut self) -> Result<(), RecordError> {
        let Some((allowed, window)) = self.quota else {
            return Ok(());
        };
        let now = self.clock.elapsed();
        while self
            .writes
            .front()
            .is_some_and(|t| now.saturating_sub(*t) >= window)
        {
            self.writes.pop_front();
        }
        if self.writes.len() >= allowed as usize {
            let oldest = self.writes.front().copied().unwrap_or(now);
            let wait = (oldest + window).saturating_sub(now);
            return Err(RecordError::RateLimited {
                seconds: (wait.as_millis() as u64).div_ceil(1000).max(1),
            });
        }
        self.writes.push_back(now);
        Ok(())
    }
}

/// A store an app's session and a test both hold.
#[derive(Clone)]
pub struct SharedStore(Arc<Mutex<MemoryStore>>);

impl SharedStore {
    pub fn new(store: MemoryStore) -> Self {
        Self(Arc::new(Mutex::new(store)))
    }

    pub fn lock(&self) -> MutexGuard<'_, MemoryStore> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Puts what a call on the store returned where the session finds it.
    pub fn tell(&self, told: Vec<Told>) {
        self.lock().outbox.extend(told);
    }
}

impl Records for SharedStore {
    fn identity(&self) -> String {
        self.lock().local.id.clone()
    }

    fn open(&mut self, types: &[String]) {
        let mut store = self.lock();
        store.declared = types.to_vec();
        store.outbox.push_back(Told::Opened(Ok(())));
    }

    fn state(&self) -> SyncState {
        self.lock().sync.clone()
    }

    fn records(&self) -> Vec<record::Record> {
        let store = self.lock();
        store.own(store.held_set()).into_values().collect()
    }

    fn record(&self, id: Uuid) -> Option<record::Record> {
        let store = self.lock();
        store
            .held_set()
            .get(&id)
            .filter(|record| store.declared.contains(&record.record_type))
            .cloned()
    }

    fn list(&mut self, request: u64, _record_type: &str, matching: Option<&str>) -> Listing {
        let mut store = self.lock();
        *store.counts.entry("list").or_insert(0) += 1;
        store
            .list_matches
            .push(matching.unwrap_or_default().to_string());
        if let Some(error) = store.failing("list") {
            return Listing::Fail(error);
        }
        if store.holding("list") {
            store.held.push(Told::Release { request });
            return Listing::Hold;
        }
        Listing::Serve
    }

    fn write(&mut self, request: u64, write: Write) {
        let mut store = self.lock();
        *store.counts.entry(write.name()).or_insert(0) += 1;
        let result = store.answer(&write);
        let answer = Told::Answer { request, result };
        if store.holding(write.name()) {
            store.held.push(answer);
        } else {
            store.outbox.push_back(answer);
        }
    }

    fn retry(&mut self) {
        let mut store = self.lock();
        if store.sync.is_final() {
            store.set_sync(SyncState::Live);
        }
    }

    fn poll(&mut self) -> Vec<Told> {
        self.lock().outbox.drain(..).collect()
    }
}
