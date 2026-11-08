# ADR 0001: lofty for tag reading (read-only use)

Status: accepted (M0)

## Context
Liner must read tags and basic properties from FLAC, MP3 (ID3v1/v2, APE), Ogg
Vorbis, Opus, MP4/M4A, WAV (RIFF INFO/ID3v2) and ideally AIFF, APE, WavPack and
Musepack, in pure Rust, without ever modifying files.

## Decision
Use **lofty 0.25.x** (pure Rust, broad format coverage, unified `Tag`/`Accessor` API
with per-format access when needed).

- Read via `lofty::probe::Probe` / `read_from_path` with `ParseOptions`
  `read_cover_art(false)`. Format detection is content-based with extension fallback.
- Map lofty `FileType` → `model::AudioFormat` and `TagType` → `model::TagKind`
  inside `metadata.rs`; no lofty type crosses the module boundary or reaches the
  report.
- Per field, take the primary tag's value, then fall back to other tags present.
  Track/disc numbers and totals come from lofty's accessors (which parse `3/12`).
  Year from `RecordingDate`/`Year`, falling back to `ReleaseDate`/
  `OriginalReleaseDate`. Compilation from `ItemKey::FlagCompilation`.
- Every lofty error becomes a `FileError` (`unsupported` vs `corrupt` vs `io`).
- Writing APIs (`save_to*`, `remove_from*`) are banned via `clippy.toml`; only the
  test fixture builder uses them, on temp copies.

## Consequences
- lofty 0.25 requires Rust ≥ 1.89 (`rust-version = "1.89"`).
- Verified in M0 against the committed templates: all six fixture formats read, tag
  round-trips work on copies (`primary_tag_type` is Id3v2 for MP3 and WAV), and the
  three corrupt fixtures produce `FileParseError`s, not panics.
- lofty's interpretation of odd tag encodings is accepted as-is; Liner reports what
  lofty sees.
