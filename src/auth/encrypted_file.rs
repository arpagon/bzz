use std::{fs, path::Path};

use argon2::{Algorithm, Argon2, Params, Version};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use rand::Rng as _;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::{
    error::{Error, Result},
    paths::set_private_permissions,
};

const AAD: &[u8] = b"dev.arpagon.bzz/key/v1";

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u8,
    kdf: String,
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
    salt: String,
    nonce: String,
    ciphertext: String,
}

pub fn seal(path: &Path, secret: &[u8], passphrase: &str) -> Result<()> {
    if passphrase.chars().count() < 12 {
        return Err(Error::Config(
            "encrypted-file passphrase must contain at least 12 characters".into(),
        ));
    }
    let mut salt = [0_u8; 16];
    let mut nonce = [0_u8; 24];
    rand::rng().fill(&mut salt);
    rand::rng().fill(&mut nonce);
    let mut key = derive_key(passphrase, &salt, 65_536, 3, 1)?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: secret,
                aad: AAD,
            },
        )
        .map_err(|_| Error::Auth("could not encrypt identity".into()))?;
    key.zeroize();
    let envelope = Envelope {
        version: 1,
        kdf: "argon2id".into(),
        memory_kib: 65_536,
        iterations: 3,
        parallelism: 1,
        salt: STANDARD.encode(salt),
        nonce: STANDARD.encode(nonce),
        ciphertext: STANDARD.encode(ciphertext),
    };
    let bytes = serde_json::to_vec_pretty(&envelope)
        .map_err(|error| Error::Serialization(error.to_string()))?;
    let temporary = path.with_extension("key.tmp");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| Error::io(parent, error))?;
    }
    fs::write(&temporary, bytes).map_err(|error| Error::io(&temporary, error))?;
    set_private_permissions(&temporary)?;
    fs::rename(&temporary, path).map_err(|error| Error::io(path, error))?;
    Ok(())
}

pub fn open(path: &Path, passphrase: &str) -> Result<Vec<u8>> {
    let bytes = fs::read(path).map_err(|error| Error::io(path, error))?;
    let envelope: Envelope = serde_json::from_slice(&bytes)
        .map_err(|_| Error::Auth("identity file is malformed".into()))?;
    if envelope.version != 1
        || envelope.kdf != "argon2id"
        || envelope.memory_kib < 65_536
        || envelope.iterations < 3
        || envelope.parallelism == 0
        || envelope.memory_kib > 1_048_576
        || envelope.iterations > 20
        || envelope.parallelism > 16
    {
        return Err(Error::Auth(
            "identity file parameters are unsupported".into(),
        ));
    }
    let salt = decode_exact::<16>(&envelope.salt)?;
    let nonce = decode_exact::<24>(&envelope.nonce)?;
    let ciphertext = STANDARD
        .decode(envelope.ciphertext)
        .map_err(|_| Error::Auth("identity file is malformed".into()))?;
    let mut key = derive_key(
        passphrase,
        &salt,
        envelope.memory_kib,
        envelope.iterations,
        envelope.parallelism,
    )?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let result = cipher
        .decrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &ciphertext,
                aad: AAD,
            },
        )
        .map_err(|_| Error::Auth("wrong passphrase or damaged identity file".into()));
    key.zeroize();
    result
}

fn derive_key(
    passphrase: &str,
    salt: &[u8],
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
) -> Result<[u8; 32]> {
    let params = Params::new(memory_kib, iterations, parallelism, Some(32))
        .map_err(|_| Error::Auth("invalid identity KDF parameters".into()))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut output = [0_u8; 32];
    argon
        .hash_password_into(passphrase.as_bytes(), salt, &mut output)
        .map_err(|_| Error::Auth("identity KDF failed".into()))?;
    Ok(output)
}

fn decode_exact<const N: usize>(encoded: &str) -> Result<[u8; N]> {
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| Error::Auth("identity file is malformed".into()))?;
    bytes
        .try_into()
        .map_err(|_| Error::Auth("identity file is malformed".into()))
}
