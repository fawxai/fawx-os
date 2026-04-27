# AOSP Prototype Plan

## Status

Fawx OS has not imported or built AOSP yet.

The current repository contains Rust-side contracts, rooted-stock recon probes,
and AOSP fixture ingest seams. Those are intentionally useful, but they are not
live AOSP proof.

Current local preflight status:

- Pixel codename is `blazer`, matching the Pixel 10 Pro target.
- The attached device reports Android 16 and verified boot `green`.
- The bootloader is currently locked, so flashing AOSP would require an unlock
  flow and will wipe the device.
- This machine currently has far less than the recommended AOSP source/build
  space. Do not start a checkout until the preflight gate passes.

## First Real Primitive

The first real AOSP primitive should be foreground observation.

Reason:

- It is high value for agent execution.
- It is low side-effect risk.
- It is easy to verify independently.
- It proves the service bridge without touching calls, messages, or payments.

The target milestone is:

> `AospPlatform` emits a real `ForegroundAppChanged` from
> `fawx-system-foreground-observer` without shell, adb, or `dumpsys`, and the
> harness treats it exactly like rooted-stock foreground evidence.

## Repository Boundary

Do not vendor AOSP into this repository.

Use a separate checkout/workspace for AOSP build work. This repo should own:

- Rust runtime contracts
- adapter schemas
- smoke scripts
- fixture ingest tests
- documentation and capability scoring

The AOSP checkout should own:

- system service implementation
- platform integration
- device image build artifacts
- privileged permission wiring

Use the repo-side preflight before creating that external checkout:

```sh
./scripts/aosp-workspace-preflight.sh
```

When the preflight passes, initialize the external workspace:

```sh
./scripts/aosp-workspace-init.sh --workspace "$HOME/aosp-fawx"
```

The init script uses the upstream AOSP manifest and `android-latest-release` by
default. It intentionally refuses to run if the preflight says the machine or
device is not ready.

## Prototype Shape

The first platform bridge can be deliberately narrow:

1. Add a minimal system/privileged foreground observer.
2. Emit the same JSON payload currently accepted by
   `--aosp-foreground-event-file`.
3. Bridge that payload into the Rust adapter through a transport that can later
   be replaced without changing runtime semantics.
4. Run `./scripts/pixel-substrate-compare-smoke.sh`.
5. Confirm AOSP mode moves from `AdapterUnavailable` to a real
   `ForegroundAppChanged` only when the platform service is present.

## Success Criteria

The first AOSP gate is passed only when all of these are true:

- The event source is `fawx-system-foreground-observer`.
- The event carries a stable `event_id`.
- No shell, adb, `dumpsys`, or rooted-stock recon parser produced the event.
- The Rust runtime closes an `OpenApp` or foreground-observation action from
  the platform event.
- The substrate comparison smoke remains honest: without the service, AOSP
  reports `AdapterUnavailable`; with the service, it reports typed platform
  evidence.

## Failure Criteria

Treat any of these as evidence against the AOSP path:

- Foreground observation requires shell or `dumpsys` in the production path.
- The service cannot run below normal app lifecycle constraints.
- Android permissions force the observer into an app-shaped UX boundary that
  prevents background operation.
- The bridge cannot preserve typed provenance.
- Event delivery cannot survive app switches or process restart in a
  maintainable way.

## After Foreground Observation

Only after foreground observation is real should we move to the next AOSP
primitive:

1. App launch/resume via `fawx-system-app-controller`.
2. Background supervision via `fawx-system-background-supervisor`.
3. Notification read via `fawx-system-notification-listener`.
4. One communication side-effect surface: messaging or telephony.

The escape matrix should be updated after every prototype. AOSP remains favored
only if the data shows it gives us durable typed control, not merely a different
set of walls.
