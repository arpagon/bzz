use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("identity is locked: {0}")]
    Locked(String),
    #[error("identity secret is missing: {0}")]
    IdentityMissing(String),
    #[error("identity storage is corrupt: {0}")]
    IdentityCorrupt(String),
    #[error("authentication failed: {0}")]
    Auth(String),
    #[error("relay protocol error: {0}")]
    Protocol(String),
    #[error("relay access denied: {0}")]
    Access(String),
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("network error: {0}")]
    Network(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("operation timed out: {0}")]
    Timeout(String),
    #[error("unsupported operation: {0}")]
    Unsupported(String),
}

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
