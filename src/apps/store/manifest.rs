//! The manifest and the brief of an app: their formats, their limits, and
//! the names an app may have.
//!
//! **The manifest** is a small TOML file, `app.toml`:
//!
//! ```toml
//! name = "board"
//! title = "Board"
//! api = 1
//! types = ["card"]
//! ```
//!
//! `api` and `types` repeat what the script says (`app.api`, `app.types`).
//! **The script is the authority**: bzz loads it and takes both from what it
//! declared. A manifest that comes with a script (a folder given to `bzz app
//! add`) is checked against that and refused when it differs. The manifest
//! bzz keeps with a version is always one bzz wrote itself, from the script
//! it loaded, with [`Manifest::to_text`]: the same four fields give the same
//! bytes. `name` and `title` are only in the manifest: the contract gives a
//! script no field for them.
//!
//! **The brief** is plain text, `brief.md`: what the app does, its record
//! types and their fields (plan D16). It is what a person reads before
//! approving and what another client's agent writes the same app from. bzz
//! never reads meaning into it; it only holds it to a size and to being
//! text.

use serde::{Deserialize, Serialize};

use crate::apps::runtime::{API_VERSION, limits, records::valid_type_name};
use crate::protocol::record::{MAX_NAME_BYTES, valid_name};

/// The files of an app, in a folder and in a kept version.
pub const SCRIPT_FILE: &str = "app.lua";
pub const MANIFEST_FILE: &str = "app.toml";
pub const BRIEF_FILE: &str = "brief.md";

/// The largest script: the runtime's own limit, and what travels as one
/// artifact (ADR, "Records on the relay").
pub const SCRIPT_BYTES: usize = limits::SCRIPT_BYTES;
/// The largest brief (the same section of the ADR).
pub const BRIEF_BYTES: usize = 16 * 1024;
/// The largest manifest bzz reads. One of 16 types of 32 bytes and a title
/// is under 1 KiB.
pub const MANIFEST_BYTES: usize = 4 * 1024;
/// The longest title, in characters: it is one line of a list and of the
/// pane's border.
pub const TITLE_CHARS: usize = 64;

/// Names Windows keeps for devices, with any extension. An app's name is a
/// folder's name on every machine of its channel.
const DEVICE_NAMES: &[&str] = &["con", "prn", "aux", "nul"];

/// Is `name` one an app may have: `[a-z][a-z0-9_-]{0,31}`, and a folder's
/// name on every platform. The error says what a name is.
pub fn check_name(name: &str) -> Result<(), String> {
    if !valid_name(name) {
        return Err(format!(
            "{name:?} is not a name an app may have: a lowercase letter, then up to {} of a-z, 0-9, _ or -",
            MAX_NAME_BYTES - 1
        ));
    }
    let device = DEVICE_NAMES.contains(&name)
        || (name.len() == 4
            && (name.starts_with("com") || name.starts_with("lpt"))
            && name.as_bytes()[3].is_ascii_digit());
    if device {
        return Err(format!(
            "{name:?} is not a name an app may have: Windows keeps it for a device, and an app's name is a folder's name"
        ));
    }
    Ok(())
}

/// A character that is not text: it moves the cursor, starts an escape
/// sequence, or is not shown at all.
fn is_control(character: char) -> bool {
    character.is_control()
}

/// A character that changes the direction text is shown in. A line that
/// holds one can read differently from what it says.
fn is_direction_override(character: char) -> bool {
    matches!(character, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Text as it is safe to put on the owner's screen: every character that is
/// not text, and every one that changes the direction of what follows, is
/// written out as `<U+XXXX>`. Line breaks and tabs stay.
///
/// What the owner approves, he approves by what he is shown. A script or a
/// brief must not be able to draw over that with an escape sequence, or to
/// show a line in another order than it runs in.
pub fn visible(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\n' | '\t' => out.push(character),
            // A Windows line ending is a line ending.
            '\r' if characters.peek() == Some(&'\n') => {}
            c if is_control(c) || is_direction_override(c) => {
                out.push_str(&format!("<U+{:04X}>", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// How many characters of `text` [`visible`] writes out: what a reader
/// would not have seen.
pub fn hidden_characters(text: &str) -> usize {
    let mut count = 0;
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\n' | '\t' => {}
            '\r' if characters.peek() == Some(&'\n') => {}
            c if is_control(c) || is_direction_override(c) => count += 1,
            _ => {}
        }
    }
    count
}

/// A brief as it was given, if it may be one: UTF-8, with something in it,
/// at most [`BRIEF_BYTES`], and text. The bytes are kept as they are.
pub fn check_brief(bytes: &[u8]) -> Result<&str, String> {
    if bytes.len() > BRIEF_BYTES {
        return Err(format!(
            "the brief is {} bytes; a brief may be {BRIEF_BYTES}",
            bytes.len()
        ));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "the brief is not text: it is not valid UTF-8".to_string())?;
    if text.trim().is_empty() {
        return Err(
            "the brief is empty: say what the app does, its record types and their fields".into(),
        );
    }
    let mut characters = text.chars().peekable();
    let mut line = 1;
    while let Some(character) = characters.next() {
        match character {
            '\n' => line += 1,
            '\t' => {}
            '\r' if characters.peek() == Some(&'\n') => {}
            c if is_control(c) => {
                return Err(format!(
                    "the brief is not plain text: line {line} holds the control character U+{:04X}",
                    c as u32
                ));
            }
            _ => {}
        }
    }
    Ok(text)
}

/// A title as it is kept: one line of text, trimmed, with something in it,
/// at most [`TITLE_CHARS`] characters.
pub fn check_title(title: &str) -> Result<String, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("the title is empty".into());
    }
    if title.chars().count() > TITLE_CHARS {
        return Err(format!(
            "the title is {} characters; a title may be {TITLE_CHARS}",
            title.chars().count()
        ));
    }
    if title
        .chars()
        .any(|c| is_control(c) || is_direction_override(c))
    {
        return Err("the title is not one line of plain text".into());
    }
    Ok(title.to_owned())
}

/// A title for an app that was given none: its name, with a capital.
pub fn title_from_name(name: &str) -> String {
    let mut characters = name.chars();
    match characters.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + characters.as_str(),
        None => String::new(),
    }
}

/// `app.toml`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The app's name: `[a-z][a-z0-9_-]{0,31}`. Its record types on the
    /// relay are `bzz.app.<name>.<type>`, so two apps of one name in one
    /// channel share their records.
    pub name: String,
    /// What a person is shown.
    pub title: String,
    /// The number of the contract the script is written against.
    pub api: i64,
    /// The record types the script declares, in its order.
    pub types: Vec<String>,
}

impl Manifest {
    /// Reads a manifest and checks every field. The error names the field.
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > MANIFEST_BYTES {
            return Err(format!(
                "the manifest is {} bytes; a manifest may be {MANIFEST_BYTES}",
                text.len()
            ));
        }
        let mut manifest: Self = toml::from_str(text).map_err(|error| {
            let message = error.message().trim();
            match error
                .span()
                .map(|span| text[..span.start.min(text.len())].matches('\n').count() + 1)
            {
                Some(line) => format!("line {line}: {message}"),
                None => message.to_owned(),
            }
        })?;
        check_name(&manifest.name)?;
        manifest.title = check_title(&manifest.title)?;
        if manifest.api != API_VERSION {
            return Err(format!(
                "api = {}: this bzz knows api = {API_VERSION}",
                manifest.api
            ));
        }
        if manifest.types.is_empty() || manifest.types.len() > limits::TYPES {
            return Err(format!(
                "types: an app declares 1 to {} record types, and this names {}",
                limits::TYPES,
                manifest.types.len()
            ));
        }
        for (index, name) in manifest.types.iter().enumerate() {
            if !valid_type_name(name) {
                return Err(format!(
                    "types: {name:?} is not a record type's name: a lowercase letter, then up to {} of a-z, 0-9, _ or -",
                    limits::TYPE_NAME_BYTES - 1
                ));
            }
            if manifest.types[..index].contains(name) {
                return Err(format!("types: {name:?} is named twice"));
            }
        }
        Ok(manifest)
    }

    /// The manifest as bzz writes it: the same fields always give the same
    /// bytes.
    pub fn to_text(&self) -> String {
        let quoted = |text: &str| toml::Value::String(text.to_owned()).to_string();
        let types = self
            .types
            .iter()
            .map(|name| quoted(name))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "name = {}\ntitle = {}\napi = {}\ntypes = [{types}]\n",
            quoted(&self.name),
            quoted(&self.title),
            self.api
        )
    }

    /// Does this manifest declare what a loaded script declared? The order
    /// of the types is the script's and does not matter here.
    pub fn agrees_with(&self, loaded_types: &[String]) -> Result<(), String> {
        let mut said = self.types.clone();
        let mut loaded = loaded_types.to_vec();
        said.sort();
        loaded.sort();
        if said == loaded {
            return Ok(());
        }
        Err(format!(
            "{MANIFEST_FILE} says the record types are {}; the script declares {}. The script is the authority: make {MANIFEST_FILE} say the same",
            self.types.join(", "),
            loaded_types.join(", ")
        ))
    }
}

/// The SHA-256 of some bytes, in hex.
pub fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    hex::encode(Sha256::digest(bytes))
}

/// A size as a person reads it.
pub fn bytes(count: u64) -> String {
    if count < 1024 {
        format!("{count} bytes")
    } else {
        format!("{:.1} KiB", count as f64 / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_a_record_name_and_a_folder_on_every_platform() {
        for name in ["board", "a", "rota-2", "my_app", &"a".repeat(32)] {
            assert!(check_name(name).is_ok(), "{name}");
        }
        for name in [
            "",
            "Board",
            "2fast",
            "a b",
            "a/b",
            "..",
            "a.b",
            "tablón",
            &"a".repeat(33),
        ] {
            assert!(check_name(name).is_err(), "{name}");
        }
        for name in ["con", "nul", "aux", "prn", "com1", "lpt9"] {
            let error = check_name(name).unwrap_err();
            assert!(error.contains("Windows"), "{error}");
        }
        // Not devices: only the exact names are.
        for name in ["console", "com", "com10", "lpta"] {
            assert!(check_name(name).is_ok(), "{name}");
        }
    }

    #[test]
    fn the_manifest_bzz_writes_reads_back_the_same() {
        let manifest = Manifest {
            name: "board".into(),
            title: "Tablón \"de\" tareas \\ 日本".into(),
            api: 1,
            types: vec!["card".into(), "vote-2".into()],
        };
        let text = manifest.to_text();
        assert_eq!(Manifest::parse(&text).unwrap(), manifest);
        assert_eq!(text, manifest.to_text(), "the same bytes every time");
        assert!(text.starts_with("name = \"board\"\ntitle = "), "{text}");
        assert!(text.ends_with("api = 1\ntypes = [\"card\", \"vote-2\"]\n"));
    }

    #[test]
    fn a_manifest_that_is_wrong_says_which_field() {
        let with = |change: &str| {
            let mut lines = vec![
                "name = \"board\"".to_string(),
                "title = \"Board\"".to_string(),
                "api = 1".to_string(),
                "types = [\"card\"]".to_string(),
            ];
            let field = change.split(' ').next().unwrap();
            lines.retain(|line| !line.starts_with(&format!("{field} ")));
            lines.push(change.to_string());
            Manifest::parse(&lines.join("\n")).unwrap_err()
        };
        assert!(with("api = 2").contains("this bzz knows api = 1"));
        assert!(with("name = \"Board\"").contains("not a name an app may have"));
        assert!(with("types = []").contains("1 to 16 record types"));
        assert!(with("types = [\"Card\"]").contains("not a record type's name"));
        assert!(with("types = [\"a\", \"a\"]").contains("named twice"));
        assert!(with("title = \"\"").contains("title is empty"));
        assert!(with("title = \"a\\u001b[2Jb\"").contains("one line of plain text"));
        let unknown = with("network = true");
        assert!(
            unknown.contains("line 5") && unknown.contains("network"),
            "{unknown}"
        );
        assert!(
            Manifest::parse("name = \"board\"")
                .unwrap_err()
                .contains("title")
        );
        assert!(
            Manifest::parse(&"#".repeat(MANIFEST_BYTES + 1))
                .unwrap_err()
                .contains("a manifest may be")
        );
    }

    #[test]
    fn the_manifest_is_checked_against_the_script_whatever_the_order() {
        let manifest = Manifest {
            name: "poll".into(),
            title: "Poll".into(),
            api: 1,
            types: vec!["poll".into(), "vote".into()],
        };
        assert!(
            manifest
                .agrees_with(&["vote".into(), "poll".into()])
                .is_ok()
        );
        let error = manifest.agrees_with(&["poll".into()]).unwrap_err();
        assert!(
            error.contains("app.toml says the record types are poll, vote")
                && error.contains("the script declares poll")
                && error.contains("The script is the authority"),
            "{error}"
        );
    }

    #[test]
    fn a_brief_is_text_of_a_bounded_size() {
        assert_eq!(
            check_brief(b"A board.\r\n\tcard: title\n"),
            Ok("A board.\r\n\tcard: title\n")
        );
        assert!(check_brief(b" \n\t").unwrap_err().contains("empty"));
        assert!(check_brief(&[0xff, 0xfe]).unwrap_err().contains("UTF-8"));
        assert!(check_brief(&vec![b'a'; BRIEF_BYTES]).is_ok());
        assert!(
            check_brief(&vec![b'a'; BRIEF_BYTES + 1])
                .unwrap_err()
                .contains("a brief may be 16384")
        );
        let escape = check_brief(b"fine\nA board.\x1b[2J").unwrap_err();
        assert!(
            escape.contains("line 2") && escape.contains("U+001B"),
            "{escape}"
        );
        assert!(check_brief(b"a\rb").is_err(), "a lone carriage return");
    }

    #[test]
    fn what_is_not_text_is_written_out() {
        assert_eq!(
            visible("ok\x1b[2J\r\nnext\tcol\rover\u{202E}desrever\u{7f}"),
            "ok<U+001B>[2J\nnext\tcol<U+000D>over<U+202E>desrever<U+007F>"
        );
        assert_eq!(hidden_characters("ok\x1b[2J\r\nnext\rover\u{202E}x"), 3);
        assert_eq!(hidden_characters("plain\n\ttext — 日本"), 0);
        // Marks that scripts of the world need stay as they are.
        assert_eq!(visible("a\u{200D}b\u{200F}"), "a\u{200D}b\u{200F}");
    }

    #[test]
    fn a_title_is_one_line() {
        assert_eq!(check_title("  Board  ").unwrap(), "Board");
        assert!(check_title("a\nb").is_err());
        assert!(check_title(&"a".repeat(TITLE_CHARS + 1)).is_err());
        assert_eq!(title_from_name("board-2"), "Board-2");
    }
}
