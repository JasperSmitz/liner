//! Stage 1: walk the library and find candidate audio files.
//!
//! Owner: developer A (M1). Public signatures are frozen; internals are free.
//!
//! Contract:
//! - Never follows symlinks unless `follow_symlinks` (walkdir handles loops; a loop
//!   becomes a `FileErrorKind::Walk` entry).
//! - Skips hidden entries (name starts with `.`) unless `include_hidden`; a hidden
//!   directory is not descended into. The root itself is never considered hidden.
//! - Classifies by file extension only (case-insensitive, [`AUDIO_EXTENSIONS`]);
//!   content sniffing is the metadata stage's job.
//! - Per-entry errors are collected, never fatal. Only an unusable root is fatal.
//! - Opens nothing for writing; reads only directory listings and file metadata.
//! - Output `audio_files` is sorted by `rel_path`.

use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::model::FileError;

/// Extensions (lowercase, without dot) treated as audio files.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "flac", "mp3", "ogg", "oga", "opus", "spx", "m4a", "m4b", "mp4", "aac", "wav", "aif", "aiff",
    "ape", "wv", "mpc",
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScanOptions {
    pub follow_symlinks: bool,
    pub include_hidden: bool,
}

/// A file whose extension marks it as audio. Not yet opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedFile {
    /// Root joined with the walked path; use this to open the file.
    pub path: PathBuf,
    /// Root-relative, `/`-separated (see `model::rel_path_string`).
    pub rel_path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanResult {
    /// Sorted by `rel_path`.
    pub audio_files: Vec<ScannedFile>,
    /// Count of regular files seen that are not audio (cover art, cue, logs, ...).
    pub other_files: u64,
    /// Entries that could not be walked/stat'ed. Sorted by `rel_path`.
    pub errors: Vec<FileError>,
}

impl ScanResult {
    /// All regular files seen (audio + other), i.e. `summary.files_scanned`.
    pub fn files_scanned(&self) -> u64 {
        self.audio_files.len() as u64 + self.other_files
    }
}

/// `true` if `ext` (without dot, any case) is in [`AUDIO_EXTENSIONS`].
pub fn is_audio_extension(ext: &str) -> bool {
    AUDIO_EXTENSIONS
        .iter()
        .any(|known| known.eq_ignore_ascii_case(ext))
}

/// Walk `root` recursively.
///
/// Errors: [`crate::error::LinerError::RootNotFound`] /
/// [`crate::error::LinerError::RootNotDirectory`] if the root is unusable.
pub fn scan(root: &Path, options: &ScanOptions) -> Result<ScanResult> {
    // M1 placeholder: real walk to be implemented by developer A.
    let _ = (root, options);
    Ok(ScanResult::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_extension_check_is_case_insensitive() {
        assert!(is_audio_extension("FLAC"));
        assert!(is_audio_extension("m4a"));
        assert!(!is_audio_extension("jpg"));
        assert!(!is_audio_extension("cue"));
    }
}
