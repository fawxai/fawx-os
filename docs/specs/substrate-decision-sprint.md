# Substrate Decision Sprint

## Purpose

The substrate decision sprint answers one question before we spend capital on
storage, cloud compute, or a full AOSP build:

> Can the Fawx OS control spine sail on rooted stock Android long enough to
> prove the agent-phone architecture, or has the substrate already shown us a
> core privilege wall that justifies AOSP investment?

This is intentionally not an AOSP build plan. It is the cheap evidence gate
before an AOSP build plan.

## Doctrine

- Do not buy hardware or rent cloud capacity to answer questions that rooted
  stock probes can answer.
- Do not treat shell success as a production primitive.
- Do not fake AOSP success from rooted-stock evidence.
- Keep the runtime contract stable: typed intents, typed actions, typed
  observations, typed policy grants, and typed blockers.
- Add AOSP only behind the same adapter contract, so rooted-stock learning is
  not throwaway work.

## Command

Run:

```sh
./scripts/pixel-substrate-decision-sprint.sh
```

The script requires:

- a connected ADB device,
- Android release binaries built with `./scripts/android-build.sh`,
- `jq`.

It runs:

- `pixel-substrate-compare-smoke.sh`
- `pixel-real-task-harness.sh`
- rooted-stock and AOSP-mode probe collection
- local model package-surface probing

It writes a JSON report to a temp file by default. To choose the output path:

```sh
FAWX_OS_DECISION_REPORT=/tmp/fawx-os-decision.json \
  ./scripts/pixel-substrate-decision-sprint.sh
```

## Decision Outputs

The report has three important sections:

- `recommendation`: the current platform investment recommendation.
- `must_have_primitives`: the gate table for foreground observation, launch,
  background execution, notification read, communication, and local model access.
- `raw_evidence`: unmodified probe output so the recommendation can be audited.

The default decision should remain `do_not_buy_ssd_or_build_aosp_yet` until at
least one must-have primitive produces concrete evidence that rooted stock is
blocked or too fragile and a privileged adapter path is ready to test.

## Go / No-Go Rules

Keep building on rooted stock if:

- action execution and observation closure are stable,
- the control plane can express blockers and handoffs without guessing,
- rooted-stock probes are good enough to learn the next primitive,
- AOSP mode still refuses to synthesize success without platform events.

Move toward AOSP only when:

- a must-have primitive is blocked or gross on rooted stock,
- the failure is captured as typed evidence,
- the corresponding AOSP adapter seam already exists,
- we can test one real privileged event without rewriting the harness.

Pause AOSP investment if:

- the only argument is "AOSP should make this possible,"
- the probe has no typed failure,
- success depends on shell output, dumpsys parsing, private APIs, or UI scraping
  that the kernel cannot independently verify.

## Current Expected Result

At this stage, the expected result is conservative:

- rooted-stock recon proves the control spine is alive,
- AOSP mode remains explicitly unavailable without real platform producers,
- communication and local model access remain unresolved,
- the recommendation is not to spend money yet.

That is a good result. It means we are learning without confusing a prototype
adapter with the future OS.
