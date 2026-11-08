//! Library-scope rules.

use std::collections::BTreeMap;

use serde_json::json;

use crate::group::{AlbumUnit, most_common};
use crate::model::Issue;

use super::codes;
use super::util::{details, issue, paths, quote, quoted_list};

pub(super) fn check(albums: &[AlbumUnit], out: &mut Vec<Issue>) {
    split_album(albums, out);
}

fn split_album(albums: &[AlbumUnit], out: &mut Vec<Issue>) {
    let mut by_key: BTreeMap<(String, String), Vec<&AlbumUnit>> = BTreeMap::new();
    for unit in albums {
        if let Some(key) = unit.album_key() {
            by_key.entry(key).or_default().push(unit);
        }
    }
    for mut units in by_key.into_values().filter(|u| u.len() > 1) {
        units.sort_by(|a, b| a.id.cmp(&b.id));
        let first = units[0];
        // album_key() is Some, so the title and an artist exist.
        let album = first.title.clone().unwrap_or_default();
        let album_artist = first
            .album_artist
            .clone()
            .or_else(|| most_common(first.tracks.iter().filter_map(|t| t.info.artist.as_deref())))
            .unwrap_or_default();
        let album_ids: Vec<String> = units.iter().map(|u| u.id.clone()).collect();
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
            paths(units.iter().flat_map(|u| &u.tracks)),
            details([
                ("album", json!(album)),
                ("album_artist", json!(album_artist)),
                ("album_ids", json!(album_ids)),
            ]),
        ));
    }
}
