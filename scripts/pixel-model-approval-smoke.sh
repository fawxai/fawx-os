#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
source "$script_dir/android-common.sh"
cd "$repo_root"

root="/data/local/tmp/fawx-os"
bin_dir="$root/bin"
run_id="$(date +%Y%m%d%H%M%S)-$$"
task_dir="$root/tasks/model-approval-$run_id"

select_adb_device "$@"
"${ADB[@]}" shell "mkdir -p '$bin_dir' '$task_dir'"
push_android_binary fawx-terminal-runner "$bin_dir"
"${ADB[@]}" shell "chmod +x '$bin_dir/fawx-terminal-runner'"

echo "== model-candidate approval smoke =="
echo "task dir: $task_dir"

"${ADB[@]}" shell "input keyevent KEYCODE_HOME"
sleep 1

suggest_output="$(
  "${ADB[@]}" shell "printf '%s\n' 'suggest open settings' 'quit' | FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' session" \
    | tr -d '\r'
)"
echo "$suggest_output"

task_id="$(sed -n 's/.*approve with: approve //p' <<<"$suggest_output" | tail -n 1 | awk '{print $1}')"
if [[ -z "$task_id" ]]; then
  echo "assertion failed: model candidate did not print an approve-by-id task" >&2
  exit 1
fi

approve_output="$(
  "${ADB[@]}" shell "printf '%s\n' 'suggest open settings' 'approve last' 'quit' | FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' session" \
    | tr -d '\r'
)"
echo "$approve_output"

by_id_output="$(
  "${ADB[@]}" shell "input keyevent KEYCODE_HOME >/dev/null; printf '%s\n' 'approve $task_id' 'quit' | FAWX_OS_TASK_DIR='$task_dir' '$bin_dir/fawx-terminal-runner' session" \
    | tr -d '\r'
)"
echo "$by_id_output"

require_line() {
  local pattern="$1"
  local message="$2"
  local haystack="$3"
  if ! grep -Fq "$pattern" <<<"$haystack"; then
    echo "assertion failed: $message" >&2
    exit 1
  fi
}

require_line "needs confirmation:" "model candidate paused for owner approval" "$suggest_output"
require_line "approve with: approve $task_id" "pending task can be approved by id" "$suggest_output"
require_line "needs confirmation:" "approve-last setup paused for owner approval" "$approve_output"
require_line "accepted: OpenApp Accepted" "approve last accepted the stored candidate action" "$approve_output"
require_line "done: com.android.settings is foreground" "approve last closed from foreground evidence" "$approve_output"
require_line "accepted: OpenApp Accepted" "approve by id accepted the stored candidate action" "$by_id_output"
require_line "executed: runtime launch command succeeded" "approve by id executed" "$by_id_output"
require_line "done: com.android.settings is foreground" "approve by id closed from foreground evidence" "$by_id_output"

echo "PASS model-candidate approval smoke"
