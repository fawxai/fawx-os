//! Thin harness contracts for the Fawx OS runtime loop.
//!
//! The harness should orchestrate model I/O, tool dispatch, and explicit
//! completion without smuggling policy or substrate assumptions into the loop.

pub use fawx_kernel::TaskPhase;
use fawx_kernel::{
    ActionBoundary, ActionBoundaryState, AgentAction, AgentActionKind, AgentActionStatus,
    AgentActivity, AgentActivityKind, AgentActivitySource, AgentActivityTarget,
    AttentionRequirement, ExecutionContract, TaskBlocker, TaskCheckpoint,
};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Whether the task is currently running in the foreground or can safely
/// continue without it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionMode {
    ForegroundAssisted,
    BackgroundCapable,
}

/// Device-agnostic provenance for runtime observations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeObservationSource {
    Android { substrate: String },
    Browser { surface: String },
    Cloud { provider: String },
    Shell { name: String },
}

/// Why a runtime observation could not produce the expected foreground state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForegroundUnavailableReason {
    CommandFailed,
    EmptyOutput,
    ParseFailed,
    Unsupported,
}

/// Device-agnostic observations flowing into the harness policy layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeEvent {
    ForegroundAppChanged {
        package_name: String,
        activity_name: Option<String>,
    },
    ForegroundUnavailable {
        target: String,
        reason: ForegroundUnavailableReason,
        raw_source: Option<String>,
    },
    NotificationReceived {
        source: String,
        summary: String,
    },
    NetworkAvailabilityChanged {
        available: bool,
    },
    DeviceLockStateChanged {
        locked: bool,
    },
    RuntimeActionFailed {
        action: String,
        reason: String,
    },
}

/// A typed runtime observation with explicit source provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeObservation {
    pub source: RuntimeObservationSource,
    pub event: RuntimeEvent,
}

/// The minimum typed task state the harness needs to expose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskState {
    pub task_id: String,
    pub phase: TaskPhase,
    pub mode: ExecutionMode,
    pub attention_requirement: AttentionRequirement,
    pub contract: ExecutionContract,
    pub checkpoint: Option<TaskCheckpoint>,
    pub blocker: Option<TaskBlocker>,
    #[serde(default)]
    pub current_activity: Option<AgentActivity>,
    #[serde(default)]
    pub current_action: Option<AgentAction>,
    #[serde(default)]
    pub action_sequence: u64,
    #[serde(default)]
    pub last_runtime_observation: Option<RuntimeObservation>,
}

impl TaskState {
    pub fn new_background_task(task_id: impl Into<String>, objective: impl Into<String>) -> Self {
        let task_id = task_id.into();
        let objective = objective.into();

        Self {
            task_id,
            phase: TaskPhase::Running,
            mode: ExecutionMode::BackgroundCapable,
            attention_requirement: AttentionRequirement::BackgroundAllowed,
            contract: ExecutionContract {
                grants: vec![],
                user_intent: objective,
            },
            checkpoint: None,
            blocker: None,
            current_activity: None,
            current_action: None,
            action_sequence: 0,
            last_runtime_observation: None,
        }
    }

    fn next_action_index(&mut self) -> u64 {
        self.action_sequence = self.action_sequence.saturating_add(1);
        self.action_sequence
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForegroundPolicyDecision {
    ContinueInBackground { observed_package: String },
    RequireForeground { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskTransitionError {
    TerminalTask {
        task_id: String,
        phase: TaskPhase,
    },
    BlockedTask {
        task_id: String,
        blocker: TaskBlocker,
    },
    CheckpointClock,
    InvalidActivity {
        task_id: String,
        reason: String,
    },
    InvalidAction {
        task_id: String,
        reason: String,
    },
}

impl Display for TaskTransitionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TerminalTask { task_id, phase } => {
                write!(f, "task {task_id} is terminal in phase {phase:?}")
            }
            Self::BlockedTask { task_id, blocker } => {
                write!(f, "task {task_id} is blocked by {blocker:?}")
            }
            Self::CheckpointClock => write!(f, "could not timestamp checkpoint"),
            Self::InvalidActivity { task_id, reason } => {
                write!(f, "task {task_id} rejected activity: {reason}")
            }
            Self::InvalidAction { task_id, reason } => {
                write!(f, "task {task_id} rejected action: {reason}")
            }
        }
    }
}

impl Error for TaskTransitionError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelActivityProposal {
    pub kind: ModelActivityKind,
    pub target: Option<AgentActivityTarget>,
    pub description: String,
}

/// Activity kinds the model is allowed to declare directly.
///
/// Waiting is intentionally absent: waiting is derived from typed blockers so
/// the control plane can keep ownership of paused/blocked state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelActivityKind {
    Observing,
    Planning,
    Executing,
    Verifying,
    Summarizing,
}

impl From<ModelActivityKind> for AgentActivityKind {
    fn from(kind: ModelActivityKind) -> Self {
        match kind {
            ModelActivityKind::Observing => Self::Observing,
            ModelActivityKind::Planning => Self::Planning,
            ModelActivityKind::Executing => Self::Executing,
            ModelActivityKind::Verifying => Self::Verifying,
            ModelActivityKind::Summarizing => Self::Summarizing,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelActionProposal {
    pub kind: ModelActionKind,
    pub target: Option<AgentActivityTarget>,
    pub reason: String,
    #[serde(default)]
    pub expected_observation: Option<String>,
    #[serde(default)]
    pub proposal_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelActionKind {
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

impl From<ModelActionKind> for AgentActionKind {
    fn from(kind: ModelActionKind) -> Self {
        match kind {
            ModelActionKind::Observe => Self::Observe,
            ModelActionKind::Navigate => Self::Navigate,
            ModelActionKind::OpenApp => Self::OpenApp,
            ModelActionKind::Interact => Self::Interact,
            ModelActionKind::Read => Self::Read,
            ModelActionKind::Write => Self::Write,
            ModelActionKind::Communicate => Self::Communicate,
            ModelActionKind::Execute => Self::Execute,
            ModelActionKind::Verify => Self::Verify,
        }
    }
}

pub fn record_action_checkpoint(
    state: TaskState,
    action_boundary: ActionBoundary,
) -> Result<TaskState, TaskTransitionError> {
    ensure_not_terminal(&state)?;
    if let Some(blocker) = state.blocker.clone() {
        return Err(TaskTransitionError::BlockedTask {
            task_id: state.task_id,
            blocker,
        });
    }

    let mut state = state;
    state.phase = TaskPhase::Checkpointed;
    state.mode = ExecutionMode::BackgroundCapable;
    state.attention_requirement = AttentionRequirement::BackgroundAllowed;
    state.checkpoint = Some(new_checkpoint(
        &state,
        TaskPhase::Checkpointed,
        action_boundary,
        None,
    )?);
    Ok(state)
}

pub fn require_foreground_attention(
    state: TaskState,
    reason: impl Into<String>,
) -> Result<TaskState, TaskTransitionError> {
    ensure_not_terminal(&state)?;
    foreground_required(state, &reason.into(), None, None)
}

pub fn require_foreground_attention_for_package(
    state: TaskState,
    expected_package: impl Into<String>,
    reason: impl Into<String>,
) -> Result<TaskState, TaskTransitionError> {
    ensure_not_terminal(&state)?;
    let expected_package = expected_package.into();
    foreground_required(
        state,
        &reason.into(),
        None,
        Some(AgentActivityTarget::AndroidPackage {
            package_name: expected_package,
        }),
    )
}

pub fn require_external_condition(
    state: TaskState,
    reason: impl Into<String>,
    observation: Option<RuntimeObservation>,
) -> Result<TaskState, TaskTransitionError> {
    ensure_not_terminal(&state)?;
    external_condition_required(state, &reason.into(), observation)
}

pub fn satisfy_external_condition(
    state: TaskState,
    observation: RuntimeObservation,
) -> Result<TaskState, TaskTransitionError> {
    ensure_not_terminal(&state)?;
    external_condition_satisfied(state, observation)
}

pub fn record_planning_activity(
    state: TaskState,
    description: impl Into<String>,
) -> Result<TaskState, TaskTransitionError> {
    ensure_not_terminal(&state)?;
    if let Some(blocker) = state.blocker.clone() {
        return Err(TaskTransitionError::BlockedTask {
            task_id: state.task_id,
            blocker,
        });
    }
    let mut state = state;
    state.current_activity = Some(system_activity(
        AgentActivityKind::Planning,
        Some(AgentActivityTarget::Task),
        description,
    )?);
    Ok(state)
}

pub fn record_model_declared_activity(
    state: TaskState,
    proposal: ModelActivityProposal,
) -> Result<TaskState, TaskTransitionError> {
    ensure_not_terminal(&state)?;
    if let Some(blocker) = state.blocker.clone() {
        return Err(TaskTransitionError::BlockedTask {
            task_id: state.task_id,
            blocker,
        });
    }
    let description = proposal.description.trim();
    if description.is_empty() {
        return Err(TaskTransitionError::InvalidActivity {
            task_id: state.task_id,
            reason: "activity description must not be empty".to_string(),
        });
    }

    let mut state = state;
    state.current_activity = Some(
        AgentActivity::new(
            proposal.kind.into(),
            proposal.target,
            description,
            AgentActivitySource::ModelDeclared,
        )
        .map_err(|_| TaskTransitionError::CheckpointClock)?,
    );
    Ok(state)
}

pub fn accept_model_action_proposal(
    state: TaskState,
    proposal: ModelActionProposal,
) -> Result<TaskState, TaskTransitionError> {
    ensure_not_terminal(&state)?;
    if let Some(blocker) = state.blocker.clone() {
        return Err(TaskTransitionError::BlockedTask {
            task_id: state.task_id,
            blocker,
        });
    }
    let reason = proposal.reason.trim();
    if reason.is_empty() {
        return Err(TaskTransitionError::InvalidAction {
            task_id: state.task_id,
            reason: "action reason must not be empty".to_string(),
        });
    }
    validate_action_target(&proposal.kind, &proposal.target).map_err(|reason| {
        TaskTransitionError::InvalidAction {
            task_id: state.task_id.clone(),
            reason,
        }
    })?;
    let expected_observation = proposal
        .expected_observation
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let mut state = state;
    let boundary_id = proposal
        .proposal_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(|proposal_id| {
            if proposal_id.starts_with(&format!("model-action:{}:", state.task_id)) {
                Err(TaskTransitionError::InvalidAction {
                    task_id: state.task_id.clone(),
                    reason: "proposal_id must not use the kernel-generated action namespace"
                        .to_string(),
                })
            } else {
                Ok(proposal_id)
            }
        })
        .transpose()?
        .unwrap_or_else(|| {
            let task_id = state.task_id.clone();
            let action_index = state.next_action_index();
            format!("model-action:{}:{}", task_id, action_index)
        });

    state.current_action = Some(
        AgentAction::new(
            proposal.kind.into(),
            proposal.target,
            reason,
            expected_observation,
            ActionBoundary::new(boundary_id, ActionBoundaryState::Planned, reason),
        )
        .map_err(|_| TaskTransitionError::CheckpointClock)?,
    );
    Ok(state)
}

fn validate_action_target(
    kind: &ModelActionKind,
    target: &Option<AgentActivityTarget>,
) -> Result<(), String> {
    let valid = match kind {
        ModelActionKind::Observe | ModelActionKind::Verify => true,
        ModelActionKind::OpenApp => {
            matches!(target, Some(AgentActivityTarget::AndroidPackage { .. }))
        }
        ModelActionKind::Navigate => matches!(target, Some(AgentActivityTarget::Url { .. })),
        ModelActionKind::Read | ModelActionKind::Write => matches!(
            target,
            Some(AgentActivityTarget::File { .. })
                | Some(AgentActivityTarget::Url { .. })
                | Some(AgentActivityTarget::Service { .. })
        ),
        ModelActionKind::Interact => matches!(
            target,
            Some(AgentActivityTarget::AndroidPackage { .. })
                | Some(AgentActivityTarget::Url { .. })
                | Some(AgentActivityTarget::File { .. })
                | Some(AgentActivityTarget::Service { .. })
                | Some(AgentActivityTarget::Contact { .. })
                | Some(AgentActivityTarget::Network)
                | Some(AgentActivityTarget::RuntimeAction { .. })
                | Some(AgentActivityTarget::Task)
        ),
        ModelActionKind::Communicate => matches!(
            target,
            Some(AgentActivityTarget::Contact { .. }) | Some(AgentActivityTarget::Service { .. })
        ),
        ModelActionKind::Execute => matches!(
            target,
            Some(AgentActivityTarget::RuntimeAction { .. })
                | Some(AgentActivityTarget::Service { .. })
        ),
    };

    valid
        .then_some(())
        .ok_or_else(|| format!("action kind {kind:?} requires a compatible typed target"))
}

pub fn record_current_blocker_activity(state: TaskState) -> Result<TaskState, TaskTransitionError> {
    ensure_not_terminal(&state)?;
    let mut state = state;
    let existing_activity_matches_blocker = matches!(
        (&state.current_activity, blocker_reason(&state.blocker)),
        (Some(activity), Some(reason))
            if activity.kind == AgentActivityKind::Waiting && activity.description == reason
    );
    if !existing_activity_matches_blocker {
        state.current_activity = blocker_activity(&state.blocker)?;
    }
    mark_current_action(state, AgentActionStatus::Blocked)
}

fn mark_current_action(
    mut state: TaskState,
    status: AgentActionStatus,
) -> Result<TaskState, TaskTransitionError> {
    if let Some(action) = state.current_action.as_mut() {
        action.status = status;
        action.boundary.state = match status {
            AgentActionStatus::Accepted | AgentActionStatus::Executing => action.boundary.state,
            AgentActionStatus::Observed => ActionBoundaryState::Committed,
            AgentActionStatus::Verified => ActionBoundaryState::Verified,
            AgentActionStatus::Blocked | AgentActionStatus::Failed => ActionBoundaryState::Aborted,
        };
    }
    Ok(state)
}

pub fn clear_agent_activity(mut state: TaskState) -> TaskState {
    let status = if state.phase == TaskPhase::Completed {
        Some(AgentActionStatus::Verified)
    } else if state.phase == TaskPhase::Failed {
        Some(AgentActionStatus::Failed)
    } else {
        None
    };
    if let Some(status) = status
        && let Some(action) = state.current_action.as_mut()
    {
        action.status = status;
        action.boundary.state = match status {
            AgentActionStatus::Verified => ActionBoundaryState::Verified,
            AgentActionStatus::Failed => ActionBoundaryState::Aborted,
            _ => action.boundary.state,
        };
    }
    state.current_activity = None;
    state
}

pub fn apply_foreground_policy(
    state: TaskState,
    observation: &RuntimeObservation,
    expected_package: &str,
) -> Result<(TaskState, ForegroundPolicyDecision), TaskTransitionError> {
    ensure_not_terminal(&state)?;

    let decision = match &observation.event {
        RuntimeEvent::ForegroundAppChanged { package_name, .. }
            if package_name == expected_package =>
        {
            ForegroundPolicyDecision::ContinueInBackground {
                observed_package: package_name.clone(),
            }
        }
        RuntimeEvent::ForegroundAppChanged { package_name, .. } => {
            ForegroundPolicyDecision::RequireForeground {
                reason: format!(
                    "expected foreground package {expected_package}, saw {package_name}"
                ),
            }
        }
        RuntimeEvent::ForegroundUnavailable { target, reason, .. } => {
            ForegroundPolicyDecision::RequireForeground {
                reason: format!(
                    "expected foreground package {expected_package}, but {target} was unavailable: {reason:?}"
                ),
            }
        }
        _ => ForegroundPolicyDecision::RequireForeground {
            reason: format!(
                "expected foreground package {expected_package}, but observation was not foreground state"
            ),
        },
    };

    let state = match &decision {
        ForegroundPolicyDecision::ContinueInBackground { .. } => {
            foreground_available(state, observation.clone())?
        }
        ForegroundPolicyDecision::RequireForeground { reason } => foreground_required(
            state,
            reason,
            Some(observation.clone()),
            Some(AgentActivityTarget::AndroidPackage {
                package_name: expected_package.to_string(),
            }),
        )?,
    };

    Ok((state, decision))
}

fn foreground_available(
    mut state: TaskState,
    observation: RuntimeObservation,
) -> Result<TaskState, TaskTransitionError> {
    let was_waiting_for_foreground = matches!(
        state.blocker,
        Some(TaskBlocker::WaitingForForeground { .. })
    );
    let blocker = match state.blocker.take() {
        Some(TaskBlocker::WaitingForForeground { .. }) | None => None,
        Some(blocker) => Some(blocker),
    };
    state.blocker = blocker.clone();
    state.last_runtime_observation = Some(observation);

    if blocker.is_none() {
        if was_waiting_for_foreground {
            state.phase = TaskPhase::Running;
        }
        state.mode = ExecutionMode::BackgroundCapable;
        state.attention_requirement = AttentionRequirement::BackgroundAllowed;
        state.current_activity = Some(system_activity(
            AgentActivityKind::Observing,
            Some(AgentActivityTarget::AndroidPackage {
                package_name: observed_package_from_observation(&state.last_runtime_observation)
                    .unwrap_or_else(|| "unknown".to_string()),
            }),
            "foreground observation accepted",
        )?);
        state = mark_current_action(state, AgentActionStatus::Observed)?;
    } else {
        state.current_activity = blocker_activity(&blocker)?;
        state = mark_current_action(state, AgentActionStatus::Blocked)?;
    }

    Ok(state)
}

fn foreground_required(
    mut state: TaskState,
    reason: &str,
    observation: Option<RuntimeObservation>,
    target: Option<AgentActivityTarget>,
) -> Result<TaskState, TaskTransitionError> {
    let blocker = match state.blocker.take() {
        Some(TaskBlocker::WaitingForForeground { .. }) | None => {
            Some(TaskBlocker::WaitingForForeground {
                reason: reason.to_string(),
            })
        }
        Some(blocker) => Some(blocker),
    };

    if matches!(blocker, Some(TaskBlocker::WaitingForForeground { .. })) {
        state.phase = TaskPhase::Waiting;
        state.mode = ExecutionMode::ForegroundAssisted;
        state.attention_requirement = AttentionRequirement::ForegroundRequired;
    }

    state.blocker = blocker.clone();
    state.last_runtime_observation = observation;
    state.current_activity = blocker_activity_with_target(&blocker, target)?;
    mark_current_action(state, AgentActionStatus::Blocked)
}

fn external_condition_satisfied(
    mut state: TaskState,
    observation: RuntimeObservation,
) -> Result<TaskState, TaskTransitionError> {
    let was_waiting_for_external_condition = matches!(
        state.blocker,
        Some(TaskBlocker::WaitingForExternalCondition { .. })
    );
    let blocker = match state.blocker.take() {
        Some(TaskBlocker::WaitingForExternalCondition { .. }) | None => None,
        Some(blocker) => Some(blocker),
    };
    state.blocker = blocker.clone();
    state.last_runtime_observation = Some(observation);

    if blocker.is_none() && was_waiting_for_external_condition {
        state.phase = if state.checkpoint.is_some() {
            TaskPhase::Checkpointed
        } else {
            TaskPhase::Running
        };
        state.mode = ExecutionMode::BackgroundCapable;
        state.attention_requirement = AttentionRequirement::BackgroundAllowed;
        state.current_activity = Some(system_activity(
            AgentActivityKind::Observing,
            Some(AgentActivityTarget::Network),
            "external condition satisfied",
        )?);
        state = mark_current_action(state, AgentActionStatus::Observed)?;
    } else {
        state.current_activity = blocker_activity(&blocker)?;
        state = mark_current_action(state, AgentActionStatus::Blocked)?;
    }

    Ok(state)
}

fn external_condition_required(
    mut state: TaskState,
    reason: &str,
    observation: Option<RuntimeObservation>,
) -> Result<TaskState, TaskTransitionError> {
    let blocker = match state.blocker.take() {
        Some(TaskBlocker::WaitingForExternalCondition { .. }) | None => {
            Some(TaskBlocker::WaitingForExternalCondition {
                reason: reason.to_string(),
            })
        }
        Some(blocker) => Some(blocker),
    };

    if matches!(
        blocker,
        Some(TaskBlocker::WaitingForExternalCondition { .. })
    ) {
        state.phase = TaskPhase::Waiting;
    }

    state.blocker = blocker.clone();
    state.last_runtime_observation = observation;
    state.current_activity = blocker_activity(&blocker)?;
    mark_current_action(state, AgentActionStatus::Blocked)
}

fn ensure_not_terminal(state: &TaskState) -> Result<(), TaskTransitionError> {
    if state.phase.is_terminal() {
        Err(TaskTransitionError::TerminalTask {
            task_id: state.task_id.clone(),
            phase: state.phase,
        })
    } else {
        Ok(())
    }
}

fn new_checkpoint(
    state: &TaskState,
    phase: TaskPhase,
    action_boundary: ActionBoundary,
    blocker: Option<TaskBlocker>,
) -> Result<TaskCheckpoint, TaskTransitionError> {
    TaskCheckpoint::new(
        state.task_id.clone(),
        phase,
        state.contract.user_intent.clone(),
        action_boundary,
        blocker,
    )
    .map_err(|_| TaskTransitionError::CheckpointClock)
}

fn system_activity(
    kind: AgentActivityKind,
    target: Option<AgentActivityTarget>,
    description: impl Into<String>,
) -> Result<AgentActivity, TaskTransitionError> {
    AgentActivity::new(
        kind,
        target,
        description,
        AgentActivitySource::SystemDerived,
    )
    .map_err(|_| TaskTransitionError::CheckpointClock)
}

fn observed_package_from_observation(observation: &Option<RuntimeObservation>) -> Option<String> {
    match observation.as_ref().map(|observation| &observation.event) {
        Some(RuntimeEvent::ForegroundAppChanged { package_name, .. }) => Some(package_name.clone()),
        _ => None,
    }
}

fn blocker_activity(
    blocker: &Option<TaskBlocker>,
) -> Result<Option<AgentActivity>, TaskTransitionError> {
    blocker_activity_with_target(blocker, None)
}

fn blocker_activity_with_target(
    blocker: &Option<TaskBlocker>,
    target_override: Option<AgentActivityTarget>,
) -> Result<Option<AgentActivity>, TaskTransitionError> {
    match blocker {
        Some(TaskBlocker::WaitingForForeground { reason }) => Ok(Some(system_activity(
            AgentActivityKind::Waiting,
            target_override,
            reason.clone(),
        )?)),
        Some(TaskBlocker::WaitingForExternalCondition { reason }) => Ok(Some(system_activity(
            AgentActivityKind::Waiting,
            Some(AgentActivityTarget::Network),
            reason.clone(),
        )?)),
        Some(TaskBlocker::WaitingForUserApproval { reason })
        | Some(TaskBlocker::WaitingForUserInput { reason }) => Ok(Some(system_activity(
            AgentActivityKind::Waiting,
            Some(AgentActivityTarget::Task),
            reason.clone(),
        )?)),
        None => Ok(None),
    }
}

fn blocker_reason(blocker: &Option<TaskBlocker>) -> Option<&str> {
    match blocker {
        Some(TaskBlocker::WaitingForForeground { reason })
        | Some(TaskBlocker::WaitingForExternalCondition { reason })
        | Some(TaskBlocker::WaitingForUserApproval { reason })
        | Some(TaskBlocker::WaitingForUserInput { reason }) => Some(reason.as_str()),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fawx_kernel::{ActionBoundaryState, CapabilityGrant, CapabilitySurface};

    fn foreground_observation(package_name: &str) -> RuntimeObservation {
        RuntimeObservation {
            source: RuntimeObservationSource::Android {
                substrate: "ReconRootedStock".to_string(),
            },
            event: RuntimeEvent::ForegroundAppChanged {
                package_name: package_name.to_string(),
                activity_name: None,
            },
        }
    }

    #[test]
    fn background_capable_task_can_be_checkpointed_without_shell_state() {
        let state = TaskState {
            task_id: "task-1".to_string(),
            phase: TaskPhase::Checkpointed,
            mode: ExecutionMode::BackgroundCapable,
            attention_requirement: AttentionRequirement::BackgroundAllowed,
            contract: ExecutionContract {
                grants: vec![CapabilityGrant {
                    surface: CapabilitySurface::Browser,
                    name: "navigate".to_string(),
                }],
                user_intent: "cancel that subscription".to_string(),
            },
            checkpoint: Some(TaskCheckpoint::at(
                "task-1",
                TaskPhase::Waiting,
                1,
                "cancel that subscription",
                ActionBoundary::new(
                    "support-form-submit",
                    ActionBoundaryState::Committed,
                    "support form submitted",
                ),
                Some(TaskBlocker::WaitingForExternalCondition {
                    reason: "waiting for provider confirmation".to_string(),
                }),
            )),
            blocker: Some(TaskBlocker::WaitingForExternalCondition {
                reason: "waiting for provider confirmation".to_string(),
            }),
            current_activity: None,
            current_action: None,
            action_sequence: 0,
            last_runtime_observation: None,
        };

        assert_eq!(state.mode, ExecutionMode::BackgroundCapable);
        assert_eq!(state.phase, TaskPhase::Checkpointed);
        assert!(state.checkpoint.is_some());
    }

    #[test]
    fn foreground_required_task_exposes_explicit_blocker() {
        let state = TaskState {
            task_id: "task-2".to_string(),
            phase: TaskPhase::Waiting,
            mode: ExecutionMode::ForegroundAssisted,
            attention_requirement: AttentionRequirement::ForegroundRequired,
            contract: ExecutionContract {
                grants: vec![],
                user_intent: "finish checkout".to_string(),
            },
            checkpoint: None,
            blocker: Some(TaskBlocker::WaitingForForeground {
                reason: "checkout flow needs active app focus".to_string(),
            }),
            current_activity: None,
            current_action: None,
            action_sequence: 0,
            last_runtime_observation: None,
        };

        assert_eq!(
            state.attention_requirement,
            AttentionRequirement::ForegroundRequired
        );
        assert!(matches!(
            state.blocker,
            Some(TaskBlocker::WaitingForForeground { .. })
        ));
    }

    #[test]
    fn old_task_json_without_current_activity_deserializes() {
        let payload = r#"{
          "task_id": "task-1",
          "phase": "Running",
          "mode": "BackgroundCapable",
          "attention_requirement": "BackgroundAllowed",
          "contract": {
            "grants": [],
            "user_intent": "legacy task"
          },
          "checkpoint": null,
          "blocker": null,
          "last_runtime_observation": null
        }"#;

        let state: TaskState = serde_json::from_str(payload).expect("deserialize legacy task");

        assert_eq!(state.task_id, "task-1");
        assert!(state.current_activity.is_none());
        assert!(state.current_action.is_none());
        assert_eq!(state.action_sequence, 0);
    }

    #[test]
    fn model_declared_activity_records_source_and_target_when_unblocked() {
        let state = TaskState::new_background_task("task-1", "inspect settings");

        let state = record_model_declared_activity(
            state,
            ModelActivityProposal {
                kind: ModelActivityKind::Observing,
                target: Some(AgentActivityTarget::AndroidPackage {
                    package_name: "com.android.settings".to_string(),
                }),
                description: "checking settings state".to_string(),
            },
        )
        .expect("record model activity");

        let activity = state.current_activity.expect("activity");
        assert_eq!(activity.kind, AgentActivityKind::Observing);
        assert_eq!(activity.source, AgentActivitySource::ModelDeclared);
        assert!(matches!(
            activity.target,
            Some(AgentActivityTarget::AndroidPackage { package_name })
                if package_name == "com.android.settings"
        ));
        assert_eq!(activity.description, "checking settings state");
    }

    #[test]
    fn model_declared_activity_rejects_blocked_tasks() {
        let mut state = TaskState::new_background_task("task-1", "approve checkout");
        state.blocker = Some(TaskBlocker::WaitingForUserApproval {
            reason: "approve checkout".to_string(),
        });

        let error = record_model_declared_activity(
            state,
            ModelActivityProposal {
                kind: ModelActivityKind::Planning,
                target: Some(AgentActivityTarget::Task),
                description: "planning checkout".to_string(),
            },
        )
        .expect_err("blocked model activity should reject");

        assert!(matches!(error, TaskTransitionError::BlockedTask { .. }));
    }

    #[test]
    fn model_activity_proposal_schema_rejects_waiting_kind() {
        let payload = r#"{
          "kind": "Waiting",
          "target": "Task",
          "description": "waiting on something"
        }"#;

        let error = serde_json::from_str::<ModelActivityProposal>(payload)
            .expect_err("waiting must not be in the model-declared schema");

        assert!(error.to_string().contains("unknown variant"));
    }

    #[test]
    fn model_action_proposal_records_accepted_action_boundary() {
        let state = TaskState::new_background_task("task-1", "open settings");

        let state = accept_model_action_proposal(
            state,
            ModelActionProposal {
                kind: ModelActionKind::OpenApp,
                target: Some(AgentActivityTarget::AndroidPackage {
                    package_name: "com.android.settings".to_string(),
                }),
                reason: "open settings to inspect permissions".to_string(),
                expected_observation: Some("settings is foreground".to_string()),
                proposal_id: None,
            },
        )
        .expect("accept action");

        let action = state.current_action.expect("current action");
        assert_eq!(action.kind, AgentActionKind::OpenApp);
        assert_eq!(action.status, fawx_kernel::AgentActionStatus::Accepted);
        assert_eq!(action.reason, "open settings to inspect permissions");
        assert_eq!(
            action.expected_observation.as_deref(),
            Some("settings is foreground")
        );
        assert_eq!(action.boundary.id, "model-action:task-1:1");
        assert_eq!(action.boundary.state, ActionBoundaryState::Planned);
        assert!(matches!(
            action.target,
            Some(AgentActivityTarget::AndroidPackage { package_name })
                if package_name == "com.android.settings"
        ));
    }

    #[test]
    fn model_action_proposal_rejects_blocked_tasks() {
        let mut state = TaskState::new_background_task("task-1", "approve checkout");
        state.blocker = Some(TaskBlocker::WaitingForUserApproval {
            reason: "approve checkout".to_string(),
        });

        let error = accept_model_action_proposal(
            state,
            ModelActionProposal {
                kind: ModelActionKind::Interact,
                target: Some(AgentActivityTarget::Task),
                reason: "click approve anyway".to_string(),
                expected_observation: None,
                proposal_id: None,
            },
        )
        .expect_err("blocked action should reject");

        assert!(matches!(error, TaskTransitionError::BlockedTask { .. }));
    }

    #[test]
    fn model_action_proposal_rejects_empty_reason() {
        let state = TaskState::new_background_task("task-1", "open settings");

        let error = accept_model_action_proposal(
            state,
            ModelActionProposal {
                kind: ModelActionKind::OpenApp,
                target: Some(AgentActivityTarget::Task),
                reason: "   ".to_string(),
                expected_observation: None,
                proposal_id: None,
            },
        )
        .expect_err("empty reason should reject");

        assert!(matches!(error, TaskTransitionError::InvalidAction { .. }));
    }

    #[test]
    fn model_action_proposal_rejects_concrete_action_without_compatible_target() {
        let state = TaskState::new_background_task("task-1", "open settings");

        let error = accept_model_action_proposal(
            state,
            ModelActionProposal {
                kind: ModelActionKind::OpenApp,
                target: None,
                reason: "open settings".to_string(),
                expected_observation: None,
                proposal_id: None,
            },
        )
        .expect_err("open app action without android package should reject");

        assert!(matches!(error, TaskTransitionError::InvalidAction { .. }));
    }

    #[test]
    fn model_action_proposal_generates_distinct_boundary_ids() {
        let state = TaskState::new_background_task("task-1", "open settings");
        let state = accept_model_action_proposal(
            state,
            ModelActionProposal {
                kind: ModelActionKind::Observe,
                target: None,
                reason: "observe first".to_string(),
                expected_observation: None,
                proposal_id: None,
            },
        )
        .expect("accept first action");
        assert_eq!(
            state
                .current_action
                .as_ref()
                .map(|action| action.boundary.id.as_str()),
            Some("model-action:task-1:1")
        );

        let state = accept_model_action_proposal(
            state,
            ModelActionProposal {
                kind: ModelActionKind::Observe,
                target: None,
                reason: "observe second".to_string(),
                expected_observation: None,
                proposal_id: None,
            },
        )
        .expect("accept second action");

        assert_eq!(
            state
                .current_action
                .as_ref()
                .map(|action| action.boundary.id.as_str()),
            Some("model-action:task-1:2")
        );
        assert_eq!(state.action_sequence, 2);
    }

    #[test]
    fn model_action_proposal_rejects_generated_namespace_proposal_id() {
        let state = TaskState::new_background_task("task-1", "open settings");

        let error = accept_model_action_proposal(
            state,
            ModelActionProposal {
                kind: ModelActionKind::Observe,
                target: None,
                reason: "observe".to_string(),
                expected_observation: None,
                proposal_id: Some("model-action:task-1:1".to_string()),
            },
        )
        .expect_err("caller must not use generated namespace");

        assert!(matches!(error, TaskTransitionError::InvalidAction { .. }));
    }

    #[test]
    fn model_action_proposal_rejects_unknown_for_concrete_interaction() {
        let state = TaskState::new_background_task("task-1", "interact");

        let error = accept_model_action_proposal(
            state,
            ModelActionProposal {
                kind: ModelActionKind::Interact,
                target: Some(AgentActivityTarget::Unknown),
                reason: "interact somehow".to_string(),
                expected_observation: None,
                proposal_id: None,
            },
        )
        .expect_err("unknown target should reject for concrete action");

        assert!(matches!(error, TaskTransitionError::InvalidAction { .. }));
    }

    #[test]
    fn foreground_policy_continues_when_expected_package_matches() {
        let state = TaskState::new_background_task("task-1", "watch settings");
        let observation = foreground_observation("com.android.settings");

        let (state, decision) =
            apply_foreground_policy(state, &observation, "com.android.settings")
                .expect("foreground transition");

        assert!(matches!(
            decision,
            ForegroundPolicyDecision::ContinueInBackground { .. }
        ));
        assert_eq!(
            state.attention_requirement,
            AttentionRequirement::BackgroundAllowed
        );
        assert!(state.blocker.is_none());
        assert!(state.checkpoint.is_none());
        assert_eq!(state.last_runtime_observation, Some(observation));
    }

    #[test]
    fn foreground_policy_blocks_when_expected_package_differs() {
        let state = TaskState::new_background_task("task-1", "watch settings");
        let observation = foreground_observation("com.google.android.apps.nexuslauncher");

        let (state, decision) =
            apply_foreground_policy(state, &observation, "com.android.settings")
                .expect("foreground transition");

        assert!(matches!(
            decision,
            ForegroundPolicyDecision::RequireForeground { .. }
        ));
        assert_eq!(
            state.attention_requirement,
            AttentionRequirement::ForegroundRequired
        );
        assert!(matches!(
            state.blocker,
            Some(TaskBlocker::WaitingForForeground { .. })
        ));
        assert_eq!(state.checkpoint, None);
        assert_eq!(state.last_runtime_observation, Some(observation));
        assert!(matches!(
            state.current_activity.as_ref().and_then(|activity| activity.target.as_ref()),
            Some(AgentActivityTarget::AndroidPackage { package_name })
                if package_name == "com.android.settings"
        ));
    }

    #[test]
    fn foreground_policy_only_clears_waiting_for_foreground_blockers() {
        let mut state = TaskState::new_background_task("task-1", "watch settings");
        state.phase = TaskPhase::Waiting;
        state.blocker = Some(TaskBlocker::WaitingForExternalCondition {
            reason: "waiting for receipt".to_string(),
        });
        let observation = foreground_observation("com.android.settings");

        let (state, decision) =
            apply_foreground_policy(state, &observation, "com.android.settings")
                .expect("foreground transition");

        assert!(matches!(
            decision,
            ForegroundPolicyDecision::ContinueInBackground { .. }
        ));
        assert_eq!(state.phase, TaskPhase::Waiting);
        assert!(matches!(
            state.blocker,
            Some(TaskBlocker::WaitingForExternalCondition { .. })
        ));
        assert_eq!(state.last_runtime_observation, Some(observation));
    }

    #[test]
    fn foreground_policy_preserves_unrelated_blocker_when_foreground_is_missing() {
        let mut state = TaskState::new_background_task("task-1", "watch settings");
        state.phase = TaskPhase::Waiting;
        state.blocker = Some(TaskBlocker::WaitingForUserApproval {
            reason: "approve checkout".to_string(),
        });
        let observation = foreground_observation("com.android.settings.other");

        let (state, decision) =
            apply_foreground_policy(state, &observation, "com.android.settings")
                .expect("foreground transition");

        assert!(matches!(
            decision,
            ForegroundPolicyDecision::RequireForeground { .. }
        ));
        assert!(matches!(
            state.blocker,
            Some(TaskBlocker::WaitingForUserApproval { .. })
        ));
        assert_eq!(state.last_runtime_observation, Some(observation));
    }

    #[test]
    fn foreground_policy_preserves_last_action_boundary_when_blocking() {
        let mut state = TaskState::new_background_task("task-1", "submit form");
        state = record_action_checkpoint(
            state,
            ActionBoundary::new(
                "support-form-submit",
                ActionBoundaryState::Committed,
                "support form submitted",
            ),
        )
        .expect("record action checkpoint");
        let original_checkpoint = state.checkpoint.clone();
        let observation = foreground_observation("com.android.settings.other");

        let (state, decision) =
            apply_foreground_policy(state, &observation, "com.android.settings")
                .expect("foreground transition");

        assert!(matches!(
            decision,
            ForegroundPolicyDecision::RequireForeground { .. }
        ));
        assert_eq!(state.checkpoint, original_checkpoint);
        assert_eq!(state.last_runtime_observation, Some(observation));
        assert!(matches!(
            state.blocker,
            Some(TaskBlocker::WaitingForForeground { .. })
        ));
    }

    #[test]
    fn foreground_policy_preserves_last_action_boundary_when_clearing_blocker() {
        let mut state = TaskState::new_background_task("task-1", "submit form");
        state = record_action_checkpoint(
            state,
            ActionBoundary::new(
                "support-form-submit",
                ActionBoundaryState::Committed,
                "support form submitted",
            ),
        )
        .expect("record action checkpoint");
        let original_checkpoint = state.checkpoint.clone();
        state = require_foreground_attention(state, "return to settings")
            .expect("require foreground attention");
        let observation = foreground_observation("com.android.settings");

        let (state, decision) =
            apply_foreground_policy(state, &observation, "com.android.settings")
                .expect("foreground transition");

        assert!(matches!(
            decision,
            ForegroundPolicyDecision::ContinueInBackground { .. }
        ));
        assert_eq!(state.checkpoint, original_checkpoint);
        assert_eq!(state.last_runtime_observation, Some(observation));
        assert!(state.blocker.is_none());
    }

    #[test]
    fn external_condition_sets_waiting_blocker_and_preserves_checkpoint() {
        let mut state = TaskState::new_background_task("task-1", "wait for network");
        state = record_action_checkpoint(
            state,
            ActionBoundary::new(
                "support-form-submit",
                ActionBoundaryState::Committed,
                "support form submitted",
            ),
        )
        .expect("record action checkpoint");
        let original_checkpoint = state.checkpoint.clone();
        let observation = RuntimeObservation {
            source: RuntimeObservationSource::Android {
                substrate: "ReconRootedStock".to_string(),
            },
            event: RuntimeEvent::NetworkAvailabilityChanged { available: false },
        };

        let state =
            require_external_condition(state, "network unavailable", Some(observation.clone()))
                .expect("require external condition");

        assert_eq!(state.phase, TaskPhase::Waiting);
        assert_eq!(state.checkpoint, original_checkpoint);
        assert_eq!(state.last_runtime_observation, Some(observation));
        assert!(matches!(
            state.blocker,
            Some(TaskBlocker::WaitingForExternalCondition { .. })
        ));
        assert!(matches!(
            state
                .current_activity
                .as_ref()
                .map(|activity| activity.kind),
            Some(AgentActivityKind::Waiting)
        ));
        assert!(matches!(
            state
                .current_activity
                .as_ref()
                .and_then(|activity| activity.target.as_ref()),
            Some(AgentActivityTarget::Network)
        ));
    }

    #[test]
    fn external_condition_preserves_unrelated_blocker() {
        let mut state = TaskState::new_background_task("task-1", "approve checkout");
        state.phase = TaskPhase::Waiting;
        state.blocker = Some(TaskBlocker::WaitingForUserApproval {
            reason: "approve checkout".to_string(),
        });
        let observation = RuntimeObservation {
            source: RuntimeObservationSource::Android {
                substrate: "ReconRootedStock".to_string(),
            },
            event: RuntimeEvent::NetworkAvailabilityChanged { available: false },
        };

        let state =
            require_external_condition(state, "network unavailable", Some(observation.clone()))
                .expect("require external condition");

        assert_eq!(state.phase, TaskPhase::Waiting);
        assert_eq!(state.last_runtime_observation, Some(observation));
        assert!(matches!(
            state.blocker,
            Some(TaskBlocker::WaitingForUserApproval { .. })
        ));
        assert_eq!(
            state
                .current_activity
                .as_ref()
                .map(|activity| activity.description.as_str()),
            Some("approve checkout")
        );
    }

    #[test]
    fn external_condition_satisfaction_clears_matching_blocker() {
        let state = TaskState::new_background_task("task-1", "wait for network");
        let blocked = require_external_condition(state, "network unavailable", None)
            .expect("require external condition");
        let observation = RuntimeObservation {
            source: RuntimeObservationSource::Android {
                substrate: "ReconRootedStock".to_string(),
            },
            event: RuntimeEvent::NetworkAvailabilityChanged { available: true },
        };

        let state =
            satisfy_external_condition(blocked, observation.clone()).expect("satisfy condition");

        assert_eq!(state.phase, TaskPhase::Running);
        assert!(state.blocker.is_none());
        assert_eq!(state.last_runtime_observation, Some(observation));
        assert!(matches!(
            state
                .current_activity
                .as_ref()
                .map(|activity| activity.kind),
            Some(AgentActivityKind::Observing)
        ));
    }

    #[test]
    fn external_condition_satisfaction_preserves_checkpoint_phase() {
        let mut state = TaskState::new_background_task("task-1", "wait for receipt");
        state = record_action_checkpoint(
            state,
            ActionBoundary::new(
                "support-form-submit",
                ActionBoundaryState::Committed,
                "support form submitted",
            ),
        )
        .expect("record action checkpoint");
        let blocked = require_external_condition(state, "waiting for provider confirmation", None)
            .expect("require external condition");
        let observation = RuntimeObservation {
            source: RuntimeObservationSource::Android {
                substrate: "ReconRootedStock".to_string(),
            },
            event: RuntimeEvent::NetworkAvailabilityChanged { available: true },
        };

        let state =
            satisfy_external_condition(blocked, observation).expect("satisfy external condition");

        assert_eq!(state.phase, TaskPhase::Checkpointed);
        assert!(state.blocker.is_none());
        assert!(state.checkpoint.is_some());
    }

    #[test]
    fn external_condition_satisfaction_preserves_unrelated_blocker() {
        let mut state = TaskState::new_background_task("task-1", "approve checkout");
        state.phase = TaskPhase::Waiting;
        state.blocker = Some(TaskBlocker::WaitingForUserApproval {
            reason: "approve checkout".to_string(),
        });
        let observation = RuntimeObservation {
            source: RuntimeObservationSource::Android {
                substrate: "ReconRootedStock".to_string(),
            },
            event: RuntimeEvent::NetworkAvailabilityChanged { available: true },
        };

        let state =
            satisfy_external_condition(state, observation).expect("satisfy external condition");

        assert_eq!(state.phase, TaskPhase::Waiting);
        assert!(matches!(
            state.blocker,
            Some(TaskBlocker::WaitingForUserApproval { .. })
        ));
        assert_eq!(
            state
                .current_activity
                .as_ref()
                .map(|activity| activity.description.as_str()),
            Some("approve checkout")
        );
    }

    #[test]
    fn foreground_policy_does_not_turn_existing_checkpoint_into_running_work() {
        let mut state = TaskState::new_background_task("task-1", "submit form");
        state = record_action_checkpoint(
            state,
            ActionBoundary::new(
                "support-form-submit",
                ActionBoundaryState::Committed,
                "support form submitted",
            ),
        )
        .expect("record action checkpoint");
        let observation = foreground_observation("com.android.settings");

        let (state, decision) =
            apply_foreground_policy(state, &observation, "com.android.settings")
                .expect("foreground transition");

        assert!(matches!(
            decision,
            ForegroundPolicyDecision::ContinueInBackground { .. }
        ));
        assert_eq!(state.phase, TaskPhase::Checkpointed);
        assert_eq!(state.last_runtime_observation, Some(observation));
    }

    #[test]
    fn foreground_policy_rejects_terminal_tasks() {
        let mut state = TaskState::new_background_task("task-1", "watch settings");
        state.phase = TaskPhase::Completed;
        let observation = foreground_observation("com.android.settings");

        let error = apply_foreground_policy(state, &observation, "com.android.settings")
            .expect_err("terminal task should reject transition");

        assert!(matches!(error, TaskTransitionError::TerminalTask { .. }));
    }

    #[test]
    fn heartbeat_checkpoint_rejects_blocked_tasks() {
        let mut state = TaskState::new_background_task("task-1", "watch settings");
        state.blocker = Some(TaskBlocker::WaitingForForeground {
            reason: "needs foreground".to_string(),
        });

        let error = record_action_checkpoint(
            state,
            ActionBoundary::new(
                "heartbeat:1",
                ActionBoundaryState::Verified,
                "heartbeat 1/1",
            ),
        )
        .expect_err("blocked checkpoint should reject");

        assert!(matches!(error, TaskTransitionError::BlockedTask { .. }));
    }

    #[test]
    fn heartbeat_checkpoint_rejects_terminal_tasks() {
        let mut state = TaskState::new_background_task("task-1", "watch settings");
        state.phase = TaskPhase::Failed;

        let error = record_action_checkpoint(
            state,
            ActionBoundary::new(
                "heartbeat:1",
                ActionBoundaryState::Verified,
                "heartbeat 1/1",
            ),
        )
        .expect_err("terminal checkpoint should reject");

        assert!(matches!(error, TaskTransitionError::TerminalTask { .. }));
    }
}
