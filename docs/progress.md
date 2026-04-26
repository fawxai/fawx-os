# Fawx OS Progress Gates

This list tracks the remaining gates before we can call the rooted Android prototype ready for repeatable live testing.

## Completed

- Rust terminal binaries build for Android and run on a Pixel.
- Typed task persistence works on-device.
- Foreground observations flow through typed runtime observations.
- Action execution can close through typed observation evidence.
- Background runner ticks persisted tasks without broadcasting foreground evidence.
- Android capability map exists as typed adapter data and docs.
- Permission and safety boundary gates sensitive actions through typed grants.

## Remaining

1. Human handoff contract.
   - When foreground/user help is needed, the task pauses with typed handoff state.
   - The task must resume from explicit evidence, not from prose.

2. Real task harness.
   - Add a small repeatable rooted-phone task suite.
   - Score each task by typed outcomes and observation evidence.
