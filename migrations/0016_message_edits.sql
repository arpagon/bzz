-- Message edits (kind 40003). One row per stored edit, keyed by the first
-- valid 64-hex `e` tag in signed order (lowercased), scoped to the edit's
-- `h` channel. Selection of the governing edit happens at read time.
CREATE TABLE edit_targets(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  edit_event_id TEXT NOT NULL CHECK(length(edit_event_id)=64),
  target_event_id TEXT NOT NULL CHECK(length(target_event_id)=64),
  channel_id TEXT NOT NULL,
  PRIMARY KEY(community_id,edit_event_id)
);
CREATE INDEX edit_targets_by_target ON edit_targets(community_id,target_event_id);

-- Earlier bzz stored edits without applying them. Index those already held.
INSERT OR IGNORE INTO edit_targets(community_id,edit_event_id,target_event_id,channel_id)
SELECT community_id,event_id,target,channel_id FROM (
  SELECT e.community_id,e.event_id,e.channel_id,
    (SELECT lower(json_extract(t.value,'$[1]')) FROM json_each(e.tags_json) t
      WHERE json_type(t.value)='array'
        AND json_extract(t.value,'$[0]')='e'
        AND length(json_extract(t.value,'$[1]'))=64
        AND lower(json_extract(t.value,'$[1]')) NOT GLOB '*[^0-9a-f]*'
      ORDER BY t.key LIMIT 1) AS target
  FROM events e WHERE e.kind=40003 AND e.channel_id IS NOT NULL
) WHERE target IS NOT NULL;
