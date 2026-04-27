#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/android-common.sh"

export PATH="$HOME/bin:$PATH"

workspace="${FAWX_AOSP_WORKSPACE:-$HOME/aosp-fawx}"
repo_root="$(cd -- "$script_dir/.." && pwd -P)"
min_free_gib="${FAWX_AOSP_MIN_FREE_GIB:-400}"
expected_device="${FAWX_AOSP_DEVICE_CODENAME:-blazer}"
require_unlocked="${FAWX_AOSP_REQUIRE_UNLOCKED:-1}"
check_device=1

usage() {
  cat >&2 <<'USAGE'
usage: aosp-workspace-preflight.sh [--workspace PATH] [--min-free-gib N] [--device-codename NAME] [--skip-device-check] [android-serial]

Checks whether this machine/device is ready for the first real AOSP prototype.
No AOSP source is downloaded and no device state is changed.

Environment:
  FAWX_AOSP_WORKSPACE          default: ~/aosp-fawx
  FAWX_AOSP_MIN_FREE_GIB       default: 400
  FAWX_AOSP_DEVICE_CODENAME    default: blazer
  FAWX_AOSP_REQUIRE_UNLOCKED   default: 1
USAGE
}

serial_arg=""
while (($#)); do
  case "$1" in
    --workspace)
      workspace="${2:?missing --workspace value}"
      shift 2
      ;;
    --min-free-gib)
      min_free_gib="${2:?missing --min-free-gib value}"
      shift 2
      ;;
    --device-codename)
      expected_device="${2:?missing --device-codename value}"
      shift 2
      ;;
    --skip-device-check)
      check_device=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    -*)
      echo "unknown option: $1" >&2
      usage
      exit 2
      ;;
    *)
      if [[ -n "$serial_arg" ]]; then
        echo "unexpected extra argument: $1" >&2
        usage
        exit 2
      fi
      serial_arg="$1"
      shift
      ;;
  esac
done

failures=0
warnings=0

pass() {
  printf 'PASS\t%s\n' "$1"
}

warn() {
  warnings=$((warnings + 1))
  printf 'WARN\t%s\n' "$1"
}

fail() {
  failures=$((failures + 1))
  printf 'FAIL\t%s\n' "$1"
}

require_command() {
  local name="$1"
  if command -v "$name" >/dev/null 2>&1; then
    pass "$name found: $(command -v "$name")"
  else
    fail "$name not found on PATH"
  fi
}

free_gib_for_path() {
  local path="$1"
  local probe="$path"
  while [[ ! -e "$probe" && "$probe" != "/" ]]; do
    probe="$(dirname "$probe")"
  done
  df -Pk "$probe" | awk 'NR == 2 { printf "%.0f", $4 / 1024 / 1024 }'
}

canonical_path() {
  local path="$1"
  if [[ -e "$path" ]]; then
    cd -- "$path" && pwd -P
    return
  fi
  local parent
  parent="$(dirname "$path")"
  while [[ ! -e "$parent" && "$parent" != "/" ]]; do
    parent="$(dirname "$parent")"
  done
  local base
  base="$(basename "$path")"
  printf '%s/%s\n' "$(cd -- "$parent" && pwd -P)" "$base"
}

workspace_is_inside_repo() {
  local workspace_path="$1"
  local repo_path="$2"
  [[ "$workspace_path" == "$repo_path" || "$workspace_path" == "$repo_path/"* ]]
}

echo "== AOSP workspace preflight =="
echo "workspace: $workspace"
echo "minimum free space: ${min_free_gib}GiB"

canonical_workspace="$(canonical_path "$workspace")"
if workspace_is_inside_repo "$canonical_workspace" "$repo_root"; then
  fail "AOSP workspace must be outside this repository: $canonical_workspace"
else
  pass "AOSP workspace is outside this repository: $canonical_workspace"
fi

require_command repo
require_command adb
require_command fastboot
require_command python3
require_command javac

available_gib="$(free_gib_for_path "$workspace")"
if ((available_gib >= min_free_gib)); then
  pass "free space ${available_gib}GiB >= ${min_free_gib}GiB"
else
  fail "free space ${available_gib}GiB < ${min_free_gib}GiB for AOSP checkout/build"
fi

if ((check_device)); then
  if select_adb_device ${serial_arg:+"$serial_arg"} >/tmp/fawx-aosp-preflight-adb.txt; then
    cat /tmp/fawx-aosp-preflight-adb.txt
    device_codename="$("${ADB[@]}" shell getprop ro.product.device | tr -d '\r')"
    android_release="$("${ADB[@]}" shell getprop ro.build.version.release | tr -d '\r')"
    fingerprint="$("${ADB[@]}" shell getprop ro.build.fingerprint | tr -d '\r')"
    flash_locked="$("${ADB[@]}" shell getprop ro.boot.flash.locked | tr -d '\r')"
    verified_boot="$("${ADB[@]}" shell getprop ro.boot.verifiedbootstate | tr -d '\r')"

    if [[ "$device_codename" == "$expected_device" ]]; then
      pass "device codename is $device_codename"
    else
      fail "device codename is $device_codename, expected $expected_device"
    fi

    pass "Android release: $android_release"
    pass "build fingerprint: $fingerprint"
    pass "verified boot state: ${verified_boot:-unknown}"

    if [[ "$require_unlocked" == "1" && "$flash_locked" == "1" ]]; then
      fail "bootloader is locked; AOSP flashing requires unlock and will wipe the device"
    elif [[ "$flash_locked" == "0" ]]; then
      pass "bootloader is unlocked"
    else
      warn "bootloader lock state is '${flash_locked:-unknown}'"
    fi
  else
    cat /tmp/fawx-aosp-preflight-adb.txt >&2 || true
    fail "no usable adb device"
  fi
else
  warn "device checks skipped"
fi

echo "== result =="
echo "failures: $failures"
echo "warnings: $warnings"

if ((failures > 0)); then
  exit 1
fi
