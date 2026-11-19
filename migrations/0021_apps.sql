-- v0.16 M4: the apps installed on this machine (ADR v0.16 apps, rule 16).
--
-- The bytes of a version (its script, manifest and brief) are files under
-- <data>/apps/<community>/<name>/versions/<version>/. What was decided about
-- them is here, and only here: a file on disk says nothing about whether it
-- may run. A script is handed to a runtime only when the bytes of the file
-- hash to `script_sha256` of the app's `current_version`.
--
-- apps: one row per installed app of a community.
-- app_versions: every version that was proposed, who proposed it and what
--   became of it.
-- app_channels: the channels an app is open in. One instance per channel.
CREATE TABLE apps (
    community_id TEXT NOT NULL REFERENCES communities (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    -- The approved version that runs. NULL: none was approved yet.
    current_version INTEGER,
    -- The version that ran before the current one was put to run. bzz goes
    -- back to it when the current one fails as it is started.
    previous_version INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (community_id, name)
);

CREATE TABLE app_versions (
    community_id TEXT NOT NULL,
    name TEXT NOT NULL,
    version INTEGER NOT NULL CHECK (version > 0),
    state TEXT NOT NULL CHECK (state IN ('waiting', 'approved', 'refused', 'failed', 'superseded')),
    -- refused: the owner's reason. failed: the error, with file and line.
    -- superseded: what replaced it.
    detail TEXT,
    -- What the manifest says. NULL for a version that never loaded.
    title TEXT,
    api INTEGER,
    types_json TEXT,
    script_sha256 TEXT NOT NULL CHECK (length(script_sha256) = 64),
    manifest_sha256 TEXT CHECK (manifest_sha256 IS NULL OR length(manifest_sha256) = 64),
    brief_sha256 TEXT NOT NULL CHECK (length(brief_sha256) = 64),
    script_bytes INTEGER NOT NULL CHECK (script_bytes >= 0),
    brief_bytes INTEGER NOT NULL CHECK (brief_bytes >= 0),
    -- Who proposed it: the owner, one of his agents, a person of the channel.
    proposer_kind TEXT NOT NULL CHECK (proposer_kind IN ('owner', 'agent', 'member')),
    -- The public key of the agent or the person. NULL for the owner.
    proposer_id TEXT,
    -- Its name when it proposed, for showing.
    proposer_name TEXT,
    -- The channel it was proposed or offered in, when there is one.
    proposer_channel TEXT,
    proposed_at INTEGER NOT NULL,
    decided_at INTEGER,
    -- Whether the version's files are still on disk.
    kept INTEGER NOT NULL DEFAULT 0 CHECK (kept IN (0, 1)),
    PRIMARY KEY (community_id, name, version),
    FOREIGN KEY (community_id, name) REFERENCES apps (community_id, name) ON DELETE CASCADE
);

CREATE INDEX app_versions_waiting ON app_versions (community_id, name) WHERE state = 'waiting';

CREATE TABLE app_channels (
    community_id TEXT NOT NULL,
    name TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    opened_at INTEGER NOT NULL,
    PRIMARY KEY (community_id, name, channel_id),
    FOREIGN KEY (community_id, name) REFERENCES apps (community_id, name) ON DELETE CASCADE
);

CREATE INDEX app_channels_by_channel ON app_channels (community_id, channel_id);
