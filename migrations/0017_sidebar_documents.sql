-- Synced sidebar documents (kind 30078, d=channel-sections / channel-sort):
-- the current head per identity, decoded into its register tree.
CREATE TABLE sidebar_documents(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  pubkey TEXT NOT NULL CHECK(length(pubkey)=64),
  lane TEXT NOT NULL CHECK(lane IN ('channel-sections','channel-sort')),
  event_id TEXT NOT NULL CHECK(length(event_id)=64),
  created_at INTEGER NOT NULL CHECK(created_at >= 0),
  tree_json TEXT NOT NULL,
  PRIMARY KEY(community_id,pubkey,lane)
);
