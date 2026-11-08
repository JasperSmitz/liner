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
//! - Units and disc folders are identified by the *real* directory path (the
//!   parent of `TrackInfo::path`, relative to the root, as an `OsStr` path), never
//!   by the lossy `rel_path` string, so directories whose names differ only in
//!   invalid UTF-8 bytes stay separate.
//! - Album id = root-relative directory path of the unit (`"."` for the root),
//!   lossy UTF-8. When distinct units' lossy ids collide, the unit whose raw path
//!   bytes sort first keeps the plain id and the others get ` #2`, ` #3`, ... in
//!   raw-path-byte order (skipping any suffixed id that is already taken). Disc
//!   folder paths (`disc_folder`) colliding within a unit are suffixed the same way.
//! - Album partitions ([`AlbumUnit::partitions`]): each track's partition key is
//!   (normalised album title, normalised album artist). The artist part is used
//!   only when the unit has 2+ distinct normalised album artists; a track without
//!   an album artist then uses its normalised track artist. If 2+ distinct keys
//!   with an album title exist, each is one partition and tracks without an album
//!   tag form one more partition; otherwise the whole unit is one partition.
//!   A partition without any album title is not a positional album (rules skip
//!   position, numbering and disc rules for it).
//! - A partition is multi-disc if any of its tracks has `disc_no > 1`,
//!   `disc_total > 1` or `folder_disc > 1`, its tracks come from 2+ distinct disc
//!   folders, or some come from a disc folder and others lie loose in the parent.
//!   A unit is multi-disc if any partition is.
//! - `effective_disc`: tag disc number if present; else the folder disc hint; else
//!   `Some(1)` if the track's *partition* is single-disc; else `None` (unknown).
//! - Output units sorted by `id`; tracks within a unit sorted by
//!   (effective_disc with None last, track_no with None last, rel_path).
//! - Pure function of its input: no I/O.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

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
    /// Root-relative directory path, `/`-separated, lossy UTF-8; `"."` for the root.
    /// Stable report id; suffixed ` #N` when distinct directories' lossy paths
    /// collide (see module docs).
    pub id: String,
    /// Sorted, see module docs. Never empty.
    pub tracks: Vec<GroupedTrack>,
    /// `true` if any album partition is multi-disc (see module docs). Always `true`
    /// when `disc_count > 1`; the converse does not hold when only one disc is
    /// known (e.g. disc 2 of 2 alone, or loose files without disc tags next to `CD1`).
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
    /// Key used for split-album detection, for a unit that is a single album
    /// partition: that partition's [`AlbumPartition::album_key`]. `None` for a mixed
    /// unit (several partitions; use [`AlbumUnit::partitions`] and each partition's
    /// key instead), and whenever the partition has no key.
    pub fn album_key(&self) -> Option<(String, String)> {
        match self.partitions().as_slice() {
            [only] => only.album_key(),
            _ => None,
        }
    }

    /// The album partitions of this unit (see module docs). Never empty for a
    /// non-empty unit; exactly one unless the tracks carry 2+ distinct partition
    /// keys with an album title. Partitions are ordered by normalised (album title,
    /// artist), tracks without an album tag first; tracks keep the unit's order.
    pub fn partitions(&self) -> Vec<AlbumPartition<'_>> {
        let infos: Vec<&TrackInfo> = self.tracks.iter().map(|t| &t.info).collect();
        let by_artist = keys_by_artist(&infos);
        let groups = partition_indices(&infos);
        let mixed = groups.len() > 1;
        groups
            .into_iter()
            .map(|indices| {
                let tracks: Vec<&GroupedTrack> = indices.iter().map(|&i| &self.tracks[i]).collect();
                let title = most_common(tracks.iter().filter_map(|t| t.info.album.as_deref()));
                let artist = by_artist
                    .then(|| {
                        most_common(
                            tracks
                                .iter()
                                .filter_map(|t| partition_artist(&t.info, true)),
                        )
                    })
                    .flatten();
                let label = mixed.then(|| match (&title, &artist) {
                    (Some(album), Some(artist)) => format!("Album '{album}' by '{artist}'"),
                    (Some(album), None) => format!("Album '{album}'"),
                    (None, _) => "Tracks without an album tag".to_owned(),
                });
                let is_multi_disc = detect_multi_disc(tracks.iter().copied());
                AlbumPartition {
                    label,
                    title,
                    tracks,
                    is_multi_disc,
                }
            })
            .collect()
    }
}

/// One album inside a unit: all of the unit's tracks, or (in a mixed unit) the
/// tracks sharing one partition key (normalised album title, plus the album artist
/// when the unit has several; see module docs). Multi-disc decision, effective-disc
/// fallback and the split-album key are computed per partition, so one album's tags
/// never affect another album stored in the same directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumPartition<'a> {
    /// Display name for messages (`Album 'X'`, `Album 'X' by 'Y'` when partitions
    /// are keyed by artist, or `Tracks without an album tag`); `None` when the unit
    /// is a single partition.
    pub label: Option<String>,
    /// Most common raw `album` value of these tracks (same tie-break as the unit).
    pub title: Option<String>,
    /// In unit order. Never empty.
    pub tracks: Vec<&'a GroupedTrack>,
    /// Same criteria as the unit-level decision, over these tracks only.
    pub is_multi_disc: bool,
}

impl AlbumPartition<'_> {
    /// `true` if no track of this partition has an album tag. Such tracks (loose
    /// singles, untagged files) never form a positional album: position, numbering
    /// and disc rules skip this partition.
    pub fn is_untitled(&self) -> bool {
        self.title.is_none()
    }

    /// `true` if any track has a disc tag or a disc folder hint.
    pub fn has_disc_info(&self) -> bool {
        self.tracks
            .iter()
            .any(|t| t.info.disc_no.is_some() || t.folder_disc.is_some())
    }

    /// Split-album key: normalised (artist, album), from this partition's own
    /// consensus. The artist is the album artist if the tracks carry exactly one
    /// distinct normalised album artist; otherwise the single normalised track
    /// artist shared by all tracks that have one. `None` if there is no album title,
    /// no unambiguous artist, every track is flagged as a compilation, or the artist
    /// normalises to `various artists`, `various` or `va` (unrelated compilations
    /// commonly share generic titles such as "Greatest Hits").
    pub fn album_key(&self) -> Option<(String, String)> {
        if self.tracks.iter().all(|t| t.info.compilation) {
            return None;
        }
        let album = normalize_for_compare(self.title.as_deref()?);
        let artist = self
            .single_normalized(|t| t.info.album_artist.as_deref())
            .or_else(|| self.single_normalized(|t| t.info.artist.as_deref()))?;
        if album.is_empty() || artist.is_empty() || is_various_artists(&artist) {
            return None;
        }
        Some((artist, album))
    }

    /// Original spelling of the key's artist for display: the most common album
    /// artist (or, failing that, track artist) spelling that normalises to it.
    pub fn key_artist_spelling(&self, normalized: &str) -> Option<String> {
        let pick = |get: fn(&GroupedTrack) -> Option<&str>| {
            most_common(
                self.tracks
                    .iter()
                    .filter_map(|t| get(t))
                    .filter(|v| normalize_for_compare(v) == normalized),
            )
        };
        pick(|t| t.info.album_artist.as_deref()).or_else(|| pick(|t| t.info.artist.as_deref()))
    }

    /// The one normalised value all tracks that have the field agree on.
    fn single_normalized(&self, get: impl Fn(&GroupedTrack) -> Option<&str>) -> Option<String> {
        let values: BTreeSet<String> = self
            .tracks
            .iter()
            .filter_map(|t| get(t))
            .map(normalize_for_compare)
            .collect();
        if values.len() == 1 {
            values.into_iter().next()
        } else {
            None
        }
    }
}

/// Normalised album artist values that mark a various-artists compilation.
fn is_various_artists(normalized: &str) -> bool {
    matches!(normalized, "various artists" | "various" | "va")
}

/// `true` if partitions are keyed by artist as well: the tracks carry 2+ distinct
/// normalised album artists.
fn keys_by_artist(tracks: &[&TrackInfo]) -> bool {
    let artists: BTreeSet<String> = tracks
        .iter()
        .filter_map(|t| t.album_artist.as_deref())
        .map(normalize_for_compare)
        .collect();
    artists.len() >= 2
}

/// Raw artist value used for a track's partition key: the album artist, else (when
/// keying by artist) the track artist.
fn partition_artist(info: &TrackInfo, by_artist: bool) -> Option<&str> {
    if !by_artist {
        return None;
    }
    info.album_artist.as_deref().or(info.artist.as_deref())
}

/// Indices of `tracks` per album partition (see [`AlbumUnit::partitions`]).
fn partition_indices(tracks: &[&TrackInfo]) -> Vec<Vec<usize>> {
    let by_artist = keys_by_artist(tracks);
    let mut untitled: Vec<usize> = Vec::new();
    let mut titled: BTreeMap<(String, Option<String>), Vec<usize>> = BTreeMap::new();
    for (i, info) in tracks.iter().enumerate() {
        match info.album.as_deref() {
            Some(album) => {
                let artist = partition_artist(info, by_artist).map(normalize_for_compare);
                titled
                    .entry((normalize_for_compare(album), artist))
                    .or_default()
                    .push(i);
            }
            None => untitled.push(i),
        }
    }
    if titled.len() <= 1 {
        return vec![(0..tracks.len()).collect()];
    }
    let mut groups = Vec::with_capacity(titled.len() + 1);
    if !untitled.is_empty() {
        groups.push(untitled);
    }
    groups.extend(titled.into_values());
    groups
}

/// Multi-disc decision over a set of tracks (a partition): any disc tag > 1, any
/// disc total > 1, any disc folder number > 1, 2+ distinct disc folders, or tracks
/// from a disc folder alongside loose files in the parent directory.
fn detect_multi_disc<'a>(tracks: impl Iterator<Item = &'a GroupedTrack> + Clone) -> bool {
    // Identity by the real directory, not the (possibly colliding) lossy string.
    let disc_folders: BTreeSet<PathBuf> = tracks
        .clone()
        .filter(|t| t.disc_folder.is_some())
        .map(|t| rel_dir(&t.info))
        .collect();
    let has_loose = tracks.clone().any(|t| t.disc_folder.is_none());
    disc_folders.len() >= 2
        || (!disc_folders.is_empty() && has_loose)
        || tracks.into_iter().any(|t| {
            t.info.disc_no > Some(1) || t.info.disc_total > Some(1) || t.folder_disc > Some(1)
        })
}

/// Parse a disc folder name, case-insensitively: `CD1`, `CD 2`, `cd-03`, `Disc 1`,
/// `Disk02`, `Disc 1 - Title`, `DISC_2`, `CD1(Bonus)`, `Disc 2[Live]`. Pattern
/// (informal): `^(cd|disc|disk)[\s._-]*(\d{1,3})([\s._-]+.*|[(\[].*)?$` with
/// number >= 1. Only ASCII digits count (full-width digits do not).
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
    if tail
        .chars()
        .next()
        .is_some_and(|c| !is_disc_separator(c) && !matches!(c, '(' | '['))
    {
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

/// Root-relative directory of a track, with the original (possibly non-UTF-8)
/// bytes: the parent of the last `n` components of `info.path`, where `n` is the
/// number of components of `rel_path`. Falls back to the directory part of
/// `rel_path` if `path` does not end in components matching `rel_path` (only for
/// hand-built `TrackInfo`s). Empty for the root.
fn rel_dir(info: &TrackInfo) -> PathBuf {
    let rel_parts: Vec<&str> = info.rel_path.split('/').collect();
    let tail: Vec<&OsStr> = info
        .path
        .components()
        .rev()
        .take(rel_parts.len())
        .filter_map(|c| match c {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect();
    let matches = tail.len() == rel_parts.len()
        && tail
            .iter()
            .rev()
            .zip(&rel_parts)
            .all(|(raw, lossy)| raw.to_string_lossy() == *lossy);
    let dir_parts = rel_parts.len().saturating_sub(1);
    if matches {
        tail.iter().rev().take(dir_parts).collect()
    } else {
        rel_parts.iter().take(dir_parts).collect()
    }
}

/// Lossy `/`-separated form of a root-relative directory; `"."` for the root.
fn lossy_id(dir: &Path) -> String {
    let parts: Vec<String> = dir
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    }
}

/// Where a track goes: (real unit directory, real disc folder directory + its number).
fn locate(info: &TrackInfo) -> (PathBuf, Option<(PathBuf, u32)>) {
    let dir = rel_dir(info);
    let Some(last) = dir.file_name() else {
        return (dir, None);
    };
    match parse_disc_folder_name(&last.to_string_lossy()) {
        Some(n) => {
            let parent = dir.parent().map(Path::to_path_buf).unwrap_or_default();
            (parent, Some((dir, n)))
        }
        None => (dir, None),
    }
}

/// Unique lossy display ids for distinct real paths (see module docs). Paths whose
/// lossy forms collide are numbered in raw-byte order: the first keeps the plain
/// id, later ones get the smallest free ` #N` (N >= 2).
fn unique_ids<'a>(paths: impl IntoIterator<Item = &'a Path>) -> BTreeMap<&'a Path, String> {
    let mut by_id: BTreeMap<String, Vec<&Path>> = BTreeMap::new();
    for p in paths {
        by_id.entry(lossy_id(p)).or_default().push(p);
    }
    let mut taken: BTreeSet<String> = by_id.keys().cloned().collect();
    let mut out = BTreeMap::new();
    for (id, mut group) in by_id {
        group.sort_by(|a, b| {
            a.as_os_str()
                .as_encoded_bytes()
                .cmp(b.as_os_str().as_encoded_bytes())
        });
        group.dedup();
        let mut iter = group.into_iter();
        if let Some(first) = iter.next() {
            out.insert(first, id.clone());
        }
        let mut n = 2u64;
        for p in iter {
            let candidate = loop {
                let c = format!("{id} #{n}");
                n += 1;
                if !taken.contains(&c) {
                    break c;
                }
            };
            taken.insert(candidate.clone());
            out.insert(p, candidate);
        }
    }
    out
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
    // Keyed by the real unit directory; values carry each track's real disc folder.
    type Located = (TrackInfo, Option<(PathBuf, u32)>);
    let mut units: BTreeMap<PathBuf, Vec<Located>> = BTreeMap::new();
    for info in tracks {
        let (dir, disc) = locate(&info);
        units.entry(dir).or_default().push((info, disc));
    }
    let ids = unique_ids(units.keys().map(PathBuf::as_path));
    let mut out: Vec<AlbumUnit> = units
        .iter()
        .map(|(dir, located)| {
            let folder_names = unique_ids(
                located
                    .iter()
                    .filter_map(|(_, disc)| disc.as_ref().map(|(p, _)| p.as_path())),
            );
            let tracks = located
                .iter()
                .map(|(info, disc)| GroupedTrack {
                    info: info.clone(),
                    folder_disc: disc.as_ref().map(|&(_, n)| n),
                    disc_folder: disc
                        .as_ref()
                        .map(|(p, _)| folder_names[p.as_path()].clone()),
                    effective_disc: None,
                })
                .collect();
            build_unit(ids[dir.as_path()].clone(), tracks)
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn build_unit(id: String, mut tracks: Vec<GroupedTrack>) -> AlbumUnit {
    let mut is_unit_multi_disc = false;
    let infos: Vec<&TrackInfo> = tracks.iter().map(|t| &t.info).collect();
    let groups = partition_indices(&infos);
    for indices in groups {
        let multi = detect_multi_disc(indices.iter().map(|&i| &tracks[i]));
        is_unit_multi_disc |= multi;
        for i in indices {
            let t = &mut tracks[i];
            t.effective_disc = t.info.disc_no.or(t.folder_disc).or((!multi).then_some(1));
        }
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
            .then_with(|| a.info.path.cmp(&b.info.path))
    });

    let title = most_common(tracks.iter().filter_map(|t| t.info.album.as_deref()));
    let album_artist = most_common(tracks.iter().filter_map(|t| t.info.album_artist.as_deref()));
    let known_discs: BTreeSet<u32> = tracks.iter().filter_map(|t| t.effective_disc).collect();
    let disc_count = u32::try_from(known_discs.len()).unwrap_or(u32::MAX);

    AlbumUnit {
        id,
        tracks,
        is_multi_disc: is_unit_multi_disc,
        title,
        album_artist,
        disc_count,
    }
}
