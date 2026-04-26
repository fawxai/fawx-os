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
- Human handoff requests persist typed resume conditions and clear only from matching evidence.
- Real task harness scores repeatable rooted-phone tasks by typed outcomes and observation evidence.

## Remaining

- Runtime-owned Android app-control executor.
  - Current harness proves action acceptance, executing state, and typed evidence closure.
  - A future slice should make the runtime launch the app itself rather than using ADB setup.
