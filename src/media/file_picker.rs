//! Explicit OS file chooser boundary for composer attachments.
//!
//! Results are transient and contain no retained native handles. Callers must
//! still exact-target scope and securely stage every selected path.

use std::path::PathBuf;

use async_trait::async_trait;

const MAX_PICKED_FILES: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilePickerRejection {
    TooManyFiles,
    InvalidSelection,
}

impl FilePickerRejection {
    pub const fn status(self) -> &'static str {
        match self {
            Self::TooManyFiles => "file picker selected more than 8 files",
            Self::InvalidSelection => "file picker returned an unsupported selection",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FilePickerOutcome {
    Files(Vec<PathBuf>),
    Cancelled,
    Unavailable,
    Rejected(FilePickerRejection),
}

#[async_trait]
pub trait FilePicker: Send + Sync {
    async fn pick_files(&self) -> FilePickerOutcome;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeFilePicker;

#[async_trait]
impl FilePicker for NativeFilePicker {
    async fn pick_files(&self) -> FilePickerOutcome {
        pick_native_files().await
    }
}

fn bounded_selection(paths: Vec<PathBuf>) -> FilePickerOutcome {
    if paths.is_empty() {
        FilePickerOutcome::Cancelled
    } else if paths.len() > MAX_PICKED_FILES {
        FilePickerOutcome::Rejected(FilePickerRejection::TooManyFiles)
    } else if paths.iter().any(|path| !path.is_absolute()) {
        FilePickerOutcome::Rejected(FilePickerRejection::InvalidSelection)
    } else {
        FilePickerOutcome::Files(paths)
    }
}

#[cfg(target_os = "linux")]
async fn pick_native_files() -> FilePickerOutcome {
    use ashpd::desktop::file_chooser::SelectedFiles;

    let request = match SelectedFiles::open_file()
        .title("Attach files to bzz")
        .accept_label("Attach")
        .modal(true)
        .multiple(true)
        .send()
        .await
    {
        Ok(request) => request,
        Err(_) => return FilePickerOutcome::Unavailable,
    };
    let selected = match request.response() {
        Ok(selected) => selected,
        Err(_) => return FilePickerOutcome::Cancelled,
    };
    let mut paths = Vec::with_capacity(selected.uris().len().min(MAX_PICKED_FILES));
    for uri in selected.uris() {
        let Ok(uri) = url::Url::parse(uri.as_str()) else {
            return FilePickerOutcome::Rejected(FilePickerRejection::InvalidSelection);
        };
        if uri.scheme() != "file" || uri.host_str().is_some_and(|host| host != "localhost") {
            return FilePickerOutcome::Rejected(FilePickerRejection::InvalidSelection);
        }
        let Ok(path) = uri.to_file_path() else {
            return FilePickerOutcome::Rejected(FilePickerRejection::InvalidSelection);
        };
        paths.push(path);
    }
    bounded_selection(paths)
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
async fn pick_native_files() -> FilePickerOutcome {
    let selected = rfd::AsyncFileDialog::new()
        .set_title("Attach files to bzz")
        .pick_files()
        .await;
    let Some(selected) = selected else {
        return FilePickerOutcome::Cancelled;
    };
    bounded_selection(
        selected
            .into_iter()
            .map(|handle| handle.path().to_path_buf())
            .collect(),
    )
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
async fn pick_native_files() -> FilePickerOutcome {
    FilePickerOutcome::Unavailable
}

#[cfg(test)]
mod tests {
    use super::{FilePickerOutcome, FilePickerRejection, bounded_selection};

    #[test]
    fn selections_are_absolute_nonempty_and_bounded() {
        let root = std::env::temp_dir();
        assert_eq!(bounded_selection(Vec::new()), FilePickerOutcome::Cancelled);
        assert!(matches!(
            bounded_selection(vec![root.join("a.txt")]),
            FilePickerOutcome::Files(_)
        ));
        assert_eq!(
            bounded_selection(vec![std::path::PathBuf::from("relative.txt")]),
            FilePickerOutcome::Rejected(FilePickerRejection::InvalidSelection)
        );
        assert_eq!(
            bounded_selection((0..9).map(|index| root.join(index.to_string())).collect()),
            FilePickerOutcome::Rejected(FilePickerRejection::TooManyFiles)
        );
    }
}
