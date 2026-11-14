# Prompts recorded from `buzz-acp`

What Buzz's `buzz-acp` sends as the text of `session/prompt` to the agent it
starts. bzz's Pi Durable host reads the place and the request out of this text
when it runs as a remote copy (`docs/agent-host-protocol.md`, section 7), so a
change of wording in Buzz breaks it. `agent-host/test/remote.test.mjs` reads
these files and fails when that happens.

- **Buzz revision:** `block/buzz@bf3bdd0eaefe39289bf9620594f254c7f68897f5`,
  the pin of bzz v0.15.0 (`crates/buzz-acp/src/queue.rs`, `format_prompt`).
- **Recorded:** 2026-11-13, by spike S10: the pinned `buzz-acp` started the
  host against the pinned relay harness and the host wrote each prompt it
  received to a file.
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

1. Build `buzz-acp` and the `buzz` CLI at the new revision and start the
   relay harness, as `scripts/test-relay.sh` does.
2. Start `buzz-acp` with the host as its agent
   (`node agent-host/main.mjs <folder>`), with a disposable agent key and
   `BZZ_AGENT_HOST_RECORD=<folder for the recording>` in its environment.
   The folder needs an `agent.json`; any model will do.
3. From a disposable identity: mention the agent at the top level of a
   channel; kill and restart `buzz-acp`; reply to the agent in that thread;
   mention it again at the top level.
4. The host wrote one `prompt-<pid>-<n>.txt` per prompt it received. Replace
   these four files with them, update `expected.json`, and run `npm test` in
   `agent-host/`.
