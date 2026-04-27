#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
cd "$repo_root"

if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq is required for typed AOSP app launch assertions" >&2
  exit 1
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
result_file="$tmp_dir/aosp-app-launch-result.json"

# This is a contract fixture, not live platform proof. It verifies that the
# runtime side accepts only the typed app-controller result shape; the escape
# rubric stays at score 1 until a real privileged AOSP service emits it.
cat >"$result_file" <<'JSON'
{
  "package_name": "com.android.settings",
  "activity_name": "com.android.settings.Settings",
  "source": {
    "service_name": "fawx-system-app-controller",
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

echo "== AOSP platform default has no app controller =="
default_output="$(
  cargo run -q -p fawx-android-probe -- --substrate aosp-platform
)"
echo "$default_output"

assert_json "$default_output" '[.observations[] | select(.android_observation.event.AppLaunchUnavailable.reason == "AdapterUnavailable")] | length == 1' "AOSP app launch remains unavailable without platform result source"
assert_json "$default_output" '[.observations[] | select(.android_observation.event.AppLaunchCompleted != null)] | length == 0' "AOSP default path must not synthesize app launch success"

echo "== AOSP platform app launch result ingest =="
ingest_output="$(
  cargo run -q -p fawx-android-probe -- \
    --substrate aosp-platform \
    --aosp-app-launch-result-file "$result_file"
)"
echo "$ingest_output"

assert_json "$ingest_output" '[.observations[] | select(.name == "aosp-app-controller" and .ok == true)] | length == 1' "AOSP app controller reports explicit result source"
assert_json "$ingest_output" '[.observations[] | select(.android_observation.substrate == "AospPlatform" and .android_observation.event.AppLaunchCompleted.package_name == "com.android.settings")] | length == 1' "AOSP app launch success comes from typed platform result"
assert_json "$ingest_output" '[.observations[] | select(.android_observation.provenance.AospPlatformEvent.source.service_name == "fawx-system-app-controller")] | length == 1' "AOSP app launch observation preserves platform provenance"

echo "PASS AOSP app launch ingest smoke"
