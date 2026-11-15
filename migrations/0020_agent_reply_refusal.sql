-- A stored reply that will never be published: the relay refused it, or bzz
-- withheld it because the agent may no longer write in that place. The row
-- stays, so that its request can never get a second signed reply, and
-- nothing publishes it again. `refusal` says why, by kind.
ALTER TABLE agent_replies ADD COLUMN refused_at INTEGER;
ALTER TABLE agent_replies ADD COLUMN refusal TEXT;

DROP INDEX agent_replies_unpublished;
CREATE INDEX agent_replies_unpublished ON agent_replies (agent_id)
    WHERE published_at IS NULL AND refused_at IS NULL;
