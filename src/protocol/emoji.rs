//! NIP-30 custom emoji on messages and reactions, NIP-51 kind 30030
//! `buzz:custom-emoji` sets, and NIP-CE kind 30180 entries.
//!
//! The scanner and set-union rules are ported from block/buzz
//! `crates/buzz-cli/src/commands/emoji.rs` at f3b127e3 (Apache-2.0), which
//! mirror desktop `customEmojiTags.ts` / `customEmojiFromTags`.

use std::collections::{HashMap, HashSet};

use nostr::Event;

pub const KIND_EMOJI_SET: u16 = 30_030;
pub const KIND_EMOJI_ENTRY: u16 = 30_180;
pub const SET_D_TAG: &str = "buzz:custom-emoji";
/// NIP-CE: a filter SHOULD NOT carry more than 50 names.
pub const MAX_LOOKUP_NAMES: usize = 50;

/// One resolvable custom emoji.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Emoji {
    pub shortcode: String,
    pub url: String,
}

/// Candidate shortcodes in `content`: `:([a-z0-9_-]+):`, case-insensitive,
/// lowercase, first appearance order, each once.
pub fn scan_shortcodes(content: &str) -> Vec<String> {
    let bytes = content.as_bytes();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b':' {
            index += 1;
            continue;
        }
        let start = index + 1;
        let mut end = start;
        while end < bytes.len()
            && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_' || bytes[end] == b'-')
        {
            end += 1;
        }
        if end > start && end < bytes.len() && bytes[end] == b':' {
            let shortcode = content[start..end].to_lowercase();
            if shortcode.len() <= 64 && seen.insert(shortcode.clone()) {
                out.push(shortcode);
            }
            index = end + 1;
        } else {
            index += 1;
        }
    }
    out
}

/// `["emoji", shortcode, url]` tags of one event: normalized shortcode, valid
/// URL, first occurrence wins.
pub fn tags_of(tags: &[Vec<String>]) -> Vec<Emoji> {
    let mut seen = HashSet::new();
    tags.iter()
        .filter(|tag| tag.first().map(String::as_str) == Some("emoji"))
        .filter_map(|tag| {
            let shortcode = buzz_sdk::normalize_custom_emoji_shortcode(tag.get(1)?).ok()?;
            let url = tag.get(2)?;
            buzz_sdk::check_custom_emoji_url(url).ok()?;
            seen.insert(shortcode.clone()).then(|| Emoji {
                shortcode,
                url: url.clone(),
            })
        })
        .collect()
}

fn event_tags(event: &Event) -> Vec<Vec<String>> {
    event
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect()
}

/// A palette contribution from one 30030 set or 30180 entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Contribution {
    pub emoji: Emoji,
    pub created_at: u64,
    pub event_id: String,
}

/// Palette contributions carried by a kind 30030 `buzz:custom-emoji` set or a
/// valid kind 30180 entry (`d` equals the `emoji` shortcode, no `h`).
pub fn contributions(event: &Event) -> Vec<Contribution> {
    let tags = event_tags(event);
    let d = tags
        .iter()
        .find(|tag| tag.first().map(String::as_str) == Some("d"))
        .and_then(|tag| tag.get(1))
        .cloned();
    let entries = match event.kind.as_u16() {
        KIND_EMOJI_SET if d.as_deref() == Some(SET_D_TAG) => tags_of(&tags),
        KIND_EMOJI_ENTRY => {
            let entries = tags_of(&tags);
            let scoped = tags
                .iter()
                .any(|tag| tag.first().map(String::as_str) == Some("h"));
            if entries.len() != 1 || scoped || d.as_deref() != Some(entries[0].shortcode.as_str()) {
                return Vec::new();
            }
            entries
        }
        _ => return Vec::new(),
    };
    entries
        .into_iter()
        .map(|emoji| Contribution {
            emoji,
            created_at: event.created_at.as_secs(),
            event_id: event.id.to_hex(),
        })
        .collect()
}

/// Whether `candidate` replaces `current` for one shortcode: newest
/// `created_at` wins across sets and entries (NIP-CE prefers the newest); the
/// tie-break is the lowest event id, then the smallest URL.
pub fn replaces(candidate: &Contribution, current: &Contribution) -> bool {
    (
        candidate.created_at,
        std::cmp::Reverse(&candidate.event_id),
        std::cmp::Reverse(&candidate.emoji.url),
    ) > (
        current.created_at,
        std::cmp::Reverse(&current.event_id),
        std::cmp::Reverse(&current.emoji.url),
    )
}

/// Resolve shortcodes with a message's own tags first, then the palette.
pub fn resolve<'a>(
    shortcode: &str,
    message: &'a [Emoji],
    palette: &'a HashMap<String, String>,
) -> Option<&'a str> {
    message
        .iter()
        .find(|emoji| emoji.shortcode == shortcode)
        .map(|emoji| emoji.url.as_str())
        .or_else(|| palette.get(shortcode).map(String::as_str))
}

/// NIP-30 tags for an outgoing message: one per shortcode known to the
/// palette, in first-appearance order. Unknown shortcodes stay plain text.
pub fn outgoing_tags(content: &str, palette: &HashMap<String, String>) -> Vec<Vec<String>> {
    if !content.contains(':') {
        return Vec::new();
    }
    scan_shortcodes(content)
        .into_iter()
        .filter_map(|shortcode| {
            palette
                .get(&shortcode)
                .map(|url| vec!["emoji".to_owned(), shortcode, url.clone()])
        })
        .collect()
}

/// Palette shortcodes matching `query`: prefix matches first, then
/// substring matches, each alphabetical, at most `limit`.
pub fn matches(palette: &HashMap<String, String>, query: &str, limit: usize) -> Vec<Emoji> {
    let query = query.to_lowercase();
    let mut prefix = Vec::new();
    let mut inner = Vec::new();
    for (shortcode, url) in palette {
        let emoji = Emoji {
            shortcode: shortcode.clone(),
            url: url.clone(),
        };
        if shortcode.starts_with(&query) {
            prefix.push(emoji);
        } else if shortcode.contains(&query) {
            inner.push(emoji);
        }
    }
    prefix.sort_by(|left, right| left.shortcode.cmp(&right.shortcode));
    inner.sort_by(|left, right| left.shortcode.cmp(&right.shortcode));
    prefix.into_iter().chain(inner).take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    #[test]
    fn scanner_matches_desktop() {
        assert_eq!(
            scan_shortcodes("hi :Wave: and :wave: :a::b: : x:"),
            vec!["wave", "a", "b"]
        );
        assert!(scan_shortcodes("no emoji: here").is_empty());
        assert_eq!(scan_shortcodes(":party_parrot-2:"), vec!["party_parrot-2"]);
    }

    #[test]
    fn tags_normalize_and_drop_malformed_entries() {
        let tags = vec![
            vec![
                "emoji".into(),
                "  :WAVE:  ".into(),
                "https://x.io/w.png".into(),
            ],
            vec![
                "emoji".into(),
                "wave".into(),
                "https://x.io/second.png".into(),
            ],
            vec!["emoji".into(), "bad".into(), "javascript:alert(1)".into()],
            vec!["emoji".into(), "nourl".into()],
        ];
        assert_eq!(
            tags_of(&tags),
            vec![Emoji {
                shortcode: "wave".into(),
                url: "https://x.io/w.png".into()
            }]
        );
    }

    #[test]
    fn entries_and_sets_contribute_and_newest_wins() {
        let keys = Keys::generate();
        let entry = |d: &str, shortcode: &str, at: u64, extra: Option<&str>| {
            let mut tags = vec![
                Tag::parse(["d", d]).unwrap(),
                Tag::parse(["emoji", shortcode, "https://x.io/p.gif"]).unwrap(),
            ];
            if let Some(h) = extra {
                tags.push(Tag::parse(["h", h]).unwrap());
            }
            EventBuilder::new(Kind::Custom(KIND_EMOJI_ENTRY), shortcode.replace('_', " "))
                .tags(tags)
                .custom_created_at(nostr::Timestamp::from(at))
                .sign_with_keys(&keys)
                .unwrap()
        };
        assert_eq!(
            contributions(&entry("party_parrot", "party_parrot", 1, None)).len(),
            1
        );
        assert!(
            contributions(&entry("other", "party_parrot", 1, None)).is_empty(),
            "d mismatch"
        );
        assert!(
            contributions(&entry("p", "p", 1, Some("chan"))).is_empty(),
            "channel scoped"
        );
        let set = EventBuilder::new(Kind::Custom(KIND_EMOJI_SET), "")
            .tags([
                Tag::parse(["d", SET_D_TAG]).unwrap(),
                Tag::parse(["emoji", "a", "https://x.io/a.png"]).unwrap(),
                Tag::parse(["emoji", "b", "https://x.io/b.png"]).unwrap(),
            ])
            .custom_created_at(nostr::Timestamp::from(5))
            .sign_with_keys(&keys)
            .unwrap();
        assert_eq!(contributions(&set).len(), 2);
        let older = &contributions(&entry("a", "a", 1, None))[0];
        let newer = &contributions(&set)[0];
        assert!(replaces(newer, older));
        assert!(!replaces(older, newer));
    }

    #[test]
    fn matches_rank_prefix_before_substring() {
        let palette = HashMap::from([
            ("sad_parrot".to_owned(), "https://x.io/1".to_owned()),
            ("parrot".to_owned(), "https://x.io/2".to_owned()),
            ("party".to_owned(), "https://x.io/3".to_owned()),
        ]);
        let found = matches(&palette, "PAR", 8)
            .into_iter()
            .map(|emoji| emoji.shortcode)
            .collect::<Vec<_>>();
        assert_eq!(found, vec!["parrot", "party", "sad_parrot"]);
        assert_eq!(matches(&palette, "par", 1).len(), 1);
    }

    #[test]
    fn outgoing_tags_only_for_known_shortcodes() {
        let palette = HashMap::from([("wave".to_owned(), "https://x.io/w.png".to_owned())]);
        assert_eq!(
            outgoing_tags("hi :wave: :unknown: :WAVE:", &palette),
            vec![vec![
                "emoji".to_owned(),
                "wave".into(),
                "https://x.io/w.png".into()
            ]]
        );
        assert!(outgoing_tags("plain", &palette).is_empty());
    }
}
