# ADR: v0.15 local agents

**Status:** Accepted. Changes two rules of
[`adr-v0.13-agent-broker.md`](adr-v0.13-agent-broker.md): bzz now starts
agent processes (decision 1 there said it never does), and a local agent may
write in a channel where it is a member without the `bot` role (decision 4
there required it).

**Date:** 2026-11-14

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
     revoked when the process exits.
7. **A Pi Durable agent's default world is its own disk**: a SQLite file
   reached only through the host, with a shell and git that start no real
   process, and a list of hosts it may reach. A real folder appears under
   `/mnt/<name>` only by the owner's grant, read-only unless he allows writes.
   Grants are kept in bzz's configuration and do not move with the agent.
8. **A coding harness is the owner's own program.** bzz starts the installed
   `grok`, `codex` or `claude` with the sign-in the owner made there, in a
   folder bzz creates plus the folders he grants, with the harness's own
   sandbox on where it has one. bzz says, at creation and in `agent show`,
   that such a harness runs a real shell as the owner's user, and where it has
   no sandbox.
9. **Processes are owned.** Each agent runs in its own process group with an
   environment built from nothing. bzz records pid, start time and boot id,
   stops leftovers at startup before it recovers anything, and stops a group
   with TERM, a grace period, then KILL.
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

## Consequences

- bzz without agents is unchanged: one binary, no Node.
- Agents need Node 22.19 or newer. The Pi Durable host's packages are
  installed on first use from a lock file inside the bzz binary, with install
  scripts off.
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
