# AGENTS.md

Global operating rules:

- Fix root causes, not symptoms.
- Do not settle for bandaids when the bug reveals a missing contract or broken control plane.
- Prefer architectural fixes over local patches when regressions expose a systemic flaw.
- Treat hidden or implicit contracts as bugs. If the kernel or control plane cannot see the real contract, fix the surface first.
- Use typed state, explicit ownership, and clear boundaries instead of prose inference or string heuristics whenever possible.
- Keep control planes aligned. Do not allow overlapping config, policy, and runtime layers to silently contradict one another.
- Deterministic requests should use deterministic execution paths. Do not force simple utility requests through broad planning loops.
- If a rule must be inviolable, enforce it in code, tests, or policy — not only in prompt text.
- Respect each repository's local doctrine. When a repo provides `AGENTS.md`, `ENGINEERING.md`, `DOCTRINE.md`, `TASTE.md`, or similar project guidance, treat those files as authoritative within that repo.

Working style:

- Be honest about whether a fix is architectural or a targeted patch.
- When a live failure reveals a primitive, extract it into a focused regression and fix that primitive directly.
- Prefer one clean durable fix to a chain of compensating heuristics.
