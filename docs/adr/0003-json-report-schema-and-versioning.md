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
  UTC), `root` (absolute), `summary`, `albums[]`, `issues[]`, `errors[]`. Optional
  values are always present as `null` (never omitted), so consumers can rely on
  key presence.
- **Determinism**: all arrays sorted (see `report.rs` docs), maps are
  `BTreeMap`, pretty-printed with a trailing newline. Same input + same
  `generated_at` ⇒ identical bytes, so reports are diffable.
- `errors[]` lists files that could not be walked or read. They carry no issue
  code or severity in the JSON, but they are a library-health signal: the CLI
  treats them as error severity for `--fail-on` (exit 1 unless `--fail-on never`).
  Consumers should likewise not treat a report with a non-empty `errors[]` as
  clean.
- Severity of each code is fixed per schema major; changing it is a minor change
  (consumers must not hard-code severities), documented in the changelog.

## Consequences
- Every report type change is a visible diff in `schema/` and needs a deliberate
  version decision in review.
- Unknown-field tolerance lets us add data (e.g. scan options) in 1.x without
  coordinating with consumers.
