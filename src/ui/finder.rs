use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

use crate::domain::Channel;

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

    use super::rank;
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
}
