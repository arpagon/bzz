use std::fs;

use bzz::auth::encrypted_file;
use tempfile::TempDir;

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
