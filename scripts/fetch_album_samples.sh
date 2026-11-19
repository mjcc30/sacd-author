#!/bin/sh
# Download the 3-album test tracks used for the multi-track demo ISO.
# All are freely licensed (CC0 / CC BY-NC-SA 4.0) and sha256-verified.
# Files are >100 MB so they cannot live in git; this script is the
# reproducible source of truth instead (see samples/README).
set -eu

DIR="${1:-samples}"
mkdir -p "$DIR"

fetch() { # url sha256 outfile
    url="$1"; want="$2"; out="$3"
    if [ -f "$out" ] && echo "$want  $out" | sha256sum -c - >/dev/null 2>&1; then
        echo "OK (cached): $out"
        return 0
    fi
    echo "downloading $out ..."
    curl -fL --retry 3 -C - -o "$out" "$url"
    echo "$want  $out" | sha256sum -c -
    echo "OK: $out"
}

# CC0: Open Goldberg Variations — Kimiko Ishizaka (via decaud.io)
fetch "https://www.decaud.io/downloads/dsd/decaud-goldberg-var5-dsd64.dsf" \
    "bd3fbb4db97c7f8f7e9f4f780da34421c5ccaaf5a6a49182c62f4371db43234e" \
    "$DIR/goldberg-var5-dsd64.dsf"

# CC BY-NC-SA 4.0: Proyecto Interacciones (2024) Década (via archive.org)
fetch "https://archive.org/download/decada_202411/Proyecto%20Interacciones%20(2024)%20D%C3%A9cada/DSD%2064%202.8MHz/II/10.%20Lounge-Fashioned%20(Segunda%20Escala)%20%5BFernando%20Morales%20-%20Diego%20Psardi%20-%20Joe%20Design%20-%20Cristobal%20Ambas%5D.dsf" \
    "f22cffa432f69d49be8da2d01956bf5272a7e83726fa6cd9dc0b2b1d5469f1e2" \
    "$DIR/decada-lounge-fashioned.dsf"

fetch "https://archive.org/download/decada_202411/Proyecto%20Interacciones%20(2024)%20D%C3%A9cada/DSD%2064%202.8MHz/III/10.%20Una%20Sensaci%C3%B3n%20(Tercera%20Escala)%20%5BFernando%20Morales%20-%20Daniel%20Perez%20Ibarburen%20-%20Joel%20Szenkier%20-%20Diego%20Palma%5D.dsf" \
    "94b0591b1bb8da42d15ae45585cd2d83752552a7c910d2db55e90fa521d1d3f0" \
    "$DIR/decada-una-sensacion.dsf"

echo "All samples ready in $DIR. Build the demo album:"
echo "  sacd-author $DIR/goldberg-var5-dsd64.dsf $DIR/decada-lounge-fashioned.dsf $DIR/decada-una-sensacion.dsf -o album.iso"
