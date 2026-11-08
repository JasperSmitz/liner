//! Stage 4: lint rules over album units (catalogue: `docs/RULES.md`).
//!
//! Owner: developer B (M2). `codes`, `RuleInfo`, `CATALOG` and the `run_all`
//! signature are frozen; rule implementations are free (plain functions per rule,
//! split into submodules `rules/*.rs` if desired).
//!
//! Contract:
//! - Each code has exactly one fixed severity and scope (from [`CATALOG`]); create
//!   issues only via [`RuleInfo::issue`] so they cannot diverge.
//! - Rules are pure functions of the album units: no I/O.
//! - `run_all` returns issues sorted by [`Issue::sort_key`]; `paths` inside each issue
//!   are sorted and deduplicated.
//! - Adding a code is a minor schema-compatible change (consumers must tolerate unknown
//!   codes); renaming/removing a code or changing its meaning is breaking (ADR 0003).

use std::collections::BTreeMap;

use crate::group::AlbumUnit;
use crate::model::{Issue, Scope, Severity};

mod album_tags;
mod file;
mod library;
mod numbering;
mod util;

/// Stable issue codes. Keep in sync with `docs/RULES.md` and [`CATALOG`].
pub mod codes {
    // File scope
    pub const MISSING_ALBUM: &str = "missing-album";
    pub const MISSING_ARTIST: &str = "missing-artist";
    pub const MISSING_TITLE: &str = "missing-title";
    pub const MISSING_TRACK_NUMBER: &str = "missing-track-number";
    pub const TRACK_NUMBER_EXCEEDS_TOTAL: &str = "track-number-exceeds-total";
    pub const DISC_NUMBER_EXCEEDS_TOTAL: &str = "disc-number-exceeds-total";
    // Album scope: positions
    pub const DUPLICATE_TRACK_POSITION: &str = "duplicate-track-position";
    pub const LIKELY_MISSING_DISC_NUMBERS: &str = "likely-missing-disc-numbers";
    pub const MISSING_DISC_NUMBER: &str = "missing-disc-number";
    pub const DISC_FOLDER_MISMATCH: &str = "disc-folder-mismatch";
    pub const TRACK_NUMBER_GAP: &str = "track-number-gap";
    pub const DISC_NUMBER_GAP: &str = "disc-number-gap";
    pub const INCONSISTENT_TRACK_TOTAL: &str = "inconsistent-track-total";
    pub const INCONSISTENT_DISC_TOTAL: &str = "inconsistent-disc-total";
    // Album scope: album-level tags
    pub const MIXED_ALBUM_DIRECTORY: &str = "mixed-album-directory";
    pub const INCONSISTENT_TAG_SPELLING: &str = "inconsistent-tag-spelling";
    pub const PARTIAL_ALBUM_ARTIST: &str = "partial-album-artist";
    pub const MISSING_ALBUM_ARTIST: &str = "missing-album-artist";
    pub const INCONSISTENT_YEAR: &str = "inconsistent-year";
    // Library scope
    pub const SPLIT_ALBUM: &str = "split-album";
}

/// Static description of a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleInfo {
    pub code: &'static str,
    pub severity: Severity,
    pub scope: Scope,
    /// One-line description for `liner rules`.
    pub summary: &'static str,
}

impl RuleInfo {
    /// Build an issue for this rule. Sorts and deduplicates `paths`.
    pub fn issue(
        &self,
        message: impl Into<String>,
        album_id: Option<&str>,
        mut paths: Vec<String>,
        details: BTreeMap<String, serde_json::Value>,
    ) -> Issue {
        paths.sort();
        paths.dedup();
        Issue {
            code: self.code.to_owned(),
            severity: self.severity,
            scope: self.scope,
            message: message.into(),
            album_id: album_id.map(str::to_owned),
            paths,
            details,
        }
    }
}

const fn rule(
    code: &'static str,
    severity: Severity,
    scope: Scope,
    summary: &'static str,
) -> RuleInfo {
    RuleInfo {
        code,
        severity,
        scope,
        summary,
    }
}

use Scope::{Album, File, Library};
use Severity::{Error, Info, Warning};

/// All rules, in documentation order.
pub const CATALOG: &[RuleInfo] = &[
    rule(
        codes::MISSING_ALBUM,
        Warning,
        File,
        "Track has no album tag",
    ),
    rule(
        codes::MISSING_ARTIST,
        Warning,
        File,
        "Track has neither an artist nor an album artist tag",
    ),
    rule(codes::MISSING_TITLE, Info, File, "Track has no title tag"),
    rule(
        codes::MISSING_TRACK_NUMBER,
        Warning,
        File,
        "Track has no track number",
    ),
    rule(
        codes::TRACK_NUMBER_EXCEEDS_TOTAL,
        Warning,
        File,
        "Track number is greater than the track total",
    ),
    rule(
        codes::DISC_NUMBER_EXCEEDS_TOTAL,
        Warning,
        File,
        "Disc number is greater than the disc total",
    ),
    rule(
        codes::DUPLICATE_TRACK_POSITION,
        Error,
        Album,
        "Several files share the same (disc, track) position",
    ),
    rule(
        codes::LIKELY_MISSING_DISC_NUMBERS,
        Error,
        Album,
        "Track numbers repeat but no file has a disc number: probably a multi-disc album without disc tags",
    ),
    rule(
        codes::MISSING_DISC_NUMBER,
        Warning,
        Album,
        "Multi-disc album has tracks without a disc number tag",
    ),
    rule(
        codes::DISC_FOLDER_MISMATCH,
        Error,
        Album,
        "Disc number tag disagrees with the disc folder the file is in",
    ),
    rule(
        codes::TRACK_NUMBER_GAP,
        Warning,
        Album,
        "Track numbers on a disc have gaps",
    ),
    rule(
        codes::DISC_NUMBER_GAP,
        Warning,
        Album,
        "Disc numbers have gaps",
    ),
    rule(
        codes::INCONSISTENT_TRACK_TOTAL,
        Warning,
        Album,
        "Tracks on the same disc disagree on the track total",
    ),
    rule(
        codes::INCONSISTENT_DISC_TOTAL,
        Warning,
        Album,
        "Tracks of the album disagree on the disc total",
    ),
    rule(
        codes::MIXED_ALBUM_DIRECTORY,
        Warning,
        Album,
        "Directory holds tracks tagged with different albums or album artists",
    ),
    rule(
        codes::INCONSISTENT_TAG_SPELLING,
        Warning,
        Album,
        "Album or album artist differs only in case or whitespace",
    ),
    rule(
        codes::PARTIAL_ALBUM_ARTIST,
        Warning,
        Album,
        "Some tracks have an album artist tag and others do not",
    ),
    rule(
        codes::MISSING_ALBUM_ARTIST,
        Warning,
        Album,
        "Tracks have different artists but no album artist tag",
    ),
    rule(
        codes::INCONSISTENT_YEAR,
        Info,
        Album,
        "Tracks of the album have different years",
    ),
    rule(
        codes::SPLIT_ALBUM,
        Warning,
        Library,
        "The same album appears in several directories",
    ),
];

/// Look up a rule by code.
pub fn rule_info(code: &str) -> Option<&'static RuleInfo> {
    CATALOG.iter().find(|r| r.code == code)
}

/// Run every rule over all album units. Output sorted by [`Issue::sort_key`].
pub fn run_all(albums: &[AlbumUnit]) -> Vec<Issue> {
    let mut issues = Vec::new();
    for unit in albums {
        file::check(unit, &mut issues);
        numbering::check(unit, &mut issues);
        album_tags::check(unit, &mut issues);
    }
    library::check(albums, &mut issues);
    issues.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_codes_are_unique_kebab_case() {
        let mut seen = std::collections::BTreeSet::new();
        for r in CATALOG {
            assert!(seen.insert(r.code), "duplicate code {}", r.code);
            assert!(
                !r.code.is_empty()
                    && !r.code.starts_with('-')
                    && !r.code.ends_with('-')
                    && !r.code.contains("--")
                    && r.code.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "code {} is not kebab-case",
                r.code
            );
        }
    }

    #[test]
    fn issue_helper_uses_catalog_severity_and_sorts_paths() {
        let info = rule_info(codes::DUPLICATE_TRACK_POSITION).unwrap();
        let issue = info.issue(
            "dup",
            Some("A"),
            vec!["A/02.flac".into(), "A/01.flac".into(), "A/01.flac".into()],
            BTreeMap::new(),
        );
        assert_eq!(issue.severity, Severity::Error);
        assert_eq!(issue.scope, Scope::Album);
        assert_eq!(issue.paths, vec!["A/01.flac", "A/02.flac"]);
    }
}
