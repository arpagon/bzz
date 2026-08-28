use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

use futures_util::{SinkExt as _, StreamExt as _};
use nostr::{Event, JsonUtil as _};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

pub struct FakeRelay {
    pub url: url::Url,
    stop: tokio::sync::oneshot::Sender<()>,
}

#[derive(Clone)]
struct FakeRelayConfig {
    event_accepted: bool,
    event_message: String,
    close_frames: Option<Arc<AtomicUsize>>,
    close_subscriptions_on_req: bool,
    request_times: Option<Arc<Mutex<Vec<Instant>>>>,
    rate_limit_first_request: bool,
    legacy_notice_on_event: bool,
    request_frames: Arc<AtomicUsize>,
    event_frames: Arc<AtomicUsize>,
}

impl Default for FakeRelayConfig {
    fn default() -> Self {
        Self {
            event_accepted: true,
            event_message: "stored".into(),
            close_frames: None,
            close_subscriptions_on_req: false,
            request_times: None,
            rate_limit_first_request: false,
            legacy_notice_on_event: false,
            request_frames: Arc::new(AtomicUsize::new(0)),
            event_frames: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl FakeRelay {
    #[allow(dead_code)]
    pub async fn start() -> Self {
        Self::start_with_event_ack(true, "stored").await
    }

    pub async fn start_with_event_ack(accepted: bool, message: &str) -> Self {
        Self::start_configured(FakeRelayConfig {
            event_accepted: accepted,
            event_message: message.to_owned(),
            ..FakeRelayConfig::default()
        })
        .await
    }

    #[allow(dead_code)]
    pub async fn start_closing_subscriptions() -> (Self, Arc<AtomicUsize>) {
        let close_frames = Arc::new(AtomicUsize::new(0));
        let relay = Self::start_configured(FakeRelayConfig {
            close_frames: Some(close_frames.clone()),
            close_subscriptions_on_req: true,
            ..FakeRelayConfig::default()
        })
        .await;
        (relay, close_frames)
    }

    #[allow(dead_code)]
    pub async fn start_acknowledging_closes() -> (Self, Arc<AtomicUsize>) {
        let close_frames = Arc::new(AtomicUsize::new(0));
        let relay = Self::start_configured(FakeRelayConfig {
            close_frames: Some(close_frames.clone()),
            ..FakeRelayConfig::default()
        })
        .await;
        (relay, close_frames)
    }

    #[allow(dead_code)]
    pub async fn start_recording_requests() -> (Self, Arc<Mutex<Vec<Instant>>>) {
        let request_times = Arc::new(Mutex::new(Vec::new()));
        let relay = Self::start_configured(FakeRelayConfig {
            request_times: Some(request_times.clone()),
            ..FakeRelayConfig::default()
        })
        .await;
        (relay, request_times)
    }

    #[allow(dead_code)]
    pub async fn start_rate_limiting_first_request() -> (Self, Arc<AtomicUsize>) {
        let request_frames = Arc::new(AtomicUsize::new(0));
        let relay = Self::start_configured(FakeRelayConfig {
            rate_limit_first_request: true,
            request_frames: request_frames.clone(),
            ..FakeRelayConfig::default()
        })
        .await;
        (relay, request_frames)
    }

    #[allow(dead_code)]
    pub async fn start_legacy_rate_limit_notice() -> (Self, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let event_frames = Arc::new(AtomicUsize::new(0));
        let request_frames = Arc::new(AtomicUsize::new(0));
        let relay = Self::start_configured(FakeRelayConfig {
            event_message: "rate-limited: quota exceeded; retry in 1s".into(),
            legacy_notice_on_event: true,
            request_frames: request_frames.clone(),
            event_frames: event_frames.clone(),
            ..FakeRelayConfig::default()
        })
        .await;
        (relay, event_frames, request_frames)
    }

    #[allow(dead_code)]
    pub async fn start_counting_event_ack(
        accepted: bool,
        message: &str,
    ) -> (Self, Arc<AtomicUsize>) {
        let event_frames = Arc::new(AtomicUsize::new(0));
        let relay = Self::start_configured(FakeRelayConfig {
            event_accepted: accepted,
            event_message: message.to_owned(),
            event_frames: event_frames.clone(),
            ..FakeRelayConfig::default()
        })
        .await;
        (relay, event_frames)
    }

    async fn start_configured(config: FakeRelayConfig) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _=&mut stop_rx=>break,
                    connection=listener.accept()=>{
                        let (stream,_)=connection.unwrap();
                        let config=config.clone();
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
                                                if let Some(request_times)=&config.request_times {
                                                    request_times.lock().unwrap().push(Instant::now());
                                                }
                                                let number=config.request_frames.fetch_add(1,Ordering::SeqCst);
                                                let id=value[1].as_str().unwrap();
                                                let response=if config.rate_limit_first_request && number==0 {
                                                    serde_json::json!(["CLOSED",id,"rate-limited: quota exceeded; retry in 1s"])
                                                } else if config.close_subscriptions_on_req {
                                                    serde_json::json!(["CLOSED",id,""])
                                                } else {
                                                    serde_json::json!(["EOSE",id])
                                                };
                                                socket.send(Message::Text(response.to_string().into())).await.unwrap();
                                            }
                                            "EVENT"=>{
                                                config.event_frames.fetch_add(1,Ordering::SeqCst);
                                                let event=Event::from_json(value[1].to_string()).unwrap();
                                                let response=if config.legacy_notice_on_event {
                                                    serde_json::json!(["NOTICE",config.event_message])
                                                } else {
                                                    serde_json::json!(["OK",event.id.to_hex(),config.event_accepted,config.event_message])
                                                };
                                                socket.send(Message::Text(response.to_string().into())).await.unwrap();
                                            }
                                            "CLOSE"=>{
                                                if let Some(close_frames)=&config.close_frames {
                                                    close_frames.fetch_add(1,Ordering::SeqCst);
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
