//! Thin harness contracts for the Fawx OS runtime loop.
//!
//! The harness should orchestrate model I/O, tool dispatch, and explicit
//! completion without smuggling policy or substrate assumptions into the loop.

pub use fawx_kernel::TaskPhase;
use fawx_kernel::{
    ActionBoundary, AttentionRequirement, ExecutionContract, TaskBlocker, TaskCheckpoint,
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
            last_runtime_observation: None,
        }
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
        }
    }
}

impl Error for TaskTransitionError {}

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
    foreground_required(state, &reason.into(), None)
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
        ForegroundPolicyDecision::RequireForeground { reason } => {
            foreground_required(state, reason, Some(observation.clone()))?
        }
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
    }

    Ok(state)
}

fn foreground_required(
    mut state: TaskState,
    reason: &str,
    observation: Option<RuntimeObservation>,
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
    Ok(state)
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
