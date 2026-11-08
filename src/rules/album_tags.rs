//! Album-scope rules about album-level tags.
//!
//! `mixed-album-directory` and `inconsistent-tag-spelling` look at the whole unit;
//! the album-artist and year rules run per album partition (see
//! [`AlbumUnit::partitions`]).

use std::collections::BTreeSet;

use serde_json::json;

use crate::group::{AlbumPartition, AlbumUnit, GroupedTrack, most_common};
use crate::model::Issue;

use super::codes;
use super::util::{
    PartitionMessage, count_noun, details, group_normalized, issue, paths, quote, quoted_list,
    representative_spellings,
};

pub(super) fn check(unit: &AlbumUnit, out: &mut Vec<Issue>) {
    for field in [Field::Album, Field::AlbumArtist] {
        mixed_album_directory(unit, field, out);
        inconsistent_tag_spelling(unit, field, out);
    }
    for part in unit.partitions() {
        partial_album_artist(unit, &part, out);
        missing_album_artist(unit, &part, out);
        inconsistent_year(unit, &part, out);
    }
}

#[derive(Clone, Copy)]
enum Field {
    Album,
    AlbumArtist,
}

impl Field {
    fn key(self) -> &'static str {
        match self {
            Field::Album => "album",
            Field::AlbumArtist => "album_artist",
        }
    }

    fn get(self, t: &GroupedTrack) -> Option<&str> {
        match self {
            Field::Album => t.info.album.as_deref(),
            Field::AlbumArtist => t.info.album_artist.as_deref(),
        }
    }

    fn singular(self) -> &'static str {
        match self {
            Field::Album => "album title",
            Field::AlbumArtist => "album artist",
        }
    }

    fn capitalized(self) -> &'static str {
        match self {
            Field::Album => "Album title",
            Field::AlbumArtist => "Album artist",
        }
    }
}

fn mixed_album_directory(unit: &AlbumUnit, field: Field, out: &mut Vec<Issue>) {
    let groups = group_normalized(unit.tracks.iter().filter_map(|t| field.get(t)));
    if groups.len() < 2 {
        return;
    }
    let values = representative_spellings(&groups);
    let message = format!(
        "Directory mixes {} {}s: {}",
        values.len(),
        field.singular(),
        quoted_list(&values)
    );
    out.push(issue(
        codes::MIXED_ALBUM_DIRECTORY,
        message,
        Some(&unit.id),
        paths(unit.tracks.iter().filter(|t| field.get(t).is_some())),
        details([("field", json!(field.key())), ("values", json!(values))]),
    ));
}

fn inconsistent_tag_spelling(unit: &AlbumUnit, field: Field, out: &mut Vec<Issue>) {
    let groups = group_normalized(unit.tracks.iter().filter_map(|t| field.get(t)));
    for spellings in groups.values() {
        let distinct: BTreeSet<&str> = spellings.iter().copied().collect();
        if distinct.len() < 2 {
            continue;
        }
        let values: Vec<String> = distinct.iter().map(|s| (*s).to_owned()).collect();
        let message = format!(
            "{} is spelled {} ways: {}",
            field.capitalized(),
            values.len(),
            quoted_list(&values)
        );
        out.push(issue(
            codes::INCONSISTENT_TAG_SPELLING,
            message,
            Some(&unit.id),
            paths(
                unit.tracks
                    .iter()
                    .filter(|t| field.get(t).is_some_and(|v| distinct.contains(v))),
            ),
            details([("field", json!(field.key())), ("values", json!(values))]),
        ));
    }
}

fn partial_album_artist(unit: &AlbumUnit, part: &AlbumPartition<'_>, out: &mut Vec<Issue>) {
    let (with, without): (Vec<&GroupedTrack>, Vec<&GroupedTrack>) = part
        .tracks
        .iter()
        .partition(|t| t.info.album_artist.is_some());
    let Some(value) = most_common(with.iter().filter_map(|t| t.info.album_artist.as_deref()))
    else {
        return;
    };
    if without.is_empty() {
        return;
    }
    let message = part.message(format!(
        "{} of {} {} no album artist, the others have {}",
        without.len(),
        count_noun(part.tracks.len(), "track", "tracks"),
        if without.len() == 1 { "has" } else { "have" },
        quote(&value)
    ));
    out.push(issue(
        codes::PARTIAL_ALBUM_ARTIST,
        message,
        Some(&unit.id),
        paths(without),
        details([("album_artist", json!(value))]),
    ));
}

fn missing_album_artist(unit: &AlbumUnit, part: &AlbumPartition<'_>, out: &mut Vec<Issue>) {
    if part.tracks.iter().any(|t| t.info.album_artist.is_some()) {
        return;
    }
    let groups = group_normalized(part.tracks.iter().filter_map(|t| t.info.artist.as_deref()));
    if groups.len() < 2 {
        return;
    }
    let artists = representative_spellings(&groups);
    let message = part.message(format!(
        "No album artist tag, but the tracks have {} different artists: {}",
        artists.len(),
        quoted_list(&artists)
    ));
    out.push(issue(
        codes::MISSING_ALBUM_ARTIST,
        message,
        Some(&unit.id),
        paths(part.tracks.iter().copied()),
        details([("artists", json!(artists))]),
    ));
}

fn inconsistent_year(unit: &AlbumUnit, part: &AlbumPartition<'_>, out: &mut Vec<Issue>) {
    let with_year: Vec<&GroupedTrack> = part
        .tracks
        .iter()
        .copied()
        .filter(|t| t.info.year.is_some())
        .collect();
    let values: BTreeSet<u32> = with_year.iter().filter_map(|t| t.info.year).collect();
    if values.len() < 2 {
        return;
    }
    let listed: Vec<String> = values.iter().map(u32::to_string).collect();
    let message = part.message(format!(
        "Tracks have {} different years: {}",
        values.len(),
        listed.join(", ")
    ));
    out.push(issue(
        codes::INCONSISTENT_YEAR,
        message,
        Some(&unit.id),
        paths(with_year),
        details([("values", json!(values))]),
    ));
}
