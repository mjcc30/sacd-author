---
name: sacd-spec-lookup
description: Look up the normative Scarlet Book layout in the aclater/sacd-spec repo (spec/*.md + CC0 corpus images) before touching any TOC/ISO byte in sacd-author.
---

# sacd-spec-lookup

1. Clone if needed: `git clone --depth 1 https://github.com/aclater/sacd-spec /tmp/sacd-spec`.
2. Relevant normative files: `spec/01-disc-image-layout.md` (LBA map, master TOC at
   510, copies), `spec/02-master-toc-and-text.md` (master fields, locales, SACDText
   16-pointer order, charsets), `spec/03-area-toc.md` (area header, sub-structure walk,
   TRL1/TRL2, track text), `spec/04-audio-multiplexing.md` (sector packets, 4704 B/ch
   frames, bit order), `spec/00-conventions.md` (tc = mm:ss:ff, frame = (m*60+s)*75+f).
3. Corpus images for byte-level comparison: `corpus/images/*.iso` (e.g. `charset-1.iso`).
   Extract sectors with `dd if=img bs=2048 skip=LBA count=N`.
4. Always cite the spec section number when changing a byte in `src/toc.rs`/`src/image.rs`.
