use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span, Text},
};

use crate::{
    render::sanitize,
    ui::theme::{HighlightGroup, Theme},
};

const MAX_TABLE_CELL_CHARS: usize = 48;

#[derive(Clone, Copy)]
struct ListState {
    next: u64,
    ordered: bool,
}

/// Renders a bounded, terminal-safe Markdown subset. Message source is never
/// changed: these are presentation markers only, and links remain inert text.
pub fn render(input: &str, theme: &Theme) -> Text<'static> {
    let safe = sanitize::text(input);
    let parser = Parser::new_ext(
        &safe,
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS,
    );
    let mut lines = vec![Line::default()];
    let mut markup = Style::default();
    let mut links = Vec::<String>::new();
    let mut heading_depth = 0_u8;
    let mut quote_depth = 0_u8;
    let mut lists = Vec::<ListState>::new();
    let mut code_block = false;
    let mut in_table = false;
    let mut table_cell = false;

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
            Event::Start(Tag::Heading { level, .. }) => {
                newline(&mut lines);
                heading_depth = level as u8;
                current_line(&mut lines).push_span(Span::styled(
                    format!("{} ", "#".repeat(usize::from(heading_depth))),
                    theme.style(HighlightGroup::MarkdownMarker),
                ));
            }
            Event::End(TagEnd::Heading(_)) => {
                heading_depth = 0;
                newline(&mut lines);
            }
            Event::Start(Tag::BlockQuote(_)) => {
                newline(&mut lines);
                quote_depth = quote_depth.saturating_add(1);
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                quote_depth = quote_depth.saturating_sub(1);
                newline(&mut lines);
            }
            Event::Start(Tag::List(first)) => lists.push(ListState {
                next: first.unwrap_or(1),
                ordered: first.is_some(),
            }),
            Event::End(TagEnd::List(_)) => {
                lists.pop();
                newline(&mut lines);
            }
            Event::Start(Tag::Item) => {
                newline(&mut lines);
                quote_prefix(&mut lines, quote_depth, theme);
                let indentation = "  ".repeat(lists.len().saturating_sub(1));
                current_line(&mut lines).push_span(Span::styled(
                    indentation,
                    theme.style(HighlightGroup::MarkdownMarker),
                ));
                let marker = if let Some(list) = lists.last_mut() {
                    if list.ordered {
                        let value = format!("{}. ", list.next);
                        list.next = list.next.saturating_add(1);
                        value
                    } else {
                        "• ".into()
                    }
                } else {
                    "• ".into()
                };
                current_line(&mut lines).push_span(Span::styled(
                    marker,
                    theme.style(HighlightGroup::MarkdownMarker),
                ));
            }
            Event::End(TagEnd::Item | TagEnd::Paragraph) => newline(&mut lines),
            Event::Start(Tag::CodeBlock(kind)) => {
                newline(&mut lines);
                code_block = true;
                let language = match kind {
                    CodeBlockKind::Fenced(language) if !language.is_empty() => {
                        Some(sanitize::single_line(&language))
                    }
                    _ => None,
                };
                let label = language
                    .as_deref()
                    .map_or_else(|| "code".into(), |language| format!("code · {language}"));
                current_line(&mut lines).push_span(Span::styled(
                    format!("┌ {label}"),
                    theme.style(HighlightGroup::MarkdownMarker),
                ));
                newline(&mut lines);
            }
            Event::End(TagEnd::CodeBlock) => {
                current_line(&mut lines).push_span(Span::styled(
                    "└",
                    theme.style(HighlightGroup::MarkdownMarker),
                ));
                code_block = false;
                newline(&mut lines);
            }
            Event::Start(Tag::Table(_)) => {
                newline(&mut lines);
                in_table = true;
            }
            Event::End(TagEnd::Table) => {
                in_table = false;
                table_cell = false;
                newline(&mut lines);
            }
            Event::Start(Tag::TableRow) => {
                newline(&mut lines);
                current_line(&mut lines).push_span(Span::styled(
                    "│ ",
                    theme.style(HighlightGroup::MarkdownMarker),
                ));
            }
            Event::End(TagEnd::TableRow) => newline(&mut lines),
            Event::Start(Tag::TableCell) => table_cell = true,
            Event::End(TagEnd::TableCell) => {
                table_cell = false;
                current_line(&mut lines).push_span(Span::styled(
                    " │ ",
                    theme.style(HighlightGroup::MarkdownMarker),
                ));
            }
            Event::End(TagEnd::TableHead) => {
                current_line(&mut lines).push_span(Span::styled(
                    "├────────┤",
                    theme.style(HighlightGroup::MarkdownMarker),
                ));
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
                let value = sanitize::text(&value);
                if in_table && table_cell {
                    push_multiline(
                        &mut lines,
                        &bounded_table_cell(&value),
                        theme.apply(HighlightGroup::MessageBody, markup),
                    );
                } else if code_block {
                    code_lines(&mut lines, &value, theme, quote_depth);
                } else {
                    quote_prefix(&mut lines, quote_depth, theme);
                    let group = if heading_depth > 0 {
                        HighlightGroup::MessageAuthor
                    } else {
                        HighlightGroup::MessageBody
                    };
                    push_multiline(&mut lines, &value, theme.apply(group, markup));
                }
            }
            Event::Code(value) => {
                quote_prefix(&mut lines, quote_depth, theme);
                push_multiline(
                    &mut lines,
                    &sanitize::text(&value),
                    theme.apply(
                        HighlightGroup::MarkdownCode,
                        theme.apply(HighlightGroup::MessageBody, markup),
                    ),
                )
            }
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
            _ => {}
        }
    }
    while lines.len() > 1 && lines.last().is_some_and(|line| line.spans.is_empty()) {
        lines.pop();
    }
    Text::from(lines)
}

fn bounded_table_cell(value: &str) -> String {
    let mut output = String::new();
    for (index, character) in value
        .chars()
        .filter(|character| *character != '\n')
        .enumerate()
    {
        if index == MAX_TABLE_CELL_CHARS {
            output.push('…');
            break;
        }
        output.push(character);
    }
    output
}

fn code_lines(lines: &mut Vec<Line<'static>>, value: &str, theme: &Theme, quote_depth: u8) {
    for (index, part) in value.split('\n').enumerate() {
        if index > 0 {
            newline(lines);
        }
        quote_prefix(lines, quote_depth, theme);
        current_line(lines).push_span(Span::styled(
            "│ ",
            theme.style(HighlightGroup::MarkdownMarker),
        ));
        current_line(lines).push_span(Span::styled(
            part.to_owned(),
            theme.style(HighlightGroup::MarkdownCode),
        ));
    }
}

fn quote_prefix(lines: &mut Vec<Line<'static>>, depth: u8, theme: &Theme) {
    if depth > 0 && current_line(lines).spans.is_empty() {
        current_line(lines).push_span(Span::styled(
            "│ ".repeat(usize::from(depth)),
            theme.style(HighlightGroup::MarkdownMarker),
        ));
    }
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
