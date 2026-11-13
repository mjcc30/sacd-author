# sacd-author

Open-source Rust SACD authoring tool to convert DSF files into burnable SACD-R ISO images
(target: Sony BDP-S490 / MediaTek MT8580).

## Supported formats

Input audio is **native DSD64 only**, in any of these containers (chosen by
file extension):

- `.dsf` — Sony DSF (bit-reversed DSD bytes, ID3v2 tags at the end)
- `.dff` — Philips DSDIFF (native SACD bit order, optional `ID3 ` chunk)
- `.wav` — **WAV-DSD / DoP** (DSD over PCM, DoP Open Standard v1.1: 24-bit
  IEEE-float carrier at 176.4 kHz, marker bytes 0x05/0xFA; RIFF `LIST/INFO`
  INAM/IART/IPRD and `id3 ` chunks are read as tags)

`extract` writes DSF by default, DFF with `-f dff`.
Plain PCM formats (FLAC, PCM WAV, WavPack, ...) are intentionally not
supported: a SACD-R needs 1-bit DSD, and PCM->DSD requires a lossy
sigma-delta modulation that is out of scope. Convert PCM to DSF first with
an external tool (sox/JRiver/dsd tools), then use sacd-author.

## Usage

```bash
# DSF/DFF/DoP-WAV -> SACD-R ISO (legacy form still works)
sacd-author track01.dsf track02.dff track03.wav ... -o album.iso
sacd-author convert track01.dsf track02.dsf -o album.iso

# SACD-R ISO -> DSF or DFF tracks (for non-SACD players / archiving)
sacd-author extract album.iso -o extracted/              # DSF (default)
sacd-author extract album.iso -o extracted_dff/ -f dff    # DFF
```

- Inputs: stereo DSD64 (2.8224 MHz) **DSF, DFF or DoP-WAV** files, given in track order.
- Output: hybrid ISO 9660 + UDF 1.02 image, 2048-byte sectors, `SACD/` folder,
  `Master1.TOC` written at LBA 510 (byte offset 0xFF000).

## Pipeline

1. `dsf`/`dff`/`dop` — parse DSF headers + ID3v2, DFF chunks, or DoP WAV
   (RIFF chunks, 0x05/0xFA markers, LIST/INFO tags); all yield the same
   track representation
2. `sacd` — convert the DSD stream (4096-byte block pairs, or DoP 24-bit
   frames unpacked to the same layout) to byte-interleaved 2048-byte SACD
   sectors, zero-pad the last block
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
