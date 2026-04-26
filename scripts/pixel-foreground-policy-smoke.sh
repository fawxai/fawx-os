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
log_path="$task_dir/foreground-policy.log"

select_adb_device "$@"
"${ADB[@]}" shell "mkdir -p '$bin_dir' '$task_dir'"
push_android_binary fawx-terminal-runner "$bin_dir"
"${ADB[@]}" shell "chmod +x '$bin_dir/fawx-terminal-runner'"

echo "== create foreground policy task =="
echo "task dir: $task_dir"
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' create task-foreground-policy 'watch settings foreground policy'"

echo "== launch foreground policy watcher =="
"${ADB[@]}" shell "rm -f '$log_path'"
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' nohup '$bin_dir/fawx-terminal-runner' watch-foreground task-foreground-policy com.android.settings 8 500 > '$log_path' 2>&1 < /dev/null & echo started"

sleep 1
echo "== switch foreground to settings =="
"${ADB[@]}" shell "am start -a android.settings.SETTINGS >/dev/null"

sleep 1
echo "== switch foreground to launcher =="
"${ADB[@]}" shell "input keyevent KEYCODE_HOME"

sleep 4

echo "== foreground policy log =="
"${ADB[@]}" shell "cat '$log_path'"

echo "== final status =="
"${ADB[@]}" shell "FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' status task-foreground-policy"
