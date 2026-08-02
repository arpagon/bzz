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

## ratatui-image

`ratatui-image` v11.0.5 (`00920803a50e7b7763ceb69978be90c4391325ad`)
is used under the MIT license for Kitty, Sixel, iTerm2, Unicode half-block, cell
metric, and vertically sliced Ratatui image rendering. Default features are
disabled so `bzz` does not introduce the optional native `libchafa` backend.
Copyright belongs to Benjamin Große, Atanas Yankov, wooster0, and the
ratatui-image contributors. The complete license is included in crate and SBOM
metadata.
