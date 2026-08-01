use keyring::{Entry, Error as KeyringError};
use zeroize::Zeroizing;

use crate::error::{Error, Result};

const RELEASE_SERVICE: &str = "dev.arpagon.bzz";
const DEBUG_SERVICE: &str = "dev.arpagon.bzz.debug";

pub const fn service_name() -> &'static str {
    service_name_for(cfg!(debug_assertions))
}

const fn service_name_for(debug: bool) -> &'static str {
    if debug {
        DEBUG_SERVICE
    } else {
        RELEASE_SERVICE
    }
}

pub fn store(reference: &str, secret: &str) -> Result<()> {
    let entry = entry(reference)?;
    let previous = match entry.get_password() {
        Ok(value) => Some(Zeroizing::new(value)),
        Err(KeyringError::NoEntry | KeyringError::BadEncoding(_)) => None,
        Err(error) => return Err(classify_load_error(reference, error)),
    };
    entry
        .set_password(secret)
        .map_err(|error| classify_write_error(reference, error))?;

    let verification_error = match entry.get_password() {
        Ok(value) => {
            let verified = Zeroizing::new(value);
            if verified.as_str() == secret {
                return Ok(());
            }
            Error::IdentityCorrupt(format!(
                "credential {reference} failed read-back verification"
            ))
        }
        Err(error) => classify_load_error(reference, error),
    };
    if let Some(previous) = previous {
        let _ = entry.set_password(&previous);
    } else {
        let _ = entry.delete_credential();
    }
    Err(verification_error)
}

pub fn load(reference: &str) -> Result<String> {
    entry(reference)?
        .get_password()
        .map_err(|error| classify_load_error(reference, error))
}

pub fn delete(reference: &str) -> Result<()> {
    match entry(reference)?.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(error) => Err(classify_write_error(reference, error)),
    }
}

fn entry(reference: &str) -> Result<Entry> {
    Entry::new(service_name(), reference).map_err(|error| match error {
        KeyringError::Invalid(_, _) | KeyringError::TooLong(_, _) => {
            Error::Config(format!("invalid credential reference {reference}"))
        }
        other => classify_write_error(reference, other),
    })
}

fn classify_load_error(reference: &str, error: KeyringError) -> Error {
    match error {
        KeyringError::NoEntry => Error::IdentityMissing(format!(
            "credential {reference} is absent; restore this identity from its backup"
        )),
        KeyringError::NoStorageAccess(_) | KeyringError::PlatformFailure(_) => Error::Locked(
            "the OS credential service is unavailable; unlock it and restart bzz".into(),
        ),
        KeyringError::BadEncoding(_) | KeyringError::Ambiguous(_) => Error::IdentityCorrupt(
            format!("credential {reference} cannot be decoded unambiguously"),
        ),
        KeyringError::Invalid(_, _) | KeyringError::TooLong(_, _) => {
            Error::Config(format!("invalid credential reference {reference}"))
        }
        _ => Error::Locked("the OS credential service could not be read".into()),
    }
}

fn classify_write_error(reference: &str, error: KeyringError) -> Error {
    match error {
        KeyringError::NoStorageAccess(_) | KeyringError::PlatformFailure(_) => {
            Error::Locked("the OS credential service is unavailable; unlock it and retry".into())
        }
        KeyringError::BadEncoding(_) | KeyringError::Ambiguous(_) => {
            Error::IdentityCorrupt(format!("credential {reference} cannot be updated safely"))
        }
        KeyringError::Invalid(_, _) | KeyringError::TooLong(_, _) => {
            Error::Config(format!("invalid credential reference {reference}"))
        }
        KeyringError::NoEntry => {
            Error::Locked("the OS credential service rejected the write".into())
        }
        _ => Error::Locked("the OS credential service could not be updated".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{DEBUG_SERVICE, RELEASE_SERVICE, classify_load_error, service_name_for};
    use crate::error::Error;
    use keyring::Error as KeyringError;

    #[test]
    fn release_and_debug_services_are_isolated() {
        assert_eq!(service_name_for(false), RELEASE_SERVICE);
        assert_eq!(service_name_for(true), DEBUG_SERVICE);
        assert_ne!(service_name_for(false), service_name_for(true));
    }

    #[test]
    fn missing_and_unavailable_credentials_are_distinct() {
        assert!(matches!(
            classify_load_error("identity:test", KeyringError::NoEntry),
            Error::IdentityMissing(_)
        ));
        assert!(matches!(
            classify_load_error(
                "identity:test",
                KeyringError::NoStorageAccess(Box::new(std::io::Error::other("locked")))
            ),
            Error::Locked(_)
        ));
        assert!(matches!(
            classify_load_error("identity:test", KeyringError::BadEncoding(vec![0xff])),
            Error::IdentityCorrupt(_)
        ));
    }
}
