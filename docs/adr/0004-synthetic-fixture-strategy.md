# ADR 0004: Synthetic fixture strategy

Status: accepted (M0)

## Context
Tests need many tag permutations across formats, without real music (copyright,
size) and without ever touching the real library (`/storage/music`).

## Decision
- **Templates**: one tiny, silent, tag-free file per format (FLAC, MP3, Ogg Vorbis,
  Opus, M4A/AAC, WAV; 0.2 s mono, 8 kHz, bitexact), generated once by
  `scripts/gen-fixture-templates.sh` (ffmpeg) and committed under
  `tests/fixtures/templates/silence.*` (largest ≈ 8 KB). Tests never need ffmpeg.
- **Corrupt fixtures** committed under `tests/fixtures/corrupt/`: `empty.flac`
  (0 bytes), `truncated.flac` (FLAC magic cut inside STREAMINFO), `garbage.flac`
  (1 KiB deterministic pseudo-random bytes).
- **Builder** `tests/common/mod.rs`: `Library::new()` (a `TempDir`),
  `add(rel, Fixture, &Tags)` copies a template and writes tags with lofty onto the
  copy, `add_corrupt`, `add_raw` for non-audio files. Integration tests describe a
  library declaratively, then run `liner::analyze` or the binary on it.
- Tag writing with lofty exists **only** in this test helper, opted out of the
  clippy write ban explicitly, and only inside its temp dir.
- Unit tests for `group`/`rules` construct `TrackInfo` values directly (no files).
- CLI tests override `XDG_CONFIG_HOME`/`HOME` so a developer's config (and their
  library) is never used.

## Consequences
- Fast, hermetic tests; fixtures add < 20 KB to the repo.
- Format quirks of real-world taggers (e.g. ID3v2.3 vs 2.4, iTunes atoms) are only
  covered as far as lofty's writer produces them; specific quirks get dedicated
  committed fixtures if a bug requires it.
