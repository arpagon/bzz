//! The records of a session on the relay: `service::records`, reached from
//! a thread that must not wait.
//!
//! Every call returns at once. Opening, each write, and the forwarding of
//! what the service tells run as tasks of the Tokio runtime the session
//! was made on; each puts its result in a queue and wakes the session's
//! owner, who takes it with [`Records::poll`].

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use tokio::runtime::Handle;
use uuid::Uuid;

use super::backend::{Records, Told, Write};
use crate::apps::runtime::Waker;
use crate::protocol::record::{Record, RecordError};
use crate::service::records::{AppRecords, RecordEvent, RecordsService, SyncState};

/// How long opening waits for an earlier opening of the same app in the
/// same channel to end: the sync of a version with other types, closed a
/// moment ago.
const REOPEN_WAIT: Duration = Duration::from_secs(5);
const REOPEN_STEP: Duration = Duration::from_millis(10);

enum Inbound {
    /// The opening numbered so has ended.
    Opened(u64, Result<AppRecords, RecordError>),
    /// The service told the opening numbered so.
    Event(u64, RecordEvent),
    Answer(u64, Result<Record, RecordError>),
}

pub struct RelayRecords {
    service: RecordsService,
    channel: Uuid,
    app: String,
    tokio: Handle,
    waker: Waker,
    sender: Sender<Inbound>,
    inbound: Receiver<Inbound>,
    handle: Option<AppRecords>,
    /// Counts openings: what an earlier one still says is not listened to.
    opening: u64,
}

impl RelayRecords {
    /// The records of `app` in `channel`. Must be called inside a Tokio
    /// runtime: its tasks do the waiting.
    pub fn new(
        service: RecordsService,
        channel: Uuid,
        app: impl Into<String>,
        waker: Waker,
    ) -> std::io::Result<Self> {
        let tokio = Handle::try_current().map_err(std::io::Error::other)?;
        let (sender, inbound) = mpsc::channel();
        Ok(Self {
            service,
            channel,
            app: app.into(),
            tokio,
            waker,
            sender,
            inbound,
            handle: None,
            opening: 0,
        })
    }
}

/// The answer a write is owed, given also when its task ends without one.
struct Owed {
    request: u64,
    sender: Sender<Inbound>,
    waker: Waker,
    answered: bool,
}

impl Owed {
    fn answer(mut self, result: Result<Record, RecordError>) {
        self.answered = true;
        let _ = self.sender.send(Inbound::Answer(self.request, result));
        (self.waker)();
    }
}

impl Drop for Owed {
    fn drop(&mut self) {
        if !self.answered {
            let _ = self.sender.send(Inbound::Answer(
                self.request,
                Err(RecordError::Unavailable {
                    reason: "bzz stopped before the relay answered; the write may still have been stored".into(),
                }),
            ));
            (self.waker)();
        }
    }
}

impl Records for RelayRecords {
    fn identity(&self) -> String {
        self.service.owner_pubkey()
    }

    fn open(&mut self, types: &[String]) {
        // Letting go of the handle ends the sync of the earlier types.
        self.handle = None;
        self.opening += 1;
        let (service, channel, app, types) = (
            self.service.clone(),
            self.channel,
            self.app.clone(),
            types.to_vec(),
        );
        let (opening, sender, waker) = (self.opening, self.sender.clone(), self.waker.clone());
        self.tokio.spawn(async move {
            // The app's earlier sync leaves the open set when its task has
            // ended, which is after its last handle was dropped; an opening
            // that was overtaken by this one may still take the place first.
            let deadline = tokio::time::Instant::now() + REOPEN_WAIT;
            let opened = loop {
                while service.is_open(channel, &app) && tokio::time::Instant::now() < deadline {
                    tokio::time::sleep(REOPEN_STEP).await;
                }
                match service.open(channel, &app, &types).await {
                    Err(RecordError::Invalid { .. })
                        if service.is_open(channel, &app)
                            && tokio::time::Instant::now() < deadline => {}
                    other => break other,
                }
            };
            match opened {
                Ok((handle, mut events)) => {
                    if sender.send(Inbound::Opened(opening, Ok(handle))).is_err() {
                        return;
                    }
                    waker();
                    // Ends when the last handle is gone and the sync with it.
                    while let Some(event) = events.recv().await {
                        if sender.send(Inbound::Event(opening, event)).is_err() {
                            return;
                        }
                        waker();
                    }
                }
                Err(error) => {
                    let _ = sender.send(Inbound::Opened(opening, Err(error)));
                    waker();
                }
            }
        });
    }

    fn state(&self) -> SyncState {
        self.handle
            .as_ref()
            .map_or(SyncState::Loading, AppRecords::state)
    }

    fn records(&self) -> Vec<Record> {
        self.handle
            .as_ref()
            .map_or_else(Vec::new, AppRecords::records)
    }

    fn record(&self, id: Uuid) -> Option<Record> {
        self.handle.as_ref()?.record(id)
    }

    fn write(&mut self, request: u64, write: Write) {
        let owed = Owed {
            request,
            sender: self.sender.clone(),
            waker: self.waker.clone(),
            answered: false,
        };
        let Some(handle) = self.handle.clone() else {
            owed.answer(Err(RecordError::Unavailable {
                reason: "not sent: the app's records are not open".into(),
            }));
            return;
        };
        self.tokio.spawn(async move {
            let result = match write {
                Write::Create {
                    record_type,
                    content,
                } => handle.create(&record_type, content).await,
                Write::Update {
                    id,
                    revision,
                    content,
                } => handle.update(id, &revision, content).await,
                Write::Delete { id, revision } => handle.delete(id, &revision).await,
            };
            owed.answer(result);
        });
    }

    fn retry(&mut self) {
        if let Some(handle) = &self.handle {
            handle.retry();
        }
    }

    fn poll(&mut self) -> Vec<Told> {
        let mut told = Vec::new();
        while let Ok(inbound) = self.inbound.try_recv() {
            match inbound {
                Inbound::Opened(opening, _) | Inbound::Event(opening, _)
                    if opening != self.opening => {}
                Inbound::Opened(_, Ok(handle)) => {
                    self.handle = Some(handle);
                    told.push(Told::Opened(Ok(())));
                }
                Inbound::Opened(_, Err(error)) => told.push(Told::Opened(Err(error))),
                Inbound::Event(_, RecordEvent::State(state)) => told.push(Told::State(state)),
                Inbound::Event(_, RecordEvent::Changes { changes, cause }) => {
                    told.push(Told::Changes { changes, cause });
                }
                Inbound::Answer(request, result) => told.push(Told::Answer { request, result }),
            }
        }
        told
    }

    fn relay(&self) -> Option<AppRecords> {
        self.handle.clone()
    }
}
