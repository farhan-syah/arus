//! Forecast cycles, publication eligibility gates, and ordered publication manifests.
//!
//! Execution completion alone cannot authorise forecast publication. Publication requires
//! separate execution completion, numerical assessment pass, scientific approval,
//! monotonic cycle ordering, and a valid non-expired window.

use arus_domain::RunClassification;
use arus_model_api::AdvanceOutcome;
use serde::{Deserialize, Serialize};

/// Separate approval states required before a forecast cycle can publish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
}

/// Candidate forecast cycle submitted to the publication gate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublicationCandidate {
    pub cycle_id: String,
    /// Strictly increasing cycle sequence number for monotonic publication ordering.
    pub cycle_sequence: u64,
    pub run_id: String,
    pub classification: RunClassification,
    pub execution_outcome: AdvanceOutcome,
    pub numerical_assessment_passed: bool,
    pub scientific_approval: ApprovalStatus,
    pub owner_approval: ApprovalStatus,
    /// Valid-from timestamp in Unix seconds.
    pub valid_from_unix_s: i64,
    /// Expiry timestamp in Unix seconds.
    pub expires_at_unix_s: i64,
}

/// Published immutable forecast manifest pointer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicationManifest {
    pub cycle_id: String,
    pub cycle_sequence: u64,
    pub run_id: String,
    pub valid_from_unix_s: i64,
    pub expires_at_unix_s: i64,
    pub supersedes_cycle_id: Option<String>,
}

/// Decision emitted by the publication gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum PublicationDecision {
    Published(PublicationManifest),
    Refused { reasons: Vec<String> },
}

/// Evaluates whether `candidate` can atomically supersede `current` at `now_unix_s`.
pub fn evaluate_publication(
    candidate: &PublicationCandidate,
    current: Option<&PublicationManifest>,
    now_unix_s: i64,
) -> PublicationDecision {
    let mut reasons = Vec::new();

    if candidate.cycle_id.trim().is_empty() || candidate.run_id.trim().is_empty() {
        reasons.push("cycle_id and run_id must be non-empty".to_owned());
    }
    if candidate.classification != RunClassification::OperationalForecast {
        reasons.push(format!(
            "classification {} is ineligible for operational publication",
            candidate.classification.id()
        ));
    }
    if candidate.execution_outcome != AdvanceOutcome::ReachedTarget {
        reasons.push(format!(
            "execution outcome {:?} did not reach requested horizon",
            candidate.execution_outcome
        ));
    }
    if !candidate.numerical_assessment_passed {
        reasons.push("numerical conservation and budget assessment did not pass".to_owned());
    }
    if candidate.scientific_approval != ApprovalStatus::Approved {
        reasons.push(format!(
            "scientific approval is {:?}",
            candidate.scientific_approval
        ));
    }
    if candidate.owner_approval != ApprovalStatus::Approved {
        reasons.push(format!(
            "owner publication approval is {:?}",
            candidate.owner_approval
        ));
    }
    if candidate.expires_at_unix_s <= candidate.valid_from_unix_s {
        reasons.push("validity window is empty or inverted".to_owned());
    }
    if now_unix_s >= candidate.expires_at_unix_s {
        reasons.push("candidate forecast is already expired".to_owned());
    }
    if let Some(existing) = current
        && candidate.cycle_sequence <= existing.cycle_sequence
    {
        reasons.push(format!(
            "cycle_sequence {} does not advance beyond published sequence {}",
            candidate.cycle_sequence, existing.cycle_sequence
        ));
    }

    if !reasons.is_empty() {
        return PublicationDecision::Refused { reasons };
    }

    PublicationDecision::Published(PublicationManifest {
        cycle_id: candidate.cycle_id.clone(),
        cycle_sequence: candidate.cycle_sequence,
        run_id: candidate.run_id.clone(),
        valid_from_unix_s: candidate.valid_from_unix_s,
        expires_at_unix_s: candidate.expires_at_unix_s,
        supersedes_cycle_id: current.map(|c| c.cycle_id.clone()),
    })
}
