use std::error::Error;
use std::fs;
use std::process::ExitCode;

use fawx_android_adapter::{
    AndroidCapabilityStatusEntry, AndroidEvent, AndroidObservation, AndroidReconCommand,
    AndroidSubstrate, android_capability_statuses,
    aosp_app_launch_observation_from_platform_result_json, aosp_app_launch_unavailable_observation,
    aosp_background_supervisor_observation_from_platform_event_json,
    aosp_background_supervisor_unavailable_observation,
    aosp_foreground_observation_from_platform_event_json,
    aosp_notification_observation_from_platform_event_json,
    aosp_notification_unavailable_observation, aosp_platform_adapter_unavailable_observation,
    foreground_observation, run_recon_command,
};
use serde::Serialize;

fn main() -> ExitCode {
    match run() {
        Ok(ProbeExit::Success) => ExitCode::SUCCESS,
        Ok(ProbeExit::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "usage: fawx-android-probe [--substrate recon-rooted-stock|aosp-platform] [--aosp-foreground-event-file PATH] [--aosp-background-supervisor-event-file PATH] [--aosp-app-launch-result-file PATH] [--aosp-notification-event-file PATH]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeExit {
    Success,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProbeArgs {
    Run {
        substrate: AndroidSubstrate,
        aosp_foreground_event_file: Option<String>,
        aosp_background_supervisor_event_file: Option<String>,
        aosp_app_launch_result_file: Option<String>,
        aosp_notification_event_file: Option<String>,
    },
    Help,
}

fn run() -> Result<ProbeExit, Box<dyn Error>> {
    let ProbeArgs::Run {
        substrate,
        aosp_foreground_event_file,
        aosp_background_supervisor_event_file,
        aosp_app_launch_result_file,
        aosp_notification_event_file,
    } = parse_probe_args(std::env::args().skip(1))?
    else {
        return Ok(ProbeExit::Help);
    };
    let report = ProbeReport {
        substrate,
        capability_statuses: android_capability_statuses(substrate),
        observations: probe_observations(
            substrate,
            aosp_foreground_event_file.as_deref(),
            aosp_background_supervisor_event_file.as_deref(),
            aosp_app_launch_result_file.as_deref(),
            aosp_notification_event_file.as_deref(),
        ),
    };

    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(ProbeExit::Success)
}

fn parse_probe_args<I>(args: I) -> Result<ProbeArgs, Box<dyn Error>>
where
    I: Iterator<Item = String>,
{
    let mut substrate = AndroidSubstrate::ReconRootedStock;
    let mut aosp_foreground_event_file = None;
    let mut aosp_background_supervisor_event_file = None;
    let mut aosp_app_launch_result_file = None;
    let mut aosp_notification_event_file = None;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(ProbeArgs::Help),
            "--substrate" => {
                let value = args.next().ok_or("missing value after --substrate")?;
                substrate = value.parse()?;
            }
            "--aosp-foreground-event-file" => {
                let value = args
                    .next()
                    .ok_or("missing value after --aosp-foreground-event-file")?;
                if value.starts_with("--") {
                    return Err("missing value after --aosp-foreground-event-file".into());
                }
                aosp_foreground_event_file = Some(value);
            }
            "--aosp-background-supervisor-event-file" => {
                let value = args
                    .next()
                    .ok_or("missing value after --aosp-background-supervisor-event-file")?;
                if value.starts_with("--") {
                    return Err(
                        "missing value after --aosp-background-supervisor-event-file".into(),
                    );
                }
                aosp_background_supervisor_event_file = Some(value);
            }
            "--aosp-app-launch-result-file" => {
                let value = args
                    .next()
                    .ok_or("missing value after --aosp-app-launch-result-file")?;
                if value.starts_with("--") {
                    return Err("missing value after --aosp-app-launch-result-file".into());
                }
                aosp_app_launch_result_file = Some(value);
            }
            "--aosp-notification-event-file" => {
                let value = args
                    .next()
                    .ok_or("missing value after --aosp-notification-event-file")?;
                if value.starts_with("--") {
                    return Err("missing value after --aosp-notification-event-file".into());
                }
                aosp_notification_event_file = Some(value);
            }
            value if value.starts_with("--") => {
                return Err(format!("unexpected probe option: {value}").into());
            }
            value => {
                substrate = value.parse()?;
            }
        }
    }

    if (aosp_foreground_event_file.is_some()
        || aosp_background_supervisor_event_file.is_some()
        || aosp_app_launch_result_file.is_some()
        || aosp_notification_event_file.is_some())
        && substrate != AndroidSubstrate::AospPlatform
    {
        return Err(
            "AOSP platform event files are only valid with --substrate aosp-platform".into(),
        );
    }

    Ok(ProbeArgs::Run {
        substrate,
        aosp_foreground_event_file,
        aosp_background_supervisor_event_file,
        aosp_app_launch_result_file,
        aosp_notification_event_file,
    })
}

#[derive(Debug, Serialize)]
struct ProbeReport {
    substrate: AndroidSubstrate,
    capability_statuses: Vec<AndroidCapabilityStatusEntry>,
    observations: Vec<ProbeObservation>,
}

#[derive(Debug, Serialize)]
struct ProbeObservation {
    name: String,
    ok: bool,
    summary: String,
    android_observation: Option<AndroidObservation>,
}

fn probe_observations(
    substrate: AndroidSubstrate,
    aosp_foreground_event_file: Option<&str>,
    aosp_background_supervisor_event_file: Option<&str>,
    aosp_app_launch_result_file: Option<&str>,
    aosp_notification_event_file: Option<&str>,
) -> Vec<ProbeObservation> {
    match substrate {
        AndroidSubstrate::ReconRootedStock => vec![
            command_observation("whoami", AndroidReconCommand::Whoami),
            command_observation("id", AndroidReconCommand::Id),
            command_observation("uname", AndroidReconCommand::Uname),
            root_observation(),
            foreground_probe_observation(substrate, AospForegroundEventObservation::Unavailable),
        ],
        AndroidSubstrate::AospPlatform => {
            let foreground_event_observation =
                aosp_foreground_event_observation_from_file(aosp_foreground_event_file);
            let supervisor_event_observation =
                aosp_background_supervisor_event_observation_from_file(
                    aosp_background_supervisor_event_file,
                );
            let app_launch_result_observation =
                aosp_app_launch_result_observation_from_file(aosp_app_launch_result_file);
            let notification_event_observation =
                aosp_notification_event_observation_from_file(aosp_notification_event_file);
            vec![
                aosp_adapter_probe_observation(
                    aosp_foreground_event_file,
                    &foreground_event_observation,
                ),
                foreground_probe_observation(substrate, foreground_event_observation),
                aosp_background_supervisor_probe_observation(
                    aosp_background_supervisor_event_file,
                    &supervisor_event_observation,
                ),
                background_supervisor_probe_observation(supervisor_event_observation),
                aosp_app_controller_probe_observation(
                    aosp_app_launch_result_file,
                    &app_launch_result_observation,
                ),
                app_launch_probe_observation(app_launch_result_observation),
                aosp_notification_listener_probe_observation(
                    aosp_notification_event_file,
                    &notification_event_observation,
                ),
                notification_probe_observation(notification_event_observation),
            ]
        }
    }
}

fn command_observation(name: &str, command: AndroidReconCommand) -> ProbeObservation {
    match run_recon_command(command) {
        Ok(output) => ProbeObservation {
            name: name.to_string(),
            ok: true,
            summary: output,
            android_observation: None,
        },
        Err(error) => ProbeObservation {
            name: name.to_string(),
            ok: false,
            summary: error,
            android_observation: None,
        },
    }
}

fn root_observation() -> ProbeObservation {
    let rooted = run_recon_command(AndroidReconCommand::RootCheck).is_ok();

    ProbeObservation {
        name: "root".to_string(),
        ok: rooted,
        summary: if rooted {
            "su command succeeded".to_string()
        } else {
            "su command unavailable or denied".to_string()
        },
        android_observation: None,
    }
}

fn aosp_adapter_probe_observation(
    aosp_foreground_event_file: Option<&str>,
    event_observation: &AospForegroundEventObservation,
) -> ProbeObservation {
    match (aosp_foreground_event_file, event_observation) {
        (Some(path), AospForegroundEventObservation::Observed(_)) => ProbeObservation {
            name: "aosp-platform-adapter".to_string(),
            ok: true,
            summary: format!("AOSP foreground event source connected: {path}"),
            android_observation: None,
        },
        (Some(path), AospForegroundEventObservation::Invalid { reason }) => ProbeObservation {
            name: "aosp-platform-adapter".to_string(),
            ok: false,
            summary: format!("AOSP foreground event source invalid: {path}: {reason}"),
            android_observation: None,
        },
        (Some(path), AospForegroundEventObservation::Unavailable) => ProbeObservation {
            name: "aosp-platform-adapter".to_string(),
            ok: false,
            summary: format!("AOSP foreground event source unavailable: {path}"),
            android_observation: None,
        },
        (None, AospForegroundEventObservation::Unavailable) => ProbeObservation {
            name: "aosp-platform-adapter".to_string(),
            ok: false,
            summary: "AOSP platform adapter is not connected in this terminal binary".to_string(),
            android_observation: None,
        },
        (None, _) => ProbeObservation {
            name: "aosp-platform-adapter".to_string(),
            ok: false,
            summary: "AOSP platform adapter state is inconsistent".to_string(),
            android_observation: None,
        },
    }
}

fn foreground_probe_observation(
    substrate: AndroidSubstrate,
    aosp_foreground_event_observation: AospForegroundEventObservation,
) -> ProbeObservation {
    let observation = match substrate {
        AndroidSubstrate::ReconRootedStock => foreground_observation(substrate),
        AndroidSubstrate::AospPlatform => match aosp_foreground_event_observation {
            AospForegroundEventObservation::Observed(observation) => observation,
            AospForegroundEventObservation::Invalid { reason } => {
                return foreground_unavailable(format!("foreground unavailable: {reason}"), None);
            }
            AospForegroundEventObservation::Unavailable => {
                aosp_platform_adapter_unavailable_observation("foreground")
            }
        },
    };

    match observation.event().clone() {
        AndroidEvent::ForegroundAppChanged {
            package_name,
            activity_name,
        } => ProbeObservation {
            name: "foreground".to_string(),
            ok: true,
            summary: match activity_name {
                Some(activity) => format!("{package_name}/{activity}"),
                None => package_name,
            },
            android_observation: Some(observation),
        },
        AndroidEvent::ForegroundObservationUnavailable {
            reason, raw_source, ..
        } => foreground_unavailable(
            match raw_source {
                Some(raw_source) if !raw_source.is_empty() => {
                    format!("foreground unavailable: {reason:?}; raw_source={raw_source}")
                }
                _ => format!("foreground unavailable: {reason:?}"),
            },
            Some(observation),
        ),
        AndroidEvent::TargetSurfaceBecameUnavailable { .. } => foreground_unavailable(
            "foreground target unavailable".to_string(),
            Some(observation),
        ),
        _ => foreground_unavailable(
            "unexpected foreground observation".to_string(),
            Some(observation),
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AospForegroundEventObservation {
    Observed(AndroidObservation),
    Invalid { reason: String },
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AospBackgroundSupervisorEventObservation {
    Observed(AndroidObservation),
    Invalid { reason: String },
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AospAppLaunchResultObservation {
    Observed(AndroidObservation),
    Invalid { reason: String },
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AospNotificationEventObservation {
    Observed(AndroidObservation),
    Invalid { reason: String },
    Unavailable,
}

fn aosp_foreground_event_observation_from_file(
    aosp_foreground_event_file: Option<&str>,
) -> AospForegroundEventObservation {
    let Some(path) = aosp_foreground_event_file else {
        return AospForegroundEventObservation::Unavailable;
    };

    let event_json = match fs::read_to_string(path) {
        Ok(event_json) => event_json,
        Err(error) => {
            return AospForegroundEventObservation::Invalid {
                reason: format!("failed to read {path}: {error}"),
            };
        }
    };

    match aosp_foreground_observation_from_platform_event_json(&event_json) {
        Ok(observation) => AospForegroundEventObservation::Observed(observation),
        Err(error) => AospForegroundEventObservation::Invalid { reason: error },
    }
}

fn aosp_background_supervisor_probe_observation(
    aosp_background_supervisor_event_file: Option<&str>,
    event_observation: &AospBackgroundSupervisorEventObservation,
) -> ProbeObservation {
    match (aosp_background_supervisor_event_file, event_observation) {
        (Some(path), AospBackgroundSupervisorEventObservation::Observed(_)) => ProbeObservation {
            name: "aosp-background-supervisor".to_string(),
            ok: true,
            summary: format!("AOSP background supervisor event source connected: {path}"),
            android_observation: None,
        },
        (Some(path), AospBackgroundSupervisorEventObservation::Invalid { reason }) => {
            ProbeObservation {
                name: "aosp-background-supervisor".to_string(),
                ok: false,
                summary: format!(
                    "AOSP background supervisor event source invalid: {path}: {reason}"
                ),
                android_observation: None,
            }
        }
        (Some(path), AospBackgroundSupervisorEventObservation::Unavailable) => ProbeObservation {
            name: "aosp-background-supervisor".to_string(),
            ok: false,
            summary: format!("AOSP background supervisor event source unavailable: {path}"),
            android_observation: None,
        },
        (None, AospBackgroundSupervisorEventObservation::Unavailable) => ProbeObservation {
            name: "aosp-background-supervisor".to_string(),
            ok: false,
            summary: "AOSP background supervisor is not connected in this terminal binary"
                .to_string(),
            android_observation: None,
        },
        (None, _) => ProbeObservation {
            name: "aosp-background-supervisor".to_string(),
            ok: false,
            summary: "AOSP background supervisor state is inconsistent".to_string(),
            android_observation: None,
        },
    }
}

fn background_supervisor_probe_observation(
    event_observation: AospBackgroundSupervisorEventObservation,
) -> ProbeObservation {
    let observation = match event_observation {
        AospBackgroundSupervisorEventObservation::Observed(observation) => observation,
        AospBackgroundSupervisorEventObservation::Invalid { reason } => {
            return background_supervisor_unavailable(
                format!("background supervisor unavailable: {reason}"),
                None,
            );
        }
        AospBackgroundSupervisorEventObservation::Unavailable => {
            aosp_background_supervisor_unavailable_observation("background-supervisor")
        }
    };

    match observation.event().clone() {
        AndroidEvent::BackgroundSupervisorHeartbeat {
            supervisor_id,
            active_tasks,
        } => ProbeObservation {
            name: "background-supervisor".to_string(),
            ok: true,
            summary: format!("{supervisor_id}: {active_tasks} active task(s)"),
            android_observation: Some(observation),
        },
        AndroidEvent::BackgroundSupervisorUnavailable {
            reason, raw_source, ..
        } => background_supervisor_unavailable(
            match raw_source {
                Some(raw_source) if !raw_source.is_empty() => {
                    format!(
                        "background supervisor unavailable: {reason:?}; raw_source={raw_source}"
                    )
                }
                _ => format!("background supervisor unavailable: {reason:?}"),
            },
            Some(observation),
        ),
        _ => background_supervisor_unavailable(
            "unexpected background supervisor observation".to_string(),
            Some(observation),
        ),
    }
}

fn aosp_background_supervisor_event_observation_from_file(
    aosp_background_supervisor_event_file: Option<&str>,
) -> AospBackgroundSupervisorEventObservation {
    let Some(path) = aosp_background_supervisor_event_file else {
        return AospBackgroundSupervisorEventObservation::Unavailable;
    };

    let event_json = match fs::read_to_string(path) {
        Ok(event_json) => event_json,
        Err(error) => {
            return AospBackgroundSupervisorEventObservation::Invalid {
                reason: format!("failed to read {path}: {error}"),
            };
        }
    };

    match aosp_background_supervisor_observation_from_platform_event_json(&event_json) {
        Ok(observation) => AospBackgroundSupervisorEventObservation::Observed(observation),
        Err(error) => AospBackgroundSupervisorEventObservation::Invalid { reason: error },
    }
}

fn background_supervisor_unavailable(
    summary: String,
    android_observation: Option<AndroidObservation>,
) -> ProbeObservation {
    ProbeObservation {
        name: "background-supervisor".to_string(),
        ok: false,
        summary,
        android_observation,
    }
}

fn aosp_app_controller_probe_observation(
    aosp_app_launch_result_file: Option<&str>,
    result_observation: &AospAppLaunchResultObservation,
) -> ProbeObservation {
    match (aosp_app_launch_result_file, result_observation) {
        (Some(path), AospAppLaunchResultObservation::Observed(_)) => ProbeObservation {
            name: "aosp-app-controller".to_string(),
            ok: true,
            summary: format!("AOSP app controller result source connected: {path}"),
            android_observation: None,
        },
        (Some(path), AospAppLaunchResultObservation::Invalid { reason }) => ProbeObservation {
            name: "aosp-app-controller".to_string(),
            ok: false,
            summary: format!("AOSP app controller result source invalid: {path}: {reason}"),
            android_observation: None,
        },
        (Some(path), AospAppLaunchResultObservation::Unavailable) => ProbeObservation {
            name: "aosp-app-controller".to_string(),
            ok: false,
            summary: format!("AOSP app controller result source unavailable: {path}"),
            android_observation: None,
        },
        (None, AospAppLaunchResultObservation::Unavailable) => ProbeObservation {
            name: "aosp-app-controller".to_string(),
            ok: false,
            summary: "AOSP app controller is not connected in this terminal binary".to_string(),
            android_observation: None,
        },
        (None, _) => ProbeObservation {
            name: "aosp-app-controller".to_string(),
            ok: false,
            summary: "AOSP app controller state is inconsistent".to_string(),
            android_observation: None,
        },
    }
}

fn app_launch_probe_observation(
    result_observation: AospAppLaunchResultObservation,
) -> ProbeObservation {
    let observation = match result_observation {
        AospAppLaunchResultObservation::Observed(observation) => observation,
        AospAppLaunchResultObservation::Invalid { reason } => {
            return app_launch_unavailable(format!("app launch unavailable: {reason}"), None);
        }
        AospAppLaunchResultObservation::Unavailable => {
            aosp_app_launch_unavailable_observation("app-launch")
        }
    };

    match observation.event().clone() {
        AndroidEvent::AppLaunchCompleted {
            package_name,
            activity_name,
        } => ProbeObservation {
            name: "app-launch".to_string(),
            ok: true,
            summary: match activity_name {
                Some(activity) => format!("{package_name}/{activity}"),
                None => package_name,
            },
            android_observation: Some(observation),
        },
        AndroidEvent::AppLaunchUnavailable {
            reason, raw_source, ..
        } => app_launch_unavailable(
            match raw_source {
                Some(raw_source) if !raw_source.is_empty() => {
                    format!("app launch unavailable: {reason:?}; raw_source={raw_source}")
                }
                _ => format!("app launch unavailable: {reason:?}"),
            },
            Some(observation),
        ),
        _ => app_launch_unavailable(
            "unexpected app launch observation".to_string(),
            Some(observation),
        ),
    }
}

fn aosp_app_launch_result_observation_from_file(
    aosp_app_launch_result_file: Option<&str>,
) -> AospAppLaunchResultObservation {
    let Some(path) = aosp_app_launch_result_file else {
        return AospAppLaunchResultObservation::Unavailable;
    };

    let result_json = match fs::read_to_string(path) {
        Ok(result_json) => result_json,
        Err(error) => {
            return AospAppLaunchResultObservation::Invalid {
                reason: format!("failed to read {path}: {error}"),
            };
        }
    };

    match aosp_app_launch_observation_from_platform_result_json(&result_json) {
        Ok(observation) => AospAppLaunchResultObservation::Observed(observation),
        Err(error) => AospAppLaunchResultObservation::Invalid { reason: error },
    }
}

fn app_launch_unavailable(
    summary: String,
    android_observation: Option<AndroidObservation>,
) -> ProbeObservation {
    ProbeObservation {
        name: "app-launch".to_string(),
        ok: false,
        summary,
        android_observation,
    }
}

fn aosp_notification_listener_probe_observation(
    aosp_notification_event_file: Option<&str>,
    event_observation: &AospNotificationEventObservation,
) -> ProbeObservation {
    match (aosp_notification_event_file, event_observation) {
        (Some(path), AospNotificationEventObservation::Observed(_)) => ProbeObservation {
            name: "aosp-notification-listener".to_string(),
            ok: true,
            summary: format!("AOSP notification event source connected: {path}"),
            android_observation: None,
        },
        (Some(path), AospNotificationEventObservation::Invalid { reason }) => ProbeObservation {
            name: "aosp-notification-listener".to_string(),
            ok: false,
            summary: format!("AOSP notification event source invalid: {path}: {reason}"),
            android_observation: None,
        },
        (Some(path), AospNotificationEventObservation::Unavailable) => ProbeObservation {
            name: "aosp-notification-listener".to_string(),
            ok: false,
            summary: format!("AOSP notification event source unavailable: {path}"),
            android_observation: None,
        },
        (None, AospNotificationEventObservation::Unavailable) => ProbeObservation {
            name: "aosp-notification-listener".to_string(),
            ok: false,
            summary: "AOSP notification listener is not connected in this terminal binary"
                .to_string(),
            android_observation: None,
        },
        (None, _) => ProbeObservation {
            name: "aosp-notification-listener".to_string(),
            ok: false,
            summary: "AOSP notification listener state is inconsistent".to_string(),
            android_observation: None,
        },
    }
}

fn notification_probe_observation(
    event_observation: AospNotificationEventObservation,
) -> ProbeObservation {
    let observation = match event_observation {
        AospNotificationEventObservation::Observed(observation) => observation,
        AospNotificationEventObservation::Invalid { reason } => {
            return notification_unavailable(format!("notification unavailable: {reason}"), None);
        }
        AospNotificationEventObservation::Unavailable => {
            aosp_notification_unavailable_observation("notifications")
        }
    };

    match observation.event().clone() {
        AndroidEvent::NotificationReceived { source, summary } => ProbeObservation {
            name: "notification".to_string(),
            ok: true,
            summary: format!("{source}: {summary}"),
            android_observation: Some(observation),
        },
        AndroidEvent::NotificationUnavailable {
            reason, raw_source, ..
        } => notification_unavailable(
            match raw_source {
                Some(raw_source) if !raw_source.is_empty() => {
                    format!("notification unavailable: {reason:?}; raw_source={raw_source}")
                }
                _ => format!("notification unavailable: {reason:?}"),
            },
            Some(observation),
        ),
        _ => notification_unavailable(
            "unexpected notification observation".to_string(),
            Some(observation),
        ),
    }
}

fn aosp_notification_event_observation_from_file(
    aosp_notification_event_file: Option<&str>,
) -> AospNotificationEventObservation {
    let Some(path) = aosp_notification_event_file else {
        return AospNotificationEventObservation::Unavailable;
    };

    let event_json = match fs::read_to_string(path) {
        Ok(event_json) => event_json,
        Err(error) => {
            return AospNotificationEventObservation::Invalid {
                reason: format!("failed to read {path}: {error}"),
            };
        }
    };

    match aosp_notification_observation_from_platform_event_json(&event_json) {
        Ok(observation) => AospNotificationEventObservation::Observed(observation),
        Err(error) => AospNotificationEventObservation::Invalid { reason: error },
    }
}

fn notification_unavailable(
    summary: String,
    android_observation: Option<AndroidObservation>,
) -> ProbeObservation {
    ProbeObservation {
        name: "notification".to_string(),
        ok: false,
        summary,
        android_observation,
    }
}

fn foreground_unavailable(
    summary: String,
    android_observation: Option<AndroidObservation>,
) -> ProbeObservation {
    ProbeObservation {
        name: "foreground".to_string(),
        ok: false,
        summary,
        android_observation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_probe_to_rooted_stock_recon() {
        let args = parse_probe_args(std::iter::empty()).expect("default substrate should parse");

        assert_eq!(
            args,
            ProbeArgs::Run {
                substrate: AndroidSubstrate::ReconRootedStock,
                aosp_foreground_event_file: None,
                aosp_background_supervisor_event_file: None,
                aosp_app_launch_result_file: None,
                aosp_notification_event_file: None,
            }
        );
    }

    #[test]
    fn parses_explicit_aosp_substrate_arg() {
        let args = parse_probe_args(
            ["--substrate", "aosp-platform"]
                .into_iter()
                .map(str::to_string),
        )
        .expect("aosp substrate should parse");

        assert_eq!(
            args,
            ProbeArgs::Run {
                substrate: AndroidSubstrate::AospPlatform,
                aosp_foreground_event_file: None,
                aosp_background_supervisor_event_file: None,
                aosp_app_launch_result_file: None,
                aosp_notification_event_file: None,
            }
        );
    }

    #[test]
    fn parses_aosp_foreground_event_file() {
        let args = parse_probe_args(
            [
                "--substrate",
                "aosp-platform",
                "--aosp-foreground-event-file",
                "/run/fawx/foreground.json",
            ]
            .into_iter()
            .map(str::to_string),
        )
        .expect("platform event file should parse");

        assert_eq!(
            args,
            ProbeArgs::Run {
                substrate: AndroidSubstrate::AospPlatform,
                aosp_foreground_event_file: Some("/run/fawx/foreground.json".to_string()),
                aosp_background_supervisor_event_file: None,
                aosp_app_launch_result_file: None,
                aosp_notification_event_file: None,
            }
        );
    }

    #[test]
    fn parses_aosp_background_supervisor_event_file() {
        let args = parse_probe_args(
            [
                "--substrate",
                "aosp-platform",
                "--aosp-background-supervisor-event-file",
                "/run/fawx/background-supervisor.json",
            ]
            .into_iter()
            .map(str::to_string),
        )
        .expect("background supervisor event file should parse");

        assert_eq!(
            args,
            ProbeArgs::Run {
                substrate: AndroidSubstrate::AospPlatform,
                aosp_foreground_event_file: None,
                aosp_background_supervisor_event_file: Some(
                    "/run/fawx/background-supervisor.json".to_string()
                ),
                aosp_app_launch_result_file: None,
                aosp_notification_event_file: None,
            }
        );
    }

    #[test]
    fn parses_aosp_app_launch_result_file() {
        let args = parse_probe_args(
            [
                "--substrate",
                "aosp-platform",
                "--aosp-app-launch-result-file",
                "/run/fawx/app-launch.json",
            ]
            .into_iter()
            .map(str::to_string),
        )
        .expect("app launch result file should parse");

        assert_eq!(
            args,
            ProbeArgs::Run {
                substrate: AndroidSubstrate::AospPlatform,
                aosp_foreground_event_file: None,
                aosp_background_supervisor_event_file: None,
                aosp_app_launch_result_file: Some("/run/fawx/app-launch.json".to_string()),
                aosp_notification_event_file: None,
            }
        );
    }

    #[test]
    fn parses_aosp_notification_event_file() {
        let args = parse_probe_args(
            [
                "--substrate",
                "aosp-platform",
                "--aosp-notification-event-file",
                "/run/fawx/notification.json",
            ]
            .into_iter()
            .map(str::to_string),
        )
        .expect("notification event file should parse");

        assert_eq!(
            args,
            ProbeArgs::Run {
                substrate: AndroidSubstrate::AospPlatform,
                aosp_foreground_event_file: None,
                aosp_background_supervisor_event_file: None,
                aosp_app_launch_result_file: None,
                aosp_notification_event_file: Some("/run/fawx/notification.json".to_string()),
            }
        );
    }

    #[test]
    fn rejects_aosp_foreground_event_file_on_recon_substrate() {
        let error = parse_probe_args(
            [
                "--substrate",
                "recon-rooted-stock",
                "--aosp-foreground-event-file",
                "/run/fawx/foreground.json",
            ]
            .into_iter()
            .map(str::to_string),
        )
        .expect_err("platform event file must not be accepted on recon substrate");

        assert!(error.to_string().contains("aosp-platform"));
    }

    #[test]
    fn parses_help_as_successful_probe_exit() {
        let args = parse_probe_args(["--help"].into_iter().map(str::to_string))
            .expect("help should parse");

        assert_eq!(args, ProbeArgs::Help);
    }

    #[test]
    fn aosp_probe_uses_unavailable_platform_observation_until_adapter_exists() {
        let observations =
            probe_observations(AndroidSubstrate::AospPlatform, None, None, None, None);

        assert!(observations.iter().any(|observation| {
            observation.name == "aosp-platform-adapter"
                && !observation.ok
                && observation.summary.contains("not connected")
        }));
        assert!(observations.iter().any(|observation| {
            matches!(
                observation.android_observation.as_ref().map(|value| value.event()),
                Some(AndroidEvent::ForegroundObservationUnavailable {
                    reason: fawx_android_adapter::AndroidForegroundUnavailableReason::AdapterUnavailable,
                    ..
                })
            )
        }));
        assert!(observations.iter().any(|observation| {
            matches!(
                observation
                    .android_observation
                    .as_ref()
                    .map(|value| value.event()),
                Some(AndroidEvent::BackgroundSupervisorUnavailable {
                    reason: fawx_android_adapter::AndroidBackgroundSupervisorUnavailableReason::AdapterUnavailable,
                    ..
                })
            )
        }));
        assert!(observations.iter().any(|observation| {
            matches!(
                observation
                    .android_observation
                    .as_ref()
                    .map(|value| value.event()),
                Some(AndroidEvent::AppLaunchUnavailable {
                    reason:
                        fawx_android_adapter::AndroidAppLaunchUnavailableReason::AdapterUnavailable,
                    ..
                })
            )
        }));
        assert!(observations.iter().any(|observation| {
            matches!(
                observation
                    .android_observation
                    .as_ref()
                    .map(|value| value.event()),
                Some(AndroidEvent::NotificationUnavailable {
                    reason: fawx_android_adapter::AndroidNotificationUnavailableReason::AdapterUnavailable,
                    ..
                })
            )
        }));
    }

    #[test]
    fn aosp_probe_can_ingest_privileged_foreground_event_file() {
        let event_path = std::env::temp_dir().join(format!(
            "fawx-aosp-foreground-event-{}.json",
            std::process::id()
        ));
        std::fs::write(
            &event_path,
            r#"{
                "package_name": "com.android.settings",
                "activity_name": "com.android.settings.Settings",
                "source": {
                    "service_name": "fawx-system-foreground-observer",
                    "event_id": "event-123"
                }
            }"#,
        )
        .expect("test event should write");

        let observations = probe_observations(
            AndroidSubstrate::AospPlatform,
            Some(event_path.to_str().expect("temp path should be utf8")),
            None,
            None,
            None,
        );
        let _ = std::fs::remove_file(event_path);

        assert!(observations.iter().any(|observation| {
            observation.name == "aosp-platform-adapter"
                && observation.ok
                && observation.summary.contains("event source connected")
        }));
        assert!(observations.iter().any(|observation| {
            matches!(
                observation.android_observation.as_ref().map(|value| value.event()),
                Some(AndroidEvent::ForegroundAppChanged { package_name, .. })
                    if package_name == "com.android.settings"
            )
        }));
    }

    #[test]
    fn aosp_probe_marks_invalid_event_file_as_disconnected() {
        let event_path = std::env::temp_dir().join(format!(
            "fawx-aosp-invalid-foreground-event-{}.json",
            std::process::id()
        ));
        std::fs::write(
            &event_path,
            r#"{
                "package_name": "com.android.settings",
                "source": {
                    "service_name": "dumpsys",
                    "event_id": "event-123"
                }
            }"#,
        )
        .expect("test event should write");

        let observations = probe_observations(
            AndroidSubstrate::AospPlatform,
            Some(event_path.to_str().expect("temp path should be utf8")),
            None,
            None,
            None,
        );
        let _ = std::fs::remove_file(event_path);

        assert!(observations.iter().any(|observation| {
            observation.name == "aosp-platform-adapter"
                && !observation.ok
                && observation.summary.contains("invalid")
        }));
        assert!(observations.iter().any(|observation| {
            observation.name == "foreground"
                && !observation.ok
                && observation.android_observation.is_none()
        }));
    }

    #[test]
    fn aosp_probe_can_ingest_privileged_background_supervisor_event_file() {
        let event_path = std::env::temp_dir().join(format!(
            "fawx-aosp-background-supervisor-event-{}.json",
            std::process::id()
        ));
        std::fs::write(
            &event_path,
            r#"{
                "supervisor_id": "supervisor-1",
                "active_tasks": 2,
                "source": {
                    "service_name": "fawx-system-background-supervisor",
                    "event_id": "event-123"
                }
            }"#,
        )
        .expect("test event should write");

        let observations = probe_observations(
            AndroidSubstrate::AospPlatform,
            None,
            Some(event_path.to_str().expect("temp path should be utf8")),
            None,
            None,
        );
        let _ = std::fs::remove_file(event_path);

        assert!(observations.iter().any(|observation| {
            observation.name == "aosp-background-supervisor"
                && observation.ok
                && observation.summary.contains("event source connected")
        }));
        assert!(observations.iter().any(|observation| {
            matches!(
                observation.android_observation.as_ref().map(|value| value.event()),
                Some(AndroidEvent::BackgroundSupervisorHeartbeat {
                    supervisor_id,
                    active_tasks: 2,
                }) if supervisor_id == "supervisor-1"
            )
        }));
    }

    #[test]
    fn aosp_probe_marks_invalid_background_supervisor_event_file_as_disconnected() {
        let event_path = std::env::temp_dir().join(format!(
            "fawx-aosp-invalid-background-supervisor-event-{}.json",
            std::process::id()
        ));
        std::fs::write(
            &event_path,
            r#"{
                "supervisor_id": "supervisor-1",
                "active_tasks": 2,
                "source": {
                    "service_name": "adb",
                    "event_id": "event-123"
                }
            }"#,
        )
        .expect("test event should write");

        let observations = probe_observations(
            AndroidSubstrate::AospPlatform,
            None,
            Some(event_path.to_str().expect("temp path should be utf8")),
            None,
            None,
        );
        let _ = std::fs::remove_file(event_path);

        assert!(observations.iter().any(|observation| {
            observation.name == "aosp-background-supervisor"
                && !observation.ok
                && observation.summary.contains("invalid")
        }));
        assert!(observations.iter().any(|observation| {
            observation.name == "background-supervisor"
                && !observation.ok
                && observation.android_observation.is_none()
        }));
    }

    #[test]
    fn aosp_probe_can_ingest_privileged_app_launch_result_file() {
        let result_path = std::env::temp_dir().join(format!(
            "fawx-aosp-app-launch-result-{}.json",
            std::process::id()
        ));
        std::fs::write(
            &result_path,
            r#"{
                "package_name": "com.android.settings",
                "activity_name": "com.android.settings.Settings",
                "source": {
                    "service_name": "fawx-system-app-controller",
                    "event_id": "event-123"
                }
            }"#,
        )
        .expect("test result should write");

        let observations = probe_observations(
            AndroidSubstrate::AospPlatform,
            None,
            None,
            Some(result_path.to_str().expect("temp path should be utf8")),
            None,
        );
        let _ = std::fs::remove_file(result_path);

        assert!(observations.iter().any(|observation| {
            observation.name == "aosp-app-controller"
                && observation.ok
                && observation.summary.contains("result source connected")
        }));
        assert!(observations.iter().any(|observation| {
            matches!(
                observation.android_observation.as_ref().map(|value| value.event()),
                Some(AndroidEvent::AppLaunchCompleted { package_name, .. })
                    if package_name == "com.android.settings"
            )
        }));
    }

    #[test]
    fn aosp_probe_can_ingest_privileged_notification_event_file() {
        let event_path = std::env::temp_dir().join(format!(
            "fawx-aosp-notification-event-{}.json",
            std::process::id()
        ));
        std::fs::write(
            &event_path,
            r#"{
                "app_package_name": "com.example.mail",
                "summary": "New message from Ada",
                "source": {
                    "service_name": "fawx-system-notification-listener",
                    "event_id": "event-123"
                }
            }"#,
        )
        .expect("test event should write");

        let observations = probe_observations(
            AndroidSubstrate::AospPlatform,
            None,
            None,
            None,
            Some(event_path.to_str().expect("temp path should be utf8")),
        );
        let _ = std::fs::remove_file(event_path);

        assert!(observations.iter().any(|observation| {
            observation.name == "aosp-notification-listener"
                && observation.ok
                && observation.summary.contains("event source connected")
        }));
        assert!(observations.iter().any(|observation| {
            matches!(
                observation.android_observation.as_ref().map(|value| value.event()),
                Some(AndroidEvent::NotificationReceived { source, summary })
                    if source == "com.example.mail" && summary == "New message from Ada"
            )
        }));
    }
}
