use std::{
    fs::{self, OpenOptions},
    io::Write as _,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, SyncSender, TrySendError},
    },
    time::Duration,
};

use crate::{
    config::LocalJournalMode,
    diagnostics::event::{DiagnosticEvent, DiagnosticRecord},
    error::{Error, Result},
    paths::{Paths, set_private_permissions},
    telemetry::TelemetryHandle,
};

pub const JOURNAL_FILE_BYTES: u64 = 2 * 1024 * 1024;
pub const JOURNAL_FILE_COUNT: usize = 3;
pub const JOURNAL_QUEUE_CAPACITY: usize = 256;

#[derive(Debug)]
enum JournalCommand {
    Record(DiagnosticRecord),
    Shutdown(mpsc::Sender<()>),
}

#[derive(Clone)]
pub struct DiagnosticHandle {
    session_id: Arc<str>,
    journal: Option<SyncSender<JournalCommand>>,
    telemetry: Option<TelemetryHandle>,
    dropped: Arc<AtomicU64>,
}

impl DiagnosticHandle {
    pub fn disabled() -> Self {
        Self {
            session_id: Arc::from("disabled"),
            journal: None,
            telemetry: None,
            dropped: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn start(
        paths: &Paths,
        mode: LocalJournalMode,
        telemetry: Option<TelemetryHandle>,
    ) -> Result<Self> {
        let session_id: Arc<str> = Arc::from(uuid::Uuid::new_v4().simple().to_string());
        let dropped = Arc::new(AtomicU64::new(0));
        let journal = if mode == LocalJournalMode::On {
            let (sender, receiver) = mpsc::sync_channel(JOURNAL_QUEUE_CAPACITY);
            let directory = paths.diagnostics_dir();
            let writer_dropped = dropped.clone();
            std::thread::Builder::new()
                .name("bzz-diagnostics".into())
                .spawn(move || run_writer(&directory, receiver, &writer_dropped))
                .map_err(|error| Error::io("diagnostics writer thread", error))?;
            Some(sender)
        } else {
            None
        };
        Ok(Self {
            session_id,
            journal,
            telemetry,
            dropped,
        })
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Non-blocking by contract. Saturation drops diagnostic evidence, never
    /// application work, and exposes only a count in the next journal record.
    pub fn emit(&self, event: DiagnosticEvent) {
        let record = DiagnosticRecord::new(self.session_id.to_string(), event);
        if !record.is_safe() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if let Some(telemetry) = &self.telemetry {
            telemetry.try_emit(&record);
        }
        if let Some(sender) = &self.journal {
            match sender.try_send(JournalCommand::Record(record)) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    self.dropped.fetch_add(1, Ordering::Relaxed);
                }
                Err(TrySendError::Disconnected(_)) => {}
            }
        }
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub async fn shutdown(&self) {
        if let Some(telemetry) = &self.telemetry {
            telemetry.shutdown().await;
        }
        let Some(sender) = self.journal.clone() else {
            return;
        };
        let _ = tokio::time::timeout(
            Duration::from_millis(250),
            tokio::task::spawn_blocking(move || {
                let (done_tx, done_rx) = mpsc::channel();
                if sender.send(JournalCommand::Shutdown(done_tx)).is_ok() {
                    let _ = done_rx.recv_timeout(Duration::from_millis(200));
                }
            }),
        )
        .await;
    }
}

fn run_writer(directory: &Path, receiver: mpsc::Receiver<JournalCommand>, dropped: &AtomicU64) {
    let mut writable = prepare_directory(directory).is_ok();
    while let Ok(command) = receiver.recv() {
        match command {
            JournalCommand::Record(record) => {
                if !writable {
                    continue;
                }
                let skipped = dropped.swap(0, Ordering::Relaxed);
                if skipped > 0 {
                    let dropped_record = DiagnosticRecord::new(
                        record.session_id.clone(),
                        DiagnosticEvent::EventsDropped {
                            count: skipped,
                            queue_capacity: JOURNAL_QUEUE_CAPACITY as u32,
                        },
                    );
                    if append_record(directory, &dropped_record).is_err() {
                        writable = false;
                        continue;
                    }
                }
                if append_record(directory, &record).is_err() {
                    writable = false;
                }
            }
            JournalCommand::Shutdown(done) => {
                let _ = done.send(());
                break;
            }
        }
    }
}

fn prepare_directory(directory: &Path) -> Result<()> {
    fs::create_dir_all(directory).map_err(|error| Error::io(directory, error))?;
    set_private_permissions(directory)
}

fn append_record(directory: &Path, record: &DiagnosticRecord) -> Result<()> {
    let mut encoded =
        serde_json::to_vec(record).map_err(|error| Error::Serialization(error.to_string()))?;
    encoded.push(b'\n');
    if encoded.len() as u64 > JOURNAL_FILE_BYTES {
        return Ok(());
    }
    let active = journal_path(directory, 0);
    let current = fs::metadata(&active).map_or(0, |metadata| metadata.len());
    if current.saturating_add(encoded.len() as u64) > JOURNAL_FILE_BYTES {
        rotate(directory)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&active)
        .map_err(|error| Error::io(&active, error))?;
    set_private_permissions(&active)?;
    file.write_all(&encoded)
        .map_err(|error| Error::io(&active, error))?;
    file.flush().map_err(|error| Error::io(&active, error))
}

fn rotate(directory: &Path) -> Result<()> {
    let oldest = journal_path(directory, JOURNAL_FILE_COUNT - 1);
    if oldest.exists() {
        fs::remove_file(&oldest).map_err(|error| Error::io(&oldest, error))?;
    }
    for index in (1..JOURNAL_FILE_COUNT).rev() {
        let from = journal_path(directory, index - 1);
        let to = journal_path(directory, index);
        if from.exists() {
            fs::rename(&from, &to).map_err(|error| Error::io(&from, error))?;
        }
    }
    Ok(())
}

pub fn journal_path(directory: &Path, index: usize) -> PathBuf {
    if index == 0 {
        directory.join("journal.jsonl")
    } else {
        directory.join(format!("journal.jsonl.{index}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn rotation_stays_inside_the_compiled_file_count() {
        let temp = TempDir::new().unwrap();
        prepare_directory(temp.path()).unwrap();
        for index in 0..JOURNAL_FILE_COUNT {
            fs::write(journal_path(temp.path(), index), b"old\n").unwrap();
        }
        rotate(temp.path()).unwrap();
        assert!(!journal_path(temp.path(), 0).exists());
        assert!(journal_path(temp.path(), JOURNAL_FILE_COUNT - 1).exists());
        assert!(!journal_path(temp.path(), JOURNAL_FILE_COUNT).exists());
    }

    #[test]
    fn saturated_journal_queue_drops_without_waiting() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        sender
            .try_send(JournalCommand::Record(DiagnosticRecord::new(
                "a".repeat(32),
                DiagnosticEvent::AuthStarted,
            )))
            .unwrap();
        let handle = DiagnosticHandle {
            session_id: Arc::from("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            journal: Some(sender),
            telemetry: None,
            dropped: Arc::new(AtomicU64::new(0)),
        };
        let started = std::time::Instant::now();
        handle.emit(DiagnosticEvent::AuthStarted);
        assert!(started.elapsed() < Duration::from_millis(50));
        assert_eq!(handle.dropped(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn journal_is_owner_private() {
        use std::os::unix::fs::PermissionsExt as _;
        let temp = TempDir::new().unwrap();
        append_record(
            temp.path(),
            &DiagnosticRecord::new("a".repeat(32), DiagnosticEvent::AuthStarted),
        )
        .unwrap();
        assert_eq!(
            fs::metadata(journal_path(temp.path(), 0))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
