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
    /// NIP-OA owner `auth` tag (compact JSON) for `agent`, made by this key.
    OwnerAuthTag {
        agent: PublicKey,
        conditions: String,
        response: oneshot::Sender<Result<String>>,
    },
    /// NIP-AE memory `d` tag for `slug`, keyed by this (agent) key and `owner`.
    MemoryDTag {
        owner: PublicKey,
        slug: String,
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
                    Command::OwnerAuthTag {
                        agent,
                        conditions,
                        response,
                    } => {
                        let result = buzz_sdk::nip_oa::compute_auth_tag(&keys, &agent, &conditions)
                            .map_err(|error| {
                                Error::Auth(format!("owner authorization failed: {error}"))
                            });
                        let _ = response.send(result);
                    }
                    Command::MemoryDTag {
                        owner,
                        slug,
                        response,
                    } => {
                        let key = buzz_core::engram::conversation_key(keys.secret_key(), &owner);
                        let _ = response.send(Ok(buzz_core::engram::d_tag(&key, &slug)));
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

    /// A signer whose key lives in Buzz Desktop. `pubkey` is the configured
    /// identity; every returned signature is checked against it. Requests run
    /// concurrently so an approval held for a sensitive kind cannot block
    /// relay AUTH.
    pub fn spawn_remote(
        client: std::sync::Arc<crate::auth::provider::ProviderClient>,
        public_key: PublicKey,
    ) -> Self {
        let (sender, mut receiver) = mpsc::channel(QUEUE);
        tokio::spawn(async move {
            while let Some(command) = receiver.recv().await {
                if let Command::Lock(response) = command {
                    let _ = response.send(());
                    break;
                }
                let client = client.clone();
                tokio::spawn(async move { remote_command(&client, public_key, command).await });
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

    /// The NIP-OA `auth` tag that lets `agent` act for this (owner) key.
    pub async fn owner_auth_tag(&self, agent: PublicKey, conditions: &str) -> Result<String> {
        let (response, received) = oneshot::channel();
        self.sender
            .send(Command::OwnerAuthTag {
                agent,
                conditions: conditions.to_owned(),
                response,
            })
            .await
            .map_err(|_| Error::Locked("signer is locked".into()))?;
        received
            .await
            .map_err(|_| Error::Locked("signer stopped".into()))?
    }

    /// NIP-AE memory record `d` tag for `slug` (this key is the agent's).
    pub async fn memory_d_tag(&self, owner: PublicKey, slug: &str) -> Result<String> {
        let (response, received) = oneshot::channel();
        self.sender
            .send(Command::MemoryDTag {
                owner,
                slug: slug.to_owned(),
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

fn nip98_builder(method: &str, url: &str, body: Option<&[u8]>) -> Result<EventBuilder> {
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
    Ok(EventBuilder::new(nostr::Kind::Custom(27_235), "").tags(tags))
}

fn sign_nip98(keys: &Keys, method: &str, url: &str, body: Option<&[u8]>) -> Result<String> {
    let event = nip98_builder(method, url, body)?
        .sign_with_keys(keys)
        .map_err(|error| Error::Auth(format!("NIP-98 signing failed: {error}")))?;
    Ok(format!("Nostr {}", STANDARD.encode(event.as_json())))
}

/// Send a complete unsigned event to Buzz Desktop and accept the result only
/// if it is a valid signature by `pubkey` over exactly what was sent.
async fn remote_sign(
    client: &crate::auth::provider::ProviderClient,
    pubkey: PublicKey,
    builder: EventBuilder,
) -> Result<Event> {
    let unsigned = builder.build(pubkey);
    let request = serde_json::json!({
        "pubkey": pubkey.to_hex(),
        "created_at": unsigned.created_at.as_secs(),
        "kind": unsigned.kind.as_u16(),
        "tags": unsigned.tags,
        "content": unsigned.content,
    });
    let signed = client
        .call("sign_event", &[request.to_string()])
        .await
        .map_err(Error::from)?;
    let event = Event::from_json(signed)
        .map_err(|_| Error::Auth("Buzz Desktop returned a malformed event".into()))?;
    event
        .verify()
        .map_err(|_| Error::Auth("Buzz Desktop returned an invalid signature".into()))?;
    if event.pubkey != pubkey
        || event.kind != unsigned.kind
        || event.created_at != unsigned.created_at
        || event.content != unsigned.content
        || event.tags != unsigned.tags
    {
        return Err(Error::Auth(
            "Buzz Desktop signed a different event than requested".into(),
        ));
    }
    Ok(event)
}

async fn remote_command(
    client: &crate::auth::provider::ProviderClient,
    pubkey: PublicKey,
    command: Command,
) {
    match command {
        Command::Sign(builder, response) => {
            let _ = response.send(remote_sign(client, pubkey, builder).await);
        }
        Command::Auth {
            challenge,
            relay,
            response,
        } => {
            let _ = response
                .send(remote_sign(client, pubkey, EventBuilder::auth(challenge, relay)).await);
        }
        Command::Nip98 {
            method,
            url,
            body,
            response,
        } => {
            let result = async {
                let event = remote_sign(
                    client,
                    pubkey,
                    nip98_builder(&method, &url, body.as_deref())?,
                )
                .await?;
                Ok(format!("Nostr {}", STANDARD.encode(event.as_json())))
            }
            .await;
            let _ = response.send(result);
        }
        Command::EncryptSelf {
            plaintext,
            response,
        } => {
            let result = client
                .call("nip44_encrypt", &[pubkey.to_hex(), plaintext])
                .await
                .map_err(Error::from);
            let _ = response.send(result);
        }
        Command::DecryptSelf {
            ciphertext,
            response,
        } => {
            let result = client
                .call("nip44_decrypt", &[pubkey.to_hex(), ciphertext])
                .await
                .map_err(|_| Error::Auth("read-state decryption failed".into()));
            let _ = response.send(result);
        }
        Command::OwnerAuthTag { response, .. } | Command::MemoryDTag { response, .. } => {
            let _ = response.send(Err(Error::Unsupported(
                "a Buzz Desktop identity cannot host local agents".into(),
            )));
        }
        Command::Lock(response) => {
            let _ = response.send(());
        }
    }
}
