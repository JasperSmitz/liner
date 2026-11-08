//! Stage 5: the serialisable report — Liner's public contract (ADR 0003).
//!
//! Owner: developer C (M3). The *types* in this file are frozen by the architect and
//! mirrored in `schema/liner-report.v1.schema.json`; `tests/schema.rs` fails if they
//! drift. Changing them requires a schema version bump per ADR 0003.
//!
//! Determinism: every list is sorted (albums by id, discs by number with unknown
//! last, tracks by (track_no with None last, path), issues by `Issue::sort_key`,
//! errors by path); maps are `BTreeMap`. Given the same library and the same
//! `generated_at`, the JSON output is byte-identical.

use std::collections::BTreeMap;
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::group::AlbumUnit;
use crate::model::{AudioFormat, FileError, FileErrorKind, Issue, Severity, TagKind};
use crate::scan::ScanResult;

/// Version of the JSON report schema (semver, independent of the crate version).
pub const SCHEMA_VERSION: &str = "1.0.0";

/// Top-level report document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(
    title = "Liner report",
    description = "Metadata lint report produced by `liner scan --format json`. Consumers must ignore unknown fields and tolerate unknown enum values in `code`."
)]
pub struct Report {
    /// Semver of this schema, e.g. "1.0.0". Consumers should check the major version.
    pub schema_version: String,
    pub tool: ToolInfo,
    /// RFC 3339 UTC timestamp, e.g. "2026-11-08T10:00:00Z".
    pub generated_at: String,
    /// Absolute path of the scanned library root (lossy UTF-8).
    pub root: String,
    pub summary: Summary,
    pub albums: Vec<AlbumEntry>,
    pub issues: Vec<Issue>,
    pub errors: Vec<FileErrorEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ToolInfo {
    /// Always "liner".
    pub name: String,
    /// Crate version of the producing binary.
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Summary {
    /// Regular files seen during the walk (audio + non-audio).
    pub files_scanned: u64,
    /// Files with an audio extension (readable or not).
    pub audio_files: u64,
    /// Number of entries in `errors`.
    pub unreadable_files: u64,
    /// Number of entries in `albums`.
    pub albums: u64,
    /// Counts of the issues in this report (after `--min-severity` filtering).
    pub issues_by_severity: SeverityCounts,
    /// Issue count per code; only codes with at least one issue appear.
    pub issues_by_code: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SeverityCounts {
    pub error: u64,
    pub warning: u64,
    pub info: u64,
}

/// One album unit (a directory, with disc sub-folders merged).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AlbumEntry {
    /// Stable id: root-relative directory path, "/"-separated; "." for the root.
    pub id: String,
    /// Same as `id` in schema 1.x (kept separate so ids may change form later).
    pub path: String,
    /// Most common album tag value, original spelling.
    pub title: Option<String>,
    /// Most common album artist tag value, original spelling.
    pub album_artist: Option<String>,
    pub disc_count: u32,
    pub track_count: u32,
    pub discs: Vec<DiscEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DiscEntry {
    /// Effective disc number; null when it cannot be determined (multi-disc album
    /// with untagged files outside disc folders).
    pub number: Option<u32>,
    /// Root-relative disc folder (e.g. "Artist/Album/CD1"), when the tracks live in one.
    pub folder: Option<String>,
    pub tracks: Vec<TrackEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TrackEntry {
    /// Root-relative file path, "/"-separated.
    pub path: String,
    pub format: AudioFormat,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_no: Option<u32>,
    pub track_total: Option<u32>,
    /// Disc number as tagged (not the effective/folder disc).
    pub disc_no: Option<u32>,
    pub disc_total: Option<u32>,
    pub year: Option<u32>,
    pub genre: Option<String>,
    pub compilation: bool,
    pub duration_ms: Option<u64>,
    pub tag_types: Vec<TagKind>,
}

/// A file that could not be walked or read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FileErrorEntry {
    /// Root-relative path, "/"-separated.
    pub path: String,
    pub kind: FileErrorKind,
    /// Human-readable reason; wording is not part of the contract.
    pub message: String,
}

/// Everything needed to assemble a report.
#[derive(Debug, Clone, Copy)]
pub struct ReportInput<'a> {
    /// Absolute library root.
    pub root: &'a Path,
    pub generated_at: jiff::Timestamp,
    pub scan: &'a ScanResult,
    /// Read failures from the metadata stage (scan errors come from `scan.errors`).
    pub read_errors: &'a [FileError],
    pub albums: &'a [AlbumUnit],
    /// Issues from `rules::run_all` (any order; `build_report` sorts).
    pub issues: &'a [Issue],
}

/// Assemble the report: convert, sort, compute the summary.
pub fn build_report(input: ReportInput<'_>) -> Report {
    // M3 placeholder: conversion of albums/errors and sorting to be implemented by
    // developer C. Header fields are final.
    let mut report = Report {
        schema_version: SCHEMA_VERSION.to_owned(),
        tool: ToolInfo {
            name: env!("CARGO_PKG_NAME").to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        },
        generated_at: input.generated_at.to_string(),
        root: input.root.to_string_lossy().into_owned(),
        summary: Summary {
            files_scanned: input.scan.files_scanned(),
            audio_files: input.scan.audio_files.len() as u64,
            unreadable_files: 0,
            albums: 0,
            issues_by_severity: SeverityCounts::default(),
            issues_by_code: BTreeMap::new(),
        },
        albums: Vec::new(),
        issues: Vec::new(),
        errors: Vec::new(),
    };
    report.recompute_summary();
    report
}

impl Report {
    /// Highest severity among `issues`, if any.
    pub fn max_severity(&self) -> Option<Severity> {
        self.issues.iter().map(|i| i.severity).max()
    }

    /// Drop issues below `min` and recompute the summary.
    pub fn retain_min_severity(&mut self, min: Severity) {
        self.issues.retain(|i| i.severity >= min);
        self.recompute_summary();
    }

    /// Recompute the derived counters in `summary` from the lists
    /// (`files_scanned` and `audio_files` are left untouched).
    pub fn recompute_summary(&mut self) {
        let mut by_severity = SeverityCounts::default();
        let mut by_code = BTreeMap::new();
        for issue in &self.issues {
            match issue.severity {
                Severity::Error => by_severity.error += 1,
                Severity::Warning => by_severity.warning += 1,
                Severity::Info => by_severity.info += 1,
            }
            *by_code.entry(issue.code.clone()).or_insert(0) += 1;
        }
        self.summary.unreadable_files = self.errors.len() as u64;
        self.summary.albums = self.albums.len() as u64;
        self.summary.issues_by_severity = by_severity;
        self.summary.issues_by_code = by_code;
    }
}

/// JSON Schema (draft 2020-12) of [`Report`], as committed in
/// `schema/liner-report.v1.schema.json`.
pub fn json_schema() -> serde_json::Value {
    let schema = schemars::schema_for!(Report);
    serde_json::to_value(schema).expect("schema serialises to JSON")
}
