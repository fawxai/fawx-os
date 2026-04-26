use std::env;
use std::error::Error;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use fawx_agent_loop::{AgentLoop, LoopStepRequest};
use fawx_android_adapter::{
    AndroidEvent, AndroidForegroundUnavailableReason, AndroidObservation, AndroidSubstrate,
    foreground_observation,
};
use fawx_harness::{
    ForegroundPolicyDecision, ForegroundUnavailableReason, ModelActivityKind,
    ModelActivityProposal, RuntimeEvent, RuntimeObservation, RuntimeObservationSource, TaskState,
    TaskTransitionError, apply_foreground_policy, record_action_checkpoint,
    require_foreground_attention,
};
use fawx_kernel::{ActionBoundary, ActionBoundaryState, AgentActivityTarget};
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
        "agent-step" => {
            let task_id = required_arg(&args, 1, "task id")?;
            let options = AgentStepOptions::parse(&args[2..])?;
            run_agent_step(&store, task_id, options)?;
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
}

impl AgentStepOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut expected_foreground_package = None;
        let mut sample_foreground = false;
        let mut activity_kind = None;
        let mut activity_description = None;
        let mut activity_target = None;
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

        Ok(Self {
            expected_foreground_package,
            sample_foreground,
            model_activity,
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
    })?;

    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
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
    eprintln!(
        "  fawx-terminal-runner agent-step <task-id> [--expected-foreground <package>] [--sample-foreground]"
    );
    eprintln!("    [--activity-kind observing|planning|executing|verifying|summarizing]");
    eprintln!("    [--activity-description <description>] [--activity-target <target>]");
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
