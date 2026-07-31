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
    error::{Error, Result},
    protocol::envelope::{self, RelayMessage},
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
        response: oneshot::Sender<Result<Ack>>,
    },
    Shutdown,
}

#[derive(Clone)]
pub struct SessionHandle {
    sender: mpsc::Sender<Command>,
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
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::Publish {
                event,
                response: sender,
            })
            .await
            .map_err(|_| Error::Network("session is offline".into()))?;
        tokio::time::timeout(Duration::from_secs(25), receiver)
            .await
            .map_err(|_| Error::Timeout("relay acknowledgement".into()))?
            .map_err(|_| Error::Network("session stopped".into()))?
    }

    pub async fn shutdown(&self) {
        let _ = self.sender.send(Command::Shutdown).await;
    }
}

pub async fn connect(
    relay: Url,
    signer: SignerHandle,
) -> Result<(SessionHandle, mpsc::Receiver<SessionEvent>)> {
    let (socket, _response) = connect_async(relay.as_str())
        .await
        .map_err(|error| Error::Network(error.to_string()))?;
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

    let (command_tx, mut command_rx) = mpsc::channel(COMMAND_CAPACITY);
    let (event_tx, event_rx) = mpsc::channel(EVENT_CAPACITY);
    let handle = SessionHandle { sender: command_tx };
    tokio::spawn(async move {
        let _ = event_tx.send(SessionEvent::Authenticated).await;
        let mut pending: HashMap<String, oneshot::Sender<Result<Ack>>> = HashMap::new();
        let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last_inbound = Instant::now();
        let outcome: Result<()> = async {
            loop {
                tokio::select! {
                biased;
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
                    Some(Command::Publish{event,response})=>{
                        let id=event.id.to_hex();
                        if pending.contains_key(&id) { let _=response.send(Err(Error::Protocol("event is already awaiting acknowledgement".into()))); continue; }
                        sink.send(Message::Text(envelope::publish(&event).into())).await.map_err(|error|Error::Network(error.to_string()))?;
                        pending.insert(id,response);
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
                            Ok(RelayMessage::Ok{event_id,accepted,message})=>if let Some(response)=pending.remove(&event_id){let _=response.send(Ok(Ack{event_id,accepted,message}));},
                            Ok(RelayMessage::Notice(message))=>{
                                if message.starts_with("rate-limited:") && pending.len()==1
                                    && let Some((_,response))=pending.drain().next()
                                {
                                    let _=response.send(Err(Error::Network(message.clone())));
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
                    Some(Ok(Message::Close(_)))|None=>break Err(Error::Network("relay closed the connection".into())),
                    Some(Ok(_))=>{},
                    Some(Err(error))=>break Err(Error::Network(error.to_string())),
                },
                _=heartbeat.tick()=>{
                    if last_inbound.elapsed()>Duration::from_secs(60){break Err(Error::Timeout("relay heartbeat".into()));}
                    sink.send(Message::Ping(Vec::new().into())).await.map_err(|error|Error::Network(error.to_string()))?;
                }
                }
            }
        }
        .await;
        let message = outcome
            .err()
            .map_or_else(|| "session stopped".into(), |error| error.to_string());
        for (_, response) in pending {
            let _ = response.send(Err(Error::Network(message.clone())));
        }
        let _ = event_tx.send(SessionEvent::Disconnected(message)).await;
    });
    Ok((handle, event_rx))
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
