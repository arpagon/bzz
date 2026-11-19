//! A record store in memory, behind the same boundary the relay's service
//! will stand behind ([`super::records`]). The headless driver runs an app
//! on it, and so do the tests of every platform.
//!
//! It keeps the promises of the contract and none it does not make: ids do
//! not sort by creation, a list comes in no useful order, a revision is an
//! opaque string. It can also be made to misbehave the way a relay does: a
//! second participant, a conflict, a quota, a refusal, a lost connection, a
//! reply that comes late, and a set that had to be fetched again.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::Event;
use super::clock::SharedClock;
use super::limits;
use super::records::{
    Change, Record, RecordAnswer, RecordError, RecordOp, RecordPush, RecordReply, RecordRequest,
    Who, content_bytes, differences, matches, valid_type_name,
};

/// The next update of a record first loses to another participant.
struct Armed {
    record_id: String,
    who: Who,
    content: Map<String, Value>,
}

pub struct MemoryStore {
    /// Who the app's own requests are signed by.
    local: Who,
    clock: SharedClock,
    records: Vec<Record>,
    counter: u64,
    /// Live records an app may have, and bytes of one record's content.
    pub live_limit: usize,
    pub content_limit: usize,
    /// Writes allowed in a window, when a quota is set.
    quota: Option<(u32, Duration)>,
    writes: VecDeque<Duration>,
    available: bool,
    refusal: Option<String>,
    fail_next: Vec<(String, RecordError)>,
    hold_next: Vec<String>,
    held: Vec<RecordReply>,
    armed: Vec<Armed>,
    /// The set as the app last knew it, while the connection is lost.
    before_loss: Option<Vec<Record>>,
    counts: HashMap<&'static str, usize>,
    list_matches: Vec<String>,
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
        Self {
            local,
            clock,
            records: Vec::new(),
            counter: 0,
            live_limit: limits::LIVE_RECORDS,
            content_limit: limits::CONTENT_BYTES,
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
        }
    }

    fn digest(&mut self, what: &str) -> String {
        self.counter += 1;
        let hash = Sha256::digest(format!("{what}:{}", self.counter));
        hash.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// An id as the relay gives one: nothing in it says when it was made.
    fn new_id(&mut self) -> String {
        let hex = self.digest("record");
        format!(
            "{}-{}-{}-{}-{}",
            &hex[..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        )
    }

    fn new_rev(&mut self) -> String {
        self.digest("rev")
    }

    // ------------------------------------------------------------- reading

    /// Every live record, in no order.
    pub fn records(&self) -> &[Record] {
        &self.records
    }

    /// The first record whose content has the string `value` in `field`.
    pub fn find_by(&self, field: &str, value: &str) -> Option<&Record> {
        self.records
            .iter()
            .find(|r| r.content.get(field).and_then(Value::as_str) == Some(value))
    }

    pub fn get(&self, record_id: &str) -> Option<&Record> {
        self.records.iter().find(|r| r.id == record_id)
    }

    /// How many requests of `op` (`list`, `create`, `update`, `delete`) the
    /// app has made.
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

    // --------------------------------------------------- making it misbehave

    /// A record that is simply there, written by `by`.
    pub fn seed(&mut self, record_type: &str, content: Map<String, Value>, by: &Who) -> Record {
        let record = Record {
            id: self.new_id(),
            record_type: record_type.to_string(),
            rev: self.new_rev(),
            content,
            by: by.clone(),
        };
        self.records.push(record.clone());
        record
    }

    /// `writes` writes are taken in any `window`; the next is answered
    /// `rate_limited` with the seconds to wait.
    pub fn set_quota(&mut self, writes: u32, window: Duration) {
        self.quota = Some((writes, window));
    }

    /// Without a connection every request is answered `unavailable`.
    pub fn set_available(&mut self, available: bool) {
        self.available = available;
    }

    /// With a reason, every write is answered `refused` with it (a guest,
    /// an archived channel). Reading still works.
    pub fn set_refusal(&mut self, reason: Option<&str>) {
        self.refusal = reason.map(str::to_string);
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
    pub fn release(&mut self) -> Vec<Event> {
        self.held.drain(..).map(Event::Reply).collect()
    }

    /// The next update or delete of `record_id` by the app first loses to
    /// `who`, who writes `content`: the app gets that change as a push and
    /// then a conflict with the record as it is now.
    pub fn arm_conflict(&mut self, record_id: &str, who: &Who, content: Map<String, Value>) {
        self.armed.push(Armed {
            record_id: record_id.to_string(),
            who: who.clone(),
            content,
        });
    }

    /// The connection is lost. Until [`MemoryStore::resync`] the app gets
    /// `unavailable` and no push; other participants go on writing.
    pub fn lose_connection(&mut self) {
        if self.before_loss.is_none() {
            self.before_loss = Some(self.records.clone());
        }
        self.available = false;
    }

    /// The set was fetched again. The app is told only what really differs
    /// from what it knew: nothing at all when nothing changed.
    pub fn resync(&mut self) -> Vec<Event> {
        self.available = true;
        let before = self
            .before_loss
            .take()
            .unwrap_or_else(|| self.records.clone());
        let mut after = self.records.clone();
        after.sort_by(|a, b| a.id.cmp(&b.id));
        differences(&before, &after)
            .into_iter()
            .map(Event::Push)
            .collect()
    }

    // ------------------------------------------------- the other participant

    fn push(&self, change: Change, record: &Record, by: &Who) -> Vec<Event> {
        if self.before_loss.is_some() {
            return Vec::new();
        }
        vec![Event::Push(RecordPush {
            change,
            record: record.clone(),
            by: by.clone(),
        })]
    }

    /// `who` creates a record. Returns it, and what the app is told.
    pub fn remote_create(
        &mut self,
        who: &Who,
        record_type: &str,
        content: Map<String, Value>,
    ) -> (Record, Vec<Event>) {
        let record = self.seed(record_type, content, who);
        let events = self.push(Change::Create, &record, who);
        (record, events)
    }

    /// `who` replaces the content of a record, whatever its revision.
    pub fn remote_update(
        &mut self,
        who: &Who,
        record_id: &str,
        content: Map<String, Value>,
    ) -> Vec<Event> {
        let rev = self.new_rev();
        let Some(record) = self.records.iter_mut().find(|r| r.id == record_id) else {
            return Vec::new();
        };
        record.content = content;
        record.rev = rev;
        record.by = who.clone();
        let record = record.clone();
        self.push(Change::Update, &record, who)
    }

    pub fn remote_delete(&mut self, who: &Who, record_id: &str) -> Vec<Event> {
        let Some(at) = self.records.iter().position(|r| r.id == record_id) else {
            return Vec::new();
        };
        let record = self.records.remove(at);
        self.push(Change::Delete, &record, who)
    }

    // ------------------------------------------------------ the app's requests

    /// Serves one request of the app. Returns what the app is to be sent
    /// now: its reply, unless that is held back, and before it a push when
    /// a conflict was armed.
    pub fn serve(&mut self, request: RecordRequest) -> Vec<Event> {
        let op = request.op.name();
        *self.counts.entry(op).or_insert(0) += 1;
        if let RecordOp::List { matching, .. } = &request.op {
            self.list_matches.push(matching.clone().unwrap_or_default());
        }
        let mut events = Vec::new();
        let result = self.answer(&request.op, &mut events);
        let reply = RecordReply {
            id: request.id,
            result,
        };
        match self.hold_next.iter().position(|held| held == op) {
            Some(at) => {
                self.hold_next.remove(at);
                self.held.push(reply);
            }
            None => events.push(Event::Reply(reply)),
        }
        events
    }

    fn answer(
        &mut self,
        op: &RecordOp,
        events: &mut Vec<Event>,
    ) -> Result<RecordAnswer, RecordError> {
        if let Some(at) = self
            .fail_next
            .iter()
            .position(|(name, _)| name == op.name())
        {
            return Err(self.fail_next.remove(at).1);
        }
        if !self.available {
            return Err(RecordError::Unavailable {
                message: "no connection to the relay".into(),
            });
        }
        if !valid_type_name(op.record_type()) {
            return Err(RecordError::BadRequest {
                message: format!("\"{}\" is not a record type's name", op.record_type()),
            });
        }
        if op.is_write() {
            if let Some(reason) = &self.refusal {
                return Err(RecordError::Refused {
                    reason: reason.clone(),
                });
            }
            self.spend_a_write()?;
        }
        match op {
            RecordOp::List {
                record_type,
                matching,
            } => {
                let mut found: Vec<Record> = self
                    .records
                    .iter()
                    .filter(|r| &r.record_type == record_type && matches(r, matching.as_deref()))
                    .cloned()
                    .collect();
                // By id: an order that says nothing about creation.
                found.sort_by(|a, b| a.id.cmp(&b.id));
                Ok(RecordAnswer::Records(found))
            }
            RecordOp::Create {
                record_type,
                content,
            } => {
                self.check_size(content)?;
                if self.records.len() >= self.live_limit {
                    return Err(RecordError::Limit {
                        message: format!(
                            "the app has {} records, the most it may have",
                            self.live_limit
                        ),
                    });
                }
                let by = self.local.clone();
                Ok(RecordAnswer::Record(self.seed(
                    record_type,
                    content.clone(),
                    &by,
                )))
            }
            RecordOp::Update {
                record_id,
                rev,
                content,
                ..
            } => {
                self.check_size(content)?;
                self.lose_if_armed(record_id, events);
                let new_rev = self.new_rev();
                let by = self.local.clone();
                let record = self.current(record_id, rev)?;
                record.content = content.clone();
                record.rev = new_rev;
                record.by = by;
                Ok(RecordAnswer::Record(record.clone()))
            }
            RecordOp::Delete { record_id, rev, .. } => {
                self.lose_if_armed(record_id, events);
                self.current(record_id, rev)?;
                let at = self
                    .records
                    .iter()
                    .position(|r| &r.id == record_id)
                    .ok_or(RecordError::NotFound)?;
                Ok(RecordAnswer::Record(self.records.remove(at)))
            }
        }
    }

    /// The record `record_id`, if `rev` is still its revision.
    fn current(&mut self, record_id: &str, rev: &str) -> Result<&mut Record, RecordError> {
        let record = self
            .records
            .iter_mut()
            .find(|r| r.id == record_id)
            .ok_or(RecordError::NotFound)?;
        if record.rev != rev {
            return Err(RecordError::Conflict {
                current: Box::new(record.clone()),
            });
        }
        Ok(record)
    }

    fn lose_if_armed(&mut self, record_id: &str, events: &mut Vec<Event>) {
        if let Some(at) = self.armed.iter().position(|a| a.record_id == record_id) {
            let armed = self.armed.remove(at);
            events.extend(self.remote_update(&armed.who, record_id, armed.content));
        }
    }

    fn check_size(&self, content: &Map<String, Value>) -> Result<(), RecordError> {
        let bytes = content_bytes(content);
        if bytes > self.content_limit {
            return Err(RecordError::TooLarge {
                message: format!(
                    "the content is {bytes} bytes as JSON; a record may hold {}",
                    self.content_limit
                ),
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
                retry_after_secs: (wait.as_millis() as u64).div_ceil(1000).max(1),
            });
        }
        self.writes.push_back(now);
        Ok(())
    }
}
