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
    domain::{Message, Profile, Reaction, SystemEvent, SystemEventKind, ThreadSummary},
    media::{
        MediaKind,
        model::human_size,
        runtime::{MediaRuntime, MediaState},
    },
    render::{markdown, sanitize},
    store::agents::RemoteAgentView,
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
    /// Anchor for an explicit local copy range. It is an event ID, never a row
    /// coordinate, so asynchronous arrivals and wrapping cannot change what a
    /// user selected.
    pub copy_anchor: Option<String>,
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
        if self
            .copy_anchor
            .as_ref()
            .is_some_and(|anchor| !messages.iter().any(|message| &message.event_id == anchor))
        {
            self.copy_anchor = None;
        }
    }

    /// Starts a local range at the current selected event, or cancels an
    /// existing range. It never changes timeline selection or read state.
    pub fn toggle_copy_selection(&mut self, messages: &[Message]) -> Option<usize> {
        if self.copy_anchor.take().is_some() {
            return Some(0);
        }
        let index = self.selected_index(messages)?;
        self.copy_anchor = Some(messages[index].event_id.clone());
        Some(1)
    }

    /// Inclusive bounds in the current stable timeline sequence for the local
    /// copy range. With no anchor, they describe the selected one-message
    /// range and allocate nothing during ordinary rendering.
    pub fn copy_bounds(&self, messages: &[Message]) -> Option<(usize, usize)> {
        let selected = self.selected_index(messages)?;
        let anchor = self
            .copy_anchor
            .as_ref()
            .and_then(|event_id| {
                messages
                    .iter()
                    .position(|message| &message.event_id == event_id)
            })
            .unwrap_or(selected);
        Some(if anchor <= selected {
            (anchor, selected)
        } else {
            (selected, anchor)
        })
    }

    /// Indexes in the current stable timeline sequence included by the local
    /// copy range. This allocating helper is used only by the explicit copy
    /// action; renderers use [`Self::copy_bounds`].
    pub fn copy_indexes(&self, messages: &[Message]) -> Vec<usize> {
        self.copy_bounds(messages)
            .map(|(start, end)| (start..=end).collect())
            .unwrap_or_default()
    }

    pub fn selected_index(&self, messages: &[Message]) -> Option<usize> {
        self.selected_event
            .as_ref()
            .and_then(|id| messages.iter().position(|message| &message.event_id == id))
    }

    /// Whether every measured row is currently visible at the newest edge.
    /// A detached message selection intentionally clears `at_live_bottom`, but
    /// when the entire timeline fits in the viewport there is no hidden newer
    /// content and it is safe to acknowledge the visible channel as read.
    pub const fn visible_at_live_edge(&self) -> bool {
        self.at_live_bottom
            || (self.viewport_height > 0 && self.content_height <= self.viewport_height)
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

const AVATAR_WIDTH: u16 = 4;
/// One selection cell, a four-cell profile photo, and one breathing cell.
/// Body text shares this alignment whether a photograph is ready or the
/// textual author marker is the fallback.
const MESSAGE_TEXT_INDENT: u16 = 6;

enum TimelineRow {
    Text(Line<'static>),
    Indented { line: Line<'static>, indent: u16 },
    Image(Arc<SlicedProtocol>),
}

struct AvatarPlacement {
    protocol: Arc<SlicedProtocol>,
    /// Index of the first text row that the photograph accompanies. Date
    /// separators remain full width; the author header and body form the
    /// adjacent text column.
    before_row: usize,
}

struct MessageBlock {
    rows: Vec<TimelineRow>,
    avatar: Option<AvatarPlacement>,
    /// Rows before the actual message (currently only a date separator) do not
    /// participate in the compact selection gutter.
    selection_before_row: usize,
    selected: bool,
    copy_selected: bool,
}

impl MessageBlock {
    fn row_height(row: &TimelineRow, width: u16) -> u16 {
        match row {
            TimelineRow::Text(line) => text_height(line, width),
            TimelineRow::Indented { line, indent } => {
                text_height(line, width.saturating_sub(*indent).max(1))
            }
            TimelineRow::Image(protocol) => protocol.size().height,
        }
    }

    fn avatar_top(&self, width: u16) -> Option<u16> {
        let avatar = self.avatar.as_ref()?;
        Some(
            self.rows
                .iter()
                .take(avatar.before_row)
                .map(|row| Self::row_height(row, width))
                .fold(0_u16, u16::saturating_add),
        )
    }

    fn height(&self, width: u16) -> u16 {
        let text_height = self
            .rows
            .iter()
            .map(|row| Self::row_height(row, width))
            .fold(0_u16, u16::saturating_add);
        self.avatar.as_ref().map_or(text_height, |avatar| {
            text_height.max(
                self.avatar_top(width)
                    .unwrap_or_default()
                    .saturating_add(avatar.protocol.size().height),
            )
        })
    }

    fn selection_top(&self, width: u16) -> u16 {
        self.rows
            .iter()
            .take(self.selection_before_row)
            .map(|row| Self::row_height(row, width))
            .fold(0_u16, u16::saturating_add)
    }
}

fn text_height(line: &Line<'_>, width: u16) -> u16 {
    let width = width.max(1);
    // The overwhelmingly common one-row case needs no reflow allocation.
    // Longer lines use the same Paragraph/Wrap implementation as rendering:
    // dividing total display width undercounts when a word moves to the next
    // row and desynchronizes scroll coordinates.
    if line.width() <= usize::from(width) {
        return 1;
    }
    let count = Paragraph::new(line.clone())
        .wrap(Wrap { trim: false })
        .line_count(width);
    u16::try_from(count.max(1)).unwrap_or(u16::MAX)
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
    thread_summaries: &HashMap<String, ThreadSummary>,
    profiles: &HashMap<String, Profile>,
    agents: &HashMap<String, RemoteAgentView>,
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
        thread_summaries,
        profiles,
        agents,
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
    thread_summaries: &HashMap<String, ThreadSummary>,
    profiles: &HashMap<String, Profile>,
    agents: &HashMap<String, RemoteAgentView>,
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
        thread_summaries,
        profiles,
        agents,
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
    thread_summaries: &HashMap<String, ThreadSummary>,
    profiles: &HashMap<String, Profile>,
    agents: &HashMap<String, RemoteAgentView>,
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
        thread_summaries,
        profiles,
        agents,
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
    thread_summaries: &HashMap<String, ThreadSummary>,
    profiles: &HashMap<String, Profile>,
    agents: &HashMap<String, RemoteAgentView>,
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
        thread_summaries,
        profiles,
        agents,
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
    thread_summaries: &HashMap<String, ThreadSummary>,
    profiles: &HashMap<String, Profile>,
    agents: &HashMap<String, RemoteAgentView>,
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
        thread_summaries,
        profiles,
        agents,
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
    thread_summaries: &HashMap<String, ThreadSummary>,
    profiles: &HashMap<String, Profile>,
    agents: &HashMap<String, RemoteAgentView>,
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

    // Keep wrapped body text within the six-cell avatar/author gutter.
    let image_width = content.width.saturating_sub(5).max(2);
    let selected_index = state.selected_index(messages);
    if let Some(runtime) = media.as_deref_mut() {
        let center = selected_index.unwrap_or_else(|| messages.len().saturating_sub(1));
        let start = center.saturating_sub(32);
        let end = (center + 33).min(messages.len());
        for message in &messages[start..end] {
            for attachment in &message.attachments {
                runtime.request_inline(attachment, image_width, false);
            }
            if message.system.is_none()
                && let Some(picture) = profiles
                    .get(&message.pubkey)
                    .and_then(|profile| profile.picture.as_deref())
            {
                runtime.request_avatar(&message.pubkey, picture, AVATAR_WIDTH);
            }
        }
    }

    let copy_bounds = state
        .copy_anchor
        .as_ref()
        .and_then(|_| state.copy_bounds(messages));
    let blocks = messages
        .iter()
        .enumerate()
        .map(|(index, message)| {
            message_block(
                message,
                messages.get(index.saturating_sub(1)).filter(|_| index > 0),
                thread_summaries.get(&message.event_id),
                profiles,
                agents,
                reactions,
                theme,
                self_pubkey,
                selected_index == Some(index),
                copy_bounds.is_some_and(|(start, end)| (start..=end).contains(&index)),
                media.as_deref(),
                image_width,
                AVATAR_WIDTH,
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
        let avatar = block.avatar.as_ref().and_then(|avatar| {
            block
                .avatar_top(content.width)
                .map(|offset| (avatar.protocol.clone(), offset))
        });
        let marker_offset = block.selection_top(content.width);
        let mut row_y = global_y;
        for row in block.rows {
            let row_height = u32::from(MessageBlock::row_height(&row, content.width));
            if row_y + row_height > scroll && row_y < scroll + viewport {
                match row {
                    TimelineRow::Text(line) | TimelineRow::Indented { line, indent: 0 } => {
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
                                    .style(theme.style(HighlightGroup::Normal))
                                    .wrap(Wrap { trim: false })
                                    .scroll((u16::try_from(skipped).unwrap_or(u16::MAX), 0)),
                                Rect::new(content.x, y, content.width, height),
                            );
                        }
                    }
                    TimelineRow::Indented { line, indent } => {
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
                        let width = content.width.saturating_sub(indent);
                        if y < content.bottom() && height > 0 && width > 0 {
                            frame.render_widget(
                                Paragraph::new(line)
                                    .style(theme.style(HighlightGroup::Normal))
                                    .wrap(Wrap { trim: false })
                                    .scroll((u16::try_from(skipped).unwrap_or(u16::MAX), 0)),
                                Rect::new(content.x.saturating_add(indent), y, width, height),
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
        // Render after the adjacent text rows: their indentation intentionally
        // writes blanks into the avatar gutter, and the Kitty placeholders
        // must be the last cells written there.
        if let Some((avatar, avatar_offset)) = avatar {
            let avatar_y = global_y.saturating_add(u32::from(avatar_offset));
            let avatar_height = u32::from(avatar.size().height);
            if avatar_y.saturating_add(avatar_height) > scroll && avatar_y < scroll + viewport {
                let relative_y = i32::try_from(avatar_y).unwrap_or(i32::MAX)
                    - i32::try_from(scroll).unwrap_or(i32::MAX);
                frame.render_widget(
                    SlicedImage::new(
                        avatar.as_ref(),
                        SignedPosition::from((
                            1,
                            relative_y.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                        )),
                    ),
                    content,
                );
            }
        }
        if block.selected || block.copy_selected {
            let marker_top = global_y.saturating_add(u32::from(marker_offset));
            let marker_bottom = global_y.saturating_add(block_height);
            let visible_top = marker_top.max(scroll);
            let visible_bottom = marker_bottom.min(scroll.saturating_add(viewport));
            if visible_bottom > visible_top {
                let y = content.y.saturating_add(
                    u16::try_from(visible_top.saturating_sub(scroll)).unwrap_or(u16::MAX),
                );
                let height =
                    u16::try_from(visible_bottom.saturating_sub(visible_top)).unwrap_or(u16::MAX);
                let marker = if block.selected { "▌" } else { "│" };
                let markers = std::iter::repeat_n(marker, usize::from(height))
                    .collect::<Vec<_>>()
                    .join("\n");
                frame.render_widget(
                    Paragraph::new(markers).style(theme.style(HighlightGroup::MessageSelected)),
                    Rect::new(content.x, y, 1, height),
                );
            }
        }
        global_y += block_height;
    }
}

#[allow(clippy::too_many_arguments)]
fn message_block(
    message: &Message,
    previous: Option<&Message>,
    thread_summary: Option<&ThreadSummary>,
    profiles: &HashMap<String, Profile>,
    agents: &HashMap<String, RemoteAgentView>,
    reactions: &HashMap<String, Vec<Reaction>>,
    theme: &Theme,
    self_pubkey: Option<&str>,
    selected: bool,
    copy_selected: bool,
    media: Option<&MediaRuntime>,
    image_width: u16,
    avatar_width: u16,
) -> MessageBlock {
    if let Some(system) = &message.system {
        return system_message_block(
            message,
            previous,
            system,
            profiles,
            theme,
            self_pubkey,
            selected,
            copy_selected,
        );
    }
    let author = profiles
        .get(&message.pubkey)
        .map(Profile::label)
        .unwrap_or_else(|| crate::domain::abbreviated_pubkey(&message.pubkey));
    let author = sanitize::single_line(&author);
    let agent = agents.get(&message.pubkey);
    let agent_marker = agent.map(|agent| if agent.stale { "◇ " } else { "◆ " });
    let owner_presentation = agent.filter(|agent| !agent.stale).map(|agent| {
        if self_pubkey.is_some_and(|pubkey| agent.owner_pubkey.eq_ignore_ascii_case(pubkey)) {
            " · managed by you".to_owned()
        } else {
            let owner = profiles
                .get(&agent.owner_pubkey)
                .map(Profile::label)
                .unwrap_or_else(|| crate::domain::abbreviated_pubkey(&agent.owner_pubkey));
            format!(" · owned by {}", sanitize::single_line(&owner))
        }
    });
    let grouped = previous.is_some_and(|previous| same_message_group(previous, message));
    let avatar = (!grouped)
        .then(|| {
            profiles
                .get(&message.pubkey)
                .and_then(|profile| profile.picture.as_deref())
                .and_then(|picture| {
                    media.and_then(|runtime| {
                        runtime.avatar_state(&message.pubkey, picture, avatar_width)
                    })
                })
                .and_then(|state| match state {
                    MediaState::Ready(protocol) => Some(protocol.clone()),
                    MediaState::Loading | MediaState::Failed(_) => None,
                })
        })
        .flatten();
    let mut rows = vec![];
    if previous.is_none_or(|previous| day_key(previous.created_at) != day_key(message.created_at)) {
        rows.push(TimelineRow::Text(Line::styled(
            format!("──── {} ────", format_day(message.created_at)),
            theme.style(HighlightGroup::MessageDateSeparator),
        )));
    }
    // Date separators remain full width. The compact selection gutter and the
    // optional avatar begin beside the actual message header.
    let selection_before_row = rows.len();
    let avatar_before_row = rows.len();
    let mut body_lines = if message.deleted {
        Vec::new().into_iter()
    } else {
        markdown::render_with_width(&message.content, theme, image_width.saturating_sub(1))
            .lines
            .into_iter()
    };
    let grouped_body = grouped.then(|| body_lines.next()).flatten();
    let mut header = vec![];
    if grouped {
        header.push(Span::styled(
            format_time(message.created_at),
            theme.style(HighlightGroup::MessageTimestamp),
        ));
    } else {
        // The first cell is reserved for the message-scoped selection gutter.
        header.push(Span::raw(" "));
        if avatar.is_some() {
            header.push(Span::raw("     "));
        } else {
            header.push(Span::styled(
                format!("{} ", avatar_marker(&message.pubkey, &author)),
                theme.style(HighlightGroup::MessageAvatar),
            ));
        }
        if let Some(marker) = agent_marker {
            header.push(Span::styled(
                marker,
                theme.style(HighlightGroup::SelectionMarker),
            ));
        }
        header.push(Span::styled(
            author,
            theme.style(HighlightGroup::MessageAuthor),
        ));
        if let Some(owner) = owner_presentation {
            header.push(Span::styled(
                owner,
                theme.style(HighlightGroup::MessageTimestamp),
            ));
        }
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
    } else {
        match message.delivery {
            crate::domain::DeliveryState::Pending => header.push(Span::styled(
                " [pending]",
                theme.style(HighlightGroup::Pending),
            )),
            crate::domain::DeliveryState::Unknown => header.push(Span::styled(
                " [delivery unknown]",
                theme.style(HighlightGroup::Pending),
            )),
            crate::domain::DeliveryState::Rejected => header.push(Span::styled(
                " [rejected]",
                theme.style(HighlightGroup::Rejected),
            )),
            crate::domain::DeliveryState::Delivered => {}
        }
    }
    if let Some(line) = grouped_body {
        header.push(Span::raw("  "));
        header.extend(line.spans);
    }
    let header = Line::from(header);
    if grouped {
        rows.push(TimelineRow::Indented {
            line: header,
            indent: MESSAGE_TEXT_INDENT,
        });
    } else {
        rows.push(TimelineRow::Text(header));
    }
    if !message.deleted && message.delivery == crate::domain::DeliveryState::Unknown {
        rows.push(TimelineRow::Indented {
            line: Line::styled(
                "use :reconnect or bzz diagnostics outbox",
                theme.style(HighlightGroup::Pending),
            ),
            indent: MESSAGE_TEXT_INDENT,
        });
    }
    if message.deleted {
        rows.push(TimelineRow::Indented {
            line: Line::styled(
                "message deleted",
                theme.style(HighlightGroup::MessageDeleted),
            ),
            indent: MESSAGE_TEXT_INDENT,
        });
    } else {
        for line in body_lines {
            rows.push(TimelineRow::Indented {
                line,
                indent: MESSAGE_TEXT_INDENT,
            });
        }
        for attachment in &message.attachments {
            let state = media.and_then(|runtime| runtime.state(attachment, image_width));
            rows.push(TimelineRow::Indented {
                line: attachment_line(attachment, state, theme),
                indent: MESSAGE_TEXT_INDENT,
            });
            if let Some(MediaState::Ready(protocol)) = state
                && attachment.kind == MediaKind::Image
                && !attachment.spoiler
            {
                rows.push(TimelineRow::Image(protocol.clone()));
            }
        }
        if let Some(summary) = thread_summary
            && summary.descendant_count > 0
        {
            rows.push(TimelineRow::Indented {
                line: thread_summary_line(summary, nostr::Timestamp::now().as_secs(), theme),
                indent: MESSAGE_TEXT_INDENT,
            });
        }
        if let Some(line) = reaction_line(message, reactions, self_pubkey, theme) {
            rows.push(TimelineRow::Indented {
                line,
                indent: MESSAGE_TEXT_INDENT,
            });
        }
    }
    MessageBlock {
        rows,
        avatar: avatar.map(|protocol| AvatarPlacement {
            protocol,
            before_row: avatar_before_row,
        }),
        selection_before_row,
        selected,
        copy_selected,
    }
}

#[allow(clippy::too_many_arguments)]
fn system_message_block(
    message: &Message,
    previous: Option<&Message>,
    system: &SystemEvent,
    profiles: &HashMap<String, Profile>,
    theme: &Theme,
    self_pubkey: Option<&str>,
    selected: bool,
    copy_selected: bool,
) -> MessageBlock {
    let mut rows = Vec::new();
    if previous.is_none_or(|previous| day_key(previous.created_at) != day_key(message.created_at)) {
        rows.push(TimelineRow::Text(Line::styled(
            format!("──── {} ────", format_day(message.created_at)),
            theme.style(HighlightGroup::MessageDateSeparator),
        )));
    }
    let selection_before_row = rows.len();
    let summary = if message.deleted {
        "System event removed".to_owned()
    } else {
        system_summary(system, profiles, self_pubkey)
    };
    rows.push(TimelineRow::Text(Line::from(vec![
        Span::raw(" "),
        Span::styled(
            format!("  · {summary} · "),
            theme.style(HighlightGroup::MessageTimestamp),
        ),
        Span::styled(
            format_time(message.created_at),
            theme.style(HighlightGroup::MessageTimestamp),
        ),
    ])));
    MessageBlock {
        rows,
        avatar: None,
        selection_before_row,
        selected,
        copy_selected,
    }
}

pub(crate) fn system_summary(
    system: &SystemEvent,
    profiles: &HashMap<String, Profile>,
    self_pubkey: Option<&str>,
) -> String {
    let label = |pubkey: &str| {
        sanitize::single_line(
            &profiles
                .get(pubkey)
                .map(Profile::label)
                .unwrap_or_else(|| crate::domain::abbreviated_pubkey(pubkey)),
        )
    };
    let actor = system.actor.as_deref().map(&label);
    let target = system.target.as_deref().map(&label);
    match system.kind {
        SystemEventKind::DmCreated => {
            let mut participants = system
                .participants
                .iter()
                .filter(|pubkey| {
                    self_pubkey.is_none_or(|self_pubkey| !pubkey.eq_ignore_ascii_case(self_pubkey))
                })
                .map(|pubkey| label(pubkey))
                .collect::<Vec<_>>();
            if participants.is_empty() {
                participants = system
                    .participants
                    .iter()
                    .map(|pubkey| label(pubkey))
                    .collect();
            }
            format!("Direct message started with {}", participants.join(", "))
        }
        SystemEventKind::ChannelCreated => actor.map_or_else(
            || "Channel created".to_owned(),
            |actor| format!("Channel created by {actor}"),
        ),
        SystemEventKind::MemberJoined => match (target, actor) {
            (Some(target), Some(actor)) if target != actor => {
                format!("{target} joined · added by {actor}")
            }
            (Some(target), _) => format!("{target} joined"),
            _ => "A member joined".to_owned(),
        },
        SystemEventKind::MemberLeft => target.or(actor).map_or_else(
            || "A member left".to_owned(),
            |target| format!("{target} left"),
        ),
        SystemEventKind::MemberRemoved => match (target, actor) {
            (Some(target), Some(actor)) => format!("{target} was removed by {actor}"),
            (Some(target), None) => format!("{target} was removed"),
            _ => "A member was removed".to_owned(),
        },
        SystemEventKind::ChannelArchived => actor.map_or_else(
            || "Channel archived".to_owned(),
            |actor| format!("Channel archived by {actor}"),
        ),
        SystemEventKind::ChannelUnarchived => actor.map_or_else(
            || "Channel restored".to_owned(),
            |actor| format!("Channel restored by {actor}"),
        ),
        SystemEventKind::MessageDeleted => actor.map_or_else(
            || "A message was deleted".to_owned(),
            |actor| format!("A message was deleted by {actor}"),
        ),
        SystemEventKind::Unsupported => "Unsupported system event".to_owned(),
    }
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
        Span::styled("▣ ", theme.style(HighlightGroup::MediaBorder)),
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

fn thread_summary_line(summary: &ThreadSummary, now: u64, theme: &Theme) -> Line<'static> {
    let count = summary.descendant_count;
    let label = if count == 1 { "reply" } else { "replies" };
    let mut spans = vec![
        Span::styled("↳ ", theme.style(HighlightGroup::SelectionMarker)),
        Span::styled(
            format!("{count} {label}"),
            theme.style(HighlightGroup::MessageBody),
        ),
    ];
    if let Some(last_reply_at) = summary.last_reply_at {
        spans.push(Span::styled(
            format!(
                " · last reply {}",
                format_relative_reply_time(last_reply_at, now)
            ),
            theme.style(HighlightGroup::MessageTimestamp),
        ));
    }
    Line::from(spans)
}

pub(crate) fn format_relative_reply_time(timestamp: u64, now: u64) -> String {
    let elapsed = now.saturating_sub(timestamp);
    if elapsed < 60 {
        "just now".into()
    } else if elapsed < 60 * 60 {
        relative_unit(elapsed / 60, "minute")
    } else if elapsed < 24 * 60 * 60 {
        relative_unit(elapsed / (60 * 60), "hour")
    } else if elapsed < 7 * 24 * 60 * 60 {
        relative_unit(elapsed / (24 * 60 * 60), "day")
    } else {
        format!("on {}", format_day(timestamp))
    }
}

fn relative_unit(value: u64, unit: &str) -> String {
    format!("{value} {unit}{} ago", if value == 1 { "" } else { "s" })
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
    let mut spans = vec![];
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
    previous.system.is_none()
        && current.system.is_none()
        && previous.pubkey == current.pubkey
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
    use std::{collections::HashMap, sync::Arc};

    use super::{
        AvatarPlacement, MESSAGE_TEXT_INDENT, MessageBlock, TimelineRow, TimelineState,
        format_relative_reply_time, message_block, readable_area, same_message_group, text_height,
    };
    use crate::{domain::Message, ui::theme::Theme};
    use ratatui::layout::Rect;
    use ratatui_image::sliced::SlicedProtocol;
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
            delivery: crate::domain::DeliveryState::Delivered,
            system: None,
        }
    }

    #[test]
    fn readable_area_keeps_the_pane_but_bounds_message_measure() {
        let area = readable_area(Rect::new(4, 2, 180, 10), Some(110));
        assert_eq!(area, Rect::new(5, 2, 110, 10));
        assert_eq!(readable_area(Rect::new(0, 0, 80, 5), Some(110)).width, 80);
    }

    #[test]
    fn copy_range_uses_event_ids_and_keeps_chronological_indexes() {
        let messages = vec![message("a", 1), message("b", 2), message("c", 3)];
        let mut state = TimelineState {
            selected_event: Some(messages[0].event_id.clone()),
            ..TimelineState::default()
        };
        assert_eq!(state.toggle_copy_selection(&messages), Some(1));
        state.selected_event = Some(messages[2].event_id.clone());
        assert_eq!(state.copy_indexes(&messages), vec![0, 1, 2]);
        assert_eq!(state.toggle_copy_selection(&messages), Some(0));
        assert_eq!(state.copy_indexes(&messages), vec![2]);
    }

    #[test]
    fn word_wrapped_rows_use_the_same_height_as_the_paragraph_renderer() {
        assert_eq!(
            text_height(&ratatui::text::Line::raw("12345 12345 12345"), 10),
            3
        );
    }

    #[test]
    fn avatar_shares_the_author_and_body_block_instead_of_inserting_a_row() {
        let picker = ratatui_image::picker::Picker::halfblocks();
        let protocol = Arc::new(
            SlicedProtocol::new_with_resize(
                &picker,
                image::DynamicImage::new_rgb8(4, 3),
                ratatui::layout::Size::new(4, 3),
                ratatui_image::Resize::Fit(None),
            )
            .unwrap(),
        );
        let avatar_height = protocol.size().height;
        let block = MessageBlock {
            rows: vec![
                TimelineRow::Text(ratatui::text::Line::raw("date")),
                TimelineRow::Text(ratatui::text::Line::raw("      author")),
                TimelineRow::Text(ratatui::text::Line::raw("      body")),
            ],
            avatar: Some(AvatarPlacement {
                protocol,
                before_row: 1,
            }),
            selection_before_row: 1,
            selected: false,
            copy_selected: false,
        };
        assert_eq!(block.avatar_top(80), Some(1));
        assert_eq!(block.height(80), (1 + avatar_height).max(3));
        assert!(block.height(80) < 3 + avatar_height);
    }

    #[test]
    fn grouped_message_keeps_timestamp_and_body_in_one_indented_row() {
        let mut first = message("a", 1_700_000_000);
        first.content = "first".into();
        let mut second = message("a", 1_700_000_100);
        second.content = "follow-up text".into();
        let block = message_block(
            &second,
            Some(&first),
            None,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &Theme::default(),
            None,
            true,
            false,
            None,
            40,
            4,
        );
        assert_eq!(block.rows.len(), 1);
        let TimelineRow::Indented { line, indent } = &block.rows[0] else {
            panic!("grouped message should use one hanging-indent row");
        };
        assert_eq!(*indent, MESSAGE_TEXT_INDENT);
        let text = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(text.contains("follow-up text"));
        assert_eq!(block.selection_top(40), 0);
    }

    #[test]
    fn body_rows_wrap_inside_the_message_gutter() {
        let mut value = message("a", 1_700_000_000);
        value.content = "one two three four five six seven eight nine ten".into();
        let block = message_block(
            &value,
            None,
            None,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &Theme::default(),
            None,
            true,
            false,
            None,
            18,
            4,
        );
        let body = block
            .rows
            .iter()
            .find_map(|row| match row {
                TimelineRow::Indented { line, indent } if line.width() > 0 => Some((line, *indent)),
                _ => None,
            })
            .expect("indented message body");
        assert_eq!(body.1, MESSAGE_TEXT_INDENT);
        assert!(text_height(body.0, 18 - MESSAGE_TEXT_INDENT) > 1);
        assert_eq!(
            block.selection_top(18),
            MessageBlock::row_height(&block.rows[0], 18)
        );
    }

    #[test]
    fn reply_activity_uses_expanded_relative_units_and_clamps_future_time() {
        let now = 1_700_000_000;
        assert_eq!(format_relative_reply_time(now + 1, now), "just now");
        assert_eq!(format_relative_reply_time(now - 60, now), "1 minute ago");
        assert_eq!(format_relative_reply_time(now - 120, now), "2 minutes ago");
        assert_eq!(format_relative_reply_time(now - 3_600, now), "1 hour ago");
        assert_eq!(format_relative_reply_time(now - 86_400, now), "1 day ago");
        assert!(format_relative_reply_time(now - 7 * 86_400, now).starts_with("on "));
    }

    #[test]
    fn same_author_messages_group_only_within_a_short_same_day_run() {
        let first = message("a", 1_700_000_000);
        assert!(same_message_group(&first, &message("a", 1_700_000_299)));
        assert!(!same_message_group(&first, &message("a", 1_700_000_301)));
        assert!(!same_message_group(&first, &message("b", 1_700_000_100)));
    }
}
