# Fawx OS

Fawx OS is a Rust-first agent runtime for Android-first personal computing.

The goal is not to build a generic agent framework or another mobile app. The goal is to build a personal operating environment where:

- the phone is the primary computer
- a local agent handles the majority of day-to-day work
- hard tasks can be escalated to a cloud agent
- browser, shell, and device actions are first-class capabilities
- security is enforced by a kernel, not by prompt text

Android is the bootstrap substrate, not the product center. We use it to inherit hardware compatibility, then push as much of the real system as possible into a portable Rust runtime.

## What This Repo Is

This repo is the founding codebase for the next-generation Fawx runtime:

- a thin harness, not a thick framework
- a broad native action surface, not a narrow curated toy API
- a hard security kernel, not a trust-me orchestration layer
- a stock ADB-shell reconnaissance slice today, with rooted/AOSP adapters as
  explicit runtime targets
- a local-first architecture with explicit cloud escalation

OpenShell is a strong reference point for runtime shape, policy enforcement, and Rust-first systems design. Browser Use is a strong reference point for treating browser execution as a first-class capability rather than a sidecar. Fawx OS will learn from both, but it will not become a rebrand of either.

## Founding Principles

1. The harness must stay thin.
2. The security kernel must stay hard.
3. Device, browser, shell, and network actions must be first-class.
4. Completion must be explicit and typed.
5. Android-specific bindings must be isolated so they can be replaced over time.
6. The core runtime should be majority Rust.

## Initial Architecture

```text
fawx-os/
├── device/   # Android adapters, sensors, actions, app control
├── cloud/    # Remote escalation runtime and contracts
├── kernel/   # Policy, authority, audit, secrets, execution contracts
├── browser/  # Browser runtime and automation substrate
├── docs/     # Architecture, decisions, specs
└── scripts/  # Build and validation scripts
```

## Foundation Hypotheses

- Base mobile substrate hypothesis: AOSP-first, evidence-gated by rooted Pixel
  probes before any capital-intensive checkout/build work
- Core implementation language: Rust
- Android framework dependence: minimize and isolate
- Local/cloud split: local by default, cloud when necessary
- Browser automation: built in as a core capability

Start here:

- [/Users/joseph/fawx-os/docs/architecture/foundation.md](/Users/joseph/fawx-os/docs/architecture/foundation.md)
- [/Users/joseph/fawx-os/docs/architecture/rust-ownership.md](/Users/joseph/fawx-os/docs/architecture/rust-ownership.md)
- [/Users/joseph/fawx-os/docs/architecture/reference-systems.md](/Users/joseph/fawx-os/docs/architecture/reference-systems.md)
- [/Users/joseph/fawx-os/docs/architecture/agent-loop-principles.md](/Users/joseph/fawx-os/docs/architecture/agent-loop-principles.md)
- [/Users/joseph/fawx-os/docs/architecture/android-boundary.md](/Users/joseph/fawx-os/docs/architecture/android-boundary.md)
- [/Users/joseph/fawx-os/docs/specs/background-task-lifecycle.md](/Users/joseph/fawx-os/docs/specs/background-task-lifecycle.md)
- [/Users/joseph/fawx-os/docs/specs/pixel-terminal-recon.md](/Users/joseph/fawx-os/docs/specs/pixel-terminal-recon.md)
- [/Users/joseph/fawx-os/docs/specs/terminal-agent-session.md](/Users/joseph/fawx-os/docs/specs/terminal-agent-session.md)
- [/Users/joseph/fawx-os/docs/specs/local-model-provider.md](/Users/joseph/fawx-os/docs/specs/local-model-provider.md)

## Current Status

This repo is in the founding architecture phase. The current proven vertical
slice is rooted-stock Android reconnaissance: Rust binaries can be built for
Android, pushed with ADB, observe foreground state, persist typed task
checkpoints, execute app-control through the runtime adapter, and close actions
from typed foreground evidence.

The interactive terminal session is deterministic for now. It does not call a
local model yet; it exercises the same typed contracts that a local model will
eventually feed.

The local model provider contract now exists as a safe boundary: provider probes
can discover likely on-device model surfaces, but model output is only an
intent candidate below policy, execution, and observation.

The next step is to grow that slice into:

1. Android runtime adapters with explicit stock/rooted/AOSP boundaries
2. Rust security kernel
3. local model runtime contract
4. browser capability
5. explicit cloud escalation boundary

The first real AOSP gate is foreground observation from a privileged platform
producer. Before attempting any AOSP checkout or flash, run:

```sh
./scripts/aosp-workspace-preflight.sh
```

AOSP source and build artifacts intentionally live outside this repository.

Before spending money on storage or cloud build capacity, run the capital-free
substrate decision sprint:

```sh
./scripts/pixel-substrate-decision-sprint.sh
```

That report keeps the AOSP decision tied to typed evidence from the rooted Pixel
instead of optimism about what a full platform build might unlock.

Before review, run:

```sh
./scripts/check.sh
```
