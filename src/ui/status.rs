use crate::domain::ConnectionState;

pub fn connection_label(state: ConnectionState) -> &'static str {
    match state {
        ConnectionState::Locked => "locked",
        ConnectionState::Offline => "offline cache",
        ConnectionState::Connecting => "connecting",
        ConnectionState::Authenticating => "authenticating",
        ConnectionState::Online => "online",
        ConnectionState::Backfilling => "backfilling",
        ConnectionState::AccessDenied => "access denied",
        ConnectionState::ClockSkew => "clock skew",
    }
}
