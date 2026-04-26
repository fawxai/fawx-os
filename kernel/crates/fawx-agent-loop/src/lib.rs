//! Deterministic agent loop skeleton for Fawx OS.
//!
//! This crate owns the first executable loop contract: read a persisted task,
//! reduce typed runtime observations, persist the resulting task state through
//! the atomic task-store transition primitive, and return a typed next action.
//! It deliberately does not call a model yet.

use std::error::Error;
use std::fmt::{Display, Formatter};

use fawx_harness::{
    ForegroundPolicyDecision, ModelActivityProposal, RuntimeEvent, RuntimeObservation, TaskPhase,
    TaskState, TaskTransitionError, apply_foreground_policy, clear_agent_activity,
    record_action_checkpoint, record_current_blocker_activity, record_model_declared_activity,
    record_planning_activity, require_external_condition, require_foreground_attention_for_package,
    satisfy_external_condition,
};
use fawx_kernel::{ActionBoundary, ActionBoundaryState, TaskBlocker};
use fawx_task_store::{StoredTask, TaskStore, TaskStoreError, TaskStoreTransitionError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopStepRequest {
    pub task_id: String,
    pub observations: Vec<RuntimeObservation>,
    pub expected_foreground_package: Option<String>,
    #[serde(default)]
    pub model_activity: Option<ModelActivityProposal>,
}

impl LoopStepRequest {
    pub fn new(task_id: impl Into<String>) -> Self {
        Self {
            task_id: task_id.into(),
            observations: vec![],
            expected_foreground_package: None,
            model_activity: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopStepResult {
    pub task: StoredTask,
    pub decision: LoopDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopDecision {
    pub task_id: String,
    pub phase: TaskPhase,
    pub next_action: NextAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NextAction {
    ContinueLocalWork {
        reason: String,
        checkpoint_id: Option<String>,
    },
    AwaitObservation {
        reason: String,
    },
    ReacquireForeground {
        reason: String,
    },
    WaitForExternalCondition {
        reason: String,
    },
    StopTerminal {
        phase: TaskPhase,
    },
}

#[derive(Debug)]
pub enum AgentLoopError {
    Store(TaskStoreError),
    Transition(TaskTransitionError),
    MissingDecision,
}

impl Display for AgentLoopError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => Display::fmt(error, f),
            Self::Transition(error) => Display::fmt(error, f),
            Self::MissingDecision => write!(f, "loop transition did not produce a decision"),
        }
    }
}

impl Error for AgentLoopError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::Transition(error) => Some(error),
            Self::MissingDecision => None,
        }
    }
}

impl From<TaskStoreError> for AgentLoopError {
    fn from(error: TaskStoreError) -> Self {
        Self::Store(error)
    }
}

impl From<TaskTransitionError> for AgentLoopError {
    fn from(error: TaskTransitionError) -> Self {
        Self::Transition(error)
    }
}

impl From<TaskStoreTransitionError<TaskTransitionError>> for AgentLoopError {
    fn from(error: TaskStoreTransitionError<TaskTransitionError>) -> Self {
        match error {
            TaskStoreTransitionError::Store(error) => Self::Store(error),
            TaskStoreTransitionError::Transition(error) => Self::Transition(error),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentLoop {
    store: TaskStore,
}

impl AgentLoop {
    pub fn new(store: TaskStore) -> Self {
        Self { store }
    }

    pub fn step(&self, request: LoopStepRequest) -> Result<LoopStepResult, AgentLoopError> {
        let mut decision = None;
        let task = self.store.transition_state(&request.task_id, |state| {
            let (state, next_decision) = reduce_step(state, &request)?;
            decision = Some(next_decision);
            Ok::<_, TaskTransitionError>(state)
        })?;

        Ok(LoopStepResult {
            task,
            decision: decision.ok_or(AgentLoopError::MissingDecision)?,
        })
    }
}

fn reduce_step(
    state: TaskState,
    request: &LoopStepRequest,
) -> Result<(TaskState, LoopDecision), TaskTransitionError> {
    if state.phase.is_terminal() {
        let phase = state.phase;
        return Ok(decision_for(
            clear_agent_activity(state),
            NextAction::StopTerminal { phase },
        ));
    }

    let mut state = state;
    let mut latest_foreground_observation = None;
    let expects_foreground = request.expected_foreground_package.is_some();
    for observation in &request.observations {
        if expects_foreground && is_foreground_observation(observation) {
            latest_foreground_observation = Some(observation);
            continue;
        }

        state = reduce_non_foreground_observation(state, observation)?;
    }

    if let Some(expected_package) = request.expected_foreground_package.as_deref() {
        if let Some(observation) = latest_foreground_observation {
            let (next_state, foreground_decision) =
                apply_foreground_policy(state, observation, expected_package)?;
            state = next_state;
            if let Some(next_action) = action_for_blocker(&state.blocker) {
                return Ok(decision_for(
                    record_current_blocker_activity(state)?,
                    next_action,
                ));
            }
            if let ForegroundPolicyDecision::RequireForeground { reason } = foreground_decision {
                return Ok(decision_for(
                    state,
                    NextAction::ReacquireForeground { reason },
                ));
            }
        } else if let Some(next_action) = action_for_blocker(&state.blocker) {
            return Ok(decision_for(
                record_current_blocker_activity(state)?,
                next_action,
            ));
        } else {
            let reason = format!(
                "expected foreground package {expected_package}, but no foreground observation was supplied"
            );
            let state =
                require_foreground_attention_for_package(state, expected_package, reason.clone())?;
            return Ok(decision_for(
                state,
                NextAction::ReacquireForeground { reason },
            ));
        }
    }

    if let Some(next_action) = action_for_blocker(&state.blocker) {
        return Ok(decision_for(
            record_current_blocker_activity(state)?,
            next_action,
        ));
    }

    if state.checkpoint.is_none() {
        let checkpoint_id = "loop:initial-plan".to_string();
        let state = record_action_checkpoint(
            state,
            ActionBoundary::new(
                checkpoint_id.clone(),
                ActionBoundaryState::Planned,
                "agent loop accepted task for local planning",
            ),
        )?;
        let state = record_continue_activity(state, request, "planning local work")?;
        return Ok(decision_for(
            state,
            NextAction::ContinueLocalWork {
                reason: "task accepted for local planning".to_string(),
                checkpoint_id: Some(checkpoint_id),
            },
        ));
    }

    Ok(decision_for(
        record_continue_activity(state, request, "continuing local planning from checkpoint")?,
        NextAction::ContinueLocalWork {
            reason: "task already has a checkpoint and can continue local planning".to_string(),
            checkpoint_id: None,
        },
    ))
}

fn is_foreground_observation(observation: &RuntimeObservation) -> bool {
    matches!(
        observation.event,
        RuntimeEvent::ForegroundAppChanged { .. } | RuntimeEvent::ForegroundUnavailable { .. }
    )
}

fn reduce_non_foreground_observation(
    state: TaskState,
    observation: &RuntimeObservation,
) -> Result<TaskState, TaskTransitionError> {
    match &observation.event {
        RuntimeEvent::NetworkAvailabilityChanged { available: false } => {
            let reason = "network is unavailable".to_string();
            require_external_condition(state, reason, Some(observation.clone()))
        }
        RuntimeEvent::NetworkAvailabilityChanged { available: true } => {
            satisfy_external_condition(state, observation.clone())
        }
        RuntimeEvent::RuntimeActionFailed { action, reason } => {
            let reason = format!("runtime action {action} failed: {reason}");
            require_external_condition(state, reason, Some(observation.clone()))
        }
        _ => {
            let mut state = state;
            state.last_runtime_observation = Some(observation.clone());
            Ok(state)
        }
    }
}

fn action_for_blocker(blocker: &Option<TaskBlocker>) -> Option<NextAction> {
    match blocker {
        Some(TaskBlocker::WaitingForForeground { reason }) => {
            Some(NextAction::ReacquireForeground {
                reason: reason.clone(),
            })
        }
        Some(TaskBlocker::WaitingForExternalCondition { reason }) => {
            Some(NextAction::WaitForExternalCondition {
                reason: reason.clone(),
            })
        }
        Some(TaskBlocker::WaitingForUserApproval { reason })
        | Some(TaskBlocker::WaitingForUserInput { reason }) => Some(NextAction::AwaitObservation {
            reason: reason.clone(),
        }),
        None => None,
    }
}

fn decision_for(state: TaskState, next_action: NextAction) -> (TaskState, LoopDecision) {
    let decision = LoopDecision {
        task_id: state.task_id.clone(),
        phase: state.phase,
        next_action,
    };
    (state, decision)
}

fn record_continue_activity(
    state: TaskState,
    request: &LoopStepRequest,
    description: impl Into<String>,
) -> Result<TaskState, TaskTransitionError> {
    if let Some(model_activity) = request.model_activity.clone() {
        record_model_declared_activity(state, model_activity)
    } else {
        record_planning_activity(state, description)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fawx_harness::{ModelActivityKind, RuntimeObservationSource, TaskState};
    use fawx_kernel::{AgentActivityKind, AgentActivitySource, AgentActivityTarget};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_loop() -> (TaskStore, AgentLoop) {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let store = TaskStore::new(
            std::env::temp_dir().join(format!("fawx-agent-loop-test-{}-{id}", std::process::id())),
        );
        let loop_runner = AgentLoop::new(store.clone());
        (store, loop_runner)
    }

    fn android_foreground(package_name: &str) -> RuntimeObservation {
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
    fn first_step_persists_initial_planning_checkpoint() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task(
                "task-1",
                "cancel subscription",
            ))
            .expect("create task");

        let result = loop_runner
            .step(LoopStepRequest::new("task-1"))
            .expect("step task");

        assert!(matches!(
            result.decision.next_action,
            NextAction::ContinueLocalWork { .. }
        ));
        assert_eq!(result.task.state.phase, TaskPhase::Checkpointed);
        assert_eq!(
            result
                .task
                .state
                .checkpoint
                .as_ref()
                .expect("checkpoint")
                .action_boundary
                .id,
            "loop:initial-plan"
        );
        assert!(matches!(
            result
                .task
                .state
                .current_activity
                .as_ref()
                .map(|activity| activity.kind),
            Some(AgentActivityKind::Planning)
        ));
        assert!(matches!(
            result
                .task
                .state
                .current_activity
                .as_ref()
                .and_then(|activity| activity.target.as_ref()),
            Some(AgentActivityTarget::Task)
        ));
    }

    #[test]
    fn model_declared_activity_overrides_default_planning_activity_when_unblocked() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task("task-1", "inspect settings"))
            .expect("create task");

        let result = loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![],
                expected_foreground_package: None,
                model_activity: Some(ModelActivityProposal {
                    kind: ModelActivityKind::Observing,
                    target: Some(AgentActivityTarget::AndroidPackage {
                        package_name: "com.android.settings".to_string(),
                    }),
                    description: "checking settings state".to_string(),
                }),
            })
            .expect("step task");

        assert!(matches!(
            result.decision.next_action,
            NextAction::ContinueLocalWork { .. }
        ));
        let activity = result
            .task
            .state
            .current_activity
            .as_ref()
            .expect("model activity");
        assert_eq!(activity.kind, AgentActivityKind::Observing);
        assert_eq!(activity.source, AgentActivitySource::ModelDeclared);
        assert_eq!(activity.description, "checking settings state");
        assert!(matches!(
            activity.target.as_ref(),
            Some(AgentActivityTarget::AndroidPackage { package_name })
                if package_name == "com.android.settings"
        ));
    }

    #[test]
    fn model_declared_activity_cannot_override_blocker_activity() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task("task-1", "inspect settings"))
            .expect("create task");
        loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![],
                expected_foreground_package: Some("com.android.settings".to_string()),
                model_activity: None,
            })
            .expect("block task");

        let result = loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![],
                expected_foreground_package: None,
                model_activity: Some(ModelActivityProposal {
                    kind: ModelActivityKind::Planning,
                    target: Some(AgentActivityTarget::Task),
                    description: "planning despite blocker".to_string(),
                }),
            })
            .expect("blocked model activity should preserve blocker");

        assert!(matches!(
            result.decision.next_action,
            NextAction::ReacquireForeground { .. }
        ));
        assert!(matches!(
            result
                .task
                .state
                .current_activity
                .as_ref()
                .map(|activity| activity.kind),
            Some(AgentActivityKind::Waiting)
        ));
    }

    #[test]
    fn terminal_task_ignores_model_declared_activity_without_reopening_work() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task("task-1", "already done"))
            .expect("create task");
        store
            .transition_state("task-1", |mut state| {
                state.phase = TaskPhase::Completed;
                Ok::<_, TaskStoreError>(state)
            })
            .expect("complete task");

        let result = loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![],
                expected_foreground_package: None,
                model_activity: Some(ModelActivityProposal {
                    kind: ModelActivityKind::Observing,
                    target: Some(AgentActivityTarget::Task),
                    description: "trying to reopen work".to_string(),
                }),
            })
            .expect("step terminal task");

        assert_eq!(
            result.decision.next_action,
            NextAction::StopTerminal {
                phase: TaskPhase::Completed
            }
        );
        assert!(result.task.state.current_activity.is_none());
    }

    #[test]
    fn foreground_mismatch_requires_foreground_and_persists_blocker() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task("task-1", "watch settings"))
            .expect("create task");

        let result = loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![android_foreground("com.google.android.apps.nexuslauncher")],
                expected_foreground_package: Some("com.android.settings".to_string()),
                model_activity: None,
            })
            .expect("step task");

        assert!(matches!(
            result.decision.next_action,
            NextAction::ReacquireForeground { .. }
        ));
        assert!(matches!(
            result.task.state.blocker,
            Some(TaskBlocker::WaitingForForeground { .. })
        ));
        assert!(matches!(
            result
                .task
                .state
                .current_activity
                .as_ref()
                .map(|activity| activity.kind),
            Some(AgentActivityKind::Waiting)
        ));
        assert!(matches!(
            result
                .task
                .state
                .current_activity
                .as_ref()
                .and_then(|activity| activity.target.as_ref()),
            Some(AgentActivityTarget::AndroidPackage { package_name })
                if package_name == "com.android.settings"
        ));
    }

    #[test]
    fn missing_foreground_observation_blocks_when_foreground_package_is_expected() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task("task-1", "watch settings"))
            .expect("create task");

        let result = loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![],
                expected_foreground_package: Some("com.android.settings".to_string()),
                model_activity: None,
            })
            .expect("step task");

        assert!(matches!(
            result.decision.next_action,
            NextAction::ReacquireForeground { .. }
        ));
        assert!(matches!(
            result.task.state.blocker,
            Some(TaskBlocker::WaitingForForeground { .. })
        ));
        assert!(result.task.state.checkpoint.is_none());
    }

    #[test]
    fn missing_foreground_observation_preserves_existing_user_blocker_decision() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task("task-1", "approve checkout"))
            .expect("create task");
        store
            .transition_state("task-1", |mut state| {
                state.phase = TaskPhase::Waiting;
                state.blocker = Some(TaskBlocker::WaitingForUserApproval {
                    reason: "approve checkout".to_string(),
                });
                Ok::<_, TaskStoreError>(state)
            })
            .expect("block task");

        let result = loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![],
                expected_foreground_package: Some("com.android.settings".to_string()),
                model_activity: None,
            })
            .expect("step task");

        assert_eq!(
            result.decision.next_action,
            NextAction::AwaitObservation {
                reason: "approve checkout".to_string(),
            }
        );
        assert!(matches!(
            result.task.state.blocker,
            Some(TaskBlocker::WaitingForUserApproval { .. })
        ));
        assert_eq!(
            result
                .task
                .state
                .current_activity
                .as_ref()
                .map(|activity| activity.description.as_str()),
            Some("approve checkout")
        );
    }

    #[test]
    fn foreground_match_clears_existing_foreground_blocker() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task("task-1", "watch settings"))
            .expect("create task");
        loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![android_foreground("com.google.android.apps.nexuslauncher")],
                expected_foreground_package: Some("com.android.settings".to_string()),
                model_activity: None,
            })
            .expect("block task");

        let result = loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![android_foreground("com.android.settings")],
                expected_foreground_package: Some("com.android.settings".to_string()),
                model_activity: None,
            })
            .expect("unblock task");

        assert!(matches!(
            result.decision.next_action,
            NextAction::ContinueLocalWork { .. } | NextAction::AwaitObservation { .. }
        ));
        assert!(result.task.state.blocker.is_none());
        assert_eq!(result.task.state.phase, TaskPhase::Checkpointed);
        assert_eq!(
            result
                .task
                .state
                .checkpoint
                .as_ref()
                .expect("checkpoint")
                .action_boundary
                .id,
            "loop:initial-plan"
        );
    }

    #[test]
    fn latest_foreground_observation_wins_within_batch() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task("task-1", "watch settings"))
            .expect("create task");

        let result = loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![
                    android_foreground("com.google.android.apps.nexuslauncher"),
                    android_foreground("com.android.settings"),
                ],
                expected_foreground_package: Some("com.android.settings".to_string()),
                model_activity: None,
            })
            .expect("step task");

        assert!(matches!(
            result.decision.next_action,
            NextAction::ContinueLocalWork { .. }
        ));
        assert!(result.task.state.blocker.is_none());
    }

    #[test]
    fn latest_foreground_observation_can_block_within_batch() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task("task-1", "watch settings"))
            .expect("create task");

        let result = loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![
                    android_foreground("com.android.settings"),
                    android_foreground("com.google.android.apps.nexuslauncher"),
                ],
                expected_foreground_package: Some("com.android.settings".to_string()),
                model_activity: None,
            })
            .expect("step task");

        assert!(matches!(
            result.decision.next_action,
            NextAction::ReacquireForeground { .. }
        ));
        assert!(matches!(
            result.task.state.blocker,
            Some(TaskBlocker::WaitingForForeground { .. })
        ));
    }

    #[test]
    fn foreground_mismatch_preserves_existing_user_blocker_decision() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task("task-1", "approve checkout"))
            .expect("create task");
        store
            .transition_state("task-1", |mut state| {
                state.phase = TaskPhase::Waiting;
                state.blocker = Some(TaskBlocker::WaitingForUserApproval {
                    reason: "approve checkout".to_string(),
                });
                Ok::<_, TaskStoreError>(state)
            })
            .expect("block task");

        let result = loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![android_foreground("com.google.android.apps.nexuslauncher")],
                expected_foreground_package: Some("com.android.settings".to_string()),
                model_activity: None,
            })
            .expect("step task");

        assert_eq!(
            result.decision.next_action,
            NextAction::AwaitObservation {
                reason: "approve checkout".to_string(),
            }
        );
        assert!(matches!(
            result.task.state.blocker,
            Some(TaskBlocker::WaitingForUserApproval { .. })
        ));
    }

    #[test]
    fn network_loss_blocks_on_external_condition() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task("task-1", "poll network"))
            .expect("create task");

        let result = loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![RuntimeObservation {
                    source: RuntimeObservationSource::Cloud {
                        provider: "test".to_string(),
                    },
                    event: RuntimeEvent::NetworkAvailabilityChanged { available: false },
                }],
                expected_foreground_package: None,
                model_activity: None,
            })
            .expect("step task");

        assert!(matches!(
            result.decision.next_action,
            NextAction::WaitForExternalCondition { .. }
        ));
        assert!(matches!(
            result.task.state.blocker,
            Some(TaskBlocker::WaitingForExternalCondition { .. })
        ));
    }

    #[test]
    fn network_recovery_clears_external_condition_blocker() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task("task-1", "poll network"))
            .expect("create task");
        loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![RuntimeObservation {
                    source: RuntimeObservationSource::Cloud {
                        provider: "test".to_string(),
                    },
                    event: RuntimeEvent::NetworkAvailabilityChanged { available: false },
                }],
                expected_foreground_package: None,
                model_activity: None,
            })
            .expect("block task");

        let result = loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![RuntimeObservation {
                    source: RuntimeObservationSource::Cloud {
                        provider: "test".to_string(),
                    },
                    event: RuntimeEvent::NetworkAvailabilityChanged { available: true },
                }],
                expected_foreground_package: None,
                model_activity: None,
            })
            .expect("recover task");

        assert!(matches!(
            result.decision.next_action,
            NextAction::ContinueLocalWork { .. }
        ));
        assert!(result.task.state.blocker.is_none());
        assert_eq!(result.task.state.phase, TaskPhase::Checkpointed);
        assert!(matches!(
            result
                .task
                .state
                .current_activity
                .as_ref()
                .map(|activity| activity.kind),
            Some(AgentActivityKind::Planning)
        ));
    }

    #[test]
    fn latest_network_observation_wins_within_batch() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task("task-1", "poll network"))
            .expect("create task");

        let result = loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![
                    RuntimeObservation {
                        source: RuntimeObservationSource::Cloud {
                            provider: "test".to_string(),
                        },
                        event: RuntimeEvent::NetworkAvailabilityChanged { available: false },
                    },
                    RuntimeObservation {
                        source: RuntimeObservationSource::Cloud {
                            provider: "test".to_string(),
                        },
                        event: RuntimeEvent::NetworkAvailabilityChanged { available: true },
                    },
                ],
                expected_foreground_package: None,
                model_activity: None,
            })
            .expect("step task");

        assert!(matches!(
            result.decision.next_action,
            NextAction::ContinueLocalWork { .. }
        ));
        assert!(result.task.state.blocker.is_none());
    }

    #[test]
    fn terminal_task_returns_stop_without_reopening_work() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task("task-1", "already done"))
            .expect("create task");
        loop_runner
            .step(LoopStepRequest::new("task-1"))
            .expect("seed current activity");
        store
            .transition_state("task-1", |mut state| {
                assert!(state.current_activity.is_some());
                state.phase = TaskPhase::Completed;
                Ok::<_, TaskStoreError>(state)
            })
            .expect("complete task");

        let result = loop_runner
            .step(LoopStepRequest::new("task-1"))
            .expect("step terminal task");

        assert_eq!(
            result.decision.next_action,
            NextAction::StopTerminal {
                phase: TaskPhase::Completed
            }
        );
        assert_eq!(result.task.state.phase, TaskPhase::Completed);
        assert!(result.task.state.current_activity.is_none());
    }
}
