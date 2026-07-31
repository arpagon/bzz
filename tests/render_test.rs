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

#[test]
fn links_are_inert_visible_text() {
    let text = markdown::render("[click](https://example.test/x)");
    let rendered = text
        .lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(rendered.contains("click"));
    assert!(rendered.contains("https://example.test/x"));
}

proptest::proptest! {#[test]fn sanitizer_never_emits_escape(input in ".*"){let safe=sanitize::text(&input);prop_assert!(!safe.contains('\x1b'));}}
