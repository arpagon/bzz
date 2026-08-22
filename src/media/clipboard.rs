//! Explicit, bounded native clipboard import for the composer.
//!
//! This module never polls or retains the system clipboard. Callers invoke a
//! [`ClipboardReader`] only after a composer-owned paste action, then move the
//! returned data directly into bounded staging or composer insertion.

use std::{
    io,
    path::{Path, PathBuf},
};

use arboard::Clipboard;
use image::{ExtendedColorType, ImageEncoder as _, codecs::png::PngEncoder};

pub const MAX_CLIPBOARD_TEXT_BYTES: usize = 64 * 1024;
const MAX_CLIPBOARD_FILES: usize = 8;
const MAX_IMAGE_AXIS: usize = 16_384;
const MAX_IMAGE_PIXELS: usize = 25_000_000;
const MAX_IMAGE_BYTES: usize = 50 * 1024 * 1024;
const CLIPBOARD_IMAGE_NAME: &str = "pasted-image.png";

/// Data made available by one explicit clipboard read. It holds no platform
/// clipboard handles and therefore can move to a worker without UI-thread I/O.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClipboardContents {
    Files(Vec<PathBuf>),
    Image(ClipboardImage),
    Text(String),
    Empty,
    Unavailable,
    Rejected(ClipboardRejection),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipboardRejection {
    TooManyFiles,
    TextTooLarge,
    ImageTooLarge,
    InvalidImage,
}

impl ClipboardRejection {
    pub const fn status(self) -> &'static str {
        match self {
            Self::TooManyFiles => "clipboard has more than 8 files",
            Self::TextTooLarge => "clipboard text exceeds the 64 KiB safety limit",
            Self::ImageTooLarge => "clipboard image exceeds the safety limit",
            Self::InvalidImage => "clipboard image is invalid",
        }
    }
}

/// A copied bitmap in straight RGBA8 form. The bytes are not persisted; they
/// are immediately converted to a sanitized staged PNG by the caller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipboardImage {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// A narrow boundary around platform clipboard access. The production reader
/// has no retained state, while deterministic tests can provide a fake reader
/// without touching an owner's real clipboard.
pub trait ClipboardReader: Send + Sync {
    fn read_once(&self) -> ClipboardContents;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeClipboard;

impl ClipboardReader for NativeClipboard {
    fn read_once(&self) -> ClipboardContents {
        let Ok(mut clipboard) = Clipboard::new() else {
            return ClipboardContents::Unavailable;
        };

        // A native file list wins over every other representation. It prevents
        // a copied filesystem path from becoming message text accidentally.
        if let Ok(paths) = clipboard.get().file_list()
            && !paths.is_empty()
        {
            return if paths.len() > MAX_CLIPBOARD_FILES {
                ClipboardContents::Rejected(ClipboardRejection::TooManyFiles)
            } else {
                ClipboardContents::Files(paths)
            };
        }

        if let Ok(image) = clipboard.get().image() {
            let image = ClipboardImage {
                width: image.width,
                height: image.height,
                rgba: image.bytes.into_owned(),
            };
            return match validate_image(&image) {
                Ok(()) => ClipboardContents::Image(image),
                Err(rejection) => ClipboardContents::Rejected(rejection),
            };
        }

        match clipboard.get().text() {
            Ok(text) => contents_from_text(&text),
            Err(_) => ClipboardContents::Empty,
        }
    }
}

/// Handles the plain-text representation only after native file-list and image
/// reads were unavailable. Some Linux file managers offer a standards-compliant
/// `file:` URI list as their text fallback, so recognize a complete URI list
/// without exposing source paths in the UI. Other text remains composer text.
fn contents_from_text(text: &str) -> ClipboardContents {
    if let Some(paths) = local_file_uris(text) {
        return if paths.len() > MAX_CLIPBOARD_FILES {
            ClipboardContents::Rejected(ClipboardRejection::TooManyFiles)
        } else {
            ClipboardContents::Files(paths)
        };
    }
    match sanitize_pasted_text(text) {
        Ok(text) if text.is_empty() => ClipboardContents::Empty,
        Ok(text) => ClipboardContents::Text(text),
        Err(rejection) => ClipboardContents::Rejected(rejection),
    }
}

/// Converts only an entire local `file:` URI list. The optional `copy`/`cut`
/// verb is used by common Linux file-manager fallback text. The source text is
/// consumed during this explicit import and is never written to a draft, log,
/// or status message.
fn local_file_uris(value: &str) -> Option<Vec<PathBuf>> {
    let mut entries = value
        .lines()
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>();
    if matches!(entries.first(), Some(&"copy" | &"cut")) {
        entries.remove(0);
    }
    if entries.is_empty() {
        return None;
    }
    entries
        .into_iter()
        .map(|entry| {
            let uri = url::Url::parse(entry).ok()?;
            if uri.scheme() != "file" || uri.host_str().is_some_and(|host| host != "localhost") {
                return None;
            }
            let path = uri.to_file_path().ok()?;
            Path::is_absolute(&path).then_some(path)
        })
        .collect()
}

/// Produces a source-free, metadata-free PNG for the ordinary attachment
/// staging pipeline. PNG output is capped while encoding so a hostile bitmap
/// cannot grow beyond the attachment image limit.
pub fn encode_clipboard_png(
    image: ClipboardImage,
) -> Result<(String, Vec<u8>), ClipboardRejection> {
    validate_image(&image)?;
    let mut output = CappedBuffer::new(MAX_IMAGE_BYTES);
    PngEncoder::new(&mut output)
        .write_image(
            &image.rgba,
            u32::try_from(image.width).map_err(|_| ClipboardRejection::InvalidImage)?,
            u32::try_from(image.height).map_err(|_| ClipboardRejection::InvalidImage)?,
            ExtendedColorType::Rgba8,
        )
        .map_err(|_| ClipboardRejection::ImageTooLarge)?;
    Ok((CLIPBOARD_IMAGE_NAME.to_owned(), output.into_inner()))
}

/// Normalizes an explicit terminal/native text paste before composer insertion.
/// Newlines are retained, carriage returns become newlines, tabs become spaces,
/// and every other control character is omitted. This mirrors typed input's
/// control-character boundary while allowing multiline paste.
pub fn sanitize_pasted_text(value: &str) -> Result<String, ClipboardRejection> {
    if value.len() > MAX_CLIPBOARD_TEXT_BYTES {
        return Err(ClipboardRejection::TextTooLarge);
    }
    let mut output = String::with_capacity(value.len());
    let mut previous_was_carriage_return = false;
    for character in value.chars() {
        match character {
            '\r' => {
                output.push('\n');
                previous_was_carriage_return = true;
            }
            '\n' if previous_was_carriage_return => previous_was_carriage_return = false,
            '\n' => output.push('\n'),
            '\t' => {
                output.push_str("    ");
                previous_was_carriage_return = false;
            }
            character
                if character.is_control()
                    || matches!(
                        character,
                        '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}'
                    ) =>
            {
                previous_was_carriage_return = false
            }
            character => {
                output.push(character);
                previous_was_carriage_return = false;
            }
        }
        if output.len() > MAX_CLIPBOARD_TEXT_BYTES {
            return Err(ClipboardRejection::TextTooLarge);
        }
    }
    Ok(output)
}

fn validate_image(image: &ClipboardImage) -> Result<(), ClipboardRejection> {
    if image.width == 0
        || image.height == 0
        || image.width > MAX_IMAGE_AXIS
        || image.height > MAX_IMAGE_AXIS
    {
        return Err(ClipboardRejection::InvalidImage);
    }
    let pixels = image
        .width
        .checked_mul(image.height)
        .ok_or(ClipboardRejection::ImageTooLarge)?;
    if pixels > MAX_IMAGE_PIXELS {
        return Err(ClipboardRejection::ImageTooLarge);
    }
    let expected = pixels
        .checked_mul(4)
        .ok_or(ClipboardRejection::ImageTooLarge)?;
    if expected > MAX_IMAGE_BYTES.saturating_mul(2) {
        return Err(ClipboardRejection::ImageTooLarge);
    }
    if image.rgba.len() != expected {
        return Err(ClipboardRejection::InvalidImage);
    }
    Ok(())
}

struct CappedBuffer {
    bytes: Vec<u8>,
    limit: usize,
}

impl CappedBuffer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }

    fn into_inner(self) -> Vec<u8> {
        self.bytes
    }
}

impl io::Write for CappedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > self.limit {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "clipboard PNG exceeds the image limit",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use image::GenericImageView as _;

    use super::{
        ClipboardContents, ClipboardImage, ClipboardReader, ClipboardRejection,
        MAX_CLIPBOARD_TEXT_BYTES, contents_from_text, encode_clipboard_png, local_file_uris,
        sanitize_pasted_text,
    };

    #[derive(Clone)]
    struct FakeClipboard(ClipboardContents);

    impl ClipboardReader for FakeClipboard {
        fn read_once(&self) -> ClipboardContents {
            self.0.clone()
        }
    }

    #[test]
    fn fake_reader_is_deterministic_without_a_platform_clipboard() {
        let reader = FakeClipboard(ClipboardContents::Text("hello".into()));
        assert_eq!(reader.read_once(), ClipboardContents::Text("hello".into()));
    }

    #[test]
    fn complete_local_file_uri_lists_are_attachment_fallbacks() {
        let directory = std::env::temp_dir();
        let first = directory.join("a file.yaml");
        let second = directory.join("b.txt");
        let first_uri = url::Url::from_file_path(&first).unwrap();
        let second_uri = url::Url::from_file_path(&second).unwrap();
        assert_eq!(
            local_file_uris(&format!("copy\n{first_uri}\n{second_uri}\n")),
            Some(vec![first.clone(), second])
        );
        assert!(local_file_uris("file://example.invalid/a.txt").is_none());
        assert!(local_file_uris(&format!("notes\n{first_uri}")).is_none());
        assert!(matches!(
            contents_from_text(first_uri.as_str()),
            ClipboardContents::Files(paths) if paths == vec![first]
        ));
    }

    #[test]
    fn text_paste_normalizes_controls_without_changing_unicode() {
        assert_eq!(
            sanitize_pasted_text("hé\r\nthere\t\u{0007}\u{202e}!").unwrap(),
            "hé\nthere    !"
        );
    }

    #[test]
    fn text_paste_has_a_hard_byte_cap() {
        assert_eq!(
            sanitize_pasted_text(&"x".repeat(MAX_CLIPBOARD_TEXT_BYTES + 1)),
            Err(ClipboardRejection::TextTooLarge)
        );
    }

    #[test]
    fn bitmap_is_encoded_as_a_bounded_png() {
        let (_, bytes) = encode_clipboard_png(ClipboardImage {
            width: 1,
            height: 1,
            rgba: vec![12, 34, 56, 255],
        })
        .unwrap();
        let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).unwrap();
        assert_eq!(decoded.dimensions(), (1, 1));
        assert_eq!(decoded.to_rgba8().as_raw(), &[12, 34, 56, 255]);
    }

    #[test]
    fn malformed_bitmap_is_rejected_before_encoding() {
        assert_eq!(
            encode_clipboard_png(ClipboardImage {
                width: 2,
                height: 1,
                rgba: vec![0; 4],
            }),
            Err(ClipboardRejection::InvalidImage)
        );
    }
}
