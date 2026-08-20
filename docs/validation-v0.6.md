# v0.6 conversation-action validation evidence

**Recorded:** 2026-08-20 UTC

All committed fixtures use synthetic bzz-owned content. No profile picture,
community conversation, identity, clipboard payload, or third-party artwork is
stored as evidence.

## Candidate gates

```text
cargo fmt --check                                      PASS
cargo clippy --all-targets --locked -- -D warnings     PASS
cargo test --locked                                    PASS (104 unit tests)
cargo deny check                                       PASS (documented duplicate warnings)
cargo audit                                            PASS (two documented transitive advisories)
cargo build --release --locked                         PASS
./target/release/bzz --version                         bzz 0.6.0
```

`cargo audit` reported only the documented transitive `instant` unmaintained
and `lru` panic-safety advisories. Dependency pins were not changed. The
opt-in real-relay journey passed with a removed disposable worktree at pinned
Buzz revision `ede26863345a518ec46edd6d7692e0281883491b`.

The `bzz 0.6.0` release binary also passed the isolated, credential-free Herdr
startup/help/quit and custom-keymap/Inbox scenarios. The disposable pane and
profile root were removed after the run.

## Local rendering measurements

Criterion `timeline` measurements on AMD Ryzen 7 3700X/Linux 6.11/rustc 1.95:

| Case | Result |
|---|---:|
| Timeline render: 500 messages, 180×48, 110-cell measure | 1.923–1.963 ms |
| Redraw gate: 1,000 idle admissions | 285.62–290.02 ns |

The redraw-gate unit coverage continues to prove zero extra frame admissions
across 1,000 idle ticks after the initial frame.

## New behavior coverage

- Sidebar ordering tests verify smart/recent/alphabetical ordering and stable
  tie breaks; app viewports use the same ordered ID sequence for render, mouse,
  and keyboard navigation.
- Identicon tests prove deterministic 10×10 local raster generation. Existing
  text-marker and no-control rendering tests cover the fallback.
- Markdown tests cover headings, quotes, lists, code fences, tables, bounded
  cells, inert links, and escape sanitization.
- OSC 52 tests cover base64-only framing and the 64-KiB refusal. Timeline copy
  range tests verify event-ID anchors and chronological ranges. Typed reducer
  and keymap tests cover copy/sort/reaction entry without service access.

## Manual review required

Before a v0.6 tag, inspect the release binary in Kitty (and one non-graphics
terminal) using generated/disposable content. Exercise `Space s`, `r`, `v`/
`y`, Markdown blocks, and `ui.clipboard = "disabled"`; confirm local identicons
overlay rather than replace author text and no private payload is shown in
status feedback.
