# Agent prompt — hardware player debug loop (BDP-S490 / BDP-S4100)

You are driving the physical test loop for SACD-R playback on real players.

## Known state

- BDP-S490 (MT8580, firmware **M12.R.0430**) shows the disc as "SACD" but the track
  list is **empty** ("it loads, nothing displays"). Same with the TOC rewrite
  (a8327fe: frame_format=2, disc_type=0x00, max_byte_rate=705600, area 0x80-0x8F=0,
  Latin-1 SACDText with spec-02 pointer order).
- foobar2000 (foo_input_sacd) plays our ISO bit-identically to the source DSF, and
  `extract` round-trips bit-exact — so the *content* is right; the *player acceptance*
  is the open issue.

## What to collect from the user each round

Attach in chat (drag&drop works):
1. Photo of the TV screen **after** HOME → « Super Audio CD » → OK (track list screen,
   empty list, or error message — transcribe all text).
2. Photo of the player's **LED display** during loading (LOADING / SACD / DVD / STOP…).
3. Photo of the home menu showing the disc badge at the top.
4. If needed: `dd if=album.iso bs=2048 skip=510 count=110 of=toc.bin` attached as `.bin`.

## Hypotheses to test, in order

1. **Burn quality / mode**: fresh DVD-R, TAO (BU40N has no DAO96), 4x, finalize ON
   (CDBurnerXP). Try DVD+R once for comparison. Try burning with the Verbatim 43888
   (USB-C slimline writer) if available.
2. **disc_type**: we now write 0x00 (no CD layer). If still empty, check whether the
   player wants the hybrid bit — no: a DVD-R has no CD layer; keep 0x00 unless a
   *conforming* reference says otherwise.
3. **Filesystem layer**: the CC0 corpus image has **no ISO9660/UDF at all** (raw sectors
   from LBA 0/510). Build a `raw` image variant (no PVD/UDF, just master TOC @510 onward)
   and test — the MT8580 may prefer the raw layout.
4. **TOC copies**: master at 510/520/530 must be identical (spec 01 §1.5.1 compares
   copies); area TOC-1 @540 and TOC-2 after audio must be identical (spec 03 §3.2
   warns area-toc-copies-differ). Verify byte-identical in our image.
5. **Control test**: play a *commercial* SACD in the S490 and photograph the track list —
   proves the player/firmware displays text at all. Also test our DVD-R in the S4100.
6. If all fail: 3-way TOC diff (see `toc-diff-agent.md`) against the user's own-disc
   rip sectors.

## Rules

- AGENTS.md rules 1-3 always. Never suggest firmware downgrade hacks or protection
  circumvention; M12.R.0430 is already installed and sufficient.
- Give the user exact WSL/PowerShell commands and exact CDBurnerXP clicks each round.
- One variable per burn when possible; label each disc (TOC version) in the report.
