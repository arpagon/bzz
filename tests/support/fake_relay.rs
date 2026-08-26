use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use futures_util::{SinkExt as _, StreamExt as _};
use nostr::{Event, JsonUtil as _};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

pub struct FakeRelay {
    pub url: url::Url,
    stop: tokio::sync::oneshot::Sender<()>,
}

impl FakeRelay {
    #[allow(dead_code)]
    pub async fn start() -> Self {
        Self::start_with_event_ack(true, "stored").await
    }

    pub async fn start_with_event_ack(accepted: bool, message: &str) -> Self {
        Self::start_configured(accepted, message, None, false).await
    }

    #[allow(dead_code)]
    pub async fn start_closing_subscriptions() -> (Self, Arc<AtomicUsize>) {
        let close_frames = Arc::new(AtomicUsize::new(0));
        let relay = Self::start_configured(true, "stored", Some(close_frames.clone()), true).await;
        (relay, close_frames)
    }

    #[allow(dead_code)]
    pub async fn start_acknowledging_closes() -> (Self, Arc<AtomicUsize>) {
        let close_frames = Arc::new(AtomicUsize::new(0));
        let relay = Self::start_configured(true, "stored", Some(close_frames.clone()), false).await;
        (relay, close_frames)
    }

    async fn start_configured(
        accepted: bool,
        message: &str,
        close_frames: Option<Arc<AtomicUsize>>,
        close_subscriptions_on_req: bool,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel();
        let event_message = message.to_owned();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _=&mut stop_rx=>break,
                    connection=listener.accept()=>{
                        let (stream,_)=connection.unwrap();
                        let event_message = event_message.clone();
                        let close_frames = close_frames.clone();
                        tokio::spawn(async move {
                            let mut socket=accept_async(stream).await.unwrap();
                            socket.send(Message::Text(r#"["AUTH","fake-challenge"]"#.into())).await.unwrap();
                            while let Some(Ok(frame))=socket.next().await {
                                match frame {
                                    Message::Text(text)=>{
                                        let value:serde_json::Value=serde_json::from_str(&text).unwrap();
                                        match value[0].as_str().unwrap_or_default() {
                                            "AUTH"=>{
                                                let event=Event::from_json(value[1].to_string()).unwrap();
                                                event.verify().unwrap();
                                                socket.send(Message::Text(serde_json::json!(["OK",event.id.to_hex(),true,"authenticated"]).to_string().into())).await.unwrap();
                                            }
                                            "REQ"=>{
                                                let id=value[1].as_str().unwrap();
                                                let response = if close_subscriptions_on_req {
                                                    serde_json::json!(["CLOSED",id,""])
                                                } else {
                                                    serde_json::json!(["EOSE",id])
                                                };
                                                socket.send(Message::Text(response.to_string().into())).await.unwrap();
                                            }
                                            "EVENT"=>{let event=Event::from_json(value[1].to_string()).unwrap();socket.send(Message::Text(serde_json::json!(["OK",event.id.to_hex(),accepted,event_message]).to_string().into())).await.unwrap();}
                                            "CLOSE"=>{
                                                if let Some(close_frames) = &close_frames {
                                                    close_frames.fetch_add(1, Ordering::SeqCst);
                                                    let id=value[1].as_str().unwrap();
                                                    socket.send(Message::Text(serde_json::json!(["CLOSED",id,""]).to_string().into())).await.unwrap();
                                                }
                                            }
                                            _=>{}
                                        }
                                    }
                                    Message::Ping(payload)=>{socket.send(Message::Pong(payload)).await.unwrap();}
                                    Message::Close(_)=>break,
                                    _=>{}
                                }
                            }
                        });
                    }
                }
            }
        });
        Self {
            url: url::Url::parse(&format!("ws://{address}/")).unwrap(),
            stop: stop_tx,
        }
    }
    pub fn stop(self) {
        let _ = self.stop.send(());
    }
}
