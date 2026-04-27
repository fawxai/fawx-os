# Terminal Agent Session

## Purpose

The terminal session runner is the first interactive harness for the rooted
Android prototype.

It is intentionally not a local-model runner yet. The current session command
uses deterministic typed intent parsing through the same `IntentCandidate`
boundary that local inference will eventually feed. That lets us test the
runtime contract without mixing in inference quality, provider setup, or prompt
behavior.

## Command

Run on the Pixel through the pushed `fawx-terminal-runner` binary:

```sh
fawx-terminal-runner session
```

The session supports:

- `open settings`
- `open launcher`
- `open package <android.package>`
- `list`
- `help`
- `quit`

The runner also exposes a non-session diagnostic:

```sh
fawx-terminal-runner local-model-probe
```

That command reports likely local model surfaces on the phone, such as AICore or
Gemini packages, but it does not call them or treat package presence as proof of
an inference API.

```sh
fawx-terminal-runner candidate-dry-run "open settings"
```

That command emits the `ModelCandidate` candidate JSON without granting,
executing, or observing anything.

## Contract

For an `open ...` prompt, the runner must:

1. Create a persisted task.
2. Add a scoped `AppControl` grant for the target Android package.
3. Submit a typed `OpenApp` action proposal to the agent loop.
4. Execute the action through the Android runtime adapter.
5. Sample foreground state.
6. Close the action only when typed foreground evidence matches the target package.

Command success alone is not completion. Completion requires observation
evidence.

## Model Boundary

The local model boundary is still future work.

When local inference lands, it must not be allowed to mint its own authority.
The current deterministic parser produces `OwnerCommand` intents because the
user directly names the action target. A model should instead produce intent
candidates that are accepted only under existing policy or after explicit owner
confirmation.

The session source is typed:

- `OwnerCommand` may add the exact scoped grant implied by the user's direct
  command.
- `ModelCandidate` may propose the same `IntentCandidate` shape, but it must not
  add grants. It can only be accepted when policy is already satisfied or after
  a separate owner-confirmation path adds the grant.

The rest of the session flow should stay the same:

```text
owner/model intent -> typed action proposal -> runtime execution -> observation -> verification
```

That keeps the model below the control plane instead of making prompt text the
source of authority.
