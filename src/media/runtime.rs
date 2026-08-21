use std::{
    collections::{HashMap, HashSet},
    io::IsTerminal as _,
    path::PathBuf,
    sync::Arc,
};

use ratatui::layout::Size;
use ratatui_image::{
    FontSize, Resize,
    picker::{Picker, ProtocolType},
    sliced::SlicedProtocol,
};
use sha2::{Digest as _, Sha256};
use tokio::sync::{Semaphore, mpsc};
use uuid::Uuid;

use crate::{
    config::{MediaAutoload, MediaConfig, MediaProtocol, ProfileAvatars},
    error::{Error, Result},
    paths::Paths,
    store::writer::StoreHandle,
};

use super::{
    avatar::AvatarClient,
    client::MediaClient,
    decode::decode_image,
    model::{Attachment, MediaKind},
};

pub enum MediaState {
    Loading,
    Ready(Arc<SlicedProtocol>),
    Failed(String),
}

enum Completed {
    Ready {
        generation: u64,
        key: String,
        protocol: SlicedProtocol,
        weight: u64,
    },
    Failed {
        generation: u64,
        key: String,
        message: String,
    },
}

pub struct MediaRuntime {
    config: MediaConfig,
    profile_avatars: ProfileAvatars,
    community_id: Option<Uuid>,
    identity_id: Option<Uuid>,
    cache_root: PathBuf,
    avatar_cache_root: PathBuf,
    picker: Picker,
    protocol_name: String,
    generation: u64,
    client: Option<MediaClient>,
    avatar_client: AvatarClient,
    avatar_network_enabled: bool,
    store: StoreHandle,
    states: HashMap<String, MediaState>,
    in_flight: HashSet<String>,
    weights: HashMap<String, u64>,
    last_used: HashMap<String, u64>,
    use_clock: u64,
    completed_tx: mpsc::Sender<Completed>,
    completed_rx: mpsc::Receiver<Completed>,
    decode_slots: Arc<Semaphore>,
}

impl MediaRuntime {
    pub fn new(
        config: MediaConfig,
        profile_avatars: ProfileAvatars,
        paths: &Paths,
        store: StoreHandle,
    ) -> Self {
        let (completed_tx, completed_rx) = mpsc::channel(64);
        let cache_root = paths.media_cache_dir();
        let avatar_cache_root = paths.avatar_cache_dir();
        cleanup_partial_files(&cache_root);
        cleanup_partial_files(&avatar_cache_root);
        Self {
            decode_slots: Arc::new(Semaphore::new(config.decode_concurrency)),
            avatar_client: AvatarClient::new(config.download_concurrency),
            config,
            profile_avatars,
            community_id: None,
            identity_id: None,
            cache_root,
            avatar_cache_root,
            picker: Picker::halfblocks(),
            protocol_name: "halfblocks".into(),
            generation: 0,
            client: None,
            avatar_network_enabled: false,
            store,
            states: HashMap::new(),
            in_flight: HashSet::new(),
            weights: HashMap::new(),
            last_used: HashMap::new(),
            use_clock: 0,
            completed_tx,
            completed_rx,
        }
    }

    pub fn bind(&mut self, community_id: Uuid, identity_id: Uuid, client: MediaClient) {
        if self.community_id != Some(community_id) || self.identity_id != Some(identity_id) {
            self.generation = self.generation.wrapping_add(1);
            self.clear_memory_state();
        }
        self.community_id = Some(community_id);
        self.identity_id = Some(identity_id);
        self.client = Some(client);
        self.avatar_network_enabled = true;
    }

    pub fn select_cached(&mut self, community_id: Uuid, identity_id: Uuid) {
        if self.community_id != Some(community_id) || self.identity_id != Some(identity_id) {
            self.generation = self.generation.wrapping_add(1);
            self.clear_memory_state();
        }
        self.community_id = Some(community_id);
        self.identity_id = Some(identity_id);
        self.client = None;
        self.avatar_network_enabled = false;
    }

    pub fn unbind(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.community_id = None;
        self.identity_id = None;
        self.client = None;
        self.avatar_network_enabled = false;
        self.clear_memory_state();
    }

    pub fn initialize_terminal(&mut self) {
        if !self.config.enabled || self.config.protocol == MediaProtocol::Off {
            self.protocol_name = "off".into();
            self.generation = self.generation.wrapping_add(1);
            self.clear_memory_state();
            return;
        }
        let mut picker = safe_picker();
        let forced = match self.config.protocol {
            MediaProtocol::Auto => auto_protocol_from_environment(),
            MediaProtocol::Kitty => Some(ProtocolType::Kitty),
            MediaProtocol::Sixel => Some(ProtocolType::Sixel),
            MediaProtocol::Iterm2 => Some(ProtocolType::Iterm2),
            MediaProtocol::Halfblocks => Some(ProtocolType::Halfblocks),
            MediaProtocol::Off => None,
        };
        if let Some(protocol) = forced {
            picker.set_protocol_type(protocol);
        }
        self.protocol_name = format!("{:?}", picker.protocol_type()).to_ascii_lowercase();
        self.generation = self.generation.wrapping_add(1);
        self.picker = picker;
        self.clear_memory_state();
    }

    pub fn protocol_name(&self) -> &str {
        &self.protocol_name
    }

    /// Returns a ready local rendering of a remote profile photograph. The
    /// caller still renders the author label and textual marker beside it.
    pub fn avatar_state(&self, pubkey: &str, picture: &str, width: u16) -> Option<&MediaState> {
        self.states.get(&avatar_key(pubkey, picture, width))
    }

    /// Queues a bounded profile-image retrieval. Public third-party pictures
    /// use the credential-free avatar client; canonical image paths belonging
    /// to the active community relay use a narrowly scoped media-read token.
    /// It is a no-op in locked/cache-only mode and on terminals that cannot
    /// safely display an allocated raster row.
    pub fn request_avatar(&mut self, pubkey: &str, picture: &str, width: u16) {
        if !should_request_avatar(
            self.profile_avatars,
            self.avatar_network_enabled,
            self.graphics_avatar_protocol(),
            width,
        ) {
            return;
        }
        let (Some(community), Some(identity)) = (self.community_id, self.identity_id) else {
            return;
        };
        let key = avatar_key(pubkey, picture, width);
        if self.states.contains_key(&key) {
            self.touch(&key);
            return;
        }
        if self.in_flight.len() >= 64 || !self.in_flight.insert(key.clone()) {
            return;
        }
        self.states.insert(key.clone(), MediaState::Loading);
        let destination = self
            .avatar_cache_root
            .join(community.to_string())
            .join(identity.to_string())
            .join(format!(
                "{}-{}.avatar",
                profile_digest(pubkey),
                url_digest(picture)
            ));
        let avatar_client = self.avatar_client.clone();
        let relay_media = self.client.clone();
        let picker = self.picker.clone();
        let decode_slots = self.decode_slots.clone();
        let tx = self.completed_tx.clone();
        let generation = self.generation;
        let memory_limit = self.config.memory_cache_bytes;
        let picture = picture.to_owned();
        tokio::spawn(async move {
            let result: Result<(SlicedProtocol, u64)> = async {
                let path = if destination.exists() {
                    destination.clone()
                } else if let Some(media) = relay_media
                    .as_ref()
                    .filter(|media| media.is_relay_profile_avatar(&picture))
                {
                    media.fetch_profile_avatar(&picture, &destination).await?
                } else {
                    avatar_client.fetch(&picture, &destination).await?
                };
                let _permit = decode_slots
                    .acquire_owned()
                    .await
                    .map_err(|_| Error::Protocol("profile avatar decode queue stopped".into()))?;
                let decode_path = path.clone();
                let (protocol, weight) = tokio::task::spawn_blocking(move || {
                    let image = decode_image(&decode_path)?;
                    let allocation = Size::new(width, 3);
                    // The protocol retains its resized terminal representation,
                    // not the full decoded source photograph. Charging the
                    // original dimensions makes a few ordinary avatars evict
                    // one another during redraws, which requeues them and
                    // produces visible flicker.
                    let weight = prepared_weight(&picker, allocation);
                    if memory_limit == 0 || weight > memory_limit {
                        return Err(Error::Protocol(
                            "prepared profile avatar exceeds the memory cache limit".into(),
                        ));
                    }
                    let protocol = SlicedProtocol::new_with_resize(
                        &picker,
                        image,
                        allocation,
                        Resize::Fit(None),
                    )
                    .map_err(|error| Error::Protocol(error.to_string()))?;
                    Ok((protocol, weight))
                })
                .await
                .map_err(|_| Error::Protocol("profile avatar worker stopped".into()))??;
                Ok((protocol, weight))
            }
            .await;
            if result.is_err() {
                let _ = tokio::fs::remove_file(&destination).await;
            }
            let completed = match result {
                Ok((protocol, weight)) => Completed::Ready {
                    generation,
                    key,
                    protocol,
                    weight,
                },
                Err(error) => Completed::Failed {
                    generation,
                    key,
                    message: safe_error(&error),
                },
            };
            let _ = tx.send(completed).await;
        });
    }

    fn graphics_avatar_protocol(&self) -> bool {
        self.config.enabled
            && self.config.protocol != MediaProtocol::Off
            && matches!(
                self.picker.protocol_type(),
                ProtocolType::Kitty | ProtocolType::Sixel | ProtocolType::Iterm2
            )
    }

    pub fn state(&self, attachment: &Attachment, width: u16) -> Option<&MediaState> {
        self.states.get(&cache_key(attachment, width))
    }

    pub fn poster_state(&self, attachment: &Attachment, width: u16) -> Option<&MediaState> {
        poster_details(attachment).and_then(|(_, hash)| self.states.get(&format!("{hash}:{width}")))
    }

    pub fn request_poster(&mut self, attachment: &Attachment, width: u16) {
        if !self.config.enabled
            || self.config.protocol == MediaProtocol::Off
            || attachment.kind != MediaKind::Video
            || !attachment.valid()
            || width < 2
        {
            return;
        }
        let Some((url, hash)) = poster_details(attachment) else {
            return;
        };
        let Some(community) = self.community_id else {
            return;
        };
        let key = format!("{hash}:{width}");
        if self.states.contains_key(&key) {
            self.touch(&key);
            return;
        }
        if self.in_flight.len() >= 64 || !self.in_flight.insert(key.clone()) {
            return;
        }
        self.states.insert(key.clone(), MediaState::Loading);
        let extension = url
            .rsplit_once('.')
            .map(|(_, extension)| extension)
            .filter(|extension| {
                extension.len() <= 16 && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
            .unwrap_or("bin");
        let destination = self
            .cache_root
            .join(community.to_string())
            .join(format!("{hash}.{extension}"));
        let client = self.client.clone();
        if !destination.exists() && client.is_none() {
            self.states.insert(
                key.clone(),
                MediaState::Failed("video poster is not available in the verified cache".into()),
            );
            self.in_flight.remove(&key);
            return;
        }
        let picker = self.picker.clone();
        let max_rows = self.config.max_inline_rows;
        let memory_limit = self.config.memory_cache_bytes;
        let disk_limit = self.config.disk_cache_bytes;
        let decode_slots = self.decode_slots.clone();
        let tx = self.completed_tx.clone();
        let generation = self.generation;
        let store = self.store.clone();
        let cache_root = self.cache_root.clone();
        tokio::spawn(async move {
            let result: Result<(SlicedProtocol, u64)> = async {
                let verified = if destination.exists() {
                    super::client::verify_poster_file(&destination, &hash).await?
                } else {
                    client
                        .ok_or_else(|| Error::Locked("media signer is unavailable".into()))?
                        .fetch_poster(&url, &hash, &destination)
                        .await?
                };
                touch_cache_file(&verified.path)?;
                let removed = prune_cache(&cache_root, disk_limit, &verified.path).await?;
                if !removed.is_empty() {
                    store
                        .call(move |store| store.delete_media_cache_entries(&removed))
                        .await?;
                }
                let ephemeral = disk_limit == 0 || verified.size > disk_limit;
                if !ephemeral {
                    let record_hash = verified.sha256.clone();
                    let record_mime = verified.mime.clone();
                    let record_size = verified.size;
                    store
                        .call(move |store| {
                            store.record_media_cache(
                                community,
                                &record_hash,
                                &record_mime,
                                record_size,
                                None,
                                None,
                            )
                        })
                        .await?;
                }
                let _permit = decode_slots
                    .acquire_owned()
                    .await
                    .map_err(|_| Error::Protocol("media decode queue stopped".into()))?;
                let decode_path = verified.path.clone();
                let (protocol, weight) = tokio::task::spawn_blocking(move || {
                    let image = decode_image(&decode_path)?;
                    let allocation = Size::new(width, max_rows);
                    let weight = prepared_weight(&picker, allocation);
                    if memory_limit == 0 || weight > memory_limit {
                        return Err(Error::Protocol(
                            "prepared video poster exceeds the memory cache limit".into(),
                        ));
                    }
                    let protocol = SlicedProtocol::new_with_resize(
                        &picker,
                        image,
                        allocation,
                        Resize::Fit(None),
                    )
                    .map_err(|error| Error::Protocol(error.to_string()))?;
                    Ok((protocol, weight))
                })
                .await
                .map_err(|_| Error::Protocol("video poster worker stopped".into()))??;
                if ephemeral {
                    let _ = tokio::fs::remove_file(&verified.path).await;
                }
                Ok((protocol, weight))
            }
            .await;
            let completed = match result {
                Ok((protocol, weight)) => Completed::Ready {
                    generation,
                    key,
                    protocol,
                    weight,
                },
                Err(error) => Completed::Failed {
                    generation,
                    key,
                    message: safe_error(&error),
                },
            };
            let _ = tx.send(completed).await;
        });
    }

    pub fn request_inline(&mut self, attachment: &Attachment, width: u16, reveal: bool) {
        if !self.config.enabled
            || self.config.protocol == MediaProtocol::Off
            || (!reveal && self.config.autoload != MediaAutoload::Visible)
            || attachment.kind != MediaKind::Image
            || !attachment.valid()
            || (attachment.spoiler && !reveal)
            || (!reveal && attachment.size > self.config.auto_download_bytes)
            || width < 2
        {
            return;
        }
        let Some(community) = self.community_id else {
            return;
        };
        let client = self.client.clone();
        let key = cache_key(attachment, width);
        if self.states.contains_key(&key) {
            self.touch(&key);
            return;
        }
        if self.in_flight.len() >= 64 || !self.in_flight.insert(key.clone()) {
            return;
        }
        self.states.insert(key.clone(), MediaState::Loading);
        let destination = self.cache_path(community, attachment);
        if !destination.exists() && client.is_none() {
            self.states.insert(
                key.clone(),
                MediaState::Failed("media is not available in the verified offline cache".into()),
            );
            self.in_flight.remove(&key);
            return;
        }
        let attachment = attachment.clone();
        let picker = self.picker.clone();
        let max_rows = self.config.max_inline_rows;
        let decode_slots = self.decode_slots.clone();
        let tx = self.completed_tx.clone();
        let generation = self.generation;
        let store = self.store.clone();
        let cache_root = self.cache_root.clone();
        let disk_limit = self.config.disk_cache_bytes;
        let memory_limit = self.config.memory_cache_bytes;
        let ephemeral = disk_limit == 0 || attachment.size > disk_limit;
        tokio::spawn(async move {
            let result: Result<(SlicedProtocol, u64)> = async {
                let path = if destination.exists() {
                    super::client::verify_file(&destination, &attachment.sha256, attachment.size)
                        .await?;
                    destination.clone()
                } else {
                    client
                        .ok_or_else(|| Error::Locked("media signer is unavailable".into()))?
                        .fetch(&attachment, &destination)
                        .await?
                };
                touch_cache_file(&path)?;
                let removed = prune_cache(&cache_root, disk_limit, &path).await?;
                if !removed.is_empty() {
                    store
                        .call(move |store| store.delete_media_cache_entries(&removed))
                        .await?;
                }
                if !ephemeral {
                    let cache_attachment = attachment.clone();
                    store
                        .call(move |store| {
                            store.record_media_cache(
                                community,
                                &cache_attachment.sha256,
                                &cache_attachment.mime,
                                cache_attachment.size,
                                cache_attachment.width,
                                cache_attachment.height,
                            )
                        })
                        .await?;
                }
                let _permit = decode_slots
                    .acquire_owned()
                    .await
                    .map_err(|_| Error::Protocol("media decode queue stopped".into()))?;
                let decode_path = path.clone();
                let (protocol, weight) = tokio::task::spawn_blocking(move || {
                    let image = decode_image(&decode_path)?;
                    let allocation = Size::new(width, max_rows);
                    let weight = prepared_weight(&picker, allocation);
                    if memory_limit == 0 || weight > memory_limit {
                        return Err(Error::Protocol(
                            "prepared image exceeds the memory cache limit".into(),
                        ));
                    }
                    let protocol = SlicedProtocol::new_with_resize(
                        &picker,
                        image,
                        allocation,
                        Resize::Fit(None),
                    )
                    .map_err(|error| Error::Protocol(error.to_string()))?;
                    Ok((protocol, weight))
                })
                .await
                .map_err(|_| Error::Protocol("media decode worker stopped".into()))??;
                if ephemeral {
                    let _ = tokio::fs::remove_file(path).await;
                }
                Ok((protocol, weight))
            }
            .await;
            let completed = match result {
                Ok((protocol, weight)) => Completed::Ready {
                    generation,
                    key,
                    protocol,
                    weight,
                },
                Err(error) => Completed::Failed {
                    generation,
                    key,
                    message: safe_error(&error),
                },
            };
            let _ = tx.send(completed).await;
        });
    }

    pub fn retry(&mut self, attachment: &Attachment, _width: u16) {
        let hash = if attachment.kind == MediaKind::Video {
            poster_details(attachment)
                .map(|(_, hash)| hash)
                .unwrap_or_else(|| attachment.sha256.clone())
        } else {
            attachment.sha256.clone()
        };
        let prefix = format!("{hash}:");
        self.states.retain(|key, _| !key.starts_with(&prefix));
        self.in_flight.retain(|key| !key.starts_with(&prefix));
        self.weights.retain(|key, _| !key.starts_with(&prefix));
        self.last_used.retain(|key, _| !key.starts_with(&prefix));
    }

    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(completed) = self.completed_rx.try_recv() {
            match completed {
                Completed::Ready {
                    generation,
                    key,
                    protocol,
                    weight,
                } if generation == self.generation => {
                    self.in_flight.remove(&key);
                    let current = key.clone();
                    self.weights.insert(key.clone(), weight);
                    self.states
                        .insert(key, MediaState::Ready(Arc::new(protocol)));
                    self.touch(&current);
                    self.trim_memory_cache(&current);
                }
                Completed::Failed {
                    generation,
                    key,
                    message,
                } if generation == self.generation => {
                    self.in_flight.remove(&key);
                    self.states.insert(key, MediaState::Failed(message));
                }
                Completed::Ready { .. } | Completed::Failed { .. } => {}
            }
            changed = true;
        }
        changed
    }

    fn touch(&mut self, key: &str) {
        self.use_clock = self.use_clock.wrapping_add(1);
        self.last_used.insert(key.to_owned(), self.use_clock);
    }

    fn trim_memory_cache(&mut self, keep: &str) {
        let limit = self.config.memory_cache_bytes;
        while self
            .weights
            .values()
            .copied()
            .fold(0_u64, u64::saturating_add)
            > limit
        {
            let candidate = oldest_cached_key(&self.weights, &self.last_used, keep);
            let Some(candidate) = candidate else { break };
            self.states.remove(&candidate);
            self.weights.remove(&candidate);
            self.last_used.remove(&candidate);
        }
    }

    fn clear_memory_state(&mut self) {
        self.states.clear();
        self.in_flight.clear();
        self.weights.clear();
        self.last_used.clear();
    }

    pub fn cache_path(&self, community: Uuid, attachment: &Attachment) -> PathBuf {
        let extension = attachment
            .url
            .rsplit_once('.')
            .map(|(_, extension)| extension)
            .filter(|extension| {
                extension.len() <= 16 && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
            .unwrap_or("bin");
        self.cache_root
            .join(community.to_string())
            .join(format!("{}.{}", attachment.sha256, extension))
    }

    pub fn staging_dir(&self, community: Uuid) -> PathBuf {
        self.cache_root.join(community.to_string()).join("staging")
    }

    pub async fn repair_cache_metadata(&self) -> Result<usize> {
        let mut directories = vec![self.cache_root.clone()];
        let mut present = std::collections::HashSet::new();
        let mut visited = 0_usize;
        while let Some(directory) = directories.pop() {
            let mut entries = match tokio::fs::read_dir(&directory).await {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(Error::io(&directory, error)),
            };
            while let Some(entry) = entries
                .next_entry()
                .await
                .map_err(|error| Error::io(&directory, error))?
            {
                visited += 1;
                if visited > 10_000 {
                    return Ok(0);
                }
                let path = entry.path();
                let file_type = entry
                    .file_type()
                    .await
                    .map_err(|error| Error::io(&path, error))?;
                if file_type.is_symlink() {
                    let _ = tokio::fs::remove_file(path).await;
                } else if file_type.is_dir() {
                    if entry.file_name() != "staging" {
                        directories.push(path);
                    }
                } else if file_type.is_file()
                    && let Some((community, hash)) = cache_identity(&self.cache_root, &path)
                {
                    present.insert((community.to_string(), hash));
                }
            }
        }
        self.store
            .call(move |store| store.retain_media_cache_entries(&present))
            .await
    }

    pub async fn clear_cache(&mut self, community: Option<Uuid>) -> Result<()> {
        self.generation = self.generation.wrapping_add(1);
        let paths = [
            community.map_or_else(
                || self.cache_root.clone(),
                |id| self.cache_root.join(id.to_string()),
            ),
            community.map_or_else(
                || self.avatar_cache_root.clone(),
                |id| self.avatar_cache_root.join(id.to_string()),
            ),
        ];
        for path in paths {
            if path.exists() {
                tokio::fs::remove_dir_all(&path)
                    .await
                    .map_err(|error| Error::io(&path, error))?;
            }
            tokio::fs::create_dir_all(&path)
                .await
                .map_err(|error| Error::io(&path, error))?;
            crate::paths::set_private_permissions(&path)?;
        }
        self.clear_memory_state();
        Ok(())
    }
}

fn safe_picker() -> Picker {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Picker::halfblocks();
    }
    let Ok(window) = crossterm::terminal::window_size() else {
        return Picker::halfblocks();
    };
    if window.columns == 0 || window.rows == 0 || window.width == 0 || window.height == 0 {
        return Picker::halfblocks();
    }
    let width = (window.width / window.columns).max(1);
    let height = (window.height / window.rows).max(1);
    #[allow(deprecated)]
    Picker::from_fontsize(FontSize::new(width, height))
}

fn auto_protocol_from_environment() -> Option<ProtocolType> {
    let value = |name: &str| std::env::var(name).unwrap_or_default();
    let term = value("TERM").to_ascii_lowercase();
    let program = value("TERM_PROGRAM").to_ascii_lowercase();
    if !value("KITTY_WINDOW_ID").is_empty() || program == "ghostty" {
        Some(ProtocolType::Kitty)
    } else if !value("WEZTERM_EXECUTABLE").is_empty()
        || matches!(program.as_str(), "iterm.app" | "wezterm")
    {
        Some(ProtocolType::Iterm2)
    } else if term.starts_with("foot") || term.contains("sixel") {
        Some(ProtocolType::Sixel)
    } else {
        Some(ProtocolType::Halfblocks)
    }
}

fn cache_key(attachment: &Attachment, width: u16) -> String {
    format!("{}:{width}", attachment.sha256)
}

fn should_request_avatar(
    mode: ProfileAvatars,
    network_enabled: bool,
    graphics_protocol: bool,
    width: u16,
) -> bool {
    mode == ProfileAvatars::Trusted && network_enabled && graphics_protocol && width >= 2
}

fn avatar_key(pubkey: &str, picture: &str, width: u16) -> String {
    format!(
        "avatar:{}:{}:{width}",
        profile_digest(pubkey),
        url_digest(picture)
    )
}

fn profile_digest(pubkey: &str) -> String {
    hex::encode(Sha256::digest(pubkey.as_bytes()))
}

fn url_digest(url: &str) -> String {
    hex::encode(Sha256::digest(url.as_bytes()))
}

fn poster_details(attachment: &Attachment) -> Option<(String, String)> {
    let url = attachment.poster.clone()?;
    let hash = super::imeta::hash_from_path(&url)?;
    Some((url, hash))
}

/// Upper bound for the resized pixels and protocol data retained by a prepared
/// terminal image. `SlicedProtocol::new_with_resize(..., Resize::Fit)` drops
/// the decoded source before it enters the memory cache, so source dimensions
/// must not be used here.
fn prepared_weight(picker: &Picker, allocation: Size) -> u64 {
    let font = picker.font_size();
    u64::from(allocation.width)
        .saturating_mul(u64::from(font.width))
        .saturating_mul(u64::from(allocation.height))
        .saturating_mul(u64::from(font.height))
        // Cover RGBA pixels, encoded protocol data, and cell representation.
        .saturating_mul(8)
        .saturating_add(64 * 1024)
}

fn oldest_cached_key(
    weights: &HashMap<String, u64>,
    last_used: &HashMap<String, u64>,
    keep: &str,
) -> Option<String> {
    weights
        .keys()
        .filter(|key| key.as_str() != keep)
        .min_by_key(|key| last_used.get(*key).copied().unwrap_or_default())
        .cloned()
}

fn cleanup_partial_files(root: &std::path::Path) {
    let mut directories = vec![root.to_path_buf()];
    let mut visited = 0_usize;
    while let Some(directory) = directories.pop() {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > 10_000 {
                return;
            }
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                let _ = std::fs::remove_file(path);
            } else if file_type.is_dir() {
                directories.push(path);
            } else if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with('.') && name.ends_with(".part"))
            {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

async fn prune_cache(
    root: &std::path::Path,
    limit: u64,
    keep: &std::path::Path,
) -> Result<Vec<(Uuid, String)>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut directories = vec![root.to_path_buf()];
    let mut files = Vec::new();
    let mut total = 0_u64;
    let mut visited = 0_usize;
    while let Some(directory) = directories.pop() {
        let mut entries = match tokio::fs::read_dir(&directory).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(Error::io(&directory, error)),
        };
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|error| Error::io(&directory, error))?
        {
            visited += 1;
            if visited > 10_000 {
                return Err(Error::Config(
                    "media cache contains too many entries to prune safely".into(),
                ));
            }
            let path = entry.path();
            let file_type = entry
                .file_type()
                .await
                .map_err(|error| Error::io(&path, error))?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if entry.file_name() != "staging" {
                    directories.push(path);
                }
            } else if file_type.is_file()
                && path.extension().is_none_or(|extension| extension != "part")
            {
                let metadata = entry
                    .metadata()
                    .await
                    .map_err(|error| Error::io(&path, error))?;
                total = total.saturating_add(metadata.len());
                let modified = metadata
                    .modified()
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                files.push((modified, metadata.len(), path));
            }
        }
    }
    files.sort_by_key(|(modified, _, _)| *modified);
    let mut removed = Vec::new();
    for (_, size, path) in files {
        if total <= limit {
            break;
        }
        if path == keep {
            continue;
        }
        if tokio::fs::remove_file(&path).await.is_ok() {
            total = total.saturating_sub(size);
            if let Some(entry) = cache_identity(root, &path) {
                removed.push(entry);
            }
        }
    }
    Ok(removed)
}

fn touch_cache_file(path: &std::path::Path) -> Result<()> {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|error| Error::io(path, error))?;
    file.set_times(std::fs::FileTimes::new().set_modified(std::time::SystemTime::now()))
        .map_err(|error| Error::io(path, error))
}

fn cache_identity(root: &std::path::Path, path: &std::path::Path) -> Option<(Uuid, String)> {
    let relative = path.strip_prefix(root).ok()?;
    let mut components = relative.components();
    let community = components.next()?.as_os_str().to_str()?.parse().ok()?;
    let filename = components.next()?.as_os_str().to_str()?;
    if components.next().is_some() {
        return None;
    }
    let hash = filename.split_once('.')?.0;
    (hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')))
    .then(|| (community, hash.to_owned()))
}

fn safe_error(error: &Error) -> String {
    match error {
        Error::Protocol(message) | Error::Access(message) | Error::Config(message) => message
            .chars()
            .filter(|character| !character.is_control())
            .take(160)
            .collect(),
        Error::Locked(_)
        | Error::Auth(_)
        | Error::IdentityMissing(_)
        | Error::IdentityCorrupt(_) => "media authorization is unavailable".into(),
        Error::Network(_) | Error::Timeout(_) => "media network operation failed".into(),
        Error::Io { .. } => "media cache I/O failed".into(),
        Error::Database(_) | Error::Serialization(_) | Error::Unsupported(_) => {
            "media operation failed".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_requests_require_enabled_network_and_graphics() {
        assert!(should_request_avatar(
            ProfileAvatars::Trusted,
            true,
            true,
            4
        ));
        assert!(!should_request_avatar(ProfileAvatars::Off, true, true, 4));
        assert!(!should_request_avatar(
            ProfileAvatars::Trusted,
            false,
            true,
            4
        ));
        assert!(!should_request_avatar(
            ProfileAvatars::Trusted,
            true,
            false,
            4
        ));
        assert!(!should_request_avatar(
            ProfileAvatars::Trusted,
            true,
            true,
            1
        ));
    }

    #[test]
    fn avatar_state_keys_are_url_free_and_profile_specific() {
        let first = avatar_key("a", "https://cdn.example.test/a.png", 4);
        assert!(!first.contains("example"));
        assert_ne!(first, avatar_key("b", "https://cdn.example.test/a.png", 4));
        assert_ne!(first, avatar_key("a", "https://cdn.example.test/b.png", 4));
        assert_ne!(first, avatar_key("a", "https://cdn.example.test/a.png", 5));
    }

    #[test]
    fn weighted_cache_evicts_the_oldest_non_visible_entry() {
        let weights = HashMap::from([
            ("old".to_owned(), 8),
            ("new".to_owned(), 2),
            ("visible".to_owned(), 20),
        ]);
        let used = HashMap::from([
            ("old".to_owned(), 1),
            ("new".to_owned(), 2),
            ("visible".to_owned(), 3),
        ]);
        assert_eq!(
            oldest_cached_key(&weights, &used, "visible").as_deref(),
            Some("old")
        );
    }

    #[test]
    fn prepared_cache_weight_tracks_terminal_allocation_not_source_dimensions() {
        let picker = Picker::halfblocks();
        let font = picker.font_size();
        let avatar = prepared_weight(&picker, Size::new(4, 3));
        let expected = u64::from(4_u16)
            .saturating_mul(u64::from(font.width))
            .saturating_mul(u64::from(3_u16))
            .saturating_mul(u64::from(font.height))
            .saturating_mul(8)
            .saturating_add(64 * 1024);
        assert_eq!(avatar, expected);
        assert!(avatar < 100_000, "a 4×3 avatar stays inexpensive");
        assert!(prepared_weight(&picker, Size::new(72, 12)) > avatar);
    }

    #[test]
    fn every_terminal_protocol_prepares_a_sliced_image() {
        for protocol_type in [
            ProtocolType::Halfblocks,
            ProtocolType::Kitty,
            ProtocolType::Sixel,
            ProtocolType::Iterm2,
        ] {
            let mut picker = Picker::halfblocks();
            picker.set_protocol_type(protocol_type);
            let protocol = SlicedProtocol::new_with_resize(
                &picker,
                image::DynamicImage::new_rgb8(2, 2),
                Size::new(4, 2),
                Resize::Fit(None),
            )
            .unwrap_or_else(|error| panic!("{protocol_type:?} preparation failed: {error}"));
            assert!(protocol.size().width > 0);
            assert!(protocol.size().height > 0);
        }
    }
}
