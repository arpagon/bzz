-- v0.13.0: custom emoji palette contributions from kind 30030
-- `buzz:custom-emoji` sets and NIP-CE kind 30180 entries. One row per
-- (source coordinate, shortcode); a newer replaceable source replaces all of
-- its rows. The palette picks the newest contribution per shortcode.
CREATE TABLE emoji_contributions(
  community_id TEXT NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
  source_pubkey TEXT NOT NULL CHECK(length(source_pubkey)=64),
  source_kind INTEGER NOT NULL CHECK(source_kind IN (30030,30180)),
  source_d TEXT NOT NULL,
  shortcode TEXT NOT NULL CHECK(length(shortcode) BETWEEN 1 AND 64),
  url TEXT NOT NULL CHECK(length(url) <= 2048),
  created_at INTEGER NOT NULL,
  event_id TEXT NOT NULL,
  PRIMARY KEY(community_id,source_pubkey,source_kind,source_d,shortcode)
);
CREATE INDEX emoji_contributions_shortcode ON emoji_contributions(community_id,shortcode);
-- NIP-30 custom emoji reactions carry their image URL in an `emoji` tag.
ALTER TABLE reactions ADD COLUMN emoji_url TEXT;
