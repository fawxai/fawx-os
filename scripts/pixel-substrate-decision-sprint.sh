#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
source "$script_dir/android-common.sh"
cd "$repo_root"

if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq is required for substrate decision reports" >&2
  exit 1
fi

root="/data/local/tmp/fawx-os"
bin_dir="$root/bin"
report_path="${FAWX_OS_DECISION_REPORT:-}"
if [[ -z "$report_path" ]]; then
  report_path="$(mktemp -t fawx-os-substrate-decision.XXXXXX.json)"
fi

select_adb_device "$@"
"${ADB[@]}" shell "mkdir -p '$bin_dir'"
push_android_binary fawx-android-probe "$bin_dir"
push_android_binary fawx-terminal-runner "$bin_dir"
"${ADB[@]}" shell "chmod +x '$bin_dir/fawx-android-probe' '$bin_dir/fawx-terminal-runner'"

echo "== substrate boundary gate =="
"$script_dir/pixel-substrate-compare-smoke.sh" "$ANDROID_SERIAL"

echo "== real task control-plane gate =="
"$script_dir/pixel-real-task-harness.sh" "$ANDROID_SERIAL"

echo "== collecting decision evidence =="
recon_output="$("${ADB[@]}" shell "'$bin_dir/fawx-android-probe' --substrate recon-rooted-stock" | tr -d '\r')"
aosp_output="$("${ADB[@]}" shell "'$bin_dir/fawx-android-probe' --substrate aosp-platform" | tr -d '\r')"
local_model_output="$("${ADB[@]}" shell "'$bin_dir/fawx-terminal-runner' local-model-probe" | tr -d '\r')"

device_model="$("${ADB[@]}" shell getprop ro.product.model | tr -d '\r')"
device_name="$("${ADB[@]}" shell getprop ro.product.device | tr -d '\r')"
build_fingerprint="$("${ADB[@]}" shell getprop ro.build.fingerprint | tr -d '\r')"
bootloader_locked="$("${ADB[@]}" shell getprop ro.boot.flash.locked | tr -d '\r')"

jq -n \
  --arg generated_at "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
  --arg android_serial "$ANDROID_SERIAL" \
  --arg device_model "$device_model" \
  --arg device_name "$device_name" \
  --arg build_fingerprint "$build_fingerprint" \
  --arg bootloader_locked "$bootloader_locked" \
  --argjson recon "$recon_output" \
  --argjson aosp "$aosp_output" \
  --argjson local_model "$local_model_output" '
    def require_observation($report; $name):
      (($report.observations // error("probe report missing observations"))
        | map(select(.name == $name))
        | first)
        // error("missing required observation: \($name)");
    def require_capability($report; $name):
      (($report.capability_statuses // error("probe report missing capability_statuses"))
        | map(select(.capability == $name))
        | first)
        // error("missing required capability: \($name)");
    def require_local_model_providers:
      $local_model.providers // error("local model probe missing providers");
    def local_model_signal:
      if (require_local_model_providers | map(select(.status == "PresentButNoPublicTerminalApi")) | length) > 0 then
        "present-but-no-public-terminal-api"
      elif (require_local_model_providers | map(select(.status == "Indeterminate")) | length) > 0 then
        "indeterminate"
      else
        "not-found"
      end;
    def communication_score:
      if (require_observation($aosp; "messaging").ok or require_observation($aosp; "phone-call").ok) then
        3
      else
        "U"
      end;
    {
      generated_at: $generated_at,
      device: {
        android_serial: $android_serial,
        model: $device_model,
        device: $device_name,
        build_fingerprint: $build_fingerprint,
        bootloader_locked: $bootloader_locked
      },
      recommendation: {
        decision: "do_not_buy_ssd_or_build_aosp_yet",
        summary: "The rooted-stock control spine is testable now; AOSP investment should wait until the decision sprint proves a core primitive is blocked or gross on rooted stock and cleaner through a privileged adapter.",
        next_capital_free_step: "Keep extending rooted-stock probes behind the same typed PlatformAdapter contract and update this report after each Android boundary change."
      },
      gates: {
        substrate_boundary: "passed",
        real_task_control_plane: "passed",
        aosp_default_must_stay_unavailable_without_platform_events: true
      },
      must_have_primitives: [
        {
          primitive: "foreground_observation",
          rooted_stock_signal: (require_observation($recon; "foreground") | if .ok then "typed-recon-observation" else "blocked" end),
          aosp_signal: (require_observation($aosp; "foreground") | .summary),
          current_score: 1,
          exit_pressure: "medium",
          capital_decision: "do-not-build-aosp-yet",
          next_gate: "Replace AospPlatform AdapterUnavailable with one real privileged ForegroundAppChanged event."
        },
        {
          primitive: "app_launch_resume",
          rooted_stock_signal: (require_capability($recon; "LaunchApp") | .status),
          aosp_signal: (require_observation($aosp; "app-launch") | .summary),
          current_score: 1,
          exit_pressure: "medium",
          capital_decision: "continue-rooted-stock-probing",
          next_gate: "Keep action execution and foreground closure stable before adding a privileged app-controller producer."
        },
        {
          primitive: "background_execution",
          rooted_stock_signal: (require_capability($recon; "BackgroundExecution") | .status),
          aosp_signal: (require_observation($aosp; "background-supervisor") | .summary),
          current_score: 1,
          exit_pressure: "high",
          capital_decision: "continue-rooted-stock-probing",
          next_gate: "Prove supervised background work through typed heartbeat/observation closure before AOSP checkout."
        },
        {
          primitive: "notification_read",
          rooted_stock_signal: (require_capability($recon; "ReadNotifications") | .status),
          aosp_signal: (require_observation($aosp; "notification") | .summary),
          current_score: 1,
          exit_pressure: "high",
          capital_decision: "do-not-pretend-shell-evidence-is-enough",
          next_gate: "Only AOSP listener provenance may close notification-read actions."
        },
        {
          primitive: "communication_surface",
          rooted_stock_signal: {
            send_message: (require_capability($recon; "SendMessage") | .status),
            place_call: (require_capability($recon; "PlaceCall") | .status)
          },
          aosp_signal: {
            messaging: (require_observation($aosp; "messaging") | .summary),
            phone_call: (require_observation($aosp; "phone-call") | .summary)
          },
          current_score: communication_score,
          exit_pressure: "high",
          capital_decision: "insufficient-evidence",
          next_gate: "Prototype one explicit messaging or calling adapter contract before platform commitment."
        },
        {
          primitive: "local_model_access",
          rooted_stock_signal: local_model_signal,
          aosp_signal: "no privileged/local-provider inference bridge yet",
          current_score: 1,
          exit_pressure: "medium",
          capital_decision: "probe-api-surface-before-aosp",
          next_gate: "Determine whether AICore/Gemini exposes a callable API we can bridge into IntentCandidate."
        }
      ],
      raw_evidence: {
        recon_probe: $recon,
        aosp_probe: $aosp,
        local_model_probe: $local_model
      }
    }
  ' | tee "$report_path"

echo "== report =="
echo "$report_path"
