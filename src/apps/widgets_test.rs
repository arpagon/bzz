//! v0.16.0 M0, question 6: everything of Ratatui that the app contract draws
//! with is available under bzz's own feature set (`default-features =
//! false`, with `crossterm_0_29`, `layout-cache` and
//! `unstable-rendered-line-info`).
//!
//! The list is what the prototype's drawing uses
//! (`f1/s-embedded/host/src/draw.rs` and `ui.rs` of the experiment), one use
//! of each, drawn to the test backend. If a widget were behind a feature bzz
//! does not enable, this file would not compile.

use std::str::FromStr;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Flex, Layout, Margin, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Bar, BarChart, BarGroup, Block, BorderType, Borders, Clear, Gauge, HighlightSpacing, LineGauge,
    List, ListDirection, ListItem, ListState, Padding, Paragraph, RenderDirection, Row, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Sparkline, Table, TableState, Tabs, Wrap,
};

/// The screen as text, one line per row.
fn rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn every_widget_of_the_app_contract_is_available() {
    let mut terminal = Terminal::new(TestBackend::new(60, 40)).expect("terminal");
    let mut cursor = None;
    terminal
        .draw(|f| {
            // Sizes: every constraint a `size` of the contract maps to.
            let [top, tabs, gauges, charts, tables, bottom] = Layout::new(
                Direction::Vertical,
                [
                    Constraint::Length(6),
                    Constraint::Max(1),
                    Constraint::Min(3),
                    Constraint::Percentage(20),
                    Constraint::Ratio(1, 4),
                    Constraint::Fill(1),
                ],
            )
            .spacing(0)
            .flex(Flex::Legacy)
            .areas(f.area());

            // block, text, span, line.
            let style = Style::new()
                .fg(Color::from_str("light_blue").expect("a colour name"))
                .bg(Color::from_str("#102030").expect("a hex colour"))
                .add_modifier(
                    Modifier::BOLD
                        | Modifier::DIM
                        | Modifier::ITALIC
                        | Modifier::UNDERLINED
                        | Modifier::REVERSED
                        | Modifier::CROSSED_OUT,
                );
            let block = Block::new()
                .borders(Borders::TOP | Borders::BOTTOM | Borders::LEFT | Borders::RIGHT)
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(Color::Indexed(8)))
                .padding(Padding::new(1, 1, 0, 0))
                .style(Style::new())
                .title_alignment(Alignment::Center)
                .title("Board")
                .title_bottom(Line::raw("3 cards"));
            let inner = block.inner(top);
            f.render_widget(block, top);
            let text = Text::from(vec![
                Line::from(vec![Span::raw("plain "), Span::styled("styled", style)])
                    .alignment(Alignment::Left),
                Line::raw("a line that is longer than the block is wide, to be wrapped by it"),
            ]);
            f.render_widget(
                Paragraph::new(text)
                    .style(Style::new())
                    .alignment(Alignment::Left)
                    .scroll((0, 0))
                    .wrap(Wrap { trim: false }),
                inner,
            );
            for (border, padding) in [
                (BorderType::Plain, Padding::ZERO),
                (BorderType::Double, Padding::uniform(1)),
                (BorderType::Thick, Padding::symmetric(1, 0)),
            ] {
                let _ = Block::bordered()
                    .border_type(border)
                    .padding(padding)
                    .borders(Borders::ALL | Borders::NONE);
            }

            // tabs.
            f.render_widget(
                Tabs::new(vec!["Todo", "Doing", "Done"])
                    .select(1)
                    .divider("|")
                    .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
                    .style(Style::new()),
                tabs,
            );

            // gauge, line_gauge, input (a paragraph and the cursor).
            let [gauge, line_gauge, input] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .flex(Flex::Start)
            .areas(gauges);
            f.render_widget(
                Gauge::default()
                    .ratio(0.5)
                    .label("half")
                    .style(Style::new())
                    .gauge_style(Style::new().fg(Color::Green))
                    .use_unicode(true),
                gauge,
            );
            f.render_widget(
                LineGauge::default()
                    .ratio(0.25)
                    .label("quarter")
                    .style(Style::new())
                    .filled_style(Style::new().fg(Color::Green))
                    .unfilled_style(Style::new().fg(Color::DarkGray)),
                line_gauge,
            );
            f.render_widget(Paragraph::new(Line::styled("typed", Style::new())), input);
            cursor = Some(Position::new(input.x + 5, input.y));

            // sparkline, bar_chart, scrollbar.
            let [spark, bars, scroll] = Layout::horizontal([
                Constraint::Fill(1),
                Constraint::Fill(2),
                Constraint::Length(1),
            ])
            .flex(Flex::SpaceBetween)
            .areas(charts);
            f.render_widget(
                Sparkline::default()
                    .data([1u64, 4, 2, 8])
                    .max(8)
                    .style(Style::new())
                    .direction(RenderDirection::RightToLeft),
                spark,
            );
            let bar = [
                Bar::default()
                    .value(3)
                    .label("yes")
                    .text_value("3")
                    .style(Style::new()),
                Bar::default().value(1).label("no"),
            ];
            f.render_widget(
                BarChart::default()
                    .data(BarGroup::default().bars(&bar))
                    .bar_width(3)
                    .bar_gap(1)
                    .max(4)
                    .style(Style::new())
                    .value_style(Style::new())
                    .label_style(Style::new())
                    .direction(Direction::Vertical),
                bars,
            );
            let mut state = ScrollbarState::new(40)
                .position(10)
                .viewport_content_length(8);
            f.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .style(Style::new())
                    .thumb_style(Style::new())
                    .track_style(Style::new()),
                scroll,
                &mut state,
            );
            for orientation in [
                ScrollbarOrientation::VerticalLeft,
                ScrollbarOrientation::HorizontalBottom,
                ScrollbarOrientation::HorizontalTop,
            ] {
                let _ = Scrollbar::new(orientation);
            }

            // list and table, with their states.
            let [list, table] =
                Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
                    .areas(tables.inner(Margin::new(0, 0)));
            let mut list_state = ListState::default().with_selected(Some(1));
            f.render_stateful_widget(
                List::new(["first", "second", "third"].map(ListItem::new))
                    .style(Style::new())
                    .highlight_symbol("> ")
                    .highlight_spacing(HighlightSpacing::Always)
                    .highlight_style(Style::new().add_modifier(Modifier::BOLD))
                    .scroll_padding(1)
                    .direction(ListDirection::TopToBottom),
                list,
                &mut list_state,
            );
            let _ = List::default().direction(ListDirection::BottomToTop);
            let mut table_state = TableState::default().with_selected(Some(0));
            f.render_stateful_widget(
                Table::new(
                    [Row::new(["Mon", "ana"]), Row::new(["Tue", "bo"])],
                    [Constraint::Length(6), Constraint::Fill(1)],
                )
                .header(Row::new(["Day", "Who"]).style(Style::new().add_modifier(Modifier::BOLD)))
                .footer(Row::new(["2 days", ""]))
                .style(Style::new())
                .column_spacing(1)
                .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
                .highlight_symbol("* ")
                .highlight_spacing(HighlightSpacing::Always),
                table,
                &mut table_state,
            );

            // layers and popup: centred with Flex, cleared, then drawn over.
            let [column] = Layout::horizontal([Constraint::Length(20)])
                .flex(Flex::Center)
                .areas(bottom);
            let [popup] = Layout::vertical([Constraint::Length(3)])
                .flex(Flex::Center)
                .areas(column);
            f.render_widget(Clear, popup);
            f.render_widget(
                Block::bordered()
                    .border_type(BorderType::Double)
                    .title("Delete?"),
                popup,
            );
            for flex in [Flex::End, Flex::SpaceAround, Flex::SpaceEvenly, Flex::Start] {
                let _ = Layout::horizontal([Constraint::Fill(1)])
                    .flex(flex)
                    .split(Rect::new(0, 0, 4, 1));
            }
            if let Some(position) = cursor {
                f.set_cursor_position(position);
            }
        })
        .expect("draws");

    let screen = rows(&terminal).join("\n");
    for expected in [
        "Board",
        "3 cards",
        "plain styled",
        "Todo",
        "Doing",
        "half",
        "quarter",
        "typed",
        "yes",
        "> second",
        "Day",
        "* Mon",
        "2 days",
        "Delete?",
        "╭",
        "╔",
    ] {
        assert!(screen.contains(expected), "no {expected:?} in\n{screen}");
    }
    // A sparkline and a gauge drew something that is not text.
    assert!(screen.contains('█'), "{screen}");
    assert_eq!(
        cursor.map(|p| p.x),
        Some(5),
        "the input's cursor is a position bzz sets"
    );
}
