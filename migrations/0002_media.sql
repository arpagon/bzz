ALTER TABLE drafts ADD COLUMN attachments_json TEXT NOT NULL DEFAULT '[]';

CREATE TABLE media_cache(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  sha256 TEXT NOT NULL CHECK(length(sha256)=64),
  variant TEXT NOT NULL DEFAULT 'original',
  mime TEXT NOT NULL,
  byte_size INTEGER NOT NULL CHECK(byte_size >= 0),
  width INTEGER,
  height INTEGER,
  validated_at INTEGER NOT NULL,
  last_accessed_at INTEGER NOT NULL,
  PRIMARY KEY(community_id,sha256,variant)
);
CREATE INDEX media_cache_lru ON media_cache(community_id,last_accessed_at);
