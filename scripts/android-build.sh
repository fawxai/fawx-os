#!/usr/bin/env bash
set -euo pipefail

target="${FAWX_OS_ANDROID_TARGET:-aarch64-linux-android}"
api="${FAWX_OS_ANDROID_API:-35}"
packages=(
  "fawx-android-probe"
  "fawx-terminal-runner"
)

find_ndk() {
  if [[ -n "${ANDROID_NDK_HOME:-}" && -d "${ANDROID_NDK_HOME:-}" ]]; then
    printf '%s\n' "$ANDROID_NDK_HOME"
    return 0
  fi

  if [[ -n "${NDK_HOME:-}" && -d "${NDK_HOME:-}" ]]; then
    printf '%s\n' "$NDK_HOME"
    return 0
  fi

  local sdk_root="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
  if [[ -n "$sdk_root" && -d "$sdk_root/ndk" ]]; then
    find "$sdk_root/ndk" -mindepth 1 -maxdepth 1 -type d | sort | tail -1
    return 0
  fi

  local homebrew_sdk="/opt/homebrew/share/android-commandlinetools"
  if [[ -d "$homebrew_sdk/ndk" ]]; then
    find "$homebrew_sdk/ndk" -mindepth 1 -maxdepth 1 -type d | sort | tail -1
    return 0
  fi

  return 1
}

host_tag() {
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) printf 'darwin-x86_64\n' ;;
    Darwin-x86_64) printf 'darwin-x86_64\n' ;;
    Linux-x86_64) printf 'linux-x86_64\n' ;;
    *) return 1 ;;
  esac
}

ndk_root="$(find_ndk || true)"
if [[ -z "$ndk_root" ]]; then
  cat >&2 <<'EOF'
error: Android NDK not found.

Install it with:
  sdkmanager "ndk;28.0.13004108"

Or set ANDROID_NDK_HOME to an installed NDK directory.
EOF
  exit 1
fi

host="$(host_tag)"
linker="$ndk_root/toolchains/llvm/prebuilt/$host/bin/${target}${api}-clang"
if [[ ! -x "$linker" ]]; then
  echo "error: Android linker not found: $linker" >&2
  exit 1
fi

if ! rustup target list --installed | grep -qx "$target"; then
  rustup target add "$target"
fi

export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$linker"

for package in "${packages[@]}"; do
  cargo build --release --target "$target" -p "$package"
done

echo "built Android binaries in target/$target/release"
