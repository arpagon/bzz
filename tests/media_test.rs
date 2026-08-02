use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use bzz::{
    auth::signer::SignerHandle,
    config::{CommunityConfig, Config, IdentityConfig, KeyBackend, MediaProtocol},
    media::{
        Attachment, DraftAttachment, MediaKind, PendingAttachment,
        client::MediaClient,
        runtime::{MediaRuntime, MediaState},
    },
    paths::Paths,
    store::{Store, writer::StoreHandle},
};
use nostr::{Event, EventBuilder, JsonUtil as _, Keys, Kind, Tag};
use sha2::{Digest as _, Sha256};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
};
use url::Url;
use uuid::Uuid;

async fn serve_once(body: Vec<u8>, mime: &'static str) -> (Url, Arc<Mutex<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let captured = Arc::new(Mutex::new(String::new()));
    let request = captured.clone();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = vec![0_u8; 32 * 1024];
        let read = stream.read(&mut bytes).await.unwrap();
        *request.lock().unwrap() = String::from_utf8_lossy(&bytes[..read]).into_owned();
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(header.as_bytes()).await.unwrap();
        stream.write_all(&body).await.unwrap();
    });
    (Url::parse(&format!("http://{address}/")).unwrap(), captured)
}

async fn serve_upload_once(body: Vec<u8>) -> (Url, Arc<Mutex<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let base = Url::parse(&format!("http://{address}/")).unwrap();
    let captured = Arc::new(Mutex::new(String::new()));
    let request = captured.clone();
    let response_base = base.clone();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut received = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            let read = stream.read(&mut chunk).await.unwrap();
            if read == 0 {
                break;
            }
            received.extend_from_slice(&chunk[..read]);
            if let Some(header_end) = received.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&received[..header_end + 4]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|value| value.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if received.len() >= header_end + 4 + length {
                    break;
                }
            }
        }
        *request.lock().unwrap() = String::from_utf8_lossy(&received).into_owned();
        let hash = hex::encode(Sha256::digest(&body));
        let descriptor = serde_json::json!({
            "url": response_base.join(&format!("media/{hash}.bin")).unwrap().to_string(),
            "sha256": hash,
            "size": body.len(),
            "type": "application/octet-stream",
            "uploaded": 1
        })
        .to_string();
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            descriptor.len()
        );
        stream.write_all(header.as_bytes()).await.unwrap();
        stream.write_all(descriptor.as_bytes()).await.unwrap();
    });
    (base, captured)
}

fn attachment(base: &Url, body: &[u8], mime: &str, kind: MediaKind) -> Attachment {
    let sha256 = hex::encode(Sha256::digest(body));
    Attachment {
        index: 0,
        url: base
            .join(&format!("media/{sha256}.bin"))
            .unwrap()
            .to_string(),
        mime: mime.into(),
        sha256,
        size: body.len() as u64,
        width: None,
        height: None,
        alt: None,
        blurhash: None,
        thumb: None,
        poster: None,
        filename: Some("safe.bin".into()),
        duration_millis: None,
        kind,
        spoiler: false,
        error: None,
    }
}

#[tokio::test]
async fn authenticated_media_download_is_hash_verified_and_origin_bound() {
    let body = b"generic attachment".to_vec();
    let (base, captured) = serve_once(body.clone(), "application/octet-stream").await;
    let signer = SignerHandle::spawn(Keys::generate());
    let client = MediaClient::new(base.clone(), base.authority().to_owned(), signer, 1).unwrap();
    let descriptor = attachment(&base, &body, "application/octet-stream", MediaKind::File);
    let temporary = TempDir::new().unwrap();
    let destination = temporary.path().join("blob.bin");
    client.fetch(&descriptor, &destination).await.unwrap();
    assert_eq!(tokio::fs::read(destination).await.unwrap(), body);
    let request = captured.lock().unwrap().clone();
    let lower = request.to_ascii_lowercase();
    assert!(lower.starts_with("get /media/"));
    assert!(lower.contains("authorization: nostr "));
    assert!(lower.contains("accept-encoding: identity"));
    let encoded = request
        .lines()
        .find(|line| {
            line.to_ascii_lowercase()
                .starts_with("authorization: nostr ")
        })
        .and_then(|line| line.split_whitespace().nth(2))
        .unwrap();
    let event = Event::from_json(URL_SAFE_NO_PAD.decode(encoded).unwrap()).unwrap();
    assert_eq!(event.kind, Kind::Custom(24_242));
    assert!(event.tags.iter().any(|tag| tag.as_slice() == ["t", "get"]));
    assert!(
        event
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["x", descriptor.sha256.as_str()])
    );
}

#[tokio::test]
async fn video_posters_are_hash_bound_and_magic_verified() {
    let image = image::DynamicImage::new_rgb8(3, 2);
    let mut encoded = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let body = encoded.into_inner();
    let hash = hex::encode(Sha256::digest(&body));
    let (base, captured) = serve_once(body.clone(), "image/png").await;
    let client = MediaClient::new(
        base.clone(),
        base.authority().to_owned(),
        SignerHandle::spawn(Keys::generate()),
        1,
    )
    .unwrap();
    let temporary = TempDir::new().unwrap();
    let destination = temporary.path().join("poster.png");
    let poster_url = base.join(&format!("media/{hash}.png")).unwrap();
    let verified = client
        .fetch_poster(poster_url.as_str(), &hash, &destination)
        .await
        .unwrap();
    assert_eq!(verified.sha256, hash);
    assert_eq!(verified.mime, "image/png");
    assert_eq!(verified.size, body.len() as u64);
    assert_eq!(tokio::fs::read(destination).await.unwrap(), body);
    let request = captured.lock().unwrap().clone();
    assert!(request.to_ascii_lowercase().starts_with("get /media/"));
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: nostr ")
    );
}

#[tokio::test]
async fn startup_repair_drops_metadata_for_missing_cache_files() {
    let temporary = TempDir::new().unwrap();
    let paths = Paths {
        config_dir: temporary.path().join("config"),
        data_dir: temporary.path().join("data"),
        cache_dir: temporary.path().join("cache"),
    };
    paths.ensure().unwrap();
    let keys = Keys::generate();
    let identity = Uuid::new_v4();
    let community = Uuid::new_v4();
    let config = Config {
        identities: vec![IdentityConfig {
            id: identity,
            label: "repair".into(),
            pubkey: keys.public_key().to_hex(),
            backend: KeyBackend::Keychain,
            key_ref: "repair".into(),
        }],
        communities: vec![CommunityConfig {
            id: community,
            label: "repair".into(),
            relay_url: "wss://buzz.example/".into(),
            identity_id: identity,
            allow_insecure_localhost: false,
            theme: None,
        }],
        default_community: Some(community),
        ..Config::default()
    };
    let mut store = Store::open(paths.database_file()).unwrap();
    store.sync_config(&config).unwrap();
    store
        .record_media_cache(community, &"9".repeat(64), "image/png", 42, None, None)
        .unwrap();
    let handle = StoreHandle::spawn(store).unwrap();
    let runtime = MediaRuntime::new(config.media.clone(), &paths, handle);
    assert_eq!(runtime.repair_cache_metadata().await.unwrap(), 1);
}

#[tokio::test]
async fn verified_offline_images_are_prepared_off_the_ui_thread() {
    let temporary = TempDir::new().unwrap();
    let paths = Paths {
        config_dir: temporary.path().join("config"),
        data_dir: temporary.path().join("data"),
        cache_dir: temporary.path().join("cache"),
    };
    paths.ensure().unwrap();
    let keys = Keys::generate();
    let identity = Uuid::new_v4();
    let community = Uuid::new_v4();
    let config = Config {
        identities: vec![IdentityConfig {
            id: identity,
            label: "offline-media".into(),
            pubkey: keys.public_key().to_hex(),
            backend: KeyBackend::Keychain,
            key_ref: "offline-media".into(),
        }],
        communities: vec![CommunityConfig {
            id: community,
            label: "offline-media".into(),
            relay_url: "wss://buzz.example/".into(),
            identity_id: identity,
            allow_insecure_localhost: false,
            theme: None,
        }],
        default_community: Some(community),
        ..Config::default()
    };
    let mut store = Store::open(paths.database_file()).unwrap();
    store.sync_config(&config).unwrap();
    let handle = StoreHandle::spawn(store).unwrap();
    let mut media_config = config.media.clone();
    media_config.protocol = MediaProtocol::Halfblocks;
    let mut runtime = MediaRuntime::new(media_config, &paths, handle);
    runtime.select_cached(community);

    let image = image::DynamicImage::new_rgb8(2, 2);
    let mut encoded = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let body = encoded.into_inner();
    let hash = hex::encode(Sha256::digest(&body));
    let attachment = Attachment {
        index: 0,
        url: format!("https://buzz.example/media/{hash}.png"),
        mime: "image/png".into(),
        sha256: hash,
        size: body.len() as u64,
        width: Some(2),
        height: Some(2),
        alt: None,
        blurhash: None,
        thumb: None,
        poster: None,
        filename: Some("offline.png".into()),
        duration_millis: None,
        kind: MediaKind::Image,
        spoiler: false,
        error: None,
    };
    let cache = runtime.cache_path(community, &attachment);
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::write(&cache, body).unwrap();
    runtime.request_inline(&attachment, 20, false);
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        runtime.poll();
        if matches!(runtime.state(&attachment, 20), Some(MediaState::Ready(_))) {
            return;
        }
    }
    panic!("offline image was not prepared");
}

#[tokio::test]
async fn verified_offline_video_posters_are_prepared_for_preview() {
    let temporary = TempDir::new().unwrap();
    let paths = Paths {
        config_dir: temporary.path().join("config"),
        data_dir: temporary.path().join("data"),
        cache_dir: temporary.path().join("cache"),
    };
    paths.ensure().unwrap();
    let keys = Keys::generate();
    let identity = Uuid::new_v4();
    let community = Uuid::new_v4();
    let config = Config {
        identities: vec![IdentityConfig {
            id: identity,
            label: "offline-poster".into(),
            pubkey: keys.public_key().to_hex(),
            backend: KeyBackend::Keychain,
            key_ref: "offline-poster".into(),
        }],
        communities: vec![CommunityConfig {
            id: community,
            label: "offline-poster".into(),
            relay_url: "wss://buzz.example/".into(),
            identity_id: identity,
            allow_insecure_localhost: false,
            theme: None,
        }],
        default_community: Some(community),
        ..Config::default()
    };
    let mut store = Store::open(paths.database_file()).unwrap();
    store.sync_config(&config).unwrap();
    let handle = StoreHandle::spawn(store).unwrap();
    let mut media_config = config.media.clone();
    media_config.protocol = MediaProtocol::Halfblocks;
    let mut runtime = MediaRuntime::new(media_config, &paths, handle);
    runtime.select_cached(community);

    let image = image::DynamicImage::new_rgb8(3, 2);
    let mut encoded = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let body = encoded.into_inner();
    let poster_hash = hex::encode(Sha256::digest(&body));
    let video_hash = "4".repeat(64);
    let attachment = Attachment {
        index: 0,
        url: format!("https://buzz.example/media/{video_hash}.mp4"),
        mime: "video/mp4".into(),
        sha256: video_hash,
        size: 42,
        width: None,
        height: None,
        alt: Some("generated video".into()),
        blurhash: None,
        thumb: None,
        poster: Some(format!("https://buzz.example/media/{poster_hash}.png")),
        filename: Some("generated.mp4".into()),
        duration_millis: Some(1_000),
        kind: MediaKind::Video,
        spoiler: false,
        error: None,
    };
    let cache = paths
        .media_cache_dir()
        .join(community.to_string())
        .join(format!("{poster_hash}.png"));
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::write(&cache, body).unwrap();
    runtime.request_poster(&attachment, 20);
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        runtime.poll();
        if matches!(
            runtime.poster_state(&attachment, 20),
            Some(MediaState::Ready(_))
        ) {
            return;
        }
    }
    panic!("offline video poster was not prepared");
}

#[tokio::test]
async fn upload_streams_exact_bytes_with_hash_bound_auth() {
    let body = b"upload body".to_vec();
    let (base, captured) = serve_upload_once(body.clone()).await;
    let client = MediaClient::new(
        base.clone(),
        base.authority().to_owned(),
        SignerHandle::spawn(Keys::generate()),
        1,
    )
    .unwrap();
    let temporary = TempDir::new().unwrap();
    let source = temporary.path().join("upload.bin");
    tokio::fs::write(&source, &body).await.unwrap();
    let uploaded = client
        .upload(
            &source,
            "application/octet-stream",
            Some("upload.bin".into()),
        )
        .await
        .unwrap();
    assert_eq!(uploaded.sha256, hex::encode(Sha256::digest(&body)));
    assert_eq!(uploaded.filename.as_deref(), Some("upload.bin"));
    let request = captured.lock().unwrap().clone().to_ascii_lowercase();
    assert!(request.starts_with("put /upload "));
    assert!(request.contains("authorization: nostr "));
    assert!(request.contains(&format!("x-sha-256: {}", uploaded.sha256)));
}

#[test]
fn stored_imeta_is_projected_and_the_generated_body_line_is_hidden() {
    let keys = Keys::generate();
    let identity = Uuid::new_v4();
    let community = Uuid::new_v4();
    let channel = Uuid::new_v4();
    let config = Config {
        identities: vec![IdentityConfig {
            id: identity,
            label: "media-test".into(),
            pubkey: keys.public_key().to_hex(),
            backend: KeyBackend::Keychain,
            key_ref: "media-test".into(),
        }],
        communities: vec![CommunityConfig {
            id: community,
            label: "media-test".into(),
            relay_url: "wss://buzz.example/".into(),
            identity_id: identity,
            allow_insecure_localhost: false,
            theme: None,
        }],
        default_community: Some(community),
        ..Config::default()
    };
    let hash = "a".repeat(64);
    let media_url = format!("https://buzz.example/media/{hash}.png");
    let channel_id = channel.to_string();
    let tags = vec![
        Tag::parse(["h", &channel_id]).unwrap(),
        Tag::parse(vec![
            "imeta".to_owned(),
            format!("url {media_url}"),
            "m image/png".into(),
            format!("x {hash}"),
            "size 68".into(),
            "dim 1x1".into(),
        ])
        .unwrap(),
    ];
    let event = EventBuilder::new(Kind::Custom(9), format!("hello\n![image]({media_url})"))
        .tags(tags)
        .sign_with_keys(&keys)
        .unwrap();
    let mut store = Store::open_memory().unwrap();
    store.sync_config(&config).unwrap();
    store.apply_event(community, &event).unwrap();
    let messages = store.messages(community, channel, 10).unwrap();
    assert_eq!(messages[0].content, "hello");
    assert_eq!(messages[0].attachments.len(), 1);
    assert!(messages[0].attachments[0].valid());

    let pending = DraftAttachment::Pending(PendingAttachment {
        cache_name: format!("{hash}.png"),
        mime: "image/png".into(),
        filename: "generated.png".into(),
        sha256: hash,
        size: 68,
    });
    store
        .save_draft_with_media(
            community,
            channel,
            None,
            "draft",
            std::slice::from_ref(&pending),
        )
        .unwrap();
    let (body, attachments) = store.draft_with_media(community, channel, None).unwrap();
    assert_eq!(body, "draft");
    assert_eq!(attachments, vec![pending]);
}

#[tokio::test]
async fn mismatched_media_never_reaches_the_cache() {
    let expected = b"expected".to_vec();
    let served = b"tampered".to_vec();
    let (base, _) = serve_once(served, "application/octet-stream").await;
    let client = MediaClient::new(
        base.clone(),
        base.authority().to_owned(),
        SignerHandle::spawn(Keys::generate()),
        1,
    )
    .unwrap();
    let descriptor = attachment(
        &base,
        &expected,
        "application/octet-stream",
        MediaKind::File,
    );
    let temporary = TempDir::new().unwrap();
    let destination = temporary.path().join("blob.bin");
    assert!(client.fetch(&descriptor, &destination).await.is_err());
    assert!(!Path::new(&destination).exists());
    assert!(
        std::fs::read_dir(temporary.path())
            .unwrap()
            .next()
            .is_none()
    );
}
