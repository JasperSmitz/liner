//! Library-scope rules.

use std::collections::BTreeMap;

use serde_json::json;

use crate::group::{AlbumPartition, AlbumUnit};
use crate::model::Issue;

use super::codes;
use super::util::{details, issue, paths, quote, quoted_list};

pub(super) fn check(albums: &[AlbumUnit], out: &mut Vec<Issue>) {
    split_album(albums, out);
}

/// One album partition that has a split-album key.
struct Keyed<'a> {
    unit: &'a AlbumUnit,
    part: AlbumPartition<'a>,
}

fn split_album(albums: &[AlbumUnit], out: &mut Vec<Issue>) {
    let mut by_key: BTreeMap<(String, String), Vec<Keyed<'_>>> = BTreeMap::new();
    for unit in albums {
        for part in unit.partitions() {
            if let Some(key) = part.album_key() {
                by_key.entry(key).or_default().push(Keyed { unit, part });
            }
        }
    }
    // Partitions of one unit have different normalised titles, so each unit occurs
    // at most once per key.
    for ((artist_key, _), mut found) in by_key.into_iter().filter(|(_, f)| f.len() > 1) {
        found.sort_by(|a, b| a.unit.id.cmp(&b.unit.id));
        let first = &found[0].part;
        // album_key() is Some, so the title and an artist exist.
        let album = first.title.clone().unwrap_or_default();
        let album_artist = first.key_artist_spelling(&artist_key).unwrap_or_default();
        let album_ids: Vec<String> = found.iter().map(|k| k.unit.id.clone()).collect();
        let message = format!(
            "Album {} by {} is split over {} directories: {}",
            quote(&album),
            quote(&album_artist),
            album_ids.len(),
            quoted_list(&album_ids)
        );
        out.push(issue(
            codes::SPLIT_ALBUM,
            message,
            None,
            paths(found.iter().flat_map(|k| k.part.tracks.iter().copied())),
            details([
                ("album", json!(album)),
                ("album_artist", json!(album_artist)),
                ("album_ids", json!(album_ids)),
            ]),
        ));
    }
}
