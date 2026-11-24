//! Feature-film rehearsal (runs/experiments/promo-video/feature-apps), not a
//! file of bzz: copied into `examples/` of a `git archive` of the tag
//! v0.16.0 on cold storage and built there, beside `promo_fixture`.
//!
//! `promo_fixture seed` writes a bzz profile for the viewer ("You") only.
//! A film with several people's screens needs a profile for each of them:
//!
//! ```text
//! film_people profile <fixture dir> <ada|lin|sam> <empty out dir>
//! ```
//!
//! reads the generated key of that person from `<fixture dir>/state.json`
//! and writes, as `promo_fixture`'s `write_profile` does for the viewer, an
//! isolated `config/`, `data/`, `cache/` and a fresh disposable passphrase,
//! all owner-only: an encrypted-file identity, the community "Studio" on
//! `ws://localhost:3030`, `[media] protocol = "sixel"`. Generated people only.
use std::path::Path;

use bzz::{
    auth::IdentityManager,
    config::{Config, KeyBackend},
    paths::Paths,
};
use nostr::Keys;
use secrecy::SecretString;
use serde_json::Value;

fn owner_only(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    assert!(
        arguments.len() == 4 && arguments[0] == "profile",
        "usage: film_people profile <fixture dir> <who> <out dir>"
    );
    let (root, who, out) = (Path::new(&arguments[1]), &arguments[2], Path::new(&arguments[3]));
    assert!(who != "viewer", "the viewer's profile is promo_fixture's");
    assert!(
        !out.exists() || std::fs::read_dir(out).unwrap().next().is_none(),
        "use an empty profile directory"
    );
    let state: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("state.json")).unwrap()).unwrap();
    let keys = Keys::parse(
        state["keys"][who.as_str()]
            .as_str()
            .unwrap_or_else(|| panic!("no generated person {who} in state.json")),
    )
    .unwrap();
    std::fs::create_dir_all(out).unwrap();
    let paths = Paths {
        config_dir: out.join("config"),
        data_dir: out.join("data"),
        cache_dir: out.join("cache"),
    };
    paths.ensure().unwrap();
    let passphrase = hex::encode(rand::random::<[u8; 16]>());
    owner_only(&out.join("passphrase"), &format!("{passphrase}\n"));
    let mut config = Config::default();
    let identity = IdentityManager::new(&paths)
        .import_keys(
            &mut config,
            who.clone(),
            KeyBackend::EncryptedFile,
            keys,
            Some(&SecretString::from(passphrase)),
        )
        .unwrap();
    config
        .add_community(
            "Studio".into(),
            "ws://localhost:3030".into(),
            identity.id,
            true,
        )
        .unwrap();
    config.media.protocol = bzz::config::MediaProtocol::Sixel;
    config.save(&paths).unwrap();
    println!("profile of {who} ready: {}", out.display());
}
