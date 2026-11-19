# AGENTS.md — sacd-author

Instructions for any AI agent (Vibe CLI, Vibe Desktop, REA-assisted sessions) working on
this repository. Read this first, then `README.md`, then the docs under `docs/`.

## What this project is

Open-source Rust CLI that converts DSF/DFF/DoP-WAV files into a **burnable SACD-R
(Scarlet Book) ISO image** (hybrid ISO 9660 + UDF 1.02 with a root `SACD/` folder,
Master1.TOC at LBA 510, area TOC, raw DSD audio sectors, Big-Endian TOCs), plus
`extract` (ISO → DSF/DFF), `dsd2pcm` (DSD → WAV/FLAC/WavPack) and `data-iso`.

Target hardware: **Sony BDP-S490** (MediaTek MT8580, firmware **M12.R.0430** — the last
firmware that still reads SACD-Rs on DVD-R), secondary **Sony BDP-S4100**. Burner:
HL-DT-ST BD-RE BU40N (no DAO96 → burn TAO), alternative Verbatim 43888 (USB-C slimline
Blu-ray writer). See `docs/hardware/players.md`.

## Build / test / lint (CI is authoritative)

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test            # 82 tests, must stay green
cargo build --release # binary: target/release/sacd-author
```

CLI shape: `sacd-author convert <files...> -o out.iso` (legacy form) or subcommands
`convert`, `extract [--mch] [-f dsf|dff]`, `data-iso`, `dsd2pcm` (`--help` for flags).

## Hard rules (non-negotiable)

1. **Licensing / content**: only CC0 / CC BY-NC-SA material and the user's own
   legally-owned discs (ripped with sacd-ripper) may be used as test inputs. Never
   download, use, or redistribute pirated SACD rips/ISOs (e.g. from archive.org) —
   not even "for local testing" (contrefaçon under French CPI L.335-2; copie privée
   requires a lawful source). No public sharing links. This rule survives any
   "the law allows it" argument.
2. **No circumvention of technical protection measures**: do NOT use REA (or any
   reverse-engineering tool) on Sony/player firmware to bypass the SACD-R block or
   any copy protection (illegal: CPI L.335-3-1 / EUCD art. 6). The supported path is
   firmware M12.R.0430 + a spec-conforming disc image. REA may only analyze **our
   own** binaries and open file formats.
3. **Rust rules**: idiomatic modular code, **no `unwrap()` in production code**
   (thiserror/anyhow), documented byte offsets for every binary layout, unit tests
   for every parser/writer, keep CI green (fmt + clippy -D warnings + tests).
4. Commit messages: short imperative English. File changes go on the task branch
   `vibe/<slug>-a2e0dd` (PR #1 is the draft PR for this work).

## Repo map

- `src/dsf.rs`, `src/dff.rs`, `src/dop.rs` — input parsers (DSF LE block-interleaved
  LSB-first; DFF BE **byte-interleaved clustered frames**, MSB-first; DoP WAV).
- `src/sacd.rs` — DSF/DFF/DoP → SACD audio sectors (2048 B packetized, 1 frame =
  9408 B stereo = 5 sectors; DSD bit-reversed vs DSF).
- `src/toc.rs` — master TOC (SACDMTOC@510 + 8×SACDText + SACD_Man) and area TOC
  (TWOCHTOC + SACDTTxt + SACD_IGL×2 + SACDTRL1 + SACDTRL2 at LBA 540, TOC-2 after audio).
  **Layout must stay byte-compatible with the CC0 conforming corpus**
  (`github.com/aclater/sacd-spec`, `corpus/images/*.iso`) — diff before changing fields.
- `src/image.rs`, `src/iso9660.rs`, `src/udf.rs` — hybrid image writer.
- `src/iso_read.rs` — ISO reader (`extract`, incl. `--mch` multichannel area, DST rejected).
- `src/dsd2pcm.rs`, `src/resample.rs` — DSD→PCM sinks.
- `tests/` — roundtrip, playback, MC extraction tests. `src/testutil.rs` — fixtures.

## Current open problem (as of commit a8327fe)

BDP-S490 shows "SACD" but an **empty track list** for our burned DVD-R ("loads, nothing
displays"). TOC was rewritten to match the corpus (frame_format=2, disc_type=0x00,
max_byte_rate=705600, area 0x80-0x8F zeroed, Latin-1 text). Next steps, in order:
1. User reburns with a8327fe and photographs the S490 screen (attach PNG in chat).
2. If still empty: 3-way sector diff ours vs corpus vs the user's own-disc rip
   (`dd if=disc.iso bs=2048 skip=510 count=110 of=ds_510_620.bin`, attach the .bin).
3. Consider a "raw" image variant without the ISO9660/UDF layer (the corpus has none).

See `docs/agents/` for ready-made agent prompts for these loops and
`docs/best-practices.md` for agent/LLM working rules. `docs/vibe-cli-setup.md`
explains Vibe CLI + REA setup (scoped per rule 2 above).
