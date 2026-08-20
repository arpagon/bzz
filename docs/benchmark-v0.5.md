# v0.5 workspace rendering benchmark notes

**Recorded:** 2026-08-20 UTC

**Host:** AMD Ryzen 7 3700X; Linux 6.11.0-29-generic; rustc 1.95.0

**Command:**

```sh
cargo bench --bench timeline --locked -- \
  'redraw gate 1k idle ticks|timeline render 500 messages at 110-cell measure'
```

These are local Criterion measurements, not a release-performance promise.
They use Ratatui's `TestBackend`, synthetic bzz-owned messages, and no relay,
identity, or user content.

| Case | Result |
|---|---:|
| Timeline render: 500 messages, 180×48 terminal, 110-cell measure | 1.775–1.805 ms |
| Redraw gate: 1,000 idle tick admissions | 283.58–292.39 ns |

The redraw-gate unit test also proves the behavioral result that matters more
than the admission microbenchmark: after its initial frame, 1,000 idle ticks
admit **zero** additional frames. The old loop unconditionally drew once per
100-ms tick; the current loop requests a draw only for input, resize,
network/background state, visible timer work, or media readiness.

Repeat the command after changes that affect `ui::timeline`, layout, media
placement, or redraw scheduling. Treat a material regression or a new
unexpected idle frame as a release blocker until explained.
