use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};

pub const SPINNER_FRAME_COUNT: usize = 10;
const SPINNER_FRAMES: [char; SPINNER_FRAME_COUNT] =
    ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Formats the compact, scope-local agent activity shown in the status bar.
///
/// The signal remains typing-specific even though the presentation is small:
/// the animated cell replaces prose, not protocol meaning. A leading diamond
/// identifies a verified remote agent and additional active agents collapse to
/// a bounded count.
pub fn format_typing_indicator(labels: &[String], frame: usize, width: u16) -> Option<String> {
    if labels.is_empty() || width < 5 {
        return None;
    }
    let spinner = SPINNER_FRAMES[frame % SPINNER_FRAME_COUNT];
    let count = (labels.len() > 1).then(|| format!(" +{}", labels.len() - 1));
    let count = count.as_deref().unwrap_or_default();
    let full = format!("◆ {}{count} {spinner}", labels[0]);
    let width = usize::from(width);
    if full.as_str().width() <= width {
        return Some(full);
    }

    if labels.len() > 1 {
        let compact = format!("◆ {} {spinner}", labels.len());
        if compact.as_str().width() <= width {
            return Some(compact);
        }
    }

    let prefix = "◆ ";
    let suffix = format!("{count} {spinner}");
    let fixed = prefix.width().saturating_add(suffix.as_str().width());
    if width <= fixed {
        return None;
    }
    let label = truncate_cells(&labels[0], width - fixed);
    Some(format!("{prefix}{label}{suffix}"))
}

pub fn truncate_cells(value: &str, width: usize) -> String {
    if value.width() <= width {
        return value.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    if width == 1 {
        return "…".into();
    }
    let mut output = String::new();
    let mut used = 0_usize;
    for character in value.chars() {
        let cells = character.width().unwrap_or(0);
        if used.saturating_add(cells).saturating_add(1) > width {
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
    use super::{SPINNER_FRAME_COUNT, format_typing_indicator};

    #[test]
    fn formats_one_agent_as_a_compact_animated_indicator() {
        assert_eq!(
            format_typing_indicator(&["Fizz".into()], 2, 80).as_deref(),
            Some("◆ Fizz ⠹")
        );
        assert_eq!(
            format_typing_indicator(&["Fizz".into()], SPINNER_FRAME_COUNT + 2, 80).as_deref(),
            Some("◆ Fizz ⠹")
        );
    }

    #[test]
    fn collapses_multiple_agents_to_a_bounded_count() {
        assert_eq!(
            format_typing_indicator(&["Fizz".into(), "Honey".into(), "Bumble".into()], 0, 80)
                .as_deref(),
            Some("◆ Fizz +2 ⠋")
        );
        assert_eq!(
            format_typing_indicator(&["Fizz".into(), "Honey".into(), "Bumble".into()], 0, 6)
                .as_deref(),
            Some("◆ 3 ⠋")
        );
    }

    #[test]
    fn narrow_width_truncates_by_terminal_cells() {
        let indicator = format_typing_indicator(&["蜜蜂エージェント".into()], 1, 12).unwrap();
        assert!(unicode_width::UnicodeWidthStr::width(indicator.as_str()) <= 12);
        assert!(indicator.starts_with("◆ "));
        assert!(indicator.ends_with(" ⠙"));
        assert!(format_typing_indicator(&["Fizz".into()], 0, 4).is_none());
    }
}
