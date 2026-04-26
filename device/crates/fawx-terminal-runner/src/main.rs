use std::env;
use std::error::Error;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use fawx_agent_loop::{
    AgentLoop, BackgroundObservation, BackgroundRunner, BackgroundTickRequest, LoopStepRequest,
};
use fawx_android_adapter::{
    AndroidEvent, AndroidForegroundUnavailableReason, AndroidObservation, AndroidSubstrate,
    foreground_observation,
};
use fawx_harness::{
    ForegroundPolicyDecision, ForegroundUnavailableReason, ModelActionKind, ModelActionProposal,
    ModelActivityKind, ModelActivityProposal, RuntimeEvent, RuntimeObservation,
    RuntimeObservationSource, TaskState, TaskTransitionError, apply_foreground_policy,
    begin_current_action_execution, record_action_checkpoint, require_foreground_attention,
};
use fawx_kernel::{
    ActionBoundary, ActionBoundaryState, AgentActionStatus, AgentActivityTarget, SafetyCapability,
    SafetyGrant, SafetyScope,
};
use fawx_task_store::{StoredTask, TaskStore, default_task_store_path};

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), Box<dyn Error>> {
    let Some(command) = args.first().map(String::as_str) else {
        print_usage();
        return Ok(());
    };

    let store = TaskStore::new(default_task_store_path());

    match command {
        "create" => {
            let task_id = required_arg(&args, 1, "task id")?;
            let objective = joined_tail(&args, 2, "objective")?;
            let task = store.create(TaskState::new_background_task(task_id, objective))?;
            print_task(&task)?;
        }
        "status" => {
            let task_id = required_arg(&args, 1, "task id")?;
            print_task(&store.load(task_id)?)?;
        }
        "list" => {
            for task in store.list()? {
                println!(
                    "{}\t{:?}\t{}",
                    task.state.task_id, task.state.phase, task.state.contract.user_intent
                );
            }
        }
        "checkpoint" => {
            let task_id = required_arg(&args, 1, "task id")?;
            let boundary = joined_tail(&args, 2, "last action boundary")?;
            let task = store.transition_state(task_id, |state| {
                manual_checkpoint_state(task_id, boundary, state)
            })?;
            print_task(&task)?;
        }
        "block-foreground" => {
            let task_id = required_arg(&args, 1, "task id")?;
            let reason = joined_tail(&args, 2, "reason")?;
            let task =
                store.transition_state(task_id, |state| foreground_block_state(reason, state))?;
            print_task(&task)?;
        }
        "grant" => {
            let task_id = required_arg(&args, 1, "task id")?;
            let capability = parse_safety_capability(required_arg(&args, 2, "capability")?)?;
            let scope = parse_safety_scope(required_arg(&args, 3, "scope")?)?;
            let task = store.transition_state(task_id, |mut state| {
                state
                    .contract
                    .safety_grants
                    .push(SafetyGrant::scoped(capability, scope));
                Ok::<_, TaskTransitionError>(state)
            })?;
            print_task(&task)?;
        }
        "agent-step" => {
            let task_id = required_arg(&args, 1, "task id")?;
            let options = AgentStepOptions::parse(&args[2..])?;
            run_agent_step(&store, task_id, options)?;
        }
        "background-tick" => {
            let options = BackgroundTickOptions::parse(&args[1..])?;
            run_background_tick(&store, options)?;
        }
        "begin-action" => {
            let task_id = required_arg(&args, 1, "task id")?;
            let task = store.transition_state(task_id, begin_current_action_execution)?;
            print_task(&task)?;
        }
        "heartbeat" => {
            let task_id = required_arg(&args, 1, "task id")?;
            let count = optional_usize(&args, 2, 5)?;
            let interval_ms = optional_u64(&args, 3, 1000)?;
            let sample_foreground = args.iter().any(|arg| arg == "--foreground");
            run_heartbeat(
                &store,
                task_id,
                count,
                Duration::from_millis(interval_ms),
                sample_foreground,
            )?;
        }
        "watch-foreground" => {
            let task_id = required_arg(&args, 1, "task id")?;
            let expected_package = required_arg(&args, 2, "expected package")?;
            let count = optional_usize(&args, 3, 5)?;
            let interval_ms = optional_u64(&args, 4, 1000)?;
            run_foreground_watch(
                &store,
                task_id,
                expected_package,
                count,
                Duration::from_millis(interval_ms),
            )?;
        }
        _ => print_usage(),
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentStepOptions {
    expected_foreground_package: Option<String>,
    sample_foreground: bool,
    model_activity: Option<ModelActivityProposal>,
    model_action: Option<ModelActionProposal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BackgroundTickOptions {
    count: usize,
    interval: Duration,
    sample_foreground: bool,
    foreground_task: Option<String>,
}

impl BackgroundTickOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut count = None;
        let mut interval_ms = None;
        let mut sample_foreground = false;
        let mut foreground_task = None;
        let mut index = 0;

        while index < args.len() {
            match args[index].as_str() {
                "--foreground" => {
                    sample_foreground = true;
                    index += 1;
                }
                "--foreground-task" => {
                    let task_id = non_flag_value(args, index, "--foreground-task")?;
                    foreground_task = Some(task_id.to_string());
                    sample_foreground = true;
                    index += 2;
                }
                value if value.starts_with("--") => {
                    return Err(format!("unknown background-tick option: {value}"));
                }
                value => {
                    if count.is_none() {
                        count = Some(
                            value
                                .parse()
                                .map_err(|error| format!("invalid integer '{value}': {error}"))?,
                        );
                    } else if interval_ms.is_none() {
                        interval_ms = Some(
                            value
                                .parse()
                                .map_err(|error| format!("invalid integer '{value}': {error}"))?,
                        );
                    } else {
                        return Err(format!("unexpected background-tick argument: {value}"));
                    }
                    index += 1;
                }
            }
        }

        Ok(Self {
            count: count.unwrap_or(1),
            interval: Duration::from_millis(interval_ms.unwrap_or(1000)),
            sample_foreground,
            foreground_task,
        })
    }
}

impl AgentStepOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut expected_foreground_package = None;
        let mut sample_foreground = false;
        let mut activity_kind = None;
        let mut activity_description = None;
        let mut activity_target = None;
        let mut action_kind = None;
        let mut action_reason = None;
        let mut action_target = None;
        let mut expected_observation = None;
        let mut index = 0;

        while index < args.len() {
            match args[index].as_str() {
                "--expected-foreground" => {
                    let package = args
                        .get(index + 1)
                        .ok_or_else(|| "missing --expected-foreground package".to_string())?;
                    if package.starts_with("--") {
                        return Err(format!(
                            "missing --expected-foreground package before option {package}"
                        ));
                    }
                    expected_foreground_package = Some(package.clone());
                    index += 2;
                }
                "--sample-foreground" => {
                    sample_foreground = true;
                    index += 1;
                }
                "--activity-kind" => {
                    let value = non_flag_value(args, index, "--activity-kind")?;
                    activity_kind = Some(parse_activity_kind(value)?);
                    index += 2;
                }
                "--activity-description" => {
                    let value = non_flag_value(args, index, "--activity-description")?;
                    activity_description = Some(value.to_string());
                    index += 2;
                }
                "--activity-target" => {
                    let value = non_flag_value(args, index, "--activity-target")?;
                    activity_target = Some(parse_activity_target(value)?);
                    index += 2;
                }
                "--action-kind" => {
                    let value = non_flag_value(args, index, "--action-kind")?;
                    action_kind = Some(parse_action_kind(value)?);
                    index += 2;
                }
                "--action-reason" => {
                    let value = non_flag_value(args, index, "--action-reason")?;
                    action_reason = Some(value.to_string());
                    index += 2;
                }
                "--action-target" => {
                    let value = non_flag_value(args, index, "--action-target")?;
                    action_target = Some(parse_activity_target(value)?);
                    index += 2;
                }
                "--expected-observation" => {
                    let value = non_flag_value(args, index, "--expected-observation")?;
                    expected_observation = Some(value.to_string());
                    index += 2;
                }
                value => return Err(format!("unknown agent-step option: {value}")),
            }
        }

        let model_activity = match (activity_kind, activity_description) {
            (Some(kind), Some(description)) => Some(ModelActivityProposal {
                kind,
                target: activity_target,
                description,
            }),
            (None, None) if activity_target.is_none() => None,
            (Some(_), None) => return Err("missing --activity-description".to_string()),
            (None, Some(_)) | (None, None) => return Err("missing --activity-kind".to_string()),
        };
        let model_action = match (action_kind, action_reason) {
            (Some(kind), Some(reason)) => Some(ModelActionProposal {
                kind,
                target: action_target,
                reason,
                expected_observation,
                proposal_id: None,
            }),
            (None, None) if action_target.is_none() && expected_observation.is_none() => None,
            (Some(_), None) => return Err("missing --action-reason".to_string()),
            (None, Some(_)) | (None, None) => return Err("missing --action-kind".to_string()),
        };

        Ok(Self {
            expected_foreground_package,
            sample_foreground,
            model_activity,
            model_action,
        })
    }
}

fn run_agent_step(
    store: &TaskStore,
    task_id: &str,
    options: AgentStepOptions,
) -> Result<(), Box<dyn Error>> {
    let observations = if options.sample_foreground {
        let android_observation = foreground_observation(AndroidSubstrate::ReconRootedStock);
        vec![runtime_observation_from_android(&android_observation)]
    } else {
        vec![]
    };

    let result = AgentLoop::new(store.clone()).step(LoopStepRequest {
        task_id: task_id.to_string(),
        observations,
        expected_foreground_package: options.expected_foreground_package,
        model_activity: options.model_activity,
        model_action: options.model_action,
    })?;

    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn run_background_tick(
    store: &TaskStore,
    options: BackgroundTickOptions,
) -> Result<(), Box<dyn Error>> {
    let runner = BackgroundRunner::new(store.clone());
    for tick in 1..=options.count {
        let observations = if options.sample_foreground {
            let tasks = store.list()?;
            let android_observation = foreground_observation(AndroidSubstrate::ReconRootedStock);
            scoped_foreground_observations(
                &tasks,
                &android_observation,
                options.foreground_task.as_deref(),
            )?
        } else {
            vec![]
        };
        let result = runner.tick(BackgroundTickRequest {
            tick_id: tick as u64,
            observations,
        })?;

        println!("{}", serde_json::to_string_pretty(&result)?);

        if tick < options.count {
            thread::sleep(options.interval);
        }
    }

    Ok(())
}

fn scoped_foreground_observations(
    tasks: &[StoredTask],
    android_observation: &AndroidObservation,
    explicit_task_id: Option<&str>,
) -> Result<Vec<BackgroundObservation>, String> {
    let runtime_observation = runtime_observation_from_android(android_observation);
    if let Some(task_id) = explicit_task_id {
        if tasks.iter().any(|task| task.state.task_id == task_id) {
            return Ok(vec![BackgroundObservation::for_task(
                task_id.to_string(),
                runtime_observation,
            )]);
        }
        return Err(format!("unknown --foreground-task: {task_id}"));
    }

    let Some(observed_package) = foreground_package_from_runtime_observation(&runtime_observation)
    else {
        return Ok(vec![]);
    };
    let matching_task_ids = tasks
        .iter()
        .filter(|task| task_expects_executing_foreground_package(&task.state, observed_package))
        .map(|task| task.state.task_id.as_str())
        .collect::<Vec<_>>();

    match matching_task_ids.as_slice() {
        [] => Ok(vec![]),
        [task_id] => Ok(vec![BackgroundObservation::for_task(
            (*task_id).to_string(),
            runtime_observation,
        )]),
        task_ids => Err(format!(
            "ambiguous foreground observation for package {observed_package}; matching tasks: {}",
            task_ids.join(", ")
        )),
    }
}

fn foreground_package_from_runtime_observation(observation: &RuntimeObservation) -> Option<&str> {
    match &observation.event {
        RuntimeEvent::ForegroundAppChanged { package_name, .. } => Some(package_name),
        _ => None,
    }
}

fn task_expects_executing_foreground_package(state: &TaskState, observed_package: &str) -> bool {
    state.current_action.as_ref().is_some_and(|action| {
        action.status == AgentActionStatus::Executing
            && action.boundary.state == ActionBoundaryState::Prepared
            && matches!(
                action.target.as_ref(),
                Some(AgentActivityTarget::AndroidPackage { package_name })
                    if package_name == observed_package
            )
    })
}

fn manual_checkpoint_state(
    task_id: &str,
    boundary: String,
    state: TaskState,
) -> Result<TaskState, TaskTransitionError> {
    record_action_checkpoint(
        state,
        ActionBoundary::new(
            format!("manual-checkpoint:{task_id}"),
            ActionBoundaryState::Verified,
            boundary,
        ),
    )
}

fn foreground_block_state(
    reason: String,
    state: TaskState,
) -> Result<TaskState, TaskTransitionError> {
    require_foreground_attention(state, reason)
}

fn run_foreground_watch(
    store: &TaskStore,
    task_id: &str,
    expected_package: &str,
    count: usize,
    interval: Duration,
) -> Result<(), Box<dyn Error>> {
    for tick in 1..=count {
        let android_observation = foreground_observation(AndroidSubstrate::ReconRootedStock);
        let foreground = describe_foreground_event(&android_observation.event);
        let observation = runtime_observation_from_android(&android_observation);
        let mut decision = None;
        let stored = store.transition_state(task_id, |state| {
            let (state, policy_decision) =
                apply_foreground_policy(state, &observation, expected_package)?;
            decision = Some(policy_decision);
            Ok::<_, TaskTransitionError>(state)
        })?;
        let Some(decision) = decision else {
            return Err("foreground policy transition did not return a decision".into());
        };
        let decision_label = match decision {
            ForegroundPolicyDecision::ContinueInBackground { .. } => "continue",
            ForegroundPolicyDecision::RequireForeground { .. } => "foreground-required",
        };

        println!(
            "watch-foreground\t{}\t{}\t{}\t{}\t{:?}",
            tick, stored.state.task_id, foreground, decision_label, stored.state.phase
        );

        if tick < count {
            thread::sleep(interval);
        }
    }

    Ok(())
}

fn run_heartbeat(
    store: &TaskStore,
    task_id: &str,
    count: usize,
    interval: Duration,
    sample_foreground: bool,
) -> Result<(), Box<dyn Error>> {
    for tick in 1..=count {
        let stored = store.transition_state(task_id, |state| {
            record_action_checkpoint(
                state,
                ActionBoundary::new(
                    format!("heartbeat:{tick}/{count}"),
                    ActionBoundaryState::Verified,
                    format!("heartbeat {tick}/{count}"),
                ),
            )
        })?;
        let foreground = if sample_foreground {
            Some(describe_foreground())
        } else {
            None
        };

        println!(
            "heartbeat\t{}\t{}\t{}{}",
            tick,
            stored.state.task_id,
            stored
                .state
                .checkpoint
                .as_ref()
                .map(|checkpoint| checkpoint.action_boundary.description.as_str())
                .unwrap_or("no checkpoint"),
            foreground
                .as_deref()
                .map(|value| format!("\tforeground={value}"))
                .unwrap_or_default()
        );

        if tick < count {
            thread::sleep(interval);
        }
    }

    Ok(())
}

fn describe_foreground() -> String {
    describe_foreground_event(&foreground_observation(AndroidSubstrate::ReconRootedStock).event)
}

fn describe_foreground_event(event: &AndroidEvent) -> String {
    match event {
        AndroidEvent::ForegroundAppChanged {
            package_name,
            activity_name,
        } => match activity_name.as_ref() {
            Some(activity) => format!("{package_name}/{activity}"),
            None => package_name.to_string(),
        },
        AndroidEvent::TargetSurfaceBecameUnavailable { target } => {
            format!("unavailable:{target}")
        }
        AndroidEvent::ForegroundObservationUnavailable {
            target,
            reason,
            raw_source,
        } => match raw_source {
            Some(raw_source) if !raw_source.is_empty() => {
                format!("unavailable:{target}:{reason:?}:{raw_source}")
            }
            _ => format!("unavailable:{target}:{reason:?}"),
        },
        _ => "unavailable:unexpected-event".to_string(),
    }
}

fn runtime_observation_from_android(observation: &AndroidObservation) -> RuntimeObservation {
    RuntimeObservation {
        source: RuntimeObservationSource::Android {
            substrate: format!("{:?}", observation.substrate),
        },
        event: match &observation.event {
            AndroidEvent::ForegroundAppChanged {
                package_name,
                activity_name,
            } => RuntimeEvent::ForegroundAppChanged {
                package_name: package_name.clone(),
                activity_name: activity_name.clone(),
            },
            AndroidEvent::ForegroundObservationUnavailable {
                target,
                reason,
                raw_source,
            } => RuntimeEvent::ForegroundUnavailable {
                target: target.clone(),
                reason: match reason {
                    AndroidForegroundUnavailableReason::CommandFailed => {
                        ForegroundUnavailableReason::CommandFailed
                    }
                    AndroidForegroundUnavailableReason::EmptyOutput => {
                        ForegroundUnavailableReason::EmptyOutput
                    }
                    AndroidForegroundUnavailableReason::ParseFailed => {
                        ForegroundUnavailableReason::ParseFailed
                    }
                },
                raw_source: raw_source.clone(),
            },
            AndroidEvent::TargetSurfaceBecameUnavailable { target } => {
                RuntimeEvent::ForegroundUnavailable {
                    target: target.clone(),
                    reason: ForegroundUnavailableReason::Unsupported,
                    raw_source: None,
                }
            }
            AndroidEvent::NotificationReceived { source, summary } => {
                RuntimeEvent::NotificationReceived {
                    source: source.clone(),
                    summary: summary.clone(),
                }
            }
            AndroidEvent::NetworkAvailabilityChanged { available } => {
                RuntimeEvent::NetworkAvailabilityChanged {
                    available: *available,
                }
            }
            AndroidEvent::DeviceLockStateChanged { locked } => {
                RuntimeEvent::DeviceLockStateChanged { locked: *locked }
            }
            AndroidEvent::RootedActionFailed { action, reason } => {
                RuntimeEvent::RuntimeActionFailed {
                    action: action.clone(),
                    reason: reason.clone(),
                }
            }
        },
    }
}

fn print_task(task: &StoredTask) -> Result<(), Box<dyn Error>> {
    println!("{}", serde_json::to_string_pretty(task)?);
    Ok(())
}

fn required_arg<'a>(args: &'a [String], index: usize, name: &str) -> Result<&'a str, String> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| format!("missing {name}"))
}

fn joined_tail(args: &[String], start: usize, name: &str) -> Result<String, String> {
    if args.get(start).is_none() {
        return Err(format!("missing {name}"));
    }
    Ok(args[start..].join(" "))
}

fn optional_usize(args: &[String], index: usize, default: usize) -> Result<usize, String> {
    args.get(index)
        .map(|value| {
            value
                .parse()
                .map_err(|error| format!("invalid integer '{value}': {error}"))
        })
        .unwrap_or(Ok(default))
}

fn optional_u64(args: &[String], index: usize, default: u64) -> Result<u64, String> {
    args.get(index)
        .map(|value| {
            value
                .parse()
                .map_err(|error| format!("invalid integer '{value}': {error}"))
        })
        .unwrap_or(Ok(default))
}

fn non_flag_value<'a>(args: &'a [String], index: usize, option: &str) -> Result<&'a str, String> {
    let value = args
        .get(index + 1)
        .ok_or_else(|| format!("missing {option} value"))?;
    if value.starts_with("--") {
        return Err(format!("missing {option} value before option {value}"));
    }
    Ok(value)
}

fn parse_activity_kind(value: &str) -> Result<ModelActivityKind, String> {
    match value {
        "observing" => Ok(ModelActivityKind::Observing),
        "planning" => Ok(ModelActivityKind::Planning),
        "executing" => Ok(ModelActivityKind::Executing),
        "verifying" => Ok(ModelActivityKind::Verifying),
        "summarizing" => Ok(ModelActivityKind::Summarizing),
        "waiting" => Err("activity kind 'waiting' is reserved for typed blockers".to_string()),
        _ => Err(format!("unknown activity kind: {value}")),
    }
}

fn parse_action_kind(value: &str) -> Result<ModelActionKind, String> {
    match value {
        "observe" => Ok(ModelActionKind::Observe),
        "navigate" => Ok(ModelActionKind::Navigate),
        "open-app" => Ok(ModelActionKind::OpenApp),
        "interact" => Ok(ModelActionKind::Interact),
        "read" => Ok(ModelActionKind::Read),
        "write" => Ok(ModelActionKind::Write),
        "communicate" => Ok(ModelActionKind::Communicate),
        "execute" => Ok(ModelActionKind::Execute),
        "verify" => Ok(ModelActionKind::Verify),
        _ => Err(format!("unknown action kind: {value}")),
    }
}

fn parse_safety_capability(value: &str) -> Result<SafetyCapability, String> {
    match value {
        "app-control" => Ok(SafetyCapability::AppControl),
        "calling" => Ok(SafetyCapability::Calling),
        "messaging" => Ok(SafetyCapability::Messaging),
        "filesystem-read" => Ok(SafetyCapability::FilesystemRead),
        "filesystem-write" => Ok(SafetyCapability::FilesystemWrite),
        "network" => Ok(SafetyCapability::Network),
        "notifications-read" => Ok(SafetyCapability::NotificationsRead),
        "notifications-post" => Ok(SafetyCapability::NotificationsPost),
        "runtime-execution" => Ok(SafetyCapability::RuntimeExecution),
        _ => Err(format!("unknown safety capability: {value}")),
    }
}

fn parse_safety_scope(value: &str) -> Result<SafetyScope, String> {
    if value == "any" {
        return Ok(SafetyScope::Any);
    }
    if value == "network" {
        return Ok(SafetyScope::Network);
    }
    if value == "notifications" {
        return Ok(SafetyScope::NotificationSurface);
    }
    match parse_activity_target(value)? {
        AgentActivityTarget::AndroidPackage { package_name } => {
            Ok(SafetyScope::AndroidPackage { package_name })
        }
        AgentActivityTarget::Url { url } => Ok(SafetyScope::Url { url }),
        AgentActivityTarget::File { path } => Ok(SafetyScope::File { path }),
        AgentActivityTarget::Service { name } => Ok(SafetyScope::Service { name }),
        AgentActivityTarget::Contact { label } => Ok(SafetyScope::Contact { label }),
        AgentActivityTarget::RuntimeAction { name } => Ok(SafetyScope::RuntimeAction { name }),
        AgentActivityTarget::Network => Ok(SafetyScope::Network),
        AgentActivityTarget::Task => Ok(SafetyScope::Task),
        AgentActivityTarget::Unknown => Err(format!("safety scope cannot use target: {value}")),
    }
}

fn parse_activity_target(value: &str) -> Result<AgentActivityTarget, String> {
    if value == "task" {
        return Ok(AgentActivityTarget::Task);
    }
    if value == "network" {
        return Ok(AgentActivityTarget::Network);
    }
    if value == "unknown" {
        return Ok(AgentActivityTarget::Unknown);
    }
    let Some((kind, payload)) = value.split_once(':') else {
        return Err(format!("invalid activity target: {value}"));
    };
    if payload.is_empty() {
        return Err(format!("empty activity target payload: {value}"));
    }
    match kind {
        "android-package" => Ok(AgentActivityTarget::AndroidPackage {
            package_name: payload.to_string(),
        }),
        "url" => Ok(AgentActivityTarget::Url {
            url: payload.to_string(),
        }),
        "file" => Ok(AgentActivityTarget::File {
            path: payload.to_string(),
        }),
        "service" => Ok(AgentActivityTarget::Service {
            name: payload.to_string(),
        }),
        "contact" => Ok(AgentActivityTarget::Contact {
            label: payload.to_string(),
        }),
        "runtime-action" => Ok(AgentActivityTarget::RuntimeAction {
            name: payload.to_string(),
        }),
        _ => Err(format!("unknown activity target kind: {kind}")),
    }
}

fn print_usage() {
    eprintln!("usage:");
    eprintln!("  fawx-terminal-runner create <task-id> <objective>");
    eprintln!("  fawx-terminal-runner status <task-id>");
    eprintln!("  fawx-terminal-runner list");
    eprintln!("  fawx-terminal-runner checkpoint <task-id> <last-action-boundary>");
    eprintln!("  fawx-terminal-runner block-foreground <task-id> <reason>");
    eprintln!("  fawx-terminal-runner grant <task-id> <capability> <scope>");
    eprintln!(
        "    capabilities: app-control|calling|messaging|filesystem-read|filesystem-write|network|notifications-read|notifications-post|runtime-execution"
    );
    eprintln!(
        "    scopes: any|network|notifications|android-package:<id>|url:<url>|file:<path>|service:<name>|contact:<label>|runtime-action:<name>"
    );
    eprintln!(
        "  fawx-terminal-runner agent-step <task-id> [--expected-foreground <package>] [--sample-foreground]"
    );
    eprintln!("    [--activity-kind observing|planning|executing|verifying|summarizing]");
    eprintln!("    [--activity-description <description>] [--activity-target <target>]");
    eprintln!(
        "    [--action-kind observe|navigate|open-app|interact|read|write|communicate|execute|verify]"
    );
    eprintln!("    [--action-reason <reason>] [--action-target <target>]");
    eprintln!("    [--expected-observation <observation>]");
    eprintln!(
        "  fawx-terminal-runner background-tick [count] [interval-ms] [--foreground] [--foreground-task <task-id>]"
    );
    eprintln!("  fawx-terminal-runner begin-action <task-id>");
    eprintln!("  fawx-terminal-runner heartbeat <task-id> [count] [interval-ms] [--foreground]");
    eprintln!(
        "  fawx-terminal-runner watch-foreground <task-id> <expected-package> [count] [interval-ms]"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use fawx_harness::TaskPhase;
    use fawx_kernel::TaskBlocker;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_store() -> TaskStore {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        TaskStore::new(std::env::temp_dir().join(format!(
            "fawx-terminal-runner-test-{}-{id}",
            std::process::id()
        )))
    }

    fn foreground(package_name: &str) -> AndroidObservation {
        AndroidObservation {
            substrate: AndroidSubstrate::ReconRootedStock,
            event: AndroidEvent::ForegroundAppChanged {
                package_name: package_name.to_string(),
                activity_name: Some(".ExampleActivity".to_string()),
            },
        }
    }

    fn create_open_app_action(
        store: &TaskStore,
        task_id: &str,
        package_name: &str,
        begin_execution: bool,
    ) {
        let mut state = TaskState::new_background_task(task_id, "open app");
        state.contract.safety_grants.push(SafetyGrant::scoped(
            SafetyCapability::AppControl,
            SafetyScope::AndroidPackage {
                package_name: package_name.to_string(),
            },
        ));
        store.create(state).expect("create task");
        AgentLoop::new(store.clone())
            .step(LoopStepRequest {
                task_id: task_id.to_string(),
                observations: vec![],
                expected_foreground_package: None,
                model_activity: None,
                model_action: Some(ModelActionProposal {
                    kind: ModelActionKind::OpenApp,
                    target: Some(AgentActivityTarget::AndroidPackage {
                        package_name: package_name.to_string(),
                    }),
                    reason: format!("open {package_name}"),
                    expected_observation: None,
                    proposal_id: None,
                }),
            })
            .expect("accept action");
        if begin_execution {
            store
                .transition_state(task_id, begin_current_action_execution)
                .expect("begin action");
        }
    }

    #[test]
    fn manual_checkpoint_uses_harness_blocker_guard() {
        let mut state = TaskState::new_background_task("task-1", "watch settings");
        state.blocker = Some(TaskBlocker::WaitingForForeground {
            reason: "needs foreground".to_string(),
        });

        let error = manual_checkpoint_state("task-1", "side effect".to_string(), state)
            .expect_err("blocked checkpoint should be rejected");

        assert!(matches!(error, TaskTransitionError::BlockedTask { .. }));
    }

    #[test]
    fn foreground_block_uses_harness_terminal_guard() {
        let mut state = TaskState::new_background_task("task-1", "watch settings");
        state.phase = TaskPhase::Completed;

        let error = foreground_block_state("needs foreground".to_string(), state)
            .expect_err("terminal foreground block should be rejected");

        assert!(matches!(error, TaskTransitionError::TerminalTask { .. }));
    }

    #[test]
    fn agent_step_options_parse_expected_foreground_and_sampling() {
        let options = AgentStepOptions::parse(&[
            "--expected-foreground".to_string(),
            "com.android.settings".to_string(),
            "--sample-foreground".to_string(),
        ])
        .expect("parse agent step options");

        assert_eq!(
            options.expected_foreground_package,
            Some("com.android.settings".to_string())
        );
        assert!(options.sample_foreground);
        assert!(options.model_activity.is_none());
        assert!(options.model_action.is_none());
    }

    #[test]
    fn background_tick_options_parse_foreground_task_owner() {
        let options = BackgroundTickOptions::parse(&[
            "--foreground".to_string(),
            "--foreground-task".to_string(),
            "task-1".to_string(),
            "2".to_string(),
            "50".to_string(),
        ])
        .expect("parse background tick options");

        assert_eq!(options.count, 2);
        assert_eq!(options.interval, Duration::from_millis(50));
        assert!(options.sample_foreground);
        assert_eq!(options.foreground_task.as_deref(), Some("task-1"));
    }

    #[test]
    fn foreground_scoping_routes_to_single_executing_matching_action() {
        let store = test_store();
        create_open_app_action(
            &store,
            "task-executing-launcher",
            "com.google.android.apps.nexuslauncher",
            true,
        );
        create_open_app_action(
            &store,
            "task-accepted-launcher",
            "com.google.android.apps.nexuslauncher",
            false,
        );

        let observations = scoped_foreground_observations(
            &store.list().expect("list tasks"),
            &foreground("com.google.android.apps.nexuslauncher"),
            None,
        )
        .expect("scope foreground");

        assert_eq!(observations.len(), 1);
        assert!(matches!(
            observations[0].scope,
            fawx_agent_loop::BackgroundObservationScope::Task { ref task_id }
                if task_id == "task-executing-launcher"
        ));
    }

    #[test]
    fn foreground_scoping_rejects_ambiguous_executing_matching_actions() {
        let store = test_store();
        create_open_app_action(
            &store,
            "task-one",
            "com.google.android.apps.nexuslauncher",
            true,
        );
        create_open_app_action(
            &store,
            "task-two",
            "com.google.android.apps.nexuslauncher",
            true,
        );

        let error = scoped_foreground_observations(
            &store.list().expect("list tasks"),
            &foreground("com.google.android.apps.nexuslauncher"),
            None,
        )
        .expect_err("ambiguous foreground ownership should fail");

        assert!(error.contains("ambiguous foreground observation"));
        assert!(error.contains("task-one"));
        assert!(error.contains("task-two"));
    }

    #[test]
    fn agent_step_options_parse_model_activity_contract() {
        let options = AgentStepOptions::parse(&[
            "--activity-kind".to_string(),
            "observing".to_string(),
            "--activity-description".to_string(),
            "checking settings state".to_string(),
            "--activity-target".to_string(),
            "android-package:com.android.settings".to_string(),
        ])
        .expect("parse model activity");

        let activity = options.model_activity.expect("model activity");
        assert_eq!(activity.kind, ModelActivityKind::Observing);
        assert_eq!(activity.description, "checking settings state");
        assert!(matches!(
            activity.target,
            Some(AgentActivityTarget::AndroidPackage { package_name })
                if package_name == "com.android.settings"
        ));
        assert!(options.model_action.is_none());
    }

    #[test]
    fn agent_step_options_parse_model_action_contract() {
        let options = AgentStepOptions::parse(&[
            "--action-kind".to_string(),
            "open-app".to_string(),
            "--action-reason".to_string(),
            "open settings to inspect permissions".to_string(),
            "--action-target".to_string(),
            "android-package:com.android.settings".to_string(),
            "--expected-observation".to_string(),
            "settings is foreground".to_string(),
        ])
        .expect("parse model action");

        let action = options.model_action.expect("model action");
        assert_eq!(action.kind, ModelActionKind::OpenApp);
        assert_eq!(action.reason, "open settings to inspect permissions");
        assert_eq!(
            action.expected_observation.as_deref(),
            Some("settings is foreground")
        );
        assert!(action.proposal_id.is_none());
        assert!(matches!(
            action.target,
            Some(AgentActivityTarget::AndroidPackage { package_name })
                if package_name == "com.android.settings"
        ));
    }

    #[test]
    fn safety_grant_parser_accepts_typed_package_scope() {
        assert_eq!(
            parse_safety_capability("app-control").expect("parse capability"),
            SafetyCapability::AppControl
        );
        assert_eq!(
            parse_safety_scope("android-package:com.android.settings").expect("parse scope"),
            SafetyScope::AndroidPackage {
                package_name: "com.android.settings".to_string()
            }
        );
    }

    #[test]
    fn safety_grant_parser_accepts_task_scope() {
        assert_eq!(
            parse_safety_scope("task").expect("task is a runtime safety scope"),
            SafetyScope::Task
        );
    }

    #[test]
    fn safety_grant_parser_rejects_unknown_scope() {
        let error = parse_safety_scope("unknown").expect_err("unknown is not a safety scope");

        assert!(error.contains("safety scope cannot use target"));
    }

    #[test]
    fn agent_step_options_reject_activity_without_kind() {
        let error = AgentStepOptions::parse(&[
            "--activity-description".to_string(),
            "checking settings state".to_string(),
        ])
        .expect_err("description without kind should fail");

        assert!(error.contains("missing --activity-kind"));
    }

    #[test]
    fn agent_step_options_reject_waiting_activity_kind() {
        let error = AgentStepOptions::parse(&[
            "--activity-kind".to_string(),
            "waiting".to_string(),
            "--activity-description".to_string(),
            "waiting maybe".to_string(),
        ])
        .expect_err("waiting kind should fail");

        assert!(error.contains("reserved for typed blockers"));
    }

    #[test]
    fn agent_step_options_reject_action_without_kind() {
        let error =
            AgentStepOptions::parse(&["--action-reason".to_string(), "open settings".to_string()])
                .expect_err("action reason without kind should fail");

        assert!(error.contains("missing --action-kind"));
    }

    #[test]
    fn agent_step_options_reject_unknown_action_kind() {
        let error = AgentStepOptions::parse(&[
            "--action-kind".to_string(),
            "mystery".to_string(),
            "--action-reason".to_string(),
            "do something".to_string(),
        ])
        .expect_err("unknown action kind should fail");

        assert!(error.contains("unknown action kind"));
    }

    #[test]
    fn agent_step_options_reject_unknown_options() {
        let error = AgentStepOptions::parse(&["--surprise".to_string()])
            .expect_err("unknown option should fail");

        assert!(error.contains("--surprise"));
    }

    #[test]
    fn agent_step_options_reject_flag_as_expected_foreground_package() {
        let error = AgentStepOptions::parse(&[
            "--expected-foreground".to_string(),
            "--sample-foreground".to_string(),
        ])
        .expect_err("flag should not parse as package");

        assert!(error.contains("missing --expected-foreground package"));
    }
}
