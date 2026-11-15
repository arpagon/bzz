# Prompts recorded from `buzz-acp`

What Buzz's `buzz-acp` sends as the text of `session/prompt` to the agent it
starts. bzz's Pi Durable host reads the place and the request out of this text
when it runs as a remote copy (`docs/agent-host-protocol.md`, section 7), so a
change of wording in Buzz breaks it. `agent-host/test/remote.test.mjs` reads
these files and fails when that happens.

- **Buzz revision:** `block/buzz@bf3bdd0eaefe39289bf9620594f254c7f68897f5`,
  the pin of bzz v0.15.0 (`crates/buzz-acp/src/queue.rs`, `format_prompt`).
- **Recorded:** 2026-11-14, by
  `real_relay_agent_moves_to_another_bzz_and_to_buzz_acp` in
  `tests/relay_integration.rs`: the pinned `buzz-acp` ran the host from a
  launch kit that `bzz agent launch-kit` wrote, against the pinned relay
  harness, and the host wrote each prompt it received to a file
  (`BZZ_AGENT_HOST_RECORD`).
- **As a launch kit starts it:** `buzz-acp` ran with the environment of the
  kit's `buzz-acp.env`. That switches off Buzz's own orientation for agents
  and its relay-side memory (`BUZZ_ACP_NO_BASE_PROMPT`, `BUZZ_ACP_NO_MEMORY`).
  Without those two, `buzz-acp` at this pin puts a `<base>` section of about
  21 kB and a `<core-memory>` section before `<context>` in the first prompt
  of every session, because the host speaks version 1 of the protocol. The
  host finds the place and the request behind them all the same; these files
  do not show that form.
- **Identities:** disposable keys and a channel made for that run on a local
  relay. Nothing here names a person, a community or a deployed relay.

Each `.txt` file is one prompt, byte for byte, with no newline added at the
end. `expected.json` says what the host must read from each.

| File | What Buzz was doing |
|---|---|
| `01-top-level-mention.txt` | a top-level mention in a channel |
| `02-same-mention-after-restart.txt` | `buzz-acp` was killed and restarted, and handed the same mention over again, with the thread's context |
| `03-reply-in-thread.txt` | a second mention, as a reply in that thread |
| `04-new-top-level-mention.txt` | another top-level mention in the same channel |

Not recorded: a DM (`Scope: dm`), a batch of several events
(`<buzz-events count="N">`), an edit, and a channel with a description or a
project. The host's reading of the first two is tested with text built from
the same Buzz source, not with a recording.

## Recording again at a new Buzz pin

One command, with a checkout of Buzz at the new revision (and `PIN` moved in
the three places that name it). It needs Docker, tmux and Node.js 22.19 or
newer:

```sh
BZZ_REFRESH_BUZZ_ACP_PROMPTS=1 BZZ_RELAY_TEST_FILTER=real_relay_agent_moves \
  BZZ_BUZZ_SOURCE=<the checkout> ./scripts/test-relay.sh
```

That test moves an agent to `buzz-acp` and, under it, mentions the agent at
the top level of a channel, kills and restarts `buzz-acp`, replies in that
thread and mentions the agent at the top level again. With the variable set
it replaces the four files here with what the host received and writes
`expected.json` from the events it published itself. Then run the host's own
tests, which say whether the host still reads them:

```sh
cd agent-host && npm test
```

Without the variable the same test is the guard: it compares what `buzz-acp`
sends with these files, ids, names, times and words aside, and fails when
the wording or a field differs. It also fails when the agent, run by the
real `buzz-acp`, no longer answers in the right conversation.
