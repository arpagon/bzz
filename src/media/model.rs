use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MediaKind {
    Image,
    Video,
    File,
    Unsupported,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Attachment {
    pub index: usize,
    pub url: String,
    pub mime: String,
    pub sha256: String,
    pub size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub alt: Option<String>,
    pub blurhash: Option<String>,
    pub thumb: Option<String>,
    pub poster: Option<String>,
    pub filename: Option<String>,
    pub duration_millis: Option<u64>,
    pub kind: MediaKind,
    pub spoiler: bool,
    pub error: Option<String>,
}

impl Attachment {
    pub fn valid(&self) -> bool {
        self.error.is_none()
    }

    pub fn label(&self) -> &str {
        self.alt
            .as_deref()
            .or(self.filename.as_deref())
            .unwrap_or(match self.kind {
                MediaKind::Image => "image",
                MediaKind::Video => "video",
                MediaKind::File => "attachment",
                MediaKind::Unsupported => "unsupported attachment",
            })
    }

    pub fn imeta_tag(&self) -> Vec<String> {
        let mut tag = vec![
            "imeta".to_owned(),
            format!("url {}", self.url),
            format!("m {}", self.mime),
            format!("x {}", self.sha256),
            format!("size {}", self.size),
        ];
        if let (Some(width), Some(height)) = (self.width, self.height) {
            tag.push(format!("dim {width}x{height}"));
        }
        if let Some(value) = &self.blurhash {
            tag.push(format!("blurhash {value}"));
        }
        if let Some(value) = &self.alt {
            tag.push(format!("alt {value}"));
        }
        if let Some(value) = &self.thumb {
            tag.push(format!("thumb {value}"));
        }
        if let Some(value) = &self.poster {
            tag.push(format!("image {value}"));
        }
        if let Some(value) = &self.filename {
            tag.push(format!("filename {value}"));
        }
        if let Some(value) = self.duration_millis {
            tag.push(format!("duration {:.3}", value as f64 / 1_000.0));
        }
        tag
    }

    pub fn markdown_line(&self) -> String {
        match self.kind {
            MediaKind::Image => format!("![image]({})", self.url),
            MediaKind::Video => format!("![video]({})", self.url),
            MediaKind::File | MediaKind::Unsupported => {
                let label = self
                    .filename
                    .as_deref()
                    .unwrap_or("file")
                    .replace('\\', "\\\\")
                    .replace('[', "\\[")
                    .replace(']', "\\]");
                format!("[{label}]({})", self.url)
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PendingAttachment {
    pub cache_name: String,
    pub mime: String,
    pub filename: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum DraftAttachment {
    Pending(PendingAttachment),
    Uploaded(Attachment),
}

impl DraftAttachment {
    pub fn uploaded(&self) -> Option<&Attachment> {
        match self {
            Self::Uploaded(attachment) => Some(attachment),
            Self::Pending(_) => None,
        }
    }
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
