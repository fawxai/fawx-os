#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
source "$script_dir/android-common.sh"
cd "$repo_root"

if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq is required for control gate assertions" >&2
  exit 1
fi

root="/data/local/tmp/fawx-os"
bin_dir="$root/bin"
run_id="$(date +%Y%m%d%H%M%S)-$$"
probe_file="$root/probes/control-gates-$run_id.txt"

select_adb_device "$@"
"${ADB[@]}" shell "mkdir -p '$bin_dir' '$root/probes'"
push_android_binary fawx-android-probe "$bin_dir"
push_android_binary fawx-terminal-runner "$bin_dir"
"${ADB[@]}" shell "chmod +x '$bin_dir/fawx-android-probe' '$bin_dir/fawx-terminal-runner'"

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

assert_json_with_home_package() {
  local json="$1"
  local home_package="$2"
  local filter="$3"
  local message="$4"
  if ! jq -e --arg home_package "$home_package" "$filter" >/dev/null <<<"$json"; then
    echo "assertion failed: $message" >&2
    echo "$json" >&2
    exit 1
  fi
}

run_runner() {
  "${ADB[@]}" shell "$bin_dir/fawx-terminal-runner" "$@" | tr -d '\r'
}

run_probe() {
  "${ADB[@]}" shell "'$bin_dir/fawx-android-probe'" | tr -d '\r'
}

resolve_home_package() {
  local component
  component="$("${ADB[@]}" shell cmd package resolve-activity --brief -a android.intent.action.MAIN -c android.intent.category.HOME | tr -d '\r' | tail -n 1)"
  if [[ -z "$component" || "$component" != */* ]]; then
    echo "failed to resolve home activity component" >&2
    exit 1
  fi
  printf '%s\n' "${component%%/*}"
}

expect_android_command_failure() {
  local label="$1"
  shift
  local output
  if output="$(run_runner android-command "$@" 2>&1)"; then
    echo "expected android-command failure for $label" >&2
    echo "$output" >&2
    exit 1
  fi
  printf '%s\n' "$output"
  if ! grep -q "RequiresAospPrivilege" <<<"$output"; then
    echo "expected RequiresAospPrivilege for $label" >&2
    exit 1
  fi
}

echo "== gate 3: typed input/computer-use control =="
home_package="$(resolve_home_package)"
run_runner android-command keyevent KEYCODE_HOME >/dev/null
sleep 1
foreground_output="$(run_probe)"
echo "$foreground_output"
assert_json_with_home_package "$foreground_output" "$home_package" '[.observations[] | select(.name == "foreground" and .ok == true and .android_observation.event.ForegroundAppChanged.package_name == $home_package)] | length == 1' "KEYCODE_HOME should produce home foreground evidence"

echo "== gate 4: notification post remains explicit unavailable contract =="
expect_android_command_failure "notification post" \
  post-notification Fawx "control gate notification"

echo "== gate 5: communication surfaces remain explicit unavailable contracts =="
expect_android_command_failure "send message" send-message Ada "hello from fawx-os"
expect_android_command_failure "place call" place-call +15555550100

echo "== gate 6: local model probe reports package surfaces without claiming inference =="
local_model_output="$(run_runner local-model-probe)"
echo "$local_model_output"
assert_json "$local_model_output" '.providers | length == 3' "local model probe should report known provider buckets"
assert_json "$local_model_output" '[.providers[] | select(.status == "PresentButNoPublicTerminalApi")] | length >= 1' "local model package presence must not claim callable inference"

echo "== gate 7: runtime scratch storage read/write uses typed file commands =="
write_output="$(run_runner android-command write-file "$probe_file" "fawx control gate storage proof")"
echo "$write_output"
assert_json "$write_output" '.execution.success == true' "runtime scratch write succeeds"
read_output="$(run_runner android-command read-file "$probe_file")"
echo "$read_output"
assert_json "$read_output" '.execution.success == true and (.execution.stdout | contains("fawx control gate storage proof"))' "runtime scratch read returns written contents"

echo "== gate 8: human handoff approval path =="
"$script_dir/pixel-model-approval-smoke.sh" "$ANDROID_SERIAL"

echo "PASS Pixel control gates smoke"
