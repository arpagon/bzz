use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span, Text},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    render::sanitize,
    ui::theme::{HighlightGroup, Theme},
};

const MAX_TABLE_CELL_CHARS: usize = 48;
const MIN_TABLE_CELL_WIDTH: usize = 3;

#[derive(Clone, Copy)]
struct ListState {
    next: u64,
    ordered: bool,
}

#[derive(Default)]
struct TableState {
    header: Option<Vec<String>>,
    rows: Vec<Vec<String>>,
    row: Vec<String>,
    cell: String,
}

/// Renders a bounded, terminal-safe Markdown subset at a conservative default
/// measure. Use [`render_with_width`] when the available body width is known.
/// Message source is never changed: these are presentation markers only, and
/// links remain inert text.
pub fn render(input: &str, theme: &Theme) -> Text<'static> {
    render_with_width(input, theme, 80)
}

/// Renders a bounded, terminal-safe Markdown subset at the supplied content
/// measure. Tables use a measured Unicode grid when it fits; very wide tables
/// become labelled row cards rather than wrapping misleading columns.
pub fn render_with_width(input: &str, theme: &Theme, content_width: u16) -> Text<'static> {
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
    let mut table = None::<TableState>;
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
            Event::End(TagEnd::Item | TagEnd::Paragraph) if !table_cell => newline(&mut lines),
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
                table = Some(TableState::default());
                table_cell = false;
            }
            Event::End(TagEnd::Table) => {
                table_cell = false;
                if let Some(table) = table.take() {
                    append_table(&mut lines, table, theme, usize::from(content_width));
                }
                newline(&mut lines);
            }
            // pulldown-cmark places header cells directly under TableHead;
            // body cells are grouped by TableRow.
            Event::Start(Tag::TableHead) => {
                if let Some(table) = table.as_mut() {
                    table.row.clear();
                }
            }
            Event::End(TagEnd::TableHead) => {
                if let Some(table) = table.as_mut() {
                    table.header = Some(std::mem::take(&mut table.row));
                }
            }
            Event::Start(Tag::TableRow) => {
                if let Some(table) = table.as_mut() {
                    table.row.clear();
                }
            }
            Event::End(TagEnd::TableRow) => {
                if let Some(table) = table.as_mut() {
                    let row = std::mem::take(&mut table.row);
                    table.rows.push(row);
                }
            }
            Event::Start(Tag::TableCell) => {
                table_cell = true;
                if let Some(table) = table.as_mut() {
                    table.cell.clear();
                }
            }
            Event::End(TagEnd::TableCell) => {
                table_cell = false;
                if let Some(table) = table.as_mut() {
                    table.row.push(std::mem::take(&mut table.cell));
                }
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                links.push(sanitize::single_line(&dest_url))
            }
            Event::End(TagEnd::Link) => {
                if let Some(url) = links.pop() {
                    if table_cell {
                        if let Some(table) = table.as_mut() {
                            table.cell.push_str(" <");
                            table.cell.push_str(&url);
                            table.cell.push('>');
                        }
                    } else {
                        current_line(&mut lines).push_span(Span::styled(
                            format!(" <{url}>"),
                            theme.apply(
                                HighlightGroup::MarkdownLink,
                                theme.apply(HighlightGroup::MessageBody, markup),
                            ),
                        ));
                    }
                }
            }
            Event::Text(value) if table_cell => {
                if let Some(table) = table.as_mut() {
                    table.cell.push_str(&sanitize::text(&value));
                }
            }
            Event::Text(value) => {
                let value = sanitize::text(&value);
                if code_block {
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
            Event::Code(value) if table_cell => {
                if let Some(table) = table.as_mut() {
                    table.cell.push('`');
                    table.cell.push_str(&sanitize::text(&value));
                    table.cell.push('`');
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
            Event::SoftBreak | Event::HardBreak if table_cell => {
                if let Some(table) = table.as_mut() {
                    table.cell.push(' ');
                }
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
            Event::TaskListMarker(checked) if table_cell => {
                if let Some(table) = table.as_mut() {
                    table.cell.push_str(if checked { "[x] " } else { "[ ] " });
                }
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

fn append_table(lines: &mut Vec<Line<'static>>, table: TableState, theme: &Theme, width: usize) {
    let columns = table
        .header
        .as_ref()
        .into_iter()
        .chain(table.rows.iter())
        .map(Vec::len)
        .max()
        .unwrap_or_default();
    if columns == 0 {
        return;
    }

    let headers = table.header.unwrap_or_else(|| {
        (1..=columns)
            .map(|number| format!("column {number}"))
            .collect()
    });
    let desired = table_widths(&headers, &table.rows, columns);
    let desired_frame = table_frame_width(&desired);
    // A grid with more than five truncated columns is harder to interpret than
    // a concise labelled record. Preserve each source value in that case.
    if desired_frame > width && columns > 5 {
        append_table_cards(lines, &headers, &table.rows, columns, theme, width);
        return;
    }

    let Some(widths) = fit_table_widths(desired, width) else {
        append_table_cards(lines, &headers, &table.rows, columns, theme, width);
        return;
    };
    append_table_grid(lines, &headers, &table.rows, &widths, theme);
}

fn table_widths(headers: &[String], rows: &[Vec<String>], columns: usize) -> Vec<usize> {
    (0..columns)
        .map(|column| {
            std::iter::once(headers.get(column))
                .chain(rows.iter().map(|row| row.get(column)))
                .flatten()
                .map(|value| display_width(&bounded_table_cell(value)))
                .max()
                .unwrap_or(MIN_TABLE_CELL_WIDTH)
                .clamp(MIN_TABLE_CELL_WIDTH, MAX_TABLE_CELL_CHARS)
        })
        .collect()
}

fn fit_table_widths(mut widths: Vec<usize>, available: usize) -> Option<Vec<usize>> {
    if table_frame_width(&widths) > available {
        while table_frame_width(&widths) > available {
            let (index, _) = widths
                .iter()
                .enumerate()
                .filter(|(_, width)| **width > MIN_TABLE_CELL_WIDTH)
                .max_by_key(|(_, width)| **width)?;
            widths[index] = widths[index].saturating_sub(1);
        }
    }
    Some(widths)
}

fn table_frame_width(widths: &[usize]) -> usize {
    widths
        .iter()
        .fold(widths.len().saturating_add(1), |total, width| {
            total.saturating_add(width.saturating_add(2))
        })
}

fn append_table_grid(
    lines: &mut Vec<Line<'static>>,
    headers: &[String],
    rows: &[Vec<String>],
    widths: &[usize],
    theme: &Theme,
) {
    push_block_line(
        lines,
        Line::styled(
            table_border(widths, '┌', '┬', '┐'),
            theme.style(HighlightGroup::MarkdownMarker),
        ),
    );
    push_block_line(lines, table_row(headers, widths, theme, true));
    push_block_line(
        lines,
        Line::styled(
            table_border(widths, '├', '┼', '┤'),
            theme.style(HighlightGroup::MarkdownMarker),
        ),
    );
    for row in rows {
        push_block_line(lines, table_row(row, widths, theme, false));
    }
    push_block_line(
        lines,
        Line::styled(
            table_border(widths, '└', '┴', '┘'),
            theme.style(HighlightGroup::MarkdownMarker),
        ),
    );
}

fn append_table_cards(
    lines: &mut Vec<Line<'static>>,
    headers: &[String],
    rows: &[Vec<String>],
    columns: usize,
    theme: &Theme,
    width: usize,
) {
    push_block_line(
        lines,
        Line::styled(
            format!("┌ table · {} rows × {columns} columns", rows.len()),
            theme.style(HighlightGroup::MarkdownMarker),
        ),
    );
    for (index, row) in rows.iter().enumerate() {
        push_block_line(
            lines,
            Line::styled(
                format!("├ row {}", index + 1),
                theme.style(HighlightGroup::MarkdownMarker),
            ),
        );
        let fields = (0..columns)
            .filter_map(|column| {
                let value = row.get(column)?;
                let header = headers.get(column).map_or("column", String::as_str);
                Some(format!("{header}: {}", bounded_table_cell(value)))
            })
            .collect::<Vec<_>>();
        for line in wrap_table_fields(&fields, width.saturating_sub(2)) {
            push_block_line(
                lines,
                Line::styled(
                    format!("│ {line}"),
                    theme.style(HighlightGroup::MessageBody),
                ),
            );
        }
    }
    push_block_line(
        lines,
        Line::styled("└", theme.style(HighlightGroup::MarkdownMarker)),
    );
}

fn wrap_table_fields(fields: &[String], width: usize) -> Vec<String> {
    let width = width.max(12);
    let mut lines = Vec::new();
    let mut current = String::new();
    for field in fields {
        let separator = if current.is_empty() { "" } else { " · " };
        if !current.is_empty()
            && display_width(&current)
                .saturating_add(display_width(separator))
                .saturating_add(display_width(field))
                > width
        {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push_str(" · ");
        }
        current.push_str(field);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

fn table_border(widths: &[usize], left: char, join: char, right: char) -> String {
    let mut output = String::new();
    output.push(left);
    for (index, width) in widths.iter().enumerate() {
        output.push_str(&"─".repeat(width.saturating_add(2)));
        output.push(if index + 1 == widths.len() {
            right
        } else {
            join
        });
    }
    output
}

fn table_row(cells: &[String], widths: &[usize], theme: &Theme, header: bool) -> Line<'static> {
    let mut spans = vec![Span::styled(
        "│ ",
        theme.style(HighlightGroup::MarkdownMarker),
    )];
    for (index, width) in widths.iter().enumerate() {
        let value = cells.get(index).map_or("", String::as_str);
        let value = truncate_display(&bounded_table_cell(value), *width);
        let padding = " ".repeat(width.saturating_sub(display_width(&value)));
        spans.push(Span::styled(
            format!("{value}{padding}"),
            theme.style(if header {
                HighlightGroup::MessageAuthor
            } else {
                HighlightGroup::MessageBody
            }),
        ));
        spans.push(Span::styled(
            if index + 1 == widths.len() {
                " │"
            } else {
                " │ "
            },
            theme.style(HighlightGroup::MarkdownMarker),
        ));
    }
    Line::from(spans)
}

fn bounded_table_cell(value: &str) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut characters = normalized.chars();
    let mut output = characters
        .by_ref()
        .take(MAX_TABLE_CELL_CHARS)
        .collect::<String>();
    if characters.next().is_some() {
        output.push('…');
    }
    output
}

fn truncate_display(value: &str, width: usize) -> String {
    if display_width(value) <= width {
        return value.to_owned();
    }
    if width <= 1 {
        return "…".into();
    }
    let mut output = String::new();
    let mut used = 0_usize;
    for character in value.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or_default();
        if used.saturating_add(character_width).saturating_add(1) > width {
            break;
        }
        output.push(character);
        used = used.saturating_add(character_width);
    }
    output.push('…');
    output
}

fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

fn push_block_line(lines: &mut Vec<Line<'static>>, line: Line<'static>) {
    if lines.last().is_some_and(|line| line.spans.is_empty()) {
        *current_line(lines) = line;
    } else {
        lines.push(line);
    }
    lines.push(Line::default());
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
