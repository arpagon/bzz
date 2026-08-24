use std::{fs, path::PathBuf};

use directories::ProjectDirs;

use crate::error::{Error, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl Paths {
    pub fn discover() -> Result<Self> {
        let dirs = ProjectDirs::from("dev", "arpagon", application_name())
            .ok_or_else(|| Error::Config("the operating system has no home directory".into()))?;
        let config_dir = std::env::var_os("BZZ_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs.config_dir().to_path_buf());
        let data_dir = std::env::var_os("BZZ_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs.data_dir().to_path_buf());
        let cache_dir = std::env::var_os("BZZ_CACHE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs.cache_dir().to_path_buf());
        Ok(Self {
            config_dir,
            data_dir,
            cache_dir,
        })
    }

    pub fn ensure(&self) -> Result<()> {
        for path in [
            &self.config_dir,
            &self.data_dir,
            &self.cache_dir,
            &self.keys_dir(),
            &self.media_cache_dir(),
            &self.avatar_cache_dir(),
            &self.diagnostics_dir(),
        ] {
            fs::create_dir_all(path).map_err(|error| Error::io(path, error))?;
            set_private_permissions(path)?;
        }
        Ok(())
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn theme_file(&self) -> PathBuf {
        self.config_dir.join("theme.toml")
    }

    pub fn keymap_file(&self) -> PathBuf {
        self.config_dir.join("keymap.toml")
    }

    pub fn database_file(&self) -> PathBuf {
        self.data_dir.join("bzz.db")
    }

    pub fn keys_dir(&self) -> PathBuf {
        self.data_dir.join("keys")
    }

    pub fn diagnostics_dir(&self) -> PathBuf {
        self.data_dir.join("diagnostics")
    }

    pub fn telemetry_health_file(&self) -> PathBuf {
        self.diagnostics_dir().join("telemetry-health.json")
    }

    pub fn media_cache_dir(&self) -> PathBuf {
        self.cache_dir.join("media")
    }

    /// Private local cache for profile-image bytes, intentionally separate from
    /// authenticated community attachment media.
    pub fn avatar_cache_dir(&self) -> PathBuf {
        self.cache_dir.join("avatars")
    }
}

const fn application_name() -> &'static str {
    application_name_for(cfg!(debug_assertions))
}

const fn application_name_for(debug: bool) -> &'static str {
    if debug { "bzz-dev" } else { "bzz" }
}

#[cfg(unix)]
pub fn set_private_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if path.is_dir() { 0o700 } else { 0o600 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| Error::io(path, error))
}

#[cfg(not(unix))]
pub fn set_private_permissions(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn debug_and_release_names_are_distinct() {
        assert_eq!(super::application_name_for(true), "bzz-dev");
        assert_eq!(super::application_name_for(false), "bzz");
    }
}
