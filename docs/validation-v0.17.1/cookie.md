# v0.17.1 validation: a huddle joins a sound server when no cookie exists

Branch `t3/cookie`, cut from `main` at `c598b08` (the code of tag
`v0.17.0`: `git diff v0.17.0 main -- src Cargo.toml Cargo.lock` is empty).
Linux x86_64, kernel 7.0, 2026-11-28. Method:
[`bzz-prove-it-works`](../../.agents/skills/bzz-prove-it-works/SKILL.md).

`<scratch>` is a folder outside the repository. Every sound server below
ran in a disposable container with `--network none`, no host sound socket
and no `/dev/snd`; every bzz had `PULSE_SERVER` at a container's socket or
at one that does not exist. This machine's own sound was never played
through, recorded from or reconfigured, and its own cookie was not looked
at.

## What was wrong

The PulseAudio protocol starts with `AUTH`: a version and a cookie. bzz
0.17.0 read the cookie from the files libpulse reads (`PULSE_COOKIE`,
`$XDG_CONFIG_HOME/pulse/cookie`, `~/.config/pulse/cookie`,
`~/.pulse-cookie`); **with none of them present it sent an empty cookie**.
Both servers refuse a cookie of any length but 256 before they look at it,
so every join, and `:huddle devices`, said "huddle audio needs PipeWire or
PulseAudio running" with a server running and reachable. Its comment said
that PipeWire ignores cookies: it ignores their content, not their length.

A cookie file exists when some libpulse program (`pactl`, a browser, a
desktop's volume control) or a PulseAudio daemon of the same user has run
in that home: libpulse creates one when it finds none. So it bit where none
had: PipeWire sessions with nothing libpulse in them yet (minimal window
managers, servers, fresh accounts, containers, a bzz with its own `HOME`),
and **WSLg**, whose PulseAudio runs in the system distro and lets anyone in
(`module-native-protocol-unix socket=/mnt/wslg/PulseServer
auth-anonymous=true`, `microsoft/wslg` `WSLGd/main.cpp:531` at `6942bb5`)
while the user's distro may hold no cookie at all. A PulseAudio desktop
writes its cookie in the user's home when the daemon starts, and was not
affected. bzz's own tests did not see it: in each container `pactl`, in the
test's own home, had written a cookie before the test ran.

## What each server does with a cookie

**Claim.** pipewire-pulse takes any 256 bytes and refuses another length;
PulseAudio drops a connection whose cookie is not 256 bytes, even when it
lets anonymous clients in, and otherwise takes its own cookie, or any 256
bytes from an anonymous socket or from a client of its own user. **Rung:**
the socket itself: `<scratch>/probe.py` sends one `AUTH` (protocol 35) and
prints the answer, against servers in containers.

| Cookie sent | PipeWire 1.4.2 | PulseAudio 17.0, anonymous | PulseAudio 17.0, same uid, not anonymous | PulseAudio 17.0, other uid (1234), not anonymous |
|---|---|---|---|---|
| none (0 bytes), what bzz 0.17.0 sent | ERROR 3 (invalid) | connection closed | connection closed | connection closed |
| 255 or 257 random bytes | ERROR 3 | connection closed | connection closed | connection closed |
| 256 zero bytes | accepted | accepted | accepted | ERROR 1 (access) |
| 256 random bytes | accepted | accepted | accepted | ERROR 1 |
| the daemon's own cookie | | accepted | | accepted |
| the daemon's cookie and 44 more bytes | | | | connection closed |

The same with `SO_PASSCRED` set on the probe's socket. PulseAudio's log
for the same-uid case: `Got credentials: uid=1000 gid=1000 success=1`; for
uid 1234: `success=0`, then `Denied access to client with invalid
authentication data`; for a wrong length: `protocol error, kicking client`.

The sources say the same (read in the cache, not run):

- pipewire `src/modules/module-protocol-pulse/pulse-server.c`
  `do_command_auth` (lines 118–145 at `1ea436cf4`): `if (len !=
  NATIVE_COOKIE_LENGTH) return -EINVAL;` (256, `defs.h:29`), then the
  client is authenticated whatever the bytes. The same check is in tags
  0.3.30, 0.3.65, 1.0.0 and 1.4.2: every PipeWire people run.
- pulseaudio `src/pulsecore/protocol-native.c` `command_auth` (lines
  2563–2645 at `v17.0-139-g5ec2aec5`):
  `pa_tagstruct_get_arbitrary(t, &cookie, PA_NATIVE_COOKIE_LENGTH)` fails
  for another length and ends in `protocol_error(c)`, before the check of
  `c->authorized` that an anonymous socket sets. Then, unless anonymous,
  the peer's credentials (`creds->uid == getuid()`) or a `memcmp` with the
  daemon's cookie. The server enables `SO_PASSCRED` on every connection
  (line 5304), and the kernel then passes the client's uid.

## What libpulse does when no cookie exists

`src/pulse/client-conf.c` `pa_client_conf_load_cookie` (lines 169–232) tries
`PULSE_COOKIE`, the X11 root window's `PULSE_COOKIE`, the application's and
`client.conf`'s cookie files, `$XDG_CONFIG_HOME/pulse/cookie` (or
`~/.config/pulse/cookie`), then `~/.pulse-cookie`, and then **creates**
`~/.config/pulse/cookie` with 256 random bytes (`pa_authkey_load(…, true,
…)`; `src/pulsecore/authkey.c` `load` and `generate`, lines 44–123). If it
cannot, it fills the cookie with zeros and connects anyway
(`memset(cookie, 0, cookie_length)`; `context.c:633`, "No cookie loaded.
Attempting to connect without."). It always sends exactly 256 bytes
(`context.c:649`): the first 256 of a longer file, and a shorter file is
rewritten with a new cookie. It also sends its credentials
(`SCM_CREDENTIALS`) with the `AUTH`.

**What bzz does now.** The same lookup in the same order (without X11 and
`client.conf`, as before), the first 256 bytes of each file, a shorter file
treated as no cookie, and with none found **256 zero bytes**. It does not
create a cookie: the plan's rule is that bzz writes nothing outside its own
folders, and a cookie made up on the spot matches no daemon's, so every
server answers zeros as it would answer libpulse's new random cookie (table
above: zeros and random bytes get the same answer from all four). The one
case where libpulse's new file would later matter is a PulseAudio daemon
started afterwards in that home; it then finds the file, and so does bzz.

A daemon that wants its own cookie is not weakened: bzz sends only what it
found, or zeros, and is refused unless the daemon lets that client in on
its own terms (last column).

## The fix in the code

`src/huddle/pulse_devices.rs`: `cookies()` reads as above and takes its
environment as a function (for tests); `connect_to()` sends every cookie
found (or the zeros), notes whether any socket answered, and says
`SOUND_SERVER_REFUSED` ("the sound server refused bzz: check
PULSE_COOKIE", `src/huddle/audio.rs`) when one answered and refused,
`NO_SOUND_SERVER` (unchanged) when none answered. `:huddle devices`
(`src/ui/huddle.rs` `no_devices_reason`) says the same as the join: the
last listing records which of the two it met. The comment no longer says
that PipeWire ignores cookies.

## Proofs

### The handshake against a scripted socket

**Claim.** With no cookie anywhere, bzz gets into a server that answers as
pipewire-pulse does; a long cookie file is sent as its first 256 bytes; a
short one is no cookie; every cookie found is tried; a server that refuses
is named so. **Rung:** unit tests, a Unix socket in a temporary folder
answering with the `pulseaudio` crate's own encoder as each real server
answers (table above).

```sh
cargo test --locked --lib huddle::audio::pulse_devices
```

| Test | With the fix | With 0.17.0's cookie handling put back (empty cookie, whole files) |
|---|---|---|
| `with_no_cookie_anywhere_pipewire_lets_bzz_in` | ok | **FAILED**, `Unsupported("the sound server refused bzz: check PULSE_COOKIE")` |
| `a_cookie_is_read_as_libpulse_reads_it` | ok | **FAILED**, the same |
| `every_cookie_found_is_tried_before_giving_up` | ok | ok |
| `a_server_that_refuses_bzz_is_named_so` | ok | ok |
| `the_sound_server_lines_fit_an_80_column_status_line` (both lines, three modes) | ok | ok |

14 passed in `pulse_devices`; with 0.17.0's message the red rows would
have said "needs PipeWire or PulseAudio running" instead, as the next
proof shows.

### T7 with no cookie anywhere, on real servers

**Claim.** The real device layer opens its streams and carries a tone
through PipeWire, a PulseAudio of the same user and an anonymous PulseAudio
of another user with no cookie in the test's home; a PulseAudio of another
user that wants its cookie refuses and the line says so, and lets bzz in
with its cookie in `PULSE_COOKIE`; bzz writes no cookie. **Rung:** T7
(`tests/huddle_device_loopback_test.rs`) in containers. The script's new
cases: `BZZ_T7_HOME` in `scripts/test-audio-loopback.sh` runs the test with
an empty home, `PULSE_COOKIE` and `XDG_CONFIG_HOME` unset, and fails if a
cookie is there before or after.

```sh
scripts/test-audio-containers.sh t7 <scratch>/bin/t7-before   # main's code
scripts/test-audio-containers.sh t7 <scratch>/bin/t7-after    # this branch
```

T7 built in the CI image (`bzz-build:smoke`, glibc 2.35), `release` profile
with LTO off and 16 codegen units, as `docs/validation-v0.17.0/sound.md`
did.

| Case | `main` (0.17.0's code) | This branch |
|---|---|---|
| PipeWire 1.4.2 Debian, cookie written by `pactl` | ok, 6.4 s | ok, 15 ms round trip, 0 holes |
| PipeWire 1.4.11 EL9, the same | ok | ok, 26 ms, 0 holes |
| PulseAudio, private server as CI runs it | ok | ok, 19 ms, 0 holes |
| no sound server | "needs PipeWire or PulseAudio running" (expected) | the same (expected) |
| **PipeWire 1.4.2, no cookie anywhere** | **FAILED**: "huddle audio needs PipeWire or PulseAudio running" | ok, 21 ms, 0 holes, no cookie written |
| **PulseAudio of the same user, no cookie anywhere** | **FAILED**, the same line | ok, 20 ms, 0 holes, no cookie written |
| **PulseAudio of another user, anonymous, no cookie anywhere** (as WSLg) | **FAILED**, the same line | ok, 19 ms, 0 holes, no cookie written |
| PulseAudio of another user, its cookie needed, no cookie anywhere | the same line (false: a server answered) | "the sound server refused bzz: check PULSE_COOKIE" (expected) |
| PulseAudio of another user, its cookie in `PULSE_COOKIE` | ok | ok, 20 ms, 0 holes |

Exit 1 for `main`, **exit 0** for this branch (`<scratch>/t7-before.txt`,
`<scratch>/t7-after.txt`).

### The published binary

**Claim.** bzz 0.17.0 as published fails the same way. **Rung:** the
published binary on the socket. Not reached here: a join and `:huddle
devices` need a connected community with a channel, so a relay, and the
relay harness was not mine to use. What stands for it: the published
binary's code is this branch's base (the diff above is empty), and the
film's run of the published binary under `strace` showed the same bytes
(`AUTH` with a cookie of length 0, `x\0\0\0\0`, answered `ERROR 3` by
PipeWire 1.4.2).

### Gates

`cargo fmt --check`; `cargo clippy --locked --all-targets -- -D warnings`;
the same with `--no-default-features`; `cargo build --locked --example
scripted_harness`; `cargo test --locked --all-targets` (71 test binaries,
all ok); `cargo test --locked --lib` (1054 passed, 3 ignored); `cargo test
--locked --no-default-features --lib` (727 passed). `Cargo.toml` and
`Cargo.lock` are unchanged.

## Not proven

- **A real desktop session**, PipeWire or PulseAudio, with and without a
  cookie in the home: only containers with null sinks.
- **WSL**: WSLg's configuration was read, not run. The container case
  "PulseAudio of another user, anonymous" is its model.
- A full join on a relay with the fixed binary (the device layer, T7, is
  what a join opens first; the relay part did not change).
- PulseAudio's same-user acceptance relies on the kernel passing bzz's
  credentials because the server asked for them (`SO_PASSCRED` on its end);
  bzz does not send them itself as libpulse does. Seen on kernel 7.0 every
  time; older kernels not tried. Setting `SO_PASSCRED` on bzz's own socket
  would remove the doubt (`rustix`'s `net` feature is already in the build
  through other crates) and was left out of this fix. It matters only for a
  PulseAudio daemon of the same user whose cookie bzz cannot read, which a
  daemon in the user's own home does not have.
- Systems other than Linux x86_64 (aarch64 Linux uses the same code).
