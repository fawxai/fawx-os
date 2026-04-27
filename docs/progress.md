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
- The terminal session now routes deterministic prompts through the `IntentCandidate` seam, preserving provider/candidate provenance before policy acceptance.
- Candidate acceptance policy exists: owner commands and model candidates are evaluated differently, and unauthorized model candidates pause with a typed owner-approval handoff instead of executing or failing opaquely.
- Interactive terminal sessions can now exercise a model-candidate approval path end-to-end: `suggest ...` pauses, `approve ...` consumes the stored candidate, execution runs, and foreground evidence closes the action.
- Android probes can project both `ReconRootedStock` and `AospPlatform` substrate contracts, with AOSP observations explicitly blocked as `AdapterUnavailable` until a real platform adapter exists.
- AOSP escape analysis exists as a decision rubric so platform probes can answer whether AOSP provides enough fine-grained control or whether Fawx OS should consider another Linux-based mobile foundation.
- AOSP foreground observation now has a typed platform-event success seam: shell/recon helpers still return `AdapterUnavailable`, and only an auditable `AospForegroundEvent` can produce `AospPlatform` foreground evidence.
- The Android probe can ingest a privileged AOSP foreground event file, proving the runtime-side event contract while preserving `AdapterUnavailable` as the default AOSP behavior.
- AOSP background execution now has a typed supervisor heartbeat ingest seam: adb/recon process survival still does not count as platform supervision, and only `fawx-system-background-supervisor` provenance can produce an AOSP supervisor heartbeat.
- AOSP app launch/resume now has a typed app-controller result ingest seam: `monkey` remains rooted-stock recon only, and only `fawx-system-app-controller` provenance can produce an AOSP app-launch success.
- AOSP notification read now has a typed notification-listener ingest seam: notification scraping remains non-platform evidence, and only `fawx-system-notification-listener` provenance can produce an AOSP notification event.

## Remaining

- Local model inference is not connected yet. The current terminal session uses deterministic intent parsing so the runtime contract can be tested before model quality is introduced.
- If AICore/Gemini Nano exposes a supported API surface, add it as a provider adapter that emits candidates into the existing contract.
- AOSP/system-image testing is not connected yet. The next AOSP slice should make real privileged services produce the foreground, app-controller, notification-listener, and background-supervisor events currently supplied by probe ingest files.
- The escape-analysis matrix is initially scored from rooted-stock evidence and contract assumptions. It needs real AOSP/system-service evidence before we make a durable platform commitment.
