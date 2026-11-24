use arus_domain::RunClassification;
use arus_forecasting::{
    ApprovalStatus, PublicationCandidate, PublicationDecision, evaluate_publication,
};
use arus_model_api::AdvanceOutcome;

fn valid_candidate(cycle_sequence: u64) -> PublicationCandidate {
    PublicationCandidate {
        cycle_id: format!("cycle-{cycle_sequence}"),
        cycle_sequence,
        run_id: format!("run-{cycle_sequence}"),
        classification: RunClassification::OperationalForecast,
        execution_outcome: AdvanceOutcome::ReachedTarget,
        numerical_assessment_passed: true,
        scientific_approval: ApprovalStatus::Approved,
        owner_approval: ApprovalStatus::Approved,
        valid_from_unix_s: 1_000,
        expires_at_unix_s: 4_600,
    }
}

#[test]
fn execution_completion_alone_cannot_publish() {
    let mut candidate = valid_candidate(1);
    candidate.classification = RunClassification::NumericalDemonstration;
    candidate.scientific_approval = ApprovalStatus::Pending;
    candidate.owner_approval = ApprovalStatus::Pending;

    let decision = evaluate_publication(&candidate, None, 1_500);
    assert!(matches!(decision, PublicationDecision::Refused { reasons } if reasons.len() == 3));
}

#[test]
fn ordered_publication_supersedes_only_strictly_newer_valid_cycles() {
    let first = valid_candidate(10);
    let published = match evaluate_publication(&first, None, 1_500) {
        PublicationDecision::Published(manifest) => manifest,
        PublicationDecision::Refused { reasons } => {
            assert!(reasons.is_empty(), "unexpected refusal: {reasons:?}");
            return;
        }
    };
    assert_eq!(published.supersedes_cycle_id, None);

    // Older or equal sequence is refused (fencing out stale workers).
    let stale = valid_candidate(10);
    assert!(matches!(
        evaluate_publication(&stale, Some(&published), 1_600),
        PublicationDecision::Refused { .. }
    ));

    // Expired candidate is refused.
    let expired = valid_candidate(11);
    assert!(matches!(
        evaluate_publication(&expired, Some(&published), 5_000),
        PublicationDecision::Refused { .. }
    ));

    // Newer valid candidate supersedes cleanly.
    let next = valid_candidate(11);
    let decision = evaluate_publication(&next, Some(&published), 2_000);
    assert!(matches!(
        decision,
        PublicationDecision::Published(m)
            if m.cycle_sequence == 11 && m.supersedes_cycle_id.as_deref() == Some("cycle-10")
    ));
}
