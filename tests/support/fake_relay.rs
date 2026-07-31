use futures_util::{SinkExt as _, StreamExt as _};
use nostr::{Event, JsonUtil as _};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

pub struct FakeRelay {
    pub url: url::Url,
    stop: tokio::sync::oneshot::Sender<()>,
}

impl FakeRelay {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _=&mut stop_rx=>break,
                    accepted=listener.accept()=>{
                        let (stream,_)=accepted.unwrap();
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
                                            "REQ"=>{let id=value[1].as_str().unwrap();socket.send(Message::Text(serde_json::json!(["EOSE",id]).to_string().into())).await.unwrap();}
                                            "EVENT"=>{let event=Event::from_json(value[1].to_string()).unwrap();socket.send(Message::Text(serde_json::json!(["OK",event.id.to_hex(),true,"stored"]).to_string().into())).await.unwrap();}
                                            "CLOSE"=>{}
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
