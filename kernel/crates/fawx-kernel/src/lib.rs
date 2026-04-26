//! Hard security kernel contracts for Fawx OS.
//!
//! This crate owns the typed authority model, policy surface, and audit-facing
//! execution contracts. It should remain small, explicit, and difficult to
//! accidentally dilute with adapter concerns.

use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::time::{SystemTime, UNIX_EPOCH};

/// Describes which execution surface a capability belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilitySurface {
    Browser,
    Cloud,
    Device,
    Shell,
}

/// A typed capability granted to a runtime component.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CapabilityGrant {
    pub surface: CapabilitySurface,
    pub name: String,
}

/// The smallest useful kernel contract: explicit authority plus explicit user
/// intent. More detailed policy types will grow from this root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionContract {
    pub grants: Vec<CapabilityGrant>,
    pub user_intent: String,
}

/// High-level execution phases for a long-lived task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskPhase {
    Queued,
    Running,
    Waiting,
    Checkpointed,
    Completed,
    Failed,
}

impl TaskPhase {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

/// Whether a task can proceed without owning the visible foreground UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttentionRequirement {
    BackgroundAllowed,
    ForegroundPreferred,
    ForegroundRequired,
}

/// Why a task is currently unable to make forward progress.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskBlocker {
    WaitingForUserApproval { reason: String },
    WaitingForUserInput { reason: String },
    WaitingForForeground { reason: String },
    WaitingForExternalCondition { reason: String },
}

/// A coarse typed description of the agent's current activity. This is backend
/// state, not a UI string contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentActivityKind {
    Observing,
    Planning,
    Executing,
    Waiting,
    Verifying,
    Summarizing,
}

/// What the current activity is about, when the backend can express that
/// without guessing from prose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentActivityTarget {
    AndroidPackage { package_name: String },
    Url { url: String },
    File { path: String },
    Service { name: String },
    Contact { label: String },
    Network,
    RuntimeAction { name: String },
    Task,
    Unknown,
}

/// Where an activity description came from. This lets future renderers treat
/// model-declared intent differently from tool-derived or system-derived state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentActivitySource {
    ModelDeclared,
    ToolDerived,
    SystemDerived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentActivity {
    pub kind: AgentActivityKind,
    pub target: Option<AgentActivityTarget>,
    pub description: String,
    pub source: AgentActivitySource,
    pub started_at_ms: u128,
}

impl AgentActivity {
    pub fn new(
        kind: AgentActivityKind,
        target: Option<AgentActivityTarget>,
        description: impl Into<String>,
        source: AgentActivitySource,
    ) -> Result<Self, CheckpointClockError> {
        Ok(Self::at(kind, target, description, source, now_ms()?))
    }

    pub fn at(
        kind: AgentActivityKind,
        target: Option<AgentActivityTarget>,
        description: impl Into<String>,
        source: AgentActivitySource,
        started_at_ms: u128,
    ) -> Self {
        Self {
            kind,
            target,
            description: description.into(),
            source,
            started_at_ms,
        }
    }
}

/// Where an external action boundary sits relative to the outside world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionBoundaryState {
    Planned,
    Prepared,
    Committed,
    Verified,
    Aborted,
}

/// A durable side-effect boundary used to avoid duplicating external actions
/// after resume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionBoundary {
    pub id: String,
    pub state: ActionBoundaryState,
    pub description: String,
}

impl ActionBoundary {
    pub fn new(
        id: impl Into<String>,
        state: ActionBoundaryState,
        description: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            state,
            description: description.into(),
        }
    }
}

/// A persisted point from which a task can safely resume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCheckpoint {
    pub task_id: String,
    pub phase: TaskPhase,
    pub recorded_at_ms: u128,
    pub objective: String,
    pub action_boundary: ActionBoundary,
    pub blocker: Option<TaskBlocker>,
}

impl TaskCheckpoint {
    pub fn new(
        task_id: impl Into<String>,
        phase: TaskPhase,
        objective: impl Into<String>,
        action_boundary: ActionBoundary,
        blocker: Option<TaskBlocker>,
    ) -> Result<Self, CheckpointClockError> {
        Ok(Self::at(
            task_id,
            phase,
            now_ms()?,
            objective,
            action_boundary,
            blocker,
        ))
    }

    pub fn at(
        task_id: impl Into<String>,
        phase: TaskPhase,
        recorded_at_ms: u128,
        objective: impl Into<String>,
        action_boundary: ActionBoundary,
        blocker: Option<TaskBlocker>,
    ) -> Self {
        Self {
            task_id: task_id.into(),
            phase,
            recorded_at_ms,
            objective: objective.into(),
            action_boundary,
            blocker,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckpointClockError;

impl Display for CheckpointClockError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "system clock is before unix epoch")
    }
}

impl Error for CheckpointClockError {}

fn now_ms() -> Result<u128, CheckpointClockError> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CheckpointClockError)?
        .as_millis())
}
