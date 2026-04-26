#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

for script in scripts/*.sh; do
  bash -n "$script"
done
