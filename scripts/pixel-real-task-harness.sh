#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
source "$script_dir/android-common.sh"
cd "$repo_root"

if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq is required for typed harness assertions" >&2
  exit 1
fi

root="/data/local/tmp/fawx-os"
bin_dir="$root/bin"
run_id="$(date +%Y%m%d%H%M%S)-$$"
task_dir="$root/tasks/real-task-$run_id"

passed=0
failed=0
current_case=""

select_adb_device "$@"
"${ADB[@]}" shell "mkdir -p '$bin_dir' '$task_dir'"
push_android_binary fawx-android-probe "$bin_dir"
push_android_binary fawx-terminal-runner "$bin_dir"
"${ADB[@]}" shell "chmod +x '$bin_dir/fawx-android-probe' '$bin_dir/fawx-terminal-runner'"

echo "== real task harness =="
echo "task dir: $task_dir"

finish() {
  local status=$?
  if [[ $status -ne 0 && -n "$current_case" ]]; then
    echo "FAIL $current_case"
    failed=$((failed + 1))
    current_case=""
    summary
  fi
  exit "$status"
}

trap finish EXIT

summary() {
  echo "== score =="
  jq -n \
    --argjson passed "$passed" \
    --argjson failed "$failed" \
    --arg task_dir "$task_dir" \
    '{passed: $passed, failed: $failed, task_dir: $task_dir}'
}

begin_case() {
  current_case="$1"
  echo "== case: $current_case =="
}

pass_case() {
  echo "PASS $current_case"
  passed=$((passed + 1))
  current_case=""
}

run_probe() {
  "${ADB[@]}" shell "'$bin_dir/fawx-android-probe'" | tr -d '\r'
}

run_runner() {
  "${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' $*" | tr -d '\r'
}

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

begin_case "probe foreground observation"
probe_output="$(run_probe)"
echo "$probe_output"
assert_json "$probe_output" '.substrate == "ReconRootedStock"' "probe substrate is rooted-stock recon"
assert_json "$probe_output" '[.observations[] | select(.name == "foreground" and .ok == true and .android_observation.event.ForegroundAppChanged.package_name != null)] | length == 1' "probe reports typed foreground package"
pass_case

begin_case "heartbeat checkpoints background task"
run_runner "create task-heartbeat 'prove heartbeat checkpointing'" >/dev/null
heartbeat_output="$(run_runner "heartbeat task-heartbeat 3 100")"
echo "$heartbeat_output"
heartbeat_status="$(run_runner "status task-heartbeat")"
echo "$heartbeat_status"
assert_json "$heartbeat_status" '.state.phase == "Checkpointed"' "heartbeat task reaches checkpointed phase"
assert_json "$heartbeat_status" '.state.checkpoint.action_boundary.state == "Verified"' "heartbeat checkpoint is verified"
assert_json "$heartbeat_status" '.state.checkpoint.action_boundary.id == "heartbeat:3/3"' "heartbeat records final tick boundary"
pass_case

begin_case "runtime-owned open-app action closes from typed foreground evidence"
run_runner "create task-open-settings 'prove runtime-owned app launch observation closure'" >/dev/null
run_runner "grant task-open-settings app-control android-package:com.android.settings" >/dev/null
"${ADB[@]}" shell "input keyevent KEYCODE_HOME"
sleep 1
accept_output="$(run_runner "agent-step task-open-settings --action-kind open-app --action-reason 'open settings to inspect system state' --action-target android-package:com.android.settings --expected-observation 'settings is foreground'")"
echo "$accept_output"
assert_json "$accept_output" '.task.state.current_action.status == "Accepted"' "model action is accepted"
execute_output="$(run_runner "execute-action task-open-settings")"
echo "$execute_output"
assert_json "$execute_output" '.task.state.current_action.status == "Executing"' "runtime begins action execution"
assert_json "$execute_output" '.execution.success == true' "runtime app launch command succeeds"
sleep 1
tick_output="$(run_runner "background-tick 1 0 --foreground")"
echo "$tick_output"
status_output="$(run_runner "status task-open-settings")"
echo "$status_output"
assert_json "$status_output" '.state.current_action.status == "Observed"' "action becomes observed"
assert_json "$status_output" '.state.current_action.boundary.state == "Committed"' "action boundary commits"
assert_json "$status_output" '.state.current_action.last_observation.evidence.ForegroundPackage.package_name == "com.android.settings"' "foreground evidence is attached to action"
pass_case

begin_case "foreground handoff resumes from matching foreground observation"
run_runner "create task-settings-handoff 'prove foreground handoff evidence'" >/dev/null
handoff_output="$(run_runner "agent-step task-settings-handoff --expected-foreground com.android.settings")"
echo "$handoff_output"
assert_json "$handoff_output" '.task.state.phase == "Waiting"' "task waits for foreground"
assert_json "$handoff_output" '.task.state.current_handoff.resume_condition.ForegroundPackage.package_name == "com.android.settings"' "foreground handoff is typed"
"${ADB[@]}" shell "am start -a android.settings.SETTINGS >/dev/null"
sleep 1
foreground_tick="$(run_runner "background-tick 1 0 --foreground")"
echo "$foreground_tick"
foreground_status="$(run_runner "status task-settings-handoff")"
echo "$foreground_status"
assert_json "$foreground_status" '.state.blocker == null' "foreground blocker clears"
assert_json "$foreground_status" '.state.current_handoff == null' "foreground handoff clears"
assert_json "$foreground_status" '.state.completed_handoffs | length == 1' "foreground handoff evidence recorded"
assert_json "$foreground_status" '.state.completed_handoffs[0].condition.ForegroundPackage.package_name == "com.android.settings"' "completed handoff records foreground package"
pass_case

begin_case "manual handoff resumes generic foreground blocker"
run_runner "create task-manual-handoff 'prove explicit user handoff completion'" >/dev/null
manual_block="$(run_runner "block-foreground task-manual-handoff 'needs human foreground help'")"
echo "$manual_block"
assert_json "$manual_block" '.state.current_handoff.resume_condition == "Manual"' "generic foreground handoff is manual"
manual_complete="$(run_runner "complete-handoff task-manual-handoff handoff:task-manual-handoff:manual 'user confirmed foreground help is complete'")"
echo "$manual_complete"
assert_json "$manual_complete" '.state.blocker == null' "manual foreground blocker clears"
assert_json "$manual_complete" '.state.current_handoff == null' "manual handoff clears"
assert_json "$manual_complete" '.state.completed_handoffs | length == 1' "manual completion evidence recorded"
pass_case

summary
