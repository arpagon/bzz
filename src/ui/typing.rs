use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};

pub fn format_typing_line(labels: &[String], width: u16) -> Option<String> {
    if labels.is_empty() || width < 12 {
        return None;
    }
    let full = match labels {
        [one] => format!("◆ {one} is typing…"),
        [one, two] => format!("◆ {one} and ◆ {two} are typing…"),
        [one, two, rest @ ..] => {
            format!("◆ {one}, ◆ {two}, and {} others are typing…", rest.len())
        }
        [] => return None,
    };
    let width = usize::from(width);
    if full.as_str().width() <= width {
        return Some(full);
    }
    if labels.len() > 1 {
        let compact = format!("{} verified agents are typing…", labels.len());
        if compact.as_str().width() <= width {
            return Some(compact);
        }
        return Some(truncate_cells(&compact, width));
    }
    let suffix = " is typing…";
    let prefix = "◆ ";
    let fixed = prefix.width() + suffix.width();
    if width <= fixed {
        return Some(truncate_cells("agent typing…", width));
    }
    let label = truncate_cells(&labels[0], width - fixed);
    Some(format!("{prefix}{label}{suffix}"))
}

fn truncate_cells(value: &str, width: usize) -> String {
    if value.width() <= width {
        return value.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let ellipsis_width = 1;
    if width <= ellipsis_width {
        return "…".into();
    }
    let mut output = String::new();
    let mut used = 0_usize;
    for character in value.chars() {
        let cells = character.width().unwrap_or(0);
        if used.saturating_add(cells).saturating_add(ellipsis_width) > width {
            break;
        }
        output.push(character);
        used = used.saturating_add(cells);
    }
    output.push('…');
    output
}

#[cfg(test)]
mod tests {
    use super::format_typing_line;

    #[test]
    fn formats_one_and_many_verified_agents() {
        assert_eq!(
            format_typing_line(&["Fizz".into()], 80).as_deref(),
            Some("◆ Fizz is typing…")
        );
        assert_eq!(
            format_typing_line(&["Fizz".into(), "Honey".into()], 80).as_deref(),
            Some("◆ Fizz and ◆ Honey are typing…")
        );
        assert_eq!(
            format_typing_line(
                &["Fizz".into(), "Honey".into(), "Bumble".into(), "Bee".into()],
                80
            )
            .as_deref(),
            Some("◆ Fizz, ◆ Honey, and 2 others are typing…")
        );
    }

    #[test]
    fn narrow_width_collapses_many_and_preserves_single_meaning() {
        assert_eq!(
            format_typing_line(&["Fizz".into(), "Honey".into()], 29).as_deref(),
            Some("2 verified agents are typing…")
        );
        let single = format_typing_line(&["A very long agent name".into()], 18).unwrap();
        assert!(single.starts_with("◆ "));
        assert!(single.ends_with(" is typing…"));
        assert!(unicode_width::UnicodeWidthStr::width(single.as_str()) <= 18);
        assert!(format_typing_line(&["Fizz".into()], 10).is_none());
    }

    #[test]
    fn unicode_labels_are_truncated_by_cells() {
        let line = format_typing_line(&["蜜蜂エージェント".into()], 20).unwrap();
        assert!(unicode_width::UnicodeWidthStr::width(line.as_str()) <= 20);
        assert!(line.ends_with(" is typing…"));
    }
}
