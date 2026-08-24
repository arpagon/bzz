# Third-party licenses

`bzz` is distributed under MIT OR Apache-2.0. Its Rust dependencies retain
their respective licenses; exact versions are recorded in `Cargo.lock` and a
CycloneDX SBOM is attached to releases.

## Buzz

`buzz-core` and `buzz-sdk` are used from
`block/buzz@ede26863345a518ec46edd6d7692e0281883491b` under Apache-2.0.
Copyright belongs to the Buzz contributors. See `LICENSE-APACHE` for the
Apache License 2.0 text.

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
