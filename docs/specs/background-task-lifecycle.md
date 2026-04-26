# Background Task Lifecycle

## Purpose

Fawx OS must be able to continue useful work while the user keeps using the phone.

This document defines the minimum runtime contract for background-capable agent work. It exists to prevent a common failure mode: a system that appears autonomous in demos but silently depends on owning the foreground UI the entire time.

## Product Requirement

The user must be able to:

1. ask for an outcome
2. leave the agent view
3. continue using the phone
4. return later to one of four states:
   - done
   - blocked on approval
   - blocked on missing information
   - waiting for an external condition

Anything less is not an agent operating layer. It is a foreground assistant session.

## Runtime Model

Every task has:

- an execution phase
- an attention requirement
- a resumability status
- a latest checkpoint

These are runtime truths. They must not be inferred from UI state.

## Task Phases

### 1. Queued

The task exists but has not started active work yet.

### 2. Running

The task is actively executing.

Running may happen in one of two modes:

- foreground-assisted
- background-capable

### 3. Waiting

The task is paused on something external:

- user approval
- user input
- network/service condition
- scheduled retry time
- remote/cloud completion

### 4. Checkpointed

The task has persisted enough state to survive interruption and resume later without losing its place.

Checkpointed is not terminal. It is a safety condition the runtime should prefer whenever work may span time or attention boundaries.

### 5. Completed

The requested outcome is finished and verified to the degree required by policy.

### 6. Failed

The task cannot continue without a new plan, new permissions, or user intervention.

## Attention Requirement

Every task step must declare one of these requirements.

### BackgroundAllowed

The step may continue without owning the visible UI.

Examples:

- cloud reasoning
- network polling
- local planning
- waiting for emails or messages
- background-safe browser or service work

### ForegroundPreferred

The step can continue without foreground ownership, but quality or reliability improves if the user lets the agent keep focus.

Examples:

- visually unstable web flows
- app interactions likely to be interrupted by user navigation

### ForegroundRequired

The step must own the foreground before continuing.

Examples:

- direct UI driving that depends on the current visible surface
- high-risk confirmation flows
- flows where the OS or target app will invalidate interaction if focus changes

If a task hits `ForegroundRequired`, the runtime must not fake background progress. It must raise an explicit reacquire-attention request.

## Reacquire Attention Contract

When a task needs foreground control again, the runtime must emit:

- why attention is required
- what capability requires it
- whether the task is safe to defer
- what checkpoint was saved before pausing

The shell may render this however it wants. The kernel and harness must expose it as typed state.

## Checkpoints

A checkpoint is the minimum persisted state needed to resume work safely.

At minimum it should include:

- task identifier
- phase
- timestamp
- last known objective
- action boundary id
- action boundary state
- action boundary description
- pending blocker, if any

Checkpoints should occur:

- before yielding the foreground
- before any long wait
- before escalation to cloud
- after any meaningful external side effect

## Side-Effect Rule

If the task has changed the outside world, the checkpoint must record that boundary.

This is required for:

- audit
- resume safety
- duplicate-action prevention

The runtime must be able to distinguish:

- "planning to send an email"
- "email composed but not sent"
- "email sent"

That distinction is persisted as a typed action boundary, not prose alone. The
boundary id gives resume logic a stable de-duplication key, and the boundary
state records whether the external side effect is still planned, prepared,
committed, verified, or aborted.

## Shell Rule

The shell is not the task owner.

That means:

- leaving chat cannot cancel the task by accident
- closing a panel cannot erase execution state
- switching apps cannot be interpreted as task cancellation

Shells may observe, control, approve, or cancel tasks, but task state lives below them.

## Cancellation Rule

Cancellation must be explicit.

The runtime may stop work automatically only when:

- policy forbids further progress
- the task becomes unsafe
- the task becomes impossible with current capabilities

User distraction is not cancellation.

## First Implementation Scope

The first implementation only needs these guarantees:

1. typed task lifecycle state
2. typed attention requirement
3. typed blocker/reacquire state
4. persisted checkpoints
5. separation between shell presence and task existence

That is enough to build a real background-capable agent runtime without prematurely over-designing scheduling.
