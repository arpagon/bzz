use std::time::Duration;

use crate::error::Error;

pub const FRAME_INTERVAL: Duration = Duration::from_millis(125);
pub const DEFAULT_RETRY_AFTER: Duration = Duration::from_secs(10);
pub const MAX_RETRY_AFTER: Duration = Duration::from_secs(300);
pub const PUBLICATION_QUEUE_TIMEOUT: Duration = Duration::from_secs(25);
pub const MAX_PENDING_PUBLICATIONS: usize = 64;
/// App writes wait in a queue of their own, so that no number of them can
/// fill the queue a person's messages use.
pub const MAX_PENDING_APP_PUBLICATIONS: usize = 32;
/// What the supervisor reports for `CLOSED resync: ...`, whatever followed
/// the prefix: the subscription is over and its set must be asked again.
pub const CLOSED_RESYNC: &str = "resync: relay asked for a new sync";
/// What the supervisor reports when one sync matched more than the relay's
/// `max_events`. Asking again gives the same answer.
pub const CLOSED_SYNC_OVERFLOW: &str = "error: sync set exceeds max_events";
const LOCAL_ADMISSION_PREFIX: &str = "rate-limited: local admission ";

pub fn local_admission_error(reason: &'static str) -> Error {
    Error::Network(format!("{LOCAL_ADMISSION_PREFIX}{reason}"))
}

pub fn is_local_admission_error(error: &Error) -> bool {
    matches!(error, Error::Network(message) if message.starts_with(LOCAL_ADMISSION_PREFIX))
}

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum SubscriptionPriority {
    Foreground,
    Baseline,
    #[default]
    Background,
}

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum PublishPriority {
    #[default]
    Interactive,
    Recovery,
    Maintenance,
    /// A write an app asked for (v0.16). It goes out after everything bzz
    /// sends for the person, and is never held while the relay's quota gate
    /// is closed: it fails at once with the wait.
    App,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClosureDisposition {
    RateLimited {
        retry_after: Duration,
    },
    Retryable,
    /// `resync:`: a NIP-AR sync ended because its set can no longer be kept
    /// current. Final for this subscription; the owner asks again.
    Resync,
    /// `error: sync set exceeds max_events`: final, and so is a new attempt
    /// with the same filter.
    SyncOverflow,
    TerminalAccess,
    TerminalProtocol,
}

/// Why a subscription ended for good, read from the fixed text the supervisor
/// puts in `SessionEvent::Closed`. The relay's own words never reach here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClosedReason {
    Resync,
    SyncOverflow,
    Restricted,
    Other,
}

pub fn closed_reason(message: &str) -> ClosedReason {
    let lower = message.trim().to_ascii_lowercase();
    if lower.starts_with("resync:") {
        ClosedReason::Resync
    } else if is_sync_overflow(&lower) {
        ClosedReason::SyncOverflow
    } else if lower.starts_with("restricted:") {
        ClosedReason::Restricted
    } else {
        ClosedReason::Other
    }
}

fn is_sync_overflow(lower: &str) -> bool {
    lower.starts_with("error:") && lower.contains("sync set exceeds max_events")
}

/// The answer to an app's write that bzz did not send: the relay's quota gate
/// is closed, or the app queue is full or too slow. Whole seconds, rounded up.
pub fn app_write_deferred(wait: Duration) -> Error {
    let seconds = wait.as_secs() + u64::from(wait.subsec_nanos() > 0);
    Error::Network(fixed_rate_limit_message(Duration::from_secs(seconds)))
}

/// Reduces an untrusted relay reason to a bounded rate-limit hint. Source text
/// must be discarded by the caller immediately after this function returns.
pub fn rate_limit_retry_after(message: &str) -> Option<Duration> {
    let lower = message.trim().to_ascii_lowercase();
    let prefix = lower
        .split_once(':')
        .map_or(lower.as_str(), |(prefix, _)| prefix.trim());
    if prefix != "rate-limited" && prefix != "rate limited" {
        return None;
    }
    let seconds = lower
        .find("retry in ")
        .and_then(|start| {
            let suffix = &lower[start + "retry in ".len()..];
            let digits = suffix.bytes().take_while(u8::is_ascii_digit).count();
            (digits > 0).then(|| &suffix[..digits])
        })
        .map(|digits| {
            digits.bytes().fold(0_u64, |seconds, digit| {
                seconds
                    .saturating_mul(10)
                    .saturating_add(u64::from(digit - b'0'))
            })
        })
        .filter(|seconds| *seconds > 0)
        .unwrap_or(DEFAULT_RETRY_AFTER.as_secs())
        .min(MAX_RETRY_AFTER.as_secs());
    Some(Duration::from_secs(seconds))
}

pub fn fixed_rate_limit_message(retry_after: Duration) -> String {
    let seconds = retry_after.as_secs().max(1).min(MAX_RETRY_AFTER.as_secs());
    format!("rate-limited: retry in {seconds}s")
}

pub fn classify_closed(message: &str) -> ClosureDisposition {
    if let Some(retry_after) = rate_limit_retry_after(message) {
        return ClosureDisposition::RateLimited { retry_after };
    }
    let lower = message.trim().to_ascii_lowercase();
    if lower.starts_with("resync:") {
        return ClosureDisposition::Resync;
    }
    if is_sync_overflow(&lower) {
        return ClosureDisposition::SyncOverflow;
    }
    if lower.starts_with("blocked:")
        || lower.starts_with("restricted:")
        || lower.starts_with("auth-required:")
        || lower.contains("forbidden")
        || lower.contains("denied")
        || lower.contains("not a member")
        || lower.contains("authentication required")
    {
        ClosureDisposition::TerminalAccess
    } else if lower.contains("temporar")
        || lower.contains("server error")
        || lower.contains("unavailable")
        || lower.contains("timeout")
        || lower.contains("busy")
        || lower.contains("try again")
    {
        ClosureDisposition::Retryable
    } else {
        // Unknown and malformed reasons are fail-closed. Retrying arbitrary
        // relay text would create a remotely controlled hot loop.
        ClosureDisposition::TerminalProtocol
    }
}

pub const fn terminal_message(disposition: ClosureDisposition) -> &'static str {
    match disposition {
        ClosureDisposition::TerminalAccess => "restricted: relay denied the subscription",
        ClosureDisposition::TerminalProtocol => "invalid: relay closed the subscription",
        ClosureDisposition::Resync => CLOSED_RESYNC,
        ClosureDisposition::SyncOverflow => CLOSED_SYNC_OVERFLOW,
        ClosureDisposition::RateLimited { .. } | ClosureDisposition::Retryable => {
            "relay subscription temporarily unavailable"
        }
    }
}

pub fn retry_backoff(attempt: u32) -> Duration {
    Duration::from_secs(1_u64.checked_shl(attempt.min(5)).unwrap_or(30).min(30))
}

/// Stable per-subscription jitter avoids synchronized retries without adding a
/// dependency or making paused-time tests nondeterministic.
pub fn retry_jitter(subscription: &str) -> Duration {
    let hash = subscription.bytes().fold(0_u64, |hash, byte| {
        hash.wrapping_mul(109).wrapping_add(u64::from(byte))
    });
    Duration::from_millis(100 + hash % 401)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_hints_are_canonical_bounded_and_case_insensitive() {
        assert_eq!(
            rate_limit_retry_after("rate-limited: quota exceeded; retry in 1s"),
            Some(Duration::from_secs(1))
        );
        assert_eq!(
            rate_limit_retry_after("RATE-LIMITED: retry in 5s"),
            Some(Duration::from_secs(5))
        );
        assert_eq!(
            rate_limit_retry_after("rate-limited: retry in 0s"),
            Some(DEFAULT_RETRY_AFTER)
        );
        assert_eq!(
            rate_limit_retry_after("rate-limited: retry in 999999999999999999999s"),
            Some(MAX_RETRY_AFTER)
        );
        assert_eq!(
            rate_limit_retry_after("rate-limited: retry in 999s"),
            Some(MAX_RETRY_AFTER)
        );
        assert_eq!(rate_limit_retry_after("quota exceeded"), None);
    }

    #[test]
    fn closure_policy_retries_only_bounded_transient_classes() {
        assert!(matches!(
            classify_closed("rate-limited: retry in 2s"),
            ClosureDisposition::RateLimited { retry_after } if retry_after == Duration::from_secs(2)
        ));
        assert_eq!(
            classify_closed("error: temporarily unavailable"),
            ClosureDisposition::Retryable
        );
        assert_eq!(
            classify_closed("restricted: not a member"),
            ClosureDisposition::TerminalAccess
        );
        assert_eq!(
            classify_closed("hostile nsec1secret /private/path"),
            ClosureDisposition::TerminalProtocol
        );
        assert_eq!(classify_closed(""), ClosureDisposition::TerminalProtocol);
    }

    #[test]
    fn a_closed_sync_says_which_of_three_things_happened() {
        // What the pinned relay sends (M0 question 3), and what reaches the
        // owner of the subscription: a fixed text, never the relay's.
        for (relay, disposition, reason) in [
            (
                "resync: redaction",
                ClosureDisposition::Resync,
                ClosedReason::Resync,
            ),
            (
                "resync: channel access changed",
                ClosureDisposition::Resync,
                ClosedReason::Resync,
            ),
            (
                "resync: nsec1secret access denied",
                ClosureDisposition::Resync,
                ClosedReason::Resync,
            ),
            (
                "error: sync set exceeds max_events",
                ClosureDisposition::SyncOverflow,
                ClosedReason::SyncOverflow,
            ),
            (
                "restricted: channel not readable",
                ClosureDisposition::TerminalAccess,
                ClosedReason::Restricted,
            ),
            (
                "invalid: unsupported artifact predicate",
                ClosureDisposition::TerminalProtocol,
                ClosedReason::Other,
            ),
        ] {
            let classified = classify_closed(relay);
            assert_eq!(classified, disposition, "{relay}");
            let reported = terminal_message(classified);
            assert!(!reported.contains("nsec"), "{reported}");
            assert_eq!(closed_reason(reported), reason, "{relay}");
        }
        // A quota closure of a sync is still a wait, not a resync.
        assert!(matches!(
            classify_closed("rate-limited: quota exceeded; retry in 4s"),
            ClosureDisposition::RateLimited { .. }
        ));
    }

    #[test]
    fn a_deferred_app_write_carries_whole_seconds_rounded_up() {
        for (wait, seconds) in [
            (Duration::from_millis(1), 1),
            (Duration::from_millis(39_200), 40),
            (Duration::from_secs(40), 40),
            (Duration::ZERO, 1),
        ] {
            let error = app_write_deferred(wait);
            let Error::Network(message) = &error else {
                panic!("{error:?}")
            };
            assert_eq!(
                rate_limit_retry_after(message),
                Some(Duration::from_secs(seconds))
            );
            assert!(!is_local_admission_error(&error));
        }
    }

    #[test]
    fn backoff_and_jitter_are_bounded_and_deterministic() {
        assert_eq!(retry_backoff(0), Duration::from_secs(1));
        assert_eq!(retry_backoff(30), Duration::from_secs(30));
        assert_eq!(retry_jitter("same"), retry_jitter("same"));
        assert!(
            (Duration::from_millis(100)..=Duration::from_millis(500))
                .contains(&retry_jitter("subscription"))
        );
    }
}
