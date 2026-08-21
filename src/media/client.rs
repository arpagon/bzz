use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::StreamExt as _;
use nostr::{EventBuilder, JsonUtil as _, Kind, Tag, Timestamp};
use reqwest::{Client, StatusCode, header};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use tokio::{io::AsyncWriteExt as _, sync::Semaphore};
use url::Url;
use uuid::Uuid;

use crate::{
    auth::signer::SignerHandle,
    error::{Error, Result},
    paths::set_private_permissions,
};

use super::{
    avatar::{MAX_RELAY_AVATAR_BYTES, write_avatar_response},
    imeta::{hash_from_path, validate_media_url},
    model::{Attachment, MediaKind},
};

const IMAGE_LIMIT: u64 = 50 * 1024 * 1024;
const GIF_LIMIT: u64 = 10 * 1024 * 1024;
const FILE_LIMIT: u64 = 100 * 1024 * 1024;
const VIDEO_LIMIT: u64 = 500 * 1024 * 1024;

#[derive(Clone)]
pub struct MediaClient {
    base: Url,
    authority: String,
    signer: SignerHandle,
    client: Client,
    transfer_slots: Arc<Semaphore>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedPoster {
    pub path: PathBuf,
    pub sha256: String,
    pub size: u64,
    pub mime: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlobDescriptor {
    url: String,
    sha256: String,
    size: u64,
    #[serde(rename = "type")]
    mime: String,
    #[allow(dead_code)]
    uploaded: i64,
    dim: Option<String>,
    blurhash: Option<String>,
    thumb: Option<String>,
    duration: Option<f64>,
}

impl MediaClient {
    pub fn new(
        base: Url,
        authority: String,
        signer: SignerHandle,
        concurrency: usize,
    ) -> Result<Self> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(600))
            .user_agent(concat!("bzz/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| Error::Network(error.to_string()))?;
        Ok(Self {
            base,
            authority,
            signer,
            client,
            transfer_slots: Arc::new(Semaphore::new(concurrency)),
        })
    }

    /// Fetch a profile picture hosted by the active community relay. Unlike a
    /// general profile URL, this accepts only a canonical, content-addressed
    /// relay `/media/<sha256>.<image-extension>` URL before minting the same
    /// short-lived read authorization as verified attachment retrieval.
    ///
    /// The signer is therefore never invoked for a third-party profile URL.
    /// Redirects remain refused by the underlying client, and the response is
    /// bounded, MIME/magic checked, and hash checked by the avatar writer.
    pub async fn fetch_profile_avatar(&self, source: &str, destination: &Path) -> Result<PathBuf> {
        if destination.exists() {
            return Ok(destination.to_path_buf());
        }
        let (url, sha256) = self.relay_avatar_url(source)?;
        let _permit = self
            .transfer_slots
            .acquire()
            .await
            .map_err(|_| Error::Network("profile avatar transfer queue stopped".into()))?;
        let response = self.send_get(&url, &sha256).await?;
        write_avatar_response(response, destination, Some(&sha256), MAX_RELAY_AVATAR_BYTES).await
    }

    /// Returns true only when `source` is a supported image address belonging
    /// to this exact community relay origin. This is deliberately stricter
    /// than a generic same-origin URL because callers use it to select the
    /// authorization-bearing branch.
    pub fn is_relay_profile_avatar(&self, source: &str) -> bool {
        self.relay_avatar_url(source).is_ok()
    }

    pub async fn fetch(&self, attachment: &Attachment, destination: &Path) -> Result<PathBuf> {
        if !attachment.valid() {
            return Err(Error::Protocol("attachment descriptor is invalid".into()));
        }
        let limit = transfer_limit(attachment);
        if attachment.size > limit {
            return Err(Error::Protocol(
                "attachment exceeds the transfer limit".into(),
            ));
        }
        if destination.exists() {
            verify_file(destination, &attachment.sha256, attachment.size).await?;
            return Ok(destination.to_path_buf());
        }
        let _permit = self
            .transfer_slots
            .acquire()
            .await
            .map_err(|_| Error::Network("media transfer queue stopped".into()))?;
        let url = validate_media_url(&attachment.url, &self.base, Some(&attachment.sha256), false)
            .map_err(Error::Protocol)?;
        let response = self.send_get(&url, &attachment.sha256).await?;
        if response
            .content_length()
            .is_some_and(|size| size != attachment.size || size > limit)
        {
            return Err(Error::Protocol(
                "media response size does not match its descriptor".into(),
            ));
        }
        validate_identity_encoding(&response)?;
        if let Some(response_hash) = response.headers().get("X-SHA-256")
            && response_hash.to_str().unwrap_or_default() != attachment.sha256
        {
            return Err(Error::Protocol(
                "media response hash header does not match its descriptor".into(),
            ));
        }
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .map(normalized_content_type)
            .ok_or_else(|| Error::Protocol("media response MIME is missing".into()))?;
        if !content_type.eq_ignore_ascii_case(&attachment.mime) {
            return Err(Error::Protocol(
                "media response MIME does not match its descriptor".into(),
            ));
        }
        let parent = destination
            .parent()
            .ok_or_else(|| Error::Config("media cache path has no parent".into()))?;
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
        let mut stream = response.bytes_stream();
        let mut digest = Sha256::new();
        let mut written = 0_u64;
        let result = async {
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|error| Error::Network(error.to_string()))?;
                written = written
                    .checked_add(chunk.len() as u64)
                    .ok_or_else(|| Error::Protocol("media response size overflow".into()))?;
                if written > attachment.size || written > limit {
                    return Err(Error::Protocol(
                        "media response exceeded its declared size".into(),
                    ));
                }
                digest.update(&chunk);
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
            if written != attachment.size {
                return Err(Error::Protocol(
                    "media response ended before its declared size".into(),
                ));
            }
            if hex::encode(digest.finalize()) != attachment.sha256 {
                return Err(Error::Protocol("media response hash mismatch".into()));
            }
            Ok(())
        }
        .await;
        drop(output);
        if let Err(error) = result {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(error);
        }
        if attachment.kind == MediaKind::Image {
            let inferred = infer::get_from_path(&temporary)
                .map_err(|error| Error::io(&temporary, error))?
                .map(|kind| kind.mime_type());
            if inferred != Some(attachment.mime.as_str()) {
                let _ = tokio::fs::remove_file(&temporary).await;
                return Err(Error::Protocol(
                    "media bytes do not match the declared image MIME".into(),
                ));
            }
        }
        if let Err(error) = tokio::fs::rename(&temporary, destination).await {
            if destination.exists() {
                let _ = tokio::fs::remove_file(&temporary).await;
                verify_file(destination, &attachment.sha256, attachment.size).await?;
            } else {
                let _ = tokio::fs::remove_file(&temporary).await;
                return Err(Error::io(destination, error));
            }
        }
        Ok(destination.to_path_buf())
    }

    /// Fetch a separately content-addressed NIP-71 video poster. `imeta image`
    /// carries a hash-bound URL but no size/MIME descriptor, so those values are
    /// discovered from a bounded response and checked against the file magic.
    pub async fn fetch_poster(
        &self,
        poster_url: &str,
        expected_hash: &str,
        destination: &Path,
    ) -> Result<VerifiedPoster> {
        let url = validate_media_url(poster_url, &self.base, Some(expected_hash), false)
            .map_err(Error::Protocol)?;
        if destination.exists() {
            return verify_poster_file(destination, expected_hash).await;
        }
        let _permit = self
            .transfer_slots
            .acquire()
            .await
            .map_err(|_| Error::Network("media transfer queue stopped".into()))?;
        let response = self.send_get(&url, expected_hash).await?;
        validate_identity_encoding(&response)?;
        let declared_size = response.content_length();
        if declared_size.is_some_and(|size| size == 0 || size > IMAGE_LIMIT) {
            return Err(Error::Protocol(
                "video poster exceeds the image transfer limit".into(),
            ));
        }
        if let Some(response_hash) = response.headers().get("X-SHA-256")
            && response_hash.to_str().unwrap_or_default() != expected_hash
        {
            return Err(Error::Protocol(
                "video poster hash header does not match its URL".into(),
            ));
        }
        let mime = response
            .headers()
            .get(header::CONTENT_TYPE)
            .map(normalized_content_type)
            .filter(|mime| supported_image_mime(mime))
            .ok_or_else(|| Error::Protocol("video poster MIME is missing or unsupported".into()))?
            .to_owned();
        let limit = if mime == "image/gif" {
            GIF_LIMIT
        } else {
            IMAGE_LIMIT
        };
        if response.content_length().is_some_and(|size| size > limit) {
            return Err(Error::Protocol(
                "video poster exceeds its format limit".into(),
            ));
        }

        let parent = destination
            .parent()
            .ok_or_else(|| Error::Config("video poster cache path has no parent".into()))?;
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
        let mut stream = response.bytes_stream();
        let mut digest = Sha256::new();
        let mut written = 0_u64;
        let result = async {
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|error| Error::Network(error.to_string()))?;
                written = written
                    .checked_add(chunk.len() as u64)
                    .ok_or_else(|| Error::Protocol("video poster size overflow".into()))?;
                if written > limit {
                    return Err(Error::Protocol(
                        "video poster exceeded its transfer limit".into(),
                    ));
                }
                digest.update(&chunk);
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
            if declared_size.is_some_and(|size| size != written) {
                return Err(Error::Protocol(
                    "video poster response length is inconsistent".into(),
                ));
            }
            if written == 0 || hex::encode(digest.finalize()) != expected_hash {
                return Err(Error::Protocol("video poster hash mismatch".into()));
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
        if inferred != Some(mime.as_str()) {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(Error::Protocol(
                "video poster bytes do not match the response MIME".into(),
            ));
        }
        if let Err(error) = tokio::fs::rename(&temporary, destination).await {
            if destination.exists() {
                let _ = tokio::fs::remove_file(&temporary).await;
                return verify_poster_file(destination, expected_hash).await;
            }
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(Error::io(destination, error));
        }
        Ok(VerifiedPoster {
            path: destination.to_path_buf(),
            sha256: expected_hash.to_owned(),
            size: written,
            mime,
        })
    }

    pub async fn upload(
        &self,
        staged: &Path,
        mime: &str,
        filename: Option<String>,
    ) -> Result<Attachment> {
        let metadata = tokio::fs::metadata(staged)
            .await
            .map_err(|error| Error::io(staged, error))?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(Error::Config(
                "attachment must be a non-empty regular file".into(),
            ));
        }
        let limit = mime_limit(mime);
        if metadata.len() > limit {
            return Err(Error::Config("attachment exceeds the upload limit".into()));
        }
        let local_dimensions = if mime.starts_with("image/") {
            let path = staged.to_path_buf();
            Some(
                tokio::task::spawn_blocking(move || {
                    image::ImageReader::open(&path)
                        .map_err(|error| Error::io(&path, error))?
                        .with_guessed_format()
                        .map_err(|_| Error::Protocol("uploaded image format is invalid".into()))?
                        .into_dimensions()
                        .map_err(|_| {
                            Error::Protocol("uploaded image dimensions are invalid".into())
                        })
                })
                .await
                .map_err(|_| Error::Protocol("image inspection worker stopped".into()))??,
            )
        } else {
            None
        };
        let sha256 = hash_file(staged).await?;
        let auth = self.upload_auth(&sha256).await?;
        let _permit = self
            .transfer_slots
            .acquire()
            .await
            .map_err(|_| Error::Network("media transfer queue stopped".into()))?;
        let upload = self
            .base
            .join("upload")
            .map_err(|error| Error::Config(error.to_string()))?;
        let mut response = self
            .send_upload(upload, &auth, mime, &sha256, staged, metadata.len())
            .await?;
        if matches!(
            response.status(),
            StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
        ) {
            let legacy = self
                .base
                .join("media/upload")
                .map_err(|error| Error::Config(error.to_string()))?;
            response = self
                .send_upload(legacy, &auth, mime, &sha256, staged, metadata.len())
                .await?;
        }
        if !response.status().is_success() {
            return Err(status_error(response.status()));
        }
        let descriptor_bytes = read_response_limited(response, 64 * 1024).await?;
        let descriptor: BlobDescriptor = serde_json::from_slice(&descriptor_bytes)
            .map_err(|_| Error::Protocol("media upload returned an invalid descriptor".into()))?;
        if descriptor.sha256 != sha256
            || descriptor.size != metadata.len()
            || descriptor.mime != mime
        {
            return Err(Error::Protocol(
                "media upload descriptor does not match the uploaded bytes".into(),
            ));
        }
        let url = validate_media_url(&descriptor.url, &self.base, Some(&sha256), false)
            .map_err(Error::Protocol)?;
        let dimensions = descriptor.dim.as_deref().and_then(parse_dimensions);
        if descriptor.dim.is_some() && dimensions.is_none() {
            return Err(Error::Protocol(
                "media upload returned invalid dimensions".into(),
            ));
        }
        if local_dimensions.is_some() && dimensions != local_dimensions {
            return Err(Error::Protocol(
                "media upload dimensions do not match the uploaded image".into(),
            ));
        }
        let (width, height) = dimensions
            .map(|(width, height)| (Some(width), Some(height)))
            .unwrap_or((None, None));
        let kind = if mime.starts_with("image/") {
            MediaKind::Image
        } else if mime == "video/mp4" {
            MediaKind::Video
        } else {
            MediaKind::File
        };
        Ok(Attachment {
            index: 0,
            url,
            mime: mime.to_owned(),
            sha256,
            size: metadata.len(),
            width,
            height,
            alt: None,
            blurhash: descriptor.blurhash,
            thumb: match descriptor.thumb {
                Some(value) => Some(
                    validate_media_url(&value, &self.base, None, true).map_err(Error::Protocol)?,
                ),
                None => None,
            },
            poster: None,
            filename,
            duration_millis: descriptor
                .duration
                .filter(|value| value.is_finite() && *value > 0.0)
                .map(|value| (value * 1_000.0) as u64),
            kind,
            spoiler: false,
            error: None,
        })
    }

    fn relay_avatar_url(&self, source: &str) -> Result<(String, String)> {
        // Kind-0 `picture` is a public URL field. Do not turn a relative value
        // into an authorized active-relay path on its behalf.
        Url::parse(source)
            .map_err(|_| Error::Protocol("profile avatar is not an active-relay image".into()))?;
        let url = validate_media_url(source, &self.base, None, false)
            .map_err(|_| Error::Protocol("profile avatar is not an active-relay image".into()))?;
        let parsed = Url::parse(&url)
            .map_err(|_| Error::Protocol("profile avatar is not an active-relay image".into()))?;
        let extension = parsed
            .path()
            .rsplit_once('.')
            .map(|(_, extension)| extension);
        if !matches!(extension, Some("jpg" | "jpeg" | "png" | "gif" | "webp")) {
            return Err(Error::Protocol(
                "profile avatar is not an active-relay image".into(),
            ));
        }
        let sha256 = hash_from_path(&url)
            .ok_or_else(|| Error::Protocol("profile avatar is not an active-relay image".into()))?;
        Ok((url, sha256))
    }

    async fn send_get(&self, url: &str, sha256: &str) -> Result<reqwest::Response> {
        let auth = self.get_auth(sha256).await?;
        let mut attempt = 0_u32;
        loop {
            let response = self
                .client
                .get(url)
                .header(header::AUTHORIZATION, &auth)
                .header(header::ACCEPT_ENCODING, "identity")
                .send()
                .await;
            match response {
                Ok(response)
                    if attempt < 2
                        && (response.status() == StatusCode::TOO_MANY_REQUESTS
                            || response.status().is_server_error()) =>
                {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_millis(200 * (1 << attempt))).await;
                }
                Ok(response) if response.status().is_redirection() => {
                    return Err(Error::Network("media redirect refused".into()));
                }
                Ok(response) if !response.status().is_success() => {
                    return Err(status_error(response.status()));
                }
                Ok(response) => return Ok(response),
                Err(error) if attempt < 2 && (error.is_connect() || error.is_timeout()) => {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_millis(200 * (1 << attempt))).await;
                }
                Err(error) => return Err(Error::Network(error.to_string())),
            }
        }
    }

    async fn send_upload(
        &self,
        url: Url,
        auth: &str,
        mime: &str,
        sha256: &str,
        path: &Path,
        length: u64,
    ) -> Result<reqwest::Response> {
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|error| Error::io(path, error))?;
        let body = reqwest::Body::wrap_stream(tokio_util::io::ReaderStream::new(file));
        self.client
            .put(url)
            .header(header::AUTHORIZATION, auth)
            .header(header::CONTENT_TYPE, mime)
            .header(header::CONTENT_LENGTH, length)
            .header("X-SHA-256", sha256)
            .body(body)
            .send()
            .await
            .map_err(|error| Error::Network(error.to_string()))
    }

    async fn get_auth(&self, sha256: &str) -> Result<String> {
        let now = Timestamp::now().as_secs();
        let tags = vec![
            Tag::parse(["t", "get"]).map_err(|error| Error::Protocol(error.to_string()))?,
            Tag::parse(["x", sha256]).map_err(|error| Error::Protocol(error.to_string()))?,
            Tag::parse(["expiration", &(now + 600).to_string()])
                .map_err(|error| Error::Protocol(error.to_string()))?,
            Tag::parse(["server", &self.authority])
                .map_err(|error| Error::Protocol(error.to_string()))?,
        ];
        let event = self
            .signer
            .sign(EventBuilder::new(Kind::Custom(24_242), "Get buzz-media").tags(tags))
            .await?;
        Ok(format!("Nostr {}", URL_SAFE_NO_PAD.encode(event.as_json())))
    }

    async fn upload_auth(&self, sha256: &str) -> Result<String> {
        let now = Timestamp::now().as_secs();
        let tags = vec![
            Tag::parse(["t", "upload"]).map_err(|error| Error::Protocol(error.to_string()))?,
            Tag::parse(["x", sha256]).map_err(|error| Error::Protocol(error.to_string()))?,
            Tag::parse(["expiration", &(now + 300).to_string()])
                .map_err(|error| Error::Protocol(error.to_string()))?,
            Tag::parse(["server", &self.authority])
                .map_err(|error| Error::Protocol(error.to_string()))?,
        ];
        let event = self
            .signer
            .sign(EventBuilder::new(Kind::Custom(24_242), "Upload buzz-media").tags(tags))
            .await?;
        Ok(format!("Nostr {}", URL_SAFE_NO_PAD.encode(event.as_json())))
    }
}

pub async fn verify_file(path: &Path, expected_hash: &str, expected_size: u64) -> Result<()> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|error| Error::io(path, error))?;
    if metadata.len() != expected_size || hash_file(path).await? != expected_hash {
        return Err(Error::Protocol(
            "cached media failed integrity verification".into(),
        ));
    }
    Ok(())
}

pub async fn verify_poster_file(path: &Path, expected_hash: &str) -> Result<VerifiedPoster> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|error| Error::io(path, error))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > IMAGE_LIMIT {
        return Err(Error::Protocol(
            "cached video poster exceeds the image limit".into(),
        ));
    }
    if hash_file(path).await? != expected_hash {
        return Err(Error::Protocol(
            "cached video poster failed integrity verification".into(),
        ));
    }
    let mime = infer::get_from_path(path)
        .map_err(|error| Error::io(path, error))?
        .map(|kind| kind.mime_type())
        .filter(|mime| supported_image_mime(mime))
        .ok_or_else(|| Error::Protocol("cached video poster type is unsupported".into()))?;
    if mime == "image/gif" && metadata.len() > GIF_LIMIT {
        return Err(Error::Protocol(
            "cached video poster exceeds the GIF limit".into(),
        ));
    }
    Ok(VerifiedPoster {
        path: path.to_path_buf(),
        sha256: expected_hash.to_owned(),
        size: metadata.len(),
        mime: mime.to_owned(),
    })
}

async fn hash_file(path: &Path) -> Result<String> {
    use tokio::io::AsyncReadExt as _;
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| Error::io(path, error))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|error| Error::io(path, error))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

fn parse_dimensions(value: &str) -> Option<(u32, u32)> {
    let (width, height) = value.split_once('x')?;
    let width = width.parse::<u32>().ok()?;
    let height = height.parse::<u32>().ok()?;
    (width > 0
        && height > 0
        && width <= 16_384
        && height <= 16_384
        && u64::from(width) * u64::from(height) <= 25_000_000)
        .then_some((width, height))
}

async fn read_response_limited(response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(Error::Protocol(
            "media response metadata is too large".into(),
        ));
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| Error::Network(error.to_string()))?;
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(Error::Protocol(
                "media response metadata is too large".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn validate_identity_encoding(response: &reqwest::Response) -> Result<()> {
    if response
        .headers()
        .get(header::CONTENT_ENCODING)
        .is_some_and(|value| !value.as_bytes().eq_ignore_ascii_case(b"identity"))
    {
        return Err(Error::Protocol(
            "encoded media responses are not accepted".into(),
        ));
    }
    Ok(())
}

fn normalized_content_type(value: &header::HeaderValue) -> &str {
    value
        .to_str()
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
}

fn supported_image_mime(mime: &str) -> bool {
    matches!(
        mime,
        "image/jpeg" | "image/png" | "image/gif" | "image/webp"
    )
}

fn transfer_limit(attachment: &Attachment) -> u64 {
    match attachment.kind {
        MediaKind::Image if attachment.mime == "image/gif" => GIF_LIMIT,
        MediaKind::Image => IMAGE_LIMIT,
        MediaKind::Video => VIDEO_LIMIT,
        MediaKind::File | MediaKind::Unsupported => FILE_LIMIT,
    }
}

fn mime_limit(mime: &str) -> u64 {
    match mime {
        "image/gif" => GIF_LIMIT,
        value if value.starts_with("image/") => IMAGE_LIMIT,
        "video/mp4" => VIDEO_LIMIT,
        _ => FILE_LIMIT,
    }
}

fn status_error(status: StatusCode) -> Error {
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        Error::Access(format!("media access denied ({status})"))
    } else {
        Error::Network(format!("media request failed with {status}"))
    }
}
