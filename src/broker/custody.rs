//! Hosted agent key custody. bzz creates each agent key, seals it with NIP-44
//! to its owner's own key (so it opens only while the owner is unlocked, on
//! every backend), verifies the sealed copy by reading it back, and never
//! hands it to the agent. The owner's NIP-OA tag is made once, at creation.

use std::path::{Path, PathBuf};

use nostr::{Keys, PublicKey};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    auth::signer::SignerHandle,
    broker::host::{HostedAgent, credential_hash, hosted_actions, new_credential},
    config::HostedAgentConfig,
    error::{Error, Result},
};

fn key_file(keys_dir: &Path, id: Uuid) -> PathBuf {
    keys_dir.join(format!("agent-{id}.nip44"))
}

fn write_sealed(path: &Path, sealed: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| Error::io(parent, error))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|error| Error::io(path, error))?;
    std::io::Write::write_all(&mut file, sealed.as_bytes()).map_err(|error| Error::io(path, error))
}

async fn open_sealed(path: &Path, owner: &SignerHandle, pubkey: &str) -> Result<Keys> {
    let sealed = std::fs::read_to_string(path).map_err(|_| {
        Error::IdentityMissing(format!(
            "sealed agent key {} is absent",
            path.file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
        ))
    })?;
    let secret = Zeroizing::new(
        owner
            .decrypt_self(sealed)
            .await
            .map_err(|_| Error::Auth("the agent key does not open with its owner".into()))?,
    );
    let keys = Keys::parse(secret.as_str())
        .map_err(|_| Error::IdentityCorrupt("sealed agent key is invalid".into()))?;
    if keys.public_key().to_hex() != pubkey {
        return Err(Error::IdentityCorrupt(
            "sealed agent key does not match its configured public key".into(),
        ));
    }
    Ok(keys)
}

/// A new agent and its bearer credential, which is shown once and never
/// stored (only its hash is).
pub async fn create(
    keys_dir: &Path,
    owner: &SignerHandle,
    owner_identity: Uuid,
    community: Uuid,
    label: &str,
) -> Result<(HostedAgentConfig, String)> {
    let label = label.trim();
    if label.is_empty() || label.chars().count() > 64 {
        return Err(Error::Config(
            "an agent label must be 1 to 64 characters".into(),
        ));
    }
    let keys = Keys::generate();
    let id = Uuid::new_v4();
    let secret = Zeroizing::new(keys.secret_key().to_secret_hex());
    let sealed = owner.encrypt_self(secret.to_string()).await?;
    let path = key_file(keys_dir, id);
    write_sealed(&path, &sealed)?;
    let pubkey = keys.public_key().to_hex();
    // Read-back verification before anything refers to the key.
    if let Err(error) = open_sealed(&path, owner, &pubkey).await {
        let _ = std::fs::remove_file(&path);
        return Err(error);
    }
    let auth_tag = owner.owner_auth_tag(keys.public_key(), "").await?;
    let credential = new_credential();
    Ok((
        HostedAgentConfig {
            id,
            label: label.to_owned(),
            pubkey,
            community,
            owner_identity,
            auth_tag,
            credential_hash: credential_hash(&credential),
            // Reads and memory addressing by default; writes are opt-in.
            allowed: vec!["channel.read".into(), "storage.address".into()],
            paused: false,
            local: None,
        },
        credential,
    ))
}

/// The agent's signer; its key never leaves this actor.
pub async fn unlock(
    keys_dir: &Path,
    owner: &SignerHandle,
    agent: &HostedAgentConfig,
) -> Result<SignerHandle> {
    let tag = crate::broker::relay::auth_tag(&agent.auth_tag)?;
    Ok(SignerHandle::spawn_with_auth_tag(
        open_sealed(&key_file(keys_dir, agent.id), owner, &agent.pubkey).await?,
        Some(tag),
    ))
}

/// Replaces the credential; the old one stops resolving immediately.
pub fn rotate(agent: &mut HostedAgentConfig) -> String {
    let credential = new_credential();
    agent.credential_hash = credential_hash(&credential);
    credential
}

pub fn delete(keys_dir: &Path, agent: &HostedAgentConfig) -> Result<()> {
    let path = key_file(keys_dir, agent.id);
    if path.exists() {
        std::fs::remove_file(&path).map_err(|error| Error::io(path, error))?;
    }
    Ok(())
}

/// Validates and sets the agent's allowed actions.
pub fn set_allowed(agent: &mut HostedAgentConfig, actions: &[String]) -> Result<()> {
    let hosted = hosted_actions();
    if let Some(unknown) = actions.iter().find(|action| !hosted.contains(*action)) {
        return Err(Error::Config(format!(
            "unknown or unhosted action {unknown}; hosted: {}",
            hosted.into_iter().collect::<Vec<_>>().join(", ")
        )));
    }
    let mut actions = actions.to_vec();
    actions.sort();
    actions.dedup();
    agent.allowed = actions;
    Ok(())
}

pub fn hosted(agent: &HostedAgentConfig) -> HostedAgent {
    HostedAgent {
        id: agent.id,
        label: agent.label.clone(),
        pubkey: agent.pubkey.clone(),
        credential_hash: agent.credential_hash.clone(),
        allowed: agent.allowed.iter().cloned().collect(),
        paused: agent.paused,
    }
}

pub fn owner_pubkey(auth_tag: &str, agent: &str) -> Result<PublicKey> {
    let agent = PublicKey::from_hex(agent)
        .map_err(|_| Error::IdentityCorrupt("agent public key is invalid".into()))?;
    buzz_sdk::nip_oa::verify_auth_tag(auth_tag, &agent)
        .map_err(|error| Error::IdentityCorrupt(format!("owner authorization: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sealed_key_round_trips_only_with_its_owner_and_never_leaves() {
        let temporary = tempfile::TempDir::new().unwrap();
        let owner_keys = Keys::generate();
        let owner = SignerHandle::spawn(owner_keys.clone());
        let (agent, credential) = create(
            temporary.path(),
            &owner,
            Uuid::new_v4(),
            Uuid::new_v4(),
            " helper ",
        )
        .await
        .unwrap();
        assert_eq!(agent.label, "helper");
        assert_eq!(agent.credential_hash, credential_hash(&credential));
        assert!(!agent.credential_hash.contains(&credential));
        // The owner tag verifies for this agent and names the owner.
        assert_eq!(
            owner_pubkey(&agent.auth_tag, &agent.pubkey).unwrap(),
            owner_keys.public_key()
        );
        // The file on disk is sealed: the secret is not in it.
        let signer = unlock(temporary.path(), &owner, &agent).await.unwrap();
        assert_eq!(signer.public_key().to_hex(), agent.pubkey);
        let on_disk = std::fs::read_to_string(key_file(temporary.path(), agent.id)).unwrap();
        let serialized = toml::to_string(&agent).unwrap();
        for text in [&on_disk, &serialized] {
            assert!(!text.contains(&credential));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(key_file(temporary.path(), agent.id))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // Another owner cannot open it.
        let stranger = SignerHandle::spawn(Keys::generate());
        assert!(unlock(temporary.path(), &stranger, &agent).await.is_err());
        // Rotation invalidates the old credential hash.
        let mut rotated = agent.clone();
        let fresh = rotate(&mut rotated);
        assert_ne!(fresh, credential);
        assert_ne!(rotated.credential_hash, agent.credential_hash);
        // Policy accepts only hosted actions.
        assert!(set_allowed(&mut rotated, &["agents.create".into()]).is_err());
        set_allowed(
            &mut rotated,
            &["message.post".into(), "channel.read".into()],
        )
        .unwrap();
        assert_eq!(rotated.allowed, vec!["channel.read", "message.post"]);
        delete(temporary.path(), &rotated).unwrap();
        assert!(unlock(temporary.path(), &owner, &rotated).await.is_err());
    }
}
