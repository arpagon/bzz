//! The one static table of what a person can do in bzz (v0.17 C1).
//!
//! Each row names an action as a person would search for it and points at a
//! path that already exists: a typed [`UiAction`], a `:` command text, or a
//! shell command when only the shell can do it. Nothing here executes; the
//! palette reads it, and `App` runs the row through the same code a key or a
//! typed `:` command reaches. The key shown beside a row is read from the
//! live key map, never written here.
//!
//! Rule 23 of the v0.17 plan: one place says what an action is. Wave 2 (C8)
//! grows this table into the registry that generates the key map, the help
//! and the leader panel; arguments as steps (C9) hang off [`Run::Prefill`].

use crate::ui::keymap::UiAction;

/// Where a row can run. The Inbox route reduces only its own small set of
/// actions (`action::reduce_inbox`); everything else waits for the
/// workspace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Where {
    /// Any route: a `:` command, or an action both reducers take.
    Anywhere,
    /// The workspace (channels, messages, threads).
    Workspace,
    /// The Inbox route.
    Inbox,
    /// Only while writing: the composer's editing keys. Not listed in the
    /// palette (it does not open over the composer); the help lists them.
    Writing,
}

/// What must be true before a row can run, beyond what
/// `actions::derive` already decides for the selected item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Needs {
    Nothing,
    /// A channel is selected.
    Channel,
    /// A message is selected (the palette moves the focus to the timeline
    /// before it runs a message action from another pane).
    Message,
    /// The identity is unlocked and a community connected.
    Connected,
    /// A thread is open.
    Thread,
    /// The selected channel is a DM.
    Dm,
    /// A huddle runs in the selected channel.
    Huddle,
    /// I am in the huddle's call.
    InCall,
    /// An app is open beside the selected channel.
    AppHere,
    /// An artifact is selected in the `:artifacts` panel.
    Artifact,
    /// Two communities or more are set up.
    Communities,
}

impl Needs {
    /// The reason shown when the need is not met. Where `actions::derive`
    /// has a reason for the same action, the palette shows that one instead.
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Nothing => "",
            Self::Channel => "select a channel first",
            Self::Message => "select a message first",
            Self::Connected => "identity is locked or unavailable",
            Self::Thread => "open a thread first (Enter on a message)",
            Self::Dm => "open a DM first",
            Self::Huddle => "no huddle is running here",
            Self::InCall => "you are not in a huddle call",
            Self::AppHere => "no app is open here",
            Self::Artifact => "open :artifacts and select an artifact first",
            Self::Communities => "only one community is set up",
        }
    }
}

/// The palette's groups, in the order the help will list them (C3, C8).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Group {
    Message,
    Go,
    View,
    Inbox,
    Channel,
    Huddle,
    Apps,
    Agents,
    Me,
    Community,
    Bzz,
    Writing,
    Shell,
}

impl Group {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Message => "Message",
            Self::Go => "Go",
            Self::View => "View",
            Self::Inbox => "Inbox",
            Self::Channel => "Channel",
            Self::Huddle => "Huddle",
            Self::Apps => "Apps",
            Self::Agents => "Agents",
            Self::Me => "Me",
            Self::Community => "Community",
            Self::Bzz => "bzz",
            Self::Writing => "Writing",
            Self::Shell => "Shell",
        }
    }
}

/// How a row runs. Each points at a path that exists in v0.16.0.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Run {
    /// Dispatched as the key would be (`App::dispatch_route_action`).
    Ui(UiAction),
    /// A complete `:` command, run at once (`App::execute_command`).
    Command(&'static str),
    /// A `:` command that needs an argument, or one that changes something
    /// without asking: the `:` line opens filled in with it, the cursor at
    /// its end, as the artifacts panel does for `r` and `m`.
    Prefill(&'static str),
    /// The quit question, as `q` asks it with nothing open.
    Quit,
    /// Only the shell can do this: the command a person copies.
    Shell(&'static str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionSpec {
    /// Stable id: "msg.react". Kept for keymap.toml and docs (C8).
    pub id: &'static str,
    /// What a person would search for: "React to message".
    pub title: &'static str,
    /// Other words that find it. An exact match of one of `aliases` wins
    /// outright ("q" finds "Quit bzz").
    pub words: &'static str,
    pub aliases: &'static [&'static str],
    pub group: Group,
    pub run: Run,
    /// The `:` form a person can type, with its usage, when there is one.
    pub command: Option<&'static str>,
    /// The action whose key is shown beside this row, when it is not the
    /// one the row runs ("Open thread" shows Enter, the key that opens a
    /// thread on a message).
    pub key_of: Option<UiAction>,
    pub needs: Needs,
    pub place: Where,
}

impl ActionSpec {
    const fn new(id: &'static str, title: &'static str, group: Group, run: Run) -> Self {
        let place = match run {
            Run::Ui(_) => Where::Workspace,
            _ => Where::Anywhere,
        };
        Self {
            id,
            title,
            words: "",
            aliases: &[],
            group,
            run,
            command: None,
            key_of: None,
            needs: Needs::Nothing,
            place,
        }
    }

    const fn words(mut self, words: &'static str) -> Self {
        self.words = words;
        self
    }

    const fn aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.aliases = aliases;
        self
    }

    const fn command(mut self, command: &'static str) -> Self {
        self.command = Some(command);
        self
    }

    const fn key_of(mut self, action: UiAction) -> Self {
        self.key_of = Some(action);
        self
    }

    const fn needs(mut self, needs: Needs) -> Self {
        self.needs = needs;
        self
    }

    const fn place(mut self, place: Where) -> Self {
        self.place = place;
        self
    }

    /// The action whose key the palette shows beside the row.
    pub const fn key_action(&self) -> Option<UiAction> {
        match (self.key_of, self.run) {
            (Some(action), _) | (None, Run::Ui(action)) => Some(action),
            _ => None,
        }
    }

    /// Listed in the palette: everything but the composer's editing keys.
    pub const fn listed(&self) -> bool {
        !matches!(self.place, Where::Writing)
    }

    /// The `:` text shown dim beside the title.
    pub fn command_hint(&self) -> Option<&'static str> {
        self.command
    }
}

use Group as G;
use Needs as N;
use Run::{Command as Cmd, Prefill, Shell, Ui};
use UiAction as A;

const fn ui(id: &'static str, title: &'static str, group: Group, action: UiAction) -> ActionSpec {
    ActionSpec::new(id, title, group, Ui(action))
}

const fn cmd(
    id: &'static str,
    title: &'static str,
    group: Group,
    text: &'static str,
) -> ActionSpec {
    ActionSpec::new(id, title, group, Cmd(text))
}

const fn fill(
    id: &'static str,
    title: &'static str,
    group: Group,
    text: &'static str,
) -> ActionSpec {
    ActionSpec::new(id, title, group, Prefill(text))
}

const fn shell(id: &'static str, title: &'static str, text: &'static str) -> ActionSpec {
    ActionSpec::new(id, title, G::Shell, Shell(text))
}

const fn writing(id: &'static str, title: &'static str, action: UiAction) -> ActionSpec {
    ui(id, title, G::Writing, action).place(Where::Writing)
}

/// Every interface action of v0.16.0 (`actions.tsv` of the R2 research),
/// and the shell commands worth knowing. At equal score the palette keeps
/// this order, so the commoner action of two similar ones comes first.
pub static TABLE: &[ActionSpec] = &[
    // Message: what the selected message allows (`actions::derive`).
    ui("msg.thread", "Open thread", G::Message, A::ToggleThread)
        .words("reply in thread conversation context close thread")
        .key_of(A::ActivateFocused)
        .needs(N::Message),
    ui("msg.compose", "Write a message", G::Message, A::Compose)
        .words("compose reply send post new message")
        .needs(N::Channel),
    ui("msg.react", "React to message", G::Message, A::React)
        .words("emoji reaction")
        .aliases(&["react"])
        .needs(N::Message),
    ui("msg.edit", "Edit my message", G::Message, A::Edit)
        .words("change fix")
        .aliases(&["edit"])
        .needs(N::Message),
    ui("msg.delete", "Delete my message", G::Message, A::Delete)
        .words("remove")
        .aliases(&["delete"])
        .needs(N::Message),
    ui(
        "msg.mark_unread",
        "Mark unread from here",
        G::Message,
        A::MarkUnread,
    )
    .words("unread new")
    .needs(N::Message),
    ui("msg.copy", "Copy message", G::Message, A::CopyMessages)
        .words("clipboard yank copy messages")
        .aliases(&["copy"])
        .needs(N::Message),
    ui(
        "msg.select_range",
        "Select several messages",
        G::Message,
        A::ToggleCopySelection,
    )
    .words("range visual copy selection")
    .needs(N::Message),
    ui("msg.preview_media", "Preview media", G::Message, A::Preview)
        .words("image picture attachment file")
        .needs(N::Message),
    ui(
        "msg.profile",
        "View author profile",
        G::Message,
        A::ViewProfile,
    )
    .words("person who user")
    .command(":profile [npub|hex]")
    .needs(N::Message),
    fill(
        "msg.report",
        "Report message or author",
        G::Message,
        "report ",
    )
    .words("moderators spam abuse")
    .command(":report [author] <reason> [note]")
    .needs(N::Message),
    cmd(
        "msg.to_channel",
        "Send thread draft to channel",
        G::Message,
        "tochannel",
    )
    .words("also send to channel broadcast")
    .command(":tochannel")
    .needs(N::Thread),
    // Go: finding and moving.
    ui(
        "finder.open",
        "Go to channel or DM",
        G::Go,
        A::ChannelSwitcher,
    )
    .words("find switch jump open channel")
    .aliases(&["go"]),
    ui("search.open", "Search messages", G::Go, A::Search)
        .words("find text from in after before")
        .command(":search")
        .key_of(A::Filter)
        .aliases(&["search"]),
    ui("inbox.open", "Open Inbox", G::Go, A::OpenInbox)
        .words("mentions personal work notifications")
        .command(":inbox")
        .aliases(&["inbox"]),
    ui("dm.new", "Start a DM", G::Go, A::NewDm)
        .words("direct message person chat private")
        .command(":dm")
        .aliases(&["dm"])
        .needs(N::Connected),
    ui("item.open", "Open selected", G::Go, A::ActivateFocused)
        .words("channel thread community enter"),
    ui("focus.channels", "Focus channels", G::Go, A::FocusChannels).words("sidebar pane"),
    ui("focus.timeline", "Focus messages", G::Go, A::FocusTimeline).words("timeline pane"),
    ui("focus.context", "Focus thread", G::Go, A::FocusContext).words("context pane"),
    ui(
        "focus.communities",
        "Focus communities",
        G::Go,
        A::FocusCommunities,
    )
    .words("rail pane"),
    ui("focus.app", "Focus the app", G::Go, A::FocusApp)
        .words("app pane")
        .needs(N::AppHere),
    ui(
        "community.switch",
        "Switch community",
        G::Go,
        A::FocusCommunities,
    )
    .words("change community rail")
    .needs(N::Communities),
    ui("focus.next", "Next pane", G::Go, A::NextFocus)
        .words("focus cycle")
        .place(Where::Anywhere),
    ui("focus.prev", "Previous pane", G::Go, A::PreviousFocus)
        .words("focus cycle back")
        .place(Where::Anywhere),
    ui("sel.next", "Next item", G::Go, A::SelectNext)
        .words("down move")
        .place(Where::Anywhere),
    ui("sel.prev", "Previous item", G::Go, A::SelectPrevious)
        .words("up move")
        .place(Where::Anywhere),
    ui("sel.first", "Jump to first", G::Go, A::JumpTop)
        .words("top start oldest")
        .place(Where::Anywhere),
    ui("sel.last", "Jump to last", G::Go, A::JumpBottom)
        .words("bottom end newest latest")
        .place(Where::Anywhere),
    ui(
        "view.scroll_down",
        "Scroll down",
        G::Go,
        A::ScrollViewportDown,
    )
    .place(Where::Anywhere),
    ui("view.scroll_up", "Scroll up", G::Go, A::ScrollViewportUp).place(Where::Anywhere),
    ui("view.half_down", "Half page down", G::Go, A::HalfPageDown)
        .words("scroll page")
        .place(Where::Anywhere),
    ui("view.half_up", "Half page up", G::Go, A::HalfPageUp)
        .words("scroll page")
        .place(Where::Anywhere),
    ui("cmd.open", "Command line", G::Go, A::OpenCommand).words("colon type a command"),
    ui(
        "actions.menu",
        "Actions for the selected item",
        G::Go,
        A::OpenContextActions,
    )
    .words("context menu")
    .place(Where::Anywhere),
    ui("palette.open", "Find any action", G::Go, A::OpenPalette)
        .words("palette command commands")
        .place(Where::Anywhere),
    // View: panes, themes, help.
    ui("theme.pick", "Change theme…", G::View, A::OpenOptions)
        .words("colors colours options appearance dark light")
        .aliases(&["theme", "themes"])
        .command(":theme"),
    ui(
        "help.keymap",
        "Show keyboard shortcuts",
        G::View,
        A::OpenHelp,
    )
    .words("help keys keymap bindings")
    .aliases(&["help", "keys", "?"])
    .command(":help")
    .place(Where::Anywhere),
    ui(
        "view.toggle_communities",
        "Show or hide communities",
        G::View,
        A::ToggleCommunities,
    )
    .words("toggle rail pane"),
    ui(
        "view.toggle_channels",
        "Show or hide channels",
        G::View,
        A::ToggleChannels,
    )
    .words("toggle sidebar pane"),
    ui(
        "view.toggle_context",
        "Show or hide thread pane",
        G::View,
        A::ToggleContext,
    )
    .words("toggle context pane"),
    ui("app.zoom", "Zoom the app", G::View, A::ZoomApp)
        .words("app full screen")
        .command(":app zoom")
        .needs(N::AppHere),
    ui(
        "pane.narrow",
        "Narrow side pane",
        G::View,
        A::ResizeFocusedNarrow,
    )
    .words("resize width smaller"),
    ui(
        "pane.widen",
        "Widen side pane",
        G::View,
        A::ResizeFocusedWide,
    )
    .words("resize width larger"),
    ui("sync.refresh", "Refresh", G::View, A::Refresh)
        .words("reload sync update")
        .place(Where::Anywhere),
    ui(
        "sidebar.sort_cycle",
        "Cycle channel sort",
        G::View,
        A::CycleChannelSort,
    )
    .words("order sidebar alphabetical recent"),
    fill("sidebar.sort", "Set channel group sort", G::View, "sort ")
        .words("order sidebar alpha recent default")
        .command(":sort alpha|recent|default")
        .needs(N::Channel),
    cmd(
        "sidebar.system",
        "Show or hide System channels",
        G::View,
        "system",
    )
    .words("toggle sidebar access")
    .command(":system"),
    fill(
        "section.new",
        "New sidebar section",
        G::View,
        "section new ",
    )
    .words("group folder sidebar")
    .command(":section new <name>")
    .needs(N::Channel),
    fill(
        "section.rename",
        "Rename sidebar section",
        G::View,
        "section rename ",
    )
    .words("group folder sidebar")
    .command(":section rename <name>")
    .needs(N::Channel),
    fill(
        "section.delete",
        "Delete sidebar section",
        G::View,
        "section delete",
    )
    .words("group folder sidebar remove")
    .command(":section delete")
    .needs(N::Channel),
    fill(
        "section.move",
        "Move channel to a section",
        G::View,
        "section move ",
    )
    .words("group folder sidebar")
    .command(":section move <name>")
    .needs(N::Channel),
    cmd(
        "section.remove",
        "Take channel out of its section",
        G::View,
        "section remove",
    )
    .words("group folder sidebar")
    .command(":section remove")
    .needs(N::Channel),
    cmd("section.up", "Move section up", G::View, "section up")
        .words("group folder sidebar order")
        .command(":section up")
        .needs(N::Channel),
    cmd("section.down", "Move section down", G::View, "section down")
        .words("group folder sidebar order")
        .command(":section down")
        .needs(N::Channel),
    cmd("theme.reload", "Reload theme file", G::View, "theme reload")
        .words("colors refresh")
        .command(":theme reload"),
    cmd(
        "media.reload",
        "Reload media renderer",
        G::View,
        "media reload",
    )
    .words("images pictures graphics")
    .command(":media reload"),
    // Inbox: its own route and keys.
    ui("inbox.reply", "Reply in place", G::Inbox, A::Compose)
        .words("inbox answer write")
        .place(Where::Inbox),
    ui("inbox.filter", "Cycle Inbox filter", G::Inbox, A::Filter)
        .words("inbox mentions unread show")
        .place(Where::Inbox),
    ui(
        "inbox.open_source",
        "Open source context",
        G::Inbox,
        A::OpenCanonicalContext,
    )
    .words("inbox go to channel thread")
    .place(Where::Inbox),
    ui("inbox.mark_read", "Mark read", G::Inbox, A::MarkRead)
        .words("inbox done")
        .place(Where::Inbox),
    ui("inbox.mark_unread", "Mark unread", G::Inbox, A::MarkUnread)
        .words("inbox")
        .place(Where::Inbox),
    ui(
        "inbox.mark_visible_read",
        "Mark visible rows read",
        G::Inbox,
        A::MarkVisibleRead,
    )
    .words("inbox all done")
    .place(Where::Inbox),
    // Channel: what lives in a channel.
    cmd(
        "artifacts.panel",
        "Artifacts of channel or thread",
        G::Channel,
        "artifacts",
    )
    .words("tasks files documents records")
    .command(":artifacts")
    .needs(N::Channel),
    fill(
        "artifact.rename",
        "Rename artifact",
        G::Channel,
        "artifact rename ",
    )
    .command(":artifact rename <title>")
    .needs(N::Artifact),
    fill(
        "artifact.delete",
        "Delete artifact",
        G::Channel,
        "artifact delete",
    )
    .command(":artifact delete")
    .needs(N::Artifact),
    cmd(
        "artifact.restore",
        "Restore artifact",
        G::Channel,
        "artifact restore",
    )
    .command(":artifact restore")
    .needs(N::Artifact),
    fill(
        "artifact.move",
        "Move artifact to a channel",
        G::Channel,
        "artifact move #",
    )
    .command(":artifact move <#channel>")
    .needs(N::Artifact),
    cmd("tasks.panel", "My tasks", G::Channel, "tasks")
        .words("assigned todo artifacts")
        .command(":tasks"),
    cmd("net.resync", "Resync channel history", G::Channel, "resync")
        .words("reload history messages")
        .command(":resync")
        .needs(N::Channel),
    fill("dm.hide", "Hide this DM", G::Channel, "dm hide")
        .words("direct message close")
        .command(":dm hide")
        .needs(N::Dm),
    cmd("dm.add", "Add a person to this DM", G::Channel, "dm add")
        .words("direct message invite group")
        .command(":dm add")
        .needs(N::Dm),
    fill("solo.new", "New Solo chat", G::Channel, "solo ")
        .words("notes private channel")
        .command(":solo <name>")
        .needs(N::Connected),
    // Huddle: voice in a channel.
    cmd("huddle.panel", "Huddle panel", G::Huddle, "huddle")
        .words("call voice audio")
        .command(":huddle")
        .aliases(&["huddle"])
        .needs(N::Channel),
    cmd("huddle.start", "Start huddle", G::Huddle, "huddle start")
        .words("call voice audio")
        .command(":huddle start")
        .needs(N::Channel),
    cmd("huddle.join", "Join huddle", G::Huddle, "huddle join")
        .words("call voice audio")
        .command(":huddle join")
        .needs(N::Huddle),
    cmd("huddle.mute", "Mute or unmute", G::Huddle, "huddle mute")
        .words("huddle microphone call")
        .command(":huddle mute")
        .needs(N::InCall),
    cmd("huddle.leave", "Leave huddle", G::Huddle, "huddle leave")
        .words("call hang up")
        .command(":huddle leave")
        .needs(N::InCall),
    cmd(
        "huddle.end",
        "End huddle for everyone",
        G::Huddle,
        "huddle end",
    )
    .words("call stop")
    .command(":huddle end")
    .needs(N::Huddle),
    fill(
        "huddle.react",
        "React in huddle",
        G::Huddle,
        "huddle react ",
    )
    .words("call emoji")
    .command(":huddle react <emoji>")
    .needs(N::Huddle),
    cmd(
        "huddle.devices",
        "Audio devices",
        G::Huddle,
        "huddle devices",
    )
    .words("huddle microphone speaker sound")
    .command(":huddle devices"),
    fill(
        "huddle.input",
        "Choose microphone",
        G::Huddle,
        "huddle input ",
    )
    .words("huddle audio device input")
    .command(":huddle input <name>"),
    fill(
        "huddle.output",
        "Choose speaker",
        G::Huddle,
        "huddle output ",
    )
    .words("huddle audio device output headphones")
    .command(":huddle output <name>"),
    // Apps: v0.16's channel apps.
    cmd("apps.panel", "Installed apps", G::Apps, "apps")
        .words("app list")
        .command(":apps")
        .aliases(&["apps"]),
    fill("app.open", "Open app here", G::Apps, "app open ")
        .words("board app run")
        .command(":app open <name>")
        .needs(N::Channel),
    cmd("app.close", "Close app here", G::Apps, "app close")
        .command(":app close")
        .needs(N::AppHere),
    fill("app.share", "Share app", G::Apps, "app share ")
        .words("app channel")
        .command(":app share [name]"),
    fill("app.unshare", "Stop sharing app", G::Apps, "app unshare ")
        .words("app channel")
        .command(":app unshare [name]"),
    cmd(
        "app.pending",
        "Review waiting app version",
        G::Apps,
        "app pending",
    )
    .words("approve update version")
    .command(":app pending [name]"),
    fill(
        "app.approve",
        "Approve app version",
        G::Apps,
        "app approve ",
    )
    .words("update version")
    .command(":app approve <name>"),
    fill("app.refuse", "Refuse app version", G::Apps, "app refuse ")
        .words("reject update version")
        .command(":app refuse <name> [reason]"),
    fill("app.rollback", "Roll back app", G::Apps, "app rollback ")
        .words("previous version undo")
        .command(":app rollback <name> [version]"),
    fill("app.enable", "Enable app", G::Apps, "app enable ").command(":app enable <name>"),
    fill("app.disable", "Disable app", G::Apps, "app disable ").command(":app disable <name>"),
    fill("app.remove", "Remove app", G::Apps, "app remove ")
        .words("delete uninstall")
        .command(":app remove <name>"),
    fill("app.add", "Propose app from folder", G::Apps, "app add ")
        .words("install")
        .command(":app add <folder>"),
    fill("app.new", "New app from template", G::Apps, "app new ")
        .words("create")
        .command(":app new <name> [template]"),
    fill("app.dev", "Dev-run app folder", G::Apps, "app dev ")
        .words("develop test")
        .command(":app dev <folder>")
        .needs(N::Channel),
    cmd("app.brief", "Post app brief", G::Apps, "app brief")
        .words("describe agent")
        .command(":app brief [name]"),
    // Agents.
    cmd("agents.panel", "My agents", G::Agents, "agents")
        .words("local agents bots")
        .command(":agents [local|directory]")
        .aliases(&["agents"]),
    cmd(
        "agents.directory",
        "Agent directory",
        G::Agents,
        "agents directory",
    )
    .words("verified remote agents bots")
    .command(":agents directory"),
    cmd("agent.new", "Create a local agent", G::Agents, "agent new")
        .words("new bot add agent")
        .command(":agent new [name]"),
    cmd("broker.panel", "Agent broker", G::Agents, "broker")
        .words("hosted agents")
        .command(":broker"),
    cmd("approval.open", "Answer what waits", G::Agents, "approvals")
        .words("approve approval allow refuse agent command app version ctrl-y")
        .command(":approvals"),
    // Me.
    fill("status.set", "Set my status…", G::Me, "status ")
        .words("away lunch busy presence")
        .command(":status [--expires N[smhdw]] <text>")
        .aliases(&["status"])
        .needs(N::Connected),
    cmd("status.clear", "Clear my status", G::Me, "status clear")
        .command(":status clear")
        .needs(N::Connected),
    fill("presence.toggle", "Presence on or off", G::Me, "presence ")
        .words("online away visible")
        .command(":presence [on|off]"),
    cmd("id.lock", "Lock identity", G::Me, "lock")
        .words("key secure away")
        .command(":lock")
        .aliases(&["lock"]),
    cmd("auth.signin", "Federated sign-in", G::Me, "signin")
        .words("sign in login browser")
        .command(":signin"),
    // Community.
    cmd("net.reconnect", "Reconnect", G::Community, "reconnect")
        .words("network relay online")
        .command(":reconnect"),
    fill(
        "cache.purge",
        "Purge cache of this community",
        G::Community,
        "purge-cache",
    )
    .words("clear delete local data")
    .command(":purge-cache"),
    shell(
        "community.add",
        "Add a community",
        "bzz community add <url>",
    )
    .words("join new relay")
    .command(":community add"),
    shell(
        "community.remove",
        "Remove a community",
        "bzz community remove <name>",
    )
    .words("leave delete")
    .command(":community remove"),
    // bzz.
    ActionSpec::new("app.quit", "Quit bzz", G::Bzz, Run::Quit)
        .words("exit close leave")
        .aliases(&["q", "quit", "exit"])
        .command(":quit")
        .key_of(A::BackOrQuit),
    ui("nav.back", "Back or close", G::Bzz, A::BackOrQuit)
        .words("escape")
        .place(Where::Anywhere),
    // Writing: the composer's own keys (not listed in the palette).
    writing("composer.send", "Send", A::Submit),
    writing("composer.newline", "New line", A::InsertNewline),
    writing(
        "composer.complete",
        "Complete @mention or :emoji",
        A::Complete,
    ),
    writing(
        "composer.del_word",
        "Delete previous word",
        A::DeletePreviousWord,
    ),
    writing(
        "composer.del_start",
        "Delete to line start",
        A::DeleteToStart,
    ),
    writing("composer.del_end", "Delete to line end", A::DeleteToEnd),
    writing("composer.clear", "Clear draft", A::ClearComposer),
    writing(
        "composer.paste",
        "Paste text or attachments",
        A::PasteClipboard,
    ),
    writing("composer.attach", "Choose files", A::AttachFile),
    writing("composer.attach_path", "Attach by path", A::AttachPath),
    writing(
        "composer.remove_attachment",
        "Remove last attachment",
        A::RemoveLastAttachment,
    ),
    writing(
        "composer.retry",
        "Retry failed attachments",
        A::RetryAttachments,
    ),
    writing("composer.word_left", "Previous word", A::MoveWordLeft),
    writing("composer.word_right", "Next word", A::MoveWordRight),
    writing("composer.home", "Line start", A::MoveLineStart),
    writing("composer.end", "Line end", A::MoveLineEnd),
    // Shell: what only the shell does today, worth knowing.
    shell("cli.identity.new", "Create an identity", "bzz identity new").words("key account nsec"),
    shell(
        "cli.identity.import",
        "Import an identity",
        "bzz identity import",
    )
    .words("key nsec"),
    shell(
        "cli.identity.backup",
        "Back up identity",
        "bzz identity backup",
    )
    .words("key export save"),
    shell(
        "cli.identity.restore",
        "Restore identity",
        "bzz identity restore",
    )
    .words("key backup"),
    shell("cli.identity.list", "List identities", "bzz identity list").words("keys accounts"),
    shell(
        "cli.identity.pair_desktop",
        "Pair with Buzz desktop",
        "bzz identity pair-desktop",
    )
    .words("signer link"),
    shell(
        "cli.identity.remove",
        "Remove identity",
        "bzz identity remove",
    )
    .words("delete key"),
    shell(
        "cli.community.default",
        "Default community",
        "bzz community default <name>",
    )
    .words("start"),
    shell(
        "cli.agent.edit",
        "Edit agent settings",
        "bzz agent edit <name>",
    ),
    shell(
        "cli.agent.delete",
        "Delete agent",
        "bzz agent delete <name>",
    )
    .words("remove"),
    shell("cli.agent.doctor", "Agent doctor", "bzz agent doctor").words("check diagnose"),
    shell(
        "cli.agent.credential",
        "Git credentials for an agent",
        "bzz agent credential set <name>",
    )
    .words("token password"),
    shell(
        "cli.agent.launch_kit",
        "Agent launch kit",
        "bzz agent launch-kit <name>",
    )
    .words("move remote"),
    shell("cli.app.show", "Show app", "bzz app show <name>").words("versions script"),
    shell(
        "cli.app.check",
        "Check app script",
        "bzz app check <folder>",
    )
    .words("lint lua"),
    shell("cli.app.try", "Try app headless", "bzz app try <folder>").words("test"),
    shell(
        "cli.broker.create",
        "Create broker agent",
        "bzz broker create <name>",
    ),
    shell(
        "cli.broker.delete",
        "Delete broker agent",
        "bzz broker delete <name>",
    ),
    shell("cli.media.status", "Media status", "bzz media status").words("images cache"),
    shell("cli.media.clear", "Clear media cache", "bzz media clear").words("images delete"),
    shell(
        "cli.diag.report",
        "Diagnostics report",
        "bzz diagnostics report",
    )
    .words("bug logs debug"),
    shell(
        "cli.telemetry.status",
        "Telemetry status",
        "bzz telemetry status",
    ),
    shell(
        "cli.telemetry.disable",
        "Telemetry disable",
        "bzz telemetry disable",
    )
    .words("off privacy"),
    shell(
        "cli.theme.export",
        "Export theme",
        "bzz theme export <name>",
    )
    .words("colors"),
    shell("cli.theme.path", "Theme path", "bzz theme path").words("colors file"),
    shell("cli.check", "Check config and database", "bzz check").words("doctor"),
    shell("cli.paths", "Print paths", "bzz paths").words("folders files config"),
    shell(
        "cli.completions",
        "Shell completions",
        "bzz completions <shell>",
    )
    .words("bash zsh fish"),
];

/// The row of `id`, if it exists.
pub fn by_id(id: &str) -> Option<(usize, &'static ActionSpec)> {
    TABLE.iter().enumerate().find(|(_, spec)| spec.id == id)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{Run, TABLE, Where};
    use crate::ui::{
        command::{Command, parse},
        keymap::UiAction,
    };

    // Each macro writes an exhaustive `match`: a variant added to the enum
    // and not named here stops this test from compiling, and a variant
    // named here that the table does not mention fails it.
    macro_rules! all_ui_actions {
        ($($name:ident),* $(,)?) => {
            const ALL_UI_ACTIONS: &[UiAction] = &[$(UiAction::$name),*];
            #[allow(dead_code)]
            const fn named(action: UiAction) {
                match action {
                    $(UiAction::$name => {}),*
                }
            }
        };
    }

    all_ui_actions!(
        BackOrQuit,
        OpenHelp,
        ToggleCommunities,
        ToggleChannels,
        ToggleContext,
        FocusCommunities,
        FocusChannels,
        FocusTimeline,
        FocusContext,
        FocusApp,
        ZoomApp,
        NextFocus,
        PreviousFocus,
        ResizeFocusedNarrow,
        ResizeFocusedWide,
        SelectPrevious,
        SelectNext,
        ScrollViewportUp,
        ScrollViewportDown,
        HalfPageUp,
        HalfPageDown,
        JumpTop,
        JumpBottom,
        ActivateFocused,
        Compose,
        Filter,
        Search,
        OpenInbox,
        CycleChannelSort,
        ChannelSwitcher,
        OpenContextActions,
        Refresh,
        OpenOptions,
        OpenCommand,
        NewDm,
        ToggleThread,
        ToggleCopySelection,
        CopyMessages,
        React,
        Delete,
        Edit,
        MarkUnread,
        ViewProfile,
        MarkRead,
        MarkVisibleRead,
        OpenCanonicalContext,
        Preview,
        Submit,
        InsertNewline,
        Complete,
        DeletePreviousWord,
        DeleteToStart,
        DeleteToEnd,
        ClearComposer,
        PasteClipboard,
        AttachFile,
        AttachPath,
        RemoveLastAttachment,
        RetryAttachments,
        MoveWordLeft,
        MoveWordRight,
        MoveLineStart,
        MoveLineEnd,
        OpenPalette,
    );

    macro_rules! all_commands {
        ($($name:ident $(($pattern:pat))?),* $(,)?) => {
            const ALL_COMMANDS: &[&str] = &[$(stringify!($name)),*];
            fn command_kind(command: &Command) -> &'static str {
                match command {
                    $(Command::$name $(($pattern))? => stringify!($name)),*
                }
            }
        };
    }

    all_commands!(
        Lock,
        Reconnect,
        Resync,
        ThemeReload,
        MediaReload,
        Inbox,
        Search,
        Dm,
        DmHide,
        DmAdd,
        Agents(_),
        Agent(_),
        Approvals,
        AddCommunity,
        RemoveCommunity,
        PurgeCache,
        System,
        SignIn,
        ToChannel,
        Report(_),
        Status(_),
        Artifacts,
        Tasks,
        Broker,
        Huddle(_),
        Presence(_),
        Profile(_),
        Artifact(_),
        Section(_),
        Sort(_),
        Solo(_),
        Apps,
        App(_),
        Quit,
        Help,
        Theme(_),
        Unknown(_),
    );

    /// The `:` texts the table runs or shows, without their usage.
    fn command_texts() -> Vec<String> {
        let mut texts = Vec::new();
        for spec in TABLE {
            if let Run::Command(text) | Run::Prefill(text) = spec.run {
                texts.push(text.to_owned());
            }
            if let Some(command) = spec.command {
                // ":status [--expires …] <text>" → "status"; the usage's
                // optional and required parts are not part of the name.
                let name = command
                    .trim_start_matches(':')
                    .split(' ')
                    .take_while(|word| !word.starts_with(['[', '<']))
                    .collect::<Vec<_>>()
                    .join(" ");
                texts.push(name);
            }
        }
        texts
    }

    #[test]
    fn every_ui_action_is_in_the_table() {
        let mentioned = TABLE
            .iter()
            .flat_map(|spec| {
                let run = match spec.run {
                    Run::Ui(action) => Some(action),
                    _ => None,
                };
                [run, spec.key_of]
            })
            .flatten()
            .collect::<Vec<_>>();
        for action in ALL_UI_ACTIONS {
            assert!(
                mentioned.contains(action),
                "{action:?} is in no row of src/ui/action_table.rs"
            );
        }
    }

    #[test]
    fn every_command_is_in_the_table() {
        let kinds = command_texts()
            .iter()
            .map(|text| command_kind(&parse(text)))
            .collect::<HashSet<_>>();
        for kind in ALL_COMMANDS {
            if *kind == "Unknown" {
                continue;
            }
            assert!(
                kinds.contains(kind),
                "Command::{kind} is in no row of src/ui/action_table.rs"
            );
        }
    }

    #[test]
    fn every_command_text_is_one_bzz_knows() {
        for text in command_texts() {
            assert!(
                !matches!(parse(&text), Command::Unknown(_)),
                ":{text} is not a command bzz parses"
            );
        }
    }

    #[test]
    fn the_verbs_of_huddle_and_app_each_have_a_row() {
        let texts = command_texts();
        for verb in [
            "huddle start",
            "huddle join",
            "huddle leave",
            "huddle end",
            "huddle mute",
            "huddle react",
            "huddle devices",
            "huddle input",
            "huddle output",
            "app open",
            "app close",
            "app zoom",
            "app share",
            "app unshare",
            "app pending",
            "app approve",
            "app refuse",
            "app rollback",
            "app enable",
            "app disable",
            "app remove",
            "app add",
            "app new",
            "app dev",
            "app brief",
            "artifact rename",
            "artifact delete",
            "artifact restore",
            "artifact move",
            "section new",
            "section rename",
            "section delete",
            "section move",
            "section remove",
            "section up",
            "section down",
        ] {
            assert!(
                texts.iter().any(|text| text.trim() == verb
                    || text.starts_with(&format!("{verb} "))
                    || text.starts_with(&format!("{verb} #"))),
                ":{verb} has no row"
            );
        }
    }

    #[test]
    fn ids_are_unique_and_rows_well_formed() {
        let mut ids = HashSet::new();
        for spec in TABLE {
            assert!(ids.insert(spec.id), "{} twice", spec.id);
            assert!(!spec.title.is_empty());
            if let Run::Shell(text) = spec.run {
                assert!(text.starts_with("bzz "), "{}", spec.id);
            }
            if spec.place == Where::Writing {
                assert!(!spec.listed());
            }
            for alias in spec.aliases {
                assert_eq!(*alias, alias.to_lowercase(), "{}", spec.id);
            }
        }
    }
}
