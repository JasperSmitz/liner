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
//! - Per-entry errors are collected, never fatal. Only an unusable root is fatal:
//!   missing, not a directory, or not listable (e.g. mode 000) => `LinerError`.
//! - Opens nothing for writing; reads only directory listings and file metadata.
//! - Output `audio_files` is sorted by `rel_path`.

use std::path::{Path, PathBuf};

use walkdir::{DirEntry, WalkDir};

use crate::error::{LinerError, Result};
use crate::model::{FileError, FileErrorKind, rel_path_string};

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
/// [`crate::error::LinerError::RootNotDirectory`] if the root is unusable, and
/// [`crate::error::LinerError::Io`] if it cannot be stat'ed or listed.
///
/// Symlinks are skipped entirely (neither audio nor "other") unless
/// `follow_symlinks` is set, in which case they are resolved and treated like their
/// targets. Special files (FIFOs, sockets, devices) are ignored.
pub fn scan(root: &Path, options: &ScanOptions) -> Result<ScanResult> {
    check_root(root)?;

    let mut result = ScanResult::default();
    let walker = WalkDir::new(root)
        .follow_links(options.follow_symlinks)
        .into_iter()
        .filter_entry(|entry| options.include_hidden || entry.depth() == 0 || !is_hidden(entry));

    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                result.errors.push(walk_error(root, &err));
                continue;
            }
        };
        let file_type = entry.file_type();
        if !file_type.is_file() {
            // Directories are descended by walkdir; unfollowed symlinks and special
            // files are deliberately ignored.
            continue;
        }
        let path = entry.path();
        let audio = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(is_audio_extension);
        if !audio {
            result.other_files += 1;
            continue;
        }
        match entry.metadata() {
            Ok(meta) => result.audio_files.push(ScannedFile {
                path: path.to_path_buf(),
                rel_path: rel_path_string(root, path),
                size: meta.len(),
            }),
            Err(err) => result.errors.push(walk_error(root, &err)),
        }
    }

    // Lossy rel_paths of distinct non-UTF-8 names can collide; break ties on the
    // real path so the order stays total and deterministic.
    result
        .audio_files
        .sort_by(|a, b| (&a.rel_path, &a.path).cmp(&(&b.rel_path, &b.path)));
    result.errors.sort_by(|a, b| {
        (&a.rel_path, &a.path, &a.message).cmp(&(&b.rel_path, &b.path, &b.message))
    });
    Ok(result)
}

/// Fatal checks on the root: it must exist, be a directory (symlinks resolved) and
/// be listable. Without the listing check an unreadable root would surface as one
/// non-fatal walk error and an apparently empty library.
fn check_root(root: &Path) -> Result<()> {
    match std::fs::metadata(root) {
        Ok(meta) if meta.is_dir() => match std::fs::read_dir(root) {
            Ok(_) => Ok(()),
            Err(source) => Err(LinerError::Io {
                path: root.to_path_buf(),
                source,
            }),
        },
        Ok(_) => Err(LinerError::RootNotDirectory(root.to_path_buf())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Err(LinerError::RootNotFound(root.to_path_buf()))
        }
        Err(source) => Err(LinerError::Io {
            path: root.to_path_buf(),
            source,
        }),
    }
}

fn is_hidden(entry: &DirEntry) -> bool {
    entry.file_name().as_encoded_bytes().starts_with(b".")
}

fn walk_error(root: &Path, err: &walkdir::Error) -> FileError {
    let path = err.path().unwrap_or(root).to_path_buf();
    let message = match (err.loop_ancestor(), err.io_error()) {
        (Some(ancestor), _) => format!(
            "symlink loop: points to ancestor {}",
            rel_path_string(root, ancestor)
        ),
        (None, Some(io)) => io.to_string(),
        (None, None) => err.to_string(),
    };
    FileError {
        rel_path: rel_path_string(root, &path),
        path,
        kind: FileErrorKind::Walk,
        message,
    }
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
