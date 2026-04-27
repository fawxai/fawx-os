use std::error::Error;
use std::fs;
use std::process::ExitCode;

use fawx_android_adapter::{
    AndroidCapabilityStatusEntry, AndroidEvent, AndroidObservation, AndroidReconCommand,
    AndroidSubstrate, android_capability_statuses,
    aosp_foreground_observation_from_platform_event_json,
    aosp_platform_adapter_unavailable_observation, foreground_observation, run_recon_command,
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

const USAGE: &str = "usage: fawx-android-probe [--substrate recon-rooted-stock|aosp-platform] [--aosp-foreground-event-file PATH]";

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
    },
    Help,
}

fn run() -> Result<ProbeExit, Box<dyn Error>> {
    let ProbeArgs::Run {
        substrate,
        aosp_foreground_event_file,
    } = parse_probe_args(std::env::args().skip(1))?
    else {
        return Ok(ProbeExit::Help);
    };
    let report = ProbeReport {
        substrate,
        capability_statuses: android_capability_statuses(substrate),
        observations: probe_observations(substrate, aosp_foreground_event_file.as_deref()),
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
            value if value.starts_with("--") => {
                return Err(format!("unexpected probe option: {value}").into());
            }
            value => {
                substrate = value.parse()?;
            }
        }
    }

    if aosp_foreground_event_file.is_some() && substrate != AndroidSubstrate::AospPlatform {
        return Err(
            "--aosp-foreground-event-file is only valid with --substrate aosp-platform".into(),
        );
    }

    Ok(ProbeArgs::Run {
        substrate,
        aosp_foreground_event_file,
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
            let event_observation =
                aosp_foreground_event_observation_from_file(aosp_foreground_event_file);
            vec![
                aosp_adapter_probe_observation(aosp_foreground_event_file, &event_observation),
                foreground_probe_observation(substrate, event_observation),
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
                aosp_foreground_event_file: None
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
                aosp_foreground_event_file: None
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
                aosp_foreground_event_file: Some("/run/fawx/foreground.json".to_string())
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
        let observations = probe_observations(AndroidSubstrate::AospPlatform, None);

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
}
