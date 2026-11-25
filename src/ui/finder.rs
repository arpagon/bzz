use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

use crate::domain::Channel;

/// What the finder holds while it is open: the query and the selected row
/// (v0.17 C5). The rows are [`rows`] of the query.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FinderState {
    pub query: String,
    pub selected: usize,
}

impl FinderState {
    pub fn clear(&mut self) {
        self.query.clear();
        self.selected = 0;
    }

    /// A new query starts again at the best match.
    pub fn push(&mut self, character: char) {
        if !character.is_control() && self.query.chars().count() < 80 {
            self.query.push(character);
            self.selected = 0;
        }
    }

    pub fn pop(&mut self) {
        self.query.pop();
        self.selected = 0;
    }

    /// Move through `len` rows, wrapping at both ends.
    pub fn move_by(&mut self, delta: isize, len: usize) {
        if len == 0 {
            self.selected = 0;
            return;
        }
        let steps = delta.unsigned_abs() % len;
        let current = self.selected.min(len - 1);
        self.selected = if delta.is_negative() {
            (current + len - steps) % len
        } else {
            (current + steps) % len
        };
    }

    /// The first row of a window of `height` rows that shows the selection.
    pub fn window_start(&self, height: usize) -> usize {
        self.selected.saturating_sub(height.saturating_sub(1))
    }
}

/// One row of the finder: a channel or DM to open, or, last, the way to
/// start a DM with someone who has none yet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinderRow<'a> {
    Channel(&'a Channel),
    NewDm,
}

/// The finder's rows for a query: channels and DMs, best first, then the row
/// that starts a DM. A DM is found by the names of the people in it.
pub fn rows<'a>(query: &str, channels: &'a [Channel]) -> Vec<FinderRow<'a>> {
    let mut rows = rank(query, channels)
        .into_iter()
        .map(FinderRow::Channel)
        .collect::<Vec<_>>();
    rows.push(FinderRow::NewDm);
    rows
}

/// The mark before a row's name: `@` a DM, `#` a channel you are in, `+` one
/// you may join.
pub fn marker(channel: &Channel) -> &'static str {
    if channel.kind.is_dm() {
        "@"
    } else if channel.is_member {
        "#"
    } else {
        "+"
    }
}

/// Channels the viewer may browse, best match first. Unlisted channels the
/// viewer has not joined are named-only (NIP-CL) and never listed here.
pub fn rank<'a>(query: &str, channels: &'a [Channel]) -> Vec<&'a Channel> {
    if query.is_empty() {
        let mut values = channels
            .iter()
            .filter(|channel| channel.listed_for_viewer())
            .collect::<Vec<_>>();
        values.sort_by_key(|channel| {
            (
                !channel.is_member,
                std::cmp::Reverse(channel.last_event_at.unwrap_or_default()),
                channel.name.to_lowercase(),
            )
        });
        return values;
    }
    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    let mut values = channels
        .iter()
        .filter(|channel| channel.listed_for_viewer())
        .filter_map(|channel| {
            let mut buffer = Vec::new();
            pattern
                .score(
                    Utf32Str::new(channel.name.as_str(), &mut buffer),
                    &mut matcher,
                )
                .map(|score| (score, channel))
        })
        .collect::<Vec<_>>();
    values.sort_by_key(|(score, channel)| {
        (
            std::cmp::Reverse(*score),
            !channel.is_member,
            std::cmp::Reverse(channel.last_event_at.unwrap_or_default()),
            channel.name.to_lowercase(),
        )
    });
    values.into_iter().map(|(_, channel)| channel).collect()
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{FinderRow, FinderState, marker, rank, rows};
    use crate::domain::{Channel, ChannelFacts, ChannelKind, Visibility};

    fn channel(name: &str, is_member: bool, unlisted: bool) -> Channel {
        Channel {
            id: Uuid::new_v4(),
            name: name.into(),
            about: String::new(),
            kind: ChannelKind::Stream,
            visibility: Visibility::Public,
            is_member,
            is_hidden: false,
            member_count: 0,
            last_event_at: None,
            facts: ChannelFacts {
                unlisted,
                ..ChannelFacts::default()
            },
        }
    }

    #[test]
    fn unlisted_channels_are_listed_only_for_members() {
        let channels = vec![
            channel("listed-open", false, false),
            channel("unlisted-open", false, true),
            channel("unlisted-joined", true, true),
        ];
        for query in ["", "listed", "unlisted"] {
            let names = rank(query, &channels)
                .into_iter()
                .map(|channel| channel.name.as_str())
                .collect::<Vec<_>>();
            assert!(!names.contains(&"unlisted-open"), "{query:?}: {names:?}");
        }
        let names = rank("unlisted", &channels)
            .into_iter()
            .map(|channel| channel.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["unlisted-joined"]);
    }

    /// v0.17 C5: the selection moves and wraps; a new query starts again at
    /// the top; the window keeps the selection in view.
    #[test]
    fn the_selection_moves_wraps_and_restarts_with_the_query() {
        let mut state = FinderState::default();
        state.move_by(1, 3);
        assert_eq!(state.selected, 1);
        state.move_by(1, 3);
        state.move_by(1, 3);
        assert_eq!(state.selected, 0);
        state.move_by(-1, 3);
        assert_eq!(state.selected, 2);
        assert_eq!(state.window_start(2), 1);
        state.push('g');
        assert_eq!((state.query.as_str(), state.selected), ("g", 0));
        state.move_by(1, 0);
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn dms_are_found_by_name_and_a_new_dm_comes_last() {
        let mut dm = channel("Scout", true, false);
        dm.kind = ChannelKind::Dm;
        let channels = vec![channel("general", true, false), dm];
        let found = rows("sco", &channels);
        assert!(matches!(found[0], FinderRow::Channel(found) if found.name == "Scout"));
        assert_eq!(found.last(), Some(&FinderRow::NewDm));
        assert_eq!(marker(&channels[1]), "@");
        assert_eq!(marker(&channels[0]), "#");
        assert_eq!(marker(&channel("open", false, false)), "+");
    }
}
