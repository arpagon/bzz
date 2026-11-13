//! Mention rules, version 3, as specified by `block/buzz-app`
//! `src/bundled/mentions/README.md` (vendored fixtures in
//! `tests/fixtures/buzz-app/`): who can be mentioned, how a typed name
//! matches and orders the choices, when Space or closing punctuation selects
//! a name, and the `p` / `mention` tags a message carries.
//!
//! Labels are presentation only: messages carry exact keys.

use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
};

use icu_properties::{
    CodePointMapData,
    props::{GeneralCategory, GeneralCategoryGroup},
};
use unicode_normalization::UnicodeNormalization as _;

use crate::protocol::identity_names::{is_js_space, js_trim};

// ---------------------------------------------------------------------------
// Normalization and matching (section 3)
// ---------------------------------------------------------------------------

/// The "Combining Diacritical Marks" blocks: accents that fold away.
fn is_accent(c: char) -> bool {
    matches!(c,
        '\u{0300}'..='\u{036F}' | '\u{1AB0}'..='\u{1AFF}' | '\u{1DC0}'..='\u{1DFF}'
        | '\u{20D0}'..='\u{20FF}' | '\u{FE20}'..='\u{FE2F}')
}

/// Fold one code point: NFKD, no accents, lowercase, `σ` for final `ς`.
fn fold_point(c: char, out: &mut Vec<char>) {
    if c.is_ascii() {
        out.push(c.to_ascii_lowercase());
        return;
    }
    let decomposed = std::iter::once(c)
        .nfkd()
        .filter(|c| !is_accent(*c))
        .collect::<String>();
    out.extend(
        decomposed
            .to_lowercase()
            .chars()
            .map(|c| if c == 'ς' { 'σ' } else { c }),
    );
}

fn fold_chars(text: &str) -> Vec<char> {
    let mut out = Vec::with_capacity(text.len());
    for c in text.chars() {
        fold_point(c, &mut out);
    }
    out
}

/// A name or query without accents and case; keeps spaces at the ends.
pub fn fold_name(text: &str) -> String {
    fold_chars(text).into_iter().collect()
}

/// The normalized form for comparing and ordering names.
pub fn normalize_name(text: &str) -> String {
    js_trim(&fold_name(text)).to_owned()
}

/// A name character: Unicode general category L, M or N.
pub fn is_name_char(c: char) -> bool {
    let category = CodePointMapData::<GeneralCategory>::new().get(c);
    GeneralCategoryGroup::Letter.contains(category)
        || GeneralCategoryGroup::Mark.contains(category)
        || GeneralCategoryGroup::Number.contains(category)
}

fn is_word(c: Option<&char>) -> bool {
    c.is_some_and(|c| is_name_char(*c))
}

fn trimmed_bounds(chars: &[char]) -> (usize, usize) {
    let mut start = 0;
    let mut end = chars.len();
    while start < end && is_js_space(chars[start]) {
        start += 1;
    }
    while end > start && is_js_space(chars[end - 1]) {
        end -= 1;
    }
    (start, end)
}

/// The tier (0 equal, 1 prefix, 2 a word equals, 3 a word starts) at which
/// `name` matches the normalized `needle`, or `None`.
fn search(chars: &[char], needle: &[char]) -> Option<u32> {
    let (start, end) = trimmed_bounds(chars);
    if needle.is_empty() {
        return Some(0);
    }
    let at =
        |from: usize| from + needle.len() <= end && chars[from..from + needle.len()] == *needle;
    if end - start == needle.len() && at(start) {
        return Some(0);
    }
    if end - start > needle.len() && at(start) {
        return Some(1);
    }
    if needle.iter().any(|c| is_js_space(*c)) {
        return None;
    }
    let mut prefix = false;
    for from in start + 1..end {
        if !is_word(chars.get(from)) || is_word(chars.get(from - 1)) || !at(from) {
            continue;
        }
        if !is_word(chars.get(from + needle.len())) {
            return Some(2);
        }
        prefix = true;
    }
    prefix.then_some(3)
}

/// How `name` matches `query`: a tier 0–3, or `None`. An empty query
/// matches at tier 0.
pub fn match_tier(name: &str, query: &str) -> Option<u32> {
    let needle = normalize_name(query).chars().collect::<Vec<_>>();
    search(&fold_chars(name), &needle)
}

/// Whether `name` is longer than `query` and continues it with a new word
/// (`Jose Luis` and `Jose-Luis` continue `jose`; `Josefa` does not).
pub fn extends_name(name: &str, query: &str) -> bool {
    let whole = normalize_name(name).chars().collect::<Vec<_>>();
    let needle = normalize_name(query).chars().collect::<Vec<_>>();
    !needle.is_empty()
        && whole.len() > needle.len()
        && whole[..needle.len()] == *needle
        && !is_word(whole.get(needle.len()))
}

/// The key a query names exactly: 64 hex characters in either case, or a
/// complete all-lowercase or all-uppercase npub, with or without a `nostr:`
/// prefix in any case. Anything else is a name search.
pub fn typed_public_key(query: &str) -> Option<String> {
    let value = js_trim(query);
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Some(value.to_ascii_lowercase());
    }
    let value = match value.get(..6) {
        Some(prefix) if prefix.eq_ignore_ascii_case("nostr:") => &value[6..],
        _ => value,
    };
    let uniform = value == value.to_ascii_lowercase() || value == value.to_ascii_uppercase();
    if !uniform || !value.to_ascii_lowercase().starts_with("npub1") {
        return None;
    }
    use nostr::nips::nip19::FromBech32 as _;
    nostr::PublicKey::from_bech32(&value.to_ascii_lowercase())
        .ok()
        .map(|key| key.to_hex())
}

// ---------------------------------------------------------------------------
// Choices and order (sections 1, 3, 4)
// ---------------------------------------------------------------------------

/// One key the user can mention.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Choice {
    pub pubkey: String,
    /// The identity's own name.
    pub name: String,
    /// The name shown, possibly qualified (`Wes’s Honey`, `Honey · 4prr`).
    pub label: String,
    /// The generated key suffix that ends `label`, if any.
    pub qualifier: Option<String>,
    /// Real names known for this key; empty when only a key fallback exists.
    pub aliases: Vec<String>,
    pub member: bool,
    /// The host of another community this agent comes from.
    pub elsewhere: Option<String>,
    pub agent: bool,
    pub owned: bool,
    pub managed: bool,
}

const NO_MATCH: u32 = u32::MAX;

fn name_match(text: &str, needle: &[char]) -> u32 {
    if needle.is_empty() {
        return 0;
    }
    search(&fold_chars(text), needle).unwrap_or(NO_MATCH)
}

fn alias_match(choice: &Choice, needle: &[char]) -> u32 {
    choice
        .aliases
        .iter()
        .map(|name| name_match(name, needle))
        .min()
        .unwrap_or(NO_MATCH)
}

fn label_match(choice: &Choice, needle: &[char], key: Option<&str>) -> u32 {
    if let Some(key) = key {
        return if choice.pubkey == key { 0 } else { NO_MATCH };
    }
    if choice.aliases.is_empty() {
        return NO_MATCH;
    }
    let visible = name_match(&choice.label, needle);
    if visible != NO_MATCH {
        return visible;
    }
    alias_match(choice, needle).saturating_add(4)
}

/// JavaScript string order: UTF-16 code units.
fn compare_utf16(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Presence {
    Online,
    Away,
    Unknown,
}

struct Ranked {
    index: usize,
    tier: [u32; 5],
    label: String,
    block: String,
    block_label: String,
    recent: u64,
    online: u8,
}

/// The matching choices in mention order, as indexes into `choices`.
pub fn rank(
    choices: &[Choice],
    query: &str,
    history: &HashMap<String, u64>,
    presence: impl Fn(&str) -> Presence,
) -> Vec<usize> {
    let needle = normalize_name(query).chars().collect::<Vec<_>>();
    let key = typed_public_key(query);
    let mut ranked = Vec::new();
    for (index, choice) in choices.iter().enumerate() {
        let matched = if needle.is_empty() {
            0
        } else {
            label_match(choice, &needle, key.as_deref())
        };
        if matched == NO_MATCH {
            continue;
        }
        let tier = [
            u32::from(!choice.member),
            u32::from(choice.elsewhere.is_some()),
            matched,
            alias_match(choice, &needle),
            u32::from(!(choice.agent && choice.owned)),
        ];
        let block = if choice.agent {
            let tiers = tier
                .iter()
                .map(|value| {
                    if *value == NO_MATCH {
                        "null".to_owned()
                    } else {
                        value.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(",");
            let name = serde_json::to_string(&normalize_name(&choice.name)).unwrap_or_default();
            format!("1[[{tiers}],{name}]")
        } else {
            format!("0{}", choice.pubkey)
        };
        ranked.push(Ranked {
            index,
            tier,
            label: normalize_name(&choice.label),
            block,
            block_label: String::new(),
            recent: history.get(&choice.pubkey).copied().unwrap_or(0),
            online: match presence(&choice.pubkey) {
                Presence::Online => 0,
                Presence::Away => 1,
                Presence::Unknown => 2,
            },
        });
    }
    let mut block_labels: HashMap<String, String> = HashMap::new();
    for entry in &ranked {
        match block_labels.get(&entry.block) {
            Some(label) if compare_utf16(&entry.label, label) != Ordering::Less => {}
            _ => {
                block_labels.insert(entry.block.clone(), entry.label.clone());
            }
        }
    }
    for entry in &mut ranked {
        entry.block_label = block_labels.get(&entry.block).cloned().unwrap_or_default();
    }
    ranked.sort_by(|a, b| {
        a.tier
            .cmp(&b.tier)
            .then_with(|| compare_utf16(&a.block_label, &b.block_label))
            .then_with(|| compare_utf16(&a.block, &b.block))
            .then_with(|| b.recent.cmp(&a.recent))
            .then_with(|| choices[b.index].managed.cmp(&choices[a.index].managed))
            .then_with(|| a.online.cmp(&b.online))
            .then_with(|| compare_utf16(&a.label, &b.label))
            .then_with(|| compare_utf16(&choices[a.index].pubkey, &choices[b.index].pubkey))
    });
    ranked.into_iter().map(|entry| entry.index).collect()
}

// ---------------------------------------------------------------------------
// Space and closing punctuation (section 5)
// ---------------------------------------------------------------------------

/// Names that continue the query, so no exact name can be selected yet.
pub fn continuing_names(choices: &[Choice], query: &str) -> Vec<String> {
    if typed_public_key(query).is_some() {
        return Vec::new();
    }
    let needle = normalize_name(query);
    if needle.is_empty() {
        return Vec::new();
    }
    let mut names: Vec<(String, String)> = Vec::new();
    for choice in choices {
        let own = choice
            .aliases
            .iter()
            .chain(std::iter::once(&choice.label))
            .any(|name| normalize_name(name) == needle);
        if own {
            continue;
        }
        let label = match &choice.qualifier {
            Some(qualifier) if choice.label.ends_with(&format!(" \u{00B7} {qualifier}")) => {
                &choice.label[..choice.label.len() - qualifier.len() - " \u{00B7} ".len()]
            }
            _ => choice.label.as_str(),
        };
        let generated_agent = choice.agent
            && choice
                .aliases
                .iter()
                .any(|name| format!("{} (agent)", js_trim(name)) == label);
        let mut longer = choice
            .aliases
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        if !generated_agent {
            longer.push(label);
        }
        for name in longer {
            let normalized = normalize_name(name);
            if extends_name(name, &needle) && !names.iter().any(|(key, _)| *key == normalized) {
                names.push((normalized, js_trim(name).to_owned()));
            }
        }
    }
    names.into_iter().map(|(_, name)| name).collect()
}

/// The key Space selects for `query` among `choices`: the one exact name
/// (or exact public key) that `among` allows, when no known name continues
/// it. `None` means Space is an ordinary space.
pub fn exact_mention(
    choices: &[Choice],
    query: &str,
    among: impl Fn(&Choice) -> bool,
) -> Option<String> {
    if let Some(key) = typed_public_key(query) {
        return choices
            .iter()
            .find(|choice| choice.pubkey == key && among(choice))
            .map(|choice| choice.pubkey.clone());
    }
    if !continuing_names(choices, query).is_empty() {
        return None;
    }
    let needle = normalize_name(query);
    if needle.is_empty() {
        return None;
    }
    let exact = choices
        .iter()
        .filter(|choice| {
            !choice.aliases.is_empty()
                && among(choice)
                && choice
                    .aliases
                    .iter()
                    .chain(std::iter::once(&choice.label))
                    .any(|name| normalize_name(name) == needle)
        })
        .collect::<Vec<_>>();
    match exact.as_slice() {
        [one] => Some(one.pubkey.clone()),
        _ => None,
    }
}

/// Characters that select like Space when typed straight after a query.
pub fn is_closing_punctuation(c: char) -> bool {
    matches!(
        c,
        ',' | '.' | '!' | '?' | ';' | ':' | ')' | '\'' | '\u{2019}' | '"' | '\u{201D}'
    )
}

// ---------------------------------------------------------------------------
// Query syntax and admission (section 2)
// ---------------------------------------------------------------------------

/// An active mention query: `start` is the `@`, `end` the caret (bytes).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Query {
    pub start: usize,
    pub end: usize,
    pub query: String,
}

fn opens_query(c: char) -> bool {
    is_js_space(c) || matches!(c, '(' | '[' | '{')
}

/// The mention being typed at byte `caret` of `text`, if any.
pub fn mention_query(text: &str, caret: usize) -> Option<Query> {
    if caret > text.len() || !text.is_char_boundary(caret) {
        return None;
    }
    let before = &text[..caret];
    // Look back at most 160 characters.
    let window_start = before
        .char_indices()
        .rev()
        .nth(159)
        .map_or(0, |(index, _)| index);
    let window = &before[window_start..];
    let at = window.rfind('@')?;
    let query = &window[at + 1..];
    if query.contains(['@', '\n', '\r', '\t']) {
        return None;
    }
    if at > 0 && !window[..at].chars().next_back().is_some_and(opens_query) {
        return None;
    }
    let start = window_start + at;
    // A completed `[@name](nostr:npub1…)` link is markup, not a search.
    if let Some(open) = query.rfind("](nostr:npub1")
        && query.ends_with(')')
        && start > 0
        && text[..start].ends_with('[')
    {
        let npub = &query[open + 8..query.len() - 1];
        if typed_public_key(npub).is_some() {
            return None;
        }
    }
    if start > 0 && !text[..start].chars().next_back().is_some_and(opens_query) {
        return None;
    }
    Some(Query {
        start,
        end: caret,
        query: query.to_owned(),
    })
}

/// A query with a space is admitted only while it is still the start of a
/// known name, and not a complete name followed by a space.
pub fn admitted(query: &str, names: &[String]) -> bool {
    if !query.contains(' ') {
        return true;
    }
    let lower = fold_name(query);
    if lower.ends_with(' ')
        && names
            .iter()
            .any(|name| normalize_name(name) == lower.trim_end_matches(is_js_space))
    {
        return false;
    }
    names
        .iter()
        .any(|name| normalize_name(name).starts_with(&lower))
}

// ---------------------------------------------------------------------------
// Tags (section 8)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TagError {
    NotMember,
    Invalid,
    TooMany,
}

impl TagError {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotMember => "not_member",
            Self::Invalid => "invalid",
            Self::TooMany => "too_many",
        }
    }
}

pub const MAX_KEYS_PER_LIST: usize = 32;

fn valid_key(key: &str) -> bool {
    key.len() == 64
        && key
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn unique(keys: &[String]) -> Vec<&str> {
    let mut seen = HashSet::new();
    keys.iter()
        .map(String::as_str)
        .filter(|key| seen.insert(*key))
        .collect()
}

/// The `p` (recipient) and `mention` (reference) tags for a message.
/// Recipients must be members (`sender` counts as one). A bad key rejects
/// the message instead of being dropped.
pub fn write_tags(
    sender: &str,
    members: &[String],
    recipients: &[String],
    references: &[String],
) -> Result<Vec<Vec<String>>, TagError> {
    if recipients
        .iter()
        .chain(references)
        .any(|key| !valid_key(key))
    {
        return Err(TagError::Invalid);
    }
    let recipients = unique(recipients);
    let references = unique(references);
    if recipients.len() > MAX_KEYS_PER_LIST || references.len() > MAX_KEYS_PER_LIST {
        return Err(TagError::TooMany);
    }
    if recipients
        .iter()
        .any(|key| *key != sender && !members.iter().any(|member| member == key))
    {
        return Err(TagError::NotMember);
    }
    Ok(recipients
        .into_iter()
        .map(|key| vec!["p".to_owned(), key.to_owned()])
        .chain(
            references
                .into_iter()
                .map(|key| vec!["mention".to_owned(), key.to_owned()]),
        )
        .collect())
}

/// Addressed mentions (`p`) and references (two-field `mention`), valid and
/// deduplicated in first-seen order. A key in both lists is addressed.
pub fn read_tags(tags: &[Vec<String>]) -> (Vec<String>, Vec<String>) {
    let mut mentions = Vec::new();
    let mut references = Vec::new();
    for tag in tags {
        match tag.as_slice() {
            [name, key] if name == "mention" && valid_key(key) && !references.contains(key) => {
                references.push(key.clone());
            }
            [name, key, ..] if name == "p" && valid_key(key) && !mentions.contains(key) => {
                mentions.push(key.clone());
            }
            _ => {}
        }
    }
    (mentions, references)
}
