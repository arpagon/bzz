//! Credentials of `bzz agent-tools` (`docs/agent-host-protocol.md`, section
//! 6): one per session of one agent, made by bzz when it opens the session,
//! held in memory as a hash only, and revoked when the agent's process exits.
//!
//! The credential alone says who calls. A tool call names no agent, no
//! session and no request, so a credential can act only for its own session.

use std::{collections::HashMap, sync::Mutex};

use uuid::Uuid;

use crate::broker::host::{constant_time_eq, credential_hash};

const PREFIX: &str = "bzzt_";

/// The session a credential was made for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Grant {
    pub agent: Uuid,
    /// Which start of the agent's process it belongs to.
    pub run: u64,
    /// Absent until the harness has answered `session/new`.
    pub session: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refused {
    /// Not a credential bzz made, or one whose process has exited.
    UnknownOrRevoked,
    /// Made, but its session is not open yet.
    SessionNotOpen,
}

#[derive(Default)]
pub struct Credentials {
    /// Hash of the credential -> its grant.
    grants: Mutex<HashMap<String, Grant>>,
}

impl Credentials {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Grant>> {
        self.grants
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A fresh 256-bit credential for a session about to be opened.
    pub fn mint(&self, agent: Uuid, run: u64) -> String {
        let credential = format!("{PREFIX}{}", hex::encode(rand::random::<[u8; 32]>()));
        self.lock().insert(
            credential_hash(&credential),
            Grant {
                agent,
                run,
                session: None,
            },
        );
        credential
    }

    /// Ties a credential to the session the harness opened with it.
    pub fn bind(&self, credential: &str, session: &str) {
        if let Some(grant) = self.lock().get_mut(&credential_hash(credential)) {
            grant.session = Some(session.to_owned());
        }
    }

    /// Drops one credential whose session never opened.
    pub fn forget(&self, credential: &str) {
        self.lock().remove(&credential_hash(credential));
    }

    /// Revokes every credential of an agent: its process exited.
    pub fn revoke_agent(&self, agent: Uuid) -> usize {
        let mut grants = self.lock();
        let before = grants.len();
        grants.retain(|_, grant| grant.agent != agent);
        before - grants.len()
    }

    /// Who an `Authorization` header speaks for.
    pub fn resolve(&self, authorization: Option<&str>) -> Result<(Uuid, u64, String), Refused> {
        let token = authorization
            .and_then(|header| header.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|token| token.starts_with(PREFIX))
            .ok_or(Refused::UnknownOrRevoked)?;
        let hash = credential_hash(token);
        let grants = self.lock();
        let mut found = None;
        // Compared against every grant, so timing does not tell which.
        for (stored, grant) in grants.iter() {
            if constant_time_eq(stored.as_bytes(), hash.as_bytes()) {
                found = Some(grant.clone());
            }
        }
        let grant = found.ok_or(Refused::UnknownOrRevoked)?;
        let session = grant.session.ok_or(Refused::SessionNotOpen)?;
        Ok((grant.agent, grant.run, session))
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bearer(credential: &str) -> String {
        format!("Bearer {credential}")
    }

    #[test]
    fn a_credential_speaks_for_one_session_and_dies_with_the_process() {
        let credentials = Credentials::default();
        let (scout, other) = (Uuid::new_v4(), Uuid::new_v4());
        let first = credentials.mint(scout, 1);
        let second = credentials.mint(scout, 1);
        let foreign = credentials.mint(other, 4);
        assert_ne!(first, second);
        assert!(first.len() >= PREFIX.len() + 64, "256 bits");
        // Not usable before its session exists.
        assert_eq!(
            credentials.resolve(Some(&bearer(&first))),
            Err(Refused::SessionNotOpen)
        );
        credentials.bind(&first, "session-a");
        credentials.bind(&second, "session-b");
        credentials.bind(&foreign, "session-a");
        assert_eq!(
            credentials.resolve(Some(&bearer(&first))),
            Ok((scout, 1, "session-a".into()))
        );
        assert_eq!(
            credentials.resolve(Some(&bearer(&second))),
            Ok((scout, 1, "session-b".into())),
            "each session has its own"
        );
        // Another agent's credential resolves to that agent, never to this one.
        assert_eq!(
            credentials.resolve(Some(&bearer(&foreign))),
            Ok((other, 4, "session-a".into()))
        );
        // Anything bzz did not make is refused: no header, another scheme, a
        // v0.13 broker credential, a guess.
        let guess = format!("{PREFIX}{}", "0".repeat(64));
        for header in [
            None,
            Some("Bearer "),
            Some("Basic abc"),
            Some(first.as_str()),
            Some("Bearer bzzb_0000"),
            Some(&format!("Bearer {guess}")),
        ] {
            assert_eq!(
                credentials.resolve(header),
                Err(Refused::UnknownOrRevoked),
                "{header:?}"
            );
        }
        // The process exits: both of its credentials stop resolving at once.
        assert_eq!(credentials.revoke_agent(scout), 2);
        for revoked in [&first, &second] {
            assert_eq!(
                credentials.resolve(Some(&bearer(revoked))),
                Err(Refused::UnknownOrRevoked)
            );
        }
        assert!(credentials.resolve(Some(&bearer(&foreign))).is_ok());
        credentials.forget(&foreign);
        assert!(credentials.is_empty());
    }

    #[test]
    fn the_credential_is_kept_only_as_a_hash() {
        let credentials = Credentials::default();
        let credential = credentials.mint(Uuid::new_v4(), 1);
        let held = format!("{:?}", credentials.lock());
        assert!(!held.contains(&credential) && !held.contains(&credential[PREFIX.len()..]));
    }
}
