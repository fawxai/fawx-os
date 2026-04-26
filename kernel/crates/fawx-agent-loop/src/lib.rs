//! Deterministic agent loop skeleton for Fawx OS.
//!
//! This crate owns the first executable loop contract: read a persisted task,
//! reduce typed runtime observations, persist the resulting task state through
//! the atomic task-store transition primitive, and return a typed next action.
//! It deliberately does not call a model yet.

use std::error::Error;
use std::fmt::{Display, Formatter};

use fawx_harness::{
    ForegroundPolicyDecision, ModelActionProposal, ModelActivityProposal, RuntimeEvent,
    RuntimeObservation, TaskPhase, TaskState, TaskTransitionError, accept_model_action_proposal,
    apply_foreground_policy, clear_agent_activity, record_action_checkpoint,
    record_current_blocker_activity, record_model_declared_activity, record_planning_activity,
    require_external_condition, require_foreground_attention_for_package,
    satisfy_external_condition,
};
use fawx_kernel::{
    ActionBoundary, ActionBoundaryState, AgentActionStatus, AgentActivityTarget, TaskBlocker,
};
use fawx_task_store::{StoredTask, TaskStore, TaskStoreError, TaskStoreTransitionError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopStepRequest {
    pub task_id: String,
    pub observations: Vec<RuntimeObservation>,
    pub expected_foreground_package: Option<String>,
    #[serde(default)]
    pub model_activity: Option<ModelActivityProposal>,
    #[serde(default)]
    pub model_action: Option<ModelActionProposal>,
}

impl LoopStepRequest {
    pub fn new(task_id: impl Into<String>) -> Self {
        Self {
            task_id: task_id.into(),
            observations: vec![],
            expected_foreground_package: None,
            model_activity: None,
            model_action: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopStepResult {
    pub task: StoredTask,
    pub decision: LoopDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundTickRequest {
    pub tick_id: u64,
    #[serde(default)]
    pub observations: Vec<BackgroundObservation>,
}

impl BackgroundTickRequest {
    pub fn new(tick_id: u64) -> Self {
        Self {
            tick_id,
            observations: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundObservation {
    pub scope: BackgroundObservationScope,
    pub observation: RuntimeObservation,
}

impl BackgroundObservation {
    pub fn global(observation: RuntimeObservation) -> Self {
        Self {
            scope: BackgroundObservationScope::Global,
            observation,
        }
    }

    pub fn for_task(task_id: impl Into<String>, observation: RuntimeObservation) -> Self {
        Self {
            scope: BackgroundObservationScope::Task {
                task_id: task_id.into(),
            },
            observation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundObservationScope {
    /// Substrate-wide state, such as network availability, that may be reduced
    /// by every task. Foreground observations are never treated as global.
    Global,
    /// Evidence explicitly owned by one task. Foreground observations require
    /// this scope so one visible app sample cannot accidentally close several
    /// unrelated actions.
    Task { task_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundTickResult {
    pub tick_id: u64,
    pub tasks: Vec<BackgroundTaskTick>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundTaskTick {
    pub task_id: String,
    pub outcome: BackgroundTaskTickOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundTaskTickOutcome {
    Stepped { decision: LoopDecision },
    SkippedTerminal { phase: TaskPhase },
    Failed { failure: BackgroundTaskTickFailure },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundTaskTickFailure {
    pub kind: BackgroundTaskTickFailureKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundTaskTickFailureKind {
    Store,
    Transition,
    MissingDecision,
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

#[derive(Debug, Clone)]
pub struct BackgroundRunner {
    store: TaskStore,
}

impl BackgroundRunner {
    pub fn new(store: TaskStore) -> Self {
        Self { store }
    }

    pub fn tick(
        &self,
        request: BackgroundTickRequest,
    ) -> Result<BackgroundTickResult, AgentLoopError> {
        let mut tasks = Vec::new();
        let loop_runner = AgentLoop::new(self.store.clone());
        for stored_task in self.store.list()? {
            let task_id = stored_task.state.task_id.clone();
            if stored_task.state.phase.is_terminal() {
                tasks.push(BackgroundTaskTick {
                    task_id,
                    outcome: BackgroundTaskTickOutcome::SkippedTerminal {
                        phase: stored_task.state.phase,
                    },
                });
                continue;
            }

            let (observations, expected_foreground_package) =
                scoped_observations_for_task(&stored_task.state, &request.observations);
            let step_request = LoopStepRequest {
                task_id: task_id.clone(),
                observations,
                expected_foreground_package,
                model_activity: None,
                model_action: None,
            };
            let outcome = match loop_runner.step(step_request) {
                Ok(result) => BackgroundTaskTickOutcome::Stepped {
                    decision: result.decision,
                },
                Err(error) => BackgroundTaskTickOutcome::Failed {
                    failure: BackgroundTaskTickFailure::from_error(&error),
                },
            };
            tasks.push(BackgroundTaskTick { task_id, outcome });
        }

        Ok(BackgroundTickResult {
            tick_id: request.tick_id,
            tasks,
        })
    }
}

impl BackgroundTaskTickFailure {
    fn from_error(error: &AgentLoopError) -> Self {
        let kind = match error {
            AgentLoopError::Store(_) => BackgroundTaskTickFailureKind::Store,
            AgentLoopError::Transition(_) => BackgroundTaskTickFailureKind::Transition,
            AgentLoopError::MissingDecision => BackgroundTaskTickFailureKind::MissingDecision,
        };
        Self {
            kind,
            message: error.to_string(),
        }
    }
}

fn scoped_observations_for_task(
    state: &TaskState,
    observations: &[BackgroundObservation],
) -> (Vec<RuntimeObservation>, Option<String>) {
    let expected_foreground_package = expected_foreground_package_from_current_action(state);
    let mut scoped = Vec::new();
    for observation in observations {
        match &observation.scope {
            BackgroundObservationScope::Global => {
                if !is_foreground_observation(&observation.observation) {
                    scoped.push(observation.observation.clone());
                }
            }
            BackgroundObservationScope::Task { task_id } if task_id == &state.task_id => {
                scoped.push(observation.observation.clone());
            }
            BackgroundObservationScope::Task { .. } => {}
        }
    }

    let expected_foreground_package = scoped
        .iter()
        .any(is_foreground_observation)
        .then_some(expected_foreground_package)
        .flatten();

    (scoped, expected_foreground_package)
}

fn expected_foreground_package_from_current_action(state: &TaskState) -> Option<String> {
    match state.current_action.as_ref() {
        Some(action)
            if action.status == AgentActionStatus::Executing
                && action.boundary.state == ActionBoundaryState::Prepared =>
        {
            match action.target.as_ref() {
                Some(AgentActivityTarget::AndroidPackage { package_name }) => {
                    Some(package_name.clone())
                }
                _ => None,
            }
        }
        _ => None,
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

    let mut state = maybe_accept_model_action(state, request)?;
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
        let state = record_continue_state(state, request, "planning local work")?;
        return Ok(decision_for(
            state,
            NextAction::ContinueLocalWork {
                reason: "task accepted for local planning".to_string(),
                checkpoint_id: Some(checkpoint_id),
            },
        ));
    }

    Ok(decision_for(
        record_continue_state(state, request, "continuing local planning from checkpoint")?,
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

fn record_continue_state(
    state: TaskState,
    request: &LoopStepRequest,
    description: impl Into<String>,
) -> Result<TaskState, TaskTransitionError> {
    let state = if let Some(model_activity) = request.model_activity.clone() {
        record_model_declared_activity(state, model_activity)
    } else {
        record_planning_activity(state, description)
    }?;

    if state.current_action.is_none() {
        maybe_accept_model_action(state, request)
    } else {
        Ok(state)
    }
}

fn maybe_accept_model_action(
    state: TaskState,
    request: &LoopStepRequest,
) -> Result<TaskState, TaskTransitionError> {
    if let Some(model_action) = request.model_action.clone() {
        accept_model_action_proposal(state, model_action)
    } else {
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fawx_harness::{
        ModelActionKind, ModelActivityKind, RuntimeObservationSource, TaskState,
        begin_current_action_execution,
    };
    use fawx_kernel::{
        AgentActionKind, AgentActionStatus, AgentActivityKind, AgentActivitySource,
        AgentActivityTarget,
    };
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

    fn test_background_runner() -> (TaskStore, BackgroundRunner) {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let store = TaskStore::new(std::env::temp_dir().join(format!(
            "fawx-background-runner-test-{}-{id}",
            std::process::id()
        )));
        let runner = BackgroundRunner::new(store.clone());
        (store, runner)
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
    fn background_runner_ticks_all_nonterminal_tasks_in_stable_order() {
        let (store, runner) = test_background_runner();
        store
            .create(TaskState::new_background_task("task-b", "second"))
            .expect("create task b");
        store
            .create(TaskState::new_background_task("task-a", "first"))
            .expect("create task a");

        let result = runner
            .tick(BackgroundTickRequest::new(7))
            .expect("background tick");

        assert_eq!(result.tick_id, 7);
        assert_eq!(
            result
                .tasks
                .iter()
                .map(|task| task.task_id.as_str())
                .collect::<Vec<_>>(),
            vec!["task-a", "task-b"]
        );
        assert!(
            result
                .tasks
                .iter()
                .all(|task| matches!(task.outcome, BackgroundTaskTickOutcome::Stepped { .. }))
        );
        assert!(
            store
                .load("task-a")
                .expect("load task a")
                .state
                .checkpoint
                .is_some()
        );
        assert!(
            store
                .load("task-b")
                .expect("load task b")
                .state
                .checkpoint
                .is_some()
        );
    }

    #[test]
    fn background_runner_skips_terminal_tasks_without_reopening_them() {
        let (store, runner) = test_background_runner();
        store
            .create(TaskState::new_background_task("task-done", "done"))
            .expect("create task");
        store
            .transition_state("task-done", |mut state| {
                state.phase = TaskPhase::Completed;
                Ok::<_, TaskStoreError>(state)
            })
            .expect("complete task");

        let result = runner
            .tick(BackgroundTickRequest::new(1))
            .expect("background tick");

        assert_eq!(result.tasks.len(), 1);
        assert_eq!(result.tasks[0].task_id, "task-done");
        assert_eq!(
            result.tasks[0].outcome,
            BackgroundTaskTickOutcome::SkippedTerminal {
                phase: TaskPhase::Completed
            }
        );
        assert_eq!(
            store.load("task-done").expect("load task").state.phase,
            TaskPhase::Completed
        );
    }

    #[test]
    fn background_runner_reduces_observations_for_waiting_tasks() {
        let (store, runner) = test_background_runner();
        store
            .create(TaskState::new_background_task(
                "task-network",
                "wait for network",
            ))
            .expect("create task");
        AgentLoop::new(store.clone())
            .step(LoopStepRequest {
                task_id: "task-network".to_string(),
                observations: vec![RuntimeObservation {
                    source: RuntimeObservationSource::Cloud {
                        provider: "test".to_string(),
                    },
                    event: RuntimeEvent::NetworkAvailabilityChanged { available: false },
                }],
                expected_foreground_package: None,
                model_activity: None,
                model_action: None,
            })
            .expect("block on network");

        let result = runner
            .tick(BackgroundTickRequest {
                tick_id: 2,
                observations: vec![BackgroundObservation::global(RuntimeObservation {
                    source: RuntimeObservationSource::Cloud {
                        provider: "test".to_string(),
                    },
                    event: RuntimeEvent::NetworkAvailabilityChanged { available: true },
                })],
            })
            .expect("background tick");

        assert!(matches!(
            result.tasks[0].outcome,
            BackgroundTaskTickOutcome::Stepped {
                decision: LoopDecision {
                    next_action: NextAction::ContinueLocalWork { .. },
                    ..
                }
            }
        ));
        let task = store.load("task-network").expect("load task");
        assert!(task.state.blocker.is_none());
        assert_eq!(task.state.phase, TaskPhase::Checkpointed);
    }

    #[test]
    fn background_runner_closes_task_scoped_foreground_action() {
        let (store, runner) = test_background_runner();
        store
            .create(TaskState::new_background_task(
                "task-open-launcher",
                "open launcher",
            ))
            .expect("create task");
        AgentLoop::new(store.clone())
            .step(LoopStepRequest {
                task_id: "task-open-launcher".to_string(),
                observations: vec![],
                expected_foreground_package: None,
                model_activity: None,
                model_action: Some(ModelActionProposal {
                    kind: ModelActionKind::OpenApp,
                    target: Some(AgentActivityTarget::AndroidPackage {
                        package_name: "com.google.android.apps.nexuslauncher".to_string(),
                    }),
                    reason: "open launcher".to_string(),
                    expected_observation: Some("launcher is foreground".to_string()),
                    proposal_id: None,
                }),
            })
            .expect("accept action");
        store
            .transition_state("task-open-launcher", begin_current_action_execution)
            .expect("begin action");

        let result = runner
            .tick(BackgroundTickRequest {
                tick_id: 3,
                observations: vec![BackgroundObservation::for_task(
                    "task-open-launcher",
                    android_foreground("com.google.android.apps.nexuslauncher"),
                )],
            })
            .expect("background tick");

        assert!(matches!(
            result.tasks[0].outcome,
            BackgroundTaskTickOutcome::Stepped {
                decision: LoopDecision {
                    next_action: NextAction::ContinueLocalWork { .. },
                    ..
                }
            }
        ));
        let task = store.load("task-open-launcher").expect("load task");
        let action = task.state.current_action.expect("current action");
        assert_eq!(action.status, AgentActionStatus::Observed);
        assert_eq!(action.boundary.state, ActionBoundaryState::Committed);
    }

    #[test]
    fn background_runner_does_not_apply_global_foreground_to_actions() {
        let (store, runner) = test_background_runner();
        store
            .create(TaskState::new_background_task(
                "task-open-launcher",
                "open launcher",
            ))
            .expect("create task");
        AgentLoop::new(store.clone())
            .step(LoopStepRequest {
                task_id: "task-open-launcher".to_string(),
                observations: vec![],
                expected_foreground_package: None,
                model_activity: None,
                model_action: Some(ModelActionProposal {
                    kind: ModelActionKind::OpenApp,
                    target: Some(AgentActivityTarget::AndroidPackage {
                        package_name: "com.google.android.apps.nexuslauncher".to_string(),
                    }),
                    reason: "open launcher".to_string(),
                    expected_observation: None,
                    proposal_id: None,
                }),
            })
            .expect("accept action");
        store
            .transition_state("task-open-launcher", begin_current_action_execution)
            .expect("begin action");

        runner
            .tick(BackgroundTickRequest {
                tick_id: 4,
                observations: vec![BackgroundObservation::global(android_foreground(
                    "com.google.android.apps.nexuslauncher",
                ))],
            })
            .expect("background tick");

        let task = store.load("task-open-launcher").expect("load task");
        let action = task.state.current_action.expect("current action");
        assert_eq!(action.status, AgentActionStatus::Executing);
        assert_eq!(action.boundary.state, ActionBoundaryState::Prepared);
    }

    #[test]
    fn background_runner_does_not_close_or_block_accepted_action_from_foreground_sample() {
        let (store, runner) = test_background_runner();
        store
            .create(TaskState::new_background_task(
                "task-open-settings",
                "open settings",
            ))
            .expect("create task");
        AgentLoop::new(store.clone())
            .step(LoopStepRequest {
                task_id: "task-open-settings".to_string(),
                observations: vec![],
                expected_foreground_package: None,
                model_activity: None,
                model_action: Some(ModelActionProposal {
                    kind: ModelActionKind::OpenApp,
                    target: Some(AgentActivityTarget::AndroidPackage {
                        package_name: "com.android.settings".to_string(),
                    }),
                    reason: "open settings".to_string(),
                    expected_observation: None,
                    proposal_id: None,
                }),
            })
            .expect("accept action");

        runner
            .tick(BackgroundTickRequest {
                tick_id: 5,
                observations: vec![BackgroundObservation::for_task(
                    "task-open-settings",
                    android_foreground("com.google.android.apps.nexuslauncher"),
                )],
            })
            .expect("background tick");

        let task = store.load("task-open-settings").expect("load task");
        assert!(task.state.blocker.is_none());
        let action = task.state.current_action.expect("current action");
        assert_eq!(action.status, AgentActionStatus::Accepted);
        assert_eq!(action.boundary.state, ActionBoundaryState::Planned);
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
                model_action: None,
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
    fn model_action_proposal_is_accepted_on_unblocked_continue() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task("task-1", "open settings"))
            .expect("create task");

        let result = loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![],
                expected_foreground_package: None,
                model_activity: None,
                model_action: Some(ModelActionProposal {
                    kind: ModelActionKind::OpenApp,
                    target: Some(AgentActivityTarget::AndroidPackage {
                        package_name: "com.android.settings".to_string(),
                    }),
                    reason: "open settings to inspect permissions".to_string(),
                    expected_observation: Some("settings is foreground".to_string()),
                    proposal_id: None,
                }),
            })
            .expect("step task");

        assert!(matches!(
            result.decision.next_action,
            NextAction::ContinueLocalWork { .. }
        ));
        let action = result
            .task
            .state
            .current_action
            .as_ref()
            .expect("current action");
        assert_eq!(action.kind, AgentActionKind::OpenApp);
        assert_eq!(action.status, AgentActionStatus::Accepted);
        assert_eq!(action.reason, "open settings to inspect permissions");
        assert_eq!(
            action.expected_observation.as_deref(),
            Some("settings is foreground")
        );
        assert_eq!(action.boundary.id, "model-action:task-1:1");
        assert!(matches!(
            action.target.as_ref(),
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
                model_action: None,
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
                model_action: None,
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
    fn model_action_proposal_rejects_when_task_is_already_blocked() {
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
                model_action: None,
            })
            .expect("block task");

        let error = loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![],
                expected_foreground_package: None,
                model_activity: None,
                model_action: Some(ModelActionProposal {
                    kind: ModelActionKind::Interact,
                    target: Some(AgentActivityTarget::Task),
                    reason: "interact despite blocker".to_string(),
                    expected_observation: None,
                    proposal_id: None,
                }),
            })
            .expect_err("blocked action should reject");

        assert!(matches!(
            error,
            AgentLoopError::Transition(TaskTransitionError::BlockedTask { .. })
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
                model_action: None,
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
    fn terminal_task_ignores_model_action_without_reopening_work() {
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
                model_activity: None,
                model_action: Some(ModelActionProposal {
                    kind: ModelActionKind::OpenApp,
                    target: Some(AgentActivityTarget::AndroidPackage {
                        package_name: "com.android.settings".to_string(),
                    }),
                    reason: "trying to reopen work".to_string(),
                    expected_observation: None,
                    proposal_id: None,
                }),
            })
            .expect("step terminal task");

        assert_eq!(
            result.decision.next_action,
            NextAction::StopTerminal {
                phase: TaskPhase::Completed
            }
        );
        assert!(result.task.state.current_action.is_none());
    }

    #[test]
    fn accepted_model_action_is_marked_blocked_when_task_blocks_later() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task("task-1", "open settings"))
            .expect("create task");
        loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![],
                expected_foreground_package: None,
                model_activity: None,
                model_action: Some(ModelActionProposal {
                    kind: ModelActionKind::OpenApp,
                    target: Some(AgentActivityTarget::AndroidPackage {
                        package_name: "com.android.settings".to_string(),
                    }),
                    reason: "open settings to inspect permissions".to_string(),
                    expected_observation: None,
                    proposal_id: None,
                }),
            })
            .expect("accept action");
        store
            .transition_state("task-1", begin_current_action_execution)
            .expect("begin action");

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
                model_action: None,
            })
            .expect("block later");

        let action = result
            .task
            .state
            .current_action
            .as_ref()
            .expect("current action");
        assert_eq!(action.status, AgentActionStatus::Blocked);
        assert_eq!(action.boundary.state, ActionBoundaryState::Aborted);
    }

    #[test]
    fn model_action_is_marked_blocked_when_same_step_requires_foreground() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task("task-1", "open settings"))
            .expect("create task");

        let result = loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![],
                expected_foreground_package: Some("com.android.settings".to_string()),
                model_activity: None,
                model_action: Some(ModelActionProposal {
                    kind: ModelActionKind::OpenApp,
                    target: Some(AgentActivityTarget::AndroidPackage {
                        package_name: "com.android.settings".to_string(),
                    }),
                    reason: "open settings to inspect permissions".to_string(),
                    expected_observation: None,
                    proposal_id: None,
                }),
            })
            .expect("block same step");

        assert!(matches!(
            result.decision.next_action,
            NextAction::ReacquireForeground { .. }
        ));
        let action = result
            .task
            .state
            .current_action
            .as_ref()
            .expect("current action");
        assert_eq!(action.status, AgentActionStatus::Blocked);
        assert_eq!(action.boundary.state, ActionBoundaryState::Aborted);
    }

    #[test]
    fn accepted_model_action_is_marked_observed_when_expected_foreground_matches() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task("task-1", "open settings"))
            .expect("create task");
        loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![],
                expected_foreground_package: None,
                model_activity: None,
                model_action: Some(ModelActionProposal {
                    kind: ModelActionKind::OpenApp,
                    target: Some(AgentActivityTarget::AndroidPackage {
                        package_name: "com.android.settings".to_string(),
                    }),
                    reason: "open settings to inspect permissions".to_string(),
                    expected_observation: None,
                    proposal_id: None,
                }),
            })
            .expect("accept action");
        store
            .transition_state("task-1", begin_current_action_execution)
            .expect("begin action");

        let result = loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![android_foreground("com.android.settings")],
                expected_foreground_package: Some("com.android.settings".to_string()),
                model_activity: None,
                model_action: None,
            })
            .expect("observe foreground");

        let action = result
            .task
            .state
            .current_action
            .as_ref()
            .expect("current action");
        assert_eq!(action.status, AgentActionStatus::Observed);
        assert_eq!(action.boundary.state, ActionBoundaryState::Committed);
    }

    #[test]
    fn open_model_action_cannot_be_overwritten_before_observation_reduces() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task("task-1", "open settings"))
            .expect("create task");
        loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![],
                expected_foreground_package: None,
                model_activity: None,
                model_action: Some(ModelActionProposal {
                    kind: ModelActionKind::OpenApp,
                    target: Some(AgentActivityTarget::AndroidPackage {
                        package_name: "com.android.settings".to_string(),
                    }),
                    reason: "open settings".to_string(),
                    expected_observation: None,
                    proposal_id: None,
                }),
            })
            .expect("accept first action");
        store
            .transition_state("task-1", begin_current_action_execution)
            .expect("begin first action");

        let error = loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![android_foreground("com.android.settings")],
                expected_foreground_package: Some("com.android.settings".to_string()),
                model_activity: None,
                model_action: Some(ModelActionProposal {
                    kind: ModelActionKind::OpenApp,
                    target: Some(AgentActivityTarget::AndroidPackage {
                        package_name: "com.google.android.apps.nexuslauncher".to_string(),
                    }),
                    reason: "replace with launcher".to_string(),
                    expected_observation: None,
                    proposal_id: None,
                }),
            })
            .expect_err("open action must not be overwritten before evidence reduces");

        assert!(matches!(
            error,
            AgentLoopError::Transition(TaskTransitionError::InvalidAction { .. })
        ));
    }

    #[test]
    fn terminal_completed_task_marks_existing_action_verified() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task("task-1", "already done"))
            .expect("create task");
        loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![],
                expected_foreground_package: None,
                model_activity: None,
                model_action: Some(ModelActionProposal {
                    kind: ModelActionKind::Observe,
                    target: None,
                    reason: "observe completion".to_string(),
                    expected_observation: None,
                    proposal_id: None,
                }),
            })
            .expect("accept action");
        store
            .transition_state("task-1", |mut state| {
                state.phase = TaskPhase::Completed;
                Ok::<_, TaskStoreError>(state)
            })
            .expect("complete task");

        let result = loop_runner
            .step(LoopStepRequest::new("task-1"))
            .expect("step terminal task");

        let action = result
            .task
            .state
            .current_action
            .as_ref()
            .expect("current action");
        assert_eq!(action.status, AgentActionStatus::Verified);
        assert_eq!(action.boundary.state, ActionBoundaryState::Verified);
    }

    #[test]
    fn terminal_failed_task_marks_existing_action_failed() {
        let (store, loop_runner) = test_loop();
        store
            .create(TaskState::new_background_task("task-1", "failed task"))
            .expect("create task");
        loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![],
                expected_foreground_package: None,
                model_activity: None,
                model_action: Some(ModelActionProposal {
                    kind: ModelActionKind::Observe,
                    target: None,
                    reason: "observe failure".to_string(),
                    expected_observation: None,
                    proposal_id: None,
                }),
            })
            .expect("accept action");
        store
            .transition_state("task-1", |mut state| {
                state.phase = TaskPhase::Failed;
                Ok::<_, TaskStoreError>(state)
            })
            .expect("fail task");

        let result = loop_runner
            .step(LoopStepRequest::new("task-1"))
            .expect("step terminal task");

        let action = result
            .task
            .state
            .current_action
            .as_ref()
            .expect("current action");
        assert_eq!(action.status, AgentActionStatus::Failed);
        assert_eq!(action.boundary.state, ActionBoundaryState::Aborted);
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
                model_action: None,
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
                model_action: None,
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
                model_action: None,
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
                model_action: None,
            })
            .expect("block task");

        let result = loop_runner
            .step(LoopStepRequest {
                task_id: "task-1".to_string(),
                observations: vec![android_foreground("com.android.settings")],
                expected_foreground_package: Some("com.android.settings".to_string()),
                model_activity: None,
                model_action: None,
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
                model_action: None,
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
                model_action: None,
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
                model_action: None,
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
                model_action: None,
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
                model_action: None,
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
                model_action: None,
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
                model_action: None,
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
