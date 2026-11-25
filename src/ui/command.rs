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
        value if value == "join" || value.starts_with("join ") => {
            Command::Join(value.trim_start_matches("join").trim().to_owned())
        }
        value if value.starts_with("community add ") => {
            Command::Join(value.trim_start_matches("community add").trim().to_owned())
        }
        value => Command::Unknown(value.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, parse};

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
        assert!(matches!(parse(":theme watch"), Command::Unknown(_)));
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
}
