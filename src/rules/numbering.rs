//! Album-scope rules about positions and numbering.
//!
//! Every rule here runs per album partition ([`AlbumUnit::partitions`]), matching
//! the per-partition multi-disc decision and effective-disc fallback made in
//! `group`: one album's disc tags never affect another album in the same directory.
//! A partition without any album tag is skipped entirely (it is not an album).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use crate::group::{AlbumPartition, AlbumUnit, GroupedTrack};
use crate::model::{Issue, normalize_for_compare};

use super::codes;
use super::util::{
    MAX_GAP_RANGE, PartitionMessage, count_noun, details, file_name, issue, number_ranges, paths,
    quote,
};

pub(super) fn check(unit: &AlbumUnit, out: &mut Vec<Issue>) {
    for part in unit.partitions() {
        // Tracks without any album tag (loose singles) are not a positional album:
        // their track and disc numbers are not comparable with each other.
        if part.is_untitled() {
            continue;
        }
        missing_disc_number(unit, &part, out);
        disc_folder_mismatch(unit, &part, out);
        disc_number_gap(unit, &part, out);
        inconsistent_disc_total(unit, &part, out);

        // When disc numbers are likely missing, (disc, track) positions are known to
        // be unreliable: the duplicate/gap/total rules would only restate that.
        if !part.has_disc_info() && likely_missing_disc_numbers(unit, &part, out) {
            continue;
        }
        let discs = tracks_by_disc(&part.tracks);
        duplicate_track_position(unit, &part, &discs, out);
        track_number_gap(unit, &part, &discs, out);
        inconsistent_track_total(unit, &part, &discs, out);
    }
}

/// Tracks with a known effective disc, keyed by that disc.
fn tracks_by_disc<'a>(tracks: &[&'a GroupedTrack]) -> BTreeMap<u32, Vec<&'a GroupedTrack>> {
    let mut discs: BTreeMap<u32, Vec<&GroupedTrack>> = BTreeMap::new();
    for t in tracks {
        if let Some(d) = t.effective_disc {
            discs.entry(d).or_default().push(t);
        }
    }
    discs
}

/// " on disc 2" for multi-disc partitions; nothing for single-disc ones.
fn on_disc(part: &AlbumPartition<'_>, disc: u32) -> String {
    if part.is_multi_disc {
        format!(" on disc {disc}")
    } else {
        String::new()
    }
}

/// Returns `true` if the issue was emitted.
fn likely_missing_disc_numbers(
    unit: &AlbumUnit,
    part: &AlbumPartition<'_>,
    out: &mut Vec<Issue>,
) -> bool {
    let mut by_number: BTreeMap<u32, Vec<&GroupedTrack>> = BTreeMap::new();
    for t in &part.tracks {
        if let Some(n) = t.info.track_no {
            by_number.entry(n).or_default().push(t);
        }
    }
    let repeated: BTreeMap<u32, Vec<&GroupedTrack>> = by_number
        .into_iter()
        .filter(|(_, ts)| ts.len() > 1)
        .collect();
    // Every disc starts at track 1, so distinct tracks sharing track 1 (and at least
    // one more number) are the signature of merged discs. Copies of the same track
    // (one album in two formats, `01 (1).flac`) are plain duplicates.
    let distinct: BTreeSet<u32> = repeated
        .iter()
        .filter(|(_, ts)| are_distinct_tracks(ts))
        .map(|(&n, _)| n)
        .collect();
    if !distinct.contains(&1) || distinct.len() < 2 {
        return false;
    }
    let numbers: BTreeSet<u32> = repeated.keys().copied().collect();
    let affected = paths(repeated.values().flatten().copied());
    let (noun, verb) = if numbers.len() == 1 {
        ("number", "occurs")
    } else {
        ("numbers", "occur")
    };
    let message = part.message(format!(
        "Track {noun} {} {verb} more than once but no file has a disc number (probably several discs without disc tags)",
        number_ranges(&numbers)
    ));
    out.push(issue(
        codes::LIKELY_MISSING_DISC_NUMBERS,
        message,
        Some(&unit.id),
        affected,
        details([("duplicated_tracks", json!(numbers))]),
    ));
    true
}

/// `true` if the files are at least two different tracks: by normalised title when
/// every file has one, else by file stem (see [`copy_stem`]).
fn are_distinct_tracks(tracks: &[&GroupedTrack]) -> bool {
    let identities: BTreeSet<String> = if tracks.iter().all(|t| t.info.title.is_some()) {
        tracks
            .iter()
            .filter_map(|t| t.info.title.as_deref())
            .map(normalize_for_compare)
            .collect()
    } else {
        tracks
            .iter()
            .map(|t| copy_stem(file_name(&t.info.rel_path)))
            .collect()
    };
    identities.len() > 1
}

/// Normalised file name without extension and without trailing ` (N)` copy
/// suffixes: `01 - Song (1).flac` and `01 - Song.mp3` both give `01 - song`.
fn copy_stem(name: &str) -> String {
    let mut stem = name
        .rsplit_once('.')
        .map_or(name, |(stem, _)| stem)
        .trim_end();
    while let Some(rest) = stem.strip_suffix(')')
        && let Some((head, digits)) = rest.rsplit_once('(')
        && !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && head.ends_with(char::is_whitespace)
    {
        stem = head.trim_end();
    }
    normalize_for_compare(stem)
}

fn duplicate_track_position(
    unit: &AlbumUnit,
    part: &AlbumPartition<'_>,
    discs: &BTreeMap<u32, Vec<&GroupedTrack>>,
    out: &mut Vec<Issue>,
) {
    for (&disc, tracks) in discs {
        let mut by_number: BTreeMap<u32, Vec<&GroupedTrack>> = BTreeMap::new();
        for t in tracks {
            if let Some(n) = t.info.track_no {
                by_number.entry(n).or_default().push(t);
            }
        }
        for (track, files) in by_number.into_iter().filter(|(_, f)| f.len() > 1) {
            let message = part.message(format!(
                "Track {track}{} appears {} times",
                on_disc(part, disc),
                files.len()
            ));
            out.push(issue(
                codes::DUPLICATE_TRACK_POSITION,
                message,
                Some(&unit.id),
                paths(files),
                details([("disc", json!(disc)), ("track", json!(track))]),
            ));
        }
    }
}

/// The single value all present values agree on (absent values ignored).
fn agreed<I: IntoIterator<Item = u32>>(values: I) -> Option<u32> {
    let set: BTreeSet<u32> = values.into_iter().collect();
    if set.len() == 1 {
        set.into_iter().next()
    } else {
        None
    }
}

fn track_number_gap(
    unit: &AlbumUnit,
    part: &AlbumPartition<'_>,
    discs: &BTreeMap<u32, Vec<&GroupedTrack>>,
    out: &mut Vec<Issue>,
) {
    let numbers_of = |tracks: &[&GroupedTrack]| -> BTreeSet<u32> {
        tracks.iter().filter_map(|t| t.info.track_no).collect()
    };
    for (&disc, tracks) in discs {
        let numbers = numbers_of(tracks);
        // A disc without any track number is fully covered by missing-track-number.
        let (Some(&lowest), Some(&highest)) = (numbers.first(), numbers.last()) else {
            continue;
        };
        // Continuous numbering across discs (disc 2 starting at 13 after disc 1
        // ended at 12) is a common convention, not a gap. A disc continues the
        // previous one when its lowest number is above both 1 and the previous disc's
        // highest; numbers missing at the boundary are then reported once, on the
        // later disc (whose range starts right after the previous disc's highest).
        // With continuous numbering, track totals are usually album-wide, so they do
        // not extend a disc that the next one continues.
        let continuation = |earlier: u32, later: u32| -> Option<u32> {
            let (a, b) = (discs.get(&earlier)?, discs.get(&later)?);
            let a_highest = *numbers_of(a).last()?;
            let b_lowest = *numbers_of(b).first()?;
            (b_lowest > a_highest && b_lowest > 1).then_some(a_highest)
        };
        let previous_highest = if disc > 1 {
            continuation(disc - 1, disc)
        } else {
            None
        };
        let continued_by_next = disc < u32::MAX && continuation(disc, disc + 1).is_some();
        // `previous_highest < lowest`, so the addition cannot overflow.
        let start = previous_highest.map_or(1, |h| h + 1);
        let expected_total = agreed(tracks.iter().filter_map(|t| t.info.track_total));
        let end = if continued_by_next {
            highest
        } else {
            highest.max(expected_total.unwrap_or(0))
        };
        if end > MAX_GAP_RANGE {
            continue;
        }
        let missing: BTreeSet<u32> = (start..=end).filter(|n| !numbers.contains(n)).collect();
        if missing.is_empty() {
            continue;
        }
        let what = if part.is_multi_disc {
            format!("Disc {disc} is missing")
        } else {
            "Missing".to_owned()
        };
        let mut total = match expected_total {
            Some(total) => format!(" (track total {total}"),
            None => format!(" (highest track {highest}"),
        };
        if previous_highest.is_some() && missing.first().is_some_and(|&m| m < lowest) {
            total.push_str(&format!(", numbering continues from disc {}", disc - 1));
        }
        total.push(')');
        let message = part.message(format!(
            "{what} {} {}{total}",
            if missing.len() == 1 {
                "track"
            } else {
                "tracks"
            },
            number_ranges(&missing)
        ));
        out.push(issue(
            codes::TRACK_NUMBER_GAP,
            message,
            Some(&unit.id),
            paths(tracks.iter().copied()),
            details([
                ("disc", json!(disc)),
                ("missing", json!(missing)),
                ("expected_total", json!(expected_total)),
            ]),
        ));
    }
}

fn inconsistent_track_total(
    unit: &AlbumUnit,
    part: &AlbumPartition<'_>,
    discs: &BTreeMap<u32, Vec<&GroupedTrack>>,
    out: &mut Vec<Issue>,
) {
    for (&disc, tracks) in discs {
        let with_total: Vec<&GroupedTrack> = tracks
            .iter()
            .copied()
            .filter(|t| t.info.track_total.is_some())
            .collect();
        let values: BTreeSet<u32> = with_total
            .iter()
            .filter_map(|t| t.info.track_total)
            .collect();
        if values.len() < 2 {
            continue;
        }
        let message = part.message(format!(
            "Tracks{} disagree on the track total: {}",
            on_disc(part, disc),
            join_numbers(&values)
        ));
        out.push(issue(
            codes::INCONSISTENT_TRACK_TOTAL,
            message,
            Some(&unit.id),
            paths(with_total),
            details([("disc", json!(disc)), ("values", json!(values))]),
        ));
    }
}

fn join_numbers(values: &BTreeSet<u32>) -> String {
    values
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn missing_disc_number(unit: &AlbumUnit, part: &AlbumPartition<'_>, out: &mut Vec<Issue>) {
    if !part.is_multi_disc {
        return;
    }
    let untagged: Vec<&GroupedTrack> = part
        .tracks
        .iter()
        .copied()
        .filter(|t| t.info.disc_no.is_none())
        .collect();
    if untagged.is_empty() {
        return;
    }
    let count = untagged.len();
    let message = part.message(format!(
        "{} of {} in this multi-disc album {} no disc number tag",
        count,
        count_noun(part.tracks.len(), "track", "tracks"),
        if count == 1 { "has" } else { "have" }
    ));
    out.push(issue(
        codes::MISSING_DISC_NUMBER,
        message,
        Some(&unit.id),
        paths(untagged),
        details([("count", json!(count))]),
    ));
}

fn disc_folder_mismatch(unit: &AlbumUnit, part: &AlbumPartition<'_>, out: &mut Vec<Issue>) {
    // (folder, tag disc) -> (folder disc, files)
    let mut groups: BTreeMap<(&str, u32), (u32, Vec<&GroupedTrack>)> = BTreeMap::new();
    for &t in &part.tracks {
        if let (Some(tag), Some(folder_disc), Some(folder)) =
            (t.info.disc_no, t.folder_disc, t.disc_folder.as_deref())
            && tag != folder_disc
        {
            groups
                .entry((folder, tag))
                .or_insert_with(|| (folder_disc, Vec::new()))
                .1
                .push(t);
        }
    }
    for ((folder, tag_disc), (folder_disc, files)) in groups {
        let message = part.message(format!(
            "{} in folder {} (disc {folder_disc}) {} tagged disc {tag_disc}",
            count_noun(files.len(), "file", "files"),
            quote(folder),
            if files.len() == 1 { "is" } else { "are" }
        ));
        out.push(issue(
            codes::DISC_FOLDER_MISMATCH,
            message,
            Some(&unit.id),
            paths(files),
            details([
                ("tag_disc", json!(tag_disc)),
                ("folder_disc", json!(folder_disc)),
                ("folder", json!(folder)),
            ]),
        ));
    }
}

fn disc_number_gap(unit: &AlbumUnit, part: &AlbumPartition<'_>, out: &mut Vec<Issue>) {
    let known: BTreeSet<u32> = part
        .tracks
        .iter()
        .filter_map(|t| t.effective_disc)
        .collect();
    // No known disc at all is covered by missing-disc-number.
    let Some(&highest) = known.last() else {
        return;
    };
    let expected_total = agreed(part.tracks.iter().filter_map(|t| t.info.disc_total));
    let end = highest.max(expected_total.unwrap_or(0));
    if end > MAX_GAP_RANGE {
        return;
    }
    let missing: BTreeSet<u32> = (1..=end).filter(|d| !known.contains(d)).collect();
    if missing.is_empty() {
        return;
    }
    let total = match expected_total {
        Some(total) => format!(", disc total {total}"),
        None => String::new(),
    };
    let message = part.message(format!(
        "Missing {} {} (found {} {}{total})",
        if missing.len() == 1 { "disc" } else { "discs" },
        number_ranges(&missing),
        if known.len() == 1 { "disc" } else { "discs" },
        number_ranges(&known)
    ));
    out.push(issue(
        codes::DISC_NUMBER_GAP,
        message,
        Some(&unit.id),
        paths(part.tracks.iter().copied()),
        details([
            ("missing", json!(missing)),
            ("expected_total", json!(expected_total)),
        ]),
    ));
}

fn inconsistent_disc_total(unit: &AlbumUnit, part: &AlbumPartition<'_>, out: &mut Vec<Issue>) {
    let with_total: Vec<&GroupedTrack> = part
        .tracks
        .iter()
        .copied()
        .filter(|t| t.info.disc_total.is_some())
        .collect();
    let values: BTreeSet<u32> = with_total
        .iter()
        .filter_map(|t| t.info.disc_total)
        .collect();
    if values.len() < 2 {
        return;
    }
    let message = part.message(format!(
        "Tracks disagree on the disc total: {}",
        join_numbers(&values)
    ));
    out.push(issue(
        codes::INCONSISTENT_DISC_TOTAL,
        message,
        Some(&unit.id),
        paths(with_total),
        details([("values", json!(values))]),
    ));
}
