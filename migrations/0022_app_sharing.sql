-- v0.16 M6: apps offered in a channel (plan D3; ADR v0.16 apps, decision 9).
--
-- An offer is one NIP-AR artifact of type `bzz.app` in a channel: script,
-- manifest and brief, signed by whoever published its head revision. Any
-- writer of the channel can publish a new revision of it, delete it or
-- restore it, so what this bzz takes as an offer is one revision and its
-- signer. Nothing here runs anything: a received offer becomes a waiting
-- version in `app_versions` (proposer 'member'), and only an approval of its
-- exact bytes lets it run.
--
-- app_shares: what this bzz offered, one artifact per app and channel.
-- app_offers: every revision of an offer this bzz took in, and what became
--   of it, so that a resync does not take it in twice and an offer the
--   owner refused does not come back as the same bytes.
CREATE TABLE app_shares (
    community_id TEXT NOT NULL REFERENCES communities (id) ON DELETE CASCADE,
    channel_id TEXT NOT NULL,
    name TEXT NOT NULL,
    -- The artifact (`d`) of the offer.
    artifact_id TEXT NOT NULL,
    -- The version of the app it carries, and that version's script.
    version INTEGER NOT NULL CHECK (version > 0),
    script_sha256 TEXT NOT NULL CHECK (length(script_sha256) = 64),
    -- The event id of the last revision this bzz published.
    revision TEXT NOT NULL CHECK (length(revision) = 64),
    -- The version whose brief was posted to the channel as a message: once
    -- for each version shared.
    brief_posted_version INTEGER,
    shared_at INTEGER NOT NULL,
    -- Set when the offer was withdrawn (a delete revision). The row stays,
    -- so that sharing the same version again does not post its brief twice.
    withdrawn_at INTEGER,
    PRIMARY KEY (community_id, channel_id, name)
);

CREATE TABLE app_offers (
    community_id TEXT NOT NULL REFERENCES communities (id) ON DELETE CASCADE,
    -- The event id of the revision.
    revision TEXT NOT NULL CHECK (length(revision) = 64),
    channel_id TEXT NOT NULL,
    artifact_id TEXT NOT NULL,
    -- Who signed this revision (public key, hex).
    signer TEXT NOT NULL,
    -- What the revision says, when it could be read.
    name TEXT,
    script_sha256 TEXT,
    brief_sha256 TEXT,
    -- The version of the app it was recorded as, while there is one.
    version INTEGER,
    -- waiting: a version that waits for the owner's answer.
    -- installed: approved here, or the bytes that already run.
    -- declined: the owner refused it. The same bytes are not offered again.
    -- failed: it did not load; never offered.
    -- refused: bzz did not take it (tampered, too large, an unknown api, a
    --   name no app may have, a direct message, too many waiting). `detail`
    --   says which.
    -- withdrawn: deleted in the channel before an answer.
    -- replaced: a later revision of the same artifact took its place before
    --   an answer.
    state TEXT NOT NULL CHECK (state IN ('waiting', 'installed', 'declined', 'failed', 'refused', 'withdrawn', 'replaced')),
    detail TEXT,
    received_at INTEGER NOT NULL,
    PRIMARY KEY (community_id, revision)
);

CREATE INDEX app_offers_by_artifact ON app_offers (community_id, artifact_id);
CREATE INDEX app_offers_waiting ON app_offers (community_id, channel_id) WHERE state = 'waiting';
