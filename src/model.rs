//! Shared domain types used by every pipeline stage.
//!
//! Owner: architect (frozen interface). Changes here affect all modules; coordinate
//! through the lead. Types that also appear in the JSON report (`Severity`, `Scope`,
//! `Issue`, `AudioFormat`, `TagKind`, `FileErrorKind`) are part of the public report
//! contract: any change to them is a schema change (see ADR 0003) and is caught by
//! the committed-schema test in `tests/schema.rs`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Audio container/codec as detected by the tag reader (content-based, not by extension).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
#[schemars(transform = open_enum)]
pub enum AudioFormat {
    Flac,
    Mp3,
    OggVorbis,
    Opus,
    Speex,
    Mp4,
    Aac,
    Wav,
    Aiff,
    Ape,
    WavPack,
    Musepack,
}

/// Kind of tag block present in a file. Mirrors lofty's `TagType` without leaking it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
#[schemars(transform = open_enum)]
pub enum TagKind {
    Id3v1,
    Id3v2,
    Ape,
    VorbisComments,
    Mp4Ilst,
    RiffInfo,
    AiffText,
}

/// Metadata of one successfully read audio file.
///
/// Text fields are `None` when absent or whitespace-only; original text is otherwise
/// preserved verbatim (no trimming, no case changes). Numeric fields are `None` when
/// absent, unparsable, or zero (a stored `0` is treated as "not set").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackInfo {
    /// Path used to open the file (root joined with the walked relative path).
    pub path: PathBuf,
    /// Root-relative path, `/`-separated, lossy UTF-8. See [`rel_path_string`].
    pub rel_path: String,
    pub format: AudioFormat,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_no: Option<u32>,
    pub track_total: Option<u32>,
    pub disc_no: Option<u32>,
    pub disc_total: Option<u32>,
    /// Year extracted from the best available date field.
    pub year: Option<u32>,
    /// Raw date text as stored (e.g. `2003`, `2003-05-12`).
    pub date: Option<String>,
    pub genre: Option<String>,
    /// `true` only when a compilation flag is present and set.
    pub compilation: bool,
    pub duration_ms: Option<u64>,
    /// Tag blocks present in the file, sorted, deduplicated.
    pub tag_types: Vec<TagKind>,
}

impl TrackInfo {
    /// A track with no tags set. Convenient base for struct-update syntax in tests:
    /// `TrackInfo { album: Some("X".into()), ..TrackInfo::new(p, "a/01.flac", AudioFormat::Flac) }`.
    pub fn new(path: impl Into<PathBuf>, rel_path: impl Into<String>, format: AudioFormat) -> Self {
        Self {
            path: path.into(),
            rel_path: rel_path.into(),
            format,
            title: None,
            artist: None,
            album: None,
            album_artist: None,
            track_no: None,
            track_total: None,
            disc_no: None,
            disc_total: None,
            year: None,
            date: None,
            genre: None,
            compilation: false,
            duration_ms: None,
            tag_types: Vec::new(),
        }
    }
}

/// Why a file could not be scanned or read.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
#[schemars(transform = open_enum)]
pub enum FileErrorKind {
    /// Directory walk failed for this entry (permission denied, symlink loop, ...).
    Walk,
    /// I/O error while opening/reading the file.
    Io,
    /// Format not recognised or not supported by the tag reader.
    Unsupported,
    /// Recognised format but the file is malformed, truncated or empty.
    Corrupt,
}

/// A per-file failure. Never fatal: collected and reported in `errors[]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileError {
    pub path: PathBuf,
    /// Root-relative, `/`-separated. Empty only if the root itself failed.
    pub rel_path: String,
    pub kind: FileErrorKind,
    /// Human-readable reason (from the underlying error's `Display`).
    pub message: String,
}

/// Issue severity. Ordering: `Info < Warning < Error`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
    clap::ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

/// What an issue is about.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    File,
    Album,
    Library,
}

/// One finding produced by a lint rule. Serialised verbatim into `issues[]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(transform = all_fields_required)]
pub struct Issue {
    /// Stable kebab-case rule code, see `docs/RULES.md` and `rules::codes`.
    pub code: String,
    pub severity: Severity,
    pub scope: Scope,
    /// Human-readable, single line. Not part of the stable contract (wording may change).
    pub message: String,
    /// Album id (root-relative directory) for file/album issues; null for library scope.
    pub album_id: Option<String>,
    /// Affected files, root-relative, sorted.
    pub paths: Vec<String>,
    /// Rule-specific structured data; keys per rule are documented in `docs/RULES.md`.
    pub details: BTreeMap<String, serde_json::Value>,
}

impl Issue {
    /// Sort key giving the deterministic order of `issues[]`:
    /// severity (error first), code, album_id, paths, message.
    pub fn sort_key(&self) -> impl Ord + '_ {
        (
            std::cmp::Reverse(self.severity),
            self.code.as_str(),
            self.album_id.as_deref(),
            self.paths.as_slice(),
            self.message.as_str(),
        )
    }
}

/// Schema transform for report structs: every property is `required`.
///
/// The report never omits a key (optional values are serialised as `null`, see
/// ADR 0003), but schemars leaves `Option` fields out of `required` by default.
pub(crate) fn all_fields_required(schema: &mut schemars::Schema) {
    let Some(props) = schema.get("properties").and_then(|p| p.as_object()) else {
        return;
    };
    let mut required: Vec<serde_json::Value> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    for key in props.keys() {
        let key = serde_json::Value::from(key.as_str());
        if !required.contains(&key) {
            required.push(key);
        }
    }
    schema.insert("required".to_owned(), required.into());
}

/// Schema transform for enums whose value set is open within a schema major
/// (`format`, `tag_types`, error `kind`; ADR 0003): the known values, or any other
/// string added by a later 1.x minor version.
pub(crate) fn open_enum(schema: &mut schemars::Schema) {
    let mut known = std::mem::take(schema);
    let description = known.remove("description");
    let mut open = serde_json::Map::new();
    if let Some(d) = description {
        let d = d.as_str().unwrap_or_default();
        open.insert(
            "description".to_owned(),
            format!("{d} Open set: consumers must tolerate values not listed here.").into(),
        );
    }
    open.insert(
        "anyOf".to_owned(),
        serde_json::json!([
            known,
            {"type": "string", "description": "A value added in a later 1.x minor version."}
        ]),
    );
    *schema = schemars::Schema::from(open);
}

/// Root-relative, `/`-separated, lossy-UTF-8 representation of `path`.
///
/// Returns `"."` for the root itself. If `path` is not under `root`, the full lossy
/// path is returned (should not happen for walked entries).
pub fn rel_path_string(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    }
}

/// Normalisation used for *comparing* tag values (never for display):
/// trims Unicode whitespace, collapses internal whitespace runs to one space,
/// lowercases (Unicode `to_lowercase`, an approximation of full case folding).
pub fn normalize_for_compare(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_trims_collapses_and_folds() {
        assert_eq!(normalize_for_compare("  The\u{00A0} Wall\t "), "the wall");
        assert_eq!(normalize_for_compare("ÄBC"), "äbc");
        assert_eq!(normalize_for_compare("   "), "");
    }

    #[test]
    fn rel_path_is_slash_separated_and_dot_for_root() {
        let root = Path::new("/lib");
        assert_eq!(rel_path_string(root, Path::new("/lib")), ".");
        assert_eq!(
            rel_path_string(root, Path::new("/lib/A/B/01.flac")),
            "A/B/01.flac"
        );
    }

    #[test]
    fn severity_orders_info_lowest() {
        assert!(Severity::Info < Severity::Warning && Severity::Warning < Severity::Error);
    }
}
