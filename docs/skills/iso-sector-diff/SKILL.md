---
name: iso-sector-diff
description: Byte-level diff of SACD images (ours vs corpus vs own-disc rip) with a small Python script; prints field tables for master TOC, area TOC, sub-structures and audio sector headers.
---

# iso-sector-diff

Use a Python heredoc (no extra deps) reading 2048-byte sectors:

```python
import struct
def sec(f, lba):
    with open(f,'rb') as fh:
        fh.seek(lba*2048); return fh.read(2048)
```

Dump and compare: master 510 fields (0x40 ptrs, 0x50 disc_type, 0x54/56 sizes, 0x80
tcc, 0x88 locales), area header fields (0x10 max_byte_rate, 0x15 frame_format, 0x20 ch,
0x40 playtime tc, 0x48/0x4C audio LSNs inclusive, 0x50 tcc, 0x58 locales, 0x80-0x8F),
sub-structure signatures (walk from area+1: SACDTTxt/SACD_IGL/SACDTRL1/SACDTRL2/SACD_ACC),
TRL1/TRL2 entries, first audio sector header byte + packet info u16 + 3-byte timecode,
SACDText pointer table @511. Output a 3-column table and rank candidate fixes.
