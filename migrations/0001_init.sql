CREATE TABLE IF NOT EXISTS schema_migrations(
  version INTEGER PRIMARY KEY,
  sha256 TEXT NOT NULL,
  applied_at INTEGER NOT NULL
);

CREATE TABLE identities(
  id TEXT PRIMARY KEY,
  pubkey TEXT UNIQUE NOT NULL CHECK(length(pubkey)=64),
  label TEXT NOT NULL,
  key_backend TEXT NOT NULL CHECK(key_backend IN ('keychain','encrypted-file')),
  key_ref TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE communities(
  id TEXT PRIMARY KEY,
  identity_id TEXT NOT NULL REFERENCES identities(id),
  relay_url TEXT UNIQUE NOT NULL,
  authority TEXT NOT NULL,
  http_base_url TEXT NOT NULL,
  label TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
  relay_pubkey TEXT,
  last_connected_at INTEGER,
  last_error_code TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE channels(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  channel_id TEXT NOT NULL,
  name TEXT NOT NULL,
  about TEXT NOT NULL DEFAULT '',
  channel_type TEXT NOT NULL DEFAULT 'stream',
  visibility TEXT NOT NULL CHECK(visibility IN ('public','private')),
  is_member INTEGER NOT NULL DEFAULT 0 CHECK(is_member IN (0,1)),
  is_hidden INTEGER NOT NULL DEFAULT 0 CHECK(is_hidden IN (0,1)),
  member_count INTEGER NOT NULL DEFAULT 0 CHECK(member_count >= 0),
  metadata_event_id TEXT,
  metadata_created_at INTEGER,
  last_event_at INTEGER,
  PRIMARY KEY(community_id,channel_id)
);
CREATE INDEX channels_order ON channels(community_id,is_member DESC,name);

CREATE TABLE memberships(
  community_id TEXT NOT NULL,
  channel_id TEXT NOT NULL,
  pubkey TEXT NOT NULL CHECK(length(pubkey)=64),
  role TEXT NOT NULL DEFAULT 'member',
  source_event_id TEXT NOT NULL,
  PRIMARY KEY(community_id,channel_id,pubkey),
  FOREIGN KEY(community_id,channel_id) REFERENCES channels(community_id,channel_id) ON DELETE CASCADE
);

CREATE TABLE profiles(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  pubkey TEXT NOT NULL CHECK(length(pubkey)=64),
  display_name TEXT,
  name TEXT,
  picture TEXT,
  nip05 TEXT,
  about TEXT,
  event_id TEXT NOT NULL,
  created_at INTEGER NOT NULL CHECK(created_at >= 0),
  raw_json TEXT NOT NULL,
  PRIMARY KEY(community_id,pubkey)
);

CREATE TABLE events(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  event_id TEXT NOT NULL CHECK(length(event_id)=64),
  kind INTEGER NOT NULL,
  pubkey TEXT NOT NULL CHECK(length(pubkey)=64),
  created_at INTEGER NOT NULL CHECK(created_at >= 0),
  channel_id TEXT,
  content TEXT NOT NULL,
  tags_json TEXT NOT NULL,
  raw_json TEXT NOT NULL,
  root_event_id TEXT,
  parent_event_id TEXT,
  deleted_by_event_id TEXT,
  received_at INTEGER NOT NULL,
  PRIMARY KEY(community_id,event_id)
);
CREATE INDEX events_channel_order ON events(community_id,channel_id,created_at,event_id);
CREATE INDEX events_thread_order ON events(community_id,root_event_id,created_at,event_id);
CREATE INDEX events_kind_author ON events(community_id,kind,pubkey,created_at);

CREATE TABLE deletion_targets(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  deletion_event_id TEXT NOT NULL,
  target_event_id TEXT NOT NULL,
  deletion_kind INTEGER NOT NULL,
  deletion_pubkey TEXT NOT NULL,
  PRIMARY KEY(community_id,deletion_event_id,target_event_id)
);
CREATE INDEX deletion_targets_target ON deletion_targets(community_id,target_event_id);

CREATE TABLE reactions(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  reaction_event_id TEXT NOT NULL,
  target_event_id TEXT NOT NULL,
  pubkey TEXT NOT NULL,
  emoji TEXT NOT NULL,
  created_at INTEGER NOT NULL CHECK(created_at >= 0),
  deleted_by_event_id TEXT,
  PRIMARY KEY(community_id,reaction_event_id)
);
CREATE INDEX reactions_target ON reactions(community_id,target_event_id,emoji);

CREATE TABLE read_contexts(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  identity_pubkey TEXT NOT NULL,
  context_id TEXT NOT NULL CHECK(length(CAST(context_id AS BLOB)) <= 256),
  read_at INTEGER NOT NULL CHECK(read_at BETWEEN 0 AND 4294967295),
  source_created_at INTEGER NOT NULL DEFAULT 0,
  publishable INTEGER NOT NULL DEFAULT 0 CHECK(publishable IN (0,1)),
  PRIMARY KEY(community_id,identity_pubkey,context_id)
);

CREATE TABLE read_slots(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  identity_pubkey TEXT NOT NULL,
  slot_id TEXT NOT NULL,
  client_id TEXT NOT NULL,
  event_id TEXT,
  event_created_at INTEGER NOT NULL DEFAULT 0,
  is_local INTEGER NOT NULL DEFAULT 0 CHECK(is_local IN (0,1)),
  PRIMARY KEY(community_id,identity_pubkey,slot_id)
);

CREATE TABLE sync_cursors(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  scope TEXT NOT NULL,
  scope_id TEXT NOT NULL,
  high_created_at INTEGER NOT NULL DEFAULT 0,
  high_event_id TEXT NOT NULL DEFAULT '',
  complete_through INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY(community_id,scope,scope_id)
);

CREATE TABLE outbox(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  event_id TEXT NOT NULL,
  event_json TEXT NOT NULL,
  kind INTEGER NOT NULL,
  channel_id TEXT,
  state TEXT NOT NULL CHECK(state IN ('pending','unknown','delivered','rejected')),
  attempts INTEGER NOT NULL DEFAULT 0,
  last_error_code TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY(community_id,event_id)
);
CREATE INDEX outbox_pending ON outbox(community_id,state,created_at);

CREATE TABLE drafts(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  channel_id TEXT NOT NULL,
  thread_root_id TEXT NOT NULL DEFAULT '',
  body TEXT NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY(community_id,channel_id,thread_root_id)
);

CREATE TABLE ui_state(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  key TEXT NOT NULL,
  value_json TEXT NOT NULL,
  PRIMARY KEY(community_id,key)
);
