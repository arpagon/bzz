use super::{Eligibility, RespondTo};

/// Evaluate the active human identity against a verified owner's public policy.
///
/// Buzz hardens DMs to owner-only even when normal-channel policy is broader.
/// Unknown channel kinds are passed as `is_dm = true` by callers.
pub fn evaluate(
    respond_to: Option<RespondTo>,
    allowlist: &[String],
    owner_pubkey: &str,
    active_pubkey: &str,
    is_dm: bool,
) -> Eligibility {
    let Some(respond_to) = respond_to else {
        return Eligibility::PolicyUnknown;
    };
    if active_pubkey.eq_ignore_ascii_case(owner_pubkey) {
        return Eligibility::Eligible;
    }
    if is_dm {
        return Eligibility::Ineligible;
    }
    match respond_to {
        RespondTo::OwnerOnly => Eligibility::Ineligible,
        RespondTo::Allowlist => {
            if allowlist
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(active_pubkey))
            {
                Eligibility::Eligible
            } else {
                Eligibility::Ineligible
            }
        }
        RespondTo::Anyone => Eligibility::Eligible,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const VIEWER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn absent_policy_is_unknown() {
        assert_eq!(
            evaluate(None, &[], OWNER, VIEWER, false),
            Eligibility::PolicyUnknown
        );
    }

    #[test]
    fn owner_is_eligible_in_every_exposed_mode_and_dm() {
        for policy in [
            RespondTo::OwnerOnly,
            RespondTo::Allowlist,
            RespondTo::Anyone,
        ] {
            assert_eq!(
                evaluate(Some(policy), &[], OWNER, OWNER, true),
                Eligibility::Eligible
            );
        }
    }

    #[test]
    fn allowlist_is_exact_and_case_insensitive() {
        assert_eq!(
            evaluate(
                Some(RespondTo::Allowlist),
                &[VIEWER.to_ascii_uppercase()],
                OWNER,
                VIEWER,
                false,
            ),
            Eligibility::Eligible
        );
        assert_eq!(
            evaluate(Some(RespondTo::Allowlist), &[], OWNER, VIEWER, false),
            Eligibility::Ineligible
        );
    }

    #[test]
    fn dm_hardening_overrides_anyone_and_allowlist() {
        assert_eq!(
            evaluate(Some(RespondTo::Anyone), &[], OWNER, VIEWER, true),
            Eligibility::Ineligible
        );
        assert_eq!(
            evaluate(
                Some(RespondTo::Allowlist),
                &[VIEWER.into()],
                OWNER,
                VIEWER,
                true,
            ),
            Eligibility::Ineligible
        );
    }
}
