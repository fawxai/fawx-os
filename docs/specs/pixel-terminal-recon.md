# Pixel Terminal Recon

## Purpose

This is the first live-device phase for Fawx OS.

It proves that the Rust runtime artifacts can run on a Pixel from a terminal environment, persist task state, and emit substrate-tagged Android observations.

It does not make stock Android the product foundation.

## Substrate

This phase uses `ReconRootedStock`.

That means:

- stock/rooted Android is allowed as a probe environment
- all observations must carry recon provenance
- failures are evidence for the AOSP adapter boundary
- stock Android limitations must not become core runtime semantics

## Build

Requires an Android NDK. If one is not installed:

```sh
sdkmanager "ndk;28.0.13004108"
```

```sh
./scripts/android-build.sh
```

The script builds:

- `fawx-android-probe`
- `fawx-terminal-runner`

for `aarch64-linux-android` by default.

## Push

```sh
./scripts/pixel-smoke.sh
```

This script pushes both binaries and runs the probe plus task-store smoke test.
Each run writes task state under a unique `/data/local/tmp/fawx-os/tasks/<run-id>`
directory so duplicate-create protection remains enabled without making reruns
fail on previous smoke artifacts.

## Probe

```sh
adb shell /data/local/tmp/fawx-os/bin/fawx-android-probe
```

Expected output is JSON containing:

- `substrate = ReconRootedStock`
- command observations for `whoami`, `id`, and `uname`
- root availability summary
- parsed foreground package/activity observation or explicit foreground-unavailable observation

## Task Store Smoke Test

```sh
task_dir="/data/local/tmp/fawx-os/tasks/$(date +%Y%m%d%H%M%S)-$$"
adb shell "mkdir -p '$task_dir'"
adb shell "FAWX_OS_TASK_DIR='$task_dir' /data/local/tmp/fawx-os/bin/fawx-terminal-runner create task-demo 'cancel that subscription'"
adb shell "FAWX_OS_TASK_DIR='$task_dir' /data/local/tmp/fawx-os/bin/fawx-terminal-runner checkpoint task-demo 'created on device'"
adb shell "FAWX_OS_TASK_DIR='$task_dir' /data/local/tmp/fawx-os/bin/fawx-terminal-runner block-foreground task-demo 'target app needs foreground focus'"
adb shell "FAWX_OS_TASK_DIR='$task_dir' /data/local/tmp/fawx-os/bin/fawx-terminal-runner status task-demo"
```

This proves:

- task state survives process exit
- checkpoints serialize on device
- foreground blockers are explicit typed state

## Heartbeat Smoke Test

```sh
task_dir="/data/local/tmp/fawx-os/tasks/$(date +%Y%m%d%H%M%S)-$$"
adb shell "mkdir -p '$task_dir'"
adb shell "FAWX_OS_TASK_DIR='$task_dir' /data/local/tmp/fawx-os/bin/fawx-terminal-runner create task-heartbeat 'prove background checkpoint updates'"
adb shell "FAWX_OS_TASK_DIR='$task_dir' /data/local/tmp/fawx-os/bin/fawx-terminal-runner heartbeat task-heartbeat 3 250"
adb shell "FAWX_OS_TASK_DIR='$task_dir' /data/local/tmp/fawx-os/bin/fawx-terminal-runner status task-heartbeat"
```

This proves:

- a long-running terminal process can repeatedly update checkpoints
- heartbeat checkpoints survive after the process exits
- the task remains `BackgroundCapable` while updating

## Detached Heartbeat Smoke Test

```sh
./scripts/pixel-detached-smoke.sh
```

This proves:

- a heartbeat process can be launched from ADB and allowed to continue after the launch command returns
- checkpoint state can be inspected while the heartbeat process is still running
- final checkpoint state can be inspected after the detached process exits

## Interruption Smoke Test

```sh
./scripts/pixel-interruption-smoke.sh
```

This proves:

- detached heartbeat continues while foreground apps change
- each heartbeat can sample foreground package/activity
- final checkpoint state survives after app switches and process exit

## Foreground Policy Smoke Test

```sh
./scripts/pixel-foreground-policy-smoke.sh
```

This proves:

- Android observations can drive task-state policy decisions
- matching expected foreground clears `WaitingForForeground` and can resume that task to `Running`
- mismatched foreground produces a typed `WaitingForForeground` blocker

## First Pixel Run

Date: 2026-04-26

Device:

- Pixel 10 Pro
- ADB model: `Pixel_10_Pro`
- ADB device codename: `blazer`

Results:

- `fawx-android-probe` ran successfully from `/data/local/tmp/fawx-os/bin`
- probe user: `shell`
- kernel: `Linux localhost 6.6.98-android15-8-g4b48560cd07d-ab14239520-4k`
- `su` was unavailable or denied from the ADB shell
- foreground focus was observable through `dumpsys window`
- foreground focus parsed into package/activity:
  - package: `com.google.android.apps.nexuslauncher`
  - activity: `com.google.android.apps.nexuslauncher.NexusLauncherActivity`
- `fawx-terminal-runner` created, checkpointed, foreground-blocked, and reloaded a task from `/data/local/tmp/fawx-os/tasks/<run-id>`
- `heartbeat task-heartbeat 3 250` wrote three on-device checkpoint updates and reloaded with `action_boundary.description = heartbeat 3/3`
- detached `heartbeat task-detached-heartbeat 6 500` returned control after launch, persisted immediate state at `heartbeat 1/6`, continued through `heartbeat 6/6`, and reloaded final state from disk
- interruption `heartbeat task-interruption-heartbeat 8 500 --foreground` continued while foreground moved from launcher to Settings and back to launcher
- observed foreground sequence included:
  - `com.google.android.apps.nexuslauncher/com.google.android.apps.nexuslauncher.NexusLauncherActivity`
  - `com.android.settings/com.android.settings.SubSettings`
  - `com.google.android.apps.nexuslauncher/com.google.android.apps.nexuslauncher.NexusLauncherActivity`
- foreground policy `watch-foreground task-foreground-policy com.android.settings 8 500` continued while Settings matched and persisted `WaitingForForeground` when Launcher was foreground

Conclusion:

- Rust terminal binaries work on the Pixel
- task-state persistence works on-device
- foreground observation is available from ADB shell
- foreground package/activity parsing works on-device
- root access is not available in the current shell context
- background-capable heartbeat checkpointing works from a long-running terminal process
- detached heartbeat checkpointing works after the launching ADB shell returns
- detached heartbeat checkpointing continues across foreground app changes
- foreground observations now drive task policy transitions between foreground-required `Waiting` and resumable work; a task waiting only on foreground can return to `Running`, while an already `Checkpointed` task remains checkpointed until resumed by work

This remains recon evidence. It does not redefine the AOSP-oriented adapter boundary.
