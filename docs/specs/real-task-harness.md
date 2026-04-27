# Real Task Harness

## Purpose

The rooted Android prototype needs a repeatable live harness that scores typed
task outcomes instead of relying on human inspection of smoke logs.

The harness is intentionally small. It should prove the core control-plane
spine on a real phone:

- observe the Android foreground as typed evidence
- checkpoint background work
- accept an app-control action, have the runtime execute it, and close it from evidence
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

The app-control case may use ADB only for neutral setup, such as returning to
home before the assertion begins. The launch under test must be runtime-owned:
`fawx-terminal-runner execute-action` marks the accepted action executing and
asks the Android adapter to resume the target package. The action is still not
considered complete from command success alone; it closes only after a later
typed foreground observation reports the expected package.
