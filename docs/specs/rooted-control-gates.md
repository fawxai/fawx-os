# Rooted Control Gates

## Purpose

The rooted control gates are the no-AOSP test surface for gates 3-8 of the
agent-phone decision process:

3. input/computer-use control
4. notifications
5. communication surfaces
6. local model access
7. storage/file access
8. human handoff and approval

They do not claim rooted stock Android is the production substrate. They prove
which primitives can be exercised now through typed contracts and which ones
remain explicit privilege gaps.

## Command

Run:

```sh
./scripts/pixel-control-gates-smoke.sh
```

The script pushes the Android probe and terminal runner, then verifies:

- `InputKeyEvent` can drive the foreground back to Launcher and produce typed
  foreground evidence.
- notification posting fails as `RequiresAospPrivilege` instead of pretending a
  shell command is enough.
- messaging and calling fail as `RequiresAospPrivilege`.
- local model probing reports package surfaces without claiming inference.
- runtime scratch file read/write succeeds only under `/data/local/tmp/fawx-os`.
- the model-candidate human approval path still pauses and resumes through
  typed owner approval.

## Contract Shape

The rooted-stock adapter now has typed Android commands for:

- `InputKeyEvent`
- `InputTap`
- `InputText`
- `InputSwipe`
- `ReadScopedFile`
- `WriteScopedFile`
- `PostNotification`
- `SendMessage`
- `PlaceCall`

Only the safe rooted-stock probes execute today. Sensitive side-effect commands
exist so the control plane can test and display the privilege gap explicitly;
they should remain unavailable until a platform adapter can provide independent
evidence and an appropriate user permission flow.

`ReadScopedFile` and `WriteScopedFile` are intentionally mapped to runtime
scratch storage, not Android shared/scoped storage. They prove the harness can
write and re-read evidence under its own `/data/local/tmp/fawx-os` root; they
must not be used as proof that MediaStore, SAF, app-private storage, or user
shared storage boundaries are solved.

## Current Interpretation

Passing this smoke means:

- gates 3, 6, 7, and 8 are testable on rooted stock,
- gates 4 and 5 have explicit typed blockers,
- AOSP is not yet justified by these gates alone,
- the next high-value work is either richer UI-state observation for computer
  use or a real notification-listener/communication adapter prototype.
