# Human Handoff Contract

## Purpose

When an agent cannot continue without the human or foreground device state, it
must pause as typed state and resume only from typed evidence.

This avoids a common failure mode: the UI shows "waiting for you" while the
runtime has only a prose blocker and no durable contract for what would unblock
the task.

## Runtime Model

`TaskState.current_handoff` stores the active handoff request.

A handoff request includes:

- `id`: stable handoff identifier.
- `kind`: `Foreground`, `UserApproval`, or `UserInput`.
- `reason`: user-facing explanation.
- `target`: optional typed target, such as an Android package.
- `resume_condition`: the evidence required to resume.
- `requested_at_ms`: creation time.
- `last_evidence`: optional evidence captured before the request is cleared.

The blocker still exists, but it is not the whole contract. The blocker says why
the task is paused. The handoff says what evidence will let it resume.

## Resume Conditions

Current resume conditions:

- `ForegroundPackage`: resume when that package is observed in foreground.
- `ExplicitUserApproval`: resume from a matching handoff completion event.
- `ExplicitUserInput`: resume from a matching handoff completion event.
- `Manual`: reserved for development or shell-driven handoff completion.

## Evidence Rule

The runtime must not clear a handoff from prose or from a generic loop step.

It clears only when:

- foreground policy observes the expected package for a `ForegroundPackage`
  handoff
- a `HumanHandoffCompleted` runtime event names the current handoff id

Mismatched foreground observations preserve the handoff. Wrong handoff ids are
recorded as observations but do not resume the task.

## Terminal Runner

The terminal runner exposes:

```sh
fawx-terminal-runner complete-handoff <task-id> <handoff-id> <summary>
```

This is a prototype control surface for explicit user approval/input evidence.
Future shells should expose the same event as a UI action rather than inventing
a separate resume path.
