# Agent host protocol

How bzz talks to the process that runs a local agent. It is the contract
between bzz's agent runtime (Rust, `src/agents/`) and a harness: bzz's own Pi
Durable host (Node, `agent-host/`) or a coding agent (Grok, Codex, Claude
Code). Decisions and their reasons are in
[`adr-v0.15-local-agents.md`](adr-v0.15-local-agents.md).

The base is the [Agent Client Protocol](https://agentclientprotocol.com),
version 1: JSON-RPC 2.0, one message per line, on the child's stdin and
stdout. bzz is the client. Everything bzz adds is under `_meta` keys that
start with `bzz` or in methods that start with `_bzz/`, as ACP allows. A
harness that knows none of them still works.

## 1. Files

| Path | What |
|---|---|
| `<data>/agents/<agent id>/` | one agent's folder; `<agent id>` is the UUID in bzz's configuration |
| `…/memory.sqlite` | Pi Durable's storage: conversations, tasks, the place map |
| `…/files.sqlite` | the agent's disk |
| `…/agent.json` | settings a remote copy needs: name, model, instructions. Written by `agent move` and `agent launch-kit`, never read by a local launch |
| `…/workspace/` | the working folder of a coding harness |
| `<data>/agents/run/<agent id>.json` | the ownership record of a running process |
| `<data>/agent-host/<sha16>/` | the Pi Durable host's sources and `node_modules`, where `<sha16>` is the first 16 hex characters of the SHA-256 of the lock file inside the bzz binary |

`memory.sqlite` and `files.sqlite` hold no path of the machine. They are what
a move carries.

## 2. Starting and stopping

- bzz starts one process per agent, in its own process group, with standard
  input and output as the protocol and standard error as a log.
- The environment is built from nothing: `PATH`, `HOME`, `LANG`, `TMPDIR`,
  and what the harness needs to find its own sign-in. It never holds an agent
  key, an owner key, a relay address or a bzz credential, with one exception:
  the `bzz agent-tools` server of section 6 gets its own session's credential.
- Pi Durable host: `node <host dir>/main.mjs <agent folder>`.
- To stop, bzz closes the child's standard input. A harness must close its
  storage and exit within 2 s. After that bzz sends TERM to the group, and
  KILL 2 s later.
- The Pi Durable host also exits when its parent process is gone.

## 3. Messages bzz sends

### `initialize`

```json
{"protocolVersion": 1,
 "clientCapabilities": {},
 "clientInfo": {"name": "bzz", "version": "0.15.0"},
 "_meta": {"bzz": {
   "agent": {"id": "<uuid>", "pubkey": "<hex>", "name": "Scout"},
   "model": {"provider": "openai-codex", "modelId": "gpt-6-luna", "thinkingLevel": "low"},
   "instructions": "…",
   "mounts": [{"name": "site", "path": "/home/ann/site", "write": false}],
   "network": ["github.com"],
   "piDir": "/home/ann/.pi/agent"
 }}}
```

- `model` may be absent: the host then uses Pi's own default.
- `mounts` and `network` come from bzz's configuration at every start. The
  host never stores them.
- The host answers with `agentCapabilities` and, when it is bzz's own host,
  `"_meta": {"bzz": {"host": "<version>", "tools": "pipe"}}`. `"tools":
  "pipe"` means it will ask for tools with `_bzz/tool` (section 5) and needs
  no MCP server.

### `session/new`

```json
{"cwd": "/home/agent", "mcpServers": [],
 "_meta": {"bzz/place": "<channel uuid>" }}
```

- A **place** is where a conversation happens: `<channel uuid>` for a DM or a
  Solo chat, `<channel uuid>/<thread root event id>` for a thread in a
  channel. A top-level mention starts a thread, so its place is
  `<channel uuid>/<its own event id>`.
- bzz's own host keeps one durable conversation per place and finds it again
  after any restart, whatever the session id. bzz therefore never needs
  `session/load` for it.
- For other harnesses bzz stores the session id the harness returned for the
  place and uses `session/load` after a restart when the harness offers it.
- `mcpServers` is empty for a `"tools": "pipe"` host and holds the server of
  section 6 otherwise.

### `session/prompt`

```json
{"sessionId": "…",
 "prompt": [{"type": "text", "text": "…"}],
 "_meta": {"bzz/request": "<event id of the mention>"}}
```

- The **request** is the event the turn answers. bzz's own host uses it as
  the idempotency key: the same request submitted again is the same
  submission and starts no new work.
- The text is what the model reads: who wrote, where, the message, and the
  recent messages of the thread. Its format is bzz's to change; no harness
  parses it.
- The response is `{"stopReason": …}` when the turn is over.

### `session/cancel`

A notification. The harness stops the turn and answers the pending
`session/prompt` with `stopReason: "cancelled"`.

### `_bzz/checkpoint`

A request to bzz's own host, with no parameters: finish writes, checkpoint
both SQLite files so that no write-ahead file is left, and answer `{}`. bzz
sends it before a move and when the agent has been idle for a while.

## 4. Messages a harness sends

- `session/update` notifications. bzz uses `agent_message_chunk` to keep the
  turn's last plain text, and `tool_call` and `tool_call_update` to show what
  the agent is doing. It ignores kinds it does not know.
- `session/request_permission`. bzz allows its own chat tools. Anything else
  is refused unless the agent has a grant that covers it (plan, M7).
- Notifications in a namespace bzz does not know, such as `_x.ai/…`, are
  ignored. A request bzz does not know gets error `-32601`.

## 5. Tools over the pipe: `_bzz/tool`

Sent by bzz's own host:

```json
{"sessionId": "…", "name": "send", "arguments": {"text": "On it.", "final": true}}
```

bzz answers:

```json
{"text": "sent as <event id>", "final": true}
```

or, when the call is refused, `{"text": "<why, and what to do>", "isError":
true}`.

bzz finds the agent from the process and the turn from the session. The
arguments never say which mention is answered.

| Tool | Arguments | What bzz does |
|---|---|---|
| `send` | `text` (string, required), `final` (boolean), `mentions` (array of pubkeys) | posts `text` in the turn's place, as a reply to the request. With `final`, ends the turn: one per request, under the rule of the ADR |
| `read` | `limit` (1–100), `before` (event id) | returns recent messages of the place, newest last |
| `react` | `emoji` (string, required), `event` (event id; default: the request) | adds a reaction |

A call with a missing or misnamed argument is refused with a text that names
the right one. `message` and `content` are accepted as other names for `text`.

## 6. Tools for other harnesses: `bzz agent-tools`

`bzz agent-tools` is a hidden command: an MCP server over stdio with the same
three tools. A harness starts it because bzz listed it in `mcpServers`:

```json
{"name": "buzz", "command": "<path to bzz>", "args": ["agent-tools"],
 "env": [{"name": "BZZ_AGENT_TOOLS", "value": "<address>#<credential>"}]}
```

- The credential is made by bzz for one agent and one session, kept in memory,
  and revoked when that agent's process exits.
- The server holds no key. It sends each call to bzz on the loopback address
  with the credential, and bzz treats the call exactly as in section 5.

## 7. The Pi Durable host as a remote copy

Under Buzz's `buzz-acp` there is no `_meta.bzz`. The host then:

- reads name, model and instructions from `agent.json` in its folder;
- reads the place and the request from the text of `buzz-acp`'s prompt
  (`Channel: … (#<uuid>)`, `Thread root: <id>`, `Event ID: <id>`);
- posts with the `buzz` command, which has the key in its environment as
  Buzz gives it.

`tests/fixtures/agents/buzz-acp-prompts/` holds prompts recorded from the
pinned `buzz-acp`; the host's tests read them, and they are recorded again at
each Buzz pin.
