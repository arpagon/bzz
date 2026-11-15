# Third-party licenses

`bzz` is distributed under MIT OR Apache-2.0. Its Rust dependencies retain
their respective licenses; exact versions are recorded in `Cargo.lock` and a
CycloneDX SBOM is attached to releases.

## Buzz

`buzz-core` and `buzz-sdk` are used from
`block/buzz@bf3bdd0eaefe39289bf9620594f254c7f68897f5` under Apache-2.0.
`buzz-audio-client` (feature `huddles-audio`) comes from the same revision.
`src/protocol/huddle_tracker.rs` is ported from
`crates/buzz-cli/src/commands/huddle_tracker.rs`, and
`tests/fixtures/huddle_sequences.json` is copied from
`crates/buzz-cli/tests/fixtures/huddle_sequences.json`, at that revision.
`src/protocol/sidebar_doc.rs` is ported from
`crates/buzz-cli/src/commands/sidebar_doc.rs`, and
`tests/fixtures/buzz/solo-creation.json` is copied from
`test-fixtures/solo-creation.json`, at the same revision.
`tests/fixtures/buzz-app/*.fixtures.json` are copied unchanged from
`block/buzz-app@dfdb5582e6810e76681e24a33c3489ba2aa77e5c` (Apache-2.0), and
`src/protocol/identity_names.rs` and `src/protocol/mention_rules.rs`
implement the contracts specified there.
`agent-host/src/model.mjs` adapts the `AuthFile` class and the reading of
Pi's default model from `src/bundled/pi/host.mjs` at
`block/buzz-app@095df3f0f6f858b16c6d7c5e94c9cfbcd8c2360e` (Apache-2.0); the
changes are listed at the top of that file. `agent-host/src/packages.mjs`
adapts `loadPackages`, `loadPackage`, `isProvider` and the import hook for
Pi packages from the same file at the same revision, with its changes listed
at its top; the package rules it follows are those of
`src/bundled/pi/EXTENSIONS.md` there. The rest of the agent host is
independently written; Buzz's host was studied for its ideas (the event id as
the request id, one conversation per channel or thread, a final send that ends
the turn).
`tests/fixtures/agents/buzz-acp-prompts/*.txt` are prompts produced by
`buzz-acp` at `block/buzz@bf3bdd0eaefe39289bf9620594f254c7f68897f5`
(`crates/buzz-acp/src/queue.rs`), recorded with disposable identities.
Copyright belongs to the Buzz contributors. See `LICENSE-APACHE` for the
Apache License 2.0 text.

## Agent host (Node)

The Pi Durable agent host in `agent-host/` is not part of the bzz binary's
Rust dependency tree. Its packages are installed from npm on the owner's
machine, from `agent-host/package-lock.json`, with install scripts off; bzz
ships none of them. Direct dependencies, all pinned to exact versions:

| Package | Version | License |
|---|---|---|
| `@earendil-works/pi-durable`, `chord`, `pi-ai`, `pi-schema` | 1.5.0 | MIT |
| `just-bash` | 3.6.0 | Apache-2.0 |
| `isomorphic-git` | 1.44.11 | MIT |
| `proper-lockfile` | 4.1.2 | MIT |
| `diff` | 8.0.4 | BSD-3-Clause |

`agent-host/src/disk.mjs`, `adapters.mjs`, `env.mjs` and `git.mjs` use the
public interfaces of those packages and copy none of their source. What bzz
installs (`--omit=optional --omit=dev`) is 176 packages: MIT (106),
Apache-2.0 (43), BSD-3-Clause (15), ISC (7), and one each under
BSD-2-Clause, Unlicense, 0BSD, "MIT AND Zlib" and "MIT AND BSD-3-Clause".
Two of them (`@google/genai`, `protobufjs`) declare an install script,
which `--ignore-scripts` never runs. The whole lock file lists 204 packages
under those terms, and one under LGPL-3.0: `node-liblzma` 2.2.0, an optional
dependency of `just-bash` for `xz` archives. It is native code that only an
install script would build, so it never runs; installing with
`--omit=optional` leaves it and 27 other optional packages out, and the
host's tests pass without them.

## Adapters of the coding harnesses (Node)

For an agent on the `codex` or the `claude-code` harness (both experimental
in v0.15.0), bzz drives the owner's own program through that program's ACP
adapter. Like the agent host, an adapter is not part of the bzz binary: only
its `package.json` and lock file are (`agent-adapters/codex/`,
`agent-adapters/claude-code/`). It is installed from npm on the owner's
machine the first time such an agent needs it, with
`npm ci --ignore-scripts --omit=optional --omit=dev`. An owner who creates no
such agent installs none of it.

| Package | Version | License | Installed with it |
|---|---|---|---|
| `@agentclientprotocol/codex-acp` | 2.5.0 | Apache-2.0 | 24 packages: MIT (18), Apache-2.0 (3), ISC (2), BSD-3-Clause (1) |
| `@agentclientprotocol/claude-agent-acp` | 0.101.0 | Apache-2.0 | 107 packages: MIT (90), ISC (7), Apache-2.0 (4), BSD-3-Clause (3), BSD-2-Clause (1), Unlicense (1), and one under its publisher's own terms (below) |

**Not open source:** `@anthropic-ai/claude-agent-sdk` 0.3.318, which the
Claude Code adapter needs, is published under Anthropic's own terms
("SEE LICENSE IN README.md" in its package). bzz does not ship it, copy from
it or link to it; `npm` installs it on the machine of an owner who creates a
`claude-code` agent, where it starts that owner's own `claude`. The optional
packages of both adapters, which are copies of `claude` and `codex`
themselves for each platform (the first under the same terms, the second
under Apache-2.0), are left out by `--omit=optional`: bzz points each adapter
at the program the owner installed.

## slk

The interaction design was independently implemented after studying
`gammons/slk@8149c3b18ed04c259efe5feb545d040ab043d922`. The built-in theme
palette data in `src/ui/theme/builtin.rs` is adapted from that MIT-licensed
revision. Its media architecture also informed capability probing, bounded
fetch/cache work, partial-visibility fallbacks, and preview behavior; the Rust
implementation is independent.

> MIT License
>
> Copyright (c) 2026 Grant Ammons
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all
> copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
> SOFTWARE.

## Concord

The semantic theme behavior and public `theme.toml` documentation were studied
at `chojs23/concord@7cc1b98c6ad59067e8158d2ba0f61d7108f21daa`, which is
GPL-3.0-only. No Concord source code, assets, or dependency are included. The
smaller `bzz` group inventory, parser, resolver, and runtime ownership were
implemented independently for Ratatui. Concord's bounded media workers,
generation/LRU concepts, and visible-target behavior were also reviewed as
behavior only; no GPL media implementation was copied.

## arboard

`arboard` v3.6.1 is a pinned direct dependency under MIT OR Apache-2.0. bzz
uses its documented native clipboard API only behind the explicit, one-shot
composer import boundary; bzz's precedence, staging, state, configuration, and
tests are independently authored. Its Wayland data-control support is included
for desktop compatibility and degrades locally when unavailable. Its Windows
bridge `clipboard-win` v5.4.1 uses the permissive Boost Software License 1.0;
that standard license is explicitly approved in `deny.toml`.

## Native file chooser

`ashpd` v0.13.13 is a pinned Linux direct dependency under MIT. bzz enables
only its Tokio and file-chooser features and uses it to call XDG Desktop Portal
directly; no shell or zenity fallback is present. `rfd` v0.17.2 is a pinned MIT
direct dependency only on Windows and macOS, where it uses the platforms'
native open-dialog APIs with default Linux backends disabled. Their selected
paths are handled only through bzz's independently authored transient picker
boundary and secure staging lifecycle. Complete dependency license texts and
versions are retained in the crates and release SBOM.

## OpenTelemetry protobuf messages

`opentelemetry-proto` v0.32.0 and `prost` v0.14.3 are pinned direct
dependencies under Apache-2.0 for generated OTLP log message types and protobuf
encoding. bzz does not include the OpenTelemetry SDK exporter pipeline, metrics,
or traces. The bounded HTTP exporter, consent/configuration, credential,
allowlist, batching, retry, and privacy boundaries are independently authored.
Complete dependency license texts and transitive versions are retained in the
crates and release SBOM.

## ratatui-image

`ratatui-image` v11.0.5 (`00920803a50e7b7763ceb69978be90c4391325ad`)
is used under the MIT license for Kitty, Sixel, iTerm2, Unicode half-block, cell
metric, and vertically sliced Ratatui image rendering. Default features are
disabled so `bzz` does not introduce the optional native `libchafa` backend.
Copyright belongs to Benjamin Große, Atanas Yankov, wooster0, and the
ratatui-image contributors. The complete license is included in crate and SBOM
metadata.
