# bzz v0.15.0 validation

**Status:** M0 in progress on branch `v0.15.0` (PR #9). Implemented and proven
below (2026-11-14): M1, the agent runtime in bzz; M2, the Pi Durable host,
on both sides; M3, the agent's disk, with credentials for a private remote on
both sides; M4, creating and managing an agent; M7, hands on this machine, on
both sides; M6, the other harnesses. Not in this document yet: M5 (move). Plan:
[`docs/planning/2026-11-13/v0.15.0.md`](planning/2026-11-13/v0.15.0.md).
Method: [`bzz-prove-it-works`](../.agents/skills/bzz-prove-it-works/SKILL.md).

Buzz pin `bf3bdd0eaefe39289bf9620594f254c7f68897f5`, Rust 1.98.1,
`@earendil-works/pi-durable` 1.5.0. The spikes named below live in
`runs/experiments/agents/pi-durable-harness/spikes/` (local and untracked),
each with the commands to repeat it in `spikes/README.md`.

## M0 — Baseline and the unknowns that can change the plan

### Experiment 1 and 4: one ACP client, four harnesses (2026-11-13)

Linux x86_64, Node 26.5. A minimal ACP client (`m0-acp-client/acp-client.mjs`)
starts a harness, opens two sessions in the same process, gives each a `send`
tool with its own credential, and prompts both at once. The coding agents are
the owner's installed programs with his own sign-ins (D17).

| Harness | Speaks ACP | Two sessions in one process | Each answer sent through the tool, from the right session |
|---|---|---|---|
| bzz's Pi Durable host (stand-in model) | itself | yes | yes, asked over the ACP pipe |
| Grok Build 1.0.77, sandbox profile `workspace` | itself | yes | yes |
| Claude Code 2.1.322 through `claude-agent-acp` 0.101.0 | adapter | yes | yes |
| Codex 0.166.0 through `codex-acp` 2.5.0 | adapter | yes | not reliably |

- **D12 (one process per agent) holds:** every harness kept two sessions
  apart in one process.
- **D15 (one ACP client) holds** for starting a harness, opening sessions,
  prompting and serving the tool.
- **D16 (Codex experimental) is justified:** in three of four runs Codex
  called `send` with `message` where the schema says `text`. Refused with a
  message naming the right argument, it retried correctly once and gave up
  once, ending one turn with plain text and one with nothing.
- **Two rules added to M1 because of this:** bzz checks tool arguments and
  says what is wrong; and a turn that ends without a final `send` has a
  defined outcome. Claude Code writes plain text after it sends, so that text
  is published only when no `send` happened.

Not tried: a shell command or a file edit by a coding harness, a restart with
a harness's saved session, a cancelled turn, macOS, Windows.

### Pending in M0

| Item | State |
|---|---|
| Experiment 2: how Buzz 1.0 shows an agent whose `runtime` it does not know | not run |
| Experiment 3: the role Buzz 1.0 gives an agent it adds to a channel, and whether bzz v0.14 lists that agent | not run |
| Baselines fixed in the repository: the host's `package.json` and lock file, Node detection | `agent-host/package.json` and `package-lock.json` added 2026-11-14 (see M2); Node detection in bzz done 2026-11-14 (see "bzz's side of M2") |
| ADR "local agents" and the agent host protocol | written 2026-11-14: `adr-v0.15-local-agents.md`, `agent-host-protocol.md` |

## M1 — The agent runtime in bzz

Linux x86_64, Rust 1.98.1, Buzz pin `bf3bdd0eaefe`, 2026-11-14. Code at
`c9714d2` on `v0.15.0` (commits `12f7065..c9714d2`). The contract is
[`agent-host-protocol.md`](agent-host-protocol.md); the rules are
[`adr-v0.15-local-agents.md`](adr-v0.15-local-agents.md).

**Where it is.** `src/agents/runtime/` (process, ACP client, admission, tools,
credentials, the `agent-tools` MCP server and its loopback endpoint, one
agent's turns, the host, the relay port and intake, the service the TUI and
`bzz agent run` share), `src/store/agent_runtime.rs` with migration 18,
`[hosted_agents.local]` in `src/config.rs`. No harness of the release exists
yet, so every proof uses the scripted harness `examples/scripted_harness.rs`:
it speaks ACP and follows a script, asking for tools over the pipe as bzz's
own host does, or through the MCP server of `session/new` as a coding harness
does. No test needs Node or a model.

**How to repeat.**

```sh
. runs/env.sh
cargo test --locked --all-features --lib -- agents::runtime store::agent_runtime   # unit
cargo test --locked --all-features --test agent_runtime_test                      # scripted harness, recorded relay
cargo test --locked --all-features --test secret_scan_test
BZZ_BUZZ_SOURCE=$PWD/runs/buzz-pin ./scripts/test-relay.sh                        # pinned relay, Docker
# only the M1 relay test:
BZZ_RELAY_TEST_FILTER=real_relay_local_agent BZZ_BUZZ_SOURCE=$PWD/runs/buzz-pin ./scripts/test-relay.sh
```

**Red runs.** Each "red" below is the named test run against the code with
one change undone or reversed (the change is in the cell), seen failing with
the quoted message, then restored. "Not run" means the test passed and no
such run was made. `T:` is a test of
`tests/agent_runtime_test.rs`; `U:` is a unit test in the named module.

### Configuration, inbox and ledger

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| A v0.13 hosted agent loads unchanged and is never run by bzz; a local agent is the same record with a `local` table; unknown harnesses and settings are refused; paused and moved-away agents do not run | `config_test::hosted_agents_of_v013_load_unchanged_and_local_agents_add_settings`; `T: paused_and_moved_away_agents_never_start` | pausing only sets the label, the process is left: `a paused agent has no process` | pass |
| Migration 18 applies to a fresh and to an older database | `store_migration_test` (12, schema 18) | not run | pass |
| A request is admitted once and handed over once; the first final reply wins and a later one, however worded, gets it back; a refused reply is discarded only while unpublished | `U: store::agent_runtime` (5) | `INSERT OR REPLACE` in the claim: `assertion failed: !second.fresh` | pass |

### Processes

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| The harness's environment is `PATH`, `HOME`, `LANG`, `TMPDIR` and nothing of bzz's own; it runs with an ownership record (pid, start time, boot id, owner) | `U: process::the_environment_is_built_from_nothing`; `T: the_harness_gets_a_built_environment_a_record_and_a_group_stop` | without `env_clear()`: `left: ["AI_AGENT", "CARGO", "CARGO_BIN_EXE_bzz", …] right: ["HOME", "LANG", "PATH", "TMPDIR"]` | pass |
| Stop is end of input, then TERM, then KILL, on the whole group: a harness deaf to both dies, and so does what it started | `U: process::a_harness_leaves_on_end_of_input_and_its_record_goes`, `a_deaf_harness_and_what_it_started_are_killed_as_a_group` | without `process_group(0)`: the process it started survives (`what it started is gone too`) | pass |
| At startup a harness whose bzz is gone is stopped with its group; one owned by a live bzz is left alone and not started here; a stale or torn record is removed | `U: process::the_sweep_stops_a_leftover_whose_bzz_is_gone_and_spares_a_live_one`; `T: an_agent_run_by_another_live_bzz_is_not_started` | sweep that does not stop: `the leftover was stopped`; owner check off: `left: [Stopped …] right: [OwnedElsewhere …]` | pass |
| The sweep runs before recovery, and recovery before any start | `T: at_startup_leftovers_are_stopped_before_recovery_and_recovery_before_any_start` | no sweep in `AgentHost::start`: `left: [] right: [Stopped { … }]` | pass |

### The ACP client

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| Framing: requests, notifications and answers are told apart; anything else on the line is ignored; a message is one line; answers reach their requests in any order | `U: acp` (8) | next row | pass |
| A line over 4 MiB is skipped without being held; a harness that does not read is reported, not waited for; a closed harness ends every waiting request | `U: acp::an_oversized_line_is_skipped_and_the_next_one_is_read`, `a_harness_that_does_not_read_is_reported_not_waited_for`, `a_closed_harness_ends_every_waiting_request` | no line limit: `left: 0 right: 1` (lines skipped) | pass |
| `initialize`, `session/new` and `session/prompt` carry exactly what the protocol document says, the place and the request as fields | `T: a_mention_is_answered_once_through_the_tool_over_the_pipe` | request field dropped: `left: Object {} right: Object {"bzz/request": …}` | pass |
| Several sessions run at once in one process | `T: several_sessions_run_at_once_in_one_process` (each turn waits for the other to be live) | one turn at a time: `left: ["I could not answer in time, and stopped."] right: ["answer beta"]` | pass |
| A message for a busy conversation waits and becomes the next prompt of the same session | `T: a_message_for_a_busy_conversation_is_the_next_prompt_of_the_same_session` | no busy check: `left: Replied right: Queued` | pass |
| An unknown request gets `-32601`; an unknown notification and an unknown update are ignored; a permission is allowed once for bzz's chat tools and refused for anything else | `T: unknown_requests_are_refused_unknown_notifications_ignored_permissions_ruled`; `U: agent::only_the_chat_tools_are_allowed_and_only_once` | answering unknown requests: `left: Null right: -32601`; no kind check: a shell command titled `mcp__buzz__send` is allowed | pass |
| A flood (120,000 updates, 500 lines that are no message, one 6 MB line) neither blocks bzz nor is kept: another conversation is answered meanwhile and the kept text is bounded | `T: a_flooding_harness_does_not_block_bzz`; `U: agent::the_last_plain_text_is_the_message_after_the_last_tool_call` | a line that is no message closes the connection: `left: Failed right: Replied`; no bound: `held in memory left: 48000 right: 16000` | pass |
| A harness that does not answer `initialize`, or answers another version, is stopped and counted | `T: a_harness_that_stalls_at_start_is_stopped_and_counted` | another version accepted: `left: Some("no-reply") right: Some("unavailable")` | pass |
| For another harness the session id is stored and `session/load` used after a restart; a refused load gets a new session; a credential is never reused | `T: a_stored_session_is_loaded_after_a_restart_when_the_harness_offers_it` | no load: `session/new` count `left: 2 right: 1` | pass |
| bzz's own host is asked to `_bzz/checkpoint` when idle and on request, never during a turn; another harness never | `T: bzzs_own_host_is_asked_to_checkpoint_when_idle` | no idle task: `once, after it went idle left: 0 right: 1` | pass |

### Tools and `bzz agent-tools`

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| Arguments are checked and a wrong call is answered with the right name; `message` and `content` stand for `text`; no argument can name the request, the place or the session | `U: tools` (3); `T: tools_check_their_arguments_and_act_only_in_the_place` | unknown arguments accepted: the guessed `msg` is answered as a missing `text` and never named | pass |
| `read` returns the conversation newest last; `react` reacts to the request by default and only to messages of the place | `T: tools_check_their_arguments_and_act_only_in_the_place` | not run | pass |
| A second final `send` returns the stored reply and publishes nothing; after the final reply the turn posts nothing more | `T: a_harness_that_sends_twice_yields_one_reply` | `INSERT OR REPLACE` in the claim: `left: ["first wording", "second wording"] right: ["first wording"]` | pass |
| A turn posts only so much besides its final reply | `T: a_turn_posts_only_so_much_besides_its_final_reply` | no limit: `left: 6 right: 3` | pass |
| `bzz agent-tools` lists the three tools and passes calls to bzz; the target must be a loopback address | `U: mcp` (2) | not run | pass |
| Another harness gets the server in `session/new` with one credential per session; a call acts only in the calling session's live turn; a credential bzz never made, a v0.13 broker credential, and every credential after the process exits get 401 | `U: credentials` (2); `T: another_harness_calls_bzz_agent_tools_with_a_credential_per_session` | turn taken from any session: the "sneak" is sent (`left: Null right: true`); no revocation at exit: `no credential outlives the process` | pass |

### Admission, delivery and how a turn ends

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| The owner always wakes the agent, others only by its rule; a DM is the owner's alone and needs no mention; the agent never wakes itself; a forged author is not a message | `U: admission` (4); `T: only_an_author_who_passes_the_rule_wakes_the_agent` | owner-only lets anyone pass: `left: Queued right: Refused(AuthorNotAllowed)`; DM author unchecked: `left: Ok(Admitted …) right: Err(DmNotOwnerOnly)` | pass |
| The place is the channel for a DM or a Solo chat and the thread otherwise; a reply follows it (flat in a DM) | `U: admission::the_place_is_the_channel_or_the_thread_and_replies_follow_it`; same `T` | not run | pass |
| The prompt says who, where, what and what came before; a coding harness also gets the instructions; a resumed turn gets the warning | `U: prompt` (1); `T: killed_before_…` | not run | pass |
| The agent's typing indicator is published in the request's thread while the turn runs and stops with the final reply | `U: typing::a_built_indicator_parses_in_its_scope`; `T: a_mention_is_answered_once_…` | no typing task: `a typing indicator was published` | pass |
| With no `send`, the last plain text is published once through the ledger | `T: a_turn_without_a_send_publishes_the_last_plain_text_once` | text not published: `left: Failed right: Replied` | pass |
| Plain text is never published when a `send` happened, final or not | `T: plain_text_is_not_published_when_a_send_happened` | the check off: `left: (Replied, None, 1) right: (Ended, None, 1)` | pass |
| A failed turn posts one notice named by kind (no reply, refused, sign-in needed, limit reached, provider error) and the harness's own words never leave bzz | `U: runtime::failures_are_named_by_kind_from_code_and_wording`; `T: a_turn_that_fails_posts_one_notice_named_by_kind` | ACP's sign-in code ignored: `left: (Failed, Some("provider-error")) right: (Failed, Some("sign-in-needed"))` | pass |
| A turn has a time limit: a harness that honours the cancel keeps its process and session, one that does not is stopped | `T: a_turn_has_a_time_limit` | no limit: `request never settled` | pass |

### After a crash

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| Killed before the publish: the request is handed over again with a warning and answered once, also by a harness that cannot tell a request | `T: killed_before_the_publish_the_request_is_handed_over_again` | never handed over again: `left: (Failed, 1) right: (Replied, 2)` | pass |
| Killed during the publish (the relay holds it while the harness is killed): one reply, not handed over again | `T: killed_during_the_publish_the_reply_is_published_once` | reply stored only after the publish (the spikes' order) and the turn not waiting for it: `left: ["the answer", "a second answer"] right: ["the answer"]` | pass |
| Killed after the publish: nothing is handed over or published again | `T: killed_after_the_publish_nothing_is_published_again` | a stored reply does not settle the request: `request never settled` (handed over again) | pass |
| After something of the turn was published, a harness that cannot tell a request is not handed it again and the turn ends with a notice; bzz's own host continues | `T: killed_after_a_progress_message_only_a_harness_that_knows_the_request_continues`, `recovery_ends_a_doubtful_request_with_a_notice_and_starts_nothing` | always handed over again: `left: (Replied, None, 2) right: (Failed, Some("interrupted"), 1)` and `left: (Queued, None) right: (Failed, Some("interrupted"))` | pass |
| After a crash of bzz the stored reply is published again byte for byte before any harness starts, whether the relay had it or not, and the relay keeps one | `T: after_a_crash_of_bzz_the_stored_reply_is_published_again_unchanged` | recovery does not publish: `Lost: exactly one on the relay left: [] right: ["the answer"]` | pass |
| A harness that keeps dying is started a fixed number of times (three in the test, five by default), then left alone until five minutes have passed; its requests get one notice each | `T: a_harness_that_keeps_dying_is_not_restarted_forever` | breaker off: `request never settled` | pass |

### Where it runs, and the key rule

| Claim | Proof | Red without the change | Result |
|---|---|---|---|
| In the TUI the agents run while the owner is unlocked, beside the broker, which does not serve them; `:broker` shows a status line each; space pauses one; locking stops them | `app::tests::local_agents_run_while_the_owner_is_unlocked_and_stop_on_lock`, `broker_panel_shows_a_status_line_per_local_agent` (TestBackend) | broker serves local agents: `the broker does not serve a local agent`; no stop on lock: `assertion failed: app.local_agents.is_none() && app.runtime.is_none()` | pass |
| No agent key, owner key or broker credential is in a harness's environment, arguments, messages, tool results or logs, nor in what bzz wrote for it; the session credential is in the server entry of `session/new` and nowhere else | `secret_scan_test::no_key_reaches_a_harness_environment_arguments_or_logs` (both tool paths, a key sealed on disk) | without `env_clear()`: `pi-durable environment and arguments left: ["AI_AGENT", "CARGO", …]` | pass |

### On the pinned relay

`real_relay_local_agent_answers_a_mention_exactly_once`, in
`tests/relay_integration.rs`. The profile is made by the bzz binary (`identity
new`, `broker create`); the agent is a plain channel member with no `bot`
role; its requests come from its own relay connection.

| Claim | Observed | Result |
|---|---|---|
| A mention is answered once; the reply is the agent's, in the mention's thread, with a valid owner tag; the relay delivers the agent's typing indicator to a subscriber | `plain answer`, one reply, kind 20002 from the agent | pass |
| Harness killed before the publish | one reply, from the second attempt | pass |
| Harness killed during the publish (it sends and exits without waiting) | one reply | pass |
| Harness killed after the publish | one reply, the first | pass |
| A harness that sends twice | one reply, the first wording | pass |
| The owner's DM is answered flat without a mention; a stranger's mention is not answered (owner only) and leaves no inbox row | one message in the DM; none for the stranger | pass |
| A reply stored and on the relay but not recorded as published, and one stored and never sent, are both published again unchanged at the next start; the relay holds each once; recovery starts no harness | one event per id, identical JSON, `published` set | pass |
| `bzz agent run` answers a mention without the TUI | one reply | pass |
| bzz itself killed (SIGKILL) during a turn: the harness outlives it; the next `bzz agent run` stops it first, then the request is answered once | `stopped a harness left by an earlier bzz (process N)`, one reply | pass |
| Interrupted, `bzz agent run` stops its harness and leaves no record | exit 0, no process, no record | pass |
| Nothing a harness saw holds the agent key, the owner key, the passphrase or the relay address | scan of both harness logs and the agent's folder | pass |
| The intake subscribes a few times, not in a loop | 1 to 4 `REQ` frames in the relay's own log | pass |

Red on the relay, two runs:

- **The ledger off** (`INSERT OR REPLACE` in the claim):
  `sent twice: ["the first wording", "the second wording"]`, `left: 2 right: 1`.
- **A bug the test found.** The first run failed with
  `Network("rate-limited: quota exceeded; retry in 17s")`. The relay sends a
  stored membership notice (kind 44100) again with every new subscription,
  and the intake subscribed again on each: two `REQ` frames and eight HTTP
  queries a second until the relay answered 429. A notice is now acted on
  once (`U: relay::a_membership_notice_is_acted_on_once`), reasons to
  subscribe again are joined, and two subscriptions are never closer than
  two seconds. The test's own polling was slowed too: the relay allows a
  person 300 HTTP calls a minute.

### Gates after M1

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --check` | clean |
| Lint | `cargo clippy --locked --all-targets --all-features -- -D warnings` | clean (also `--no-default-features`) |
| Tests | `cargo test --locked --all-features` | 576 passed, 0 failed, 24 ignored; 70 passing tests are new (40 unit, 28 scripted harness, 1 configuration, 1 secret scan) |
| Pinned relay | `BZZ_BUZZ_SOURCE=$PWD/runs/buzz-pin ./scripts/test-relay.sh` | 17 passed (the 16 of v0.14 and the one above) |
| Supply chain | `cargo deny check`; `cargo audit` | ok; the 4 allowed warnings of v0.14. `Cargo.lock` is unchanged: tokio gains its `process` and `io-std` features, and rustix, already locked, gains `process` |

`agent_runtime_test` was also run 100 times, ten at once, with no failure.

### Not proven in M1

| Item | State |
|---|---|
| macOS and Windows | not compiled or run here (only the Linux target is installed). The Windows path (`taskkill /T`, a start time read through PowerShell) and the macOS path (start time from `ps`) are written and untested |
| A real harness | when M1 was written none existed in this tree. The Pi Durable host has since been joined to the runtime ("bzz's side of M2"); the coding harnesses are M6 |
| The release TUI through Herdr ("the agent panel shows a turn") | not run. The TUI path is proven by the two `app::tests` above, and what it calls (`service::serve`) on the relay |
| A federated-identity community | the agents enroll as the broker's do (same code); `./scripts/test-relay.sh --nip-fi` has no local agent in it |
| Which permission titles the real adapters use for bzz's tools | the rule is tested with `mcp__buzz__send` and three other spellings; a harness that asks under another name is refused, and its turn then ends as plain text |
| `session/load` with a real adapter | scripted only |
| The pause toggle and community switch in the running TUI | the toggle is covered by the `app` test; a switch of community with agents running is not |
| Growth over time | closed in M4 for inbox rows and stored sessions ("What M1 left open"). A harness process still keeps every session it opened until it is restarted |

## M2 — The Pi Durable host

**Scope of this entry:** the host itself, the Node program in `agent-host/`
(`docs/agent-host-protocol.md`, sections 2 to 5 and 7). bzz's side of M2, the
install on first use and the tests "against bzz itself", is not part of it.

**Run on:** Linux x86_64, commit `e7dbba3`, 2026-11-14. No network, no real
model, no relay, no Buzz: a scripted client stands in for bzz, a scripted
module for the model (`BZZ_AGENT_HOST_SCRIPTED_MODEL`), and a script for the
`buzz` command.

```sh
cd agent-host && npm ci --ignore-scripts && npm test
```

| Node | Result |
|---|---|
| 26.5.0 | 78 tests, 78 pass, 0 skipped, 39 s |
| 22.19.0, the minimum (official build, run from a temporary folder) | 78 pass, 0 skipped |
| 24.15.0 | 78 pass, 0 skipped |

Also green with `npm ci --ignore-scripts --omit=dev --omit=optional` (176
packages instead of 204). `npm audit --omit=dev`: 0 advisories.

**Red runs.** Every test was seen failing: the fault named below was planted
in the source, the test was run alone (`node --test --test-name-pattern=…
test/<file>`), failed as shown, and the fault was removed. 55 red runs for M2
and M3 together, all on `e7dbba3`. Pi Durable's suite counts as one test here.

| Claim | Test (`agent-host/test/`) | Fault planted | How it failed |
|---|---|---|---|
| `initialize` answers with the host's version and `"tools": "pipe"`; an unknown method gets `-32601`, a bad line `-32700`, a missing place or request `-32602` | `acp.test.mjs` "initialize answers as bzz's own host…" | no `_meta` in the answer | `_meta` is `undefined` |
| One durable conversation per place, found again whatever the session id and after a restart | `acp.test.mjs` "one conversation per place…" | conversations keyed by session id | counts `1, 1, 1` where `1, 2, 1` was expected |
| The request is the idempotency key: handed over again, at once or in the next process, it starts no work | `acp.test.mjs` "the same request handed over again…" | no lookup of the request and no `requestId` | `3 !== 1` tool calls |
| Several sessions at once in one process, each tool call on its own session | `acp.test.mjs` "two sessions at once…" (one turn's send is held until the other's arrives) | every `_bzz/tool` sent with the first session | the second turn never ends: timeout |
| A turn cut off by a kill continues in the next process, and its tool calls wait for the session bzz opens there | `acp.test.mjs` "a turn cut off by a kill…" (host killed inside `_bzz/tool`) | a tool call without a session fails instead of waiting | no `send` reached the client: `[]` |
| `session/cancel` ends the turn and the prompt answers `cancelled`, also inside an unanswered tool call | `acp.test.mjs` "session/cancel stops the turn…" | the notification does nothing | timeout waiting for the cancelled prompt |
| `_bzz/checkpoint` leaves `memory.sqlite` and `files.sqlite` with no write-ahead file, waits for a running turn, and the host goes on | `acp.test.mjs` "_bzz/checkpoint leaves both files whole…" | storage is not closed | `files.sqlite-shm` and `-wal` still in the folder |
| A checkpoint does not hang on a cut-off turn nobody asked for again | `acp.test.mjs` "a checkpoint does not wait…" | the waiting tool call ignores the stop | `_bzz/checkpoint` unanswered after 10 s |
| The host leaves within 2 s of the end of its input, with nothing beside its two files | `acp.test.mjs` "the host leaves within 2 s…" (observed: 14 to 19 ms) | the process stays after closing | timeout after 2000 ms |
| The host leaves when its parent is gone although its input is still open | `acp.test.mjs` "the host leaves when its parent is gone…" (a second process holds the pipe) | the parent is never checked | the orphan is still alive after 6 s |
| A provider that is not available fails the turn with a message that says so; so do a missing model, a missing sign-in and a `!command` key. Pi's default model is used when bzz names none | `acp.test.mjs` "a model that is not available…" | no check before the turn | `The turn failed: no_model` instead of the reason |
| A provider error during a turn fails the prompt with its text; the next mention works | `acp.test.mjs` "a provider that fails during the turn…" | a failed turn answers `end_turn` | no error: code `undefined`, not `-32000` |
| Pi's `read`, `write`, `edit` and `bash` work on the agent's disk; a commit made through `bash` is in the repository | `acp.test.mjs` "Pi's read, write, edit and bash…" | Pi's tools not installed | `Tool bash is not available` |
| bzz is told of tool calls (`tool_call`, `tool_call_update`) and gets the turn's plain text (`agent_message_chunk`) | same test | no update for an answer's text | the last update is a tool update, not the text |
| The stand-in model exists only when its environment variable is set | `model.test.mjs` "Pi's defaults, and the model of a turn" | always added | the provider `scripted` exists without the switch |
| Reading Pi's login leaves Pi's folder as it was | `model.test.mjs` "a read of Pi's login…" | the folder is created on read, as Buzz's host does | the folder exists |
| A refreshed token is written under Pi's lock | `model.test.mjs` "a refreshed token…" | no lock taken | the write finished while `pi` held the lock |
| A `!command` or `$NAME` key is refused, with what to do | `model.test.mjs` "a key that Pi would get by running a command…" | the key is used as it is | no refusal |
| The recorded prompts of the pinned `buzz-acp` are read as `expected.json` says | `remote.test.mjs` "the recorded prompts…" | `Event Id:` for `Event ID:`, as if Buzz had reworded | `01-top-level-mention.txt: no place or request found. Did Buzz change its prompt?` |
| What people write cannot move the place: a channel name with an id in it, a description and a message that imitate fields | `remote.test.mjs` "what people write cannot move the place…" | the first id on the channel line is taken, not the last | the place is in the other channel |
| Remote copy: one conversation per thread across restarts, a mention handed over twice answered once, replies posted with `buzz --reply-to` | `remote.test.mjs` "under buzz-acp…" | no idempotency key | `the replayed mention was not answered again: 2 !== 1` |
| The key Buzz puts in the environment reaches `buzz` and is in neither the agent's shell, the log, the pipe nor `buzz`'s arguments | `remote.test.mjs` "the key Buzz puts in the environment…" | the shell is given the process's environment | the shell printed it |
| `read` and `react` go through `buzz`; a failure of `buzz` reaches the model as an error | `remote.test.mjs` "read and react go through buzz too…" | a failure is answered as success | no `failed` tool update |
| A remote copy checks tool arguments and names the right one; `message` and `content` stand for `text`; a second final send posts nothing | `remote.test.mjs` "the chat tools of a remote copy…" | only `text` is read | `send needs text` for `message` |
| Without `agent.json` the host says what is missing; a prompt that is not buzz-acp's is refused | `remote.test.mjs` "without agent.json…" | a missing file is read as empty | no refusal |
| A move: a conversation and a file made under bzz continue under `buzz-acp` from copies of the two files; the mention answered before is not answered again | `remote.test.mjs` "a move…", "places are named as bzz names them…" | the remote copy names places differently | the DM's place differs; the moved conversation is not found |
| No path of the machine is kept: not buzz-acp's working directory, not a granted folder's real path in an error | `remote.test.mjs` "under buzz-acp…", `acp.test.mjs` "grants and the allow-list…" | the working directory is kept; errors carry the real path | `memory.sqlite holds /tmp/bzz-agent-host-…` (both) |
| `BZZ_AGENT_HOST_RECORD` saves buzz-acp's prompts byte for byte | `remote.test.mjs` "BZZ_AGENT_HOST_RECORD…" | nothing is saved | the folder is not there |

**The plan's exit gate** ("S2, S3 and S12 as tests against bzz itself: resume
after a kill, one reply, memory across turns and restarts"):

- [x] The host's half, against a scripted client: resume after a kill, the
  same request once, memory across turns and restarts.
- [x] Against bzz itself, on the pinned relay: see "bzz's side of M2" below
  (2026-11-14).
- [x] "One reply per mention" is bzz's rule, not the host's. The host only
  makes it cheap: a request handed over again starts nothing. Shown with the
  real host under a kill, below.
- [ ] S12 with a real model through this host: not run. It needs the owner's
  Pi login, which no test may read.

**Not proven here:**

- macOS and Windows. The CI job `agent-host` runs the same tests there; it
  has not run yet. On Windows the test of "the parent is gone" is skipped, so
  that path is unproven there by design of the test.
- A token refresh against a real provider, and any real model.
- The real `buzz-acp` and `buzz` at the pin with this host (spike S10 did it
  with the prototype). Only recorded prompts and a stand-in `buzz` ran.
- Prompts of `buzz-acp` for a DM, a batch of events and an edit were not
  recorded: the reading of the first two is tested with text built from
  Buzz's source at the pin. An edit is not handled: the reply goes to the
  edit event, where Buzz asks for the original.
- A remote copy records its final reply after `buzz` returns. A kill between
  the two can repeat a reply. Under bzz, bzz's ledger closes that.
- Buzz does not escape message text in its prompt. A message that imitates a
  whole event header can make the remote copy take another event of the same
  channel as the request (the test above shows it). The channel cannot be
  moved that way.

### bzz's side of M2: the host inside the binary, its install, the default launch

Linux x86_64, Rust 1.98.1, Node 26.5.0, Buzz pin `bf3bdd0eaefe`, 2026-11-14.
Commits `fc1ac6e` and `54652aa` (written by one agent and checked, red runs
included, by the next), with `41b5f6d` and `0e82b34`.

**Where it is.** `src/agents/runtime/install.rs` (the embedded files, Node and
npm, the install), `Shared::pi_host` and `Agent::launch` in
`src/agents/runtime/agent.rs` (the default launch), `tests/agent_host_test.rs`
and `tests/support/pi_host.rs`.

**How the stand-in model is switched on.** The host adds its scripted model
only when `BZZ_AGENT_HOST_SCRIPTED_MODEL` is in its environment, and bzz
builds that environment from nothing. So a test gives bzz, as `BZZ_NODE`, a
two-line program that sets the variable and runs the real Node. bzz never sets
it: the test reads the running host's environment from `/proc` and finds
`HOME`, `LANG`, `PATH`, `TMPDIR` and the one variable the stand-in added.

**How to repeat.**

```sh
. runs/env.sh
cargo test --locked --all-features --lib -- agents::runtime::install   # stand-ins for node and npm; no Node needed
cargo test --locked --all-features --test agent_host_test              # the real host, a recorded relay
BZZ_RELAY_TEST_FILTER=real_relay_agent_new BZZ_BUZZ_SOURCE=$PWD/runs/buzz-pin ./scripts/test-relay.sh
```

`agent_host_test` and the relay test need Node.js 22.19 or newer and, the
first time, the npm registry (npm's cache is then kept under `target/tmp`).
Without Node they print `SKIPPED …: it runs bzz's real agent host, which needs
Node.js 22.19 or newer` and pass; with `BZZ_TEST_REQUIRE_NODE=1`, which the
relay job in CI sets after installing Node, they fail instead.

`U:` is a unit test of `agents::runtime::install`; `H:` is a test of
`tests/agent_host_test.rs`. Each red is the named test with the one fault in
the cell planted, seen failing as quoted, then removed.

| Claim | Proof | Red with the fault | Result |
|---|---|---|---|
| Every file the host reads at run time is inside the bzz binary. A file of `agent-host/` that is neither embedded nor on the short list of files not needed at run time fails the test | `U: every_file_of_the_host_is_embedded_or_named_as_not_needed` | `src/vfs.mjs` taken out of the embedded list: `left: [… "src/rpc.mjs", "src/vfs.mjs"] right: [… "src/rpc.mjs"]`. First seen without planting anything: `.gitattributes` and `.gitignore` were in the folder and on neither list | pass |
| Node is the first on `PATH` that is 22.19 or newer, or the one `BZZ_NODE` names; a missing, too old or unusable one is named with what to do | `U: node_is_the_first_on_path_that_is_new_enough`, `versions_are_read_as_node_prints_them` | any version accepted: `unwrap_err() on an Ok value: Node { program: "…/old/node", version: (20, 11, 1) }` | pass |
| The install writes the embedded files and runs `npm ci --ignore-scripts --omit=optional --omit=dev`, with nothing of bzz's in npm's environment; two bzz processes installing at once both end with one whole install; an installed host is not installed again | `U: the_host_is_installed_once_and_only_a_whole_install_counts` | without `--ignore-scripts`: `left: "ci --omit=optional --omit=dev --no-audit --no-fund"`; the loser of the rename not tolerated: `Io { … why: "Directory not empty (os error 39)" }`; npm started without a cleared environment: the test prints npm's environment, with `CARGO_PKG_REPOSITORY=…` in it | pass |
| A half install is never used: `npm` failing, `npm` never finishing, and what a killed bzz leaves are each not an install, and leave nothing behind; the error names the log and says to try again | `U: a_failed_or_interrupted_install_is_never_used` | a folder counts without its marker: `a folder without its marker is no install` | pass |
| Installs of other versions are removed once no agent of the data folder runs, and not before | `U: installs_of_other_versions_go_once_no_agent_runs` | removed while one runs: `assertion failed: remove_unused_installs(&data).is_empty()` | pass |
| Pi's folder is found as Pi finds it (`PI_CODING_AGENT_DIR`, `~`) | `U: pis_folder_is_found_as_pi_finds_it` | not run | pass |
| The default launch of `pi-durable`: nothing is installed before an agent needs it; the first mention installs the host and starts `node <host>/main.mjs <agent folder>` with a built environment; it is answered once | `H: the_real_host_is_installed_started_and_answers_once_across_kills_and_restarts` | no default launch (the state of M1): `left: (Failed, 0) right: (Replied, 1)` | pass |
| The host is killed in the middle of a turn: the request is handed over again to a new host and the turn ends with one reply | same | never handed over again: `handed over again to the new process left: (Failed, 1) right: (Replied, 2)` | pass |
| After bzz and the host have both restarted, a second message in the same thread shows the conversation's memory | same | `memory.sqlite` removed at each start: `left: ["pong: this conversation holds 1 message(s) from people"] right: ["pong: … 2 …"]` | pass |
| A file the agent wrote on its disk is still there after that restart, and never was on the machine | same | `files.sqlite` removed at each start: `["cat: /home/agent/note.txt: No such file or directory …"]` | pass |
| Without a Node that is new enough the agent is not started, its status says what to do, whoever asked is told once, and nothing is installed | `H: without_node_the_agent_says_what_to_do` | the reason dropped: `the status says what to do: installing the Pi Durable host (first use)` | pass |
| `initialize` carries the grants as the host expects them: a granted folder is read under `/mnt/<name>`, and one granted read-only is not written | `H: the_real_host_keeps_to_the_grants_and_names_its_failures` | the grants not sent: `["cat: /mnt/site/hello.txt: No such file or directory …"]`; every grant sent as writable: `a folder granted read-only is not written left: "changed\n" right: "hello from the machine\n"` | pass |
| A failure the host names is posted by its kind: a provider's error as that, a model that cannot be used as that, with none of the provider's words | same; `agent_runtime_test::a_failure_bzzs_own_host_names_is_read_from_its_kind`; `U: runtime::failures_are_named_by_kind_from_code_and_wording`, `acp::frames_are_told_apart…` | the kind ignored: `left: (Failed, Some("sign-in-needed")) right: (Failed, Some("provider-error"))`, with the real host and with the scripted one. **This was a bug found by joining the two parts**: the host uses -32000 for every failed turn, which is ACP's code for "sign in", so every failure of the real host was posted as "my owner needs to sign in" | pass |
| The check of a model before an agent exists is the installed host's own: available, a provider Pi does not have, a model it does not have, and no sign-in are told apart, and Pi's folder is not even created | same | the check always says yes: `Some(AgentModelConfig { provider: "nope", … }): Available { … }` | pass |

**On the pinned relay** (`real_relay_agent_new_to_delete_on_the_pi_durable_host`
in `tests/relay_integration.rs`, 2026-11-14, 103 s alone and green in the full
suite). The agent is made by `bzz agent new` and added by `bzz agent add`; bzz
is the binary, run as `bzz agent run`; the host is the real one, installed by
that binary on first use, on the stand-in model.

| Claim | Observed | Result |
|---|---|---|
| A mention is answered once by the real host, with the owner's tag | `pong: this conversation holds 1 message(s) from people`, one reply; the host's command line is `node <data>/agent-host/<id>/main.mjs <agent folder>` | pass |
| The host is killed (SIGKILL) while its turn is running: the turn ends once | the request is `delivered`, the host is killed, one reply `answered after the host was killed`, the request `replied` at attempt 2, a new process | pass |
| bzz and the host both restart; a second message in the same thread shows the memory | `pong: this conversation holds 2 message(s) from people` | pass |
| A file the agent wrote on its disk is still there after the restart, and is not on the machine | `kept across restarts` read back; no `note.txt` in the agent's folder | pass |
| In a Solo chat the answer is flat, in the chat | one message, no thread reference | pass |
| Nothing the host saw or wrote holds the agent's key, the owner's key, the passphrase or the relay's address | scan of `harness.log`, `memory.sqlite`, `files.sqlite` and every other file of the agent's folder | pass |

Red on the relay: a turn cut off by the kill never handed over again
(`RR3` below, under M4).

**Not proven for bzz's side of M2:**

- macOS and Windows: not compiled or run here. `agent_host_test` and the relay
  test are Unix only (they kill processes and write a shell stand-in for
  `node`). On Windows the install looks for `node.exe` and `npm.cmd`; that
  path is written and has never run.
- A real model (S12 as a test). No test may read the owner's Pi login.
- The install against the real npm registry behind a proxy, offline, or with a
  full disk. The failure paths are tested with a stand-in `npm`.
- The relay job in CI with Node mounted into its container: the workflow is
  written (`.github/workflows/integration.yml`) and has not run yet.
- `_bzz/checkpoint` with the real host after 60 s idle: scripted only (M1).

### Pi packages, and what a moved agent carries (second pass)

**Scope:** providers and extensions from the Pi Durable entries of Pi
packages (`agent-host/src/packages.mjs`, protocol section 10), the switch
`_meta.bzz.packages`, and `network` and `packages` in `agent.json` (protocol
sections 1 and 7). bzz's side, which sends the switch and writes `agent.json`,
is not part of it.

**Run on:** Linux x86_64, commit `3658e52`, 2026-11-14. As above: no
network, no real model, no relay, no Buzz. Pi's folder and every package are
written into temporary folders by the tests; the real `~/.pi` is never read.

```sh
cd agent-host && npm ci --ignore-scripts --omit=optional --omit=dev && npm test
```

| Node | Result |
|---|---|
| 26.5.0 | 98 tests, 98 pass, 0 skipped, 49 s |
| 22.19.0, the minimum (official build, checksum verified, run from a temporary folder) | 98 pass, 0 skipped |
| 24.15.0 | 98 pass, 0 skipped |

176 packages are installed that way. `npm audit --omit=dev`: 0 advisories.
The 98 are the 78 above and 20 new: 6 in `packages.test.mjs`, 1 in
`remote.test.mjs`, 4 in `credentials.test.mjs` (M3) and 9 in `run.test.mjs`
(M7).

**Red runs**, as above: the fault planted, the one test run alone, the
failure read, the fault removed, all on `3658e52`. 53 red runs for the second
pass: 16 here, 10 in M3 and 27 in M7.

| Claim | Test (`agent-host/test/`) | Fault planted | How it failed |
|---|---|---|---|
| A package's Pi Durable entry brings providers, and they join Pi's own | `packages.test.mjs` "providers and extensions come from…" | the providers of packages are not added to Pi's | `The provider "acme" is not available: it is not one of Pi's built-in providers, and no Pi package loaded for this agent adds it` |
| A package gets the host's copies of the `@earendil-works` packages and installs none itself | same test | no redirection of those imports | the report says of `acme`: `Cannot find package '@earendil-works/pi-ai' imported from …/acme/durable.ts` |
| A package may be TypeScript, also in a dependency under its own `node_modules` | same test | types are not stripped there | the report says of `guard`: `Stripping types is currently unsupported for files under node_modules` |
| A package that hangs, at import or in its setup, is left out after its time limit, and two that hang cost one limit | `packages.test.mjs` "a package that fails is left out…" (limit 1.5 s; observed 1.5 s for both) | no time limit | the test never ends: stopped after 15 s |
| A package that throws, returns what cannot be installed, or is not in a local folder is left out with its reason, and the packages before and after it load (19 kinds of failure between two good packages) | same test | a failure is passed on instead of recorded | loading failed as a whole: `It took over 1.5 seconds to load` |
| A package cannot take the name of one of bzz's tools (`run`, `buzz_…`) | same test | the names are not checked | the package `tool-run` loaded: `true !== false` |
| A provider that is missing is traced to a package without a Pi Durable entry that names it, in the message and in `data.package` | `packages.test.mjs` "a provider that is missing is traced…" | the search of the package's files finds nothing | `[]` where `…/pi-acme-cloud` was expected |
| In the host: a turn runs on a package's model; its tool, prompt section and hook work; `initialize` reports each package | `packages.test.mjs` "in the host…" | extensions are not installed for the agent | `Tool word_count is not available` |
| What a package prints goes to the log; the pipe carries only the protocol | same test | standard output is not redirected | the client read the package's line as a message: `"the guard p"... is not valid JSON` |
| `_meta.bzz.packages: false`: no code of any package runs, its tools and providers are not there, and the error for its provider says packages are off | `packages.test.mjs` "bzz can switch package loading off…" | the switch is ignored | `_meta.bzz.packages` lists `acme` and the others where `[]` was expected |
| `"packages": false` in `agent.json` keeps them off in a moved copy | same test | the field is not read | the same, for the moved copy |
| What a package leaves uncaught after it loaded does not end the host: a promise that fails at import, one that fails later with no file of the package in its stack, an exception from a timer. The other package's tool and its own still work | `packages.test.mjs` "what a package leaves uncaught…" | the two handlers in `main.mjs` are not installed | the host ended before it answered: `timed out after 30000 ms: initialize` |
| The limit of that: an exception that no stack traces to a package still ends the host, with code 1 | same test | every exception is survived | `timed out after 5000 ms: the host to end` |
| A moved agent keeps its allowed hosts: `network` in `agent.json` | `remote.test.mjs` "a moved agent keeps its allowed hosts, and gets no folder…" | the list is not read from the file | `curl` to the listed server refused: `is not on this agent's allow-list (it is empty)` |
| A moved agent gets no folder of the machine it lands on, whatever `agent.json` says | same test | `mounts` is read from the file | `ls /mnt` shows `there`, and `a file of the other machine` is read |
| …and never has `run` | same test | also `exec` is honoured without bzz | `_meta.bzz.run` is `['there']` where `[]` was expected |

**Not proven here:**

- A real Pi package. Every package that ran was written by a test. The
  owner's own packages are in Pi's folder, which no test may read. A
  provider from a package with a real sign-in, and its token refresh, did
  not run.
- A package that never gives the thread back (an endless loop). It cannot be
  timed out and holds the host: `initialize` is never answered. bzz needs a
  time limit of its own there. Not tested, because the test would be that
  hang.
- A package that calls `process.exit`, or throws something that carries no
  stack (a string), ends the host. The second is shown by the test above as
  the limit it is.
- macOS and Windows: the CI job has not run with these tests. On Windows a
  package's path and the import redirection are unproven.
- "`agent new` says so when the chosen model is not available" (plan, M2) is
  bzz's side. The host gives it the words and `data.package`.

**Found on the way (second pass):**

- A promise that a package left to fail ended the host with code 1, at every
  start: Node's default. The first version of the package code had tests for
  a setup that throws and one that hangs, all of which the host awaits, and
  none for what nobody awaits. A probe with `Promise.reject(…)` at the top of
  a package's file showed it; `main.mjs` now survives it, and the test above
  is its proof.
- A later package's tool silently replaces an earlier package's tool of the
  same name, as a package's `bash` replaces Pi's. It is said in the protocol
  (section 10) and the README; nothing refuses it.

## M3 — The agent's disk

**Scope:** `files.sqlite`, the shell, git, granted folders and the network
allow-list, in `agent-host/src/`. Same commit, machine and command as M2.

| Claim | Test (`agent-host/test/`) | Fault planted | How it failed |
|---|---|---|---|
| Files, directories, symbolic and hard links, renames, with the errors a filesystem gives | `disk.test.mjs` "files, directories, links and renames" | a link at the end of a path is not followed | reading through the link gave nothing |
| A link that loops is refused | `disk.test.mjs` "a link that loops…" | no limit on hops | not `ELOOP` |
| Content is stored in chunks named by their SHA-256, equal content once | `disk.test.mjs` "content is stored in chunks…" | the hash also covers the file and position | `6 !== 3` stored chunks |
| A reader holds its file through a rename | `disk.test.mjs` "a reader holds its file…" | the reader reads by path | `ENOENT … '/a'` |
| A checkpoint of the disk leaves no write-ahead file, and the disk opens again at the next call | `disk.test.mjs` "what a new process sees…" | the connection stays open | `-shm` and `-wal` beside the file |
| The self-check names each kind of damage | `disk.test.mjs` "the self-check names each kind of damage" | the check reports nothing | the damage planted by the test is not reported |
| The shell works on the disk, `/dev/null` included, and offers no other interpreter | `disk.test.mjs` "the shell works on the disk" | no `/dev/null` | `ENOENT … '/dev/null'` |
| Thirty kills in the middle of writes and commits leave the disk consistent and its repository readable; real `git fsck` is clean afterwards | `kill.test.mjs` (30 kills, 24 to 28 commits per run) | no transaction around a call | `the disk after kill 17: content with a wrong use count: 1` (the kill and the damage vary by run) |
| The disk and shell are a valid Pi Durable environment: Pi's own suite passes | `env-conformance.test.mjs`, 24 cases of `@earendil-works/pi-durable/testing` 1.5.0 | exit code always 0 and watch reports nothing | 9 of 25 fail |
| Clone, edit, diff, commit, push, pull and a merge with no real git: the real `git` reads the pushed commit in the remote (`git http-backend`) and accepts the agent's repository (`fsck`, `status`, `rev-list`) | `git.test.mjs` "clone, edit, diff, commit, push and pull…" | `push` reports success without pushing | the remote still holds `alpha\nbeta\ngamma` |
| What git lacks it names: `rebase`, `stash`, SSH remotes | `git.test.mjs` "what the sandbox's git lacks…" | `rebase` not refused as unsupported | `1 !== 129` |
| A merge that conflicts changes nothing and says so | `git.test.mjs` "a merge that conflicts…" | the merge is not stopped at a conflict | conflict markers in the file |
| A granted folder is seen alike by the shell, git and Pi's tools | `mounts.test.mjs` "a granted folder is visible…" | paths below `/mnt` are not sent to the granted folder | `/mnt/site` is empty |
| A folder granted read-only cannot be changed by any of the three | `mounts.test.mjs` "a folder granted read-only…" (11 commands, then the machine's folder is compared) | no write check | `echo x > /mnt/site/new.txt` succeeded |
| A folder granted for writing is changed on the machine; the folder itself cannot be removed, renamed or locked | `mounts.test.mjs` "a folder granted for writing…" | a write writes nothing | the file is empty |
| `/mnt` holds granted folders and nothing else, and nothing about grants is on the disk | `mounts.test.mjs` "/mnt holds granted folders and nothing else" | `/mnt` accepts changes | `mkdir /mnt/mine` succeeded |
| No path leads out of a granted folder: `..`, a Windows-style name, a link the agent plants, a hard link or a move across | `mounts.test.mjs` "no path leads out…" | `..` handed to the real filesystem below `/mnt` | the path reached the real parent folder: `EACCES` where `ENOENT` was expected |
| Links already in a granted folder are followed only while they stay inside; nothing is created or changed through one | `mounts.test.mjs` "links already inside a granted folder…" | the containment check always passes | a link to a file outside was read; `ln -s ../private/secret.txt` succeeded |
| An error names the virtual path, never the real one | `mounts.test.mjs` "an error names the virtual path…" | errors carry Node's path | `ENOTDIR … scandir '/tmp/bzz-agent-host-mounts-…/site/index.html'` |
| A granted folder that is missing is empty of everything; a pipe is not read | `mounts.test.mjs` "a granted folder that is not there…" | a pipe is opened like a file | the test never ends, as the host would not: stopped after 20 s |
| `curl` and `git` reach a listed host and nothing else; a refused request never leaves | `network.test.mjs` "curl reaches a listed host…", "git is held to the same list…" (the unlisted server counts its requests: 0) | the list is not consulted | the other host was reached, by `curl` and by `git` |
| A redirect to an unlisted host is refused before it is followed | `network.test.mjs` "a redirect to a host that is not listed…" | only the first address is checked | the other host was reached |
| The list is exact: HTTPS, that host, that port; an entry is a host, not a URL or a pattern | `network.test.mjs` "the list is exact…" | hosts compared without their port | `example.org:8443`, which is listed, was refused |
| Credentials do not follow a redirect to another host | `network.test.mjs` "credentials given for one host…" | headers are kept | `Basic abc` sent to the second host |
| A compressed answer reaches the caller as its content | `network.test.mjs` "a compressed answer…" | no decoding | gzip bytes instead of the text |
| Grants and the allow-list arrive in `initialize`, are checked there, and apply to Pi's tools and the shell | `acp.test.mjs` "grants and the allow-list…" | the grants sent are ignored | a bad grant is not refused |

**Seen by hand, with the network** (not part of `npm test`):

- `git clone https://github.com/arpagon/bzz` in the sandbox, allow-list
  `github.com`: 7.8 s, HEAD equal to `git ls-remote`, 465 files, self-check
  clean, 396 MB of memory at the peak, `files.sqlite` 80 MB. Real git took
  5.4 s for the same clone.
- With that list, `git clone https://gitlab.com/…` and `curl
  https://example.com` are refused by name. With `github.com` listed,
  `curl -L https://github.com/arpagon/bzz/raw/main/LICENSE-MIT` is refused
  at GitHub's redirect to `raw.githubusercontent.com`.

**The plan's exit gate:**

- [x] Pi Durable's environment suite passes: on Linux.
- [ ] …on three platforms: macOS and Windows are in the CI job, not yet run.
- [x] A clone, commit, push and pull against real git.
- [x] Thirty kills without inconsistency.

**Not proven here, and not built:**

- macOS and Windows, as above. On Windows, links can be made only by some
  accounts: the link cases say so and are skipped when they cannot run.
- Credentials for a private remote "from bzz at the moment of use" (plan,
  M3): not built in the first pass. Built in the second, below.
- A repository of several hundred MB, files of hundreds of MB, many agents at
  once.
- HTTPS from `curl` is not in `npm test`: it would need the network or a
  certificate. It was seen by hand, above.
- A granted folder is checked, then used. Another program of the machine
  that swaps a link between the two can win. The agent cannot: its file
  access is this process, one call at a time.

**Found on the way:**

- Node's `fetch` took 28 s for the 39 MB pack that `node:http` takes 5 s for
  (Node 26.5, same request, four runs). The host sends through `node:http`.
- just-bash blocks parts of the runtime while a script runs. Node's TLS reads
  the environment and fails inside it, so the host's request runs as trusted
  code (`DefenseInDepthBox.runTrustedAsync`). The allow-list is inside it.
- just-bash lets a refused write of a redirection (`echo x > file`) escape
  the whole script as an exception. The host turns it into a failed command.
- just-bash expects `/dev/null` in the filesystem. The host answers for it;
  nothing named `/dev` is on the disk.
- Node closes its end of a child's pipes when that child exits. The first
  version of the "parent is gone" test passed for that reason, on the end of
  input, and proved nothing about the parent; its red run showed it.
- The parent's id must be read before the host's packages load, or a parent
  that dies during startup is mistaken for its successor.
- The first red run for "no path of the machine is kept" did not fail: with
  real paths planted in the errors of a granted folder, nothing in the test
  made the model see such an error. The test now makes Pi's `read` and the
  shell's `touch` hit one, and the planted fault is caught in
  `memory.sqlite`.
- `just-bash` 3.6.0 pins `guarded-fetch` 0.1.3, which pins `undici` 7.29.0,
  with 18 advisories. `package.json` overrides it to 7.30.2.
- The lock file holds one LGPL-3.0 package, `node-liblzma`, optional and
  never built. `--omit=optional` leaves it out; the tests pass without it.

### Credentials for a private remote (second pass)

**Scope:** `_bzz/credential` (protocol section 8), in
`agent-host/src/git.mjs` and `host.mjs`. Same commit, machine and command as
the second pass of M2. The remote is the real `git http-backend` behind a
server on this machine that answers 401 without the right `Authorization`
header. The secret is a marker, not a token. bzz's side, which keeps the
credentials and answers, is not part of it.

| Claim | Test (`agent-host/test/`) | Fault planted | How it failed |
|---|---|---|---|
| When a listed remote asks who is calling, git asks bzz for that host on the turn's session, and the clone, the push (read back with real `git`) and the fetch succeed with the answer | `credentials.test.mjs` "a private remote: bzz is asked…" | the shell's git is given nobody to ask | `asks for credentials, and this copy of the agent has none`, `exit 128` |
| Nothing is kept from one git command to the next: each asks again | same test | the answer is remembered per host | `1 !== 2` questions after the clone and the push |
| Wrong credentials: the remote refuses, git says so, and bzz is not asked a second time for that command. Half an answer, an error of bzz and a bzz that does not know the request each end the command with the reason; a failed clone leaves nothing | same test | no handler for the remote's refusal | `git: HTTP Error: 401 Unauthorized`, `exit 1` where `refused the credentials bzz has for it`, `exit 128` was expected |
| The secret is not in the log | same test (after `_bzz/checkpoint` and a clean exit, both files, the log and every line the host wrote on the pipe are searched for five forms of two secrets; a marker the model wrote is found in both files, so the search sees what is there) | the answer is logged | `the log holds the secret, as ghp_NotARealToken…` |
| The secret is not in `files.sqlite` | same test | the answer is written to a file on the agent's disk | `files.sqlite holds the secret, as ghp_NotARealToken…` |
| The secret is not in `memory.sqlite` | same test | the answer is put in a document of the conversation store | `memory.sqlite holds the secret, as ghp_NotARealToken…` |
| A cancelled turn does not wait for bzz's answer: the command ends and its half-made clone is removed | `credentials.test.mjs` "a cancelled turn does not wait…" (bzz never answers) | the question ignores the stop | the clone still waits and its folder stays: `'1' !== '0'` entries |
| Under `buzz-acp` there is nobody to ask: git says that this copy has no credentials, and `buzz-acp` is asked nothing | `credentials.test.mjs` "under buzz-acp there is nobody to ask…" | a remote copy asks too | `bzz gave none: Method not found: _bzz/credential` where `this copy of the agent has none` was expected |
| A host that is not on the allow-list never causes the request. In the host: an unlisted private remote receives no request and bzz is not asked ("a private remote…"). And where the challenge got through by a fault in the transport, the question has its own check | `credentials.test.mjs` "a remote that is not on the list is never asked about…" | no check where the question is asked | git went on to `other.example refused the credentials bzz has for it` where the allow-list's refusal was expected: bzz had been asked |
| A remote that repeats what it was sent does not get the secret into what git prints | same test | nothing is starred out | `403 Forbidden for Basic c2NvdXQt…` where `Basic ***` was expected |

Credentials do not follow a redirect to another host: that is the first
pass's "credentials given for one host…" in `network.test.mjs`, above.

**Not proven here:**

- A real private remote. No token of any forge was used, and nothing went
  over HTTPS: the test remote speaks plain HTTP on this machine. GitHub and
  GitLab with a token as the password, the 203 that Azure DevOps sends in
  place of 401, and a forge that answers 404 for a private repository (bzz
  is then never asked) did not run.
- bzz's side of `_bzz/credential`: where the credentials are kept, and the
  owner being asked. A scripted client answered. Built and proven since:
  "Credentials for a private remote, bzz's side", below.
- The answer stays in this process's memory until Node frees it. Nothing
  wipes it. "Passes through memory only" is what is shown, not "is erased".
- macOS and Windows: the CI job has not run with these tests. They need
  `git` with `http-backend` and say so when they are skipped.

### Credentials for a private remote, bzz's side

Linux x86_64, Rust 1.98.1, Node 26.5.0, git 2.x with `http-backend`, Buzz pin
`bf3bdd0eaefe`, 2026-11-14. Commits `30d65a5..11d6c91` on `v0.15.0`.

**Where it is.** `src/agents/git_credentials.rs` (the store, sealed to the
owner's key), `Agent::credential` in `src/agents/runtime/agent.rs` (the
answer to `_bzz/credential`), `bzz agent credential set|list|remove` in
`src/cli_agent.rs`, `src/agents/activity.rs` (the record of each request).

**How to repeat.**

```sh
. runs/env.sh
cargo build --locked --example scripted_harness
cargo test --locked --all-features --lib -- agents::git_credentials agents::activity
cargo test --locked --all-features --test agent_runtime_test a_git_credential what_the_owner_takes_away several_requests
cargo test --locked --all-features --test agent_manage_test deleting_an_agent
cargo test --locked --all-features --test secret_scan_test a_stored_git_credential
cargo test --locked --all-features --test agent_host_test the_real_host_clones    # real host; needs Node and a git with http-backend
BZZ_RELAY_TEST_FILTER=real_relay_agent_runs_a_real_command BZZ_BUZZ_SOURCE=$PWD/runs/buzz-pin ./scripts/test-relay.sh
```

**Red runs.** Each is the named test with the one fault in the cell planted,
seen failing as quoted, then removed. 52 ran in a clone of the repository
with a build folder of its own, on `eccb009`, `1bf8764` and `11d6c91`
(`runs/v015/c4-red.py` plants and restores each; local, untracked). Six ran
before that in the main checkout, while the code was written: the host's
two, the descriptors, and the three of the settings file.
`U:` is a unit test in the named module; `T:` a test of
`tests/agent_runtime_test.rs` (scripted harness); `H:` of
`tests/agent_host_test.rs` (the real host on its stand-in model, a recorded
relay); `S:` of `tests/secret_scan_test.rs`.

| Claim | Proof | Red with the fault | Result |
|---|---|---|---|
| A credential is sealed with NIP-44 to the owner's key, one file per agent and host, for the owner only. Nothing on disk holds the user name or the token; the hosts are listed without opening anything; another owner cannot open it; storing again replaces; what cannot be a credential is refused | `U: git_credentials::a_credential_is_sealed_for_one_agent_and_one_host` | next row | pass |
| What is sealed names the agent and the host: a file copied to another agent's folder, or under another host's name, opens for nobody | same | the check of the agent and the host removed: `assertion failed: get(keys, &owner, ranger, "github.com").await.is_err()` | pass |
| bzz answers `_bzz/credential` from the stored value for a host on that agent's allow-list, in any case of letters | `T: a_git_credential_is_given_for_a_listed_host_of_this_agent_only` | next rows | pass |
| A host that is not on the allow-list gets none, although one is stored, and the message says what the owner can do | same | the allow-list not looked at: `{"id":"h2",…"result":{"password":"tok-unlisted-marker","username":"scout-bot"}}` where an error was expected | pass |
| A credential stored for another agent only is not this agent's | same; `H:` below | covered by the row on the seal, and by the store's path, which is the agent's | pass |
| A session with no turn running gets none | same | the session not looked at: the token is answered to `not-a-session` (`{"id":"h5",…"result":{"password":"ghp_NotARealToken…`) | pass |
| Only bzz's own host may ask: from another harness the request is one bzz does not know | `T: only_bzzs_own_host_may_ask_for_a_command_or_a_credential` | any harness answered: `left: Null right: Number(-32601)` | pass |
| A host the owner takes off the list while bzz runs gets no credential from then on; a settings file that cannot be read, and an agent paused or gone from the settings, get none | `T: what_the_owner_takes_away_is_refused_at_once_by_a_running_bzz` | the settings file not read again: round 2 is given the credential (`assert!(!allowed && !given)`); an unreadable file counted as no file: round 5 is given it | pass |
| Each request is a line in the agent's activity: the host and what happened. Never the user name or the token | `T: a_git_credential…`; `U: activity::a_question_and_its_answer_are_kept_as_one_line_each` | the token written into the line: `left: ["git credential for github.com · given ghp_NotARealToken…` | pass |
| The activity file is for the owner only, one line per entry, and is cut when it grows | `U: activity::the_file_does_not_grow_without_bound` | never cut: `1232000` bytes | pass |
| `bzz agent credential set` takes the user name and the token from standard input when it is no terminal, never as arguments, and prints neither; `list` prints hosts only; `remove` removes; `agent show` lists the hosts and the activity | `S: a_stored_git_credential_is_in_no_log_no_database_and_no_activity_line` (the bzz binary) | `credential set` printing the token it read: `the token leaked into credential set` | pass |
| **The secret scan:** a stored credential is in no file of the profile (settings, database, activity, the harness's log, the sealed file itself), in four forms, nor in anything a command prints. It reaches a harness on one line only: the answer to its request | same; `H:` below | bzz writing the token it gave to a file of its data folder: `the token leaked into …/d/debug-trace.txt`; with the real host: `…/data/debug-trace.txt holds the token` | pass |
| `agent delete` removes the agent's credentials with its key, and leaves another agent's | `agent_manage_test::deleting_an_agent_removes_its_git_credentials`; relay test below | the removal skipped: `Removal { removed: ["its settings", "its key", …] }` without the credentials | pass |
| **With the real host and a private remote** (`git http-backend` behind a server that answers 401): with nothing stored the clone fails and the agent is told what its owner can do | `H: the_real_host_clones_a_private_remote_with_a_stored_credential_and_not_without` | an error that says nothing: `["git: 127.0.0.1:43935 asks for credentials, and bzz gave none: refused\nexit 128\n"]` | pass |
| …stored for this agent while its host runs, the next clone works: `Cloned into`, the file's content, `exit 0` | same | bzz answering "none is stored" whatever is stored: `the clone works with the stored credential: ["git: 127.0.0.1:38931 asks for credentials, and bzz gave none: …` | pass |
| …a host taken off the list is refused by the host before any request leaves, and bzz is not asked | same | not run | pass |

**On the pinned relay** (`real_relay_agent_runs_a_real_command_and_clones_a_private_remote`,
with M7 below): the agent is made by `bzz agent new --allow-host <remote>`;
the first clone is answered in the chat with `No credential is stored for
127.0.0.1:<port>. Your owner can store one with …` and `exit 128`; the bzz
binary then stores one (`agent credential set`, standard input) while the
agent runs; the second clone is answered with `Cloned into`, `alpha beta
gamma` and `exit 0`. Every file of the profile is searched for the token in
four forms, and for the passphrase; `agent delete` reports `removed: its 1
stored git credential(s)` and leaves no `agent-*` file in the keys folder.

**Not proven here:**

- A real forge. The remote is plain HTTP on this machine. GitHub, GitLab and
  the 203 of Azure DevOps did not run; no real token was used.
- macOS and Windows: not compiled or run here (only the Linux target is
  installed). The store has no Unix-only code but the file mode.
- A Buzz Desktop identity as the owner: sealing goes through the same signer
  call as an agent's key does, and was run with an encrypted-file identity
  and with keys held in memory only.
- The token stays in bzz's memory until the answer is written and freed: the
  stored value is wiped when dropped, the JSON that carries it is not.
- `credential set` on a terminal (the prompts without echo): the test feeds
  standard input. `rpassword` reads the terminal as it does for a
  passphrase.

## M4 — Creating and managing an agent

Linux x86_64, Rust 1.98.1, Node 26.5.0, Buzz pin `bf3bdd0eaefe`, 2026-11-14.
Commits `8b78d05..fa9b07b` on `v0.15.0`. Buzz 1.0's own code was read at
`block/buzz-app@095df3f0f6f8` for the records an agent needs: the profile
(`crates/agent-controller/src/app_agent.rs`, `profile_event`), the owner's
record and its removal (`src/features/agents/relay-removal.ts`), how the
record is read (`src/features/agents/service.ts`, `listingsOf`), and adding to
a channel (`src/features/relay/channel-writes.ts`, `addAgents`).

**Where it is.**

| What | Where |
|---|---|
| The records, creation and its undo, add, edit, pause, delete, the questions and how they are read | `src/agents/manage.rs` |
| The capability records | `src/agents/capability.rs` |
| `bzz agent new\|list\|show\|add\|edit\|pause\|resume\|delete\|doctor\|run` | `src/cli_agent.rs` |
| `:agents`, `:agent new` | `src/ui/local_agents.rs`, `src/app.rs` |
| The directory without the `bot` role | `src/store/agents.rs`, `src/store/queries.rs`, `src/sync/directory.rs` |
| What M1 left open | `src/agents/runtime/{agent,relay,acp,mod}.rs`, `src/store/agent_runtime.rs` |
| The owner's guide | [`agents.md`](agents.md) |

**How to repeat.**

```sh
. runs/env.sh
cargo test --locked --all-features --lib -- agents::capability agents::manage store::agents store::agent_runtime ui::local_agents agent_new_in_the_tui
cargo test --locked --all-features --test agent_manage_test      # creation against a recorded relay that fails at each step
cargo test --locked --all-features --test agent_runtime_test     # scripted harness
BZZ_RELAY_TEST_FILTER=real_relay_agent_new BZZ_BUZZ_SOURCE=$PWD/runs/buzz-pin ./scripts/test-relay.sh
```

Each red is the named test with the one fault in the cell planted, seen
failing as quoted, then removed. `U:` is a unit test in the named module;
`C:` is a test of `tests/agent_manage_test.rs`; `T:` of
`tests/agent_runtime_test.rs`.

### The capability record (design point 3, rule 12, D18, D19)

| Claim | Proof | Red with the fault | Result |
|---|---|---|---|
| Every harness states what survives a restart, its own disk, git with no real process, real folders, a move with memory, what runs on the machine, its sandbox, and for each rule who enforces it; a capability it lacks is stated with what takes its place | `U: capability::every_harness_states_each_capability_and_who_enforces_each_rule` | no warning where there is no sandbox: `left: false right: true` | pass |
| Pi Durable's record says what its tests show. Grok, Codex and Claude Code say what the M6 groundwork saw: Grok's and Codex's own sandbox hold the folder, Claude Code is held by bzz's answers only, Grok on Windows by nothing. D19's wording is on all three, D18's warning exactly where there is no sandbox, and all three are marked "not available in this version yet" | `U: capability::the_records_say_what_the_tests_and_the_groundwork_showed` | a coding harness marked as offered: `assertion failed: !record.available` | pass |
| The record is printed at creation and by `agent show` | relay test below (`capability record: pi-durable`, `one final reply per mention: bzz`, `its own disk:        yes`) | not run | pass |

### The records, and creation all or nothing

| Claim | Proof | Red with the fault | Result |
|---|---|---|---|
| The profile is a kind 0 with the name, `"bot": true` and the picture when there is one | `U: manage::the_profile_is_a_bot_with_its_name_and_picture` | see next row | pass |
| The owner's record is kind 30177 with `d` the agent's key, `runtime` `bzz/<harness>` (which passes Buzz's own test of a runtime for all four harnesses), and content `parallelism`, `respond_to` in Buzz's words, `name`, `picture`. bzz's own reader of v0.11 takes profile and record as a verified agent of that owner. An edit keeps what another client put in the record that is not bzz's to set, as Buzz does, and the removal names the record by its coordinate | `U: manage::the_owners_record_is_what_buzz_reads` | not run for the unit test; on the relay the same content is read back from the relay | pass |
| An agent is added as a member, with the `bot` role only in a Solo chat (D13) | `U: manage::an_agent_joins_as_a_member_and_as_a_bot_only_in_a_solo_chat`; relay test | on the relay: `RR1` below | pass |
| The answers of the command and of the form are read the same way; contradictions are refused with what to write | `U: manage::the_answers_are_read_as_written`; `U: ui::local_agents::the_form_asks_what_the_command_asks_and_reads_it_the_same_way` | not run for the first | pass |
| A created agent has its sealed key, its folder, its settings saved, its profile signed by the agent with the owner's authorization, and the owner's record; the broker serves it nothing | `C: a_created_agent_has_its_key_folder_settings_profile_and_owners_record` | a profile not marked as a bot: `left: Object {"name": …, "picture": …} right: Object {"bot": Bool(true), …}` | pass |
| **A creation that fails at any step leaves nothing**: the profile refused, lost, or landed and unconfirmed; the owner's record refused, lost, or landed and unconfirmed. After each: no key, no folder, no settings in memory or on disk, and the recorded relay holds nothing of the agent. What was published is removed first, by the agent's own key while bzz still holds it | `C: a_creation_that_fails_at_any_step_leaves_nothing` (six cases) | the undo keeps the key: `the profile is refused left: (1, 0, 0) right: (0, 0, 0)`; the undo leaves the profile: `the profile is lost left: [("agent", 0)] right: [("agent", 0), ("agent", 5)]` | pass |
| A failure on this machine (the folder cannot be made, the settings cannot be saved) publishes nothing and leaves nothing; a harness of another task, a name that is no name and an empty allowlist are refused before anything is made | `C: a_creation_that_fails_on_this_machine_publishes_nothing` | a coding harness not refused: `unwrap_err()` on a created Grok agent | pass |
| When the relay will not take back what it holds, the key, the folder and the settings still go and the failure names each record left | `C: what_an_undo_cannot_take_back_is_named` | what the relay kept is not reported: `left: 1 right: 2` | pass |
| The checks before a creation: Grok, Codex and Claude Code are refused with "not available in this version yet"; a machine without Node is told what to install; nothing is installed or made | `C: the_checks_before_a_creation_say_what_to_do` | the harness not looked at: `Grok: … needs Node.js 22.19 or newer, and no node was found …` where "not available" was expected | pass |
| An edit says which records changed: a name or a picture both, who may wake it the owner's record, the rest nothing; a contradiction changes nothing; a moved agent is not resumed | `U: manage::an_edit_says_which_records_changed` | not run | pass |

### The agent directory without the `bot` role (D13)

| Claim | Proof | Red with the fault | Result |
|---|---|---|---|
| A plain member of a channel with the records bzz and Buzz 1.0 publish is listed as an agent, is a mention candidate, is validated by its owner's rule, and is gone from the list when it leaves | `U: store::agents::a_plain_member_with_its_owners_proof_is_an_agent` | the `bot` role required again in the reconciliation: `left: 0 right: 1`; in the list: `left: 0 right: 1` | pass |
| The proof requirements are unchanged: a member with no proof, with a profile that only says it is a bot, or with an authorization made for another key is a person. It has no row in the directory in any state, and mentioning it needs no validation | `U: store::agents::a_member_without_a_valid_owners_proof_is_not_an_agent` | every member with a profile taken as a candidate: `none of them is in the directory, in any state left: 1 right: 0` | pass |
| A v0.11 test asserted that a plain member of a channel has no authority. It now asserts the opposite | `U: store::agents::owner_controlled_dm_member_verifies_without_relaxing_channel_bot_authority` | the test as it was, against the change: `assertion failed: matches!(store.validate_agent_mentions(…), Err(Error::Access(_)))` | pass |
| The other v0.11 requirements still hold (wrong-owner record, DM owner only, removal, unknown policy) | the six older tests of `store::agents` and `agents::protocol`, unchanged | not run | pass |

Places that assumed the role and were changed: the reconciliation and its
candidates, the list, an agent's channels, mention candidates
(`store/queries.rs`), mention validation, the refresh
(`sync/directory.rs`, which now also asks for the profiles of members that
may carry a proof), and one message of the TUI. The v0.13 broker's write rule
(`broker/relay.rs`) keeps the role on purpose.

### The TUI

| Claim | Proof | Red with the fault | Result |
|---|---|---|---|
| The panel shows, for each local agent, its state, what it is doing now, its last outcome, its settings and its capability record; with bzz locked it says so; `l` shows the end of the log, made safe to print, in place of the record | `U: ui::local_agents::the_panel_shows_each_agents_state_doing_last_outcome_and_record` (TestBackend) | the record left out: `capability record: pi-durable (everything in v0.15.0)` is not on the screen | pass |
| The form asks every question of the command, takes typed and pasted text on one line into the answer in focus, takes none while it is confirming, and says what will be signed | `U: ui::local_agents::the_form_asks_what_the_command_asks_and_reads_it_the_same_way` | text taken while confirming: `ignored while confirming` is on the screen | pass |
| `:agent new` in the running App: refused while locked; an answer that cannot be read is refused in place; the checks run in the background and install the host on first use; nothing exists until Enter; Enter creates the agent (key, settings on disk, two records published) and shows its capability record | `app::tests::agent_new_in_the_tui_creates_an_agent_and_the_panel_manages_it` (TestBackend, stand-ins for `node` and `npm`, a recorded relay) | the answer of the checks dropped: the confirmation never shows; an entry written before Enter: `assertion failed: app.config.hosted_agents.is_empty()` | pass |
| The first local agent of a community is started by the next tick, without a restart of bzz | same | the retry removed: `the agent never became Idle: None` | pass |
| `:agents` then shows it; its last outcome comes from bzz's record of the turn; Space pauses and resumes its process; `x` with nothing running says so; `l` shows its log; Tab goes to the directory and back; `:agents` is the panel when the community has a local agent and the directory when it has none | same | the last turn not read: `last outcome: failed: timeout` is not on the screen; a pause that does not reach the process: `the agent never became Paused: Some(Idle)` | pass |
| A creation that fails in the TUI goes back to the answers with the reason, and leaves no agent, no key and no settings | same | covered by the reds of `C:` above (same code) | pass |
| The `:broker` panel still shows agents whose process the user runs, and a status line per local agent | `app::tests::local_agents_run_while_the_owner_is_unlocked_and_stop_on_lock`, `broker_panel_shows_a_status_line_per_local_agent` (unchanged since M1) | M1 | pass |

### What M1 left open

| Claim | Proof | Red with the fault | Result |
|---|---|---|---|
| Finished requests and idle stored sessions are pruned by a stated rule: over and older than 30 days, or beyond the newest 1000 of an agent, with their stored replies; never what is queued, being answered or unacknowledged; another agent's rows are not touched | `U: store::agent_runtime::finished_requests_and_idle_sessions_are_pruned_by_the_stated_rule` | an unacknowledged reply pruned: `left: Pruned { requests: 3, sessions: 1 } right: Pruned { requests: 2, sessions: 1 }` | pass |
| The rule runs when an agent starts | `T: an_agents_finished_requests_are_pruned_when_it_starts` | no pruning at the start: `the old request is gone` | pass |
| A message older than a day is never a request, so a pruned request cannot come back as a new one | `U: runtime::relay::a_message_older_than_a_day_is_never_a_request` | off by one second: `assertion failed: !too_old(now - MAX_REQUEST_AGE.as_secs(), now)` | pass |
| Two agents that may wake each other stop: another agent's message wakes this one only so often in one conversation (six times an hour; three in the test). A person in the same conversation, the owner, and the other agent in another conversation still wake it | `T: two_agents_that_wake_each_other_stop_after_a_few_exchanges`; `U: store::agent_runtime::requests_of_one_author_in_one_place_are_counted_and_an_agent_is_forgotten_whole` | the limit off: `they never stop` | pass |
| The owner stops a running turn: it ends once with a notice that says so; a harness that honours the cancel keeps its process, one that does not is stopped and the turn is not run again | `T: the_owner_stops_a_running_turn_and_it_ends_once` | the stopped turn treated as any cancelled turn: `left: (Failed, Some("no-reply"), 1) right: (Failed, Some("stopped"), 1)` | pass |
| `agent delete` forgets every row of the agent and no row of another | `U: store::agent_runtime::requests_of_one_author…` | the sessions left: `left: 7 right: 8` | pass |

### On the pinned relay

`real_relay_agent_new_to_delete_on_the_pi_durable_host`, in
`tests/relay_integration.rs`: every step is the `bzz` binary run as a command
on a disposable profile, with the owner's passphrase on a file descriptor.

| Claim | Observed | Result |
|---|---|---|
| `agent doctor` reports Node, npm and the install, and installs nothing | `Node.js:`, `not installed yet`; no install afterwards | pass |
| `agent new --harness grok` is refused; a model Pi does not have is refused by the host's own check after the install of first use; neither leaves a key, a folder, settings or a record | `not available in this version yet`; `cannot be used (provider_unavailable)`; `(0, 0, 0)` on the machine; no kind 30177 by the owner on the relay | pass |
| `agent new` creates the agent and prints its capability record and the next step | `model scripted/scripted`, `created in c1`, `capability record: pi-durable`, `bzz agent add Scout <channel>`; `(1, 1, 1)` on the machine | pass |
| The relay holds what Buzz reads: the agent's profile `{"name": "Scout", "bot": true}` with the owner's tag, and the owner's record with `runtime` `bzz/pi-durable` and `{"parallelism": 1, "respond_to": "owner-only", "name": "Scout"}` | read back from the relay; bzz's v0.11 reader verifies the owner | pass |
| `agent add` by the channel's name (never synced: bzz asks the relay) adds a plain member; in a Solo chat, the `bot` role; an unknown channel is refused | the relay's own member list: role `member`, and `bot` in the Solo chat | pass |
| bzz's own directory lists the agent although it is a plain member (D13) | `agents refresh`: `verified=1`; `agents list --json`: name, owner, `eligible`, `owner-only`, both channels | pass |
| **A mention written by Buzz's own CLI at the pin, by the agent's name, wakes it** | `buzz messages send --content "@Scout are you there?"`: the event carries the agent's key as a mention (Buzz resolved the name from the profile bzz published), and is answered once | pass |
| `agent show` while it runs | state and process, harness, model, who may wake it, hosts, both channels with their roles, `answered · attempt 2`, the capability record | pass |
| `agent delete` of a running agent is refused | `is running` | pass |
| `pause` and `resume` | the settings; `agent list`; under `bzz agent run` the status line `Scout · pi-durable · paused` | pass |
| `edit --name --wake --picture` publishes the profile and the owner's record again, each replacing the one before; instructions and hosts publish nothing; a model that cannot be used is not saved | one profile `{"name": "Ranger", "bot": true, "picture": …}`; one record with `respond_to` `anyone`; the model unchanged | pass |
| `agent delete` removes the owner's record from the relay, the agent from its channels, and its key, folder, settings and rows | no kind 30177 for the agent, no membership in either channel, `(0, 0, 0)`, no row | pass |

Red on the relay, four runs, each the same test with one fault planted:

- **RR1, an agent added with the `bot` role everywhere:**
  `a plain member, as Buzz 1.0 adds an agent (D13) left: Some("bot") right: Some("member")`.
- **RR2, the directory asking for the `bot` role again:**
  `remote agent directory refreshed: candidates=1 verified=0 …`. The first
  version of this check ran after the agent had the `bot` role in the Solo
  chat, and its red run **passed**: the role made the agent a candidate
  whatever the change. The check now runs while the agent is only a plain
  member of a channel.
- **RR3, a turn cut off by the kill never handed over again:**
  `left: "I could not answer: my process stopped unexpectedly." right: "answered after the host was killed"`.
- **RR4, `agent delete` reporting the owner's record as removed without
  removing it:** `the owner's record is gone from the relay, so Buzz lists no agent nobody runs`.

Two more runs failed before the test began, with `psql: … the database system
is shutting down` and `database "buzz" does not exist`: the relay harness was
started again too soon after its own teardown. They were repeated.

### The plan's exit gate for M4

"An agent made by bzz is listed, mentioned and answered from Buzz 1.0 in a
disposable sandbox, and from bzz."

- [x] From bzz: created, listed in bzz's own directory, mentioned and answered
  on the pinned relay.
- [x] From Buzz's side as far as the pin goes: Buzz's own CLI at the pin
  resolves the agent's name and its mention is answered; the records are the
  ones Buzz 1.0's code writes and reads, compared field by field with its
  source and checked against its own test of a `runtime` value.
- [ ] **From the Buzz 1.0 desktop app: not run.** No Buzz 1.0 was started
  against a disposable relay. Whether its agent list shows a `bzz/pi-durable`
  agent, and what it shows for a `runtime` it does not know, is M0's
  experiment 2 and is still open.

### Not proven in M4

| Item | State |
|---|---|
| macOS and Windows | not compiled or run here. The agent commands have no Unix-only code; the tests that run processes are Unix only |
| The release binary in a terminal ("`agent new` end to end" in Herdr) | not run. The TUI is proven with TestBackend and stand-ins for `node` and `npm`; the same creation code ran on the pinned relay through the commands |
| `x` in the panel with a turn running | the stop itself is proven at the runtime (`T:` above); in the TUI the key was pressed only with nothing running |
| The interactive questions of `bzz agent new` on a terminal | every question is tested as a flag and in the form; the prompts themselves were not driven by a test |
| A community with federated identity (NIP-FI) | `agent new`, `add`, `edit` and `delete` enroll and revoke the agent as `broker create` and `broker delete` do, by the same code; no test ran them against the adapter |
| `agent delete --force` with the relay unreachable, and a deletion the relay refuses | written; not run |
| Editing while a bzz TUI is open | prevented since: "One writer for the settings", under "M7 — Hands on this machine (bzz's side)" |
| The refresh of the directory in a community of thousands | it now asks for the profiles of members without a stored profile, at most 2000 a refresh. Not measured |
| Benchmarks against v0.14.0 for a profile without agents | not run in this task |

### Gates after M2 (bzz's side) and M4

Run on `fa9b07b`, 2026-11-14, Linux x86_64, Node 26.5.0.

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --check` | clean |
| Lint | `cargo clippy --locked --all-targets --all-features -- -D warnings` | clean (also `--no-default-features`) |
| Tests | `cargo test --locked --all-features` | 610 passed, 0 failed, 25 ignored. 34 passing tests are new since M1: 22 unit (7 install, 5 manage, 2 capability, 2 `store::agents`, 2 `store::agent_runtime`, 2 `ui::local_agents`, 1 relay intake, 1 App), 5 creation, 4 scripted harness, 3 real host |
| Pinned relay | `BZZ_BUZZ_SOURCE=$PWD/runs/buzz-pin ./scripts/test-relay.sh` | 18 passed (the 17 of M1 and the journey above) in 328 s, then `agent_host_test`, 3 passed |
| Agent host | `cd agent-host && npm ci --ignore-scripts --omit=optional && npm test` | 78 tests, 78 pass, 0 skipped; `npm ci` reports 0 vulnerabilities. `agent-host/` is unchanged by this work |
| Supply chain | none run | `Cargo.toml` and `Cargo.lock` are unchanged since `5791420`: no new dependency |

Two faults CI found on `5791420`, both in tests, are fixed in `8b78d05`:

- The Linux `test` job runs `cargo test --locked --all-targets`, which builds
  the examples as tests and leaves no `target/debug/examples/scripted_harness`.
  The job now builds that example first. Repeated here: with the example's
  binary removed, `cargo test --locked --all-targets` fails with
  `…/target/debug/examples/scripted_harness is missing`; after
  `cargo build --locked --example scripted_harness` it passes.
- On Windows the folder grant `/home/ann/site` of `config_test` is not an
  absolute path. The test now grants a path that is absolute where it runs.
  Not run on Windows here; CI will show it.

## M7 — Hands on this machine (host side)

**Scope:** the `run` tool and `_bzz/approve` (protocol section 9), in
`agent-host/src/run.mjs`, `run-child.mjs` and `host.mjs`. bzz's side is not
part of it: the grant with `exec` in bzz's configuration, the question in the
TUI and "allowed without asking" are not built. A scripted client answers
`_bzz/approve`.

**Run on:** Linux x86_64 (`/bin/sh` is dash), commit `3658e52`, 2026-11-14,
with the command and the results of the second pass of M2: 98 pass on Node
26.5.0, 22.19.0 and 24.15.0. The 9 tests of `run.test.mjs` start real
commands (`sh`, `sleep`, `seq`, `env`, `ps`, `node`) in temporary folders and
take about 17 s together.

**Red runs:** 27, as in M2.

| Claim | Test (`agent-host/test/run.test.mjs`) | Fault planted | How it failed |
|---|---|---|---|
| bzz is asked before anything starts, with the turn's session, the folder's name and the command | "an approved command runs in the granted folder, exactly as approved…" | the question is skipped | no request reached the client: `[]` where the `_bzz/approve` was expected |
| What was approved is what runs: the shell is started with the approved string, whole and alone, as its one command | same test (the command prints its own shell's arguments) | `cd . && ` is put before the string | `/bin/sh -c -- cd . && ps -o args= -p $$` |
| The string cannot be read as an option of the shell | same test | no `--` before it | `/bin/sh -c ps -o args= -p $$` |
| The environment is built from nothing: `PATH`, `HOME`, `LANG`, `TMPDIR`, and what the shell adds | same test | the host's environment is handed on | `A_SECRET_OF_THE_HOST`, `BZZ_AGENT_HOST_SCRIPTED_MODEL` and `USERPROFILE` in the command's environment |
| The command starts in the granted folder | same test | no working directory is given | `pwd` printed the host's own folder, and `made.txt` was written there |
| It runs in a process group of its own, which is neither the host's nor the caller's | same test | the shell is not made a group leader | its group is not its own id: `99142 !== 99149` (the numbers vary) |
| It has no terminal and an empty standard input | same test | the keeper's input is handed on | `cat: -: Resource temporarily unavailable` before `end of input` |
| The model is told that it has `run`, where, and that the owner may be asked | same test | the sentence is left out | the system prompt holds no `run` sentence |
| On Linux a command has no open file but its three streams: a descriptor the host was born with is closed | "a command gets its three streams and no other open file of the host" (the host is started with a file open as number 200) | the keeper closes nothing | `200 is open` where `200: EBADF` was expected |
| Only `{"allow": true}` starts a command: `false` with a reason, `{}`, `"true"`, `1`, `null` and a string do not, and the reason reaches the model | "a refused command does not start, and the model is told why" | only `allow: false` refuses | `{}` started the command: the result was empty where the refusal was expected |
| An error of bzz, or a bzz that does not know the request, is a no | same test | an error is taken as a yes | the command ran: `''` where `The command was not started: bzz did not approve it (…)` was expected |
| Two commands in one answer of the model are put to bzz one after the other | same test | the calls of a round run at once | the second was asked while the first had not run: `first has run: false` |
| A command that outlives its time limit is stopped | "a command that outlives its time limit is stopped, with the children it started" (limit 1 s) | no timer | the test never ends: stopped after 20 s |
| …with the children it started | same test | the signal goes to the shell, not to its group | its two children are alive |
| …and one that ignores TERM, children included, is killed after the grace | same test | no KILL after the grace | `these processes are still there after 1000 ms`, all three |
| What a command leaves running ends with it, also a child of a child | "what a command leaves running ends with it, and a turn that is cancelled ends its command" | the group is not killed when the command returns | `these processes are still there after 1000 ms`, both |
| A cancelled turn ends its command, and the prompt answers `cancelled` | same test | the tool does not listen to the stop | `timed out after 8000 ms: the cancelled prompt` |
| A turn cancelled while bzz is being asked starts nothing | same test | the question ignores the stop | `timed out after 8000 ms: the prompt cancelled during the question` |
| A long output comes back as its end, within Pi's bound | "a long output comes back bounded, and the whole of it is on the agent's disk" (5000 lines; 9 MB) | no rule to keep the end | the last line, `5000`, is not in the result |
| …and the whole of it, up to 8 MiB, is on the agent's disk, where the agent reads it with its own tools; the result says where | same test (the file is read back from `files.sqlite` after a clean exit) | nothing is saved | no `Full output: /tmp/run-….log` in the result |
| A grant without `exec` gives no command there, also when it allows writes | "without a folder granted for it there is no `run`, and a grant without `exec` gives none" | `write` counts as `exec` | the command ran in `docs`: an empty result where the refusal was expected |
| Only `"exec": true` is a grant: without one the tool does not exist and the model is not told of it | same test | any value that is not empty counts | `_meta.bzz.run` is `['docs']` for `"exec": "true"` |
| A folder that is not there is said, and bzz is not asked | same test | the folder is not looked at | the command ran, in the host's own folder: `x` |
| A host that is killed outright does not leave its command behind: the keeper sees it go | "a command does not outlive a host that is killed, and is not started again after a restart" | the keeper does not watch its input | `these processes are still there after 5000 ms`, all three |
| A command cut off by a restart is not started again, and bzz is not asked again: the model is told it was interrupted | same test | the tool is marked safe to run again | it was put to bzz again: `The command was not allowed: not a second time` where `Tool run was interrupted` was expected |
| Under a killed host, a command that ignores TERM is killed by the keeper after the grace | same test | no KILL after the grace | `these processes are still there after 5000 ms`, all three |
| A host stopped by the end of its input leaves within 2 s with code 0, and its command ends with it | "a host stopped by the end of its input leaves in time, and its command ends with it" | the keeper does not watch its input | `these processes are still there after 5000 ms`, all three |

A remote copy never has `run`: that is `remote.test.mjs` "a moved agent
keeps its allowed hosts, and gets no folder…", in the second pass of M2.

**The plan's exit gate** ("an approved command runs in a granted folder and
its result is on the agent's disk; a refused one does not run"):

- [x] The host's half, against a scripted client: an approved command runs
  in the granted folder; a refused one does not start. A result too long for
  the model is on the agent's disk; a file a command made is brought to the
  agent's disk by the agent's own `cp` and read back from `files.sqlite`. A
  short result is in the conversation, which is `memory.sqlite`, not on the
  disk.
- [x] Against bzz itself: "M7 — Hands on this machine (bzz's side)", below.
  An approved command runs in the granted folder and its output reaches the
  chat, on the pinned relay; a refused one does not run.
- [ ] Approve and refuse in the TUI, **on the release binary**: not run. The
  TUI is proven with the real App on a test terminal (below).

**Not proven here:**

- macOS. Nothing ran there. `/bin/sh` is another shell there, and the tests
  lean on `sh -c --`, `ps -o args=` and process groups as Linux has them.
  The CI job has not run with these tests.
- On macOS a command inherits the descriptors the host was started with:
  only Linux tells a process which those are. The test says so and asserts
  nothing there. bzz must start the host with none.
- Windows: the host does not offer `run`, and the tests are skipped.
- The host and the keeper killed at the same moment: the command goes on.
  Not tested; it is a limit, written in the protocol and the README.
- A process that leaves the group (`setsid`) is not ended. The test shows
  it as the limit it is.
- A real workload: a build, a test suite that runs for minutes, output of
  gigabytes. The longest limit tested is 1 s, the largest output 9 MB.
- Linux without `/proc`: the keeper then closes nothing.
- The folder is where a command starts, not a wall. Nothing here confines a
  command, and no test claims it.

**Found on the way:**

- A command inherited every descriptor that whoever started bzz had left
  open. A probe that listed a command's descriptors showed about 80 of the
  application this terminal runs in: its database files, sockets and the
  master side of a terminal. They were numbered 23 and up; number 3 did not
  get through. That fits what Node does at start: it marks the descriptors
  it finds as not to be passed on, and stops at the first gap above 15. What
  lies after the gap goes through the host and the keeper to the command.
  The keeper now closes those on Linux, and the protocol (section 2) asks
  bzz to hand the host none.
- `agent-host/made.txt` was in the tree: a test command had once run in the
  host's own folder, and its file was committed. It is removed. The red run
  for "starts in the granted folder" writes it again, which is how its
  origin was found.
- The red run for "the secret is not in `files.sqlite`" first stayed green.
  The fault kept the answer in one file, and the last answer the test gave
  was half an answer with no password, which replaced it. The test now ends
  with a whole answer, and that fault is caught.
- The tests' cleanup of leftover processes ran after the temporary folder
  was removed, and it read the process ids from a file in that folder. A
  failing test therefore left its `sleep 300` processes behind and held the
  test runner for five minutes. The cleanup is now registered first.
- The red run for "not started again after a restart" first failed by a
  30 s time-out: the command was approved again and ran. The test's client
  now refuses a second question, so the fault shows in 1 s and by name.
- With `run?.stopAll()` removed from the host's close, the tests stay green:
  the keeper sees the host's end of the pipe close in any case. The call
  only gives the order sooner. It is not a claim, and it has no red run.
- `replay` was not set on the tool. Pi Durable's default is "not safe to run
  again", which is what kept a cut-off command from starting twice. It is
  now set by name.

## M7 — Hands on this machine (bzz's side)

**Scope:** what bzz does with a folder granted for commands, with
`_bzz/approve`, and with what the host reports; the question in the TUI; Pi
packages; how the host is started; and the two things M4 left: a key in the
agents panel, and one writer for the settings. The host's own half is the
section above.

Linux x86_64, Rust 1.98.1, Node 26.5.0, Buzz pin `bf3bdd0eaefe`, 2026-11-14.
Commits `f76bfc3..11d6c91` on `v0.15.0`.

**Where it is.**

| What | Where |
|---|---|
| `exec` on a folder grant, `packages` on an agent, one writer for the file | `src/config.rs` |
| The open questions: answered once, void, nobody to ask | `src/agents/runtime/approval.rs` |
| The answers to `_bzz/approve` and `_bzz/credential`; `initialize`; the settings read again | `src/agents/runtime/agent.rs` |
| The agent's activity | `src/agents/activity.rs` |
| What the host reports at `initialize`; the model check through the host's entry | `src/agents/runtime/install.rs` |
| No inherited descriptors | `src/agents/runtime/process.rs` |
| The dialog and the line at the top; the command made visible | `src/ui/approval.rs`, `src/render/sanitize.rs`, `src/app.rs` |
| `--folder …:exec`, `--no-packages`, `agent credential`, `agent show`, `agent doctor` | `src/agents/manage.rs`, `src/cli_agent.rs` |
| The capability record of one agent | `src/agents/capability.rs` |

**How to repeat.**

```sh
. runs/env.sh
cargo build --locked --example scripted_harness
cargo test --locked --all-features --lib -- agents::runtime::approval agents::activity agents::capability \
  ui::approval render::sanitize a_command_waits a_question_is_void a_commands_edit a_harness_inherits the_limits_fit
cargo test --locked --all-features --test agent_runtime_test     # scripted harness, a recorded relay
cargo test --locked --all-features --test config_test
cargo test --locked --all-features --test agent_host_test        # the real host; needs Node and git
BZZ_RELAY_TEST_FILTER=real_relay_agent_runs_a_real_command BZZ_BUZZ_SOURCE=$PWD/runs/buzz-pin ./scripts/test-relay.sh
cd agent-host && npm ci --ignore-scripts --omit=optional && npm test
```

Red runs as in "Credentials for a private remote, bzz's side": one fault
planted, the named test seen failing as quoted, the fault removed. `A:` is a
test of `app::tests`, which drives the real App on a test terminal
(`TestBackend`) with the real agent runtime and, as the harness, a shell
script that speaks just enough of the protocol to ask.

### Folders granted for commands

| Claim | Proof | Red with the fault | Result |
|---|---|---|---|
| A folder grant carries `exec`: absent (no command), `ask` or `without-asking`. A file written before it existed means no command. Any other value is refused when the file is read, never read as a yes. It is written back as it was read | `config_test::a_folder_grant_carries_exec_and_an_agent_its_packages_switch` | `exec` never written: `left: false right: true` (the saved file lost it) | pass |
| `--folder name=/path[:rw][:exec\|:exec-without-asking]` is read from the end, apart from `:rw`; what `agent show` prints can be given back to `--folder`; a word bzz does not know is no grant | `U: manage::the_answers_are_read_as_written` | not run | pass |
| `agent edit` sets the folders and the switch for Pi packages, publishes nothing for them, and `agent show` says for each folder what it grants | `U: manage::an_edit_says_which_records_changed` | not run | pass |
| `initialize` sends `exec` for each folder, apart from `write`, and nothing that says whether the owner is asked | `T: the_host_is_told_which_folders_take_commands_and_what_it_reports_is_kept` | every folder sent with `exec: false`: `left: […"exec": Bool(false), "name": String("work")…] right: […"exec": Bool(true)…]` | pass |
| What the host answers (its version, where it offers `run`, each package loaded or left out and why) is kept in the agent's folder and printed by `agent show` and `agent doctor`; only a simple name is taken as a folder and a package's words are made safe; a grant the host does not offer is said | same; relay test below | the report not written: `a report` (`expect` on a missing file) | pass |
| The capability record of an agent says where it may run real commands, in which folders the owner is asked, that a command is not held in its folder, and whether Pi packages are loaded. An agent with no such folder has the record it had | `U: capability::an_agents_record_says_where_it_may_run_commands_and_whether_you_are_asked`, `every_harness_states…`, `the_records_say…` | the record taken from the harness alone: `assertion failed: plain.runs.contains("no folder is granted for commands")` | pass |
| The checks before a creation end with what the answers give the agent on this machine, so that it is read before the owner confirms | `H: agent_new_says_which_pi_package_names_a_missing_provider` | the notes left out: `` `In /mnt/work each command waits for your yes in the TUI.` in: `` (nothing) | pass |

### Approvals: the rules, with the scripted harness

| Claim | Proof | Red with the fault | Result |
|---|---|---|---|
| A command in a folder that asks waits for the owner. He is given the agent, the conversation, who it is answering, the folder with its real path, and the command untouched, with its lines, its tab and its terminal escape. Nothing starts while it waits; the agent's line says what it waits for | `T: a_command_runs_only_after_the_owner_allows_it` | every folder for commands treated as allowed without asking: `timed out waiting for a command to wait for the owner` | pass |
| His yes starts it: the harness is answered `{"allow": true}` and nothing of the command | same | covered by the rows on a refusal and on the TUI's yes | pass |
| His no starts nothing, and his reason reaches the agent | same | a refusal passed on as a yes: `left: Bool(true) right: Bool(false)` | pass |
| A question waits a stated time (300 s; 1 s in the test) and then counts as refused, with a reason that says so | `T: an_unanswered_command_counts_as_refused_and_a_late_answer_starts_nothing` | no time limit: the harness is answered only when the turn's own limit passes (`…was withdrawn: the turn's time limit passed…` where the refusal was expected) | pass |
| A late answer starts nothing: the question is closed when its time is up… | same; `U: approval::a_question_is_answered_once_and_a_late_answer_does_nothing` | closing leaves it in the list: `assertion failed: !approvals.close(timed)` | pass |
| …and an answer whose asker has gone is not taken | `U: approval::an_answer_whose_asker_has_gone_starts_nothing` | an answer counted as taken whoever waits: `assertion failed: !approvals.answer(id, Answer::Allow)` | pass |
| A question is void when its turn is stopped: it leaves the list at once, the harness is told it was withdrawn, a yes afterwards finds nothing, the command does not start | `T: a_waiting_command_is_void_when_its_turn_is_stopped_the_agent_paused_or_bzz_quits` | stopping a turn leaves its question open: `stop: it no longer waits` | pass |
| …when the agent is paused | same | no void at a pause nor at the end of the process: the reason recorded is `void: the turn ended` where `the agent was paused` was expected | pass |
| …and when bzz quits | same | no void at shutdown, at a stop, nor at the end of the process: `quit: […"void: the turn ended"]` | pass |
| Without the TUI nobody is asked: a command in a folder that asks is refused at once, with a reason that tells the agent to say in the conversation that its owner must open bzz | `T: without_the_tui_a_command_is_refused_and_one_allowed_without_asking_runs` | a question opened although nobody is there: it waits until the turn's limit (`` `needs your owner's approval` in: The question to your owner was withdrawn… ``) | pass |
| A folder allowed without asking runs, with the owner there or not, and is never put to him | same | treated as one that asks: `left: {"allow": false, "reason": "Nothing was started: a command in /mnt/ops needs your owner's approval…"} right: {"allow": true}` | pass |
| …but not for an agent that anyone may wake: there the owner is asked whatever the folder says, and with nobody to ask the answer is no. `agent new` and `agent edit` refuse that pair of settings | `T: a_folder_allowed_without_asking_still_asks_when_anyone_may_wake_the_agent`; `U: manage::the_answers_are_read_as_written` | the rule for who may wake it not looked at: the command is allowed (`{"allow":true}`, `left: Bool(true) right: Bool(false)`); the pair of settings accepted: `unwrap_err() on an Ok value` | pass |
| bzz refuses a command in a folder its own settings do not grant for commands, in one that is not granted at all, an empty one and one with a NUL, whatever the host was told | same | any granted folder taking commands: `docs` is allowed (`left: Bool(true) right: Bool(false)`) | pass |
| What the owner takes away while bzz runs holds at once: a folder that now asks is refused with nobody there; a grant taken away, and the same name for another folder, are refused; a settings file that cannot be read is a no; so is an agent paused in the settings | `T: what_the_owner_takes_away_is_refused_at_once_by_a_running_bzz` | the settings file not read again: round 2 is allowed; an unreadable file counted as no file: round 5 is allowed | pass |
| Several requests of the host are open at once and answered in whatever order they end: while a command waits, a credential and a chat tool of the same turn are answered, and a second conversation has a question of its own. The ids are strings (`h1`…), each answered under its own. One conversation has one question open | `T: several_requests_of_the_host_are_open_at_once_and_answered_in_any_order` | `_bzz/approve` answered in the loop that reads the host: `timed out waiting for both questions to wait` | pass |
| Two agents each have one open; an answer for one does nothing to the other; each has its own activity | `T: two_agents_may_each_have_a_command_waiting`; `A:` below | one question for the whole host: `timed out waiting for both agents to ask` | pass |
| Every question and its answer goes to the agent's activity: when, the folder, the command's first line with what would not show written out, the answer | `T: a_command_runs_only_after…` and the others; `U: activity` (2) | nothing recorded: `left: [] right: ["command in /mnt/work · cd app && (and 1 more line(s)) · asked of you", …]` | pass |
| Only bzz's own host may ask | `T: only_bzzs_own_host_may_ask_for_a_command_or_a_credential` | any harness answered: `left: Null right: Number(-32601)` | pass |

A red run that did not go red: with "a question whose time is up stays in
the list" planted in the runtime, `an_unanswered_command_counts_as_refused…`
still passed. Two things each keep a late answer from starting anything: the
question is closed, and the task that asked is gone, so an answer has nobody
to reach. The runtime test cannot tell one from the other; the two unit rows
above show each alone. The same holds for a pause: with the void at the pause
removed, the question is still void a moment later, when its turn ends, and
the test fails only on the reason that is recorded.

### Approvals: the TUI

`A: a_command_waits_for_the_owner_on_every_screen_and_runs_only_on_his_yes`,
unless another test is named.

| Claim | Proof | Red with the fault | Result |
|---|---|---|---|
| A waiting approval is visible from anywhere: a line at the top of the workspace, with the agents directory, a search, the broker panel, the agents panel or the help open, and in the Inbox. It names the agent, the folder, the conversation, how many more wait, the key, and the time left | the test; `U: ui::approval::the_banner_says_who_asks_where_and_how_to_answer` | the line drawn only when no panel is open: `` `:agents directory`: … `` (the first row has no such line) | pass |
| The dialog never opens by itself, and a plain `y` or `a` typed anywhere answers nothing: only `Ctrl-Y` or `:approvals` opens it | the test | `y` without Ctrl opening it: `assertion failed: !world.app.global_key(key(KeyCode::Char('y')))` | pass |
| The dialog shows the agent, the conversation and who is being answered, the folder with its real path, that the command runs as the owner, the time left, and the command whole: each line numbered, the rest of a long line without a number | the test; `U: ui::approval::a_command_is_shown_whole_with_its_lines_and_what_would_not_show`, `the_dialog_shows_who_asks…` | only the first line shown: `left: [Some(1)] right: [Some(1), Some(2), None, Some(3), None, None, Some(4)]`, and `` ` 2 │   npm test -- --watch=false\t# \x1b[2K` `` is not on the screen | pass |
| Anything unprintable is made visible: a terminal escape, a carriage return, a tab, a right-to-left override, a zero-width and a no-break space are written out; an escape is never cut in two by a row's end; nothing of the command is lost | same; `U: render::sanitize::what_would_not_show_is_written_out` | the command drawn as it is: `left: "  npm test\t# \u{1b}[2K\rrm -rf ~" right: "  npm test\\t# \\x1b[2K\\rrm -rf ~"`; only control characters written out: `left: "a\u{202e}b\u{200b}c…"` | pass |
| His yes starts the command: the harness reads `{"allow":true}` under its own id and the command starts; the dialog closes on what was open before | the test | the dialog's yes passed on as a refusal: `left: Some("refused: Scout was told no, and nothing was started") right: Some("allowed: Scout runs its command in /mnt/work")` | pass |
| A yes is taken only after the end of the command has been on screen: for one of 60 lines, `a` answers nothing and says to scroll | the test; `U: ui::approval::the_dialog_shows_who_asks…` | a yes taken at once: the dialog closed (`left: None right: Some(Approval)`) | pass |
| His no, with a reason he writes, starts nothing: the harness reads `Your owner refused this command, and says: not on a Friday`. While he writes, every key is a letter of the reason | the test | covered at the runtime (a refusal passed on as a yes) | pass |
| A question that goes away while it is looked at says so, and a yes then starts nothing and says that nothing was started | the test | not run | pass |
| Two agents each have one open, and the second is the one shown after the first is answered | the test | covered at the runtime | pass |
| Locking bzz leaves no question behind: it is void, a yes afterwards finds nothing, the command does not start, the line is gone | `A: a_question_is_void_when_bzz_locks_and_the_panel_adds_an_agent_to_the_open_channel` | covered at the runtime (void at shutdown) | pass |

### With the real host, and on the pinned relay

`H: the_real_host_runs_a_command_only_with_the_owners_yes`: bzz's real Pi
Durable host on its stand-in model, a recorded relay, the owner's answers
given through `AgentHost::answer_approval`, which is what the TUI's keys
call.

| Claim | Proof | Red with the fault | Result |
|---|---|---|---|
| An approved command runs in the granted folder exactly as the owner saw it (two lines, a quote, a redirection) and its output reaches the chat; the file it made is on the machine. The host reports `run` in the two folders granted for commands | `H:` | bzz answering every command with a refusal: `its output reached the chat: ["The command was not allowed: no\nIt was not started."]` | pass |
| A refused command does not run, and the agent tells why | `H:` | a refusal passed on as a yes: `the agent was told why: ["done"]` (the command ran) | pass |
| A folder allowed without asking is not put to the owner; the host does not even ask about a folder without `exec` | `H:` | covered with the scripted harness | pass |
| Without the TUI a command that needs approval is refused, and the reply in the chat says the owner must open bzz; the folder allowed without asking still works | `H:` | a question opened with nobody there: `the request never settled`, the agent's status `waiting for your answer: run in /mnt/work: touch headless.txt` | pass |

**On the pinned relay**
(`real_relay_agent_runs_a_real_command_and_clones_a_private_remote` in
`tests/relay_integration.rs`, 2026-11-14, 88 s alone). The agent is made by
`bzz agent new Scout --folder work=…:exec --folder ops=…:exec-without-asking
--allow-host <remote>` and added by `bzz agent add`; its requests come from
its own relay connection; the host is the real one.

| Claim | Observed | Result |
|---|---|---|
| `agent new` prints, with the capability record, where the agent has hands | `real commands of this machine, as your user, started in /mnt/work, /mnt/ops`, `In /mnt/work each command waits for your yes in the TUI.`, `Warning: in /mnt/ops commands run WITHOUT asking you`, `The folder is where it starts, not a wall` | pass |
| `agent show` before any start | both folders with their rights, `git credentials: none stored`, `its host: not started yet`, no activity | pass |
| With the owner there (the runtime as the TUI runs it, `owner_present`): a mention asks for a command of two lines; it waits; the owner is given that command, the agent, the folder, the channel and himself as the one answered; nothing has run; after his yes the reply on the relay holds the command's output and the folder's path, and the file it made is in the folder | `made by a real command` and the path, in the agent's one reply | pass |
| A refused command does not run, and the reply says why | `Your owner refused this command, and says: not in this folder`; no `refused.txt` | pass |
| A clone from the private remote, without and with a stored credential | see "Credentials for a private remote, bzz's side" | pass |
| Without the owner (`bzz agent run`, the binary): a command in the folder that asks is refused, and the reply tells the people in the chat | `needs your owner's approval`, `your owner must open bzz to approve the command`; no `headless.txt` | pass |
| …and one in the folder allowed without asking runs | `restarted`; `done.txt` in the folder | pass |
| `agent show` beside the running bzz: the host a credential is stored for, what the host reported, every question with its answer; `agent doctor` likewise | `real commands: the `run` tool, in /mnt/work, /mnt/ops`, `Pi packages: none named in Pi's settings`, eight activity lines from `asked of you` to `allowed without asking` | pass |
| The secret scan over the whole profile; `agent delete` | no file holds the token or the passphrase; `removed: its 1 stored git credential(s)` | pass |

Red on the relay, one run, the same test in a clone of the repository with
"a refusal is passed on as a yes" planted: `panicked at tests/relay_integration.rs:5544:5: done`. The command the owner
refused ran, and the agent's reply in the chat was `done` where his refusal
was expected.

### Pi packages, and the model check

| Claim | Proof | Red with the fault | Result |
|---|---|---|---|
| An agent carries `packages`, on unless it says `false`; `agent new --no-packages` and the form's tenth question set it, `agent edit --packages on\|off` changes it; `initialize` sends it, `true` or `false` | `config_test::a_folder_grant_carries_exec…`; `U: manage::the_answers_are_read_as_written`, `an_edit_says…`; `T: the_host_is_told…` (`"packages": false`) | not run for the switch alone; the `initialize` row above fails on the same assertion block | pass |
| The model check is the host's own entry, `node main.mjs --check-model`: one line of JSON, whatever a package prints; a model of a provider that only a package brings can be used; the four reasons are told apart; asking creates nothing in Pi's folder | `agent-host/test/packages.test.mjs` "`main.mjs --check-model` answers…"; `H: the_real_host_keeps_to_the_grants_and_names_its_failures` | the entry never loading the packages: `actual: { ok: true, … packages: [] } expected: { … packages: [ [Object], …5 ] }` | pass |
| `agent new` says, when the chosen model's provider is missing, which Pi package names it without a Pi Durable entry; or that none does | `H: agent_new_says_which_pi_package_names_a_missing_provider` (packages written by the test) | what the host reports of the package dropped: `` `bzz found the Pi package …/pi-acme-cloud in Pi's settings` in: … `` | pass |
| With packages off for the agent, the check loads none, and bzz says they are off | same | the switch not passed on: the model of a package's provider is found (`unwrap_err() on an Ok value: "…model acme/acme-1…"`) | pass |
| A failed turn has a code of the host's own, `-32010`, which bzz does not read as ACP's "sign in"; with a kind, the kind decides for either code | `agent-host/test/acp.test.mjs` "a provider that fails during the turn…"; `U: runtime::failures_are_named_by_kind_from_code_and_wording` | the host still answering `-32000`: `-32000 !== -32010`; bzz reading `-32010` as a missing sign-in: `left: SignInNeeded right: ProviderError` | pass |

**Found on the way:** the check of a model before an agent exists never
loaded the Pi packages. It imported `src/model.mjs` and asked with Pi's own
providers only, so a model whose provider a package brings was always
reported as missing by `agent new --model`, although an agent would have run
on it. Moving the question to the host's entry showed it.

### Starting the host

| Claim | Proof | Red with the fault | Result |
|---|---|---|---|
| A harness gets its three streams and no other open descriptor, also none bzz itself was born with: a descriptor left open and unmarked in bzz, number 200 or above, is not open in the harness, and stays open in bzz | `U: process::a_harness_inherits_no_descriptor_but_its_three_streams` | the marking not done before the start: `left: "0 open,1 open,2 open,200 open," right: "0 open,1 open,2 open,200 closed,"` | pass |
| `initialize` has a time limit of bzz's own, 30 s, longer than the 10 s the host gives its packages; a harness that does not answer is stopped and counted | `U: runtime::the_limits_fit_what_the_host_and_the_owner_need`; `T: a_harness_that_stalls_at_start_is_stopped_and_counted` (M1) | the limit set to 10 s: `10s` | pass |
| Requests of the host with string ids, several open, answered in any order | `T: several_requests_of_the_host…`, above | above | pass |
| `HOME`, `PATH`, `LANG` and `TMPDIR` and no `NODE_*` variable | `H: the_real_host_is_installed_started_and_answers_once…` (M2, unchanged: the host's environment read from `/proc`) | M2 | pass |

On descriptors, what was and was not done:

- **Linux and macOS:** before a harness, `npm` or a probe of `node` is
  started, bzz marks every descriptor it holds above 2 as not to be handed on
  (`close_fds::set_fds_cloexec_threadsafe(3, &[])`). That covers what bzz
  inherited and what a C library opened. The test above ran on Linux only.
  macOS is the same call and has not run.
- **What it leaves:** a descriptor another thread opens without the mark
  between that call and the start of the child.
- **Windows:** nothing is done. A handle is handed on only when it is
  inheritable, and Rust makes none that way; one bzz itself inherited as
  inheritable cannot be named in safe Rust. The host offers no `run` there.
- `unsafe` stays forbidden: safe Rust cannot name a descriptor it did not
  open, so one dependency was added for this, `close_fds` 0.3.

### What M4 left: a key in the panel, and one writer for the settings

| Claim | Proof | Red with the fault | Result |
|---|---|---|---|
| `i` in `:agents` adds the selected agent to the channel that is open, by the call `bzz agent add` makes; with no channel open it says so; the owner is told how it ended | `A: a_question_is_void_when_bzz_locks_and_the_panel_adds_an_agent_to_the_open_channel` (no relay answers there: the notice is the failure, named) | the key doing nothing: `left: [] right: [(<agent>, <channel>)]` | pass |
| **One writer for the settings.** A copy that saves writes only what it changed since it last read or wrote the file, over what the file holds then. A command's edit of an agent, and an agent it created, survive a later save by a copy that was open all along, also when that copy changed the same agent; and the reverse | `config_test::a_save_does_not_write_over_what_another_bzz_saved_meanwhile` | a save writing its own copy whole: `the command's edit of an agent survived the TUI's save… left: ([], Paused) right: (["github.com"], Paused)` | pass |
| …in the running App: `bzz agent edit Scout --allow-host … --packages off` from another terminal, then `Space` on two agents in the TUI's panel, leave the edit and both pauses in the file; and the TUI's change survives a command that had read the file before it | `A: a_commands_edit_survives_a_save_by_the_running_tui_and_the_reverse` | the same fault: `left: ([], true, Paused) right: (["github.com"], false, Paused)` | pass |
| An agent either side deleted stays deleted, whatever the other changed in it; a setting both changed takes the later save's value, and only that setting; two changes that do not fit together are not saved; a copy that never read the file writes itself whole | `config_test::a_save_does_not…` | same test | pass |
| Two saves at the same moment both land | `config_test::two_saves_at_once_both_land` (20 rounds, two threads) | no lock taken: `round 0: neither save wrote over the other left: (20, 44) right: (20, 31)`, or the file missing for the reader | pass |

The command in these tests is the code the command runs (`Config::load`, the
edit, `Config::save`), in the test's own process. The bzz binary beside a
running TUI was not run.

### The plan's exit gate for M7

"An approved command runs in a granted folder and its result is on the
agent's disk; a refused one does not run."

- [x] An approved command runs in a granted folder, with the real host, on
  the pinned relay, and its output reaches the chat; a refused one does not
  run.
- [x] Its result is with the agent: a short output is in its conversation, a
  long one on its disk, and a file is brought there by the agent's own `cp`.
  That half is the host's (`run.test.mjs`, the section above).
- [x] The approval rules, with the scripted harness; the question in the TUI,
  with the real App on a test terminal.
- [ ] **Approve and refuse in the TUI on the release binary** (the plan's
  Herdr column): not run.

### Not proven for bzz's side of M7

| Item | State |
|---|---|
| One journey with everything real: the release TUI, a key pressed by a person, the real host, the relay | not run. The TUI is proven with a shell script as the harness; the real host and the relay with the answer given through the call the TUI's keys make |
| macOS and Windows | not compiled or run here. The tests that start processes are Unix only. On Windows the host offers no `run`; the grant is kept and `agent show` says it is not offered |
| Descriptors on macOS | the same call as on Linux; not run |
| A real Pi package, a real model, a real forge | none ran: packages are written by the tests, the model is the stand-in, the remote is on this machine |
| A real workload | the commands in the tests take milliseconds. A build of minutes with the owner away, and a turn that reaches its ten minutes while a command runs, did not run |
| A letter that looks like another one | not made visible; the dialog only says when a command holds characters outside ASCII |
| A very wide or very small terminal | the dialog was drawn at 110×20 and 150×40. On a terminal too small for the head of the dialog the end of a command is never "seen", so it cannot be allowed there |
| The bzz binary editing the settings beside a running TUI | the same code, in one process; see above |
| A settings file edited by hand while bzz runs | read again before each command and each credential; a file that does not parse is a no until it does |
| `Ctrl-Y` bound by a user's keymap | it is taken by bzz only while a command waits |
| The owner's answer from another machine, or from the chat | not built, by decision (ADR, "Rejected") |

### Found on the way

- **The model check never loaded Pi packages** (above).
- **Two relay harness runs started in the same second**, mine and another
  agent's, each having waited for the harness to be free. Both failed while
  the database was set up (`duplicate key value violates unique constraint
  "pg_extension_name_index"`, `connection refused`), and the cleanup of mine
  removed the containers of both. No test had run. `scripts/test-relay.sh`
  has no lock: two runs on one machine must be kept apart by hand.
- **A test that is started while another writes a program** failed once in
  seven runs of the unit tests: `NodeMissing` for a stand-in `node` that had
  just been written. A program that is open for writing in any process
  cannot be started ("text file busy"), and a test thread that starts a
  process holds, for a moment, the descriptors of every other thread. bzz
  now tries such a start again (`install::run_to_end`); twelve runs in a row
  passed afterwards. The cause was not seen directly, only that the failures
  stopped.
- **Ten copies of the scripted-harness suite at once** fail now and then on
  two tests of M1 and M4 that measure time
  (`the_owner_stops_a_running_turn_and_it_ends_once`, "its process stays";
  once `at_startup_leftovers_are_stopped…`): 3 of 50 runs with this work, 1
  of 50 with the code before it (`0626ddb`), on a machine with a load of 40
  to 70. One copy at a time: 30 of 30, and the unit tests 25 of 25. M1's
  record says 100 runs, ten at once, without a failure; that did not repeat
  here. A longer grace for the cancel did not change it, so its cause is
  not known. No test of this work failed in those runs.
- A red run for "a clone works with the stored credential" did not compile
  (a `match` left without an arm), which is no red run. It was planted again
  another way.
- The first red run for the secret scan with the real host failed on the
  activity line, before the files were searched. A second fault, the token
  written to a file of the data folder, shows the search itself.

### Gates after M7 (bzz's side)

Run on `11d6c91`, 2026-11-14, Linux x86_64, Node 26.5.0. The agent host's
own tests ran on `d902505`; `agent-host/` has not changed since `f76bfc3`.

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --check` | clean |
| Lint | `cargo clippy --locked --all-targets --all-features -- -D warnings` | clean (also `--no-default-features`) |
| Tests | `cargo test --locked --all-features` | 644 passed, 0 failed, 26 ignored. 34 passing tests are new since M4: 15 unit (3 App, 3 `ui::approval`, 2 `approval`, 2 `activity`, 1 each of `git_credentials`, `capability`, `process`, `runtime`, `sanitize`), 11 scripted harness, 3 real host, 3 configuration, 1 creation, 1 secret scan |
| Pinned relay | `BZZ_BUZZ_SOURCE=$PWD/runs/buzz-pin ./scripts/test-relay.sh` | 19 passed (the 18 of M4 and the journey above) in 456 s, then `agent_host_test`, 6 passed. It had passed the same way on `d902505`, before the last change. The other agent's work uses the same harness: the run waited for it to be free |
| Agent host | `cd agent-host && npm ci --ignore-scripts --omit=optional && npm test` | 99 tests, 99 pass, 0 skipped (the 98 of the second pass and the one for `--check-model`); `npm ci` reports 0 vulnerabilities |
| Supply chain | `cargo deny check`; `cargo audit` | `advisories ok, bans ok, licenses ok, sources ok`; the 4 allowed warnings of v0.14. `Cargo.lock` gains one package, `close_fds` 0.3.2 (MIT), whose own dependencies were there already |

## M6 — The other harnesses

**Scope:** Grok, Codex and Claude Code as harnesses: finding and starting
the owner's own program, the two adapters, the guard bzz sets, the answer to
every permission request, version ranges, creation and the records, and the
tests: with a scripted harness, with transcripts recorded from the real
programs, and with the real programs on the pinned relay.

Linux x86_64, Rust 1.98.1, Node 26.5.0, Buzz pin `bf3bdd0eaefe`, 2026-11-14.
Commits `2a77e06..` on `v0.15.0`. The programs: Grok Build 1.0.77, Codex
0.166.0 with `codex-acp` 2.5.0, Claude Code 2.1.322 with `claude-agent-acp`
0.101.0, each with the owner's own sign-in.

**Where it is.**

| What | Where |
|---|---|
| Each program: where it is, its versions, its launch, its guard, how it takes a session; the adapters and their install; what `doctor` says | `src/agents/runtime/coding.rs` |
| The lock files of the two adapters | `agent-adapters/claude-code/`, `agent-adapters/codex/` |
| What a request asks leave for, and the rule: paths, commands, never "always" | `src/agents/runtime/permission.rs` |
| The guard on a session, the answer to a request, a command put to the owner | `src/agents/runtime/agent/guard.rs` |
| The launch, the session, the tool calls a harness reported, a turn that ends after a refusal | `src/agents/runtime/agent.rs` |
| A transcript of one harness process | `src/agents/runtime/record.rs`, `src/agents/runtime/acp.rs` |
| The capability records | `src/agents/capability.rs` |
| Creation, the checks before it, what a harness lacks, `agent show`, `agent doctor` | `src/agents/manage.rs`, `src/cli_agent.rs` |
| A coding harness's command in the dialog | `src/ui/approval.rs`, `src/agents/runtime/approval.rs`, `src/agents/activity.rs` |
| bzz's memory kept from other processes | `src/agents/runtime/process.rs` |
| The scripted harness as a coding program, and as a player of transcripts | `examples/scripted_harness.rs` |
| The transcripts | `tests/fixtures/agents/transcripts/<harness>/` |

**How to repeat.**

```sh
. runs/env.sh
cargo build --locked --example scripted_harness
cargo test --locked --all-features --lib -- agents::runtime::coding agents::runtime::permission \
  agents::runtime::record agents::capability agents::activity ui::approval memory_tests
cargo test --locked --all-features --test agent_coding_test    # the rules, the launches, the transcripts
cargo test --locked --all-features --test agent_manage_test    # creation
cargo test --locked --all-features --test secret_scan_test     # the three launches
# With your own grok, codex and claude, signed in. Three short prompts of
# each, on the cheapest model. Never part of a default run or of CI:
BZZ_REAL_HARNESSES=grok,codex,claude-code BZZ_RELAY_TEST_FILTER=real_relay_coding_harnesses \
  BZZ_BUZZ_SOURCE=$PWD/runs/buzz-pin ./scripts/test-relay.sh
```

To record the transcripts again, the same command with a folder for them:

```sh
BZZ_AGENT_RECORD=$PWD/target/m6-transcripts BZZ_REAL_HARNESSES=grok,codex,claude-code \
  BZZ_RELAY_TEST_FILTER=real_relay_coding_harnesses BZZ_BUZZ_SOURCE=$PWD/runs/buzz-pin ./scripts/test-relay.sh
# target/m6-transcripts/<harness>/ then holds two files per harness, in the
# order they were written: copy the first to
# tests/fixtures/agents/transcripts/<harness>/first-run.ndjson and the second
# to after-restart.ndjson. Read them before you commit them: they hold what a
# model wrote.
```

Red runs: one fault planted in bzz, the named test seen failing as quoted,
the fault removed (`git checkout`). All of them were run on `89dfe92` by one
script, 35 runs, each of which failed. `U:` is a unit test, `C:`
`tests/agent_coding_test.rs`, `M:` `tests/agent_manage_test.rs`, `S:`
`tests/secret_scan_test.rs`. `replay` is
`C: bzz_answers_what_each_real_program_sent_as_it_did_when_recorded`.

### Finding and starting the program, and its version (points 1 and 4, D16, D17)

| Claim | Proof | Red with the fault | Result |
|---|---|---|---|
| The version is read as each program prints it | `U: coding::versions_are_read_as_each_program_prints_them` | not run | pass |
| A version outside the range is refused by the checks before a creation, by `doctor` and by the start, each saying the version found, the versions known and what to do. Nothing is started | `U: coding::a_program_is_found_asked_for_its_version_and_held_to_its_range`, `…a_version_outside_the_tested_range_is_refused_with_both_versions`; `M: an_agent_on_a_coding_harness_is_checked_before_its_key_is_made`; `C: bzz_starts_the_owners_own_grok_and_only_a_version_it_knows` | the range check always yes: ``called `Result::unwrap_err()` on an `Ok` value: Ready {…`` (unit); `…on an `Ok` value: "Grok Build 1.1.0 at …"` (creation); `left: None right: Some("unavailable")` (the start: the agent answered) | pass |
| A missing program starts nothing; the owner reads which program, how to sign in and which variable names another copy; the people in the chat are told the agent is not available | same three; `M: the_checks_before_a_creation_say_what_to_do` | covered by the row above for the start | pass |
| Grok is started as `grok agent --no-leader stdio`, in the agent's working folder, with `GROK_SANDBOX` and nothing of bzz's | `C: bzz_starts_the_owners_own_grok…` (a stand-in `grok` that becomes the scripted harness) | a key of bzz's environment handed on: `{"argv":[…"agent","--no-leader","stdio"],…"env":{"ANTHROPIC_API_KEY":"mu…` | pass |
| Codex and Claude Code are started as `node <adapter entry>`, with the owner's own program in `CODEX_PATH` or `CLAUDE_CODE_EXECUTABLE`, links followed | `C: an_adapter_is_installed_at_the_first_start_and_pointed_at_the_owners_program` | below | pass |
| On Linux Grok needs bubblewrap, and bzz says so before it starts a Grok that would stop | `U: coding::a_program_is_found…` | not run | pass |

### The adapters (point 2, rule 13)

| Claim | Proof | Red with the fault | Result |
|---|---|---|---|
| **Both adapters run without their optional packages.** Installed with `npm ci --ignore-scripts --omit=optional --omit=dev` they are 77 MB (107 packages) and 21 MB (24 packages), against 836 MB with the copies of `claude` and `codex`; both answered `initialize` and `session/new`, and then whole turns, when pointed at the owner's programs | by hand, then the real runs below | none: it is a finding | seen |
| The lock files pin exactly the versions the code names, and every package comes from the npm registry with a hash | `U: coding::the_lock_files_pin_the_adapters_the_code_names` | not run | pass |
| An adapter is installed once, by a rename of a whole install, also when two bzz start it at once; a failed or half install is never used and leaves nothing; npm runs as rule 13 says with nothing of bzz's in its environment | `U: coding::an_adapter_is_installed_once_whole_and_at_the_pinned_version` | below | pass |
| An install that holds another version than the pinned one is not kept | same | the version not compared: ``called `Result::unwrap_err()` on an `Ok` value: Ready { spec: ProgramSpec { harness: Codex…`` | pass |
| A missing adapter is installed at the first start; when it cannot be (no npm), nothing starts and the owner reads why | `C: an_adapter_is_installed_at_the_first_start…` | an adapter taken as installed without looking: `ClaudeCode left: None right: Some("unavailable")` (a harness was started from a folder that is not there) | pass |
| `agent doctor` reports each program, its version and range, its adapter and which sign-in is in use, and opens no session | `U: coding::an_adapter_is_installed_once…` (the report), `…a_program_is_found…`; on the relay below | not run | pass |
| `npm audit --omit=dev --omit=optional` on both lock files | by hand, 2026-11-14; a CI job (`agent-adapters`) | none | 0 vulnerabilities |

### The guard, before a session is prompted (point 3)

| Claim | Proof | Red with the fault | Result |
|---|---|---|---|
| The mode is set on every session before its first prompt, on a new session and on a loaded one: `default` for Claude Code, `workspace-write` for Codex, whatever mode the session starts in (the scripted harness starts in `auto`). Also when a loaded session lists no modes | `C: the_mode_is_set_before_the_first_prompt_on_a_new_and_on_a_loaded_session`; `replay` | `session/set_mode` not sent: `no session/set_mode number 0 in ["initialize", "session/new", "session/prompt"]`; `replay`: `Codex, a mention: bzz did not send what it sent when this was recorded` | pass |
| A session that does not take the mode is never prompted: the mode is not on its list, it has no modes, or it refuses. The chat is told, the owner reads why, and no credential stays behind | `C: a_session_that_does_not_take_the_mode_is_never_prompted` | a refusal of the mode passed over: `it has no modes at all left: None right: Some("not-guarded")` (the agent was prompted and answered) | pass |
| Grok is sent no mode. Its guard is a profile bzz writes in its working folder before every start, **in place of whatever the agent left there**, with the folders granted for writing and bzz's keys hidden, and the variable that names it | `C: the_mode_is_set…` (last part); `U: coding::the_guard_is_in_the_environment_and_the_profile_replaces_what_was_there`, `…groks_profile_extends_workspace…` | the profile not written: `left: "[profiles.bzz-agent]\nextends = \"devbox\"\nread_write = [\"/\"]\n"` (what the agent had left was still there) | pass |
| The model the owner chose is set on the session before the first prompt, and the thinking level; a model the program does not offer ends the turn with a notice and the owner reads which it offers; a thinking level it lacks is passed over | `C: the_owners_model_is_set_on_the_session_or_the_turn_says_it_is_not_offered`; `U: coding::a_model_is_set_only_when_the_harness_offers_it` | a model that is not offered passed over: `left: None right: Some("model-unavailable")` (another model answered) | pass |
| The rules of the chat, the instructions and the folders go to each program in the way it takes them: `systemPrompt` and `additionalDirectories` for the adapters, `_meta.rules` for Grok; Codex is told only of folders it may write; Claude Code is told to load none of the owner's settings; a loaded session is told the same; the instructions then travel in no message | `C: a_session_is_told_its_prompt_and_its_folders_as_its_program_takes_them`; `U: coding::a_session_is_told_its_prompt_and_folders…`, `…the_system_prompt_says_where_the_agent_works…`; `replay` | the options for Claude Code left empty: `left: Object {} right: Object {"settingSources": Array [], "strictMcpConfig": Bool(true)}`; `replay`: `ClaudeCode, a mention: bzz did not send what it sent…` | pass |

### What a harness asks leave for (point 3)

| Claim | Proof | Red with the fault | Result |
|---|---|---|---|
| Each kind of request is read as the three programs send it: the chat tools under their three names, an edit with its path in three places, a read, a command as text or as words with where it starts; anything else is unclear | `U: permission::each_kind_of_request_is_read_as_the_three_programs_send_it` | not run | pass |
| **A file edit is allowed only inside the working folder and folders granted for writing.** Refused: a folder granted for reading, any other folder, the agent's own folder above its working folder, a sibling whose name starts alike, a link that leaves (to a folder, to a file, to nowhere), a path with `..`, a path that is not absolute, no path, and two paths of which one is outside | `U: permission::an_edit_is_allowed_only_inside…`; `C: each_thing_a_coding_harness_asks_leave_for_is_answered_by_rule` (23 requests in one turn); `replay` | every path taken as inside: `assertion failed: read(docs.join("a.md")) && !write(docs.join("a.md"))`; `an edit in a folder granted for reading left: "allow-once" right: "reject"`; `replay`: `Codex, the two writes: bzz did not send what it sent…` | pass |
| bzz's own chat tools are allowed; a read, inside the working folder and any granted folder; a fetch, a deletion, a tool of another server, a command dressed as the chat tool, a request with no kind: refused | `C: each_thing…` | covered by the rows above and below | pass |
| **Never "always".** The option that allows once or refuses once is chosen by its kind; with only "always" on offer the request is cancelled, also for an edit bzz would allow | `U: permission::an_answer_is_once_by_kind_and_never_always`, `agent::only_the_chat_tools_are_allowed_and_only_once`; `C: each_thing…` | "always" chosen for a yes: `left: …"optionId": String("always")… right: …String("yes")`; `bzz's chat tool, as Claude Code names it left: "allow-with-updates" right: "allow-once"` | pass |
| What is refused is in the agent's activity with its path and the reason; what is allowed is not | `C: each_thing…`; `U: activity::a_question_and_its_answer_are_kept_as_one_line_each` | not run | pass |
| A request that names only the id of a call (Codex) is read from what the harness reported under that id: bzz's chat tool is allowed, a tool of another server and a call never reported are not, and a file change is held to the files of the reported diffs. A file's text is not kept | `U: permission::a_request_that_names_only_its_call_is_read_from_what_was_reported`; `C: a_request_that_names_only_its_call_is_answered_from_what_was_reported`; `replay` | nothing kept of a reported call: `left: ["cancel", "cancel", "cancel", "cancel", "cancel"] right: ["allow_once", "cancel", "cancel", "allow_once", "cancel"]`; `replay`: `Codex, a mention: bzz did not send…` (bzz refused Codex its own `send`) | pass |
| **A shell command follows the folders granted for commands and the owner's answer.** With none granted it is refused. With one that asks, the owner is given the command, where it would start and that a coding harness asks; his yes allows it once and his no refuses it; each step is in the activity | `C: a_command_runs_only_by_the_folders_granted_and_the_owners_answer`; `U: permission::a_command_follows_the_folders_granted_for_commands` | the owner never asked: `timed out waiting for a question for the owner`; a command with no grant let through: `left: Ok(CommandPlace { folders: [], … asks: false }) right: Err("no folder is granted for commands")`, `a shell command, with no folder granted for commands left: "allow-once" right: "reject"` | pass |
| With nobody at bzz a folder that asks refuses; a folder allowed without asking lets it run; one folder that asks among several decides for a command that starts in the working folder; an agent anyone may wake is always asked about; a command that would start in a folder not granted for commands, or anywhere that is neither the agent's nor granted, is refused | `C: a_command_runs_only…` (seven cases) | covered by the row above | pass |
| The dialog shows a coding harness's command with its real folder, as its program reports it, and says that the program starts it | `U: ui::approval::a_coding_harness_command_is_shown_with_its_real_folder_and_whose_word_it_is`, `…the_banner_says…` | not run | pass |
| A turn that ends with nothing after a refusal tells the chat that the agent needed something its owner has not allowed; without a refusal it says only that there was no answer | `C: a_turn_that_ends_with_nothing_after_a_refusal_says_so` | the refusal not noted on the turn: `left: Some("no-reply") right: Some("not-allowed")` | pass |
| What the owner takes away while bzz runs is refused at once: a folder's writing and its commands | `C: what_the_owner_takes_away_from_a_coding_harness_is_refused_at_once` | the settings file not read again: `left: ["allow-once", "allow-once"] right: ["reject", "reject"]` | pass |

### Creation and the records (point 5, D18, D19, rule 12)

| Claim | Proof | Red with the fault | Result |
|---|---|---|---|
| `agent new --harness grok\|codex\|claude-code` checks the program, its version and the adapter (installing it) before a key is made, and says which program, which sign-in is in use, when the model is checked and where its sessions also appear | `M: an_agent_on_a_coding_harness_is_checked_before_its_key_is_made`; on the relay below | above (the range) | pass |
| What these harnesses do not have is refused where it would be set, with the reason: a list of hosts, Pi packages (on or off), a folder for commands for Grok, in `agent new` and in `agent edit`; a git credential; a move with memory (`manage::lacks`, for the command that moves) | same; `M: a_creation_that_fails_on_this_machine_publishes_nothing` | the check always yes: ``called `Result::unwrap_err()` on an `Ok` value: Created {…`` (an agent with a list of hosts was made) | pass |
| A model is written as the program names it, with or without the harness before it | same | not run | pass |
| The records: offered; no disk, no git without a process, no move; D19's words; for each rule who holds it; what each can read apart from what it can write; what the owner's own settings give it; with which version and system it was seen. D18's warning exactly where there is no sandbox: Claude Code, and Grok on Windows | `U: capability::every_harness_states…`, `…the_records_say…`, `…an_agents_record_says…`; `M: an_agent_on_a_coding_harness…` (`agent show`) | Claude Code given a sandbox: `assertion failed: record_on(Harness::ClaudeCode, false).warning.is_some()`; `` `Warning: this harness has no sandbox of its own here…` in: `` (`agent show` without it) | pass |
| Codex and Claude Code are marked experimental wherever a harness is shown: the record, `agent show`, `agent list`, `agent doctor`, the panel, the status line | `M: an_agent_on_a_coding_harness…` (`harness_label`, `agent show`) | not run | pass |

### The key rule (rule 10)

| Claim | Proof | Red with the fault | Result |
|---|---|---|---|
| The secret scan covers the three launches as bzz makes them (the program found on `PATH`, the adapter installed, Node): no agent key, owner key or broker credential in a harness's environment, arguments, messages, tool results, log, folder, sandbox profile, adapter install, the database or a transcript. Its environment holds the four variables, its guard, where its adapter finds the program, and the owner's proxy: nothing else, and no API key of bzz's environment | `S: no_key_reaches_a_harness_environment_arguments_or_logs`; `U: coding::the_guard_is_in_the_environment…`; `C: bzz_starts_the_owners_own_grok…`, `…a_session_is_told…` | three API keys handed on: `a key of bzz's environment leaked into grok harness log`; `[("GROK_HOME", "/srv/grok"), ("XAI_API_KEY", "must not pass"), …` | pass |
| A transcript holds no session credential, no path of the machine, no notification's content but `session/update`, nothing of an answer that bzz does not read, and nothing a tool gave back | `U: record::a_transcript_holds_no_credential_no_path_and_nothing_bzz_does_not_read`; `S:` above | the credential not written over: `` `bzzs_secret` in: `` …; `a session credential leaked into …/transcripts/grok-….ndjson` | pass |
| On Linux a bzz that runs agents cannot be read by another process of the same user: its memory files are root's, and a `sh` it starts cannot read its environment | `U: process::memory_tests::a_bzz_that_runs_agents_is_not_readable_by_other_processes_of_the_user` | the call left out: `left: Dumpable right: NotDumpable` | pass |

### Recorded transcripts

`tests/fixtures/agents/transcripts/<harness>/first-run.ndjson` and
`after-restart.ndjson`, six files, 227 KB: what Grok 1.0.77, Codex 0.166.0
and Claude Code 2.1.322 sent over ACP on 2026-11-14 in the last run on the
pinned relay below (`b5515ac`), and what bzz sent them. For each program: one answered mention;
what it did and asked when told to write inside and outside its folders; and,
in a second process, its saved session loaded and asked what it remembered.

The test (`replay`) runs bzz's real runtime with the scripted harness in the
program's place. The harness sends what the program sent, calls bzz's tools
through the real `bzz agent-tools` where the program did, and ends at the
first message of bzz that is not what bzz sent when it was recorded. So CI,
which has no sign-in, tests bzz against each program's real messages. Four of
the red runs above are caught by it and by nothing else of the real
programs.

What the transcripts hold that a hand-written script did not:

- Codex asks leave for a tool of an MCP server by the id of a call it
  reported before, with `kind: execute`; and for a file change with
  `kind: edit` and the file in `locations`.
- Codex ends its turn (`stopReason: cancelled`) when a request is refused.
- A loaded session of each program sends its past again, tool calls and
  all, before `session/load` is answered. bzz ignores it: no reply is
  published twice.
- A loaded Codex session comes back on the owner's default model, and says
  so in an `agent_message_chunk`; bzz sets the model again before the
  prompt.
- Grok sends more than fifty notifications of its own (`_x.ai/…`) around
  one turn; Claude Code asks leave for bzz's own tool every time.

They are as the recorder wrote them, with one change by hand: two sentences
of a Codex warning that named a setting in the owner's own Codex
configuration. An earlier recording showed why the recorder keeps nothing a
tool gave back: Grok's search for bzz's tools listed the owner's own tools
beside them.

### With the real programs, on the pinned relay

`real_relay_coding_harnesses_answer_once_remember_and_stay_in_their_folders`
(`tests/relay_integration.rs`). It runs only with `BZZ_REAL_HARNESSES`;
without it, it prints that it was skipped and why. An agent is made by
`bzz agent new --harness <name>` with a folder granted `:rw`, added to a
channel, and run by the runtime in the test's process as the TUI runs it,
with nobody to ask. Models: `grok-4.5` (low), `gpt-5.6-luna` (low), `haiku`.

| Claim | Grok 1.0.77 | Codex 0.166.0 | Claude Code 2.1.322 |
|---|---|---|---|
| `agent doctor` names the program and its version as in range, and the sign-in in use | pass | pass | pass |
| A version outside the range is refused by `agent new` before a key is made (the real program behind a script that reports 99.0.0) | pass | pass | pass |
| `agent new` creates the agent, with its record, D19's words and D18's warning only for Claude Code | pass | pass | pass |
| A mention is answered exactly once | "noted" | "noted." | "noted" |
| A file in the folder granted for writing is written | pass | pass | pass (bzz allowed the edit) |
| A file in a folder that is not granted is not written | pass: Grok's sandbox, `Permission denied (os error 13)`; Grok asked nothing | pass: Codex asked, bzz refused, Codex ended its turn; the chat got "I could not finish: I needed something on my owner's machine that my owner has not allowed." | pass: Claude Code asked, bzz refused; it answered that the write was refused |
| A file in the folder of bzz's keys is not read | pass: `Permission denied` | not asked | not asked |
| Its process is running, and stops when the runtime stops | pass | pass | pass |
| After bzz and the program have stopped and started again, the next message in the thread is answered from the saved session: `session/load`, no `session/new`, no `read` of the thread, 21 messages between so that the word is not in what the message shows | "heron" | "heron" | "heron" |
| Still exactly one reply to each of the three messages | pass | pass | pass |
| The mode is set before the first prompt of each session, new and loaded | no modes | `workspace-write` | `default` |
| No credential and no key in what was recorded | pass | pass | pass |

Passing runs, 2026-11-14: the three in one run of 179 s on `b5515ac`, which
is the run the transcripts are from. Before it, on the tree that became
`f3ff49f`: Grok alone in 63 s, then Codex and Claude Code together in 121 s.
Nine prompts each time, and the three failed runs below: about thirty short
prompts in all on the owner's accounts, with the experiments that found how
each program asks.

Red runs, with the real programs, are the runs that failed on the way:

| What failed | How it showed | What changed |
|---|---|---|
| Codex, told to write outside its folders, **wrote there** | `FILES inside=true outside=true`, with bzz and Codex 0.166.0 and a recorded relay | the folder was under `/tmp`, which Codex's sandbox lets it write. The record now says "and the machine's temporary folders", and the test's folder is elsewhere |
| Codex answered in plain text, never through `send` | the same run: three requests answered `cancel`, the activity `a command bzz could not read · refused` three times | Codex asks leave for an MCP tool by the id of a call. bzz now keeps reported calls |
| Grok did not answer a mention | `grok, a mention: no reply` after 240 s on the pinned relay | the test started the test's own program as `bzz agent-tools`. `service::serve_with_tools` names bzz. What Grok did meanwhile is below |

### The plan's exit gate for M6

"Each harness answers a mention once on the pinned relay, survives a restart
with its own saved session, and refuses versions outside its range."

- [x] Grok, Codex and Claude Code each answer a mention exactly once on the
  pinned relay, with the owner's own program and sign-in.
- [x] Each survives a restart of bzz and of the program with its own saved
  session, and answers from it.
- [x] Each refuses a version outside its range: at `agent new`, in `agent
  doctor` and at the start.
- [x] A write outside the granted folders does not happen, for each of the
  three, and a write inside does.
- [ ] **Each harness in the panel on the release binary** (the plan's Herdr
  column): not run.

### Not proven in M6

| Item | State |
|---|---|
| macOS and Windows | not compiled or run here. Grok's and Codex's sandboxes on macOS, and what the three do on Windows, are read from their documents, not seen. The records say "not tried" |
| Any version but the three named | the ranges accept later versions of the same line (Grok below 1.1.0, Claude Code below 2.2.0, Codex below 0.167.0). None was run |
| An allow rule in the owner's own Claude settings | bzz starts Claude Code with no settings source, so that none is read. Whether such a rule would let an edit through without a request was not tried: the owner's settings were not mine to change. A settings file in the agent's working folder that allowed `Write`, `Edit` and `Bash` did not: Claude Code still asked (one run, before bzz set the option) |
| An MCP server of the owner's own under Codex | Codex asked leave for bzz's own tool, and a scripted request for `postgres.query` is refused. That Codex asks for every server's tools is not seen |
| Grok's and Codex's own settings | apply as the owner has them: MCP servers, hooks, rules, plugins. bzz does not switch them off. In one run Grok read the owner's own skills folder and ran one of his scripts with `--help` |
| A shell command of Claude Code or Codex allowed by the owner, with the real program | not run: the real runs had nobody to ask. The rule is proven with the scripted harness, the dialog with a test terminal |
| A link put in place of a folder after bzz looked | bzz checks where a path leads when it answers. A command the owner allowed can change that afterwards |
| The owner's key in an OS keychain | Grok's sandbox hides bzz's key folder. A key the operating system keeps for the user is reachable by any program of that user |
| Reading under Codex | Codex's sandbox hides nothing: a Codex agent can read bzz's key folder (the keys there are sealed or encrypted) and its database |
| A turn cancelled half way, with the real programs | seen in the spike (all three stop at once); not in a test here |
| Two conversations at once in one real process | seen in M0; the real runs here were one after the other |
| A usage limit, a missing sign-in | none occurred. The notices for them are M1's, by kind, from a scripted error |
| Codex's warnings as text | Codex writes its own warnings as message text. In a turn with no `send` and no other text, such a warning would be published as the answer. Seen only while a session was loading, where bzz ignores it |

### Found on the way

- **A harness's own messages can hold the owner's secrets.** Grok's
  `_x.ai/mcp/servers_updated` notification lists the owner's MCP servers with
  their arguments and environment. On this machine that held two live
  credentials, which were printed into the log of the work session that made
  this milestone before the cause was known. bzz never read them: it ignores
  the notification. The recorder keeps such a notification by its name only.
- **A model that cannot post looks for another way.** In the failed Grok run
  above, its `send` tool did not work. In four minutes Grok ran fifty tool
  calls: it read the owner's own skill for posting to his real Buzz and ran
  that script with `--help`, listed processes, read bzz's source to learn how
  `agent-tools` works, searched its own session files, `/tmp` and its
  parent's memory map for the session credential, read the test profile's
  key files and its passphrase file, and queried bzz's database. It posted
  nothing anywhere and found no credential; everything it reached belonged
  to the disposable profile, the passphrase included. The transcript was
  deleted. Three changes followed: Grok's sandbox is told to hide bzz's key
  folder (shown above), every coding harness is told what to do when `send`
  fails, and bzz keeps its memory from other processes on Linux.
- **"Enforced by bzz only" was the tightest of the three in these runs.**
  Claude Code, with no folder granted for commands, could write and read only
  in its folders. Grok and Codex, with sandboxes of their own, read
  everything and run commands without asking.
- **Buzz's relay script fails on a busy machine**: it takes Postgres as ready
  before its first start has made the database. `scripts/test-relay.sh` now
  waits for the database itself.
- `tests/agent_host_test.rs` failed once in a full run under a load of 13
  (`a new host is running`), and passed four times alone and in the next full
  runs. It is M2's test and was not changed.

### Gates after M6

Run on `09cc5ce`, 2026-11-14, Linux x86_64, Node 26.5.0.

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --check` | clean |
| Lint | `cargo clippy --locked --all-targets --all-features -- -D warnings` | clean (also `--no-default-features`) |
| Tests | `cargo test --locked --all-features` | 675 passed, 0 failed, 27 ignored. 31 passing tests are new since M7: 18 unit (10 `coding`, 5 `permission`, 1 each of `record`, `process`, `ui::approval`), 12 in `tests/agent_coding_test.rs`, 1 creation. The secret scan and two tests of M4 were extended. The one new ignored test is the one with the real programs |
| Pinned relay | `BZZ_BUZZ_SOURCE=$PWD/runs/buzz-pin ./scripts/test-relay.sh` | 20 passed in 446 s (the 19 of M7, and the test with the real programs, which skipped itself and said so), then `agent_host_test`, 6 passed |
| Pinned relay, with the real programs | the same with `BZZ_REAL_HARNESSES=grok,codex,claude-code` and `BZZ_RELAY_TEST_FILTER=real_relay_coding_harnesses` | 1 passed in 179 s, on `b5515ac`; nothing of bzz's own code changed after it |
| Agent host | `cd agent-host && npm ci --ignore-scripts --omit=optional && npm test` | 99 tests, 99 pass; `npm ci` reports 0 vulnerabilities. `agent-host/` was not changed |
| Supply chain | `cargo deny check`; `cargo audit`; `npm audit --omit=dev --omit=optional` in both `agent-adapters/` | `advisories ok, bans ok, licenses ok, sources ok`; the 4 allowed warnings of v0.14; 0 vulnerabilities. `Cargo.toml` and `Cargo.lock` are unchanged: M6 adds no Rust dependency. It adds two npm packages that bzz installs at run time, with what they need: 107 and 24 packages |

## Evidence gathered before the plan (2026-11-13)

These spikes ran before any product code and are the reason for the design.
They prove nothing about bzz itself yet; each becomes a test in its milestone.

| Spike | Claim | Result |
|---|---|---|
| S2, S3 | a Pi Durable host as a separate process answers a mention on the pinned relay through the v0.13 broker, and is killed after the publish | one reply on the relay with "one final reply per mention" kept on bzz's side; two without it |
| S8, S9 | the agent's disk in SQLite, with a shell and git that start no real process, is a valid Pi Durable environment | a 40 MB repository cloned with 0 of 457 files different from real git; push and pull against real git; 30 kills, 0 inconsistencies; Pi Durable's suite 24 of 24, 7 failing with two planted faults |
| S10 | the pinned `buzz-acp` starts the host over ACP | reply in 1.1 s; the conversation survives a `buzz-acp` restart; a mention handed over twice is answered once; `!shutdown` stops it |
| S11 | a move carries memory and disk | the remote copy answers once and continues the conversation; the agent's file is on the moved disk |
| S12 | a real model through Pi's own login | answers through the tool, remembers, publishes once after a kill |
