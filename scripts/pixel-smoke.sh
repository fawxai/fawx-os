#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
source "$script_dir/android-common.sh"
cd "$repo_root"

root="/data/local/tmp/fawx-os"
bin_dir="$root/bin"
run_id="$(date +%Y%m%d%H%M%S)-$$"
task_dir="$root/tasks/$run_id"

select_adb_device "$@"
"${ADB[@]}" shell "mkdir -p '$bin_dir' '$task_dir'"
push_android_binary fawx-android-probe "$bin_dir"
push_android_binary fawx-terminal-runner "$bin_dir"
"${ADB[@]}" shell "chmod +x '$bin_dir/fawx-android-probe' '$bin_dir/fawx-terminal-runner'"

echo "== probe =="
"${ADB[@]}" shell "$bin_dir/fawx-android-probe"

echo "== task lifecycle =="
echo "task dir: $task_dir"
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' create task-demo 'cancel that subscription'"
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' checkpoint task-demo 'created on device'"
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' block-foreground task-demo 'target app needs foreground focus'"
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' status task-demo"

echo "== background heartbeat =="
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' create task-heartbeat 'prove background checkpoint updates'"
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' heartbeat task-heartbeat 3 250"
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' status task-heartbeat"

echo "== deterministic agent step =="
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' create task-agent-step 'prove agent loop runner integration'"
agent_step_output="$("${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' agent-step task-agent-step")"
echo "$agent_step_output"
[[ "$agent_step_output" == *'"phase": "Checkpointed"'* ]]
[[ "$agent_step_output" == *'"ContinueLocalWork"'* ]]
[[ "$agent_step_output" == *'"checkpoint_id": "loop:initial-plan"'* ]]
[[ "$agent_step_output" == *'"current_activity"'* ]]
[[ "$agent_step_output" == *'"kind": "Planning"'* ]]
[[ "$agent_step_output" == *'"target": "Task"'* ]]
[[ "$agent_step_output" == *'"source": "SystemDerived"'* ]]

echo "== background runner tick =="
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' create task-background-a 'prove background runner can tick task a'"
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' create task-background-b 'prove background runner can tick task b'"
background_tick_output="$("${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' background-tick 1 0")"
echo "$background_tick_output"
[[ "$background_tick_output" == *'"tick_id": 1'* ]]
[[ "$background_tick_output" == *'"task_id": "task-background-a"'* ]]
[[ "$background_tick_output" == *'"task_id": "task-background-b"'* ]]
[[ "$background_tick_output" == *'"Stepped"'* ]]
[[ "$background_tick_output" == *'"ContinueLocalWork"'* ]]

"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' create task-agent-model-activity 'prove model declared activity contract'"
agent_model_activity_output="$("${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' agent-step task-agent-model-activity --activity-kind observing --activity-description 'checking settings state' --activity-target android-package:com.android.settings")"
echo "$agent_model_activity_output"
[[ "$agent_model_activity_output" == *'"ContinueLocalWork"'* ]]
[[ "$agent_model_activity_output" == *'"current_activity"'* ]]
[[ "$agent_model_activity_output" == *'"kind": "Observing"'* ]]
[[ "$agent_model_activity_output" == *'"AndroidPackage"'* ]]
[[ "$agent_model_activity_output" == *'"package_name": "com.android.settings"'* ]]
[[ "$agent_model_activity_output" == *'"description": "checking settings state"'* ]]
[[ "$agent_model_activity_output" == *'"source": "ModelDeclared"'* ]]
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' create task-agent-model-action 'prove model action proposal contract'"
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' grant task-agent-model-action app-control android-package:com.android.settings"
agent_model_action_output="$("${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' agent-step task-agent-model-action --action-kind open-app --action-reason 'open settings to inspect permissions' --action-target android-package:com.android.settings --expected-observation 'settings is foreground'")"
echo "$agent_model_action_output"
[[ "$agent_model_action_output" == *'"ContinueLocalWork"'* ]]
[[ "$agent_model_action_output" == *'"current_action"'* ]]
[[ "$agent_model_action_output" == *'"kind": "OpenApp"'* ]]
[[ "$agent_model_action_output" == *'"AndroidPackage"'* ]]
[[ "$agent_model_action_output" == *'"package_name": "com.android.settings"'* ]]
[[ "$agent_model_action_output" == *'"reason": "open settings to inspect permissions"'* ]]
[[ "$agent_model_action_output" == *'"expected_observation": "settings is foreground"'* ]]
[[ "$agent_model_action_output" == *'"status": "Accepted"'* ]]
[[ "$agent_model_action_output" == *'"id": "model-action:task-agent-model-action:1"'* ]]
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' create task-agent-action-closure 'prove action execution and observation closure'"
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' grant task-agent-action-closure app-control android-package:com.google.android.apps.nexuslauncher"
agent_action_accept_output="$("${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' agent-step task-agent-action-closure --action-kind open-app --action-reason 'open launcher to inspect home screen' --action-target android-package:com.google.android.apps.nexuslauncher --expected-observation 'launcher is foreground'")"
echo "$agent_action_accept_output"
[[ "$agent_action_accept_output" == *'"status": "Accepted"'* ]]
agent_action_begin_output="$("${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' begin-action task-agent-action-closure")"
echo "$agent_action_begin_output"
[[ "$agent_action_begin_output" == *'"status": "Executing"'* ]]
[[ "$agent_action_begin_output" == *'"state": "Prepared"'* ]]
background_action_tick_output="$("${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' background-tick 1 0 --foreground")"
echo "$background_action_tick_output"
[[ "$background_action_tick_output" == *'"task_id": "task-agent-action-closure"'* ]]
[[ "$background_action_tick_output" == *'"Stepped"'* ]]
agent_action_observed_output="$("${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' status task-agent-action-closure")"
echo "$agent_action_observed_output"
[[ "$agent_action_observed_output" == *'"status": "Observed"'* ]]
[[ "$agent_action_observed_output" == *'"state": "Committed"'* ]]
[[ "$agent_action_observed_output" == *'"last_observation"'* ]]
[[ "$agent_action_observed_output" == *'"ForegroundPackage"'* ]]
[[ "$agent_action_observed_output" == *'"package_name": "com.google.android.apps.nexuslauncher"'* ]]
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' create task-agent-foreground 'prove agent loop foreground contract'"
agent_foreground_output="$("${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' agent-step task-agent-foreground --expected-foreground com.android.settings")"
echo "$agent_foreground_output"
[[ "$agent_foreground_output" == *'"phase": "Waiting"'* ]]
[[ "$agent_foreground_output" == *'"WaitingForForeground"'* ]]
[[ "$agent_foreground_output" == *'"ReacquireForeground"'* ]]
[[ "$agent_foreground_output" == *'"current_activity"'* ]]
[[ "$agent_foreground_output" == *'"kind": "Waiting"'* ]]
[[ "$agent_foreground_output" == *'"AndroidPackage"'* ]]
[[ "$agent_foreground_output" == *'"package_name": "com.android.settings"'* ]]
[[ "$agent_foreground_output" == *'"source": "SystemDerived"'* ]]
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' create task-agent-sampled-foreground 'prove sampled foreground observation contract'"
agent_sampled_foreground_output="$("${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' agent-step task-agent-sampled-foreground --expected-foreground com.android.settings --sample-foreground")"
echo "$agent_sampled_foreground_output"
[[ "$agent_sampled_foreground_output" == *'"last_runtime_observation"'* ]]
[[ "$agent_sampled_foreground_output" == *'"ForegroundAppChanged"'* ]]
[[ "$agent_sampled_foreground_output" == *'"ReacquireForeground"'* ]]
[[ "$agent_sampled_foreground_output" == *'"current_activity"'* ]]
[[ "$agent_sampled_foreground_output" == *'"kind": "Waiting"'* ]]
[[ "$agent_sampled_foreground_output" == *'"AndroidPackage"'* ]]
[[ "$agent_sampled_foreground_output" == *'"package_name": "com.android.settings"'* ]]
[[ "$agent_sampled_foreground_output" == *'"source": "SystemDerived"'* ]]
