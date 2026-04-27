#!/usr/bin/env bash
set -euo pipefail

cat <<'MATRIX'
AOSP Escape Matrix
==================

Score legend:
U = untested or no adapter yet
0 = not accessible
1 = shell/recon only
2 = app/framework hook but fragile
3 = privileged AOSP integration works
4 = stable typed system-service/native bridge
5 = Linux/native primitive independent of Android framework

Capability                  Score  Exit Pressure  Current Evidence
--------------------------  -----  -------------  ----------------
Foreground observation      1      Medium         rooted recon uses dumpsys; AOSP AdapterUnavailable
App launch/resume           1      Medium         rooted recon uses monkey; AOSP command not implemented
Background execution        1      High           Rust process runs via adb/recon; no AOSP supervisor
Notification read           U      High           no adapter yet
Notification post           U      Medium         requires AOSP privilege; no adapter yet
Phone call                  U      High           no telephony adapter yet
Messaging                   U      High           no messaging adapter yet
Shared storage read/write   1      Medium         rooted paths only; no scoped platform mediator
Local model access          1      Medium         AICore/Gemini detected; no inference adapter
UI automation/computer use  U      High           no background-capable control surface
Ephemeral verification UX   1      Medium         terminal approval exists; no OS-native surface

Next evidence gates:
1. Replace AOSP foreground AdapterUnavailable with a real platform event.
2. Replace shell launch with a platform app launch/resume command.
3. Prove a background supervisor survives app switching.
4. Bridge notifications as typed runtime events.

Decision gate:
- Must-have primitives: background execution, foreground/window observation,
  app launch/resume, notification read, one communication surface
  (messaging or calling), and local model access or credible local-provider
  bridge.
- If a must-have stays U, we do not have enough evidence.
- If a must-have stays 0-2 after a serious prototype, pause AOSP investment.
- Score 3 is acceptable only with a credible path to score 4.
MATRIX
