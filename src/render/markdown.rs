use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span, Text},
};

use crate::render::sanitize;

pub fn render(input: &str) -> Text<'static> {
    let safe = sanitize::text(input);
    let parser = Parser::new_ext(
        &safe,
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS,
    );
    let mut lines = vec![Line::default()];
    let mut style = Style::default();
    let mut links = Vec::<String>::new();
    for event in parser {
        match event {
            Event::Start(Tag::Strong) => style = style.add_modifier(Modifier::BOLD),
            Event::End(TagEnd::Strong) => style = style.remove_modifier(Modifier::BOLD),
            Event::Start(Tag::Emphasis) => style = style.add_modifier(Modifier::ITALIC),
            Event::End(TagEnd::Emphasis) => style = style.remove_modifier(Modifier::ITALIC),
            Event::Start(Tag::Strikethrough) => style = style.add_modifier(Modifier::CROSSED_OUT),
            Event::End(TagEnd::Strikethrough) => {
                style = style.remove_modifier(Modifier::CROSSED_OUT)
            }
            Event::Start(Tag::CodeBlock(_)) => style = style.add_modifier(Modifier::DIM),
            Event::End(TagEnd::CodeBlock) => {
                style = style.remove_modifier(Modifier::DIM);
                newline(&mut lines);
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                links.push(sanitize::single_line(&dest_url))
            }
            Event::End(TagEnd::Link) => {
                if let Some(url) = links.pop() {
                    current_line(&mut lines).push_span(Span::styled(
                        format!(" <{url}>"),
                        Style::default().add_modifier(Modifier::UNDERLINED),
                    ));
                }
            }
            Event::Text(value) | Event::Code(value) => {
                push_multiline(&mut lines, &sanitize::text(&value), style)
            }
            Event::SoftBreak | Event::HardBreak => newline(&mut lines),
            Event::Rule => {
                newline(&mut lines);
                current_line(&mut lines).push_span(Span::raw("────────"));
                newline(&mut lines);
            }
            Event::TaskListMarker(checked) => {
                current_line(&mut lines).push_span(Span::raw(if checked { "[x] " } else { "[ ] " }))
            }
            Event::Start(Tag::Item) => current_line(&mut lines).push_span(Span::raw("• ")),
            Event::End(TagEnd::Item | TagEnd::Paragraph | TagEnd::Heading(_)) => {
                newline(&mut lines)
            }
            _ => {}
        }
    }
    while lines.len() > 1 && lines.last().is_some_and(|line| line.spans.is_empty()) {
        lines.pop();
    }
    Text::from(lines)
}

fn push_multiline(lines: &mut Vec<Line<'static>>, value: &str, style: Style) {
    for (index, part) in value.split('\n').enumerate() {
        if index > 0 {
            newline(lines);
        }
        current_line(lines).push_span(Span::styled(part.to_owned(), style));
    }
}
fn current_line<'a>(lines: &'a mut Vec<Line<'static>>) -> &'a mut Line<'static> {
    if lines.is_empty() {
        lines.push(Line::default());
    }
    let index = lines.len() - 1;
    &mut lines[index]
}

fn newline(lines: &mut Vec<Line<'static>>) {
    if !lines.last().is_some_and(|line| line.spans.is_empty()) {
        lines.push(Line::default());
    }
}
