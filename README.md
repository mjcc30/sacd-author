# sacd-author

Open-source Rust SACD authoring tool to convert DSF files into burnable SACD-R ISO images
(target: Sony BDP-S490 / MediaTek MT8580).

## Supported formats

Input/output audio is **native DSD64 only**: DSF and DFF (both accepted as
input by extension; `extract` writes DSF by default, DFF with `-f dff`).
PCM formats (FLAC, WAV, WavPack, ...) are intentionally not supported: a
SACD-R needs 1-bit DSD, and PCM->DSD requires a lossy sigma-delta
modulation that is out of scope. Convert PCM to DSF first with an external
tool (sox/JRiver/dsd tools), then use sacd-author.

## Usage

```bash
# DSF -> SACD-R ISO (legacy form still works)
sacd-author track01.dsf track02.dsf ... -o album.iso
sacd-author convert track01.dsf track02.dsf -o album.iso

# SACD-R ISO -> DSF or DFF tracks (for non-SACD players / archiving)
sacd-author extract album.iso -o extracted/              # DSF (default)
sacd-author extract album.iso -o extracted_dff/ -f dff    # DFF
```

- Inputs: stereo DSD64 **DSF or DFF** files (2.8224 MHz), given in track order.
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

## Test sample

`samples/goldberg-var5-dsd64.dsf` is a CC0 (public domain) DSD64 file
("Variatio 5", Open Goldberg Variations — Kimiko Ishizaka, via decaud.io),
sha256 `bd3fbb4db97c7f8f7e9f4f780da34421c5ccaaf5a6a49182c62f4371db43234e`.
Re-download it with `scripts/fetch_test_sample.sh`. Regenerate the demo image:

```bash
sacd-author samples/goldberg-var5-dsd64.dsf -o goldberg.iso
```

## Multi-track demo album

```bash
scripts/fetch_album_samples.sh samples/   # downloads 2 more CC BY-NC-SA tracks (sha256-checked)
sacd-author samples/goldberg-var5-dsd64.dsf samples/decada-lounge-fashioned.dsf \
    samples/decada-una-sensacion.dsf -o album.iso
```
See `samples/README.md`.
