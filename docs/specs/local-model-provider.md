# Local Model Provider Contract

## Purpose

Fawx OS should be able to use a device-local model when the phone exposes one,
but the model must live below the control plane. Local inference may propose an
intent candidate. It may not grant permissions, execute actions, or mark work
complete.

The current prototype keeps deterministic terminal parsing as the first provider
behind this boundary and adds a read-only Pixel probe for possible local model
surfaces.

## Contract

The durable flow is:

```text
prompt -> IntentCandidate -> policy acceptance -> runtime action -> observation -> verification
```

An `IntentCandidate` may contain:

- provider identity and locality,
- the original prompt,
- optional typed activity,
- optional typed action proposal.

An `IntentCandidate` must not contain:

- safety grants,
- runtime execution commands,
- observation evidence,
- completion state.

The harness accepts or rejects the candidate using the same policy path as every
other model action. A candidate that proposes `OpenApp(com.android.settings)`
still fails unless the task already has an `AppControl` grant scoped to
`android-package:com.android.settings`, or the owner command path explicitly
adds that grant.

When a candidate action is accepted, its action boundary preserves candidate
provenance with an `intent-candidate:<provider>:<candidate>` id. That gives the
kernel an audit handle without making provider identity a source of authority.

## Deterministic Provider

`fawx-terminal-runner session` currently uses a deterministic provider:

```text
provider_id: deterministic-session-parser
locality: DeterministicFallback
```

This is intentionally boring. It proves the candidate contract and keeps phone
control testable before local inference is connected. A future AICore/Gemini
adapter should produce the same `IntentCandidate` shape rather than bypassing
the terminal/session action path.

`fawx-terminal-runner candidate-dry-run <prompt>` exercises the non-owner
`ModelCandidate` source without executing the candidate. It is a contract probe
for future provider adapters, not a phone-control command.

## Pixel Gemini / AICore Probe

`fawx-terminal-runner local-model-probe` runs on the phone and reports known
local model surfaces as typed provider probes.

The probe is intentionally conservative:

- It inspects installed packages with `pm list packages`.
- It records evidence such as `com.google.android.aicore` being present.
- It reports `PresentButNoPublicTerminalApi` rather than claiming inference is
  usable.
- It reports `Indeterminate` if package-manager inspection fails, rather than
  flattening probe failure into provider unavailability.
- It does not scrape app-private storage, tokens, or hidden Gemini app state.

This gives us a safe answer to "is there a likely local model surface on this
device?" without pretending that package presence is an inference API.

## Adapter Principle

AICore/Gemini Nano should be an adapter, not the foundation. If Android later
requires a framework SDK or privileged AOSP service to call the local model,
that adapter can be added without changing the task/action/observation contract.

The fallback remains deterministic terminal parsing so phone-control tests stay
stable when local inference is unavailable.

The terminal session has two explicit sources:

- `OwnerCommand` for direct user commands. This path may mint the exact scoped
  grant needed by the command.
- `ModelCandidate` for provider-generated candidates. This path must never mint
  grants; it depends on existing policy or a separate owner confirmation.
