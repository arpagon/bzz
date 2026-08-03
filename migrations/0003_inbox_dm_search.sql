CREATE INDEX events_inbox_order
  ON events(community_id,created_at DESC,event_id);
CREATE INDEX events_inbox_author_order
  ON events(community_id,pubkey,created_at DESC,event_id);

CREATE TABLE event_mentions(
  community_id TEXT NOT NULL,
  event_id TEXT NOT NULL,
  mentioned_pubkey TEXT NOT NULL CHECK(length(mentioned_pubkey)=64),
  created_at INTEGER NOT NULL CHECK(created_at >= 0),
  PRIMARY KEY(community_id,event_id,mentioned_pubkey),
  FOREIGN KEY(community_id,event_id) REFERENCES events(community_id,event_id) ON DELETE CASCADE
);
CREATE INDEX event_mentions_inbox
  ON event_mentions(community_id,mentioned_pubkey,created_at DESC,event_id);

CREATE TABLE channel_membership_heads(
  community_id TEXT NOT NULL,
  channel_id TEXT NOT NULL,
  source_event_id TEXT NOT NULL CHECK(length(source_event_id)=64),
  source_created_at INTEGER NOT NULL CHECK(source_created_at >= 0),
  PRIMARY KEY(community_id,channel_id),
  FOREIGN KEY(community_id,channel_id)
    REFERENCES channels(community_id,channel_id) ON DELETE CASCADE
);
INSERT INTO channel_membership_heads(community_id,channel_id,source_event_id,source_created_at)
SELECT m.community_id,m.channel_id,min(m.source_event_id),max(e.created_at)
FROM memberships m
JOIN events e ON e.community_id=m.community_id AND e.event_id=m.source_event_id
GROUP BY m.community_id,m.channel_id;

CREATE TABLE dm_visibility_heads(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  identity_pubkey TEXT NOT NULL CHECK(length(identity_pubkey)=64),
  source_event_id TEXT NOT NULL CHECK(length(source_event_id)=64),
  source_created_at INTEGER NOT NULL CHECK(source_created_at >= 0),
  PRIMARY KEY(community_id,identity_pubkey)
);

CREATE TABLE dm_visibility(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  identity_pubkey TEXT NOT NULL CHECK(length(identity_pubkey)=64),
  channel_id TEXT NOT NULL,
  source_event_id TEXT NOT NULL CHECK(length(source_event_id)=64),
  source_created_at INTEGER NOT NULL CHECK(source_created_at >= 0),
  PRIMARY KEY(community_id,identity_pubkey,channel_id),
  FOREIGN KEY(community_id,identity_pubkey)
    REFERENCES dm_visibility_heads(community_id,identity_pubkey) ON DELETE CASCADE
);
CREATE INDEX dm_visibility_channels
  ON dm_visibility(community_id,identity_pubkey,channel_id);

CREATE TABLE inbox_overrides(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  identity_pubkey TEXT NOT NULL CHECK(length(identity_pubkey)=64),
  conversation_id TEXT NOT NULL CHECK(length(CAST(conversation_id AS BLOB)) <= 256),
  forced_unread INTEGER NOT NULL DEFAULT 0 CHECK(forced_unread IN (0,1)),
  local_done_at INTEGER,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY(community_id,identity_pubkey,conversation_id)
);

CREATE TABLE search_projection_meta(
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE search_documents(
  rowid INTEGER PRIMARY KEY,
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  event_id TEXT NOT NULL CHECK(length(event_id)=64),
  channel_id TEXT NOT NULL,
  pubkey TEXT NOT NULL CHECK(length(pubkey)=64),
  kind INTEGER NOT NULL CHECK(kind IN (9,40002)),
  created_at INTEGER NOT NULL CHECK(created_at >= 0),
  content TEXT NOT NULL CHECK(length(CAST(content AS BLOB)) <= 65536),
  UNIQUE(community_id,event_id)
);
CREATE INDEX search_documents_scope
  ON search_documents(community_id,channel_id,created_at DESC,event_id);
CREATE INDEX search_documents_author
  ON search_documents(community_id,pubkey,created_at DESC,event_id);

CREATE VIRTUAL TABLE search_fts USING fts5(
  content,
  content='search_documents',
  content_rowid='rowid',
  tokenize='unicode61 remove_diacritics 2'
);

CREATE TRIGGER search_documents_ai AFTER INSERT ON search_documents BEGIN
  INSERT INTO search_fts(rowid,content) VALUES(new.rowid,new.content);
END;
CREATE TRIGGER search_documents_ad AFTER DELETE ON search_documents BEGIN
  INSERT INTO search_fts(search_fts,rowid,content) VALUES('delete',old.rowid,old.content);
END;
CREATE TRIGGER search_documents_au AFTER UPDATE ON search_documents BEGIN
  INSERT INTO search_fts(search_fts,rowid,content) VALUES('delete',old.rowid,old.content);
  INSERT INTO search_fts(rowid,content) VALUES(new.rowid,new.content);
END;

-- Prior clients treated NIP-29's metadata-only `hidden` tag as viewer state.
-- Viewer-specific hiding is projected exclusively from owner-scoped kind 30622.
UPDATE channels SET is_hidden=0;
