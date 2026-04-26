# Real Task Harness

## Purpose

The rooted Android prototype needs a repeatable live harness that scores typed
task outcomes instead of relying on human inspection of smoke logs.

The harness is intentionally small. It should prove the core control-plane
spine on a real phone:

- observe the Android foreground as typed evidence
- checkpoint background work
- accept an app-control action, mark it executing, and close it from evidence
- close that action from scoped foreground evidence
- pause on foreground handoff and resume from matching foreground evidence
- pause on manual handoff and resume from explicit human completion evidence

## Script

Run:

```sh
./scripts/pixel-real-task-harness.sh
```

The script pushes the current Android binaries to the attached Pixel, creates an
isolated task directory under `/data/local/tmp/fawx-os/tasks`, and prints a
JSON score:

```json
{
  "passed": 5,
  "failed": 0,
  "task_dir": "/data/local/tmp/fawx-os/tasks/real-task-..."
}
```

## Contract

Each case must assert typed JSON state with `jq`. String presence checks are not
enough for this harness.

A passing case must prove at least one durable control-plane fact, such as:

- `TaskState.phase`
- `TaskState.current_action.status`
- `TaskState.current_action.last_observation.evidence`
- `TaskState.blocker`
- `TaskState.current_handoff`
- `TaskState.completed_handoffs`

## Relationship To Smoke Tests

`pixel-smoke.sh` remains the broad compatibility smoke.

`pixel-real-task-harness.sh` is the scored readiness gate. It should stay small,
stable, and semantically strict so we can run it frequently while moving toward
an AOSP/system-image prototype.

This harness does not yet prove a full Android app-launch executor. The current
app-control case drives the foreground with ADB, then verifies that the Fawx
control plane closes the typed action from scoped foreground evidence. A future
executor slice should replace that setup step with a runtime-owned app-launch
command.
