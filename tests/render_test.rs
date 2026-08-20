use bzz::render::{markdown, sanitize};
use proptest::prop_assert;
use unicode_width::UnicodeWidthStr;

#[test]
fn terminal_controls_and_bidi_overrides_are_never_preserved() {
    let hostile = "hello\x1b]52;c;secret\x07\u{202e}txt\u{2066}";
    let safe = sanitize::text(hostile);
    assert!(!safe.contains('\x1b'));
    assert!(!safe.contains('\x07'));
    assert!(!safe.contains('\u{202e}'));
    assert!(!safe.contains('\u{2066}'));
}

fn rendered_text(input: &str) -> String {
    markdown::render(input, &bzz::ui::theme::Theme::default())
        .lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect()
}

#[test]
fn links_are_inert_visible_text() {
    let rendered = rendered_text("[click](https://example.test/x)");
    assert!(rendered.contains("click"));
    assert!(rendered.contains("https://example.test/x"));
}

#[test]
fn practical_markdown_has_safe_block_markers() {
    let rendered = rendered_text(
        "# Heading\n\n> quote\n\n1. first\n2. second\n\n```rust\nlet x = 1;\n```\n\n| a | b |\n| - | - |\n| one | two |",
    );
    for expected in [
        "# Heading",
        "│ quote",
        "1. first",
        "code · rust",
        "let x = 1;",
        "one",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?} in {rendered:?}"
        );
    }
    assert!(!rendered.contains('\x1b'));
}

#[test]
fn table_cells_are_bounded() {
    let rendered = rendered_text(&format!("| column |\n| - |\n| {} |", "x".repeat(100)));
    assert!(rendered.contains('…'));
    assert!(!rendered.contains(&"x".repeat(100)));
}

#[test]
fn measured_tables_use_complete_bounded_unicode_grids() {
    let rendered = markdown::render_with_width(
        "| Customer | Location | Score |\n| - | - | - |\n| MilkyMoo | GrandPlaza | 100 |",
        &bzz::ui::theme::Theme::default(),
        30,
    );
    let lines = rendered
        .lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert!(lines.iter().any(|line| line.starts_with('┌')));
    assert!(lines.iter().any(|line| line.starts_with('├')));
    assert!(lines.iter().any(|line| line.starts_with('└')));
    assert!(
        lines
            .iter()
            .all(|line| UnicodeWidthStr::width(line.as_str()) <= 30)
    );
}

#[test]
fn very_wide_tables_become_labelled_records_without_losing_cells() {
    let rendered = rendered_text(
        "| Score | Customer | Location | Linear | Tnl | Audio | Healthy | POS | Video | Historical pipeline | Profiles |\n| - | - | - | - | - | - | - | - | - | - | - |\n| 100 | MilkyMoo | GrandPlaza | EMI-15 | 1/1 | 7/7 | yes | 7/7 | 6/7 | 33d / 308 sessions | 3 |",
    );
    assert!(rendered.contains("table · 1 rows × 11 columns"));
    assert!(rendered.contains("Customer: MilkyMoo"));
    assert!(rendered.contains("Healthy: yes"));
}

proptest::proptest! {#[test]fn sanitizer_never_emits_escape(input in ".*"){let safe=sanitize::text(&input);prop_assert!(!safe.contains('\x1b'));}}
