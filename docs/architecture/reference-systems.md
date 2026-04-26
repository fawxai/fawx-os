# Reference Systems

Fawx OS is not starting from nothing. It is learning from several existing systems while keeping its own product thesis intact.

This document exists to prevent accidental cargo-culting. A reference is useful only if we are explicit about what we are taking and what we are rejecting.

## OpenShell

OpenShell is the closest current reference for runtime posture.

We should learn from OpenShell:

- Rust-first systems implementation
- sandboxed execution as a default, not an afterthought
- declarative policy enforcement
- agent runtime as infrastructure, not just a prompt wrapper

We should not inherit OpenShell's product boundary wholesale:

- OpenShell is a safe agent runtime
- Fawx OS is an agent-native personal operating environment for rooted phones

That means OpenShell is a structural reference, not a product template.

## Browser Use

Browser Use is the clearest reference for browser capability design.

We should learn from Browser Use:

- browser execution as a first-class capability
- giving the model a broad and useful action surface
- resisting over-designed "framework intelligence"

We should not inherit Browser Use's limits as our own:

- browser automation is necessary, but it is only one capability
- Fawx OS must also own shell, device, sensors, notifications, apps, and local policy

Browser Use is the browser reference, not the system architecture.

## pi-mono

pi-mono is a strong reference for agent-loop simplicity.

We should learn from pi-mono:

- keep the loop small enough to reason about
- preserve a native agent message format and convert only at the model boundary
- emit explicit lifecycle events for messages, tool execution, and turn completion
- support steering and follow-up messages as first-class loop inputs
- validate and prepare tool calls immediately before execution
- allow sequential and parallel tool execution without turning the loop into a planner

We should not inherit pi-mono's exact implementation shape:

- Fawx OS is Rust-first
- Fawx OS has long-lived background tasks, checkpointing, and Android substrate boundaries
- Fawx OS must enforce security and authority in the kernel, not only in callbacks around tool execution

pi-mono is a harness reference. It is valuable because it keeps the loop legible.

## Codex Agent Loop

Codex remains an important reference for durable coding-agent ergonomics.

We should learn from Codex:

- explicit tool/result transcript structure
- persistent work across long-running tasks
- useful activity narration without hiding execution details
- strong terminal/tool integration
- practical compaction and continuation behavior

We should not copy Codex's product assumptions wholesale:

- Fawx OS is not primarily a coding agent
- phone, browser, messaging, calls, and device control are first-class surfaces
- background phone execution is a core requirement

Codex is a reference for serious agent work, not for the whole OS product model.

## OpenAI Agents SDK

The OpenAI Agents SDK is a reference for mainstream agent orchestration APIs.

We should learn from it:

- clean handoff concepts
- tool registration ergonomics
- model/runtime separation
- tracing and observability patterns

We should not let SDK shape dominate the core runtime:

- Fawx OS must remain provider-portable
- the kernel must own policy and completion semantics
- cloud agents are escalation surfaces, not the center of the OS

The SDK is an API design reference, not the architecture authority.

## Fawx

Fawx remains the closest internal reference.

We should carry forward:

- the doctrine around root-cause fixes
- typed external action contracts
- strong control-plane thinking
- audit and authority boundaries

We should leave behind:

- desktop-app-centered assumptions
- thick orchestration logic that substitutes for model capability
- UX-driven constraints that belong to an app, not to an OS runtime

## Design Rule

When a reference and the Fawx OS thesis conflict, the thesis wins.

The thesis is:

- local-first
- Rust-first
- rooted Android target
- thin harness
- broad action surface
- hard security kernel
