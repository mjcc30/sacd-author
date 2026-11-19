---
name: licensing-guard
description: Enforce the project's content rules: only CC0 / CC BY-NC-SA sources and the user's own legally-owned disc rips (sacd-ripper); refuse pirated SACD rips/ISOs even for local testing; no public sharing links; no circumvention of copy protection or firmware SACD-R blocks (CPI L.335-2, L.335-3-1).
---

# licensing-guard

Before using ANY external audio/disc file in this project, verify its license:

- Allowed: CC0 (e.g. decaud.io Goldberg samples), CC BY-NC-SA (e.g. Década /
  Proyecto Interacciones), the user's own discs ripped with sacd-ripper.
- Refused: commercial SACD ISOs/rips found online (archive.org etc.), "PD-marked"
  bogus items, anything without a clear license. Refusal is final even if the user
  argues local-download legality — downloading without authorization is contrefaçon
  (CPI L.335-2); copie privée requires a lawful source.
- Never produce or request public download links for content.
- Never analyze player firmware to bypass protections (CPI L.335-3-1). The supported
  path is firmware M12.R.0430 + a spec-conforming image.
- When in doubt, ask the user for the source/license instead of assuming.
