# Rust Ownership Map

This document defines what should belong to Rust from day one, what can temporarily remain Android-native, and what must be architected for eventual removal.

## Rust From Day One

These are the core of the system. They should be implemented in Rust immediately.

### Kernel

- authority model
- permission and policy evaluation
- external action contracts
- audit logging
- secrets and credential mediation
- task and execution state machines

### Harness Runtime

- model request/response loop
- explicit completion contracts
- tool and capability dispatch
- streaming event model
- compaction and context-budget mechanics
- background task lifecycle and resumability

### Device Runtime Core

- device action contracts
- high-level action planner/executor bridge
- notification ingestion pipeline
- structured sensor/event ingestion
- local persistence for state and memory
- foreground/background execution coordination

### Browser Runtime

- browser session coordination
- action and observation contracts
- DOM/interaction execution interfaces
- artifact capture and replay metadata

### Cloud Runtime

- escalation policy
- remote task envelope
- cloud/local handoff contracts
- result reconciliation

## Android-Native Temporarily

These can start as Android-specific adapters, but they should be kept thin and replaceable.

- Binder-facing service shims
- accessibility and app-control adapters
- notification bridge glue
- telephony integration glue
- foreground service / lifecycle integration
- package and activity inspection adapters

The rule is simple: if a component exists only to talk to Android, keep it at the edge.

## Not a Goal for Early Rust Ownership

These are not the right targets for early ambition:

- majority Linux kernel rewrite
- vendor driver rewrite
- graphics stack rewrite
- complete Android framework replacement

Those are possible long-term explorations, but they are not the bootstrap path.

## Interface Rule

All Android-specific code should depend on Rust contracts, not the other way around.

Good:

- Android adapter calls into Rust kernel APIs
- Android event sources feed typed Rust event envelopes

Bad:

- Rust core imports Android assumptions directly
- kernel semantics are shaped by Android framework convenience

## Migration Goal

If we do this correctly, the future port away from Android looks like replacing adapters, not rewriting the system.

That means:

- the kernel survives
- the harness survives
- the browser runtime survives
- the cloud runtime survives
- only the substrate-specific edge adapters change
