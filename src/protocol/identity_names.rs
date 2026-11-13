//! Contextual identity names, version 1: distinct readable labels for the
//! identities in one view, as specified by `block/buzz-app`
//! `src/bundled/identity-naming/README.md` (vendored fixtures in
//! `tests/fixtures/buzz-app/`). Labels are presentation only: keys, mentions
//! and tags never change, and an owner prefix grants nothing.

use std::collections::{HashMap, HashSet};

use nostr::{PublicKey, nips::nip19::ToBech32 as _};

/// One naming fact: a key, the name to show for it, and display hints.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Fact {
    pub pubkey: String,
    pub name: String,
    pub is_agent: bool,
    pub owner_pubkey: Option<String>,
}

/// A key's resolved label. `qualifier` is the generated key suffix, if any.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Resolved {
    pub name: String,
    pub qualifier: Option<String>,
}

/// ECMAScript `String.prototype.trim` white space.
pub fn is_js_space(c: char) -> bool {
    matches!(c,
        '\u{0009}'..='\u{000D}' | '\u{0020}' | '\u{00A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}'
        | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}')
}

pub fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_space)
}

struct Row {
    key: String,
    name: String,
    is_agent: bool,
    owner: Option<String>,
    base: String,
    label: String,
    priority: u8,
    mine: bool,
    length: usize,
    suffix: Option<String>,
}

/// Resolve labels for `candidates` (all facts when `None`). Input order
/// matters: the last fact for a key is its preferred alias.
pub fn resolve(
    facts: &[Fact],
    viewer: Option<&str>,
    candidates: Option<&[String]>,
) -> HashMap<String, Resolved> {
    let viewer = viewer.map(str::to_ascii_lowercase);
    let lower = |key: &str| key.to_ascii_lowercase();
    let selected: Option<HashSet<String>> =
        candidates.map(|keys| keys.iter().map(|key| lower(key)).collect());
    // Outside a selection, rows only supply the selected agents' owners' names.
    let scoped: Vec<&Fact> = match &selected {
        Some(selected) => {
            let mut keys = selected.clone();
            for fact in facts {
                if selected.contains(&lower(&fact.pubkey))
                    && let Some(owner) = &fact.owner_pubkey
                {
                    keys.insert(lower(owner));
                }
            }
            facts
                .iter()
                .filter(|fact| keys.contains(&lower(&fact.pubkey)))
                .collect()
        }
        None => facts.iter().collect(),
    };
    // The preferred (last) fact per key.
    let mut preferred: HashMap<String, &Fact> = HashMap::new();
    for fact in &scoped {
        preferred.insert(lower(&fact.pubkey), fact);
    }
    // One row per (key, trimmed name); the last such fact supplies metadata.
    let mut order: Vec<(String, String)> = Vec::new();
    let mut aliases: HashMap<(String, String), &Fact> = HashMap::new();
    for fact in &scoped {
        let id = (lower(&fact.pubkey), js_trim(&fact.name).to_owned());
        if !aliases.contains_key(&id) {
            order.push(id.clone());
        }
        aliases.insert(id, fact);
    }
    let mut rows: Vec<Row> = order
        .into_iter()
        .filter(|(key, _)| {
            selected
                .as_ref()
                .is_none_or(|selected| selected.contains(key))
        })
        .map(|id| {
            let fact = aliases[&id];
            let (key, name) = id;
            let owner = fact.owner_pubkey.as_deref().map(lower);
            let mine = viewer
                .as_ref()
                .is_some_and(|viewer| *viewer == key || owner.as_ref() == Some(viewer));
            let priority = match (fact.is_agent, mine) {
                (true, true) => 2,
                (true, false) => 3,
                (false, _) if viewer.as_ref() == Some(&key) => 0,
                (false, _) => 1,
            };
            Row {
                key,
                base: name.clone(),
                label: name.clone(),
                name,
                is_agent: fact.is_agent,
                owner,
                priority,
                mine,
                length: 0,
                suffix: None,
            }
        })
        .collect();
    loop {
        // Groups by label, in first-seen order.
        let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            match groups.iter_mut().find(|(label, _)| *label == row.label) {
                Some((_, members)) => members.push(index),
                None => groups.push((row.label.clone(), vec![index])),
            }
        }
        let collisions = groups
            .into_iter()
            .map(|(_, members)| members)
            .filter(|members| {
                members
                    .iter()
                    .map(|index| &rows[*index].key)
                    .collect::<HashSet<_>>()
                    .len()
                    > 1
            })
            .collect::<Vec<_>>();
        if collisions.is_empty() {
            break;
        }
        for group in collisions {
            let best = group
                .iter()
                .map(|index| rows[*index].priority)
                .min()
                .unwrap_or(0);
            let winners = group
                .iter()
                .filter(|index| rows[**index].priority == best)
                .map(|index| rows[*index].key.clone())
                .collect::<HashSet<_>>();
            let changing = group
                .iter()
                .copied()
                .filter(|index| winners.len() != 1 || !winners.contains(&rows[*index].key))
                .collect::<Vec<_>>();
            let any_human = group.iter().any(|index| !rows[*index].is_agent);
            let mut qualified = false;
            for &index in &changing {
                let row = &rows[index];
                let owner_name = row
                    .owner
                    .as_ref()
                    .and_then(|owner| preferred.get(owner))
                    .map(|fact| js_trim(&fact.name).to_owned())
                    .filter(|name| !name.is_empty());
                let readable = if row.is_agent && row.length == 0 {
                    match owner_name {
                        Some(owner) if !row.mine => format!("{owner}\u{2019}s {}", row.name),
                        _ if any_human => format!("{} (agent)", row.name),
                        _ => row.base.clone(),
                    }
                } else {
                    row.base.clone()
                };
                if readable != row.base {
                    let row = &mut rows[index];
                    row.base = readable.clone();
                    row.label = readable;
                    qualified = true;
                }
            }
            if qualified {
                continue;
            }
            for index in changing {
                let row = &mut rows[index];
                row.length = if row.length == 0 { 4 } else { row.length + 1 };
                let npub = npub(&row.key);
                let length = npub.len();
                let suffix = if row.length <= length {
                    npub[length - row.length..].to_owned()
                } else {
                    format!("{npub} \u{00B7} {}", row.length - length)
                };
                row.label = format!("{} \u{00B7} {suffix}", row.base);
                row.suffix = Some(suffix);
            }
        }
    }
    rows.into_iter()
        .filter(|row| {
            preferred
                .get(&row.key)
                .is_some_and(|fact| js_trim(&fact.name) == row.name)
        })
        .map(|row| {
            (
                row.key,
                Resolved {
                    name: row.label,
                    qualifier: row.suffix,
                },
            )
        })
        .collect()
}

fn npub(key: &str) -> String {
    PublicKey::from_hex(key)
        .ok()
        .and_then(|key| key.to_bech32().ok())
        .unwrap_or_else(|| key.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact(key: char, name: &str, agent: bool, owner: Option<char>) -> Fact {
        Fact {
            pubkey: key.to_string().repeat(64),
            name: name.into(),
            is_agent: agent,
            owner_pubkey: owner.map(|owner| owner.to_string().repeat(64)),
        }
    }

    #[test]
    fn human_mine_and_others_namesakes_are_told_apart() {
        let facts = [
            fact('1', "Logan", false, None),
            fact('2', "Wes", false, None),
            fact('a', "Honey", false, None),
            fact('b', "Honey", true, Some('1')),
            fact('c', "Honey", true, Some('2')),
        ];
        let viewer = "1".repeat(64);
        let keys = ["a", "b", "c"].map(|key| key.repeat(64));
        let labels = resolve(&facts, Some(&viewer), Some(&keys));
        assert_eq!(labels[&keys[0]].name, "Honey");
        assert_eq!(labels[&keys[1]].name, "Honey (agent)");
        assert_eq!(labels[&keys[2]].name, "Wes\u{2019}s Honey");
        assert!(labels.values().all(|label| label.qualifier.is_none()));
    }
}
