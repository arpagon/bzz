use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

use crate::domain::Channel;

pub fn rank<'a>(query: &str, channels: &'a [Channel]) -> Vec<&'a Channel> {
    if query.is_empty() {
        let mut values = channels.iter().collect::<Vec<_>>();
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
