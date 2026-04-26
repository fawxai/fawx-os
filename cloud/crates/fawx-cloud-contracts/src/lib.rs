//! Contracts for local-to-cloud escalation.
//!
//! Cloud execution is a subordinate capability. This crate should define only
//! the task envelope and result boundary, not a cloud-centric architecture.

/// A bounded task the local runtime may delegate remotely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudTaskEnvelope {
    pub task_id: String,
    pub objective: String,
}
