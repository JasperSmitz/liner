//! File-scope rules: one issue per affected file.

use serde_json::json;

use crate::group::AlbumUnit;
use crate::model::Issue;

use super::codes;
use super::util::{Details, details, file_name, issue, quote};

pub(super) fn check(unit: &AlbumUnit, out: &mut Vec<Issue>) {
    for t in &unit.tracks {
        let info = &t.info;
        let name = quote(file_name(&info.rel_path));
        let mut emit = |code: &str, message: String, details: Details| {
            out.push(issue(
                code,
                message,
                Some(&unit.id),
                vec![info.rel_path.clone()],
                details,
            ));
        };

        if info.album.is_none() {
            emit(
                codes::MISSING_ALBUM,
                format!("{name} has no album tag"),
                Details::new(),
            );
        }
        if info.artist.is_none() && info.album_artist.is_none() {
            emit(
                codes::MISSING_ARTIST,
                format!("{name} has neither an artist nor an album artist tag"),
                Details::new(),
            );
        }
        if info.title.is_none() {
            emit(
                codes::MISSING_TITLE,
                format!("{name} has no title tag"),
                Details::new(),
            );
        }
        match (info.track_no, info.track_total) {
            (None, _) => emit(
                codes::MISSING_TRACK_NUMBER,
                format!("{name} has no track number"),
                Details::new(),
            ),
            (Some(no), Some(total)) if no > total => emit(
                codes::TRACK_NUMBER_EXCEEDS_TOTAL,
                format!("{name}: track number {no} exceeds track total {total}"),
                details([("track_no", json!(no)), ("track_total", json!(total))]),
            ),
            _ => {}
        }
        if let (Some(no), Some(total)) = (info.disc_no, info.disc_total)
            && no > total
        {
            emit(
                codes::DISC_NUMBER_EXCEEDS_TOTAL,
                format!("{name}: disc number {no} exceeds disc total {total}"),
                details([("disc_no", json!(no)), ("disc_total", json!(total))]),
            );
        }
    }
}
