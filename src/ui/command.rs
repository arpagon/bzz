#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
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
    /// `:agents`: the local agents of the open community when it has any,
    /// else the directory of verified remote agents. `:agents local` and
    /// `:agents directory` name one.
    Agents(String),
    /// `:agent new [name]`: the form that creates a local agent.
    Agent(String),
    /// `:approvals`: the real commands that wait for the owner's answer.
    Approvals,
    AddCommunity,
    RemoveCommunity,
    PurgeCache,
    /// Expand or collapse the System sidebar group.
    System,
    /// Federated (NIP-FA) browser sign-in for the active community.
    SignIn,
    /// Send the open thread's draft to its channel as a new message.
    ToChannel,
    /// Report the selected message to the relay moderators (NIP-56).
    Report(String),
    /// NIP-38 status: `:status [--expires N[smhdw]] text`, or `:status clear`.
    Status(String),
    /// NIP-AR panel for the selected channel, or the open thread.
    Artifacts,
    /// Cross-channel `buzz.task` artifacts assigned to me.
    Tasks,
    /// Local agent broker: hosted agents and activity.
    Broker,
    /// `:huddle [start|join|leave|end|mute|react <emoji>|devices|input <name>|output <name>]`.
    Huddle(String),
    /// `:presence [on|off]`: show or change the opt-in presence setting.
    Presence(String),
    /// Profile card for the selected message's author, or `:profile <npub|hex>`.
    Profile(String),
    /// `:artifact rename <title> | delete | restore | move <#channel>`.
    Artifact(String),
    /// Synced sidebar sections: `:section new <name> | rename <name> |
    /// delete | move <name> | remove | up | down`, acting on the selected
    /// channel's section.
    Section(String),
    /// Synced sort mode of the selected channel's group:
    /// `:sort alpha | recent | default`.
    Sort(String),
    /// Create a Solo chat: `:solo <name>`.
    Solo(String),
    /// `:apps`: the installed apps, and where each is open.
    Apps,
    /// `:app open <name> | close | zoom`: the app of the selected channel.
    App(String),
    /// `:join [address]` and `:community add [address]`: the join form,
    /// with the address filled in when one is given (v0.17).
    Join(String),
    Unknown(String),
    /// `:q` or `:quit`: asks, as `q` does (v0.17 C4).
    Quit,
    /// `:help`: the help, with the keys and these commands (v0.17 C4).
    Help,
    /// `:theme` opens the theme picker; `:theme <name>` sets that theme.
    /// `:theme reload` stays [`Command::ThemeReload`] (v0.17 C4).
    Theme(String),
    /// `:dm <npub|hex> [npub|hex…]`: open the DM with these people, as the
    /// DM picker's Enter does (v0.17 C9: the palette's "Start a DM" step).
    DmWith(String),
    /// `:dm add <npub|hex>`: add this person to the open DM, as the picker
    /// does in `:dm add` (v0.17 C9).
    DmAddPerson(String),
}

pub fn parse(input: &str) -> Command {
    match input.trim().trim_start_matches(':') {
        "lock" => Command::Lock,
        "reconnect" => Command::Reconnect,
        "resync" => Command::Resync,
        "theme reload" => Command::ThemeReload,
        "media reload" => Command::MediaReload,
        "inbox" => Command::Inbox,
        "search" => Command::Search,
        "dm" => Command::Dm,
        "dm hide" => Command::DmHide,
        "dm add" => Command::DmAdd,
        value if value == "agents" || value.starts_with("agents ") => {
            Command::Agents(value.trim_start_matches("agents").trim().to_owned())
        }
        value if value == "agent" || value.starts_with("agent ") => {
            Command::Agent(value.trim_start_matches("agent").trim().to_owned())
        }
        "approvals" | "approval" | "approve" => Command::Approvals,
        "community add" => Command::AddCommunity,
        "community remove" => Command::RemoveCommunity,
        "purge-cache" => Command::PurgeCache,
        "system" => Command::System,
        "signin" | "sign-in" => Command::SignIn,
        "tochannel" | "send-to-channel" => Command::ToChannel,
        "artifacts" => Command::Artifacts,
        "apps" => Command::Apps,
        value if value == "app" || value.starts_with("app ") => {
            Command::App(value.trim_start_matches("app").trim().to_owned())
        }
        "tasks" | "my-tasks" => Command::Tasks,
        "broker" => Command::Broker,
        value if value == "presence" || value.starts_with("presence ") => {
            Command::Presence(value.trim_start_matches("presence").trim().to_owned())
        }
        value if value == "profile" || value.starts_with("profile ") => {
            Command::Profile(value.trim_start_matches("profile").trim().to_owned())
        }
        value if value == "huddle" || value.starts_with("huddle ") => {
            Command::Huddle(value.trim_start_matches("huddle").trim().to_owned())
        }
        value if value.starts_with("artifact ") => {
            Command::Artifact(value.trim_start_matches("artifact").trim().to_owned())
        }
        value if value == "status" || value.starts_with("status ") => {
            Command::Status(value.trim_start_matches("status").trim().to_owned())
        }
        value if value == "section" || value.starts_with("section ") => {
            Command::Section(value.trim_start_matches("section").trim().to_owned())
        }
        value if value == "sort" || value.starts_with("sort ") => {
            Command::Sort(value.trim_start_matches("sort").trim().to_owned())
        }
        value if value == "solo" || value.starts_with("solo ") => {
            Command::Solo(value.trim_start_matches("solo").trim().to_owned())
        }
        value if value == "report" || value.starts_with("report ") => {
            Command::Report(value.trim_start_matches("report").trim().to_owned())
        }
        "q" | "quit" => Command::Quit,
        "help" => Command::Help,
        value if value == "theme" || value.starts_with("theme ") => {
            Command::Theme(value.trim_start_matches("theme").trim().to_owned())
        }
        value if value.starts_with("dm add ") => {
            Command::DmAddPerson(value.trim_start_matches("dm add").trim().to_owned())
        }
        value if value.starts_with("dm ") => {
            Command::DmWith(value.trim_start_matches("dm").trim().to_owned())
        }
        value if value == "join" || value.starts_with("join ") => {
            Command::Join(value.trim_start_matches("join").trim().to_owned())
        }
        value if value.starts_with("community add ") => {
            Command::Join(value.trim_start_matches("community add").trim().to_owned())
        }
        value => Command::Unknown(value.to_owned()),
    }
}

/// One `:` command as the help lists it and as a mistake is answered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandSpec {
    /// The first word, without the colon.
    pub name: &'static str,
    /// Other first words that do the same.
    pub aliases: &'static [&'static str],
    /// What may follow the name, as a usage line writes it; empty for none.
    pub args: &'static str,
    /// What it does, in a few words.
    pub summary: &'static str,
}

impl CommandSpec {
    /// `:name args`, as typed.
    pub fn usage(&self) -> String {
        if self.args.is_empty() {
            format!(":{}", self.name)
        } else {
            format!(":{} {}", self.name, self.args)
        }
    }
}

/// Every `:` command of [`parse`], one per first word, in the order of the
/// registry (`crate::ui::action_table::TABLE`): the row that stands for a
/// command says its aliases, arguments and summary (v0.17 C8, plan rule
/// 23). A test keeps these and [`parse`] together.
pub fn commands() -> impl Iterator<Item = &'static CommandSpec> {
    crate::ui::action_table::TABLE
        .iter()
        .filter_map(|row| row.head.as_ref())
}

/// The command whose first word (or alias) is `word`.
pub fn find(word: &str) -> Option<&'static CommandSpec> {
    commands().find(|spec| spec.name == word || spec.aliases.contains(&word))
}

/// The command nearest to a mistyped first word: one whose name starts with
/// it, or the closest within a typo or two. `None` when nothing is near.
pub fn nearest(word: &str) -> Option<&'static CommandSpec> {
    let word = word.to_lowercase();
    if word.is_empty() {
        return None;
    }
    let names = || {
        commands().flat_map(|spec| {
            std::iter::once(spec.name)
                .chain(spec.aliases.iter().copied())
                .map(move |name| (name, spec))
        })
    };
    if let Some((_, spec)) = names().find(|(name, _)| *name == word) {
        return Some(spec);
    }
    // "hud" is the start of one command only: that one.
    let mut prefixed = names()
        .filter(|(name, _)| name.starts_with(&word))
        .map(|(_, spec)| spec.name)
        .collect::<Vec<_>>();
    prefixed.dedup();
    if let [name] = prefixed.as_slice() {
        return find(name);
    }
    let limit = match word.chars().count() {
        0..=2 => 1,
        3..=5 => 2,
        _ => 3,
    };
    names()
        .map(|(name, spec)| (edit_distance(&word, name), spec))
        .filter(|(distance, _)| *distance <= limit)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, spec)| spec)
}

/// The answer to a command [`parse`] does not know. Never a bare "unknown
/// command": it names the nearest command, or the usage of the one whose
/// first word was right, or where the list is.
pub fn unknown_answer(input: &str) -> String {
    let input = input.trim().trim_start_matches(':').trim();
    let word = input.split_whitespace().next().unwrap_or_default();
    if word.is_empty() {
        return "type a command after the colon · :help lists them".into();
    }
    if let Some(spec) = find(word) {
        return format!("usage: {} · {}", spec.usage(), spec.summary);
    }
    match nearest(word) {
        Some(spec) => format!(
            "unknown command :{} · did you mean :{}? · :help lists them all",
            crate::render::sanitize::single_line(word),
            spec.name
        ),
        None => format!(
            "unknown command :{} · :help lists the commands",
            crate::render::sanitize::single_line(word)
        ),
    }
}

/// What `:community add` and `:community remove` answer: the shell command.
pub const COMMUNITY_ANSWER: &str = "a community is removed in the shell: bzz community list shows its id; bzz community remove <id> --yes removes it. :join adds one, here in bzz";

/// Optimal-string-alignment distance: insertions, deletions, substitutions
/// and swaps of two neighbours each cost one.
fn edit_distance(left: &str, right: &str) -> usize {
    let left = left.chars().collect::<Vec<_>>();
    let right = right.chars().collect::<Vec<_>>();
    let width = right.len() + 1;
    let mut table = vec![0_usize; (left.len() + 1) * width];
    for (index, cell) in table.iter_mut().enumerate().take(width) {
        *cell = index;
    }
    for row in 0..=left.len() {
        table[row * width] = row;
    }
    for row in 1..=left.len() {
        for column in 1..=right.len() {
            let cost = usize::from(left[row - 1] != right[column - 1]);
            let mut best = (table[(row - 1) * width + column] + 1)
                .min(table[row * width + column - 1] + 1)
                .min(table[(row - 1) * width + column - 1] + cost);
            if row > 1
                && column > 1
                && left[row - 1] == right[column - 2]
                && left[row - 2] == right[column - 1]
            {
                best = best.min(table[(row - 2) * width + column - 2] + 1);
            }
            table[row * width + column] = best;
        }
    }
    table[left.len() * width + right.len()]
}

/// What the `:` line completes from beside the registry: what bzz has now
/// (v0.17 C11).
#[derive(Clone, Copy, Debug, Default)]
pub struct Words<'a> {
    /// Theme ids, as `:theme <name>` takes them.
    pub themes: &'a [&'a str],
    /// The installed apps of the open community, by name.
    pub apps: &'a [String],
    /// Joined channels, by name, without the `#`.
    pub channels: &'a [String],
}

/// What the `:` line offers for what is typed: the words that may come
/// next, and the usage of the command typed so far.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Offer {
    /// Everything before the word being typed, as typed.
    pub stem: String,
    /// The word being typed (after the last space).
    pub partial: String,
    /// The words that complete it, in the registry's order.
    pub candidates: Vec<String>,
    /// `usage: :huddle [start | …] · the huddle of this channel`.
    pub usage: Option<String>,
}

type RowPath = (Vec<&'static str>, &'static [crate::ui::action_table::Arg]);

/// The words of a row's `:` text and what its steps ask for after them:
/// `"huddle react"`, `[Emoji]`. A row with no `:` text to run takes the
/// literal words of its usage (`:theme` for the theme step).
fn row_path(row: &crate::ui::action_table::ActionSpec) -> Option<RowPath> {
    use crate::ui::action_table::{Run, Then};
    let (text, args) = match row.run {
        Run::Command(text) | Run::Prefill(text) => (text, &[][..]),
        Run::Steps(Then::Command(text), args) => (text, args),
        Run::Steps(_, args) => (row.command?, args),
        Run::Shell(_) | Run::Ui(_) | Run::Quit => (row.command?, &[][..]),
    };
    let words = text
        .trim_start_matches(':')
        .split(' ')
        .take_while(|word| !word.is_empty() && !word.starts_with(['[', '<']))
        .collect::<Vec<_>>();
    (!words.is_empty()).then_some((words, args))
}

/// The answers a step of `ask` has in a fixed list or in what bzz has.
fn ask_words(ask: crate::ui::action_table::Ask, words: &Words<'_>) -> Vec<String> {
    use crate::ui::action_table::Ask;
    match ask {
        Ask::Choice(choices) => choices
            .iter()
            .map(|(word, _)| (*word).to_owned())
            .filter(|word| !word.is_empty())
            .collect(),
        Ask::Theme => words
            .themes
            .iter()
            .map(|theme| (*theme).to_owned())
            .collect(),
        Ask::App => words.apps.to_vec(),
        Ask::Channel => words
            .channels
            .iter()
            .map(|channel| format!("#{channel}"))
            .collect(),
        Ask::Emoji => crate::ui::reaction_picker::REACTIONS
            .iter()
            .map(|emoji| (*emoji).to_owned())
            .collect(),
        Ask::Person | Ask::Duration | Ask::Text => Vec::new(),
    }
}

fn head_usage(spec: &CommandSpec) -> String {
    format!("usage: {} \u{b7} {}", spec.usage(), spec.summary)
}

/// What may follow `line` (the `:` line without its colon), read from the
/// registry's rows: a command's name, then its sub-commands and fixed
/// words, then the answers its steps take (themes, apps, channels).
pub fn offer(line: &str, words: &Words<'_>) -> Offer {
    let split = line.rfind(' ').map_or(0, |index| index + 1);
    let (stem, partial) = line.split_at(split);
    let lower = partial.to_lowercase();
    let typed = stem.split_whitespace().collect::<Vec<_>>();
    let mut candidates = Vec::<String>::new();
    let mut push = |word: String| {
        if word.to_lowercase().starts_with(&lower) && !candidates.contains(&word) {
            candidates.push(word);
        }
    };
    let mut offer = Offer {
        stem: stem.to_owned(),
        partial: partial.to_owned(),
        ..Offer::default()
    };
    let Some(first) = typed.first() else {
        for spec in commands() {
            push(spec.name.to_owned());
        }
        offer.usage = find(partial)
            .or_else(|| match candidates.as_slice() {
                [only] => find(only),
                _ => None,
            })
            .map(head_usage);
        offer.candidates = candidates;
        return offer;
    };
    let Some(head) = find(first) else {
        return offer;
    };
    let rest = &typed[1..];
    let mut usage = None;
    for row in crate::ui::action_table::TABLE {
        let Some((path, args)) = row_path(row) else {
            continue;
        };
        if path[0] != head.name {
            continue;
        }
        let after = &path[1..];
        if rest.len() < after.len() {
            if after[..rest.len()] == *rest {
                push(after[rest.len()].to_owned());
            }
            continue;
        }
        if rest[..after.len()] != *after {
            continue;
        }
        // A sub-command typed whole: its own usage is the one to show.
        if rest.len() == after.len() && !after.is_empty() && usage.is_none() {
            usage = row.command.map(|command| format!("usage: {command}"));
        }
        let step = rest.len() - after.len();
        if let Some(arg) = args.iter().filter(|arg| !arg.lead).nth(step) {
            for word in ask_words(arg.ask, words) {
                push(word);
            }
        }
    }
    offer.candidates = candidates;
    offer.usage = usage.or_else(|| Some(head_usage(head)));
    offer
}

/// Tab on the `:` line (v0.17 C11): the first Tab completes the word being
/// typed; while the line is what the last Tab wrote, Tab and Shift-Tab
/// cycle through the other candidates. A single candidate is written with
/// a space after it, so the next Tab offers what follows. Enter still runs
/// exactly what is typed.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Completion {
    offer: Offer,
    /// The candidate written, while cycling.
    index: Option<usize>,
    /// The line as the last Tab left it.
    written: String,
}

impl Completion {
    /// Completes or cycles `line`; false when nothing completes it.
    pub fn tab(&mut self, line: &mut String, words: &Words<'_>, forward: bool) -> bool {
        if let Some(index) = self.index
            && *line == self.written
            && !self.offer.candidates.is_empty()
        {
            let count = self.offer.candidates.len();
            let next = if forward {
                (index + 1) % count
            } else {
                (index + count - 1) % count
            };
            self.index = Some(next);
            *line = format!("{}{}", self.offer.stem, self.offer.candidates[next]);
            self.written.clone_from(line);
            return true;
        }
        self.offer = offer(line, words);
        self.index = None;
        let count = self.offer.candidates.len();
        match count {
            0 => false,
            1 => {
                *line = format!("{}{} ", self.offer.stem, self.offer.candidates[0]);
                self.written.clone_from(line);
                true
            }
            _ => {
                let index = if forward { 0 } else { count - 1 };
                *line = format!("{}{}", self.offer.stem, self.offer.candidates[index]);
                self.index = Some(index);
                self.written.clone_from(line);
                true
            }
        }
    }

    /// Forgets the cycle (the prompt closed).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// What is shown under the line as it is typed: the candidates (the
    /// one Tab wrote in brackets while cycling) and the usage.
    pub fn lines(&self, line: &str, words: &Words<'_>) -> Vec<String> {
        let (offer, current) = match self.index {
            Some(index) if line == self.written => (self.offer.clone(), Some(index)),
            _ => (offer(line, words), None),
        };
        let mut lines = Vec::new();
        let word = crate::render::sanitize::single_line(offer.partial.trim());
        if offer.candidates.is_empty() {
            let first = line.split_whitespace().next().unwrap_or_default();
            if !word.is_empty() && offer.stem.trim().is_empty() {
                lines.push(match nearest(&word) {
                    Some(spec) => format!(
                        "no command starts with {word} \u{b7} did you mean :{}?",
                        spec.name
                    ),
                    None => format!("no command starts with {word} \u{b7} :help lists them"),
                });
            } else if !word.is_empty() && find(first).is_some() {
                lines.push(format!(
                    "nothing after :{} starts with {word}",
                    crate::render::sanitize::single_line(offer.stem.trim())
                ));
            }
        } else {
            let shown = offer
                .candidates
                .iter()
                .enumerate()
                .map(|(index, candidate)| {
                    let candidate = crate::render::sanitize::single_line(candidate);
                    if Some(index) == current {
                        format!("[{candidate}]")
                    } else {
                        candidate
                    }
                })
                .collect::<Vec<_>>()
                .join("  ");
            lines.push(format!("Tab: {shown}"));
        }
        if let Some(usage) = offer.usage {
            lines.push(usage);
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, commands, find, nearest, parse, unknown_answer};

    #[test]
    fn theme_reload_is_an_explicit_command() {
        assert_eq!(parse(":theme reload"), Command::ThemeReload);
        assert_eq!(parse(":media reload"), Command::MediaReload);
        assert_eq!(parse(":agents"), Command::Agents(String::new()));
        assert_eq!(parse(":approvals"), Command::Approvals);
        assert_eq!(parse(":approve"), Command::Approvals);
        assert_eq!(
            parse(":agents directory"),
            Command::Agents("directory".into())
        );
        assert_eq!(
            parse(":agent new Scout"),
            Command::Agent("new Scout".into())
        );
        assert_eq!(parse(":agent"), Command::Agent(String::new()));
        // v0.17 C4: `:theme <name>` sets a theme; v0.16 did not know it.
        assert_eq!(parse(":theme watch"), Command::Theme("watch".into()));
        assert_eq!(
            parse(":status --expires 2h lunch"),
            Command::Status("--expires 2h lunch".into())
        );
        assert_eq!(parse(":status clear"), Command::Status("clear".into()));
        assert_eq!(
            parse(":report spam noisy"),
            Command::Report("spam noisy".into())
        );
        assert_eq!(parse(":tochannel"), Command::ToChannel);
        assert_eq!(
            parse(":section new Design team"),
            Command::Section("new Design team".into())
        );
        assert_eq!(parse(":sort alpha"), Command::Sort("alpha".into()));
        assert_eq!(parse(":solo Plans"), Command::Solo("Plans".into()));
        assert_eq!(parse(":artifacts"), Command::Artifacts);
        assert_eq!(parse(":apps"), Command::Apps);
        assert_eq!(parse(":app open board"), Command::App("open board".into()));
        assert_eq!(parse(":app close"), Command::App("close".into()));
        assert_eq!(parse(":app"), Command::App(String::new()));
        assert!(matches!(parse(":application"), Command::Unknown(_)));
        assert_eq!(parse(":community add"), Command::AddCommunity);
        assert_eq!(parse(":join"), Command::Join(String::new()));
        assert_eq!(
            parse(":join my-team.example"),
            Command::Join("my-team.example".into())
        );
        assert_eq!(
            parse(":community add wss://my-team.example"),
            Command::Join("wss://my-team.example".into())
        );
        assert!(matches!(parse(":joinx"), Command::Unknown(_)));
        assert_eq!(parse(":tasks"), Command::Tasks);
        assert_eq!(parse(":broker"), Command::Broker);
        assert_eq!(parse(":profile"), Command::Profile(String::new()));
        assert_eq!(parse(":presence off"), Command::Presence("off".into()));
        assert_eq!(parse(":profile npub1x"), Command::Profile("npub1x".into()));
        assert_eq!(parse(":huddle"), Command::Huddle(String::new()));
        assert_eq!(parse(":huddle start"), Command::Huddle("start".into()));
        assert_eq!(
            parse(":huddle react 🎉"),
            Command::Huddle("react 🎉".into())
        );
        assert_eq!(
            parse(":artifact rename Ship it"),
            Command::Artifact("rename Ship it".into())
        );
        assert_eq!(
            parse(":artifact move #ops"),
            Command::Artifact("move #ops".into())
        );
    }

    #[test]
    fn quit_help_and_theme_are_commands() {
        assert_eq!(parse(":q"), Command::Quit);
        assert_eq!(parse(":quit"), Command::Quit);
        assert_eq!(parse(":help"), Command::Help);
        assert_eq!(parse(":theme"), Command::Theme(String::new()));
        assert_eq!(parse(":theme nord"), Command::Theme("nord".into()));
        assert_eq!(parse(":theme reload"), Command::ThemeReload);
    }

    #[test]
    fn a_mistyped_command_is_answered_with_the_nearest() {
        assert_eq!(
            unknown_answer("hudle"),
            "unknown command :hudle · did you mean :huddle? · :help lists them all"
        );
        assert!(unknown_answer("stauts").contains("did you mean :status?"));
        assert!(unknown_answer("serach").contains("did you mean :search?"));
        assert!(unknown_answer("hud").contains("did you mean :huddle?"));
        assert!(unknown_answer("commands").contains(":help"));
        assert!(unknown_answer("zzzzzzzz").contains(":help lists the commands"));
        // The first word is right, what follows is not: its usage.
        assert!(unknown_answer("dm everyone").starts_with("usage: :dm [add | hide | <npub>…]"));
        assert!(unknown_answer("media").starts_with("usage: :media reload"));
        for input in ["hudle", "x", "dm everyone", "", "zzzzzzzz"] {
            assert!(
                !unknown_answer(input).starts_with("unknown command:"),
                "{input}"
            );
        }
        assert_eq!(nearest("quiet").map(|spec| spec.name), Some("quit"));
    }

    /// Every command of the table is one that [`parse`] knows, by its name
    /// and by each alias, so the help never lists a word bzz refuses.
    #[test]
    fn the_table_and_the_parser_agree() {
        for spec in commands() {
            for word in std::iter::once(spec.name).chain(spec.aliases.iter().copied()) {
                let sample = match word {
                    "artifact" => "artifact delete".to_owned(),
                    "media" => "media reload".to_owned(),
                    "community" => "community add".to_owned(),
                    other => other.to_owned(),
                };
                assert!(
                    !matches!(parse(&format!(":{sample}")), Command::Unknown(_)),
                    "{sample}"
                );
                assert_eq!(find(word), Some(spec));
            }
        }
    }

    /// v0.17 C8: the commands are read from the registry's rows, one row a
    /// first word, and that row's own usage starts with the same word.
    #[test]
    fn each_first_word_has_one_row_of_the_registry() {
        let mut names = commands().map(|spec| spec.name).collect::<Vec<_>>();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "a first word stands in two rows");
        let mut expected = vec![
            "help",
            "quit",
            "search",
            "inbox",
            "dm",
            "huddle",
            "status",
            "presence",
            "profile",
            "theme",
            "tochannel",
            "report",
            "artifacts",
            "artifact",
            "tasks",
            "section",
            "sort",
            "solo",
            "system",
            "agents",
            "agent",
            "approvals",
            "broker",
            "apps",
            "app",
            "signin",
            "community",
            "join",
            "lock",
            "reconnect",
            "resync",
            "media",
            "purge-cache",
        ];
        expected.sort_unstable();
        assert_eq!(names, expected);
        for row in crate::ui::action_table::TABLE {
            if let Some(head) = &row.head {
                let usage = row.command.unwrap_or_default();
                assert_eq!(
                    usage.trim_start_matches(':').split(' ').next(),
                    Some(head.name),
                    "{}",
                    row.id
                );
            }
        }
    }

    fn words<'a>(themes: &'a [&'a str], apps: &'a [String]) -> super::Words<'a> {
        super::Words {
            themes,
            apps,
            channels: &[],
        }
    }

    /// v0.17 C11: `:hu`, Tab gives `:huddle `, then Tab cycles its verbs,
    /// read from the registry's rows.
    #[test]
    fn tab_completes_a_command_then_its_verbs_and_cycles() {
        let words = words(&[], &[]);
        let mut completion = super::Completion::default();
        let mut line = "hu".to_owned();
        assert!(completion.tab(&mut line, &words, true));
        assert_eq!(line, "huddle ");
        let offered = super::offer(&line, &words).candidates;
        for verb in [
            "start", "join", "leave", "mute", "end", "devices", "react", "input", "output",
        ] {
            assert!(
                offered.iter().any(|word| word == verb),
                "{verb}: {offered:?}"
            );
        }
        assert!(completion.tab(&mut line, &words, true));
        assert_eq!(line, "huddle start");
        assert!(completion.tab(&mut line, &words, true));
        assert_eq!(line, "huddle join");
        assert!(completion.tab(&mut line, &words, false));
        assert_eq!(line, "huddle start");
        // Under the line: the candidates, the one written in brackets, and
        // the usage of the command.
        let lines = completion.lines(&line, &words);
        assert!(lines[0].starts_with("Tab: [start]  join"), "{lines:?}");
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("usage: :huddle [start")),
            "{lines:?}"
        );
        // Typing goes on from what Tab wrote: Enter runs what is typed.
        line.push('x');
        assert_eq!(parse(&line), Command::Huddle("startx".into()));
    }

    #[test]
    fn tab_completes_themes_apps_and_fixed_words() {
        let apps = vec!["board".to_owned(), "poll".to_owned()];
        let words = words(&["default", "nord", "paper"], &apps);
        let mut completion = super::Completion::default();
        let mut line = "theme n".to_owned();
        assert!(completion.tab(&mut line, &words, true));
        assert_eq!(line, "theme nord ");
        assert_eq!(
            super::offer("theme ", &words).candidates,
            ["default", "nord", "paper", "reload"]
        );
        let mut line = "app open ".to_owned();
        assert!(completion.tab(&mut line, &words, true));
        assert_eq!(line, "app open board");
        assert!(completion.tab(&mut line, &words, true));
        assert_eq!(line, "app open poll");
        assert_eq!(
            super::offer("app open ", &words).usage.as_deref(),
            Some("usage: :app open <name>")
        );
        assert_eq!(
            super::offer("sort ", &words).candidates,
            ["alpha", "recent", "default"]
        );
        assert_eq!(super::offer("presence ", &words).candidates, ["on", "off"]);
        let verbs = super::offer("app ", &words).candidates;
        for verb in ["open", "close", "zoom", "share", "approve", "dev", "brief"] {
            assert!(verbs.iter().any(|word| word == verb), "{verb}: {verbs:?}");
        }
        // Every first word, from nothing typed.
        assert_eq!(
            super::offer("", &words).candidates.len(),
            commands().count()
        );
    }

    #[test]
    fn a_wrong_word_completes_nothing_and_says_so() {
        let words = words(&[], &[]);
        let mut completion = super::Completion::default();
        let mut line = "hudle".to_owned();
        assert!(!completion.tab(&mut line, &words, true));
        assert_eq!(line, "hudle");
        let lines = completion.lines(&line, &words);
        assert_eq!(
            lines,
            ["no command starts with hudle \u{b7} did you mean :huddle?"]
        );
        let mut line = "huddle xyz".to_owned();
        assert!(!completion.tab(&mut line, &words, true));
        let lines = completion.lines(&line, &words);
        assert_eq!(lines[0], "nothing after :huddle starts with xyz");
        assert!(lines[1].starts_with("usage: :huddle"));
    }

    #[test]
    fn dm_takes_people_and_its_bare_forms_stay() {
        assert_eq!(parse(":dm"), Command::Dm);
        assert_eq!(parse(":dm add"), Command::DmAdd);
        assert_eq!(parse(":dm hide"), Command::DmHide);
        assert_eq!(parse(":dm npub1x"), Command::DmWith("npub1x".into()));
        assert_eq!(
            parse(":dm npub1x npub1y"),
            Command::DmWith("npub1x npub1y".into())
        );
        assert_eq!(
            parse(":dm add npub1x"),
            Command::DmAddPerson("npub1x".into())
        );
    }
}
