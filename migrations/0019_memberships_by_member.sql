-- v0.15.0: the agent directory no longer requires the `bot` role, so it asks
-- for a member's channels by the member, whatever its role. The index of
-- migration 0007 starts with the role, and the primary key with the channel:
-- without this one every such question read all memberships of the community.
CREATE INDEX memberships_by_member ON memberships(community_id,pubkey,channel_id);
