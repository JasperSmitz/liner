# Milestones

M1–M3 run **in parallel** on top of M0 (one developer each, separate files, see
"Module ownership" in ARCHITECTURE.md). M4 integrates. Every milestone must keep
`cargo build`, `cargo clippy --all-targets -- -D warnings`, `cargo test` and
`cargo fmt --check` green, must not change frozen interfaces without the lead, and
must never touch `/storage/music`.

## M0: Skeleton (architect) — done

- Docs: ARCHITECTURE, ADRs 0001–0005, MILESTONES, RULES.
- Cargo package `liner` (edition 2024, lib + bin), all modules with frozen public
  types and signatures, placeholder bodies, no `todo!()`.
- `clippy.toml` write ban; `render::create_output_file` guard.
- Committed fixture templates + corrupt fixtures + generator script; test builder
  signatures in `tests/common/mod.rs`.
- Committed schema `schema/liner-report.v1.schema.json` and `tests/schema.rs`.
- Acceptance: the four commands above pass; `liner rules`, `liner schema` work;
  `liner scan <dir>` prints an empty report.

## M1: Scan + metadata + fixtures (developer A)

Files: `src/scan.rs`, `src/metadata.rs`, `tests/common/mod.rs`, new `tests/scan*.rs`,
`tests/metadata*.rs`.

- `scan`: walkdir walk; no symlink following by default (`--follow-symlinks` opt-in,
  loops become `walk` errors); hidden files/dirs skipped unless `include_hidden`;
  extension classification; non-audio counted; per-entry errors collected; root
  missing/not-dir → fatal error; sorted output.
- `metadata`: lofty read with cover art disabled; all `TrackInfo` fields mapped per
  ADR 0001; whitespace text and `0` numbers → `None`; errors classified
  `io`/`unsupported`/`corrupt`; rayon `read_all`; sorted output.
- `tests/common`: `Library::add` writes all `Tags` fields for all six formats.
Acceptance:
- Round-trip test: for each `Fixture::ALL`, tags written by the builder are read
  back identically by `metadata::read_track` (title, artist, album, album artist,
  track/total, disc/total, year, genre, compilation).
- Each corrupt fixture yields exactly one `FileError` (`corrupt`), no panic; a
  `.txt` renamed to `.mp3` yields `unsupported` or `corrupt`, not a panic.
- Hidden dir/file skipped by default and included with the option; a symlinked
  directory is not followed by default; a symlink loop with following on yields a
  `walk` error and terminates.
- Unreadable directory (permissions 000 in a temp dir; skip test if running as
  root) yields a `walk` error and the scan continues.
- `read_all` output identical across repeated runs.

## M2: Grouping + rules (developer B)

Files: `src/group.rs`, `src/rules.rs` (+ optional `src/rules/*.rs`), new
`tests/group*.rs`, `tests/rules*.rs`. Tests use hand-built `TrackInfo`s.

- `parse_disc_folder_name` per ADR 0002; `group_tracks` with merge, folder hints,
  effective disc, multi-disc detection, title/album artist consensus, disc_count,
  sorting; `AlbumUnit::album_key`.
- Every rule in `docs/RULES.md` implemented with the documented triggers and
  `details` keys; `run_all` sorted.
Acceptance:
- Disc folder name table test: positives (`CD1`, `cd 2`, `CD-03`, `Disc 1`,
  `DISK02`, `Disc 1 - The Beginning`, `disc_2`) and negatives (`CD`, `CDs`,
  `Discography`, `CD0`, `Disco 2`, `1 CD`).
- At least one positive and one negative (near-miss) test per rule code.
- Multi-disc scenarios: (a) CD1/CD2 folders, no disc tags → `missing-disc-number`
  only; (b) flat folder, tracks 1–10 twice, no disc tags →
  `likely-missing-disc-numbers` only (no `duplicate-track-position`); (c) disc tags
  1 and 3 → `disc-number-gap`; (d) file tagged disc 2 in `CD1` →
  `disc-folder-mismatch`; (e) a clean 2-disc album → no issues.
- Compilation with per-track artists and album artist "Various Artists" → no
  issues; same without album artist → only `missing-album-artist`.
- A test asserting every `CATALOG` code is emitted by at least one test scenario.

## M3: Report model + JSON schema + human renderer + CLI (developer C)

Files: `src/report.rs` (functions only; types frozen), `src/render.rs`,
`src/config.rs`, `src/cli.rs`, new `tests/cli*.rs`, `tests/report*.rs`.

- `build_report`: convert units → `AlbumEntry/DiscEntry/TrackEntry`, merge scan
  and read errors into `errors[]`, sort everything per `report.rs` docs, summary.
- `render_human`: album-grouped issues, then library issues, then errors, then
  summary; stable and readable at 80 columns.
- `config`: `default_config_path`, `load` per docs; unknown keys rejected.
- `cli`: as specified in ARCHITECTURE.md; exit codes 0/1/2.
Acceptance:
- `tests/schema.rs` passes; schema change only with a version decision.
- Golden test: a `Report` built from fixed inputs and a fixed timestamp serialises
  to a committed JSON file; deserialising it back yields an equal `Report`.
- Same inputs ⇒ identical bytes (run twice, with inputs shuffled).
- Exit code tests: issues below threshold → 0, at threshold → 1, `--fail-on never`
  → 0, bad root / bad config / unknown flag → 2; `--min-severity warning` drops
  info issues but `--fail-on info` still exits 1.
- Config precedence tests (CLI > file > default), explicit missing config → 2,
  missing default config ignored, unknown key → 2.
- `--output report.flac` refused; `--output dir/` refused.
- `liner scan … | head` (broken pipe) exits quietly, without an error message.
- Error messages: `LinerError`/`ConfigError` Display strings never repeat their
  `#[source]` (main prints the chain with `{:#}`).

## M4: Integration tests + docs + review hardening (all / lead)

- End-to-end tests on synthetic libraries built with `tests/common` covering every
  rule code through the binary, both output formats.
- Read-only test: snapshot (bytes + mtime + size) of every file in a fixture
  library before and after `liner scan --format json` (with `--output` outside the
  library) — must be identical, and no new files inside the library.
- Robustness: corrupt fixtures, non-UTF-8 file names (Linux), empty library, a
  library with only non-audio files, deep nesting.
- Performance smoke test: 2 000 generated files analysed in < 10 s in release
  mode (ignored by default; run in CI/release check).
- README: install, usage, exit codes, config, JSON contract pointer, consumer
  guidance (ADR 0003).
- Review pass (code review + simplification), all clippy/test/fmt green, no
  `unwrap` on file-derived data outside tests.
