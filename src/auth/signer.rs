use base64::{Engine as _, engine::general_purpose::STANDARD};
use nostr::{
    Event, EventBuilder, JsonUtil as _, Keys, PublicKey, RelayUrl, Tag,
    nips::nip44::{self, Version},
};
use sha2::{Digest as _, Sha256};
use tokio::sync::{mpsc, oneshot};

use crate::error::{Error, Result};

const QUEUE: usize = 64;

#[derive(Clone)]
pub struct SignerHandle {
    public_key: PublicKey,
    sender: mpsc::Sender<Command>,
}

enum Command {
    Sign(EventBuilder, oneshot::Sender<Result<Event>>),
    Auth {
        challenge: String,
        relay: RelayUrl,
        response: oneshot::Sender<Result<Event>>,
    },
    Nip98 {
        method: String,
        url: String,
        body: Option<Vec<u8>>,
        response: oneshot::Sender<Result<String>>,
    },
    EncryptSelf {
        plaintext: String,
        response: oneshot::Sender<Result<String>>,
    },
    DecryptSelf {
        ciphertext: String,
        response: oneshot::Sender<Result<String>>,
    },
    Lock(oneshot::Sender<()>),
}

impl SignerHandle {
    pub fn spawn(keys: Keys) -> Self {
        let public_key = keys.public_key();
        let (sender, mut receiver) = mpsc::channel(QUEUE);
        tokio::spawn(async move {
            let keys = keys;
            while let Some(command) = receiver.recv().await {
                match command {
                    Command::Sign(builder, response) => {
                        let result = builder
                            .sign_with_keys(&keys)
                            .map_err(|error| Error::Auth(format!("event signing failed: {error}")));
                        let _ = response.send(result);
                    }
                    Command::Auth {
                        challenge,
                        relay,
                        response,
                    } => {
                        let result = EventBuilder::auth(challenge, relay)
                            .sign_with_keys(&keys)
                            .map_err(|error| Error::Auth(format!("AUTH signing failed: {error}")));
                        let _ = response.send(result);
                    }
                    Command::Nip98 {
                        method,
                        url,
                        body,
                        response,
                    } => {
                        let result = sign_nip98(&keys, &method, &url, body.as_deref());
                        let _ = response.send(result);
                    }
                    Command::EncryptSelf {
                        plaintext,
                        response,
                    } => {
                        let result = nip44::encrypt(
                            keys.secret_key(),
                            &keys.public_key(),
                            plaintext,
                            Version::V2,
                        )
                        .map_err(|error| {
                            Error::Auth(format!("read-state encryption failed: {error}"))
                        });
                        let _ = response.send(result);
                    }
                    Command::DecryptSelf {
                        ciphertext,
                        response,
                    } => {
                        let result =
                            nip44::decrypt(keys.secret_key(), &keys.public_key(), ciphertext)
                                .map_err(|_| Error::Auth("read-state decryption failed".into()));
                        let _ = response.send(result);
                    }
                    Command::Lock(response) => {
                        let _ = response.send(());
                        break;
                    }
                }
            }
        });
        Self { public_key, sender }
    }

    pub const fn public_key(&self) -> PublicKey {
        self.public_key
    }

    pub async fn sign(&self, builder: EventBuilder) -> Result<Event> {
        let (response, received) = oneshot::channel();
        self.sender
            .send(Command::Sign(builder, response))
            .await
            .map_err(|_| Error::Locked("signer is locked".into()))?;
        received
            .await
            .map_err(|_| Error::Locked("signer stopped".into()))?
    }

    pub async fn auth(&self, challenge: &str, relay: RelayUrl) -> Result<Event> {
        if challenge.len() > 1024 {
            return Err(Error::Protocol("AUTH challenge is too large".into()));
        }
        let (response, received) = oneshot::channel();
        self.sender
            .send(Command::Auth {
                challenge: challenge.to_owned(),
                relay,
                response,
            })
            .await
            .map_err(|_| Error::Locked("signer is locked".into()))?;
        received
            .await
            .map_err(|_| Error::Locked("signer stopped".into()))?
    }

    pub async fn nip98_header(
        &self,
        method: &str,
        url: &str,
        body: Option<&[u8]>,
    ) -> Result<String> {
        let (response, received) = oneshot::channel();
        self.sender
            .send(Command::Nip98 {
                method: method.to_ascii_uppercase(),
                url: url.to_owned(),
                body: body.map(ToOwned::to_owned),
                response,
            })
            .await
            .map_err(|_| Error::Locked("signer is locked".into()))?;
        received
            .await
            .map_err(|_| Error::Locked("signer stopped".into()))?
    }

    pub async fn encrypt_self(&self, plaintext: String) -> Result<String> {
        let (response, received) = oneshot::channel();
        self.sender
            .send(Command::EncryptSelf {
                plaintext,
                response,
            })
            .await
            .map_err(|_| Error::Locked("signer is locked".into()))?;
        received
            .await
            .map_err(|_| Error::Locked("signer stopped".into()))?
    }

    pub async fn decrypt_self(&self, ciphertext: String) -> Result<String> {
        let (response, received) = oneshot::channel();
        self.sender
            .send(Command::DecryptSelf {
                ciphertext,
                response,
            })
            .await
            .map_err(|_| Error::Locked("signer is locked".into()))?;
        received
            .await
            .map_err(|_| Error::Locked("signer stopped".into()))?
    }

    pub async fn lock(&self) {
        let (response, received) = oneshot::channel();
        if self.sender.send(Command::Lock(response)).await.is_ok() {
            let _ = received.await;
        }
    }
}

fn sign_nip98(keys: &Keys, method: &str, url: &str, body: Option<&[u8]>) -> Result<String> {
    let mut tags = vec![
        Tag::parse(["u", url]).map_err(|error| Error::Protocol(error.to_string()))?,
        Tag::parse(["method", method]).map_err(|error| Error::Protocol(error.to_string()))?,
        Tag::parse(["nonce", &uuid::Uuid::new_v4().to_string()])
            .map_err(|error| Error::Protocol(error.to_string()))?,
    ];
    if let Some(body) = body {
        let digest = hex::encode(Sha256::digest(body));
        tags.push(
            Tag::parse(["payload", &digest]).map_err(|error| Error::Protocol(error.to_string()))?,
        );
    }
    let event = EventBuilder::new(nostr::Kind::Custom(27_235), "")
        .tags(tags)
        .sign_with_keys(keys)
        .map_err(|error| Error::Auth(format!("NIP-98 signing failed: {error}")))?;
    Ok(format!("Nostr {}", STANDARD.encode(event.as_json())))
}
