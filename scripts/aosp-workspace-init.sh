#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

export PATH="$HOME/bin:$PATH"

workspace="${FAWX_AOSP_WORKSPACE:-$HOME/aosp-fawx}"
branch="${FAWX_AOSP_BRANCH:-android-latest-release}"
manifest_url="${FAWX_AOSP_MANIFEST_URL:-https://android.googlesource.com/platform/manifest}"
dry_run=0
skip_device_check=0

usage() {
  cat >&2 <<'USAGE'
usage: aosp-workspace-init.sh [--workspace PATH] [--branch NAME] [--dry-run] [--skip-device-check] [android-serial]

Initializes an external AOSP checkout for Fawx OS prototype work.
This script intentionally refuses to run until aosp-workspace-preflight passes.

Defaults:
  workspace: ~/aosp-fawx
  branch:    android-latest-release

The checkout is outside this repository by design.
USAGE
}

serial_arg=""
while (($#)); do
  case "$1" in
    --workspace)
      workspace="${2:?missing --workspace value}"
      shift 2
      ;;
    --branch)
      branch="${2:?missing --branch value}"
      shift 2
      ;;
    --dry-run)
      dry_run=1
      shift
      ;;
    --skip-device-check)
      skip_device_check=1
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

repo_root="$(cd -- "$script_dir/.." && pwd -P)"
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

canonical_workspace="$(canonical_path "$workspace")"
if [[ "$canonical_workspace" == "$repo_root" || "$canonical_workspace" == "$repo_root/"* ]]; then
  echo "AOSP workspace must be outside this repository: $canonical_workspace" >&2
  exit 2
fi

preflight_args=(--workspace "$workspace")
if ((skip_device_check)); then
  preflight_args+=(--skip-device-check)
fi
"$script_dir/aosp-workspace-preflight.sh" "${preflight_args[@]}" ${serial_arg:+"$serial_arg"}

echo "== AOSP workspace init =="
echo "workspace: $workspace"
echo "manifest: $manifest_url"
echo "branch: $branch"

if ((dry_run)); then
  echo "dry run: would run repo init and repo sync in $workspace"
  exit 0
fi

mkdir -p "$workspace"
cd "$workspace"

if [[ -d .repo ]]; then
  echo "repo checkout already initialized: $workspace"
else
  repo init --partial-clone -b "$branch" -u "$manifest_url"
fi

repo sync -c -j"$(sysctl -n hw.logicalcpu 2>/dev/null || echo 8)"
