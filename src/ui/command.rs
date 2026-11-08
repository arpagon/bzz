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
    Agents,
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
    /// `:artifact rename <title> | delete | restore | move <#channel>`.
    Artifact(String),
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
        "agents" => Command::Agents,
        "community add" => Command::AddCommunity,
        "community remove" => Command::RemoveCommunity,
        "purge-cache" => Command::PurgeCache,
        "system" => Command::System,
        "signin" | "sign-in" => Command::SignIn,
        "tochannel" | "send-to-channel" => Command::ToChannel,
        "artifacts" => Command::Artifacts,
        "tasks" | "my-tasks" => Command::Tasks,
        "broker" => Command::Broker,
        value if value.starts_with("artifact ") => {
            Command::Artifact(value.trim_start_matches("artifact").trim().to_owned())
        }
        value if value == "status" || value.starts_with("status ") => {
            Command::Status(value.trim_start_matches("status").trim().to_owned())
        }
        value if value == "report" || value.starts_with("report ") => {
            Command::Report(value.trim_start_matches("report").trim().to_owned())
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
        assert_eq!(parse(":agents"), Command::Agents);
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
        assert_eq!(parse(":artifacts"), Command::Artifacts);
        assert_eq!(parse(":tasks"), Command::Tasks);
        assert_eq!(parse(":broker"), Command::Broker);
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
