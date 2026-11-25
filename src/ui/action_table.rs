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
//! and the leader panel. A row that needs something more says what, in
//! order, with [`Run::Steps`] (C9): the palette asks for each [`Arg`] in a
//! step of its own and runs the `:` command they build.

use crate::ui::{
    command::CommandSpec,
    keymap::{KeyScope, UiAction},
};

/// One default key sequence of a row, in the scope that owns it, as
/// `keymap.toml` writes keys: `(GLOBAL, &["space", "n"])`.
pub type DefaultKey = (KeyScope, &'static [&'static str]);

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
    /// Asks for each argument in a step of the palette, in order, then runs
    /// what they build (v0.17 C9).
    Steps(Then, &'static [Arg]),
}

/// What a row with steps runs once the last step is answered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Then {
    /// The `:` command whose first words these are, the answers after them
    /// ([`command_text`]); run as if typed on the `:` line.
    Command(&'static str),
    /// React to the selected message with the emoji, as the reaction picker
    /// does: bzz has no `:` form for it.
    React,
    /// Keep the theme, as the theme picker's Enter does. Seam: when `:theme
    /// <name>` exists (agent `find`, v0.17 C4), this becomes
    /// `Command("theme")`.
    Theme,
}

/// What one step asks for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ask {
    /// A person bzz has a profile of; the answer is their npub.
    Person,
    /// A joined channel; the answer is `#name`.
    Channel,
    /// An emoji, recent ones first; the answer is the emoji.
    Emoji,
    /// How long until it expires: `30m`, `2h`, `1d`, checked as typed.
    Duration,
    /// Free text; the usage of the command is shown under the field.
    Text,
    /// One of a few words: (the word, what it means).
    Choice(&'static [(&'static str, &'static str)]),
    /// A theme, previewed as the selection moves; the answer is its id.
    Theme,
    /// An installed app; the answer is its name.
    App,
}

/// One argument of a row with steps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Arg {
    pub ask: Ask,
    /// The step's name in the breadcrumb: "Start a DM › person".
    pub label: &'static str,
    /// Written before the answer: "--expires " for the status's expiry.
    pub form: &'static str,
    /// Written before the answers of the steps asked earlier (`:status
    /// --expires 2h text` asks the text first).
    pub lead: bool,
    /// The step may be left out; this names that choice ("no expiry").
    pub skip: Option<&'static str>,
}

impl Arg {
    const fn new(ask: Ask, label: &'static str) -> Self {
        Self {
            ask,
            label,
            form: "",
            lead: false,
            skip: None,
        }
    }

    const fn optional(mut self, skip: &'static str) -> Self {
        self.skip = Some(skip);
        self
    }

    const fn lead(mut self, form: &'static str) -> Self {
        self.form = form;
        self.lead = true;
        self
    }
}

/// The `:` text a row with steps builds from its answers, in the order the
/// steps were asked: "status", ["In a meeting", "2h"] →
/// "status --expires 2h In a meeting". An empty answer (a step left out)
/// writes nothing.
pub fn command_text(command: &str, args: &[Arg], answers: &[&str]) -> String {
    let mut text = command.to_owned();
    for lead in [true, false] {
        for (arg, answer) in args.iter().zip(answers) {
            let answer = answer.trim();
            if arg.lead != lead || answer.is_empty() {
                continue;
            }
            text.push(' ');
            text.push_str(arg.form);
            text.push_str(answer);
        }
    }
    text
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
    /// The built-in keys of the row's [`ActionSpec::key_action`], scope by
    /// scope, in the order the help names them. `KeyMap::builtin` is
    /// generated from these (v0.17 C8); a user's `keymap.toml` goes on top.
    pub keys: &'static [DefaultKey],
    /// The row that stands for a `:` command's first word carries what the
    /// help, "did you mean" and the `:` line say of it: its aliases, its
    /// arguments and what it does. `command::commands` lists these rows'.
    pub head: Option<CommandSpec>,
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
            keys: &[],
            head: None,
        }
    }

    /// This row stands for the `:` command `name`: `args` as a usage line
    /// writes them, `summary` what it does.
    const fn head(
        mut self,
        name: &'static str,
        aliases: &'static [&'static str],
        args: &'static str,
        summary: &'static str,
    ) -> Self {
        self.head = Some(CommandSpec {
            name,
            aliases,
            args,
            summary,
        });
        self
    }

    const fn keys(mut self, keys: &'static [DefaultKey]) -> Self {
        self.keys = keys;
        self
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

const GLOBAL: KeyScope = KeyScope::Global;
const WORKSPACE: KeyScope = KeyScope::Workspace;
const INBOX: KeyScope = KeyScope::Inbox;
const COMPOSER: KeyScope = KeyScope::Composer;
const OVERLAY: KeyScope = KeyScope::Overlay;

const fn steps(
    id: &'static str,
    title: &'static str,
    group: Group,
    then: Then,
    args: &'static [Arg],
) -> ActionSpec {
    ActionSpec::new(id, title, group, Run::Steps(then, args))
}

const fn ask(
    id: &'static str,
    title: &'static str,
    group: Group,
    command: &'static str,
    args: &'static [Arg],
) -> ActionSpec {
    steps(id, title, group, Then::Command(command), args)
}

const PERSON: Arg = Arg::new(Ask::Person, "person");
const CHANNEL: Arg = Arg::new(Ask::Channel, "channel");
const EMOJI: Arg = Arg::new(Ask::Emoji, "emoji");
const APP: Arg = Arg::new(Ask::App, "app");
const NAME: Arg = Arg::new(Ask::Text, "name");
const FOLDER: Arg = Arg::new(Ask::Text, "folder");

/// The NIP-56 reasons, as `buzz_sdk::REPORT_TYPES` lists them (a test
/// keeps the two together).
const REPORT_REASONS: &[(&str, &str)] = &[
    ("spam", "unwanted or repeated"),
    ("impersonation", "pretends to be someone"),
    ("profanity", "abusive language"),
    ("nudity", "explicit content"),
    ("illegal", "against the law"),
    ("malware", "harmful links or files"),
    ("other", "something else"),
];

const SORTS: &[(&str, &str)] = &[
    ("alpha", "by name"),
    ("recent", "latest activity first"),
    ("default", "the community's order"),
];

const HUDDLE_VERBS: &[(&str, &str)] = &[
    ("start", "start a huddle here"),
    ("join", "join the huddle here"),
    ("leave", "leave the call"),
    ("mute", "mute or unmute your microphone"),
    ("end", "end it for everyone"),
    ("devices", "list the audio devices"),
];

const REPORT_WHAT: &[(&str, &str)] = &[("", "the selected message"), ("author", "its author")];

const PRESENCE: &[(&str, &str)] = &[
    ("on", "publish online, away, offline"),
    ("off", "publish nothing"),
];

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
        .keys(&[(GLOBAL, &["i"])])
        .needs(N::Channel),
    steps(
        "msg.react",
        "React to message",
        G::Message,
        Then::React,
        &[EMOJI],
    )
    .key_of(A::React)
    .keys(&[(GLOBAL, &["r"])])
    .place(Where::Workspace)
    .words("emoji reaction")
    .aliases(&["react"])
    .needs(N::Message),
    ui("msg.edit", "Edit my message", G::Message, A::Edit)
        .words("change fix")
        .keys(&[(GLOBAL, &["e"])])
        .aliases(&["edit"])
        .needs(N::Message),
    ui("msg.delete", "Delete my message", G::Message, A::Delete)
        .words("remove")
        .keys(&[(WORKSPACE, &["d"])])
        .aliases(&["delete"])
        .needs(N::Message),
    ui(
        "msg.mark_unread",
        "Mark this channel unread",
        G::Message,
        A::MarkUnread,
    )
    .words("unread new")
    .keys(&[(WORKSPACE, &["u"])])
    .needs(N::Message),
    ui("msg.copy", "Copy message", G::Message, A::CopyMessages)
        .words("clipboard yank copy messages")
        .keys(&[(GLOBAL, &["y"])])
        .aliases(&["copy"])
        .needs(N::Message),
    ui(
        "msg.select_range",
        "Select several messages",
        G::Message,
        A::ToggleCopySelection,
    )
    .words("range visual copy selection")
    .keys(&[(GLOBAL, &["v"])])
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
    .keys(&[(GLOBAL, &["space", "p"])])
    .command(":profile [npub|hex]")
    .head(
        "profile",
        &[],
        "[npub | hex]",
        "the profile of the selected author, or of a key",
    )
    .needs(N::Message),
    ask(
        "msg.report",
        "Report message or author",
        G::Message,
        "report",
        &[
            Arg::new(Ask::Choice(REPORT_WHAT), "what"),
            Arg::new(Ask::Choice(REPORT_REASONS), "reason"),
            Arg::new(Ask::Text, "note").optional("no note"),
        ],
    )
    .words("moderators spam abuse")
    .command(":report [author] <reason> [note]")
    .head(
        "report",
        &[],
        "[author] <reason> [note]",
        "report the selected message, or its author",
    )
    .needs(N::Message),
    cmd(
        "msg.to_channel",
        "Send thread draft to channel",
        G::Message,
        "tochannel",
    )
    .words("also send to channel broadcast")
    .command(":tochannel")
    .head(
        "tochannel",
        &["send-to-channel"],
        "",
        "send the open thread's draft to its channel",
    )
    .needs(N::Thread),
    // Go: finding and moving.
    ui(
        "finder.open",
        "Go to channel or DM",
        G::Go,
        A::ChannelSwitcher,
    )
    .words("find switch jump open channel")
    .keys(&[(GLOBAL, &["space", "space"])])
    .aliases(&["go"]),
    ui("search.open", "Search messages", G::Go, A::Search)
        .words("find text from in after before")
        .command(":search")
        .head(
            "search",
            &[],
            "",
            "search messages",
        )
        .key_of(A::Filter)
        .keys(&[(GLOBAL, &["/"])])
        .aliases(&["search"]),
    ui("inbox.open", "Open Inbox", G::Go, A::OpenInbox)
        .words("mentions personal work notifications")
        .command(":inbox")
        .head(
            "inbox",
            &[],
            "",
            "open the Inbox",
        )
        .keys(&[(GLOBAL, &["space", "n"])])
        .aliases(&["inbox"]),
    ask("dm.new", "Start a DM", G::Go, "dm", &[PERSON])
        .key_of(A::NewDm)
        .words("direct message person chat private")
        .command(":dm <npub|hex>")
        .head(
            "dm",
            &[],
            "[add | hide | <npub>…]",
            "start a DM; add a person to this DM; hide it",
        )
        .aliases(&["dm"])
        .needs(N::Connected),
    cmd("dm.pick", "Start a group DM", G::Go, "dm")
        .words("direct message several people pick choose")
        .command(":dm")
        .needs(N::Connected),
    ask(
        "person.profile",
        "View a person's profile",
        G::Go,
        "profile",
        &[PERSON],
    )
    .words("people who user member agent")
    .command(":profile <npub|hex>"),
    ui("item.open", "Open selected", G::Go, A::ActivateFocused)
        .words("channel thread community enter")
        .keys(&[(GLOBAL, &["enter"])]),
    ui("focus.channels", "Focus channels", G::Go, A::FocusChannels)
        .words("sidebar pane")
        .keys(&[(GLOBAL, &["2"])]),
    ui("focus.timeline", "Focus messages", G::Go, A::FocusTimeline)
        .words("timeline pane")
        .keys(&[(GLOBAL, &["3"])]),
    ui("focus.context", "Focus thread", G::Go, A::FocusContext)
        .words("context pane")
        .keys(&[(GLOBAL, &["4"])]),
    ui(
        "focus.communities",
        "Focus communities",
        G::Go,
        A::FocusCommunities,
    )
    .words("rail pane")
    .keys(&[(GLOBAL, &["1"])]),
    ui("focus.app", "Focus the app", G::Go, A::FocusApp)
        .words("app pane")
        .keys(&[(GLOBAL, &["5"])])
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
        .keys(&[(GLOBAL, &["tab"]), (GLOBAL, &["l"])])
        .place(Where::Anywhere),
    ui("focus.prev", "Previous pane", G::Go, A::PreviousFocus)
        .words("focus cycle back")
        .keys(&[(GLOBAL, &["backtab"]), (GLOBAL, &["h"])])
        .place(Where::Anywhere),
    ui("sel.next", "Next item", G::Go, A::SelectNext)
        .words("down move")
        .keys(&[(GLOBAL, &["j"]), (GLOBAL, &["down"]), (GLOBAL, &["ctrl-n"])])
        .place(Where::Anywhere),
    ui("sel.prev", "Previous item", G::Go, A::SelectPrevious)
        .words("up move")
        .keys(&[(GLOBAL, &["k"]), (GLOBAL, &["up"]), (GLOBAL, &["ctrl-p"])])
        .place(Where::Anywhere),
    ui("sel.first", "Jump to first", G::Go, A::JumpTop)
        .words("top start oldest")
        .keys(&[(GLOBAL, &["g", "g"])])
        .place(Where::Anywhere),
    ui("sel.last", "Jump to last", G::Go, A::JumpBottom)
        .words("bottom end newest latest")
        .keys(&[(GLOBAL, &["G"])])
        .place(Where::Anywhere),
    ui(
        "view.scroll_down",
        "Scroll down",
        G::Go,
        A::ScrollViewportDown,
    )
    .keys(&[(GLOBAL, &["J"])])
    .place(Where::Anywhere),
    ui("view.scroll_up", "Scroll up", G::Go, A::ScrollViewportUp)
        .keys(&[(GLOBAL, &["K"])])
        .place(Where::Anywhere),
    ui("view.half_down", "Half page down", G::Go, A::HalfPageDown)
        .words("scroll page")
        .keys(&[(GLOBAL, &["ctrl-d"])])
        .place(Where::Anywhere),
    ui("view.half_up", "Half page up", G::Go, A::HalfPageUp)
        .words("scroll page")
        .keys(&[(GLOBAL, &["ctrl-u"])])
        .place(Where::Anywhere),
    ui("cmd.open", "Command line", G::Go, A::OpenCommand)
        .words("colon type a command")
        .keys(&[(GLOBAL, &[":"])]),
    ui(
        "actions.menu",
        "Actions for the selected item",
        G::Go,
        A::OpenContextActions,
    )
    .words("context menu")
    .keys(&[(GLOBAL, &["space", "a"])])
    .place(Where::Anywhere),
    ui("palette.open", "Find any action", G::Go, A::OpenPalette)
        .words("palette command commands")
        // v0.17 D4. Text-owning scopes do not inherit Global keys, so
        // Ctrl-K in the composer keeps deleting to the line end.
        .keys(&[(GLOBAL, &["ctrl-k"]), (GLOBAL, &["space", "k"])])
        .place(Where::Anywhere),
    // View: panes, themes, help.
    steps(
        "theme.pick",
        "Change theme…",
        G::View,
        Then::Theme,
        &[Arg::new(Ask::Theme, "theme")],
    )
    .key_of(A::OpenOptions)
    .keys(&[(GLOBAL, &["space", "o"])])
    .words("colors colours options appearance dark light")
    .aliases(&["theme", "themes"])
    .command(":theme")
    .head(
        "theme",
        &[],
        "[<name> | reload]",
        "pick a theme; set one by name; reload the theme file",
    ),
    ui(
        "help.keymap",
        "Show keyboard shortcuts",
        G::View,
        A::OpenHelp,
    )
    .words("help keys keymap bindings")
    .aliases(&["help", "keys", "?"])
    .command(":help")
    .head(
        "help",
        &[],
        "",
        "the keys and these commands",
    )
    .keys(&[(GLOBAL, &["?"])])
    .place(Where::Anywhere),
    ui(
        "view.toggle_communities",
        "Show or hide communities",
        G::View,
        A::ToggleCommunities,
    )
    .words("toggle rail pane")
    .keys(&[(GLOBAL, &["space", "1"])]),
    ui(
        "view.toggle_channels",
        "Show or hide channels",
        G::View,
        A::ToggleChannels,
    )
    .words("toggle sidebar pane")
    .keys(&[(GLOBAL, &["space", "2"])]),
    ui(
        "view.toggle_context",
        "Show or hide thread pane",
        G::View,
        A::ToggleContext,
    )
    .words("toggle context pane")
    .keys(&[(GLOBAL, &["space", "4"])]),
    ui("app.zoom", "Zoom the app", G::View, A::ZoomApp)
        .words("app full screen")
        .keys(&[(GLOBAL, &["space", "5"])])
        .command(":app zoom")
        .needs(N::AppHere),
    ui(
        "pane.narrow",
        "Narrow side pane",
        G::View,
        A::ResizeFocusedNarrow,
    )
    .words("resize width smaller")
    .keys(&[(GLOBAL, &["alt-h"])]),
    ui(
        "pane.widen",
        "Widen side pane",
        G::View,
        A::ResizeFocusedWide,
    )
    .words("resize width larger")
    .keys(&[(GLOBAL, &["alt-l"])]),
    ui("sync.refresh", "Refresh", G::View, A::Refresh)
        .words("reload sync update")
        .keys(&[(GLOBAL, &["space", "r"])])
        .place(Where::Anywhere),
    ui(
        "sidebar.sort_cycle",
        "Cycle channel sort",
        G::View,
        A::CycleChannelSort,
    )
    .words("order sidebar alphabetical recent")
    .keys(&[(GLOBAL, &["space", "s"])]),
    ask(
        "sidebar.sort",
        "Set channel group sort",
        G::View,
        "sort",
        &[Arg::new(Ask::Choice(SORTS), "sort")],
    )
    .words("order sidebar alpha recent default")
    .command(":sort alpha|recent|default")
    .head(
        "sort",
        &[],
        "alpha | recent | default",
        "how the selected channel's group is sorted",
    )
    .needs(N::Channel),
    cmd(
        "sidebar.system",
        "Show or hide System channels",
        G::View,
        "system",
    )
    .words("toggle sidebar access")
    .command(":system")
    .head(
        "system",
        &[],
        "",
        "show or hide the System channels",
    ),
    ask(
        "section.new",
        "New sidebar section",
        G::View,
        "section new",
        &[NAME],
    )
    .words("group folder sidebar")
    .command(":section new <name>")
    .head(
        "section",
        &[],
        "new <name> | rename <name> | delete | move <name> | remove | up | down",
        "sidebar sections, for the selected channel",
    )
    .needs(N::Channel),
    ask(
        "section.rename",
        "Rename sidebar section",
        G::View,
        "section rename",
        &[NAME],
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
    ask(
        "section.move",
        "Move channel to a section",
        G::View,
        "section move",
        &[Arg::new(Ask::Text, "section")],
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
    .command(":media reload")
    .head(
        "media",
        &[],
        "reload",
        "detect the terminal's image support again",
    ),
    // Inbox: its own route and keys.
    ui("inbox.reply", "Reply in place", G::Inbox, A::Compose)
        .words("inbox answer write")
        .place(Where::Inbox),
    ui("inbox.filter", "Cycle Inbox filter", G::Inbox, A::Filter)
        .words("inbox mentions unread show")
        .keys(&[(INBOX, &["f"])])
        .place(Where::Inbox),
    ui(
        "inbox.open_source",
        "Open source context",
        G::Inbox,
        A::OpenCanonicalContext,
    )
    .words("inbox go to channel thread")
    .keys(&[(INBOX, &["o"])])
    .place(Where::Inbox),
    ui("inbox.mark_read", "Mark read", G::Inbox, A::MarkRead)
        .words("inbox done")
        .keys(&[(INBOX, &["m"])])
        .place(Where::Inbox),
    ui("inbox.mark_unread", "Mark unread", G::Inbox, A::MarkUnread)
        .words("inbox")
        .keys(&[(INBOX, &["U"])])
        .place(Where::Inbox),
    ui(
        "inbox.mark_visible_read",
        "Mark visible rows read",
        G::Inbox,
        A::MarkVisibleRead,
    )
    .words("inbox all done")
    .keys(&[(INBOX, &["a"])])
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
    .head(
        "artifacts",
        &[],
        "",
        "the artifacts of this channel or thread",
    )
    .needs(N::Channel),
    ask(
        "artifact.rename",
        "Rename artifact",
        G::Channel,
        "artifact rename",
        &[Arg::new(Ask::Text, "title")],
    )
    .command(":artifact rename <title>")
    .head(
        "artifact",
        &[],
        "rename <title> | delete | restore | move <#channel>",
        "change the selected artifact",
    )
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
    ask(
        "artifact.move",
        "Move artifact to a channel",
        G::Channel,
        "artifact move",
        &[CHANNEL],
    )
    .command(":artifact move <#channel>")
    .needs(N::Artifact),
    cmd("tasks.panel", "My tasks", G::Channel, "tasks")
        .words("assigned todo artifacts")
        .command(":tasks")
        .head(
            "tasks",
            &["my-tasks"],
            "",
            "the tasks assigned to you",
        ),
    cmd("net.resync", "Resync channel history", G::Channel, "resync")
        .words("reload history messages")
        .command(":resync")
        .head(
            "resync",
            &[],
            "",
            "load this channel's history again",
        )
        .needs(N::Channel),
    fill("dm.hide", "Hide this DM", G::Channel, "dm hide")
        .words("direct message close")
        .command(":dm hide")
        .needs(N::Dm),
    ask(
        "dm.add",
        "Add a person to this DM",
        G::Channel,
        "dm add",
        &[PERSON],
    )
    .words("direct message invite group")
    .command(":dm add <npub|hex>")
    .needs(N::Dm),
    ask("solo.new", "New Solo chat", G::Channel, "solo", &[NAME])
        .words("notes private channel")
        .command(":solo <name>")
        .head(
            "solo",
            &[],
            "<name>",
            "create a Solo chat",
        )
        .needs(N::Connected),
    // Huddle: voice in a channel.
    cmd("huddle.panel", "Huddle panel", G::Huddle, "huddle")
        .words("call voice audio")
        .command(":huddle")
        .head(
            "huddle",
            &[],
            "[start | join | leave | end | mute | react <emoji> | devices | input <name> | output <name>]",
            "the huddle of this channel",
        )
        .aliases(&["huddle"])
        .needs(N::Channel),
    ask(
        "huddle.verb",
        "Huddle: start, join, leave, mute…",
        G::Huddle,
        "huddle",
        &[Arg::new(Ask::Choice(HUDDLE_VERBS), "what")],
    )
    .words("call voice audio")
    .command(":huddle <start|join|leave|mute|end|devices>")
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
    ask(
        "huddle.react",
        "React in huddle",
        G::Huddle,
        "huddle react",
        &[EMOJI],
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
    ask(
        "huddle.input",
        "Choose microphone",
        G::Huddle,
        "huddle input",
        &[Arg::new(Ask::Text, "device")],
    )
    .words("huddle audio device input")
    .command(":huddle input <name>"),
    ask(
        "huddle.output",
        "Choose speaker",
        G::Huddle,
        "huddle output",
        &[Arg::new(Ask::Text, "device")],
    )
    .words("huddle audio device output headphones")
    .command(":huddle output <name>"),
    // Apps: v0.16's channel apps.
    cmd("apps.panel", "Installed apps", G::Apps, "apps")
        .words("app list")
        .command(":apps")
        .head(
            "apps",
            &[],
            "",
            "the installed apps",
        )
        .aliases(&["apps"]),
    ask("app.open", "Open app here", G::Apps, "app open", &[APP])
        .words("board app run")
        .command(":app open <name>")
        .head(
            "app",
            &[],
            "open <name> | close | zoom | share | unshare | pending | approve | refuse | rollback | enable | disable | remove | add | new | dev | brief",
            "the app of this channel, and managing apps",
        )
        .needs(N::Channel),
    cmd("app.close", "Close app here", G::Apps, "app close")
        .command(":app close")
        .needs(N::AppHere),
    ask(
        "app.share",
        "Share app",
        G::Apps,
        "app share",
        &[APP.optional("the app open here")],
    )
    .words("app channel")
    .command(":app share [name]"),
    ask(
        "app.unshare",
        "Stop sharing app",
        G::Apps,
        "app unshare",
        &[APP.optional("the app open here")],
    )
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
    ask(
        "app.approve",
        "Approve app version",
        G::Apps,
        "app approve",
        &[APP],
    )
    .words("update version")
    .command(":app approve <name>"),
    ask(
        "app.refuse",
        "Refuse app version",
        G::Apps,
        "app refuse",
        &[APP, Arg::new(Ask::Text, "reason").optional("no reason")],
    )
    .words("reject update version")
    .command(":app refuse <name> [reason]"),
    ask(
        "app.rollback",
        "Roll back app",
        G::Apps,
        "app rollback",
        &[
            APP,
            Arg::new(Ask::Text, "version").optional("the previous version"),
        ],
    )
    .words("previous version undo")
    .command(":app rollback <name> [version]"),
    ask("app.enable", "Enable app", G::Apps, "app enable", &[APP]).command(":app enable <name>"),
    ask("app.disable", "Disable app", G::Apps, "app disable", &[APP])
        .command(":app disable <name>"),
    ask("app.remove", "Remove app", G::Apps, "app remove", &[APP])
        .words("delete uninstall")
        .command(":app remove <name>"),
    ask(
        "app.add",
        "Propose app from folder",
        G::Apps,
        "app add",
        &[FOLDER],
    )
    .words("install")
    .command(":app add <folder>"),
    ask(
        "app.new",
        "New app from template",
        G::Apps,
        "app new",
        &[
            NAME,
            Arg::new(Ask::Text, "template").optional("the default template"),
        ],
    )
    .words("create")
    .command(":app new <name> [template]"),
    ask(
        "app.dev",
        "Dev-run app folder",
        G::Apps,
        "app dev",
        &[FOLDER],
    )
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
        .head(
            "agents",
            &[],
            "[local | directory]",
            "your agents, or the directory of verified agents",
        )
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
        .command(":agent new [name]")
        .head(
            "agent",
            &[],
            "new [name]",
            "create a local agent",
        ),
    cmd("broker.panel", "Agent broker", G::Agents, "broker")
        .words("hosted agents")
        .command(":broker")
        .head(
            "broker",
            &[],
            "",
            "the local agent broker",
        ),
    cmd("approval.open", "Answer what waits", G::Agents, "approvals")
        .words("approve approval allow refuse agent command app version ctrl-y")
        .command(":approvals")
        .head(
            "approvals",
            &["approve", "approval"],
            "",
            "what waits for your answer (also Ctrl-y)",
        ),
    // Me.
    ask(
        "status.set",
        "Set my status…",
        G::Me,
        "status",
        &[
            Arg::new(Ask::Text, "status"),
            Arg::new(Ask::Duration, "expires")
                .optional("no expiry")
                .lead("--expires "),
        ],
    )
    .words("away lunch busy presence")
    .command(":status [--expires N[smhdw]] <text>")
    .head(
        "status",
        &[],
        "[[--expires N[smhdw]] <text> | clear]",
        "show your status; set it; clear it",
    )
    .aliases(&["status"])
    .needs(N::Connected),
    cmd("status.clear", "Clear my status", G::Me, "status clear")
        .command(":status clear")
        .needs(N::Connected),
    ask(
        "presence.toggle",
        "Presence on or off",
        G::Me,
        "presence",
        &[Arg::new(Ask::Choice(PRESENCE), "presence")],
    )
    .words("online away visible")
    .command(":presence [on|off]")
    .head(
        "presence",
        &[],
        "[on | off]",
        "show or change whether bzz publishes presence",
    ),
    cmd("id.lock", "Lock identity", G::Me, "lock")
        .words("key secure away")
        .command(":lock")
        .head(
            "lock",
            &[],
            "",
            "lock the identity until bzz restarts",
        )
        .aliases(&["lock"]),
    cmd("auth.signin", "Federated sign-in", G::Me, "signin")
        .words("sign in login browser")
        .command(":signin")
        .head(
            "signin",
            &["sign-in"],
            "",
            "federated sign-in for this community",
        ),
    // Community.
    cmd("net.reconnect", "Reconnect", G::Community, "reconnect")
        .words("network relay online")
        .command(":reconnect")
        .head(
            "reconnect",
            &[],
            "",
            "reconnect to the relay",
        ),
    fill(
        "cache.purge",
        "Purge cache of this community",
        G::Community,
        "purge-cache",
    )
    .words("clear delete local data")
    .command(":purge-cache")
    .head(
        "purge-cache",
        &[],
        "",
        "delete this community's local cache",
    ),
    shell(
        "community.add",
        "Add a community",
        "bzz community add <url>",
    )
    .words("join new relay")
    .command(":community add")
    .head(
        "community",
        &[],
        "add | remove",
        "done from the shell: bzz community add | remove",
    ),
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
        .head(
            "quit",
            &["q"],
            "",
            "quit bzz (asks first)",
        )
        .key_of(A::BackOrQuit),
    ui("nav.back", "Back or close", G::Bzz, A::BackOrQuit)
        .words("escape")
        .keys(&[
            (GLOBAL, &["q"]),
            (GLOBAL, &["esc"]),
            // Overlays never inherit workspace actions: their owner
            // decides which small set of close keys it accepts.
            (OVERLAY, &["esc"]),
            (OVERLAY, &["q"]),
            (OVERLAY, &["?"]),
            (COMPOSER, &["esc"]),
        ])
        .place(Where::Anywhere),
    // Writing: the composer's own keys (not listed in the palette).
    writing("composer.send", "Send", A::Submit).keys(&[(COMPOSER, &["enter"])]),
    writing("composer.newline", "New line", A::InsertNewline)
        .keys(&[(COMPOSER, &["ctrl-j"]), (COMPOSER, &["alt-enter"])]),
    writing(
        "composer.complete",
        "Complete @mention or :emoji",
        A::Complete,
    )
    .keys(&[(COMPOSER, &["tab"])]),
    writing(
        "composer.del_word",
        "Delete previous word",
        A::DeletePreviousWord,
    )
    .keys(&[(COMPOSER, &["ctrl-w"])]),
    writing(
        "composer.del_start",
        "Delete to line start",
        A::DeleteToStart,
    )
    .keys(&[(COMPOSER, &["ctrl-u"])]),
    writing("composer.del_end", "Delete to line end", A::DeleteToEnd)
        .keys(&[(COMPOSER, &["ctrl-k"])]),
    writing("composer.clear", "Clear draft", A::ClearComposer).keys(&[(COMPOSER, &["ctrl-c"])]),
    writing(
        "composer.paste",
        "Paste text or attachments",
        A::PasteClipboard,
    )
    .keys(&[(COMPOSER, &["ctrl-v"])]),
    writing("composer.attach", "Choose files", A::AttachFile).keys(&[(COMPOSER, &["ctrl-o"])]),
    writing("composer.attach_path", "Attach by path", A::AttachPath)
        .keys(&[(COMPOSER, &["alt-o"])]),
    writing(
        "composer.remove_attachment",
        "Remove last attachment",
        A::RemoveLastAttachment,
    )
    .keys(&[(COMPOSER, &["delete"])]),
    writing(
        "composer.retry",
        "Retry failed attachments",
        A::RetryAttachments,
    )
    .keys(&[(COMPOSER, &["ctrl-r"])]),
    writing("composer.word_left", "Previous word", A::MoveWordLeft)
        .keys(&[(COMPOSER, &["ctrl-left"])]),
    writing("composer.word_right", "Next word", A::MoveWordRight)
        .keys(&[(COMPOSER, &["ctrl-right"])]),
    writing("composer.home", "Line start", A::MoveLineStart).keys(&[(COMPOSER, &["home"])]),
    writing("composer.end", "Line end", A::MoveLineEnd).keys(&[(COMPOSER, &["end"])]),
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

    use super::{Ask, Run, TABLE, Then, Where, command_text};
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
        DmWith(_),
        DmAddPerson(_),
    );

    /// The `:` forms of each command that take an argument a person must
    /// give. An exhaustive `match`: a command added to bzz does not compile
    /// here until someone says whether it takes one, and then
    /// `every_command_with_an_argument_has_its_steps` asks for its steps.
    fn forms_with_an_argument(command: &Command) -> &'static [&'static str] {
        match command {
            Command::Status(_) => &["status"],
            Command::Report(_) => &["report"],
            Command::Profile(_) => &["profile"],
            Command::Presence(_) => &["presence"],
            Command::Sort(_) => &["sort"],
            Command::Solo(_) => &["solo"],
            Command::Huddle(_) => &["huddle", "huddle react", "huddle input", "huddle output"],
            Command::Artifact(_) => &["artifact rename", "artifact move"],
            Command::Section(_) => &["section new", "section rename", "section move"],
            Command::App(_) => &[
                "app open",
                "app share",
                "app unshare",
                "app approve",
                "app refuse",
                "app rollback",
                "app enable",
                "app disable",
                "app remove",
                "app add",
                "app new",
                "app dev",
            ],
            Command::DmWith(_) => &["dm"],
            Command::DmAddPerson(_) => &["dm add"],
            // `local` and `directory` each have a row of their own.
            Command::Agents(_) => &[],
            // `:agent new` opens a form that asks the name.
            Command::Agent(_) => &[],
            // The theme row asks for the theme in a step that previews it;
            // `:theme <name>` is the typed form of the same.
            Command::Theme(_) => &[],
            Command::Quit
            | Command::Help
            | Command::Lock
            | Command::Reconnect
            | Command::Resync
            | Command::ThemeReload
            | Command::MediaReload
            | Command::Inbox
            | Command::Search
            | Command::Dm
            | Command::DmHide
            | Command::DmAdd
            | Command::Approvals
            | Command::AddCommunity
            | Command::RemoveCommunity
            | Command::PurgeCache
            | Command::System
            | Command::SignIn
            | Command::ToChannel
            | Command::Artifacts
            | Command::Tasks
            | Command::Broker
            | Command::Apps
            | Command::Unknown(_) => &[],
        }
    }

    /// One answer of each kind, to build a row's `:` text in a test.
    fn sample(ask: Ask) -> &'static str {
        match ask {
            Ask::Person => "npub1sg6plzptd64u62a878hep2kev88swjh3tw00gjsfl8f237lmu63q0uf63m",
            Ask::Channel => "#general",
            Ask::Emoji => "🎉",
            Ask::Duration => "2h",
            Ask::Text => "Plans",
            Ask::Choice(choices) => choices
                .iter()
                .map(|(word, _)| *word)
                .next_back()
                .unwrap_or(""),
            Ask::Theme => "nord",
            Ask::App => "board",
        }
    }

    /// The `:` texts the table runs or shows, without their usage.
    fn command_texts() -> Vec<String> {
        let mut texts = Vec::new();
        for spec in TABLE {
            if let Run::Command(text) | Run::Prefill(text) | Run::Steps(Then::Command(text), _) =
                spec.run
            {
                texts.push(text.to_owned());
            }
            // And what its steps build: `:dm <npub>` is not `:dm`.
            if let Run::Steps(Then::Command(text), args) = spec.run {
                let answers = args.iter().map(|arg| sample(arg.ask)).collect::<Vec<_>>();
                texts.push(command_text(text, args, &answers));
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

    /// C9: every `:` command that takes an argument is asked for in steps.
    #[test]
    fn every_command_with_an_argument_has_its_steps() {
        let stepped = TABLE
            .iter()
            .filter_map(|spec| match spec.run {
                Run::Steps(Then::Command(command), args) if !args.is_empty() => Some(command),
                _ => None,
            })
            .collect::<Vec<_>>();
        for text in command_texts() {
            for form in forms_with_an_argument(&parse(&text)) {
                assert!(
                    stepped.contains(form),
                    ":{form} takes an argument and no row of src/ui/action_table.rs asks for it in steps"
                );
            }
        }
        // And the forms are reached from the table at all.
        for kind in ALL_COMMANDS {
            let found = command_texts()
                .iter()
                .any(|text| command_kind(&parse(text)) == *kind);
            assert!(found || *kind == "Unknown", "{kind}");
        }
    }

    /// A usage with a required `<argument>` is never opened as a bare
    /// prefill: its row has steps.
    #[test]
    fn no_row_leaves_a_required_argument_to_be_typed() {
        for spec in TABLE {
            let required = spec.command.is_some_and(|usage| usage.contains('<'));
            if required && !matches!(spec.run, Run::Shell(_)) {
                assert!(
                    matches!(spec.run, Run::Steps(..)),
                    "{} needs an argument and has no steps",
                    spec.id
                );
            }
        }
    }

    /// What the steps build is a command bzz parses, of the row's kind.
    #[test]
    fn the_steps_build_commands_bzz_parses() {
        for spec in TABLE {
            let Run::Steps(then, args) = spec.run else {
                continue;
            };
            assert!(!args.is_empty(), "{}", spec.id);
            let Then::Command(command) = then else {
                continue;
            };
            let answers = args.iter().map(|arg| sample(arg.ask)).collect::<Vec<_>>();
            let text = command_text(command, args, &answers);
            let parsed = parse(&text);
            assert!(
                !matches!(parsed, Command::Unknown(_)),
                "{}: :{text} is not a command bzz parses",
                spec.id
            );
            assert_eq!(
                command_kind(&parsed),
                command_kind(&parse(&format!("{command} x"))),
                "{}: :{text}",
                spec.id
            );
        }
        assert_eq!(
            command_text(
                "status",
                match super::by_id("status.set").unwrap().1.run {
                    Run::Steps(_, args) => args,
                    _ => unreachable!(),
                },
                &["In a meeting", "2h"]
            ),
            "status --expires 2h In a meeting"
        );
    }

    #[test]
    fn the_report_reasons_are_the_relays() {
        let words = super::REPORT_REASONS
            .iter()
            .map(|(word, _)| *word)
            .collect::<HashSet<_>>();
        let relay = buzz_sdk::REPORT_TYPES
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        assert_eq!(words, relay);
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
            // A row's keys bind the action it shows a key for.
            assert!(
                spec.keys.is_empty() || spec.key_action().is_some(),
                "{} has keys and no action",
                spec.id
            );
        }
    }
}
