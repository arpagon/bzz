pub mod encrypted_file;
pub mod keychain;
pub mod signer;

use std::{fs, io::Read as _, path::PathBuf};

use nostr::Keys;
use secrecy::{ExposeSecret as _, SecretString};
use uuid::Uuid;
use zeroize::{Zeroize as _, Zeroizing};

use crate::{
    config::{Config, IdentityConfig, KeyBackend},
    error::{Error, Result},
    paths::Paths,
};

pub struct IdentityManager<'a> {
    paths: &'a Paths,
}

impl<'a> IdentityManager<'a> {
    pub const fn new(paths: &'a Paths) -> Self {
        Self { paths }
    }

    pub fn create(
        &self,
        config: &mut Config,
        label: String,
        backend: KeyBackend,
        passphrase: Option<&SecretString>,
    ) -> Result<IdentityConfig> {
        self.store_keys(config, label, backend, Keys::generate(), passphrase)
    }

    pub fn import(
        &self,
        config: &mut Config,
        label: String,
        backend: KeyBackend,
        mut input: Zeroizing<String>,
        passphrase: Option<&SecretString>,
    ) -> Result<IdentityConfig> {
        let keys = Keys::parse(input.trim())
            .map_err(|_| Error::Auth("the supplied Nostr secret is invalid".into()))?;
        input.zeroize();
        self.store_keys(config, label, backend, keys, passphrase)
    }

    pub fn unlock(
        &self,
        identity: &IdentityConfig,
        passphrase: Option<&SecretString>,
    ) -> Result<Keys> {
        let mut secret = match identity.backend {
            KeyBackend::Keychain => Zeroizing::new(keychain::load(&identity.key_ref)?),
            KeyBackend::EncryptedFile => {
                let passphrase = passphrase
                    .ok_or_else(|| Error::Locked("encrypted identity needs a passphrase".into()))?;
                let path = self.key_path(&identity.key_ref);
                let bytes =
                    Zeroizing::new(encrypted_file::open(&path, passphrase.expose_secret())?);
                let plaintext = std::str::from_utf8(bytes.as_slice())
                    .map_err(|_| Error::Auth("identity plaintext is malformed".into()))?;
                Zeroizing::new(plaintext.to_owned())
            }
        };
        let keys = Keys::parse(secret.as_str())
            .map_err(|_| Error::Auth("stored identity is invalid".into()))?;
        secret.zeroize();
        if keys.public_key().to_hex() != identity.pubkey {
            return Err(Error::Auth(
                "stored identity does not match its public key".into(),
            ));
        }
        Ok(keys)
    }

    pub fn delete(&self, identity: &IdentityConfig) -> Result<()> {
        match identity.backend {
            KeyBackend::Keychain => keychain::delete(&identity.key_ref),
            KeyBackend::EncryptedFile => {
                let path = self.key_path(&identity.key_ref);
                if path.exists() {
                    fs::remove_file(&path).map_err(|error| Error::io(path, error))?;
                }
                Ok(())
            }
        }
    }

    fn store_keys(
        &self,
        config: &mut Config,
        label: String,
        backend: KeyBackend,
        keys: Keys,
        passphrase: Option<&SecretString>,
    ) -> Result<IdentityConfig> {
        if label.trim().is_empty() {
            return Err(Error::Config("identity label cannot be empty".into()));
        }
        let id = Uuid::new_v4();
        let reference = format!("identity:{id}");
        let mut secret = Zeroizing::new(keys.secret_key().to_secret_hex());
        match backend {
            KeyBackend::Keychain => keychain::store(&reference, &secret)?,
            KeyBackend::EncryptedFile => {
                let passphrase = passphrase.ok_or_else(|| {
                    Error::Config("encrypted-file backend needs a passphrase".into())
                })?;
                encrypted_file::seal(
                    &self.key_path(&reference),
                    secret.as_bytes(),
                    passphrase.expose_secret(),
                )?;
            }
        }
        secret.zeroize();
        let identity = IdentityConfig {
            id,
            label,
            pubkey: keys.public_key().to_hex(),
            backend,
            key_ref: reference,
        };
        config.identities.push(identity.clone());
        if let Err(error) = config.validate() {
            config.identities.retain(|entry| entry.id != id);
            let _ = self.delete(&identity);
            return Err(error);
        }
        Ok(identity)
    }

    fn key_path(&self, reference: &str) -> PathBuf {
        let name = reference.replace(':', "-");
        self.paths.keys_dir().join(format!("{name}.key"))
    }
}

pub fn read_passphrase(prompt: &str, confirm: bool) -> Result<SecretString> {
    let passphrase = Zeroizing::new(if let Some(fd) = std::env::var_os("BZZ_PASSPHRASE_FD") {
        let fd = fd
            .to_str()
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or_else(|| Error::Config("BZZ_PASSPHRASE_FD must be an integer".into()))?;
        let path = inherited_fd_path(fd);
        let mut file = fs::File::open(&path).map_err(|error| Error::io(&path, error))?;
        let mut value = String::new();
        file.read_to_string(&mut value)
            .map_err(|error| Error::io(&path, error))?;
        value.trim_end_matches(['\r', '\n']).to_owned()
    } else {
        rpassword::prompt_password(prompt)
            .map_err(|error| Error::io("controlling terminal", error))?
    });
    if confirm && std::env::var_os("BZZ_PASSPHRASE_FD").is_none() {
        let second = Zeroizing::new(
            rpassword::prompt_password("Confirm passphrase: ")
                .map_err(|error| Error::io("controlling terminal", error))?,
        );
        if passphrase.as_str() != second.as_str() {
            return Err(Error::Auth("passphrases did not match".into()));
        }
    }
    Ok(SecretString::from(passphrase.as_str().to_owned()))
}

#[cfg(unix)]
fn inherited_fd_path(fd: u32) -> PathBuf {
    PathBuf::from(format!("/dev/fd/{fd}"))
}

#[cfg(not(unix))]
fn inherited_fd_path(fd: u32) -> PathBuf {
    PathBuf::from(format!(r"\\.\pipe\bzz-passphrase-{fd}"))
}
