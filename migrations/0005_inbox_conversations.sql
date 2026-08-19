-- Rebuildable local Inbox projection. Events, drafts, membership, DM visibility,
-- read contexts, outbox state, and overrides remain the source of truth.
CREATE TABLE inbox_projection_meta(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  identity_pubkey TEXT NOT NULL CHECK(length(identity_pubkey)=64),
  dirty INTEGER NOT NULL DEFAULT 1 CHECK(dirty IN (0,1)),
  rebuilt_at INTEGER,
  PRIMARY KEY(community_id,identity_pubkey)
);

CREATE TABLE inbox_conversations(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  identity_pubkey TEXT NOT NULL CHECK(length(identity_pubkey)=64),
  conversation_id TEXT NOT NULL CHECK(length(CAST(conversation_id AS BLOB)) <= 256),
  latest_event_id TEXT CHECK(latest_event_id IS NULL OR length(latest_event_id)=64),
  latest_activity_at INTEGER NOT NULL CHECK(latest_activity_at >= 0),
  first_unread_event_id TEXT CHECK(first_unread_event_id IS NULL OR length(first_unread_event_id)=64),
  first_unread_at INTEGER,
  unread_count INTEGER NOT NULL DEFAULT 0 CHECK(unread_count >= 0),
  categories_json TEXT NOT NULL CHECK(length(CAST(categories_json AS BLOB)) <= 512),
  channel_id TEXT,
  thread_root_id TEXT CHECK(thread_root_id IS NULL OR length(thread_root_id)=64),
  sender_pubkey TEXT CHECK(sender_pubkey IS NULL OR length(sender_pubkey)=64),
  preview TEXT NOT NULL CHECK(length(CAST(preview AS BLOB)) <= 1024),
  draft_count INTEGER NOT NULL DEFAULT 0 CHECK(draft_count >= 0),
  latest_draft_at INTEGER,
  forced_unread INTEGER NOT NULL DEFAULT 0 CHECK(forced_unread IN (0,1)),
  local_done_at INTEGER,
  PRIMARY KEY(community_id,identity_pubkey,conversation_id)
);
CREATE INDEX events_inbox_participation
  ON events(community_id,pubkey,root_event_id,created_at DESC,event_id);

CREATE INDEX inbox_conversations_page
  ON inbox_conversations(community_id,identity_pubkey,latest_activity_at DESC,conversation_id ASC);
CREATE INDEX inbox_conversations_channel
  ON inbox_conversations(community_id,identity_pubkey,channel_id,thread_root_id);

-- This is a bounded local window for Inbox detail/reconciliation. It is a
-- projection index, not a second message store: event bodies remain only in
-- `events` and each row points to an existing local event.
CREATE TABLE inbox_conversation_events(
  community_id TEXT NOT NULL,
  identity_pubkey TEXT NOT NULL,
  conversation_id TEXT NOT NULL,
  event_id TEXT NOT NULL CHECK(length(event_id)=64),
  created_at INTEGER NOT NULL CHECK(created_at >= 0),
  PRIMARY KEY(community_id,identity_pubkey,conversation_id,event_id),
  FOREIGN KEY(community_id,identity_pubkey,conversation_id)
    REFERENCES inbox_conversations(community_id,identity_pubkey,conversation_id)
    ON DELETE CASCADE,
  FOREIGN KEY(community_id,event_id)
    REFERENCES events(community_id,event_id) ON DELETE CASCADE
);
CREATE INDEX inbox_conversation_events_window
  ON inbox_conversation_events(community_id,identity_pubkey,conversation_id,created_at DESC,event_id ASC);
