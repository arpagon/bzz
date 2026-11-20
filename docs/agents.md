# Local agents

A **local agent** is an agent bzz creates and runs on your machine: bzz holds
its key, starts its process, decides who may wake it and signs what it says.
This guide is for the owner of one. The reasons behind it are in
[`adr-v0.15-local-agents.md`](adr-v0.15-local-agents.md).

This guide is first about an agent on **`pi-durable`**, bzz's own harness
and the default. An agent can also run on one of three coding programs you
have installed, Grok, Codex or Claude Code: those are different in what they
can touch, and have [a section of their own](#coding-harnesses).

An agent is three things, kept apart:

| Piece | Where it is | Rule |
|---|---|---|
| Its **key** | sealed to your own key, in bzz's data folder | the model and its tools never see it |
| Its **memory and disk** | two SQLite files in the agent's own folder | nothing of your machine is in them |
| Its **hands** | nowhere, until you grant a folder, a host, or a folder for commands | never by default |

`bzz agents` (with an `s`) and `:agents directory` are something else: the
list of every verified agent of the community, those other people run and
your own, which it marks as yours. `bzz broker` is for an agent
whose process you run yourself: it does not list a local agent and refuses
to change one.

## What you need

- **An identity whose key bzz holds.** An identity paired with Buzz Desktop
  (`bzz identity pair-desktop`) cannot have local agents: an agent needs its
  owner's authorization (NIP-OA), signed with your key, and Buzz Desktop
  does not sign one for a program that is paired with it. `bzz agent new`
  and `:agent new` say so before they look for anything or install
  anything. Use an identity made or imported in bzz (`bzz identity new`,
  `bzz identity import`); the same key can be used in Buzz too.
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
| Harness | `--harness pi-durable\|grok\|codex\|claude-code` | `pi-durable`. The others: [coding harnesses](#coding-harnesses) |
| Model | `--model provider/model` as Pi names it (for a coding harness, a model its own program offers), `--thinking <level>` | the harness's own default model |
| Instructions | `--instructions <text>` or `--instructions-file <file>` | none |
| Who may wake it | `--wake owner-only\|allowlist\|anyone` | `owner-only` |
| People who may wake it | `--allow <npub or hex>`, repeated | none |
| Folders it may read | `--folder name=/path`, repeated. Add `:rw` to let it write there, and `:exec` to let it run real commands there ([below](#letting-an-agent-run-commands)) | none |
| Hosts it may reach (`pi-durable` only) | `--allow-host github.com`, repeated | none |
| Picture | `--picture https://…` | none |
| Pi packages (`pi-durable` only) | `--no-packages` | on: packages named in Pi's settings are loaded ([below](#pi-packages)) |

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
   `bzz/pi-durable`. A Buzz 1.0 that is open reads these records when it
   starts: it lists a new agent after it is started again.

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

An agent answers only where it is a member, and only where it may write.

```sh
bzz agent add Scout general        # by the channel's name, or its id
```

You add it as its owner, with the `bot` role: that is what Buzz 1.0 gives an
agent it adds to a channel, or to a Solo chat that exists. (The agent a Solo
chat is created with is a plain member there. bzz answers and lists an agent
with either role.) You need the right to add members there.

In Buzz you can change what an agent of yours may do in a channel, or take
it out:

- **"Viewing only"** (the `guest` role): the agent may read there and not
  write. bzz does not hand a mention from that channel to the agent at all.
  `bzz agent show` says `read-only` beside the channel, and `failed:
  read-only` for the mention. "Participating" gives the right back.
- **"Remove from channel"**: the agent no longer hears that channel. If it
  was answering there, its answer is not posted: bzz asks the relay whether
  the agent is still a member before it publishes anything as the agent.

Then mention it in that channel. It answers:

- **while bzz is open** and your identity is unlocked, or
- under **`bzz agent run`**, which runs your agents without the TUI until you
  interrupt it.

A direct message between you and the agent needs no mention. A direct message
with anyone else in it never wakes the agent. Buzz 1.0 offers no way to
start a direct message with a bzz agent (its "New message" does not list
one): use a Solo chat there.

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
- **The apps of that channel** ([`apps.md`](apps.md#agents)): through five
  more tools, `apps`, `records`, `create_record`, `update_record` and
  `delete_record`, it reads and changes the records of the apps you opened
  in the channel it was woken in, and only the record types each declares.
  bzz signs them with the agent's key; they count against the agent's quota
  at the relay, at most 60 a minute. They need no yes from you, as a
  message does not, and each is a line of its activity in `bzz agent show`.
- **Writing apps** ([`apps.md`](apps.md#agents-write-and-change-apps)):
  through `app_guide`, `apps` with an `app`, `try_app` and `propose_app`
  it reads how an app is written and an app open in that channel, tries a
  script apart on records in memory, and proposes a version with its brief
  when someone who may wake it asks. You approve each version's exact
  bytes; until then nothing of it runs, and it cannot approve, open,
  enable or remove an app. It has at most three versions waiting. At its
  next turn in that channel it is told what became of each: applied,
  refused with your reason, or failed with the error. Nothing about apps is
  in its prompt before that.
- **Your key and its own key**: never. Neither is in its process, its
  environment, its files or anything a tool returns.

What bzz itself enforces, whatever the model does:

- **One final reply per mention.** The reply is signed and stored before it is
  published. If anything is killed on the way, or the relay does not answer,
  the same reply is published again and the relay keeps one. If the relay
  refuses it, that is final: the mention fails as `reply-refused`, and
  nothing else is signed for it.
- **It posts only where it may write.** Before anything is published as the
  agent (a reply, a reply that waited, a message of a turn, a reaction, a
  typing indicator), bzz asks the relay's own records whether the agent is
  still a member of that channel and may write there. If not, nothing is
  posted there, then or later.
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
(with the folders where it may run commands, and whether you are asked),
whether it writes apps, and for each rule whether bzz or the harness
enforces it. Grok, Codex and Claude
Code have records of their own: they run a real shell as your user, and
[what holds each of them](#coding-harnesses) differs.

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
- A command has no terminal and no input, and 2 minutes unless the agent
  asks for more. It is ended, with everything it started, when its turn is
  stopped or reaches its ten minutes, the wait for your answer included. A
  program that detaches itself (`setsid`, a daemon) is not ended.
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

## Coding harnesses

An agent can run on a coding program **you have installed and signed in
to**, instead of bzz's own host:

| Harness | Program | What v0.15.0 promises | Versions bzz starts |
|---|---|---|---|
| `grok` | Grok Build (`grok`) | supported | 1.0.77, and later ones below 1.1.0 |
| `codex` | Codex (`codex`) | **experimental** | 0.166.0, and later ones below 0.167.0 |
| `claude-code` | Claude Code (`claude`) | **experimental** | 2.1.322, and later ones below 2.2.0 |

What bzz says of every one of them, and means:

> It runs a real shell as your user, in a folder bzz makes plus the folders
> you grant, with the harness's own sandbox on where it has one. bzz never
> gives it the key; bzz cannot promise the key is out of reach.

This is not the `pi-durable` agent of the rest of this guide. That one has a
disk of its own and starts no program of your machine. A coding harness is
your own program at work on your own machine.

### What you need

- **The program, installed and signed in by you**: `grok login`,
  `codex login`, or `claude auth login`. bzz has no sign-in of its own, does
  not read yours, and hands no API key of its environment on. A sign-in that
  is missing shows at the agent's first message, as "my owner needs to sign
  in to my model again".
- **A version bzz knows to work** (the table above). The first of each range
  is the one the tests ran with. Outside the range bzz starts nothing, and
  says which version it found. `BZZ_GROK`, `BZZ_CODEX` and `BZZ_CLAUDE` name
  another copy of the program than the first on your `PATH`.
- For **Codex and Claude Code**: Node.js 22.19 or newer with `npm`. They are
  driven through an adapter, which bzz installs the first time it is needed
  (about 20 MB and 80 MB), from a lock file inside bzz, without install
  scripts. The adapter runs **your** program, not a copy of its own.
- For **Grok on Linux**: bubblewrap (`bwrap`), which Grok's sandbox needs to
  hide bzz's keys.

```sh
bzz agent doctor      # each program, its version, its adapter; opens no session
bzz agent new Scout --harness grok --model grok-4.5 --folder work=/home/ann/work:rw
```

`agent new` checks the program, its version and the adapter before it makes
a key, and prints the harness's capability record before you confirm.

### What each can touch

| | Grok | Codex | Claude Code |
|---|---|---|---|
| **Writes** | its working folder, folders granted `:rw`, its own `~/.grok`, and the machine's temporary folders (`/tmp`) | its working folder, folders granted `:rw`, and the temporary folders | its working folder and folders granted `:rw` |
| Who holds that | Grok's own sandbox | Codex's own sandbox; what would leave it, Codex asks for and bzz refuses | **only bzz**, by answering each request by the path it names |
| **Reads** | everything you can read, but the folder of bzz's keys | everything you can read | its folders; with a folder granted for commands, whatever a command reads |
| **Shell commands** | run without asking, inside the sandbox | run without asking inside the sandbox, with no network; to leave it, [as below](#commands-of-a-coding-harness) | none, unless you grant a folder for commands; then [as below](#commands-of-a-coding-harness) |
| **Your own settings of that program** (MCP servers, hooks, rules, plugins) | all apply, and Grok uses them without asking | apply; for a tool of an MCP server Codex asks, and bzz allows only its own | none: bzz tells the adapter to load none |
| Its own sandbox | yes (not on Windows) | yes | **none**: bzz prints a warning at creation and in `agent show` |

Read the row "Reads" twice. A sandbox that holds an agent's writes does not
keep it from reading: a Grok or a Codex agent can read your other files and
bzz's own database of your messages, and whoever may wake it can ask it to.
Wake rules matter more for these harnesses than for `pi-durable`.

How bzz keeps this, whatever your own settings of the program say (in the
mode your settings name, all three write anywhere without asking):

- **Grok**: before every start bzz writes a sandbox profile in the agent's
  working folder (`.grok/sandbox.toml`) and tells Grok to use it. Grok
  refuses to start when it cannot apply it.
- **Codex**: bzz sets the mode `workspace-write` on every conversation
  before the first message. A conversation that does not take the mode is
  never sent anything, and the agent says so.
- **Claude Code**: bzz sets the mode `default`, in which it asks before each
  edit and each command, and answers each request itself:
  - one of bzz's own chat tools: yes;
  - a file edit: yes when every file it names is inside the working folder
    or a folder granted `:rw`, where the path really leads (links are
    followed); otherwise no;
  - a shell command: by the folders granted for commands, below;
  - anything else, and anything bzz cannot read: no.
- bzz never answers "always": a program would remember that in its own
  files, where your settings in bzz do not reach.

What bzz refused, and every command an agent asked to run, is in
`bzz agent show <name>`.

### Commands of a coding harness

`:exec` on a folder means for Codex and Claude Code what it means for a
`pi-durable` agent ([above](#letting-an-agent-run-commands)): each command
waits for your yes in the TUI, and `:exec-without-asking` lets it run without
it. The differences:

- A command that starts in the agent's own working folder follows **every**
  folder you granted for commands: with none it is refused, and you are
  asked unless all of them are `:exec-without-asking`.
- You are shown the command **as the program reports it**. bzz answers the
  program's question and starts nothing itself.
- Your reason for a refusal stays in the agent's activity: the program has
  no place to take it.
- For Codex this is only about commands that would leave its sandbox.
- **Grok takes no `:exec`**: it never asks, so there is nothing to answer.
  bzz refuses the grant and says why.

One more thing about Codex: it writes its own warnings into the
conversation as if they were text of the model. bzz does not take them for
an answer. A Codex turn that ends with nothing but such a warning says "I
finished without an answer".

### What they do not have

| | Instead |
|---|---|
| A disk of its own, and a shell and git that start no program | a real folder, `<data>/agents/<id>/workspace/`, and your real shell and git |
| A list of hosts (`--allow-host`) | the program reaches what it reaches; bzz refuses the flag |
| Pi packages | none are loaded; bzz refuses `--no-packages` and `--packages` |
| Git credentials from bzz (`agent credential`) | your own git, with your own credentials; bzz refuses to store one |
| A move to another machine with its memory | its memory is the program's own saved sessions on this machine |

Its conversations are the program's own sessions: after a restart bzz asks
the program to load the one it kept for the thread. They also show in that
program's own history (`grok sessions`, `codex resume`, `claude --resume`).

### What was seen, and what was not

On Linux, with the three versions named above, on a relay: each answered a
mention once, wrote in a folder granted `:rw`, did not write in a folder that
was not granted, and remembered its conversation after bzz and the program
had restarted. Not tried: macOS, Windows, any other version, and whether an
allow rule in your own Claude settings would get past bzz (bzz starts Claude
Code without your settings so that none is read).

One run is worth knowing. A Grok agent whose `send` tool did not work looked
for another way to answer: in four minutes it ran fifty commands through its
shell, among them a search of bzz's own files for a key. It found none it
could use. bzz now hides its keys from Grok, tells every coding harness to
stop when `send` fails, and keeps its own memory from other processes on
Linux. That is the plain meaning of "bzz cannot promise the key is out of
reach".

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

How a turn ended is said in bzz's own words, here and in `bzz agent show`:

| It says | What happened |
|---|---|
| `answered` | the relay accepted the reply. Not before |
| `sending its reply: signed, and not yet accepted by the relay` | the reply is stored, and the relay has not taken it yet. bzz publishes the same reply again until the relay answers |
| `failed: reply-refused` | the relay refused the reply. Nothing was posted, and nothing is tried again |
| `failed: read-only` | the agent may only read in that channel, so it was not asked |
| `failed: not-a-member` | the agent was taken out of the channel before it could answer |
| `failed: <another kind>` | the turn itself failed; the agent said so in the thread, unless the line adds that its notice was refused or is not accepted yet |

**Presence.** While bzz runs an agent, the agent says so on the relay with
its own key, once a minute, and Buzz shows it as online. It says that it is
gone when you pause it, when bzz closes or locks, and when it is deleted or
moved away. A bzz that is killed says nothing: the relay then forgets the
agent within three minutes. Your own key signs nothing for this.

The directory (`Tab`, `:agents directory`, `bzz agents list`) holds every
verified agent of the community, your own among them. bzz marks those as
yours (`yours · run by this bzz`, `paused here`, `moved away`) and calls
only the others remote.

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
pauses the running agent at once.

The commands change the settings, and a bzz that runs the agent (the TUI, or
`bzz agent run`) follows them within a second: on `pause` it stops the
agent's process, stops listening for it, and the agent says on the relay
that it is gone; on `resume` it runs the agent again. A bzz that was started
while the agent was paused never opened the agent's key: it starts the agent
at its own next start (in the TUI, `Space` in `:agents` does it at once).

`x` in `:agents` stops only the turn that is running. The agent stays.

## Change it

```sh
bzz agent edit Scout --name Ranger --wake anyone
bzz agent edit Scout --model openai-codex/gpt-6-luna
bzz agent edit Scout --folder site=/home/ann/site --allow-host github.com
bzz agent edit Scout --folder work=/home/ann/work:rw:exec --packages off
bzz agent edit Scout --no-folders --no-hosts --default-model --no-picture
bzz agent edit Scout --packages off    # and --packages on to load them again
bzz agent edit Scout --republish
```

`--folder`, `--allow-host` and `--allow` replace the whole list. A name or a
picture is published again in the agent's profile and in your record of it;
who may wake it, in your record. The model, the instructions, the folders,
the hosts and the switch for Pi packages are bzz's own and publish nothing.
With `--packages off` no code of a Pi package runs in the agent's host; the
choice moves with the agent. A model you name is checked before it is saved.

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

- your record of the agent from the relay, so that Buzz no longer lists it
  (a Buzz 1.0 that is open lists it until it is started again);
- its place in the channels you can see it in;
- its settings, its key, the git credentials stored for it, its folder
  with its memory, its disk and its activity, and its rows in bzz's
  database.

The agent's messages and its profile stay on the relay, as with any member
who leaves. If the relay cannot be reached, nothing is deleted, so that Buzz
is not left listing an agent nobody runs; `--force` deletes what is on your
machine anyway and tells you what is left on the relay. A deleted key cannot
be recovered.

If a bzz is running the agent (the TUI, or `bzz agent run`), the delete
reaches it, as a move does. That bzz stops the agent's process at once, also
in the middle of an answer; and before the agent's files go, it closes the
agent's connection and forgets its key. After a delete no bzz publishes as
that agent. If the deletion does not happen (the relay cannot be reached,
say), that bzz runs the agent again. If that bzz does not let go within 15
seconds, nothing on your machine is deleted, and bzz tells you to close it.

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
  there: `bzz agent edit Scout --packages off`.
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
| `<data>/agents/<id>/workspace/` | the working folder of an agent on a coding harness |
| `<data>/agent-adapters/` | the adapters of Codex and Claude Code, and their install logs |
| `<data>/agents/<id>/move.json` | present while it is being moved, and for as long as it is moved away |
| `<data>/agents/<id>/move.log` | what its host wrote when a move could not make its files whole |
| `<data>/agents/<id>/activity.jsonl` | the commands it asked to run and the credentials it asked for, with the answers; never a secret |
| `<data>/agents/<id>/started.json` | what its host reported when it last started |
| `<data>/keys/agent-<id>.git/` | its git credentials, each sealed to your key |
| `<data>/agents/run/<id>.json` | its running process, while it runs |
| `<data>/agents/serving/<id>.<pid>.json` | which bzz holds its key and its connection now; `bzz agent delete` waits for it to be gone |
| `<data>/agent-host/<id>/` | the agent host and its packages |

bzz's database keeps each request an agent was woken by, and the reply it
signed, for 30 days after the turn is over (at most 1000 per agent), so that
`agent show` can list the last turns. Then they are removed.

## Limits of this version

- Codex and Claude Code are experimental, and all three coding harnesses
  were tried on Linux only, each at one version.
- A move is cold: the agent is stopped, carried and started elsewhere. It is
  not kept in step between two places while it runs.
- A launch kit runs on Linux and macOS (a shell script and a systemd unit as
  an example). It was run on Linux only.
- A real command is approved in the TUI of the bzz that runs the agent.
  There is no way to approve from the chat, from another machine, or from
  `bzz agent run`.
- A command is not confined to its folder, and bzz has no list of commands
  that need no approval: a folder asks each time, or never.
- Git credentials are HTTP Basic over HTTPS only: no SSH keys, and no
  credential for the agent's `curl`.
- A bzz that is running keeps the settings it started with, but for what
  you take away, and for `pause`, `resume` and `delete`, which reach it at
  once. An edit from the command line is safe while the TUI is open; what
  it adds takes effect at the next start.
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
| The agent does not answer at all | `bzz agent show <name>` tells most of these apart. It is not a member of the channel, or was removed from it (`bzz agent add`); it is a member who may only read there ("Viewing only" in Buzz: `read-only` beside the channel; give it "Participating"); it is paused; bzz is not running (Buzz then shows it offline); or the author may not wake it |
| Buzz 1.0 does not list an agent you just made, or still lists one you deleted | Buzz 1.0 reads the agent records when it starts: start it again |
| Buzz 1.0 does not offer the agent for a direct message | it lists only its own agents there; talk to a bzz agent in a channel or a Solo chat |
| `bzz agent show` says `failed: reply-refused` | the relay refused the agent's reply (a time-out by a moderator, or a rule of the relay); nothing was posted. Ask again when the cause is gone |
| "is an identity whose key Buzz Desktop holds … cannot have local agents" | use an identity made or imported in bzz for the community's agents ([above](#what-you-need)) |
| `:agents` says "not running: unlock to run them" | bzz was started locked; restart it and unlock |
| "stopped · moved away: it runs elsewhere" | you moved it: `bzz agent show` says when and how, and `bzz agent receive` takes it back |
| "the file was cut short", "the file was changed after bzz made it" | copy the file again from where `bzz agent move` wrote it |
| "the passphrase is wrong" | the file is whole: type the passphrase you gave `bzz agent move` |
| "this agent is owned by …, and this bzz's identity in … is …" | receive it with the identity that owns it: bring that identity here first |
| "… has N request(s) it has not finished answering" | let it answer (open bzz, or `bzz agent run`), then move it |
| `bzz agent shutdown` says it is still present | a copy hears the command only where it is a member: name a channel with `--channel`, or stop it on its machine |
| The agent says a command "needs your owner's approval, and your owner is not at bzz" | it runs under `bzz agent run`: open bzz, or allow that folder without asking |
| The agent says you "did not answer within 300 seconds" | the question timed out; ask it again and press `Ctrl-Y` |
| The agent says "No real command may run in /mnt/…" | that folder is not granted for commands, or the grant was taken away: `bzz agent edit <name> --folder …:exec`, then restart bzz |
| `a` in the question says "scroll to the end of the command first" | the command is longer than the screen: `G` or `PgDn`, read it, then `a` |
| "its harness refused its settings: …" in `:agents` | a granted folder or an allowed host is not one the host takes; the line says which |
| "its harness did not answer in time" | a Pi package may hold the host at its start: `bzz agent edit <name> --packages off`, then look at the package |
| "runs your own `grok` …, and none was found on PATH" | install the program and sign in, or set `BZZ_GROK`, `BZZ_CODEX` or `BZZ_CLAUDE` to its path |
| "is version …, which is outside the versions bzz knows to work" | install a version in the range the message names; bzz does not start another |
| "needs bubblewrap (`bwrap`) on Linux" | install the package `bubblewrap` |
| "installing the adapter … failed" | read `<data>/agent-adapters/install-<name>.log`; the install needs the npm registry once |
| The agent says "my program did not accept the limits bzz sets on it" | the program refused bzz's mode; `:agents` says which. Nothing was sent to it |
| The agent says "I needed something on my owner's machine that my owner has not allowed" | it asked for a file or a command outside what you granted; `bzz agent show <name>` lists what |
| The agent says "the model my owner chose for me is not available" | the program does not offer that model; `:agents` lists the ones it does |
