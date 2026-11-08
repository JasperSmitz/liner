//! Stage 3: group tracks into album units (see ADR 0002).
//!
//! Owner: developer B (M2). Public signatures are frozen; internals are free.
//!
//! Contract:
//! - Album unit = the directory containing the audio files. A directory whose name
//!   is a disc folder ([`parse_disc_folder_name`]) is merged into its parent's unit,
//!   and its number becomes each of its tracks' `folder_disc`. Only one level is
//!   merged (`A/CD1/x.flac` -> unit `A`; `A/CD1/Extra/x.flac` -> unit `A/CD1/Extra`).
//!   A disc folder directly under the root merges into the root unit `"."`.
//! - Album id = root-relative directory path of the unit (`"."` for the root).
//! - `effective_disc`: tag disc number if present; else the folder disc hint; else
//!   `Some(1)` if the unit is single-disc; else `None` (unknown).
//! - A unit is multi-disc if any track has `disc_no > 1`, any `disc_total > 1`, or
//!   tracks come from 2+ distinct disc folders.
//! - Output units sorted by `id`; tracks within a unit sorted by
//!   (effective_disc with None last, track_no with None last, rel_path).
//! - Pure function of its input: no I/O.

use crate::model::TrackInfo;

/// A track placed in an album unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupedTrack {
    pub info: TrackInfo,
    /// Disc number parsed from the disc folder the file lives in, if any.
    pub folder_disc: Option<u32>,
    /// Root-relative path of that disc folder, if any.
    pub disc_folder: Option<String>,
    /// Disc used for position checks (see module docs).
    pub effective_disc: Option<u32>,
}

/// One album candidate: a directory (plus merged disc folders) of tracks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumUnit {
    /// Root-relative directory path, `/`-separated; `"."` for the root. Stable report id.
    pub id: String,
    /// Sorted, see module docs. Never empty.
    pub tracks: Vec<GroupedTrack>,
    pub is_multi_disc: bool,
    /// Most common raw `album` value (ties: lexicographically smallest); `None` if no track has one.
    pub title: Option<String>,
    /// Most common raw `album_artist` value, same tie-break; `None` if none.
    pub album_artist: Option<String>,
    /// Number of distinct known disc numbers (`effective_disc`), at least 1 for a
    /// single-disc unit; `max(disc_total)` is NOT used here.
    pub disc_count: u32,
}

impl AlbumUnit {
    /// Key used for split-album detection: normalised
    /// (album_artist, or the single shared artist if all tracks agree, album).
    /// `None` if the unit has no album title, or no unambiguous artist.
    pub fn album_key(&self) -> Option<(String, String)> {
        // M2 placeholder.
        None
    }
}

/// Parse a disc folder name, case-insensitively: `CD1`, `CD 2`, `cd-03`, `Disc 1`,
/// `Disk02`, `Disc 1 - Title`, `DISC_2`. Pattern (informal):
/// `^(cd|disc|disk)[\s._-]*(\d{1,3})([\s._-]+.*)?$` with number >= 1.
/// Returns `None` for anything else (e.g. `CDs`, `Discography`, `Disc`, `CD0`).
pub fn parse_disc_folder_name(name: &str) -> Option<u32> {
    // M2 placeholder.
    let _ = name;
    None
}

/// Group tracks (any order) into album units (sorted by id).
pub fn group_tracks(tracks: Vec<TrackInfo>) -> Vec<AlbumUnit> {
    // M2 placeholder.
    let _ = tracks;
    Vec::new()
}
