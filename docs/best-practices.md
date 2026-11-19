# Best practices for agents/LLMs working on sacd-author

## Working style

- **Read before writing**: AGENTS.md → README.md → the module you touch (`src/toc.rs`
  header comments document every byte offset). Match existing style.
- **Small, verifiable steps**: one logical change per commit; run
  `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test` after every
  edit (parallel tool calls can race — run cargo after edits complete).
- **Tests are mandatory**: every parser/writer change adds/updates unit tests
  (roundtrip bit-exactness where applicable). Current suite: 82 tests.
- **No `unwrap()`/`expect()` in production code** — thiserror `SacdError` / anyhow with
  context. Panics only in tests.
- **Binary layouts**: document offsets in the module header comment (see `src/dsf.rs`,
  `src/toc.rs`). Never guess a field — check the spec (`sacd-spec` repo, spec/*.md)
  and, when in doubt, the CC0 corpus image bytes.
- **Conformance target**: `github.com/aclater/sacd-spec` (`corpus/images/*.iso`,
  `spec/*.md`) is the normative reference for TOC layout. Diff our output against the
  corpus field-by-field before changing any TOC byte; the hardware player is stricter
  than foobar2000 was.
- **Hardware feedback is evidence, not gospel**: a player showing "SACD" but an empty
  list means the master TOC was read but the area/track list was not. Photograph the
  screen + LED; attach sector extracts; iterate.

## Content & licensing rules (hard)

- Test inputs: **only** CC0/CC BY-NC-SA files and the user's own legally-owned discs
  ripped with sacd-ripper. Refuse pirated rips/ISOs (archive.org commercial SACD
  rips, etc.) even "for local testing" — contrefaçon (CPI L.335-2); copie privée
  needs a lawful source. No public sharing links (Drive/Photos links are not usable
  by the agent anyway — attach files in chat instead).
- **No protection circumvention**: no firmware reverse-engineering (REA included) to
  defeat SACD-R blocks or copy protection (CPI L.335-3-1). Firmware M12.R.0430 is the
  supported path.
- When unsure whether a source is licensed, ask the user instead of assuming.

## Communication

- Report PR/commit links and CI status after pushing; ask before monitoring CI.
- When a hardware test is needed, give the user exact copy-paste commands (WSL paths,
  CDBurnerXP settings) and tell them exactly what to photograph/attach.
- French or English both fine; match the user's language. Be concise; show commands
  in code blocks.
