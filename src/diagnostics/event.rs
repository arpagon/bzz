use serde::{Deserialize, Serialize};

use crate::error::Error;

pub const SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    Dns,
    Connect,
    Tls,
    Websocket,
    Closed,
    HeartbeatTimeout,
    AckTimeout,
    AuthClock,
    AuthRejected,
    AccessDenied,
    RateLimited,
    Http4xx,
    Http5xx,
    Protocol,
    Database,
    Io,
    Unknown,
}

impl ErrorClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dns => "dns",
            Self::Connect => "connect",
            Self::Tls => "tls",
            Self::Websocket => "websocket",
            Self::Closed => "closed",
            Self::HeartbeatTimeout => "heartbeat_timeout",
            Self::AckTimeout => "ack_timeout",
            Self::AuthClock => "auth_clock",
            Self::AuthRejected => "auth_rejected",
            Self::AccessDenied => "access_denied",
            Self::RateLimited => "rate_limited",
            Self::Http4xx => "http_4xx",
            Self::Http5xx => "http_5xx",
            Self::Protocol => "protocol",
            Self::Database => "database",
            Self::Io => "io",
            Self::Unknown => "unknown",
        }
    }

    /// Classifies an internal error without retaining its source text.
    pub fn from_error(error: &Error) -> Self {
        match error {
            Error::Auth(message) if contains_clock_marker(message) => Self::AuthClock,
            Error::Auth(_) => Self::AuthRejected,
            Error::Access(_) => Self::AccessDenied,
            Error::Protocol(_) | Error::Serialization(_) => Self::Protocol,
            Error::Database(_) => Self::Database,
            Error::Io { .. } => Self::Io,
            Error::Timeout(operation) if operation.contains("heartbeat") => Self::HeartbeatTimeout,
            Error::Timeout(operation) if operation.contains("acknowledgement") => Self::AckTimeout,
            Error::Timeout(_) => Self::Connect,
            Error::Network(message) => classify_network(message),
            Error::Config(_)
            | Error::Locked(_)
            | Error::IdentityMissing(_)
            | Error::IdentityCorrupt(_)
            | Error::Unsupported(_) => Self::Unknown,
        }
    }

    /// Classifies legacy persisted outbox errors and immediately discards the
    /// source string. Diagnostic output must never expose it.
    pub fn from_legacy(value: Option<&str>) -> Option<Self> {
        value.map(|value| {
            let lower = value.to_ascii_lowercase();
            if lower.contains("clock") || lower.contains("timestamp") || lower.contains("expired") {
                Self::AuthClock
            } else if lower.contains("rate-limit") || lower.contains("429") {
                Self::RateLimited
            } else if lower.contains("acknowledgement") || lower.contains("ack timeout") {
                Self::AckTimeout
            } else if lower.contains("access")
                || lower.contains("forbidden")
                || lower.contains("denied")
            {
                Self::AccessDenied
            } else if lower.contains("tls") || lower.contains("certificate") {
                Self::Tls
            } else if lower.contains("dns") || lower.contains("resolve") {
                Self::Dns
            } else if lower.contains("closed") {
                Self::Closed
            } else {
                Self::Unknown
            }
        })
    }

    /// Reduces a NIP-01 `CLOSED` reason to a fixed class and immediately
    /// discards the source text. Relay-provided text must never enter local
    /// diagnostics, support reports, or telemetry.
    pub fn from_subscription_closed(message: &str) -> Self {
        let lower = message.trim().to_ascii_lowercase();
        let prefix = lower
            .split_once(':')
            .map_or(lower.as_str(), |(prefix, _)| prefix.trim());
        match prefix {
            "rate-limited" => Self::RateLimited,
            "blocked" | "restricted" => Self::AccessDenied,
            "auth-required" => Self::AuthRejected,
            "duplicate" | "invalid" | "pow" => Self::Protocol,
            "error" => Self::Unknown,
            _ if lower.contains("rate-limit") || lower.contains("429") => Self::RateLimited,
            _ if lower.contains("auth-required") || lower.contains("authentication required") => {
                Self::AuthRejected
            }
            _ if lower.contains("blocked")
                || lower.contains("restricted")
                || lower.contains("forbidden")
                || lower.contains("denied")
                || lower.contains("not a member") =>
            {
                Self::AccessDenied
            }
            _ if lower.contains("invalid")
                || lower.contains("unsupported")
                || lower.contains("malformed")
                || lower.contains("protocol") =>
            {
                Self::Protocol
            }
            _ if lower.contains("closed") => Self::Closed,
            _ => Self::Unknown,
        }
    }
}

fn contains_clock_marker(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("clock") || lower.contains("timestamp") || lower.contains("expired")
}

fn classify_network(message: &str) -> ErrorClass {
    let lower = message.to_ascii_lowercase();
    if lower.contains("rate-limit") || lower.contains("429") {
        ErrorClass::RateLimited
    } else if lower.contains("dns") || lower.contains("resolve") {
        ErrorClass::Dns
    } else if lower.contains("tls") || lower.contains("certificate") {
        ErrorClass::Tls
    } else if lower.contains("closed") {
        ErrorClass::Closed
    } else if lower.contains("websocket") {
        ErrorClass::Websocket
    } else {
        ErrorClass::Connect
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "event")]
pub enum DiagnosticEvent {
    #[serde(rename = "client.started")]
    ClientStarted {
        version: String,
        os: String,
        build_profile: String,
    },
    #[serde(rename = "client.stopped")]
    ClientStopped { reason: String },
    #[serde(rename = "session.connect_started")]
    ConnectStarted { relay_origin: String, attempt: u32 },
    #[serde(rename = "session.transport_connected")]
    TransportConnected { duration_ms: u64 },
    #[serde(rename = "session.auth_started")]
    AuthStarted,
    #[serde(rename = "session.authenticated")]
    Authenticated { duration_ms: u64 },
    #[serde(rename = "session.connect_failed")]
    ConnectFailed {
        phase: String,
        error_class: ErrorClass,
        duration_ms: u64,
    },
    #[serde(rename = "session.disconnected")]
    Disconnected {
        error_class: ErrorClass,
        #[serde(skip_serializing_if = "Option::is_none")]
        close_code: Option<u16>,
        connection_age_ms: u64,
    },
    #[serde(rename = "session.heartbeat_timeout")]
    HeartbeatTimeout { last_inbound_age_ms: u64 },
    #[serde(rename = "session.backoff_scheduled")]
    BackoffScheduled { attempt: u32, delay_ms: u64 },
    #[serde(rename = "session.reconnect_requested")]
    ReconnectRequested { source: String },
    #[serde(rename = "session.receiver_lagged")]
    ReceiverLagged { skipped_event_count: u64 },
    #[serde(rename = "outbox.queued")]
    OutboxQueued { event_id: String, kind: u16 },
    #[serde(rename = "publish.sent")]
    PublishSent {
        event_id: String,
        kind: u16,
        attempt: u32,
    },
    #[serde(rename = "publish.acknowledged")]
    PublishAcknowledged {
        event_id: String,
        accepted: bool,
        duration_ms: u64,
    },
    #[serde(rename = "publish.uncertain")]
    PublishUncertain {
        event_id: String,
        error_class: ErrorClass,
        duration_ms: u64,
    },
    #[serde(rename = "outbox.state_changed")]
    OutboxStateChanged {
        event_id: String,
        kind: u16,
        old_state: String,
        new_state: String,
        attempts: u32,
    },
    #[serde(rename = "outbox.reconcile_started")]
    ReconcileStarted { eligible_count: u32 },
    #[serde(rename = "outbox.reconcile_observed")]
    ReconcileObserved {
        event_id: String,
        prior_state: String,
    },
    #[serde(rename = "outbox.reconcile_republished")]
    ReconcileRepublished {
        event_id: String,
        accepted: bool,
        duration_ms: u64,
    },
    #[serde(rename = "outbox.reconcile_finished")]
    ReconcileFinished {
        delivered: u32,
        rejected: u32,
        unknown: u32,
        duration_ms: u64,
    },
    #[serde(rename = "diagnostics.events_dropped")]
    EventsDropped { count: u64, queue_capacity: u32 },
    #[serde(rename = "agents.directory_refreshed")]
    AgentDirectoryRefreshed {
        candidates: u32,
        verified: u32,
        projection_changes: u32,
        duration_ms: u64,
    },
    #[serde(rename = "agents.mention_validated")]
    AgentMentionValidated { count: u32, outcome: String },
    #[serde(rename = "agents.typing_subscription_closed")]
    AgentTypingSubscriptionClosed { error_class: ErrorClass },
    #[serde(rename = "telemetry.test")]
    TelemetryTest,
    #[serde(rename = "telemetry.export_health")]
    TelemetryExportHealth {
        error_class: ErrorClass,
        dropped: u64,
    },
}

impl DiagnosticEvent {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::ClientStarted { .. } => "client.started",
            Self::ClientStopped { .. } => "client.stopped",
            Self::ConnectStarted { .. } => "session.connect_started",
            Self::TransportConnected { .. } => "session.transport_connected",
            Self::AuthStarted => "session.auth_started",
            Self::Authenticated { .. } => "session.authenticated",
            Self::ConnectFailed { .. } => "session.connect_failed",
            Self::Disconnected { .. } => "session.disconnected",
            Self::HeartbeatTimeout { .. } => "session.heartbeat_timeout",
            Self::BackoffScheduled { .. } => "session.backoff_scheduled",
            Self::ReconnectRequested { .. } => "session.reconnect_requested",
            Self::ReceiverLagged { .. } => "session.receiver_lagged",
            Self::OutboxQueued { .. } => "outbox.queued",
            Self::PublishSent { .. } => "publish.sent",
            Self::PublishAcknowledged { .. } => "publish.acknowledged",
            Self::PublishUncertain { .. } => "publish.uncertain",
            Self::OutboxStateChanged { .. } => "outbox.state_changed",
            Self::ReconcileStarted { .. } => "outbox.reconcile_started",
            Self::ReconcileObserved { .. } => "outbox.reconcile_observed",
            Self::ReconcileRepublished { .. } => "outbox.reconcile_republished",
            Self::ReconcileFinished { .. } => "outbox.reconcile_finished",
            Self::EventsDropped { .. } => "diagnostics.events_dropped",
            Self::AgentDirectoryRefreshed { .. } => "agents.directory_refreshed",
            Self::AgentMentionValidated { .. } => "agents.mention_validated",
            Self::AgentTypingSubscriptionClosed { .. } => "agents.typing_subscription_closed",
            Self::TelemetryTest => "telemetry.test",
            Self::TelemetryExportHealth { .. } => "telemetry.export_health",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DiagnosticRecord {
    pub schema_version: u16,
    pub timestamp_unix_ms: u64,
    pub session_id: String,
    #[serde(flatten)]
    pub event: DiagnosticEvent,
}

impl DiagnosticRecord {
    pub fn new(session_id: String, event: DiagnosticEvent) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            timestamp_unix_ms: unix_millis(),
            session_id,
            event,
        }
    }

    /// Revalidates persisted evidence before it can enter a report or exporter.
    /// This makes manually modified and future malformed journal records inert.
    pub fn is_safe(&self) -> bool {
        if self.schema_version != SCHEMA_VERSION || !is_hex_id(&self.session_id, 32, 64) {
            return false;
        }
        match &self.event {
            DiagnosticEvent::ClientStarted {
                version,
                os,
                build_profile,
            } => {
                !version.is_empty()
                    && version.len() <= 32
                    && version.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+')
                    })
                    && matches!(os.as_str(), "linux" | "macos" | "windows" | "other")
                    && matches!(build_profile.as_str(), "debug" | "release")
            }
            DiagnosticEvent::ClientStopped { reason } => reason == "user",
            DiagnosticEvent::ConnectStarted { relay_origin, .. } => safe_relay_origin(relay_origin),
            DiagnosticEvent::ConnectFailed { phase, .. } => {
                matches!(phase.as_str(), "transport" | "auth")
            }
            DiagnosticEvent::ReconnectRequested { source } => {
                matches!(source.as_str(), "user" | "supervisor")
            }
            DiagnosticEvent::OutboxQueued { event_id, .. }
            | DiagnosticEvent::PublishSent { event_id, .. }
            | DiagnosticEvent::PublishAcknowledged { event_id, .. }
            | DiagnosticEvent::PublishUncertain { event_id, .. }
            | DiagnosticEvent::ReconcileRepublished { event_id, .. } => is_hex_id(event_id, 64, 64),
            DiagnosticEvent::OutboxStateChanged {
                event_id,
                old_state,
                new_state,
                ..
            } => {
                is_hex_id(event_id, 64, 64)
                    && safe_outbox_state(old_state)
                    && safe_outbox_state(new_state)
            }
            DiagnosticEvent::ReconcileObserved {
                event_id,
                prior_state,
            } => is_hex_id(event_id, 64, 64) && safe_outbox_state(prior_state),
            DiagnosticEvent::AgentMentionValidated { outcome, .. } => {
                matches!(
                    outcome.as_str(),
                    "eligible" | "ineligible" | "policy_unknown" | "refresh_failed"
                )
            }
            DiagnosticEvent::TransportConnected { .. }
            | DiagnosticEvent::AuthStarted
            | DiagnosticEvent::Authenticated { .. }
            | DiagnosticEvent::Disconnected { .. }
            | DiagnosticEvent::HeartbeatTimeout { .. }
            | DiagnosticEvent::BackoffScheduled { .. }
            | DiagnosticEvent::ReceiverLagged { .. }
            | DiagnosticEvent::ReconcileStarted { .. }
            | DiagnosticEvent::ReconcileFinished { .. }
            | DiagnosticEvent::EventsDropped { .. }
            | DiagnosticEvent::AgentDirectoryRefreshed { .. }
            | DiagnosticEvent::AgentTypingSubscriptionClosed { .. }
            | DiagnosticEvent::TelemetryTest
            | DiagnosticEvent::TelemetryExportHealth { .. } => true,
        }
    }
}

pub fn normalized_os() -> &'static str {
    match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "macos",
        "windows" => "windows",
        _ => "other",
    }
}

fn is_hex_id(value: &str, minimum: usize, maximum: usize) -> bool {
    (minimum..=maximum).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn safe_outbox_state(value: &str) -> bool {
    matches!(value, "pending" | "unknown" | "delivered" | "rejected")
}

fn safe_relay_origin(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    matches!(url.scheme(), "ws" | "wss")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && matches!(url.path(), "" | "/")
}

pub fn unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_errors_are_reduced_to_a_closed_class() {
        let secret = "sentinel-message nsec1secret /private/path";
        let record = DiagnosticRecord::new(
            "a".repeat(32),
            DiagnosticEvent::ConnectFailed {
                phase: "transport".into(),
                error_class: ErrorClass::from_error(&Error::Network(secret.into())),
                duration_ms: 4,
            },
        );
        let encoded = serde_json::to_string(&record).unwrap();
        assert!(!encoded.contains(secret));
        assert!(!encoded.contains("nsec1secret"));
        assert!(!encoded.contains("/private/path"));
    }

    #[test]
    fn subscription_close_reasons_are_classified_without_retaining_source_text() {
        let cases = [
            ("rate-limited: slow down", ErrorClass::RateLimited),
            ("blocked: secret", ErrorClass::AccessDenied),
            ("restricted: secret", ErrorClass::AccessDenied),
            ("auth-required: secret", ErrorClass::AuthRejected),
            ("invalid: unsupported filter", ErrorClass::Protocol),
            ("unsupported subscription", ErrorClass::Protocol),
            ("closed by server", ErrorClass::Closed),
            ("hostile relay details nsec1secret", ErrorClass::Unknown),
        ];
        for (source, expected) in cases {
            let record = DiagnosticRecord::new(
                "a".repeat(32),
                DiagnosticEvent::AgentTypingSubscriptionClosed {
                    error_class: ErrorClass::from_subscription_closed(source),
                },
            );
            assert_eq!(
                record.event,
                DiagnosticEvent::AgentTypingSubscriptionClosed {
                    error_class: expected
                }
            );
            let encoded = serde_json::to_string(&record).unwrap();
            assert!(!encoded.contains(source));
            assert!(!encoded.contains("secret"));
            assert!(!encoded.contains("nsec1secret"));
            assert!(record.is_safe());
        }
    }

    #[test]
    fn event_names_are_stable() {
        assert_eq!(DiagnosticEvent::AuthStarted.name(), "session.auth_started");
        assert!(
            serde_json::to_string(&DiagnosticEvent::AuthStarted)
                .unwrap()
                .contains("session.auth_started")
        );
        let record = DiagnosticRecord::new("a".repeat(32), DiagnosticEvent::AuthStarted);
        let encoded = serde_json::to_string(&record).unwrap();
        assert_eq!(
            serde_json::from_str::<DiagnosticRecord>(&encoded).unwrap(),
            record
        );
    }
}
