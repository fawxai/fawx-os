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
