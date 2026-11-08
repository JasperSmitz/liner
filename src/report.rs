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

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::group::{AlbumUnit, GroupedTrack};
use crate::model::{
    AudioFormat, FileError, FileErrorKind, Issue, Severity, TagKind, TrackInfo, all_fields_required,
};
use crate::scan::{ScanOptions, ScanResult};

/// Version of the JSON report schema (semver, independent of the crate version).
pub const SCHEMA_VERSION: &str = "1.0.0";

/// Top-level report document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(
    title = "Liner report",
    description = "Metadata lint report produced by `liner scan --format json`. Every key is always present; optional values are null. Consumers must check the major `schema_version`, ignore unknown fields, and tolerate unknown issue `code`s and unknown `format`, `tag_types` and error `kind` values. The keys of `issues[].details` for each code are documented (and contractual) in docs/RULES.md.",
    transform = all_fields_required
)]
pub struct Report {
    /// Semver of this schema, e.g. "1.0.0". Consumers should check the major version.
    pub schema_version: String,
    pub tool: ToolInfo,
    /// RFC 3339 UTC timestamp, e.g. "2026-11-08T10:00:00Z".
    #[schemars(extend("format" = "date-time"))]
    pub generated_at: String,
    /// Absolute path of the scanned library root (lossy UTF-8).
    pub root: String,
    /// Scan and filter options that shaped this report.
    pub options: ReportOptions,
    pub summary: Summary,
    pub albums: Vec<AlbumEntry>,
    pub issues: Vec<Issue>,
    pub errors: Vec<FileErrorEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(transform = all_fields_required)]
pub struct ToolInfo {
    /// Always "liner".
    pub name: String,
    /// Crate version of the producing binary.
    pub version: String,
}

/// Scan and filter options in effect, so consumers can tell a filtered report from
/// a clean library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(transform = all_fields_required)]
pub struct ReportOptions {
    /// Issues below this severity were removed from `issues` (`--min-severity`);
    /// their number is `summary.issues_hidden`.
    pub min_severity: Severity,
    /// Symbolic links were followed while walking.
    pub follow_symlinks: bool,
    /// Hidden files and directories (names starting with '.') were scanned.
    pub include_hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(transform = all_fields_required)]
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
    /// Issues removed by `--min-severity` (`options.min_severity`); not included
    /// in `issues_by_severity` or `issues_by_code`.
    pub issues_hidden: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(transform = all_fields_required)]
pub struct SeverityCounts {
    pub error: u64,
    pub warning: u64,
    pub info: u64,
}

/// One album unit (a directory, with disc sub-folders merged).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(transform = all_fields_required)]
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
#[schemars(transform = all_fields_required)]
pub struct DiscEntry {
    /// Effective disc number; null when it cannot be determined (multi-disc album
    /// with untagged files outside disc folders).
    pub number: Option<u32>,
    /// Root-relative disc folder (e.g. "Artist/Album/CD1"), when the tracks live in one.
    pub folder: Option<String>,
    pub tracks: Vec<TrackEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(transform = all_fields_required)]
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
#[schemars(transform = all_fields_required)]
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
    /// Options the scan ran with (recorded in `options`).
    pub scan_options: ScanOptions,
    pub scan: &'a ScanResult,
    /// Read failures from the metadata stage (scan errors come from `scan.errors`).
    pub read_errors: &'a [FileError],
    pub albums: &'a [AlbumUnit],
    /// Issues from `rules::run_all` (any order; `build_report` sorts).
    pub issues: &'a [Issue],
}

/// Assemble the report: convert, sort, compute the summary.
///
/// Pure: the output depends only on the input values, not on their order.
pub fn build_report(input: ReportInput<'_>) -> Report {
    let mut albums: Vec<AlbumEntry> = input.albums.iter().map(album_entry).collect();
    albums.sort_by(|a, b| a.id.cmp(&b.id));

    let mut issues = input.issues.to_vec();
    issues.sort_by(compare_issues);

    let mut errors: Vec<FileErrorEntry> = input
        .scan
        .errors
        .iter()
        .chain(input.read_errors)
        .map(|e| FileErrorEntry {
            path: e.rel_path.clone(),
            kind: e.kind,
            message: e.message.clone(),
        })
        .collect();
    errors.sort_by(|a, b| (&a.path, a.kind, &a.message).cmp(&(&b.path, b.kind, &b.message)));
    errors.dedup();

    let mut report = Report {
        schema_version: SCHEMA_VERSION.to_owned(),
        tool: ToolInfo {
            name: env!("CARGO_PKG_NAME").to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        },
        generated_at: input.generated_at.to_string(),
        root: input.root.to_string_lossy().into_owned(),
        options: ReportOptions {
            min_severity: Severity::Info,
            follow_symlinks: input.scan_options.follow_symlinks,
            include_hidden: input.scan_options.include_hidden,
        },
        summary: Summary {
            files_scanned: input.scan.files_scanned(),
            audio_files: input.scan.audio_files.len() as u64,
            unreadable_files: 0,
            albums: 0,
            issues_by_severity: SeverityCounts::default(),
            issues_by_code: BTreeMap::new(),
            issues_hidden: 0,
        },
        albums,
        issues,
        errors,
    };
    report.recompute_summary();
    report
}

/// Total order on issues: [`Issue::sort_key`], then `details` (as JSON text) so
/// that issues equal under the sort key still order independently of input order.
fn compare_issues(a: &Issue, b: &Issue) -> Ordering {
    a.sort_key().cmp(&b.sort_key()).then_with(|| {
        let details = |i: &Issue| serde_json::to_string(&i.details).unwrap_or_default();
        details(a).cmp(&details(b))
    })
}

fn album_entry(unit: &AlbumUnit) -> AlbumEntry {
    // Group by effective disc; `None` (unknown) sorts last.
    let mut by_disc: BTreeMap<(bool, u32), Vec<&GroupedTrack>> = BTreeMap::new();
    for track in &unit.tracks {
        let key = match track.effective_disc {
            Some(n) => (false, n),
            None => (true, 0),
        };
        by_disc.entry(key).or_default().push(track);
    }
    let discs = by_disc
        .into_iter()
        .map(|((_, _), tracks)| disc_entry(&tracks))
        .collect();
    AlbumEntry {
        id: unit.id.clone(),
        path: unit.id.clone(),
        title: unit.title.clone(),
        album_artist: unit.album_artist.clone(),
        disc_count: unit.disc_count,
        track_count: u32::try_from(unit.tracks.len()).unwrap_or(u32::MAX),
        discs,
    }
}

/// `tracks` is non-empty and shares one effective disc.
fn disc_entry(tracks: &[&GroupedTrack]) -> DiscEntry {
    let number = tracks.first().and_then(|t| t.effective_disc);
    // The folder is reported only when every track of the disc lives in the same one.
    let first_folder = tracks.first().and_then(|t| t.disc_folder.as_ref());
    let folder = first_folder
        .filter(|f| tracks.iter().all(|t| t.disc_folder.as_ref() == Some(*f)))
        .cloned();
    let mut entries: Vec<TrackEntry> = tracks.iter().map(|t| track_entry(&t.info)).collect();
    entries.sort_by(|a, b| {
        (a.track_no.is_none(), a.track_no, &a.path).cmp(&(
            b.track_no.is_none(),
            b.track_no,
            &b.path,
        ))
    });
    DiscEntry {
        number,
        folder,
        tracks: entries,
    }
}

fn track_entry(t: &TrackInfo) -> TrackEntry {
    TrackEntry {
        path: t.rel_path.clone(),
        format: t.format,
        title: t.title.clone(),
        artist: t.artist.clone(),
        album: t.album.clone(),
        album_artist: t.album_artist.clone(),
        track_no: t.track_no,
        track_total: t.track_total,
        disc_no: t.disc_no,
        disc_total: t.disc_total,
        year: t.year,
        genre: t.genre.clone(),
        compilation: t.compilation,
        duration_ms: t.duration_ms,
        tag_types: t.tag_types.clone(),
    }
}

impl Report {
    /// Highest severity among `issues`, if any.
    pub fn max_severity(&self) -> Option<Severity> {
        self.issues.iter().map(|i| i.severity).max()
    }

    /// Drop issues below `min`, record the filter in `options.min_severity` and
    /// `summary.issues_hidden`, and recompute the summary.
    pub fn retain_min_severity(&mut self, min: Severity) {
        let before = self.issues.len();
        self.issues.retain(|i| i.severity >= min);
        self.summary.issues_hidden += (before - self.issues.len()) as u64;
        self.options.min_severity = self.options.min_severity.max(min);
        self.recompute_summary();
    }

    /// Recompute the derived counters in `summary` from the lists
    /// (`files_scanned`, `audio_files` and `issues_hidden` are left untouched).
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
