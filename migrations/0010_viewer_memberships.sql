-- v0.13.0: the viewer's own memberships and roles from a verified NIP-CL
-- compact membership snapshot (kind 39008). While present they are the
-- authority for the viewer's role; relay rosters (39002) keep serving
-- other-member features but never override them (NIP-CL handoff rule 4).
-- Cleared whenever the capability is missing or a snapshot fails to verify.
CREATE TABLE viewer_memberships(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  channel_id TEXT NOT NULL,
  role TEXT NOT NULL CHECK(role IN ('owner','admin','member','bot','guest')),
  snapshot_id TEXT NOT NULL CHECK(length(snapshot_id)=64),
  PRIMARY KEY(community_id,channel_id),
  FOREIGN KEY(community_id,channel_id) REFERENCES channels(community_id,channel_id) ON DELETE CASCADE
);
