use std::error::Error;
use std::process::ExitCode;

use fawx_android_adapter::{
    AndroidEvent, AndroidObservation, AndroidSubstrate, foreground_observation, run_command,
};
use serde::Serialize;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let report = ProbeReport {
        substrate: AndroidSubstrate::ReconRootedStock,
        observations: vec![
            command_observation("whoami", &["whoami"]),
            command_observation("id", &["id"]),
            command_observation("uname", &["uname", "-a"]),
            root_observation(),
            foreground_probe_observation(),
        ],
    };

    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

#[derive(Debug, Serialize)]
struct ProbeReport {
    substrate: AndroidSubstrate,
    observations: Vec<ProbeObservation>,
}

#[derive(Debug, Serialize)]
struct ProbeObservation {
    name: String,
    ok: bool,
    summary: String,
    android_observation: Option<AndroidObservation>,
}

fn command_observation(name: &str, argv: &[&str]) -> ProbeObservation {
    match run_command(argv) {
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
    let rooted = run_command(&["su", "-c", "id"]).is_ok();

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

fn foreground_probe_observation() -> ProbeObservation {
    let observation = foreground_observation(AndroidSubstrate::ReconRootedStock);

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
