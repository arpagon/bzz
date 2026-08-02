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
use tokio::sync::{Semaphore, mpsc};
use uuid::Uuid;

use crate::{
    config::{MediaAutoload, MediaConfig, MediaProtocol},
    error::{Error, Result},
    paths::Paths,
    store::writer::StoreHandle,
};

use super::{
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
    },
    Failed {
        generation: u64,
        key: String,
        message: String,
    },
}

pub struct MediaRuntime {
    config: MediaConfig,
    community_id: Option<Uuid>,
    cache_root: PathBuf,
    picker: Picker,
    protocol_name: String,
    generation: u64,
    client: Option<MediaClient>,
    store: StoreHandle,
    states: HashMap<String, MediaState>,
    in_flight: HashSet<String>,
    completed_tx: mpsc::Sender<Completed>,
    completed_rx: mpsc::Receiver<Completed>,
    decode_slots: Arc<Semaphore>,
}

impl MediaRuntime {
    pub fn new(config: MediaConfig, paths: &Paths, store: StoreHandle) -> Self {
        let (completed_tx, completed_rx) = mpsc::channel(64);
        let cache_root = paths.media_cache_dir();
        cleanup_partial_files(&cache_root);
        Self {
            decode_slots: Arc::new(Semaphore::new(config.decode_concurrency)),
            config,
            community_id: None,
            cache_root,
            picker: Picker::halfblocks(),
            protocol_name: "halfblocks".into(),
            generation: 0,
            client: None,
            store,
            states: HashMap::new(),
            in_flight: HashSet::new(),
            completed_tx,
            completed_rx,
        }
    }

    pub fn bind(&mut self, community_id: Uuid, client: MediaClient) {
        if self.community_id != Some(community_id) {
            self.generation = self.generation.wrapping_add(1);
            self.states.clear();
            self.in_flight.clear();
        }
        self.community_id = Some(community_id);
        self.client = Some(client);
    }

    pub fn select_cached(&mut self, community_id: Uuid) {
        if self.community_id != Some(community_id) {
            self.generation = self.generation.wrapping_add(1);
            self.states.clear();
            self.in_flight.clear();
        }
        self.community_id = Some(community_id);
        self.client = None;
    }

    pub fn unbind(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.community_id = None;
        self.client = None;
        self.states.clear();
        self.in_flight.clear();
    }

    pub fn initialize_terminal(&mut self) {
        if !self.config.enabled || self.config.protocol == MediaProtocol::Off {
            self.protocol_name = "off".into();
            self.generation = self.generation.wrapping_add(1);
            self.states.clear();
            self.in_flight.clear();
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
        self.states.clear();
        self.in_flight.clear();
    }

    pub fn protocol_name(&self) -> &str {
        &self.protocol_name
    }

    pub fn state(&self, attachment: &Attachment, width: u16) -> Option<&MediaState> {
        self.states.get(&cache_key(attachment, width))
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
        if self.states.contains_key(&key)
            || self.in_flight.len() >= 64
            || !self.in_flight.insert(key.clone())
        {
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
        let ephemeral = disk_limit == 0 || attachment.size > disk_limit;
        tokio::spawn(async move {
            let result: Result<SlicedProtocol> = async {
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
                prune_cache(&cache_root, disk_limit, &path).await?;
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
                let _permit = decode_slots
                    .acquire_owned()
                    .await
                    .map_err(|_| Error::Protocol("media decode queue stopped".into()))?;
                let decode_path = path.clone();
                let protocol = tokio::task::spawn_blocking(move || {
                    let image = decode_image(&decode_path)?;
                    SlicedProtocol::new_with_resize(
                        &picker,
                        image,
                        Size::new(width, max_rows),
                        Resize::Fit(None),
                    )
                    .map_err(|error| Error::Protocol(error.to_string()))
                })
                .await
                .map_err(|_| Error::Protocol("media decode worker stopped".into()))??;
                if ephemeral {
                    let _ = tokio::fs::remove_file(path).await;
                }
                Ok(protocol)
            }
            .await;
            let completed = match result {
                Ok(protocol) => Completed::Ready {
                    generation,
                    key,
                    protocol,
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

    pub fn retry(&mut self, attachment: &Attachment, width: u16) {
        let prefix = format!("{}:", attachment.sha256);
        self.states.retain(|key, _| !key.starts_with(&prefix));
        self.in_flight.retain(|key| !key.starts_with(&prefix));
        self.request_inline(attachment, width, true);
    }

    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(completed) = self.completed_rx.try_recv() {
            match completed {
                Completed::Ready {
                    generation,
                    key,
                    protocol,
                } if generation == self.generation => {
                    self.in_flight.remove(&key);
                    let current = key.clone();
                    self.states
                        .insert(key, MediaState::Ready(Arc::new(protocol)));
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

    fn trim_memory_cache(&mut self, keep: &str) {
        let max_entries = usize::try_from(self.config.memory_cache_bytes / (1024 * 1024))
            .unwrap_or(512)
            .clamp(1, 512);
        while self
            .states
            .values()
            .filter(|state| matches!(state, MediaState::Ready(_)))
            .count()
            > max_entries
        {
            let candidate = self
                .states
                .iter()
                .find(|(key, state)| key.as_str() != keep && matches!(state, MediaState::Ready(_)))
                .map(|(key, _)| key.clone());
            let Some(candidate) = candidate else { break };
            self.states.remove(&candidate);
        }
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

    pub async fn clear_cache(&mut self, community: Option<Uuid>) -> Result<()> {
        let path = community.map_or_else(
            || self.cache_root.clone(),
            |id| self.cache_root.join(id.to_string()),
        );
        if path.exists() {
            tokio::fs::remove_dir_all(&path)
                .await
                .map_err(|error| Error::io(&path, error))?;
        }
        tokio::fs::create_dir_all(&path)
            .await
            .map_err(|error| Error::io(&path, error))?;
        crate::paths::set_private_permissions(&path)?;
        self.states.clear();
        self.in_flight.clear();
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
            if path.is_dir() {
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

async fn prune_cache(root: &std::path::Path, limit: u64, keep: &std::path::Path) -> Result<()> {
    if limit == 0 {
        return Ok(());
    }
    let mut directories = vec![root.to_path_buf()];
    let mut files = Vec::new();
    let mut total = 0_u64;
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
            let path = entry.path();
            let metadata = entry
                .metadata()
                .await
                .map_err(|error| Error::io(&path, error))?;
            if metadata.is_dir() {
                if entry.file_name() != "staging" {
                    directories.push(path);
                }
            } else if metadata.is_file()
                && path.extension().is_none_or(|extension| extension != "part")
            {
                total = total.saturating_add(metadata.len());
                let modified = metadata
                    .modified()
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                files.push((modified, metadata.len(), path));
            }
        }
    }
    files.sort_by_key(|(modified, _, _)| *modified);
    for (_, size, path) in files {
        if total <= limit {
            break;
        }
        if path == keep {
            continue;
        }
        if tokio::fs::remove_file(&path).await.is_ok() {
            total = total.saturating_sub(size);
        }
    }
    Ok(())
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
