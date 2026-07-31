#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Lock,
    Reconnect,
    Resync,
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
        "community add" => Command::AddCommunity,
        "community remove" => Command::RemoveCommunity,
        "purge-cache" => Command::PurgeCache,
        value => Command::Unknown(value.to_owned()),
    }
}
