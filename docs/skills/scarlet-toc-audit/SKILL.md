---
name: scarlet-toc-audit
description: Audit a generated SACD-R ISO against the Scarlet Book invariants: SACDMTOC@510 (+copies 520/530 identical), area TOC-1/TOC-2 identical, frame_format low nibble != 0, last_audio_lsn inclusive, playtime == sum(track durations), text pointers valid, audio sectors start with non-DST header.
---

# scarlet-toc-audit

Checks to run on every generated ISO (scriptable with dd/python):

- [ ] LBA 510/520/530 all start with `SACDMTOC` and are byte-identical.
- [ ] 511-518 start with `SACDText`, 519 with `SACD_Man`.
- [ ] area_1_toc_1 (master 0x40) points to a `TWOCHTOC` sector; TOC-2 (0x44) is
      byte-identical to TOC-1; size (0x54) matches.
- [ ] area frame_format@0x15 low nibble is 2 or 3 (plain DSD), never 0 (DST).
- [ ] first_audio@0x48/last_audio@0x4C (inclusive) match TRL1 starts+lengths; playtime
      tc@0x40 == sum of TRL2 durations (frame numbers).
- [ ] disc_type@0x50 bit7 clear for DVD-R (no CD layer).
- [ ] first audio sector header bit0 = 0 (not DST); packet info bit7 = frame_start on
      the first sector of each frame.
- [ ] SACDText pointers (0x10-0x2F) are 0 or point into [0x40, 2048); strings NUL-
      terminated; charset = locale[0] low 3 bits.
