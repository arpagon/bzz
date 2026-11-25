//! The microphone check before joining a huddle (plan v0.17.0 B7).
//!
//! While the join confirmation is open, your microphone's level is read into
//! a lane of its own, so you see that it works before anything is sent: the
//! call's engine has not started, nothing is encoded and no socket is open.
//! If no sound at all comes for [`NO_SIGNAL_SECONDS`], the check says so and
//! names the input device, which you can change with `:huddle input`.
//!
//! The device is opened by the caller (in bzz, the same device layer a call
//! uses) and its frames are published into [`MicCheck::levels`]; a test
//! publishes generated frames instead. Levels are drawn and forgotten (plan
//! D8); no `Debug` here prints a number.

use std::sync::Arc;

use super::{
    levels::Levels,
    meter::{Meter, QUIET_DB},
};

/// Seconds without any sound before the check says so.
pub const NO_SIGNAL_SECONDS: f64 = 3.0;

/// What the check says now.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MicState {
    /// The device is opening, or nothing has come yet.
    Listening,
    /// A sound above the quiet floor came: the microphone works.
    Works,
    /// Nothing for [`NO_SIGNAL_SECONDS`].
    NoSignal,
    /// The device could not be opened; the fixed, actionable message.
    Failed(String),
}

pub struct MicCheck {
    levels: Arc<Levels>,
    meter: Meter,
    started: Option<f64>,
    heard: bool,
    device: Option<String>,
    failed: Option<String>,
}

impl std::fmt::Debug for MicCheck {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("MicCheck { .. }")
    }
}

impl MicCheck {
    /// `device`: the configured input's name, `None` for the default.
    pub fn new(device: Option<String>) -> Self {
        Self {
            levels: Arc::new(Levels::default()),
            meter: Meter::default(),
            started: None,
            heard: false,
            device,
            failed: None,
        }
    }

    /// Where the device's frames go: [`Levels::publish_mic`].
    pub fn levels(&self) -> Arc<Levels> {
        self.levels.clone()
    }

    /// The device could not be opened.
    pub fn fail(&mut self, message: impl Into<String>) {
        self.failed = Some(message.into());
    }

    /// Reads the microphone's level and moves time to `now`. Returns whether
    /// what the check says changed.
    pub fn update(&mut self, now: f64) -> bool {
        let before = self.state();
        self.started.get_or_insert(now);
        // Peek at the level the meter takes: any sound at all.
        let probe = Probe {
            levels: &self.levels,
            seen: std::cell::Cell::new(None),
        };
        self.meter.update(&probe, &[], now);
        if probe.seen.get().is_some_and(|db| db > QUIET_DB) {
            self.heard = true;
        }
        before != self.state_at(now)
    }

    pub fn meter(&self) -> &Meter {
        &self.meter
    }

    pub fn state(&self) -> MicState {
        self.state_at(self.meter.now())
    }

    fn state_at(&self, now: f64) -> MicState {
        if let Some(message) = &self.failed {
            return MicState::Failed(message.clone());
        }
        if self.heard {
            return MicState::Works;
        }
        match self.started {
            Some(started) if now - started >= NO_SIGNAL_SECONDS => MicState::NoSignal,
            _ => MicState::Listening,
        }
    }

    /// The input device as the check names it.
    pub fn device_label(&self) -> String {
        match &self.device {
            Some(name) => format!("“{}”", crate::protocol::artifact::display(name, 40)),
            None => "the default microphone".into(),
        }
    }

    /// The line under the lane.
    pub fn message(&self) -> String {
        match self.state() {
            MicState::Listening => "say something: your level shows here".into(),
            MicState::Works => "your microphone works · nothing is sent until you join".into(),
            MicState::NoSignal => format!(
                "no sound from {} · pick another with :huddle input <name>",
                self.device_label()
            ),
            MicState::Failed(message) => message,
        }
    }
}

/// Reads the microphone slot for the meter and remembers what it read.
struct Probe<'a> {
    levels: &'a Levels,
    seen: std::cell::Cell<Option<f32>>,
}

impl super::meter::LevelSource for Probe<'_> {
    fn take_peer(&self, _index: u8) -> Option<f32> {
        None
    }
    fn take_mic(&self) -> Option<f32> {
        let db = self.levels.take_mic();
        self.seen.set(db);
        db
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::huddle::synth;

    /// A fake device: 20 ms frames into the check's levels, as the device
    /// layer sends them, while the interface reads at 30 frames a second.
    fn run(check: &mut MicCheck, seconds: f64, frame: impl Fn(f64) -> Option<Vec<f32>>) -> u32 {
        let levels = check.levels();
        let mut changes = 0;
        let mut t = 0.0;
        let mut next_frame = 0.0;
        while t <= seconds {
            while next_frame <= t {
                if let Some(samples) = frame(next_frame) {
                    levels.publish_mic(&samples);
                }
                next_frame += 0.02;
            }
            changes += u32::from(check.update(t));
            t += 1.0 / 30.0;
        }
        changes
    }

    #[test]
    fn a_voice_shows_in_the_lane_and_says_the_microphone_works() {
        let mut check = MicCheck::new(None);
        run(&mut check, 1.0, |t| {
            Some(if t > 0.4 {
                synth::tone(220.0, -24.0, 0, 960)
            } else {
                vec![0.0; 960]
            })
        });
        assert_eq!(check.state(), MicState::Works);
        assert!(check.meter().mic().speaking());
        assert!(check.meter().mic().column(0) > 0.3);
        assert!(check.message().contains("your microphone works"));
    }

    /// Frames of digital silence, or no frames at all: after 3 s the check
    /// names the device, and not before.
    #[test]
    fn three_seconds_of_nothing_names_the_device() {
        for frames in [true, false] {
            let mut check = MicCheck::new(Some("USB Mic".into()));
            run(&mut check, 2.9, |_| frames.then(|| vec![0.0; 960]));
            assert_eq!(check.state(), MicState::Listening, "{frames}");
            run(&mut check, 3.1, |_| frames.then(|| vec![0.0; 960]));
            assert_eq!(check.state(), MicState::NoSignal, "{frames}");
            assert_eq!(
                check.message(),
                "no sound from “USB Mic” · pick another with :huddle input <name>"
            );
        }
        let mut check = MicCheck::new(None);
        run(&mut check, 3.5, |_| None);
        assert!(
            check
                .message()
                .starts_with("no sound from the default microphone")
        );
        // A hiss below the quiet floor is still nothing.
        let mut check = MicCheck::new(None);
        run(&mut check, 3.5, |_| Some(synth::tone(100.0, -75.0, 0, 960)));
        assert_eq!(check.state(), MicState::NoSignal);
    }

    #[test]
    fn a_device_that_fails_says_why() {
        let mut check = MicCheck::new(None);
        check.fail("microphone unavailable");
        check.update(0.0);
        assert_eq!(check.message(), "microphone unavailable");
        assert!(!format!("{check:?}").chars().any(|c| c.is_ascii_digit()));
    }
}
