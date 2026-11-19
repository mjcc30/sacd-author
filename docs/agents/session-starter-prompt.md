# Session starter prompt — paste this at the start of a Vibe CLI session

(Copy everything below the line into the chat, in the repo root.)

---

Tu travailles sur le projet **sacd-author** (clone local dans ce dossier, branche
`vibe/dsf-parser-cli-a2e0dd`, PR draft #1 sur github.com/mjcc30/sacd-author).

Lis d'abord, dans cet ordre : `AGENTS.md`, `README.md`, `docs/best-practices.md`,
`docs/vibe-cli-setup.md`, `docs/hardware/players.md`, `docs/agents/README.md`.
Puis `git pull` pour être à jour, et vérifie l'état : `cargo fmt --check &&
cargo clippy --all-targets -- -D warnings && cargo test` (82 tests doivent passer).

**Règles dures (AGENTS.md) :** (1) contenu de test = CC0 / CC BY-NC-SA ou mes propres
disques rippés avec sacd-ripper uniquement — jamais de rips piratés, même en local ;
(2) aucune circumvention de mesure de protection (pas de reverse du firmware Sony pour
le blocage SACD-R — REA ne sert que sur nos propres binaires) ; (3) Rust idiomatique,
pas de `unwrap()` en prod, offsets documentés, tests à chaque changement, CI verte.

**Problème ouvert :** le Sony BDP-S490 (MT8580, firmware M12.R.0430) affiche « SACD »
mais une **liste de pistes vide** pour notre DVD-R gravé (« ça charge, rien ne
s'affiche »). La TOC a déjà été réécrite conforme au corpus CC0 (aclater/sacd-spec) :
frame_format=2, disc_type=0x00, max_byte_rate=705600, 0x80-0x8F à zéro, SACDText
Latin-1 avec l'ordre des pointeurs spec 02 §2.5.4. foobar2000 lit l'ISO bit-identique
et `extract` round-trip bit-exact.

**Objectif courant :** faire afficher la liste des pistes sur le BDP-S490. Procédure :
1. `cd samples && ../target/release/sacd-author convert *.dsf -o album.iso`, vérifier
   `dd if=album.iso bs=2048 skip=510 count=1 | head -c 8` → `SACDMTOC` ;
2. je grave (CDBurnerXP : image ISO, TAO/auto, 4x, finaliser, DVD-R neuf), je teste
   S490 puis S4100, je photographie l'écran TV + l'afficheur LED et je te les attache ;
3. tu analyses, tu corriges, `cargo test`, tu commit+push ; je `git pull` et on
   reburn. Boucle jusqu'à ce que la liste s'affiche.

Si la liste reste vide : diff 3 voies secteur par secteur (notre ISO vs
`corpus/images/charset-1.iso` du clone sacd-spec vs extrait de mon propre rip :
`dd if=mon.iso bs=2048 skip=510 count=110 of=ds_510_620.bin` que je t'attache) — voir
`docs/agents/toc-diff-agent.md`. Hypothèse suivante à tester : image « raw » sans
couche ISO9660/UDF (le corpus n'en a pas).

Demandes en cours côté moi : photos de l'écran du S490 après « Super Audio CD » → OK,
photo de l'afficheur LED pendant le chargement, et éventuellement `ds_510_620.bin`.

Réponds en français, concis, avec des blocs de commandes copiables.
