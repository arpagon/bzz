//! Event-driven redraw admission for the terminal render loop.
//!
//! A terminal needs an initial frame, then another frame only after an input,
//! resize, visible domain/UI change, or bounded animation requests one. This
//! keeps idle sessions from emitting a full frame on every housekeeping tick.

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedrawGate {
    dirty: bool,
    frames: u64,
}

impl Default for RedrawGate {
    fn default() -> Self {
        Self {
            // A newly entered terminal has no application frame yet.
            dirty: true,
            frames: 0,
        }
    }
}

impl RedrawGate {
    /// Request exactly one future frame. Repeated requests coalesce until the
    /// caller consumes the frame, preventing an event burst from scheduling
    /// redundant terminal output.
    pub fn request(&mut self) {
        self.dirty = true;
    }

    /// Requests a frame only if the caller knows presentation changed.
    pub fn request_if(&mut self, changed: bool) {
        if changed {
            self.request();
        }
    }

    /// Returns true once per requested frame and records that the caller is
    /// about to draw it.
    pub fn take(&mut self) -> bool {
        if !self.dirty {
            return false;
        }
        self.dirty = false;
        self.frames = self.frames.saturating_add(1);
        true
    }

    #[cfg(test)]
    pub const fn frames(&self) -> u64 {
        self.frames
    }
}

#[cfg(test)]
mod tests {
    use super::RedrawGate;

    #[test]
    fn idle_ticks_do_not_admit_extra_frames() {
        let mut gate = RedrawGate::default();
        assert!(gate.take());
        for _ in 0..1_000 {
            assert!(!gate.take());
        }
        assert_eq!(gate.frames(), 1);
    }

    #[test]
    fn requests_coalesce_until_a_frame_is_drawn() {
        let mut gate = RedrawGate::default();
        assert!(gate.take());
        gate.request();
        gate.request();
        gate.request_if(false);
        assert!(gate.take());
        assert!(!gate.take());
        gate.request_if(true);
        assert!(gate.take());
        assert_eq!(gate.frames(), 3);
    }
}
