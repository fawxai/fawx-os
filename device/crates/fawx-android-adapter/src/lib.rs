//! Android substrate adapter for Fawx OS.
//!
//! This crate is intentionally named as an adapter, not a core runtime. The
//! goal is to keep Android-specific bindings thin so they can be replaced over
//! time without rewriting the kernel or harness.

use serde::{Deserialize, Serialize};
use std::process::Command;

/// Which Android-facing substrate produced an observation or accepts a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AndroidSubstrate {
    /// Temporary stock/rooted Android probe. This must not define core runtime
    /// semantics and should remain replaceable by the AOSP adapter.
    ReconRootedStock,
    /// The real prototype target: AOSP-level platform control.
    AospPlatform,
}

/// Observations flowing upward from Android into the Fawx OS runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AndroidEvent {
    ForegroundAppChanged {
        package_name: String,
        activity_name: Option<String>,
    },
    ForegroundObservationUnavailable {
        target: String,
        reason: AndroidForegroundUnavailableReason,
        raw_source: Option<String>,
    },
    TargetSurfaceBecameUnavailable {
        target: String,
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
    RootedActionFailed {
        action: String,
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AndroidForegroundUnavailableReason {
    CommandFailed,
    EmptyOutput,
    ParseFailed,
}

/// Commands flowing downward from the runtime into the Android adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AndroidCommand {
    AcquireForeground { target: String },
    ReleaseForeground,
    ObserveNotifications { source: String },
    QueryForegroundState,
    ResumeAppSurface { package_name: String },
    PerformRootedAction { action: String, target: String },
}

/// A typed Android observation with explicit substrate provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AndroidObservation {
    pub substrate: AndroidSubstrate,
    pub event: AndroidEvent,
}

/// A typed Android command with explicit target substrate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AndroidActionRequest {
    pub substrate: AndroidSubstrate,
    pub command: AndroidCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub status: String,
    pub success: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForegroundTarget {
    pub package_name: String,
    pub activity_name: Option<String>,
}

pub fn parse_foreground_target(line: &str) -> Option<ForegroundTarget> {
    let start = line.find(" u0 ")? + " u0 ".len();
    let tail = line[start..].trim();
    let component = tail
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_end_matches('}')
        .trim_end_matches(',');

    let (package, activity) = component.split_once('/')?;
    if package.is_empty() || activity.is_empty() {
        return None;
    }

    Some(ForegroundTarget {
        package_name: package.to_string(),
        activity_name: Some(activity.to_string()),
    })
}

pub fn foreground_observation(substrate: AndroidSubstrate) -> AndroidObservation {
    foreground_observation_from_output(substrate, run_command_output(&foreground_focus_command()))
}

pub fn foreground_observation_from_result(
    substrate: AndroidSubstrate,
    command_result: Result<String, String>,
) -> AndroidObservation {
    match command_result
        .map_err(ForegroundObservationFailure::CommandFailed)
        .and_then(|summary| {
            if summary.is_empty() {
                Err(ForegroundObservationFailure::EmptyOutput)
            } else {
                Ok(summary)
            }
        }) {
        Ok(summary) => match parse_foreground_target(&summary) {
            Some(target) => AndroidObservation {
                substrate,
                event: AndroidEvent::ForegroundAppChanged {
                    package_name: target.package_name,
                    activity_name: target.activity_name,
                },
            },
            None => unavailable_foreground(
                substrate,
                AndroidForegroundUnavailableReason::ParseFailed,
                Some(summary),
            ),
        },
        Err(ForegroundObservationFailure::CommandFailed(error)) => unavailable_foreground(
            substrate,
            AndroidForegroundUnavailableReason::CommandFailed,
            Some(error),
        ),
        Err(ForegroundObservationFailure::EmptyOutput) => unavailable_foreground(
            substrate,
            AndroidForegroundUnavailableReason::EmptyOutput,
            Some(String::new()),
        ),
    }
}

pub fn foreground_observation_from_output(
    substrate: AndroidSubstrate,
    command_result: Result<CommandOutput, String>,
) -> AndroidObservation {
    match command_result {
        Ok(output) if !output.success => unavailable_foreground(
            substrate,
            AndroidForegroundUnavailableReason::CommandFailed,
            Some(output.failure_summary()),
        ),
        Ok(output) if output.stdout.trim().is_empty() => unavailable_foreground(
            substrate,
            AndroidForegroundUnavailableReason::EmptyOutput,
            Some(output.stderr.trim().to_string()),
        ),
        Ok(output) => match parse_foreground_target_from_dumpsys(&output.stdout) {
            Some(target) => AndroidObservation {
                substrate,
                event: AndroidEvent::ForegroundAppChanged {
                    package_name: target.package_name,
                    activity_name: target.activity_name,
                },
            },
            None => unavailable_foreground(
                substrate,
                AndroidForegroundUnavailableReason::ParseFailed,
                Some(output.combined_source()),
            ),
        },
        Err(error) => unavailable_foreground(
            substrate,
            AndroidForegroundUnavailableReason::CommandFailed,
            Some(error),
        ),
    }
}

pub fn parse_foreground_target_from_dumpsys(output: &str) -> Option<ForegroundTarget> {
    output
        .lines()
        .filter(|line| line.contains("mCurrentFocus") || line.contains("mFocusedApp"))
        .find_map(parse_foreground_target)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ForegroundObservationFailure {
    CommandFailed(String),
    EmptyOutput,
}

pub fn foreground_focus_command() -> [&'static str; 2] {
    ["dumpsys", "window"]
}

pub fn run_command(argv: &[&str]) -> Result<String, String> {
    let output = run_command_output(argv)?;
    if output.success {
        Ok(output.stdout.trim().to_string())
    } else {
        Err(output.failure_summary())
    }
}

pub fn run_command_output(argv: &[&str]) -> Result<CommandOutput, String> {
    let Some((program, args)) = argv.split_first() else {
        return Err("empty command".to_string());
    };

    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("failed to run {program}: {error}"))?;

    Ok(CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        status: output.status.to_string(),
        success: output.status.success(),
    })
}

impl CommandOutput {
    fn failure_summary(&self) -> String {
        let stderr = self.stderr.trim();
        let stdout = self.stdout.trim();

        match (stderr.is_empty(), stdout.is_empty()) {
            (false, false) => format!("{}; stderr={stderr}; stdout={stdout}", self.status),
            (false, true) => format!("{}; stderr={stderr}", self.status),
            (true, false) => format!("{}; stdout={stdout}", self.status),
            (true, true) => self.status.clone(),
        }
    }

    fn combined_source(&self) -> String {
        let stderr = self.stderr.trim();
        let stdout = self.stdout.trim();

        if stderr.is_empty() {
            stdout.to_string()
        } else {
            format!("stdout={stdout}; stderr={stderr}")
        }
    }
}

fn unavailable_foreground(
    substrate: AndroidSubstrate,
    reason: AndroidForegroundUnavailableReason,
    raw_source: Option<String>,
) -> AndroidObservation {
    AndroidObservation {
        substrate,
        event: AndroidEvent::ForegroundObservationUnavailable {
            target: "foreground".to_string(),
            reason,
            raw_source,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_events_are_observations_not_task_conclusions() {
        let observation = AndroidObservation {
            substrate: AndroidSubstrate::AospPlatform,
            event: AndroidEvent::ForegroundAppChanged {
                package_name: "com.android.chrome".to_string(),
                activity_name: Some("com.google.android.apps.chrome.Main".to_string()),
            },
        };

        assert!(matches!(
            observation.event,
            AndroidEvent::ForegroundAppChanged { .. }
        ));
        assert_eq!(observation.substrate, AndroidSubstrate::AospPlatform);
    }

    #[test]
    fn adapter_accepts_typed_runtime_commands() {
        let request = AndroidActionRequest {
            substrate: AndroidSubstrate::AospPlatform,
            command: AndroidCommand::AcquireForeground {
                target: "subscription-cancel-flow".to_string(),
            },
        };

        assert!(matches!(
            request.command,
            AndroidCommand::AcquireForeground { .. }
        ));
        assert_eq!(request.substrate, AndroidSubstrate::AospPlatform);
    }

    #[test]
    fn rooted_stock_substrate_is_explicitly_recon() {
        let observation = AndroidObservation {
            substrate: AndroidSubstrate::ReconRootedStock,
            event: AndroidEvent::NetworkAvailabilityChanged { available: true },
        };

        assert_eq!(observation.substrate, AndroidSubstrate::ReconRootedStock);
    }

    #[test]
    fn observation_json_preserves_substrate_and_event_shape() {
        let observation = AndroidObservation {
            substrate: AndroidSubstrate::ReconRootedStock,
            event: AndroidEvent::ForegroundObservationUnavailable {
                target: "foreground".to_string(),
                reason: AndroidForegroundUnavailableReason::ParseFailed,
                raw_source: Some("bad focus line".to_string()),
            },
        };

        let json = serde_json::to_value(&observation).expect("serialize observation");

        assert_eq!(json["substrate"], "ReconRootedStock");
        assert_eq!(
            json["event"]["ForegroundObservationUnavailable"]["target"],
            "foreground"
        );
        assert_eq!(
            json["event"]["ForegroundObservationUnavailable"]["reason"],
            "ParseFailed"
        );
    }

    #[test]
    fn parses_current_focus_component() {
        let target = parse_foreground_target(
            "mCurrentFocus=Window{39760e7 u0 com.google.android.apps.nexuslauncher/com.google.android.apps.nexuslauncher.NexusLauncherActivity}",
        )
        .expect("foreground target");

        assert_eq!(target.package_name, "com.google.android.apps.nexuslauncher");
        assert_eq!(
            target.activity_name.as_deref(),
            Some("com.google.android.apps.nexuslauncher.NexusLauncherActivity")
        );
    }

    #[test]
    fn rejects_unstructured_foreground_lines() {
        assert!(parse_foreground_target("nothing useful here").is_none());
    }

    #[test]
    fn foreground_focus_command_runs_dumpsys_directly() {
        assert_eq!(foreground_focus_command(), ["dumpsys", "window"]);
    }

    #[test]
    fn parses_foreground_target_from_full_dumpsys_output() {
        let target = parse_foreground_target_from_dumpsys(
            "Window dump:\n  mObscuringWindow=null\n  mCurrentFocus=Window{39760e7 u0 com.android.settings/.Settings}\n",
        )
        .expect("foreground target");

        assert_eq!(target.package_name, "com.android.settings");
        assert_eq!(target.activity_name.as_deref(), Some(".Settings"));
    }

    #[test]
    fn foreground_observation_preserves_command_failure_reason() {
        let observation = foreground_observation_from_result(
            AndroidSubstrate::ReconRootedStock,
            Err("dumpsys denied".to_string()),
        );

        assert!(matches!(
            observation.event,
            AndroidEvent::ForegroundObservationUnavailable {
                reason: AndroidForegroundUnavailableReason::CommandFailed,
                raw_source: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn foreground_observation_preserves_dumpsys_status_and_stderr() {
        let observation = foreground_observation_from_output(
            AndroidSubstrate::ReconRootedStock,
            Ok(CommandOutput {
                stdout: "partial stdout".to_string(),
                stderr: "permission denied".to_string(),
                status: "exit status: 20".to_string(),
                success: false,
            }),
        );

        let AndroidEvent::ForegroundObservationUnavailable {
            reason, raw_source, ..
        } = observation.event
        else {
            panic!("expected unavailable foreground observation");
        };

        assert_eq!(reason, AndroidForegroundUnavailableReason::CommandFailed);
        let raw_source = raw_source.expect("raw source");
        assert!(raw_source.contains("exit status: 20"));
        assert!(raw_source.contains("permission denied"));
        assert!(raw_source.contains("partial stdout"));
    }

    #[test]
    fn foreground_observation_preserves_empty_output_reason() {
        let observation = foreground_observation_from_result(
            AndroidSubstrate::ReconRootedStock,
            Ok(String::new()),
        );

        assert!(matches!(
            observation.event,
            AndroidEvent::ForegroundObservationUnavailable {
                reason: AndroidForegroundUnavailableReason::EmptyOutput,
                raw_source: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn foreground_observation_preserves_parse_failure_source() {
        let observation = foreground_observation_from_result(
            AndroidSubstrate::ReconRootedStock,
            Ok("mCurrentFocus=Window{not a component}".to_string()),
        );

        assert!(matches!(
            observation.event,
            AndroidEvent::ForegroundObservationUnavailable {
                reason: AndroidForegroundUnavailableReason::ParseFailed,
                raw_source: Some(_),
                ..
            }
        ));
    }
}
