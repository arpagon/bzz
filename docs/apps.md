# Apps

An **app** is a small script that bzz runs inside itself for a channel: a
task board, a poll, a rota. bzz draws it, the people of the channel share
its records, and nothing of it runs until you have approved its exact
bytes. This guide is for whoever installs, writes or approves one. The
reasons behind it are in [`adr-v0.16-apps.md`](adr-v0.16-apps.md); what a
script is written against is [`app-api.md`](app-api.md).

> **What this version has.** The store of installed apps, approval, and the
> command line `bzz app`. An app can be written, checked, tried, run from
> your own folder in a terminal, installed, approved, rolled back and
> removed. **It cannot yet be opened beside a conversation**: the pane, the
> agents' tools and sharing in the channel are not built, and each has a
> marked section below.

An app is four things, kept apart:

| Piece | Where it is | Rule |
|---|---|---|
| Its **script** | one Lua file, in bzz's data folder on your machine | it runs only after you approve its bytes, with only what bzz lends it |
| Its **brief** | a short text kept with the script | it says what the app does and what its records hold |
| Its **records** | artifacts of the channel, on the relay | bzz signs them; the script never sees a key |
| Its **pane** | a region of bzz's window | *not in this version, see [The pane](#the-pane)* |

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

| Template | What it is |
|---|---|
| `board` | Cards in three columns: To do, Doing, Done. |

<!-- Poll and Approvals (plan D9) come from the fixtures
     tests/fixtures/apps/poll/app.lua (types poll, vote) and
     tests/fixtures/apps/approvals/app.lua (type request). For each: a
     brief at src/apps/templates/<name>/brief.md, an entry in TEMPLATES in
     src/apps/store/templates.rs, and a row here. The test
     every_template_loads_and_declares_what_its_entry_says covers it. -->

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

  `--channel` is refused in this version: running your folder on a
  channel's real records needs the pane. <!-- M4-B: `bzz app dev <folder>
  --channel <name>` opens the folder's app in that channel's pane of the
  running TUI, on the channel's records, reloading on save and marked as
  unapproved in the pane's border. It needs: the pane (M3); an
  `AppSession::on_relay` started from the folder's bytes instead of from
  `Apps::runnable`, with the app's name from the folder's app.toml, given
  each save with `AppSession::reload` (`apps::dev::Dev` already does this
  on `AppSession::in_memory`); and a way for the command to reach the
  running bzz, or `:app dev <folder>` inside it. -->

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

<!-- M4-B: in the TUI a waiting version opens an approval overlay built on
     v0.15's (`src/ui/approval.rs`), from `Apps::review`. An approval made
     there or at the shell reaches the open app without a restart: the pane
     takes `Apps::runnable` again and reloads. A version that fails as the
     pane puts it to run is recorded with `Apps::failed_to_run`, and the
     version before it goes on running. -->

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

> **Not in this version.** An installed app cannot be opened beside a
> conversation yet.

<!-- M3: the pane to the right of the conversation, 42 columns with its
     border; `:apps`, `:app open <name>`, `:app close`; focus and keys
     (Ctrl+C leaves); zoom; narrow windows. Say here where an app is open
     (`bzz app show` already lists the channels: `Apps::open_in`,
     `Apps::close_in`, `Apps::open_in_channel` record them), and that the
     pane starts a script only from `Apps::runnable`. -->

## Agents

> **Not in this version.** Your agents cannot read, try or propose an app
> yet.

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
| `<name> is disabled` | `bzz app enable <name>`. |
| `the seed has records of type …, which the script does not declare` | The script will never see those records: check the seed's `type`. |
