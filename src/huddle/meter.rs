//! What the visualizer knows of a call, from the levels it reads each frame:
//! a smoothed level and a speaking gate per person and for your microphone,
//! and the last twelve seconds of each as a fixed ring.
//!
//! Time comes in as an argument (seconds on any monotonic scale); nothing
//! here reads a clock, so tests step whole frames. Every constant is in
//! seconds, so 30 and 60 frames a second decide alike.
//!
//! The rings live in memory while the call is on, are zeroed when it stops,
//! and are never stored, logged or sent. No `Debug` here prints a number.

use super::levels::Levels;
use crate::config::Motion;

/// The gate opens above this level (dBFS)…
pub const OPEN_DB: f32 = -42.0;
/// …and closes after [`HANG_SECONDS`] below this one.
pub const CLOSE_DB: f32 = -50.0;
pub const HANG_SECONDS: f32 = 0.3;
/// Below this a lane is a quiet baseline.
pub const QUIET_DB: f32 = -60.0;
/// History columns a second, and how many are kept (12 s).
pub const HISTORY_HZ: f32 = 20.0;
pub const HISTORY_LEN: usize = 240;
/// A speaker with this little level change for this long is silent.
pub const SILENT_AFTER_SECONDS: f32 = 2.0;
/// Talking while muted for this long turns the warning on.
pub const MUTED_TALK_SECONDS: f32 = 0.5;

/// A source of levels. The engine's [`Levels`] is one; a generated source
/// writes into one too.
pub trait LevelSource {
    /// The loudest chunk of peer `index` since the last read, in dBFS.
    fn take_peer(&self, index: u8) -> Option<f32>;
    /// The loudest chunk of your microphone since the last read.
    fn take_mic(&self) -> Option<f32>;
}

impl LevelSource for Levels {
    fn take_peer(&self, index: u8) -> Option<f32> {
        Levels::take_peer(self, index)
    }
    fn take_mic(&self) -> Option<f32> {
        Levels::take_mic(self)
    }
}

/// One lane: a person or your microphone.
#[derive(Clone)]
pub struct Track {
    /// 0..1, smoothed (≈30 ms up, ≈220 ms down).
    level: f32,
    speaking: bool,
    hang: f32,
    /// 0..1, fades in with speaking over ≈80 ms and out over ≈350 ms.
    glow: f32,
    last_spoke: Option<f64>,
    /// Seconds the gate has been open without a break.
    open_for: f32,
    /// Ring of history columns (0..=255), newest at `head - 1`.
    ring: [u8; HISTORY_LEN],
    head: usize,
    /// The loudest level of the column being filled.
    column: f32,
}

impl Default for Track {
    fn default() -> Self {
        Self {
            level: 0.0,
            speaking: false,
            hang: 0.0,
            glow: 0.0,
            last_spoke: None,
            open_for: 0.0,
            ring: [0; HISTORY_LEN],
            head: 0,
            column: 0.0,
        }
    }
}

impl std::fmt::Debug for Track {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Track")
            .field("speaking", &self.speaking)
            .finish_non_exhaustive()
    }
}

/// A level in dBFS as the picture draws it, 0..1 (−60 to −12 dBFS).
pub fn normalise(db: f32) -> f32 {
    ((db - QUIET_DB) / 48.0).clamp(0.0, 1.0)
}

fn approach(value: &mut f32, target: f32, dt: f32, tau: f32) {
    *value += (target - *value) * (1.0 - (-dt / tau).exp());
}

impl Track {
    pub fn speaking(&self) -> bool {
        self.speaking
    }
    pub fn level(&self) -> f32 {
        self.level
    }
    pub fn glow(&self) -> f32 {
        self.glow
    }
    /// Seconds since this person last spoke, if they did in this call.
    pub fn since_spoke(&self, now: f64) -> Option<f64> {
        self.last_spoke.map(|at| (now - at).max(0.0))
    }
    pub fn open_for(&self) -> f32 {
        self.open_for
    }

    /// The column `age` steps ago (0 is now), 0..1.
    pub fn column(&self, age: usize) -> f32 {
        if age == 0 {
            return self.column.max(self.level);
        }
        if age > HISTORY_LEN {
            return 0.0;
        }
        let index = (self.head + HISTORY_LEN - age) % HISTORY_LEN;
        f32::from(self.ring[index]) / 255.0
    }

    /// The loudest column from `age` (inclusive) to `age + span` (exclusive).
    pub fn columns_max(&self, age: usize, span: usize) -> f32 {
        (age..age + span.max(1))
            .map(|age| self.column(age))
            .fold(0.0, f32::max)
    }

    fn update(&mut self, db: Option<f32>, dt: f32, now: f64) {
        let db = db.unwrap_or(super::levels::FLOOR_DB);
        let target = normalise(db);
        let tau = if target > self.level { 0.03 } else { 0.22 };
        approach(&mut self.level, target, dt, tau);
        if db > OPEN_DB {
            if self.speaking {
                self.open_for += dt;
            } else {
                self.open_for = 0.0;
            }
            self.speaking = true;
            self.hang = HANG_SECONDS;
            self.last_spoke = Some(now);
        } else if self.speaking {
            if db < CLOSE_DB {
                self.hang -= dt;
            }
            if self.hang <= 0.0 {
                self.speaking = false;
                self.open_for = 0.0;
            } else {
                self.open_for += dt;
            }
        }
        let (goal, tau) = if self.speaking {
            (1.0, 0.08)
        } else {
            (0.0, 0.35)
        };
        approach(&mut self.glow, goal, dt, tau);
        self.column = self.column.max(self.level);
    }

    fn push_column(&mut self) {
        self.ring[self.head] = (self.column.clamp(0.0, 1.0) * 255.0).round() as u8;
        self.head = (self.head + 1) % HISTORY_LEN;
        self.column = self.level;
    }
}

/// Every lane of a call. Keyed by relay peer index; your microphone apart.
#[derive(Default)]
pub struct Meter {
    peers: Vec<(u8, Track)>,
    mic: Track,
    last: Option<f64>,
    /// Seconds into the history column being filled.
    column_elapsed: f32,
    /// When the last level above [`QUIET_DB`] or a moving lane was seen.
    loud_at: Option<f64>,
    now: f64,
}

impl std::fmt::Debug for Meter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Meter { .. }")
    }
}

impl Meter {
    /// Reads every slot of `peers` and the microphone, and moves time to `now`.
    pub fn update(&mut self, source: &dyn LevelSource, peers: &[u8], now: f64) {
        let dt = self
            .last
            .map_or(0.0, |last| (now - last).clamp(0.0, 1.0) as f32);
        self.last = Some(now);
        self.now = now;
        self.peers.retain(|(index, _)| peers.contains(index));
        for index in peers {
            if !self.peers.iter().any(|(known, _)| known == index) {
                self.peers.push((*index, Track::default()));
            }
        }
        self.peers.sort_by_key(|(index, _)| {
            peers
                .iter()
                .position(|candidate| candidate == index)
                .unwrap_or(usize::MAX)
        });
        let mut loud = false;
        for (index, track) in &mut self.peers {
            let db = source.take_peer(*index);
            loud |= db.is_some_and(|db| db > QUIET_DB);
            track.update(db, dt, now);
            loud |= track.level > 0.02;
        }
        let db = source.take_mic();
        loud |= db.is_some_and(|db| db > QUIET_DB);
        self.mic.update(db, dt, now);
        loud |= self.mic.level > 0.02;
        if loud {
            self.loud_at = Some(now);
        }
        self.column_elapsed += dt;
        let step = 1.0 / HISTORY_HZ;
        let mut pushed = 0;
        while self.column_elapsed >= step {
            self.column_elapsed -= step;
            // After a long gap the history is only quiet columns.
            if pushed < HISTORY_LEN {
                for (_, track) in &mut self.peers {
                    track.push_column();
                }
                self.mic.push_column();
                pushed += 1;
            }
        }
    }

    /// Forgets everything (the call stopped).
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn now(&self) -> f64 {
        self.now
    }

    pub fn peer(&self, index: u8) -> Option<&Track> {
        self.peers
            .iter()
            .find(|(known, _)| *known == index)
            .map(|(_, track)| track)
    }

    pub fn peers(&self) -> impl Iterator<Item = (u8, &Track)> {
        self.peers.iter().map(|(index, track)| (*index, track))
    }

    pub fn mic(&self) -> &Track {
        &self.mic
    }

    /// The peers speaking now, in lane order.
    pub fn speaking(&self) -> Vec<u8> {
        self.peers
            .iter()
            .filter(|(_, track)| track.speaking)
            .map(|(index, _)| *index)
            .collect()
    }

    /// Whether the call has been silent for [`SILENT_AFTER_SECONDS`].
    /// A call where nobody has made a sound yet is silent from the start.
    pub fn is_silent(&self) -> bool {
        self.loud_at
            .is_none_or(|at| self.now - at >= f64::from(SILENT_AFTER_SECONDS))
    }

    /// What a still meter shows: who speaks, and whether your microphone
    /// hears you. Changes of this are the only frames with motion off.
    pub fn signature(&self) -> (Vec<u8>, bool) {
        (self.speaking(), self.mic.speaking)
    }
}

/// Decides when the visualizer needs a frame (plan rule 20).
#[derive(Default)]
pub struct Pacer {
    last_frame: Option<f64>,
    shown: Option<(Vec<u8>, bool)>,
}

impl std::fmt::Debug for Pacer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Pacer { .. }")
    }
}

impl Pacer {
    /// The shortest time between two frames of motion: 30 and 5 a second.
    /// bzz's frame loop ticks every 33 ms, so a tick a little early still
    /// counts (2 ms of slack).
    pub const fn interval(motion: Motion) -> Option<f64> {
        match motion {
            Motion::Full => Some(1.0 / 30.0),
            Motion::Calm => Some(0.2),
            Motion::Off => None,
        }
    }

    /// Whether a frame should be asked for at `now`. `visible`: a call is on
    /// and its panel or strip is on screen. Asks for nothing while hidden,
    /// after [`SILENT_AFTER_SECONDS`] of silence, or, with motion off, unless
    /// the speaker changed.
    pub fn wants_frame(&mut self, meter: &Meter, visible: bool, motion: Motion, now: f64) -> bool {
        self.wants_frame_every(meter, visible, Self::interval(motion), now)
    }

    /// As [`Pacer::wants_frame`], with the shortest time between two frames
    /// of motion given (`None`: motion off).
    pub fn wants_frame_every(
        &mut self,
        meter: &Meter,
        visible: bool,
        interval: Option<f64>,
        now: f64,
    ) -> bool {
        if !visible {
            self.shown = None;
            return false;
        }
        let signature = meter.signature();
        let changed = self.shown.as_ref() != Some(&signature);
        let due = match interval {
            _ if changed => true,
            None => false,
            Some(_) if meter.is_silent() => false,
            Some(interval) => self
                .last_frame
                .is_none_or(|last| now - last >= interval - 0.002),
        };
        if due {
            self.shown = Some(signature);
            self.last_frame = Some(now);
        }
        due
    }
}

/// Steps a costly style's rate down when writing to the terminal is slow
/// (SSH, a slow link): when a frame took more than a third of its interval
/// to draw three times running, the interval doubles, up to
/// [`Throttle::SLOWEST`]; after thirty quick frames it halves back.
#[derive(Debug, Default)]
pub struct Throttle {
    /// How many times the interval was doubled.
    steps: u8,
    slow: u8,
    quick: u8,
}

impl Throttle {
    /// Five frames a second at the slowest.
    pub const SLOWEST: f64 = 0.2;

    /// The interval to use, from the style's own.
    pub fn interval(&self, base: f64) -> f64 {
        (base * f64::from(1_u32 << self.steps)).min(Self::SLOWEST.max(base))
    }

    /// How long a frame took to draw and write, against the interval it was
    /// drawn at.
    pub fn record(&mut self, seconds: f64, base: f64) {
        let interval = self.interval(base);
        if seconds > interval / 3.0 {
            self.quick = 0;
            self.slow += 1;
            if self.slow >= 3 {
                self.slow = 0;
                if self.interval(base) < Self::SLOWEST {
                    self.steps += 1;
                }
            }
        } else if seconds < interval / 6.0 {
            self.slow = 0;
            self.quick += 1;
            if self.quick >= 30 {
                self.quick = 0;
                self.steps = self.steps.saturating_sub(1);
            }
        } else {
            self.slow = 0;
            self.quick = 0;
        }
    }

    /// Back to the style's own rate (another style, or the view closed).
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{huddle::synth, ui::redraw_gate::RedrawGate};

    /// One 10 ms chunk a step: the tone while `on(t)`, else silence.
    fn drive(
        fps: f64,
        seconds: f64,
        on: impl Fn(f64) -> bool,
        mut each: impl FnMut(f64, &Meter),
    ) -> Meter {
        let levels = Levels::default();
        let mut meter = Meter::default();
        let frame = 1.0 / fps;
        let mut next_frame = 0.0;
        let mut sample = 0_u64;
        let chunks = (seconds * 100.0).round() as u64;
        for chunk in 0..chunks {
            let t = chunk as f64 / 100.0;
            if on(t) {
                levels.publish_peer(4, &synth::tone(1000.0, -20.0, sample, 480));
            } else {
                levels.publish_peer(4, &[0.0; 480]);
            }
            sample += 480;
            let t_end = (chunk + 1) as f64 / 100.0;
            while next_frame <= t_end + 1e-9 {
                meter.update(&levels, &[4], next_frame);
                each(next_frame, &meter);
                next_frame += frame;
            }
        }
        meter
    }

    /// 1 kHz at −20 dBFS from 0.5 s for 300 ms: the gate opens within one
    /// frame of the tone and closes 300 to 400 ms after it ends, at 30 and at
    /// 60 frames a second alike. A −55 dBFS hiss never opens it.
    #[test]
    fn the_gate_opens_and_closes_at_the_expected_instants() {
        for fps in [30.0, 60.0] {
            let mut opened = None;
            let mut closed = None;
            drive(
                fps,
                1.5,
                |t| (0.5..0.8).contains(&t),
                |t, meter| {
                    let speaking = meter.peer(4).unwrap().speaking();
                    if speaking && opened.is_none() {
                        opened = Some(t);
                    }
                    if !speaking && opened.is_some() && closed.is_none() {
                        closed = Some(t);
                    }
                },
            );
            let opened = opened.expect("the gate opened");
            let closed = closed.expect("the gate closed");
            assert!(
                opened >= 0.5 && opened <= 0.5 + 1.0 / fps + 0.011,
                "{fps}: {opened}"
            );
            let after = closed - 0.8;
            assert!((0.3..=0.4).contains(&after), "{fps}: closed {after}");
        }
        let levels = Levels::default();
        let mut meter = Meter::default();
        for frame in 0..60 {
            levels.publish_peer(4, &synth::tone(1000.0, -55.0, 0, 480));
            meter.update(&levels, &[4], f64::from(frame) / 30.0);
            assert!(!meter.peer(4).unwrap().speaking());
        }
    }

    #[test]
    fn two_voices_at_once_light_both_lanes() {
        let levels = Levels::default();
        let mut meter = Meter::default();
        for frame in 0..10 {
            levels.publish_peer(1, &synth::tone(440.0, -20.0, 0, 480));
            levels.publish_peer(2, &synth::tone(300.0, -25.0, 0, 480));
            meter.update(&levels, &[1, 2, 3], f64::from(frame) / 30.0);
        }
        assert_eq!(meter.speaking(), vec![1, 2]);
        assert!(meter.peer(1).unwrap().column(0) > 0.5);
        assert!(meter.peer(3).unwrap().column(0) < 0.01);
    }

    #[test]
    fn history_scrolls_by_time_not_by_frame() {
        for fps in [20.0, 30.0, 60.0] {
            let meter = drive(fps, 2.0, |t| t < 0.5, |_, _| {});
            let track = meter.peer(4).unwrap();
            // At 2 s: the last 0.75 s is quiet (the meter's release is
            // over), and 1.55 to 1.8 s ago (0.2 to 0.45 s) is the tone.
            let quiet = track.columns_max(0, 15);
            let loud = track.columns_max(31, 5);
            assert!(quiet < 0.05, "{fps}: {quiet}");
            assert!(loud > 0.5, "{fps}: {loud}");
        }
    }

    #[test]
    fn the_microphone_moves_whatever_the_mute() {
        let levels = Levels::default();
        let mut meter = Meter::default();
        for frame in 0..30 {
            levels.publish_mic(&synth::tone(200.0, -18.0, 0, 960));
            meter.update(&levels, &[], f64::from(frame) / 30.0);
        }
        assert!(meter.mic().speaking());
        assert!(meter.mic().open_for() >= MUTED_TALK_SECONDS);
    }

    #[test]
    fn debug_of_every_type_that_holds_a_level_prints_no_number() {
        let levels = Levels::default();
        let mut meter = Meter::default();
        for frame in 0..10 {
            levels.publish_peer(9, &synth::tone(1000.0, -20.0, 0, 480));
            levels.publish_mic(&synth::tone(1000.0, -30.0, 0, 480));
            meter.update(&levels, &[9], f64::from(frame) / 30.0);
        }
        let whole = format!("{meter:?}");
        assert!(!whole.chars().any(|c| c.is_ascii_digit()), "{whole}");
        let track = format!("{:?}", meter.peer(9).unwrap());
        assert!(!track.chars().any(|c| c.is_ascii_digit()), "{track}");
        let mic = format!("{:?}", meter.mic());
        assert!(!mic.chars().any(|c| c.is_ascii_digit()), "{mic}");
        let pacer = format!("{:?}", Pacer::default());
        assert!(!pacer.chars().any(|c| c.is_ascii_digit()), "{pacer}");
    }

    /// Counts the frames the redraw gate admits over ten seconds, with the
    /// frame loop ticking every 33 ms as bzz's does.
    fn count_frames(
        visible: impl Fn(f64) -> bool,
        motion: Motion,
        voice: impl Fn(f64) -> bool,
    ) -> u64 {
        let levels = Levels::default();
        let mut meter = Meter::default();
        let mut pacer = Pacer::default();
        let mut gate = RedrawGate::default();
        let mut frames = 0;
        // The first frame of the screen is not the visualizer's.
        assert!(gate.take());
        for tick in 0..300 {
            let now = f64::from(tick) * 0.033;
            if voice(now) {
                levels.publish_peer(2, &synth::tone(500.0, -20.0, 0, 480));
            }
            meter.update(&levels, &[2], now);
            gate.request_if(pacer.wants_frame(&meter, visible(now), motion, now));
            if gate.take() {
                frames += 1;
            }
        }
        frames
    }

    #[test]
    fn frames_only_while_seen_and_moving() {
        let speech = |t: f64| (1.0..4.0).contains(&t);
        // No huddle or the panel and strip hidden: no frame at all.
        assert_eq!(count_frames(|_| false, Motion::Full, speech), 0);
        // Silence from the start: one frame to show the call, then none.
        assert_eq!(count_frames(|_| true, Motion::Full, |_| false), 1);
        // Speech from 1 to 4 s: at most 30 a second while it moves, nothing
        // two seconds after it ends (plus the release of the meter): about
        // 5.8 s of frames.
        let full = count_frames(|_| true, Motion::Full, speech);
        assert!((150..=185).contains(&full), "{full}");
        let calm = count_frames(|_| true, Motion::Calm, speech);
        assert!((20..=35).contains(&calm), "{calm}");
        // Motion off: a still meter, a frame only when the speaker changes.
        let off = count_frames(|_| true, Motion::Off, speech);
        assert!((1..=3).contains(&off), "{off}");
        // Hidden halfway through: nothing after.
        let half = count_frames(|t| t < 2.0, Motion::Full, speech);
        assert!(half < full && half <= 45, "{half}");
        println!(
            "frames in 10 s: hidden 0, silence 1, full {full}, calm {calm}, off {off}, hidden at 2 s {half}"
        );
    }
}
