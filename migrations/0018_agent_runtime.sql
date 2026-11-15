-- v0.15 M1: the local agent runtime (ADR v0.15 local agents).
--
-- agent_inbox: one row per request (a mention, or a message in the owner's
-- DM) admitted for a local agent, with where its turn stands.
-- agent_replies: the signed final reply of a request, stored before it is
-- published. One per request: that is the "one final reply per mention" rule.
-- agent_sessions: the ACP session id a harness gave for a place.
-- agent_cursors: the newest request time each agent's relay intake has seen.
CREATE TABLE agent_inbox (
    agent_id TEXT NOT NULL,
    request_id TEXT NOT NULL,
    community_id TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    place TEXT NOT NULL,
    author TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    event_json TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('queued', 'delivered', 'replied', 'ended', 'failed')),
    -- How many times the request was handed to a harness.
    attempts INTEGER NOT NULL DEFAULT 0,
    -- Things published for this request other than its final reply. Counted
    -- before each publish, so a crash can only overcount.
    effects INTEGER NOT NULL DEFAULT 0,
    failure TEXT,
    received_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (agent_id, request_id)
);

CREATE INDEX agent_inbox_open ON agent_inbox (agent_id, state, created_at, request_id);

CREATE TABLE agent_replies (
    agent_id TEXT NOT NULL,
    request_id TEXT NOT NULL,
    reply_event_id TEXT NOT NULL,
    reply_event_json TEXT NOT NULL,
    origin TEXT NOT NULL CHECK (origin IN ('send', 'text', 'failure')),
    stored_at INTEGER NOT NULL,
    -- NULL until the relay acknowledged the event.
    published_at INTEGER,
    PRIMARY KEY (agent_id, request_id),
    FOREIGN KEY (agent_id, request_id) REFERENCES agent_inbox (agent_id, request_id) ON DELETE CASCADE
);

CREATE INDEX agent_replies_unpublished ON agent_replies (agent_id) WHERE published_at IS NULL;

CREATE TABLE agent_sessions (
    agent_id TEXT NOT NULL,
    place TEXT NOT NULL,
    session_id TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (agent_id, place)
);

CREATE TABLE agent_cursors (
    agent_id TEXT PRIMARY KEY,
    seen_created_at INTEGER NOT NULL
);
