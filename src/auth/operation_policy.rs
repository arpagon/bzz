//! What a Buzz Desktop credential provider will sign for bzz.
//!
//! The local credential provider contract (version 1, upstream
//! `docs/local-credential-provider.md`, "Operation policy") classifies every
//! `sign_event` request. The tables below are the ones Buzz 1.0 enforces
//! (`block/buzz-app` `src-tauri/src/credential_service/policy.rs` at
//! `dfdb5582e`, the v1.0.1 release), which are also the contract's:
//!
//! - auth proofs and ordinary content are signed without a prompt;
//! - sensitive kinds MAY be approved by a person, or refused (`denied`);
//!   Buzz 1.0 always refuses them;
//! - every other kind is refused.
//!
//! bzz asks before a person commits to an action, instead of failing after.

/// NIP-42 AUTH, NIP-98 HTTP auth and Blossom auth, bound to the paired relay.
pub const AUTH_PROOFS: [u16; 3] = [22_242, 27_235, 24_242];

/// Everyday work, signed without a prompt.
pub const ORDINARY: &[u16] = &[
    7, 9, 1617, 1618, 1619, 1621, 1630, 1631, 1632, 1633, 9035, 9036, 10000, 10001, 10003, 10030,
    20001, 20002, 20003, 24200, 30000, 30003, 30023, 30030, 30078, 30174, 30315, 40002, 40003,
    40004, 40005, 40006, 40007, 40008, 40100, 41010, 41012, 43001, 43002, 43003, 43004, 43005,
    43006, 45001, 45002, 45003, 45010, 46020,
];

/// Changes to who the identity is, what it controls or what others see about
/// it. A provider asks a person or refuses.
pub const SENSITIVE: &[u16] = &[
    0, 3, 5, 9000, 9001, 9002, 9003, 9004, 9005, 9006, 9007, 9008, 9009, 9021, 9022, 9040, 9041,
    9042, 9043, 9044, 10002, 10100, 28936, 30175, 30176, 30177, 30179, 30617, 30618, 30620, 30621,
    41011, 45011, 46030, 46031,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Policy {
    /// Signed as built.
    Signs,
    /// Needs a person's approval in Buzz, which Buzz 1.0 never gives.
    MayAsk,
    /// Never signed for a local tool.
    Refuses,
}

pub fn classify(kind: u16) -> Policy {
    if AUTH_PROOFS.contains(&kind) || ORDINARY.contains(&kind) {
        Policy::Signs
    } else if SENSITIVE.contains(&kind) {
        Policy::MayAsk
    } else {
        Policy::Refuses
    }
}

/// What bzz tells a person when Buzz will not sign an action.
pub const REFUSED: &str = "Buzz does not let other apps do this; do it in Buzz";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_match_the_provider_contract() {
        // Kinds bzz signs as the human, with the verdict Buzz 1.0.1 gave for
        // each when probed (docs/validation-v0.14.0.md, M2).
        let observed = [
            (9, Policy::Signs),
            (40_003, Policy::Signs),
            (7, Policy::Signs),
            (5, Policy::MayAsk),
            (1984, Policy::Refuses),
            (0, Policy::MayAsk),
            (30_315, Policy::Signs),
            (20_001, Policy::Signs),
            (20_002, Policy::Signs),
            (30_078, Policy::Signs),
            (41_010, Policy::Signs),
            (41_012, Policy::Signs),
            (41_011, Policy::MayAsk),
            (9007, Policy::MayAsk),
            (9021, Policy::MayAsk),
            (9022, Policy::MayAsk),
            (9002, Policy::MayAsk),
            (48_100, Policy::Refuses),
            (48_103, Policy::Refuses),
            (24_810, Policy::Refuses),
            (45_010, Policy::Signs),
            (30_030, Policy::Signs),
            (30_180, Policy::Refuses),
            (22_242, Policy::Signs),
            (27_235, Policy::Signs),
            (24_242, Policy::Signs),
        ];
        for (kind, policy) in observed {
            assert_eq!(classify(kind), policy, "kind {kind}");
        }
    }

    #[test]
    fn the_tables_are_disjoint_and_sorted() {
        for table in [ORDINARY, SENSITIVE] {
            assert!(table.windows(2).all(|pair| pair[0] < pair[1]));
        }
        assert!(ORDINARY.iter().all(|kind| !SENSITIVE.contains(kind)));
        assert!(
            AUTH_PROOFS
                .iter()
                .all(|kind| !SENSITIVE.contains(kind) && !ORDINARY.contains(kind))
        );
    }
}
