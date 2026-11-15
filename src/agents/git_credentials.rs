//! Credentials for a private git remote (plan v0.15.0, M3;
//! `docs/agent-host-protocol.md`, section 8).
//!
//! An owner stores a user name and a token for one agent and one host. bzz
//! keeps them as it keeps an agent's key: sealed with NIP-44 to the owner's
//! own key, in a file of the data folder for the owner only, so they open
//! only while the owner is unlocked, on every key backend. They are never in
//! the settings file.
//!
//! bzz gives one to the agent's host only as the answer to its request
//! (`_bzz/credential`), for a host on that agent's list of allowed hosts.
//! What is sealed names the agent and the host, so a file moved to another
//! agent's folder, or under another host's name, opens for nobody.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    auth::signer::SignerHandle,
    error::{Error, Result},
};

/// The longest user name or token that is stored.
pub const MAX_FIELD_BYTES: usize = 4096;

/// A user name and a token, as HTTP Basic credentials carry them. Wiped
/// from memory when dropped. `Debug` prints neither.
#[derive(Clone, Eq, PartialEq)]
pub struct GitCredential {
    pub username: Zeroizing<String>,
    /// The token.
    pub password: Zeroizing<String>,
}

impl std::fmt::Debug for GitCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("GitCredential(…)")
    }
}

/// The file of one stored credential: the host in the clear, so that the
/// hosts can be listed without opening anything, and the rest sealed.
#[derive(Deserialize, Serialize)]
struct Stored {
    host: String,
    sealed: String,
}

/// What is sealed. `agent` and `host` are checked when it is opened.
#[derive(Deserialize, Serialize)]
struct Sealed {
    agent: Uuid,
    host: String,
    username: String,
    password: String,
}

impl Drop for Sealed {
    fn drop(&mut self) {
        use zeroize::Zeroize as _;
        self.username.zeroize();
        self.password.zeroize();
    }
}

/// `<keys>/agent-<id>.git/`: one agent's stored credentials.
pub fn folder(keys_dir: &Path, agent: Uuid) -> PathBuf {
    keys_dir.join(format!("agent-{agent}.git"))
}

fn file(keys_dir: &Path, agent: Uuid, host: &str) -> PathBuf {
    // A host may hold a colon, which not every filesystem takes.
    let name = hex::encode(&Sha256::digest(host.as_bytes())[..16]);
    folder(keys_dir, agent).join(format!("{name}.json"))
}

/// A host as the allow-list names it: `host` or `host:port`, lower case.
pub fn host_name(text: &str) -> Result<String> {
    crate::agents::manage::parse_host(text)
}

fn field_ok(value: &str) -> bool {
    value.len() <= MAX_FIELD_BYTES && !value.chars().any(char::is_control)
}

/// Stores `username` and `password` for `agent` and `host`, in place of
/// what was stored for them before. It is opened once again before this
/// returns: a credential that reads back is the only one reported stored.
pub async fn set(
    keys_dir: &Path,
    owner: &SignerHandle,
    agent: Uuid,
    host: &str,
    username: &str,
    password: &str,
) -> Result<()> {
    let host = host_name(host)?;
    if username.is_empty() && password.is_empty() {
        return Err(Error::Config(
            "a credential needs a user name, a token, or both".into(),
        ));
    }
    if !field_ok(username) || !field_ok(password) || username.contains(':') {
        return Err(Error::Config(format!(
            "a user name and a token are each one line of at most {MAX_FIELD_BYTES} bytes, and a user name holds no colon"
        )));
    }
    let plain = Zeroizing::new(
        serde_json::to_string(&Sealed {
            agent,
            host: host.clone(),
            username: username.to_owned(),
            password: password.to_owned(),
        })
        .map_err(|error| Error::Serialization(error.to_string()))?,
    );
    let sealed = owner.encrypt_self(plain.to_string()).await?;
    let directory = folder(keys_dir, agent);
    std::fs::create_dir_all(&directory).map_err(|error| Error::io(&directory, error))?;
    crate::paths::set_private_permissions(&directory)?;
    let path = file(keys_dir, agent, &host);
    let temporary = path.with_extension("json.tmp");
    let text = serde_json::to_string(&Stored {
        host: host.clone(),
        sealed,
    })
    .map_err(|error| Error::Serialization(error.to_string()))?;
    {
        let _ = std::fs::remove_file(&temporary);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|error| Error::io(&temporary, error))?;
        std::io::Write::write_all(&mut file, text.as_bytes())
            .map_err(|error| Error::io(&temporary, error))?;
        file.sync_all()
            .map_err(|error| Error::io(&temporary, error))?;
    }
    // Read back before it takes the place of the one before.
    if let Err(error) = open(&temporary, owner, agent, &host).await {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    std::fs::rename(&temporary, &path).map_err(|error| Error::io(&path, error))
}

async fn open(path: &Path, owner: &SignerHandle, agent: Uuid, host: &str) -> Result<GitCredential> {
    let corrupt = || Error::IdentityCorrupt(format!("the credential stored for {host} is damaged"));
    let text = std::fs::read_to_string(path).map_err(|error| Error::io(path, error))?;
    let stored = serde_json::from_str::<Stored>(&text).map_err(|_| corrupt())?;
    let plain = Zeroizing::new(owner.decrypt_self(stored.sealed).await.map_err(|_| {
        Error::Auth(format!(
            "the credential stored for {host} does not open with the agent's owner"
        ))
    })?);
    let sealed = serde_json::from_str::<Sealed>(&plain).map_err(|_| corrupt())?;
    // Sealed for this agent and this host, whatever the file is called and
    // wherever it lies.
    if sealed.agent != agent || sealed.host != host || stored.host != host {
        return Err(Error::IdentityCorrupt(format!(
            "the credential stored for {host} was sealed for another agent or host"
        )));
    }
    Ok(GitCredential {
        username: Zeroizing::new(sealed.username.clone()),
        password: Zeroizing::new(sealed.password.clone()),
    })
}

/// The credential stored for `agent` and `host`, or `None`.
pub async fn get(
    keys_dir: &Path,
    owner: &SignerHandle,
    agent: Uuid,
    host: &str,
) -> Result<Option<GitCredential>> {
    let host = host_name(host)?;
    let path = file(keys_dir, agent, &host);
    if !path.is_file() {
        return Ok(None);
    }
    open(&path, owner, agent, &host).await.map(Some)
}

/// The hosts `agent` has a credential stored for. Nothing is opened.
pub fn hosts(keys_dir: &Path, agent: Uuid) -> Vec<String> {
    let mut hosts = std::fs::read_dir(folder(keys_dir, agent))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|kind| kind == "json"))
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .filter_map(|text| serde_json::from_str::<Stored>(&text).ok())
        .map(|stored| stored.host)
        .collect::<Vec<_>>();
    hosts.sort();
    hosts.dedup();
    hosts
}

/// Removes the credential stored for `agent` and `host`. `false`: there
/// was none.
pub fn remove(keys_dir: &Path, agent: Uuid, host: &str) -> Result<bool> {
    let path = file(keys_dir, agent, &host_name(host)?);
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(&path).map_err(|error| Error::io(&path, error))?;
    // The folder goes with its last credential.
    let _ = std::fs::remove_dir(folder(keys_dir, agent));
    Ok(true)
}

/// Removes every credential stored for `agent`: it is being deleted.
/// Returns how many there were.
pub fn remove_all(keys_dir: &Path, agent: Uuid) -> Result<usize> {
    let directory = folder(keys_dir, agent);
    if !directory.exists() {
        return Ok(0);
    }
    let count = hosts(keys_dir, agent).len();
    std::fs::remove_dir_all(&directory).map_err(|error| Error::io(&directory, error))?;
    Ok(count)
}

/// Where the runtime gets a credential at the moment a host asks.
#[async_trait]
pub trait GitCredentials: Send + Sync {
    async fn get(&self, agent: Uuid, host: &str) -> Result<Option<GitCredential>>;
}

/// The stored credentials, opened with the owner's own key.
pub struct SealedGitCredentials {
    pub keys_dir: PathBuf,
    pub owner: SignerHandle,
}

#[async_trait]
impl GitCredentials for SealedGitCredentials {
    async fn get(&self, agent: Uuid, host: &str) -> Result<Option<GitCredential>> {
        get(&self.keys_dir, &self.owner, agent, host).await
    }
}

#[cfg(test)]
mod tests {
    use nostr::Keys;

    use super::*;

    const TOKEN: &str = "ghp_NotARealTokenOnlyAMarkerForThisTest0000";

    #[tokio::test]
    async fn a_credential_is_sealed_for_one_agent_and_one_host() {
        let temporary = tempfile::TempDir::new().unwrap();
        let keys = temporary.path();
        let owner = SignerHandle::spawn(Keys::generate());
        let (scout, ranger) = (Uuid::new_v4(), Uuid::new_v4());
        assert_eq!(get(keys, &owner, scout, "github.com").await.unwrap(), None);
        assert!(hosts(keys, scout).is_empty());

        set(keys, &owner, scout, "GitHub.com", "scout-bot", TOKEN)
            .await
            .unwrap();
        set(keys, &owner, scout, "git.example.org:8443", "", "t2")
            .await
            .unwrap();
        let stored = get(keys, &owner, scout, "github.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (stored.username.as_str(), stored.password.as_str()),
            ("scout-bot", TOKEN)
        );
        assert!(!format!("{stored:?}").contains(TOKEN));
        // Listed by host, and only by host.
        assert_eq!(hosts(keys, scout), ["git.example.org:8443", "github.com"]);
        // Another agent has none, and neither has another host.
        assert_eq!(get(keys, &owner, ranger, "github.com").await.unwrap(), None);
        assert_eq!(get(keys, &owner, scout, "gitlab.com").await.unwrap(), None);
        assert!(hosts(keys, ranger).is_empty());

        // Nothing on disk holds the token or the user name, and the files
        // are the owner's alone.
        for entry in std::fs::read_dir(folder(keys, scout)).unwrap().flatten() {
            let text = std::fs::read_to_string(entry.path()).unwrap();
            assert!(
                !text.contains(TOKEN) && !text.contains("scout-bot"),
                "{text}"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                let mode = entry.metadata().unwrap().permissions().mode();
                assert_eq!(mode & 0o777, 0o600);
            }
        }

        // A file copied to another agent's folder opens for nobody, and so
        // does one put under another host's name.
        std::fs::create_dir_all(folder(keys, ranger)).unwrap();
        std::fs::copy(
            file(keys, scout, "github.com"),
            file(keys, ranger, "github.com"),
        )
        .unwrap();
        assert!(get(keys, &owner, ranger, "github.com").await.is_err());
        std::fs::copy(
            file(keys, scout, "github.com"),
            file(keys, scout, "evil.example"),
        )
        .unwrap();
        assert!(get(keys, &owner, scout, "evil.example").await.is_err());
        std::fs::remove_file(file(keys, scout, "evil.example")).unwrap();
        // Another owner cannot open it.
        let stranger = SignerHandle::spawn(Keys::generate());
        assert!(get(keys, &stranger, scout, "github.com").await.is_err());

        // Storing again replaces; what cannot be a credential is refused.
        set(keys, &owner, scout, "github.com", "scout-bot", "second")
            .await
            .unwrap();
        assert_eq!(
            get(keys, &owner, scout, "github.com")
                .await
                .unwrap()
                .unwrap()
                .password
                .as_str(),
            "second"
        );
        for (host, username, password) in [
            ("github.com", "", ""),
            ("github.com", "a:b", "t"),
            ("github.com", "a", "two\nlines"),
            ("https://github.com", "a", "t"),
            ("*.github.com", "a", "t"),
        ] {
            assert!(
                set(keys, &owner, scout, host, username, password)
                    .await
                    .is_err(),
                "{host} {username:?}"
            );
        }
        assert_eq!(hosts(keys, scout).len(), 2, "nothing was added or lost");

        // Removed one by one, and all at once when the agent is deleted.
        assert!(remove(keys, scout, "github.com").unwrap());
        assert!(!remove(keys, scout, "github.com").unwrap());
        assert_eq!(get(keys, &owner, scout, "github.com").await.unwrap(), None);
        assert_eq!(remove_all(keys, scout).unwrap(), 1);
        assert!(!folder(keys, scout).exists());
        assert_eq!(remove_all(keys, scout).unwrap(), 0);
    }
}
