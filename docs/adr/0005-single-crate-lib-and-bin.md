# ADR 0005: Single crate with library and binary targets

Status: accepted (M0)

## Context
Liner is one CLI. We want unit/integration tests against internal stages, parallel
development on separate modules, and no compile-time coupling with consumers.

## Decision
One package `liner` (edition 2024) with `src/lib.rs` (all logic, modules per
stage) and `src/main.rs` (parse, run, exit code). No workspace.

- The library is an implementation detail, not a published API (`publish = false`).
  External integration happens only through the JSON report (ADR 0003).
- `anyhow` is used only in `main.rs`; the library uses `thiserror` types.
- Module boundaries (one owner each) give the parallelism a workspace would, without
  the overhead of multiple manifests and versions.

## Consequences
- Integration tests in `tests/` can call `liner::analyze` directly as well as the
  binary via `assert_cmd`.
- If a reusable reader library is ever needed, `metadata` + `model` can be split
  into a crate later; nothing in the design prevents it.
