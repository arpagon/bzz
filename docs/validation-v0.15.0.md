# bzz v0.15.0 validation

**Status:** M0 in progress on branch `v0.15.0` (PR #9). M1, the agent runtime
in bzz, is implemented and proven below (2026-11-14); nothing else is. Plan:
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
| Baselines fixed in the repository: the host's `package.json` and lock file, Node detection | `agent-host/package.json` and `package-lock.json` added 2026-11-14 (see M2); Node detection in bzz not done |
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
| A real harness | none exists in this tree yet: the Pi Durable host is M2, the coding harnesses M6. Everything above is the scripted harness |
| The release TUI through Herdr ("the agent panel shows a turn") | not run. The TUI path is proven by the two `app::tests` above, and what it calls (`service::serve`) on the relay |
| A federated-identity community | the agents enroll as the broker's do (same code); `./scripts/test-relay.sh --nip-fi` has no local agent in it |
| Which permission titles the real adapters use for bzz's tools | the rule is tested with `mcp__buzz__send` and three other spellings; a harness that asks under another name is refused, and its turn then ends as plain text |
| `session/load` with a real adapter | scripted only |
| The pause toggle and community switch in the running TUI | the toggle is covered by the `app` test; a switch of community with agents running is not |
| Growth over time | inbox rows and stored sessions are never pruned, and a harness keeps every session it opened |

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
- [ ] Against bzz itself, on the pinned relay: pending bzz's runtime (M1).
- [ ] "One reply per mention" is bzz's rule, not the host's. The host only
  makes it cheap: a request handed over again starts nothing.
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
  M3): not built. The protocol has no message for it.
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
