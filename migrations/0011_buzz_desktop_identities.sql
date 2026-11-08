-- v0.13.0: identities may live in Buzz Desktop (local credential provider).
-- SQLite cannot alter a CHECK, so rebuild the table. communities references
-- identities(id) without cascade. With foreign keys deferred to commit, the
-- implicit delete of DROP TABLE counts violations that the re-insert into the
-- same table name clears again, so every reference is verified at commit.
-- (Rows must be re-inserted under the name `identities`; a RENAME would not
-- clear the deferred count.)
PRAGMA defer_foreign_keys=ON;
CREATE TEMP TABLE identities_v11_copy AS SELECT * FROM identities;
DROP TABLE identities;
CREATE TABLE identities(
  id TEXT PRIMARY KEY,
  pubkey TEXT UNIQUE NOT NULL CHECK(length(pubkey)=64),
  label TEXT NOT NULL,
  key_backend TEXT NOT NULL CHECK(key_backend IN ('keychain','encrypted-file','buzz-desktop')),
  key_ref TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
INSERT INTO identities(id,pubkey,label,key_backend,key_ref,created_at)
  SELECT id,pubkey,label,key_backend,key_ref,created_at FROM identities_v11_copy;
DROP TABLE identities_v11_copy;
