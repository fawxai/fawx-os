# AOSP Escape Analysis

## Purpose

This document defines how Fawx OS decides whether AOSP is a sufficient
foundation for an agent-native phone OS, or whether AOSP is another restrictive
container that we eventually need to leave.

The question is not "can we customize Android?" The question is:

> Does AOSP give Fawx OS enough fine-grained, durable control to build an
> agentic phone without fighting the substrate at every meaningful boundary?

If the data says no, we should seriously consider a different Linux-based
mobile framework instead of slowly recreating Android inside Android.

## Core Hypothesis

AOSP is the current preferred bootstrap substrate because it gives us:

- Pixel hardware support
- vendor driver compatibility
- telephony and radio integration
- camera, sensor, and power-management maturity
- app compatibility while Fawx OS is still young

But AOSP remains acceptable only if the important control surfaces can be moved
behind typed Fawx OS contracts without depending on fragile framework hacks,
Java/Kotlin app lifecycles, hidden APIs, or shell-style workarounds.

## Prison Test

For every important agentic capability, ask:

1. Can Fawx observe the relevant state as a typed event?
2. Can Fawx execute the relevant action through a typed command?
3. Can Fawx verify the result through independent evidence?
4. Can this happen while the user is doing something else?
5. Can the permission boundary be expressed as a Fawx policy grant?
6. Can the implementation be maintained without chasing private framework
   behavior?
7. Can the primitive survive OS updates, app switches, device lock, and process
   death?

If too many answers are "no," AOSP is not a liberating substrate. It is a
compatibility layer with a ceiling.

## Decision Scores

Each capability should be scored after a probe or prototype. Missing
implementation is not the same as substrate failure, so unprobed surfaces start
as `U`, not `0`.

| Score | Meaning | Decision Pressure |
| --- | --- | --- |
| U | Untested or no adapter yet | Requires evidence before platform conclusion |
| 0 | Not accessible at all | Strong pressure away from AOSP unless capability is non-core |
| 1 | Shell/recon only | Useful for learning, not production |
| 2 | Accessible through app/framework hooks but fragile | Warning sign; needs explicit risk tracking |
| 3 | Privileged AOSP integration works but requires framework ownership | Acceptable near-term if contract stays typed |
| 4 | Stable system-service or native bridge with typed events/actions | Good AOSP fit |
| 5 | Linux/native primitive independent of Android framework | Strong long-term fit, portable beyond AOSP |

The target for core agent-phone primitives is 4 or 5. A score of 3 can be
acceptable for hardware-heavy surfaces like telephony or camera, but only if the
Fawx contract remains stable above it.

## Capability Matrix

This matrix should be updated with evidence from
`pixel-substrate-compare-smoke.sh`, AOSP system-service prototypes, and live
rooted/AOSP probes.

| Capability | Minimum acceptable control | Current evidence | Current score | Exit pressure |
| --- | --- | --- | --- | --- |
| Foreground observation | Typed app/window focus events without shell parsing | Rooted recon uses `dumpsys`; AOSP returns `AdapterUnavailable` until real adapter exists | 1 | Medium |
| App launch/resume | Typed platform command with result evidence | Rooted recon can launch with `monkey`; AOSP contract says available but not implemented | 1 | Medium |
| Background execution | Supervised long-running service below UI lifecycle | Rust process can run through adb/recon; AOSP service not implemented | 1 | High |
| Notification read | Typed notification events with source/app metadata | Capability map says AOSP should own this; no adapter yet | U | High |
| Notification post | Typed user-visible notification action | Requires AOSP privilege on rooted stock; no adapter yet | U | Medium |
| Phone call | Typed call action with explicit user/policy grant | Requires AOSP privilege; no telephony adapter yet | U | High |
| Messaging | Typed message action with contact/policy grant | Requires AOSP privilege; no messaging adapter yet | U | High |
| Shared storage read/write | Scoped file grants and evidence | Rooted recon is path-limited; AOSP mediation not implemented | 1 | Medium |
| Local model access | Device-local model provider emits `IntentCandidate` | AICore/Gemini packages can be detected; no public inference adapter | 1 | Medium |
| UI automation/computer use | Background-capable action surface with verifiable observations | Not implemented; foreground app control only | U | High |
| Ephemeral verification UX | Disposable approval/edit/reject surfaces below task state | Terminal owner approval exists; OS-native surface not implemented | 1 | Medium |

## AOSP Is Good Enough If

AOSP remains the preferred foundation if the next prototypes prove:

- Foreground/window state can be emitted as a stable platform event.
- App launch/resume can be commanded without shell tricks.
- A Fawx system service can supervise background tasks independent of the
  current UI.
- Notifications, calls, messaging, storage, and local model access can be
  represented as typed adapters with explicit grants.
- The Rust runtime remains the owner of task state, policy, evidence, and
  completion.
- Android framework code is an adapter layer, not the center of the agent loop.

## AOSP Is Probably Another Prison If

We should consider a different Linux-based mobile foundation if evidence shows:

- Critical events are available only through private APIs, scraping, or
  unreliable shell output.
- Background execution remains subordinate to app lifecycle constraints.
- Agent actions require routing through app UI rather than system-owned
  commands.
- Permission grants cannot be expressed below Android's app permission model.
- Local model access is app-private or vendor-locked in ways we cannot bridge.
- Maintaining the platform adapter means chasing Android framework internals
  more than building Fawx OS primitives.
- We repeatedly need root-shell behavior in places where the AOSP platform model
  says root shell should be unavailable.

## Alternative Foundation Candidates

If AOSP fails the prison test, alternatives should be evaluated against the same
matrix, not by taste.

Candidates include:

- postmarketOS or another mainline-Linux phone stack
- Ubuntu Touch or Lomiri-derived userspace
- OpenEmbedded/Yocto-based custom image
- a minimal Linux system with Android compatibility isolated as a sidecar
- hybrid AOSP vendor stack plus non-Android userspace, if hardware allows

The likely tradeoff is brutal:

- AOSP buys hardware support and app compatibility but may impose framework
  constraints.
- mainline Linux buys control but may lose camera/radio/power reliability.

The data should tell us which pain is more acceptable.

## Evidence Rules

Evidence must be typed and reproducible.

Acceptable evidence:

- a smoke script result
- a typed adapter event
- a capability score update with a linked test/prototype
- a failing probe showing `AdapterUnavailable`, `RequiresAospPrivilege`, or
  another typed blocker

Unacceptable evidence:

- "it should be possible"
- a one-off shell command with no typed adapter
- a UI demo that cannot produce independent observation evidence
- a privileged hack that bypasses the Fawx policy model

## Next Experiments

The next AOSP-directed experiments should be:

1. Foreground observation system event: replace AOSP `AdapterUnavailable` with a
   real privileged event source that emits `AospForegroundEvent`.
2. App launch/resume platform command: replace shell-style launch with a system
   adapter command and typed execution result.
3. Background supervisor service: prove a Rust-owned task can continue while the
   user changes apps.
4. Notification read bridge: prove typed notification events can enter the
   runtime without app-level scraping.

After those four, update the matrix. Do not use a plain average to decide the
platform. AOSP should remain favored only if every must-have, high-exit-pressure
primitive reaches at least score 3, with a credible path to score 4.

Must-have gates:

- background execution
- foreground/window observation
- app launch/resume
- notification read
- messaging or calling, at least one communication surface first
- local model access or a credible local-provider bridge

If any must-have remains `U`, the answer is "insufficient evidence." If any
must-have remains 0-2 after a serious prototype, pause AOSP investment and
evaluate Linux alternatives seriously.
