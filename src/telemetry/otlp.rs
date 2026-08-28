use opentelemetry_proto::tonic::{
    collector::logs::v1::ExportLogsServiceRequest,
    common::v1::{AnyValue, InstrumentationScope, KeyValue, any_value},
    logs::v1::{LogRecord, ResourceLogs, ScopeLogs, SeverityNumber},
    resource::v1::Resource,
};
use prost::Message as _;

use crate::diagnostics::event::{DiagnosticEvent, DiagnosticRecord, ErrorClass, SCHEMA_VERSION};

const ALLOWED_LOG_ATTRIBUTES: &[&str] = &[
    "bzz.event",
    "bzz.schema_version",
    "bzz.session_id",
    "bzz.relay_origin",
    "bzz.phase",
    "bzz.attempt",
    "bzz.error_class",
    "bzz.duration_ms",
    "bzz.backoff_ms",
    "bzz.websocket_close_code",
    "bzz.last_inbound_age_ms",
    "bzz.receiver_lagged_count",
    "bzz.event_id",
    "bzz.event_kind",
    "bzz.outbox_state",
    "bzz.outbox_attempts",
    "bzz.outbox_age_ms",
    "bzz.export_dropped_count",
];

#[derive(Clone, Debug, PartialEq)]
pub enum AttributeValue {
    String(String),
    Integer(i64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RemoteRecord {
    pub event_name: &'static str,
    pub timestamp_unix_ms: u64,
    pub session_id: String,
    pub severity: SeverityNumber,
    pub attributes: Vec<(&'static str, AttributeValue)>,
}

impl RemoteRecord {
    pub fn from_diagnostic(record: &DiagnosticRecord) -> Option<Self> {
        if !record.is_safe() {
            return None;
        }
        let mut attributes = Vec::with_capacity(12);
        let severity = match &record.event {
            DiagnosticEvent::ClientStarted { .. }
            | DiagnosticEvent::TransportConnected { .. }
            | DiagnosticEvent::AuthStarted
            | DiagnosticEvent::Authenticated { .. }
            | DiagnosticEvent::OutboxQueued { .. }
            | DiagnosticEvent::PublishSent { .. }
            | DiagnosticEvent::PublishAcknowledged { accepted: true, .. }
            | DiagnosticEvent::ReconcileStarted { .. }
            | DiagnosticEvent::ReconcileObserved { .. }
            | DiagnosticEvent::ReconcileRepublished { accepted: true, .. }
            | DiagnosticEvent::ReconcileFinished { .. }
            | DiagnosticEvent::TelemetryTest => SeverityNumber::Info,
            DiagnosticEvent::ConnectStarted {
                relay_origin,
                attempt,
            } => {
                attributes.push((
                    "bzz.relay_origin",
                    AttributeValue::String(sanitize_origin(relay_origin)?),
                ));
                attributes.push(("bzz.attempt", AttributeValue::Integer(i64::from(*attempt))));
                SeverityNumber::Info
            }
            DiagnosticEvent::ConnectFailed {
                phase,
                error_class,
                duration_ms,
            } => {
                attributes.push((
                    "bzz.phase",
                    AttributeValue::String(allow_phase(phase)?.to_owned()),
                ));
                push_error(&mut attributes, *error_class);
                push_u64(&mut attributes, "bzz.duration_ms", *duration_ms);
                SeverityNumber::Warn
            }
            DiagnosticEvent::Disconnected {
                error_class,
                close_code,
                connection_age_ms,
            } => {
                push_error(&mut attributes, *error_class);
                if let Some(code) = close_code {
                    attributes.push((
                        "bzz.websocket_close_code",
                        AttributeValue::Integer(i64::from(*code)),
                    ));
                }
                push_u64(&mut attributes, "bzz.duration_ms", *connection_age_ms);
                SeverityNumber::Warn
            }
            DiagnosticEvent::HeartbeatTimeout {
                last_inbound_age_ms,
            } => {
                attributes.push((
                    "bzz.error_class",
                    AttributeValue::String(ErrorClass::HeartbeatTimeout.as_str().into()),
                ));
                push_u64(
                    &mut attributes,
                    "bzz.last_inbound_age_ms",
                    *last_inbound_age_ms,
                );
                SeverityNumber::Warn
            }
            DiagnosticEvent::BackoffScheduled { attempt, delay_ms } => {
                attributes.push(("bzz.attempt", AttributeValue::Integer(i64::from(*attempt))));
                push_u64(&mut attributes, "bzz.backoff_ms", *delay_ms);
                SeverityNumber::Warn
            }
            DiagnosticEvent::ReconnectRequested { source } => {
                let source = match source.as_str() {
                    "user" => "user",
                    "supervisor" => "supervisor",
                    _ => return None,
                };
                attributes.push(("bzz.phase", AttributeValue::String(source.into())));
                SeverityNumber::Info
            }
            DiagnosticEvent::ReceiverLagged {
                skipped_event_count,
            } => {
                push_u64(
                    &mut attributes,
                    "bzz.receiver_lagged_count",
                    *skipped_event_count,
                );
                SeverityNumber::Warn
            }
            DiagnosticEvent::PublishAcknowledged {
                event_id,
                accepted,
                duration_ms,
            } => {
                push_event_id(&mut attributes, event_id)?;
                attributes.push((
                    "bzz.outbox_state",
                    AttributeValue::String(if *accepted { "delivered" } else { "rejected" }.into()),
                ));
                push_u64(&mut attributes, "bzz.duration_ms", *duration_ms);
                if *accepted {
                    SeverityNumber::Info
                } else {
                    SeverityNumber::Warn
                }
            }
            DiagnosticEvent::PublishUncertain {
                event_id,
                error_class,
                duration_ms,
            } => {
                push_event_id(&mut attributes, event_id)?;
                push_error(&mut attributes, *error_class);
                push_u64(&mut attributes, "bzz.duration_ms", *duration_ms);
                SeverityNumber::Warn
            }
            DiagnosticEvent::OutboxStateChanged {
                event_id,
                kind,
                new_state,
                attempts,
                ..
            } => {
                push_event_id(&mut attributes, event_id)?;
                push_kind(&mut attributes, *kind);
                let state = allow_state(new_state)?;
                attributes.push(("bzz.outbox_state", AttributeValue::String(state.into())));
                attributes.push((
                    "bzz.outbox_attempts",
                    AttributeValue::Integer(i64::from(*attempts)),
                ));
                if state == "rejected" || state == "unknown" {
                    SeverityNumber::Warn
                } else {
                    SeverityNumber::Info
                }
            }
            DiagnosticEvent::ReconcileRepublished {
                event_id,
                accepted,
                duration_ms,
            } => {
                push_event_id(&mut attributes, event_id)?;
                attributes.push((
                    "bzz.outbox_state",
                    AttributeValue::String(if *accepted { "delivered" } else { "rejected" }.into()),
                ));
                push_u64(&mut attributes, "bzz.duration_ms", *duration_ms);
                if *accepted {
                    SeverityNumber::Info
                } else {
                    SeverityNumber::Warn
                }
            }
            DiagnosticEvent::EventsDropped { count, .. } => {
                push_u64(&mut attributes, "bzz.export_dropped_count", *count);
                SeverityNumber::Warn
            }
            DiagnosticEvent::TelemetryExportHealth { .. }
            | DiagnosticEvent::RateLimitActivated { .. }
            | DiagnosticEvent::RateLimitCleared
            | DiagnosticEvent::ClientStopped { .. }
            | DiagnosticEvent::AgentDirectoryRefreshed { .. }
            | DiagnosticEvent::AgentMentionValidated { .. }
            | DiagnosticEvent::AgentTypingSubscriptionClosed { .. } => return None,
        };

        match &record.event {
            DiagnosticEvent::TransportConnected { duration_ms }
            | DiagnosticEvent::Authenticated { duration_ms } => {
                push_u64(&mut attributes, "bzz.duration_ms", *duration_ms);
            }
            DiagnosticEvent::OutboxQueued { event_id, kind }
            | DiagnosticEvent::PublishSent { event_id, kind, .. } => {
                push_event_id(&mut attributes, event_id)?;
                push_kind(&mut attributes, *kind);
                if let DiagnosticEvent::PublishSent { attempt, .. } = &record.event {
                    attributes.push(("bzz.attempt", AttributeValue::Integer(i64::from(*attempt))));
                }
            }
            DiagnosticEvent::ReconcileStarted { .. } => {}
            DiagnosticEvent::ReconcileObserved {
                event_id,
                prior_state,
            } => {
                push_event_id(&mut attributes, event_id)?;
                attributes.push((
                    "bzz.outbox_state",
                    AttributeValue::String(allow_state(prior_state)?.into()),
                ));
            }
            DiagnosticEvent::ReconcileFinished { duration_ms, .. } => {
                push_u64(&mut attributes, "bzz.duration_ms", *duration_ms);
            }
            _ => {}
        }

        Some(Self {
            event_name: record.event.name(),
            timestamp_unix_ms: record.timestamp_unix_ms,
            session_id: record.session_id.chars().take(64).collect(),
            severity,
            attributes,
        })
    }
}

pub fn encode(records: &[RemoteRecord]) -> Vec<u8> {
    let observed_nanos = crate::diagnostics::event::unix_millis().saturating_mul(1_000_000);
    let log_records = records
        .iter()
        .map(|record| {
            let mut attributes = vec![string_kv("bzz.event", record.event_name)];
            attributes.push(integer_kv("bzz.schema_version", i64::from(SCHEMA_VERSION)));
            attributes.push(string_kv("bzz.session_id", &record.session_id));
            attributes.extend(
                record
                    .attributes
                    .iter()
                    .filter(|(key, _)| ALLOWED_LOG_ATTRIBUTES.contains(key))
                    .map(|(key, value)| match value {
                        AttributeValue::String(value) => string_kv(key, value),
                        AttributeValue::Integer(value) => integer_kv(key, *value),
                    }),
            );
            LogRecord {
                time_unix_nano: record.timestamp_unix_ms.saturating_mul(1_000_000),
                observed_time_unix_nano: observed_nanos,
                severity_number: record.severity as i32,
                severity_text: record
                    .severity
                    .as_str_name()
                    .replace("SEVERITY_NUMBER_", ""),
                body: Some(string_value(record.event_name)),
                attributes,
                dropped_attributes_count: 0,
                flags: 0,
                trace_id: Vec::new(),
                span_id: Vec::new(),
                event_name: record.event_name.into(),
            }
        })
        .collect();
    let request = ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: Some(Resource {
                attributes: resource_attributes(),
                dropped_attributes_count: 0,
                entity_refs: Vec::new(),
            }),
            scope_logs: vec![ScopeLogs {
                scope: Some(InstrumentationScope {
                    name: "bzz-diagnostics".into(),
                    version: env!("CARGO_PKG_VERSION").into(),
                    attributes: Vec::new(),
                    dropped_attributes_count: 0,
                }),
                log_records,
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    };
    request.encode_to_vec()
}

fn resource_attributes() -> Vec<KeyValue> {
    vec![
        string_kv("service.name", "bzz"),
        string_kv("service.version", env!("CARGO_PKG_VERSION")),
        string_kv("service.namespace", "arpagon"),
        string_kv("os.type", os_type()),
        string_kv("deployment.environment", "desktop"),
    ]
}

fn os_type() -> &'static str {
    match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "macos",
        "windows" => "windows",
        _ => "other",
    }
}

fn string_kv(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.into(),
        value: Some(string_value(value)),
        key_strindex: 0,
    }
}

fn integer_kv(key: &str, value: i64) -> KeyValue {
    KeyValue {
        key: key.into(),
        value: Some(AnyValue {
            value: Some(any_value::Value::IntValue(value)),
        }),
        key_strindex: 0,
    }
}

fn string_value(value: &str) -> AnyValue {
    AnyValue {
        value: Some(any_value::Value::StringValue(
            value.chars().take(256).collect(),
        )),
    }
}

fn push_error(attributes: &mut Vec<(&'static str, AttributeValue)>, value: ErrorClass) {
    attributes.push((
        "bzz.error_class",
        AttributeValue::String(value.as_str().into()),
    ));
}

fn push_event_id(attributes: &mut Vec<(&'static str, AttributeValue)>, value: &str) -> Option<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    attributes.push(("bzz.event_id", AttributeValue::String(value.into())));
    Some(())
}

fn push_kind(attributes: &mut Vec<(&'static str, AttributeValue)>, kind: u16) {
    attributes.push(("bzz.event_kind", AttributeValue::Integer(i64::from(kind))));
}

fn push_u64(attributes: &mut Vec<(&'static str, AttributeValue)>, key: &'static str, value: u64) {
    attributes.push((
        key,
        AttributeValue::Integer(i64::try_from(value).unwrap_or(i64::MAX)),
    ));
}

fn allow_phase(value: &str) -> Option<&'static str> {
    match value {
        "transport" => Some("transport"),
        "auth" => Some("auth"),
        _ => None,
    }
}

fn allow_state(value: &str) -> Option<&'static str> {
    match value {
        "pending" => Some("pending"),
        "unknown" => Some("unknown"),
        "delivered" => Some("delivered"),
        "rejected" => Some("rejected"),
        _ => None,
    }
}

fn sanitize_origin(value: &str) -> Option<String> {
    let url = url::Url::parse(value).ok()?;
    if !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    let host = url.host_str()?;
    match url.port() {
        Some(port) => Some(format!("{}://{host}:{port}", url.scheme())),
        None => Some(format!("{}://{host}", url.scheme())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_local_strings_cannot_enter_otlp() {
        let record = DiagnosticRecord::new(
            "a".repeat(32),
            DiagnosticEvent::ClientStopped {
                reason: "nsec1secret /private/path message-body".into(),
            },
        );
        assert!(RemoteRecord::from_diagnostic(&record).is_none());
    }

    #[test]
    fn remote_agent_diagnostics_remain_local_only() {
        for event in [
            DiagnosticEvent::AgentDirectoryRefreshed {
                candidates: 3,
                verified: 2,
                projection_changes: 1,
                duration_ms: 8,
            },
            DiagnosticEvent::AgentMentionValidated {
                count: 1,
                outcome: "eligible".into(),
            },
            DiagnosticEvent::AgentTypingSubscriptionClosed {
                error_class: ErrorClass::Protocol,
            },
        ] {
            let record = DiagnosticRecord::new("a".repeat(32), event);
            assert!(record.is_safe());
            assert!(RemoteRecord::from_diagnostic(&record).is_none());
        }
    }

    #[test]
    fn encoded_logs_have_no_trace_context_and_stable_body() {
        let record = DiagnosticRecord::new("a".repeat(32), DiagnosticEvent::TelemetryTest);
        let remote = RemoteRecord::from_diagnostic(&record).unwrap();
        let bytes = encode(&[remote]);
        let decoded = ExportLogsServiceRequest::decode(bytes.as_slice()).unwrap();
        let log = &decoded.resource_logs[0].scope_logs[0].log_records[0];
        assert_eq!(log.event_name, "telemetry.test");
        assert!(log.trace_id.is_empty());
        assert!(log.span_id.is_empty());
        assert!(
            log.attributes
                .iter()
                .all(|attribute| ALLOWED_LOG_ATTRIBUTES.contains(&attribute.key.as_str()))
        );
    }
}
