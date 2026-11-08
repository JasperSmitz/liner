//! Stage 2: read tags and audio properties with lofty (read-only).
//!
//! Owner: developer A (M1). Public signatures are frozen; internals are free.
//!
//! Contract (see ADR 0001):
//! - Uses only lofty's read API (`Probe`/`read_from_path` with `ParseOptions`
//!   `read_cover_art(false)`); never any `save`/`remove` API (enforced by clippy.toml).
//! - Format is detected from content (lofty probe), falling back to extension.
//! - Field source: the primary tag first, then other tags present in the file
//!   (e.g. ID3v2 before ID3v1/APE) per field.
//! - Empty/whitespace-only text => `None`; numeric `0` => `None`.
//! - Any failure (I/O, unsupported, corrupt, zero-byte) => a `FileError` in
//!   `errors`, never a panic. Zero-byte files are `FileErrorKind::Corrupt`.
//! - `read_all` reads in parallel (rayon) and returns both vectors sorted by
//!   `rel_path`, so the output is independent of thread scheduling.

use crate::model::{FileError, TrackInfo};
use crate::scan::ScannedFile;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataResult {
    /// Sorted by `rel_path`.
    pub tracks: Vec<TrackInfo>,
    /// Sorted by `rel_path`.
    pub errors: Vec<FileError>,
}

/// Read one file. Must not panic on any input.
pub fn read_track(file: &ScannedFile) -> Result<TrackInfo, FileError> {
    // M1 placeholder: real lofty reading to be implemented by developer A.
    Err(FileError {
        path: file.path.clone(),
        rel_path: file.rel_path.clone(),
        kind: crate::model::FileErrorKind::Unsupported,
        message: "metadata reading not implemented yet".to_owned(),
    })
}

/// Read all files in parallel; see module docs for ordering guarantees.
pub fn read_all(files: &[ScannedFile]) -> MetadataResult {
    // M1 placeholder: should use rayon `par_iter` over `read_track`.
    let _ = files;
    MetadataResult::default()
}
