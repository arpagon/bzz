use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use futures_util::{SinkExt as _, StreamExt as _};
use nostr::{Event, RelayUrl};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;

use crate::{
    auth::signer::SignerHandle,
    diagnostics::{DiagnosticEvent, DiagnosticHandle, ErrorClass},
    error::{Error, Result},
    protocol::envelope::{self, RelayMessage},
    realtime::admission,
};

const COMMAND_CAPACITY: usize = 256;
const EVENT_CAPACITY: usize = 1024;

#[derive(Clone, Debug)]
pub struct Ack {
    pub event_id: String,
    pub accepted: bool,
    pub message: String,
}

#[derive(Clone, Debug)]
pub enum SessionEvent {
    Authenticated,
    Event {
        subscription: String,
        event: Event,
    },
    Eose(String),
    Notice(String),
    Closed {
        subscription: String,
        message: String,
    },
    Count {
        subscription: String,
        count: u64,
    },
    Disconnected(String),
}

struct PendingAck {
    response: oneshot::Sender<Result<Ack>>,
    started: Instant,
}

enum Command {
    Subscribe {
        id: String,
        filters: Vec<Value>,
        response: oneshot::Sender<Result<()>>,
    },
    Close {
        id: String,
    },
    Publish {
        event: Event,
        admitted: oneshot::Sender<Result<()>>,
        response: oneshot::Sender<Result<Ack>>,
    },
    ForgetPending {
        event_id: String,
    },
    Shutdown,
}

#[derive(Clone)]
pub struct SessionHandle {
    sender: mpsc::Sender<Command>,
    diagnostics: DiagnosticHandle,
}

impl SessionHandle {
    pub async fn subscribe(&self, id: impl Into<String>, filters: Vec<Value>) -> Result<()> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::Subscribe {
                id: id.into(),
                filters,
                response: sender,
            })
            .await
            .map_err(|_| Error::Network("session is offline".into()))?;
        receiver
            .await
            .map_err(|_| Error::Network("session stopped".into()))?
    }

    pub async fn close(&self, id: impl Into<String>) -> Result<()> {
        self.sender
            .send(Command::Close { id: id.into() })
            .await
            .map_err(|_| Error::Network("session is offline".into()))
    }

    pub async fn publish(&self, event: Event) -> Result<Ack> {
        let event_id = event.id.to_hex();
        let (admitted_tx, admitted_rx) = oneshot::channel();
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::Publish {
                event,
                admitted: admitted_tx,
                response: sender,
            })
            .await
            .map_err(|_| Error::Network("session is offline".into()))?;
        admitted_rx
            .await
            .map_err(|_| Error::Network("session stopped before wire send".into()))??;
        match tokio::time::timeout(Duration::from_secs(25), receiver).await {
            Ok(result) => result.map_err(|_| Error::Network("session stopped".into()))?,
            Err(_) => {
                let _ = self.sender.try_send(Command::ForgetPending {
                    event_id: event_id.clone(),
                });
                self.diagnostics.emit(DiagnosticEvent::PublishUncertain {
                    event_id,
                    error_class: ErrorClass::AckTimeout,
                    duration_ms: 25_000,
                });
                Err(Error::Timeout("relay acknowledgement".into()))
            }
        }
    }

    pub async fn shutdown(&self) {
        let _ = self.sender.send(Command::Shutdown).await;
    }
}

pub async fn connect(
    relay: Url,
    signer: SignerHandle,
) -> Result<(SessionHandle, mpsc::Receiver<SessionEvent>)> {
    connect_with_diagnostics(relay, signer, DiagnosticHandle::disabled()).await
}

pub async fn connect_with_diagnostics(
    relay: Url,
    signer: SignerHandle,
    diagnostics: DiagnosticHandle,
) -> Result<(SessionHandle, mpsc::Receiver<SessionEvent>)> {
    let connect_started = Instant::now();
    let (socket, _response) = connect_async(relay.as_str())
        .await
        .map_err(|error| Error::Network(error.to_string()))?;
    diagnostics.emit(DiagnosticEvent::TransportConnected {
        duration_ms: elapsed_millis(connect_started),
    });
    let auth_started = Instant::now();
    diagnostics.emit(DiagnosticEvent::AuthStarted);
    let (mut sink, mut stream) = socket.split();
    let challenge = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            match stream.next().await {
                Some(Ok(Message::Text(text))) => match RelayMessage::parse(&text)? {
                    RelayMessage::Auth(challenge) => return Ok(challenge),
                    RelayMessage::Notice(message) => return Err(classify_auth_failure(&message)),
                    _ => {}
                },
                Some(Ok(Message::Ping(payload))) => sink
                    .send(Message::Pong(payload))
                    .await
                    .map_err(|error| Error::Network(error.to_string()))?,
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(Error::Network(error.to_string())),
                None => return Err(Error::Network("relay closed before AUTH challenge".into())),
            }
        }
    })
    .await
    .map_err(|_| Error::Timeout("relay AUTH challenge".into()))??;
    if challenge.len() > 1024 {
        return Err(Error::Protocol("AUTH challenge is too large".into()));
    }
    let relay_url =
        RelayUrl::parse(relay.as_str()).map_err(|error| Error::Config(error.to_string()))?;
    let auth_event = signer.auth(&challenge, relay_url).await?;
    sink.send(Message::Text(envelope::auth(&auth_event).into()))
        .await
        .map_err(|error| Error::Network(error.to_string()))?;
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            match stream.next().await {
                Some(Ok(Message::Text(text))) => match RelayMessage::parse(&text)? {
                    RelayMessage::Ok {
                        event_id,
                        accepted,
                        message,
                    } if event_id == auth_event.id.to_hex() => {
                        return if accepted {
                            Ok(())
                        } else {
                            Err(classify_auth_failure(&message))
                        };
                    }
                    RelayMessage::Notice(message) => return Err(classify_auth_failure(&message)),
                    _ => {}
                },
                Some(Ok(Message::Ping(payload))) => sink
                    .send(Message::Pong(payload))
                    .await
                    .map_err(|error| Error::Network(error.to_string()))?,
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(Error::Network(error.to_string())),
                None => return Err(Error::Network("relay closed during authentication".into())),
            }
        }
    })
    .await
    .map_err(|_| Error::Timeout("relay AUTH acknowledgement".into()))??;
    diagnostics.emit(DiagnosticEvent::Authenticated {
        duration_ms: elapsed_millis(auth_started),
    });

    let (command_tx, mut command_rx) = mpsc::channel(COMMAND_CAPACITY);
    let (event_tx, event_rx) = mpsc::channel(EVENT_CAPACITY);
    let handle = SessionHandle {
        sender: command_tx,
        diagnostics: diagnostics.clone(),
    };
    tokio::spawn(async move {
        let _ = event_tx.send(SessionEvent::Authenticated).await;
        let mut pending: HashMap<String, PendingAck> = HashMap::new();
        let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let session_started = Instant::now();
        let mut last_inbound = Instant::now();
        let mut close_code = None;
        let outcome: Result<()> = async {
            loop {
                tokio::select! {
                command=command_rx.recv()=>match command {
                    Some(Command::Subscribe{id,filters,response})=>{
                        let wire=envelope::request(&id,&filters);
                        match wire {
                            Ok(wire)=>{
                                let result=sink.send(Message::Text(wire.into())).await.map_err(|error|Error::Network(error.to_string()));
                                let stop=result.as_ref().err().map(ToString::to_string);
                                let _=response.send(result);
                                if let Some(message)=stop { break Err(Error::Network(message)); }
                            }
                            Err(error)=>{let _=response.send(Err(error));}
                        }
                    }
                    Some(Command::Close{id})=>{
                        sink.send(Message::Text(envelope::close(&id).into())).await.map_err(|error|Error::Network(error.to_string()))?;
                    }
                    Some(Command::Publish{event,admitted,response})=>{
                        let id=event.id.to_hex();
                        if pending.contains_key(&id) {
                            let _=admitted.send(Err(Error::Protocol("event is already awaiting acknowledgement".into())));
                            continue;
                        }
                        let kind=event.kind.as_u16();
                        if let Err(error)=sink.send(Message::Text(envelope::publish(&event).into())).await {
                            let message=error.to_string();
                            let _=admitted.send(Err(Error::Network(message.clone())));
                            break Err(Error::Network(message));
                        }
                        diagnostics.emit(DiagnosticEvent::PublishSent { event_id:id.clone(),kind,attempt:1 });
                        pending.insert(id,PendingAck { response,started:Instant::now() });
                        let _=admitted.send(Ok(()));
                    }
                    Some(Command::ForgetPending{event_id})=>{
                        pending.remove(&event_id);
                    }
                    Some(Command::Shutdown)|None=>{
                        let _=sink.send(Message::Close(None)).await;
                        break Ok(());
                    }
                },
                frame=stream.next()=>match frame {
                    Some(Ok(Message::Text(text)))=>{
                        last_inbound=Instant::now();
                        match RelayMessage::parse(&text) {
                            Ok(RelayMessage::Event{subscription,event})=>{ if event_tx.send(SessionEvent::Event{subscription,event}).await.is_err(){break Ok(());} }
                            Ok(RelayMessage::Eose(id))=>{let _=event_tx.send(SessionEvent::Eose(id)).await;}
                            Ok(RelayMessage::Ok{event_id,accepted,message})=>if let Some(pending_ack)=pending.remove(&event_id){
                                diagnostics.emit(DiagnosticEvent::PublishAcknowledged { event_id:event_id.clone(),accepted,duration_ms:elapsed_millis(pending_ack.started) });
                                let _=pending_ack.response.send(Ok(Ack{event_id,accepted,message}));
                            },
                            Ok(RelayMessage::Notice(message))=>{
                                if let Some(retry_after)=admission::rate_limit_retry_after(&message)
                                    && pending.len()==1
                                    && let Some((event_id,pending_ack))=pending.drain().next()
                                {
                                    diagnostics.emit(DiagnosticEvent::PublishUncertain { event_id,error_class:ErrorClass::RateLimited,duration_ms:elapsed_millis(pending_ack.started) });
                                    let fixed=admission::fixed_rate_limit_message(retry_after);
                                    let _=pending_ack.response.send(Err(Error::Network(fixed)));
                                }
                                let _=event_tx.send(SessionEvent::Notice(message)).await;
                            }
                            Ok(RelayMessage::Closed{subscription,message})=>{let _=event_tx.send(SessionEvent::Closed{subscription,message}).await;}
                            Ok(RelayMessage::Count{subscription,count})=>{let _=event_tx.send(SessionEvent::Count{subscription,count}).await;}
                            Ok(RelayMessage::Auth(_)|RelayMessage::Unknown(_))=>{},
                            Err(error)=>{let _=event_tx.send(SessionEvent::Notice(error.to_string())).await;}
                        }
                    }
                    Some(Ok(Message::Ping(payload)))=>{last_inbound=Instant::now();sink.send(Message::Pong(payload)).await.map_err(|error|Error::Network(error.to_string()))?;}
                    Some(Ok(Message::Pong(_)))=>last_inbound=Instant::now(),
                    Some(Ok(Message::Close(frame)))=>{
                        close_code=frame.map(|frame|u16::from(frame.code));
                        break Err(Error::Network("relay closed the connection".into()));
                    }
                    None=>break Err(Error::Network("relay closed the connection".into())),
                    Some(Ok(_))=>{},
                    Some(Err(error))=>break Err(Error::Network(error.to_string())),
                },
                _=heartbeat.tick()=>{
                    if last_inbound.elapsed()>Duration::from_secs(60){
                        diagnostics.emit(DiagnosticEvent::HeartbeatTimeout { last_inbound_age_ms:elapsed_millis(last_inbound) });
                        break Err(Error::Timeout("relay heartbeat".into()));
                    }
                    sink.send(Message::Ping(Vec::new().into())).await.map_err(|error|Error::Network(error.to_string()))?;
                }
                }
            }
        }
        .await;
        let (message, error_class) = outcome.err().map_or_else(
            || ("session stopped".into(), None),
            |error| {
                let class = ErrorClass::from_error(&error);
                (error.to_string(), Some(class))
            },
        );
        if let Some(error_class) = error_class {
            diagnostics.emit(DiagnosticEvent::Disconnected {
                error_class,
                close_code,
                connection_age_ms: elapsed_millis(session_started),
            });
        }
        for (event_id, pending_ack) in pending {
            diagnostics.emit(DiagnosticEvent::PublishUncertain {
                event_id,
                error_class: error_class.unwrap_or(ErrorClass::Closed),
                duration_ms: elapsed_millis(pending_ack.started),
            });
            let _ = pending_ack
                .response
                .send(Err(Error::Network(message.clone())));
        }
        let _ = event_tx.send(SessionEvent::Disconnected(message)).await;
    });
    Ok((handle, event_rx))
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

pub fn classify_auth_failure(message: &str) -> Error {
    let lower = message.to_ascii_lowercase();
    if lower.contains("timestamp") || lower.contains("expired") || lower.contains("clock") {
        Error::Auth(format!("clock-skew: {message}"))
    } else if lower.contains("banned")
        || lower.contains("restricted")
        || lower.contains("member")
        || lower.contains("allowlist")
    {
        Error::Access(message.to_owned())
    } else if lower.contains("rate-limit")
        || lower.contains("temporar")
        || lower.contains("timeout")
        || lower.contains("unavailable")
        || lower.contains("server error")
        || lower.contains("busy")
        || lower.contains("try again")
    {
        Error::Network(message.to_owned())
    } else {
        Error::Auth(message.to_owned())
    }
}
