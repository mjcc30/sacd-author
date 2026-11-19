# Vibe CLI + REA setup for this project

## Two interfaces, one project

- **Vibe Desktop** = chat + drag&drop attachments (screenshots, small `.bin` extracts).
  Attachments land in the session's upload dir and are directly analyzable.
- **Vibe CLI** (`vibe` in a terminal) = run commands in/around the repo. Typical loop:

```bash
cd /mnt/c/Users/mjcc/Documents/GitHub/sacd-author   # user's WSL clone
git pull                                            # get the agent's commits
cargo build --release
cd samples && ../target/release/sacd-author convert *.dsf -o album.iso
```

The agent (this session) works on its own clone and pushes to
`github.com/mjcc30/sacd-author`, branch `vibe/dsf-parser-cli-a2e0dd` (draft PR #1).
Sync is always via **git**, never via direct file access to the user's disk.

Desktop file permissions ("Tout l'ordinateur") authorize the *Desktop app's own*
agent side; they do not expose `C:\` to the analysis sandbox. To share a big file
(e.g. a 4.4 GB ISO), extract the sectors you need and attach the small `.bin`:

```bash
dd if=disc.iso bs=2048 skip=510 count=110 of=ds_510_620.bin   # ~225 KB
```

## Installing REA (morluto/rea)

REA = "Reverse Engineer Anything", an MCP that gives an agent binary/app inspection
tools (Hopper/Ghidra/IDA/JADX/JS static analysis). Official install:

```bash
npx rea-agents setup     # registers REA with your agent; see https://rea.tools/guides/
```

Node.js + npm required; on Linux a procps-compatible `ps` is needed for process
ownership checks (see REA `docs/getting-started/installation.md`). Always install
from the official repo (morluto/rea), rea.tools, or the `rea-agents` npm package —
never from a lookalike repo.

### ⚠️ Scoped use only (AGENTS.md rule 2)

REA in this project may analyze **only our own artifacts**: our Rust binary, our ISO
images, open file formats (DSF/DFF/DoP), and the public Scarlet Book spec corpus
(aclater/sacd-spec). It must **never** be pointed at Sony/player firmware or any
proprietary binary to bypass copy protection or the SACD-R firmware block — that is
circumvention of a technical protection measure (illegal: CPI L.335-3-1, EUCD art. 6)
and outside this project's licensing stance. The supported path for the BDP-S490 is
firmware M12.R.0430 + a spec-conforming image, nothing else.

## Debugging autonomously from the CLI

Loop for the open "empty track list on BDP-S490" issue:

1. `git pull && cargo build --release` — always test the latest pushed commit.
2. Build the ISO, verify structure locally:
   `dd if=album.iso bs=2048 skip=510 count=1 | head -c 8` → `SACDMTOC`.
3. Burn (CDBurnerXP: ISO image, TAO/auto, 4x, finalize, fresh DVD-R), test S490 then
   S4100, photograph the TV screen and the player's LED display.
4. Attach photos here; attach small sector extracts (`.bin`) when a byte diff is needed.
5. Agent side: 3-way diff (ours vs `sacd-spec` corpus vs own-disc rip) sector by sector
   — see `docs/agents/toc-diff-agent.md`.
