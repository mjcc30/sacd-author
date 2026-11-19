# Agent prompt — TOC 3-way sector diff

You are debugging why a hardware SACD player accepts our master TOC but shows an empty
track list. Do a byte-level, field-by-field comparison of three images:

1. **OURS**: freshly built `album.iso` (or a small test ISO from one DSF).
2. **CORPUS**: CC0 conforming image from `github.com/aclater/sacd-spec`
   (`corpus/images/charset-1.iso`; clone shallow: `git clone --depth 1`).
3. **OWN-RIP**: the user's own legally-owned disc rip (extract provided by the user as
   `ds_510_620.bin` = sectors 510-619, via
   `dd if=disc.iso bs=2048 skip=510 count=110 of=ds_510_620.bin`). Never use a
   downloaded/pirated rip (AGENTS.md rule 1).

## Procedure

1. Parse master TOC at LBA 510 in each: version, area pointers (0x40-0x4F), disc_type
   (0x50), area sizes (0x54/0x56), catalogs, genres, date, text_channel_count (0x80),
   locales (0x88). Print a side-by-side table; flag every structural difference.
2. Parse area TOC header at the area_1_toc_1 pointer: size@0x0A, max_byte_rate@0x10,
   sample_freq@0x14, **frame_format@0x15 low nibble** (0=DST, 2=DSD 3in14, 3=DSD 3in16),
   channel_count@0x20 (u8), config@0x21, total_playtime tc@0x40 (3-byte mm:ss:ff,
   frame = (m*60+s)*75+f), track_offset@0x44, track_count@0x45, first_audio@0x48,
   **last_audio@0x4C inclusive**, text_area_count@0x50, locales@0x58 (10×4B),
   0x80-0x8F reserved, area text offsets@0x90-0x97.
3. Walk the sub-structure chain from sector 1 of the area TOC (spec 03 §3.7): record
   positions of SACDTTxt, SACD_IGL (2 sectors), SACD_ACC (32, if any), SACDTRL1, SACDTRL2;
   stop at first unknown signature. Compare the sector signature sequence.
4. Compare TRL1 (start LSNs @8+4i, lengths @1028+4i) and TRL2 (start tc+flags @8+4i,
   durations @1028+4i) entry by entry; verify track_start/track_end vs audio range.
5. Compare the first audio sectors: header byte (bit0=DST), packet info u16 BE
   (bit7 frame_start, data_type bits3-5, len bits0-10), 3-byte timecode on the first
   sector of each 75 fps frame; frame = C×4704 B interleaved.
6. Compare master text sectors 511-518 (SACDText: 16 u16 pointers @0x10, order per spec
   02 §2.5.4: album 0-3, album phonetic 4-7, disc 8-11, disc phonetic 12-15; strings
   from 0x40; locale charset from master locale[0] @0x8A low 3 bits; charset 1 decodes
   0x80-0xFF as Latin-1) and SACD_Man @519.
7. Output: a diff table (field | ours | corpus | own-rip | verdict), then a ranked list
   of candidate fixes, each with the exact byte offsets to change in `src/toc.rs` /
   `src/image.rs`. Never change a field without citing the spec section or the corpus
   bytes that justify it.

## Rules

- The aclater/sacd-spec `spec/*.md` files are normative; quote section numbers.
- Keep AGENTS.md rules 1-3. No player-firmware analysis.
- After each code change: `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`,
  rebuild, regenerate the test ISO, re-run this diff until ours == corpus on all
  structural fields (content fields like catalog numbers/date/track count may differ).
