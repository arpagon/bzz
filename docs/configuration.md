# Configuration

`bzz` stores non-secret configuration in the platform configuration directory
(`bzz paths` prints it). `BZZ_CONFIG_DIR`, `BZZ_DATA_DIR`, and `BZZ_CACHE_DIR`
override directories for testing and managed deployments; they never contain
secret values.

```toml
default_community = "00000000-0000-0000-0000-000000000000"

[[identities]]
id = "00000000-0000-0000-0000-000000000001"
label = "personal"
pubkey = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
backend = "keychain"
key_ref = "identity:00000000-0000-0000-0000-000000000001"

[[communities]]
id = "00000000-0000-0000-0000-000000000000"
label = "team"
relay_url = "wss://buzz.example/"
identity_id = "00000000-0000-0000-0000-000000000001"
allow_insecure_localhost = false

[ui]
sidebar_width = 28
thread_width = 44
```

Only `wss://` root URLs are accepted by default. `ws://` requires both a
loopback host and explicit acknowledgement. Credentials, queries, fragments,
and non-root paths are rejected. Each relay authority is an isolated Buzz
community; bzz never sends a client-selected tenant identifier.
