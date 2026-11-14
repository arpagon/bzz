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
| Its **hands** | nowhere, until you grant a folder, a host, or a folder for commands | never by default |

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
| Folders it may read | `--folder name=/path`, repeated. Add `:rw` to let it write there, and `:exec` to let it run real commands there ([below](#letting-an-agent-run-commands)) | none |
| Hosts it may reach | `--allow-host github.com`, repeated | none |
| Picture | `--picture https://…` | none |
| Pi packages | `--no-packages` | on: packages named in Pi's settings are loaded ([below](#pi-packages)) |

In the TUI, `:agent new` (or `n` in `:agents`) asks the same things in a
form, for the community that is open. `Ctrl-S` runs the checks; `Enter` then
confirms.

What bzz does, in this order, all or nothing:

1. Checks what can fail early: Node, the agent host (installed now when this
   is its first use, see below), and the model when you named one. When the
   model's provider is missing, bzz says whether a Pi package of yours names
   it without being usable here ([Pi packages](#pi-packages)).
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
  agent host: **no program of your machine is started for it**, unless you
  grant a folder for commands. It has about 80 built-in commands (`cat`,
  `grep`, `sed`, `jq`, `curl`, `tar`, …) and the everyday `git` commands over
  HTTPS. It has no compiler, no package manager, no Python and no SSH.
- **Your folders**: none, unless you grant one. A granted folder appears to
  the agent as `/mnt/<name>`, read-only unless you wrote `:rw`.
- **Your programs**: none, unless you grant a folder with `:exec`. Then it
  may start real commands there, each after your yes
  ([below](#letting-an-agent-run-commands)).
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
restart, whether it has its own disk, what runs on your machine for it
(with the folders where it may run commands, and whether you are asked), and
for each rule whether bzz or the harness enforces it. Grok, Codex and Claude
Code have records too, so that you can see how they differ (they run a real
shell as your user), but bzz does not start them in this version.

## Letting an agent run commands

An agent's own shell cannot build your project or run its tests: it starts
no program. To let it, grant a folder **for commands**:

```sh
bzz agent new Scout --folder work=/home/ann/work:exec
bzz agent edit Scout --folder work=/home/ann/work:rw:exec     # replaces its folders
```

`:exec` is apart from `:rw`. `:rw` lets the agent's own file tools write in
the folder; `:exec` gives it one more tool, `run`, that starts a real command
there with `/bin/sh`.

**You are asked before each command.** When the agent wants to run one, a
line appears at the top of bzz, on whatever screen you are:

```text
 Scout asks to run a command in /mnt/work (#general (a thread)) · Ctrl-Y to see and answer it · refused in 4:32
```

`Ctrl-Y` (or `:approvals`) opens it. Nothing opens by itself, and nothing you
type elsewhere answers it. You see:

- which agent asks, in which conversation, and whose message it is answering;
- the folder, with its real path;
- **the whole command, exactly as the shell will get it**: every line with
  its number, and anything that would not show on a terminal written out
  (`\x1b`, `\t`, `\u{202e}`), so that a command cannot hide part of itself.

| Key | What |
|---|---|
| `a` | allow it. Taken only after the end of the command has been on screen |
| `r` | refuse it; you may write a short reason, which the agent is told |
| `j` / `k`, `PgDn` / `PgUp`, `g` / `G` | scroll a long command |
| `Tab` | the next one, when several agents ask |
| `Esc` | decide later: the line at the top stays |

The agent's host starts the command only on your yes, and runs the very text
you saw. Its output goes back to the agent, which answers in the chat.

What you should know before you grant it:

- **The folder is where a command starts, not a wall.** A real command runs
  as you and can read, change or delete whatever you can, anywhere. `:rw` or
  its absence binds the agent's file tools, not a command. Read the command.
- **A question waits five minutes**, then counts as refused, and the agent
  says so. It is also dropped when you stop the turn (`x` in `:agents`),
  pause the agent, lock bzz or quit. An answer after that starts nothing.
- **Without the TUI nobody can be asked.** Under `bzz agent run` a command
  is refused at once, and the agent tells the people in the chat that you
  must open bzz.
- **Allowing a folder without asking** is the other choice:
  `--folder ops=/srv/ops:exec-without-asking`. Commands there run without
  you, also under `bzz agent run`. Whoever may wake the agent can then make
  it run commands as you. Use it for a folder and an agent you would hand
  your shell to, and keep `--wake owner-only`. bzz refuses the grant for an
  agent that anyone may wake, and such an agent is asked about in any case.
- A command has no terminal and no input, 2 minutes unless the agent asks
  for more (at most an hour), and is ended with everything it started when
  its turn is stopped. A program that detaches itself (`setsid`, a daemon)
  is not ended.
- On Windows the agent's host offers no `run`: the grant is kept and does
  nothing.

`bzz agent show Scout` prints the agent's **activity**: every command it
asked to run, when, in which folder, its first line, and what it was
answered (`allowed by you`, `refused by you: …`, `allowed without asking`,
`refused: nobody was at bzz to ask`, `refused: no answer in 300 s`). It also
prints where the host offered `run` when it last started.

## Private repositories

The agent's `git` reaches only the hosts you allowed, and has no credentials
of its own. For a private repository, store one for that agent and that
host:

```sh
bzz agent edit Scout --allow-host github.com      # with the hosts it has already
bzz agent credential set Scout github.com
# User name for github.com (not shown):
# Token for github.com (not shown):
bzz agent credential list Scout                    # hosts only
bzz agent credential remove Scout github.com
```

- **Give it a token made for this**, not your password: one repository,
  the rights it needs, an expiry date. For GitHub, a fine-grained personal
  access token; the user name is what the host expects with it.
- The user name and the token are read without echo and are never
  arguments. From a script they are the first and second line of standard
  input.
- bzz seals them to your own key, like the agent's key, in
  `<data>/keys/agent-<id>.git/`. They are not in `config.toml`, and they
  open only while your identity is unlocked.
- **When they are used.** When the agent's `git clone`, `fetch`, `pull` or
  `push` is asked by the remote who is calling, its host asks bzz, and bzz
  answers from what you stored, **only for a host on that agent's list of
  allowed hosts and only for that agent**. You are not asked at that moment:
  storing the credential was your yes. The agent's host uses it for that one
  git command and keeps it nowhere; the model never sees it.
- One stored while the agent runs is used by its next git command.
- `bzz agent delete` removes an agent's credentials with its key.

What can go wrong:

| What the agent says | Why |
|---|---|
| `No credential is stored for github.com` | store one with `bzz agent credential set` |
| `… is not on this agent's list of allowed hosts` | the host of the remote is not allowed, so nothing is given: `bzz agent edit <name> --allow-host <host>` |
| `… refused the credentials bzz has for it` | the token is wrong, expired, or lacks the right: store a new one |
| a private repository "not found" | some hosts answer that instead of asking who is calling; bzz is then never asked. Check the address and the token's rights |
| nothing happens over SSH | the agent's `git` speaks HTTPS only |

A credential lets the agent do, at that host, whatever the token allows. An
agent that people other than you may wake can be asked by them to use it.

## Pi packages

Packages named under `packages` in Pi's `settings.json` are loaded by the
agent's host when they are written for it (a Pi Durable entry in their
`package.json`). They can bring a model's provider, tools and rules.

- **A package's code runs inside the agent's host, with your rights.** It is
  not in the agent's sandbox: it can read your files and Pi's sign-in.
  Install only packages you would run yourself.
- `bzz agent new --no-packages`, or `bzz agent edit <name> --packages off`,
  switches them off for one agent: no package's code runs for it.
- A package written for the `pi` command only has no Pi Durable entry and
  brings nothing here. When you choose a model whose provider comes from
  such a package, `bzz agent new` names the package.
- `bzz agent show` and `bzz agent doctor` print what the host loaded, and
  what it left out and why, when the agent last started.

## See what it is doing

In the TUI, `:agents` opens your agents when the community has any (and the
directory of other people's agents when it has none; `Tab` switches).

| Key | What |
|---|---|
| `j` / `k` | select an agent |
| `Space` | pause or resume it |
| `x` | stop the turn it is running now |
| `i` | add it to the channel that is open, as `bzz agent add` does |
| `l` | show the end of its log, and its record again |
| `n` | create another (`:agent new`) |
| `Tab` | the directory of agents other people run |
| `Esc` | close |

For the agent selected it shows its state, what it is doing now (also that it
waits for your answer to a command), how its last turn ended, its settings
and its capability record.

`Ctrl-Y`, on any screen, opens the command an agent waits to run
([above](#letting-an-agent-run-commands)).

From the command line:

```sh
bzz agent list           # each agent, its state, harness, model and key
bzz agent show Scout     # state, model, folders, hosts, who may wake it,
                         # its channels, its last turns, the hosts a git
                         # credential is stored for, what its host loaded at
                         # its last start, its activity, its capability record
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
bzz agent edit Scout --folder work=/home/ann/work:rw:exec --packages off
bzz agent edit Scout --no-folders --no-hosts --default-model --no-picture
bzz agent edit Scout --republish
```

`--folder`, `--allow-host` and `--allow` replace the whole list. A name or a
picture is published again in the agent's profile and in your record of it;
who may wake it, in your record. The model, the instructions, the folders,
the hosts and the switch for Pi packages are bzz's own and publish nothing.
A model you name is checked before it is saved.

A bzz that is running uses the new settings at its next start. You can edit
an agent from the command line while the TUI is open: neither writes over
what the other saved.

**What you take away holds at once**, also in a bzz that is running: a
folder that is no longer granted for commands takes none, a folder that now
asks is asked about, and a host you took off the list gets no credential.
bzz reads the settings again before each command and each credential. What
you add (a new folder, a new host, a folder for commands) counts after a
restart, when the agent's host is told of it.

## Delete it

```sh
bzz agent delete Scout
```

It removes, and says what it removed:

- your record of the agent from the relay, so that Buzz no longer lists it;
- its place in the channels you can see it in;
- its settings, its key, the git credentials stored for it, its folder
  with its memory, its disk and its activity, and its rows in bzz's
  database.

The agent's messages and its profile stay on the relay, as with any member
who leaves. If the relay cannot be reached, nothing is deleted, so that Buzz
is not left listing an agent nobody runs; `--force` deletes what is on your
machine anyway and tells you what is left on the relay. An agent that is
running is not deleted: close bzz or pause it first. A deleted key cannot be
recovered.

## Where things are

| Path | What |
|---|---|
| `config.toml`, `[[hosted_agents]]` with `[hosted_agents.local]` | the agent's settings ([configuration.md](configuration.md)); never a secret |
| `<data>/keys/agent-<id>.nip44` | its key, sealed to yours |
| `<data>/agents/<id>/memory.sqlite` | its conversations |
| `<data>/agents/<id>/files.sqlite` | its disk |
| `<data>/agents/<id>/harness.log` | its host's log |
| `<data>/agents/<id>/activity.jsonl` | the commands it asked to run and the credentials it asked for, with the answers; never a secret |
| `<data>/agents/<id>/started.json` | what its host reported when it last started |
| `<data>/keys/agent-<id>.git/` | its git credentials, each sealed to your key |
| `<data>/agents/run/<id>.json` | its running process, while it runs |
| `<data>/agent-host/<id>/` | the agent host and its packages |

bzz's database keeps each request an agent was woken by, and the reply it
signed, for 30 days after the turn is over (at most 1000 per agent), so that
`agent show` can list the last turns. Then they are removed.

## Limits of this version

- One harness, `pi-durable`. Grok, Codex and Claude Code are not offered yet.
- An agent cannot be moved to another machine yet.
- A real command is approved in the TUI of the bzz that runs the agent.
  There is no way to approve from the chat, from another machine, or from
  `bzz agent run`.
- A command is not confined to its folder, and bzz has no list of commands
  that need no approval: a folder asks each time, or never.
- Git credentials are HTTP Basic over HTTPS only: no SSH keys, and no
  credential for the agent's `curl`.
- A bzz that is running keeps the settings it started with, but for what
  you take away. An edit from the command line is safe while the TUI is
  open; what it adds takes effect at the next start.
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
| The agent says a command "needs your owner's approval, and your owner is not at bzz" | it runs under `bzz agent run`: open bzz, or allow that folder without asking |
| The agent says you "did not answer within 300 seconds" | the question timed out; ask it again and press `Ctrl-Y` |
| The agent says "No real command may run in /mnt/…" | that folder is not granted for commands, or the grant was taken away: `bzz agent edit <name> --folder …:exec`, then restart bzz |
| `a` in the question says "scroll to the end of the command first" | the command is longer than the screen: `G` or `PgDn`, read it, then `a` |
| "its harness refused its settings: …" in `:agents` | a granted folder or an allowed host is not one the host takes; the line says which |
| "its harness did not answer in time" | a Pi package may hold the host at its start: `bzz agent edit <name> --packages off`, then look at the package |
