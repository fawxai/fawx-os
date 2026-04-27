# AOSP Service Bridge Contract

## Purpose

The AOSP service bridge is the contract between privileged Android/system
services and the Rust Fawx OS runtime.

It exists so platform code can produce facts without owning task truth. AOSP
services may observe, execute, and report evidence. The Rust runtime remains
the owner of policy, task state, action closure, completion, and user-facing
decisions.

## Event Envelope

Every AOSP platform event must carry an auditable source:

```json
{
  "source": {
    "service_name": "fawx-system-foreground-observer",
    "event_id": "event-123"
  }
}
```

The minimum envelope fields are:

| Field | Meaning |
| --- | --- |
| `service_name` | Stable producer identity, such as `fawx-system-foreground-observer`. |
| `event_id` | Producer-scoped idempotency key for deduplication and audit. |

Future bridge transports may add `emitted_at_ms`, `capability`, or signature
metadata, but the Rust-facing typed event shape must stay stable.

## Current Services

| Service | Produces | Current status |
| --- | --- | --- |
| `fawx-system-foreground-observer` | `AospForegroundEvent` | Ingest seam exists; no real system service yet. |
| `fawx-system-app-controller` | `AospAppLaunchResult` | Ingest seam exists; no real system service yet. |
| `fawx-system-background-supervisor` | `AospBackgroundSupervisorEvent` | Ingest seam exists; no real system service yet. |
| `fawx-system-notification-listener` | `AospNotificationEvent` | Ingest seam exists; no real system service yet. |
| `fawx-system-notification-poster` | Notification post result | Typed unavailable seam only. |
| `fawx-system-messaging` | Message send result | Typed unavailable seam only. |
| `fawx-system-telephony` | Phone call result | Typed unavailable seam only. |

## Provenance Rules

Success observations require platform provenance from the expected service.

Examples:

- AOSP foreground success must come from
  `fawx-system-foreground-observer`.
- AOSP app launch success must come from `fawx-system-app-controller`.
- AOSP notification read success must come from
  `fawx-system-notification-listener`.

Unavailable observations must not carry success provenance. This matters
because `AdapterUnavailable` is not platform evidence; it is an honest absence
of platform evidence.

Recon evidence must not be promoted. Shell, adb, `dumpsys`, and rooted-stock
probes may inform the capability matrix, but they cannot become AOSP platform
success events.

## Transport Rule

The current file-ingest paths are fixtures for testing the bridge shape. They
are not live AOSP proof.

Allowed fixture:

```sh
fawx-android-probe --substrate aosp-platform \
  --aosp-foreground-event-file /path/to/event.json
```

The first real AOSP prototype may use Binder, a Unix socket, a native service,
or a minimal system service bridge. That transport choice is below the
contract. The Rust runtime should keep seeing the same typed events and source
envelope.

## Idempotency

`event_id` is producer-scoped. A future bridge receiver should treat duplicate
`(service_name, event_id)` pairs as the same platform fact, not two independent
facts.

The current terminal/probe fixtures do not persist a platform-event dedupe
store yet. That is acceptable only because they are contract fixtures, not the
production bridge.

## Sensitive Action Surfaces

Notification posting, messaging, and telephony are side-effecting user-visible
surfaces. Until their real AOSP adapters exist, they must be represented as
typed unavailable events:

- `NotificationPostUnavailable(AdapterUnavailable)`
- `MessageUnavailable(AdapterUnavailable)`
- `PhoneCallUnavailable(AdapterUnavailable)`

This keeps the control plane honest: the kernel can see the missing primitive
directly instead of inferring it from absent tools or failed prose.
