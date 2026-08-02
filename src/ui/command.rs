#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Lock,
    Reconnect,
    Resync,
    ThemeReload,
    MediaReload,
    AddCommunity,
    RemoveCommunity,
    PurgeCache,
    Unknown(String),
}

pub fn parse(input: &str) -> Command {
    match input.trim().trim_start_matches(':') {
        "lock" => Command::Lock,
        "reconnect" => Command::Reconnect,
        "resync" => Command::Resync,
        "theme reload" => Command::ThemeReload,
        "media reload" => Command::MediaReload,
        "community add" => Command::AddCommunity,
        "community remove" => Command::RemoveCommunity,
        "purge-cache" => Command::PurgeCache,
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
        assert!(matches!(parse(":theme watch"), Command::Unknown(_)));
    }
}
