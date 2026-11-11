-- v0.13.0: NIP-AR channel artifacts. One row per artifact holding only its
-- current head (as the relay last delivered it) plus the last live revision,
-- which a restore republishes over a deletion. Artifacts are state, never
-- messages: nothing here feeds unread, mention or reply counters.
CREATE TABLE artifacts(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  artifact_id TEXT NOT NULL,
  home TEXT NOT NULL,
  artifact_type TEXT NOT NULL,
  title TEXT,
  op TEXT NOT NULL CHECK(op IN ('create','update','move','delete','restore')),
  root TEXT,
  head TEXT NOT NULL CHECK(length(head)=64),
  author TEXT NOT NULL CHECK(length(author)=64),
  created_at INTEGER NOT NULL,
  event_json TEXT NOT NULL,
  live_json TEXT,
  PRIMARY KEY(community_id,artifact_id)
);
CREATE INDEX artifacts_home ON artifacts(community_id,home);
