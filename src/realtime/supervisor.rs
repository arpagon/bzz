use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use nostr::Event;
use serde_json::Value;
use tokio::sync::{broadcast, mpsc, oneshot};
use url::Url;

use crate::{
    auth::signer::SignerHandle,
    diagnostics::{DiagnosticEvent, DiagnosticHandle, ErrorClass},
    error::{Error, Result},
    realtime::session::{self, Ack, SessionEvent},
};

const COMMANDS: usize = 256;

#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum SupervisorEvent {
    Connecting,
    Session(SessionEvent),
    Terminal(String),
    Backoff(Duration),
}

enum Command {
    Subscribe(String, Vec<Value>),
    Close(String),
    Publish(Event, oneshot::Sender<Result<Ack>>),
    Reconnect,
    Shutdown,
}

#[derive(Clone)]
pub struct SupervisorHandle {
    commands: mpsc::Sender<Command>,
    events: broadcast::Sender<SupervisorEvent>,
    diagnostics: DiagnosticHandle,
}

impl SupervisorHandle {
    pub fn spawn(relay: Url, signer: SignerHandle) -> Self {
        Self::spawn_with_diagnostics(relay, signer, DiagnosticHandle::disabled())
    }

    pub fn spawn_with_diagnostics(
        relay: Url,
        signer: SignerHandle,
        diagnostics: DiagnosticHandle,
    ) -> Self {
        let (commands, receiver) = mpsc::channel(COMMANDS);
        let (events, _) = broadcast::channel(2048);
        tokio::spawn(run(
            relay,
            signer,
            receiver,
            events.clone(),
            diagnostics.clone(),
        ));
        Self {
            commands,
            events,
            diagnostics,
        }
    }
    pub fn subscribe_events(&self) -> broadcast::Receiver<SupervisorEvent> {
        self.events.subscribe()
    }
    pub fn diagnostics(&self) -> &DiagnosticHandle {
        &self.diagnostics
    }
    pub async fn subscribe(&self, id: impl Into<String>, filters: Vec<Value>) -> Result<()> {
        self.commands
            .send(Command::Subscribe(id.into(), filters))
            .await
            .map_err(|_| Error::Network("supervisor stopped".into()))
    }
    pub async fn close(&self, id: impl Into<String>) -> Result<()> {
        self.commands
            .send(Command::Close(id.into()))
            .await
            .map_err(|_| Error::Network("supervisor stopped".into()))
    }
    pub async fn publish(&self, event: Event) -> Result<Ack> {
        let (tx, rx) = oneshot::channel();
        self.commands
            .send(Command::Publish(event, tx))
            .await
            .map_err(|_| Error::Network("supervisor stopped".into()))?;
        rx.await
            .map_err(|_| Error::Network("supervisor stopped".into()))?
    }
    pub async fn reconnect(&self) {
        self.diagnostics.emit(DiagnosticEvent::ReconnectRequested {
            source: "user".into(),
        });
        let _ = self.commands.send(Command::Reconnect).await;
    }
    pub async fn shutdown(&self) {
        let _ = self.commands.send(Command::Shutdown).await;
    }
}

fn relay_origin(relay: &Url) -> String {
    relay.origin().ascii_serialization()
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

async fn run(
    relay: Url,
    signer: SignerHandle,
    mut commands: mpsc::Receiver<Command>,
    events: broadcast::Sender<SupervisorEvent>,
    diagnostics: DiagnosticHandle,
) {
    let mut desired: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut delay = Duration::from_millis(250);
    let mut attempt = 0_u32;
    let relay_origin = relay_origin(&relay);
    'outer: loop {
        attempt = attempt.saturating_add(1);
        diagnostics.emit(DiagnosticEvent::ConnectStarted {
            relay_origin: relay_origin.clone(),
            attempt,
        });
        let connect_started = Instant::now();
        let _ = events.send(SupervisorEvent::Connecting);
        let connected =
            session::connect_with_diagnostics(relay.clone(), signer.clone(), diagnostics.clone())
                .await;
        let (handle, mut session_events) = match connected {
            Ok(value) => {
                delay = Duration::from_millis(250);
                attempt = 0;
                value
            }
            Err(error @ Error::Access(_)) | Err(error @ Error::Auth(_)) => {
                diagnostics.emit(DiagnosticEvent::ConnectFailed {
                    phase: "auth".into(),
                    error_class: ErrorClass::from_error(&error),
                    duration_ms: elapsed_millis(connect_started),
                });
                let _ = events.send(SupervisorEvent::Terminal(error.to_string()));
                loop {
                    match commands.recv().await {
                        Some(Command::Reconnect) => break,
                        Some(Command::Subscribe(id, filters)) => {
                            desired.insert(id, filters);
                        }
                        Some(Command::Close(id)) => {
                            desired.remove(&id);
                        }
                        Some(Command::Publish(_, response)) => {
                            let _ = response
                                .send(Err(Error::Access("session is not authenticated".into())));
                        }
                        Some(Command::Shutdown) | None => break 'outer,
                    }
                }
                continue;
            }
            Err(error) => {
                diagnostics.emit(DiagnosticEvent::ConnectFailed {
                    phase: "transport".into(),
                    error_class: ErrorClass::from_error(&error),
                    duration_ms: elapsed_millis(connect_started),
                });
                diagnostics.emit(DiagnosticEvent::BackoffScheduled {
                    attempt,
                    delay_ms: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                });
                let _ = events.send(SupervisorEvent::Session(SessionEvent::Disconnected(
                    error.to_string(),
                )));
                let _ = events.send(SupervisorEvent::Backoff(delay));
                tokio::select! {_=tokio::time::sleep(delay)=>{}, command=commands.recv()=>match command{Some(Command::Shutdown)|None=>break 'outer,Some(Command::Subscribe(id,filters))=>{desired.insert(id,filters);},Some(Command::Close(id))=>{desired.remove(&id);},Some(Command::Publish(_,response))=>{let _=response.send(Err(Error::Network("session is offline".into())));},Some(Command::Reconnect)=>{}}}
                delay = (delay * 2).min(Duration::from_secs(20));
                continue;
            }
        };
        for (id, filters) in &desired {
            if handle.subscribe(id.clone(), filters.clone()).await.is_err() {
                continue 'outer;
            }
        }
        loop {
            tokio::select! {
                event=session_events.recv()=>match event {
                    Some(event@SessionEvent::Disconnected(_))=>{
                        diagnostics.emit(DiagnosticEvent::ReconnectRequested { source:"supervisor".into() });
                        let _=events.send(SupervisorEvent::Session(event));break;
                    }
                    Some(event)=>{let _=events.send(SupervisorEvent::Session(event));}
                    None=>break,
                },
                command=commands.recv()=>match command {
                    Some(Command::Subscribe(id,filters))=>{desired.insert(id.clone(),filters.clone());if handle.subscribe(id,filters).await.is_err(){break;}}
                    Some(Command::Close(id))=>{desired.remove(&id);let _=handle.close(id).await;}
                    Some(Command::Publish(event,response))=>{let current=handle.clone();tokio::spawn(async move{let _=response.send(current.publish(event).await);});}
                    Some(Command::Reconnect)=>{handle.shutdown().await;break;}
                    Some(Command::Shutdown)=>{
                        let subscriptions=desired.keys().cloned().collect::<Vec<_>>();
                        for id in subscriptions { let _=handle.close(id).await; }
                        handle.shutdown().await;
                        break 'outer;
                    }
                    None=>{handle.shutdown().await;break 'outer;}
                }
            }
        }
    }
}
