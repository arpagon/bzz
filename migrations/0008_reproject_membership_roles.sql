-- v0.11.0 began interpreting the fourth field of relay-signed membership
-- `p` tags as the exact managed-agent role. Existing event IDs were already
-- cached, however, so duplicate ingestion could not rebuild their historical
-- `member` projections. Repair only rows derived from the current trusted
-- membership head; all other rows remain non-authoritative `member` entries.
WITH derived_roles(row_id, derived_role) AS (
  SELECT m.rowid,
         CASE WHEN EXISTS (
           SELECT 1
           FROM json_each(e.tags_json) AS tag
           WHERE json_valid(e.tags_json)
             AND json_type(tag.value)='array'
             AND json_array_length(tag.value)=4
             AND json_extract(tag.value,'$[0]')='p'
             AND json_extract(tag.value,'$[1]')=m.pubkey
             AND json_extract(tag.value,'$[3]')='bot'
             AND m.pubkey=lower(m.pubkey)
             AND length(m.pubkey)=64
         ) THEN 'bot' ELSE 'member' END
  FROM memberships AS m
  JOIN channel_membership_heads AS h
    ON h.community_id=m.community_id
   AND h.channel_id=m.channel_id
   AND h.source_event_id=m.source_event_id
  JOIN events AS e
    ON e.community_id=m.community_id
   AND e.event_id=m.source_event_id
   AND e.kind=39002
   AND e.channel_id IS NULL
  JOIN communities AS c
    ON c.id=m.community_id
   AND c.relay_pubkey=e.pubkey
  WHERE json_valid(e.tags_json)
    AND EXISTS (
      SELECT 1 FROM json_each(e.tags_json) AS d_tag
      WHERE json_type(d_tag.value)='array'
        AND json_array_length(d_tag.value)=2
        AND json_extract(d_tag.value,'$[0]')='d'
        AND json_extract(d_tag.value,'$[1]')=m.channel_id
    )
)
UPDATE memberships
SET role=(SELECT derived_role FROM derived_roles WHERE row_id=memberships.rowid)
WHERE rowid IN (SELECT row_id FROM derived_roles)
  AND role<>(SELECT derived_role FROM derived_roles WHERE row_id=memberships.rowid);
