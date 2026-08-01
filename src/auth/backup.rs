use std::{
    fs::{self, OpenOptions},
    io::Write as _,
    path::Path,
};

use nostr::{
    FromBech32 as _, Keys, ToBech32 as _,
    nips::nip49::{EncryptedSecretKey, KeySecurity},
};
use zeroize::Zeroizing;

use crate::error::{Error, Result};

pub const BACKUP_LOG_N: u8 = 18;
pub const MAX_BACKUP_LOG_N: u8 = BACKUP_LOG_N;
pub const MIN_BACKUP_PASSPHRASE_LEN: usize = 12;
const MAX_BACKUP_BYTES: u64 = 4_096;

pub fn create_backup(keys: &Keys, passphrase: &str) -> Result<Zeroizing<String>> {
    create_backup_with_log_n(keys, passphrase, BACKUP_LOG_N)
}

fn create_backup_with_log_n(keys: &Keys, passphrase: &str, log_n: u8) -> Result<Zeroizing<String>> {
    validate_passphrase(passphrase)?;
    let encrypted =
        EncryptedSecretKey::new(keys.secret_key(), passphrase, log_n, KeySecurity::Unknown)
            .map_err(|_| Error::Auth("could not encrypt NIP-49 identity backup".into()))?;
    let encoded = Zeroizing::new(
        encrypted
            .to_bech32()
            .map_err(|_| Error::Auth("could not encode NIP-49 identity backup".into()))?,
    );
    let recovered = decrypt_backup(encoded.as_str(), passphrase)?;
    if recovered.public_key() != keys.public_key() {
        return Err(Error::IdentityCorrupt(
            "NIP-49 backup verification recovered a different identity".into(),
        ));
    }
    Ok(encoded)
}

pub fn decrypt_backup(encoded: &str, passphrase: &str) -> Result<Keys> {
    let encrypted = EncryptedSecretKey::from_bech32(encoded.trim())
        .map_err(|_| Error::IdentityCorrupt("backup is not a valid NIP-49 ncryptsec".into()))?;
    if encrypted.log_n() > MAX_BACKUP_LOG_N {
        return Err(Error::Unsupported(format!(
            "backup KDF cost {} exceeds the supported maximum {MAX_BACKUP_LOG_N}",
            encrypted.log_n()
        )));
    }
    let secret = encrypted
        .decrypt(passphrase)
        .map_err(|_| Error::Locked("wrong backup passphrase or damaged backup".into()))?;
    Ok(Keys::new(secret))
}

pub fn read_backup_file(path: &Path) -> Result<Zeroizing<String>> {
    let metadata = fs::metadata(path).map_err(|error| Error::io(path, error))?;
    if !metadata.is_file() || metadata.len() > MAX_BACKUP_BYTES {
        return Err(Error::IdentityCorrupt(
            "backup must be a regular NIP-49 file no larger than 4096 bytes".into(),
        ));
    }
    let value = Zeroizing::new(fs::read_to_string(path).map_err(|error| Error::io(path, error))?);
    if value.trim().is_empty() {
        return Err(Error::IdentityCorrupt("backup file is empty".into()));
    }
    Ok(value)
}

pub fn write_backup_file(path: &Path, encoded: &str) -> Result<()> {
    if path.exists() {
        return Err(Error::Config(format!(
            "backup output {} already exists; choose a new path",
            path.display()
        )));
    }
    let parent = path
        .parent()
        .filter(|value| !value.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| Error::io(parent, error))?;
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("identity.ncryptsec");
    let temporary = parent.join(format!(".{filename}.{}.tmp", uuid::Uuid::new_v4()));

    let result: Result<()> = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|error| Error::io(&temporary, error))?;
        file.write_all(encoded.as_bytes())
            .map_err(|error| Error::io(&temporary, error))?;
        file.sync_all()
            .map_err(|error| Error::io(&temporary, error))?;
        fs::hard_link(&temporary, path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                Error::Config(format!(
                    "backup output {} already exists; choose a new path",
                    path.display()
                ))
            } else {
                Error::io(path, error)
            }
        })?;
        Ok(())
    })();
    let _ = fs::remove_file(&temporary);
    result?;

    let on_disk = match read_backup_file(path) {
        Ok(value) => value,
        Err(error) => {
            let _ = fs::remove_file(path);
            return Err(error);
        }
    };
    if on_disk.trim() != encoded.trim() {
        let _ = fs::remove_file(path);
        return Err(Error::IdentityCorrupt(
            "backup file failed read-back verification".into(),
        ));
    }
    Ok(())
}

fn validate_passphrase(passphrase: &str) -> Result<()> {
    if passphrase.chars().count() < MIN_BACKUP_PASSPHRASE_LEN {
        return Err(Error::Config(format!(
            "backup passphrase must contain at least {MIN_BACKUP_PASSPHRASE_LEN} characters"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{create_backup_with_log_n, decrypt_backup, write_backup_file};
    use nostr::Keys;
    use tempfile::TempDir;

    #[test]
    fn nip49_backup_round_trips_and_refuses_overwrite() {
        let keys = Keys::generate();
        let encoded = create_backup_with_log_n(&keys, "correct horse battery staple", 10).unwrap();
        let restored = decrypt_backup(&encoded, "correct horse battery staple").unwrap();
        assert_eq!(restored.public_key(), keys.public_key());
        assert!(decrypt_backup(&encoded, "incorrect backup password").is_err());

        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("identity.ncryptsec");
        write_backup_file(&path, &encoded).unwrap();
        assert!(write_backup_file(&path, &encoded).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn nip49_backup_rejects_short_passphrases() {
        assert!(create_backup_with_log_n(&Keys::generate(), "too short", 10).is_err());
    }
}
