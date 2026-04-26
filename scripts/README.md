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
