ALTER TABLE drafts ADD COLUMN revision TEXT NOT NULL DEFAULT '';
ALTER TABLE drafts ADD COLUMN state TEXT NOT NULL DEFAULT 'editing' CHECK(state IN ('editing','sending'));
ALTER TABLE drafts ADD COLUMN outbox_event_id TEXT;

UPDATE drafts SET revision=lower(hex(randomblob(16))) WHERE revision='';
CREATE INDEX drafts_outbox_submission ON drafts(community_id,outbox_event_id) WHERE outbox_event_id IS NOT NULL;
