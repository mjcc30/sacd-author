# Hardware reference — burners and players

## Burners (writers)

### HL-DT-ST BD-RE BU40N (primary burner, internal)
- Blu-ray burner, USB 2.0. **Does not support DAO96** → burn SACD-R ISOs in
  **TAO / "choix automatique"** mode at **4x**, with **finalize** checked (CDBurnerXP).

### Verbatim 43888 (alternative burner)
- External **slimline Blu-ray writer**, USB 3.2 Gen 1 **USB-C**, bus-powered, black,
  141 × 136.5 × 14 mm, ~200 g. M-DISC ready, includes Nero software.
- Speeds: **BD-R write 6x**, BD-R DL write 6x, BD-R read 4x, **DVD±R write 8x**,
  BDXL 4x. Ultra HD 4K Blu-ray *playback* (as a drive, to a PC).
- Note: it is an **optical drive, not a standalone player** — it reads/writes discs
  attached to a computer. Like most slimline writers it is expected **not to support
  DAO96** either; burn in TAO at 4x and finalize.
- Source: verbatim-europe.com product page `#43888 Ultra HD 4K External Slimline
  Blu-ray Writer`.

## Players

### Sony BDP-S490 (primary target)
- 3D Blu-ray Disc player, **MediaTek MT8580** SoC, HDMI, Ethernet.
- Firmware **M12.R.0430 is the last version that reads SACD-Rs** (raw SACD images
  burned on DVD-R). Later firmwares (e.g. M12.R.0510) block SACD-R. The user's unit
  is already on M12.R.0430 — do **not** upgrade it. Downgrade path documented publicly
  (malcolmstagg.com/bdp-s390-downgrade.html) — we do not need it.
- Service manual: "BDP-S490 / BDP-S590 Service Manual" PDF (servicemanuals.us,
  scribd mirror). User manuals: sony.com support pages for BDP-S490.
- Settings used for SACD: « Couche lecture Super Audio CD », « Canaux de lecture
  Super Audio CD: DSD 2ch », HDMI audio auto.

### Sony BDP-S4100 (secondary test player)
- Blu-ray/DVD player (MT8580 family), Wi-Fi on some variants.
- Operating instructions: Sony PDF 4-442-385-51(1) / -11(2) / -E1(1) (sony.com
  support, res/manuals/4442/...).
- Service manual: "BDP-S4100 / BDP-BX510 / BDP-S5100 Service Manual ver 2.0"
  (servicemanuals.us, ~7.6 MB).

## Why SACD-R works at all on these

A SACD is physically a DVD (same capacity, same 650 nm laser); the differences are the
physical copy protection (pit-width modulation) and the minimum bitrate. A DVD-R with
a raw SACD image has neither protection nor the CD layer, so players that still accept
"unprotected SACD" (BDP-S490 ≤ M12.R.0430, BDP-S370, BDP-S4100…) read it. This is
documented publicly (journaldulapin.com, 2021/07/06, « Les SACD-R »). We author our
own images from our own/CC0 sources only — see AGENTS.md rule 1 — and we do not touch
firmware protections (rule 2).
