# liner

Liner is a command-line linter for music libraries. It walks a directory tree,
reads the tags of every audio file, groups the files into albums and reports
metadata problems: missing or duplicate track numbers, multi-disc albums without
disc tags, disc folders that disagree with the tags, inconsistent spellings, albums
split over several directories, unreadable files and more (see
[docs/RULES.md](docs/RULES.md)). It prints a human-readable report, or a stable,
versioned JSON report for other tools.

Formats: FLAC, MP3, Ogg Vorbis, Opus, MP4/M4A, WAV, and whatever else
[lofty](https://crates.io/crates/lofty) reads (AIFF, APE, WavPack, Musepack, ...).

## Safety guarantees

- **Read-only.** Liner never writes, renames, deletes or re-tags anything in your
  library. Audio files are only opened through lofty's read path. Write APIs
  (`std::fs::write`, `File::create`, `OpenOptions`, lofty `save_to*` /
  `remove_from*`, ...) are banned crate-wide through `clippy.toml`; the single
  exception is the report file you ask for with `--output`, which must be outside
  the library, must not have an audio extension, and is never written through a
  symlink or hard link (it is written to a new temporary file and renamed into
  place). `tests/readonly.rs` checks that a scanned library is byte-, mtime- and
  permission-identical afterwards.
- **No terminal injection.** Tag values and file names are untrusted: human
  output shows control characters (ESC, newline, ...) as visible `\u{1b}`-style
  escapes, and JSON `message` texts are escaped the same way, so they are always
  single-line.
- **No network access.** Liner makes no network requests at all.
- **No panics on bad files.** Corrupt, truncated or mislabelled files end up in the
  report's `errors[]` list; the scan continues.

## Build and install

Requires Rust **1.89** or newer (MSRV, from lofty 0.25).

```sh
cargo build --release          # binary at target/release/liner
cargo install --path .         # installs `liner` into ~/.cargo/bin
```

## Usage

```
liner scan [PATH] [--format human|json] [--output FILE]
           [--fail-on error|warning|info|never] [--min-severity info|warning|error]
           [--follow-symlinks] [--include-hidden] [--config FILE]
           [--color auto|always|never]
liner rules     # list rule codes with severity, scope and summary
liner schema    # print the JSON Schema of the report
```

Scan a library (the output below is real output for a small synthetic library):

```console
$ liner scan library
Sample Person/Field Notes  "Field Notes"  (1 disc, 3 tracks)
  warning   inconsistent-tag-spelling
            Album title is spelled 2 ways: 'Field Notes', 'field notes '
            - 01.mp3
            - 02.mp3
            - 04.mp3
  warning   track-number-gap
            Missing tracks 3, 5 (track total 5)
            - 01.mp3
            - 02.mp3
            - 04.mp3

The Examples/Night Drive  "Night Drive" by The Examples  (2 discs, 7 tracks)
  warning   missing-disc-number
            3 of 7 tracks in this multi-disc album have no disc number tag
            - CD2/01.flac
            - CD2/02.flac
            - CD2/03.flac

Unreadable files (1)
  corrupt   Sample Person/Field Notes/05.flac
            failed to parse Flac file: failed to fill whole buffer

12 files scanned, 11 audio, 1 unreadable, 2 albums; 0 errors, 3 warnings, 0 info
$ echo $?
1
```

Only show errors (hidden issues are still counted, and still count for
`--fail-on`):

```console
$ liner scan library --min-severity error
Unreadable files (1)
  corrupt   Sample Person/Field Notes/05.flac
            failed to parse Flac file: failed to fill whole buffer

12 files scanned, 11 audio, 1 unreadable, 2 albums; 0 errors, 0 warnings, 0 info (3 below error hidden)
```

More examples:

```sh
liner scan ~/Music --format json > report.json   # JSON on stdout
liner scan ~/Music --format json --output /tmp/report.json
liner scan ~/Music --fail-on warning             # exit 1 on any warning or worse
liner scan ~/Music --fail-on never               # always exit 0 (unless liner fails)
liner scan ~/Music --color always | less -R      # force ANSI colour
liner scan ~/Music --follow-symlinks --include-hidden
liner rules
liner schema > liner-report.v1.schema.json
```

- `--output FILE` writes the report to a file instead of stdout. It is refused
  (exit 2, before scanning) if the file is inside the library, has an audio
  extension, is a directory, symlink or other non-regular file, or its parent
  directory does not exist. "Inside the library" is decided by device and inode
  of the parent directory and each of its ancestors, so a symlinked parent or a
  bind-mount alias of the library is refused too. The report is written to a
  temporary file `.<name>.liner-tmp-<pid>` in the same directory, synced, and then
  renamed over `FILE`; a failed run (including a failed write, e.g. a full disk)
  never changes or truncates an existing report and leaves no temporary file.
  An existing `FILE` is replaced by a new file (mode 0644 minus umask), and is
  refused if it has other hard links.
- With `--follow-symlinks`, a directory symlinked *into* the library from
  elsewhere is scanned but is not recognised as part of the library by the
  `--output` check: do not put the report inside such a directory.
- `--color auto` (default) colours human output only on a terminal with
  `NO_COLOR` unset or empty. JSON is never coloured; an `--output` file only with
  `--color always`.
- Symlinks are not followed and hidden entries (names starting with `.`) are
  skipped unless requested. Symlink loops become `walk` errors.

## Configuration

An optional TOML file supplies defaults. Location: `--config FILE` if given (it
must exist), else `$XDG_CONFIG_HOME/liner/config.toml`, else
`~/.config/liner/config.toml` (a missing default file is ignored).

```toml
library_path = "/path/to/music"   # used when PATH is not given
include_hidden = false
follow_symlinks = false
fail_on = "error"                 # error | warning | info | never
min_severity = "info"             # info | warning | error
```

All keys are optional; unknown keys are an error (typo protection). A relative
`library_path` is resolved against the config file's directory, not the current
directory; `~` is not expanded. Precedence: command line > config file >
built-in default. Boolean options can only be switched on from the command line.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | No issue at or above `--fail-on` and no unreadable file. |
| `1` | Issues at or above `--fail-on` (default `error`), or `errors[]` is non-empty. Unreadable files count as error severity, so only `--fail-on never` ignores them. `--min-severity` does not affect the exit code. |
| `2` | Usage or runtime error: bad arguments, missing/invalid library root, config read/parse error, unsafe or unwritable `--output`. The message goes to stderr as `liner: ...`. |

## How albums are grouped

An album is a **directory** containing audio files (id = path relative to the
library root, `.` for the root itself). Sub-directories named like a disc folder
(`CD1`, `CD 2`, `Disc 1`, `Disk02`, `Disc 1 - Title`, case-insensitive) are merged
into their parent album and serve as a disc number hint. Tags are then
cross-checked inside each album, and the same album found in several directories
is reported as `split-album`. Details, deviations and known false positives:
[ADR 0002](docs/adr/0002-album-grouping-strategy.md) and
[docs/RULES.md](docs/RULES.md).

## JSON report (for consumers)

`liner scan --format json` emits one JSON document, described by the committed
JSON Schema [`schema/liner-report.v1.schema.json`](schema/liner-report.v1.schema.json)
(also printed by `liner schema`). The contract and its versioning rules are in
[ADR 0003](docs/adr/0003-json-report-schema-and-versioning.md). In short:

- Top level: `schema_version`, `tool`, `generated_at`, `root`, `options`,
  `summary`, `albums[]`, `issues[]`, `errors[]`. Every key is always present;
  absent values are `null`. Arrays are sorted, so reports are diffable.
- `schema_version` is semver and independent of the tool version. Check the
  **major**: minor versions only add (fields, issue codes, enum values), a major
  bump renames or removes and changes the schema file name.
- **Ignore unknown fields, and tolerate unknown issue codes** and unknown values
  of `format`, `tag_types` and error `kind`. `severity` and `scope` are closed
  sets within a major version.
- `code`, `severity`, `scope`, `album_id`, `paths` and the `details` keys
  documented in [docs/RULES.md](docs/RULES.md) are contractual; `message` texts
  are not. Do not hard-code severities per code.
- A report with a non-empty `errors[]` is not clean. `options.min_severity` and
  `summary.issues_hidden` tell a filtered report from a clean library.
- Paths are relative to `root`, `/`-separated; names that are not valid UTF-8
  are converted lossily (U+FFFD).

One issue and one error from the library above:

```json
{
  "code": "missing-disc-number",
  "severity": "warning",
  "scope": "album",
  "message": "3 of 7 tracks in this multi-disc album have no disc number tag",
  "album_id": "The Examples/Night Drive",
  "paths": [
    "The Examples/Night Drive/CD2/01.flac",
    "The Examples/Night Drive/CD2/02.flac",
    "The Examples/Night Drive/CD2/03.flac"
  ],
  "details": {
    "count": 3
  }
}
```

```json
{
  "path": "Sample Person/Field Notes/05.flac",
  "kind": "corrupt",
  "message": "failed to parse Flac file: failed to fill whole buffer"
}
```

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings   # also enforces the write ban
cargo test
cargo test --release --test perf -- --ignored --nocapture   # 2 000 files < 10 s
```

- Design docs: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md),
  [docs/adr/](docs/adr/), [docs/RULES.md](docs/RULES.md).
- Tests build synthetic libraries in temp directories with `tests/common/mod.rs`
  from tiny committed templates (`tests/fixtures/templates/`) and corrupt
  fixtures (`tests/fixtures/corrupt/`); they never touch a real library and
  override `XDG_CONFIG_HOME`/`HOME` so your config is never read.
  `scripts/gen-fixture-templates.sh` regenerates the templates (needs ffmpeg;
  dev-time only).
- After a deliberate report type change (bump `report::SCHEMA_VERSION` per
  ADR 0003), regenerate the schema and the golden report:
  `LINER_UPDATE_SCHEMA=1 cargo test --test schema` and
  `LINER_UPDATE_GOLDEN=1 cargo test --test report`.
- The write ban in `clippy.toml` applies to tests too; code that must write
  (fixture builders, regeneration) opts out locally with
  `#[allow(clippy::disallowed_methods)]` and only writes into temp directories or
  the repository's own files.
