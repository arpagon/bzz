use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span, Text},
};

use crate::{
    render::sanitize,
    ui::theme::{HighlightGroup, Theme},
};

pub fn render(input: &str, theme: &Theme) -> Text<'static> {
    let safe = sanitize::text(input);
    let parser = Parser::new_ext(
        &safe,
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS,
    );
    let mut lines = vec![Line::default()];
    let mut markup = Style::default();
    let mut code_block_depth = 0_u8;
    let mut links = Vec::<String>::new();
    for event in parser {
        match event {
            Event::Start(Tag::Strong) => markup = markup.add_modifier(Modifier::BOLD),
            Event::End(TagEnd::Strong) => markup = markup.remove_modifier(Modifier::BOLD),
            Event::Start(Tag::Emphasis) => markup = markup.add_modifier(Modifier::ITALIC),
            Event::End(TagEnd::Emphasis) => markup = markup.remove_modifier(Modifier::ITALIC),
            Event::Start(Tag::Strikethrough) => markup = markup.add_modifier(Modifier::CROSSED_OUT),
            Event::End(TagEnd::Strikethrough) => {
                markup = markup.remove_modifier(Modifier::CROSSED_OUT)
            }
            Event::Start(Tag::CodeBlock(_)) => {
                code_block_depth = code_block_depth.saturating_add(1)
            }
            Event::End(TagEnd::CodeBlock) => {
                code_block_depth = code_block_depth.saturating_sub(1);
                newline(&mut lines);
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                links.push(sanitize::single_line(&dest_url))
            }
            Event::End(TagEnd::Link) => {
                if let Some(url) = links.pop() {
                    current_line(&mut lines).push_span(Span::styled(
                        format!(" <{url}>"),
                        theme.apply(
                            HighlightGroup::MarkdownLink,
                            theme.apply(HighlightGroup::MessageBody, markup),
                        ),
                    ));
                }
            }
            Event::Text(value) => {
                let group = if code_block_depth > 0 {
                    HighlightGroup::MarkdownCode
                } else {
                    HighlightGroup::MessageBody
                };
                push_multiline(
                    &mut lines,
                    &sanitize::text(&value),
                    theme.apply(group, markup),
                )
            }
            Event::Code(value) => push_multiline(
                &mut lines,
                &sanitize::text(&value),
                theme.apply(
                    HighlightGroup::MarkdownCode,
                    theme.apply(HighlightGroup::MessageBody, markup),
                ),
            ),
            Event::SoftBreak | Event::HardBreak => newline(&mut lines),
            Event::Rule => {
                newline(&mut lines);
                current_line(&mut lines).push_span(Span::styled(
                    "────────",
                    theme.style(HighlightGroup::MarkdownMarker),
                ));
                newline(&mut lines);
            }
            Event::TaskListMarker(checked) => current_line(&mut lines).push_span(Span::styled(
                if checked { "[x] " } else { "[ ] " },
                theme.style(HighlightGroup::MarkdownMarker),
            )),
            Event::Start(Tag::Item) => current_line(&mut lines).push_span(Span::styled(
                "• ",
                theme.style(HighlightGroup::MarkdownMarker),
            )),
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
