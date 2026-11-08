-- v0.13.0: NIP-38 `d:general` user status (kind 30315), newest per person.
-- Expiry is evaluated at display time; an empty status is a clear.
CREATE TABLE user_statuses(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  pubkey TEXT NOT NULL CHECK(length(pubkey)=64),
  text TEXT NOT NULL,
  emoji TEXT NOT NULL,
  expires_at INTEGER,
  event_id TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY(community_id,pubkey)
);
