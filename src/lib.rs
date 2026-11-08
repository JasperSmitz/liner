//! Liner: a strictly read-only music library metadata linter.
//!
//! Pipeline (see docs/ARCHITECTURE.md):
//! `scan` -> `metadata` -> `group` -> `rules` -> `report` -> `render`.
//!
//! The library crate exists for testability and internal structure only; external
//! programs integrate through the versioned JSON report, never by linking this crate.

pub mod cli;
pub mod config;
pub mod error;
pub mod group;
pub mod metadata;
pub mod model;
pub mod render;
pub mod report;
pub mod rules;
pub mod scan;

use std::path::Path;

use crate::error::{LinerError, Result};
use crate::report::{Report, ReportInput};
use crate::scan::ScanOptions;

/// Run the full analysis pipeline on `root` and return the (unfiltered) report.
///
/// Read-only: performs directory walks and file reads only.
pub fn analyze(
    root: &Path,
    options: &ScanOptions,
    generated_at: jiff::Timestamp,
) -> Result<Report> {
    let root = std::path::absolute(root).map_err(|source| LinerError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    let scan = scan::scan(&root, options)?;
    let metadata = metadata::read_all(&scan.audio_files);
    let albums = group::group_tracks(metadata.tracks);
    let issues = rules::run_all(&albums);
    Ok(report::build_report(ReportInput {
        root: &root,
        generated_at,
        scan: &scan,
        read_errors: &metadata.errors,
        albums: &albums,
        issues: &issues,
    }))
}
