use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use futures_util::StreamExt as _;
use reqwest::{Client, StatusCode, header};
use sha2::{Digest as _, Sha256};
use tokio::{io::AsyncWriteExt as _, sync::Semaphore};
use url::{Host, Url};
use uuid::Uuid;

use crate::{
    error::{Error, Result},
    paths::set_private_permissions,
};

const MAX_PUBLIC_AVATAR_BYTES: u64 = 2 * 1024 * 1024;
/// Relay-hosted pictures are authenticated, content-addressed, and bounded
/// below the 16 MiB per-scope cache ceiling. They need a larger allowance than
/// arbitrary external pictures because existing community profiles commonly
/// reference lossless source assets.
pub(crate) const MAX_RELAY_AVATAR_BYTES: u64 = 10 * 1024 * 1024;
const MAX_REDIRECTS: usize = 3;
const MAX_CACHE_ENTRIES: usize = 256;
const MAX_CACHE_BYTES: u64 = 16 * 1024 * 1024;

/// Credential-free, bounded retrieval for public profile photographs. It is
/// deliberately separate from `MediaClient`: an avatar request must never
/// inherit a signer, NIP-98 header, relay authority, cookie, or proxy setting.
#[derive(Clone)]
pub struct AvatarClient {
    transfer_slots: Arc<Semaphore>,
}

impl AvatarClient {
    pub fn new(concurrency: usize) -> Self {
        Self {
            transfer_slots: Arc::new(Semaphore::new(concurrency.clamp(1, 4))),
        }
    }

    /// Fetches a validated avatar into a private cache path. The caller supplies
    /// a digest-derived destination and must decode it before presentation.
    pub async fn fetch(&self, source: &str, destination: &Path) -> Result<PathBuf> {
        let mut url = validate_avatar_url(source)?;
        let _permit = self
            .transfer_slots
            .acquire()
            .await
            .map_err(|_| Error::Network("profile avatar transfer queue stopped".into()))?;

        for redirect_count in 0..=MAX_REDIRECTS {
            let client = client_for(&url).await?;
            let response = client
                .get(url.clone())
                .header(
                    header::ACCEPT,
                    "image/jpeg, image/png, image/gif, image/webp",
                )
                .header(header::ACCEPT_ENCODING, "identity")
                .send()
                .await
                .map_err(|_| Error::Network("profile avatar request failed".into()))?;

            if response.status().is_redirection() {
                if redirect_count == MAX_REDIRECTS {
                    return Err(Error::Protocol(
                        "profile avatar exceeded the redirect limit".into(),
                    ));
                }
                let location = response
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| Error::Protocol("profile avatar redirect is invalid".into()))?;
                url = url
                    .join(location)
                    .map_err(|_| Error::Protocol("profile avatar redirect is invalid".into()))?;
                validate_avatar_url(url.as_str())?;
                continue;
            }
            if !response.status().is_success() {
                return Err(status_error(response.status()));
            }
            return write_avatar_response(response, destination, None, MAX_PUBLIC_AVATAR_BYTES)
                .await;
        }
        Err(Error::Protocol("profile avatar redirect is invalid".into()))
    }
}

/// Validates only the URL shape. Host resolution is validated and pinned for
/// every request by [`client_for`], including every manual redirect hop.
pub fn validate_avatar_url(source: &str) -> Result<Url> {
    let url =
        Url::parse(source).map_err(|_| Error::Protocol("profile avatar URL is invalid".into()))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port().is_some_and(|port| port != 443)
    {
        return Err(Error::Protocol(
            "profile avatar URL is not permitted".into(),
        ));
    }
    let Some(Host::Domain(host)) = url.host() else {
        return Err(Error::Protocol(
            "profile avatar host is not permitted".into(),
        ));
    };
    if host.len() > 253
        || !host.contains('.')
        || host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".localhost")
        || host.ends_with(".local")
    {
        return Err(Error::Protocol(
            "profile avatar host is not permitted".into(),
        ));
    }
    Ok(url)
}

async fn client_for(url: &Url) -> Result<Client> {
    let host = url
        .host_str()
        .ok_or_else(|| Error::Protocol("profile avatar host is not permitted".into()))?;
    let addresses = tokio::net::lookup_host((host, 443))
        .await
        .map_err(|_| Error::Network("profile avatar host could not be resolved".into()))?
        .filter(|address| public_ip(address.ip()))
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err(Error::Protocol(
            "profile avatar host resolved to a prohibited address".into(),
        ));
    }
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .resolve_to_addrs(host, &addresses)
        .build()
        .map_err(|_| Error::Network("profile avatar client could not start".into()))
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => public_ipv4(ip),
        IpAddr::V6(ip) => public_ipv6(ip),
    }
}

fn public_ipv4(ip: Ipv4Addr) -> bool {
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || ip.octets()[0] == 0
        || (ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]))
        || (ip.octets()[0] == 192 && ip.octets()[1] == 0 && ip.octets()[2] == 0)
        || (ip.octets()[0] == 198 && (ip.octets()[1] == 18 || ip.octets()[1] == 19))
        || (ip.octets()[0] == 198 && ip.octets()[1] == 51 && ip.octets()[2] == 100)
        || (ip.octets()[0] == 203 && ip.octets()[1] == 0 && ip.octets()[2] == 113)
        || ip.octets()[0] >= 224)
}

fn public_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return public_ipv4(v4);
    }
    let segments = ip.segments();
    // 6to4 carries an IPv4 destination in the next 32 bits. Do not allow an
    // otherwise-private v4 address to re-enter through that representation.
    if segments[0] == 0x2002 {
        return public_ipv4(Ipv4Addr::new(
            (segments[1] >> 8) as u8,
            segments[1] as u8,
            (segments[2] >> 8) as u8,
            segments[2] as u8,
        ));
    }
    let first = segments[0];
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || (first & 0xfe00) == 0xfc00 // unique-local fc00::/7
        || (first & 0xffc0) == 0xfe80 // link-local fe80::/10
        || (first & 0xff00) == 0xff00 // multicast (defensive for old stds)
        || (first == 0x0064 && segments[1] == 0xff9b) // NAT64 well-known prefix
        || (first == 0x2001 && segments[1] == 0) // Teredo
        || (first == 0x2001 && segments[1] == 0x0db8)) // documentation
}

/// Persist an already-authorized avatar response under the same bounds as an
/// external profile image. Relay-scoped callers supply the content address from
/// their canonical `/media/<sha256>.<ext>` path; arbitrary external avatars do
/// not have a trustworthy expected digest.
pub(crate) async fn write_avatar_response(
    response: reqwest::Response,
    destination: &Path,
    expected_sha256: Option<&str>,
    max_bytes: u64,
) -> Result<PathBuf> {
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(normalized_content_type)
        .filter(|mime| supported_image_mime(mime))
        .map(str::to_owned)
        .ok_or_else(|| Error::Protocol("profile avatar MIME is missing or unsupported".into()))?;
    if response
        .content_length()
        .is_some_and(|length| length == 0 || length > max_bytes)
    {
        return Err(Error::Protocol(
            "profile avatar exceeds the byte limit".into(),
        ));
    }
    if response
        .headers()
        .get(header::CONTENT_ENCODING)
        .is_some_and(|value| !value.as_bytes().eq_ignore_ascii_case(b"identity"))
    {
        return Err(Error::Protocol(
            "encoded profile avatar responses are not accepted".into(),
        ));
    }

    let parent = destination
        .parent()
        .ok_or_else(|| Error::Config("profile avatar cache path has no parent".into()))?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|error| Error::io(parent, error))?;
    set_private_permissions(parent)?;
    let temporary = parent.join(format!(".{}.part", Uuid::new_v4()));
    let mut output = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .await
        .map_err(|error| Error::io(&temporary, error))?;
    set_private_permissions(&temporary)?;

    let mut written = 0_u64;
    let mut digest = expected_sha256.map(|_| Sha256::new());
    let mut stream = response.bytes_stream();
    let result = async {
        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|_| Error::Network("profile avatar transfer failed".into()))?;
            written = written
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| Error::Protocol("profile avatar size overflow".into()))?;
            if written > max_bytes {
                return Err(Error::Protocol(
                    "profile avatar exceeds the byte limit".into(),
                ));
            }
            if let Some(digest) = &mut digest {
                digest.update(&chunk);
            }
            output
                .write_all(&chunk)
                .await
                .map_err(|error| Error::io(&temporary, error))?;
        }
        output
            .flush()
            .await
            .map_err(|error| Error::io(&temporary, error))?;
        output
            .sync_all()
            .await
            .map_err(|error| Error::io(&temporary, error))?;
        if written == 0 {
            return Err(Error::Protocol("profile avatar response is empty".into()));
        }
        if let (Some(expected), Some(digest)) = (expected_sha256, digest)
            && !hex::encode(digest.finalize()).eq_ignore_ascii_case(expected)
        {
            return Err(Error::Protocol(
                "profile avatar bytes do not match the relay media address".into(),
            ));
        }
        Ok(())
    }
    .await;
    drop(output);
    if let Err(error) = result {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(error);
    }

    let inferred = infer::get_from_path(&temporary)
        .map_err(|error| Error::io(&temporary, error))?
        .map(|kind| kind.mime_type());
    if inferred != Some(content_type.as_str()) {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(Error::Protocol(
            "profile avatar bytes do not match the response MIME".into(),
        ));
    }
    if let Err(error) = tokio::fs::rename(&temporary, destination).await {
        if destination.exists() {
            let _ = tokio::fs::remove_file(&temporary).await;
        } else {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(Error::io(destination, error));
        }
    }
    prune_cache(parent, destination)?;
    Ok(destination.to_path_buf())
}

fn prune_cache(parent: &Path, keep: &Path) -> Result<()> {
    let mut entries = Vec::new();
    let mut total = 0_u64;
    for entry in std::fs::read_dir(parent).map_err(|error| Error::io(parent, error))? {
        let entry = entry.map_err(|error| Error::io(parent, error))?;
        let file_type = entry
            .file_type()
            .map_err(|error| Error::io(entry.path(), error))?;
        let path = entry.path();
        if file_type.is_symlink() {
            let _ = std::fs::remove_file(path);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let metadata = entry.metadata().map_err(|error| Error::io(&path, error))?;
        total = total.saturating_add(metadata.len());
        entries.push((
            metadata
                .modified()
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
            metadata.len(),
            path,
        ));
    }
    entries.sort_by_key(|(modified, _, _)| *modified);
    while entries.len() > MAX_CACHE_ENTRIES || total > MAX_CACHE_BYTES {
        let Some((_, size, path)) = entries.first().cloned() else {
            break;
        };
        entries.remove(0);
        if path == keep {
            entries.push((std::time::SystemTime::now(), size, path));
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
    Ok(())
}

fn normalized_content_type(value: &str) -> &str {
    value.split(';').next().unwrap_or_default().trim()
}

fn supported_image_mime(mime: &str) -> bool {
    matches!(
        mime,
        "image/jpeg" | "image/png" | "image/gif" | "image/webp"
    )
}

fn status_error(status: StatusCode) -> Error {
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        Error::Access(format!("profile avatar access denied ({status})"))
    } else {
        Error::Network(format!("profile avatar request failed with {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::{public_ip, validate_avatar_url};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn avatar_urls_are_https_public_domains_without_credentials_or_fragments() {
        assert_eq!(
            validate_avatar_url("https://cdn.example.test/avatar.webp?size=96")
                .unwrap()
                .host_str(),
            Some("cdn.example.test")
        );
        for value in [
            "http://cdn.example.test/a.png",
            "https://localhost/a.png",
            "https://127.0.0.1/a.png",
            "https://[::1]/a.png",
            "https://user@cdn.example.test/a.png",
            "https://cdn.example.test/a.png#fragment",
            "https://cdn.example.test:8443/a.png",
            "https://single-label/a.png",
        ] {
            assert!(validate_avatar_url(value).is_err(), "accepted {value}");
        }
    }

    #[test]
    fn private_and_documentation_addresses_are_not_fetchable() {
        for ip in [
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(169, 254, 1, 1)),
            IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            IpAddr::V6("fc00::1".parse().unwrap()),
            IpAddr::V6("fe80::1".parse().unwrap()),
            IpAddr::V6("2002:0a00:0001::1".parse().unwrap()),
        ] {
            assert!(!public_ip(ip), "accepted {ip}");
        }
        assert!(public_ip(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
    }
}
