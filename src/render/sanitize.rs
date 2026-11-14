pub fn text(input: &str) -> String {
    input
        .chars()
        .map(|character| if safe(character) { character } else { '�' })
        .collect()
}

fn safe(character: char) -> bool {
    match character {
        '\n' | '\t' => true,
        '\u{0000}'..='\u{001f}'
        | '\u{007f}'..='\u{009f}'
        | '\u{202a}'..='\u{202e}'
        | '\u{2066}'..='\u{2069}'
        | '\u{feff}' => false,
        _ => true,
    }
}

pub fn single_line(input: &str) -> String {
    text(input).replace(['\n', '\t'], " ")
}

/// Whether a character shows on a terminal as itself: not a control
/// character, not one that is invisible or changes the order of what is
/// around it, and not a space that is no plain space.
fn shows_as_itself(character: char) -> bool {
    !(character.is_control()
        || matches!(
            character,
            '\u{00a0}'
                | '\u{00ad}'
                | '\u{034f}'
                | '\u{061c}'
                | '\u{115f}'
                | '\u{1160}'
                | '\u{1680}'
                | '\u{17b4}'
                | '\u{17b5}'
                | '\u{180b}'..='\u{180f}'
                | '\u{2000}'..='\u{200f}'
                | '\u{2028}'..='\u{202f}'
                | '\u{205f}'..='\u{206f}'
                | '\u{3000}'
                | '\u{3164}'
                | '\u{fe00}'..='\u{fe0f}'
                | '\u{feff}'
                | '\u{ffa0}'
                | '\u{fff9}'..='\u{fffb}'
                | '\u{1d173}'..='\u{1d17a}'
                | '\u{e0000}'..='\u{e0fff}'
        ))
}

/// Text that someone must read before agreeing to it, such as a command an
/// agent wants to run: in pieces, each either text that shows as itself or
/// (`true`) the escape of one character that would not, such as `\x1b`,
/// `\t` or `\u{202e}`. Nothing is left out and nothing is replaced by a
/// blank: what cannot be seen is written out.
pub fn visible_parts(input: &str) -> Vec<(String, bool)> {
    let mut parts: Vec<(String, bool)> = Vec::new();
    for character in input.chars() {
        if shows_as_itself(character) {
            match parts.last_mut() {
                Some((text, false)) => text.push(character),
                _ => parts.push((character.to_string(), false)),
            }
            continue;
        }
        let escape = match character {
            '\n' => "\\n".to_owned(),
            '\r' => "\\r".to_owned(),
            '\t' => "\\t".to_owned(),
            '\u{0}'..='\u{7f}' => format!("\\x{:02x}", character as u32),
            other => format!("\\u{{{:x}}}", other as u32),
        };
        parts.push((escape, true));
    }
    parts
}

/// [`visible_parts`] as one text.
pub fn visible(input: &str) -> String {
    visible_parts(input)
        .into_iter()
        .map(|(text, _)| text)
        .collect()
}

#[cfg(test)]
mod visible_tests {
    use super::*;

    #[test]
    fn what_would_not_show_is_written_out() {
        assert_eq!(
            visible("npm test -- --watch=false"),
            "npm test -- --watch=false"
        );
        assert_eq!(
            visible("café ünï 日本"),
            "café ünï 日本",
            "text that shows is left alone"
        );
        // A terminal escape that would repaint the line, a carriage return
        // that would overwrite it, and a tab.
        assert_eq!(
            visible("echo ok\u{1b}[2K\rrm -rf ~\t#"),
            "echo ok\\x1b[2K\\rrm -rf ~\\t#"
        );
        assert_eq!(visible("a\nb\u{0}c\u{7f}"), "a\\nb\\x00c\\x7f");
        // Characters that reorder or hide: right-to-left override, zero
        // width space, a no-break space, a byte order mark, a C1 control.
        assert_eq!(
            visible("a\u{202e}b\u{200b}c\u{a0}d\u{feff}e\u{85}f"),
            "a\\u{202e}b\\u{200b}c\\u{a0}d\\u{feff}e\\u{85}f"
        );
        let parts = visible_parts("ls\u{1b}\u{1b} -la");
        assert_eq!(
            parts,
            [
                ("ls".to_owned(), false),
                ("\\x1b".to_owned(), true),
                ("\\x1b".to_owned(), true),
                (" -la".to_owned(), false),
            ]
        );
        assert!(visible_parts("").is_empty());
        // Nothing of the input is lost: every character is in the output,
        // as itself or as its code.
        for sample in ["", "plain", "\u{1b}\u{202e}\u{e0041}x"] {
            let shown = visible(sample);
            assert!(shown.chars().all(shows_as_itself), "{shown:?}");
            assert!(shown.len() >= sample.chars().count());
        }
    }
}
