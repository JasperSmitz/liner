//! M2: album grouping (ADR 0002) with hand-built `TrackInfo`s.

use liner::group::{AlbumUnit, group_tracks, parse_disc_folder_name};
use liner::model::{AudioFormat, TrackInfo};

fn t(rel: &str) -> TrackInfo {
    TrackInfo::new(format!("/lib/{rel}"), rel, AudioFormat::Flac)
}

fn unit<'a>(units: &'a [AlbumUnit], id: &str) -> &'a AlbumUnit {
    units
        .iter()
        .find(|u| u.id == id)
        .unwrap_or_else(|| panic!("no unit {id}"))
}

#[test]
fn disc_folder_name_table() {
    let positives = [
        ("CD1", 1),
        ("cd 2", 2),
        ("CD-03", 3),
        ("Disc 1", 1),
        ("DISK02", 2),
        ("Disc 1 - The Beginning", 1),
        ("disc_2", 2),
        ("CD 2", 2),
        ("Disk.4", 4),
        ("cd999", 999),
        ("CD1 (Bonus)", 1),
        ("Disc\u{00A0}7", 7),
    ];
    for (name, n) in positives {
        assert_eq!(parse_disc_folder_name(name), Some(n), "{name:?}");
    }
    let negatives = [
        "CD",
        "CDs",
        "Discography",
        "CD0",
        "Disco 2",
        "1 CD",
        "Disc",
        "CD000",
        "CD1000",
        "CD1A",
        "CD 2x",
        " CD1",
        "Bonus",
        "",
        "Diśc 1",
        "Extras",
    ];
    for name in negatives {
        assert_eq!(parse_disc_folder_name(name), None, "{name:?}");
    }
}

#[test]
fn disc_folders_merge_into_parent_one_level_only() {
    let units = group_tracks(vec![
        t("A/CD1/01.flac"),
        t("A/CD2/01.flac"),
        t("A/CD1/Extra/01.flac"),
        t("CD1/01.flac"),
        t("root.flac"),
        t("B/01.flac"),
    ]);
    let ids: Vec<&str> = units.iter().map(|u| u.id.as_str()).collect();
    assert_eq!(ids, vec![".", "A", "A/CD1/Extra", "B"]);

    let a = unit(&units, "A");
    assert_eq!(a.tracks.len(), 2);
    assert!(a.is_multi_disc);
    assert_eq!(a.tracks[0].disc_folder.as_deref(), Some("A/CD1"));
    assert_eq!(a.tracks[0].folder_disc, Some(1));
    assert_eq!(a.tracks[1].folder_disc, Some(2));

    let root = unit(&units, ".");
    assert_eq!(root.tracks.len(), 2);
    let in_cd1 = root
        .tracks
        .iter()
        .find(|t| t.info.rel_path == "CD1/01.flac")
        .unwrap();
    assert_eq!(in_cd1.disc_folder.as_deref(), Some("CD1"));
    assert_eq!(in_cd1.folder_disc, Some(1));

    let extra = unit(&units, "A/CD1/Extra");
    assert_eq!(extra.tracks[0].folder_disc, None);
    assert!(!extra.is_multi_disc);
}

#[test]
fn effective_disc_tag_then_folder_then_single_disc_default() {
    let units = group_tracks(vec![
        TrackInfo {
            disc_no: Some(2),
            ..t("A/CD1/01.flac")
        },
        t("A/CD2/01.flac"),
        // Single-disc unit without any disc info: effective disc 1.
        TrackInfo {
            track_no: Some(1),
            ..t("S/01.flac")
        },
        // Multi-disc (disc total) without disc tags or folders: unknown.
        TrackInfo {
            disc_total: Some(2),
            ..t("M/01.flac")
        },
    ]);
    let a = unit(&units, "A");
    let by_path = |p: &str| a.tracks.iter().find(|t| t.info.rel_path == p).unwrap();
    assert_eq!(by_path("A/CD1/01.flac").effective_disc, Some(2));
    assert_eq!(by_path("A/CD2/01.flac").effective_disc, Some(2));
    assert_eq!(a.disc_count, 1);

    let s = unit(&units, "S");
    assert!(!s.is_multi_disc);
    assert_eq!(s.tracks[0].effective_disc, Some(1));
    assert_eq!(s.disc_count, 1);

    let m = unit(&units, "M");
    assert!(m.is_multi_disc);
    assert_eq!(m.tracks[0].effective_disc, None);
    assert_eq!(m.disc_count, 0);
}

#[test]
fn multi_disc_detection() {
    let units = group_tracks(vec![
        // disc_no > 1
        TrackInfo {
            disc_no: Some(2),
            ..t("A/01.flac")
        },
        // disc_total > 1
        TrackInfo {
            disc_no: Some(1),
            disc_total: Some(2),
            ..t("B/01.flac")
        },
        // 1/1 is single-disc
        TrackInfo {
            disc_no: Some(1),
            disc_total: Some(1),
            ..t("C/01.flac")
        },
        // a single disc folder is not multi-disc on its own
        t("D/CD1/01.flac"),
        t("D/CD1/02.flac"),
    ]);
    assert!(unit(&units, "A").is_multi_disc);
    assert!(unit(&units, "B").is_multi_disc);
    assert!(!unit(&units, "C").is_multi_disc);
    let d = unit(&units, "D");
    assert!(!d.is_multi_disc);
    assert!(d.tracks.iter().all(|t| t.effective_disc == Some(1)));
}

#[test]
fn tracks_sorted_by_disc_track_path_with_none_last() {
    let units = group_tracks(vec![
        TrackInfo {
            disc_total: Some(2),
            track_no: Some(1),
            ..t("A/z-unknown.flac")
        },
        TrackInfo {
            disc_no: Some(2),
            track_no: Some(1),
            ..t("A/d2t1.flac")
        },
        TrackInfo {
            disc_no: Some(1),
            ..t("A/d1-none.flac")
        },
        TrackInfo {
            disc_no: Some(1),
            track_no: Some(2),
            ..t("A/d1t2.flac")
        },
        TrackInfo {
            disc_no: Some(1),
            track_no: Some(2),
            ..t("A/d1t2-b.flac")
        },
        TrackInfo {
            disc_no: Some(1),
            track_no: Some(1),
            ..t("A/d1t1.flac")
        },
    ]);
    let order: Vec<&str> = units[0]
        .tracks
        .iter()
        .map(|t| t.info.rel_path.as_str())
        .collect();
    assert_eq!(
        order,
        vec![
            "A/d1t1.flac",
            "A/d1t2-b.flac",
            "A/d1t2.flac",
            "A/d1-none.flac",
            "A/d2t1.flac",
            "A/z-unknown.flac",
        ]
    );
    assert_eq!(units[0].disc_count, 2);
}

#[test]
fn title_and_album_artist_consensus() {
    let units = group_tracks(vec![
        TrackInfo {
            album: Some("Beta".into()),
            album_artist: Some("Y".into()),
            ..t("A/1.flac")
        },
        TrackInfo {
            album: Some("Alpha".into()),
            album_artist: Some("X".into()),
            ..t("A/2.flac")
        },
        TrackInfo {
            album: Some("Beta".into()),
            ..t("A/3.flac")
        },
        TrackInfo {
            album: Some("Alpha".into()),
            ..t("A/4.flac")
        },
        TrackInfo {
            album: Some("Alpha".into()),
            ..t("A/5.flac")
        },
        t("B/1.flac"),
    ]);
    let a = unit(&units, "A");
    assert_eq!(a.title.as_deref(), Some("Alpha"));
    // Tie between X and Y: lexicographically smallest.
    assert_eq!(a.album_artist.as_deref(), Some("X"));
    let b = unit(&units, "B");
    assert_eq!(b.title, None);
    assert_eq!(b.album_artist, None);
}

#[test]
fn album_key_rules() {
    let units = group_tracks(vec![
        // Album artist wins, normalised.
        TrackInfo {
            album: Some("  The  Wall ".into()),
            album_artist: Some("PINK Floyd".into()),
            artist: Some("Someone".into()),
            ..t("A/1.flac")
        },
        // Shared artist (one track without artist is ignored).
        TrackInfo {
            album: Some("Album".into()),
            artist: Some("Solo".into()),
            ..t("B/1.flac")
        },
        TrackInfo {
            album: Some("Album".into()),
            artist: Some("solo".into()),
            ..t("B/2.flac")
        },
        TrackInfo {
            album: Some("Album".into()),
            ..t("B/3.flac")
        },
        // Ambiguous artist: no key.
        TrackInfo {
            album: Some("Comp".into()),
            artist: Some("A".into()),
            ..t("C/1.flac")
        },
        TrackInfo {
            album: Some("Comp".into()),
            artist: Some("B".into()),
            ..t("C/2.flac")
        },
        // No album: no key.
        TrackInfo {
            artist: Some("A".into()),
            ..t("D/1.flac")
        },
        // No artist at all: no key.
        TrackInfo {
            album: Some("X".into()),
            ..t("E/1.flac")
        },
    ]);
    assert_eq!(
        unit(&units, "A").album_key(),
        Some(("pink floyd".into(), "the wall".into()))
    );
    assert_eq!(
        unit(&units, "B").album_key(),
        Some(("solo".into(), "album".into()))
    );
    assert_eq!(unit(&units, "C").album_key(), None);
    assert_eq!(unit(&units, "D").album_key(), None);
    assert_eq!(unit(&units, "E").album_key(), None);
}

#[test]
fn grouping_is_independent_of_input_order() {
    let tracks = vec![
        TrackInfo {
            track_no: Some(2),
            ..t("A/CD2/02.flac")
        },
        TrackInfo {
            track_no: Some(1),
            ..t("A/CD1/01.flac")
        },
        TrackInfo {
            track_no: Some(1),
            ..t("B/01.flac")
        },
        TrackInfo {
            track_no: Some(1),
            ..t("A/CD2/01.flac")
        },
    ];
    let mut reversed = tracks.clone();
    reversed.reverse();
    assert_eq!(group_tracks(tracks), group_tracks(reversed));
    assert!(group_tracks(Vec::new()).is_empty());
}
