//! The local agent runtime's inbox and reply ledger (migration 18, ADR v0.15).
//!
//! A request is the event a turn answers: a mention, or a message in the
//! owner's DM. Its final reply is signed first, stored here under the request,
//! and only then published; the stored event is what is published again after
//! a crash and what a later final `send` of the same request gets back.

use rusqlite::{OptionalExtension as _, params};
use uuid::Uuid;

use crate::{
    error::{Error, Result},
    store::{Store, events::u64_to_i64},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestState {
    /// Admitted, not yet handed to a harness (or handed back after a crash).
    Queued,
    /// Handed to a harness; its turn may be running.
    Delivered,
    /// A final reply is stored.
    Replied,
    /// The turn ended with something said but no final reply.
    Ended,
    /// The turn failed; the failure reply, if any, is the stored reply.
    Failed,
}

impl RequestState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Delivered => "delivered",
            Self::Replied => "replied",
            Self::Ended => "ended",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "queued" => Self::Queued,
            "delivered" => Self::Delivered,
            "replied" => Self::Replied,
            "ended" => Self::Ended,
            "failed" => Self::Failed,
            _ => return Err(Error::Database(rusqlite::Error::InvalidQuery)),
        })
    }
}

/// Where a stored reply came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplyOrigin {
    /// A final `send` by the harness.
    Send,
    /// The harness's last plain text, when the turn ended without a `send`.
    Text,
    /// bzz's own notice that the turn failed.
    Failure,
}

impl ReplyOrigin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Send => "send",
            Self::Text => "text",
            Self::Failure => "failure",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "send" => Self::Send,
            "text" => Self::Text,
            "failure" => Self::Failure,
            _ => return Err(Error::Database(rusqlite::Error::InvalidQuery)),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewRequest {
    pub agent_id: Uuid,
    pub request_id: String,
    pub community_id: Uuid,
    pub channel_id: Uuid,
    pub place: String,
    pub author: String,
    pub created_at: u64,
    pub event_json: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboxRow {
    pub agent_id: Uuid,
    pub request_id: String,
    pub community_id: Uuid,
    pub channel_id: Uuid,
    pub place: String,
    pub author: String,
    pub created_at: u64,
    pub event_json: String,
    pub state: RequestState,
    pub attempts: u32,
    pub effects: u32,
    pub failure: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredReply {
    pub request_id: String,
    pub event_id: String,
    pub event_json: String,
    pub origin: ReplyOrigin,
    pub published: bool,
}

/// The outcome of claiming a request's one final reply.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Claim {
    /// The reply the ledger holds: the offered one, or an earlier one.
    pub reply: StoredReply,
    /// Whether the offered event is the one stored.
    pub fresh: bool,
}

const INBOX_COLUMNS: &str = "agent_id,request_id,community_id,channel_id,place,author,created_at,event_json,state,attempts,effects,failure";

fn inbox_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<InboxRow> {
    let uuid = |index: usize| -> rusqlite::Result<Uuid> {
        Uuid::parse_str(&row.get::<_, String>(index)?).map_err(|_| rusqlite::Error::InvalidQuery)
    };
    Ok(InboxRow {
        agent_id: uuid(0)?,
        request_id: row.get(1)?,
        community_id: uuid(2)?,
        channel_id: uuid(3)?,
        place: row.get(4)?,
        author: row.get(5)?,
        created_at: u64::try_from(row.get::<_, i64>(6)?).unwrap_or_default(),
        event_json: row.get(7)?,
        state: RequestState::parse(&row.get::<_, String>(8)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        attempts: row.get(9)?,
        effects: row.get(10)?,
        failure: row.get(11)?,
    })
}

fn reply_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredReply> {
    Ok(StoredReply {
        request_id: row.get(0)?,
        event_id: row.get(1)?,
        event_json: row.get(2)?,
        origin: ReplyOrigin::parse(&row.get::<_, String>(3)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        published: row.get::<_, Option<i64>>(4)?.is_some(),
    })
}

impl Store {
    /// Admits a request once. `false`: the agent already has it.
    pub fn agent_inbox_offer(&mut self, request: &NewRequest) -> Result<bool> {
        let inserted = self.connection.execute(
            "INSERT OR IGNORE INTO agent_inbox(agent_id,request_id,community_id,channel_id,place,author,created_at,event_json,state,received_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'queued',unixepoch(),unixepoch())",
            params![
                request.agent_id.to_string(),
                request.request_id,
                request.community_id.to_string(),
                request.channel_id.to_string(),
                request.place,
                request.author,
                u64_to_i64(request.created_at)?,
                request.event_json,
            ],
        )?;
        Ok(inserted == 1)
    }

    pub fn agent_inbox_get(&self, agent: Uuid, request: &str) -> Result<Option<InboxRow>> {
        Ok(self
            .connection
            .query_row(
                &format!(
                    "SELECT {INBOX_COLUMNS} FROM agent_inbox WHERE agent_id=?1 AND request_id=?2"
                ),
                params![agent.to_string(), request],
                inbox_row,
            )
            .optional()?)
    }

    /// Requests of one agent in `state`, oldest first.
    pub fn agent_inbox_in_state(&self, agent: Uuid, state: RequestState) -> Result<Vec<InboxRow>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {INBOX_COLUMNS} FROM agent_inbox WHERE agent_id=?1 AND state=?2 ORDER BY created_at,request_id LIMIT 1000"
        ))?;
        Ok(statement
            .query_map(params![agent.to_string(), state.as_str()], inbox_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Marks a queued request as handed to a harness and counts the attempt.
    /// `false`: it was not queued, so nothing may be handed over.
    pub fn agent_inbox_delivered(&mut self, agent: Uuid, request: &str) -> Result<bool> {
        Ok(self.connection.execute(
            "UPDATE agent_inbox SET state='delivered',attempts=attempts+1,updated_at=unixepoch() WHERE agent_id=?1 AND request_id=?2 AND state='queued'",
            params![agent.to_string(), request],
        )? == 1)
    }

    /// Hands a delivered request back to the queue (its harness died).
    pub fn agent_inbox_requeue(&mut self, agent: Uuid, request: &str) -> Result<bool> {
        Ok(self.connection.execute(
            "UPDATE agent_inbox SET state='queued',updated_at=unixepoch() WHERE agent_id=?1 AND request_id=?2 AND state='delivered'",
            params![agent.to_string(), request],
        )? == 1)
    }

    /// Ends an open request without a stored reply. A request that already
    /// has its reply keeps its state.
    pub fn agent_inbox_settle(
        &mut self,
        agent: Uuid,
        request: &str,
        state: RequestState,
        failure: Option<&str>,
    ) -> Result<bool> {
        Ok(self.connection.execute(
            "UPDATE agent_inbox SET state=?3,failure=?4,updated_at=unixepoch() WHERE agent_id=?1 AND request_id=?2 AND state IN ('queued','delivered')",
            params![agent.to_string(), request, state.as_str(), failure],
        )? == 1)
    }

    /// Counts one thing about to be published for a request other than its
    /// final reply. Called before the publish, so it can only overcount.
    pub fn agent_inbox_effect(&mut self, agent: Uuid, request: &str) -> Result<()> {
        self.connection.execute(
            "UPDATE agent_inbox SET effects=effects+1,updated_at=unixepoch() WHERE agent_id=?1 AND request_id=?2",
            params![agent.to_string(), request],
        )?;
        Ok(())
    }

    /// `(queued, delivered)` for one agent.
    pub fn agent_inbox_counts(&self, agent: Uuid) -> Result<(u32, u32)> {
        Ok(self.connection.query_row(
            "SELECT COALESCE(SUM(state='queued'),0),COALESCE(SUM(state='delivered'),0) FROM agent_inbox WHERE agent_id=?1",
            [agent.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?)
    }

    /// Stores `event` as the request's one final reply unless it already has
    /// one, in a single transaction, and returns what the ledger holds. The
    /// caller publishes only a fresh claim.
    pub fn agent_reply_claim(
        &mut self,
        agent: Uuid,
        request: &str,
        event_id: &str,
        event_json: &str,
        origin: ReplyOrigin,
        failure: Option<&str>,
    ) -> Result<Claim> {
        let transaction = self.connection.transaction()?;
        let fresh = transaction.execute(
            "INSERT OR IGNORE INTO agent_replies(agent_id,request_id,reply_event_id,reply_event_json,origin,stored_at) VALUES(?1,?2,?3,?4,?5,unixepoch())",
            params![agent.to_string(), request, event_id, event_json, origin.as_str()],
        )? == 1;
        if fresh {
            let state = if origin == ReplyOrigin::Failure {
                RequestState::Failed
            } else {
                RequestState::Replied
            };
            transaction.execute(
                "UPDATE agent_inbox SET state=?3,failure=?4,updated_at=unixepoch() WHERE agent_id=?1 AND request_id=?2",
                params![agent.to_string(), request, state.as_str(), failure],
            )?;
        }
        let reply = transaction.query_row(
            "SELECT request_id,reply_event_id,reply_event_json,origin,published_at FROM agent_replies WHERE agent_id=?1 AND request_id=?2",
            params![agent.to_string(), request],
            reply_row,
        )?;
        transaction.commit()?;
        Ok(Claim { reply, fresh })
    }

    pub fn agent_reply_get(&self, agent: Uuid, request: &str) -> Result<Option<StoredReply>> {
        Ok(self
            .connection
            .query_row(
                "SELECT request_id,reply_event_id,reply_event_json,origin,published_at FROM agent_replies WHERE agent_id=?1 AND request_id=?2",
                params![agent.to_string(), request],
                reply_row,
            )
            .optional()?)
    }

    pub fn agent_reply_published(&mut self, agent: Uuid, request: &str) -> Result<()> {
        self.connection.execute(
            "UPDATE agent_replies SET published_at=unixepoch() WHERE agent_id=?1 AND request_id=?2 AND published_at IS NULL",
            params![agent.to_string(), request],
        )?;
        Ok(())
    }

    /// Removes a stored reply the relay refused outright, so nothing of it
    /// exists anywhere, and reopens the request. A published reply stays.
    pub fn agent_reply_discard(
        &mut self,
        agent: Uuid,
        request: &str,
        event_id: &str,
    ) -> Result<bool> {
        let transaction = self.connection.transaction()?;
        let removed = transaction.execute(
            "DELETE FROM agent_replies WHERE agent_id=?1 AND request_id=?2 AND reply_event_id=?3 AND published_at IS NULL",
            params![agent.to_string(), request, event_id],
        )? == 1;
        if removed {
            transaction.execute(
                "UPDATE agent_inbox SET state='delivered',failure=NULL,updated_at=unixepoch() WHERE agent_id=?1 AND request_id=?2",
                params![agent.to_string(), request],
            )?;
        }
        transaction.commit()?;
        Ok(removed)
    }

    /// Stored replies the relay has not acknowledged, oldest first. With
    /// `older_than_seconds`, only those stored at least that long ago: a
    /// younger one may still be waiting for the relay's answer.
    pub fn agent_replies_unpublished(
        &self,
        agent: Uuid,
        older_than_seconds: u64,
    ) -> Result<Vec<StoredReply>> {
        let mut statement = self.connection.prepare(
            "SELECT request_id,reply_event_id,reply_event_json,origin,published_at FROM agent_replies WHERE agent_id=?1 AND published_at IS NULL AND stored_at<=unixepoch()-?2 ORDER BY stored_at,request_id LIMIT 1000",
        )?;
        Ok(statement
            .query_map(
                params![agent.to_string(), u64_to_i64(older_than_seconds)?],
                reply_row,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// The session id a harness gave for a place.
    pub fn agent_session(&self, agent: Uuid, place: &str) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row(
                "SELECT session_id FROM agent_sessions WHERE agent_id=?1 AND place=?2",
                params![agent.to_string(), place],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn set_agent_session(&mut self, agent: Uuid, place: &str, session: &str) -> Result<()> {
        self.connection.execute(
            "INSERT INTO agent_sessions(agent_id,place,session_id,updated_at) VALUES(?1,?2,?3,unixepoch())
             ON CONFLICT(agent_id,place) DO UPDATE SET session_id=excluded.session_id,updated_at=excluded.updated_at",
            params![agent.to_string(), place, session],
        )?;
        Ok(())
    }

    /// The newest request time the agent's relay intake has seen.
    pub fn agent_cursor(&self, agent: Uuid) -> Result<Option<u64>> {
        Ok(self
            .connection
            .query_row(
                "SELECT seen_created_at FROM agent_cursors WHERE agent_id=?1",
                [agent.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .map(|value| u64::try_from(value).unwrap_or_default()))
    }

    pub fn advance_agent_cursor(&mut self, agent: Uuid, seen: u64) -> Result<()> {
        self.connection.execute(
            "INSERT INTO agent_cursors(agent_id,seen_created_at) VALUES(?1,?2)
             ON CONFLICT(agent_id) DO UPDATE SET seen_created_at=MAX(seen_created_at,excluded.seen_created_at)",
            params![agent.to_string(), u64_to_i64(seen)?],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(agent: Uuid, id: &str, at: u64) -> NewRequest {
        NewRequest {
            agent_id: agent,
            request_id: id.to_owned(),
            community_id: Uuid::nil(),
            channel_id: Uuid::nil(),
            place: Uuid::nil().to_string(),
            author: "a".repeat(64),
            created_at: at,
            event_json: "{}".into(),
        }
    }

    #[test]
    fn a_request_is_admitted_once_and_handed_over_once() {
        let mut store = Store::open_memory().unwrap();
        let agent = Uuid::new_v4();
        assert!(store.agent_inbox_offer(&request(agent, "m2", 20)).unwrap());
        assert!(store.agent_inbox_offer(&request(agent, "m1", 10)).unwrap());
        assert!(
            !store.agent_inbox_offer(&request(agent, "m1", 10)).unwrap(),
            "the same request offered again is not a second request"
        );
        // Another agent mentioned by the same event has its own row.
        assert!(
            store
                .agent_inbox_offer(&request(Uuid::new_v4(), "m1", 10))
                .unwrap()
        );
        let queued = store
            .agent_inbox_in_state(agent, RequestState::Queued)
            .unwrap();
        assert_eq!(
            queued
                .iter()
                .map(|row| row.request_id.as_str())
                .collect::<Vec<_>>(),
            ["m1", "m2"],
            "oldest first"
        );
        assert!(store.agent_inbox_delivered(agent, "m1").unwrap());
        assert!(
            !store.agent_inbox_delivered(agent, "m1").unwrap(),
            "a delivered request cannot be handed over again"
        );
        assert_eq!(store.agent_inbox_counts(agent).unwrap(), (1, 1));
        assert!(store.agent_inbox_requeue(agent, "m1").unwrap());
        assert!(store.agent_inbox_delivered(agent, "m1").unwrap());
        let row = store.agent_inbox_get(agent, "m1").unwrap().unwrap();
        assert_eq!(
            (row.state, row.attempts, row.effects),
            (RequestState::Delivered, 2, 0)
        );
        store.agent_inbox_effect(agent, "m1").unwrap();
        assert_eq!(
            store.agent_inbox_get(agent, "m1").unwrap().unwrap().effects,
            1
        );
    }

    #[test]
    fn the_first_final_reply_wins_and_later_ones_get_it_back() {
        let mut store = Store::open_memory().unwrap();
        let agent = Uuid::new_v4();
        store.agent_inbox_offer(&request(agent, "m1", 10)).unwrap();
        store.agent_inbox_delivered(agent, "m1").unwrap();
        let first = store
            .agent_reply_claim(
                agent,
                "m1",
                "r1",
                "{\"id\":\"r1\"}",
                ReplyOrigin::Send,
                None,
            )
            .unwrap();
        assert!(first.fresh && !first.reply.published);
        assert_eq!(
            store.agent_inbox_get(agent, "m1").unwrap().unwrap().state,
            RequestState::Replied
        );
        // A second final reply, worded differently: the stored one comes back.
        let second = store
            .agent_reply_claim(
                agent,
                "m1",
                "r2",
                "{\"id\":\"r2\"}",
                ReplyOrigin::Send,
                None,
            )
            .unwrap();
        assert!(!second.fresh);
        assert_eq!(second.reply.event_id, "r1");
        // Nor does the plain text or a failure notice replace it.
        for origin in [ReplyOrigin::Text, ReplyOrigin::Failure] {
            let later = store
                .agent_reply_claim(agent, "m1", "r3", "{}", origin, Some("timeout"))
                .unwrap();
            assert!(!later.fresh);
            assert_eq!(
                (later.reply.event_id.as_str(), later.reply.origin),
                ("r1", ReplyOrigin::Send)
            );
        }
        assert_eq!(
            store.agent_inbox_get(agent, "m1").unwrap().unwrap().state,
            RequestState::Replied,
            "a late failure does not turn an answered request into a failed one"
        );
        // Stored but unacknowledged replies are what recovery publishes again.
        assert_eq!(store.agent_replies_unpublished(agent, 0).unwrap().len(), 1);
        assert!(
            store
                .agent_replies_unpublished(agent, 30)
                .unwrap()
                .is_empty(),
            "a reply just stored may still be on its way"
        );
        store.agent_reply_published(agent, "m1").unwrap();
        assert!(
            store
                .agent_replies_unpublished(agent, 0)
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .agent_reply_get(agent, "m1")
                .unwrap()
                .unwrap()
                .published
        );
        // A settled request is not reopened by a late settle.
        assert!(
            !store
                .agent_inbox_settle(agent, "m1", RequestState::Failed, Some("crashed"))
                .unwrap()
        );
    }

    #[test]
    fn a_refused_reply_is_discarded_only_while_unpublished() {
        let mut store = Store::open_memory().unwrap();
        let agent = Uuid::new_v4();
        store.agent_inbox_offer(&request(agent, "m1", 10)).unwrap();
        store.agent_inbox_delivered(agent, "m1").unwrap();
        store
            .agent_reply_claim(agent, "m1", "r1", "{}", ReplyOrigin::Send, None)
            .unwrap();
        assert!(!store.agent_reply_discard(agent, "m1", "other").unwrap());
        assert!(store.agent_reply_discard(agent, "m1", "r1").unwrap());
        assert_eq!(
            store.agent_inbox_get(agent, "m1").unwrap().unwrap().state,
            RequestState::Delivered,
            "the request is open again"
        );
        let again = store
            .agent_reply_claim(agent, "m1", "r2", "{}", ReplyOrigin::Send, None)
            .unwrap();
        assert!(again.fresh);
        store.agent_reply_published(agent, "m1").unwrap();
        assert!(!store.agent_reply_discard(agent, "m1", "r2").unwrap());
        assert!(store.agent_reply_get(agent, "m1").unwrap().is_some());
    }

    #[test]
    fn a_failure_reply_marks_the_request_failed_with_its_kind() {
        let mut store = Store::open_memory().unwrap();
        let agent = Uuid::new_v4();
        store.agent_inbox_offer(&request(agent, "m1", 10)).unwrap();
        let claim = store
            .agent_reply_claim(
                agent,
                "m1",
                "f1",
                "{}",
                ReplyOrigin::Failure,
                Some("limit-reached"),
            )
            .unwrap();
        assert!(claim.fresh);
        let row = store.agent_inbox_get(agent, "m1").unwrap().unwrap();
        assert_eq!(
            (row.state, row.failure.as_deref()),
            (RequestState::Failed, Some("limit-reached"))
        );
    }

    #[test]
    fn sessions_and_the_cursor_are_kept_per_agent() {
        let mut store = Store::open_memory().unwrap();
        let (agent, other) = (Uuid::new_v4(), Uuid::new_v4());
        assert_eq!(store.agent_session(agent, "place").unwrap(), None);
        store.set_agent_session(agent, "place", "s1").unwrap();
        store.set_agent_session(agent, "place", "s2").unwrap();
        assert_eq!(
            store.agent_session(agent, "place").unwrap().as_deref(),
            Some("s2")
        );
        assert_eq!(store.agent_session(other, "place").unwrap(), None);
        assert_eq!(store.agent_cursor(agent).unwrap(), None);
        store.advance_agent_cursor(agent, 50).unwrap();
        store.advance_agent_cursor(agent, 40).unwrap();
        assert_eq!(
            store.agent_cursor(agent).unwrap(),
            Some(50),
            "never moves back"
        );
    }
}
