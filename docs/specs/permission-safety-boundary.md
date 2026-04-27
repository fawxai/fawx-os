# Permission And Safety Boundary

## Purpose

Sensitive actions must be authorized by typed grants before the harness accepts
them for execution.

The model may propose an action, but the control plane decides whether the task
has authority to carry it out. This prevents prompt text from becoming the
permission system.

## Contract

Every task owns an `ExecutionContract`.

That contract now has two authority lanes:

- `grants`: broad runtime capability grants for components and surfaces.
- `safety_grants`: action safety grants for user-impacting operations.

`safety_grants` are explicit and typed. A grant includes:

- `SafetyCapability`: the kind of sensitive power being granted.
- `SafetyScope`: the target boundary for that power.

The current safety capabilities are:

- `AppControl`
- `Calling`
- `Messaging`
- `FilesystemRead`
- `FilesystemWrite`
- `Network`
- `NotificationsRead`
- `NotificationsPost`
- `RuntimeExecution`

The current safety scopes are:

- `Any`
- `AndroidPackage`
- `Contact`
- `File`
- `Network`
- `NotificationSurface`
- `RuntimeAction`
- `Service`
- `Url`

Exact scoped grants authorize only the matching target. `Any` is the explicit
wildcard and should be used sparingly in tests or trusted development flows.

## Action Gate

The harness derives required safety grants from the typed model action:

- `OpenApp(AndroidPackage)` requires `AppControl(AndroidPackage)`.
- `Interact(AndroidPackage)` requires `AppControl(AndroidPackage)`.
- `Navigate(Url)` requires `Network(Url)`.
- `Read(File)` requires `FilesystemRead(File)`.
- `Write(File)` requires `FilesystemWrite(File)`.
- `Read/Write(Url)` requires `Network(Url)`.
- `Communicate(Contact)` requires `Messaging(Contact)`.
- `Communicate(Service)` requires `Network(Service)`.
- `Execute(RuntimeAction)` requires `RuntimeExecution(RuntimeAction)`.
- `Execute(Service)` requires `Network(Service)`.

`Observe` and `Verify` do not require safety grants by default because they do
not perform side effects. If future observations become privileged, they should
gain explicit requirements at this same boundary.

## Root Rule

The safety boundary is enforced when accepting a model action proposal, before
the action can become the current action.

That means unauthorized work cannot enter the executable action state and then
depend on later adapters to remember to reject it.

## Terminal Runner

The terminal runner exposes a development command:

```sh
fawx-terminal-runner grant <task-id> <capability> <scope>
```

Example:

```sh
fawx-terminal-runner grant task-agent app-control android-package:com.android.settings
```

This command exists so Pixel smoke tests and terminal-only prototypes can grant
authority without adding a UI yet. A future shell should expose the same
contract through a user-facing permission flow.

## Rooted Control Gate Commands

The terminal runner also exposes deterministic Android control commands for
prototype smokes:

```sh
fawx-terminal-runner android-command keyevent KEYCODE_HOME
fawx-terminal-runner android-command write-file /data/local/tmp/fawx-os/probes/example.txt proof
fawx-terminal-runner android-command read-file /data/local/tmp/fawx-os/probes/example.txt
```

These commands are still typed Android adapter requests. The rooted-stock
adapter executes only capabilities marked `Available` or `Limited` for
`ReconRootedStock`. Notification posting, messaging, and calling commands are
intentionally present but rejected as `RequiresAospPrivilege`; that gives tests
and future UI code a real contract for the missing permission boundary instead
of a vague unsupported-command failure.

The `write-file` and `read-file` commands are runtime scratch probes, not proof
of Android shared storage access. They are restricted to `/data/local/tmp/fawx-os`
and map to runtime scratch capabilities so the safety layer does not confuse
test evidence with scoped-storage mediation.
