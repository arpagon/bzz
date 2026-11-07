//! Injectable time sources.
//!
//! Monotonic deadlines (backoff, pacing, TTLs measured from "now") use
//! `tokio::time::Instant`, so tests control them with paused Tokio time
//! (`#[tokio::test(start_paused = true)]` plus `tokio::time::advance`).
//!
//! Wall-clock facts carried by signed events (NIP-38 `expiration`, NIP-FI
//! `exp`, presence freshness, NIP-OA `created_at` bounds) are compared with Unix
//! seconds from a [`WallClock`]. Production code takes a [`SharedClock`];
//! tests pass a [`ManualClock`] and move it explicitly.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// A source of Unix seconds.
pub trait WallClock: Send + Sync + std::fmt::Debug {
    fn now_unix(&self) -> u64;
}

pub type SharedClock = Arc<dyn WallClock>;

/// The operating-system clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl WallClock for SystemClock {
    fn now_unix(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs())
    }
}

/// The production clock as a shared handle.
pub fn system() -> SharedClock {
    Arc::new(SystemClock)
}

/// A test clock that only moves when told to.
#[derive(Debug, Default)]
pub struct ManualClock {
    now: AtomicU64,
}

impl ManualClock {
    pub fn new(now_unix: u64) -> Arc<Self> {
        Arc::new(Self {
            now: AtomicU64::new(now_unix),
        })
    }

    pub fn set(&self, now_unix: u64) {
        self.now.store(now_unix, Ordering::SeqCst);
    }

    pub fn advance(&self, seconds: u64) {
        self.now.fetch_add(seconds, Ordering::SeqCst);
    }
}

impl WallClock for ManualClock {
    fn now_unix(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }
}

/// Whether a Unix-second deadline has passed. Equality is expired, matching
/// NIP-FI and NIP-38 "at or after" semantics.
pub fn is_expired(clock: &dyn WallClock, deadline_unix: u64) -> bool {
    clock.now_unix() >= deadline_unix
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_moves_only_when_told() {
        let clock = ManualClock::new(1_000);
        assert_eq!(clock.now_unix(), 1_000);
        clock.advance(5);
        assert_eq!(clock.now_unix(), 1_005);
        clock.set(42);
        assert_eq!(clock.now_unix(), 42);
    }

    #[test]
    fn deadline_equality_is_expired() {
        let clock = ManualClock::new(100);
        assert!(!is_expired(clock.as_ref(), 101));
        assert!(is_expired(clock.as_ref(), 100));
        assert!(is_expired(clock.as_ref(), 99));
    }

    #[test]
    fn system_clock_is_after_2025() {
        assert!(SystemClock.now_unix() > 1_735_689_600);
    }
}
