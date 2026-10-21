use darkbox_contracts::{FindingState, TestResultState};

/// A test result never becomes a confirmation on its own.
pub fn can_confirm(
    result_states: &[TestResultState],
    has_evidence: bool,
) -> Result<(), &'static str> {
    if !has_evidence {
        return Err("a confirmed finding requires accessible evidence");
    }
    if !result_states.is_empty()
        && result_states.iter().all(|state| {
            matches!(
                state,
                TestResultState::Inconclusive | TestResultState::Blocked
            )
        })
    {
        return Err("inconclusive or blocked results cannot sustain a confirmation");
    }
    Ok(())
}

pub fn review_transition(from: FindingState, to: FindingState) -> Result<(), &'static str> {
    use FindingState::*;
    let allowed = matches!(
        (from, to),
        (Hypothesis, NeedsReview)
            | (Hypothesis, Rejected)
            | (NeedsReview, Confirmed)
            | (NeedsReview, Rejected)
            | (Confirmed, Remediated)
            | (Confirmed, Reopened)
            | (Rejected, Reopened)
            | (Remediated, Reopened)
            | (Reopened, NeedsReview)
            | (Reopened, Confirmed)
            | (Reopened, Rejected)
    );
    if allowed {
        Ok(())
    } else {
        Err("illegal finding transition")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_evidence_no_confirmation() {
        assert!(can_confirm(&[], false).is_err());
        assert!(can_confirm(&[TestResultState::Passed], true).is_ok());
    }

    #[test]
    fn inconclusive_results_do_not_confirm() {
        assert!(can_confirm(&[TestResultState::Inconclusive], true).is_err());
        assert!(can_confirm(&[TestResultState::Blocked], true).is_err());
        assert!(can_confirm(&[TestResultState::Failed], true).is_ok());
    }

    #[test]
    fn findings_cannot_skip_review() {
        assert!(review_transition(FindingState::Hypothesis, FindingState::Confirmed).is_err());
        assert!(review_transition(FindingState::Hypothesis, FindingState::NeedsReview).is_ok());
        assert!(review_transition(FindingState::NeedsReview, FindingState::Confirmed).is_ok());
    }
}
