//! Album-scope rules about positions and numbering.
//!
//! Disc-level rules (`missing-disc-number`, `disc-folder-mismatch`,
//! `disc-number-gap`, `inconsistent-disc-total`) look at the whole unit, matching the
//! unit-level multi-disc decision made in `group`. Track-level rules
//! (`likely-missing-disc-numbers`, `duplicate-track-position`, `track-number-gap`,
//! `inconsistent-track-total`) run per album partition (see
//! [`super::util::album_partitions`]).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use crate::group::{AlbumUnit, GroupedTrack};
use crate::model::Issue;

use super::codes;
use super::util::{
    MAX_GAP_RANGE, Partition, album_partitions, count_noun, details, issue, number_ranges, paths,
    quote,
};

pub(super) fn check(unit: &AlbumUnit, out: &mut Vec<Issue>) {
    missing_disc_number(unit, out);
    disc_folder_mismatch(unit, out);
    disc_number_gap(unit, out);
    inconsistent_disc_total(unit, out);

    let has_disc_info = unit
        .tracks
        .iter()
        .any(|t| t.info.disc_no.is_some() || t.folder_disc.is_some());
    for part in album_partitions(unit) {
        // When disc numbers are likely missing, (disc, track) positions are known to
        // be unreliable: the duplicate/gap/total rules would only restate that.
        if !has_disc_info && likely_missing_disc_numbers(unit, &part, out) {
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

/// "Disc 2 " prefix for multi-disc units; nothing for single-disc ones.
fn on_disc(unit: &AlbumUnit, disc: u32) -> String {
    if unit.is_multi_disc {
        format!(" on disc {disc}")
    } else {
        String::new()
    }
}

/// Returns `true` if the issue was emitted.
fn likely_missing_disc_numbers(
    unit: &AlbumUnit,
    part: &Partition<'_>,
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
    // Every disc starts at track 1, so a repeated track 1 is the signature of merged
    // discs. Other repeats (e.g. one duplicated file) are plain duplicates.
    if !repeated.contains_key(&1) {
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

fn duplicate_track_position(
    unit: &AlbumUnit,
    part: &Partition<'_>,
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
                on_disc(unit, disc),
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
    part: &Partition<'_>,
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
        // ended at 12) is a common convention, not a gap. With it, track totals are
        // usually album-wide, so they do not extend a disc that the next one continues.
        let continues = |earlier: u32, later: u32| -> bool {
            let (Some(a), Some(b)) = (discs.get(&earlier), discs.get(&later)) else {
                return false;
            };
            match (numbers_of(a).last(), numbers_of(b).first()) {
                (Some(&a_highest), Some(&b_lowest)) => b_lowest == a_highest.saturating_add(1),
                _ => false,
            }
        };
        let continues_previous = disc > 1 && continues(disc - 1, disc);
        let continued_by_next = disc < u32::MAX && continues(disc, disc + 1);
        let start = if continues_previous { lowest } else { 1 };
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
        let what = if unit.is_multi_disc {
            format!("Disc {disc} is missing")
        } else {
            "Missing".to_owned()
        };
        let total = match expected_total {
            Some(total) => format!(" (track total {total})"),
            None => format!(" (highest track {highest})"),
        };
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
    part: &Partition<'_>,
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
            on_disc(unit, disc),
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

fn missing_disc_number(unit: &AlbumUnit, out: &mut Vec<Issue>) {
    if !unit.is_multi_disc {
        return;
    }
    let untagged: Vec<&GroupedTrack> = unit
        .tracks
        .iter()
        .filter(|t| t.info.disc_no.is_none())
        .collect();
    if untagged.is_empty() {
        return;
    }
    let count = untagged.len();
    let message = format!(
        "{} of {} in this multi-disc album {} no disc number tag",
        count,
        count_noun(unit.tracks.len(), "track", "tracks"),
        if count == 1 { "has" } else { "have" }
    );
    out.push(issue(
        codes::MISSING_DISC_NUMBER,
        message,
        Some(&unit.id),
        paths(untagged),
        details([("count", json!(count))]),
    ));
}

fn disc_folder_mismatch(unit: &AlbumUnit, out: &mut Vec<Issue>) {
    // (folder, tag disc) -> (folder disc, files)
    let mut groups: BTreeMap<(&str, u32), (u32, Vec<&GroupedTrack>)> = BTreeMap::new();
    for t in &unit.tracks {
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
        let message = format!(
            "{} in folder {} (disc {folder_disc}) {} tagged disc {tag_disc}",
            count_noun(files.len(), "file", "files"),
            quote(folder),
            if files.len() == 1 { "is" } else { "are" }
        );
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

fn disc_number_gap(unit: &AlbumUnit, out: &mut Vec<Issue>) {
    let known: BTreeSet<u32> = unit
        .tracks
        .iter()
        .filter_map(|t| t.effective_disc)
        .collect();
    // No known disc at all is covered by missing-disc-number.
    let Some(&highest) = known.last() else {
        return;
    };
    let expected_total = agreed(unit.tracks.iter().filter_map(|t| t.info.disc_total));
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
    let message = format!(
        "Missing {} {} (found {} {}{total})",
        if missing.len() == 1 { "disc" } else { "discs" },
        number_ranges(&missing),
        if known.len() == 1 { "disc" } else { "discs" },
        number_ranges(&known)
    );
    out.push(issue(
        codes::DISC_NUMBER_GAP,
        message,
        Some(&unit.id),
        paths(&unit.tracks),
        details([
            ("missing", json!(missing)),
            ("expected_total", json!(expected_total)),
        ]),
    ));
}

fn inconsistent_disc_total(unit: &AlbumUnit, out: &mut Vec<Issue>) {
    let with_total: Vec<&GroupedTrack> = unit
        .tracks
        .iter()
        .filter(|t| t.info.disc_total.is_some())
        .collect();
    let values: BTreeSet<u32> = with_total
        .iter()
        .filter_map(|t| t.info.disc_total)
        .collect();
    if values.len() < 2 {
        return;
    }
    let message = format!(
        "Tracks disagree on the disc total: {}",
        join_numbers(&values)
    );
    out.push(issue(
        codes::INCONSISTENT_DISC_TOTAL,
        message,
        Some(&unit.id),
        paths(with_total),
        details([("values", json!(values))]),
    ));
}
