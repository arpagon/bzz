//! `:app dev <folder>`: the owner's own folder in the pane of the selected
//! channel, on that channel's real records, reloaded on every save (plan
//! D5). It needs no approval because it is the owner's own folder that he
//! is looking at, and the pane says so in its border.
//!
//! - **Whose records.** The app's name is the one its `app.toml` gives (or
//!   the folder's, for a folder without one): its records are
//!   `bzz.app.<name>.*` of the channel. A folder named as an installed app
//!   works on that app's records, which is how a change is tried on real
//!   data; to keep apart, give the folder another name.
//! - **A save** is given to the running app. One that does not load leaves
//!   the version before it running and shows the error; the state is kept
//!   across a save that loads.
//! - **What ends it**: `:app close`, `:app open` of another app in the
//!   channel, the script's `host.quit()`, a switch of community, `:lock`,
//!   and quitting. It is never remembered in the store: a restart of bzz
//!   does not run the folder again.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::apps::store::folder::read_bounded;
use crate::apps::store::manifest::{
    MANIFEST_BYTES, MANIFEST_FILE, Manifest, SCRIPT_BYTES, SCRIPT_FILE, check_name, title_from_name,
};

/// How often the script's file is looked at.
const LOOK_EVERY: Duration = Duration::from_millis(300);

/// A folder whose script runs in the pane.
pub struct DevFolder {
    /// The folder, as the owner named it.
    pub dir: PathBuf,
    pub script: PathBuf,
    last: Vec<u8>,
    looked: Instant,
    /// Saves given to the app.
    pub reloads: u32,
    /// The script was gone at the last look; said once.
    pub missing: bool,
}

/// What a folder in development is opened with.
pub struct Opened {
    pub folder: DevFolder,
    pub name: String,
    pub title: String,
    pub script: Vec<u8>,
}

impl DevFolder {
    /// Reads a folder (or a script) of the owner's: its name, its title
    /// and its script.
    pub fn open(path: &Path) -> Result<Opened, String> {
        let (dir, script) = if path.is_dir() {
            (path.to_path_buf(), path.join(SCRIPT_FILE))
        } else {
            (
                path.parent().map_or_else(PathBuf::new, Path::to_path_buf),
                path.to_path_buf(),
            )
        };
        let bytes = read_bounded(&script, SCRIPT_BYTES)?;
        let manifest = dir.join(MANIFEST_FILE);
        let (name, title) = if manifest.is_file() {
            let text = read_bounded(&manifest, MANIFEST_BYTES)?;
            let text = String::from_utf8(text)
                .map_err(|_| format!("{MANIFEST_FILE} is not text: it is not valid UTF-8"))?;
            let manifest =
                Manifest::parse(&text).map_err(|error| format!("{MANIFEST_FILE}: {error}"))?;
            (manifest.name, manifest.title)
        } else {
            // A folder without a manifest is named as the folder (or the
            // script, for a script given alone).
            let named = if path.is_dir() {
                dir.file_name()
            } else {
                script.file_stem()
            };
            let name = named
                .map(|name| name.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            check_name(&name).map_err(|error| {
                format!("{error}. Give the folder an {MANIFEST_FILE} with the app's name")
            })?;
            let title = title_from_name(&name);
            (name, title)
        };
        Ok(Opened {
            folder: Self {
                dir,
                script,
                last: bytes.clone(),
                looked: Instant::now(),
                reloads: 0,
                missing: false,
            },
            name,
            title,
            script: bytes,
        })
    }

    /// The script's new bytes, when it was saved since the last look.
    pub fn changed(&mut self) -> Option<Vec<u8>> {
        if self.looked.elapsed() < LOOK_EVERY {
            return None;
        }
        self.looked = Instant::now();
        match read_bounded(&self.script, SCRIPT_BYTES) {
            Ok(bytes) => {
                self.missing = false;
                if bytes == self.last || bytes.is_empty() {
                    return None;
                }
                self.last.clone_from(&bytes);
                self.reloads += 1;
                Some(bytes)
            }
            // Saved by an editor that writes a new file: it is back soon.
            Err(_) => {
                self.missing = true;
                None
            }
        }
    }

    /// What the pane's border says of it.
    pub fn label(&self, title: &str) -> String {
        let place = crate::protocol::artifact::display(&self.dir.display().to_string(), 24);
        format!("{title} · not approved · dev {place}")
    }

    #[cfg(test)]
    pub fn look_now(&mut self) {
        self.looked = Instant::now() - LOOK_EVERY;
    }
}
