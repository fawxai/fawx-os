#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
cd "$repo_root"

if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq is required for typed AOSP background supervisor assertions" >&2
  exit 1
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
event_file="$tmp_dir/aosp-background-supervisor-event.json"

cat >"$event_file" <<'JSON'
{
  "supervisor_id": "supervisor-1",
  "active_tasks": 2,
  "source": {
    "service_name": "fawx-system-background-supervisor",
    "event_id": "smoke-event-1"
  }
}
JSON

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

echo "== AOSP platform default has no background supervisor =="
default_output="$(
  cargo run -q -p fawx-android-probe -- --substrate aosp-platform
)"
echo "$default_output"

assert_json "$default_output" '[.observations[] | select(.android_observation.event.BackgroundSupervisorUnavailable.reason == "AdapterUnavailable")] | length == 1' "AOSP background supervisor remains unavailable without platform event source"
assert_json "$default_output" '[.observations[] | select(.android_observation.event.BackgroundSupervisorHeartbeat != null)] | length == 0' "AOSP default path must not synthesize supervisor heartbeat"

echo "== AOSP platform background supervisor event ingest =="
ingest_output="$(
  cargo run -q -p fawx-android-probe -- \
    --substrate aosp-platform \
    --aosp-background-supervisor-event-file "$event_file"
)"
echo "$ingest_output"

assert_json "$ingest_output" '[.observations[] | select(.name == "aosp-background-supervisor" and .ok == true)] | length == 1' "AOSP background supervisor reports explicit event source"
assert_json "$ingest_output" '[.observations[] | select(.android_observation.substrate == "AospPlatform" and .android_observation.event.BackgroundSupervisorHeartbeat.supervisor_id == "supervisor-1" and .android_observation.event.BackgroundSupervisorHeartbeat.active_tasks == 2)] | length == 1' "AOSP supervisor success comes from typed platform event"
assert_json "$ingest_output" '[.observations[] | select(.android_observation.provenance.AospPlatformEvent.source.service_name == "fawx-system-background-supervisor")] | length == 1' "AOSP supervisor observation preserves platform provenance"

echo "PASS AOSP background supervisor ingest smoke"
