# Foundation Decision

## Decision

Fawx OS will use Android as the bootstrap hardware compatibility substrate and will design against AOSP assumptions, not against a desktop app architecture.

The system will be Rust-first in its core runtime. Android-specific services, framework calls, and vendor integrations will be treated as replaceable adapters.

## Why Not Start From Scratch

Building a phone OS from a blank slate would force us to solve the wrong problems first:

- radios
- telephony
- camera stacks
- sensors
- power management
- graphics bring-up
- vendor integration

Those are real problems, but they are not the product thesis. The product thesis is an agent-native phone runtime with a strong security kernel and a broad local action surface.

## Why Android

Android already gives us:

- real phone hardware support
- a workable boot/update story
- app compatibility
- proven device drivers and vendor layers
- practical daily-driver stability

Just as important, Android gives us a realistic path to background-capable execution on a real phone. That matters because Fawx OS is not aiming for demo-only foreground autonomy. It needs to keep doing work while the user continues using the device.

That makes Android the least-wrong starting substrate for a serious phone project.

## Why AOSP Over LineageOS

LineageOS is attractive for faster bring-up, but AOSP is the cleaner long-term foundation.

We care more about:

- minimizing inherited ROM-level opinions
- reducing long-term framework baggage
- keeping a clean path toward replacing Android-specific pieces over time

So the repo architecture should assume:

- AOSP-level interfaces
- minimal reliance on Lineage-specific behavior
- Android as temporary substrate, not permanent identity

This is a hypothesis, not a religion. `docs/architecture/aosp-escape-analysis.md`
defines the evidence test for whether AOSP is actually liberating enough for an
agent-native phone OS. If AOSP cannot provide fine-grained typed control over
foreground state, background execution, notifications, telephony, messaging,
storage, and local model access, we should treat that as data in favor of a
different Linux-based foundation.

## Why Rust

Rust is the right center of gravity for this system because it supports:

- low-level systems work
- strong ownership and boundary modeling
- safer long-running privileged services
- portable core runtime logic

OpenShell is an important signal here. Its value is not just that it is "mostly Rust." Its value is that it demonstrates a serious systems-oriented shape for safe agent execution: sandboxing, policy, and runtime boundaries in a language suited to infrastructure rather than glue code.

## What We Are Explicitly Not Doing

- We are not trying to rewrite the majority of the Linux kernel in Rust.
- We are not treating Java or Android framework code as the long-term center of the system.
- We are not building a generic agent SDK.
- We are not allowing prompt-layer conventions to stand in for policy or control-plane contracts.

## Architectural Consequence

The right model is:

1. Linux kernel and vendor stack provide the hardware floor.
2. Android framework provides transitional system services where needed.
3. Fawx OS core runtime lives in Rust.
4. Device, browser, cloud, and shell surfaces speak to that Rust core through explicit contracts.

At the product boundary, this should feel like three layers rather than a
traditional app launcher:

1. Ambient intent capture: typed, permissioned signals about what the user may
   want.
2. Agentic execution: the runtime performs work across device, browser, cloud,
   API, and human-handoff surfaces.
3. Ephemeral verification UX: disposable surfaces for approval, comparison,
   editing, rejection, or enjoyment.

The UI is therefore not the operating system. The durable OS primitive is typed
intent, execution state, evidence, and verification. UI is generated around
tasks when the human needs to participate.

One consequence of this model is that task execution state must live below the current shell surface. The active UI cannot be the source of truth for whether agent work is still running. Background execution, resumability, and user interruption tolerance must be designed into the runtime from the start.

That keeps the long-term migration path open:

- first, reduce dependence on Java/framework surfaces
- then, reduce dependence on Android-specific userland assumptions
- eventually, treat Android as an implementation detail rather than the product substrate
