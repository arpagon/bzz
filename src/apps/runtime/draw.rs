//! Draws a [`Node`] tree into a Ratatui buffer, inside the area given and
//! never outside it. Scroll offsets of lists and tables with an `id` persist
//! between frames, and across reloads.
//!
//! This runs on the thread of whoever owns the runtime. What it costs is
//! bounded by the limits of a screen, which [`super::ui::parse`] holds a
//! tree to before it gets here.

use std::collections::HashMap;

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Flex, Margin, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Bar, BarChart, BarGroup, Block, Clear, Gauge, HighlightSpacing, LineGauge, List, ListDirection,
    ListItem, ListState, Paragraph, RenderDirection, Scrollbar, ScrollbarState, Sparkline,
    StatefulWidget, Tabs, Widget, Wrap,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::layout;
use super::ui::{AppTheme, Kind, Look, Node, TextLine};

/// Lists and tables that keep their scroll position, by their `id`. More
/// ids than this and the oldest are forgotten.
const KEPT_STATES: usize = 64;

/// What survives from one frame to the next.
#[derive(Default)]
pub struct States {
    lists: HashMap<String, ListState>,
    /// The first row a table shows.
    tables: HashMap<String, usize>,
    /// Where a focused input wants the terminal's cursor.
    pub cursor: Option<Position>,
}

impl States {
    fn list(&mut self, id: &str) -> &mut ListState {
        if self.lists.len() >= KEPT_STATES && !self.lists.contains_key(id) {
            self.lists.clear();
        }
        self.lists.entry(id.to_string()).or_default()
    }

    fn table(&mut self, id: &str) -> &mut usize {
        if self.tables.len() >= KEPT_STATES && !self.tables.contains_key(id) {
            self.tables.clear();
        }
        self.tables.entry(id.to_string()).or_default()
    }
}

struct Painter<'a> {
    buffer: &'a mut Buffer,
    theme: &'a AppTheme,
    states: &'a mut States,
}

/// Draws `node` into `area` of `buffer`. `area` is cut to the buffer first.
pub fn draw(node: &Node, area: Rect, buffer: &mut Buffer, theme: &AppTheme, states: &mut States) {
    let area = area.intersection(buffer.area);
    states.cursor = None;
    if area.is_empty() {
        return;
    }
    buffer.set_style(area, theme.normal);
    Painter {
        buffer,
        theme,
        states,
    }
    .node(node, area);
}

fn line<'a>(line: &'a TextLine, theme: &AppTheme) -> Line<'a> {
    let spans: Vec<Span<'a>> = line
        .pieces
        .iter()
        .map(|piece| {
            if piece.look.is_plain() {
                Span::raw(piece.text.as_str())
            } else {
                Span::styled(piece.text.as_str(), piece.look.resolve(theme))
            }
        })
        .collect();
    let mut out = Line::from(spans);
    if !line.look.is_plain() {
        out = out.style(line.look.resolve(theme));
    }
    if let Some(align) = line.align {
        out = out.alignment(align);
    }
    out
}

fn lines<'a>(all: &'a [TextLine], theme: &AppTheme) -> Vec<Line<'a>> {
    all.iter().map(|l| line(l, theme)).collect()
}

impl Painter<'_> {
    fn node(&mut self, n: &Node, area: Rect) {
        if area.is_empty() {
            return;
        }
        let theme = self.theme;
        match &n.kind {
            Kind::Text {
                lines: text,
                look,
                align,
                wrap,
                scroll,
            } => {
                let mut p = Paragraph::new(Text::from(lines(text, theme)))
                    .style(look.resolve(theme))
                    .alignment(*align)
                    .scroll((*scroll, 0));
                if *wrap {
                    p = p.wrap(Wrap { trim: false });
                }
                p.render(area, self.buffer);
            }
            Kind::Block {
                title,
                title_bottom,
                title_align,
                borders,
                border_type,
                border_look,
                padding,
                look,
                spacing,
                children,
            } => {
                let mut b = Block::new()
                    .borders(*borders)
                    .border_type(*border_type)
                    .border_style(border_look.resolve(theme))
                    .padding(*padding)
                    .style(look.resolve(theme))
                    .title_alignment(*title_align);
                if let Some(t) = title {
                    b = b.title(line(t, theme));
                }
                if let Some(t) = title_bottom {
                    b = b.title_bottom(line(t, theme));
                }
                let inner = b.inner(area).intersection(area);
                b.render(area, self.buffer);
                self.stack(children, inner, Direction::Vertical, *spacing, Flex::Legacy);
            }
            Kind::Stack {
                direction,
                spacing,
                flex,
                margin,
                look,
                children,
            } => {
                if !look.is_plain() {
                    self.buffer.set_style(area, look.resolve(theme));
                }
                let inner = area.inner(Margin::new(*margin, *margin));
                self.stack(children, inner, *direction, *spacing, *flex);
            }
            Kind::List {
                items,
                selected,
                symbol,
                highlight,
                look,
                id,
                bottom_to_top,
                scroll_padding,
            } => {
                let list = List::new(items.iter().map(|l| ListItem::new(line(l, theme))))
                    .style(look.resolve(theme))
                    .highlight_symbol(symbol.as_str())
                    .highlight_spacing(HighlightSpacing::Always)
                    .highlight_style(highlight.resolve(theme))
                    .scroll_padding(*scroll_padding)
                    .direction(if *bottom_to_top {
                        ListDirection::BottomToTop
                    } else {
                        ListDirection::TopToBottom
                    });
                let chosen = selected.filter(|i| *i < items.len());
                match id {
                    Some(id) => {
                        let state = self.states.list(id);
                        state.select(chosen);
                        StatefulWidget::render(list, area, self.buffer, state);
                    }
                    None => StatefulWidget::render(
                        list,
                        area,
                        self.buffer,
                        &mut ListState::default().with_selected(chosen),
                    ),
                }
            }
            Kind::Table {
                header,
                rows,
                footer,
                widths,
                selected,
                highlight,
                symbol,
                column_spacing,
                header_look,
                look,
                id,
            } => {
                let chosen = selected.filter(|i| *i < rows.len());
                let first = match id {
                    Some(id) => *self.states.table(id),
                    None => 0,
                };
                let first = self.table(
                    area,
                    TableParts {
                        header: header.as_deref(),
                        rows,
                        footer: footer.as_deref(),
                        widths,
                        chosen,
                        highlight,
                        symbol,
                        column_spacing: *column_spacing,
                        header_look,
                        look,
                    },
                    first,
                );
                if let Some(id) = id {
                    *self.states.table(id) = first;
                }
            }
            Kind::Tabs {
                titles,
                selected,
                divider,
                highlight,
                look,
            } => {
                Tabs::new(lines(titles, theme))
                    .select(*selected)
                    .divider(divider.as_str())
                    .highlight_style(highlight.resolve(theme))
                    .style(look.resolve(theme))
                    .render(area, self.buffer);
            }
            Kind::Gauge {
                ratio,
                label,
                look,
                gauge_look,
                unicode,
            } => {
                let mut g = Gauge::default()
                    .ratio(*ratio)
                    .style(look.resolve(theme))
                    .gauge_style(gauge_look.resolve(theme))
                    .use_unicode(*unicode);
                if let Some(l) = label {
                    g = g.label(l.as_str());
                }
                g.render(area, self.buffer);
            }
            Kind::LineGauge {
                ratio,
                label,
                look,
                filled,
                unfilled,
            } => {
                let mut g = LineGauge::default()
                    .ratio(*ratio)
                    .style(look.resolve(theme))
                    .filled_style(filled.resolve(theme))
                    .unfilled_style(unfilled.resolve(theme));
                if let Some(l) = label {
                    g = g.label(l.as_str());
                }
                g.render(area, self.buffer);
            }
            Kind::Sparkline {
                data,
                max,
                look,
                right_to_left,
            } => {
                let mut s = Sparkline::default()
                    .data(data.iter().copied())
                    .style(look.resolve(theme))
                    .direction(if *right_to_left {
                        RenderDirection::RightToLeft
                    } else {
                        RenderDirection::LeftToRight
                    });
                // A largest value of zero would have the widget divide by it.
                if let Some(m) = max.filter(|m| *m > 0) {
                    s = s.max(m);
                }
                s.render(area, self.buffer);
            }
            Kind::BarChart {
                bars,
                bar_width,
                bar_gap,
                max,
                horizontal,
                look,
                value_look,
                label_look,
            } => {
                // Ratatui multiplies the number of bars by their width in
                // sixteen bits. It gets no more bars than can be seen, and
                // none wider than the area: what is drawn is the same.
                let extent = if *horizontal { area.height } else { area.width };
                let width = (*bar_width).clamp(1, extent.max(1));
                let gap = (*bar_gap).min(extent);
                let fit = u32::from(u16::MAX) / (u32::from(width) + u32::from(gap));
                let count = bars.len().min(extent as usize).min(fit as usize);
                let shown: Vec<Bar> = bars[..count]
                    .iter()
                    .map(|b| {
                        let mut bar = Bar::default()
                            .value(b.value)
                            .label(line(&b.label, theme))
                            .style(b.look.resolve(theme));
                        if let Some(t) = &b.text {
                            bar = bar.text_value(t.as_str());
                        }
                        bar
                    })
                    .collect();
                // The largest value is of all the bars, also those that
                // do not fit; zero would have the widget divide by it.
                let most = max
                    .or_else(|| bars.iter().map(|b| b.value).max())
                    .filter(|m| *m > 0);
                let mut chart = BarChart::default()
                    .data(BarGroup::default().bars(&shown))
                    .bar_width(width)
                    .bar_gap(gap)
                    .style(look.resolve(theme))
                    .value_style(value_look.resolve(theme))
                    .label_style(label_look.resolve(theme))
                    .direction(if *horizontal {
                        Direction::Horizontal
                    } else {
                        Direction::Vertical
                    });
                if let Some(m) = most {
                    chart = chart.max(m);
                }
                chart.render(area, self.buffer);
            }
            Kind::Scrollbar {
                orientation,
                length,
                position,
                viewport,
                look,
                thumb,
                track,
            } => {
                let bar = Scrollbar::new(orientation.clone())
                    .style(look.resolve(theme))
                    .thumb_style(thumb.resolve(theme))
                    .track_style(track.resolve(theme));
                let mut state = ScrollbarState::new(*length)
                    .position(*position)
                    .viewport_content_length(*viewport);
                StatefulWidget::render(bar, area, self.buffer, &mut state);
            }
            Kind::Input {
                value,
                cursor,
                placeholder,
                focused,
                look,
            } => self.input(
                value,
                *cursor,
                placeholder,
                *focused,
                look.resolve(theme),
                area,
            ),
            Kind::Layers { children } => {
                for child in children {
                    self.node(child, area);
                }
            }
            Kind::Popup {
                width,
                height,
                title,
                border_type,
                look,
                spacing,
                children,
            } => {
                let across = layout::spans(area.width, &[width.constraint()], 0, Flex::Center)[0];
                let down = layout::spans(area.height, &[height.constraint()], 0, Flex::Center)[0];
                let rect = Rect::new(area.x + across.0, area.y + down.0, across.1, down.1)
                    .intersection(area);
                if rect.is_empty() {
                    return;
                }
                Clear.render(rect, self.buffer);
                self.buffer.set_style(rect, theme.normal);
                let mut b = Block::bordered()
                    .border_type(*border_type)
                    .style(look.resolve(theme));
                if let Some(t) = title {
                    b = b.title(line(t, theme));
                }
                let inner = b.inner(rect).intersection(rect);
                b.render(rect, self.buffer);
                self.stack(children, inner, Direction::Vertical, *spacing, Flex::Legacy);
            }
            Kind::Spacer => {}
        }
    }

    /// A one-line field. The text scrolls sideways so that the cursor stays
    /// inside the area; one pass over the text, whatever its length.
    fn input(
        &mut self,
        value: &str,
        cursor: usize,
        placeholder: &str,
        focused: bool,
        style: Style,
        area: Rect,
    ) {
        let width = area.width as usize;
        let mut widths: Vec<usize> = Vec::new();
        let mut offsets: Vec<usize> = Vec::new();
        for (at, c) in value.char_indices() {
            offsets.push(at);
            widths.push(c.width().unwrap_or(0));
        }
        let cursor = cursor.min(widths.len());
        // Cells from the leftmost character shown up to the cursor.
        let mut before: usize = widths[..cursor].iter().sum();
        let mut start = 0;
        while start < cursor && before >= width {
            before -= widths[start];
            start += 1;
        }
        let shown = offsets.get(start).map_or("", |at| &value[*at..]);
        let line = if value.is_empty() && !placeholder.is_empty() {
            Line::styled(placeholder, style.add_modifier(Modifier::DIM))
        } else {
            Line::styled(shown, style)
        };
        Paragraph::new(line).render(area, self.buffer);
        if focused {
            let x = area.x + before.min(width.saturating_sub(1)) as u16;
            self.states.cursor = Some(Position::new(x, area.y));
        }
    }

    fn stack(
        &mut self,
        children: &[Node],
        area: Rect,
        direction: Direction,
        spacing: u16,
        flex: Flex,
    ) {
        if children.is_empty() || area.is_empty() {
            return;
        }
        let constraints: Vec<Constraint> = children
            .iter()
            .map(|child| default_size(child, direction))
            .collect();
        let areas = layout::split(area, direction, &constraints, spacing, flex);
        for (child, rect) in children.iter().zip(areas.iter()) {
            self.node(child, rect.intersection(area));
        }
    }

    /// A table: a header row, the rows that fit from `first` on, a footer
    /// row. The column for the selection symbol is always there, so that
    /// rows line up whether one is selected or not. Returns the first row
    /// it showed, moved so that the selected row is in sight.
    ///
    /// Ratatui's own `Table` divides its columns with the solver that
    /// [`layout`] is written to avoid, so the table is drawn here.
    fn table(&mut self, area: Rect, table: TableParts<'_>, first: usize) -> usize {
        let theme = self.theme;
        if !table.look.is_plain() {
            self.buffer.set_style(area, table.look.resolve(theme));
        }
        let mark = (table.symbol.width() as u16).min(area.width);
        let columns = layout::spans(
            area.width - mark,
            table.widths,
            table.column_spacing,
            Flex::Start,
        );
        let top = u16::from(table.header.is_some()).min(area.height);
        let bottom = u16::from(table.footer.is_some()).min(area.height - top);
        let body = Rect::new(area.x, area.y + top, area.width, area.height - top - bottom);
        let buffer = &mut *self.buffer;
        let row = |buffer: &mut Buffer, cells: &[TextLine], y: u16, style: Option<Style>| {
            if let Some(style) = style {
                buffer.set_style(Rect::new(area.x, y, area.width, 1), style);
            }
            for (cell, (offset, width)) in cells.iter().zip(&columns) {
                let rect = Rect::new(area.x + mark + offset, y, *width, 1);
                line(cell, theme).render(rect, buffer);
            }
        };
        if let (Some(header), true) = (table.header, top > 0) {
            let style = (!table.header_look.is_plain()).then(|| table.header_look.resolve(theme));
            row(buffer, header, area.y, style);
        }
        if let (Some(footer), true) = (table.footer, bottom > 0) {
            row(buffer, footer, area.bottom() - 1, None);
        }
        let shown = body.height as usize;
        let mut first = first.min(table.rows.len().saturating_sub(shown.max(1)));
        if let Some(chosen) = table.chosen {
            if chosen < first {
                first = chosen;
            } else if shown > 0 && chosen >= first + shown {
                first = chosen + 1 - shown;
            }
        }
        for (i, cells) in table.rows.iter().enumerate().skip(first).take(shown) {
            let y = body.y + (i - first) as u16;
            let is_chosen = table.chosen == Some(i);
            let style = is_chosen.then(|| table.highlight.resolve(theme));
            row(buffer, cells, y, style);
            if is_chosen {
                buffer.set_stringn(
                    area.x,
                    y,
                    table.symbol,
                    mark as usize,
                    table.highlight.resolve(theme),
                );
            }
        }
        first
    }
}

/// What a table is drawn from.
struct TableParts<'a> {
    header: Option<&'a [TextLine]>,
    rows: &'a [Vec<TextLine>],
    footer: Option<&'a [TextLine]>,
    widths: &'a [Constraint],
    chosen: Option<usize>,
    highlight: &'a Look,
    symbol: &'a str,
    column_spacing: u16,
    header_look: &'a Look,
    look: &'a Look,
}

/// `size` if given; in a column, a text takes its line count and one-line
/// elements take one row; everything else fills the space left.
fn default_size(n: &Node, direction: Direction) -> Constraint {
    if let Some(size) = n.size {
        return size.constraint();
    }
    if direction == Direction::Vertical {
        match &n.kind {
            Kind::Text {
                lines, wrap: false, ..
            } => {
                return Constraint::Length(lines.len().min(u16::MAX as usize) as u16);
            }
            Kind::Input { .. } | Kind::LineGauge { .. } | Kind::Tabs { .. } => {
                return Constraint::Length(1);
            }
            _ => {}
        }
    }
    Constraint::Fill(1)
}

/// A buffer's cells as text, one line for each row, with the spaces at the
/// end of a row removed. A cell that a wide character covers gives nothing.
pub fn text_of(buffer: &Buffer) -> String {
    let area = buffer.area;
    let mut out = String::new();
    for y in area.top()..area.bottom() {
        let mut row = String::new();
        let mut skip = 0usize;
        for x in area.left()..area.right() {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let symbol = buffer[(x, y)].symbol();
            row.push_str(symbol);
            skip = symbol.width().saturating_sub(1);
        }
        out.push_str(row.trim_end());
        out.push('\n');
    }
    out
}
