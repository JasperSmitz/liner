# Liner architecture

Liner is a standalone, strictly **read-only** Rust CLI that scans a music library,
reads tags, groups files into albums and reports metadata inconsistencies, as
human-readable text or as a stable, versioned JSON report. Other programs (e.g.
Gargoyle) consume the JSON report; they never link Liner code. No network access.

## Crate layout

One package `liner` (edition 2024), with a library target (all logic, testable) and
a thin binary `liner` (`src/main.rs`). See ADR 0005.

```
src/
  main.rs      bin: parse CLI, call cli::run, map errors to exit code 2
  lib.rs       module list + analyze() pipeline glue
  model.rs     shared domain types (TrackInfo, FileError, Issue, Severity, Scope, ...)
  error.rs     LinerError (fatal errors only)
  scan.rs      walk the library, classify by extension
  metadata.rs  lofty -> TrackInfo, per-file FileError, rayon parallel
  group.rs     album units, disc folders, effective disc
  rules.rs     rule catalogue (codes, severity, scope) + run_all
  report.rs    serialisable report = public JSON contract, SCHEMA_VERSION, schema
  render.rs    human + JSON renderers, control-char escaping, the single
               sanctioned output-file writer (OutputTarget)
  config.rs    optional TOML config
  cli.rs       clap definitions, settings resolution, command dispatch
schema/liner-report.v1.schema.json   committed JSON Schema (checked by tests/schema.rs)
scripts/gen-fixture-templates.sh     regenerates committed fixture templates (ffmpeg)
tests/common/mod.rs                  synthetic library builder (fixtures)
tests/fixtures/{templates,corrupt}/  committed tiny audio templates + corrupt files
```

### Module ownership (parallel development)

| Owner | Files | Milestone |
|---|---|---|
| architect | `model.rs`, `error.rs`, `lib.rs`, `clippy.toml`, report *types*, docs | M0 |
| developer A | `scan.rs`, `metadata.rs`, `tests/common/mod.rs`, `tests/scan*.rs`, `tests/metadata*.rs` | M1 |
| developer B | `group.rs`, `rules.rs` (may split into `src/rules/*.rs`), `tests/group*.rs`, `tests/rules*.rs` | M2 |
| developer C | `report.rs` (functions), `render.rs`, `config.rs`, `cli.rs`, `tests/cli*.rs`, `tests/report*.rs` | M3 |

Public signatures in every module are frozen by M0. A change to another owner's
interface, to `model.rs` or to the report types goes through the lead. Developers B
and C unit-test against hand-built `TrackInfo` / `AlbumUnit` values (via
`TrackInfo::new` + struct update), so they do not wait for M1.

## Data flow

```
PATH ─► scan::scan ──► ScanResult { audio_files, other_files, errors }
            │
            ▼
      metadata::read_all (rayon) ──► MetadataResult { tracks: Vec<TrackInfo>, errors }
            │
            ▼
      group::group_tracks ──► Vec<AlbumUnit> (directory units, disc folders merged,
            │                                  album partitions, effective disc per track)
            ▼
      rules::run_all ──► Vec<Issue> (sorted)
            │
            ▼
      report::build_report ──► Report (sorted, summary computed)
            │      cli: --fail-on evaluated on all issues + errors[], then --min-severity filter
            ▼
      render::render ──► stdout or --output FILE (human | json)
```

`liner::analyze(root, &ScanOptions, generated_at)` runs scan → report and is what
the CLI and integration tests call. Every stage after `scan` is a pure function of
its input except `metadata` (file reads).

Key contracts (details in module docs):

- **Paths.** Files are opened through `path` (root joined with the walked entry).
  Everything user-visible uses `rel_path`: root-relative, `/`-separated, lossy
  UTF-8 (`model::rel_path_string`). The root album id is `"."`.
- **Text values** keep their original spelling. Comparisons use
  `model::normalize_for_compare` (Unicode NFC, curly quotes folded to straight
  ones, trim, collapse whitespace, Unicode lowercase).
  Empty/whitespace text and numeric `0` are read as "absent".
- **Untrusted text.** Tags and file names may contain control characters. The
  human renderer passes every tag- or path-derived string through
  `render::escape_control` (each `char::is_control` becomes a visible `\u{XX}`),
  and `RuleInfo::issue` does the same for `Issue.message`, so JSON messages are
  single-line. JSON paths and tag values stay exact (JSON escapes them).
- **Determinism.** All outputs are sorted (scan by rel_path, units by id, issues by
  `Issue::sort_key`, errors by rel_path then real path; maps are `BTreeMap`).
  `errors[]` is deduplicated on the real path, so two files whose lossy names
  collide are both listed. Parallel reading must not
  affect output. Same library + same timestamp ⇒ byte-identical JSON.

## CLI

```
liner scan [PATH] [--format human|json] [--output FILE]
           [--fail-on error|warning|info|never] [--min-severity info|warning|error]
           [--follow-symlinks] [--include-hidden] [--config FILE]
           [--color auto|always|never]
liner rules     # list rule codes, severity, scope, summary
liner schema    # print the report JSON Schema
```

- `PATH` falls back to `library_path` from the config file
  (`--config FILE`, else `$XDG_CONFIG_HOME/liner/config.toml`, else
  `~/.config/liner/config.toml`). An explicit `--config` must exist; a missing
  default config is ignored. Precedence: CLI > config > built-in default.
  Unknown config keys are an error. A relative `library_path` in the config is
  resolved against the config file's directory (not the current directory); `~`
  is not expanded. Boolean settings can only be switched *on* from the CLI.
- `--fail-on` (default `error`) is evaluated on **all** issues, before
  `--min-severity` (default `info`) filters the issues in the output. The report's
  `summary` describes the issues that are in the report; the filter is recorded in
  `options.min_severity` and the number of removed issues in
  `summary.issues_hidden`. Human output says so too ("No issues at or above error
  (3 lower-severity issues hidden)", and "(N below <sev> hidden)" on the summary
  line), so an exit code 1 is never unexplained.
- Exit codes: `0` no issue at/above `--fail-on` and no unreadable file; `1` such
  issues found, or `errors[]` is non-empty; `2` runtime or usage error (clap's own
  usage errors also exit 2). Entries in `errors[]` (walk/io/unsupported/corrupt)
  are a library-health problem and count as **error** severity for `--fail-on`, so
  only `--fail-on never` ignores them (lead decision, M3). The logic is the pure
  `cli::outcome` function.
- Human output is grouped per album (only albums with issues: header with id,
  title, album artist, disc/track counts; issues indented with a severity marker),
  then library issues, then unreadable files, then a summary line. ANSI colour is
  used only with `--color always`, or with `--color auto` (default) when stdout is a
  terminal and `NO_COLOR` is unset or empty. JSON is never coloured; an
  `--output` file only with `--color always`.
- A closed stdout (`liner scan | head`) ends output quietly; the exit code is still
  computed from the report.
- `--output FILE`: `render::check_output_path` runs before the scan (no audio
  extension in any case, not an existing non-regular file or symlink, has a file
  name, parent directory exists) and **opens the parent directory**, keeping the
  handle in an `OutputTarget`. The parent must not be inside the library: walking
  up from that handle (`openat(.., O_PATH)`) to the filesystem root, no ancestor
  may have the library root's `(st_dev, st_ino)`; this catches symlinked parents
  and bind-mount aliases. The report is rendered to memory and written only after
  the scan and rendering succeeded, by `OutputTarget::write`, atomically (see
  "Read-only guarantees"), so a failed run never changes an existing report.
  Not detected: with `--follow-symlinks`, a directory symlinked into the library
  from outside (documented in the README).

## Error handling

- **Fatal** (`LinerError`, exit 2): unusable root (missing/not a directory), no
  library path, config read/parse error, unsafe or unwritable `--output`, output
  write failure. `main` prints `liner: <error chain>` to stderr.
- **Per-entry** (`model::FileError`, never fatal): walk errors (permission denied,
  symlink loop) → `walk`; open/read I/O → `io`; unrecognised format → `unsupported`;
  malformed/truncated/empty → `corrupt`. Collected into the report's `errors[]`.
- **No panics** on any file content. `unwrap`/`expect` only for true invariants
  (e.g. serialising an in-memory schema). Rules must handle every combination of
  absent fields.

## Read-only guarantees

1. Audio files are only ever opened through lofty's read path (`Probe` /
   `read_from_path`), which opens files read-only. Directory walking only lists
   and stats.
2. `clippy.toml` `disallowed-methods` bans, crate-wide: lofty `save_to*` /
   `remove_from*`, `std::fs::{write, copy, rename, remove_*, set_permissions}`,
   `File::create[_new]` and `OpenOptions::new`. CI runs
   `cargo clippy --all-targets -- -D warnings`, so a violation fails the build.
   `libc::{rename, renameat, unlink, unlinkat, creat}` are banned too and allowed
   only on `render::OutputTarget::write_with`. `libc::open`/`openat` cannot be
   banned (they are also used read-only, for directory handles), so by convention
   only `OutputTarget::create_temp` passes `O_CREAT`.
3. The only exemption in non-test code is `render::OutputTarget` (the
   user-requested report). Everything happens relative to the output directory
   handle opened by `check_output_path`, so swapping the parent path during the
   scan cannot redirect the write. `OutputTarget::write`:
   1. repeats the library ancestry check (the directory may have been moved);
   2. creates `.<name>.liner-tmp-<pid>` with
      `openat(O_WRONLY|O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC, 0644)`, writes the
      whole report and `fsync`s it;
   3. `fstatat(name, AT_SYMLINK_NOFOLLOW)`: the destination must be absent or a
      regular file with `nlink == 1` (no symlink, no hard link to an audio file);
   4. `renameat`s the temp file over `name` and `fsync`s the directory.
   On any failure after step 2 the temp file is `unlinkat`ed. Renaming replaces a
   directory entry and never writes into an existing inode, and the name never
   has an audio extension. A mistyped `--output` cannot clobber music or create
   files in the library, and a failed write never truncates an existing report.
4. Test code that writes tags (`tests/common/mod.rs`) opts out explicitly and only
   writes inside its own `TempDir`. Tests never read the developer's real config
   (`XDG_CONFIG_HOME`/`HOME` are overridden) and never touch `/storage/music`.
5. `tests/readonly.rs` asserts that a scanned fixture library is byte-, mtime- and
   permission-identical before and after `liner scan`, with no new files inside.

## Performance notes

- Walking is sequential (walkdir); I/O-bound tag reading is parallel with rayon
  (`par_iter` over `ScannedFile`s), results sorted afterwards.
- lofty is configured with `read_cover_art(false)` and reads only headers/tags plus
  the properties needed for duration, so cost is roughly proportional to tag size,
  not audio size.
- Grouping uses `BTreeMap`s keyed by directory/normalised values: O(n log n).
  Library-scope split detection groups album *partitions* (not whole units) by
  their per-partition `album_key` (one map pass over all partitions; ADR 0002),
  so a mixed directory can take part in a split with each of its albums.
- Memory: one `TrackInfo` per file (a few hundred bytes); a 100k-track library
  stays well under 100 MB. No caching between runs.
- Target: 10k files on a warm cache in a few seconds. Measured by the ignored
  `tests/perf.rs` smoke test (2 000 files through the release binary in < 10 s).
