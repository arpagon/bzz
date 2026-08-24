use keyring::{Entry, Error as KeyringError};
use zeroize::Zeroizing;

use crate::error::{Error, Result};

const RELEASE_SERVICE: &str = "dev.arpagon.bzz.telemetry";
const DEBUG_SERVICE: &str = "dev.arpagon.bzz.debug.telemetry";

fn service_name() -> &'static str {
    if cfg!(debug_assertions) {
        DEBUG_SERVICE
    } else {
        RELEASE_SERVICE
    }
}

fn reference(installation_id: uuid::Uuid) -> String {
    format!("otlp:{}", installation_id.simple())
}

fn entry(installation_id: uuid::Uuid) -> Result<Entry> {
    Entry::new(service_name(), &reference(installation_id)).map_err(|_| {
        Error::Locked("the OS credential service rejected the telemetry reference".into())
    })
}

pub fn store(installation_id: uuid::Uuid, token: &str) -> Result<()> {
    if !valid_token(token) {
        return Err(Error::Config("telemetry token is invalid".into()));
    }
    let entry = entry(installation_id)?;
    entry.set_password(token).map_err(classify_write)?;
    let verified = Zeroizing::new(entry.get_password().map_err(classify_load)?);
    if verified.as_str() != token {
        let _ = entry.delete_credential();
        return Err(Error::Locked(
            "telemetry credential failed read-back verification".into(),
        ));
    }
    Ok(())
}

pub fn load(installation_id: uuid::Uuid) -> Result<Zeroizing<String>> {
    if let Ok(value) = std::env::var("BZZ_OTEL_TOKEN") {
        let value = Zeroizing::new(value);
        if !valid_token(&value) {
            return Err(Error::Config("BZZ_OTEL_TOKEN is invalid".into()));
        }
        return Ok(value);
    }
    entry(installation_id)?
        .get_password()
        .map(Zeroizing::new)
        .map_err(classify_load)
}

pub fn available(installation_id: uuid::Uuid) -> CredentialAvailability {
    if std::env::var_os("BZZ_OTEL_TOKEN").is_some() {
        return CredentialAvailability::Environment;
    }
    match entry(installation_id).and_then(|entry| entry.get_password().map_err(classify_load)) {
        Ok(mut token) => {
            zeroize::Zeroize::zeroize(&mut token);
            CredentialAvailability::Keychain
        }
        Err(Error::IdentityMissing(_)) => CredentialAvailability::Missing,
        Err(_) => CredentialAvailability::Unavailable,
    }
}

pub fn delete(installation_id: uuid::Uuid) -> Result<()> {
    match entry(installation_id)?.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(error) => Err(classify_write(error)),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialAvailability {
    Keychain,
    Environment,
    Missing,
    Unavailable,
}

impl CredentialAvailability {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Keychain => "available (OS credential service)",
            Self::Environment => "available (process environment)",
            Self::Missing => "missing",
            Self::Unavailable => "unavailable",
        }
    }

    pub const fn is_available(self) -> bool {
        matches!(self, Self::Keychain | Self::Environment)
    }
}

fn valid_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 16 * 1024
        && token.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}

fn classify_load(error: KeyringError) -> Error {
    match error {
        KeyringError::NoEntry => Error::IdentityMissing("telemetry credential is absent".into()),
        KeyringError::NoStorageAccess(_) | KeyringError::PlatformFailure(_) => {
            Error::Locked("the OS credential service is unavailable; unlock it and retry".into())
        }
        _ => Error::Locked("the telemetry credential could not be read".into()),
    }
}

fn classify_write(error: KeyringError) -> Error {
    match error {
        KeyringError::NoStorageAccess(_) | KeyringError::PlatformFailure(_) => {
            Error::Locked("the OS credential service is unavailable; unlock it and retry".into())
        }
        _ => Error::Locked("the telemetry credential could not be updated".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_and_identity_services_are_separate() {
        assert_ne!(service_name(), crate::auth::keychain::service_name());
    }

    #[test]
    fn references_are_pseudonymous_and_content_free() {
        let id = uuid::Uuid::nil();
        assert_eq!(reference(id), "otlp:00000000000000000000000000000000");
        assert!(valid_token("opaque.token-_~"));
        assert!(!valid_token("token with space"));
        assert!(!valid_token("tökén"));
    }
}
