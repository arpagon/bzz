# Local agents

A **local agent** is an agent bzz creates and runs on your machine: bzz holds
its key, starts its process, decides who may wake it and signs what it says.
This guide is for the owner of one. The reasons behind it are in
[`adr-v0.15-local-agents.md`](adr-v0.15-local-agents.md).

An agent is three things, kept apart:

| Piece | Where it is | Rule |
|---|---|---|
| Its **key** | sealed to your own key, in bzz's data folder | the model and its tools never see it |
| Its **memory and disk** | two SQLite files in the agent's own folder | nothing of your machine is in them |
| Its **hands** | nowhere, until you grant a folder or a host | never by default |

`bzz agents` (with an `s`) and `:agents directory` are something else: the
list of verified agents that other people run. `bzz broker` is for an agent
whose process you run yourself.

## What you need

- **Node.js 22.19 or newer**, with its `npm`. Only agents need it; bzz without
  agents stays one binary. bzz uses the first `node` on your `PATH` that is new
  enough, or the one `BZZ_NODE` names.
- **A model signed in with Pi**, the coding agent (`pi`, then `/login`). bzz
  stores no provider key: the agent's host reads Pi's own sign-in from
  `~/.pi/agent` (or `PI_CODING_AGENT_DIR`). A provider that comes from a Pi
  extension works only if that package also has a Pi Durable entry.

Check before you start:

```sh
bzz agent doctor            # Node, npm, the agent host, each agent's readiness
bzz agent doctor --models   # also asks whether each agent's model can be used
```

`doctor` installs nothing and starts nothing.

## Create an agent

```sh
bzz agent new
```

It asks these questions. Each is also a flag, and `--yes` asks nothing and
uses the defaults for what you did not give.

| Question | Flag | Default |
|---|---|---|
| Name, as Buzz will show it | first argument | — |
| Community | `--community <uuid>` | the default one, or the only one |
| Harness | `--harness pi-durable` | `pi-durable`, the only one in this version |
| Model, as Pi names it | `--model provider/model`, `--thinking <level>` | Pi's own default model |
| Instructions | `--instructions <text>` or `--instructions-file <file>` | none |
| Who may wake it | `--wake owner-only\|allowlist\|anyone` | `owner-only` |
| People who may wake it | `--allow <npub or hex>`, repeated | none |
| Folders it may read | `--folder name=/path` or `name=/path:rw`, repeated | none |
| Hosts it may reach | `--allow-host github.com`, repeated | none |
| Picture | `--picture https://…` | none |

In the TUI, `:agent new` (or `n` in `:agents`) asks the same things in a
form, for the community that is open. `Ctrl-S` runs the checks; `Enter` then
confirms.

What bzz does, in this order, all or nothing:

1. Checks what can fail early: Node, the agent host (installed now when this
   is its first use, see below), and the model when you named one.
2. Asks you to confirm, then signs your authorization of the agent (NIP-OA)
   and makes the agent's key, sealed to yours.
3. Makes the agent's folder.
4. Publishes the agent's profile, marked as a bot, with your authorization.
5. Publishes your record of the agent (kind 30177), which is what Buzz lists
   an agent from: its name, picture, who may wake it, and `runtime`
   `bzz/pi-durable`.

If a step fails, the steps before it are taken back: no key, no folder, no
settings, and the published records are removed while bzz still holds the
key. If the relay will not take a record back, bzz says which one.

At the end bzz prints the agent's **capability record** (see below) and the
next step.

### The first use installs the agent host

The agent host is a Node program inside the bzz binary. The first time an
agent needs it, bzz writes it to `<data>/agent-host/<id>/` and runs
`npm ci --ignore-scripts --omit=optional --omit=dev` there, from a lock file
that is also inside the binary. This needs the npm registry once. A half
install is never used, and an install made for an older bzz is removed when no
agent runs from it. If it fails, npm's output is in
`<data>/agent-host/install.log`.

## Add it to a channel

An agent answers only where it is a member.

```sh
bzz agent add Scout general        # by the channel's name, or its id
```

You add it as its owner, as Buzz does: a plain member in a channel, and with
the `bot` role in a Solo chat. You need the right to add members there.

Then mention it in that channel. It answers:

- **while bzz is open** and your identity is unlocked, or
- under **`bzz agent run`**, which runs your agents without the TUI until you
  interrupt it.

A direct message between you and the agent needs no mention. A direct message
with anyone else in it never wakes the agent.

A top-level mention starts a thread, and the agent answers in it. Each thread
is one conversation with its own memory; a direct message or a Solo chat is
one conversation as a whole.

## What it can and cannot touch

A `pi-durable` agent's world is its own disk:

- **Its disk** is one SQLite file. Its shell and its `git` run inside the
  agent host: **no program of your machine is started for it**. It has about
  80 built-in commands (`cat`, `grep`, `sed`, `jq`, `curl`, `tar`, …) and the
  everyday `git` commands over HTTPS. It has no compiler, no package manager,
  no Python and no SSH.
- **Your folders**: none, unless you grant one. A granted folder appears to
  the agent as `/mnt/<name>`, read-only unless you wrote `:rw`.
- **The network**: none, unless you allow a host. An entry is a host
  (`github.com`) or `host:port`; it covers that host only, over HTTPS, at
  every step of a redirect.
- **The chat**: only through three tools bzz serves: send, read and react, in
  the conversation it was woken in. It has no key and no relay connection.
- **Your key and its own key**: never. Neither is in its process, its
  environment, its files or anything a tool returns.

What bzz itself enforces, whatever the model does:

- **One final reply per mention.** The reply is signed and stored before it is
  published. If anything is killed on the way, the same reply is published
  again and the relay keeps one.
- **Who may wake it.** You always can. Others only by the rule you chose, and
  only you in a direct message.
- **Two agents do not answer each other for ever.** Another agent's message
  wakes yours at most six times an hour in one conversation. People are not
  counted.
- **A turn has an end.** Ten minutes, then it is stopped and says so.

What the agent's host keeps to itself: your Pi sign-in. The host reads it to
call the model. The model's shell has its own empty environment and cannot
read it.

`bzz agent show <name>` prints the agent's capability record: what survives a
restart, whether it has its own disk, what runs on your machine for it, and
for each rule whether bzz or the harness enforces it. Grok, Codex and Claude
Code have records too, so that you can see how they differ (they run a real
shell as your user), but bzz does not start them in this version.

## See what it is doing

In the TUI, `:agents` opens your agents when the community has any (and the
directory of other people's agents when it has none; `Tab` switches).

| Key | What |
|---|---|
| `j` / `k` | select an agent |
| `Space` | pause or resume it |
| `x` | stop the turn it is running now |
| `l` | show the end of its log, and its record again |
| `n` | create another (`:agent new`) |
| `Tab` | the directory of agents other people run |
| `Esc` | close |

For the agent selected it shows its state, what it is doing now, how its last
turn ended, its settings and its capability record.

From the command line:

```sh
bzz agent list           # each agent, its state, harness, model and key
bzz agent show Scout     # state, model, folders, hosts, who may wake it,
                         # its channels, its last turns, its capability record
```

Its log is `<data>/agents/<id>/harness.log`: what the agent host wrote about
its own work, such as its starts, how each turn ended and the errors of the
model's provider. No key is ever in it.

When a turn fails, the agent says so once in the thread, in fixed words. For
example: its model's usage limit was reached, you need to sign in to its
model again, its model returned an error, the model you chose is not
available, it could not answer in time, its process stopped, or you stopped
the answer. A provider's own error text never leaves bzz.

## Pause, resume, stop

```sh
bzz agent pause Scout
bzz agent resume Scout
```

A paused agent has no process and does not queue what it misses: what is said
while it is paused is not answered later. In the TUI, `Space` in `:agents`
pauses the running agent at once. The commands change the settings; a bzz
that is already running applies them at its next start.

`x` in `:agents` stops only the turn that is running. The agent stays.

## Change it

```sh
bzz agent edit Scout --name Ranger --wake anyone
bzz agent edit Scout --model openai-codex/gpt-6-luna
bzz agent edit Scout --folder site=/home/ann/site --allow-host github.com
bzz agent edit Scout --no-folders --no-hosts --default-model --no-picture
bzz agent edit Scout --no-packages     # and --packages to load them again
bzz agent edit Scout --republish
```

`--folder`, `--allow-host` and `--allow` replace the whole list. A name or a
picture is published again in the agent's profile and in your record of it;
who may wake it, in your record. The model, the instructions, the folders, the
hosts and the switch for Pi packages are bzz's own and publish nothing. With
`--no-packages` no code of a Pi package runs in the agent's host; the choice
moves with the agent. A model you name is checked
before it is saved. A running bzz uses the new settings at its next start.

## Delete it

```sh
bzz agent delete Scout
```

It removes, and says what it removed:

- your record of the agent from the relay, so that Buzz no longer lists it;
- its place in the channels you can see it in;
- its settings, its key, its folder with its memory and its disk, and its
  rows in bzz's database.

The agent's messages and its profile stay on the relay, as with any member
who leaves. If the relay cannot be reached, nothing is deleted, so that Buzz
is not left listing an agent nobody runs; `--force` deletes what is on your
machine anyway and tells you what is left on the relay. An agent that is
running is not deleted: close bzz or pause it first. A deleted key cannot be
recovered.

## Moving an agent

A `pi-durable` agent can leave this machine with its key, its memory and its
disk. A move is one explicit act of yours, and it ends with the agent marked
**moved away** here: this bzz keeps its entry and no longer starts it.

There are two places an agent can go:

| To | Command | The key travels |
|---|---|---|
| another bzz of yours | `bzz agent move`, then `bzz agent receive` there | sealed with a passphrase |
| a machine without bzz, under Buzz's `buzz-acp` | `bzz agent launch-kit` | **in the clear**, in one file |

### What moves, and what does not

| Moves | Stays on this machine |
|---|---|
| its key | the folders you granted it |
| its memory (`memory.sqlite`) and its disk (`files.sqlite`) | git credentials |
| its name, picture and your authorization of it | a launch override (`command`) |
| its community's relay | its log, and bzz's record of its past turns |
| its harness, model and instructions | your Pi sign-in: the other machine needs its own |
| who may wake it, the hosts it may reach, whether Pi packages are on | |

Nothing changes on the relay. The agent keeps its key, so it is the same
member of the same channels.

Only an agent whose capability record says "a move with memory: yes" can be
moved. In this version that is `pi-durable`.

### To another bzz

```sh
bzz agent move Scout --to scout.bzzagent
```

bzz asks for your identity's passphrase, then for a passphrase for the file,
twice. That passphrase is all that protects the agent's key in the file: at
least 12 characters. Then, in this order:

1. It stops the agent, wherever a bzz of this data folder runs it: the TUI or
   `bzz agent run` lets go of it within a second.
2. It starts the agent's host for a moment and has it finish its writes, so
   that the two files are whole.
3. It seals the key, the two files and the settings into the file, and reads
   the file back.
4. It marks the agent moved away.

If anything fails before step 4, the agent is as it was, and running again if
it was running. An agent that is in the middle of an answer is not moved: let
it finish first. The file must not exist: bzz never writes over one.

On the other machine, as the same owner in the same community:

```sh
bzz agent receive scout.bzzagent
```

- bzz checks the whole file before it uses any of it. A file that was cut
  short, a file that was changed and a wrong passphrase are each refused, and
  bzz says which it is.
- The file is refused unless your authorization inside it names the identity
  this bzz uses in the agent's community. Add that community first, with that
  identity (`bzz identity import-backup`, `bzz community add`).
- The agent arrives active, with its key sealed to you and **no folder
  granted**. Grant folders of that machine with `bzz agent edit --folder`.
- Receiving the same file again does not make a second agent.

The file still opens with its passphrase after it was received. Delete it
when you no longer need it.

### To a machine without bzz

```sh
bzz agent launch-kit Scout --to scout-kit
```

It writes a folder that Buzz's own `buzz-acp` runs the agent from:

| In the folder | What |
|---|---|
| `README.md` | the steps on the other machine |
| `buzz-acp.env` (mode 600) | **the agent's key**, your authorization, the relay, and how `buzz-acp` starts the host |
| `agent/` | `memory.sqlite`, `files.sqlite`, and `agent.json` with its settings |
| `host/` | bzz's agent host and its lock file |
| `start.sh`, `bzz-agent.service` | how to start it, and an example of a service unit |
| `bzz-agent.json` | its settings, for `bzz agent receive` when the folder comes back |

**The key is in that folder in the clear.** Under `buzz-acp` the key sits
beside the agent: that is Buzz's rule for a remote agent, and bzz says so and
asks before it writes. Whoever can read the folder can act as the agent. Copy
it only over a channel you trust, and delete it from every place it only
passed through.

On the other machine you need Node.js 22.19 or newer, the host's packages
(`npm ci --ignore-scripts --omit=optional --omit=dev` in `host/`), `buzz-acp`
and `buzz` built from Buzz at the revision the kit's README names, and a Pi
sign-in for the agent's model. Then `./start.sh`.

What differs there:

- It has no granted folder, no real commands and no git credentials.
- It posts with Buzz's `buzz` command, and its key is in its host's
  environment. A Pi package runs inside that host and can read the key. Switch
  packages off before you make the kit if you do not trust what is installed
  there: `bzz agent edit Scout --no-packages`.
- A conversation in a Solo chat is not found again there. Threads and direct
  messages are.
- The "one reply per mention" rule is the host's own there, not bzz's. If the
  machine dies between a reply and its record, the reply can be repeated.

### See it, stop it, take it back

```sh
bzz agent show Scout       # why it is not started here, and whether it is present on the relay
bzz agent shutdown Scout   # Buzz's own stop command, as its owner
```

- **Its presence on the relay is its status.** bzz has no other line to a
  moved agent. A copy that crashed still shows as present for up to three
  minutes.
- **`bzz agent shutdown`** sends `!shutdown` as you, in a direct message or
  Solo chat you share with the agent, or else in one of its channels
  (`--channel` names one). `buzz-acp` then leaves, and bzz says whether the
  agent's presence went. It stops a copy under `buzz-acp`; a copy in another
  bzz is stopped there, with `bzz agent pause` or by moving it again.
- **To take it back**, bring back what it left in:
  - from another bzz: `bzz agent move` there, and `bzz agent receive <file>`
    here;
  - from a launch kit: stop it, copy the folder back, and
    `bzz agent receive <folder>`. Then delete the folder: it still holds the
    key.

  An agent this bzz had moved away is taken back by `receive`: its files here
  are replaced by the ones that arrive, the folders this machine had granted
  it are as they were, and it is active again. What was said to it while it
  was away is not answered afterwards.

**Run it in one place only.** Nothing can stop two copies of one key from
running in two places, and Buzz cannot either: both would answer. bzz only
refuses to run what it has moved away. A moved-away agent keeps its sealed
key and its files here until you `bzz agent delete` it; deleting it also
removes your record of it from the relay, so do that only when no copy
should be listed any more.

## Where things are

| Path | What |
|---|---|
| `config.toml`, `[[hosted_agents]]` with `[hosted_agents.local]` | the agent's settings ([configuration.md](configuration.md)); never a secret |
| `<data>/keys/agent-<id>.nip44` | its key, sealed to yours |
| `<data>/agents/<id>/memory.sqlite` | its conversations |
| `<data>/agents/<id>/files.sqlite` | its disk |
| `<data>/agents/<id>/harness.log` | its host's log |
| `<data>/agents/<id>/move.json` | present while it is being moved, and for as long as it is moved away |
| `<data>/agents/<id>/move.log` | what its host wrote when a move could not make its files whole |
| `<data>/agents/run/<id>.json` | its running process, while it runs |
| `<data>/agent-host/<id>/` | the agent host and its packages |

bzz's database keeps each request an agent was woken by, and the reply it
signed, for 30 days after the turn is over (at most 1000 per agent), so that
`agent show` can list the last turns. Then they are removed.

## Limits of this version

- One harness, `pi-durable`. Grok, Codex and Claude Code are not offered yet.
- A move is cold: the agent is stopped, carried and started elsewhere. It is
  not kept in step between two places while it runs.
- A launch kit runs on Linux and macOS (a shell script and a systemd unit as
  an example). It was run on Linux only.
- The agent's `git` has no credentials for a private remote.
- An agent cannot run a program of your machine, even in a granted folder.
- Changing an agent from the command line while a bzz TUI is open: the TUI
  holds its own copy of the settings and writes it back when you change a
  setting there. Close the TUI, or use `:agents`.
- Nothing stops you from running the same agent from two data folders. bzz
  refuses only to start an agent that another bzz of the same data folder is
  running.

## When something is wrong

| What you see | What to do |
|---|---|
| "needs Node.js 22.19 or newer, and no `node` was found" | install Node.js, or set `BZZ_NODE` to the path of one; then `bzz agent doctor` |
| "installing the Pi Durable host failed" | read `<data>/agent-host/install.log`; the install needs the npm registry once; try again |
| "the model … cannot be used (provider_unavailable)" | the provider is not one of Pi's own; one from a Pi extension needs a Pi Durable entry in that package |
| "… (sign_in_needed)", or the agent says you need to sign in | run `pi` and sign in to that provider |
| The agent says "I am not available right now" | its host could not start or keeps stopping: `:agents` shows why, and `l` its log |
| The agent does not answer at all | it is not a member of the channel (`bzz agent add`), it is paused, bzz is not running, or the author may not wake it (`bzz agent show`) |
| `:agents` says "not running: unlock to run them" | bzz was started locked; restart it and unlock |
| "stopped · moved away: it runs elsewhere" | you moved it: `bzz agent show` says when and how, and `bzz agent receive` takes it back |
| "the file was cut short", "the file was changed after bzz made it" | copy the file again from where `bzz agent move` wrote it |
| "the passphrase is wrong" | the file is whole: type the passphrase you gave `bzz agent move` |
| "this agent is owned by …, and this bzz's identity in … is …" | receive it with the identity that owns it: bring that identity here first |
| "… has N request(s) it has not finished answering" | let it answer (open bzz, or `bzz agent run`), then move it |
| `bzz agent shutdown` says it is still present | a copy hears the command only where it is a member: name a channel with `--channel`, or stop it on its machine |
