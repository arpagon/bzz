# Apps

An **app** is a small script that bzz runs inside itself for a channel: a
task board, a poll, a rota. bzz draws it, the people of the channel share
its records, and nothing of it runs until you have approved its exact
bytes. This guide is for whoever installs, writes or approves one. The
reasons behind it are in [`adr-v0.16-apps.md`](adr-v0.16-apps.md); what a
script is written against is [`app-api.md`](app-api.md).

> **What this version has.** The store of installed apps, approval at the
> shell and inside bzz, the command line `bzz app`, the pane, and three
> templates: an app can be written, checked, tried, run from your own
> folder (in a terminal, or in a channel's pane on its records),
> installed, approved, rolled back, removed, and opened beside a
> conversation, where a new version replaces the running one in place.
> The agents' tools and sharing in the channel are not built, and each has
> a marked section below.

An app is four things, kept apart:

| Piece | Where it is | Rule |
|---|---|---|
| Its **script** | one Lua file, in bzz's data folder on your machine | it runs only after you approve its bytes, with only what bzz lends it |
| Its **brief** | a short text kept with the script | it says what the app does and what its records hold |
| Its **records** | artifacts of the channel, on the relay | bzz signs them; the script never sees a key |
| Its **pane** | a region of bzz's window, beside the conversation | bzz draws it; see [The pane](#the-pane) |

## What a script can and cannot do

A script computes, returns elements for bzz to draw, reacts to keys, a
paste and a timer, and asks bzz to list, create, change and delete records
of the types it declared, in the channel it is open in. That is all. It has
no file, no network, no process, no environment variable, no native code
and no other app to reach; it never sees a key or a credential; a call that
runs too long is stopped and memory is capped, so it cannot hold bzz. The
whole list of what a script has is one page, [`app-api.md`](app-api.md),
marked `api = 1`; bzz refuses a script written for another number.

## Where an app lives

An app is installed **in a community**, under a name: a lowercase letter
followed by up to 31 of `a-z`, `0-9`, `_` or `-`. Its records belong to
that name (on the relay their types are `bzz.app.<name>.<type>`), so two
apps of one name in one channel share their records.

```text
<data>/apps/<community id>/<name>/versions/0003/app.lua     the script
                                               app.toml    the manifest
                                               brief.md    the brief
```

`bzz paths` prints `<data>`. The folders are yours alone (`0700`, files
`0600`), as bzz's other data.

**Files hold bytes; bzz's database holds decisions.** Which version runs,
which hash you approved, who proposed each version and what became of it
are in the database, not in the folder. A file there says nothing about
whether it may run. You can read the folder with any tool; if a file in it
is changed, bzz finds out, because it compares the bytes with the hash it
recorded before it uses them:

```text
tasks: app.lua of version 1 is not what was approved: the recorded hash is
135c…, and the file on disk has 9f0e…. It was changed outside bzz, and it
is not used
```

The app then does not run until the approved bytes are back, or until you
go to another version. To change an app, change your own folder and add it
again.

**Versions kept.** bzz keeps the files of the version that runs, of the
one that ran before it, of the ten newest approved versions, and of every
version that waits for your answer. A refused, failed or replaced version
keeps no files, only its line in `bzz app show`: who, when, the hash and
what became of it. The fifty newest such lines are kept.

## Writing an app

```sh
bzz app new tasks                 # ./tasks, from the Board
bzz app new --templates           # the apps to start from
bzz app new votes --template board --title "Team votes" --to ~/apps/votes
```

`new` writes a folder with three files and never writes over one that
exists:

| File | What it is |
|---|---|
| `app.lua` | The script. One file, written against [`app-api.md`](app-api.md). At most 128 KiB. |
| `app.toml` | The manifest: the app's name, its title, and what the script declares. |
| `brief.md` | The brief: plain text, at most 16 KiB. |

**The manifest:**

```toml
name = "tasks"
title = "Tasks"
api = 1
types = ["card"]
```

`api` and `types` repeat what the script says (`app.api`, `app.types`).
**The script is the authority**: bzz loads it and takes both from there,
and refuses a folder whose manifest says something else. `name` and `title`
are only in the manifest. No other field is read; an unknown one is an
error.

**The brief** says what the app does, its record types and the fields of
each, and the rules another client must keep to share those records. It is
what you read before you approve, and what an agent in another client
needs to write the same app over the same records. Keep it true when the
script changes. It is text: bzz refuses one with control characters.

The templates:

| Template | Record types | What it is |
|---|---|---|
| `board` | `card` | Cards in three columns: To do, Doing, Done. A move is applied on top of someone else's change: a board merges. |
| `poll` | `poll`, `vote` | One question, its options, one vote for each person; who voted for the selected option is shown by name. |
| `approvals` | `request` | Requests that someone else approves, or rejects with a reason. A decision is never written over another. |

Each fits the pane's 40 columns by 16 rows, and each is written to be read:
the comments say where a rule of the contract is not obvious (the order
field, what a conflict means for it, the answer `rate_limited`, a list
asked for again). **A person is stored by id, and shown by name**: the
Poll's `voter`, the Approvals' `author` and `decided_by` are public keys
(`ctx.user.id`), and the app shows them with `host.name(id)`, the name bzz
knows; a name can change, an id cannot. A template's script and brief are
the files the runtime's tests and the apps bench run
(`tests/fixtures/apps/<name>/`), so what you start from is what is tested.
`:app new <name> [template]` writes the same folder from inside bzz, at
`./<name>`.

Three commands need no community, no relay and no key:

```sh
bzz app check tasks               # load it apart; errors with file and line
bzz app try tasks n "type:Buy milk" enter    # print the screen it draws
bzz app dev tasks                 # run it here, reload it on every save
```

- **`check <script or folder>`** loads the script apart, on a thread of its
  own with nothing behind it, and reports what is wrong:
  `app.lua:11: '}' expected (to close '{' at line 9) near 'local'`. Given a
  folder it also checks the manifest against the script, and the brief.
- **`try <script or folder> [steps…]`** loads the script on records kept in
  memory, does the steps in order, and prints the screen as text. A step is
  a key (`j`, `enter`, `esc`, `ctrl+u`, `shift+tab`, `f1`), `type:TEXT`,
  `paste:TEXT`, `resize:60x20` or `wait:500` (milliseconds; time passes
  only then, so the same steps give the same screen). `--size 40x16` is
  the pane's size and the default. `--seed records.json` starts with
  records:

  ```json
  [
    { "type": "card", "content": { "title": "Buy milk", "column": "todo", "order": 1 } },
    { "type": "card", "content": { "title": "Call Bo", "column": "doing", "order": 2 }, "by": "bo" }
  ]
  ```

  `--log` also prints what the script logged.
- **`dev <folder>`** runs your own folder's app in the terminal, in a
  border of the pane's size, and gives the script to the running app again
  every time you save it. The app keeps its state across a save; a save
  that does not load leaves the old version running and shows the error.
  It asks for no approval, because it is your own folder that you are
  looking at. `Ctrl+C` leaves; every other key is the app's. Its records
  are kept in memory (`--seed` as for `try`) and are gone when you leave.

  To run your folder **on a channel's real records**, do it inside bzz:
  select the channel and type `:app dev <folder>` (below, "Your folder
  in a channel"). `bzz app dev --channel` says so; the command line
  cannot reach a running bzz.

### Your folder in a channel

```text
:app dev ~/apps/tasks       in the selected channel's pane, on its records
```

- **No approval**, because it is your own folder that you are looking at
  (plan D5). The pane's border says `not approved · dev <folder>`.
- **Its records** are the channel's records of the name its `app.toml`
  gives (`bzz.app.<name>.*`), or of the folder's name if it has none. A
  folder named as an installed app works on that app's records: that is
  how you try a change on the real cards. To keep apart from them, give
  the folder another name.
- **A save** of its script is given to the running app within a third
  of a second, which keeps its state. A save that does not load leaves
  the version before running, with the error on its last row.
- **What ends it**: `:app close`, `:app open` of another app in the
  channel, the script's `host.quit()`, a switch of community, `:lock`,
  and quitting. It is never remembered: a restart of bzz does not run the
  folder again, and the app installed in that channel, if any, shows
  again when it ends.

## Installing an app

```sh
bzz app add tasks                 # the folder ./tasks
```

`add` reads the folder, loads the script apart, and records it as a new
version **proposed by you**. It then shows you the version as you will see
every version before you answer (next section) and asks. Until you
approve, nothing of it runs. Without a terminal it asks nothing and
prints the command that approves those exact bytes.

A folder that does not load is not installed, and what ran before still
runs:

```text
bzz: configuration error: tasks: it does not load, and nothing was changed (recorded as version 3, failed):
app.lua:379: unexpected symbol near <eof>
```

Use `--community <id>` when you have more than one community and no
default (`bzz community list`).

## Approval

**Nothing runs that you have not approved, and approval is of exact
bytes.** Every version, also one you add yourself, waits until you answer.
The only thing that runs without approval is your own folder under
`bzz app dev`.

```sh
bzz app pending                   # what waits, in every app
bzz app pending tasks             # one version, as you are shown it
bzz app approve tasks             # shows it, then asks
bzz app approve tasks --version 2 --hash 58e18deefbc46ffd…   # no question; the whole hash
bzz app refuse tasks --reason "no votes on this board"
```

What you are shown of a version:

```text
tasks (Tasks), version 2
From:     you
Proposed: 2026-11-19T14:47:21Z
Hash:     58e18deefbc46ffd1287deb0e7ce01f4fde7cda4c8d3ab0e3beca2a4ed642842
Script:   10.6 KiB (version 1, which runs, is 10.6 KiB)
Brief:    1.6 KiB, the same as the version that runs
Record types: card, vote
  CHANGED from the version that runs:
    + vote   new: the app could not read or write these records before

The brief:
  | # Board
  | …

Difference from version 1, which runs: 1 line added, 1 removed:
@@ -6,7 +6,7 @@
 
 local app = {}
 app.api = 1
-app.types = { "card" }
+app.types = { "card", "vote" }
 
 local COLS = { "todo", "doing", "done" }
 local NAMES = { todo = "To do", doing = "Doing", done = "Done" }
```

- **Who it comes from**: you, one of your agents, or a person of the
  channel, with their public key.
- **The brief**, and whether it changed.
- **The record types it declares, and any change in them, marked as such.**
  A new type is what the app could not touch before.
- **The sizes** of the script and the brief.
- **The difference** from the version that runs, line by line. For a first
  version every line is new.
- **The hash** of the script (SHA-256). It is what an approval names.

Whatever a proposer wrote is shown as text and nothing else: a character
that would move the cursor, start an escape sequence, or turn a line
around is written out as `<U+001B>`, and the version says how many it
has.

`approve` without `--hash` shows the version and asks. With `--hash` it
asks nothing: the hash is your answer, and it must be the hash of the
script that waits under that number. Where there is no terminal to ask at,
an approval must name its hash. If the version's files were changed on
disk after you were shown them, it is not approved and can no longer be.

`refuse` takes a reason of one line, which is kept with the version.

A version that does not load is recorded as failed, with its error, and is
never offered to you. One proposer has one version waiting at a time: a
later one from the same proposer takes the earlier one's place. Up to
eight versions of one app can wait.

### Inside bzz

A version that waits is announced at the top of the screen, wherever you
are, as an agent's command is:

```text
 tasks: version 2 from your agent Scout (e5e5e5e5…e5e5) waits for your approval · Ctrl-Y to see and answer it
```

`Ctrl+Y` (even while an app has the keyboard), `:approvals` or `:app
pending [name]` shows it, after any command of an agent's, which has a
time limit. You see what the shell shows: who it comes from, the hash,
the sizes, the record types **with any change marked `CHANGED`** in the
warning colour, then the brief and the difference, which scroll (`j`/`k`,
`PgUp`/`PgDn`, `g`/`G`). The head stays on screen.

| Key | |
|---|---|
| `a` | approve the hash shown. Taken only once the last row has been on screen |
| `r` | refuse: type a reason (one line, optional), `Enter` |
| `Tab` | the next version that waits |
| `Esc` | decide later: it keeps waiting |

`:app approve <name>` opens the same: an approval is never given by a
command alone. `:app refuse <name> [reason…]` refuses the waiting version
at once. `:app add <folder>` proposes your folder, as `bzz app add` does,
and shows it for your answer.

**An approval reaches an open app without a restart**, whether given
here or at the shell, and so do a rollback and an answer at the shell
(bzz looks at the store every second): the open app is given the new
version's exact bytes **in place**, and keeps its state (its selection,
an open form). The new version replaces the running one only if it loads,
declares the record types you approved, and runs its `app.reload` and its
first `app.view` without an error. If not, it is recorded as failed, with
its error, the version before goes on running with its state, and the
status line says so:

```text
tasks: version 3 failed to run; version 2 goes on: app.lua:41: in app.reload: …
```

**For whoever proposes a version** (M5's agents, M6's channel):
`Apps::record` after `examine`, then `Apps::outcome` says what became of
it: waiting, applied, refused with your reason, or failed with the
error.

## Managing apps

```sh
bzz app list                      # every app: what runs, what waits
bzz app show tasks                # one app, its brief and every version
bzz app show tasks --script       # the script that runs
bzz app rollback tasks            # back to the approved version before
bzz app rollback tasks --to 3     # or to the one you name
bzz app disable tasks             # installed, and run nowhere
bzz app enable tasks
bzz app remove tasks              # uninstall
```

- **`rollback`** makes an earlier approved version the one that runs
  again, with the exact bytes you approved then; no new approval is
  needed. Only versions whose files bzz still keeps can be gone back to.
  `--to` also goes forward to a later approved version.
- **`disable`** keeps the app, its versions and its channels, and runs it
  nowhere until `enable`.
- **`remove`** uninstalls: every version bzz keeps of the app is deleted
  from this machine. **The channel's records are left alone.** They belong
  to the channel, on the relay, and to everyone there who has the app.
  `remove` says how many this bzz has stored; an app of the same name
  installed again finds them. To delete records, delete them in the app
  before you uninstall it.
- `bzz community remove <id> --purge` removes the community's apps with it.

The same from inside bzz, by the same functions:

```text
:apps                          the apps; Enter opens one here, a shows its waiting version
:app rollback <name> [version]
:app disable <name>            closed wherever it is open
:app enable <name>             open again where it was
:app remove <name>             closed; the channel's records stay
```

A rollback of an open app changes it in place, as an approval does.

## Limits

| Limit | Value |
|---|---|
| A script | 128 KiB |
| A brief | 16 KiB |
| A manifest | 4 KiB |
| A title | 64 characters, one line |
| A name | `[a-z][a-z0-9_-]{0,31}`, and not a name Windows keeps for a device (`con`, `nul`, `com1`…) |
| Record types of one app | 16 |
| Versions of one app waiting for an answer | 8, one for each proposer |
| Approved versions kept with their files | the 10 newest, the one that runs and the one before it |
| Lines kept of versions without files | 50 |
| A refusal's reason | 200 characters, one line |

What a running script is held to (time for one call, memory, the size of a
screen, records and writes) is in [`app-api.md`](app-api.md#limits).

## The pane

```text
:apps                     the installed apps, and where each is open
:app open board           open the Board in the selected channel
:app close                close the app of the selected channel
:app zoom                 the app takes the whole window, or gives it back
```

**An app is opened in a channel** and follows it: bzz remembers where each
app is open (`bzz app show` lists the channels), and selecting a channel
shows its app, or none. One app is shown in a channel; opening another
there closes the first. An app is never opened in a direct message. Its
records are the channel's, so closing the app, or opening it again, loses
nothing.

**Only an approved app opens.** The pane starts a script only from the
version you approved (`Apps::runnable`); one that waits says so and how to
answer it (`Ctrl+Y`, `:app pending <name>`). If the approved version
fails as it is started, or declares other record types than you approved,
bzz records the failure and runs the version you approved before it,
when there is one.

**Where it goes.** The pane is 42 columns at the right of the window: 40
for the app and its border. The conversation keeps its 30 columns.

| Window | What is drawn |
|---|---|
| under 72 columns | the conversation alone; the app takes the whole window while it has the focus, and the status line says `app hidden · 5` otherwise |
| 72 to 99 | conversation and app |
| from 100 | the channel list too, then the communities from 122 |
| a thread and the app both open | both from 144 columns (116 with the channel list off); below that the one you asked for last, and the status line says `app behind thread · 5` |
| an app that would have fewer than 16 rows beside the conversation | it takes the whole window while it has the focus, when that gives it 16 (a window of 19 rows) |

An app needs 40 columns by 16 rows. In less, bzz says so in the pane and
the app draws nothing. While a thread waits behind the app, a message you
write goes to the channel, not into that thread.

**Keys.** `5` gives the app the keyboard (`1` to `4` are the other panes;
`Tab` reaches it too, and so does a click on it). **While it has the
keyboard every key is the app's except `Ctrl+C`, which gives it back** to
the conversation. The pane's border and the status line say so. Help,
`:` and `q` are two keys away: `Ctrl+C`, then the key. `Space 5` zooms the
app to the whole window, or back; `Ctrl+C` also leaves the zoom. `Ctrl+Y`
still opens a command one of your agents waits to run. An overlay or the
writing dock, when open, takes the keys before the app.

**A paste** reaches the app as one piece of text, never as keys, when the
app has the keyboard and nothing else is open; otherwise it goes into the
writing dock, also as one piece of text. On Windows the terminal library
gives bzz no paste event: there a paste arrives as keys, in the app as in
the writing dock.

**The mouse.** A click on the pane gives the app the keyboard; the app is
told nothing of it. The wheel over the pane does nothing, and the wheel
over another pane scrolls that pane without taking the keyboard from the
app.

**What the pane says by itself**, in its bottom border, where it costs the
app no row: a wait at the quota of writes, too many records, an archived
channel, records that cannot be read. An app's error, with its file and
line, is on the last row of the app and in the status line; so is an app
that no longer responds. Neither costs the conversation anything: bzz
never waits for a script, and a script that loops is stopped by its own
time budget.

**Colours** are the theme's, and follow it when you change it.

**While it is not shown** an app keeps running, so its state is there when
you come back to its channel; bzz keeps four running at most and closes the
one shown least recently. Every app is closed when you switch community,
lock bzz or quit.

## Agents

An agent woken in a channel can use the records of the apps open there,
without a screen and without the app's script: it reads each app's brief
to learn what the records mean, then lists, creates, changes and deletes
them, as a person does through the pane. Every harness has the tools
(`docs/agent-host-protocol.md`, section 5):

| Tool | Does |
|---|---|
| `apps` | the apps open in this channel, each with its brief and record types |
| `records` | the records of one type, with a `match` as a script's `data.list` |
| `create_record`, `update_record`, `delete_record` | a whole record's content, with the `rev` it saw; a conflict gives it the record as it is now |

- **Only what you opened here.** An app you installed, enabled and opened
  in that channel, and only the record types of the version you approved.
  An app open in another channel, or disabled, is not there for it. bzz
  checks this where it serves the tool, whatever the harness sends.
- **Signed by the agent.** bzz signs each record with the agent's key, on
  the agent's own connection: the record's author is the agent, and the
  relay counts its writes against the agent's quota, not yours. bzz sends
  at most 60 of them a minute; past that, or past the relay's quota, the
  agent is told how long to wait.
- **No question to you**, as for a message: the channel already lets the
  agent write there, and the fence is narrow. Each write is a line of the
  agent's activity (`bzz agent show`). An app you have open shows the
  agent's change at once, as anyone's.
- A guest of the channel can read and not write: the agent is told the
  relay's refusal.

> **Not in this version yet.** Your agents cannot read, try or propose an
> app's script.

<!-- M5: an agent asked in the channel proposes a version through a tool
     (`apps::store::examine`, then `Apps::record` with
     `Proposer::agent(pubkey, name).in_channel(channel)`); it opens your
     approval and the agent is told the outcome: applied, refused with
     your reason, or failed to load with the error. Say here which
     harnesses can author apps, and `:app brief`. -->

## Sharing an app

> **Not in this version.** An app reaches other people only as its folder,
> or as its brief, from which their own agent can write it again.

<!-- M6: `:app share` offers the open app to its channel as one artifact
     (script, manifest and brief: `Apps::bundle`); another bzz shows the
     offer (`Proposer::member(pubkey, name).in_channel(channel)`) and runs
     nothing until that person approves the exact bytes. -->

## Buzz desktop

Buzz desktop shows nothing of an app or of its records, and is not harmed
by them: a person there cannot tell that a channel has an app.

<!-- D11, M8: the record contract a Buzz plugin needs to read and write the
     same records (`docs/app-records.md`), and that an app's brief posted
     as a message is all Buzz desktop shows. -->

## Whose shell it is

`bzz app approve`, `refuse` and `rollback` are your answers because they
are typed at your shell: bzz does not ask who is there. An agent that you
allow to run commands on your machine without asking (`:exec-without-asking`,
[`agents.md`](agents.md#letting-an-agent-run-commands)) can type them too,
as it can do anything else that you can. Grant that with this in mind.

## When something is wrong

| What you see | What it means |
|---|---|
| `app.lua:12: …` from `check`, `try` or `add` | The script's own error, at that line. |
| `app.toml says the record types are …; the script declares …` | The manifest and the script disagree. Make the manifest say what the script says. |
| `the script is written for api 2, and this bzz knows only api 1` | The script needs a newer bzz. |
| `<name> has no approved version (one waits for your answer): nothing of it runs` | Approve or refuse it: `bzz app pending <name>`. |
| `the hash does not name version N` | The hash you gave is not that version's. Take it from `bzz app pending <name>`. |
| `… is not what was approved … It was changed outside bzz` | A file of an installed version was edited. Put it back, `bzz app rollback <name> --to <n>`, or add your folder again. |
| `version N of <name> cannot be used: it was approved, and bzz no longer keeps its files` | It is older than the versions kept. Add its folder again. |
| `<name> is disabled` | `bzz app enable <name>`, or `:app enable <name>`. |
| `<name>: version N failed to run; version M goes on: …` | The approved version loaded apart and failed in the open app: in `app.reload`, in its first `app.view`, or with other record types. It is recorded as failed (`bzz app show <name>`). |
| `scroll to the end first (j, PgDn): an approval is of all of it` | The approval takes a yes only after its last row was on screen. |
| `the seed has records of type …, which the script does not declare` | The script will never see those records: check the seed's `type`. |
