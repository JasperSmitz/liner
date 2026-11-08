//! Helpers shared by the rule submodules: issue construction, album partitions and
//! message formatting.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::group::{AlbumUnit, GroupedTrack, most_common};
use crate::model::{Issue, normalize_for_compare};

use super::{RuleInfo, rule_info};

/// Ranges larger than this are not expanded into `missing` lists (a mistyped track
/// number like 2003 would otherwise produce thousands of "missing" tracks).
pub(super) const MAX_GAP_RANGE: u32 = 999;

/// Messages list at most this many values, then "and N more".
const MAX_LISTED: usize = 5;

pub(super) type Details = BTreeMap<String, Value>;

/// Catalogue entry for a code from [`super::codes`].
pub(super) fn rule(code: &str) -> &'static RuleInfo {
    rule_info(code).expect("every code in rules::codes is in CATALOG")
}

/// Build an issue for `code` (severity/scope from the catalogue).
pub(super) fn issue(
    code: &str,
    message: String,
    album_id: Option<&str>,
    paths: Vec<String>,
    details: Details,
) -> Issue {
    rule(code).issue(message, album_id, paths, details)
}

pub(super) fn details<const N: usize>(entries: [(&str, Value); N]) -> Details {
    entries
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect()
}

pub(super) fn paths<'a>(tracks: impl IntoIterator<Item = &'a GroupedTrack>) -> Vec<String> {
    tracks
        .into_iter()
        .map(|t| t.info.rel_path.clone())
        .collect()
}

/// A subset of a unit's tracks that album-level rules treat as one album.
///
/// In a unit whose tracks carry 2+ distinct normalised album titles (a "mixed"
/// directory, reported by `mixed-album-directory`), track-numbering and album-tag
/// rules run per album title, so a flat folder of unrelated albums does not also
/// produce duplicate positions, gaps or artist issues across different albums. In
/// every other unit there is exactly one partition holding all tracks.
pub(super) struct Partition<'a> {
    /// Message prefix naming the album; `None` for the single-partition case.
    pub label: Option<String>,
    pub tracks: Vec<&'a GroupedTrack>,
}

impl Partition<'_> {
    /// Prefix `message` with the album label, if any.
    pub fn message(&self, message: String) -> String {
        match &self.label {
            Some(label) => format!("{label}: {message}"),
            None => message,
        }
    }
}

pub(super) fn album_partitions(unit: &AlbumUnit) -> Vec<Partition<'_>> {
    let mut by_album: BTreeMap<Option<String>, Vec<&GroupedTrack>> = BTreeMap::new();
    for t in &unit.tracks {
        let key = t.info.album.as_deref().map(normalize_for_compare);
        by_album.entry(key).or_default().push(t);
    }
    let titled = by_album.keys().filter(|k| k.is_some()).count();
    if titled <= 1 {
        return vec![Partition {
            label: None,
            tracks: unit.tracks.iter().collect(),
        }];
    }
    by_album
        .into_values()
        .map(|tracks| {
            let label = match most_common(tracks.iter().filter_map(|t| t.info.album.as_deref())) {
                Some(album) => format!("Album {}", quote(&album)),
                None => "Tracks without an album tag".to_owned(),
            };
            Partition {
                label: Some(label),
                tracks,
            }
        })
        .collect()
}

/// Group text values by normalised form. Values: every original spelling, in input order.
pub(super) fn group_normalized<'a>(
    values: impl IntoIterator<Item = &'a str>,
) -> BTreeMap<String, Vec<&'a str>> {
    let mut groups: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for v in values {
        groups.entry(normalize_for_compare(v)).or_default().push(v);
    }
    groups
}

/// One original spelling (the most common) per normalised value, sorted.
pub(super) fn representative_spellings(groups: &BTreeMap<String, Vec<&str>>) -> Vec<String> {
    let mut out: Vec<String> = groups
        .values()
        .filter_map(|spellings| most_common(spellings.iter().copied()))
        .collect();
    out.sort();
    out
}

pub(super) fn quote(s: &str) -> String {
    format!("'{s}'")
}

/// `'a', 'b', 'c'` (at most [`MAX_LISTED`], then "and N more").
pub(super) fn quoted_list(values: &[String]) -> String {
    let mut parts: Vec<String> = values.iter().take(MAX_LISTED).map(|v| quote(v)).collect();
    if values.len() > MAX_LISTED {
        parts.push(format!("and {} more", values.len() - MAX_LISTED));
    }
    parts.join(", ")
}

/// Compact number list with ranges: `1-4, 7, 9-10`.
pub(super) fn number_ranges(numbers: &BTreeSet<u32>) -> String {
    let mut parts = Vec::new();
    let mut iter = numbers.iter().copied().peekable();
    while let Some(start) = iter.next() {
        let mut end = start;
        while end < u32::MAX && iter.peek() == Some(&(end + 1)) {
            end += 1;
            iter.next();
        }
        parts.push(match end - start {
            0 => start.to_string(),
            1 => format!("{start}, {end}"),
            _ => format!("{start}-{end}"),
        });
    }
    parts.join(", ")
}

/// `1 file` / `3 files`.
pub(super) fn count_noun(n: usize, singular: &str, plural: &str) -> String {
    if n == 1 {
        format!("1 {singular}")
    } else {
        format!("{n} {plural}")
    }
}

/// Last path component, for file-scope messages.
pub(super) fn file_name(rel_path: &str) -> &str {
    rel_path.rsplit('/').next().unwrap_or(rel_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_are_compact() {
        let set: BTreeSet<u32> = [1, 2, 3, 4, 7, 9, 10].into_iter().collect();
        assert_eq!(number_ranges(&set), "1-4, 7, 9, 10");
        assert_eq!(number_ranges(&BTreeSet::new()), "");
        let edge: BTreeSet<u32> = [u32::MAX - 1, u32::MAX].into_iter().collect();
        assert_eq!(
            number_ranges(&edge),
            format!("{}, {}", u32::MAX - 1, u32::MAX)
        );
    }

    #[test]
    fn quoted_list_truncates() {
        let v: Vec<String> = (1..=7).map(|i| i.to_string()).collect();
        assert_eq!(quoted_list(&v), "'1', '2', '3', '4', '5', and 2 more");
    }
}
