//! Pairing with Buzz Desktop on this machine (local credential provider):
//! `bzz identity pair-desktop` and the join form's "pair with Buzz" both
//! call these. The key stays in the desktop; bzz keeps only the paired
//! token, in the OS keychain and never in a file.

use url::Url;
use uuid::Uuid;

use crate::{
    auth::{desktop_reference, keychain, provider},
    config::{Config, IdentityConfig, KeyBackend},
    error::{Error, Result},
    paths::Paths,
};

/// The running Buzz Desktop to pair with: `wanted` by name, or the only
/// one running. Its name and its advertised endpoint.
pub fn find_desktop(wanted: Option<&str>) -> Result<(String, Url)> {
    if !provider::supported_platform() {
        return Err(Error::Unsupported(
            "Buzz Desktop pairing has no Windows transport in provider version 1".into(),
        ));
    }
    let directory = provider::advertisement_dir()
        .ok_or_else(|| Error::Config("home directory is unavailable".into()))?;
    let advertised = provider::advertisements(&directory);
    match wanted {
        Some(wanted) => advertised
            .into_iter()
            .find(|(name, _)| name == wanted)
            .ok_or_else(|| Error::Config(format!("Buzz Desktop instance {wanted} is not running"))),
        None => match advertised.len() {
            0 => Err(Error::Config("no running Buzz Desktop was found".into())),
            1 => Ok(advertised.into_iter().next().expect("one advertisement")),
            _ => Err(Error::Config(
                "several Buzz Desktop instances are running; pass --instance".into(),
            )),
        },
    }
}

/// Asks the desktop `instance` at `url` to pair for `relay` (the person
/// approves it in Buzz), keeps the token in the keychain and writes the
/// identity. Nothing is left behind when a step fails.
pub async fn pair(
    paths: &Paths,
    config: &mut Config,
    label: String,
    relay: &str,
    instance: &str,
    url: Url,
) -> Result<IdentityConfig> {
    let client = provider::ProviderClient::new(provider::Endpoint::Advertised(url), None)?;
    let pairing = client.pair("bzz", relay).await.map_err(Error::from)?;
    if pairing.instance != instance {
        return Err(Error::Auth(
            "Buzz Desktop answered for another instance".into(),
        ));
    }
    if config
        .identities
        .iter()
        .any(|identity| identity.pubkey == pairing.pubkey)
    {
        return Err(Error::Config("that identity is already configured".into()));
    }
    let reference = desktop_reference(instance);
    // Paired tokens live only in the OS keychain, never in a file.
    keychain::store(&reference, &pairing.token)?;
    let identity = IdentityConfig {
        id: Uuid::new_v4(),
        label,
        pubkey: pairing.pubkey.clone(),
        backend: KeyBackend::BuzzDesktop,
        key_ref: reference.clone(),
    };
    config.identities.push(identity.clone());
    if let Err(error) = config.save(paths) {
        config.identities.retain(|entry| entry.id != identity.id);
        let _ = keychain::delete(&reference);
        return Err(error);
    }
    Ok(identity)
}
