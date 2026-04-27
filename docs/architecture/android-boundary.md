# Android Boundary

## Purpose

This document defines the boundary between Android as substrate, the Android adapter layer, and the Fawx OS core runtime.

Its job is to prevent two failure modes:

1. Fawx OS becomes an Android app with agent features.
2. Fawx OS fights the substrate in places where Android should remain the owner.

The solution is an explicit contract.

This boundary should be interpreted alongside
`docs/architecture/aosp-escape-analysis.md`. The Android adapter is not only a
build plan; it is also an evidence-gathering surface for deciding whether AOSP
is sufficient or whether Fawx OS needs a more controllable Linux-based
foundation.

## Substrate Strategy

Fawx OS has two Android-facing tracks.

### Recon substrate

Stock rooted Android may be used for probes.

Recon probes are allowed to answer concrete questions:

- can we deploy and supervise a Rust process on the device?
- what system state can root observe directly?
- which actions require Android framework integration?
- which interactions become unreliable when the user switches apps?

Recon probes are not allowed to define core runtime semantics.

Any code written for stock rooted Android must be one of two things:

- reusable in the AOSP adapter
- explicitly isolated as recon-only and safe to delete

### Prototype substrate

The real prototype target is AOSP-level control.

That means:

- privileged services
- system permissions
- custom framework or service hooks where needed
- background execution as a platform capability
- eventual shell or launcher integration

The architecture must not absorb stock-app lifecycle constraints as product truths. If stock Android limits a probe, that is evidence to classify the boundary. It is not automatically a Fawx OS constraint.

## The Three Layers

### 1. Substrate

Android and the inherited device stack own:

- boot and process model
- Linux kernel and vendor drivers
- radio and telephony primitives
- package and activity system
- notification delivery primitives
- system permission primitives
- power and foreground-service primitives
- windowing and focus state

Fawx OS does not replace these first. It builds on top of them through an AOSP-oriented adapter boundary.

### 2. Adapter

The Android adapter layer translates substrate realities into Fawx OS runtime contracts.

It should own:

- observing Android lifecycle signals
- observing app, focus, and foreground state
- bridging notifications and system events
- reacquiring foreground when required
- exposing rooted capabilities behind typed commands
- translating Android failures into typed runtime blockers

It should not own:

- task lifecycle truth
- policy decisions
- completion semantics
- cloud escalation decisions
- user-intent interpretation

### 3. Core Runtime

The Fawx OS core owns:

- task lifecycle
- checkpoints and resumability
- attention requirement semantics
- completion semantics
- policy and authority
- audit and side-effect boundaries
- task cancellation and retry rules

Android events may influence these decisions. They do not define them.

## Upward Flow: Android Events

The adapter sends typed events up to the runtime.

Examples:

- foreground app changed
- screen became unavailable
- notification arrived
- call state changed
- network became unavailable
- wake lock was lost
- rooted action failed with policy or platform reason

These are observations, not conclusions.

Bad:

- "task canceled because app paused"

Good:

- "foreground surface changed"
- "active target app no longer visible"

The core runtime decides what those facts mean for the task.

## Downward Flow: Runtime Commands

The core runtime sends typed commands down to the adapter.

Examples:

- acquire foreground for target app
- release foreground ownership
- observe notifications for a source
- perform rooted device action
- query current focus state
- open or resume an app surface

The runtime should not send Android-specific prose or shellish instructions. It should send typed intent to the adapter.

## Ownership Rules

### Android owns platform truth

Examples:

- whether an activity is visible
- whether a notification fired
- whether a wake lock exists
- whether the device is locked

### Fawx OS owns task truth

Examples:

- whether the task is still alive
- whether the task may continue in background
- whether the task needs user approval
- whether a foreground reacquire is required
- whether the task is completed

This is the most important rule in the document.

## Foreground and Background

Android can tell us:

- which app is foreground
- whether Fawx currently owns visible focus
- whether the target surface is still interactable

Only Fawx OS can decide:

- whether the task may continue without focus
- whether the task should checkpoint and yield
- whether the task must reacquire attention

Therefore:

- app switches are events
- task continuation is a runtime decision

## Rooted Privileges

The adapter may expose rooted capabilities, but only through typed commands and explicit policy gates.

The runtime must never assume that "root exists" means "all actions are acceptable."

Root broadens the action surface. It does not weaken the kernel.

## Android Capability Map

The adapter owns substrate facts. The kernel owns permission decisions.

This table is mirrored as typed data in `fawx-android-adapter` so tests and runtime code can depend on the same boundary the docs describe.

| Capability | Rooted stock Android | AOSP/system privileges | Contract note |
| --- | --- | --- | --- |
| Observe foreground app | Available | Available | Recon can use `dumpsys window`; AOSP should expose stable platform events. |
| Launch app | Limited | Available | Recon can probe activity-manager commands; reliable launch/resume belongs in a privileged adapter. |
| Control foreground app | Limited | Available | Recon UI control is fragile; durable control needs accessibility, shell, or framework integration. |
| Read notifications | Limited | Available | Production needs notification listener or system hook semantics. |
| Post notifications | Requires AOSP privilege | Available | User-visible OS notifications are platform actions, not shell strings. |
| Place call | Requires AOSP privilege | Available | Telephony side effects require explicit kernel/user authority. |
| Send message | Requires AOSP privilege | Available | Messaging side effects require explicit kernel/user authority. |
| Read shared storage | Limited | Available | Recon can read shell-accessible paths; production needs scoped storage policy. |
| Write shared storage | Limited | Available | Recon writes are path-limited and risky; AOSP should mediate writes through grants. |
| Network access | Available | Available | Available does not mean ungated; task policy still grants or denies. |
| Background execution | Limited | Available | Recon detached shell processes are useful evidence, not the final supervisor model. |
| Install packages | Limited | Available | Package install depends on device policy until we own the package-manager boundary. |
| System settings | Limited | Available | Recon can inspect or poke some settings; production needs typed framework APIs. |
| Root shell | Limited | Unavailable | Root shell is a recon escape hatch, not a production OS primitive. |

## AOSP Comparison Probe

The Android probe is substrate-selectable:

```sh
fawx-android-probe --substrate recon-rooted-stock
fawx-android-probe --substrate aosp-platform
```

`ReconRootedStock` is allowed to run shell-backed commands such as `dumpsys`
because its purpose is evidence gathering on the current rooted Pixel.

`AospPlatform` is different. Until a privileged platform adapter exists, the
probe must not relabel shell evidence as AOSP/system evidence. It may emit the
typed AOSP capability projection, but platform observations should report an
explicit `AdapterUnavailable` result. This keeps the comparison honest:

- rooted stock tells us what can be proven through recon today
- AOSP platform tells us what the eventual system adapter must own
- the delta between them tells us whether moving into AOSP is justified

When a probe changes one of these facts, update the escape-analysis matrix so
the project can see whether AOSP is opening doors or merely moving the walls.

The first AOSP foreground success path is deliberately narrow:

- `foreground_observation(AospPlatform)` returns `AdapterUnavailable`
- shell/dumpsys parser helpers return `AdapterUnavailable` when called with
  `AospPlatform`
- only an auditable `AospForegroundEvent` from a platform service may create an
  AOSP `ForegroundAppChanged`

That means the future system service has one clean integration point, and recon
code cannot accidentally masquerade as platform evidence.

The terminal probe can now ingest that platform event through an explicit
`--aosp-foreground-event-file` path. The file is not a shell observation and it
must not be populated by `dumpsys`; it is the typed handoff point for a future
privileged AOSP service such as `fawx-system-foreground-observer`. The default
AOSP probe must continue to emit `AdapterUnavailable` until that service supplies
an event.

Example event:

```json
{
  "package_name": "com.android.settings",
  "activity_name": "com.android.settings.Settings",
  "source": {
    "service_name": "fawx-system-foreground-observer",
    "event_id": "event-123"
  }
}
```

The source fields are required so the runtime can audit where platform evidence
came from. A missing source is invalid evidence, not "unknown" evidence. For
the foreground primitive, `service_name` must be
`fawx-system-foreground-observer`; shell-like producers such as `dumpsys` are
rejected by the adapter.

## Minimum First-Implementation Contract

For the first Android boundary implementation, we need:

1. typed Android events
2. typed runtime-to-Android commands
3. explicit foreground reacquire requests
4. explicit rooted action requests
5. typed substrate classification
6. no stringly-typed lifecycle inference in the core runtime

That is enough to keep the architecture honest while we build the first real adapter.
