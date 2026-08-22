use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

use image::{DynamicImage, ImageDecoder as _, ImageFormat, ImageReader};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use crate::{
    error::{Error, Result},
    paths::set_private_permissions,
};

const MAX_PIXELS: u64 = 25_000_000;
const MAX_AXIS: u32 = 16_384;
const MAX_IMAGE_BYTES: u64 = 50 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;
const MAX_VIDEO_BYTES: u64 = 500 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct StagedFile {
    pub path: PathBuf,
    pub mime: String,
    pub filename: String,
    pub sha256: String,
    pub size: u64,
}

impl StagedFile {
    pub fn pending(&self) -> crate::media::PendingAttachment {
        crate::media::PendingAttachment {
            id: String::new(),
            cache_name: self
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("invalid")
                .to_owned(),
            mime: self.mime.clone(),
            filename: self.filename.clone(),
            sha256: self.sha256.clone(),
            size: self.size,
        }
    }
}

pub fn decode_image(path: &Path) -> Result<DynamicImage> {
    let metadata = fs::metadata(path).map_err(|error| Error::io(path, error))?;
    if metadata.len() == 0 || metadata.len() > MAX_IMAGE_BYTES {
        return Err(Error::Protocol(
            "image byte size exceeds the decode limit".into(),
        ));
    }
    let reader = ImageReader::open(path)
        .map_err(|error| Error::io(path, error))?
        .with_guessed_format()
        .map_err(|_| Error::Protocol("image format could not be detected".into()))?;
    let (width, height) = reader
        .into_dimensions()
        .map_err(|_| Error::Protocol("image dimensions could not be read".into()))?;
    validate_dimensions(width, height)?;
    let image = ImageReader::open(path)
        .map_err(|error| Error::io(path, error))?
        .with_guessed_format()
        .map_err(|_| Error::Protocol("image format could not be detected".into()))?
        .decode()
        .map_err(|_| Error::Protocol("image could not be decoded safely".into()))?;
    validate_dimensions(image.width(), image.height())?;
    Ok(image)
}

/// Stages already-owned bytes such as an explicit clipboard bitmap. The caller
/// must not retain the input after this returns; only sanitized,
/// content-addressed staging metadata leaves this function.
pub fn stage_bytes(staging_dir: &Path, filename: &str, bytes: Vec<u8>) -> Result<StagedFile> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_VIDEO_BYTES {
        return Err(Error::Config(
            "attachment size is outside the supported range".into(),
        ));
    }
    let detected = infer::get(&bytes)
        .map(|kind| kind.mime_type())
        .unwrap_or("application/octet-stream");
    if blocked_mime(detected) {
        return Err(Error::Config(format!(
            "unsupported attachment type: {detected}"
        )));
    }
    if bytes.len() as u64 > upload_limit(detected) {
        return Err(Error::Config(
            "attachment exceeds the upload limit for its media type".into(),
        ));
    }
    fs::create_dir_all(staging_dir).map_err(|error| Error::io(staging_dir, error))?;
    set_private_permissions(staging_dir)?;
    let filename = sanitize_filename(filename);
    let (body, mime) = if detected.starts_with("image/") {
        sanitize_image(bytes, detected)?
    } else {
        (bytes, detected.to_owned())
    };
    if body.len() as u64 > upload_limit(&mime) {
        return Err(Error::Config(
            "processed attachment exceeds the upload limit".into(),
        ));
    }
    write_staged_bytes(staging_dir, &body, mime, filename)
}

pub fn stage_file(source: &Path, staging_dir: &Path) -> Result<StagedFile> {
    use std::io::Read as _;
    let symlink = fs::symlink_metadata(source).map_err(|error| Error::io(source, error))?;
    if symlink.file_type().is_symlink() || !symlink.file_type().is_file() {
        return Err(Error::Config(
            "attachment path must be a regular non-symlink file".into(),
        ));
    }
    if symlink.len() == 0 || symlink.len() > MAX_VIDEO_BYTES {
        return Err(Error::Config(
            "attachment size is outside the supported range".into(),
        ));
    }
    let mut source_file = fs::File::open(source).map_err(|error| Error::io(source, error))?;
    let opened = source_file
        .metadata()
        .map_err(|error| Error::io(source, error))?;
    if !same_file(&symlink, &opened) {
        return Err(Error::Config(
            "attachment path changed while it was being opened".into(),
        ));
    }
    let mut prefix = [0_u8; 8 * 1024];
    let prefix_len = source_file
        .read(&mut prefix)
        .map_err(|error| Error::io(source, error))?;
    let detected = infer::get(&prefix[..prefix_len])
        .map(|kind| kind.mime_type())
        .unwrap_or("application/octet-stream");
    if blocked_mime(detected) {
        return Err(Error::Config(format!(
            "unsupported attachment type: {detected}"
        )));
    }
    let upload_limit = upload_limit(detected);
    if symlink.len() > upload_limit {
        return Err(Error::Config(
            "attachment exceeds the upload limit for its media type".into(),
        ));
    }
    fs::create_dir_all(staging_dir).map_err(|error| Error::io(staging_dir, error))?;
    set_private_permissions(staging_dir)?;
    let filename = sanitize_filename(
        source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file"),
    );
    if detected.starts_with("image/") {
        use std::io::Seek as _;
        source_file
            .rewind()
            .map_err(|error| Error::io(source, error))?;
        let mut bytes = Vec::with_capacity(usize::try_from(symlink.len()).unwrap_or(0));
        source_file
            .take(MAX_IMAGE_BYTES.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| Error::io(source, error))?;
        if bytes.len() as u64 > MAX_IMAGE_BYTES {
            return Err(Error::Config("image exceeds the upload limit".into()));
        }
        return stage_bytes(staging_dir, &filename, bytes);
    }
    use std::io::{Seek as _, Write as _};
    source_file
        .rewind()
        .map_err(|error| Error::io(source, error))?;
    let temporary = staging_dir.join(format!(".{}.part", Uuid::new_v4()));
    let mut output = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| Error::io(&temporary, error))?;
    set_private_permissions(&temporary)?;
    let mut digest = Sha256::new();
    let mut copied = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = source_file
            .read(&mut buffer)
            .map_err(|error| Error::io(source, error))?;
        if read == 0 {
            break;
        }
        copied = copied.saturating_add(read as u64);
        if copied > upload_limit {
            let _ = fs::remove_file(&temporary);
            return Err(Error::Config("attachment exceeds the upload limit".into()));
        }
        digest.update(&buffer[..read]);
        output
            .write_all(&buffer[..read])
            .map_err(|error| Error::io(&temporary, error))?;
    }
    output
        .sync_all()
        .map_err(|error| Error::io(&temporary, error))?;
    let sha256 = hex::encode(digest.finalize());
    let mime = detected.to_owned();
    let destination = staging_dir.join(format!("{sha256}.{}", extension_for(&mime)));
    if destination.exists() {
        fs::remove_file(&temporary).map_err(|error| Error::io(&temporary, error))?;
    } else {
        fs::rename(&temporary, &destination).map_err(|error| Error::io(&destination, error))?;
    }
    Ok(StagedFile {
        path: destination,
        mime,
        filename,
        sha256,
        size: copied,
    })
}

#[cfg(unix)]
fn same_file(before: &fs::Metadata, opened: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    before.dev() == opened.dev() && before.ino() == opened.ino()
}

#[cfg(windows)]
fn same_file(before: &fs::Metadata, opened: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    before.file_attributes() == opened.file_attributes()
        && before.len() == opened.len()
        && before.created().ok() == opened.created().ok()
        && before.modified().ok() == opened.modified().ok()
}

#[cfg(not(any(unix, windows)))]
fn same_file(before: &fs::Metadata, opened: &fs::Metadata) -> bool {
    before.len() == opened.len()
        && before.modified().ok() == opened.modified().ok()
        && before.created().ok() == opened.created().ok()
}

fn upload_limit(mime: &str) -> u64 {
    if mime == "video/mp4" {
        MAX_VIDEO_BYTES
    } else if mime.starts_with("image/") {
        MAX_IMAGE_BYTES
    } else {
        MAX_FILE_BYTES
    }
}

fn write_staged_bytes(
    staging_dir: &Path,
    body: &[u8],
    mime: String,
    filename: String,
) -> Result<StagedFile> {
    use std::io::Write as _;
    let sha256 = hex::encode(Sha256::digest(body));
    let destination = staging_dir.join(format!("{sha256}.{}", extension_for(&mime)));
    if !destination.exists() {
        let temporary = staging_dir.join(format!(".{}.part", Uuid::new_v4()));
        let mut output = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| Error::io(&temporary, error))?;
        set_private_permissions(&temporary)?;
        output
            .write_all(body)
            .map_err(|error| Error::io(&temporary, error))?;
        output
            .sync_all()
            .map_err(|error| Error::io(&temporary, error))?;
        fs::rename(&temporary, &destination).map_err(|error| Error::io(&destination, error))?;
    }
    Ok(StagedFile {
        path: destination,
        mime,
        filename,
        sha256,
        size: body.len() as u64,
    })
}

fn sanitize_image(bytes: Vec<u8>, mime: &str) -> Result<(Vec<u8>, String)> {
    let format = match mime {
        "image/jpeg" => ImageFormat::Jpeg,
        "image/png" if png_is_animated(&bytes) => {
            return Ok((strip_animated_png(&bytes)?, mime.to_owned()));
        }
        "image/png" => ImageFormat::Png,
        "image/webp" if webp_is_animated(&bytes) => {
            return Ok((strip_animated_webp(&bytes)?, mime.to_owned()));
        }
        "image/webp" => ImageFormat::WebP,
        "image/gif" => return Ok((strip_gif_metadata(&bytes)?, mime.to_owned())),
        _ => return Err(Error::Config("unsupported image type".into())),
    };
    let reader = ImageReader::with_format(Cursor::new(&bytes), format);
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| Error::Protocol("image could not be decoded for metadata removal".into()))?;
    let (width, height) = decoder.dimensions();
    validate_dimensions(width, height)?;
    decoder
        .set_limits(image::Limits::default())
        .map_err(|_| Error::Protocol("image exceeds safe decoder limits".into()))?;
    let orientation = decoder
        .orientation()
        .map_err(|_| Error::Protocol("image orientation could not be read".into()))?;
    let mut image = DynamicImage::from_decoder(decoder)
        .map_err(|_| Error::Protocol("image could not be decoded for metadata removal".into()))?;
    image.apply_orientation(orientation);
    validate_dimensions(image.width(), image.height())?;
    let mut output = Cursor::new(Vec::new());
    image
        .write_to(&mut output, format)
        .map_err(|_| Error::Protocol("image could not be encoded without metadata".into()))?;
    Ok((output.into_inner(), mime.to_owned()))
}

fn png_is_animated(bytes: &[u8]) -> bool {
    png_chunks(bytes).is_ok_and(|chunks| chunks.iter().any(|(kind, _)| *kind == b"acTL"))
}

fn strip_animated_png(bytes: &[u8]) -> Result<Vec<u8>> {
    let chunks = png_chunks(bytes)?;
    if chunks
        .iter()
        .any(|(kind, _)| *kind == b"iCCP" || *kind == b"eXIf")
    {
        return Err(Error::Config(
            "animated PNG with an ICC profile or EXIF cannot be sanitized without changing appearance".into(),
        ));
    }
    let mut output = bytes[..8].to_vec();
    for (kind, raw) in chunks {
        if matches!(kind, b"tEXt" | b"zTXt" | b"iTXt" | b"eXIf" | b"iCCP") {
            continue;
        }
        output.extend_from_slice(raw);
    }
    Ok(output)
}

fn png_chunks(bytes: &[u8]) -> Result<Vec<(&[u8; 4], &[u8])>> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(Error::Protocol("invalid PNG signature".into()));
    }
    let mut chunks = Vec::new();
    let mut offset = 8_usize;
    while offset.checked_add(12).is_some_and(|end| end <= bytes.len()) {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let end = offset
            .checked_add(12)
            .and_then(|value| value.checked_add(length))
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| Error::Protocol("invalid PNG chunk length".into()))?;
        let kind: &[u8; 4] = bytes[offset + 4..offset + 8].try_into().unwrap();
        chunks.push((kind, &bytes[offset..end]));
        offset = end;
        if kind == b"IEND" {
            if offset != bytes.len() {
                return Err(Error::Protocol("PNG has trailing bytes".into()));
            }
            return Ok(chunks);
        }
    }
    Err(Error::Protocol("PNG is missing IEND".into()))
}

fn webp_is_animated(bytes: &[u8]) -> bool {
    webp_chunks(bytes).is_ok_and(|chunks| {
        chunks
            .iter()
            .any(|(kind, _)| *kind == b"ANIM" || *kind == b"ANMF")
    })
}

fn strip_animated_webp(bytes: &[u8]) -> Result<Vec<u8>> {
    let chunks = webp_chunks(bytes)?;
    if chunks.iter().any(|(kind, _)| *kind == b"ICCP") {
        return Err(Error::Config(
            "animated WebP with an ICC profile cannot be sanitized without changing colors".into(),
        ));
    }
    let mut body = Vec::new();
    for (kind, raw) in chunks {
        if matches!(kind, b"EXIF" | b"XMP ") {
            continue;
        }
        if kind == b"VP8X" {
            let mut raw = raw.to_vec();
            if raw.len() >= 9 {
                raw[8] &= !(0b0010_0000 | 0b0000_1000 | 0b0000_0100);
            }
            body.extend_from_slice(&raw);
        } else {
            body.extend_from_slice(raw);
        }
    }
    let size =
        u32::try_from(body.len() + 4).map_err(|_| Error::Protocol("WebP is too large".into()))?;
    let mut output = Vec::with_capacity(body.len() + 12);
    output.extend_from_slice(b"RIFF");
    output.extend_from_slice(&size.to_le_bytes());
    output.extend_from_slice(b"WEBP");
    output.extend_from_slice(&body);
    Ok(output)
}

fn webp_chunks(bytes: &[u8]) -> Result<Vec<(&[u8; 4], &[u8])>> {
    if bytes.len() < 12 || !bytes.starts_with(b"RIFF") || &bytes[8..12] != b"WEBP" {
        return Err(Error::Protocol("invalid WebP header".into()));
    }
    let declared = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    if declared.checked_add(8) != Some(bytes.len()) {
        return Err(Error::Protocol("invalid WebP container length".into()));
    }
    let mut chunks = Vec::new();
    let mut offset = 12_usize;
    while offset < bytes.len() {
        if offset + 8 > bytes.len() {
            return Err(Error::Protocol("truncated WebP chunk".into()));
        }
        let length = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let padded = length
            .checked_add(length & 1)
            .ok_or_else(|| Error::Protocol("invalid WebP chunk length".into()))?;
        let end = offset
            .checked_add(8 + padded)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| Error::Protocol("invalid WebP chunk length".into()))?;
        let kind: &[u8; 4] = bytes[offset..offset + 4].try_into().unwrap();
        chunks.push((kind, &bytes[offset..end]));
        offset = end;
    }
    Ok(chunks)
}

fn strip_gif_metadata(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() < 13 || !(bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return Err(Error::Protocol("invalid GIF header".into()));
    }
    let width = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
    let height = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
    validate_dimensions(width, height)?;
    let packed = bytes[10];
    let global_table = if packed & 0x80 != 0 {
        3_usize << (usize::from(packed & 0x07) + 1)
    } else {
        0
    };
    let start = 13_usize
        .checked_add(global_table)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| Error::Protocol("truncated GIF color table".into()))?;
    let mut output = bytes[..start].to_vec();
    let mut offset = start;
    let mut blocks = 0_usize;
    while offset < bytes.len() {
        blocks += 1;
        if blocks > 100_000 {
            return Err(Error::Protocol("GIF contains too many blocks".into()));
        }
        match bytes[offset] {
            0x3b => {
                output.push(0x3b);
                if offset + 1 != bytes.len() {
                    return Err(Error::Protocol("GIF has trailing bytes".into()));
                }
                return Ok(output);
            }
            0x2c => {
                let descriptor_end = offset
                    .checked_add(10)
                    .filter(|end| *end <= bytes.len())
                    .ok_or_else(|| Error::Protocol("truncated GIF image descriptor".into()))?;
                let local = if bytes[offset + 9] & 0x80 != 0 {
                    3_usize << (usize::from(bytes[offset + 9] & 0x07) + 1)
                } else {
                    0
                };
                let data_start = descriptor_end
                    .checked_add(local)
                    .and_then(|value| value.checked_add(1))
                    .filter(|end| *end <= bytes.len())
                    .ok_or_else(|| Error::Protocol("truncated GIF image data".into()))?;
                let end = gif_subblocks_end(bytes, data_start)?;
                output.extend_from_slice(&bytes[offset..end]);
                offset = end;
            }
            0x21 => {
                if offset + 2 > bytes.len() {
                    return Err(Error::Protocol("truncated GIF extension".into()));
                }
                let label = bytes[offset + 1];
                let first_size = *bytes
                    .get(offset + 2)
                    .ok_or_else(|| Error::Protocol("truncated GIF extension".into()))?
                    as usize;
                let data_start = offset
                    .checked_add(3 + first_size)
                    .filter(|end| *end <= bytes.len())
                    .ok_or_else(|| Error::Protocol("truncated GIF extension".into()))?;
                let end = gif_subblocks_end(bytes, data_start)?;
                let keep = label == 0xf9
                    || (label == 0xff
                        && bytes
                            .get(offset + 3..offset + 3 + first_size)
                            .is_some_and(|id| id == b"NETSCAPE2.0" || id == b"ANIMEXTS1.0"));
                if keep {
                    output.extend_from_slice(&bytes[offset..end]);
                }
                offset = end;
            }
            _ => return Err(Error::Protocol("unknown GIF block".into())),
        }
    }
    Err(Error::Protocol("GIF is missing a trailer".into()))
}

fn gif_subblocks_end(bytes: &[u8], mut offset: usize) -> Result<usize> {
    loop {
        let length = *bytes
            .get(offset)
            .ok_or_else(|| Error::Protocol("truncated GIF sub-block".into()))?
            as usize;
        offset += 1;
        if length == 0 {
            return Ok(offset);
        }
        offset = offset
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| Error::Protocol("truncated GIF sub-block".into()))?;
    }
}

fn validate_dimensions(width: u32, height: u32) -> Result<()> {
    if width == 0
        || height == 0
        || width > MAX_AXIS
        || height > MAX_AXIS
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err(Error::Protocol(
            "image dimensions exceed the decode limit".into(),
        ));
    }
    Ok(())
}

pub fn sanitize_filename(value: &str) -> String {
    let value = value.rsplit(['/', '\\']).next().unwrap_or(value).trim();
    let cleaned = value
        .chars()
        .filter(|character| !character.is_control())
        .take(255)
        .collect::<String>();
    if cleaned.is_empty() {
        "file".into()
    } else {
        cleaned
    }
}

fn extension_for(mime: &str) -> &str {
    match mime {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "video/mp4" => "mp4",
        "application/pdf" => "pdf",
        "text/plain" => "txt",
        _ => "bin",
    }
}

fn blocked_mime(mime: &str) -> bool {
    matches!(
        mime,
        "text/html"
            | "application/xhtml+xml"
            | "image/svg+xml"
            | "application/javascript"
            | "text/javascript"
            | "application/x-msdownload"
            | "application/x-executable"
            | "application/vnd.microsoft.portable-executable"
            | "application/x-mach-binary"
            | "application/x-sharedlib"
            | "application/x-elf"
            | "application/x-msi"
            | "application/vnd.android.package-archive"
            | "application/x-apple-diskimage"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_cannot_escape_storage() {
        assert_eq!(sanitize_filename("../../safe.png"), "safe.png");
        assert_eq!(sanitize_filename("a\nb.png"), "ab.png");
    }

    #[test]
    fn dimension_limits_are_checked() {
        assert!(validate_dimensions(1, 1).is_ok());
        assert!(validate_dimensions(16_384, 16_384).is_err());
    }

    #[test]
    fn mp4_uses_the_relays_larger_explicit_upload_ceiling() {
        assert_eq!(upload_limit("application/octet-stream"), 100 * 1024 * 1024);
        assert_eq!(upload_limit("image/png"), 50 * 1024 * 1024);
        assert_eq!(upload_limit("video/mp4"), 500 * 1024 * 1024);
    }

    #[test]
    fn ordinary_unknown_text_files_are_staged_as_generic_attachments() {
        let temporary = tempfile::TempDir::new().unwrap();
        let source = temporary.path().join("config.yaml");
        let staging = temporary.path().join("staging");
        std::fs::write(&source, b"safe: generated\n").unwrap();

        let staged = stage_file(&source, &staging).unwrap();

        assert_eq!(staged.filename, "config.yaml");
        assert_eq!(staged.mime, "application/octet-stream");
        assert_eq!(staged.size, 16);
        assert!(staged.path.is_file());
    }

    #[test]
    fn gif_sanitizer_preserves_pixels_and_removes_comments() {
        let mut gif = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff".to_vec();
        gif.extend_from_slice(b"\x21\xfe\x03gps\x00");
        gif.extend_from_slice(b"\x2c\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02\x44\x01\x00\x3b");
        let clean = strip_gif_metadata(&gif).unwrap();
        assert!(!clean.windows(3).any(|window| window == b"gps"));
        assert!(image::load_from_memory_with_format(&clean, image::ImageFormat::Gif).is_ok());
    }

    #[test]
    fn jpeg_staging_removes_metadata_segments() {
        let temporary = tempfile::TempDir::new().unwrap();
        let source = temporary.path().join("source.jpg");
        let staging = temporary.path().join("staging");
        let image = image::DynamicImage::new_rgb8(2, 2);
        let mut clean = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut clean, image::ImageFormat::Jpeg)
            .unwrap();
        let clean = clean.into_inner();
        let payload = b"Exif\0\0location";
        let mut tagged = Vec::new();
        tagged.extend_from_slice(&clean[..2]);
        tagged.extend_from_slice(&[0xff, 0xe1]);
        tagged.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
        tagged.extend_from_slice(payload);
        tagged.extend_from_slice(&clean[2..]);
        std::fs::write(&source, tagged).unwrap();
        let staged = stage_file(&source, &staging).unwrap();
        let output = std::fs::read(staged.path).unwrap();
        assert!(!output.windows(6).any(|window| window == b"Exif\0\0"));
    }
}
