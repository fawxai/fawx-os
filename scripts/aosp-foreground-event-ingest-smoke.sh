#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
cd "$repo_root"

if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq is required for typed AOSP foreground ingest assertions" >&2
  exit 1
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
event_file="$tmp_dir/aosp-foreground-event.json"

cat >"$event_file" <<'JSON'
{
  "package_name": "com.android.settings",
  "activity_name": "com.android.settings.Settings",
  "source": {
    "service_name": "fawx-system-foreground-observer",
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

echo "== AOSP platform default remains unavailable =="
default_output="$(
  cargo run -q -p fawx-android-probe -- --substrate aosp-platform
)"
echo "$default_output"

assert_json "$default_output" '[.observations[] | select(.android_observation.event.ForegroundObservationUnavailable.reason == "AdapterUnavailable")] | length == 1' "AOSP foreground remains unavailable without platform event source"
assert_json "$default_output" '[.observations[] | select(.android_observation.event.ForegroundAppChanged != null)] | length == 0' "AOSP default path must not synthesize foreground success"

echo "== AOSP platform foreground event ingest =="
ingest_output="$(
  cargo run -q -p fawx-android-probe -- \
    --substrate aosp-platform \
    --aosp-foreground-event-file "$event_file"
)"
echo "$ingest_output"

assert_json "$ingest_output" '[.observations[] | select(.name == "aosp-platform-adapter" and .ok == true)] | length == 1' "AOSP platform adapter reports explicit event source"
assert_json "$ingest_output" '[.observations[] | select(.android_observation.substrate == "AospPlatform" and .android_observation.event.ForegroundAppChanged.package_name == "com.android.settings")] | length == 1' "AOSP foreground success comes from typed platform event"

echo "PASS AOSP foreground event ingest smoke"
