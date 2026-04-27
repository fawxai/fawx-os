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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SafetyCapability {
    AppControl,
    Calling,
    Messaging,
    FilesystemRead,
    FilesystemWrite,
    Network,
    NotificationsRead,
    NotificationsPost,
    RuntimeExecution,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SafetyScope {
    Any,
    AndroidPackage { package_name: String },
    Contact { label: String },
    File { path: String },
    Network,
    NotificationSurface,
    RuntimeAction { name: String },
    Service { name: String },
    Task,
    Url { url: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SafetyGrant {
    pub capability: SafetyCapability,
    pub scope: SafetyScope,
}

impl SafetyGrant {
    pub fn any(capability: SafetyCapability) -> Self {
        Self {
            capability,
            scope: SafetyScope::Any,
        }
    }

    pub fn scoped(capability: SafetyCapability, scope: SafetyScope) -> Self {
        Self { capability, scope }
    }

    pub fn allows(&self, requirement: &SafetyRequirement) -> bool {
        self.capability == requirement.capability
            && match (&self.scope, &requirement.scope) {
                (SafetyScope::Any, _) => true,
                (granted, required) => granted == required,
            }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SafetyRequirement {
    pub capability: SafetyCapability,
    pub scope: SafetyScope,
}

impl SafetyRequirement {
    pub fn new(capability: SafetyCapability, scope: SafetyScope) -> Self {
        Self { capability, scope }
    }
}

/// The smallest useful kernel contract: explicit authority plus explicit user
/// intent. More detailed policy types will grow from this root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionContract {
    pub grants: Vec<CapabilityGrant>,
    #[serde(default)]
    pub safety_grants: Vec<SafetyGrant>,
    pub user_intent: String,
}

impl ExecutionContract {
    pub fn allows(&self, requirement: &SafetyRequirement) -> bool {
        self.safety_grants
            .iter()
            .any(|grant| grant.allows(requirement))
    }
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
    NotificationSurface,
    RuntimeAction { name: String },
    Task,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HumanHandoffKind {
    Foreground,
    UserApproval,
    UserInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HumanHandoffResumeCondition {
    ForegroundPackage { package_name: String },
    ExplicitUserApproval,
    ExplicitUserInput,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanHandoffEvidence {
    pub handoff_id: String,
    pub condition: HumanHandoffResumeCondition,
    pub summary: String,
    pub observed_at_ms: u128,
}

impl HumanHandoffEvidence {
    pub fn new(
        handoff_id: impl Into<String>,
        condition: HumanHandoffResumeCondition,
        summary: impl Into<String>,
    ) -> Result<Self, CheckpointClockError> {
        Ok(Self {
            handoff_id: handoff_id.into(),
            condition,
            summary: summary.into(),
            observed_at_ms: now_ms()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanHandoffRequest {
    pub id: String,
    pub kind: HumanHandoffKind,
    pub reason: String,
    pub target: Option<AgentActivityTarget>,
    pub resume_condition: HumanHandoffResumeCondition,
    pub requested_at_ms: u128,
    pub last_evidence: Option<HumanHandoffEvidence>,
}

impl HumanHandoffRequest {
    pub fn new(
        id: impl Into<String>,
        kind: HumanHandoffKind,
        reason: impl Into<String>,
        target: Option<AgentActivityTarget>,
        resume_condition: HumanHandoffResumeCondition,
    ) -> Result<Self, CheckpointClockError> {
        Ok(Self {
            id: id.into(),
            kind,
            reason: reason.into(),
            target,
            resume_condition,
            requested_at_ms: now_ms()?,
            last_evidence: None,
        })
    }
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

/// A typed action the harness has accepted as the next concrete thing the
/// agent is trying to do. This is separate from activity narration: activity
/// explains current work, action intent defines the boundary to observe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentActionKind {
    Observe,
    Navigate,
    OpenApp,
    Interact,
    Read,
    Write,
    Communicate,
    Execute,
    Verify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentActionStatus {
    Accepted,
    Executing,
    Observed,
    Verified,
    Blocked,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentActionEvidence {
    ForegroundPackage {
        package_name: String,
        activity_name: Option<String>,
    },
    Notification {
        source: String,
        summary: String,
    },
    NetworkAvailable,
    RuntimeActionFailed {
        action: String,
        reason: String,
    },
    Manual {
        summary: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentActionObservation {
    pub action_id: String,
    pub evidence: AgentActionEvidence,
    pub observed_at_ms: u128,
}

impl AgentActionObservation {
    pub fn new(
        action_id: impl Into<String>,
        evidence: AgentActionEvidence,
    ) -> Result<Self, CheckpointClockError> {
        Ok(Self::at(action_id, evidence, now_ms()?))
    }

    pub fn at(
        action_id: impl Into<String>,
        evidence: AgentActionEvidence,
        observed_at_ms: u128,
    ) -> Self {
        Self {
            action_id: action_id.into(),
            evidence,
            observed_at_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentAction {
    pub kind: AgentActionKind,
    pub target: Option<AgentActivityTarget>,
    pub reason: String,
    pub expected_observation: Option<String>,
    pub status: AgentActionStatus,
    pub boundary: ActionBoundary,
    pub accepted_at_ms: u128,
    #[serde(default)]
    pub last_observation: Option<AgentActionObservation>,
}

impl AgentAction {
    pub fn new(
        kind: AgentActionKind,
        target: Option<AgentActivityTarget>,
        reason: impl Into<String>,
        expected_observation: Option<String>,
        boundary: ActionBoundary,
    ) -> Result<Self, CheckpointClockError> {
        Ok(Self {
            kind,
            target,
            reason: reason.into(),
            expected_observation,
            status: AgentActionStatus::Accepted,
            boundary,
            accepted_at_ms: now_ms()?,
            last_observation: None,
        })
    }

    pub fn at(
        kind: AgentActionKind,
        target: Option<AgentActivityTarget>,
        reason: impl Into<String>,
        expected_observation: Option<String>,
        status: AgentActionStatus,
        boundary: ActionBoundary,
        accepted_at_ms: u128,
    ) -> Self {
        Self {
            kind,
            target,
            reason: reason.into(),
            expected_observation,
            status,
            boundary,
            accepted_at_ms,
            last_observation: None,
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
