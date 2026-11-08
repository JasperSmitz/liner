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

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{TrackInfo, normalize_for_compare};

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
    ///
    /// "All tracks agree" ignores tracks without an artist tag (absent values never
    /// count as a distinct value), but at least one track must have one.
    pub fn album_key(&self) -> Option<(String, String)> {
        let album = normalize_for_compare(self.title.as_deref()?);
        let artist = match &self.album_artist {
            Some(aa) => normalize_for_compare(aa),
            None => {
                let artists: BTreeSet<String> = self
                    .tracks
                    .iter()
                    .filter_map(|t| t.info.artist.as_deref())
                    .map(normalize_for_compare)
                    .collect();
                if artists.len() != 1 {
                    return None;
                }
                artists.into_iter().next()?
            }
        };
        if album.is_empty() || artist.is_empty() {
            return None;
        }
        Some((artist, album))
    }
}

/// Parse a disc folder name, case-insensitively: `CD1`, `CD 2`, `cd-03`, `Disc 1`,
/// `Disk02`, `Disc 1 - Title`, `DISC_2`. Pattern (informal):
/// `^(cd|disc|disk)[\s._-]*(\d{1,3})([\s._-]+.*)?$` with number >= 1.
/// Returns `None` for anything else (e.g. `CDs`, `Discography`, `Disc`, `CD0`).
pub fn parse_disc_folder_name(name: &str) -> Option<u32> {
    let rest = ["disc", "disk", "cd"]
        .iter()
        .find_map(|prefix| strip_prefix_ascii_ci(name, prefix))?;
    let rest = rest.trim_start_matches(is_disc_separator);
    let digits_len = rest.bytes().take_while(u8::is_ascii_digit).count();
    // A longer digit run means the next character is a digit, not a separator.
    if !(1..=3).contains(&digits_len) {
        return None;
    }
    let (digits, tail) = rest.split_at(digits_len);
    if tail.chars().next().is_some_and(|c| !is_disc_separator(c)) {
        return None;
    }
    let number: u32 = digits.parse().ok()?;
    (number >= 1).then_some(number)
}

fn is_disc_separator(c: char) -> bool {
    c.is_whitespace() || matches!(c, '.' | '_' | '-')
}

fn strip_prefix_ascii_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// Where a track goes: (unit id, disc folder path + its number).
fn locate(rel_path: &str) -> (String, Option<(String, u32)>) {
    let Some((dir, _file)) = rel_path.rsplit_once('/') else {
        return (".".to_owned(), None);
    };
    let (parent, last) = dir.rsplit_once('/').unwrap_or((".", dir));
    match parse_disc_folder_name(last) {
        Some(n) => (parent.to_owned(), Some((dir.to_owned(), n))),
        None => (dir.to_owned(), None),
    }
}

/// Most common value; ties go to the lexicographically smallest. `None` if empty.
pub(crate) fn most_common<'a>(values: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for v in values {
        *counts.entry(v).or_default() += 1;
    }
    let mut best: Option<(&str, usize)> = None;
    // BTreeMap iterates in ascending order, so a strict `>` keeps the smallest on ties.
    for (v, n) in counts {
        if best.is_none_or(|(_, b)| n > b) {
            best = Some((v, n));
        }
    }
    best.map(|(v, _)| v.to_owned())
}

/// Group tracks (any order) into album units (sorted by id).
pub fn group_tracks(tracks: Vec<TrackInfo>) -> Vec<AlbumUnit> {
    let mut units: BTreeMap<String, Vec<GroupedTrack>> = BTreeMap::new();
    for info in tracks {
        let (id, disc) = locate(&info.rel_path);
        let (disc_folder, folder_disc) = disc.unzip();
        units.entry(id).or_default().push(GroupedTrack {
            info,
            folder_disc,
            disc_folder,
            effective_disc: None,
        });
    }
    units
        .into_iter()
        .map(|(id, tracks)| build_unit(id, tracks))
        .collect()
}

fn build_unit(id: String, mut tracks: Vec<GroupedTrack>) -> AlbumUnit {
    let disc_folders: BTreeSet<&str> = tracks
        .iter()
        .filter_map(|t| t.disc_folder.as_deref())
        .collect();
    let is_multi_disc = disc_folders.len() >= 2
        || tracks
            .iter()
            .any(|t| t.info.disc_no > Some(1) || t.info.disc_total > Some(1));

    for t in &mut tracks {
        t.effective_disc = t
            .info
            .disc_no
            .or(t.folder_disc)
            .or((!is_multi_disc).then_some(1));
    }
    // `None` sorts last: map to (is_none, value).
    tracks.sort_by(|a, b| {
        let key = |t: &GroupedTrack| {
            (
                t.effective_disc.is_none(),
                t.effective_disc,
                t.info.track_no.is_none(),
                t.info.track_no,
            )
        };
        key(a)
            .cmp(&key(b))
            .then_with(|| a.info.rel_path.cmp(&b.info.rel_path))
    });

    let title = most_common(tracks.iter().filter_map(|t| t.info.album.as_deref()));
    let album_artist = most_common(tracks.iter().filter_map(|t| t.info.album_artist.as_deref()));
    let known_discs: BTreeSet<u32> = tracks.iter().filter_map(|t| t.effective_disc).collect();
    let disc_count = u32::try_from(known_discs.len()).unwrap_or(u32::MAX);

    AlbumUnit {
        id,
        tracks,
        is_multi_disc,
        title,
        album_artist,
        disc_count,
    }
}
