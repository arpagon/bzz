-- v0.13.0: NIP-FI verified profiles (relay-signed kind 30382), newest per
-- person. A row with active=0 records that verification ended.
CREATE TABLE verified_profiles(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  pubkey TEXT NOT NULL CHECK(length(pubkey)=64),
  active INTEGER NOT NULL CHECK(active IN (0,1)),
  display_name TEXT,
  username TEXT,
  full_name TEXT,
  issuer_label TEXT,
  verified_at INTEGER,
  event_id TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY(community_id,pubkey)
);
