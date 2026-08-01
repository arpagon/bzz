use std::fs;

use bzz::{
    auth::{IdentityManager, encrypted_file},
    config::{Config, KeyBackend},
    error::Error,
    paths::Paths,
};
use nostr::Keys;
use secrecy::SecretString;
use tempfile::TempDir;
use zeroize::Zeroizing;

#[test]
fn encrypted_identity_round_trips_and_tampering_fails() {
    let temporary = TempDir::new().unwrap();
    let path = temporary.path().join("identity.key");
    encrypted_file::seal(&path, b"not-a-real-secret", "correct horse battery staple").unwrap();
    assert_eq!(
        encrypted_file::open(&path, "correct horse battery staple").unwrap(),
        b"not-a-real-secret"
    );
    assert!(encrypted_file::open(&path, "incorrect password here").is_err());
    let mut bytes = fs::read(&path).unwrap();
    let index = bytes.len() - 8;
    bytes[index] ^= 1;
    fs::write(&path, bytes).unwrap();
    assert!(encrypted_file::open(&path, "correct horse battery staple").is_err());
}

#[test]
fn weak_fallback_passphrase_is_refused() {
    let temporary = TempDir::new().unwrap();
    assert!(encrypted_file::seal(&temporary.path().join("key"), b"secret", "short").is_err());
}

#[test]
fn missing_encrypted_identity_can_only_be_restored_with_the_matching_key() {
    let temporary = TempDir::new().unwrap();
    let paths = Paths {
        config_dir: temporary.path().join("config"),
        data_dir: temporary.path().join("data"),
        cache_dir: temporary.path().join("cache"),
    };
    paths.ensure().unwrap();
    let manager = IdentityManager::new(&paths);
    let passphrase = SecretString::from("correct horse battery staple".to_owned());
    let keys = Keys::generate();
    let mut config = Config::default();
    let identity = manager
        .import(
            &mut config,
            "restore-me".into(),
            KeyBackend::EncryptedFile,
            Zeroizing::new(keys.secret_key().to_secret_hex()),
            Some(&passphrase),
        )
        .unwrap();
    manager.delete(&identity).unwrap();

    assert!(matches!(
        manager.unlock(&identity, Some(&passphrase)),
        Err(Error::IdentityMissing(_))
    ));
    assert!(
        manager
            .restore_keys(&identity, Keys::generate(), Some(&passphrase))
            .is_err()
    );
    assert!(matches!(
        manager.unlock(&identity, Some(&passphrase)),
        Err(Error::IdentityMissing(_))
    ));

    manager
        .restore_keys(&identity, keys.clone(), Some(&passphrase))
        .unwrap();
    assert_eq!(
        manager
            .unlock(&identity, Some(&passphrase))
            .unwrap()
            .public_key(),
        keys.public_key()
    );
    manager
        .restore_keys(&identity, keys.clone(), Some(&passphrase))
        .unwrap();
    assert_eq!(
        manager
            .unlock(&identity, Some(&passphrase))
            .unwrap()
            .public_key(),
        keys.public_key()
    );
}
