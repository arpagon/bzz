# Releasing

1. Confirm CI and the pinned real-relay workflow pass.
2. Run `cargo fmt --check`, Clippy, all tests, `cargo deny check`, and
   `cargo audit` against the committed lockfile.
3. Update `docs/protocol-compatibility.md` only through a dedicated Buzz SHA
   compatibility change.
4. Tag `vMAJOR.MINOR.PATCH`. GitHub Actions builds Linux, macOS, and Windows
   archives, SHA-256 checksums, a CycloneDX SBOM, and keyless build-provenance
   attestations.
5. On clean VMs, verify checksums with `sha256sum -c`, verify each archive and
   SBOM with `gh attestation verify <file> --repo arpagon/bzz`, inspect the
   `*.cdx.xml` SBOM, unpack/install the native artifact, and run `bzz --version`.
   Smoke-test both the OS credential backend and encrypted fallback before
   publishing package-manager manifests.

## What cargo-dist generates

`dist-workspace.toml` is the source; `.github/workflows/release.yml` and the
installers (`bzz-installer.sh`, `bzz-installer.ps1`, `bzz.rb`) are made from
it. Never edit the generated workflow by hand beyond the two fixes that
`allow-dirty = ["ci"]` keeps (named in `dist-workspace.toml`). After a change
to `dist-workspace.toml`, with the pinned `cargo-dist` (0.30.2):

```sh
dist generate --check    # must say nothing: the workflow did not change
dist plan                # the archives and installers of the release
dist build --artifacts=global --output-format=json   # the installers, in target/distrib/
```

The shell installer refuses a Linux machine whose glibc is older than
`[dist.min-glibc-version]` (2.34 since v0.17.0; without it dist takes 2.35
from the Ubuntu 22.04 build image and refuses RHEL 9 and its rebuilds, where
the binary runs). Check it in the generated installer:
`grep 'check_glibc' target/distrib/bzz-installer.sh` shows `"2" "34"`. The
global build has no checksums of local archives, so that installer says "no
checksums to verify"; the published one checks them.

`getbzz.dev/install.sh` is not generated: it is `site/public/install.sh`,
which checks the machine and then runs the release's `bzz-installer.sh`. Its
`BZZ_SOUND` must match the release it serves (`site/README.md`, "The
installer"). After a release, run it on clean images with `BZZ_CHECK_ONLY=1`
and once in full (`docs/validation-v0.17.0/install.md` has the commands).

## Linux build image

Linux release, CI, upgrade, audio and relay tests use a reviewed Ubuntu 22.04
multi-architecture image pinned by digest. They do not download system packages
from Ubuntu mirrors. Native amd64/arm64 builds share that image; Rust/Cargo and
GitHub/GHCR still require normal network access. Refresh instructions and the
package inventory are in [`ci/linux-build/README.md`](../ci/linux-build/README.md).
Do not replace the dependency-verification step with a runtime `apt-get`.

Cache schema changes are called out in release notes. Schema v8 repairs exact
bot roles from current trusted membership heads; schema v7 adds the rebuildable
community-scoped remote-agent projection and exact bot-role index; schema v5 added the Inbox conversations projection while schema v4 added
bounded local `drafts.mentions_json` metadata. Release
validation must open upgraded v2/v3/v4 fixtures on Linux, macOS, and Windows,
pass FTS and Inbox-projection integrity/rebuild checks, verify owner-only 30622
isolation and Inbox identity/DM fences, and confirm malformed stored mention
metadata degrades to ordinary draft text before publishing. Downgrades restore
the pre-migration backup rather than running reverse SQL.

For v0.9 and later, verify the release binary's diagnostics CLI against
disposable sentinels, owner-only journal/report permissions, fixed rotation,
exact delivery labels, and an unchanged read-only database. A clean and upgraded
profile must make zero OTLP requests by default. Run fake receiver redirect/
status/timeout/queue gates; a scoped-token `telemetry.test` canary is required
only when telemetry code or enrollment behavior changes. Missing, revoked,
offline, and rate-limited telemetry must leave local bzz usable.

For v0.10, verify that an older top-level `[[local_agents]]` configuration is
removed atomically while every other unknown field remains rejected. Confirm
that CLI help and current docs expose no retired Codex-drafter surface and do
not claim managed-agent support.

For v0.11, verify exact relay-signed bot membership, NIP-OA owner binding,
agent-authored profile/declaration when present or required, owner-authored
policy when present, DM hardening, community isolation, staleness/removal, and
exact send-time `p`-tag revalidation. For v0.11.1, also verify schema-8
historical role convergence, no-declaration bot compatibility, DM-only
declaration requirements, verified-owner-only fallback for absent policy, and
pinned-relay semantic kind-40099 rendering. The pinned relay fixture must use a deterministic remote identity
without an LLM or ACP process. Release CLI/TUI language must say remote and
expose no create/start/stop/exec/log/runtime-hosting surface. Agent diagnostics
must remain local-only and content-free.

For v0.15 and later, a release with local agents must also verify, on the
candidate's own commit:

- **The agent host.** `cd agent-host && npm ci --ignore-scripts
  --omit=optional && npm test` on the oldest supported Node (22.19) and a
  current one; `npm audit --omit=dev` there and in each folder of
  `agent-adapters/`, with nothing new accepted in silence. A change to
  `agent-host/` or to a lock file changes the install's folder name, so every
  owner installs again on first use: say so in the notes.
- **The key rule.** `cargo test --locked --test secret_scan_test`: no agent
  key, owner key or relay credential in a harness's environment, arguments,
  files, logs or tool results, in a sealed file of a move, or in a launch kit
  outside its one key file.
- **One reply per mention on the pinned relay,** with the harness and bzz
  killed: `./scripts/test-relay.sh`, which needs Node and builds `buzz` and
  `buzz-acp` at the pin. It also holds decision D9: the host still reads
  `buzz-acp`'s prompt. When the Buzz pin moves, record those prompts again
  (`tests/fixtures/agents/buzz-acp-prompts/README.md`).
- **The release binary in a terminal.** `./scripts/test-relay.sh --herdr`
  runs the `t15-*` scenarios with the real agent host on its stand-in model:
  an agent answering, the agents panel during a turn, stopping a turn,
  `:agent new` from its form, a command approved and refused, a moved-away
  agent and each coding harness in the panel, and from the command line
  `agent new` on a terminal, `agent credential set` without echo, and
  `agent move` and `agent receive` between two profiles. It needs Node.js
  and, once, the npm registry ([`e2e-herdr.md`](e2e-herdr.md)).
- **The benchmarks** against the last release, for a profile without agents
  and for the agent directory: build both sides first, measure them in turn
  more than once, and record the machine's load
  (`docs/validation-v0.15.0.md`, "Benchmarks against v0.14.0").
- **The coding harnesses,** only on a machine with the owner's own `grok`,
  `codex` and `claude`, signed in, and with his consent, because it spends
  his usage: `BZZ_REAL_HARNESSES=grok,codex,claude-code
  BZZ_RELAY_TEST_FILTER=real_relay_coding_harnesses ./scripts/test-relay.sh`.
  When a program's version range or an adapter's version changes, record the
  transcripts again and read them before committing
  (`docs/validation-v0.15.0.md`, M6). Never in CI.
- **The upgrade.** `./scripts/upgrade-test.sh` from the last release, with a
  hosted agent of v0.13 in the profile: it must load unchanged and still be
  served by the broker.
- **What is installed on first use** is in `THIRD_PARTY_LICENSES.md`: the
  host's packages and each adapter's, with any license that is not an open
  one named.
- Release CLI and TUI language keeps three words apart: a *remote* agent
  (`bzz agents`, someone else runs it), a *hosted* agent of the broker
  (`bzz broker`, you run its process) and a *local* agent (`bzz agent`, bzz
  runs it).

For v0.4 and later interaction changes, also run the deterministic functional
TUI harness in ordinary CI and record a controlled release-binary Herdr
acceptance run (or an explicit release exception). See
[`release-v0.4.0.md`](release-v0.4.0.md).
