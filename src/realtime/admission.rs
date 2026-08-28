use std::time::Duration;

use crate::error::Error;

pub const FRAME_INTERVAL: Duration = Duration::from_millis(125);
pub const DEFAULT_RETRY_AFTER: Duration = Duration::from_secs(10);
pub const MAX_RETRY_AFTER: Duration = Duration::from_secs(300);
pub const PUBLICATION_QUEUE_TIMEOUT: Duration = Duration::from_secs(25);
pub const MAX_PENDING_PUBLICATIONS: usize = 64;
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClosureDisposition {
    RateLimited { retry_after: Duration },
    Retryable,
    TerminalAccess,
    TerminalProtocol,
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
