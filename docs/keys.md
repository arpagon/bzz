# bzz keys and commands

Generated from bzz's table of actions (`src/ui/action_table.rs`) by `cargo run --example keys_doc > docs/keys.md`. Do not edit it by hand: a test fails when it is stale. In bzz, `?` lists the same keys with your own `keymap.toml` applied, `Ctrl-K` finds any action by name, and Tab completes a `:` command.

## Keys

### Message

| Key | Action | Also |
|---|---|---|
| `i` | Write a message |  |
| `r` | React to message |  |
| `e` | Edit my message |  |
| `d` (channels and messages) | Delete my message |  |
| `u` (channels and messages) | Mark this channel unread |  |
| `y` | Copy message |  |
| `v` | Select several messages |  |
| `Space p` | View author profile | `:profile [npub\|hex]` |

### Go

| Key | Action | Also |
|---|---|---|
| `Space Space` | Go to channel or DM |  |
| `/` | Search messages | `:search` |
| `Space n` | Open Inbox | `:inbox` |
| `Enter` | Open selected |  |
| `2` | Focus channels |  |
| `3` | Focus messages |  |
| `4` | Focus thread |  |
| `1` | Focus communities |  |
| `5` | Focus the app |  |
| `Tab` `l` | Next pane |  |
| `BackTab` `h` | Previous pane |  |
| `j` `Down` `Ctrl+N` | Next item |  |
| `k` `Up` `Ctrl+P` | Previous item |  |
| `g g` | Jump to first |  |
| `G` | Jump to last |  |
| `J` | Scroll down |  |
| `K` | Scroll up |  |
| `Ctrl+D` | Half page down |  |
| `Ctrl+U` | Half page up |  |
| `:` | Command line |  |
| `Space a` | Actions for the selected item |  |
| `Ctrl+K` `Space k` | Find any action |  |

### View

| Key | Action | Also |
|---|---|---|
| `Space o` | Change theme… | `:theme` |
| `?` | Show keyboard shortcuts | `:help` |
| `Space 1` | Show or hide communities |  |
| `Space 2` | Show or hide channels |  |
| `Space 4` | Show or hide thread pane |  |
| `Space 5` | Zoom the app | `:app zoom` |
| `Alt+H` | Narrow side pane |  |
| `Alt+L` | Widen side pane |  |
| `Space r` | Refresh |  |
| `Space s` | Cycle channel sort |  |

### Inbox

| Key | Action | Also |
|---|---|---|
| `f` (Inbox) | Cycle Inbox filter |  |
| `o` (Inbox) | Open source context |  |
| `m` (Inbox) | Mark read |  |
| `U` (Inbox) | Mark unread |  |
| `a` (Inbox) | Mark visible rows read |  |

### bzz

| Key | Action | Also |
|---|---|---|
| `q` `Esc` · `Esc` `q` `?` (any overlay) · `Esc` (writing) | Back or close |  |

### Writing

| Key | Action | Also |
|---|---|---|
| `Enter` (writing) | Send |  |
| `Ctrl+J` `Alt+Enter` (writing) | New line |  |
| `Tab` (writing) | Complete @mention or :emoji |  |
| `Ctrl+W` (writing) | Delete previous word |  |
| `Ctrl+U` (writing) | Delete to line start |  |
| `Ctrl+K` (writing) | Delete to line end |  |
| `Ctrl+C` (writing) | Clear draft |  |
| `Ctrl+V` (writing) | Paste text or attachments |  |
| `Ctrl+O` (writing) | Choose files |  |
| `Alt+O` (writing) | Attach by path |  |
| `Delete` (writing) | Remove last attachment |  |
| `Ctrl+R` (writing) | Retry failed attachments |  |
| `Ctrl+Left` (writing) | Previous word |  |
| `Ctrl+Right` (writing) | Next word |  |
| `Home` (writing) | Line start |  |
| `End` (writing) | Line end |  |

## Commands

Press `:`, type one, Enter runs it.

| Command | What it does |
|---|---|
| `:profile [npub \| hex]` | the profile of the selected author, or of a key |
| `:report [author] <reason> [note]` | report the selected message, or its author |
| `:tochannel` `:send-to-channel` | send the open thread's draft to its channel |
| `:search` | search messages |
| `:inbox` | open the Inbox |
| `:dm [add \| hide \| <npub>…]` | start a DM; add a person to this DM; hide it |
| `:theme [<name> \| reload]` | pick a theme; set one by name; reload the theme file |
| `:help` | the keys and these commands |
| `:sort alpha \| recent \| default` | how the selected channel's group is sorted |
| `:system` | show or hide the System channels |
| `:section new <name> \| rename <name> \| delete \| move <name> \| remove \| up \| down` | sidebar sections, for the selected channel |
| `:media reload` | detect the terminal's image support again |
| `:artifacts` | the artifacts of this channel or thread |
| `:artifact rename <title> \| delete \| restore \| move <#channel>` | change the selected artifact |
| `:tasks` `:my-tasks` | the tasks assigned to you |
| `:resync` | load this channel's history again |
| `:solo <name>` | create a Solo chat |
| `:huddle [start \| join \| leave \| end \| mute \| react <emoji> \| devices \| input <name> \| output <name>]` | the huddle of this channel |
| `:apps` | the installed apps |
| `:app open <name> \| close \| zoom \| share \| unshare \| pending \| approve \| refuse \| rollback \| enable \| disable \| remove \| add \| new \| dev \| brief` | the app of this channel, and managing apps |
| `:agents [local \| directory]` | your agents, or the directory of verified agents |
| `:agent new [name]` | create a local agent |
| `:broker` | the local agent broker |
| `:approvals` `:approve` `:approval` | what waits for your answer (also Ctrl-y) |
| `:status [[--expires N[smhdw]] <text> \| clear]` | show your status; set it; clear it |
| `:presence [on \| off]` | show or change whether bzz publishes presence |
| `:lock` | lock the identity until bzz restarts |
| `:signin` `:sign-in` | federated sign-in for this community |
| `:reconnect` | reconnect to the relay |
| `:purge-cache` | delete this community's local cache |
| `:community add \| remove` | done from the shell: bzz community add \| remove |
| `:quit` `:q` | quit bzz (asks first) |

## Only in the shell

The palette lists these and copies the command.

| Command | What it does |
|---|---|
| `bzz community add <url>` | Add a community |
| `bzz community remove <name>` | Remove a community |
| `bzz identity new` | Create an identity |
| `bzz identity import` | Import an identity |
| `bzz identity backup` | Back up identity |
| `bzz identity restore` | Restore identity |
| `bzz identity list` | List identities |
| `bzz identity pair-desktop` | Pair with Buzz desktop |
| `bzz identity remove` | Remove identity |
| `bzz community default <name>` | Default community |
| `bzz agent edit <name>` | Edit agent settings |
| `bzz agent delete <name>` | Delete agent |
| `bzz agent doctor` | Agent doctor |
| `bzz agent credential set <name>` | Git credentials for an agent |
| `bzz agent launch-kit <name>` | Agent launch kit |
| `bzz app show <name>` | Show app |
| `bzz app check <folder>` | Check app script |
| `bzz app try <folder>` | Try app headless |
| `bzz broker create <name>` | Create broker agent |
| `bzz broker delete <name>` | Delete broker agent |
| `bzz media status` | Media status |
| `bzz media clear` | Clear media cache |
| `bzz diagnostics report` | Diagnostics report |
| `bzz telemetry status` | Telemetry status |
| `bzz telemetry disable` | Telemetry disable |
| `bzz theme export <name>` | Export theme |
| `bzz theme path` | Theme path |
| `bzz check` | Check config and database |
| `bzz paths` | Print paths |
| `bzz completions <shell>` | Shell completions |
