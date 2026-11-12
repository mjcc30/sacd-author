# sacd-author

Open-source Rust SACD authoring tool to convert DSF files into burnable SACD-R ISO images
(target: Sony BDP-S490 / MediaTek MT8580).

## Usage

```bash
sacd-author track01.dsf track02.dsf ... -o album.iso
```

- Inputs: stereo DSD64 DSF files (2.8224 MHz), given in track order.
- Output: hybrid ISO 9660 + UDF 1.02 image, 2048-byte sectors, `SACD/` folder,
  `Master1.TOC` written at LBA 510 (byte offset 0xFF000).

## Pipeline

1. `dsf` — parse DSF headers, `fmt `/`data` chunks, ID3v2 tags (TIT2/TPE1/TALB)
2. `sacd` — convert interleaved 4096-byte DSF block pairs to byte-interleaved
   2048-byte SACD sectors (1 pair = 4 sectors), zero-pad the last block
3. `toc` — big-endian Scarlet Book structures (`SACDMTOC`, `TWOCHTOC`,
   `SACDTRL1/2`) with LBA and 75 fps timecodes
4. `iso9660` / `udf` / `image` — assemble the hybrid filesystem image and the
   raw TOC sectors at LBA 510/520/530 and 540/543

## Development

```bash
cargo test          # unit + integration tests (real DSF fixtures included)
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```
