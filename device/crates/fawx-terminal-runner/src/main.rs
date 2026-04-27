use std::env;
use std::error::Error;
use std::io::{self, Write};
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fawx_agent_loop::{
    AgentLoop, BackgroundObservation, BackgroundRunner, BackgroundTickRequest, LoopStepRequest,
};
use fawx_android_adapter::{
    AndroidActionRequest, AndroidAppLaunchUnavailableReason,
    AndroidBackgroundSupervisorUnavailableReason, AndroidCommand, AndroidEvent,
    AndroidForegroundUnavailableReason, AndroidNotificationUnavailableReason, AndroidObservation,
    AndroidObservationProvenance, AndroidSubstrate, CommandOutput, LocalModelProbeReport,
    execute_android_action_request, foreground_observation, local_model_probe,
};
use fawx_harness::{
    AppLaunchUnavailableReason, BackgroundSupervisorUnavailableReason, CandidateAcceptanceDecision,
    ForegroundPolicyDecision, ForegroundUnavailableReason, IntentCandidate,
    IntentCandidateAuthority, LocalModelLocality, LocalModelProviderRef, ModelActionKind,
    ModelActionProposal, ModelActivityKind, ModelActivityProposal, NotificationUnavailableReason,
    RuntimeEvent, RuntimeObservation, RuntimeObservationSource, RuntimePlatformEventSource,
    TaskState, TaskTransitionError, apply_foreground_policy,
    apply_owner_command_grants_for_intent_candidate, begin_current_action_execution,
    evaluate_intent_candidate_acceptance, fail_current_action_execution, record_action_checkpoint,
    require_foreground_attention, require_owner_approval_for_intent_candidate,
    satisfy_human_handoff,
};
use fawx_kernel::{
    ActionBoundary, ActionBoundaryState, AgentActionStatus, AgentActivityTarget,
    HumanHandoffResumeCondition, SafetyCapability, SafetyGrant, SafetyScope, TaskBlocker,
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
        "complete-handoff" => {
            let task_id = required_arg(&args, 1, "task id")?;
            let handoff_id = required_arg(&args, 2, "handoff id")?.to_string();
            let summary = joined_tail(&args, 3, "handoff summary")?;
            let task = store.transition_state(task_id, |state| {
                satisfy_human_handoff(state, handoff_completion_observation(handoff_id, summary))
            })?;
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
        "execute-action" => {
            let task_id = required_arg(&args, 1, "task id")?;
            run_execute_action(&store, task_id)?;
        }
        "local-model-probe" => {
            run_local_model_probe()?;
        }
        "candidate-dry-run" => {
            let prompt = joined_tail(&args, 1, "prompt")?;
            run_candidate_dry_run(&prompt)?;
        }
        "session" => {
            run_terminal_session(&store)?;
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum TerminalSessionCommand {
    OpenApp {
        label: String,
        package_name: String,
        candidate: Box<IntentCandidate>,
    },
    ApprovePendingIntent {
        task_id: Option<String>,
    },
    Help,
    List,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalSessionIntentSource {
    /// Direct owner input parsed by the session shell. This may mint scoped
    /// grants for the exact command target because the user is the authority.
    OwnerCommand,
    /// Candidate produced by a model/provider. This may propose actions, but it
    /// must not mint grants or impersonate owner authority.
    ModelCandidate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TerminalSessionIntent {
    source: TerminalSessionIntentSource,
    command: TerminalSessionCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeterministicSessionInterpreter;

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

fn run_terminal_session(store: &TaskStore) -> Result<(), Box<dyn Error>> {
    println!("Fawx OS terminal session");
    println!(
        "Local model: not connected yet. This session uses deterministic typed intent parsing."
    );
    println!(
        "Try: open settings, suggest open settings, approve last, open launcher, list, help, quit"
    );

    let interpreter = DeterministicSessionInterpreter;
    let mut turn_index = 0_u64;
    let mut last_pending_approval_task_id: Option<String> = None;
    loop {
        print!("fawx› ");
        io::stdout().flush()?;

        let mut input = String::new();
        if io::stdin().read_line(&mut input)? == 0 {
            println!();
            return Ok(());
        }
        if input.trim().is_empty() {
            continue;
        }

        match interpreter.interpret(&input, turn_index.saturating_add(1)) {
            Ok(TerminalSessionIntent {
                source,
                command:
                    TerminalSessionCommand::OpenApp {
                        label,
                        package_name: _,
                        candidate,
                    },
            }) => {
                turn_index += 1;
                match run_session_open_app(store, turn_index, source, &label, *candidate) {
                    Ok(SessionOpenAppOutcome::Completed) => {}
                    Ok(SessionOpenAppOutcome::PendingApproval { task_id }) => {
                        last_pending_approval_task_id = Some(task_id);
                    }
                    Err(error) => println!("blocked: {error}"),
                }
            }
            Ok(TerminalSessionIntent {
                source,
                command: TerminalSessionCommand::ApprovePendingIntent { task_id },
            }) => {
                if source != TerminalSessionIntentSource::OwnerCommand {
                    println!("blocked: approval commands require direct owner input");
                    continue;
                }
                let Some(task_id) = task_id.or_else(|| last_pending_approval_task_id.clone())
                else {
                    println!(
                        "blocked: no pending approval task; try `suggest open settings` first"
                    );
                    continue;
                };
                turn_index += 1;
                if let Err(error) = run_session_approve_pending_intent(store, &task_id, turn_index)
                {
                    println!("blocked: {error}");
                } else if last_pending_approval_task_id.as_deref() == Some(task_id.as_str()) {
                    last_pending_approval_task_id = None;
                }
            }
            Ok(TerminalSessionIntent {
                command: TerminalSessionCommand::Help,
                ..
            }) => print_terminal_session_help(),
            Ok(TerminalSessionIntent {
                command: TerminalSessionCommand::List,
                ..
            }) => print_session_task_list(store)?,
            Ok(TerminalSessionIntent {
                command: TerminalSessionCommand::Quit,
                ..
            }) => return Ok(()),
            Err(error) => {
                println!("{error}");
                print_terminal_session_help();
            }
        }
    }
}

fn run_session_open_app(
    store: &TaskStore,
    turn_index: u64,
    source: TerminalSessionIntentSource,
    label: &str,
    candidate: IntentCandidate,
) -> Result<SessionOpenAppOutcome, Box<dyn Error>> {
    let task_id = session_task_id(turn_index)?;
    let objective = format!("open {label}");
    store.create(TaskState::new_background_task(&task_id, objective))?;
    if source == TerminalSessionIntentSource::OwnerCommand {
        store.transition_state(&task_id, |state| {
            apply_owner_command_grants_for_intent_candidate(state, &candidate)
        })?;
    }

    let task = store.load(&task_id)?;
    match evaluate_intent_candidate_acceptance(
        &task.state,
        &candidate,
        terminal_session_authority(source),
    )? {
        CandidateAcceptanceDecision::Accepted => {}
        CandidateAcceptanceDecision::NeedsOwnerApproval {
            reason,
            missing_requirements,
        } => {
            let task = store.transition_state(&task_id, |state| {
                require_owner_approval_for_intent_candidate(
                    state,
                    &candidate,
                    reason.clone(),
                    missing_requirements.clone(),
                )
            })?;
            println!(
                "needs confirmation: {}",
                task.state
                    .current_handoff
                    .as_ref()
                    .map(|handoff| handoff.reason.as_str())
                    .unwrap_or("owner approval required")
            );
            println!("approve with: approve {task_id}");
            return Ok(SessionOpenAppOutcome::PendingApproval { task_id });
        }
    }

    let (model_activity, model_action) = candidate.into_loop_proposals();
    let accepted = AgentLoop::new(store.clone()).step(LoopStepRequest {
        task_id: task_id.clone(),
        observations: vec![],
        expected_foreground_package: None,
        model_activity,
        model_action,
    })?;

    println!(
        "accepted: {}",
        describe_current_action_status(&accepted.task.state)
    );
    let execution = execute_action(store, &task_id)?;
    println!(
        "executed: {}",
        if execution.execution.success {
            "runtime launch command succeeded"
        } else {
            "runtime launch command failed"
        }
    );

    let task = poll_session_foreground_until_closed(
        store,
        &task_id,
        turn_index,
        10,
        Duration::from_millis(250),
        || foreground_observation(AndroidSubstrate::ReconRootedStock),
        thread::sleep,
    )?;

    match task
        .state
        .current_action
        .as_ref()
        .map(|action| action.status)
    {
        Some(AgentActionStatus::Observed) => {
            println!("done: {label} is foreground");
        }
        Some(status) => {
            println!("waiting: action is {status:?}; foreground did not settle before timeout");
        }
        None => {
            println!("waiting: task has no current action after execution");
        }
    }
    Ok(SessionOpenAppOutcome::Completed)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionOpenAppOutcome {
    Completed,
    PendingApproval { task_id: String },
}

fn run_session_approve_pending_intent(
    store: &TaskStore,
    task_id: &str,
    turn_index: u64,
) -> Result<(), Box<dyn Error>> {
    let accepted =
        accept_pending_intent_approval(store, task_id, "approved from terminal session")?;
    println!(
        "accepted: {}",
        describe_current_action_status(&accepted.state)
    );
    let label = current_action_label(&accepted.state);
    let execution = execute_action(store, task_id)?;
    println!(
        "executed: {}",
        if execution.execution.success {
            "runtime launch command succeeded"
        } else {
            "runtime launch command failed"
        }
    );

    let task = poll_session_foreground_until_closed(
        store,
        task_id,
        turn_index,
        10,
        Duration::from_millis(250),
        || foreground_observation(AndroidSubstrate::ReconRootedStock),
        thread::sleep,
    )?;

    match task
        .state
        .current_action
        .as_ref()
        .map(|action| action.status)
    {
        Some(AgentActionStatus::Observed) => println!("done: {label} is foreground"),
        Some(status) => {
            println!("waiting: action is {status:?}; foreground did not settle before timeout");
        }
        None => println!("waiting: task has no current action after approval"),
    }
    Ok(())
}

fn accept_pending_intent_approval(
    store: &TaskStore,
    task_id: &str,
    summary: &str,
) -> Result<StoredTask, Box<dyn Error>> {
    let task = store.load(task_id)?;
    let Some(handoff_id) = task
        .state
        .current_handoff
        .as_ref()
        .map(|handoff| handoff.id.clone())
    else {
        return Err(format!("task {task_id} has no active handoff").into());
    };
    if task.state.pending_intent_approval.is_none() {
        return Err(format!("task {task_id} has no pending intent approval").into());
    }

    Ok(AgentLoop::new(store.clone())
        .step(LoopStepRequest {
            task_id: task_id.to_string(),
            observations: vec![handoff_completion_observation(
                handoff_id,
                summary.to_string(),
            )],
            expected_foreground_package: None,
            model_activity: None,
            model_action: None,
        })?
        .task)
}

fn current_action_label(state: &TaskState) -> String {
    match state
        .current_action
        .as_ref()
        .and_then(|action| action.target.as_ref())
    {
        Some(AgentActivityTarget::AndroidPackage { package_name }) => package_name.clone(),
        Some(target) => format!("{target:?}"),
        None => "approved action".to_string(),
    }
}

fn terminal_session_authority(source: TerminalSessionIntentSource) -> IntentCandidateAuthority {
    match source {
        TerminalSessionIntentSource::OwnerCommand => IntentCandidateAuthority::OwnerCommand,
        TerminalSessionIntentSource::ModelCandidate => IntentCandidateAuthority::ModelCandidate,
    }
}

fn poll_session_foreground_until_closed(
    store: &TaskStore,
    task_id: &str,
    turn_index: u64,
    max_attempts: usize,
    interval: Duration,
    mut observe: impl FnMut() -> AndroidObservation,
    mut sleep: impl FnMut(Duration),
) -> Result<StoredTask, Box<dyn Error>> {
    let attempts = max_attempts.max(1);
    let expected_package = expected_session_foreground_package(&store.load(task_id)?.state);
    for attempt in 1..=attempts {
        let android_observation = observe();
        if attempt < attempts
            && let Some(expected_package) = expected_package.as_deref()
            && foreground_package_from_android_observation(&android_observation)
                .is_some_and(|observed| observed != expected_package)
        {
            sleep(interval);
            continue;
        }
        let observations =
            scoped_foreground_observations(&store.list()?, &android_observation, Some(task_id))?;
        BackgroundRunner::new(store.clone()).tick(BackgroundTickRequest {
            tick_id: turn_index + attempt as u64 - 1,
            observations,
        })?;
        let task = store.load(task_id)?;
        if session_action_is_closed(&task.state) || attempt == attempts {
            return Ok(task);
        }
        sleep(interval);
    }

    store.load(task_id).map_err(Into::into)
}

fn expected_session_foreground_package(state: &TaskState) -> Option<String> {
    match state
        .current_action
        .as_ref()
        .and_then(|action| action.target.as_ref())
    {
        Some(AgentActivityTarget::AndroidPackage { package_name }) => Some(package_name.clone()),
        _ => None,
    }
}

fn foreground_package_from_android_observation(observation: &AndroidObservation) -> Option<&str> {
    match observation.event() {
        AndroidEvent::ForegroundAppChanged { package_name, .. } => Some(package_name),
        _ => None,
    }
}

fn session_action_is_closed(state: &TaskState) -> bool {
    state.current_action.as_ref().is_some_and(|action| {
        matches!(
            action.status,
            AgentActionStatus::Observed | AgentActionStatus::Verified | AgentActionStatus::Failed
        )
    })
}

impl DeterministicSessionInterpreter {
    fn interpret(&self, input: &str, turn_index: u64) -> Result<TerminalSessionIntent, String> {
        let trimmed = input.trim();
        for prefix in ["suggest ", "model "] {
            if let Some(prompt) = trimmed.strip_prefix(prefix) {
                if matches!(
                    parse_terminal_session_command_target(prompt),
                    Ok(ParsedTerminalSessionCommand::ApprovePendingIntent { .. })
                ) {
                    return Err(
                        "model candidates cannot approve pending owner handoffs".to_string()
                    );
                }
                return Ok(TerminalSessionIntent {
                    source: TerminalSessionIntentSource::ModelCandidate,
                    command: parse_terminal_session_command_with_provider(
                        prompt,
                        turn_index,
                        LocalModelProviderRef {
                            provider_id: "terminal-model-candidate".to_string(),
                            locality: LocalModelLocality::DeviceLocal,
                        },
                    )?,
                });
            }
        }
        Ok(TerminalSessionIntent {
            source: TerminalSessionIntentSource::OwnerCommand,
            command: parse_terminal_session_command(input, turn_index)?,
        })
    }
}

fn parse_terminal_session_command(
    input: &str,
    turn_index: u64,
) -> Result<TerminalSessionCommand, String> {
    parse_terminal_session_command_with_provider(
        input,
        turn_index,
        LocalModelProviderRef {
            provider_id: "deterministic-session-parser".to_string(),
            locality: LocalModelLocality::DeterministicFallback,
        },
    )
}

fn parse_terminal_session_command_with_provider(
    input: &str,
    turn_index: u64,
    provider: LocalModelProviderRef,
) -> Result<TerminalSessionCommand, String> {
    let trimmed = input.trim();
    let parsed = parse_terminal_session_command_target(trimmed)?;
    Ok(match parsed {
        ParsedTerminalSessionCommand::OpenApp {
            label,
            package_name,
        } => open_app_command_with_candidate(
            &label,
            &package_name,
            prompt_candidate_id(turn_index),
            provider,
            trimmed,
        ),
        ParsedTerminalSessionCommand::Help => TerminalSessionCommand::Help,
        ParsedTerminalSessionCommand::List => TerminalSessionCommand::List,
        ParsedTerminalSessionCommand::ApprovePendingIntent { task_id } => {
            TerminalSessionCommand::ApprovePendingIntent { task_id }
        }
        ParsedTerminalSessionCommand::Quit => TerminalSessionCommand::Quit,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedTerminalSessionCommand {
    OpenApp { label: String, package_name: String },
    ApprovePendingIntent { task_id: Option<String> },
    Help,
    List,
    Quit,
}

fn parse_terminal_session_command_target(
    input: &str,
) -> Result<ParsedTerminalSessionCommand, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("empty prompt".to_string());
    }

    let normalized = trimmed.to_ascii_lowercase();
    match normalized.as_str() {
        "quit" | "exit" | ":q" => return Ok(ParsedTerminalSessionCommand::Quit),
        "help" | "?" => return Ok(ParsedTerminalSessionCommand::Help),
        "list" | "tasks" | "status" => return Ok(ParsedTerminalSessionCommand::List),
        "approve" | "approve last" => {
            return Ok(ParsedTerminalSessionCommand::ApprovePendingIntent { task_id: None });
        }
        "open settings" | "launch settings" => {
            return Ok(ParsedTerminalSessionCommand::OpenApp {
                label: "Settings".to_string(),
                package_name: "com.android.settings".to_string(),
            });
        }
        "open launcher" | "launch launcher" | "go home" | "home" => {
            return Ok(ParsedTerminalSessionCommand::OpenApp {
                label: "Launcher".to_string(),
                package_name: "com.google.android.apps.nexuslauncher".to_string(),
            });
        }
        _ => {}
    }

    if let Some(task_id) = trimmed.strip_prefix("approve ") {
        let task_id = task_id.trim();
        if task_id.is_empty() {
            return Err("approve requires `last` or a task id".to_string());
        }
        return Ok(ParsedTerminalSessionCommand::ApprovePendingIntent {
            task_id: Some(task_id.to_string()),
        });
    }

    for prefix in [
        "open package ",
        "launch package ",
        "open app ",
        "launch app ",
    ] {
        if normalized.starts_with(prefix) {
            let package_name = &trimmed[prefix.len()..];
            return parse_open_package_target(package_name);
        }
    }
    for prefix in ["open ", "launch "] {
        if normalized.starts_with(prefix)
            && let Some(package_name) = trimmed.get(prefix.len()..)
            && package_name.contains('.')
        {
            return parse_open_package_target(package_name);
        }
    }

    Err(format!("unsupported prompt: {trimmed}"))
}

fn prompt_candidate_id(turn_index: u64) -> String {
    format!("session-turn-{turn_index}")
}

fn parse_open_package_target(package_name: &str) -> Result<ParsedTerminalSessionCommand, String> {
    validate_android_package_target(package_name)?;
    Ok(ParsedTerminalSessionCommand::OpenApp {
        label: package_name.to_string(),
        package_name: package_name.to_string(),
    })
}

fn open_app_command_with_candidate(
    label: &str,
    package_name: &str,
    candidate_id: String,
    provider: LocalModelProviderRef,
    prompt: &str,
) -> TerminalSessionCommand {
    TerminalSessionCommand::OpenApp {
        label: label.to_string(),
        package_name: package_name.to_string(),
        candidate: Box::new(open_app_intent_candidate(
            label,
            package_name,
            candidate_id,
            provider,
            prompt,
        )),
    }
}

fn open_app_intent_candidate(
    label: &str,
    package_name: &str,
    candidate_id: String,
    provider: LocalModelProviderRef,
    prompt: &str,
) -> IntentCandidate {
    IntentCandidate {
        candidate_id,
        provider,
        prompt: prompt.to_string(),
        model_activity: Some(ModelActivityProposal {
            kind: ModelActivityKind::Planning,
            target: Some(AgentActivityTarget::AndroidPackage {
                package_name: package_name.to_string(),
            }),
            description: format!("Opening {label} through the Android runtime adapter."),
        }),
        model_action: Some(ModelActionProposal {
            kind: ModelActionKind::OpenApp,
            target: Some(AgentActivityTarget::AndroidPackage {
                package_name: package_name.to_string(),
            }),
            reason: format!("open {label}"),
            expected_observation: Some(format!("{label} is foreground")),
            proposal_id: None,
        }),
    }
}

fn print_terminal_session_help() {
    println!("supported prompts:");
    println!("  open settings");
    println!("  suggest open settings");
    println!("  approve last");
    println!("  approve <task-id>");
    println!("  open launcher");
    println!("  open package <android.package>");
    println!("  list");
    println!("  quit");
}

fn print_session_task_list(store: &TaskStore) -> Result<(), Box<dyn Error>> {
    let tasks = store.list()?;
    if tasks.is_empty() {
        println!("no tasks");
        return Ok(());
    }
    for task in tasks {
        println!(
            "{}\t{:?}\t{}",
            task.state.task_id, task.state.phase, task.state.contract.user_intent
        );
    }
    Ok(())
}

fn session_task_id(turn_index: u64) -> Result<String, Box<dyn Error>> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    Ok(format!(
        "session-{}-{}-{}",
        std::process::id(),
        now,
        turn_index
    ))
}

fn describe_current_action_status(state: &TaskState) -> String {
    match state.current_action.as_ref() {
        Some(action) => format!("{:?} {:?}", action.kind, action.status),
        None => "no current action".to_string(),
    }
}

#[derive(Debug, Clone)]
struct ActionExecutionResult {
    task: StoredTask,
    execution: CommandOutput,
}

fn run_execute_action(store: &TaskStore, task_id: &str) -> Result<(), Box<dyn Error>> {
    let result = execute_action(store, task_id)?;

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "task": result.task,
            "execution": command_output_value(&result.execution),
        }))?
    );
    if !result.execution.success {
        return Err(format!(
            "android action execution failed: {}",
            command_output_summary(&result.execution)
        )
        .into());
    }
    Ok(())
}

fn run_local_model_probe() -> Result<(), Box<dyn Error>> {
    let report = local_model_probe(AndroidSubstrate::ReconRootedStock);
    println!("{}", local_model_probe_json(&report)?);
    Ok(())
}

fn local_model_probe_json(report: &LocalModelProbeReport) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

fn run_candidate_dry_run(prompt: &str) -> Result<(), Box<dyn Error>> {
    let intent = model_candidate_intent(prompt, 1)?;
    println!("{}", candidate_dry_run_json(&intent)?);
    Ok(())
}

fn model_candidate_intent(prompt: &str, turn_index: u64) -> Result<TerminalSessionIntent, String> {
    Ok(TerminalSessionIntent {
        source: TerminalSessionIntentSource::ModelCandidate,
        command: parse_terminal_session_command_with_provider(
            prompt,
            turn_index,
            LocalModelProviderRef {
                provider_id: "dry-run-model-candidate".to_string(),
                locality: LocalModelLocality::DeviceLocal,
            },
        )?,
    })
}

fn candidate_dry_run_json(intent: &TerminalSessionIntent) -> Result<String, serde_json::Error> {
    let candidate = match &intent.command {
        TerminalSessionCommand::OpenApp { candidate, .. } => serde_json::to_value(candidate)?,
        TerminalSessionCommand::ApprovePendingIntent { .. }
        | TerminalSessionCommand::Help
        | TerminalSessionCommand::List
        | TerminalSessionCommand::Quit => serde_json::Value::Null,
    };
    let policy_decision = match &intent.command {
        TerminalSessionCommand::OpenApp { candidate, .. } => {
            let state = TaskState::new_background_task("candidate-dry-run", &candidate.prompt);
            match evaluate_intent_candidate_acceptance(
                &state,
                candidate,
                terminal_session_authority(intent.source),
            ) {
                Ok(decision) => serde_json::to_value(decision)?,
                Err(error) => serde_json::json!({
                    "Rejected": error.to_string(),
                }),
            }
        }
        TerminalSessionCommand::ApprovePendingIntent { .. }
        | TerminalSessionCommand::Help
        | TerminalSessionCommand::List
        | TerminalSessionCommand::Quit => serde_json::Value::Null,
    };
    serde_json::to_string_pretty(&serde_json::json!({
        "source": terminal_session_source_label(intent.source),
        "candidate": candidate,
        "policy_decision": policy_decision,
    }))
}

fn terminal_session_source_label(source: TerminalSessionIntentSource) -> &'static str {
    match source {
        TerminalSessionIntentSource::OwnerCommand => "OwnerCommand",
        TerminalSessionIntentSource::ModelCandidate => "ModelCandidate",
    }
}

fn execute_action(
    store: &TaskStore,
    task_id: &str,
) -> Result<ActionExecutionResult, Box<dyn Error>> {
    let current = store.load(task_id)?;
    let request = android_request_for_current_action(&current.state)?;
    let begun = store.transition_state(task_id, begin_current_action_execution)?;
    let execution = match execute_android_action_request(&request) {
        Ok(execution) => execution,
        Err(error) => {
            let failed_task =
                record_runtime_action_failure(store, task_id, &request, error.clone())?;
            return Ok(ActionExecutionResult {
                task: failed_task,
                execution: CommandOutput {
                    stdout: String::new(),
                    stderr: error,
                    status: "adapter error".to_string(),
                    success: false,
                },
            });
        }
    };
    if !execution.success {
        let failed_task = record_runtime_action_failure(
            store,
            task_id,
            &request,
            command_output_summary(&execution),
        )?;
        return Ok(ActionExecutionResult {
            task: failed_task,
            execution,
        });
    }

    Ok(ActionExecutionResult {
        task: begun,
        execution,
    })
}

fn record_runtime_action_failure(
    store: &TaskStore,
    task_id: &str,
    request: &AndroidActionRequest,
    reason: String,
) -> Result<StoredTask, Box<dyn Error>> {
    let observation = RuntimeObservation {
        source: RuntimeObservationSource::Android {
            substrate: format!("{:?}", request.substrate),
            platform_event_source: None,
        },
        event: RuntimeEvent::RuntimeActionFailed {
            action: android_command_label(&request.command).to_string(),
            reason,
        },
    };
    Ok(store.transition_state(task_id, |state| {
        fail_current_action_execution(state, observation)
    })?)
}

fn android_request_for_current_action(state: &TaskState) -> Result<AndroidActionRequest, String> {
    let Some(action) = state.current_action.as_ref() else {
        return Err(format!(
            "task {} has no current action to execute",
            state.task_id
        ));
    };
    match (&action.kind, &action.target) {
        (
            fawx_kernel::AgentActionKind::OpenApp,
            Some(AgentActivityTarget::AndroidPackage { package_name }),
        ) => {
            validate_android_package_target(package_name)?;
            Ok(AndroidActionRequest {
                substrate: AndroidSubstrate::ReconRootedStock,
                command: AndroidCommand::ResumeAppSurface {
                    package_name: package_name.clone(),
                },
            })
        }
        (kind, target) => Err(format!(
            "current action {kind:?} with target {target:?} has no rooted-stock executor"
        )),
    }
}

fn validate_android_package_target(package_name: &str) -> Result<(), String> {
    if package_name.trim().is_empty() {
        return Err("Android package target must not be empty".to_string());
    }
    if package_name.trim() != package_name {
        return Err("Android package target must not contain surrounding whitespace".to_string());
    }
    Ok(())
}

fn android_command_label(command: &AndroidCommand) -> &'static str {
    match command {
        AndroidCommand::AcquireForeground { .. } => "acquire-foreground",
        AndroidCommand::ReleaseForeground => "release-foreground",
        AndroidCommand::ObserveNotifications { .. } => "observe-notifications",
        AndroidCommand::QueryForegroundState => "query-foreground-state",
        AndroidCommand::ResumeAppSurface { .. } => "resume-app-surface",
        AndroidCommand::PerformRootedAction { .. } => "perform-rooted-action",
    }
}

fn command_output_summary(output: &CommandOutput) -> String {
    let stderr = output.stderr.trim();
    let stdout = output.stdout.trim();
    match (stderr.is_empty(), stdout.is_empty()) {
        (false, false) => format!("{}; stderr={stderr}; stdout={stdout}", output.status),
        (false, true) => format!("{}; stderr={stderr}", output.status),
        (true, false) => format!("{}; stdout={stdout}", output.status),
        (true, true) => output.status.clone(),
    }
}

fn command_output_value(output: &CommandOutput) -> serde_json::Value {
    serde_json::json!({
        "stdout": output.stdout,
        "stderr": output.stderr,
        "status": output.status,
        "success": output.success,
    })
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
        .filter(|task| task_expects_foreground_package(&task.state, observed_package))
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

fn task_expects_foreground_package(state: &TaskState, observed_package: &str) -> bool {
    task_expects_executing_foreground_package(state, observed_package)
        || task_expects_handoff_foreground_package(state, observed_package)
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

fn task_expects_handoff_foreground_package(state: &TaskState, observed_package: &str) -> bool {
    matches!(
        state.blocker,
        Some(TaskBlocker::WaitingForForeground { .. })
    ) && state.current_handoff.as_ref().is_some_and(|handoff| {
        matches!(
            &handoff.resume_condition,
            HumanHandoffResumeCondition::ForegroundPackage { package_name }
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
        let foreground = describe_foreground_event(android_observation.event());
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
    describe_foreground_event(foreground_observation(AndroidSubstrate::ReconRootedStock).event())
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
    let platform_event_source = observation.provenance().map(|provenance| match provenance {
        AndroidObservationProvenance::AospPlatformEvent { source } => RuntimePlatformEventSource {
            service_name: source.service_name.clone(),
            event_id: source.event_id.clone(),
        },
    });

    RuntimeObservation {
        source: RuntimeObservationSource::Android {
            substrate: format!("{:?}", observation.substrate()),
            platform_event_source,
        },
        event: match observation.event() {
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
                    AndroidForegroundUnavailableReason::AdapterUnavailable => {
                        ForegroundUnavailableReason::AdapterUnavailable
                    }
                },
                raw_source: raw_source.clone(),
            },
            AndroidEvent::BackgroundSupervisorHeartbeat {
                supervisor_id,
                active_tasks,
            } => RuntimeEvent::BackgroundSupervisorHeartbeat {
                supervisor_id: supervisor_id.clone(),
                active_tasks: *active_tasks,
            },
            AndroidEvent::BackgroundSupervisorUnavailable {
                target,
                reason,
                raw_source,
            } => RuntimeEvent::BackgroundSupervisorUnavailable {
                target: target.clone(),
                reason: match reason {
                    AndroidBackgroundSupervisorUnavailableReason::AdapterUnavailable => {
                        BackgroundSupervisorUnavailableReason::AdapterUnavailable
                    }
                },
                raw_source: raw_source.clone(),
            },
            AndroidEvent::AppLaunchCompleted {
                package_name,
                activity_name,
            } => RuntimeEvent::AppLaunchCompleted {
                package_name: package_name.clone(),
                activity_name: activity_name.clone(),
            },
            AndroidEvent::AppLaunchUnavailable {
                target,
                reason,
                raw_source,
            } => RuntimeEvent::AppLaunchUnavailable {
                target: target.clone(),
                reason: match reason {
                    AndroidAppLaunchUnavailableReason::AdapterUnavailable => {
                        AppLaunchUnavailableReason::AdapterUnavailable
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
            AndroidEvent::NotificationUnavailable {
                target,
                reason,
                raw_source,
            } => RuntimeEvent::NotificationUnavailable {
                target: target.clone(),
                reason: match reason {
                    AndroidNotificationUnavailableReason::AdapterUnavailable => {
                        NotificationUnavailableReason::AdapterUnavailable
                    }
                },
                raw_source: raw_source.clone(),
            },
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

fn handoff_completion_observation(handoff_id: String, summary: String) -> RuntimeObservation {
    RuntimeObservation {
        source: RuntimeObservationSource::Shell {
            name: "fawx-terminal-runner".to_string(),
        },
        event: RuntimeEvent::HumanHandoffCompleted {
            handoff_id,
            summary,
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
        AgentActivityTarget::NotificationSurface => Ok(SafetyScope::NotificationSurface),
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
    if value == "notifications" {
        return Ok(AgentActivityTarget::NotificationSurface);
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
    eprintln!("  fawx-terminal-runner complete-handoff <task-id> <handoff-id> <summary>");
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
    eprintln!("  fawx-terminal-runner execute-action <task-id>");
    eprintln!("  fawx-terminal-runner local-model-probe");
    eprintln!("  fawx-terminal-runner candidate-dry-run <prompt>");
    eprintln!("  fawx-terminal-runner session");
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
        AndroidObservation::recon_rooted_stock(AndroidEvent::ForegroundAppChanged {
            package_name: package_name.to_string(),
            activity_name: Some(".ExampleActivity".to_string()),
        })
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

    fn create_foreground_handoff(store: &TaskStore, task_id: &str, package_name: &str) {
        store
            .create(TaskState::new_background_task(
                task_id,
                "wait for foreground",
            ))
            .expect("create task");
        AgentLoop::new(store.clone())
            .step(LoopStepRequest {
                task_id: task_id.to_string(),
                observations: vec![],
                expected_foreground_package: Some(package_name.to_string()),
                model_activity: None,
                model_action: None,
            })
            .expect("create foreground handoff");
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
    fn foreground_scoping_routes_to_single_matching_handoff() {
        let store = test_store();
        create_foreground_handoff(&store, "task-settings-handoff", "com.android.settings");

        let observations = scoped_foreground_observations(
            &store.list().expect("list tasks"),
            &foreground("com.android.settings"),
            None,
        )
        .expect("scope foreground");

        assert_eq!(observations.len(), 1);
        assert!(matches!(
            observations[0].scope,
            fawx_agent_loop::BackgroundObservationScope::Task { ref task_id }
                if task_id == "task-settings-handoff"
        ));
    }

    #[test]
    fn foreground_scoping_rejects_ambiguous_action_and_handoff_matches() {
        let store = test_store();
        create_open_app_action(&store, "task-action", "com.android.settings", true);
        create_foreground_handoff(&store, "task-handoff", "com.android.settings");

        let error = scoped_foreground_observations(
            &store.list().expect("list tasks"),
            &foreground("com.android.settings"),
            None,
        )
        .expect_err("ambiguous foreground ownership should fail");

        assert!(error.contains("ambiguous foreground observation"));
        assert!(error.contains("task-action"));
        assert!(error.contains("task-handoff"));
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
    fn agent_step_options_parse_notification_read_action_contract() {
        let options = AgentStepOptions::parse(&[
            "--action-kind".to_string(),
            "read".to_string(),
            "--action-reason".to_string(),
            "read notifications".to_string(),
            "--action-target".to_string(),
            "notifications".to_string(),
        ])
        .expect("parse notification read action");

        let action = options.model_action.expect("model action");
        assert_eq!(action.kind, ModelActionKind::Read);
        assert!(matches!(
            action.target,
            Some(AgentActivityTarget::NotificationSurface)
        ));
    }

    #[test]
    fn current_open_app_action_maps_to_typed_android_resume_request() {
        let store = test_store();
        create_open_app_action(&store, "task-settings", "com.android.settings", false);
        let task = store.load("task-settings").expect("load task");

        let request =
            android_request_for_current_action(&task.state).expect("android execution request");

        assert_eq!(request.substrate, AndroidSubstrate::ReconRootedStock);
        assert_eq!(
            request.command,
            AndroidCommand::ResumeAppSurface {
                package_name: "com.android.settings".to_string()
            }
        );
    }

    #[test]
    fn current_action_request_rejects_task_without_action() {
        let state = TaskState::new_background_task("task-empty", "no action");

        let error = android_request_for_current_action(&state).expect_err("no action rejected");

        assert!(error.contains("no current action"));
    }

    #[test]
    fn current_action_request_rejects_noncanonical_android_package() {
        let store = test_store();
        create_open_app_action(&store, "task-settings", " com.android.settings", false);
        let task = store.load("task-settings").expect("load task");

        let error = android_request_for_current_action(&task.state)
            .expect_err("noncanonical package rejected");

        assert!(error.contains("surrounding whitespace"));
    }

    #[test]
    fn terminal_session_parser_supports_open_settings() {
        let command = parse_terminal_session_command("open settings", 7).expect("parse prompt");

        let TerminalSessionCommand::OpenApp {
            label,
            package_name,
            candidate,
        } = command
        else {
            panic!("expected open app command");
        };
        assert_eq!(label, "Settings");
        assert_eq!(package_name, "com.android.settings");
        assert_eq!(candidate.candidate_id, "session-turn-7");
        assert_eq!(
            candidate.provider.locality,
            LocalModelLocality::DeterministicFallback
        );
        assert_eq!(
            candidate.provider.provider_id,
            "deterministic-session-parser"
        );
        assert!(matches!(
            candidate.model_action,
            Some(ModelActionProposal {
                kind: ModelActionKind::OpenApp,
                target: Some(AgentActivityTarget::AndroidPackage { package_name }),
                ..
            }) if package_name == "com.android.settings"
        ));
    }

    #[test]
    fn deterministic_session_candidate_preserves_provider_provenance() {
        let provider = LocalModelProviderRef {
            provider_id: "pixel-aicore-candidate".to_string(),
            locality: LocalModelLocality::DeviceLocal,
        };
        let command = parse_terminal_session_command_with_provider("open settings", 3, provider)
            .expect("parse prompt");
        let TerminalSessionCommand::OpenApp { candidate, .. } = command else {
            panic!("expected open app command");
        };
        let (_activity, action) = candidate.into_loop_proposals();

        assert_eq!(
            action.expect("action").proposal_id.as_deref(),
            Some("intent-candidate:pixel-aicore-candidate:session-turn-3")
        );
    }

    #[test]
    fn model_candidate_session_source_pauses_for_owner_approval_without_minting_grants() {
        let store = test_store();
        let candidate = open_app_intent_candidate(
            "Settings",
            "com.android.settings",
            "candidate-1".to_string(),
            LocalModelProviderRef {
                provider_id: "pixel-aicore-candidate".to_string(),
                locality: LocalModelLocality::DeviceLocal,
            },
            "open settings",
        );

        run_session_open_app(
            &store,
            1,
            TerminalSessionIntentSource::ModelCandidate,
            "Settings",
            candidate,
        )
        .expect("model candidate should pause for approval");

        let task = store
            .list()
            .expect("list tasks")
            .into_iter()
            .find(|task| task.state.contract.user_intent == "open Settings")
            .expect("created task");
        assert!(task.state.contract.safety_grants.is_empty());
        assert!(task.state.current_action.is_none());
        assert!(matches!(
            task.state.blocker,
            Some(TaskBlocker::WaitingForUserApproval { .. })
        ));
        assert!(task.state.current_handoff.is_some());
        assert!(task.state.pending_intent_approval.is_some());
    }

    #[test]
    fn approving_pending_model_candidate_accepts_same_action_without_model_retry() {
        let store = test_store();
        let candidate = open_app_intent_candidate(
            "Settings",
            "com.android.settings",
            "candidate-1".to_string(),
            LocalModelProviderRef {
                provider_id: "pixel-aicore-candidate".to_string(),
                locality: LocalModelLocality::DeviceLocal,
            },
            "open settings",
        );

        let outcome = run_session_open_app(
            &store,
            1,
            TerminalSessionIntentSource::ModelCandidate,
            "Settings",
            candidate,
        )
        .expect("model candidate should pause");
        let SessionOpenAppOutcome::PendingApproval { task_id } = outcome else {
            panic!("expected pending approval");
        };

        let task =
            accept_pending_intent_approval(&store, &task_id, "approved").expect("approve pending");

        assert!(task.state.pending_intent_approval.is_none());
        assert!(task.state.current_handoff.is_none());
        assert!(
            task.state
                .contract
                .allows(&fawx_kernel::SafetyRequirement::new(
                    SafetyCapability::AppControl,
                    SafetyScope::AndroidPackage {
                        package_name: "com.android.settings".to_string(),
                    }
                ))
        );
        let action = task.state.current_action.expect("accepted action");
        assert_eq!(action.status, AgentActionStatus::Accepted);
        assert_eq!(
            action.boundary.id,
            "intent-candidate:pixel-aicore-candidate:candidate-1"
        );
    }

    #[test]
    fn terminal_session_interpreter_supports_model_suggestions() {
        let intent = DeterministicSessionInterpreter
            .interpret("suggest open settings", 8)
            .expect("parse model suggestion");

        assert_eq!(intent.source, TerminalSessionIntentSource::ModelCandidate);
        let TerminalSessionCommand::OpenApp { candidate, .. } = intent.command else {
            panic!("expected open app command");
        };
        assert_eq!(candidate.candidate_id, "session-turn-8");
        assert_eq!(candidate.provider.provider_id, "terminal-model-candidate");
    }

    #[test]
    fn terminal_session_parser_supports_approval_commands() {
        assert!(matches!(
            parse_terminal_session_command("approve last", 1).expect("parse approve last"),
            TerminalSessionCommand::ApprovePendingIntent { task_id: None }
        ));
        assert!(matches!(
            parse_terminal_session_command("approve task-123", 1).expect("parse approve task"),
            TerminalSessionCommand::ApprovePendingIntent { task_id: Some(task_id) }
                if task_id == "task-123"
        ));
    }

    #[test]
    fn terminal_session_rejects_model_sourced_approval_commands() {
        let error = DeterministicSessionInterpreter
            .interpret("suggest approve last", 1)
            .expect_err("model approval must reject");

        assert!(error.contains("model candidates cannot approve"));
    }

    #[test]
    fn candidate_dry_run_uses_model_candidate_source_without_execution() {
        let intent = model_candidate_intent("open settings", 4).expect("candidate intent");
        assert_eq!(intent.source, TerminalSessionIntentSource::ModelCandidate);

        let json = candidate_dry_run_json(&intent).expect("dry run json");
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");

        assert_eq!(value["source"], "ModelCandidate");
        assert_eq!(
            value["candidate"]["provider"]["provider_id"],
            "dry-run-model-candidate"
        );
        assert_eq!(value["candidate"]["candidate_id"], "session-turn-4");
        assert_eq!(
            value["candidate"]["model_action"]["kind"],
            serde_json::json!("OpenApp")
        );
        assert!(value["policy_decision"].get("NeedsOwnerApproval").is_some());
    }

    #[test]
    fn terminal_session_parser_supports_package_targets() {
        let command =
            parse_terminal_session_command("open package com.android.settings", 1).expect("parse");

        assert!(matches!(
            command,
            TerminalSessionCommand::OpenApp {
                label,
                package_name,
                ..
            } if label == "com.android.settings" && package_name == "com.android.settings"
        ));
    }

    #[test]
    fn terminal_session_intent_marks_owner_command_authority() {
        let intent = DeterministicSessionInterpreter
            .interpret("open settings", 1)
            .expect("parse intent");

        assert_eq!(intent.source, TerminalSessionIntentSource::OwnerCommand);
        assert!(matches!(
            intent.command,
            TerminalSessionCommand::OpenApp { package_name, .. }
                if package_name == "com.android.settings"
        ));
    }

    #[test]
    fn terminal_session_parser_supports_case_insensitive_verbs() {
        let command = parse_terminal_session_command("Open Package com.android.settings", 1)
            .expect("case-insensitive parse");

        assert!(matches!(
            command,
            TerminalSessionCommand::OpenApp {
                label,
                package_name,
                ..
            } if label == "com.android.settings" && package_name == "com.android.settings"
        ));
    }

    #[test]
    fn terminal_session_parser_rejects_unsupported_prompts() {
        let error =
            parse_terminal_session_command("send a message", 1).expect_err("unsupported prompt");

        assert!(error.contains("unsupported prompt"));
    }

    #[test]
    fn terminal_session_task_ids_are_store_safe() {
        let task_id = session_task_id(1).expect("task id");

        assert!(
            task_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        );
    }

    #[test]
    fn terminal_session_foreground_polling_waits_until_action_observed() {
        let store = test_store();
        create_open_app_action(&store, "task-settings", "com.android.settings", true);
        let mut observations = vec![
            foreground("com.google.android.apps.nexuslauncher"),
            foreground("com.android.settings"),
        ]
        .into_iter();
        let mut sleep_count = 0;

        let task = poll_session_foreground_until_closed(
            &store,
            "task-settings",
            1,
            3,
            Duration::from_millis(1),
            || observations.next().expect("foreground observation"),
            |_| sleep_count += 1,
        )
        .expect("poll foreground");

        let action = task.state.current_action.expect("current action");
        assert_eq!(action.status, AgentActionStatus::Observed);
        assert_eq!(sleep_count, 1);
    }

    #[test]
    fn aosp_adapter_unavailable_maps_to_runtime_observation_without_losing_reason() {
        let android_observation =
            fawx_android_adapter::aosp_platform_adapter_unavailable_observation("foreground");

        let runtime_observation = runtime_observation_from_android(&android_observation);

        assert_eq!(
            runtime_observation.source,
            RuntimeObservationSource::Android {
                substrate: "AospPlatform".to_string(),
                platform_event_source: None,
            }
        );
        assert!(matches!(
            runtime_observation.event,
            RuntimeEvent::ForegroundUnavailable {
                reason: ForegroundUnavailableReason::AdapterUnavailable,
                ..
            }
        ));
    }

    #[test]
    fn aosp_app_launch_runtime_observation_preserves_platform_event_source() {
        let android_observation =
            fawx_android_adapter::aosp_app_launch_observation_from_platform_result(
                fawx_android_adapter::AospAppLaunchResult {
                    package_name: "com.android.settings".to_string(),
                    activity_name: Some("com.android.settings.Settings".to_string()),
                    source: fawx_android_adapter::AospPlatformEventSource {
                        service_name: fawx_android_adapter::AOSP_APP_CONTROLLER_SERVICE.to_string(),
                        event_id: "event-123".to_string(),
                    },
                },
            )
            .expect("valid app launch result");

        let runtime_observation = runtime_observation_from_android(&android_observation);

        assert_eq!(
            runtime_observation.source,
            RuntimeObservationSource::Android {
                substrate: "AospPlatform".to_string(),
                platform_event_source: Some(RuntimePlatformEventSource {
                    service_name: "fawx-system-app-controller".to_string(),
                    event_id: "event-123".to_string(),
                }),
            }
        );
        assert!(matches!(
            runtime_observation.event,
            RuntimeEvent::AppLaunchCompleted { package_name, .. }
                if package_name == "com.android.settings"
        ));
    }

    #[test]
    fn aosp_notification_runtime_observation_preserves_platform_event_source() {
        let android_observation =
            fawx_android_adapter::aosp_notification_observation_from_platform_event(
                fawx_android_adapter::AospNotificationEvent {
                    app_package_name: "com.example.mail".to_string(),
                    summary: "New message from Ada".to_string(),
                    source: fawx_android_adapter::AospPlatformEventSource {
                        service_name: fawx_android_adapter::AOSP_NOTIFICATION_LISTENER_SERVICE
                            .to_string(),
                        event_id: "event-123".to_string(),
                    },
                },
            )
            .expect("valid notification event");

        let runtime_observation = runtime_observation_from_android(&android_observation);

        assert_eq!(
            runtime_observation.source,
            RuntimeObservationSource::Android {
                substrate: "AospPlatform".to_string(),
                platform_event_source: Some(RuntimePlatformEventSource {
                    service_name: "fawx-system-notification-listener".to_string(),
                    event_id: "event-123".to_string(),
                }),
            }
        );
        assert!(matches!(
            runtime_observation.event,
            RuntimeEvent::NotificationReceived { source, summary }
                if source == "com.example.mail" && summary == "New message from Ada"
        ));
    }

    #[test]
    fn command_output_summary_keeps_status_and_streams() {
        let summary = command_output_summary(&CommandOutput {
            stdout: "stdout text\n".to_string(),
            stderr: "stderr text\n".to_string(),
            status: "exit status: 1".to_string(),
            success: false,
        });

        assert!(summary.contains("exit status: 1"));
        assert!(summary.contains("stderr text"));
        assert!(summary.contains("stdout text"));
    }

    #[test]
    fn local_model_probe_json_preserves_provider_status_shape() {
        let report = LocalModelProbeReport {
            substrate: AndroidSubstrate::ReconRootedStock,
            providers: vec![fawx_android_adapter::LocalModelProviderProbe {
                kind: fawx_android_adapter::LocalModelProviderKind::AicoreGeminiNano,
                status: fawx_android_adapter::LocalModelProviderStatus::Indeterminate,
                package_name: None,
                evidence: vec!["pm list packages failed: denied".to_string()],
                note: "probe failed".to_string(),
            }],
        };

        let json = local_model_probe_json(&report).expect("probe json");
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");

        assert_eq!(value["substrate"], "ReconRootedStock");
        assert_eq!(
            value["providers"][0]["kind"],
            serde_json::json!("AicoreGeminiNano")
        );
        assert_eq!(
            value["providers"][0]["status"],
            serde_json::json!("Indeterminate")
        );
        assert_eq!(
            value["providers"][0]["evidence"][0],
            serde_json::json!("pm list packages failed: denied")
        );
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
    fn safety_grant_parser_accepts_notification_scope() {
        assert_eq!(
            parse_safety_capability("notifications-read").expect("parse capability"),
            SafetyCapability::NotificationsRead
        );
        assert_eq!(
            parse_safety_scope("notifications").expect("notifications is a safety scope"),
            SafetyScope::NotificationSurface
        );
    }

    #[test]
    fn safety_grant_parser_rejects_unknown_scope() {
        let error = parse_safety_scope("unknown").expect_err("unknown is not a safety scope");

        assert!(error.contains("safety scope cannot use target"));
    }

    #[test]
    fn handoff_completion_observation_is_typed_runtime_evidence() {
        let observation = handoff_completion_observation(
            "handoff:task-1:user-approval".to_string(),
            "approved".to_string(),
        );

        assert!(matches!(
            observation.source,
            RuntimeObservationSource::Shell { ref name } if name == "fawx-terminal-runner"
        ));
        assert!(matches!(
            observation.event,
            RuntimeEvent::HumanHandoffCompleted { handoff_id, summary }
                if handoff_id == "handoff:task-1:user-approval" && summary == "approved"
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
