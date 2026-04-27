use std::error::Error;
use std::process::ExitCode;

use fawx_android_adapter::{
    AndroidCapabilityStatusEntry, AndroidEvent, AndroidObservation, AndroidReconCommand,
    AndroidSubstrate, android_capability_statuses, aosp_platform_adapter_unavailable_observation,
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

const USAGE: &str = "usage: fawx-android-probe [--substrate recon-rooted-stock|aosp-platform]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeExit {
    Success,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeArgs {
    Run { substrate: AndroidSubstrate },
    Help,
}

fn run() -> Result<ProbeExit, Box<dyn Error>> {
    let ProbeArgs::Run { substrate } = parse_probe_args(std::env::args().skip(1))? else {
        return Ok(ProbeExit::Help);
    };
    let report = ProbeReport {
        substrate,
        capability_statuses: android_capability_statuses(substrate),
        observations: probe_observations(substrate),
    };

    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(ProbeExit::Success)
}

fn parse_probe_args<I>(mut args: I) -> Result<ProbeArgs, Box<dyn Error>>
where
    I: Iterator<Item = String>,
{
    let Some(first) = args.next() else {
        return Ok(ProbeArgs::Run {
            substrate: AndroidSubstrate::ReconRootedStock,
        });
    };

    let value = match first.as_str() {
        "--substrate" => args.next().ok_or("missing value after --substrate")?,
        "--help" | "-h" => return Ok(ProbeArgs::Help),
        value => value.to_string(),
    };

    if args.next().is_some() {
        return Err("unexpected extra arguments".into());
    }

    Ok(ProbeArgs::Run {
        substrate: value.parse()?,
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

fn probe_observations(substrate: AndroidSubstrate) -> Vec<ProbeObservation> {
    match substrate {
        AndroidSubstrate::ReconRootedStock => vec![
            command_observation("whoami", AndroidReconCommand::Whoami),
            command_observation("id", AndroidReconCommand::Id),
            command_observation("uname", AndroidReconCommand::Uname),
            root_observation(),
            foreground_probe_observation(substrate),
        ],
        AndroidSubstrate::AospPlatform => vec![
            aosp_adapter_probe_observation(),
            foreground_probe_observation(substrate),
        ],
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

fn aosp_adapter_probe_observation() -> ProbeObservation {
    ProbeObservation {
        name: "aosp-platform-adapter".to_string(),
        ok: false,
        summary: "AOSP platform adapter is not connected in this terminal binary".to_string(),
        android_observation: None,
    }
}

fn foreground_probe_observation(substrate: AndroidSubstrate) -> ProbeObservation {
    let observation = match substrate {
        AndroidSubstrate::ReconRootedStock => foreground_observation(substrate),
        AndroidSubstrate::AospPlatform => {
            aosp_platform_adapter_unavailable_observation("foreground")
        }
    };

    match observation.event.clone() {
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
                substrate: AndroidSubstrate::ReconRootedStock
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
                substrate: AndroidSubstrate::AospPlatform
            }
        );
    }

    #[test]
    fn parses_help_as_successful_probe_exit() {
        let args = parse_probe_args(["--help"].into_iter().map(str::to_string))
            .expect("help should parse");

        assert_eq!(args, ProbeArgs::Help);
    }

    #[test]
    fn aosp_probe_uses_unavailable_platform_observation_until_adapter_exists() {
        let observations = probe_observations(AndroidSubstrate::AospPlatform);

        assert!(observations.iter().any(|observation| {
            observation.name == "aosp-platform-adapter"
                && !observation.ok
                && observation.summary.contains("not connected")
        }));
        assert!(observations.iter().any(|observation| {
            matches!(
                observation.android_observation.as_ref().map(|value| &value.event),
                Some(AndroidEvent::ForegroundObservationUnavailable {
                    reason: fawx_android_adapter::AndroidForegroundUnavailableReason::AdapterUnavailable,
                    ..
                })
            )
        }));
    }
}
