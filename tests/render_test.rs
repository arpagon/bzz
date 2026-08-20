use bzz::render::{markdown, sanitize};
use proptest::prop_assert;

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

proptest::proptest! {#[test]fn sanitizer_never_emits_escape(input in ".*"){let safe=sanitize::text(&input);prop_assert!(!safe.contains('\x1b'));}}
