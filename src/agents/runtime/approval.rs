//! Questions an agent's host puts to the owner before a real command starts
//! (`_bzz/approve`, plan v0.15.0 M7, `docs/agent-host-protocol.md` section 9).
//!
//! The rules this file keeps:
//!
//! - **A question is answered once.** It is open while it is in the list.
//!   Whoever takes it out decides: the owner's answer, its time limit, or
//!   whatever makes it void. An answer for a question that is no longer in
//!   the list does nothing, so a late answer starts nothing.
//! - **One question per conversation.** A session that has one open is not
//!   given a second.
//! - **Nobody to ask means no.** A question is opened only while someone
//!   is at bzz to answer it ([`Approvals::set_owner_present`]).

use std::{
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

use tokio::sync::oneshot;
use uuid::Uuid;

/// The longest reason an owner can give with a refusal.
pub const MAX_REASON_CHARS: usize = 200;
/// The longest command bzz puts to the owner, as the host's own limit.
pub const MAX_COMMAND_BYTES: usize = 100_000;

/// One question, as the owner is shown it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Asked {
    /// Names the question for as long as this bzz runs. Never reused.
    pub id: u64,
    pub agent: Uuid,
    pub agent_name: String,
    /// The conversation the command was asked for in.
    pub channel: Uuid,
    /// `<channel>` or `<channel>/<thread root>`.
    pub place: String,
    /// Who wrote the message the agent is answering (hex).
    pub requester: String,
    /// The granted folder's name: the agent sees it as `/mnt/<folder>`.
    pub folder: String,
    /// Where that folder is on this machine.
    pub path: PathBuf,
    /// The command exactly as the shell will receive it.
    pub command: String,
    /// Unix seconds.
    pub asked_at: u64,
    /// When it counts as refused.
    pub expires: Instant,
}

/// What the owner answers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Answer {
    Allow,
    /// With a short reason for the agent, or none.
    Refuse(Option<String>),
}

/// How a question ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    Allowed,
    Refused(Option<String>),
    /// The turn was cancelled, the agent paused or stopped, or bzz is
    /// closing: nobody is waiting for the answer any more.
    Void(&'static str),
}

struct Open {
    asked: Asked,
    session: String,
    answer: oneshot::Sender<Outcome>,
}

/// The open questions of every agent of one host.
#[derive(Default)]
pub struct Approvals {
    open: Mutex<Vec<Open>>,
    next: AtomicU64,
    owner_present: AtomicBool,
}

/// Why a question was not opened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotAsked {
    /// Nobody is at bzz to answer.
    NobodyToAsk,
    /// This conversation already has a question open.
    OneAtATime,
}

/// A reason as the agent is given it: one line, bounded, nothing that is
/// not printable.
pub fn clean_reason(reason: &str) -> Option<String> {
    let clean = reason
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    (!clean.is_empty()).then(|| clean.chars().take(MAX_REASON_CHARS).collect())
}

impl Approvals {
    fn open(&self) -> std::sync::MutexGuard<'_, Vec<Open>> {
        self.open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Says whether someone is at bzz to answer: the TUI is, `bzz agent
    /// run` is not. When the owner leaves, what is open becomes void.
    pub fn set_owner_present(&self, present: bool) {
        self.owner_present.store(present, Ordering::Release);
        if !present {
            self.void(|_| true, "nobody is at bzz to answer any more");
        }
    }

    pub fn owner_present(&self) -> bool {
        self.owner_present.load(Ordering::Acquire)
    }

    /// Opens a question for `session` of the agent in `asked`. The id in
    /// `asked` is set here.
    pub(crate) fn ask(
        &self,
        mut asked: Asked,
        session: &str,
    ) -> Result<(u64, oneshot::Receiver<Outcome>), NotAsked> {
        let mut open = self.open();
        // Looked at under the lock: an owner who leaves voids what is open,
        // and nothing is opened after that.
        if !self.owner_present() {
            return Err(NotAsked::NobodyToAsk);
        }
        if open
            .iter()
            .any(|entry| entry.asked.agent == asked.agent && entry.session == session)
        {
            return Err(NotAsked::OneAtATime);
        }
        asked.id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let id = asked.id;
        let (answer, outcome) = oneshot::channel();
        open.push(Open {
            asked,
            session: session.to_owned(),
            answer,
        });
        Ok((id, outcome))
    }

    /// The questions that wait for the owner, oldest first.
    pub fn waiting(&self) -> Vec<Asked> {
        self.open()
            .iter()
            .map(|entry| entry.asked.clone())
            .collect()
    }

    fn take(&self, id: u64) -> Option<Open> {
        let mut open = self.open();
        let index = open.iter().position(|entry| entry.asked.id == id)?;
        Some(open.remove(index))
    }

    /// The owner's answer to the question `id`. `false`: it is no longer
    /// open (answered, timed out or void), and nothing happens.
    pub fn answer(&self, id: u64, answer: Answer) -> bool {
        let Some(entry) = self.take(id) else {
            return false;
        };
        let outcome = match answer {
            Answer::Allow => Outcome::Allowed,
            Answer::Refuse(reason) => Outcome::Refused(reason.as_deref().and_then(clean_reason)),
        };
        // `false` when the waiting side has gone meanwhile: nothing starts.
        entry.answer.send(outcome).is_ok()
    }

    /// Closes the question `id` without an answer: its time is up, or its
    /// waiting side is leaving. `true`: it was still open.
    pub(crate) fn close(&self, id: u64) -> bool {
        self.take(id).is_some()
    }

    fn void(&self, which: impl Fn(&Open) -> bool, why: &'static str) {
        let gone = {
            let mut open = self.open();
            let (gone, kept) = std::mem::take(&mut *open)
                .into_iter()
                .partition::<Vec<_>, _>(|entry| which(entry));
            *open = kept;
            gone
        };
        for entry in gone {
            let _ = entry.answer.send(Outcome::Void(why));
        }
    }

    /// Makes void the question of one conversation of an agent.
    pub(crate) fn void_session(&self, agent: Uuid, session: &str, why: &'static str) {
        self.void(
            |entry| entry.asked.agent == agent && entry.session == session,
            why,
        );
    }

    /// Makes void every question of an agent.
    pub(crate) fn void_agent(&self, agent: Uuid, why: &'static str) {
        self.void(|entry| entry.asked.agent == agent, why);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn asked(agent: Uuid, command: &str) -> Asked {
        Asked {
            id: 0,
            agent,
            agent_name: "Scout".into(),
            channel: Uuid::nil(),
            place: "place".into(),
            requester: "a".repeat(64),
            folder: "site".into(),
            path: "/home/ann/site".into(),
            command: command.into(),
            asked_at: 1,
            expires: Instant::now() + Duration::from_secs(60),
        }
    }

    #[tokio::test]
    async fn a_question_is_answered_once_and_a_late_answer_does_nothing() {
        let approvals = Approvals::default();
        let (scout, ranger) = (Uuid::new_v4(), Uuid::new_v4());
        // Nobody is there: nothing is opened.
        assert_eq!(
            approvals.ask(asked(scout, "npm test"), "s1").err(),
            Some(NotAsked::NobodyToAsk)
        );
        approvals.set_owner_present(true);
        let (first, first_outcome) = approvals.ask(asked(scout, "npm test"), "s1").unwrap();
        // One per conversation; another conversation and another agent may
        // each have one.
        assert_eq!(
            approvals.ask(asked(scout, "rm -rf ."), "s1").err(),
            Some(NotAsked::OneAtATime)
        );
        let (second, second_outcome) = approvals.ask(asked(scout, "make"), "s2").unwrap();
        let (third, third_outcome) = approvals.ask(asked(ranger, "ls"), "s1").unwrap();
        assert_eq!(
            approvals
                .waiting()
                .iter()
                .map(|asked| (asked.id, asked.command.as_str()))
                .collect::<Vec<_>>(),
            [(first, "npm test"), (second, "make"), (third, "ls")]
        );
        assert!(first != second && second != third);

        // Allowed, once: the second answer finds nothing.
        assert!(approvals.answer(first, Answer::Allow));
        assert_eq!(first_outcome.await, Ok(Outcome::Allowed));
        assert!(!approvals.answer(first, Answer::Allow), "a late answer");
        assert!(!approvals.answer(first, Answer::Refuse(None)));
        // Refused, with a reason made one bounded line.
        assert!(approvals.answer(
            second,
            Answer::Refuse(Some(format!("not\u{1b}[31m now\n{}", "x".repeat(500))))
        ));
        let Ok(Outcome::Refused(Some(reason))) = second_outcome.await else {
            panic!("a refusal with its reason")
        };
        assert!(reason.starts_with("not [31m now x"), "{reason}");
        assert_eq!(reason.chars().count(), MAX_REASON_CHARS);
        assert_eq!(clean_reason(" \n "), None);

        // Void: the agent stops. An answer after that does nothing.
        approvals.void_agent(ranger, "the agent was stopped");
        assert_eq!(
            third_outcome.await,
            Ok(Outcome::Void("the agent was stopped"))
        );
        assert!(!approvals.answer(third, Answer::Allow));
        assert!(approvals.waiting().is_empty());

        // Timed out: the waiting side closes it, and the owner's answer
        // comes too late.
        let (timed, _outcome) = approvals.ask(asked(scout, "sleep 9"), "s1").unwrap();
        assert!(approvals.close(timed));
        assert!(!approvals.close(timed));
        assert!(!approvals.answer(timed, Answer::Allow));

        // The owner leaves: what is open is void, and nothing more opens.
        let (_, left) = approvals.ask(asked(scout, "make"), "s1").unwrap();
        let (_, other) = approvals.ask(asked(scout, "make"), "s3").unwrap();
        approvals.void_session(scout, "s3", "the turn was stopped");
        assert_eq!(other.await, Ok(Outcome::Void("the turn was stopped")));
        approvals.set_owner_present(false);
        assert!(matches!(left.await, Ok(Outcome::Void(_))));
        assert_eq!(
            approvals.ask(asked(scout, "make"), "s1").err(),
            Some(NotAsked::NobodyToAsk)
        );
    }

    #[tokio::test]
    async fn an_answer_whose_asker_has_gone_starts_nothing() {
        let approvals = Approvals::default();
        approvals.set_owner_present(true);
        let (id, outcome) = approvals.ask(asked(Uuid::new_v4(), "make"), "s1").unwrap();
        // The harness died and the task that waited was dropped.
        drop(outcome);
        assert!(!approvals.answer(id, Answer::Allow));
    }
}
