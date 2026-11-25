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
pub const COMMUNITY_ANSWER: &str = "communities are added in the shell: bzz community add <label> <wss://relay-address> <identity-id> (bzz identity list shows the id; bzz community add --help explains); bzz community remove <id> --yes removes one";

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
