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
log_path="$task_dir/heartbeat-detached.log"

select_adb_device "$@"
"${ADB[@]}" shell "mkdir -p '$bin_dir' '$task_dir'"
push_android_binary fawx-terminal-runner "$bin_dir"
"${ADB[@]}" shell "chmod +x '$bin_dir/fawx-terminal-runner'"

echo "== create detached heartbeat task =="
echo "task dir: $task_dir"
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' create task-detached-heartbeat 'prove detached checkpoint updates'"

echo "== launch detached heartbeat =="
"${ADB[@]}" shell "rm -f '$log_path'"
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' nohup '$bin_dir/fawx-terminal-runner' heartbeat task-detached-heartbeat 6 500 > '$log_path' 2>&1 < /dev/null & echo started"

echo "== immediate status =="
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' status task-detached-heartbeat"

echo "== wait for detached process =="
sleep 4

echo "== detached heartbeat log =="
"${ADB[@]}" shell "cat '$log_path'"

echo "== final status =="
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' status task-detached-heartbeat"
