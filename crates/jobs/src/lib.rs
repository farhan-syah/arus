//! Job execution ownership, cancellation, resource budgets, and execution records.
//!
//! Execution completion records worker progress only. It does not establish scientific
//! qualification or authorise forecast publication.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use arus_domain::Provenance;
use arus_model_api::{AdvanceOutcome, PersistedCheckpoint, WaterBudget};
use serde::{Deserialize, Serialize};

/// Cooperative cancellation signal checked at safe accepted-step boundaries.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Creates an uncancelled token.
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation at the next accepted step boundary.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Clears a previous cancellation request before resuming.
    pub fn reset(&self) {
        self.cancelled.store(false, Ordering::SeqCst);
    }

    /// Returns `true` when cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// Step-count budget bounding one advancement request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepBudget {
    pub max_steps: usize,
}

impl StepBudget {
    /// Creates a step budget with `max_steps` accepted steps.
    pub const fn new(max_steps: usize) -> Self {
        Self { max_steps }
    }
}

/// Execution record emitted by a numerical worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub job_id: String,
    pub provenance: Provenance,
    pub outcome: AdvanceOutcome,
    pub accepted_time_s: f64,
    pub steps_taken: usize,
    pub budget: WaterBudget,
    pub checkpoint: Option<PersistedCheckpoint>,
}
