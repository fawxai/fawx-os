# scripts

Build, validation, and repository automation scripts.

Scripts should stay boring, deterministic, and easy to replace.

Run the full local doctrine gate before review:

```sh
./scripts/check.sh
```

The gate runs `cargo fmt --all --check`, `cargo test --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings`, and `bash -n` for
every `scripts/*.sh` file.

Pixel smoke scripts create a fresh task directory for every run under
`/data/local/tmp/fawx-os/tasks/<run-id>`. This keeps duplicate task-id
protection active while allowing the scripts to be rerun without clearing
previous device artifacts.

`./scripts/pixel-model-approval-smoke.sh` verifies the interactive
model-candidate path on a connected Pixel by piping `suggest open settings`,
`approve last`, explicit `approve <task-id>`, and `quit` into
`fawx-terminal-runner session`.

`./scripts/pixel-substrate-compare-smoke.sh` runs the Android probe in both
`ReconRootedStock` and `AospPlatform` modes. The AOSP mode intentionally reports
platform observations as unavailable until a real privileged adapter exists; it
is a boundary test, not a fake AOSP implementation.
