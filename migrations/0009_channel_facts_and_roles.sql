-- v0.13.0: NIP-CL channel identity and Buzz channel settings (labels, creator,
-- creation time, join_role, unlisted, archived, auto_join) are kept as one
-- bounded JSON object derived from the current relay-signed kind 39000.
-- Existing rows start unknown ('{}'); the next directory refresh re-ingests
-- the current 39000 and repairs them (Store::repair_current_channel_facts).
ALTER TABLE channels ADD COLUMN facts_json TEXT NOT NULL DEFAULT '{}'
  CHECK(json_valid(facts_json) AND length(facts_json) <= 8192);

-- v0.13.0 also projects every NIP-29 role (owner, admin, member, guest, bot),
-- not only `bot`, so announce-channel guests are read-only. Re-derive only rows
-- from the current trusted membership head, exactly as migration 0008 did.
WITH derived_roles(row_id, derived_role) AS (
  SELECT m.rowid,
         COALESCE((
           SELECT CASE
                    WHEN json_array_length(tag.value) < 4 THEN 'member'
                    WHEN json_array_length(tag.value) > 4 THEN 'unknown'
                    WHEN json_extract(tag.value,'$[3]') IN ('','member') THEN 'member'
                    WHEN json_extract(tag.value,'$[3]') IN ('owner','admin','guest','bot')
                      THEN json_extract(tag.value,'$[3]')
                    ELSE 'unknown'
                  END
           FROM json_each(e.tags_json) AS tag
           WHERE json_type(tag.value)='array'
             AND json_extract(tag.value,'$[0]')='p'
             AND json_extract(tag.value,'$[1]')=m.pubkey
           LIMIT 1
         ), m.role)
  FROM memberships AS m
  JOIN channel_membership_heads AS h
    ON h.community_id=m.community_id
   AND h.channel_id=m.channel_id
   AND h.source_event_id=m.source_event_id
  JOIN events AS e
    ON e.community_id=m.community_id
   AND e.event_id=m.source_event_id
   AND e.kind=39002
  JOIN communities AS c
    ON c.id=m.community_id
   AND c.relay_pubkey=e.pubkey
  WHERE json_valid(e.tags_json)
    AND m.pubkey=lower(m.pubkey)
    AND length(m.pubkey)=64
)
UPDATE memberships
SET role=(SELECT derived_role FROM derived_roles WHERE row_id=memberships.rowid)
WHERE rowid IN (SELECT row_id FROM derived_roles)
  AND role<>(SELECT derived_role FROM derived_roles WHERE row_id=memberships.rowid);
