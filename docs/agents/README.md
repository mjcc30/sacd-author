# Agent prompts (docs/agents/)

Ready-made system prompts for autonomous debugging sessions (Vibe CLI / Desktop).
Each assumes AGENTS.md has been read.

| Agent | File | Use when |
|---|---|---|
| TOC 3-way diff | `toc-diff-agent.md` | Player shows SACD but empty list; compare ours vs corpus vs own-disc rip |
| Hardware player debug | `hw-player-debug-agent.md` | Iterating on BDP-S490 / BDP-S4100 physical tests (photos, LED, menus) |
| Burn & test loop | `burn-test-loop.md` | Rebuild → convert → verify → burn → test → report cycle |

Skills (reusable capabilities) live in `docs/skills/`.
