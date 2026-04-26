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
