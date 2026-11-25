# v0.17.0 — task `ci` (CI green and quick)

Branch `t3/ci-pipeline`, cut from `v0.17.0` at `12b355b`. Linux x86_64,
2026-11-25, on a machine shared with other builds (16 cores). `<scratch>` is
a folder outside the repository. Every sound server below ran inside a
container (`--network none`, no host sound socket, no `/dev/snd`); this
machine's own sound was never used. CI was only read (`gh run view`,
`gh api …/logs`); nothing was started, re-run or cancelled.

"The CI image" is `ghcr.io/arpagon/bzz-build@sha256:e708b63b…`, the digest
in `.github/workflows/ci.yml`, pulled and run here. Test binaries were built
inside it (Rust 1.98.1 of this machine mounted read-only, `cargo test`'s
debug profile as CI builds it) and the T7 step was run in it as the job
runs it: as root, `HOME=/github/home`, `scripts/test-audio-loopback.sh`
starting its private PulseAudio (15.99.1).

## Claims and proofs

| Claim | Rung | Command | Seen |
|---|---|---|---|
| **1.** The defect reproduces in the CI image: T7 prints its round trip, then does not end | the CI image, T7 as CI runs it | `<scratch>/run-t7-ci.sh <scratch>/t7-debug-before` (the test binary of `12b355b`) | `T7 round trip: first tone frame after 1342 ms`, then nothing; still running when killed at 960 s (`<scratch>/repro-before-1.txt`). In CI: 731 s at `e144e52`, over two hours at `24f7486`, `006d8d1`, `0d96e80` |
| 1. Where it waits: the playback device stops asking for audio because the `pulseaudio` crate's reader never lets its writer run | stack samples | the same binary under `gdb` on this machine, as its parent, against a PulseAudio in a container (`scripts/test-audio-containers.sh serve`), 8 interrupts 2 s apart | the reactor thread in `Vec<u8>::resize` / `extend_with` (from `Reactor::recv`) in 7 of 8 samples; the test's feeder parked on the full playback channel (`<scratch>/gdb2.txt`) |
| 1. Why that is slow: before every read of the socket the crate grows its buffer by 1 MiB of zeros; unoptimised that is a loop of a million steps | measured | `<scratch>/resize.rs` built at `-C opt-level=0` and `3` | **3.924 ms** a read at opt-level 0, **0.018 ms** at 3. Its `run` loop reads until a read finds nothing waiting and only then writes playback; capture arrives every few milliseconds, so that moment almost never comes |
| 1. The crate optimised alone ends the hang | the CI image | test binary of `12b355b` built with `--config 'profile.dev.package.pulseaudio.opt-level=3'` | 7.48 s, `0 holes in 2999 ms of tone`, streams 0 / 2 / 0 (round trip still 1381 ms: claim 3) |
| **2.** Every wait in T7 has a bound and says what it waited for; the old defect now fails in seconds | the CI image | this branch's test, built *without* the optimised crate | fails after 10.0 s: `T7 waited more than 8s for the playback device to take 2 s of tone (it stopped asking for audio)` |
| **3.** The 1.4 s round trip in CI (2 s is its bound) was PulseAudio's idle null sink, not bzz: the sink sleeps up to 2 s, its latency while no stream asks for less, and gives its monitor nothing until it wakes | the CI image | this branch's settle step prints what the capture gave while it waited | `capture live after 2226 ms (260 ms of audio in that time)`; PipeWire: `live after 303 ms (300 ms of audio)`. Also tried: a `parec` kept on the monitor 2.5 s before T7 gave 20 ms (`<scratch>/scripts-keep/`); a minimal server config (`-n`) and a suspended sink did not (1383 ms, 1404 ms) |
| 3. With the settle step the round trip reads what bzz does | the CI image | as below | 19 to 20 ms |
| **4.** T7 takes well under a minute in the CI image, ten times in a row | the CI image | `for i in $(seq 10); do <scratch>/run-t7-ci.sh <scratch>/t7-after-paO3; done` | 10 of 10 pass; see "T7, ten runs" |
| 4. The script's own path (cargo inside the image, the `--config` line) builds that same binary and passes | the CI image | `./scripts/test-audio-loopback.sh` in the image as uid 1000, toolchain mounted | `T7 took 6878 ms`, same test binary hash as the runs above |
| 4. With two cores, as a hosted runner has about | the CI image, `docker run --cpus 2` | the same, three times | 8.09 to 8.21 s, round trip 20 ms, 0 holes |
| **5.** T7 on PipeWire (Debian, EL9) and PulseAudio in containers, and the one line with no server | containers | `scripts/test-audio-containers.sh t7 <scratch>/t7-after-paO3`, three times | see "T7 in containers" |
| **6.** The audio job and its T7 step have bounds | read + lint | `.github/workflows/ci.yml`; `actionlint` (`rhysd/actionlint` image, with shellcheck) | `timeout-minutes: 30` on the job, `15` on the step; actionlint: no findings |
| **7.** The agent-host test client gives a slow runner room, from an environment variable CI sets, and a hung host still fails | node, this machine (Node 26.5) | `BZZ_AGENT_HOST_WAIT_MARGIN_MS=60000 node --test test/packages.test.mjs`; a scratch test whose host logs a line and never answers, with the margin at 2000 | 7 of 7 pass; the hung host fails after **32003 ms** with `timed out after 32000 ms: initialize (the host's log so far follows)\nhost started, then hangs` |
| 7. A timeout shows the host's log at the time it ran out; before, the log as it was when the request was sent | read | `agent-host/test/support/client.mjs` at `12b355b`: the message was a template string built when `rpc` was called | in both CI failures the message ends right after `initialize`: the log was read at 0 s, empty |
| 7. The whole suite still passes with no margin | node, this machine | `npm test` in `agent-host/` (the main checkout's `node_modules`, same lock file, linked for the run and removed) | 100 of 100 pass, 66 s |
| **8.** A pull request's superseded run is cancelled; a run on `main`, a tag or by hand is not | read + lint | `concurrency` in `ci.yml` and `integration.yml`; `actionlint`; `cargo test --test build_environment_test` (in the gates) | the group is `ci-pr-<number>` / `relay-pr-<number>` on a pull request, with `cancel-in-progress` true only there; any other event's group is its own run id, so it is neither cancelled nor queued behind another. Not proven on GitHub (see "Not verified") |
| **9.** The gates | gates | the five commands of the rules | see "Gates" |

## T7, ten runs in the CI image

This branch, `<scratch>/t7-ci-image-10x.txt`:

| Run | Capture live after | Round trip | Holes in 3 s paced | Streams | T7 took |
|---|---|---|---|---|---|
| 1 | 2226 ms | 20 ms | 0 | 0 / 2 / 0 | 8.07 s |
| 2 | 2225 ms | 20 ms | 0 | 0 / 2 / 0 | 8.08 s |
| 3 | 2225 ms | 19 ms | 0 | 0 / 2 / 0 | 8.08 s |
| 4 | 2326 ms | 20 ms | 0 | 0 / 2 / 0 | 8.19 s |
| 5 | 2327 ms | 20 ms | 0 | 0 / 2 / 0 | 8.16 s |
| 6 | 2326 ms | 20 ms | 0 | 0 / 2 / 0 | 8.17 s |
| 7 | 2325 ms | 20 ms | 0 | 0 / 2 / 0 | 8.18 s |
| 8 | 2225 ms | 20 ms | 0 | 0 / 2 / 0 | 8.06 s |
| 9 | 2325 ms | 20 ms | 0 | 0 / 2 / 0 | 8.19 s |
| 10 | 2325 ms | 20 ms | 0 | 0 / 2 / 0 | 8.15 s |

Before and after, the same step in the same image:

| | T7 | Round trip | Paced holes |
|---|---|---|---|
| `12b355b` (CI, `e144e52`) | 731 s, or never ends | 1342 to 1400 ms | 3 |
| `12b355b`, crate optimised only | 7.5 s | 1381 ms | 0 |
| this branch | 8.1 s | 20 ms | 0 |

## T7 in containers

`scripts/test-audio-containers.sh t7`, three times (`<scratch>/t7-containers-3x.txt`):

| Server | Live after | Round trip | Holes | Streams | T7 took |
|---|---|---|---|---|---|
| PipeWire 1.4.2, Debian | 302–303 ms | 21, 21, 21 ms | 0, 1, 0 | 0 / 2 / 0 | 6.18–6.22 s |
| PipeWire 1.4.11, EL9 | 302–303 ms | 21, 26, 21 ms | 0, 0, 0 | 0 / 2 / 0 | 6.18–6.22 s |
| PulseAudio, Debian (as CI starts it) | 2224–2328 ms | 20, 20, 20 ms | 0, 0, 0 | 0 / 2 / 0 | 8.06–8.18 s |
| none | `huddle audio needs PipeWire or PulseAudio running`, 0.00 s | | | | |

## The agent host on the Intel Macs

CI keeps no time for `initialize` alone. The failing test ("what a package
leaves uncaught…", test 54) is one `initialize` that loads two packages,
then a wait of at most 5 s for a line of the log, so its time is an upper
bound of that `initialize`. Every run of the job on this branch:

| Commit (run) | macos-15-intel: test 54 | its `--check-model` test | suite | ubuntu: test 54 | windows: test 54 |
|---|---|---|---|---|---|
| `543dbd1` (45530273900), macos-15 Apple | 0.8 s | 2.8 s | 80.8 s | 0.9 s | 1.1 s |
| `2ff8301` (45540182641) | 2.5 s | 9.5 s | 101.6 s | 0.9 s | 1.7 s |
| `bfd340f` (45555775783) | 2.2 s | 12.8 s | 111.1 s | 1.5 s | 1.9 s |
| `64156d3` (45560292857) | 31.9 s **failed** | 8.1 s | 110.1 s | 1.1 s | 1.7 s |
| `18c4eb9` (45571201773) | 2.3 s | 8.6 s | 88.9 s | 1.1 s | 1.5 s |
| `9902c74` (45571866191) | 2.1 s | 9.8 s | 97.6 s | 1.0 s | 1.9 s |
| `e144e52` (45577698807) | 5.9 s | 16.6 s | 133.3 s | 1.4 s | 1.8 s |
| `5859f29` (45593677292) | 3.2 s | 10.9 s | 102.4 s | 1.5 s | 1.7 s |
| `24f7486` (45593885608) | 3.9 s | 16.7 s | 119.8 s | 1.3 s | 2.1 s |
| `006d8d1` (45601454766) | 1.4 s | 6.8 s | 80.7 s | 1.3 s | 2.0 s |
| `0d96e80` (45609541408) | 32.7 s **failed** | 14.9 s | 139.8 s | 1.4 s | 1.8 s |
| `12b355b` (45614530167) | 6.5 s | 31.0 s | 170.0 s | 1.0 s | 1.9 s |
| `6fcd428` (45617462153) | 2.6 s | 10.5 s | 104.6 s | 1.1 s | 1.5 s |

So on the Intel Macs the same test takes 2 to 6 times as long as on Linux,
the slowest passing runs are several times the usual (`--check-model`, about
a dozen starts of the host, took 31 s once against 5 s on Linux), and twice in twelve
`initialize` passed 30 s. Whether those two would have answered at 35 s or
never cannot be read from the logs: the message carried the log as it was
at 0 s. From now on it carries the log at the timeout, and an `initialize`
of 5 s or more is written to the test's output.

The bound chosen: `BZZ_AGENT_HOST_WAIT_MARGIN_MS`, added to the client's own
waits (a request's answer, 30 s; the host's exit, 5 s; `--check-model`,
30 s). CI sets 60000 on `macos-15-intel` (so 90 s for `initialize`, three
times the Linux bound, against a worst seen pass of about 6 s for this
test), 30000 on Windows (its one failure, `6fcd428`, was the 5 s wait for
the host's exit in a run where another test took 21 s), 0 on Linux. Bounds
a test states as claims (`stop(2_000)`: "the host leaves within 2 s") get
no margin.

## Gates

| Command | Result |
|---|---|
| `cargo fmt --check` | pass |
| `cargo clippy --locked --all-targets -- -D warnings` | pass |
| `cargo build --locked --example scripted_harness` | pass |
| `cargo test --locked --all-targets` | pass (562 s), `build_environment_test` 2 of 2 |
| `cargo test --locked --no-default-features --lib` | pass (710 tests) |

`Cargo.toml` and `Cargo.lock` did not change: `cargo deny` and `cargo audit`
were not needed.

## Not verified

- The fixes on GitHub's runners: nothing here pushes. The audio job's time
  in CI after the push, the agent-host job on `macos-15-intel`, and the
  `concurrency` groups (a pull request's earlier run cancelled, `main`
  untouched) are shown only once the principal pushes.
- T7 on macOS (BlackHole) and Windows with the settle step: the step only
  waits for audio at the pace of the clock, which BlackHole and WASAPI give
  from the start; not run here.
- Whether the two `initialize` timeouts on the Intel Macs were slowness or
  a hang: the logs of those runs cannot tell (above).
- `cargo run` of bzz itself in a debug build still uses the unoptimised
  crate: a developer's debug huddle on PulseAudio or PipeWire can starve
  its playback as T7 did. Only `Cargo.toml` (`[profile.dev.package.pulseaudio]
  opt-level = 3`) fixes that for every debug build; it is not this task's
  file. Release builds, which people install, read in 0.018 ms and are not
  affected.
