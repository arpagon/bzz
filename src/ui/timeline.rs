use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
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
    /// Measured terminal rows above the visible viewport. It is independent
    /// from selected_event so J/K and wheel input can inspect history without
    /// changing the active message.
    pub scroll: usize,
    pub viewport_height: usize,
    pub content_height: usize,
    pub keep_selection_visible: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageHit {
    pub event_id: String,
    pub area: Rect,
}
impl TimelineState {
    pub fn reconcile(&mut self, messages: &[Message]) {
        if self.at_live_bottom {
            self.selected_event = messages.last().map(|message| message.event_id.clone());
            self.newer = 0;
            self.keep_selection_visible = true;
        } else if let Some(selected) = &self.selected_event
            && !messages.iter().any(|message| &message.event_id == selected)
        {
            self.selected_event = messages.last().map(|message| message.event_id.clone());
            self.keep_selection_visible = true;
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
        self.keep_selection_visible = true;
    }

    pub fn scroll_by(&mut self, delta: isize) {
        let viewport = self.viewport_height.max(1);
        let max_scroll = self.content_height.saturating_sub(viewport);
        self.scroll = self.scroll.saturating_add_signed(delta).min(max_scroll);
        self.at_live_bottom = self.scroll == max_scroll;
        if self.at_live_bottom {
            self.newer = 0;
        }
        self.keep_selection_visible = false;
    }

    pub fn scroll_half_page(&mut self, direction: isize) {
        let amount = isize::try_from((self.viewport_height.max(2) / 2).max(1)).unwrap_or(1);
        self.scroll_by(amount.saturating_mul(direction));
    }

    fn reconcile_layout(
        &mut self,
        content_height: usize,
        viewport_height: usize,
        selected_top: Option<usize>,
        selected_bottom: Option<usize>,
    ) {
        self.content_height = content_height;
        self.viewport_height = viewport_height.max(1);
        let max_scroll = self.content_height.saturating_sub(self.viewport_height);
        if self.at_live_bottom {
            self.scroll = max_scroll;
            self.keep_selection_visible = false;
            return;
        }
        self.scroll = self.scroll.min(max_scroll);
        if !self.keep_selection_visible {
            return;
        }
        if let (Some(top), Some(bottom)) = (selected_top, selected_bottom) {
            if bottom.saturating_sub(top) >= self.viewport_height {
                self.scroll = top.min(max_scroll);
            } else if top < self.scroll {
                self.scroll = top;
            } else if bottom > self.scroll.saturating_add(self.viewport_height) {
                self.scroll = bottom.saturating_sub(self.viewport_height).min(max_scroll);
            }
        }
        self.keep_selection_visible = false;
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
    fn height(&self, width: u16) -> u16 {
        self.rows
            .iter()
            .map(|row| match row {
                TimelineRow::Text(line) => text_height(line, width),
                TimelineRow::Image(protocol) => protocol.size().height,
            })
            .fold(0_u16, u16::saturating_add)
    }
}

fn text_height(line: &Line<'_>, width: u16) -> u16 {
    let width = usize::from(width.max(1));
    let cells = line.width().max(1);
    u16::try_from(cells.div_ceil(width)).unwrap_or(u16::MAX)
}

/// Keeps conversation text to a readable measure on ultrawide terminals while
/// retaining the full pane border and a small left breathing space.
fn readable_area(inner: Rect, message_width: Option<u16>) -> Rect {
    let Some(max_width) = message_width else {
        return inner;
    };
    let width = inner.width.min(max_width.max(1));
    let left_padding = u16::from(width < inner.width);
    Rect::new(
        inner.x.saturating_add(left_padding),
        inner.y,
        width.min(inner.width.saturating_sub(left_padding)),
        inner.height,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    messages: &[Message],
    profiles: &HashMap<String, Profile>,
    reactions: &HashMap<String, Vec<Reaction>>,
    state: &mut TimelineState,
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
        None,
        None,
    );
}

/// Renders the timeline with a bounded readable text measure. The surrounding
/// pane remains full-width; only message content is constrained.
#[allow(clippy::too_many_arguments)]
pub fn render_limited(
    frame: &mut Frame<'_>,
    area: Rect,
    messages: &[Message],
    profiles: &HashMap<String, Profile>,
    reactions: &HashMap<String, Vec<Reaction>>,
    state: &mut TimelineState,
    title: &str,
    theme: &Theme,
    focused: bool,
    self_pubkey: Option<&str>,
    message_width: u16,
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
        None,
        Some(message_width),
    );
}

#[allow(clippy::too_many_arguments)]
pub fn render_with_media(
    frame: &mut Frame<'_>,
    area: Rect,
    messages: &[Message],
    profiles: &HashMap<String, Profile>,
    reactions: &HashMap<String, Vec<Reaction>>,
    state: &mut TimelineState,
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
        None,
        None,
    );
}

#[allow(clippy::too_many_arguments)]
pub fn render_with_media_and_hits(
    frame: &mut Frame<'_>,
    area: Rect,
    messages: &[Message],
    profiles: &HashMap<String, Profile>,
    reactions: &HashMap<String, Vec<Reaction>>,
    state: &mut TimelineState,
    title: &str,
    theme: &Theme,
    focused: bool,
    self_pubkey: Option<&str>,
    media: &mut MediaRuntime,
    hits: &mut Vec<MessageHit>,
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
        Some(hits),
        None,
    );
}

/// Equivalent to [`render_with_media_and_hits`] with a bounded message measure.
#[allow(clippy::too_many_arguments)]
pub fn render_with_media_and_hits_limited(
    frame: &mut Frame<'_>,
    area: Rect,
    messages: &[Message],
    profiles: &HashMap<String, Profile>,
    reactions: &HashMap<String, Vec<Reaction>>,
    state: &mut TimelineState,
    title: &str,
    theme: &Theme,
    focused: bool,
    self_pubkey: Option<&str>,
    media: &mut MediaRuntime,
    hits: &mut Vec<MessageHit>,
    message_width: u16,
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
        Some(hits),
        Some(message_width),
    );
}

#[allow(clippy::too_many_arguments)]
fn render_internal(
    frame: &mut Frame<'_>,
    area: Rect,
    messages: &[Message],
    profiles: &HashMap<String, Profile>,
    reactions: &HashMap<String, Vec<Reaction>>,
    state: &mut TimelineState,
    title: &str,
    theme: &Theme,
    focused: bool,
    self_pubkey: Option<&str>,
    mut media: Option<&mut MediaRuntime>,
    mut hits: Option<&mut Vec<MessageHit>>,
    message_width: Option<u16>,
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
    let content = readable_area(inner, message_width);
    if content.is_empty() {
        return;
    }

    let image_width = content.width.saturating_sub(4).max(2);
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
                messages.get(index.saturating_sub(1)).filter(|_| index > 0),
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
    let heights = blocks
        .iter()
        .map(|block| block.height(content.width))
        .collect::<Vec<_>>();
    let total = heights.iter().copied().fold(0_u32, |total, height| {
        total.saturating_add(u32::from(height))
    });
    let viewport = u32::from(content.height);
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
    state.reconcile_layout(
        usize::try_from(total).unwrap_or(usize::MAX),
        usize::try_from(viewport).unwrap_or(usize::MAX),
        selected_top.and_then(|value| usize::try_from(value).ok()),
        selected_bottom.and_then(|value| usize::try_from(value).ok()),
    );
    let scroll = u32::try_from(state.scroll).unwrap_or(u32::MAX);

    let mut global_y = 0_u32;
    for (index, block) in blocks.into_iter().enumerate() {
        let block_height = u32::from(block.height(content.width));
        if global_y + block_height <= scroll {
            global_y += block_height;
            continue;
        }
        if global_y >= scroll + viewport {
            break;
        }
        let visible_start = global_y.max(scroll);
        let visible_end = global_y.saturating_add(block_height).min(scroll + viewport);
        if let Some(hits) = &mut hits {
            let y = content.y.saturating_add(
                u16::try_from(visible_start.saturating_sub(scroll)).unwrap_or(u16::MAX),
            );
            let height =
                u16::try_from(visible_end.saturating_sub(visible_start)).unwrap_or(u16::MAX);
            (*hits).push(MessageHit {
                event_id: messages[index].event_id.clone(),
                area: Rect::new(content.x, y, content.width, height),
            });
        }
        let mut row_y = global_y;
        for row in block.rows {
            let row_height = match &row {
                TimelineRow::Text(line) => u32::from(text_height(line, content.width)),
                TimelineRow::Image(protocol) => u32::from(protocol.size().height),
            };
            if row_y + row_height > scroll && row_y < scroll + viewport {
                match row {
                    TimelineRow::Text(line) => {
                        let visible_top = row_y.max(scroll);
                        let visible_bottom = row_y
                            .saturating_add(row_height)
                            .min(scroll.saturating_add(viewport));
                        let skipped = visible_top.saturating_sub(row_y);
                        let y = content.y.saturating_add(
                            u16::try_from(visible_top.saturating_sub(scroll)).unwrap_or(u16::MAX),
                        );
                        let height = u16::try_from(visible_bottom.saturating_sub(visible_top))
                            .unwrap_or(u16::MAX);
                        if y < content.bottom() && height > 0 {
                            frame.render_widget(
                                Paragraph::new(line)
                                    .style(theme.style(if block.selected {
                                        HighlightGroup::SelectedRow
                                    } else {
                                        HighlightGroup::Normal
                                    }))
                                    .wrap(Wrap { trim: false })
                                    .scroll((u16::try_from(skipped).unwrap_or(u16::MAX), 0)),
                                Rect::new(content.x, y, content.width, height),
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
                        frame.render_widget(SlicedImage::new(protocol.as_ref(), position), content);
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
    previous: Option<&Message>,
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
    let author = sanitize::single_line(&author);
    let grouped = previous.is_some_and(|previous| same_message_group(previous, message));
    let mut rows = vec![];
    if previous.is_none_or(|previous| day_key(previous.created_at) != day_key(message.created_at)) {
        rows.push(TimelineRow::Text(Line::styled(
            format!("──── {} ────", format_day(message.created_at)),
            theme.style(HighlightGroup::MessageDateSeparator),
        )));
    }
    let mut header = vec![];
    if selected {
        header.push(Span::styled("▌", theme.style(HighlightGroup::SelectedRow)));
    } else {
        header.push(Span::raw(" "));
    }
    if grouped {
        header.push(Span::raw("    "));
        header.push(Span::styled(
            format_time(message.created_at),
            theme.style(HighlightGroup::MessageTimestamp),
        ));
    } else {
        header.push(Span::styled(
            format!("{} ", avatar_marker(&message.pubkey, &author)),
            theme.style(HighlightGroup::MessageAvatar),
        ));
        header.push(Span::styled(
            author,
            theme.style(HighlightGroup::MessageAuthor),
        ));
        header.push(Span::styled(
            format!("  {}", format_time(message.created_at)),
            theme.style(HighlightGroup::MessageTimestamp),
        ));
    }
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
    rows.push(TimelineRow::Text(Line::from(header)));
    if message.deleted {
        rows.push(TimelineRow::Text(Line::styled(
            "     message deleted",
            theme.style(HighlightGroup::MessageDeleted),
        )));
    } else {
        for line in markdown::render(&message.content, theme).lines {
            let mut spans = vec![Span::styled(
                "     ",
                theme.style(HighlightGroup::MessageBody),
            )];
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
        Span::styled("     ▣ ", theme.style(HighlightGroup::MediaBorder)),
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
    let mut spans = vec![Span::raw("     ")];
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

/// A compact, local-only author marker. It deliberately derives its shape
/// from the already-rendered public key and never follows a profile-picture
/// URL or starts I/O. The visible initial is merely a readable supplement; the
/// author label remains the identity-bearing text.
pub fn avatar_marker(pubkey: &str, author: &str) -> String {
    const SHAPES: [char; 4] = ['●', '◆', '■', '▲'];
    let hash = pubkey.bytes().fold(0_u8, |value, byte| {
        value.wrapping_mul(33).wrapping_add(byte)
    });
    let initial = author
        .chars()
        .find(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_uppercase())
        .or_else(|| {
            pubkey
                .chars()
                .find(|character| character.is_ascii_alphanumeric())
        })
        .unwrap_or('?');
    format!("[{}{initial}]", SHAPES[usize::from(hash) % SHAPES.len()])
}

fn same_message_group(previous: &Message, current: &Message) -> bool {
    previous.pubkey == current.pubkey
        && !previous.deleted
        && !current.deleted
        && day_key(previous.created_at) == day_key(current.created_at)
        && current.created_at.saturating_sub(previous.created_at) <= 5 * 60
}

fn local_time(timestamp: u64) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::from_unix_timestamp(i64::try_from(timestamp).ok()?)
        .ok()
        .map(|value| {
            value.to_offset(time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC))
        })
}

fn day_key(timestamp: u64) -> (i32, u16) {
    local_time(timestamp)
        .map(|value| (value.year(), value.ordinal()))
        .unwrap_or((0, 0))
}

fn format_day(timestamp: u64) -> String {
    local_time(timestamp)
        .map(|value| {
            format!(
                "{:04}-{:02}-{:02}",
                value.year(),
                u8::from(value.month()),
                value.day()
            )
        })
        .unwrap_or_else(|| "unknown date".into())
}

fn format_time(timestamp: u64) -> String {
    local_time(timestamp)
        .map(|value| format!("{:02}:{:02}", value.hour(), value.minute()))
        .unwrap_or_else(|| "--:--".into())
}

#[cfg(test)]
mod tests {
    use super::{readable_area, same_message_group};
    use crate::domain::Message;
    use ratatui::layout::Rect;
    use uuid::Uuid;

    fn message(pubkey: &str, created_at: u64) -> Message {
        Message {
            event_id: format!("{pubkey}-{created_at}"),
            channel_id: Uuid::nil(),
            pubkey: pubkey.into(),
            created_at,
            content: String::new(),
            attachments: vec![],
            root_event_id: None,
            parent_event_id: None,
            deleted: false,
            pending: false,
            rejected: None,
        }
    }

    #[test]
    fn readable_area_keeps_the_pane_but_bounds_message_measure() {
        let area = readable_area(Rect::new(4, 2, 180, 10), Some(110));
        assert_eq!(area, Rect::new(5, 2, 110, 10));
        assert_eq!(readable_area(Rect::new(0, 0, 80, 5), Some(110)).width, 80);
    }

    #[test]
    fn same_author_messages_group_only_within_a_short_same_day_run() {
        let first = message("a", 1_700_000_000);
        assert!(same_message_group(&first, &message("a", 1_700_000_299)));
        assert!(!same_message_group(&first, &message("a", 1_700_000_301)));
        assert!(!same_message_group(&first, &message("b", 1_700_000_100)));
    }
}
