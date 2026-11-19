# Test samples

`goldberg-var5-dsd64.dsf` (66 MB) is committed directly (under GitHub's
100 MB/file limit). It is CC0 (public domain): "Variatio 5" from the Open
Goldberg Variations, Kimiko Ishizaka, via decaud.io.

The two additional album tracks are **too large for git** (> 100 MB), so
they are fetched reproducibly with sha256 verification instead:

```bash
scripts/fetch_album_samples.sh samples/
```

- `decada-lounge-fashioned.dsf` / `decada-una-sensacion.dsf` — CC BY-NC-SA 4.0,
  Proyecto Interacciones (2024) "Década", via archive.org item `decada_202411`.

Build the 3-track demo SACD-R image:

```bash
sacd-author samples/goldberg-var5-dsd64.dsf samples/decada-lounge-fashioned.dsf \
    samples/decada-una-sensacion.dsf -o album.iso
```
