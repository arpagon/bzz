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

Sections 1 to 7 keep their numbers. Sections 8 to 10 are what bzz's own host
asks of bzz beyond the chat tools.

## 1. Files

| Path | What |
|---|---|
| `<data>/agents/<agent id>/` | one agent's folder; `<agent id>` is the UUID in bzz's configuration |
| `…/memory.sqlite` | Pi Durable's storage: conversations, tasks, the place map |
| `…/files.sqlite` | the agent's disk |
| `…/agent.json` | settings a remote copy needs (below). Written by `agent move` and `agent launch-kit`, never read by a local launch |
| `…/workspace/` | the working folder of a coding harness |
| `<data>/agents/run/<agent id>.json` | the ownership record of a running process |
| `<data>/agent-host/<sha16>/` | the Pi Durable host: `main.mjs`, `package.json`, `src/` and `node_modules`. `<sha16>` is the first 16 hex characters of the SHA-256 of the lock file inside the bzz binary |

- The host reads its version from `package.json`, so that file must sit
  beside `main.mjs`. `src/` must be whole: `src/run-child.mjs` is started as
  a process of its own (section 9).
- While the host works, a write-ahead file (`-wal`, `-shm`) sits beside each
  SQLite file. `_bzz/checkpoint` and a clean exit leave none.
- `memory.sqlite` and `files.sqlite` are what a move carries. They hold no
  path of the machine, with one exception: what a real command prints
  (section 9) is kept as it was printed.

`agent.json`:

```json
{"name": "Scout",
 "model": {"provider": "openai-codex", "modelId": "gpt-6-luna", "thinkingLevel": "low"},
 "instructions": "…",
 "network": ["github.com"],
 "packages": true}
```

- `name` defaults to `agent`, `instructions` to none, `network` to no host.
  `model` may be absent: Pi's default on that machine is used.
- `network` has the form of `_meta.bzz.network` (section 3). A list the host
  cannot read stops the start; it is never read as "everything".
- `packages`: only `false` means something: Pi packages stay off (section 10).
- Granted folders are not in it. If the file names any, they are ignored:
  they belong to the machine they were granted on.

## 2. Starting and stopping

- bzz starts one process per agent, in its own process group, with standard
  input and output as the protocol and standard error as a log.
- The environment is built from nothing: `PATH`, `HOME`, `LANG`, `TMPDIR`,
  and what the harness needs to find its own sign-in. It never holds an agent
  key, an owner key, a relay address or a bzz credential, with one exception:
  the `bzz agent-tools` server of section 6 gets its own session's credential.
- Pi Durable host: `node <host dir>/main.mjs <agent folder>`. Node 22.19 or
  newer; on an older one it exits with code 2 and says so.
- To stop, bzz closes the child's standard input. A harness must close its
  storage and exit within 2 s. After that bzz sends TERM to the group, and
  KILL 2 s later.
- The Pi Durable host also exits when its parent process is gone.
- The log names what happened, not what was said: no prompt, message,
  command line or credential is written to it.
- Only the protocol is written to standard output. What a Pi package prints
  goes to the log.

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
   "mounts": [{"name": "site", "path": "/home/ann/site", "write": false, "exec": false}],
   "network": ["github.com"],
   "piDir": "/home/ann/.pi/agent",
   "packages": true
 }}}
```

- The presence of `_meta.bzz` is what tells the host that bzz started it.
  Without it the host is a remote copy (section 7).
- `model` may be absent: the host then uses Pi's own default.
- `mounts` and `network` come from bzz's configuration at every start. The
  host never stores them.
- A `mounts` entry:
  - `name`: a simple name, `[A-Za-z0-9][A-Za-z0-9._-]{0,63}`, unique. The
    folder appears at `/mnt/<name>`. `/mnt` holds granted folders and nothing
    else.
  - `path`: the folder on this machine. If it is not a folder, `/mnt/<name>`
    is there and empty of everything.
  - `write`: `true` lets the agent's file tools, shell and git change the
    folder. Anything else is read-only.
  - `exec`: `true` lets the agent start real commands there (section 9).
    Anything else does not. It is separate from `write`.
- A `network` entry is `host` or `host:port`, such as `github.com` or
  `git.example.org:8443`. It is matched exactly: that host, not the names
  below it, and that port (`host` alone means 443). Only HTTPS is let
  through, and plain HTTP to this machine itself (`localhost`, `127.0.0.1`,
  `[::1]`). Each hop of a redirect is checked; `Authorization` and `Cookie`
  headers do not follow a redirect to another host.
- `piDir` is Pi's folder: `auth.json` for the sign-in, `settings.json` for
  the default model and the packages.
- `packages`: `false` switches Pi packages off for this agent. Absent or
  anything else: on (section 10).
- A bad `mounts` or `network` entry fails `initialize` with `-32602` and the
  reason. `initialize` may then be sent again, corrected. A second
  `initialize` after a successful one gets `-32600`.
- `initialize` may take as long as the package time limit, 10 s, when a
  package hangs.

The host answers:

```json
{"protocolVersion": 1,
 "agentCapabilities": {"loadSession": false, "promptCapabilities": {"image": false, "audio": false, "embeddedContext": false}},
 "agentInfo": {"name": "bzz-agent-host", "title": "bzz Pi Durable host", "version": "0.15.0"},
 "authMethods": [],
 "_meta": {"bzz": {
   "host": "0.15.0",
   "tools": "pipe",
   "packages": [{"source": "~/pi/guard", "loaded": true, "extensions": ["guard"], "providers": []},
                {"source": "npm:pi-x", "loaded": false, "error": "Only Pi packages in a local folder are loaded"}],
   "run": ["site"]
 }}}
```

- `"tools": "pipe"` means it will ask for tools with `_bzz/tool` (section 5)
  and needs no MCP server. A remote copy answers `"tools": "buzz"`: it posts
  with the `buzz` command (section 7).
- `packages`: one entry per package in Pi's settings, in their order
  (section 10). Empty when there are none or they are off.
- `run`: the names of the granted folders where the agent may start commands
  (section 9). Empty when it has no such tool.
- **A turn that was cut off runs again at `initialize`**, before bzz asks.
  Its calls to bzz (`_bzz/tool`, `_bzz/credential`, `_bzz/approve`) wait
  until bzz hands the same request over again on a new session. If bzz never
  does, the turn stays waiting; it holds nothing else up, not even a
  checkpoint. A tool call that was cut off is not made again: the model is
  told that it was interrupted.

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
  `session/load` for it. Session ids do not survive a restart of the host.
- `cwd` is a directory of the agent's disk; the host makes it if it is not
  there. A missing place gets `-32602`.
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
  submission and starts no new work. It answers as the first one did, at
  once if that one is over.
- The text is what the model reads: who wrote, where, the message, and the
  recent messages of the thread. Its format is bzz's to change; no harness
  parses it.
- Several prompts may run at once, on different sessions. A prompt that
  arrives for a place whose turn still runs joins that turn.
- The response is `{"stopReason": …}` when the turn is over: `end_turn`,
  `cancelled`, or `max_turn_requests`.
- **A turn that fails is answered with a JSON-RPC error**, not a
  `stopReason`: code `-32000`, a message that says why, and `data.kind`.

| `data.kind` | When |
|---|---|
| `no_model` | no model is set for the agent and Pi names no default |
| `provider_unavailable` | the provider is neither one of Pi's nor one of a loaded package. `data.package` names a package that names this provider but has no Pi Durable entry, when there is one (section 10) |
| `model_unavailable` | the provider has no such model |
| `sign_in_needed` | Pi has no usable sign-in for the provider |
| `model_error` | the provider failed during the turn; the message holds its text |
| another word | Pi Durable's reason for a turn it could not finish, such as `faulted`, `interrupted`, `restart_limit` |

- A missing or unknown `sessionId`, or a missing request, gets `-32602`.
- A turn that could not start (the first four kinds) leaves nothing: the
  same request sent again is tried again.
- A turn that failed while it ran is settled. The same request sent again
  gets the same error and starts no work. Only a new request is a new try.

### `session/cancel`

A notification. The harness stops the turn and answers the pending
`session/prompt` with `stopReason: "cancelled"`. bzz's own host does not wait
for an answer of bzz that is still open, and ends a real command that the
turn started (section 9).

### `_bzz/checkpoint`

A request to bzz's own host, with no parameters. It answers `{}` when both
SQLite files are whole and no write-ahead file is left. bzz sends it before a
move and when the agent has been idle for a while.

- It waits for the turns that run.
- A prompt that arrives meanwhile is held until the checkpoint is done.
- It closes storage. The next prompt opens it again.
- It does not wait for a cut-off turn that bzz has not asked for again.

## 4. Messages a harness sends

- `session/update` notifications. bzz uses `agent_message_chunk` to keep the
  turn's last plain text, and `tool_call` and `tool_call_update` to show what
  the agent is doing. It ignores kinds it does not know. bzz's own host sends
  one `agent_message_chunk` per answer of the model, a `tool_call` with
  `status: "in_progress"` before each tool, and a `tool_call_update` with
  `completed` or `failed` after it. A command's title is `run in
  /mnt/<name>: <its first line>`.
- `session/request_permission`. bzz allows its own chat tools. Anything else
  is refused unless the agent has a grant that covers it. bzz's own host
  never sends it: it asks with `_bzz/approve` (section 9).
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
true}`. `"final": true` in the answer ends the turn.

bzz finds the agent from the process and the turn from the session. The
arguments never say which mention is answered.

| Tool | Arguments | What bzz does |
|---|---|---|
| `send` | `text` (string, required), `final` (boolean), `mentions` (array of pubkeys) | posts `text` in the turn's place, as a reply to the request. With `final`, ends the turn: one per request, under the rule of the ADR |
| `read` | `limit` (1–100), `before` (event id) | returns recent messages of the place, newest last |
| `react` | `emoji` (string, required), `event` (event id; default: the request) | adds a reaction |

- These are the names on the wire. **The model sees them as `buzz_send`,
  `buzz_read` and `buzz_react`**, because Pi's file tools own the name
  `read`. bzz never sees the prefixed names.
- A call with a missing or misnamed argument is refused with a text that
  names the right one. `message` and `content` are accepted as other names
  for `text`; the host renames them before it asks.
- A JSON-RPC error, or no answer before the turn is cancelled, reaches the
  model as a failed tool call.

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

- reads its settings from `agent.json` in its folder (section 1). Without
  that file `initialize` fails and says what is missing;
- reads the place and the request from the text of `buzz-acp`'s prompt
  (`Scope: …`, `Channel: … (#<uuid>)`, `Thread root: <id>`, `Event ID: <id>`).
  A prompt that names no channel or event gets `-32602`;
- posts with the `buzz` command, which has the key in its environment as
  Buzz gives it. The host hands its environment to `buzz` and to nothing
  else;
- ignores `cwd` in `session/new`: it is a path of that machine.

It reads these from its environment:

| Variable | Use |
|---|---|
| `PI_CODING_AGENT_DIR` | Pi's folder. Default `~/.pi/agent` |
| `BZZ_AGENT_HOST_BUZZ` | the `buzz` command, as a path or a JSON array with first arguments. Default `buzz` on the `PATH` |
| `BZZ_AGENT_HOST_RECORD` | a folder where each prompt of `buzz-acp` is saved, to record the fixtures again |

`tests/fixtures/agents/buzz-acp-prompts/` holds prompts recorded from the
pinned `buzz-acp`; the host's tests read them, and they are recorded again at
each Buzz pin.

What a remote copy does not have, or does differently:

- **No granted folder, and so no `run`.** Its allow-list is the one in
  `agent.json`.
- **No credentials for a private remote.** There is nobody to ask: git says
  that this copy has none (section 8).
- **A Solo chat is read as a channel.** `buzz-acp`'s text does not tell them
  apart, so a Solo chat's place is `<channel uuid>/<event id>` there and
  `<channel uuid>` under bzz. A conversation in a Solo chat is not found
  again after a move. DMs and threads are.
- **`read` with `before` is refused**: the `buzz` command reads the newest
  messages only.
- **Buzz does not escape message text in its prompt.** A message that
  imitates an event header can make the host take another event of the same
  channel as the request. It cannot move the channel.
- **An edit is not handled**: the reply goes to the edit event.
- The final reply is recorded after `buzz` returns. A kill between the two
  can repeat a reply. Under bzz, bzz's ledger closes that.
- A Pi package runs in the host's process, and there the agent's key is in
  that process's environment (section 10).

## 8. Credentials for a private remote: `_bzz/credential`

Sent by bzz's own host when the agent's `git` gets an authentication
challenge (HTTP 401) from a host on the allow-list:

```json
{"sessionId": "…", "host": "github.com"}
```

bzz answers:

```json
{"username": "…", "password": "…"}
```

or with a JSON-RPC error when it has none or refuses. The error's message
reaches the model, so it should say what the owner can do.

- `host` has the form of an allow-list entry: `host`, or `host:port` for a
  port that is not 443. It is the host of the remote's URL.
- `sessionId` is the session of the turn whose command asked.
- A host that is not on the allow-list never causes the request.
- The host asks once per git command (`clone`, `fetch`, `pull`, `push`) and
  keeps nothing: the next command asks again. bzz may answer from memory.
- The answer goes to that command's requests as HTTP Basic credentials. A
  token goes in `password`; `username` is what the remote expects with it.
  Both must be strings and not both empty.
- The answer is written nowhere: not to either SQLite file, the repository's
  configuration, a tool result or the log. If a remote repeats it in its
  refusal, the host stars it out.
- A remote that refuses the answer ends the command. bzz is not asked a
  second time for it.
- A cancelled turn does not wait for the answer, and a question that was
  open when the host exited is void.
- Only `git` asks, and only on a challenge: 401, or the 203 that Azure
  DevOps sends in its place. The shell's `curl` does not. A remote that
  answers "not found" for a private repository never causes the request.
- Credentials are for the host in the remote's URL. They do not follow a
  redirect to another host.

## 9. Hands on this machine: `run` and `_bzz/approve`

A granted folder with `"exec": true` gives the agent one more tool, `run`,
which starts a **real** command in that folder. Without such a grant the
tool does not exist and the model is not told of it. A remote copy never has
it. The host does not offer it on Windows: it cannot end a command's whole
process tree there. `_meta.bzz.run` in the answer to `initialize` says where
it is offered.

The model calls `run` with `folder` (the grant's name), `command` (one line
for `/bin/sh`) and an optional `timeout` in seconds (default 120, at most
3600). Before anything starts, the host asks bzz:

```json
{"sessionId": "…", "kind": "exec", "mount": "site", "command": "npm test"}
```

bzz answers `{"allow": true}` or `{"allow": false, "reason": "…"}`.

- **Only `"allow": true` starts the command.** Any other answer, a JSON-RPC
  error, or no answer before the turn is cancelled, starts nothing. `reason`
  reaches the model as the tool's error.
- **What bzz approved is what runs.** `command` is the string the shell
  receives (`/bin/sh -c -- <command>`). The host does not read it, quote it
  or add to it.
- bzz is not asked about what can never run: a folder without `exec`, an
  empty command, a time limit out of range, a folder that is not there. The
  model gets an error.
- The host asks about one command at a time within a turn. Two
  conversations may each have a question open.
- bzz decides whether the owner is asked. The host knows nothing of
  "allowed without asking".
- A question that was open when the host exited is void. The next process
  does not act on an answer to it: the model is told that the call was
  interrupted, and a new call asks again.

The command:

- starts with the granted folder as its working directory;
- gets an environment built from nothing: `PATH`, `HOME`, `LANG` and
  `TMPDIR` as the host has them, and nothing else;
- has no terminal, and an empty standard input;
- runs in a process group of its own, not bzz's and not the host's;
- is ended, with its group, when the turn is cancelled, at its time limit,
  and when the host exits, however it exits. The group gets TERM, and KILL
  2 s later;
- leaves nothing running: what is still in its group when it returns is
  killed;
- is not started again after a restart. A turn that was cut off inside a
  command goes on with the model told that the command was interrupted.

Its output, standard output and standard error together, comes back bounded:
the last 2000 lines or 50 KB. A longer output is saved on the agent's disk
(`/tmp/run-<id>.log`, at most 8 MiB) and the result says where.

What this does not give:

- **The folder is where the command starts, not a wall.** A real command has
  the owner's rights and can reach whatever the owner can. `write: false`
  binds the agent's file tools, not a command.
- A process that leaves the group (`setsid`, a daemon) is not ended.
- If the host and the command's keeper process are both killed at the same
  moment, the command is not ended. bzz's sweep at startup does not see it:
  it is not in the host's group.
- A command's output is kept as printed, so paths of this machine can enter
  `memory.sqlite`.

## 10. Pi packages: `_meta.bzz.packages`

The host loads the Pi Durable entries (`"pi": {"durable": ["./durable.ts"]}`
in a package's `package.json`) of the packages listed under `packages` in
`settings.json` of Pi's folder. Their providers join Pi's own. Their
extensions (tools, prompt sections, hooks) are installed for the agent.

- `"packages": false` in `_meta.bzz` switches this off for the agent: no
  code of any package runs. The default is on.
- **A package's code runs inside the host, with the host's rights.** It is
  not in the agent's sandbox. In a remote copy it can read the agent's key
  from the environment.
- Only a package in a local folder is loaded. An entry is a path: absolute,
  `~/…`, or relative to Pi's folder. `npm:`, `git:` and URL entries are left
  out.
- Each package has 10 s to load. The packages load at the same time.
- A package that fails is left out and the others still load. The answer to
  `initialize` reports each one: `{"source", "loaded": true, "extensions":
  [names], "providers": [ids]}` or `{"source", "loaded": false, "error":
  "…"}`. A package without a Pi Durable entry is reported as not loaded,
  with that reason.
- A package may not name an extension `bzz` or `coding-tools`, nor a tool
  `run` or `buzz_…`. A tool of a package with the name of one of Pi's file
  tools (`read`, `write`, `edit`, `bash`) replaces it.
- The host uses the models a provider lists when it loads. It does not fetch
  a provider's models.
- When a turn fails with `provider_unavailable`, the message names each
  package that did not load and why, and says whether a package without a Pi
  Durable entry names that provider. That is read from the package's files
  as text: the provider's id as a quoted string.
- A change to a package or to the list takes effect when the host starts
  again.
