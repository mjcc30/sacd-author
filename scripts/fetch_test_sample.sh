#!/bin/sh
# Download the CC0 test sample used by the integration tests/smoke tests:
# "Variatio 5" from the Open Goldberg Variations (Kimiko Ishizaka),
# DSD64 DSF, published as public domain (CC0) by decaud.io.
# SHA-256 is verified after download.
set -eu

URL="https://www.decaud.io/downloads/dsd/decaud-goldberg-var5-dsd64.dsf"
SHA256="bd3fbb4db97c7f8f7e9f4f780da34421c5ccaaf5a6a49182c62f4371db43234e"
OUT="${1:-goldberg-var5-dsd64.dsf}"

curl -fL --retry 3 -o "$OUT" "$URL"

echo "$SHA256  $OUT" | sha256sum -c -
echo "OK: $OUT ($SHA256)"
