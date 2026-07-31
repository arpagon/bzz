use keyring::Entry;

use crate::error::{Error, Result};

const SERVICE: &str = "dev.arpagon.bzz";

pub fn store(reference: &str, secret: &str) -> Result<()> {
    entry(reference)?
        .set_password(secret)
        .map_err(|_| Error::Auth("credential service could not store identity".into()))
}

pub fn load(reference: &str) -> Result<String> {
    entry(reference)?
        .get_password()
        .map_err(|_| Error::Auth("credential service could not unlock identity".into()))
}

pub fn delete(reference: &str) -> Result<()> {
    entry(reference)?
        .delete_credential()
        .map_err(|_| Error::Auth("credential service could not delete identity".into()))
}

fn entry(reference: &str) -> Result<Entry> {
    Entry::new(SERVICE, reference)
        .map_err(|_| Error::Auth("credential service is unavailable".into()))
}
