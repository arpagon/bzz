-- v0.16 M5: agents write and change apps (plan D7). Migration 23: 22 is
-- M6's sharing, merged into v0.16.0 beside this one.
--
-- A version an agent proposes records, beside the agent (proposer_*), the
-- person whose message asked for it: the approval shows both. NULL for a
-- version that no person asked an agent for (the owner's own, a person's
-- offer in the channel).
--
-- told_at: when the agent that proposed the version was told what became
-- of it (applied, refused with the reason, or failed with the error). The
-- tool that proposes answers "waiting for the owner"; the outcome reaches
-- the agent at its next turn in the channel it proposed in, once. NULL:
-- not told yet. Every row from before this migration counts as told, so
-- that no agent is told of something it never asked about.
ALTER TABLE app_versions ADD COLUMN asked_by_id TEXT;
ALTER TABLE app_versions ADD COLUMN asked_by_name TEXT;
ALTER TABLE app_versions ADD COLUMN told_at INTEGER;
UPDATE app_versions SET told_at = COALESCE(decided_at, proposed_at);

CREATE INDEX app_versions_untold ON app_versions (community_id, proposer_id, proposer_channel)
    WHERE proposer_kind = 'agent' AND told_at IS NULL;
