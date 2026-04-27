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
- Runtime-owned Android app-control execution can launch an app surface on rooted-stock Android and close the action only after typed foreground evidence observes the target package.
- Interactive terminal sessions can accept deterministic typed prompts and drive the same app-control execution/observation contract.
- Local model provider contract exists: model output is an `IntentCandidate` below policy, execution, and observation.
- Pixel local-model reconnaissance can report AICore/Gemini package presence without claiming an inference API.

## Remaining

- Local model inference is not connected yet. The current terminal session uses deterministic intent parsing so the runtime contract can be tested before model quality is introduced.
- If AICore/Gemini Nano exposes a supported API surface, add it as a provider adapter that emits candidates into the existing contract.
