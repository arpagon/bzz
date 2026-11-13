# Buzz 1.0 client-contract fixtures

Vendored unchanged from [block/buzz-app](https://github.com/block/buzz-app) at
`dfdb5582e6810e76681e24a33c3489ba2aa77e5c` (the v1.0.1 release), Apache-2.0:

| File | Source | Contract |
|---|---|---|
| `identity-names.fixtures.json` | `src/bundled/identity-naming/identity-names.fixtures.json` | Contextual identity names, version 1 |
| `mention-rules.fixtures.json` | `src/bundled/mentions/mention-rules.fixtures.json` | Mention rules, version 3 |

`tests/buzz_app_contracts_test.rs` runs every case against
`bzz::protocol::identity_names` and `bzz::protocol::mention_rules`. Update
both files together, from one revision, and record it here.
