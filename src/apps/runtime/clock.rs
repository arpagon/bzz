//! The time an app can see, injected so that a test and the headless driver
//! move it by hand.
//!
//! Two readings. `wall` is what `host.time()` returns. `elapsed` only goes
//! forward: timers and the per-minute limits count in it, so a change of the
//! system's date neither fires nor starves them.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub trait Clock: Send + Sync {
    /// Time since 1970-01-01 UTC.
    fn wall(&self) -> Duration;
    /// Time since this clock was made. Never goes back.
    fn elapsed(&self) -> Duration;
}

pub type SharedClock = Arc<dyn Clock>;

/// The operating system's clocks.
pub struct SystemClock {
    started: Instant,
}

impl SystemClock {
    pub fn shared() -> SharedClock {
        Arc::new(Self {
            started: Instant::now(),
        })
    }
}

impl Clock for SystemClock {
    fn wall(&self) -> Duration {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
    }

    fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}

/// A clock that only moves when told to.
pub struct ManualClock {
    /// Wall time at the start, and time elapsed since.
    now: Mutex<(Duration, Duration)>,
}

impl ManualClock {
    /// A clock that reads `wall_secs` seconds after 1970 and has not moved.
    pub fn new(wall_secs: u64) -> Arc<Self> {
        Arc::new(Self {
            now: Mutex::new((Duration::from_secs(wall_secs), Duration::ZERO)),
        })
    }

    pub fn advance(&self, by: Duration) {
        let mut now = self.now.lock().unwrap_or_else(|e| e.into_inner());
        now.1 += by;
    }
}

impl Clock for ManualClock {
    fn wall(&self) -> Duration {
        let now = self.now.lock().unwrap_or_else(|e| e.into_inner());
        now.0 + now.1
    }

    fn elapsed(&self) -> Duration {
        self.now.lock().unwrap_or_else(|e| e.into_inner()).1
    }
}
