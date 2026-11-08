# ADR 0003: JSON report schema and versioning

Status: accepted (M0)

## Context
Gargoyle and other tools consume Liner's output without linking Liner code. They
need a stable, documented, versioned format.

## Decision
- The report types in `src/report.rs` (plus `model::{Issue, Severity, Scope,
  AudioFormat, TagKind, FileErrorKind}`) **are** the contract. The JSON Schema
  (draft 2020-12) is generated with schemars and committed at
  `schema/liner-report.v1.schema.json`; `tests/schema.rs` fails when the generated
  schema differs from the committed file. `liner schema` prints it.
- `schema_version` is a semver string (`report::SCHEMA_VERSION`, initially
  `1.0.0`), **independent of the crate version** (`tool.version`).
  - **Patch**: description/doc changes only.
  - **Minor (additive)**: new optional fields, new objects, new issue codes, new
    enum values in `details`, new `format`/`tag_types`/`kind` values.
  - **Major (breaking)**: removing/renaming a field, changing a type or meaning,
    renaming/removing an issue code, changing id semantics. The schema file name
    carries the major (`liner-report.v2.schema.json`).
- **Consumer rules**: check the major version; ignore unknown fields; tolerate
  unknown issue codes and unknown values of `format`, `tag_types` and error
  `kind`. The `severity` and `scope` value sets are closed within a major version. `message` texts are for humans and are not contractual; `code`,
  `severity`, `scope`, `album_id`, `paths` and documented `details` keys are.
- Top level: `schema_version`, `tool {name, version}`, `generated_at` (RFC 3339
  UTC, schema `format: date-time`), `root` (absolute), `options {min_severity,
  follow_symlinks, include_hidden}`, `summary`, `albums[]`, `issues[]`,
  `errors[]`. Optional values are always present as `null` (never omitted), so
  consumers can rely on key presence. The schema states this: **every property of
  every object is in `required`** (a schemars transform,
  `model::all_fields_required`, enforces it for all report types).
- `options` records how the report was produced, so a consumer can tell a report
  filtered by `--min-severity` from a clean library: `options.min_severity` is the
  filter applied to `issues[]`, and `summary.issues_hidden` counts the issues it
  removed (they are not in `issues_by_severity` / `issues_by_code`).
- **Open enums in the schema.** `format` (`AudioFormat`), `tag_types` (`TagKind`)
  and error `kind` (`FileErrorKind`) are encoded as `anyOf: [<known values>,
  {type: string}]` (transform `model::open_enum`), so a 1.x report with a newer
  value still validates against the v1 schema; in Rust they stay closed enums.
  `severity` and `scope` are closed `enum`s. `issues[].details` is an open object
  in the schema; its keys per code are documented (and contractual) in
  `docs/RULES.md`, as the schema description says.
- **Determinism**: all arrays sorted (see `report.rs` docs), maps are
  `BTreeMap`, pretty-printed with a trailing newline. Same input + same
  `generated_at` ⇒ identical bytes, so reports are diffable.
- `issues[].message` is single-line: control characters from tag values are
  escaped as `\u{XX}` text (e.g. `\u{1b}`, `\u{a}`). `paths`, `root` and the
  values in `details` are exact (JSON-escaped), never altered.
- `errors[]` lists files that could not be walked or read. Two distinct files
  whose lossy paths are equal are separate entries (deduplication uses the real
  path), so `summary.unreadable_files` counts files, not display names. They carry no issue
  code or severity in the JSON, but they are a library-health signal: the CLI
  treats them as error severity for `--fail-on` (exit 1 unless `--fail-on never`).
  Consumers should likewise not treat a report with a non-empty `errors[]` as
  clean.
- Severity of each code is fixed per schema major; changing it is a minor change
  (consumers must not hard-code severities), documented in the changelog.

## Amendments
- M3 review (pre-release, so fixed in place with `SCHEMA_VERSION` still `1.0.0`):
  all keys `required`, open enums as above, `generated_at` format, `options` and
  `summary.issues_hidden` added.
- Final review (pre-release, `SCHEMA_VERSION` still `1.0.0`): `message` is
  guaranteed single-line (control characters escaped), and `errors[]` keeps
  entries whose lossy paths collide.

## Consequences
- Every report type change is a visible diff in `schema/` and needs a deliberate
  version decision in review.
- Unknown-field tolerance lets us add data (e.g. scan options) in 1.x without
  coordinating with consumers.
