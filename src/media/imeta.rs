use std::collections::HashMap;

use url::Url;

use super::model::{Attachment, MediaKind};

pub const MAX_INBOUND_ATTACHMENTS: usize = 16;
const MAX_PARTS: usize = 32;
const MAX_FIELD_BYTES: usize = 2_048;
const MAX_FILENAME_CHARS: usize = 255;
const MAX_ALT_CHARS: usize = 2_048;

pub fn parse_tags(tags_json: &str, content: &str, base: &Url) -> Vec<Attachment> {
    let Ok(tags) = serde_json::from_str::<Vec<Vec<String>>>(tags_json) else {
        return Vec::new();
    };
    tags.into_iter()
        .filter(|tag| tag.first().is_some_and(|value| value == "imeta"))
        .take(MAX_INBOUND_ATTACHMENTS)
        .enumerate()
        .map(|(index, tag)| parse_one(index, &tag, content, base))
        .collect()
}

fn parse_one(index: usize, tag: &[String], content: &str, base: &Url) -> Attachment {
    let mut values = HashMap::<&str, &str>::new();
    let mut error = None;
    if tag.len().saturating_sub(1) > MAX_PARTS {
        error = Some("attachment metadata has too many fields".to_owned());
    }
    for part in tag.iter().skip(1).take(MAX_PARTS) {
        if part.len() > MAX_FIELD_BYTES {
            error.get_or_insert_with(|| "attachment metadata field is too large".to_owned());
            continue;
        }
        let Some((key, value)) = part.split_once(' ') else {
            error.get_or_insert_with(|| "attachment metadata field is malformed".to_owned());
            continue;
        };
        if !matches!(
            key,
            "url"
                | "m"
                | "x"
                | "size"
                | "dim"
                | "blurhash"
                | "alt"
                | "thumb"
                | "fallback"
                | "duration"
                | "bitrate"
                | "image"
                | "filename"
        ) {
            error.get_or_insert_with(|| "attachment metadata contains an unknown field".to_owned());
            continue;
        }
        if key != "fallback" && values.insert(key, value).is_some() {
            error.get_or_insert_with(|| "attachment metadata repeats a field".to_owned());
        }
    }

    let raw_url = values.get("url").copied().unwrap_or_default();
    let mime = values.get("m").copied().unwrap_or_default().to_owned();
    let sha256 = values.get("x").copied().unwrap_or_default().to_owned();
    let size = values
        .get("size")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_default();
    if raw_url.is_empty() || mime.is_empty() || sha256.is_empty() || size == 0 {
        error.get_or_insert_with(|| "attachment metadata is incomplete".to_owned());
    }
    if !valid_mime(&mime) {
        error.get_or_insert_with(|| "attachment MIME type is invalid".to_owned());
    }
    if !is_hash(&sha256) {
        error.get_or_insert_with(|| "attachment hash is invalid".to_owned());
    }

    let url = match validate_media_url(raw_url, base, Some(&sha256), false) {
        Ok(url) => url,
        Err(message) => {
            error.get_or_insert(message);
            String::new()
        }
    };

    let (width, height) = values
        .get("dim")
        .and_then(|value| parse_dimensions(value))
        .unwrap_or((None, None));
    if values.contains_key("dim") && width.is_none() {
        error.get_or_insert_with(|| "attachment dimensions are invalid".to_owned());
    }
    let filename = values.get("filename").map(|value| (*value).to_owned());
    if filename
        .as_deref()
        .is_some_and(|value| !valid_filename(value))
    {
        error.get_or_insert_with(|| "attachment filename is invalid".to_owned());
    }
    let alt = values.get("alt").map(|value| (*value).to_owned());
    if alt
        .as_deref()
        .is_some_and(|value| value.chars().count() > MAX_ALT_CHARS || unsafe_text(value))
    {
        error.get_or_insert_with(|| "attachment alt text is invalid".to_owned());
    }
    let kind = classify(&mime);
    let poster = values.get("image").and_then(|value| {
        validate_media_url(value, base, None, false)
            .ok()
            .filter(|url| hash_from_path(url).is_some())
    });
    let thumb = values
        .get("thumb")
        .and_then(|value| validate_media_url(value, base, None, true).ok());
    let duration_millis = values.get("duration").and_then(|value| {
        value
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite() && *value > 0.0)
            .map(|value| (value * 1_000.0).round() as u64)
    });
    let spoiler = !url.is_empty() && is_spoilered(content, &url);

    Attachment {
        index,
        url,
        mime,
        sha256,
        size,
        width,
        height,
        alt,
        blurhash: values.get("blurhash").map(|value| (*value).to_owned()),
        thumb,
        poster,
        filename,
        duration_millis,
        kind,
        spoiler,
        error,
    }
}

pub fn strip_attachment_lines(content: &str, attachments: &[Attachment]) -> String {
    if attachments.is_empty() {
        return content.to_owned();
    }
    let urls = attachments
        .iter()
        .filter(|attachment| attachment.valid())
        .map(|attachment| attachment.url.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut lines = content.lines().collect::<Vec<_>>();
    while let Some(line) = lines.last().copied() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            lines.pop();
            continue;
        }
        let bare = trimmed
            .strip_prefix("||")
            .and_then(|value| value.strip_suffix("||"))
            .unwrap_or(trimmed);
        if attachment_line_url(bare).is_some_and(|url| urls.contains(url)) {
            lines.pop();
            continue;
        }
        break;
    }
    lines.join("\n").trim_end().to_owned()
}

fn attachment_line_url(line: &str) -> Option<&str> {
    let start = if line.starts_with("![image](") {
        "![image](".len()
    } else if line.starts_with("![video](") {
        "![video](".len()
    } else if line.starts_with('[') {
        line.find("](")? + 2
    } else {
        return None;
    };
    line.get(start..)?.strip_suffix(')')
}

fn is_spoilered(content: &str, url: &str) -> bool {
    content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("||")
            .and_then(|value| value.strip_suffix("||"))
            .and_then(attachment_line_url)
            == Some(url)
    })
}

pub fn validate_media_url(
    input: &str,
    base: &Url,
    expected_hash: Option<&str>,
    thumbnail: bool,
) -> Result<String, String> {
    if input.len() > MAX_FIELD_BYTES || input.contains('%') || input.contains('\\') {
        return Err("attachment URL is invalid".into());
    }
    let url = if input.starts_with('/') {
        base.join(input)
    } else {
        Url::parse(input)
    }
    .map_err(|_| "attachment URL is invalid".to_owned())?;
    if url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.scheme() != base.scheme()
        || url.host_str() != base.host_str()
        || url.port_or_known_default() != base.port_or_known_default()
    {
        return Err("attachment URL is outside the active community".into());
    }
    let tail = url
        .path()
        .strip_prefix("/media/")
        .filter(|tail| !tail.contains('/'))
        .ok_or_else(|| "attachment URL does not use a canonical media path".to_owned())?;
    if thumbnail {
        let Some(hash) = tail.strip_suffix(".thumb.jpg") else {
            return Err("attachment thumbnail path is invalid".into());
        };
        if !is_hash(hash) {
            return Err("attachment thumbnail hash is invalid".into());
        }
    } else {
        let (hash, extension) = tail
            .split_once('.')
            .ok_or_else(|| "attachment media path is invalid".to_owned())?;
        if !is_hash(hash)
            || extension.is_empty()
            || extension.len() > 16
            || !extension
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        {
            return Err("attachment media path is invalid".into());
        }
        if expected_hash.is_some_and(|expected| expected != hash) {
            return Err("attachment URL hash does not match its descriptor".into());
        }
    }
    Ok(url.to_string())
}

pub fn hash_from_path(url: &str) -> Option<String> {
    let url = Url::parse(url).ok()?;
    let tail = url.path().strip_prefix("/media/")?;
    let hash = tail.split_once('.')?.0;
    is_hash(hash).then(|| hash.to_owned())
}

fn is_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn valid_mime(value: &str) -> bool {
    let Some((kind, subtype)) = value.split_once('/') else {
        return false;
    };
    !kind.is_empty()
        && !subtype.is_empty()
        && value.len() <= 127
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-' | b'/'
                )
        })
}

fn parse_dimensions(value: &str) -> Option<(Option<u32>, Option<u32>)> {
    let (width, height) = value.split_once('x')?;
    let width = width
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0 && *value <= 16_384)?;
    let height = height
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0 && *value <= 16_384)?;
    ((u64::from(width) * u64::from(height)) <= 25_000_000).then_some((Some(width), Some(height)))
}

fn classify(mime: &str) -> MediaKind {
    match mime {
        "image/jpeg" | "image/png" | "image/gif" | "image/webp" => MediaKind::Image,
        "video/mp4" => MediaKind::Video,
        value if value.starts_with("image/") || value.starts_with("video/") => {
            MediaKind::Unsupported
        }
        _ => MediaKind::File,
    }
}

fn valid_filename(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= MAX_FILENAME_CHARS
        && !value.contains(['/', '\\'])
        && !unsafe_text(value)
}

fn unsafe_text(value: &str) -> bool {
    value.chars().any(|character| {
        character.is_control()
            || matches!(
                character,
                '\u{061c}'
                    | '\u{200e}'
                    | '\u{200f}'
                    | '\u{202a}'..='\u{202e}'
                    | '\u{2066}'..='\u{2069}'
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn parses_and_strips_a_bound_image() {
        let base = Url::parse("https://buzz.example/").unwrap();
        let tags = serde_json::to_string(&vec![vec![
            "imeta",
            &format!("url https://buzz.example/media/{HASH}.png"),
            "m image/png",
            &format!("x {HASH}"),
            "size 42",
            "dim 2x3",
            "alt safe image",
        ]])
        .unwrap();
        let content = format!("hello\n![image](https://buzz.example/media/{HASH}.png)");
        let parsed = parse_tags(&tags, &content, &base);
        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].valid());
        assert_eq!(parsed[0].width, Some(2));
        assert_eq!(strip_attachment_lines(&content, &parsed), "hello");
    }

    #[test]
    fn rejects_external_and_hash_confused_urls() {
        let base = Url::parse("https://buzz.example/").unwrap();
        assert!(
            validate_media_url(
                &format!("https://evil.example/media/{HASH}.png"),
                &base,
                Some(HASH),
                false
            )
            .is_err()
        );
        let other = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        assert!(
            validate_media_url(
                &format!("https://buzz.example/media/{other}.png"),
                &base,
                Some(HASH),
                false
            )
            .is_err()
        );
    }

    proptest::proptest! {
        #[test]
        fn arbitrary_imeta_fields_never_panic(parts in proptest::collection::vec(".{0,128}", 0..64)) {
            let base = Url::parse("https://buzz.example/").unwrap();
            let mut tag = vec!["imeta".to_owned()];
            tag.extend(parts);
            let tags = serde_json::to_string(&vec![tag]).unwrap();
            let parsed = parse_tags(&tags, "", &base);
            proptest::prop_assert!(parsed.len() <= 1);
        }
    }

    #[test]
    fn spoiler_is_not_fetched_implicitly() {
        let base = Url::parse("https://buzz.example/").unwrap();
        let tags = serde_json::to_string(&vec![vec![
            "imeta",
            &format!("url /media/{HASH}.jpg"),
            "m image/jpeg",
            &format!("x {HASH}"),
            "size 1",
        ]])
        .unwrap();
        let parsed = parse_tags(
            &tags,
            &format!("||![image](https://buzz.example/media/{HASH}.jpg)||"),
            &base,
        );
        assert!(parsed[0].spoiler);
    }
}
