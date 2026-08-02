use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use ratatui_image::sliced::{SignedPosition, SlicedImage, SlicedProtocol};

use crate::{
    domain::{Message, Profile, Reaction},
    media::{
        MediaKind,
        model::human_size,
        runtime::{MediaRuntime, MediaState},
    },
    render::{markdown, sanitize},
    ui::theme::{BorderSurface, HighlightGroup, Theme},
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TimelineState {
    pub selected_event: Option<String>,
    pub at_live_bottom: bool,
    pub newer: usize,
}
impl TimelineState {
    pub fn reconcile(&mut self, messages: &[Message]) {
        if self.at_live_bottom {
            self.selected_event = messages.last().map(|message| message.event_id.clone());
            self.newer = 0;
        } else if let Some(selected) = &self.selected_event
            && !messages.iter().any(|message| &message.event_id == selected)
        {
            self.selected_event = messages.last().map(|message| message.event_id.clone());
        }
    }
    pub fn selected_index(&self, messages: &[Message]) -> Option<usize> {
        self.selected_event
            .as_ref()
            .and_then(|id| messages.iter().position(|message| &message.event_id == id))
    }
    pub fn move_by(&mut self, messages: &[Message], delta: isize) {
        if messages.is_empty() {
            return;
        }
        let current = self.selected_index(messages).unwrap_or(messages.len() - 1);
        let next = current.saturating_add_signed(delta).min(messages.len() - 1);
        self.selected_event = Some(messages[next].event_id.clone());
        self.at_live_bottom = next == messages.len() - 1;
    }
}

enum TimelineRow {
    Text(Line<'static>),
    Image(Arc<SlicedProtocol>),
}

struct MessageBlock {
    rows: Vec<TimelineRow>,
    selected: bool,
}

impl MessageBlock {
    fn height(&self) -> u16 {
        self.rows
            .iter()
            .map(|row| match row {
                TimelineRow::Text(_) => 1,
                TimelineRow::Image(protocol) => protocol.size().height,
            })
            .fold(0_u16, u16::saturating_add)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    messages: &[Message],
    profiles: &HashMap<String, Profile>,
    reactions: &HashMap<String, Vec<Reaction>>,
    state: &TimelineState,
    title: &str,
    theme: &Theme,
    focused: bool,
    self_pubkey: Option<&str>,
) {
    render_internal(
        frame,
        area,
        messages,
        profiles,
        reactions,
        state,
        title,
        theme,
        focused,
        self_pubkey,
        None,
    );
}

#[allow(clippy::too_many_arguments)]
pub fn render_with_media(
    frame: &mut Frame<'_>,
    area: Rect,
    messages: &[Message],
    profiles: &HashMap<String, Profile>,
    reactions: &HashMap<String, Vec<Reaction>>,
    state: &TimelineState,
    title: &str,
    theme: &Theme,
    focused: bool,
    self_pubkey: Option<&str>,
    media: &mut MediaRuntime,
) {
    render_internal(
        frame,
        area,
        messages,
        profiles,
        reactions,
        state,
        title,
        theme,
        focused,
        self_pubkey,
        Some(media),
    );
}

#[allow(clippy::too_many_arguments)]
fn render_internal(
    frame: &mut Frame<'_>,
    area: Rect,
    messages: &[Message],
    profiles: &HashMap<String, Profile>,
    reactions: &HashMap<String, Vec<Reaction>>,
    state: &TimelineState,
    title: &str,
    theme: &Theme,
    focused: bool,
    self_pubkey: Option<&str>,
    mut media: Option<&mut MediaRuntime>,
) {
    let border_group = if focused {
        HighlightGroup::FocusedPaneBorder
    } else {
        HighlightGroup::PaneBorder
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(theme.border_type(BorderSurface::Pane))
        .border_style(theme.style(border_group))
        .title_style(theme.style(HighlightGroup::PaneTitle))
        .title(format!(" {} ", sanitize::single_line(title)));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.is_empty() {
        return;
    }

    let image_width = inner.width.saturating_sub(4).max(2);
    let selected_index = state.selected_index(messages);
    if let Some(runtime) = media.as_deref_mut() {
        let center = selected_index.unwrap_or_else(|| messages.len().saturating_sub(1));
        let start = center.saturating_sub(32);
        let end = (center + 33).min(messages.len());
        for message in &messages[start..end] {
            for attachment in &message.attachments {
                runtime.request_inline(attachment, image_width, false);
            }
        }
    }

    let blocks = messages
        .iter()
        .enumerate()
        .map(|(index, message)| {
            message_block(
                message,
                profiles,
                reactions,
                theme,
                self_pubkey,
                selected_index == Some(index),
                media.as_deref(),
                image_width,
            )
        })
        .collect::<Vec<_>>();
    let heights = blocks.iter().map(MessageBlock::height).collect::<Vec<_>>();
    let total = heights.iter().copied().fold(0_u32, |total, height| {
        total.saturating_add(u32::from(height))
    });
    let viewport = u32::from(inner.height);
    let selected_bottom = selected_index.map(|selected| {
        heights
            .iter()
            .take(selected + 1)
            .copied()
            .fold(0_u32, |total, height| {
                total.saturating_add(u32::from(height))
            })
    });
    let selected_top = selected_index.map(|selected| {
        heights
            .iter()
            .take(selected)
            .copied()
            .fold(0_u32, |total, height| {
                total.saturating_add(u32::from(height))
            })
    });
    let scroll = if state.at_live_bottom {
        total.saturating_sub(viewport)
    } else if let (Some(top), Some(bottom)) = (selected_top, selected_bottom) {
        if bottom.saturating_sub(top) >= viewport {
            top
        } else {
            bottom.saturating_sub(viewport).min(top)
        }
    } else {
        total.saturating_sub(viewport)
    };

    let mut global_y = 0_u32;
    for block in blocks {
        let block_height = u32::from(block.height());
        if global_y + block_height <= scroll {
            global_y += block_height;
            continue;
        }
        if global_y >= scroll + viewport {
            break;
        }
        let mut row_y = global_y;
        for row in block.rows {
            let row_height = match &row {
                TimelineRow::Text(_) => 1,
                TimelineRow::Image(protocol) => u32::from(protocol.size().height),
            };
            if row_y + row_height > scroll && row_y < scroll + viewport {
                match row {
                    TimelineRow::Text(line) => {
                        let y = inner.y
                            + u16::try_from(row_y.saturating_sub(scroll)).unwrap_or(u16::MAX);
                        if y < inner.bottom() {
                            frame.render_widget(
                                Paragraph::new(line).style(theme.style(if block.selected {
                                    HighlightGroup::SelectedRow
                                } else {
                                    HighlightGroup::Normal
                                })),
                                Rect::new(inner.x, y, inner.width, 1),
                            );
                        }
                    }
                    TimelineRow::Image(protocol) => {
                        let relative_y = i32::try_from(row_y).unwrap_or(i32::MAX)
                            - i32::try_from(scroll).unwrap_or(i32::MAX);
                        let position = SignedPosition::from((
                            2,
                            relative_y.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                        ));
                        frame.render_widget(SlicedImage::new(protocol.as_ref(), position), inner);
                    }
                }
            }
            row_y += row_height;
        }
        global_y += block_height;
    }
}

#[allow(clippy::too_many_arguments)]
fn message_block(
    message: &Message,
    profiles: &HashMap<String, Profile>,
    reactions: &HashMap<String, Vec<Reaction>>,
    theme: &Theme,
    self_pubkey: Option<&str>,
    selected: bool,
    media: Option<&MediaRuntime>,
    image_width: u16,
) -> MessageBlock {
    let author = profiles
        .get(&message.pubkey)
        .map(Profile::label)
        .unwrap_or_else(|| crate::domain::abbreviated_pubkey(&message.pubkey));
    let mut header = vec![];
    if selected {
        header.push(Span::styled("▌", theme.style(HighlightGroup::SelectedRow)));
    } else {
        header.push(Span::raw(" "));
    }
    header.push(Span::styled(
        sanitize::single_line(&author),
        theme.style(HighlightGroup::MessageAuthor),
    ));
    header.push(Span::styled(
        format!("  {}", format_time(message.created_at)),
        theme.style(HighlightGroup::MessageTimestamp),
    ));
    if message.deleted {
        header.push(Span::styled(
            " [deleted]",
            theme.style(HighlightGroup::MessageDeleted),
        ));
    } else if message.pending {
        header.push(Span::styled(
            " [pending]",
            theme.style(HighlightGroup::Pending),
        ));
    } else if message.rejected.is_some() {
        header.push(Span::styled(
            " [rejected]",
            theme.style(HighlightGroup::Rejected),
        ));
    }
    let mut rows = vec![TimelineRow::Text(Line::from(header))];
    if message.deleted {
        rows.push(TimelineRow::Text(Line::styled(
            "  message deleted",
            theme.style(HighlightGroup::MessageDeleted),
        )));
    } else {
        for line in markdown::render(&message.content, theme).lines {
            let mut spans = vec![Span::styled("  ", theme.style(HighlightGroup::MessageBody))];
            spans.extend(line.spans);
            rows.push(TimelineRow::Text(Line::from(spans)));
        }
        for attachment in &message.attachments {
            let state = media.and_then(|runtime| runtime.state(attachment, image_width));
            rows.push(TimelineRow::Text(attachment_line(attachment, state, theme)));
            if let Some(MediaState::Ready(protocol)) = state
                && attachment.kind == MediaKind::Image
                && !attachment.spoiler
            {
                rows.push(TimelineRow::Image(protocol.clone()));
            }
        }
        if let Some(line) = reaction_line(message, reactions, self_pubkey, theme) {
            rows.push(TimelineRow::Text(line));
        }
    }
    MessageBlock { rows, selected }
}

fn attachment_line(
    attachment: &crate::media::Attachment,
    state: Option<&MediaState>,
    theme: &Theme,
) -> Line<'static> {
    let (status, status_group) = if let Some(error) = &attachment.error {
        (
            format!("invalid: {}", sanitize::single_line(error)),
            HighlightGroup::MediaError,
        )
    } else if attachment.spoiler {
        (
            "spoiler — press p to reveal".into(),
            HighlightGroup::MediaWarning,
        )
    } else {
        match state {
            Some(MediaState::Loading) => ("loading".into(), HighlightGroup::MediaLoading),
            Some(MediaState::Ready(_)) => ("ready".into(), HighlightGroup::MediaMetadata),
            Some(MediaState::Failed(message)) => (
                format!("failed: {}", sanitize::single_line(message)),
                HighlightGroup::MediaError,
            ),
            None if attachment.kind == MediaKind::Image => {
                ("image".into(), HighlightGroup::MediaMetadata)
            }
            None => (
                "press p to preview or save".into(),
                HighlightGroup::MediaMetadata,
            ),
        }
    };
    Line::from(vec![
        Span::styled("  ▣ ", theme.style(HighlightGroup::MediaBorder)),
        Span::styled(
            sanitize::single_line(attachment.label()),
            theme.style(HighlightGroup::MessageBody),
        ),
        Span::styled(
            format!(
                "  {} · {} · {status}",
                attachment.mime,
                human_size(attachment.size)
            ),
            theme.style(status_group),
        ),
    ])
}

fn reaction_line(
    message: &Message,
    reactions: &HashMap<String, Vec<Reaction>>,
    self_pubkey: Option<&str>,
    theme: &Theme,
) -> Option<Line<'static>> {
    let active = reactions
        .get(&message.event_id)
        .into_iter()
        .flatten()
        .filter(|reaction| !reaction.deleted)
        .map(|reaction| (reaction.emoji.as_str(), reaction.pubkey.as_str()))
        .collect::<BTreeSet<_>>();
    let mut aggregate = BTreeMap::<&str, (usize, bool)>::new();
    for (emoji, author) in active {
        let value = aggregate.entry(emoji).or_default();
        value.0 += 1;
        value.1 |= self_pubkey == Some(author);
    }
    if aggregate.is_empty() {
        return None;
    }
    let mut spans = vec![Span::raw("  ")];
    for (index, (emoji, (count, own))) in aggregate.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            format!("{emoji} {count}"),
            theme.style(if own {
                HighlightGroup::SelfReaction
            } else {
                HighlightGroup::Reaction
            }),
        ));
    }
    Some(Line::from(spans))
}

fn format_time(timestamp: u64) -> String {
    time::OffsetDateTime::from_unix_timestamp(i64::try_from(timestamp).unwrap_or(0))
        .ok()
        .map(|value| {
            let local = value
                .to_offset(time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC));
            format!("{:02}:{:02}", local.hour(), local.minute())
        })
        .unwrap_or_else(|| "--:--".into())
}
