# Agent Loop Principles

## Purpose

This document defines what Fawx OS should optimize for when we build the first real agent loop.

The loop should be simple enough to inspect, but strong enough to support long-lived background execution, typed policy, and real-world task completion.

## References

We will consider:

- pi-mono for loop simplicity
- Codex for durable tool/work ergonomics
- OpenAI Agents SDK for API and handoff concepts
- Browser Use for browser execution shape
- OpenShell for Rust-first policy/runtime posture

No single reference owns the architecture.

## Principles

### Keep The Loop Legible

The core loop should be small enough that an engineer can read it and understand:

- when the model is called
- when tools are executed
- when results are appended
- when the task continues
- when the task stops

If the loop needs a map to debug, it is already too clever.

### Keep Policy Outside The Model

The model may propose actions. The kernel authorizes them.

Tool callbacks may help prepare, validate, and summarize execution, but authority must live in typed runtime policy.

### Preserve Native Runtime Messages

Fawx OS should keep its own runtime message/event model and convert only at provider boundaries.

This keeps provider quirks from defining the core task model.

### Treat Steering As A First-Class Input

User steering during a task should enter the loop through a typed channel.

It should not be simulated as random appended text after the fact.

### Support Parallelism Without Hiding State

Some tools can run in parallel. Some must run sequentially.

The loop should support both while preserving:

- ordered results
- explicit tool lifecycle events
- checkpointable state

### Completion Is A Contract

The loop stops because a task is completed, blocked, failed, or waiting.

It must not stop only because there are no more tool calls.

### Background Tasks Are Normal

The loop must assume the user may leave the current UI.

That means task state, blockers, checkpoints, and attention requirements must live below the shell.

## First Implementation Target

The first loop should support:

1. create task
2. call model
3. execute typed tool commands
4. append tool results
5. checkpoint after side effects
6. handle steering
7. transition to completed, waiting, or failed

It should not start with:

- multi-agent orchestration
- elaborate planning graphs
- framework-level agent roles
- provider-specific assumptions in core state

