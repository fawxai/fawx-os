#!/usr/bin/env bash

select_adb_device() {
  if (($# > 1)); then
    echo "usage: ${0##*/} [android-serial]" >&2
    return 2
  fi

  local requested_serial="${1:-}"
  if [[ -n "$requested_serial" && -n "${ANDROID_SERIAL:-}" && "$requested_serial" != "$ANDROID_SERIAL" ]]; then
    echo "ANDROID_SERIAL=$ANDROID_SERIAL conflicts with script argument $requested_serial" >&2
    return 2
  fi

  local serial="${requested_serial:-${ANDROID_SERIAL:-}}"
  if [[ -z "$serial" ]]; then
    local connected_serial
    local connected_serials=()
    while IFS= read -r connected_serial; do
      connected_serials+=("$connected_serial")
    done < <(adb devices | awk 'NR > 1 && $2 == "device" { print $1 }')

    case "${#connected_serials[@]}" in
      0)
        echo "no adb devices are connected; pass a serial or set ANDROID_SERIAL" >&2
        return 2
        ;;
      1)
        serial="${connected_serials[0]}"
        ;;
      *)
        echo "multiple adb devices are connected; pass a serial or set ANDROID_SERIAL" >&2
        adb devices -l >&2
        return 2
        ;;
    esac
  fi

  ADB=(adb -s "$serial")
  export ANDROID_SERIAL="$serial"
  echo "== adb device: $serial =="
  "${ADB[@]}" get-state >/dev/null
  adb devices -l
}

require_android_binary() {
  local binary_name="$1"
  local binary_path="target/aarch64-linux-android/release/$binary_name"

  if [[ ! -f "$binary_path" ]]; then
    echo "missing Android binary: $binary_path" >&2
    echo "run ./scripts/android-build.sh before this smoke script" >&2
    return 2
  fi

  printf '%s\n' "$binary_path"
}

push_android_binary() {
  local binary_name="$1"
  local remote_dir="$2"
  local binary_path

  binary_path="$(require_android_binary "$binary_name")"
  "${ADB[@]}" push "$binary_path" "$remote_dir/"
}
