//! Android substrate adapter for Fawx OS.
//!
//! This crate is intentionally named as an adapter, not a core runtime. The
//! goal is to keep Android-specific bindings thin so they can be replaced over
//! time without rewriting the kernel or harness.

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
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
    ReadRuntimeScratchStorage,
    WriteRuntimeScratchStorage,
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
        Self::ReadRuntimeScratchStorage,
        Self::WriteRuntimeScratchStorage,
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
        aosp_platform: AndroidCapabilityStatus::Unavailable,
        note: "Recon can observe foreground focus through dumpsys; AOSP remains unavailable until a platform foreground observer is connected.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::LaunchApp,
        rooted_stock: AndroidCapabilityStatus::Limited,
        aosp_platform: AndroidCapabilityStatus::Unavailable,
        note: "Recon can use activity-manager style shell commands; AOSP launch remains unavailable until a privileged app controller is connected.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::ControlForegroundApp,
        rooted_stock: AndroidCapabilityStatus::Limited,
        aosp_platform: AndroidCapabilityStatus::Unavailable,
        note: "Recon can probe input/UI automation; AOSP foreground control remains unavailable until an accessibility or framework controller is connected.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::ReadNotifications,
        rooted_stock: AndroidCapabilityStatus::Limited,
        aosp_platform: AndroidCapabilityStatus::Unavailable,
        note: "Recon may inspect notification state opportunistically; AOSP notification read remains unavailable until a listener/system hook is connected.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::PostNotifications,
        rooted_stock: AndroidCapabilityStatus::RequiresAospPrivilege,
        aosp_platform: AndroidCapabilityStatus::Unavailable,
        note: "Posting user-visible OS notifications has a typed seam, but no platform poster adapter exists yet.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::PlaceCall,
        rooted_stock: AndroidCapabilityStatus::RequiresAospPrivilege,
        aosp_platform: AndroidCapabilityStatus::Unavailable,
        note: "Telephony side effects have a typed seam, but no platform telephony adapter exists yet.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::SendMessage,
        rooted_stock: AndroidCapabilityStatus::RequiresAospPrivilege,
        aosp_platform: AndroidCapabilityStatus::Unavailable,
        note: "Messaging side effects have a typed seam, but no platform messaging adapter exists yet.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::ReadRuntimeScratchStorage,
        rooted_stock: AndroidCapabilityStatus::Available,
        aosp_platform: AndroidCapabilityStatus::Available,
        note: "Runtime-owned scratch storage under /data/local/tmp/fawx-os is available for prototype evidence and must not be confused with Android shared storage.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::WriteRuntimeScratchStorage,
        rooted_stock: AndroidCapabilityStatus::Available,
        aosp_platform: AndroidCapabilityStatus::Available,
        note: "Runtime-owned scratch storage under /data/local/tmp/fawx-os is available for prototype evidence and must not be confused with Android shared storage.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::ReadSharedStorage,
        rooted_stock: AndroidCapabilityStatus::Limited,
        aosp_platform: AndroidCapabilityStatus::Unavailable,
        note: "Android shared/scoped storage mediation is not implemented yet; current file smokes only prove runtime scratch storage.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::WriteSharedStorage,
        rooted_stock: AndroidCapabilityStatus::Limited,
        aosp_platform: AndroidCapabilityStatus::Unavailable,
        note: "Android shared/scoped storage mediation is not implemented yet; current file smokes only prove runtime scratch storage.",
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
        aosp_platform: AndroidCapabilityStatus::Unavailable,
        note: "Recon can run detached shell processes; AOSP background execution remains unavailable until a supervised platform service is connected.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::InstallPackages,
        rooted_stock: AndroidCapabilityStatus::Limited,
        aosp_platform: AndroidCapabilityStatus::Unavailable,
        note: "Recon package install is device-policy dependent; AOSP install remains unavailable until package-manager authority is connected.",
    },
    AndroidCapabilityEntry {
        capability: AndroidCapability::SystemSettings,
        rooted_stock: AndroidCapabilityStatus::Limited,
        aosp_platform: AndroidCapabilityStatus::Unavailable,
        note: "Recon can inspect or poke some settings; AOSP settings control remains unavailable until typed framework APIs are connected.",
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
    BackgroundSupervisorHeartbeat {
        supervisor_id: String,
        active_tasks: u32,
    },
    BackgroundSupervisorUnavailable {
        target: String,
        reason: AndroidBackgroundSupervisorUnavailableReason,
        raw_source: Option<String>,
    },
    AppLaunchCompleted {
        package_name: String,
        activity_name: Option<String>,
    },
    AppLaunchUnavailable {
        target: String,
        reason: AndroidAppLaunchUnavailableReason,
        raw_source: Option<String>,
    },
    NotificationUnavailable {
        target: String,
        reason: AndroidNotificationUnavailableReason,
        raw_source: Option<String>,
    },
    NotificationPostUnavailable {
        target: String,
        reason: AndroidNotificationPostUnavailableReason,
        raw_source: Option<String>,
    },
    MessageUnavailable {
        target: String,
        reason: AndroidMessageUnavailableReason,
        raw_source: Option<String>,
    },
    PhoneCallUnavailable {
        target: String,
        reason: AndroidPhoneCallUnavailableReason,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AndroidBackgroundSupervisorUnavailableReason {
    AdapterUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AndroidAppLaunchUnavailableReason {
    AdapterUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AndroidNotificationUnavailableReason {
    AdapterUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AndroidNotificationPostUnavailableReason {
    AdapterUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AndroidMessageUnavailableReason {
    AdapterUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AndroidPhoneCallUnavailableReason {
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
    PostNotification {
        title: String,
        body: String,
    },
    QueryForegroundState,
    ResumeAppSurface {
        package_name: String,
    },
    InputKeyEvent {
        key_code: String,
    },
    InputTap {
        x: u32,
        y: u32,
    },
    InputText {
        text: String,
    },
    InputSwipe {
        x1: u32,
        y1: u32,
        x2: u32,
        y2: u32,
        duration_ms: Option<u32>,
    },
    ReadScopedFile {
        path: String,
    },
    WriteScopedFile {
        path: String,
        contents: String,
    },
    SendMessage {
        contact: String,
        body: String,
    },
    PlaceCall {
        number: String,
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
        if wire.substrate == AndroidSubstrate::AospPlatform {
            match &wire.event {
                AndroidEvent::ForegroundAppChanged { .. } => {
                    let Some(AndroidObservationProvenance::AospPlatformEvent { source }) =
                        wire.provenance.as_ref()
                    else {
                        return Err(serde::de::Error::custom(
                            "AospPlatform foreground observations require AospPlatformEvent provenance",
                        ));
                    };
                    validate_aosp_platform_event_source(source, AOSP_FOREGROUND_OBSERVER_SERVICE)
                        .map_err(serde::de::Error::custom)?;
                }
                AndroidEvent::BackgroundSupervisorHeartbeat { .. } => {
                    let Some(AndroidObservationProvenance::AospPlatformEvent { source }) =
                        wire.provenance.as_ref()
                    else {
                        return Err(serde::de::Error::custom(
                            "AospPlatform background supervisor observations require AospPlatformEvent provenance",
                        ));
                    };
                    validate_aosp_platform_event_source(source, AOSP_BACKGROUND_SUPERVISOR_SERVICE)
                        .map_err(serde::de::Error::custom)?;
                }
                AndroidEvent::AppLaunchCompleted { .. } => {
                    let Some(AndroidObservationProvenance::AospPlatformEvent { source }) =
                        wire.provenance.as_ref()
                    else {
                        return Err(serde::de::Error::custom(
                            "AospPlatform app launch observations require AospPlatformEvent provenance",
                        ));
                    };
                    validate_aosp_platform_event_source(source, AOSP_APP_CONTROLLER_SERVICE)
                        .map_err(serde::de::Error::custom)?;
                }
                AndroidEvent::NotificationReceived { .. } => {
                    let Some(AndroidObservationProvenance::AospPlatformEvent { source }) =
                        wire.provenance.as_ref()
                    else {
                        return Err(serde::de::Error::custom(
                            "AospPlatform notification observations require AospPlatformEvent provenance",
                        ));
                    };
                    validate_aosp_platform_event_source(source, AOSP_NOTIFICATION_LISTENER_SERVICE)
                        .map_err(serde::de::Error::custom)?;
                }
                AndroidEvent::ForegroundObservationUnavailable { .. }
                | AndroidEvent::BackgroundSupervisorUnavailable { .. }
                | AndroidEvent::AppLaunchUnavailable { .. }
                | AndroidEvent::NotificationUnavailable { .. }
                | AndroidEvent::NotificationPostUnavailable { .. }
                | AndroidEvent::MessageUnavailable { .. }
                | AndroidEvent::PhoneCallUnavailable { .. }
                    if matches!(
                        wire.provenance,
                        Some(AndroidObservationProvenance::AospPlatformEvent { .. })
                    ) =>
                {
                    return Err(serde::de::Error::custom(
                        "AospPlatformEvent provenance is only valid for AOSP platform success observations",
                    ));
                }
                _ => {}
            }
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
pub const AOSP_BACKGROUND_SUPERVISOR_SERVICE: &str = "fawx-system-background-supervisor";
pub const AOSP_APP_CONTROLLER_SERVICE: &str = "fawx-system-app-controller";
pub const AOSP_NOTIFICATION_LISTENER_SERVICE: &str = "fawx-system-notification-listener";

/// Foreground state emitted by a real AOSP/system adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AospForegroundEvent {
    pub package_name: String,
    pub activity_name: Option<String>,
    pub source: AospPlatformEventSource,
}

/// Heartbeat emitted by a real AOSP/system background supervisor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AospBackgroundSupervisorEvent {
    pub supervisor_id: String,
    pub active_tasks: u32,
    pub source: AospPlatformEventSource,
}

/// App launch/resume result emitted by a real AOSP/system app controller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AospAppLaunchResult {
    pub package_name: String,
    pub activity_name: Option<String>,
    pub source: AospPlatformEventSource,
}

/// Notification event emitted by a real AOSP/system notification listener.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AospNotificationEvent {
    pub app_package_name: String,
    pub summary: String,
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
            Self::PostNotification { .. } => Some(AndroidCapability::PostNotifications),
            Self::InputKeyEvent { .. }
            | Self::InputTap { .. }
            | Self::InputText { .. }
            | Self::InputSwipe { .. } => Some(AndroidCapability::ControlForegroundApp),
            Self::ReadScopedFile { .. } => Some(AndroidCapability::ReadRuntimeScratchStorage),
            Self::WriteScopedFile { .. } => Some(AndroidCapability::WriteRuntimeScratchStorage),
            Self::SendMessage { .. } => Some(AndroidCapability::SendMessage),
            Self::PlaceCall { .. } => Some(AndroidCapability::PlaceCall),
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
        AndroidCommand::InputKeyEvent { key_code } => {
            run_owned_command_output(input_keyevent_command(key_code)?)
        }
        AndroidCommand::InputTap { x, y } => run_owned_command_output(input_tap_command(*x, *y)),
        AndroidCommand::InputText { text } => run_owned_command_output(input_text_command(text)?),
        AndroidCommand::InputSwipe {
            x1,
            y1,
            x2,
            y2,
            duration_ms,
        } => run_owned_command_output(input_swipe_command(*x1, *y1, *x2, *y2, *duration_ms)),
        AndroidCommand::ReadScopedFile { path } => read_scoped_file_output(path),
        AndroidCommand::WriteScopedFile { path, contents } => {
            write_scoped_file_output(path, contents)
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

pub fn aosp_background_supervisor_unavailable_observation(target: &str) -> AndroidObservation {
    AndroidObservation {
        substrate: AndroidSubstrate::AospPlatform,
        event: AndroidEvent::BackgroundSupervisorUnavailable {
            target: target.to_string(),
            reason: AndroidBackgroundSupervisorUnavailableReason::AdapterUnavailable,
            raw_source: Some(
                "AOSP background supervisor is not connected; adb/recon processes must stay on ReconRootedStock"
                    .to_string(),
            ),
        },
        provenance: None,
    }
}

pub fn aosp_app_launch_unavailable_observation(target: &str) -> AndroidObservation {
    AndroidObservation {
        substrate: AndroidSubstrate::AospPlatform,
        event: AndroidEvent::AppLaunchUnavailable {
            target: target.to_string(),
            reason: AndroidAppLaunchUnavailableReason::AdapterUnavailable,
            raw_source: Some(
                "AOSP app controller is not connected; monkey/recon app launches must stay on ReconRootedStock"
                    .to_string(),
            ),
        },
        provenance: None,
    }
}

pub fn aosp_notification_unavailable_observation(target: &str) -> AndroidObservation {
    AndroidObservation {
        substrate: AndroidSubstrate::AospPlatform,
        event: AndroidEvent::NotificationUnavailable {
            target: target.to_string(),
            reason: AndroidNotificationUnavailableReason::AdapterUnavailable,
            raw_source: Some(
                "AOSP notification listener is not connected; notification reads must come from a platform listener"
                    .to_string(),
            ),
        },
        provenance: None,
    }
}

pub fn aosp_notification_post_unavailable_observation(target: &str) -> AndroidObservation {
    AndroidObservation {
        substrate: AndroidSubstrate::AospPlatform,
        event: AndroidEvent::NotificationPostUnavailable {
            target: target.to_string(),
            reason: AndroidNotificationPostUnavailableReason::AdapterUnavailable,
            raw_source: Some(
                "AOSP notification poster is not connected; notification posts must come from a platform poster"
                    .to_string(),
            ),
        },
        provenance: None,
    }
}

pub fn aosp_message_unavailable_observation(target: &str) -> AndroidObservation {
    AndroidObservation {
        substrate: AndroidSubstrate::AospPlatform,
        event: AndroidEvent::MessageUnavailable {
            target: target.to_string(),
            reason: AndroidMessageUnavailableReason::AdapterUnavailable,
            raw_source: Some(
                "AOSP messaging adapter is not connected; message sends must come from a platform messaging service"
                    .to_string(),
            ),
        },
        provenance: None,
    }
}

pub fn aosp_phone_call_unavailable_observation(target: &str) -> AndroidObservation {
    AndroidObservation {
        substrate: AndroidSubstrate::AospPlatform,
        event: AndroidEvent::PhoneCallUnavailable {
            target: target.to_string(),
            reason: AndroidPhoneCallUnavailableReason::AdapterUnavailable,
            raw_source: Some(
                "AOSP telephony adapter is not connected; phone calls must come from a platform telephony service"
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
    validate_aosp_platform_event_source(&event.source, AOSP_FOREGROUND_OBSERVER_SERVICE)?;
    Ok(())
}

pub fn aosp_background_supervisor_observation_from_platform_event(
    event: AospBackgroundSupervisorEvent,
) -> Result<AndroidObservation, String> {
    validate_aosp_background_supervisor_event(&event)?;
    let source = event.source.clone();
    Ok(AndroidObservation {
        substrate: AndroidSubstrate::AospPlatform,
        event: AndroidEvent::BackgroundSupervisorHeartbeat {
            supervisor_id: event.supervisor_id,
            active_tasks: event.active_tasks,
        },
        provenance: Some(AndroidObservationProvenance::AospPlatformEvent { source }),
    })
}

pub fn aosp_background_supervisor_observation_from_platform_event_json(
    event_json: &str,
) -> Result<AndroidObservation, String> {
    let event = serde_json::from_str::<AospBackgroundSupervisorEvent>(event_json)
        .map_err(|error| format!("failed to decode AOSP background supervisor event: {error}"))?;
    aosp_background_supervisor_observation_from_platform_event(event)
}

fn validate_aosp_background_supervisor_event(
    event: &AospBackgroundSupervisorEvent,
) -> Result<(), String> {
    validate_nonempty_token("AOSP background supervisor id", &event.supervisor_id)?;
    validate_no_surrounding_whitespace("AOSP background supervisor id", &event.supervisor_id)?;
    validate_aosp_platform_event_source(&event.source, AOSP_BACKGROUND_SUPERVISOR_SERVICE)?;
    Ok(())
}

pub fn aosp_app_launch_observation_from_platform_result(
    result: AospAppLaunchResult,
) -> Result<AndroidObservation, String> {
    validate_aosp_app_launch_result(&result)?;
    let source = result.source.clone();
    Ok(AndroidObservation {
        substrate: AndroidSubstrate::AospPlatform,
        event: AndroidEvent::AppLaunchCompleted {
            package_name: result.package_name,
            activity_name: result.activity_name,
        },
        provenance: Some(AndroidObservationProvenance::AospPlatformEvent { source }),
    })
}

pub fn aosp_app_launch_observation_from_platform_result_json(
    result_json: &str,
) -> Result<AndroidObservation, String> {
    let result = serde_json::from_str::<AospAppLaunchResult>(result_json)
        .map_err(|error| format!("failed to decode AOSP app launch result: {error}"))?;
    aosp_app_launch_observation_from_platform_result(result)
}

fn validate_aosp_app_launch_result(result: &AospAppLaunchResult) -> Result<(), String> {
    validate_nonempty_token("AOSP app launch package", &result.package_name)?;
    validate_no_surrounding_whitespace("AOSP app launch package", &result.package_name)?;
    if let Some(activity_name) = &result.activity_name {
        validate_nonempty_token("AOSP app launch activity", activity_name)?;
        validate_no_surrounding_whitespace("AOSP app launch activity", activity_name)?;
    }
    validate_aosp_platform_event_source(&result.source, AOSP_APP_CONTROLLER_SERVICE)?;
    Ok(())
}

pub fn aosp_notification_observation_from_platform_event(
    event: AospNotificationEvent,
) -> Result<AndroidObservation, String> {
    validate_aosp_notification_event(&event)?;
    let source = event.source.clone();
    Ok(AndroidObservation {
        substrate: AndroidSubstrate::AospPlatform,
        event: AndroidEvent::NotificationReceived {
            source: event.app_package_name,
            summary: event.summary,
        },
        provenance: Some(AndroidObservationProvenance::AospPlatformEvent { source }),
    })
}

pub fn aosp_notification_observation_from_platform_event_json(
    event_json: &str,
) -> Result<AndroidObservation, String> {
    let event = serde_json::from_str::<AospNotificationEvent>(event_json)
        .map_err(|error| format!("failed to decode AOSP notification event: {error}"))?;
    aosp_notification_observation_from_platform_event(event)
}

fn validate_aosp_notification_event(event: &AospNotificationEvent) -> Result<(), String> {
    validate_nonempty_token("AOSP notification app package", &event.app_package_name)?;
    validate_no_surrounding_whitespace("AOSP notification app package", &event.app_package_name)?;
    validate_nonempty_token("AOSP notification summary", &event.summary)?;
    validate_no_surrounding_whitespace("AOSP notification summary", &event.summary)?;
    validate_aosp_platform_event_source(&event.source, AOSP_NOTIFICATION_LISTENER_SERVICE)?;
    Ok(())
}

fn validate_aosp_platform_event_source(
    source: &AospPlatformEventSource,
    expected_service_name: &str,
) -> Result<(), String> {
    validate_nonempty_token("AOSP platform service name", &source.service_name)?;
    validate_no_surrounding_whitespace("AOSP platform service name", &source.service_name)?;
    if source.service_name != expected_service_name {
        return Err(format!(
            "AOSP platform service name must be {expected_service_name}"
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

pub fn input_keyevent_command(key_code: &str) -> Result<Vec<String>, String> {
    let key_code = key_code.trim();
    if key_code.is_empty() {
        return Err("input keyevent requires a key code".to_string());
    }
    if !key_code
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err("input keyevent key code must be alphanumeric or underscore".to_string());
    }
    Ok(vec![
        "input".to_string(),
        "keyevent".to_string(),
        key_code.to_string(),
    ])
}

pub fn input_tap_command(x: u32, y: u32) -> Vec<String> {
    vec![
        "input".to_string(),
        "tap".to_string(),
        x.to_string(),
        y.to_string(),
    ]
}

pub fn input_text_command(text: &str) -> Result<Vec<String>, String> {
    if text.is_empty() {
        return Err("input text must not be empty".to_string());
    }
    if text.contains('\n') || text.contains('\r') {
        return Err("input text must be single-line".to_string());
    }
    Ok(vec![
        "input".to_string(),
        "text".to_string(),
        escape_input_text(text),
    ])
}

pub fn input_swipe_command(
    x1: u32,
    y1: u32,
    x2: u32,
    y2: u32,
    duration_ms: Option<u32>,
) -> Vec<String> {
    let mut command = vec![
        "input".to_string(),
        "swipe".to_string(),
        x1.to_string(),
        y1.to_string(),
        x2.to_string(),
        y2.to_string(),
    ];
    if let Some(duration_ms) = duration_ms {
        command.push(duration_ms.to_string());
    }
    command
}

fn escape_input_text(text: &str) -> String {
    text.replace('%', "%25").replace(' ', "%s")
}

fn read_scoped_file_output(path: &str) -> Result<CommandOutput, String> {
    let path = scoped_fawx_os_path(path)?;
    let stdout = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    Ok(CommandOutput {
        stdout,
        stderr: String::new(),
        status: "read runtime scratch file".to_string(),
        success: true,
    })
}

fn write_scoped_file_output(path: &str, contents: &str) -> Result<CommandOutput, String> {
    let path = scoped_fawx_os_path(path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    fs::write(&path, contents)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    Ok(CommandOutput {
        stdout: format!("wrote {} byte(s) to {}", contents.len(), path.display()),
        stderr: String::new(),
        status: "write runtime scratch file".to_string(),
        success: true,
    })
}

fn scoped_fawx_os_path(path: &str) -> Result<PathBuf, String> {
    let path = Path::new(path);
    let root = Path::new("/data/local/tmp/fawx-os");
    if !path.is_absolute() {
        return Err("scoped Android file path must be absolute".to_string());
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("scoped Android file path must not contain parent traversal".to_string());
    }
    if !path.starts_with(root) {
        return Err(format!(
            "scoped Android file path must stay under {}",
            root.display()
        ));
    }
    Ok(path.to_path_buf())
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
            substrate: AndroidSubstrate::ReconRootedStock,
            command: AndroidCommand::AcquireForeground {
                target: "subscription-cancel-flow".to_string(),
            },
        };

        assert!(matches!(
            request.command,
            AndroidCommand::AcquireForeground { .. }
        ));
        assert_eq!(request.substrate, AndroidSubstrate::ReconRootedStock);
        assert_eq!(
            request.required_capability(),
            Some(AndroidCapability::LaunchApp)
        );
        assert_eq!(
            request.capability_status(),
            Some(AndroidCapabilityStatus::Limited)
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
        assert_eq!(
            foreground.aosp_platform,
            AndroidCapabilityStatus::Unavailable
        );

        let place_call =
            android_capability_entry(AndroidCapability::PlaceCall).expect("call capability");
        assert_eq!(
            place_call.rooted_stock,
            AndroidCapabilityStatus::RequiresAospPrivilege
        );
        assert_eq!(
            place_call.aosp_platform,
            AndroidCapabilityStatus::Unavailable
        );

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
                && entry.status == AndroidCapabilityStatus::Unavailable
        }));
        assert!(aosp.iter().any(|entry| {
            entry.capability == AndroidCapability::ReadRuntimeScratchStorage
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
                | AndroidCapability::ReadRuntimeScratchStorage
                | AndroidCapability::WriteRuntimeScratchStorage
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
    fn rooted_stock_input_request_is_typed_control_foreground_capability() {
        let request = AndroidActionRequest {
            substrate: AndroidSubstrate::ReconRootedStock,
            command: AndroidCommand::InputKeyEvent {
                key_code: "KEYCODE_HOME".to_string(),
            },
        };

        assert_eq!(
            request.required_capability(),
            Some(AndroidCapability::ControlForegroundApp)
        );
        assert_eq!(
            request.capability_status(),
            Some(AndroidCapabilityStatus::Limited)
        );
        assert!(ensure_supported_action_request(&request).is_ok());
    }

    #[test]
    fn input_commands_are_argument_vector_commands_not_shell_strings() {
        assert_eq!(
            input_keyevent_command("KEYCODE_HOME").expect("keyevent command"),
            ["input", "keyevent", "KEYCODE_HOME"]
        );
        assert_eq!(input_tap_command(10, 20), ["input", "tap", "10", "20"]);
        assert_eq!(
            input_text_command("hello world").expect("text command"),
            ["input", "text", "hello%sworld"]
        );
        assert_eq!(
            input_swipe_command(1, 2, 3, 4, Some(250)),
            ["input", "swipe", "1", "2", "3", "4", "250"]
        );
    }

    #[test]
    fn input_text_rejects_multiline_payloads() {
        let error = input_text_command("hello\nworld").expect_err("multiline rejected");

        assert!(error.contains("single-line"));
    }

    #[test]
    fn scoped_file_paths_must_stay_under_fawx_os_tmp_root() {
        assert!(
            scoped_fawx_os_path("/data/local/tmp/fawx-os/probes/file.txt").is_ok(),
            "in-scope path should be accepted"
        );
        let outside = scoped_fawx_os_path("/sdcard/Download/file.txt")
            .expect_err("outside path should be rejected");
        assert!(outside.contains("/data/local/tmp/fawx-os"));
        let traversal = scoped_fawx_os_path("/data/local/tmp/fawx-os/../escape")
            .expect_err("traversal should be rejected");
        assert!(traversal.contains("parent traversal"));
    }

    #[test]
    fn rooted_stock_sensitive_side_effect_commands_remain_unavailable() {
        for command in [
            AndroidCommand::PostNotification {
                title: "title".to_string(),
                body: "body".to_string(),
            },
            AndroidCommand::SendMessage {
                contact: "Ada".to_string(),
                body: "hello".to_string(),
            },
            AndroidCommand::PlaceCall {
                number: "+15555550100".to_string(),
            },
        ] {
            let request = AndroidActionRequest {
                substrate: AndroidSubstrate::ReconRootedStock,
                command,
            };

            let error = ensure_supported_action_request(&request)
                .expect_err("sensitive side-effect should require AOSP privilege");
            assert!(error.contains("RequiresAospPrivilege"));
        }
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
    fn aosp_background_supervisor_event_converts_with_provenance() {
        let observation = aosp_background_supervisor_observation_from_platform_event(
            AospBackgroundSupervisorEvent {
                supervisor_id: "supervisor-1".to_string(),
                active_tasks: 2,
                source: AospPlatformEventSource {
                    service_name: AOSP_BACKGROUND_SUPERVISOR_SERVICE.to_string(),
                    event_id: "event-1".to_string(),
                },
            },
        )
        .expect("background supervisor event should convert");

        assert_eq!(observation.substrate, AndroidSubstrate::AospPlatform);
        assert_eq!(
            observation.event,
            AndroidEvent::BackgroundSupervisorHeartbeat {
                supervisor_id: "supervisor-1".to_string(),
                active_tasks: 2,
            }
        );
        assert_eq!(
            observation.provenance,
            Some(AndroidObservationProvenance::AospPlatformEvent {
                source: AospPlatformEventSource {
                    service_name: AOSP_BACKGROUND_SUPERVISOR_SERVICE.to_string(),
                    event_id: "event-1".to_string(),
                },
            })
        );
    }

    #[test]
    fn aosp_background_supervisor_event_rejects_recon_sources() {
        let error = aosp_background_supervisor_observation_from_platform_event(
            AospBackgroundSupervisorEvent {
                supervisor_id: "supervisor-1".to_string(),
                active_tasks: 2,
                source: AospPlatformEventSource {
                    service_name: "adb".to_string(),
                    event_id: "event-1".to_string(),
                },
            },
        )
        .expect_err("adb source should reject");

        assert!(error.contains(AOSP_BACKGROUND_SUPERVISOR_SERVICE));
    }

    #[test]
    fn aosp_background_supervisor_event_json_decodes_to_platform_observation() {
        let observation = aosp_background_supervisor_observation_from_platform_event_json(
            r#"{
                "supervisor_id": "supervisor-1",
                "active_tasks": 2,
                "source": {
                    "service_name": "fawx-system-background-supervisor",
                    "event_id": "event-123"
                }
            }"#,
        )
        .expect("valid platform event json should produce observation");

        assert_eq!(observation.substrate, AndroidSubstrate::AospPlatform);
        assert!(matches!(
            observation.event,
            AndroidEvent::BackgroundSupervisorHeartbeat {
                supervisor_id,
                active_tasks: 2,
            } if supervisor_id == "supervisor-1"
        ));
        assert!(matches!(
            observation.provenance,
            Some(AndroidObservationProvenance::AospPlatformEvent { source })
                if source.service_name == AOSP_BACKGROUND_SUPERVISOR_SERVICE
                    && source.event_id == "event-123"
        ));
    }

    #[test]
    fn aosp_app_launch_result_converts_with_provenance() {
        let observation = aosp_app_launch_observation_from_platform_result(AospAppLaunchResult {
            package_name: "com.android.settings".to_string(),
            activity_name: Some("com.android.settings.Settings".to_string()),
            source: AospPlatformEventSource {
                service_name: AOSP_APP_CONTROLLER_SERVICE.to_string(),
                event_id: "event-1".to_string(),
            },
        })
        .expect("app launch result should convert");

        assert_eq!(observation.substrate, AndroidSubstrate::AospPlatform);
        assert_eq!(
            observation.event,
            AndroidEvent::AppLaunchCompleted {
                package_name: "com.android.settings".to_string(),
                activity_name: Some("com.android.settings.Settings".to_string()),
            }
        );
        assert_eq!(
            observation.provenance,
            Some(AndroidObservationProvenance::AospPlatformEvent {
                source: AospPlatformEventSource {
                    service_name: AOSP_APP_CONTROLLER_SERVICE.to_string(),
                    event_id: "event-1".to_string(),
                },
            })
        );
    }

    #[test]
    fn aosp_app_launch_result_rejects_recon_sources() {
        let error = aosp_app_launch_observation_from_platform_result(AospAppLaunchResult {
            package_name: "com.android.settings".to_string(),
            activity_name: None,
            source: AospPlatformEventSource {
                service_name: "monkey".to_string(),
                event_id: "event-1".to_string(),
            },
        })
        .expect_err("monkey source should reject");

        assert!(error.contains(AOSP_APP_CONTROLLER_SERVICE));
    }

    #[test]
    fn aosp_app_launch_result_json_decodes_to_platform_observation() {
        let observation = aosp_app_launch_observation_from_platform_result_json(
            r#"{
                "package_name": "com.android.settings",
                "activity_name": "com.android.settings.Settings",
                "source": {
                    "service_name": "fawx-system-app-controller",
                    "event_id": "event-123"
                }
            }"#,
        )
        .expect("valid app launch result json should produce observation");

        assert_eq!(observation.substrate, AndroidSubstrate::AospPlatform);
        assert!(matches!(
            observation.event,
            AndroidEvent::AppLaunchCompleted {
                package_name,
                activity_name: Some(activity_name),
            } if package_name == "com.android.settings"
                && activity_name == "com.android.settings.Settings"
        ));
        assert!(matches!(
            observation.provenance,
            Some(AndroidObservationProvenance::AospPlatformEvent { source })
                if source.service_name == AOSP_APP_CONTROLLER_SERVICE
                    && source.event_id == "event-123"
        ));
    }

    #[test]
    fn aosp_notification_event_converts_with_provenance() {
        let observation =
            aosp_notification_observation_from_platform_event(AospNotificationEvent {
                app_package_name: "com.example.mail".to_string(),
                summary: "New message from Ada".to_string(),
                source: AospPlatformEventSource {
                    service_name: AOSP_NOTIFICATION_LISTENER_SERVICE.to_string(),
                    event_id: "event-1".to_string(),
                },
            })
            .expect("notification event should convert");

        assert_eq!(observation.substrate, AndroidSubstrate::AospPlatform);
        assert_eq!(
            observation.event,
            AndroidEvent::NotificationReceived {
                source: "com.example.mail".to_string(),
                summary: "New message from Ada".to_string(),
            }
        );
        assert_eq!(
            observation.provenance,
            Some(AndroidObservationProvenance::AospPlatformEvent {
                source: AospPlatformEventSource {
                    service_name: AOSP_NOTIFICATION_LISTENER_SERVICE.to_string(),
                    event_id: "event-1".to_string(),
                },
            })
        );
    }

    #[test]
    fn aosp_notification_event_rejects_recon_sources() {
        let error = aosp_notification_observation_from_platform_event(AospNotificationEvent {
            app_package_name: "com.example.mail".to_string(),
            summary: "New message from Ada".to_string(),
            source: AospPlatformEventSource {
                service_name: "dumpsys".to_string(),
                event_id: "event-1".to_string(),
            },
        })
        .expect_err("dumpsys source should reject");

        assert!(error.contains(AOSP_NOTIFICATION_LISTENER_SERVICE));
    }

    #[test]
    fn aosp_notification_event_json_decodes_to_platform_observation() {
        let observation = aosp_notification_observation_from_platform_event_json(
            r#"{
                "app_package_name": "com.example.mail",
                "summary": "New message from Ada",
                "source": {
                    "service_name": "fawx-system-notification-listener",
                    "event_id": "event-123"
                }
            }"#,
        )
        .expect("valid notification event json should produce observation");

        assert_eq!(observation.substrate, AndroidSubstrate::AospPlatform);
        assert!(matches!(
            observation.event,
            AndroidEvent::NotificationReceived { source, summary }
                if source == "com.example.mail" && summary == "New message from Ada"
        ));
        assert!(matches!(
            observation.provenance,
            Some(AndroidObservationProvenance::AospPlatformEvent { source })
                if source.service_name == AOSP_NOTIFICATION_LISTENER_SERVICE
                    && source.event_id == "event-123"
        ));
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
    fn android_observation_deserialization_requires_background_supervisor_provenance() {
        let json = serde_json::json!({
            "substrate": "AospPlatform",
            "event": {
                "BackgroundSupervisorHeartbeat": {
                    "supervisor_id": "supervisor-1",
                    "active_tasks": 2
                }
            }
        });

        let error =
            serde_json::from_value::<AndroidObservation>(json).expect_err("forged AOSP rejected");

        assert!(error.to_string().contains("provenance"));
    }

    #[test]
    fn android_observation_deserialization_allows_background_supervisor_with_provenance() {
        let json = serde_json::json!({
            "substrate": "AospPlatform",
            "event": {
                "BackgroundSupervisorHeartbeat": {
                    "supervisor_id": "supervisor-1",
                    "active_tasks": 2
                }
            },
            "provenance": {
                "AospPlatformEvent": {
                    "source": {
                        "service_name": AOSP_BACKGROUND_SUPERVISOR_SERVICE,
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
            AndroidEvent::BackgroundSupervisorHeartbeat { .. }
        ));
    }

    #[test]
    fn android_observation_deserialization_requires_app_launch_provenance() {
        let json = serde_json::json!({
            "substrate": "AospPlatform",
            "event": {
                "AppLaunchCompleted": {
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
    fn android_observation_deserialization_allows_app_launch_with_provenance() {
        let json = serde_json::json!({
            "substrate": "AospPlatform",
            "event": {
                "AppLaunchCompleted": {
                    "package_name": "com.android.settings",
                    "activity_name": ".Settings"
                }
            },
            "provenance": {
                "AospPlatformEvent": {
                    "source": {
                        "service_name": AOSP_APP_CONTROLLER_SERVICE,
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
            AndroidEvent::AppLaunchCompleted { .. }
        ));
    }

    #[test]
    fn android_observation_deserialization_requires_notification_provenance() {
        let json = serde_json::json!({
            "substrate": "AospPlatform",
            "event": {
                "NotificationReceived": {
                    "source": "com.example.mail",
                    "summary": "New message from Ada"
                }
            }
        });

        let error =
            serde_json::from_value::<AndroidObservation>(json).expect_err("forged AOSP rejected");

        assert!(error.to_string().contains("provenance"));
    }

    #[test]
    fn android_observation_deserialization_allows_notification_with_provenance() {
        let json = serde_json::json!({
            "substrate": "AospPlatform",
            "event": {
                "NotificationReceived": {
                    "source": "com.example.mail",
                    "summary": "New message from Ada"
                }
            },
            "provenance": {
                "AospPlatformEvent": {
                    "source": {
                        "service_name": AOSP_NOTIFICATION_LISTENER_SERVICE,
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
            AndroidEvent::NotificationReceived { .. }
        ));
    }

    #[test]
    fn aosp_sensitive_action_unavailable_events_have_no_success_provenance() {
        let observations = [
            aosp_notification_post_unavailable_observation("notification-post"),
            aosp_message_unavailable_observation("messaging"),
            aosp_phone_call_unavailable_observation("phone-call"),
        ];

        for observation in observations {
            assert_eq!(observation.substrate, AndroidSubstrate::AospPlatform);
            assert_eq!(observation.provenance, None);
            match observation.event {
                AndroidEvent::NotificationPostUnavailable {
                    reason: AndroidNotificationPostUnavailableReason::AdapterUnavailable,
                    ..
                }
                | AndroidEvent::MessageUnavailable {
                    reason: AndroidMessageUnavailableReason::AdapterUnavailable,
                    ..
                }
                | AndroidEvent::PhoneCallUnavailable {
                    reason: AndroidPhoneCallUnavailableReason::AdapterUnavailable,
                    ..
                } => {}
                other => panic!("unexpected unavailable observation: {other:?}"),
            }
        }
    }

    #[test]
    fn android_observation_deserialization_rejects_aosp_success_provenance_on_sensitive_unavailable_events()
     {
        let event_payloads = [
            serde_json::json!({
                "NotificationPostUnavailable": {
                    "target": "notification-post",
                    "reason": "AdapterUnavailable",
                    "raw_source": "not connected"
                }
            }),
            serde_json::json!({
                "MessageUnavailable": {
                    "target": "messaging",
                    "reason": "AdapterUnavailable",
                    "raw_source": "not connected"
                }
            }),
            serde_json::json!({
                "PhoneCallUnavailable": {
                    "target": "phone-call",
                    "reason": "AdapterUnavailable",
                    "raw_source": "not connected"
                }
            }),
        ];

        for event in event_payloads {
            let json = serde_json::json!({
                "substrate": "AospPlatform",
                "event": event,
                "provenance": {
                    "AospPlatformEvent": {
                        "source": {
                            "service_name": "fawx-system-test",
                            "event_id": "event-1"
                        }
                    }
                }
            });

            let error = serde_json::from_value::<AndroidObservation>(json)
                .expect_err("unavailable events cannot carry success provenance");

            assert!(error.to_string().contains("success observations"));
        }
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
