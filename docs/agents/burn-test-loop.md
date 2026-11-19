# Agent prompt — burn & test loop

Standard cycle for validating a sacd-author build on real hardware:

```bash
# 1. sync + build (user side, WSL)
cd /mnt/c/Users/mjcc/Documents/GitHub/sacd-author && git pull && cargo build --release

# 2. build the album ISO
cd samples && ../target/release/sacd-author convert *.dsf -o album.iso

# 3. sanity-check structure before burning
dd if=album.iso bs=2048 skip=510 count=1 2>/dev/null | head -c 8    # expect: SACDMTOC
# optional roundtrip:
../target/release/sacd-author extract album.iso -o /tmp/x && cmp /tmp/x/track1.dsf track1.dsf
```

Burn (CDBurnerXP): « Graver une image ISO » → album.iso → drive → **méthode TAO /
choix automatique** (BU40N: no DAO96) → **vitesse 4x** → **Finaliser le disque ✔** →
fresh DVD-R.

Test: BDP-S490 first (firmware must read M12.R.0430), then BDP-S4100. Photograph:
TV screen after HOME → Super Audio CD → OK; player LED; home menu disc badge.
Attach photos in chat.

Report template: commit hash | ISO size | burn settings | S490 result (track list? titles?
playback? 1 s "shhh" on track change?) | S4100 result | next hypothesis.
