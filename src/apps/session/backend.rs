//! What a session needs of the records behind it: the relay's service
//! ([`super::relay`]) or the store in memory ([`super::memory`]). Both speak
//! the service's own types (`protocol::record`, `service::records`), so the
//! session has one set of rules whichever stands behind it.
//!
//! Nothing here blocks. A call returns at once; what comes of it is told
//! later, through [`Records::poll`], and the session's owner is woken.

use serde_json::{Map, Value};
use uuid::Uuid;

use crate::protocol::record::{Change, Record, RecordError};
use crate::service::records::{AppRecords, Cause, SyncState};

/// A write an app asked for, after the session has checked it.
#[derive(Clone, Debug, PartialEq)]
pub enum Write {
    Create {
        record_type: String,
        content: Map<String, Value>,
    },
    /// Replaces the whole content of the record at `revision`.
    Update {
        id: Uuid,
        revision: String,
        content: Map<String, Value>,
    },
    Delete {
        id: Uuid,
        revision: String,
    },
}

impl Write {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Create { .. } => "create",
            Self::Update { .. } => "update",
            Self::Delete { .. } => "delete",
        }
    }
}

/// What the records tell the session.
#[derive(Clone, Debug, PartialEq)]
pub enum Told {
    /// The records were opened for the types last asked for, or could not
    /// be. From here on [`Records::records`] is the set.
    Opened(Result<(), RecordError>),
    /// Where the set stands.
    State(SyncState),
    /// Changes by other participants. The session's own writes are answered
    /// and never repeated here.
    Changes { changes: Vec<Change>, cause: Cause },
    /// The one answer to a write: the record as stored, or for a delete its
    /// last state.
    Answer {
        request: u64,
        result: Result<Record, RecordError>,
    },
    /// The reply to a list that was kept back may go now.
    Release { request: u64 },
}

/// What the records say of a list before the session serves it.
#[derive(Clone, Debug, PartialEq)]
pub enum Listing {
    /// The session answers it from its set.
    Serve,
    /// It fails with this.
    Fail(RecordError),
    /// Its reply is made now and kept back until [`Told::Release`].
    Hold,
}

/// The records of one app in one channel, for one session.
pub trait Records: Send {
    /// The identity the owner's writes are signed with (public key, hex).
    fn identity(&self) -> String;

    /// Opens the records for these short type names, or opens them again
    /// for others: a new version of a script may declare other types. The
    /// result is told as [`Told::Opened`].
    fn open(&mut self, types: &[String]);

    /// Where the set stands. `Loading` until the records are open.
    fn state(&self) -> SyncState;

    /// The set as the records hold it now. It can be ahead of what was
    /// told: a change is in the set before the session hears of it.
    fn records(&self) -> Vec<Record>;

    fn record(&self, id: Uuid) -> Option<Record>;

    /// A list is served by the session, from its own set. The records are
    /// asked first: the store in memory counts lists, and can be made to
    /// fail one or to keep its reply back, as a slow relay would.
    fn list(&mut self, _request: u64, _record_type: &str, _matching: Option<&str>) -> Listing {
        Listing::Serve
    }

    /// Sends a write. Exactly one [`Told::Answer`] follows for `request`.
    fn write(&mut self, request: u64, write: Write);

    /// Asks for the set again after a final state.
    fn retry(&mut self);

    /// The app was closed: the records are let go of, and their sync ends.
    fn close(&mut self) {}

    /// What happened since the last time. Never blocks.
    fn poll(&mut self) -> Vec<Told>;

    /// The relay's records, for a writer other than the owner (an agent's
    /// tools, M7). None in memory, and until the records are open.
    fn relay(&self) -> Option<AppRecords> {
        None
    }
}
