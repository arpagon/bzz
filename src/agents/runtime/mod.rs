//! The local agent runtime (v0.15 M1, ADR v0.15 local agents): bzz starts an
//! agent's harness, speaks ACP to it, decides who wakes it and signs what it
//! says. The wire contract is `docs/agent-host-protocol.md`.
//!
//! - [`process`]: one process group per agent, ownership records, the sweep.
//! - [`acp`]: the protocol's framing and one connection.
//! - [`admission`]: who may wake an agent, and the place of a request.
//! - [`tools`]: `send`, `read` and `react`, their checked arguments.
//! - [`credentials`], [`endpoint`], [`mcp`]: `bzz agent-tools`.
//! - [`agent`]: one agent's turns, with the reply ledger of
//!   [`crate::store::agent_runtime`].
//! - [`host`]: every local agent of one community.
//! - [`relay`]: the agent's own relay connection, and its intake.
//! - [`service`]: all of it on the real relay, for the TUI and `agent run`.

pub mod acp;
pub mod admission;
pub mod agent;
pub mod credentials;
pub mod endpoint;
pub mod host;
pub mod mcp;
pub mod process;
pub mod prompt;
pub mod relay;
pub mod service;
pub mod tools;

use std::time::Duration;

/// Why a turn failed. The notice posted in the thread and every log carry
/// only this fixed kind, because a harness's own error text can hold private
/// configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureKind {
    LimitReached,
    SignInNeeded,
    ProviderError,
    /// The turn's time limit passed.
    Timeout,
    /// The harness died and the request could not be handed over again.
    Crashed,
    /// The harness died after something of the turn was published, and it
    /// cannot tell a request it has seen: answering again could repeat it.
    Interrupted,
    /// The turn ended with no `send` and no text.
    NoReply,
    Refused,
    /// The harness keeps dying or cannot be started.
    Unavailable,
}

impl FailureKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LimitReached => "limit-reached",
            Self::SignInNeeded => "sign-in-needed",
            Self::ProviderError => "provider-error",
            Self::Timeout => "timeout",
            Self::Crashed => "crashed",
            Self::Interrupted => "interrupted",
            Self::NoReply => "no-reply",
            Self::Refused => "refused",
            Self::Unavailable => "unavailable",
        }
    }

    /// The reply posted in the thread when a turn fails.
    pub const fn notice(self) -> &'static str {
        match self {
            Self::LimitReached => {
                "I could not answer: my model's usage limit was reached. Try again later."
            }
            Self::SignInNeeded => {
                "I could not answer: my owner needs to sign in to my model again."
            }
            Self::ProviderError => "I could not answer: my model returned an error.",
            Self::Timeout => "I could not answer in time, and stopped.",
            Self::Crashed => "I could not answer: my process stopped unexpectedly.",
            Self::Interrupted => {
                "I was interrupted while answering and may have left this unfinished. Ask again if you still need it."
            }
            Self::NoReply => "I finished without an answer.",
            Self::Refused => "I could not answer this request.",
            Self::Unavailable => "I am not available right now: my process keeps stopping.",
        }
    }

    /// Names a harness's JSON-RPC error by kind, from its code and from
    /// words providers use. The text itself is never shown.
    pub fn classify(error: &acp::RpcError) -> Self {
        let text = error.message.to_ascii_lowercase();
        let has = |words: &[&str]| words.iter().any(|word| text.contains(word));
        if error.code == acp::AUTH_REQUIRED
            || has(&[
                "sign in",
                "signed in",
                "log in",
                "login",
                "unauthorized",
                "authenticat",
                "401",
            ])
        {
            Self::SignInNeeded
        } else if has(&[
            "rate limit",
            "rate-limit",
            "usage limit",
            "quota",
            "limit reached",
            "429",
        ]) {
            Self::LimitReached
        } else {
            Self::ProviderError
        }
    }
}

/// The runtime's time limits. Tests shorten them.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// For a harness to answer `initialize`.
    pub start: Duration,
    /// For a harness to open a session.
    pub session: Duration,
    /// For one turn.
    pub turn: Duration,
    /// For a harness to answer a cancelled turn before it is stopped.
    pub cancel_grace: Duration,
    /// Between the steps of stopping a harness.
    pub stop_grace: Duration,
    /// Between two typing indicators of a running turn.
    pub typing_interval: Duration,
    /// A harness that dies `max_crashes` times within `crash_window` is not
    /// started again until the window has passed.
    pub crash_window: Duration,
    pub max_crashes: usize,
    /// The most times one request is handed to a harness. Fewer than
    /// `max_crashes`, so that one request a harness cannot survive ends with
    /// its notice and leaves the agent available to the others.
    pub max_attempts: u32,
    /// Before a start is tried again after one failed.
    pub restart_backoff: Duration,
    /// For the relay to acknowledge one event.
    pub publish: Duration,
    /// Turns of one agent that may run at once.
    pub concurrent_turns: usize,
    /// Messages and reactions one turn may publish besides its final reply.
    pub effects_per_turn: u32,
    /// How long bzz's own host is idle before it is asked to checkpoint its
    /// files (`_bzz/checkpoint`).
    pub checkpoint_idle: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            start: Duration::from_secs(30),
            session: Duration::from_secs(30),
            turn: Duration::from_secs(600),
            cancel_grace: Duration::from_secs(5),
            stop_grace: process::STOP_GRACE,
            typing_interval: Duration::from_secs(5),
            crash_window: Duration::from_secs(300),
            max_crashes: 5,
            max_attempts: 3,
            restart_backoff: Duration::from_secs(1),
            publish: Duration::from_secs(15),
            concurrent_turns: 4,
            effects_per_turn: 20,
            checkpoint_idle: Duration::from_secs(60),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_are_named_by_kind_from_code_and_wording() {
        let kind = |code, message: &str| FailureKind::classify(&acp::RpcError::new(code, message));
        assert_eq!(kind(acp::AUTH_REQUIRED, ""), FailureKind::SignInNeeded);
        assert_eq!(
            kind(-32603, "Please log in again"),
            FailureKind::SignInNeeded
        );
        assert_eq!(
            kind(-32603, "HTTP 401 Unauthorized"),
            FailureKind::SignInNeeded
        );
        assert_eq!(
            kind(-32603, "You hit your usage limit"),
            FailureKind::LimitReached
        );
        assert_eq!(
            kind(-32603, "429 Too Many Requests"),
            FailureKind::LimitReached
        );
        assert_eq!(
            kind(-32603, "Rate limit exceeded"),
            FailureKind::LimitReached
        );
        assert_eq!(
            kind(-32603, "upstream connect error"),
            FailureKind::ProviderError
        );
        // Every kind has its own name and its own notice.
        let all = [
            FailureKind::LimitReached,
            FailureKind::SignInNeeded,
            FailureKind::ProviderError,
            FailureKind::Timeout,
            FailureKind::Crashed,
            FailureKind::Interrupted,
            FailureKind::NoReply,
            FailureKind::Refused,
            FailureKind::Unavailable,
        ];
        let names = all
            .iter()
            .map(|kind| kind.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let notices = all
            .iter()
            .map(|kind| kind.notice())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!((names.len(), notices.len()), (all.len(), all.len()));
    }
}
