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
asks of bzz beyond the chat tools, each with what bzz answers. Section 11 is
the host's one entry that is not the protocol. Section 12 is what bzz does
for a coding harness beyond sections 2 to 6: how it starts Grok, Codex and
Claude Code, what it sets on each of their sessions, and how it answers what
they ask leave for.

## 1. Files

| Path | What |
|---|---|
| `<data>/agents/<agent id>/` | one agent's folder; `<agent id>` is the UUID in bzz's configuration |
| `…/memory.sqlite` | Pi Durable's storage: conversations, tasks, the place map |
| `…/files.sqlite` | the agent's disk |
| `…/agent.json` | settings a remote copy needs (below). Written by `agent move` and `agent launch-kit`, never read by a local launch |
| `…/workspace/` | the working folder of a coding harness |
| `…/workspace/.grok/sandbox.toml` | Grok's sandbox profile for this agent. Written by bzz before every start, in place of whatever is there (section 12) |
| `…/harness.log` | the harness's standard error, from its last start. At most 8 MiB are kept |
| `…/started.json` | what bzz's own host answered to its last `initialize`: its version, where it offers `run`, its packages (section 3). Written by bzz |
| `…/activity.jsonl` | every real command the agent asked to run and every git credential it asked for, with bzz's answer (sections 8 and 9); for a coding harness, also what bzz refused it (section 12). Written by bzz; never a secret |
| `<data>/agents/run/<agent id>.json` | the ownership record of a running process |
| `<data>/keys/agent-<agent id>.nip44` | the agent's key, sealed to its owner's |
| `<data>/keys/agent-<agent id>.git/` | the git credentials stored for the agent, each sealed to its owner's key (section 8) |
| `<data>/agent-host/<sha16>/` | the Pi Durable host: `main.mjs`, `package.json`, `src/` and `node_modules`. `<sha16>` is the first 16 hex characters of a SHA-256 over **every host file inside the bzz binary**, each with its path: the lock file, `package.json`, `main.mjs` and all of `src/`. A change to any of them is another install |

| `<data>/agent-adapters/<name>-<sha16>/` | the ACP adapter of Codex (`codex`) or of Claude Code (`claude-code`): `package.json`, `package-lock.json` and `node_modules`. `<sha16>` is the first 16 hex characters of a SHA-256 over the two files inside the bzz binary (section 12) |

- The host reads its version from `package.json`, so that file must sit
  beside `main.mjs`. `src/` must be whole: `src/run-child.mjs` is started as
  a process of its own (section 9).
- The host makes the agent's folder if it is not there, for its owner only
  (`0700`), and keeps the two SQLite files at `0600`. bzz makes it first,
  the same way, and writes its own three files there at `0600`.
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
  `model` has the form of `_meta.bzz.model` (section 3) and may be absent:
  Pi's default on that machine is used.
- `network` has the form of `_meta.bzz.network` (section 3). A list the host
  cannot read stops the start; it is never read as "everything".
- `packages`: only `false` means something: Pi packages stay off (section 10).
- Granted folders are not in it. If the file names any, they are ignored:
  they belong to the machine they were granted on.

## 2. Starting and stopping

- bzz starts one process per agent, in its own process group, with standard
  input and output as the protocol and standard error as a log.
- **bzz hands the child those three streams and no other open descriptor**,
  also none that bzz itself inherited from the terminal it was started in.
  A real command (section 9) would get them. On Linux the Pi Durable host
  closes what it inherited before a command starts; on other systems it
  cannot tell which those are.
  - How bzz does it, on Linux and on macOS: just before it starts a harness
    (and `npm`, and the probes of `node`), it marks every descriptor it holds
    above 2 as not to be handed on (`FD_CLOEXEC`). What Rust opened is marked
    that way from the start; this covers what bzz was born with and what a C
    library opened.
  - What that leaves: a descriptor another thread of bzz opens without the
    mark between that moment and the start of the child. On Windows a handle
    bzz itself inherited as inheritable is handed on; the host offers no
    `run` there.
- **On Linux, bzz is a process whose memory others of the same user may not
  read**, from the moment it runs agents (`PR_SET_DUMPABLE` 0): a command a
  harness runs cannot read an agent's key, or the owner's, out of
  `/proc/<bzz>/mem`. bzz then leaves no core dump either.
- The environment is built from nothing: `PATH`, `HOME`, `LANG`, `TMPDIR`,
  and for a coding harness the variables of section 12: its guard, where
  its adapter finds the owner's program, and where its sign-in is when the
  owner moved it. It never holds an agent key, an owner key, a relay address
  or a bzz credential, with one exception: the `bzz agent-tools` server of
  section 6 gets its own session's credential.
- **`HOME` is the agent's own folder for bzz's Pi Durable host**, and the
  owner's home for a coding harness, which finds its own sign-in there. So
  the Pi Durable host would look for Pi's folder in the wrong place: bzz
  tells it where Pi's folder is at every start (`piDir`, section 3), and
  does not start it when it cannot find that folder.
- The working directory is the agent's folder for the Pi Durable host, and
  `…/workspace/` for a coding harness.
- Pi Durable host: `node <host dir>/main.mjs <agent folder>`. Node 22.19 or
  newer; on an older one, or without the folder, it exits with code 2 and
  says so.
- The Pi Durable host's own code reads these from its environment, and the
  variables of section 7. Node reads its own (`NODE_OPTIONS` and the like),
  so bzz sets none of those:

| Variable | Use |
|---|---|
| `PATH`, `HOME`, `LANG`, `TMPDIR` | handed on to a real command (section 9). Without `PATH` a command gets `/usr/local/bin:/usr/bin:/bin` |
| `HOME` (`USERPROFILE` on Windows) | `~` in the path of a Pi package, and Pi's folder when nothing names it |

- To stop, bzz closes the child's standard input. A harness must close its
  storage and exit within 2 s. After that bzz sends TERM to the group, and
  KILL 2 s later. The order is always these three: end of input, TERM, KILL.
  What is still in the group afterwards is killed with it.
- The Pi Durable host exits with code 0 after the end of its input. It
  gives storage 1.5 s to close and exits then in any case. TERM is taken as
  the end of input. A real command that runs is told to stop at that moment
  (section 9).
- The Pi Durable host also exits when its parent process is gone. It looks
  once a second.
- It exits with code 1 on a fault of its own. Unfinished turns are in
  storage, and the next process continues them.
- The log names what happened, not what was said: no prompt, message,
  command line or credential is written to it. The first line of an error
  that Pi or a package reports is, and the whole of a fault that ends the
  host.
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
- Of `agent` the host reads `name` only, for the model's prompt and as the
  author of the agent's commits. It defaults to `agent`.
- `model` may be absent, or lack any of its fields: the host then uses the
  default provider, model and thinking level in Pi's `settings.json`. A
  `provider` without `modelId` gets Pi's default model only when it is Pi's
  default provider. `thinkingLevel` is brought within what the model offers;
  without one anywhere it is `medium`.
- `mounts` and `network` come from bzz's configuration at every start. The
  host never stores them.
- A `mounts` entry:
  - `name`: a simple name, `[A-Za-z0-9][A-Za-z0-9._-]{0,63}`, unique. The
    folder appears at `/mnt/<name>`. `/mnt` holds granted folders and nothing
    else.
  - `path`: the folder on this machine, as an absolute path. A link is
    followed once, at the start. If it is not a folder, `/mnt/<name>` is
    there and empty of everything.
  - `write`: `true` lets the agent's file tools, shell and git change the
    folder. Anything else is read-only.
  - `exec`: `true` lets the agent start real commands there (section 9).
    Anything else does not. It is separate from `write`. bzz sends `true`
    for a folder its settings grant for commands, whether the owner is asked
    about each command or not: that part is bzz's alone, and the host is
    never told.
- A `network` entry is `host` or `host:port`, such as `github.com` or
  `git.example.org:8443`. It is matched exactly: that host, not the names
  below it, and that port. `host` alone means the port of the scheme: 443,
  or 80 for plain HTTP. Only HTTPS is let through, and plain HTTP to this
  machine itself (`localhost`, `127.0.0.1`, `[::1]`). Each hop of a redirect
  is checked, at most five; `Authorization` and `Cookie` headers do not
  follow a redirect to another host or port.
- `piDir` is Pi's folder: `auth.json` for the sign-in, `settings.json` for
  the default model and the packages. bzz always sends it: the folder
  `PI_CODING_AGENT_DIR` names in bzz's own environment, or `~/.pi/agent` of
  the owner's home. Without it the host takes `PI_CODING_AGENT_DIR` from its
  environment, or `~/.pi/agent`, which under bzz is inside the agent's
  folder.
- `packages`: `false` switches Pi packages off for this agent. Absent or
  anything else: on (section 10). bzz always sends it, `true` or `false`,
  from the agent's settings.
- A bad `mounts` or `network` entry fails `initialize` with `-32602` and the
  reason. `initialize` may then be sent again, corrected. A second
  `initialize` after a successful one gets `-32600`, and so does any other
  request before it.
- `initialize` may take as long as the package time limit, 10 s, when a
  package hangs. A package that never gives the thread back holds the host
  for good (section 10): bzz needs a time limit of its own for `initialize`,
  longer than 10 s.
- **bzz gives `initialize` 30 s.** A harness that has not answered by then
  is stopped (end of input, TERM, KILL), and the start counts as a failed
  one: after five within five minutes the agent is left alone until that
  time has passed, and whoever asked is told that it is not available.

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
  (section 9). Empty when it has no such tool: no grant has `exec`, the host
  is a remote copy, or this is Windows.
- **bzz keeps `host`, `run` and `packages`** of a host that answers
  `"tools": "pipe"`, in `started.json` of the agent's folder, in place of
  the ones before. `bzz agent show` and `bzz agent doctor` print them. Only a
  simple name is kept as a folder, and every text of a package is bounded
  and made safe to print.
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
  there. Without one that starts with `/` it is `/home/agent`. A missing
  place gets `-32602`.
- For other harnesses bzz stores the session id the harness returned for the
  place and uses `session/load` after a restart when the harness offers it.
  `session/load` takes the parameters of `session/new` and the session's id.
- `mcpServers` is empty for a `"tools": "pipe"` host and holds the server of
  section 6 otherwise.
- A coding harness gets more in `session/new` and `session/load`, and two
  more requests before its first prompt: section 12.

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
  `stopReason`: code `-32010`, a message that says why, and `data.kind`.
  The code is the host's own, in the range JSON-RPC leaves to a server. It
  was `-32000` before, which is ACP's code for "sign in". bzz reads
  `data.kind` first, so it names a failure of either host correctly; without
  a kind, `-32010` is a failure of the model and `-32000` a missing sign-in.

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
- **bzz sends a prompt only for a request it admitted.** Before anything
  reaches a harness: the author may wake the agent (its owner, listed
  people, or anyone, by the agent's rule; only its owner in a DM); the
  message is at most a day old, so nothing older is ever a request; and
  another agent's message has woken this one fewer than six times in the
  last hour in that conversation. People are not counted.
- One turn has ten minutes, waits for the owner included. Then bzz sends
  `session/cancel` and, 5 s later, stops a harness that has not answered.

### `session/cancel`

A notification. The harness stops the turn and answers the pending
`session/prompt` with `stopReason: "cancelled"`. bzz's own host does not wait
for an answer of bzz that is still open, and ends a real command that the
turn started (section 9).

bzz sends it in two cases: when a turn's time limit passes, and when the
owner stops a running turn (`x` in `:agents`). In both, a harness that has
not answered the prompt 5 s later is stopped, and a command of that turn that
still waits for the owner's answer is void from the moment of the cancel
(section 9).

### `_bzz/checkpoint`

A request to bzz's own host, with no parameters. It answers `{}` when both
SQLite files are whole and no write-ahead file is left. bzz sends it before a
move and when the agent has been idle for 60 s: no turn started or ended in
that time. It is not sent during a turn, to a host that is being stopped, or
to another harness.

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
  `completed` or `failed` after it. A `tool_call` has `toolCallId`, `title`
  and `kind`: `read`, `edit` (for `write` and `edit`), `execute` (for `bash`
  and `run`) or `other`. The title is the tool's name, with the path for a
  file tool and the first line of the command, cut at 80 characters, for
  `bash`; for a real command it is `run in /mnt/<name>: <that line>`. The
  `tool_call` of a `run` comes before its `_bzz/approve`. A turn that
  continues after a restart sends nothing until bzz has handed its request
  over again.
- `session/request_permission`. bzz allows its own chat tools, once.
  Anything else is refused, but for a coding harness, whose requests are
  answered by the rule of section 12. An answer names an option of the
  request by its kind (`allow_once`, `reject_once`), never one the harness
  would remember (`allow_always`, `reject_always`); with no such option the
  request is cancelled. bzz's own host never sends it: it asks with
  `_bzz/approve` (section 9).
- Notifications in a namespace bzz does not know, such as `_x.ai/…`, are
  ignored. A request bzz does not know gets error `-32601`.
- The requests of bzz's own host (`_bzz/tool`, `_bzz/credential`,
  `_bzz/approve`) carry string ids: `h1`, `h2`, and so on. Several may be
  open at once, and bzz may answer them in any order. An answer to a request
  the host no longer waits for is dropped.
- bzz answers each under the id it came with, a string or a number. A
  `_bzz/approve` waits in a task of its own for as long as the owner takes,
  and everything else the host sends is read and answered meanwhile. At most
  eight `_bzz/tool` and four `_bzz/credential` of one host are worked on at
  once: a ninth tool call waits in the pipe, a fifth credential is answered
  with an error at once.
- **Only a host that answered `"tools": "pipe"` may send `_bzz/credential`
  and `_bzz/approve`.** From any other harness they are requests bzz does
  not know (`-32601`).

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
- **It travels in `session/new`**, so the harness holds it before the server
  it starts does, and can do through it exactly what the tools allow. bzz
  binds it to the session id the harness answers with; until then it
  resolves to nothing. The Pi Durable host has no such credential: it asks
  over the pipe (section 5), and `mcpServers` is empty for it.
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
| `PI_CODING_AGENT_DIR` | Pi's folder. Default `~/.pi/agent`. Under bzz it is read only when `piDir` is absent |
| `BZZ_AGENT_HOST_BUZZ` | the `buzz` command, as a path or a JSON array with first arguments. Default `buzz` on the `PATH` |
| `BZZ_AGENT_HOST_RECORD` | a folder where each prompt of `buzz-acp` is saved, to record the fixtures again |
| `BZZ_AGENT_HOST_SCRIPTED_MODEL` | tests only, and read under bzz too: a module that plays the model, as the provider `scripted`. bzz never sets it |

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
  port that is not the scheme's own. It is the host of the remote's URL, in
  lower case.
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
- The host sets no time limit on the answer. A cancelled turn does not wait
  for it, and neither does the shell command's own time limit; an answer
  that comes after either is dropped. A question that was open when the
  host exited is void.
- Only `git` asks, and only on a challenge: 401, or the 203 that Azure
  DevOps sends in its place. The shell's `curl` does not. A remote that
  answers "not found" for a private repository never causes the request.
- Credentials are for the host in the remote's URL. They do not follow a
  redirect to another host.

What bzz does with the request:

- **The agent is the one the process was started for.** No parameter names
  it, and bzz looks in that agent's stored credentials only.
- **The session must have a turn running** in that process, and not one the
  owner stopped. Otherwise: an error.
- **The host must be on that agent's list of allowed hosts** in bzz's own
  settings, compared exactly, in lower case: in the settings bzz started
  with, and in the settings file as it is at that moment. bzz checks this
  itself, whatever the host process was told at its start.
- **The answer comes from what the owner stored** with `bzz agent credential
  set <agent> <host>`: a user name and a token, sealed with NIP-44 to the
  owner's own key in `<data>/keys/agent-<agent id>.git/`, one file per host.
  They are never in the settings file. They are opened at the moment of the
  request, so one stored while the agent runs is used by its next git
  command. What is sealed names the agent and the host; a file that was
  moved to another agent, or under another host's name, opens for nobody.
- **Everything else is a JSON-RPC error with code `-32011`**, whose message
  is written for the model and says what the owner can do:

| When | The message |
|---|---|
| the host is not on the agent's list | `bzz gives no credential for <host>: it is not on this agent's list of allowed hosts. Your owner can allow it with …` |
| nothing is stored for it | `No credential is stored for <host>. Your owner can store one with …` |
| the stored one does not open | `The credential stored for <host> could not be opened. Your owner can store it again with …` |
| no turn runs in the session | `No message is being answered in this session right now, so bzz gives no credential.` |

- bzz writes one line per request to the agent's activity: the host and what
  happened (`given`, `none is stored`, `refused: …`). Never the user name or
  the token. They are in no log of bzz's, and bzz keeps them in memory only
  while it writes the answer to the pipe.
- The owner is not asked at the moment of the request: storing a credential
  for an agent and a host is the grant.
- `bzz agent delete` removes an agent's stored credentials with its key.

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
- `mount` is a name from `_meta.bzz.run`. `command` is never empty, holds
  no NUL character and is at most 100 000 bytes; it may hold several lines.
  `kind` is always `exec`.
- bzz is not asked about what can never run: a folder without `exec`, an
  empty command, one with a NUL character or longer than 100 000 bytes, a
  time limit out of range, a folder that is not there. The model gets an
  error.
- The host asks about one command at a time within a turn. Two
  conversations may each have a question open.
- The host sets no time limit on the answer: it waits until bzz answers or
  the turn is cancelled. An answer that comes after the cancel is dropped.
  The command's own time limit starts when the command does.
- bzz decides whether the owner is asked. The host knows nothing of
  "allowed without asking".
- A question that was open when the host exited is void. The next process
  does not act on an answer to it: the model is told that the call was
  interrupted, and a new call asks again.

How bzz answers. `{"allow": true}` is given only when every line below
holds; anything else is `{"allow": false, "reason": "…"}`, and the reason is
written for the model:

1. **The folder is granted for commands in bzz's own settings**, by the
   name in `mount`: in the settings bzz started the host with, and in the
   settings file as it is at that moment, with the same path. So a grant
   the owner took away while bzz runs is refused at once, and one he added
   counts after a restart. What the host was told at its start does not
   count. A settings file that cannot be read, or that no longer has the
   agent as one bzz runs, is a no.
2. **The session has a turn running** in that process, which the owner has
   not stopped, and the agent is neither paused nor being stopped.
3. `command` is not empty, holds no NUL and is at most 100 000 bytes.
4. **The folder is allowed without asking, or the owner said yes.** A folder
   that either the settings bzz started with or the settings file now says
   to ask about is asked about. So is every folder of an agent that anyone
   may wake: "without asking" holds only while the people who may wake the
   agent are ones its owner named.

The owner is asked in the TUI, and only there:

- He is shown the agent, the conversation, the folder with its real path,
  and `command` whole: every line, with what would not show on a terminal
  written out as an escape. A yes is taken only after the end of the command
  has been on screen.
- **Without the TUI (`bzz agent run`) there is nobody to ask.** A command in
  a folder that asks is refused at once, with a reason that tells the agent
  to say in the conversation that its owner must open bzz.
- **A question waits 300 s and then counts as refused.**
- **It is void**, and answered with a refusal that says so, when its turn is
  cancelled or ends, when the agent is paused or stopped, when the host's
  process ends, and when bzz quits or locks.
- **It is answered once.** The owner's answer to a question that timed out
  or became void does nothing, and he is told that nothing was started.
- One conversation has one question open. A second `_bzz/approve` for the
  same session while one waits is refused. Two conversations, and two
  agents, each have their own.
- bzz sends back yes or no, and nothing of the command: the host runs the
  string it asked about.

Every question and its answer is a line in the agent's activity
(`activity.jsonl`, printed by `bzz agent show`): when, the folder, the
command's first line, and one of `asked of you`, `allowed by you`, `refused
by you[: reason]`, `allowed without asking`, `refused: nobody was at bzz to
ask`, `refused: no answer in 300 s`, `refused: the folder is not granted for
commands`, `void: <why>`.

The command:

- starts with the granted folder as its working directory;
- gets an environment built from nothing: `PATH`, `HOME`, `LANG` and
  `TMPDIR` as the host has them, and nothing else. What bzz gives the host
  under those names is what every command gets;
- has no terminal, and an empty standard input;
- has no other open descriptor of the host, on Linux (section 2);
- runs in a process group of its own, not bzz's and not the host's;
- is ended, with its group, when the turn is cancelled, at its time limit,
  and when the host exits, however it exits. The group gets TERM, and KILL
  2 s later. A keeper process outside the host's group does this, so a
  command that ignores TERM can outlive the host by those 2 s;
- leaves nothing running: what is still in its group when it returns is
  killed;
- is not started again after a restart. A turn that was cut off inside a
  command goes on with the model told that the command was interrupted.

Its output, standard output and standard error together, comes back bounded:
the last 2000 lines or 50 KB. A longer output is saved on the agent's disk
(`/tmp/run-<id>.log`, at most 8 MiB) and the result says where. A command
that ends with another code than 0, or by a signal, is a failed tool call
with its output.

What this does not give:

- **The folder is where the command starts, not a wall.** A real command has
  the owner's rights and can reach whatever the owner can. `write: false`
  binds the agent's file tools, not a command.
- A process that leaves the group (`setsid`, a daemon) is not ended.
- If the host and the command's keeper process are both killed at the same
  moment, the command is not ended. bzz's sweep at startup does not see it:
  neither the keeper nor the command is in the host's group.
- On a system that is not Linux a command inherits the descriptors the host
  was started with (section 2).
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
- Each package has 10 s to load. The packages load at the same time. A
  package that never gives the thread back, such as an endless loop, cannot
  be timed out: `initialize` is never answered.
- A package that fails is left out and the others still load. The answer to
  `initialize` reports each one: `{"source", "loaded": true, "extensions":
  [names], "providers": [ids]}` or `{"source", "loaded": false, "error":
  "…"}`. A package without a Pi Durable entry is reported as not loaded,
  with that reason.
- A package may not name an extension `bzz` or `coding-tools` or as an
  earlier package did, nor a tool `run` or `buzz_…`. A tool of a package
  with the name of one of Pi's file tools (`read`, `write`, `edit`, `bash`),
  or of an earlier package's tool, replaces it.
- A provider of a package takes the place of one of Pi's, or of an earlier
  package's, with the same id.
- What a package leaves uncaught after it loaded does not end the host: a
  promise that fails with nobody waiting for it, and an exception from a
  timer or an event whose stack names a file of the package. It is written
  to the log. An exception that names no package ends the host with code 1.
- The host uses the models a provider lists when it loads. It does not fetch
  a provider's models.
- When a turn fails with `provider_unavailable`, the message names each
  package that did not load and why, and says whether a package without a Pi
  Durable entry names that provider. That is read from the package's files
  as text: the provider's id as a quoted string.
- A change to a package or to the list takes effect when the host starts
  again.

## 11. Asking about a model without an agent: `main.mjs --check-model`

`bzz agent new --model`, `bzz agent edit --model` and `bzz agent doctor
--models` ask whether a model can be used before any agent runs. They use the
host's own entry for it, so bzz names no file inside the host:

```sh
node <host dir>/main.mjs --check-model <Pi's folder> '<model as JSON>' [--no-packages]
```

- `<model as JSON>` has the form of `_meta.bzz.model` (section 3); `{}` asks
  about Pi's default.
- It is the check a turn makes, with the providers a turn would have: Pi's
  own and those of the Pi packages in Pi's settings. **Their code runs for
  it**, as at an agent's start. `--no-packages` loads none, and bzz passes it
  when packages are off for the agent.
- It writes one line of JSON to standard output and exits with code 0,
  whatever the answer. What a package prints goes to standard error.

```json
{"ok": true, "provider": "openai-codex", "modelId": "gpt-6-luna", "thinkingLevel": "low", "packages": [ … ]}
{"ok": false, "kind": "provider_unavailable", "message": "…", "package": "~/pi/pi-acme-cloud", "packages": [ … ]}
```

- `kind` is one of `session/prompt`'s first four (section 3), or `error` for
  a question that could not be asked.
- `package` is there for `provider_unavailable` when a package of Pi's
  settings names that provider and has no Pi Durable entry (section 10).
  `bzz agent new` then says so; without it, bzz says that no package names
  the provider, or that packages are off.
- `packages` is the report `initialize` would give.
- It reads Pi's sign-in as the host does, creates nothing in Pi's folder,
  and starts no agent. bzz gives it 30 s.

## 12. Coding harnesses: Grok, Codex and Claude Code

Each is the owner's own installed program, with the sign-in he made there
(ADR decision 8). bzz starts it, speaks ACP to it as to any harness, and
sets its guard itself: in the mode the owner's own settings name, all three
write anywhere without asking.

### Finding and starting the program

| Harness | Program | Named instead by | Versions bzz starts | Started as |
|---|---|---|---|---|
| `grok` | `grok` on `PATH` | `BZZ_GROK` | 1.0.77 up to, not including, 1.1.0 | `grok agent --no-leader stdio` |
| `codex` | `codex` on `PATH` | `BZZ_CODEX` | 0.166.0 up to, not including, 0.167.0 | `node <adapter>/node_modules/@agentclientprotocol/codex-acp/dist/index.js` |
| `claude-code` | `claude` on `PATH` | `BZZ_CLAUDE` | 2.1.322 up to, not including, 2.2.0 | `node <adapter>/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js` |

- bzz runs `<program> --version` and reads the first `a.b.c` it prints.
  Outside the range nothing is started: `agent new`, `agent doctor` and the
  start each say which version was found and which are known to work. The
  first version of each range is the one the tests ran with; later ones in
  it are accepted and were not tested.
- The adapters are `@agentclientprotocol/codex-acp` 2.5.0 and
  `@agentclientprotocol/claude-agent-acp` 0.101.0, each pinned by a lock
  file inside the bzz binary (`agent-adapters/`). They are installed on
  first use with `npm ci --ignore-scripts --omit=optional --omit=dev` in a
  folder of their own, which gets its name only when the install is whole
  and holds exactly the pinned version. The optional packages are the
  adapters' own copies of `codex` and `claude`: they are not installed and
  not used. Node is found as for bzz's own host (`BZZ_NODE`).
- An adapter is pointed at the owner's program, with links followed:
  `CODEX_PATH` for Codex, `CLAUDE_CODE_EXECUTABLE` for Claude Code.
- `command` in the agent's settings replaces the program and its arguments.
  Nothing is found, checked or installed then; the rest of this section
  still holds.

The environment, beside `PATH`, `HOME` (the owner's home), `LANG` and
`TMPDIR`:

| Variable | For | Value |
|---|---|---|
| `GROK_SANDBOX` | Grok, not on Windows | `bzz-agent` |
| `INITIAL_AGENT_MODE` | Codex | `workspace-write` |
| `CODEX_PATH`, `CLAUDE_CODE_EXECUTABLE` | the adapter | the owner's program |
| `GROK_HOME`, `CODEX_HOME`, `CLAUDE_CONFIG_DIR` | the program whose it is | as in bzz's environment, when set: where its sign-in is |
| `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` (and in lower case), `NODE_EXTRA_CA_CERTS`, `SSL_CERT_FILE`, `SSL_CERT_DIR`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME` | all three | as in bzz's environment, when set |

No API key is handed on (`XAI_API_KEY`, `OPENAI_API_KEY`, `CODEX_API_KEY`,
`ANTHROPIC_API_KEY`): a coding harness uses the sign-in the owner made with
the program itself.

### The guard

- **Grok.** Before every start bzz writes `.grok/sandbox.toml` in the
  agent's working folder, in place of whatever is there:

  ```toml
  [profiles.bzz-agent]
  extends = "workspace"
  deny = ["<data>/keys"]            # the folder of bzz's keys
  read_write = ["/home/ann/site"]   # the folders granted for writing
  ```

  and names it with `GROK_SANDBOX`. Grok applies it to its whole process:
  it reads everything the owner can but what `deny` names, and writes in
  its working folder, the folders above, its own `~/.grok` and the
  machine's temporary folders. `deny` holds for reading and for writing,
  for Grok's own tools and for every command it runs. A profile of that
  kind that cannot be applied stops Grok at its start; a built-in one would
  only be warned about. On Linux Grok needs bubblewrap (`bwrap`) for
  `deny`: bzz looks for it and says so when it is missing. Grok sends no
  permission request, so there is nothing for bzz to answer. On Windows
  Grok has no sandbox, and bzz sets none.
- **Codex.** bzz sets the mode `workspace-write` on every session. Codex's
  sandbox then holds what it runs to its working folder, the folders it was
  told of and the temporary folders, with no network, and Codex asks leave
  for the rest.
- **Claude Code.** bzz sets the mode `default` on every session. Claude Code
  has no boundary of its own: it asks leave before each edit and each
  command, and bzz's answers are all that holds it.

### What a session is told

`session/new`, and `session/load` with the session's id beside it:

```json
{"cwd": "<data>/agents/<agent id>/workspace",
 "mcpServers": [ … section 6 … ],
 "additionalDirectories": ["/home/ann/site"],
 "systemPrompt": {"mode": "append", "content": [{"type": "text", "text": "…"}]},
 "_meta": {"bzz/place": "…",
           "rules": "…",
           "claudeCode": {"options": {"settingSources": [], "strictMcpConfig": true}}}}
```

- `systemPrompt` and `additionalDirectories` are sent to a harness that
  offered them at `initialize` (`agentCapabilities.sessionCapabilities`).
  Both adapters do. Grok offers neither: it takes the same text as
  `_meta.rules`, and its folders from its sandbox profile.
- The text says who the agent is and how to answer (`send` of the `buzz`
  server), its working folder, the folders it may read and those it may
  write in, what happens to a shell command, what to do when `send` fails
  (stop and say so; never look for another way to post, nor for keys), and
  the agent's instructions. A harness that took it is not sent the
  instructions again in each message.
- `additionalDirectories`: for Claude Code, every granted folder; for Codex,
  only those granted for writing, because Codex makes writable what it is
  told of.
- `_meta.claudeCode.options`, for Claude Code only: none of the owner's
  Claude settings are loaded, nor those of the working folder, which the
  agent can write: no allow rule, hook, plugin or MCP server of his. The
  sign-in is not a setting and is still found.

Then, before the first `session/prompt` of that session, new or loaded:

1. `session/set_mode` with `{"sessionId", "modeId"}`: `workspace-write` for
   Codex, `default` for Claude Code. Grok has no modes and gets none.
   - A session whose answer lists its modes without that one is not asked.
   - A harness that refuses the mode, or does not know the method, leaves a
     session that is **never prompted**: the request ends with a notice
     (`not-guarded`), and `:agents` says why.
2. `session/set_config_option` with `{"sessionId", "configId", "value"}`,
   when the owner chose a model: the option whose `category` is `model`,
   from the `configOptions` of the session's answer. A model that is not
   among its values is not set, and the request ends with a notice
   (`model-unavailable`) that says so. Then the thinking level, in the
   option whose `category` is `thought_level`; a level the model does not
   have is passed over, and the owner is told in `:agents`.

### Permission requests

A request is read from its `toolCall`, and from what the harness reported of
that call before (`tool_call` and `tool_call_update`, by `toolCallId`): a
harness may ask leave for a call by its id alone, as Codex does. The
request's own `kind`, `title` and `rawInput` come first; every path of either
counts. Text of a file is not kept.

| What is asked | Read from | Answer |
|---|---|---|
| One of bzz's chat tools | `kind` absent or `other`, and a `title` that reads `buzz` then `send`, `read` or `react` (`mcp__buzz__send`, `buzz__send`); for Codex, a reported call marked `_meta.is_mcp_tool_call` with `rawInput.server` `buzz` and that tool | allowed |
| A file edit | `kind` `edit`; paths in `locations[].path`, in `content[]` of type `diff`, and in `rawInput.file_path`, `path` or `notebook_path` | allowed when there is a path and **every** path is inside the working folder or a folder granted for writing |
| A read | `kind` `read` or `search`; the same paths | allowed when every path is inside the working folder or a granted folder |
| A shell command | `kind` `execute`; `rawInput.command`, a text or a list of texts; `rawInput.cwd` when it says where | below |
| Anything else: `delete`, `move`, `fetch`, `switch_mode`, a tool of another MCP server, no kind, no path, no command | | refused |

- A path counts where it really is: links are followed as far as the path
  exists. A path that is not absolute, holds `..`, or ends in a link to
  nowhere is refused.
- Every request but one for a chat tool must belong to a turn that is
  running in that process and that the owner has not stopped.
- The folders are those bzz started the agent with that the settings file
  still grants at that moment, each with what both allow.
- bzz selects `allow_once` or `reject_once`. With neither on offer the
  outcome is `cancelled`.
- What bzz refuses, and every command, is a line of the agent's activity.
- A turn that ends with no answer after a refusal gets the notice
  `not-allowed`: Codex ends its turn when it is refused.

A shell command, by the rule of section 9 for a program that starts its
commands itself:

1. It is refused when no folder is granted for commands, when it would start
   (`cwd`) inside a granted folder that is not granted for commands, or when
   it would start anywhere that is neither the working folder nor a granted
   folder.
2. Started inside a folder granted for commands, that folder decides
   whether the owner is asked. Started in the working folder, or with no
   `cwd`, every folder granted for commands counts, and the owner is asked
   unless all of them are allowed without asking: a command is not held in
   the folder it starts in.
3. An agent anyone may wake is always asked about.
4. The owner is asked in the TUI as in section 9, with the same time limit
   and the same reasons for a question to be void. He is shown where the
   command would start and the command **as the program reports it**. bzz
   answers the program's request and starts nothing: that what runs is what
   was shown is the program's to keep to. His reason for a refusal stays in
   the activity; the protocol has no place to hand it to the program.

Grok sends none of these. Codex sends one when a command or a file change
would leave its sandbox, and for every tool of an MCP server. Claude Code
sends one for every edit, every command and every tool of an MCP server.

### Transcripts: `BZZ_AGENT_RECORD`

When `BZZ_AGENT_RECORD` names a folder in bzz's environment, bzz writes one
file per start of a coding harness there, `<harness>-<time>.ndjson`: one
JSON object per line.

| Line | What |
|---|---|
| `{"transcript": {"harness", "bzz"}}` | the first line |
| `{"out": <message>}` | what bzz wrote to the pipe |
| `{"in": <message>}` | what the harness wrote |
| `{"tool": {"session", "name", "arguments"}}` | a call of a chat tool, as it reached bzz |

It is for the tests (`tests/fixtures/agents/transcripts/`), and it holds the
text of the conversation it records. It never holds:

- the session credential (`<credential>`) or the path of bzz (`<bzz>`);
- paths of the machine: `<workspace>`, `<folder:NAME>`, `<data>`, `<home>`;
- what changes from run to run: a prompt's text, the system prompt, the
  place and the request (`<any>`);
- the parameters of any notification but `session/update`, and of any
  request but `session/request_permission`. Grok lists the owner's own MCP
  servers in a notification, with whatever their settings hold;
- of an answer, anything bzz does not read: of `initialize`, only the
  protocol version, the capabilities, the program's name and the ids of its
  sign-in methods; of a session, only its id, modes and options; of a
  prompt, only the stop reason. Usage and the commands on offer are left
  out of `session/update`.
