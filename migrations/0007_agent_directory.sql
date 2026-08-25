CREATE TABLE remote_agents(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  agent_pubkey TEXT NOT NULL CHECK(length(agent_pubkey)=64),
  owner_pubkey TEXT CHECK(owner_pubkey IS NULL OR length(owner_pubkey)=64),
  name TEXT NOT NULL,
  capabilities_json TEXT NOT NULL DEFAULT '[]',
  presence TEXT NOT NULL DEFAULT 'unknown'
    CHECK(presence IN ('online','away','offline','unknown')),
  respond_to TEXT
    CHECK(respond_to IS NULL OR respond_to IN ('owner-only','allowlist','anyone')),
  respond_to_allowlist_json TEXT NOT NULL DEFAULT '[]',
  verification_state TEXT NOT NULL
    CHECK(verification_state IN ('verified','incomplete','invalid','removed','stale')),
  failure_reason TEXT,
  profile_event_id TEXT,
  declaration_event_id TEXT,
  policy_event_id TEXT,
  last_verified_at INTEGER,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY(community_id,agent_pubkey)
);

CREATE INDEX remote_agents_directory
  ON remote_agents(community_id,verification_state,name,agent_pubkey);

CREATE INDEX memberships_agent_role
  ON memberships(community_id,role,pubkey,channel_id);
