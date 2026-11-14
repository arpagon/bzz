# ADR: v0.15 local agents

**Status:** Accepted. Changes two rules of
[`adr-v0.13-agent-broker.md`](adr-v0.13-agent-broker.md): bzz now starts
agent processes (decision 1 there said it never does), and a local agent may
write in a channel where it is a member without the `bot` role (decision 4
there required it).

**Date:** 2026-11-14. Decisions 13 to 16 and the details marked "as built"
were added the same day, when M3 and M7 were finished on bzz's side.

## Context

Since v0.13 bzz holds an agent's key and performs relay actions for a process
the user runs himself. The owner now wants bzz to **create and run agents**,
with a choice of four harnesses, and to **move** one to another machine. The
plan is [`planning/2026-11-13/v0.15.0.md`](planning/2026-11-13/v0.15.0.md);
its decisions D1–D20 are the owner's.

Buzz itself has two models, read from its source. Buzz 1.0 runs agents inside
the desktop app, keeps the key in native code and lets the model act only
through tools. Buzz 0.X and its remote agents run `buzz-acp`, which holds the
key, talks to the relay and starts an ACP agent beneath it.

## Decision

1. **Three pieces, placed separately.** An agent's key stays in bzz. Its
   memory and disk are files in its own folder. Its hands, the place where
   real programs run, exist only by the owner's grant.
2. **Local or remote.** An agent is local when this bzz holds its key and
   starts its process, and remote when something else does. `agent move`
   turns one into the other. A moved agent is marked *moved away* and bzz
   refuses to run it.
3. **One ACP client, one process per agent.** bzz starts each agent's harness
   as a child process and speaks ACP to it over stdio. Our Pi Durable host and
   Grok speak ACP themselves; Codex and Claude Code go through their official
   adapters. The wire contract is
   [`agent-host-protocol.md`](agent-host-protocol.md).
4. **bzz decides who wakes an agent and signs what it says.**
   - A mention or DM reaches a local agent only if its author passes the
     agent's rule: the owner only (default), listed people, or anyone. DMs
     stay owner only.
   - As built: nothing older than a day is a request, so a request bzz has
     forgotten cannot come back as a new one; and another agent's message
     wakes a local agent at most six times an hour in one conversation, so
     two agents that may wake each other stop. People are not counted.
   - The agent holds no key and no relay connection. It acts only through
     tools bzz serves: `send`, `read`, `react`.
5. **One final reply per mention.** For a final `send`, bzz takes the mention
   from the calling session's live turn, signs the reply, stores the signed
   event under the mention, and then publishes it. After a crash it publishes
   the stored event again unchanged. A second final `send` for the same
   mention returns the stored reply and publishes nothing. A turn that ends
   with no final `send` publishes the harness's last plain text once, through
   the same record, or a failure notice when there is none.
6. **Tools reach a harness in one of two ways.**
   - Our own host asks over the ACP pipe (`_bzz/tool`). No server and no
     credential exist.
   - Other harnesses get an MCP server over stdio, `bzz agent-tools`, started
     by the harness with a credential bzz made for that session. The
     credential lives in memory, is bound to one agent and one session, and is
     revoked when the process exits. As built: it travels in `session/new`,
     so the harness itself holds it before the server it starts does. That
     gives the harness nothing its tools do not already give it.
7. **A Pi Durable agent's default world is its own disk**: a SQLite file
   reached only through the host, with a shell and git that start no real
   process, and a list of hosts it may reach. A real folder appears under
   `/mnt/<name>` only by the owner's grant, read-only unless he allows writes.
   Grants are kept in bzz's configuration and do not move with the agent.
   As built: its host is started with the agent's own folder as `HOME` (a
   coding harness gets the owner's home, where its sign-in is), so bzz tells
   the host where Pi's folder is at every start.
8. **A coding harness is the owner's own program.** bzz starts the installed
   `grok`, `codex` or `claude` with the sign-in the owner made there, in a
   folder bzz creates plus the folders he grants, with the harness's own
   sandbox on where it has one. bzz says, at creation and in `agent show`,
   that such a harness runs a real shell as the owner's user, and where it has
   no sandbox.
9. **Processes are owned.** Each agent runs in its own process group with an
   environment built from nothing. bzz records pid, start time and boot id,
   stops leftovers at startup before it recovers anything, and stops a
   harness it started in three steps: the end of its input, which a harness
   answers by closing its storage and leaving; TERM to the group 2 s later;
   KILL 2 s after that. A leftover of an earlier bzz, whose input is not
   bzz's to close, gets TERM and then KILL. As built: when the owner stops a
   turn, or a turn's time is up, bzz sends `session/cancel` and stops a
   harness that has not answered 5 s later. A harness is handed its three
   standard streams and no other open descriptor, also none that bzz itself
   inherited. Each harness's standard error is its log,
   `<data>/agents/<id>/harness.log`. bzz's own host is asked to checkpoint
   its files (`_bzz/checkpoint`) when it has been idle for 60 s.
10. **Models come from the harness.** For Pi Durable that is Pi's own login
    file and settings. bzz stores no provider key.
11. **What Buzz sees.** A local agent has a profile marked as a bot and an
    owner-signed record (kind 30177) with a `runtime` value of `bzz/<harness>`.
    It is added to a channel as Buzz 1.0 adds one: as a member, with the `bot`
    role only in Solo chats.
12. **A remote copy follows Buzz's rules.** Under `buzz-acp` the key sits
    beside the agent and the agent posts with the `buzz` CLI. Our host reads
    the place and the request from `buzz-acp`'s prompt text there; a test at
    each Buzz pin guards that reading.
13. **A real command needs the owner's yes, and bzz is where it is given.**
    A Pi Durable agent has a `run` tool only in a folder the owner granted
    for commands. Before each command its host asks bzz (`_bzz/approve`), and
    bzz answers yes only when:
    - bzz's own settings grant that folder for commands, whatever the host
      was told at its start: the settings bzz started with, and the settings
      file as it is at that moment, so that what the owner takes away holds
      at once;
    - the command belongs to a turn that is running and that the owner has
      not stopped;
    - the owner said yes to that command in the TUI, or he allowed that
      folder without asking.

    Asking is the default of the two. Without the TUI there is nobody to
    ask, and the answer is no, with a reason for the people in the chat. A
    question waits five minutes and then counts as refused. It is void when
    its turn is cancelled, when the agent is paused or stopped, and when bzz
    quits; an answer after that starts nothing. The owner is shown the whole
    command as the shell will get it, and bzz answers yes or no and passes
    nothing of the command back, so what was approved is what runs. Every
    question and its answer is kept in the agent's activity.
14. **The folder is not a wall.** A real command runs as the owner's user
    and can reach whatever he can. bzz says so where the grant is made, in
    the question, in `agent show` and in the capability record. Nothing in
    bzz confines a command, and nothing claims to.
15. **Git credentials are the owner's, lent for one answer.** The owner
    stores a user name and a token per agent and host. bzz keeps them as it
    keeps an agent's key: sealed to the owner's own key, outside the
    settings file. bzz gives one to the agent's host only as the answer to
    its request (`_bzz/credential`), only for a host on that agent's own
    allow-list, and only from that agent's own store. The owner is not asked
    at that moment: storing it was the grant. It is in no log, no record and
    no tool result of bzz's, and the agent's host keeps it nowhere.
16. **Pi packages are the owner's code.** A package named in Pi's settings
    runs inside the agent's host, with the owner's rights and outside the
    agent's sandbox. They are on by default, as in Pi, and the owner can
    switch them off per agent.

## Consequences

- bzz without agents is unchanged: one binary, no Node.
- Agents need Node 22.19 or newer. The Pi Durable host's packages are
  installed on first use from a lock file inside the bzz binary, with install
  scripts off. As built: the install's folder is named by a hash of every
  host file inside the binary, not of the lock file alone, so a bzz with
  another host never runs an older install.
- The settings file has one writer at a time. A TUI and a command each hold
  a copy of the settings, and a command that edited them while a TUI was open
  used to be written over by the TUI's next save. A save now takes a lock,
  reads the file as it is, and writes that with only what its own copy
  changed since it last read or wrote it. A running bzz still uses the
  settings it started with.
- One Rust dependency was added for decision 9, `close_fds`: safe Rust
  cannot name a descriptor it did not open, and `unsafe` is forbidden in
  bzz.
- bzz now holds a second kind of secret for an agent, a git credential. It
  opens only while the owner is unlocked, as the agent's key does.
- `bzz broker` stays for a process the user runs himself, with its long-lived
  credential and the `bot`-role rule of v0.13.
- bzz cannot stop two copies of one key from running in two places; Buzz
  cannot either. bzz only refuses to run what it has moved away.
- For a coding harness, "the key stays in bzz" means bzz never gives it the
  key. A real shell running as the owner's user can reach what the owner can.
- Three things move fast under this design and are pinned: Pi Durable, the
  two ACP adapters, and the three coding programs, each with a tested version
  range.

## Rejected

- **Reusing Buzz 1.0's Pi host.** It always gives the agent the user's real
  files and shell, and waits 30 s on a stale lock after a crash.
- **A native driver per harness.** Three protocols to follow, two of them
  undocumented or experimental. Kept as the fallback for a harness whose
  adapter proves unusable.
- **`buzz-acp` for local agents.** It puts the key in the harness's
  environment and posts around bzz, so rule 5 could not be enforced.
- **A JavaScript engine inside bzz, or a port of Pi Durable to Rust.**
- **Docker, a daemon, or root** for any sandbox.
- **Asking the owner at the moment a credential is used.** A clone is
  several requests of one git command, and an agent that waits on its owner
  for each is no use while he is away; the store per agent and host, with
  the allow-list, is the grant.
- **A list of commands that need no approval.** A command is a string for a
  shell: no list of prefixes survives `;`, `&&` or a substitution. The owner
  chooses per folder, once: asked each time, or never.
- **Putting the approval in the chat.** Whoever may wake the agent could
  then answer for its owner. It is asked where the owner's key is unlocked.
