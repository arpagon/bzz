//! What moves on bzz's screen, and when the loop must next wake (plan
//! v0.17.0 L7, L8, L9).
//!
//! The animations are noticed while a frame is drawn: the focus moved, an
//! overlay opened or closed, a message arrived in the channel in view, an
//! unread count rose, a channel's history is loading. Each starts an
//! [`Anim`] on bzz's clock, is laid over the frame after the panes are
//! drawn, and says when it needs its next frame. The run loop sleeps until
//! the earliest of those instants and its own housekeeping; while nothing
//! moves, nothing asks for a frame (rule 20).

use std::{collections::HashMap, time::Duration};

use ratatui::{Frame, buffer::Buffer, buffer::Cell, layout::Rect, style::Color};
use tokio::time::Instant;
use uuid::Uuid;

use super::App;
use crate::clock::WallClock as _;
use crate::{
    config::Motion,
    ui::{
        frame::ColorMode,
        hit_map::{HitMap, HitTarget},
        layout::Panes,
        motion::{self, Anim, Ease, Role, scale},
        state::{FocusSurface, Overlay},
        theme::HighlightGroup,
        toast::Toasts,
    },
};

/// How often the loop's housekeeping runs while something it polls may
/// change (the old fixed tick).
pub(super) const HOUSEKEEPING_BUSY: Duration = Duration::from_millis(100);
/// How often it runs at rest: the store is read again each second.
pub(super) const HOUSEKEEPING_REST: Duration = Duration::from_secs(1);

/// The share of the screen above which an overlay does not unfold: a view
/// that takes the whole screen would cost a full repaint a frame.
const UNFOLD_AT_MOST: u32 = 60;

/// A message written longer ago than this is history when it comes, not
/// an arrival, and takes no tint.
const ARRIVAL_WINDOW_SECONDS: u64 = 60;

/// How far from the background towards the selection colour an arriving
/// message's tint starts, when it can blend.
const TINT_STRENGTH: f32 = 0.4;

/// The animations in progress and what they were noticed against.
#[derive(Debug, Default)]
pub(super) struct Stage {
    pub(super) toasts: Toasts,
    focus_seen: Option<FocusSurface>,
    sweep: Option<(FocusSurface, Anim)>,
    overlay_seen: Option<Overlay>,
    opening: Option<Anim>,
    /// The overlay's cells as the last frame drew them, kept to fold away.
    overlay_cells: Option<(Rect, Vec<Cell>)>,
    closing: Option<(Anim, Rect, Vec<Cell>)>,
    /// The channel in view and its last message, at the last frame.
    tail_seen: Option<(Uuid, String)>,
    /// An arriving message, its tint, and the colours it runs between
    /// (the same twice when it cannot blend).
    tints: Vec<(String, Anim, (Color, Color))>,
    counts_seen: Option<(Option<Uuid>, HashMap<Uuid, usize>)>,
    flashes: Vec<(Uuid, Anim)>,
    /// The channel whose history is being fetched, and since when.
    loading: Option<(Uuid, Instant)>,
}

impl Stage {
    /// When the animations need their next frame.
    pub(super) fn wake(&self, now: Instant, motion: Motion) -> Option<Instant> {
        let loading = self.loading.and_then(|(_, since)| {
            if !motion::plays(motion, Role::Decorates) {
                return None;
            }
            let shows_at = since + scale::FAST;
            let ends_at = since + scale::LOADING;
            if now < shows_at {
                Some(shows_at)
            } else if now < ends_at {
                Some((now + scale::FRAME).min(ends_at))
            } else {
                None
            }
        });
        motion::earliest(
            [
                self.toasts.wake(now),
                self.sweep.and_then(|(_, anim)| anim.wake(now)),
                self.opening.and_then(|anim| anim.wake(now)),
                self.closing.as_ref().and_then(|(anim, ..)| anim.wake(now)),
                loading,
            ]
            .into_iter()
            .chain(self.tints.iter().map(|(_, anim, _)| anim.wake(now)))
            .chain(self.flashes.iter().map(|(_, anim)| anim.wake(now))),
        )
    }

    /// Moves to `now`; answers whether something on screen changed.
    pub(super) fn advance(&mut self, now: Instant) -> bool {
        self.toasts.advance(now)
    }

    /// The history of `channel` is being fetched.
    pub(super) fn loading(&mut self, channel: Uuid, now: Instant) {
        self.loading = Some((channel, now));
    }

    /// The history fetch ended, one way or the other.
    pub(super) fn loaded(&mut self) {
        self.loading = None;
    }
}

/// When the loop last drew, ran its housekeeping and read the huddle's
/// levels, and so when each is next due. The loop sleeps until the earliest
/// (v0.17 L9): no fixed tick wakes it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Pacing {
    last_draw: Option<Instant>,
    last_tick: Option<Instant>,
    last_huddle: Option<Instant>,
    /// Something happened since the last housekeeping: it comes back within
    /// [`HOUSEKEEPING_BUSY`], as the old tick did.
    poked: bool,
}

/// A frame due this close to its time is drawn now: the sleep that waits
/// for it may wake a little early.
const SLACK: Duration = Duration::from_millis(2);

impl Pacing {
    pub(super) const fn new() -> Self {
        Self {
            last_draw: None,
            last_tick: None,
            last_huddle: None,
            poked: true,
        }
    }

    /// When a frame asked for may be drawn: at once, or one frame interval
    /// after the last, so a burst of events gives one frame.
    pub(super) fn frame_at(&self, pending: bool, now: Instant) -> Option<Instant> {
        pending.then(|| {
            self.last_draw
                .map_or(now, |last| (last + scale::FRAME).max(now))
        })
    }

    pub(super) fn frame_due(&self, now: Instant) -> bool {
        self.last_draw
            .is_none_or(|last| now + SLACK >= last + scale::FRAME)
    }

    pub(super) fn drawn(&mut self, now: Instant) {
        self.last_draw = Some(now);
    }

    /// When the huddle's levels are next read, while they are read at all.
    pub(super) fn huddle_at(&self, polled: bool, now: Instant) -> Option<Instant> {
        polled.then(|| {
            self.last_huddle
                .map_or(now, |last| (last + scale::FRAME).max(now))
        })
    }

    pub(super) fn huddle_due(&self, polled: bool, now: Instant) -> bool {
        polled
            && self
                .last_huddle
                .is_none_or(|last| now + SLACK >= last + scale::FRAME)
    }

    pub(super) fn huddle_read(&mut self, now: Instant) {
        self.last_huddle = Some(now);
    }

    /// When the housekeeping is next due. At rest it comes when the store's
    /// once-a-second refresh is due (`refresh_due`), never sooner than the
    /// busy interval, so that refresh is not missed by a hair and put off a
    /// second.
    pub(super) fn tick_at(&self, busy: bool, refresh_due: Instant, now: Instant) -> Instant {
        let Some(last) = self.last_tick else {
            return now;
        };
        if busy || self.poked {
            last + HOUSEKEEPING_BUSY
        } else {
            refresh_due
                .max(last + HOUSEKEEPING_BUSY)
                .min(last + HOUSEKEEPING_REST + HOUSEKEEPING_BUSY)
        }
    }

    pub(super) fn ticked(&mut self, now: Instant) {
        self.last_tick = Some(now);
        self.poked = false;
    }

    /// An input, a message from the relay or from a task came in.
    pub(super) fn poke(&mut self) {
        self.poked = true;
    }
}

#[cfg(test)]
thread_local! {
    /// Tests of the interface see the end state of every animation, as the
    /// scenarios do (`BZZ_NO_MOTION`); a test of motion sets its own.
    pub(super) static TEST_MOTION: std::cell::Cell<Option<Motion>> =
        const { std::cell::Cell::new(Some(Motion::Off)) };
}

/// Sleeps until `deadline`, or forever without one.
pub(super) async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

impl App {
    /// The motion setting in effect.
    pub(super) fn motion(&self) -> Motion {
        #[cfg(test)]
        if let Some(motion) = TEST_MOTION.with(std::cell::Cell::get) {
            return motion;
        }
        self.config.ui.motion()
    }

    /// Says a short confirmation in a toast, near where the eye is.
    pub(super) fn toast(&mut self, text: impl AsRef<str>) {
        let motion = self.motion();
        self.stage
            .toasts
            .push(text.as_ref(), Instant::now(), motion);
    }

    /// When the animations next need a frame.
    pub(super) fn motion_wake(&self, now: Instant) -> Option<Instant> {
        self.stage.wake(now, self.motion())
    }

    /// Whether the loop's housekeeping should come back within
    /// [`HOUSEKEEPING_BUSY`]: something it polls may change any moment.
    /// Otherwise it comes back after [`HOUSEKEEPING_REST`].
    pub(super) fn housekeeping_busy(&self) -> bool {
        self.media.busy()
            || self.cache_dirty
            || self.rate_limit_until.is_some()
            || !self.agent_typing.is_empty()
            || self.leader_started_at.is_some()
            || self.search_dirty_since.is_some()
            || self.dm_dirty_since.is_some()
            || self.local_agents.is_some()
            || !self.approvals.is_empty()
            || self.presentation.overlay == Some(Overlay::LocalAgents)
            || self.apps_running()
            || self
                .read_dirty_since
                .is_some_and(|since| since.elapsed() + HOUSEKEEPING_REST >= Duration::from_secs(5))
    }

    /// When the housekeeping's store refresh is due: a little after a
    /// second since the last (`on_tick` refreshes after more than one).
    pub(super) fn refresh_due(&self) -> Instant {
        Instant::from_std(self.last_cache_refresh) + HOUSEKEEPING_REST + Duration::from_millis(2)
    }

    /// Whether the huddle's levels are read at the frame rate: a call, the
    /// generated huddle, or the microphone check is on.
    pub(super) fn huddle_polled(&self) -> bool {
        self.huddle_viz.on || self.huddle_viz.mic_check.is_some() || self.huddle_feed().is_some()
    }

    /// Lays the animations over the drawn workspace: the focus sweep, the
    /// tint of arriving messages, the flash of a rising count, the loading
    /// shimmer and the toast. Called after the panes and before overlays.
    pub(super) fn stage_workspace(&mut self, frame: &mut Frame<'_>, panes: &Panes, hits: &HitMap) {
        let now = Instant::now();
        let motion = self.motion();
        let colour = self.theme.color_mode() != ColorMode::None;
        let buf = frame.buffer_mut();

        // The focus colour runs round the pane that takes the focus.
        let focus = self.presentation.focus;
        if self.stage.focus_seen.is_some_and(|seen| seen != focus) && colour {
            self.stage.sweep = Some((
                focus,
                Anim::new(now, scale::BASE, Ease::OutCubic, motion, Role::Decorates),
            ));
        }
        self.stage.focus_seen = Some(focus);
        if let Some((surface, anim)) = self.stage.sweep {
            let rect = match surface {
                FocusSurface::Communities => panes.community,
                FocusSurface::Channels => panes.sidebar,
                FocusSurface::Timeline => Some(panes.timeline),
                FocusSurface::Context => panes.thread,
                FocusSurface::App => panes.app,
                FocusSurface::InboxList | FocusSurface::InboxDetail => None,
            };
            if anim.running(now) && surface == focus {
                if let Some(rect) = rect {
                    let quiet = self.theme.style(HighlightGroup::PaneBorder);
                    motion::sweep(buf, rect, anim.progress(now), quiet);
                }
            } else {
                self.stage.sweep = None;
            }
        }

        // A message that arrives in the channel in view takes a tint.
        let channel = self.current_channel().map(|channel| channel.id);
        let tail = self.messages.last().map(|message| message.event_id.clone());
        if let (Some((seen_channel, seen_tail)), Some(channel), Some(tail)) =
            (&self.stage.tail_seen, channel, &tail)
            && *seen_channel == channel
            && seen_tail != tail
            && colour
            && let Some(from) = self
                .messages
                .iter()
                .position(|message| message.event_id == *seen_tail)
        {
            let me = self.self_pubkey().map(str::to_owned);
            // What a fetch brings from before is history, not an arrival.
            let recent = crate::clock::SystemClock
                .now_unix()
                .saturating_sub(ARRIVAL_WINDOW_SECONDS);
            let background = self.theme.style(HighlightGroup::Normal).bg;
            let tint = self
                .theme
                .style(HighlightGroup::Selection)
                .bg
                .or(self.theme.style(HighlightGroup::NewMessageMarker).fg);
            for message in &self.messages[from + 1..] {
                let shown = hits
                    .area_of(&HitTarget::TimelineMessage(message.event_id.clone()))
                    .is_some();
                if message.system.is_some()
                    || me.as_deref() == Some(message.pubkey.as_str())
                    || message.created_at < recent
                    || !shown
                {
                    continue;
                }
                let Some(tint) = tint else { continue };
                // A blend needs the theme's own background; without one
                // the tint holds and then goes, in one step.
                let pair = background
                    .and_then(|bg| motion::blend(bg, tint, TINT_STRENGTH).map(|start| (start, bg)));
                let anim = match pair {
                    Some(_) => Anim::new(now, scale::SLOW, Ease::OutCubic, motion, Role::Informs),
                    None => Anim::step(now, scale::SLOW, motion, Role::Informs),
                };
                self.stage.tints.push((
                    message.event_id.clone(),
                    anim,
                    pair.unwrap_or((tint, tint)),
                ));
            }
        }
        self.stage.tail_seen = channel.zip(tail);
        self.stage.tints.retain(|(_, anim, _)| anim.running(now));
        for (event, anim, pair) in &self.stage.tints {
            let (from, to) = *pair;
            let colour = if from == to {
                Some(from)
            } else {
                motion::blend(from, to, anim.progress(now))
            };
            if let (Some(colour), Some(area)) = (
                colour,
                hits.area_of(&HitTarget::TimelineMessage(event.clone())),
            ) {
                motion::tint(buf, area, colour);
            }
        }

        // An unread count that rises flashes once.
        let community = self.active_community_id();
        if let Some((seen_community, seen)) = &self.stage.counts_seen
            && *seen_community == community
        {
            for (channel, count) in &self.unread_counts {
                if *count > seen.get(channel).copied().unwrap_or(0) {
                    self.stage
                        .flashes
                        .retain(|(flashing, _)| flashing != channel);
                    self.stage.flashes.push((
                        *channel,
                        Anim::step(now, scale::SLOW, motion, Role::Informs),
                    ));
                }
            }
        }
        self.stage.counts_seen = Some((community, self.unread_counts.clone()));
        self.stage.flashes.retain(|(_, anim)| anim.running(now));
        for (channel, _) in &self.stage.flashes {
            if let Some(index) = self.channels.iter().position(|known| known.id == *channel)
                && let Some(row) = hits.area_of(&HitTarget::Channel(index))
            {
                motion::flash_count(buf, row);
            }
        }

        // Placeholder rows while the channel's history loads.
        if let Some((loading, since)) = self.stage.loading {
            let empty = !self.messages.iter().any(|message| message.system.is_none());
            let elapsed = now.saturating_duration_since(since);
            let shimmers = motion::plays(motion, Role::Decorates);
            if channel == Some(loading) && empty && (elapsed >= scale::FAST || !shimmers) {
                let mut content = inner(panes.timeline);
                if self.timeline.banner.is_some() && content.height > 2 {
                    content.y += 1;
                    content.height -= 1;
                }
                let quiet = self.theme.style(HighlightGroup::Muted);
                if shimmers && elapsed < scale::LOADING {
                    let phase = (elapsed.as_millis() % scale::LOOP.as_millis()) as f32
                        / scale::LOOP.as_millis() as f32;
                    let bright = self.theme.style(HighlightGroup::Normal);
                    motion::shimmer(buf, content, phase, quiet, bright);
                } else {
                    loading_line(buf, content, quiet);
                }
            }
        }

        let room = toast_room(frame.area(), panes);
        self.stage
            .toasts
            .render(frame.buffer_mut(), room, &self.theme, now);
    }

    /// The toast alone, over a screen with no workspace panes (the inbox,
    /// a zoomed app): at the bottom right above the status line.
    pub(super) fn stage_toast(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let room = Rect {
            height: area.height.saturating_sub(2),
            width: area.width.saturating_sub(1),
            ..area
        };
        self.stage
            .toasts
            .render(frame.buffer_mut(), room, &self.theme, Instant::now());
    }

    /// Before an overlay draws itself: notices that one opened or closed,
    /// and keeps the screen behind it (dimmed already) while one unfolds or
    /// is open, so its own cells can be found and animated.
    pub(super) fn stage_overlay_begin(&mut self, buf: &Buffer) -> Option<Buffer> {
        let now = Instant::now();
        let motion = self.motion();
        let overlay = self.presentation.overlay;
        let seen = self.stage.overlay_seen;
        // The huddle's panel and zoom redraw their lanes at the visualizer's
        // rate and take most of the screen: they open whole, and no frame
        // of theirs pays for a copy of the screen.
        let animated = motion::plays(motion, Role::Decorates) && overlay != Some(Overlay::Huddle);
        if seen != overlay {
            match (seen, overlay) {
                (None, Some(_)) if animated => {
                    self.stage.opening = Some(Anim::new(
                        now,
                        scale::BASE,
                        Ease::OutCubic,
                        motion,
                        Role::Decorates,
                    ));
                    self.stage.closing = None;
                }
                (Some(_), None) => {
                    self.stage.opening = None;
                    self.stage.closing = self.stage.overlay_cells.take().map(|(rect, cells)| {
                        (
                            Anim::new(now, scale::FAST, Ease::InCubic, motion, Role::Decorates),
                            rect,
                            cells,
                        )
                    });
                }
                _ => self.stage.opening = None,
            }
            self.stage.overlay_cells = None;
            self.stage.overlay_seen = overlay;
        }
        (overlay.is_some() && animated).then(|| buf.clone())
    }

    /// After an overlay drew itself: an opening one shows only the part of
    /// itself unfolded so far; a closed one folds away over the screen that
    /// is back. Only the overlay's own cells change between these frames;
    /// the screen behind is dimmed once.
    pub(super) fn stage_overlay_end(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        backdrop: Option<Buffer>,
    ) {
        let now = Instant::now();
        if self.presentation.overlay.is_some() {
            let Some(backdrop) = backdrop else {
                return;
            };
            let rect = motion::changed_rect(&backdrop, buf, area).filter(|rect| {
                u32::from(rect.width) * u32::from(rect.height) * 100
                    <= u32::from(area.width) * u32::from(area.height) * UNFOLD_AT_MOST
            });
            let Some(rect) = rect else {
                self.stage.opening = None;
                self.stage.overlay_cells = None;
                return;
            };
            self.stage.overlay_cells = Some((rect, motion::copy_cells(buf, rect)));
            if let Some(opening) = self.stage.opening {
                if opening.running(now) {
                    let shown = motion::unfolded(rect, opening.progress(now));
                    motion::restore_outside(buf, &backdrop, rect, shown);
                } else {
                    self.stage.opening = None;
                }
            }
        } else if let Some((closing, rect, cells)) = &self.stage.closing {
            if closing.running(now) {
                let shown = motion::unfolded(*rect, 1.0 - closing.progress(now));
                motion::paste_cells(buf, *rect, cells, shown);
            } else {
                self.stage.closing = None;
            }
        }
    }
}

/// The inside of a pane's frame.
fn inner(rect: Rect) -> Rect {
    Rect::new(
        rect.x.saturating_add(1),
        rect.y.saturating_add(1),
        rect.width.saturating_sub(2),
        rect.height.saturating_sub(2),
    )
}

/// Where a toast may sit: the panes' area above the composer and the status
/// line, inside the panes' outer edge.
fn toast_room(area: Rect, panes: &Panes) -> Rect {
    let bottom = panes
        .composer
        .map_or(panes.status.y, |composer| composer.y.min(panes.status.y))
        .saturating_sub(1);
    Rect::new(
        area.x,
        area.y,
        area.width.saturating_sub(1),
        bottom.saturating_sub(area.y),
    )
}

/// The plain line that says the history is loading, after the shimmer or in
/// its place.
fn loading_line(buf: &mut Buffer, content: Rect, style: ratatui::style::Style) {
    let content = content.intersection(buf.area);
    if content.height == 0 || content.width < 4 {
        return;
    }
    for y in content.top()..content.bottom() {
        for x in content.left()..content.right() {
            buf[(x, y)].reset();
        }
    }
    let text = "loading history…";
    let width = u16::try_from(unicode_width::UnicodeWidthStr::width(text)).unwrap_or(u16::MAX);
    let x = content.x + content.width.saturating_sub(width) / 2;
    let y = content.y + content.height / 2;
    buf.set_stringn(x, y, text, usize::from(content.width), style);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect, style::Modifier};
    use tempfile::TempDir;
    use tokio::time::Instant;
    use uuid::Uuid;

    use super::{Pacing, TEST_MOTION};
    use crate::{
        config::{Config, Motion},
        domain::{Channel, ChannelFacts, ChannelKind, Message, Visibility},
        paths::Paths,
        store::{Store, writer::StoreHandle},
        ui::state::{FocusSurface, Overlay},
    };

    const OWN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OTHER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn set_motion(motion: Motion) {
        TEST_MOTION.with(|cell| cell.set(Some(motion)));
    }

    /// An App with a community, an identity and `#general` open, holding
    /// two messages; nothing connects.
    async fn app(temporary: &TempDir) -> super::App {
        let paths = Paths {
            config_dir: temporary.path().join("config"),
            data_dir: temporary.path().join("data"),
            cache_dir: temporary.path().join("cache"),
        };
        paths.ensure().unwrap();
        let config = Config::default();
        let mut store = Store::open(paths.database_file()).unwrap();
        store.sync_config(&config).unwrap();
        let handle = StoreHandle::spawn(store).unwrap();
        let mut app = super::App::new(config, paths, handle).await.unwrap();
        let identity = crate::config::IdentityConfig {
            id: Uuid::new_v4(),
            label: "motion".into(),
            pubkey: OWN.into(),
            backend: crate::config::KeyBackend::Keychain,
            key_ref: "identity:motion".into(),
        };
        app.config.identities.push(identity.clone());
        app.config
            .add_community(
                "motion".into(),
                "wss://motion.example".into(),
                identity.id,
                false,
            )
            .unwrap();
        let general = Channel {
            id: Uuid::new_v4(),
            name: "general".into(),
            about: String::new(),
            kind: ChannelKind::Stream,
            visibility: Visibility::Public,
            is_member: true,
            is_hidden: false,
            member_count: 2,
            last_event_at: None,
            facts: ChannelFacts::default(),
        };
        let design = Channel {
            id: Uuid::new_v4(),
            name: "design".into(),
            ..general.clone()
        };
        let id = general.id;
        app.channels = vec![general, design];
        app.select_channel_index(0);
        app.messages = vec![message(id, "e1", OTHER, 1), message(id, "e2", OWN, 2)];
        app.presentation.set_workspace_focus(FocusSurface::Timeline);
        app
    }

    fn message(channel: Uuid, id: &str, author: &str, age: u64) -> Message {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        serde_json::from_value(serde_json::json!({
            "event_id": id,
            "channel_id": channel,
            "pubkey": author,
            "created_at": now - age,
            "content": format!("generated message {id}"),
            "root_event_id": null,
            "parent_event_id": null,
            "deleted": false,
        }))
        .unwrap()
    }

    fn draw(app: &mut super::App, terminal: &mut Terminal<TestBackend>) -> Buffer {
        terminal.draw(|frame| app.render(frame)).unwrap();
        terminal.backend().buffer().clone()
    }

    /// The loop's way with the animations alone, on paused time: sleep to
    /// the next wake time, draw a frame, ask again. Answers each frame's
    /// time in ms from the first call.
    async fn frames(
        app: &mut super::App,
        terminal: &mut Terminal<TestBackend>,
        until: Duration,
    ) -> Vec<u64> {
        let start = Instant::now();
        let mut drawn = Vec::new();
        while let Some(at) = app.motion_wake(Instant::now()) {
            if at.saturating_duration_since(start) > until {
                break;
            }
            tokio::time::advance(at.saturating_duration_since(Instant::now())).await;
            app.stage.advance(Instant::now());
            draw(app, terminal);
            drawn.push(u64::try_from((Instant::now() - start).as_millis()).unwrap());
            assert!(drawn.len() < 2_000, "an animation never ends");
        }
        drawn
    }

    #[tokio::test(start_paused = true)]
    async fn at_rest_nothing_asks_for_a_frame() {
        for motion in [Motion::Full, Motion::Calm, Motion::Off] {
            set_motion(motion);
            let temporary = TempDir::new().unwrap();
            let mut app = app(&temporary).await;
            let mut terminal = Terminal::new(TestBackend::new(120, 32)).unwrap();
            draw(&mut app, &mut terminal);
            draw(&mut app, &mut terminal);
            assert_eq!(app.motion_wake(Instant::now()), None, "{motion:?}");
        }
    }

    /// L8: the focus colour runs round the pane in 150 ms: five frames;
    /// with motion off or calm, none.
    #[tokio::test(start_paused = true)]
    async fn the_focus_sweep_takes_150_ms_and_only_with_full_motion() {
        for (motion, expected) in [
            (Motion::Full, vec![33, 66, 99, 132, 150]),
            (Motion::Calm, vec![]),
            (Motion::Off, vec![]),
        ] {
            set_motion(motion);
            let temporary = TempDir::new().unwrap();
            let mut app = app(&temporary).await;
            let mut terminal = Terminal::new(TestBackend::new(120, 32)).unwrap();
            draw(&mut app, &mut terminal);
            app.presentation.set_workspace_focus(FocusSurface::Channels);
            draw(&mut app, &mut terminal);
            let drawn = frames(&mut app, &mut terminal, Duration::from_secs(5)).await;
            assert_eq!(drawn, expected, "{motion:?}");
        }
    }

    /// L8: an overlay opens in 150 ms and closes in 80 ms; while it opens
    /// only its own cells change, the dimmed screen behind it does not.
    #[tokio::test(start_paused = true)]
    async fn an_overlay_unfolds_in_150_ms_and_folds_in_80_ms() {
        set_motion(Motion::Full);
        let temporary = TempDir::new().unwrap();
        let mut app = app(&temporary).await;
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        let before = draw(&mut app, &mut terminal);
        app.presentation.open_overlay(Overlay::Finder);
        let first = draw(&mut app, &mut terminal);
        let popup = super::super::centered(Rect::new(0, 0, 100, 30), 70, 12);
        // At its first frame the finder is a line in its middle: its
        // corner still shows the dimmed screen.
        assert!(first[(popup.x, popup.y)].modifier.contains(Modifier::DIM));
        let drawn = frames(&mut app, &mut terminal, Duration::from_secs(5)).await;
        assert_eq!(drawn, vec![33, 66, 99, 132, 150]);
        let open = terminal.backend().buffer().clone();
        assert!(!open[(popup.x, popup.y)].modifier.contains(Modifier::DIM));
        // Outside the finder, the opening frames changed nothing.
        for y in 0..30 {
            for x in 0..100 {
                if !popup.contains(ratatui::layout::Position { x, y }) {
                    assert_eq!(first[(x, y)], open[(x, y)], "({x}, {y})");
                }
            }
        }
        app.presentation.close_overlay();
        let closing = draw(&mut app, &mut terminal);
        // It folds over the screen that is back.
        let middle = (popup.x + popup.width / 2, popup.y + popup.height / 2);
        assert_eq!(closing[middle], open[middle]);
        let drawn = frames(&mut app, &mut terminal, Duration::from_secs(5)).await;
        assert_eq!(drawn, vec![33, 66, 80]);
        let closed = terminal.backend().buffer().clone();
        assert_eq!(closed, before, "folded away, the screen is as it was");
    }

    #[tokio::test(start_paused = true)]
    async fn with_motion_off_an_overlay_shows_whole_at_once() {
        set_motion(Motion::Off);
        let temporary = TempDir::new().unwrap();
        let mut app = app(&temporary).await;
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        draw(&mut app, &mut terminal);
        app.presentation.open_overlay(Overlay::Finder);
        let first = draw(&mut app, &mut terminal);
        let popup = super::super::centered(Rect::new(0, 0, 100, 30), 70, 12);
        assert!(!first[(popup.x, popup.y)].modifier.contains(Modifier::DIM));
        assert_eq!(app.motion_wake(Instant::now()), None);
        app.presentation.close_overlay();
        draw(&mut app, &mut terminal);
        assert_eq!(app.motion_wake(Instant::now()), None);
    }

    /// The huddle's panel redraws at the visualizer's rate: it opens whole,
    /// and asks for no frame of its own.
    #[tokio::test(start_paused = true)]
    async fn the_huddle_panel_opens_whole() {
        set_motion(Motion::Full);
        let temporary = TempDir::new().unwrap();
        let mut app = app(&temporary).await;
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        draw(&mut app, &mut terminal);
        app.presentation.open_overlay(Overlay::Huddle);
        draw(&mut app, &mut terminal);
        assert_eq!(app.motion_wake(Instant::now()), None);
        app.presentation.close_overlay();
        draw(&mut app, &mut terminal);
        assert_eq!(app.motion_wake(Instant::now()), None);
    }

    /// L7: a message that arrives in the channel in view takes a tint for
    /// 250 ms. Blended where the theme has its own background (eight
    /// frames), held and dropped in one step where it has none (one).
    #[tokio::test(start_paused = true)]
    async fn an_arriving_message_takes_a_tint_for_250_ms() {
        for (theme, motion, expected) in [
            ("bzz", Motion::Full, 1_usize),
            ("dark", Motion::Full, 8),
            ("dark", Motion::Calm, 8),
            ("dark", Motion::Off, 0),
        ] {
            set_motion(motion);
            let temporary = TempDir::new().unwrap();
            let mut app = app(&temporary).await;
            app.theme = crate::ui::theme::Theme::builtin(theme).unwrap();
            let mut terminal = Terminal::new(TestBackend::new(120, 32)).unwrap();
            draw(&mut app, &mut terminal);
            let channel = app.channels[0].id;
            app.messages.push(message(channel, "e3", OTHER, 0));
            draw(&mut app, &mut terminal);
            let drawn = frames(&mut app, &mut terminal, Duration::from_secs(5)).await;
            assert_eq!(drawn.len(), expected, "{theme} {motion:?}: {drawn:?}");
            assert!(drawn.last().is_none_or(|&last| last == 250), "{drawn:?}");
            // My own message, and one written long ago, take none.
            app.messages.push(message(channel, "e4", OWN, 0));
            app.messages.push(message(channel, "e5", OTHER, 3_600));
            draw(&mut app, &mut terminal);
            assert_eq!(app.motion_wake(Instant::now()), None, "{theme} {motion:?}");
        }
    }

    /// L7: an unread count that rises flashes once, 250 ms, in one step.
    #[tokio::test(start_paused = true)]
    async fn a_rising_unread_count_flashes_once() {
        set_motion(Motion::Full);
        let temporary = TempDir::new().unwrap();
        let mut app = app(&temporary).await;
        let mut terminal = Terminal::new(TestBackend::new(120, 32)).unwrap();
        let design = app.channels[1].id;
        app.unread_counts.insert(design, 1);
        draw(&mut app, &mut terminal);
        assert_eq!(
            app.motion_wake(Instant::now()),
            None,
            "a count seen first does not flash"
        );
        app.unread_counts.insert(design, 2);
        let flashing = draw(&mut app, &mut terminal);
        let reversed = (0..120).any(|x| {
            (0..32).any(|y| {
                flashing[(x, y)].symbol() == "2"
                    && flashing[(x, y)].modifier.contains(Modifier::REVERSED)
            })
        });
        assert!(reversed, "the count is drawn reversed while it flashes");
        let drawn = frames(&mut app, &mut terminal, Duration::from_secs(5)).await;
        assert_eq!(drawn, vec![250]);
        app.unread_counts.insert(design, 1);
        draw(&mut app, &mut terminal);
        assert_eq!(
            app.motion_wake(Instant::now()),
            None,
            "a count that falls does not"
        );
    }

    /// L8: while a channel's history loads with nothing cached, placeholder
    /// rows shimmer, for 10 s at most; then the plain loading line, and no
    /// frame. With motion off the plain line at once.
    #[tokio::test(start_paused = true)]
    async fn a_loading_shimmer_asks_for_frames_for_10_s_at_most() {
        for motion in [Motion::Full, Motion::Off] {
            set_motion(motion);
            let temporary = TempDir::new().unwrap();
            let mut app = app(&temporary).await;
            app.messages.clear();
            let channel = app.channels[0].id;
            let mut terminal = Terminal::new(TestBackend::new(120, 32)).unwrap();
            app.stage.loading(channel, Instant::now());
            draw(&mut app, &mut terminal);
            let drawn = frames(&mut app, &mut terminal, Duration::from_secs(60)).await;
            let screen = terminal.backend().buffer().clone();
            let text = screen
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(text.contains("loading history…"), "{motion:?}");
            assert!(!text.contains('░'), "{motion:?}");
            match motion {
                Motion::Off => assert!(drawn.is_empty(), "{drawn:?}"),
                _ => {
                    assert_eq!(drawn.first(), Some(&80), "it waits 80 ms before showing");
                    assert_eq!(drawn.last(), Some(&10_000));
                    assert!(drawn.len() <= 10_000 / 33 + 2, "{}", drawn.len());
                }
            }
            // Loaded: nothing more.
            app.stage.loaded();
            assert_eq!(app.motion_wake(Instant::now()), None);
        }
    }

    /// L7 at the level of the App: the toast's frames, and the composer
    /// never under it.
    #[tokio::test(start_paused = true)]
    async fn a_toast_moves_150_ms_rests_3_s_moves_250_ms_and_never_covers_the_composer() {
        set_motion(Motion::Full);
        let temporary = TempDir::new().unwrap();
        let mut app = app(&temporary).await;
        let mut terminal = Terminal::new(TestBackend::new(120, 32)).unwrap();
        app.presentation.composer_target = Some(crate::ui::state::ComposerTarget {
            community_id: app.active_community_id().unwrap(),
            channel_id: app.channels[0].id,
            thread_root_id: None,
            parent_event_id: None,
        });
        draw(&mut app, &mut terminal);
        app.toast("attachment saved");
        draw(&mut app, &mut terminal);
        let drawn = frames(&mut app, &mut terminal, Duration::from_millis(200)).await;
        assert_eq!(drawn, vec![33, 66, 99, 132, 150]);
        let screen = terminal.backend().buffer().clone();
        let rows = (0..32)
            .map(|y| {
                (0..120)
                    .map(|x| screen[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        let at = rows
            .iter()
            .position(|row| row.contains("attachment saved"))
            .expect("the toast is on screen");
        let composer = rows
            .iter()
            .position(|row| row.contains("message #general"))
            .expect("the composer is open");
        assert!(at < composer, "the toast sits above the composer");
        let rest = app.motion_wake(Instant::now()).unwrap() - Instant::now();
        assert_eq!(rest, Duration::from_secs(3), "it rests 3 s without a frame");
        let drawn = frames(&mut app, &mut terminal, Duration::from_secs(10)).await;
        assert_eq!(drawn.first(), Some(&3_000));
        assert_eq!(drawn.last(), Some(&3_250));
        assert_eq!(drawn.len(), 9);
        let gone = terminal.backend().buffer().clone();
        let text = gone
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(!text.contains("attachment saved"));
    }

    /// L9: the loop's housekeeping at rest comes once a second, within
    /// 100 ms after anything happened or while something it polls is busy;
    /// frames asked for in a burst are one frame interval apart.
    #[tokio::test(start_paused = true)]
    async fn the_loop_sleeps_until_its_next_deadline() {
        let start = Instant::now();
        let ms = Duration::from_millis;
        let mut pacing = Pacing::new();
        let due = start + ms(1_002);
        assert_eq!(
            pacing.tick_at(false, due, start),
            start,
            "the first comes at once"
        );
        pacing.ticked(start);
        // At rest: when the store's refresh is due, a little after a second.
        assert_eq!(pacing.tick_at(false, due, start), due);
        // Never sooner than 100 ms, never later than 1.1 s.
        assert_eq!(pacing.tick_at(false, start, start), start + ms(100));
        assert_eq!(
            pacing.tick_at(false, start + ms(5_000), start),
            start + ms(1_100)
        );
        assert_eq!(pacing.tick_at(true, due, start), start + ms(100));
        pacing.poke();
        assert_eq!(pacing.tick_at(false, due, start), start + ms(100));
        pacing.ticked(start);
        // Frames.
        assert_eq!(
            pacing.frame_at(false, start),
            None,
            "no frame asked, no wake"
        );
        assert_eq!(pacing.frame_at(true, start), Some(start));
        pacing.drawn(start);
        let soon = start + Duration::from_millis(5);
        assert_eq!(
            pacing.frame_at(true, soon),
            Some(start + Duration::from_millis(33))
        );
        assert!(!pacing.frame_due(soon));
        assert!(pacing.frame_due(start + Duration::from_millis(32)));
        // The huddle's levels.
        assert_eq!(pacing.huddle_at(false, start), None);
        assert_eq!(pacing.huddle_at(true, start), Some(start));
        pacing.huddle_read(start);
        assert_eq!(
            pacing.huddle_at(true, soon),
            Some(start + Duration::from_millis(33))
        );
    }

    /// Rule 20, in the loop's order: the input is polled before the frame,
    /// so a key that waits when a frame of an animation is due is handled
    /// first. This is the order of `run_to_exit`'s `select!`.
    #[tokio::test(start_paused = true)]
    async fn a_waiting_key_is_handled_before_a_frame_of_an_animation() {
        let (keys, mut input) = tokio::sync::mpsc::unbounded_channel::<char>();
        let mut handled = Vec::new();
        let due = Some(Instant::now());
        keys.send('j').unwrap();
        keys.send('k').unwrap();
        for _ in 0..3 {
            tokio::select! {
                biased;
                Some(key) = input.recv() => handled.push(format!("key {key}")),
                () = super::sleep_until(due) => handled.push("frame".to_owned()),
            }
        }
        assert_eq!(handled, ["key j", "key k", "frame"]);
    }

    /// One look: the palette and the first screen draw their frame, title
    /// and keys through `frame.rs`, so their keys are in the hint colour and
    /// the palette's title is a label, as every overlay's.
    #[tokio::test(start_paused = true)]
    async fn the_palette_and_the_first_screen_look_like_the_rest() {
        use crate::ui::theme::HighlightGroup;
        set_motion(Motion::Off);
        let hint = |app: &super::App| {
            crate::ui::frame::reduce(
                app.theme.style(HighlightGroup::HintKey),
                app.theme.color_mode(),
            )
            .fg
        };
        let find = |buf: &Buffer, text: &str| -> (u16, u16) {
            for y in 0..buf.area.height {
                let row = (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<Vec<_>>();
                for x in 0..row.len() {
                    if row[x..].concat().starts_with(text) {
                        return (u16::try_from(x).unwrap(), y);
                    }
                }
            }
            panic!("{text} is not on screen");
        };
        let temporary = TempDir::new().unwrap();
        let mut app = app(&temporary).await;
        let mut terminal = Terminal::new(TestBackend::new(120, 32)).unwrap();
        app.open_palette();
        let screen = draw(&mut app, &mut terminal);
        let at = find(&screen, "Esc close");
        assert_eq!(Some(screen[at].fg), hint(&app), "the palette's keys");
        let title = find(&screen, " Actions ");
        assert!(
            screen[title].modifier.contains(Modifier::REVERSED),
            "a label"
        );
        // The first screen, with no community.
        let temporary = TempDir::new().unwrap();
        let mut empty = app_without_community(&temporary).await;
        let screen = draw(&mut empty, &mut terminal);
        let at = find(&screen, "Enter choose");
        assert_eq!(Some(screen[at].fg), hint(&empty), "the first screen's keys");
    }

    async fn app_without_community(temporary: &TempDir) -> super::App {
        let paths = Paths {
            config_dir: temporary.path().join("config"),
            data_dir: temporary.path().join("data"),
            cache_dir: temporary.path().join("cache"),
        };
        paths.ensure().unwrap();
        let config = Config::default();
        let mut store = Store::open(paths.database_file()).unwrap();
        store.sync_config(&config).unwrap();
        let handle = StoreHandle::spawn(store).unwrap();
        super::App::new(config, paths, handle).await.unwrap()
    }

    /// A writer that counts what bzz writes to the terminal.
    #[derive(Clone, Default)]
    struct Counting(std::sync::Arc<std::sync::atomic::AtomicUsize>);

    impl std::io::Write for Counting {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.0
                .fetch_add(data.len(), std::sync::atomic::Ordering::Relaxed);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Rule 20 over SSH: the bytes a transition writes through a real
    /// terminal backend at 160x45, and none at rest. The study measured 1 to
    /// 3 KB a transition and about 23 KB for an overlay with its dimming;
    /// here the backdrop is dimmed once and only the overlay's own cells
    /// change while it unfolds.
    #[tokio::test(start_paused = true)]
    async fn the_bytes_of_each_transition_are_bounded_and_none_at_rest() {
        use std::sync::atomic::Ordering::Relaxed;
        set_motion(Motion::Full);
        let temporary = TempDir::new().unwrap();
        let mut app = app(&temporary).await;
        let counter = Counting::default();
        let size = Rect::new(0, 0, 160, 45);
        let mut terminal = ratatui::Terminal::with_options(
            ratatui::backend::CrosstermBackend::new(counter.clone()),
            ratatui::TerminalOptions {
                viewport: ratatui::Viewport::Fixed(size),
            },
        )
        .unwrap();
        let mut frame = |app: &mut super::App| {
            terminal.draw(|frame| app.render(frame)).unwrap();
        };
        // Runs the animations to their end, as the loop does; answers the
        // bytes written and the frames drawn.
        async fn settle(
            app: &mut super::App,
            frame: &mut impl FnMut(&mut super::App),
            counter: &Counting,
        ) -> (usize, usize) {
            let before = counter.0.load(Relaxed);
            frame(app);
            let mut frames = 1;
            while let Some(at) = app.motion_wake(Instant::now()) {
                tokio::time::advance(at.saturating_duration_since(Instant::now())).await;
                app.stage.advance(Instant::now());
                frame(app);
                frames += 1;
            }
            (counter.0.load(Relaxed) - before, frames)
        }
        frame(&mut app);
        let mut report = Vec::new();
        let rest = counter.0.load(Relaxed);
        for _ in 0..10 {
            frame(&mut app);
        }
        let idle = counter.0.load(Relaxed) - rest;
        // A frame drawn with nothing changed writes only the cursor's
        // codes; the loop draws none at rest (the assertion below).
        report.push(format!("ten frames with nothing changed: {idle} B"));
        assert!(idle <= 10 * 32, "{idle}");
        assert_eq!(
            app.motion_wake(Instant::now()),
            None,
            "and none asked at rest"
        );

        app.toast("attachment saved");
        let toast = settle(&mut app, &mut frame, &counter).await;
        report.push(format!(
            "toast in, held, out: {} B in {} frames",
            toast.0, toast.1
        ));
        assert!(toast.0 < 4_096, "{toast:?}");

        app.presentation.set_workspace_focus(FocusSurface::Channels);
        let sweep = settle(&mut app, &mut frame, &counter).await;
        report.push(format!("focus sweep: {} B in {} frames", sweep.0, sweep.1));
        assert!(sweep.0 < 4_096, "{sweep:?}");
        app.presentation.set_workspace_focus(FocusSurface::Timeline);
        settle(&mut app, &mut frame, &counter).await;

        let channel = app.channels[0].id;
        app.messages.push(message(channel, "e9", OTHER, 0));
        let tint = settle(&mut app, &mut frame, &counter).await;
        report.push(format!(
            "arrival with tint (step): {} B in {} frames",
            tint.0, tint.1
        ));

        app.presentation.open_overlay(Overlay::Finder);
        let open = settle(&mut app, &mut frame, &counter).await;
        report.push(format!(
            "overlay opens (with dimming): {} B in {} frames",
            open.0, open.1
        ));
        app.presentation.close_overlay();
        let close = settle(&mut app, &mut frame, &counter).await;
        report.push(format!(
            "overlay closes (undims): {} B in {} frames",
            close.0, close.1
        ));
        assert!(open.0 + close.0 < 40_000, "{open:?} {close:?}");
        // The same overlay with motion off: what the dimming alone costs.
        set_motion(Motion::Off);
        app.presentation.open_overlay(Overlay::Finder);
        let still_open = settle(&mut app, &mut frame, &counter).await;
        app.presentation.close_overlay();
        let still_close = settle(&mut app, &mut frame, &counter).await;
        report.push(format!(
            "the same with motion off: {} B and {} B, one frame each",
            still_open.0, still_close.0
        ));
        assert_eq!((still_open.1, still_close.1), (1, 1));
        println!("{}", report.join("\n"));
    }
}
