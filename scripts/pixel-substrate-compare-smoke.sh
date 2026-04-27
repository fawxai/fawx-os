#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
source "$script_dir/android-common.sh"
cd "$repo_root"

if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq is required for typed substrate assertions" >&2
  exit 1
fi

root="/data/local/tmp/fawx-os"
bin_dir="$root/bin"

select_adb_device "$@"
"${ADB[@]}" shell "mkdir -p '$bin_dir'"
push_android_binary fawx-android-probe "$bin_dir"
"${ADB[@]}" shell "chmod +x '$bin_dir/fawx-android-probe'"

echo "== rooted-stock recon substrate =="
recon_output="$("${ADB[@]}" shell "'$bin_dir/fawx-android-probe' --substrate recon-rooted-stock" | tr -d '\r')"
echo "$recon_output"

echo "== AOSP platform contract substrate =="
aosp_output="$("${ADB[@]}" shell "'$bin_dir/fawx-android-probe' --substrate aosp-platform" | tr -d '\r')"
echo "$aosp_output"

assert_json() {
  local json="$1"
  local filter="$2"
  local message="$3"
  if ! jq -e "$filter" >/dev/null <<<"$json"; then
    echo "assertion failed: $message" >&2
    echo "$json" >&2
    exit 1
  fi
}

assert_json "$recon_output" '.substrate == "ReconRootedStock"' "recon probe labels rooted-stock substrate"
assert_json "$recon_output" '[.observations[] | select(.name == "foreground")] | length == 1' "recon probe includes foreground observation"
assert_json "$recon_output" '.capability_statuses | length > 0' "recon probe emits capability map projection"
assert_json "$recon_output" '[.capability_statuses[] | select(.capability == "RootShell" and .status == "Limited")] | length == 1' "recon root shell remains a limited recon escape hatch"

assert_json "$aosp_output" '.substrate == "AospPlatform"' "AOSP probe labels platform substrate"
assert_json "$aosp_output" '[.observations[] | select(.name == "aosp-platform-adapter" and .ok == false)] | length == 1' "AOSP probe reports adapter connectivity"
assert_json "$aosp_output" '[.observations[] | select(.summary == "AOSP platform adapter is not connected in this terminal binary")] | length == 1' "AOSP probe does not fake platform connectivity"
assert_json "$aosp_output" '[.observations[] | select(.android_observation.event.ForegroundObservationUnavailable.reason == "AdapterUnavailable")] | length == 1' "AOSP foreground observation is explicitly unavailable"
assert_json "$aosp_output" '[.observations[] | select(.android_observation.event.ForegroundAppChanged != null)] | length == 0' "AOSP probe emits no shell-backed foreground success"
assert_json "$aosp_output" '[.observations[] | select(.android_observation.event.BackgroundSupervisorUnavailable.reason == "AdapterUnavailable")] | length == 1' "AOSP background supervisor is explicitly unavailable"
assert_json "$aosp_output" '[.observations[] | select(.android_observation.event.BackgroundSupervisorHeartbeat != null)] | length == 0' "AOSP probe emits no recon-backed supervisor success"
assert_json "$aosp_output" '[.observations[] | select(.android_observation.event.AppLaunchUnavailable.reason == "AdapterUnavailable")] | length == 1' "AOSP app launch is explicitly unavailable"
assert_json "$aosp_output" '[.observations[] | select(.android_observation.event.AppLaunchCompleted != null)] | length == 0' "AOSP probe emits no recon-backed app launch success"
assert_json "$aosp_output" '[.observations[] | select(.android_observation.event.NotificationUnavailable.reason == "AdapterUnavailable")] | length == 1' "AOSP notification read is explicitly unavailable"
assert_json "$aosp_output" '[.observations[] | select(.android_observation.event.NotificationReceived != null)] | length == 0' "AOSP probe emits no recon-backed notification success"
assert_json "$aosp_output" '[.capability_statuses[] | select(.capability == "PlaceCall" and .status == "Available")] | length == 1' "AOSP probe shows privileged platform capabilities from typed map"
assert_json "$aosp_output" '[.capability_statuses[] | select(.capability == "RootShell" and .status == "Unavailable")] | length == 1' "AOSP probe marks root shell unavailable as a platform primitive"

echo "PASS substrate comparison smoke"
