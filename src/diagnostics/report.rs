use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{BufRead as _, BufReader, Write as _},
    path::Path,
};

use serde::Serialize;
use uuid::Uuid;

use crate::{
    diagnostics::{
        event::{DiagnosticEvent, DiagnosticRecord, SCHEMA_VERSION, unix_millis},
        journal::{JOURNAL_FILE_COUNT, journal_path},
    },
    error::{Error, Result},
    paths::{Paths, set_private_permissions},
    store::{Store, models::OutboxDiagnosticRow},
};

const REPORT_EVENT_LIMIT: usize = 2_048;

#[derive(Clone, Debug, Serialize)]
pub struct DiagnosticStatus {
    pub schema_version: u16,
    pub generated_at_unix_ms: u64,
    pub journal_records: usize,
    pub journal_health: String,
    pub latest_connection_phase: Option<String>,
    pub reconnect_backoff_count: u64,
    pub last_backoff_ms: Option<u64>,
    pub latest_event: Option<String>,
    pub latest_event_at_unix_ms: Option<u64>,
    pub latest_authenticated_at_unix_ms: Option<u64>,
    pub latest_disconnect_class: Option<String>,
    pub agent_typing_subscription_closed_count: u64,
    pub latest_agent_typing_subscription_close_class: Option<String>,
    pub receiver_lagged_count: u64,
    pub diagnostics_dropped_count: u64,
    pub outbox_counts: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct OutboxDiagnosticView {
    pub event_id: String,
    pub kind: u16,
    pub state: crate::store::models::OutboxState,
    pub attempts: u32,
    pub age_seconds: u64,
    pub updated_age_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_class: Option<crate::diagnostics::event::ErrorClass>,
}

#[derive(Debug, Serialize)]
pub struct SupportReport {
    pub schema_version: u16,
    pub generated_at_unix_ms: u64,
    pub bzz_version: &'static str,
    pub os: &'static str,
    pub status: DiagnosticStatus,
    pub outbox: Vec<OutboxDiagnosticView>,
    pub recent_events: Vec<DiagnosticRecord>,
    pub redaction_manifest: RedactionManifest,
}

#[derive(Debug, Serialize)]
pub struct RedactionManifest {
    pub included: Vec<&'static str>,
    pub excluded: Vec<&'static str>,
}

pub fn load_records(paths: &Paths) -> Vec<DiagnosticRecord> {
    let directory = paths.diagnostics_dir();
    let mut records = Vec::new();
    for index in (0..JOURNAL_FILE_COUNT).rev() {
        let path = journal_path(&directory, index);
        let Ok(file) = fs::File::open(path) else {
            continue;
        };
        for line in BufReader::new(file)
            .lines()
            .map_while(std::result::Result::ok)
        {
            if line.len() > 64 * 1024 {
                continue;
            }
            if let Ok(record) = serde_json::from_str::<DiagnosticRecord>(&line)
                && record.is_safe()
            {
                records.push(record);
                if records.len() > REPORT_EVENT_LIMIT {
                    records.remove(0);
                }
            }
        }
    }
    records
}

pub fn load_outbox(paths: &Paths, community: Option<Uuid>) -> Result<Vec<OutboxDiagnosticRow>> {
    let database = paths.database_file();
    if !database.exists() {
        return Ok(Vec::new());
    }
    Store::open_read_only(database)?.outbox_diagnostics(community)
}

pub fn outbox_view(rows: &[OutboxDiagnosticRow]) -> Vec<OutboxDiagnosticView> {
    let now = unix_millis() / 1_000;
    rows.iter()
        .map(|row| OutboxDiagnosticView {
            event_id: row.event_id.clone(),
            kind: row.kind,
            state: row.state,
            attempts: row.attempts,
            age_seconds: now.saturating_sub(row.created_at),
            updated_age_seconds: now.saturating_sub(row.updated_at),
            error_class: row.error_class,
        })
        .collect()
}

pub fn status(
    paths: &Paths,
    records: &[DiagnosticRecord],
    outbox: &[OutboxDiagnosticRow],
) -> DiagnosticStatus {
    let mut counts = BTreeMap::from([
        ("pending".into(), 0_usize),
        ("unknown".into(), 0),
        ("delivered".into(), 0),
        ("rejected".into(), 0),
    ]);
    for row in outbox {
        *counts.entry(row.state.as_str().into()).or_default() += 1;
    }
    let mut authenticated = None;
    let mut disconnect = None;
    let mut connection_phase = None;
    let mut backoff_count = 0_u64;
    let mut last_backoff_ms = None;
    let mut typing_subscription_closed = 0_u64;
    let mut latest_typing_subscription_close = None;
    let mut lagged = 0_u64;
    let mut dropped = 0_u64;
    for record in records {
        match &record.event {
            DiagnosticEvent::ConnectStarted { .. } => connection_phase = Some("connecting".into()),
            DiagnosticEvent::TransportConnected { .. } | DiagnosticEvent::AuthStarted => {
                connection_phase = Some("authenticating".into());
            }
            DiagnosticEvent::Authenticated { .. } => {
                authenticated = Some(record.timestamp_unix_ms);
                connection_phase = Some("online".into());
            }
            DiagnosticEvent::Disconnected { error_class, .. } => {
                disconnect = Some(error_class.as_str().to_owned());
                connection_phase = Some("offline".into());
            }
            DiagnosticEvent::BackoffScheduled { delay_ms, .. } => {
                backoff_count = backoff_count.saturating_add(1);
                last_backoff_ms = Some(*delay_ms);
                connection_phase = Some("backoff".into());
            }
            DiagnosticEvent::AgentTypingSubscriptionClosed { error_class } => {
                typing_subscription_closed = typing_subscription_closed.saturating_add(1);
                latest_typing_subscription_close = Some(error_class.as_str().to_owned());
            }
            DiagnosticEvent::ReceiverLagged {
                skipped_event_count,
            } => lagged = lagged.saturating_add(*skipped_event_count),
            DiagnosticEvent::EventsDropped { count, .. } => {
                dropped = dropped.saturating_add(*count);
            }
            _ => {}
        }
    }
    DiagnosticStatus {
        schema_version: SCHEMA_VERSION,
        generated_at_unix_ms: unix_millis(),
        journal_records: records.len(),
        journal_health: journal_health(paths).into(),
        latest_connection_phase: connection_phase,
        reconnect_backoff_count: backoff_count,
        last_backoff_ms,
        latest_event: records.last().map(|record| record.event.name().into()),
        latest_event_at_unix_ms: records.last().map(|record| record.timestamp_unix_ms),
        latest_authenticated_at_unix_ms: authenticated,
        latest_disconnect_class: disconnect,
        agent_typing_subscription_closed_count: typing_subscription_closed,
        latest_agent_typing_subscription_close_class: latest_typing_subscription_close,
        receiver_lagged_count: lagged,
        diagnostics_dropped_count: dropped,
        outbox_counts: counts,
    }
}

fn journal_health(paths: &Paths) -> &'static str {
    let Ok(metadata) = fs::metadata(paths.diagnostics_dir()) else {
        return "unavailable";
    };
    if !metadata.is_dir() {
        return "unavailable";
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o200 == 0 {
            return "unwritable";
        }
    }
    "available"
}

pub fn create_report(paths: &Paths, output: &Path) -> Result<()> {
    let records = load_records(paths);
    let outbox = load_outbox(paths, None)?;
    let report = SupportReport {
        schema_version: SCHEMA_VERSION,
        generated_at_unix_ms: unix_millis(),
        bzz_version: env!("CARGO_PKG_VERSION"),
        os: crate::diagnostics::event::normalized_os(),
        status: status(paths, &records, &outbox),
        outbox: outbox_view(&outbox),
        recent_events: records,
        redaction_manifest: RedactionManifest {
            included: vec![
                "typed connection and acknowledgement events",
                "locally authored event IDs and kinds",
                "normalized errors, counts, states, and durations",
            ],
            excluded: vec![
                "message, draft, reaction, profile, agent, clipboard, and attachment data",
                "identity, community, channel, thread, participant, and media metadata",
                "credentials, auth material, paths, configuration, and environment",
                "relay payloads, response bodies, raw errors, and event JSON",
            ],
        },
    };
    let bytes = serde_json::to_vec_pretty(&report)
        .map_err(|error| Error::Serialization(error.to_string()))?;
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temporary = parent.join(format!(
        ".bzz-diagnostics-{}.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| Error::io(&temporary, error))?;
        set_private_permissions(&temporary)?;
        file.write_all(&bytes)
            .map_err(|error| Error::io(&temporary, error))?;
        file.sync_all()
            .map_err(|error| Error::io(&temporary, error))?;
        // Creating a second hard link is an atomic no-clobber publication on
        // the same filesystem. It cannot replace a destination which appeared
        // after the initial existence check.
        fs::hard_link(&temporary, output).map_err(|error| Error::io(output, error))?;
        fs::remove_file(&temporary).map_err(|error| Error::io(&temporary, error))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn clear(paths: &Paths) -> Result<usize> {
    let directory = paths.diagnostics_dir();
    let mut removed = 0;
    for index in 0..JOURNAL_FILE_COUNT {
        let path = journal_path(&directory, index);
        match fs::remove_file(&path) {
            Ok(()) => removed += 1,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(Error::io(path, error)),
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn paths(temp: &TempDir) -> Paths {
        Paths {
            config_dir: temp.path().join("config"),
            data_dir: temp.path().join("data"),
            cache_dir: temp.path().join("cache"),
        }
    }

    #[test]
    fn report_refuses_to_replace_an_existing_file() {
        let temp = TempDir::new().unwrap();
        let paths = paths(&temp);
        paths.ensure().unwrap();
        let output = temp.path().join("report.json");
        fs::write(&output, b"keep").unwrap();
        assert!(create_report(&paths, &output).is_err());
        assert_eq!(fs::read(output).unwrap(), b"keep");
    }

    #[test]
    fn manually_modified_unsafe_journal_record_is_ignored() {
        let temp = TempDir::new().unwrap();
        let paths = paths(&temp);
        paths.ensure().unwrap();
        let unsafe_record = serde_json::json!({
            "schema_version": 1,
            "timestamp_unix_ms": 1,
            "session_id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "event": "client.stopped",
            "reason": "SENTINEL-MESSAGE /private/path nsec1secret"
        });
        fs::write(
            crate::diagnostics::journal::journal_path(&paths.diagnostics_dir(), 0),
            format!("{unsafe_record}\n"),
        )
        .unwrap();
        let output = temp.path().join("safe-report.json");
        create_report(&paths, &output).unwrap();
        let report = fs::read_to_string(output).unwrap();
        assert!(!report.contains("SENTINEL"));
        assert!(!report.contains("nsec1secret"));
    }

    #[test]
    fn status_summarizes_typing_subscription_closures_without_scope_identifiers() {
        let temp = TempDir::new().unwrap();
        let paths = paths(&temp);
        paths.ensure().unwrap();
        let records = vec![
            DiagnosticRecord::new(
                "a".repeat(32),
                DiagnosticEvent::AgentTypingSubscriptionClosed {
                    error_class: crate::diagnostics::ErrorClass::AccessDenied,
                },
            ),
            DiagnosticRecord::new(
                "a".repeat(32),
                DiagnosticEvent::AgentTypingSubscriptionClosed {
                    error_class: crate::diagnostics::ErrorClass::Protocol,
                },
            ),
        ];
        let status = status(&paths, &records, &[]);
        assert_eq!(status.agent_typing_subscription_closed_count, 2);
        assert_eq!(
            status
                .latest_agent_typing_subscription_close_class
                .as_deref(),
            Some("protocol")
        );
        let encoded = serde_json::to_string(&status).unwrap();
        assert!(!encoded.contains("channel"));
        assert!(!encoded.contains("community"));
        assert!(!encoded.contains("relay"));
        assert!(!encoded.contains("pubkey"));
    }

    #[test]
    fn report_manifest_names_prohibited_classes_without_content() {
        let temp = TempDir::new().unwrap();
        let paths = paths(&temp);
        paths.ensure().unwrap();
        let output = temp.path().join("report.json");
        create_report(&paths, &output).unwrap();
        let text = fs::read_to_string(output).unwrap();
        assert!(text.contains("redaction_manifest"));
        assert!(!text.contains("event_json"));
    }
}
