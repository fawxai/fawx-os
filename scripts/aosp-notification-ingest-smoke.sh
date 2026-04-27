#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
cd "$repo_root"

if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq is required for typed AOSP notification assertions" >&2
  exit 1
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
event_file="$tmp_dir/aosp-notification-event.json"

# This is a contract fixture, not live platform proof. It verifies that the
# runtime side accepts only the typed notification-listener event shape; the
# escape rubric stays at score 1 until a real privileged AOSP service emits it.
cat >"$event_file" <<'JSON'
{
  "app_package_name": "com.example.mail",
  "summary": "New message from Ada",
  "source": {
    "service_name": "fawx-system-notification-listener",
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

echo "== AOSP platform default has no notification listener =="
default_output="$(
  cargo run -q -p fawx-android-probe -- --substrate aosp-platform
)"
echo "$default_output"

assert_json "$default_output" '[.observations[] | select(.android_observation.event.NotificationUnavailable.reason == "AdapterUnavailable")] | length == 1' "AOSP notification read remains unavailable without platform event source"
assert_json "$default_output" '[.observations[] | select(.android_observation.event.NotificationReceived != null)] | length == 0' "AOSP default path must not synthesize notification success"

echo "== AOSP platform notification event ingest =="
ingest_output="$(
  cargo run -q -p fawx-android-probe -- \
    --substrate aosp-platform \
    --aosp-notification-event-file "$event_file"
)"
echo "$ingest_output"

assert_json "$ingest_output" '[.observations[] | select(.name == "aosp-notification-listener" and .ok == true)] | length == 1' "AOSP notification listener reports explicit event source"
assert_json "$ingest_output" '[.observations[] | select(.android_observation.substrate == "AospPlatform" and .android_observation.event.NotificationReceived.source == "com.example.mail" and .android_observation.event.NotificationReceived.summary == "New message from Ada")] | length == 1' "AOSP notification success comes from typed platform event"
assert_json "$ingest_output" '[.observations[] | select(.android_observation.provenance.AospPlatformEvent.source.service_name == "fawx-system-notification-listener")] | length == 1' "AOSP notification observation preserves platform provenance"

echo "PASS AOSP notification ingest smoke"
