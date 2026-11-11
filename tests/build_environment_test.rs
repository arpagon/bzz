//! Prevent release jobs from reintroducing the live Ubuntu mirror dependency.
//! Runtime proof is the offline container build and the real GitHub matrix.
use std::collections::BTreeSet;

#[test]
fn linux_release_and_ci_use_pinned_prebuilt_dependencies() {
    let config: toml::Value = toml::from_str(include_str!("../dist-workspace.toml")).unwrap();
    let runners = &config["dist"]["github-custom-runners"];
    let mut images = BTreeSet::new();
    for target in ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"] {
        let container = &runners[target]["container"];
        assert_eq!(container["host"].as_str(), Some(target));
        let image = container["image"].as_str().unwrap();
        let digest = image
            .strip_prefix("ghcr.io/arpagon/bzz-build@sha256:")
            .unwrap();
        assert_eq!(digest.len(), 64, "pin the image by immutable digest");
        assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
        images.insert(image);
    }
    assert_eq!(images.len(), 1, "use the same multi-architecture image");
    let image = images.first().unwrap();
    for (name, workflow) in [
        ("ci", include_str!("../.github/workflows/ci.yml")),
        (
            "relay",
            include_str!("../.github/workflows/integration.yml"),
        ),
        ("release", include_str!("../.github/workflows/release.yml")),
    ] {
        assert!(
            !workflow.contains("apt-get") && !workflow.contains("apt install"),
            "{name} must not install distro packages at job runtime"
        );
        if name != "release" {
            assert!(
                workflow.contains(image),
                "{name} must use the release image"
            );
        }
    }
}

#[test]
fn linux_keyring_vendors_dbus() {
    let manifest: toml::Value = toml::from_str(include_str!("../Cargo.toml")).unwrap();
    let features = manifest["dependencies"]["keyring"]["features"]
        .as_array()
        .unwrap();
    assert!(
        features
            .iter()
            .any(|feature| feature.as_str() == Some("vendored"))
    );
    assert!(
        features
            .iter()
            .any(|feature| feature.as_str() == Some("sync-secret-service"))
    );
}
