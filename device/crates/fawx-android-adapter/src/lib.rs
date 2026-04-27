//! Android substrate adapter for Fawx OS.
//!
//! This crate is intentionally named as an adapter, not a core runtime. The
//! goal is to keep Android-specific bindings thin so they can be replaced over
//! time without rewriting the kernel or harness.

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;
use std::fmt;
use std::process::Command;
use std::str::FromStr;

/// Which Android-facing substrate produced an observation or accepts a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AndroidSubstrate {
    /// Temporary stock/rooted Android probe. This must not define core runtime
    /// semantics and should remain replaceable by the AOSP adapter.
    ReconRootedStock,
    /// The real prototype target: AOSP-level platform control.
    AospPlatform,
}

impl AndroidSubstrate {
    pub fn as_wire_name(self) -> &'static str {
        match self {
            Self::ReconRootedStock => "ReconRootedStock",
            Self::AospPlatform => "AospPlatform",
        }
    }

    pub fn parse_name(name: &str) -> Option<Self> {
        let normalized = name
            .trim()
            .chars()
            .filter(|ch| *ch != '-' && *ch != '_' && !ch.is_whitespace())
            .flat_map(char::to_lowercase)
            .collect::<String>();

        match normalized.as_str() {
            "recon" | "rootedstock" | "reconrootedstock" => Some(Self::ReconRootedStock),
            "aosp" | "aospplatform" | "system" | "platform" => Some(Self::AospPlatform),
            _ => None,
        }
    }
}

impl fmt::Display for AndroidSubstrate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_wire_name())
    }
}

impl FromStr for AndroidSubstrate {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_name(value).ok_or_else(|| {
            format!(
                "unknown Android substrate '{value}'; expected recon-rooted-stock or aosp-platform"
            )
        })
    }
}

/// The durable Android capability categories the runtime can reason about.
/// These are adapter facts, not permission grants. The kernel still decides
/// whether a task may use an available capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AndroidCapability {
    ObserveForegroundApp,
    LaunchApp,
    ControlForegroundApp,
    ReadNotifications,
    PostNotifications,
    PlaceCall,
    SendMessage,
    ReadSharedStorage,
    WriteSharedStorage,
    NetworkAccess,
    BackgroundExecution,
    InstallPackages,
    SystemSettings,
    RootShell,
}

impl AndroidCapability {
    pub const ALL: &'static [Self] = &[
        Self::ObserveForegroundApp,
        Self::LaunchApp,
        Self::ControlForegroundApp,
        Self::ReadNotifications,
        Self::PostNotifications,
        Self::PlaceCall,
        Self::SendMessage,
        Self::ReadSharedStorage,
        Self::WriteSharedStorage,
        Self::NetworkAccess,
        Self::BackgroundExecution,
        Self::InstallPackages,
        Self::SystemSettings,
        Self::RootShell,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AndroidCapabilityStatus {
    Available,
    Limited,
    RequiresAospPrivilege,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AndroidCapabilityEntry {
    pub capability: AndroidCapability,
    pub rooted_stock: AndroidCapabilityStatus,
    pub aosp_platform: AndroidCapabilityStatus,
    pub note: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AndroidCapabilityStatusEntry {
    pub capability: AndroidCapability,
    pub status: AndroidCapabilityStatus,
    pub note: &'static str,
}

pub const ANDROID_CAPABILITY_MAP: &[AndroidCapabilityEntry] = &[
    AndroidCapabilityEntry {
        capability: AndroidCapability::ObserveForegroundApp,
        rooted_stock: AndroidCapabilityStatus::Available,
        aosp_platform: AndroidCapabilityStatus::Available,
        note: "Recon can observe foreground focus through dumpsys; AOSP should expose a stable platform event.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::LaunchApp,
        rooted_stock: AndroidCapabilityStatus::Limited,
        aosp_platform: AndroidCapabilityStatus::Available,
        note: "Recon can use activity-manager style shell commands, but reliable launch/resume belongs in a privileged adapter.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::ControlForegroundApp,
        rooted_stock: AndroidCapabilityStatus::Limited,
        aosp_platform: AndroidCapabilityStatus::Available,
        note: "Recon can probe input/UI automation, but durable control needs accessibility, shell, or framework integration.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::ReadNotifications,
        rooted_stock: AndroidCapabilityStatus::Limited,
        aosp_platform: AndroidCapabilityStatus::Available,
        note: "Recon may inspect notification state opportunistically; production needs a notification listener/system hook.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::PostNotifications,
        rooted_stock: AndroidCapabilityStatus::RequiresAospPrivilege,
        aosp_platform: AndroidCapabilityStatus::Available,
        note: "Posting user-visible OS notifications should be a platform-owned capability, not a shell trick.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::PlaceCall,
        rooted_stock: AndroidCapabilityStatus::RequiresAospPrivilege,
        aosp_platform: AndroidCapabilityStatus::Available,
        note: "Telephony side effects require explicit user/kernel authority and privileged platform integration.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::SendMessage,
        rooted_stock: AndroidCapabilityStatus::RequiresAospPrivilege,
        aosp_platform: AndroidCapabilityStatus::Available,
        note: "Messaging side effects require explicit user/kernel authority and privileged platform integration.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::ReadSharedStorage,
        rooted_stock: AndroidCapabilityStatus::Limited,
        aosp_platform: AndroidCapabilityStatus::Available,
        note: "Recon can read shell-accessible paths; AOSP should expose scoped storage through typed policy.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::WriteSharedStorage,
        rooted_stock: AndroidCapabilityStatus::Limited,
        aosp_platform: AndroidCapabilityStatus::Available,
        note: "Recon writes are path-limited and risky; AOSP should mediate writes through grants.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::NetworkAccess,
        rooted_stock: AndroidCapabilityStatus::Available,
        aosp_platform: AndroidCapabilityStatus::Available,
        note: "Network access is available but must still be grant-gated by task policy.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::BackgroundExecution,
        rooted_stock: AndroidCapabilityStatus::Limited,
        aosp_platform: AndroidCapabilityStatus::Available,
        note: "Recon can run detached shell processes; production requires supervised platform services.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::InstallPackages,
        rooted_stock: AndroidCapabilityStatus::Limited,
        aosp_platform: AndroidCapabilityStatus::Available,
        note: "Recon package install is device-policy dependent; production requires package-manager authority.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::SystemSettings,
        rooted_stock: AndroidCapabilityStatus::Limited,
        aosp_platform: AndroidCapabilityStatus::Available,
        note: "Recon can inspect or poke some settings; production needs typed framework APIs.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::RootShell,
        rooted_stock: AndroidCapabilityStatus::Limited,
        aosp_platform: AndroidCapabilityStatus::Unavailable,
        note: "Root shell is a recon escape hatch, not a production OS primitive.",
    },
];

pub fn android_capability_map() -> &'static [AndroidCapabilityEntry] {
    ANDROID_CAPABILITY_MAP
}

pub fn android_capability_entry(
    capability: AndroidCapability,
) -> Option<&'static AndroidCapabilityEntry> {
    ANDROID_CAPABILITY_MAP
        .iter()
        .find(|entry| entry.capability == capability)
}

pub fn android_capability_status(
    substrate: AndroidSubstrate,
    capability: AndroidCapability,
) -> Option<AndroidCapabilityStatus> {
    let entry = android_capability_entry(capability)?;
    Some(match substrate {
        AndroidSubstrate::ReconRootedStock => entry.rooted_stock,
        AndroidSubstrate::AospPlatform => entry.aosp_platform,
    })
}

pub fn android_capability_statuses(
    substrate: AndroidSubstrate,
) -> Vec<AndroidCapabilityStatusEntry> {
    ANDROID_CAPABILITY_MAP
        .iter()
        .map(|entry| AndroidCapabilityStatusEntry {
            capability: entry.capability,
            status: match substrate {
                AndroidSubstrate::ReconRootedStock => entry.rooted_stock,
                AndroidSubstrate::AospPlatform => entry.aosp_platform,
            },
            note: entry.note,
        })
        .collect()
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
    AdapterUnavailable,
}

/// Commands flowing downward from the runtime into the Android adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AndroidCommand {
    AcquireForeground {
        target: String,
    },
    ReleaseForeground,
    ObserveNotifications {
        source: String,
    },
    QueryForegroundState,
    ResumeAppSurface {
        package_name: String,
    },
    PerformRootedAction {
        capability: AndroidCapability,
        target: String,
    },
}

/// A typed Android observation with explicit substrate provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AndroidObservation {
    substrate: AndroidSubstrate,
    event: AndroidEvent,
    #[serde(skip_serializing_if = "Option::is_none")]
    provenance: Option<AndroidObservationProvenance>,
}

impl AndroidObservation {
    pub fn recon_rooted_stock(event: AndroidEvent) -> Self {
        Self {
            substrate: AndroidSubstrate::ReconRootedStock,
            event,
            provenance: None,
        }
    }

    pub fn substrate(&self) -> AndroidSubstrate {
        self.substrate
    }

    pub fn event(&self) -> &AndroidEvent {
        &self.event
    }

    pub fn provenance(&self) -> Option<&AndroidObservationProvenance> {
        self.provenance.as_ref()
    }
}

impl<'de> Deserialize<'de> for AndroidObservation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WireAndroidObservation {
            substrate: AndroidSubstrate,
            event: AndroidEvent,
            provenance: Option<AndroidObservationProvenance>,
        }

        let wire = WireAndroidObservation::deserialize(deserializer)?;
        if let (AndroidSubstrate::AospPlatform, AndroidEvent::ForegroundAppChanged { .. }) =
            (&wire.substrate, &wire.event)
        {
            let Some(AndroidObservationProvenance::AospPlatformEvent { source }) =
                wire.provenance.as_ref()
            else {
                return Err(serde::de::Error::custom(
                    "AospPlatform foreground observations require AospPlatformEvent provenance",
                ));
            };
            validate_aosp_platform_event_source(source).map_err(serde::de::Error::custom)?;
        }

        if matches!(
            (&wire.substrate, &wire.event),
            (
                AndroidSubstrate::AospPlatform,
                AndroidEvent::ForegroundObservationUnavailable { .. }
            )
        ) && matches!(
            wire.provenance,
            Some(AndroidObservationProvenance::AospPlatformEvent { .. })
        ) {
            return Err(serde::de::Error::custom(
                "AospPlatformEvent provenance is only valid for AOSP platform foreground success",
            ));
        }

        Ok(Self {
            substrate: wire.substrate,
            event: wire.event,
            provenance: wire.provenance,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AndroidObservationProvenance {
    AospPlatformEvent { source: AospPlatformEventSource },
}

/// Provenance for an AOSP platform event.
///
/// This intentionally excludes shell and dumpsys-style sources. AOSP platform
/// observations may only be created from a privileged adapter surface that can
/// be audited independently of recon probes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AospPlatformEventSource {
    pub service_name: String,
    pub event_id: String,
}

pub const AOSP_FOREGROUND_OBSERVER_SERVICE: &str = "fawx-system-foreground-observer";

/// Foreground state emitted by a real AOSP/system adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AospForegroundEvent {
    pub package_name: String,
    pub activity_name: Option<String>,
    pub source: AospPlatformEventSource,
}

/// A typed Android command with explicit target substrate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AndroidActionRequest {
    pub substrate: AndroidSubstrate,
    pub command: AndroidCommand,
}

impl AndroidActionRequest {
    pub fn required_capability(&self) -> Option<AndroidCapability> {
        self.command.required_capability()
    }

    pub fn capability_status(&self) -> Option<AndroidCapabilityStatus> {
        android_capability_status(self.substrate, self.required_capability()?)
    }
}

impl AndroidCommand {
    pub fn required_capability(&self) -> Option<AndroidCapability> {
        match self {
            Self::AcquireForeground { .. } | Self::ResumeAppSurface { .. } => {
                Some(AndroidCapability::LaunchApp)
            }
            Self::ReleaseForeground | Self::QueryForegroundState => {
                Some(AndroidCapability::ObserveForegroundApp)
            }
            Self::ObserveNotifications { .. } => Some(AndroidCapability::ReadNotifications),
            Self::PerformRootedAction { capability, .. } => Some(*capability),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub status: String,
    pub success: bool,
}

/// The device-local model surfaces Fawx OS knows how to reason about.
///
/// These are provider probes, not execution permissions. A present provider may
/// still be unusable from this process if Android exposes it only through a
/// framework SDK, Play services binding, or app-private surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocalModelProviderKind {
    AicoreGeminiNano,
    GeminiApp,
    UnknownGoogleAiSurface,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocalModelProviderStatus {
    /// A known package exists, but this terminal-only prototype has not proven
    /// a stable public inference API.
    PresentButNoPublicTerminalApi,
    /// A known package was not found on the current Android image.
    Unavailable,
    /// The package-manager probe failed, so provider availability is unknown.
    Indeterminate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalModelProviderProbe {
    pub kind: LocalModelProviderKind,
    pub status: LocalModelProviderStatus,
    pub package_name: Option<String>,
    pub evidence: Vec<String>,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalModelProbeReport {
    pub substrate: AndroidSubstrate,
    pub providers: Vec<LocalModelProviderProbe>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AndroidReconCommand {
    Whoami,
    Id,
    Uname,
    RootCheck,
}

impl AndroidReconCommand {
    fn argv(self) -> &'static [&'static str] {
        match self {
            Self::Whoami => &["whoami"],
            Self::Id => &["id"],
            Self::Uname => &["uname", "-a"],
            Self::RootCheck => &["su", "-c", "id"],
        }
    }
}

pub fn run_recon_command(command: AndroidReconCommand) -> Result<String, String> {
    let output = run_command_output(command.argv())?;
    if output.success {
        Ok(output.stdout.trim().to_string())
    } else {
        Err(output.failure_summary())
    }
}

pub fn local_model_probe(substrate: AndroidSubstrate) -> LocalModelProbeReport {
    local_model_probe_from_package_result(substrate, installed_package_names())
}

fn local_model_probe_from_package_result(
    substrate: AndroidSubstrate,
    packages: Result<BTreeSet<String>, String>,
) -> LocalModelProbeReport {
    let packages = match packages {
        Ok(packages) => packages,
        Err(error) => {
            return LocalModelProbeReport {
                substrate,
                providers: indeterminate_local_model_probes(error),
            };
        }
    };

    LocalModelProbeReport {
        substrate,
        providers: vec![
            probe_known_local_model_package(
                LocalModelProviderKind::AicoreGeminiNano,
                &packages,
                &["com.google.android.aicore"],
                "AICore/Gemini Nano is the preferred local-model surface if Android exposes a stable API.",
            ),
            probe_known_local_model_package(
                LocalModelProviderKind::GeminiApp,
                &packages,
                &[
                    "com.google.android.apps.bard",
                    "com.google.android.apps.gemini",
                    "com.google.android.googlequicksearchbox",
                ],
                "The Gemini app/Search package may be present, but app-private surfaces are not a Fawx OS control-plane contract.",
            ),
            probe_google_ai_package_family(&packages),
        ],
    }
}

fn indeterminate_local_model_probes(error: String) -> Vec<LocalModelProviderProbe> {
    [
        (
            LocalModelProviderKind::AicoreGeminiNano,
            "Could not inspect package manager state; AICore/Gemini Nano availability is unknown.",
        ),
        (
            LocalModelProviderKind::GeminiApp,
            "Could not inspect package manager state; Gemini app availability is unknown.",
        ),
        (
            LocalModelProviderKind::UnknownGoogleAiSurface,
            "Could not inspect package manager state; Google AI package-family availability is unknown.",
        ),
    ]
    .into_iter()
    .map(|(kind, note)| LocalModelProviderProbe {
        kind,
        status: LocalModelProviderStatus::Indeterminate,
        package_name: None,
        evidence: vec![format!("pm list packages failed: {error}")],
        note: note.to_string(),
    })
    .collect()
}

fn installed_package_names() -> Result<BTreeSet<String>, String> {
    let output = run_command_output(&["pm", "list", "packages"])?;
    if !output.success {
        return Err(output.failure_summary());
    }
    Ok(parse_pm_list_packages(&output.stdout))
}

fn parse_pm_list_packages(output: &str) -> BTreeSet<String> {
    output
        .lines()
        .filter_map(|line| line.trim().strip_prefix("package:"))
        .map(str::trim)
        .filter(|package| !package.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn probe_known_local_model_package(
    kind: LocalModelProviderKind,
    packages: &BTreeSet<String>,
    known_packages: &[&str],
    note: &str,
) -> LocalModelProviderProbe {
    match known_packages
        .iter()
        .find(|package| packages.contains(**package))
    {
        Some(package) => LocalModelProviderProbe {
            kind,
            status: LocalModelProviderStatus::PresentButNoPublicTerminalApi,
            package_name: Some((*package).to_string()),
            evidence: vec![format!("installed package: {package}")],
            note: note.to_string(),
        },
        None => LocalModelProviderProbe {
            kind,
            status: LocalModelProviderStatus::Unavailable,
            package_name: None,
            evidence: Vec::new(),
            note: note.to_string(),
        },
    }
}

fn probe_google_ai_package_family(packages: &BTreeSet<String>) -> LocalModelProviderProbe {
    let evidence = packages
        .iter()
        .filter(|package| {
            let lower = package.to_ascii_lowercase();
            lower.contains("google")
                && (lower.contains("ai") || lower.contains("gemini") || lower.contains("aicore"))
        })
        .take(12)
        .map(|package| format!("installed package: {package}"))
        .collect::<Vec<_>>();
    let status = if evidence.is_empty() {
        LocalModelProviderStatus::Unavailable
    } else {
        LocalModelProviderStatus::PresentButNoPublicTerminalApi
    };

    LocalModelProviderProbe {
        kind: LocalModelProviderKind::UnknownGoogleAiSurface,
        status,
        package_name: None,
        evidence,
        note: "Informational package-family probe. It must not be used as an inference API without a typed adapter contract.".to_string(),
    }
}

pub fn execute_android_action_request(
    request: &AndroidActionRequest,
) -> Result<CommandOutput, String> {
    ensure_supported_action_request(request)?;
    match &request.command {
        AndroidCommand::ResumeAppSurface { package_name } => {
            run_owned_command_output(resume_app_surface_command(package_name)?)
        }
        command => Err(format!(
            "android command {command:?} is not executable by rooted-stock recon"
        )),
    }
}

fn ensure_supported_action_request(request: &AndroidActionRequest) -> Result<(), String> {
    if request.substrate != AndroidSubstrate::ReconRootedStock {
        return Err(format!(
            "android substrate {:?} is not executable by rooted-stock recon",
            request.substrate
        ));
    }

    match request.capability_status() {
        Some(AndroidCapabilityStatus::Available | AndroidCapabilityStatus::Limited) => Ok(()),
        Some(status) => Err(format!(
            "android capability {:?} has unsupported rooted-stock status {status:?}",
            request.required_capability()
        )),
        None => Ok(()),
    }
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
    match substrate {
        AndroidSubstrate::ReconRootedStock => foreground_observation_from_output(
            substrate,
            run_command_output(&foreground_focus_command()),
        ),
        AndroidSubstrate::AospPlatform => {
            aosp_platform_adapter_unavailable_observation("foreground")
        }
    }
}

pub fn aosp_platform_adapter_unavailable_observation(target: &str) -> AndroidObservation {
    AndroidObservation {
        substrate: AndroidSubstrate::AospPlatform,
        event: AndroidEvent::ForegroundObservationUnavailable {
            target: target.to_string(),
            reason: AndroidForegroundUnavailableReason::AdapterUnavailable,
            raw_source: Some(
                "AOSP platform adapter is not connected; shell recon evidence must stay on ReconRootedStock"
                    .to_string(),
            ),
        },
        provenance: None,
    }
}

pub fn aosp_foreground_observation_from_platform_event(
    event: AospForegroundEvent,
) -> Result<AndroidObservation, String> {
    validate_aosp_foreground_event(&event)?;
    let source = event.source.clone();
    Ok(AndroidObservation {
        substrate: AndroidSubstrate::AospPlatform,
        event: AndroidEvent::ForegroundAppChanged {
            package_name: event.package_name,
            activity_name: event.activity_name,
        },
        provenance: Some(AndroidObservationProvenance::AospPlatformEvent { source }),
    })
}

pub fn aosp_foreground_observation_from_platform_event_json(
    event_json: &str,
) -> Result<AndroidObservation, String> {
    let event = serde_json::from_str::<AospForegroundEvent>(event_json)
        .map_err(|error| format!("failed to decode AOSP foreground event: {error}"))?;
    aosp_foreground_observation_from_platform_event(event)
}

fn validate_aosp_foreground_event(event: &AospForegroundEvent) -> Result<(), String> {
    validate_nonempty_token("AOSP foreground package", &event.package_name)?;
    validate_no_surrounding_whitespace("AOSP foreground package", &event.package_name)?;
    if let Some(activity_name) = &event.activity_name {
        validate_nonempty_token("AOSP foreground activity", activity_name)?;
        validate_no_surrounding_whitespace("AOSP foreground activity", activity_name)?;
    }
    validate_aosp_platform_event_source(&event.source)?;
    Ok(())
}

fn validate_aosp_platform_event_source(source: &AospPlatformEventSource) -> Result<(), String> {
    validate_nonempty_token("AOSP platform service name", &source.service_name)?;
    validate_no_surrounding_whitespace("AOSP platform service name", &source.service_name)?;
    if source.service_name != AOSP_FOREGROUND_OBSERVER_SERVICE {
        return Err(format!(
            "AOSP platform service name must be {AOSP_FOREGROUND_OBSERVER_SERVICE}"
        ));
    }
    validate_nonempty_token("AOSP platform event id", &source.event_id)?;
    validate_no_surrounding_whitespace("AOSP platform event id", &source.event_id)?;
    Ok(())
}

fn validate_nonempty_token(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{label} must not be empty"))
    } else {
        Ok(())
    }
}

fn validate_no_surrounding_whitespace(label: &str, value: &str) -> Result<(), String> {
    if value.trim() == value {
        Ok(())
    } else {
        Err(format!("{label} must not contain surrounding whitespace"))
    }
}

pub fn foreground_observation_from_result(
    substrate: AndroidSubstrate,
    command_result: Result<String, String>,
) -> AndroidObservation {
    if substrate == AndroidSubstrate::AospPlatform {
        return aosp_platform_adapter_unavailable_observation("foreground");
    }

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
                provenance: None,
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
    if substrate == AndroidSubstrate::AospPlatform {
        return aosp_platform_adapter_unavailable_observation("foreground");
    }

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
                provenance: None,
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

pub fn resume_app_surface_command(package_name: &str) -> Result<Vec<String>, String> {
    let original_package_name = package_name;
    let package_name = original_package_name.trim();
    if package_name.is_empty() {
        return Err("cannot resume empty Android package".to_string());
    }
    if package_name != original_package_name {
        return Err("Android package must not contain surrounding whitespace".to_string());
    }

    Ok(vec![
        "monkey".to_string(),
        "-p".to_string(),
        package_name.to_string(),
        "-c".to_string(),
        "android.intent.category.LAUNCHER".to_string(),
        "1".to_string(),
    ])
}

fn run_owned_command_output(argv: Vec<String>) -> Result<CommandOutput, String> {
    let borrowed = argv.iter().map(String::as_str).collect::<Vec<_>>();
    run_command_output(&borrowed)
}

fn run_command_output(argv: &[&str]) -> Result<CommandOutput, String> {
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
        provenance: None,
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
            provenance: Some(AndroidObservationProvenance::AospPlatformEvent {
                source: AospPlatformEventSource {
                    service_name: AOSP_FOREGROUND_OBSERVER_SERVICE.to_string(),
                    event_id: "event-1".to_string(),
                },
            }),
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
        assert_eq!(
            request.required_capability(),
            Some(AndroidCapability::LaunchApp)
        );
        assert_eq!(
            request.capability_status(),
            Some(AndroidCapabilityStatus::Available)
        );
    }

    #[test]
    fn rooted_stock_substrate_is_explicitly_recon() {
        let observation = AndroidObservation {
            substrate: AndroidSubstrate::ReconRootedStock,
            event: AndroidEvent::NetworkAvailabilityChanged { available: true },
            provenance: None,
        };

        assert_eq!(observation.substrate, AndroidSubstrate::ReconRootedStock);
    }

    #[test]
    fn capability_map_keeps_recon_and_aosp_boundaries_explicit() {
        let foreground = android_capability_entry(AndroidCapability::ObserveForegroundApp)
            .expect("foreground capability");
        assert_eq!(foreground.rooted_stock, AndroidCapabilityStatus::Available);
        assert_eq!(foreground.aosp_platform, AndroidCapabilityStatus::Available);

        let place_call =
            android_capability_entry(AndroidCapability::PlaceCall).expect("call capability");
        assert_eq!(
            place_call.rooted_stock,
            AndroidCapabilityStatus::RequiresAospPrivilege
        );
        assert_eq!(place_call.aosp_platform, AndroidCapabilityStatus::Available);

        let root_shell =
            android_capability_entry(AndroidCapability::RootShell).expect("root capability");
        assert_eq!(root_shell.rooted_stock, AndroidCapabilityStatus::Limited);
        assert_eq!(
            root_shell.aosp_platform,
            AndroidCapabilityStatus::Unavailable
        );
    }

    #[test]
    fn capability_map_has_unique_entries() {
        let map = android_capability_map();
        assert!(!map.is_empty());
        for (index, entry) in map.iter().enumerate() {
            assert!(
                map.iter()
                    .skip(index + 1)
                    .all(|other| other.capability != entry.capability),
                "duplicate capability: {:?}",
                entry.capability
            );
            assert!(!entry.note.trim().is_empty());
        }
    }

    #[test]
    fn capability_map_covers_every_android_capability_variant() {
        assert_eq!(android_capability_map().len(), AndroidCapability::ALL.len());
        for capability in AndroidCapability::ALL {
            assert!(
                android_capability_entry(*capability).is_some(),
                "missing capability map entry for {capability:?}"
            );
        }
    }

    #[test]
    fn parses_android_substrate_names_without_stringly_call_sites() {
        assert_eq!(
            "recon-rooted-stock".parse::<AndroidSubstrate>(),
            Ok(AndroidSubstrate::ReconRootedStock)
        );
        assert_eq!(
            "ReconRootedStock".parse::<AndroidSubstrate>(),
            Ok(AndroidSubstrate::ReconRootedStock)
        );
        assert_eq!(
            "aosp-platform".parse::<AndroidSubstrate>(),
            Ok(AndroidSubstrate::AospPlatform)
        );
        assert_eq!(
            "system".parse::<AndroidSubstrate>(),
            Ok(AndroidSubstrate::AospPlatform)
        );
        assert!("stock".parse::<AndroidSubstrate>().is_err());
    }

    #[test]
    fn capability_statuses_project_each_substrate_from_same_map() {
        let recon = android_capability_statuses(AndroidSubstrate::ReconRootedStock);
        let aosp = android_capability_statuses(AndroidSubstrate::AospPlatform);

        assert_eq!(recon.len(), AndroidCapability::ALL.len());
        assert_eq!(aosp.len(), AndroidCapability::ALL.len());
        assert!(recon.iter().any(|entry| {
            entry.capability == AndroidCapability::RootShell
                && entry.status == AndroidCapabilityStatus::Limited
        }));
        assert!(aosp.iter().any(|entry| {
            entry.capability == AndroidCapability::RootShell
                && entry.status == AndroidCapabilityStatus::Unavailable
        }));
        assert!(aosp.iter().any(|entry| {
            entry.capability == AndroidCapability::PlaceCall
                && entry.status == AndroidCapabilityStatus::Available
        }));
    }

    #[test]
    fn capability_all_list_exercises_every_variant_with_exhaustive_match() {
        for capability in AndroidCapability::ALL {
            match capability {
                AndroidCapability::ObserveForegroundApp
                | AndroidCapability::LaunchApp
                | AndroidCapability::ControlForegroundApp
                | AndroidCapability::ReadNotifications
                | AndroidCapability::PostNotifications
                | AndroidCapability::PlaceCall
                | AndroidCapability::SendMessage
                | AndroidCapability::ReadSharedStorage
                | AndroidCapability::WriteSharedStorage
                | AndroidCapability::NetworkAccess
                | AndroidCapability::BackgroundExecution
                | AndroidCapability::InstallPackages
                | AndroidCapability::SystemSettings
                | AndroidCapability::RootShell => {}
            }
        }
    }

    #[test]
    fn parses_installed_package_names_from_pm_output() {
        let packages = parse_pm_list_packages(
            "package:com.android.settings\npackage: com.google.android.aicore \nignored\n",
        );

        assert!(packages.contains("com.android.settings"));
        assert!(packages.contains("com.google.android.aicore"));
        assert_eq!(packages.len(), 2);
    }

    #[test]
    fn local_model_probe_reports_aicore_without_claiming_inference_api() {
        let packages = BTreeSet::from(["com.google.android.aicore".to_string()]);
        let probe = probe_known_local_model_package(
            LocalModelProviderKind::AicoreGeminiNano,
            &packages,
            &["com.google.android.aicore"],
            "test note",
        );

        assert_eq!(probe.kind, LocalModelProviderKind::AicoreGeminiNano);
        assert_eq!(
            probe.status,
            LocalModelProviderStatus::PresentButNoPublicTerminalApi
        );
        assert_eq!(
            probe.package_name.as_deref(),
            Some("com.google.android.aicore")
        );
        assert!(probe.evidence[0].contains("installed package"));
    }

    #[test]
    fn local_model_probe_reports_unavailable_when_known_package_missing() {
        let packages = BTreeSet::from(["com.android.settings".to_string()]);
        let probe = probe_known_local_model_package(
            LocalModelProviderKind::AicoreGeminiNano,
            &packages,
            &["com.google.android.aicore"],
            "test note",
        );

        assert_eq!(probe.status, LocalModelProviderStatus::Unavailable);
        assert!(probe.package_name.is_none());
        assert!(probe.evidence.is_empty());
    }

    #[test]
    fn local_model_probe_collects_unknown_google_ai_surfaces_as_informational() {
        let packages = BTreeSet::from([
            "com.android.settings".to_string(),
            "com.google.android.apps.gemini".to_string(),
            "com.google.android.aicore".to_string(),
        ]);
        let probe = probe_google_ai_package_family(&packages);

        assert_eq!(probe.kind, LocalModelProviderKind::UnknownGoogleAiSurface);
        assert_eq!(
            probe.status,
            LocalModelProviderStatus::PresentButNoPublicTerminalApi
        );
        assert!(probe.package_name.is_none());
        assert_eq!(probe.evidence.len(), 2);
        assert!(probe.note.contains("must not be used as an inference API"));
    }

    #[test]
    fn local_model_probe_reports_indeterminate_when_package_manager_fails() {
        let report = local_model_probe_from_package_result(
            AndroidSubstrate::ReconRootedStock,
            Err("cmd package denied".to_string()),
        );

        assert_eq!(report.substrate, AndroidSubstrate::ReconRootedStock);
        assert_eq!(report.providers.len(), 3);
        for provider in report.providers {
            assert_eq!(provider.status, LocalModelProviderStatus::Indeterminate);
            assert!(provider.package_name.is_none());
            assert_eq!(
                provider.evidence,
                vec!["pm list packages failed: cmd package denied".to_string()]
            );
        }
    }

    #[test]
    fn rooted_actions_are_typed_capability_requests() {
        let request = AndroidActionRequest {
            substrate: AndroidSubstrate::ReconRootedStock,
            command: AndroidCommand::PerformRootedAction {
                capability: AndroidCapability::PlaceCall,
                target: "tel:+15555550100".to_string(),
            },
        };

        assert_eq!(
            request.required_capability(),
            Some(AndroidCapability::PlaceCall)
        );
        assert_eq!(
            request.capability_status(),
            Some(AndroidCapabilityStatus::RequiresAospPrivilege)
        );
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
            provenance: None,
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
    fn resume_app_surface_command_uses_package_only_launcher_intent() {
        let argv = resume_app_surface_command("com.android.settings").expect("resume app command");

        assert_eq!(
            argv,
            [
                "monkey",
                "-p",
                "com.android.settings",
                "-c",
                "android.intent.category.LAUNCHER",
                "1"
            ]
        );
    }

    #[test]
    fn resume_app_surface_command_rejects_empty_package() {
        let error = resume_app_surface_command("  ").expect_err("empty package rejected");

        assert!(error.contains("empty Android package"));
    }

    #[test]
    fn resume_app_surface_command_rejects_surrounding_whitespace() {
        let error =
            resume_app_surface_command(" com.android.settings").expect_err("package rejected");

        assert!(error.contains("surrounding whitespace"));
    }

    #[test]
    fn rooted_stock_resume_app_request_is_executable() {
        let request = AndroidActionRequest {
            substrate: AndroidSubstrate::ReconRootedStock,
            command: AndroidCommand::ResumeAppSurface {
                package_name: "com.android.settings".to_string(),
            },
        };

        assert_eq!(
            request.required_capability(),
            Some(AndroidCapability::LaunchApp)
        );
        assert_eq!(
            request.capability_status(),
            Some(AndroidCapabilityStatus::Limited)
        );
        assert!(ensure_supported_action_request(&request).is_ok());
    }

    #[test]
    fn aosp_resume_app_request_is_not_executed_by_recon_adapter() {
        let request = AndroidActionRequest {
            substrate: AndroidSubstrate::AospPlatform,
            command: AndroidCommand::ResumeAppSurface {
                package_name: "com.android.settings".to_string(),
            },
        };

        let error = ensure_supported_action_request(&request).expect_err("aosp execution rejected");

        assert!(error.contains("not executable by rooted-stock recon"));
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

    #[test]
    fn aosp_platform_probe_does_not_relabel_shell_recon_as_platform_evidence() {
        let observation = aosp_platform_adapter_unavailable_observation("foreground");

        let AndroidEvent::ForegroundObservationUnavailable {
            reason, raw_source, ..
        } = observation.event
        else {
            panic!("expected unavailable foreground observation");
        };

        assert_eq!(observation.substrate, AndroidSubstrate::AospPlatform);
        assert_eq!(
            reason,
            AndroidForegroundUnavailableReason::AdapterUnavailable
        );
        assert!(
            raw_source
                .expect("raw source")
                .contains("shell recon evidence must stay on ReconRootedStock")
        );
    }

    #[test]
    fn aosp_foreground_primitives_refuse_to_parse_shell_output_as_platform_evidence() {
        let dumpsys = Ok(CommandOutput {
            stdout: "mCurrentFocus=Window{39760e7 u0 com.android.settings/.Settings}\n".to_string(),
            stderr: String::new(),
            status: "exit status: 0".to_string(),
            success: true,
        });
        let observation =
            foreground_observation_from_output(AndroidSubstrate::AospPlatform, dumpsys);

        assert_eq!(observation.substrate, AndroidSubstrate::AospPlatform);
        assert!(matches!(
            observation.event,
            AndroidEvent::ForegroundObservationUnavailable {
                reason: AndroidForegroundUnavailableReason::AdapterUnavailable,
                ..
            }
        ));

        let observation = foreground_observation_from_result(
            AndroidSubstrate::AospPlatform,
            Ok("mCurrentFocus=Window{39760e7 u0 com.android.settings/.Settings}".to_string()),
        );

        assert!(matches!(
            observation.event,
            AndroidEvent::ForegroundObservationUnavailable {
                reason: AndroidForegroundUnavailableReason::AdapterUnavailable,
                ..
            }
        ));
    }

    #[test]
    fn aosp_platform_foreground_event_is_the_only_success_path_for_aosp_foreground() {
        let observation = aosp_foreground_observation_from_platform_event(AospForegroundEvent {
            package_name: "com.android.settings".to_string(),
            activity_name: Some(".Settings".to_string()),
            source: AospPlatformEventSource {
                service_name: AOSP_FOREGROUND_OBSERVER_SERVICE.to_string(),
                event_id: "event-1".to_string(),
            },
        })
        .expect("platform event should convert");

        assert_eq!(observation.substrate, AndroidSubstrate::AospPlatform);
        assert_eq!(
            observation.event,
            AndroidEvent::ForegroundAppChanged {
                package_name: "com.android.settings".to_string(),
                activity_name: Some(".Settings".to_string())
            }
        );
        assert_eq!(
            observation.provenance,
            Some(AndroidObservationProvenance::AospPlatformEvent {
                source: AospPlatformEventSource {
                    service_name: AOSP_FOREGROUND_OBSERVER_SERVICE.to_string(),
                    event_id: "event-1".to_string(),
                },
            })
        );
    }

    #[test]
    fn aosp_platform_foreground_event_requires_auditable_source() {
        let error = aosp_foreground_observation_from_platform_event(AospForegroundEvent {
            package_name: "com.android.settings".to_string(),
            activity_name: Some(".Settings".to_string()),
            source: AospPlatformEventSource {
                service_name: String::new(),
                event_id: "event-1".to_string(),
            },
        })
        .expect_err("missing service name should reject");

        assert!(error.contains("service name"));

        let error = aosp_foreground_observation_from_platform_event(AospForegroundEvent {
            package_name: " com.android.settings".to_string(),
            activity_name: None,
            source: AospPlatformEventSource {
                service_name: AOSP_FOREGROUND_OBSERVER_SERVICE.to_string(),
                event_id: "event-1".to_string(),
            },
        })
        .expect_err("whitespace package should reject");

        assert!(error.contains("surrounding whitespace"));

        let error = aosp_foreground_observation_from_platform_event(AospForegroundEvent {
            package_name: "com.android.settings".to_string(),
            activity_name: Some(String::new()),
            source: AospPlatformEventSource {
                service_name: AOSP_FOREGROUND_OBSERVER_SERVICE.to_string(),
                event_id: "event-1".to_string(),
            },
        })
        .expect_err("empty activity should reject");

        assert!(error.contains("activity"));

        let error = aosp_foreground_observation_from_platform_event(AospForegroundEvent {
            package_name: "com.android.settings".to_string(),
            activity_name: Some(".Settings".to_string()),
            source: AospPlatformEventSource {
                service_name: "dumpsys".to_string(),
                event_id: "event-1".to_string(),
            },
        })
        .expect_err("shell-like event source should reject");

        assert!(error.contains(AOSP_FOREGROUND_OBSERVER_SERVICE));
    }

    #[test]
    fn aosp_platform_foreground_event_json_decodes_to_platform_observation() {
        let observation = aosp_foreground_observation_from_platform_event_json(
            r#"{
                "package_name": "com.android.settings",
                "activity_name": "com.android.settings.Settings",
                "source": {
                    "service_name": "fawx-system-foreground-observer",
                    "event_id": "event-123"
                }
            }"#,
        )
        .expect("valid platform event json should produce observation");

        assert_eq!(observation.substrate, AndroidSubstrate::AospPlatform);
        assert!(matches!(
            observation.event,
            AndroidEvent::ForegroundAppChanged {
                package_name,
                activity_name: Some(activity_name),
            } if package_name == "com.android.settings"
                && activity_name == "com.android.settings.Settings"
        ));
        assert!(matches!(
            observation.provenance,
            Some(AndroidObservationProvenance::AospPlatformEvent { source })
                if source.service_name == AOSP_FOREGROUND_OBSERVER_SERVICE
                    && source.event_id == "event-123"
        ));
    }

    #[test]
    fn aosp_platform_foreground_event_json_rejects_invalid_event_shape() {
        let error = aosp_foreground_observation_from_platform_event_json(
            r#"{
                "package_name": " com.android.settings",
                "source": {
                    "service_name": "fawx-system-foreground-observer",
                    "event_id": "event-123"
                }
            }"#,
        )
        .expect_err("invalid platform event json should be rejected");

        assert!(error.contains("package"));
    }

    #[test]
    fn android_observation_deserialization_rejects_forged_aosp_foreground_success() {
        let json = serde_json::json!({
            "substrate": "AospPlatform",
            "event": {
                "ForegroundAppChanged": {
                    "package_name": "com.android.settings",
                    "activity_name": ".Settings"
                }
            }
        });

        let error =
            serde_json::from_value::<AndroidObservation>(json).expect_err("forged AOSP rejected");

        assert!(error.to_string().contains("provenance"));
    }

    #[test]
    fn android_observation_deserialization_allows_aosp_foreground_with_provenance() {
        let json = serde_json::json!({
            "substrate": "AospPlatform",
            "event": {
                "ForegroundAppChanged": {
                    "package_name": "com.android.settings",
                    "activity_name": ".Settings"
                }
            },
            "provenance": {
                "AospPlatformEvent": {
                    "source": {
                        "service_name": AOSP_FOREGROUND_OBSERVER_SERVICE,
                        "event_id": "event-1"
                    }
                }
            }
        });

        let observation =
            serde_json::from_value::<AndroidObservation>(json).expect("provenance is valid");

        assert_eq!(observation.substrate, AndroidSubstrate::AospPlatform);
        assert!(matches!(
            observation.event,
            AndroidEvent::ForegroundAppChanged { .. }
        ));
        assert!(matches!(
            observation.provenance,
            Some(AndroidObservationProvenance::AospPlatformEvent { source })
                if source.service_name == AOSP_FOREGROUND_OBSERVER_SERVICE
        ));
    }

    #[test]
    fn android_observation_deserialization_rejects_shell_like_aosp_provenance() {
        let json = serde_json::json!({
            "substrate": "AospPlatform",
            "event": {
                "ForegroundAppChanged": {
                    "package_name": "com.android.settings",
                    "activity_name": ".Settings"
                }
            },
            "provenance": {
                "AospPlatformEvent": {
                    "source": {
                        "service_name": "dumpsys",
                        "event_id": "event-1"
                    }
                }
            }
        });

        let error = serde_json::from_value::<AndroidObservation>(json)
            .expect_err("shell-like provenance rejected");

        assert!(error.to_string().contains(AOSP_FOREGROUND_OBSERVER_SERVICE));
    }

    #[test]
    fn android_observation_deserialization_allows_aosp_adapter_unavailable() {
        let json = serde_json::json!({
            "substrate": "AospPlatform",
            "event": {
                "ForegroundObservationUnavailable": {
                    "target": "foreground",
                    "reason": "AdapterUnavailable",
                    "raw_source": "platform adapter not connected"
                }
            }
        });

        let observation =
            serde_json::from_value::<AndroidObservation>(json).expect("unavailable is valid");

        assert_eq!(observation.substrate, AndroidSubstrate::AospPlatform);
        assert!(matches!(
            observation.event,
            AndroidEvent::ForegroundObservationUnavailable {
                reason: AndroidForegroundUnavailableReason::AdapterUnavailable,
                ..
            }
        ));
    }
}
