//! The local agent runtime (v0.15 M1, ADR v0.15 local agents): bzz starts an
//! agent's harness, speaks ACP to it, decides who wakes it and signs what it
//! says. The wire contract is `docs/agent-host-protocol.md`.
//!
//! - [`process`]: one process group per agent, ownership records, the sweep.
//! - [`acp`]: the protocol's framing and one connection.
//! - [`admission`]: who may wake an agent, and the place of a request.
//! - [`tools`]: `send`, `read` and `react`, their checked arguments.
//! - [`approval`]: a real command put to the owner before it starts.
//! - [`credentials`], [`endpoint`], [`mcp`]: `bzz agent-tools`.
//! - [`coding`]: Grok, Codex and Claude Code: where each is, the versions
//!   bzz starts, its adapter, its guard, and how it takes a session.
//! - [`permission`]: what a coding harness asks leave for, and the rule.
//! - [`record`]: a transcript of one harness process, for the tests.
//! - [`agent`]: one agent's turns, with the reply ledger of
//!   [`crate::store::agent_runtime`].
//! - [`host`]: every local agent of one community.
//! - [`install`]: bzz's own Pi Durable host, its Node and its install.
//! - [`relay`]: the agent's own relay connection, and its intake.
//! - [`service`]: all of it on the real relay, for the TUI and `agent run`.

pub mod acp;
pub mod admission;
pub mod agent;
pub mod approval;
pub mod coding;
pub mod credentials;
pub mod endpoint;
pub mod host;
pub mod install;
pub mod mcp;
pub mod permission;
pub mod process;
pub mod prompt;
pub mod record;
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
    /// The model the owner chose cannot be used, or none is set.
    ModelUnavailable,
    /// The owner stopped the turn from the agents panel.
    Stopped,
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
    /// A coding harness did not take the mode bzz sets on every session,
    /// so nothing was sent to it.
    NotGuarded,
    /// The turn ended without an answer after bzz refused something the
    /// harness asked leave for.
    NotAllowed,
}

impl FailureKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LimitReached => "limit-reached",
            Self::SignInNeeded => "sign-in-needed",
            Self::ProviderError => "provider-error",
            Self::ModelUnavailable => "model-unavailable",
            Self::Stopped => "stopped",
            Self::Timeout => "timeout",
            Self::Crashed => "crashed",
            Self::Interrupted => "interrupted",
            Self::NoReply => "no-reply",
            Self::Refused => "refused",
            Self::Unavailable => "unavailable",
            Self::NotGuarded => "not-guarded",
            Self::NotAllowed => "not-allowed",
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
            Self::ModelUnavailable => {
                "I could not answer: the model my owner chose for me is not available."
            }
            Self::Stopped => "My owner stopped this answer.",
            Self::Timeout => "I could not answer in time, and stopped.",
            Self::Crashed => "I could not answer: my process stopped unexpectedly.",
            Self::Interrupted => {
                "I was interrupted while answering and may have left this unfinished. Ask again if you still need it."
            }
            Self::NoReply => "I finished without an answer.",
            Self::Refused => "I could not answer this request.",
            Self::Unavailable => "I am not available right now: my process keeps stopping.",
            Self::NotGuarded => {
                "I could not answer: my program did not accept the limits bzz sets on it, so nothing was sent to it."
            }
            Self::NotAllowed => {
                "I could not finish: I needed something on my owner's machine that my owner has not allowed."
            }
        }
    }

    /// Names a harness's JSON-RPC error by kind, from its code and from
    /// words providers use. The text itself is never shown.
    ///
    /// bzz's own host names its failures itself (`error.data.kind`) and uses
    /// one code of its own for all of them ([`acp::TURN_FAILED`]). An older
    /// host used -32000 there, which is also ACP's code for "sign in": with
    /// a kind, the code says nothing and the kind decides.
    pub fn classify(error: &acp::RpcError) -> Self {
        let text = error.message.to_ascii_lowercase();
        let has = |words: &[&str]| words.iter().any(|word| text.contains(word));
        match error.kind.as_deref() {
            Some("sign_in_needed") => return Self::SignInNeeded,
            Some("provider_unavailable" | "model_unavailable" | "no_model") => {
                return Self::ModelUnavailable;
            }
            // bzz's own word for a session it would not prompt.
            Some("not_guarded") => return Self::NotGuarded,
            _ => {}
        }
        if (error.code == acp::AUTH_REQUIRED && error.kind.is_none())
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
    /// How many times another agent may wake this one in one conversation
    /// within `agent_exchange_window`. People are not counted.
    pub agent_exchanges: u32,
    pub agent_exchange_window: Duration,
    /// Between two prunings of an agent's finished requests and idle
    /// sessions (`Store::agent_prune`).
    pub prune_interval: Duration,
    /// How long a real command waits for the owner's answer before it
    /// counts as refused (`_bzz/approve`).
    pub approval: Duration,
}

/// How long bzz's own host gives a Pi package to load, all of them at the
/// same time (`agent-host/src/packages.mjs`, `PACKAGE_MS`). `initialize` is
/// answered only after that, so [`Limits::start`] must be longer.
pub const HOST_PACKAGE_LIMIT: Duration = Duration::from_secs(10);

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
            agent_exchanges: 6,
            agent_exchange_window: Duration::from_secs(3600),
            prune_interval: Duration::from_secs(24 * 3600),
            approval: Duration::from_secs(300),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_limits_fit_what_the_host_and_the_owner_need() {
        let limits = Limits::default();
        // `initialize` waits for the host's packages; a package that never
        // gives the thread back holds the host, and bzz stops it then.
        assert!(limits.start > HOST_PACKAGE_LIMIT * 2, "{:?}", limits.start);
        // An owner has minutes to answer, and the turn outlasts the wait.
        assert!(limits.approval >= Duration::from_secs(60));
        assert!(limits.turn > limits.approval);
    }

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
        // bzz's own host: one code of its own for every failure, and a kind
        // that says which.
        let named = |name: &str, message: &str| {
            FailureKind::classify(&acp::RpcError::new(acp::TURN_FAILED, message).with_kind(name))
        };
        // Without a kind that code is a failure of the model, never a
        // missing sign-in.
        assert_eq!(kind(acp::TURN_FAILED, ""), FailureKind::ProviderError);
        // An older host used ACP's "sign in" code for every failure: with a
        // kind, the kind decides.
        assert_eq!(
            FailureKind::classify(
                &acp::RpcError::new(acp::AUTH_REQUIRED, "The turn failed: boom")
                    .with_kind("model_error")
            ),
            FailureKind::ProviderError
        );
        assert_eq!(
            named("error", "The turn failed: upstream exploded"),
            FailureKind::ProviderError
        );
        assert_eq!(
            named("error", "The turn failed: 429 Too Many Requests"),
            FailureKind::LimitReached
        );
        assert_eq!(named("sign_in_needed", ""), FailureKind::SignInNeeded);
        for unavailable in ["provider_unavailable", "model_unavailable", "no_model"] {
            assert_eq!(named(unavailable, ""), FailureKind::ModelUnavailable);
        }
        // Every kind has its own name and its own notice.
        let all = [
            FailureKind::LimitReached,
            FailureKind::SignInNeeded,
            FailureKind::ProviderError,
            FailureKind::ModelUnavailable,
            FailureKind::Stopped,
            FailureKind::Timeout,
            FailureKind::Crashed,
            FailureKind::Interrupted,
            FailureKind::NoReply,
            FailureKind::Refused,
            FailureKind::Unavailable,
            FailureKind::NotGuarded,
            FailureKind::NotAllowed,
        ];
        assert_eq!(named("not_guarded", ""), FailureKind::NotGuarded);
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
