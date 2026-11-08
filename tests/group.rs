//! M2: album grouping (ADR 0002) with hand-built `TrackInfo`s.

use std::collections::BTreeSet;

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
        ("CD1(Bonus)", 1),
        ("Disc 2[Live]", 2),
        ("cd3[", 3),
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
        "CD(1)",
        "\u{FF23}\u{FF24}1",
        "CD\u{FF11}",
        "CD1\u{FF11}",
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
        // a single disc folder 1 is not multi-disc on its own
        t("D/CD1/01.flac"),
        t("D/CD1/02.flac"),
        // a single disc folder > 1 is
        t("E/CD2/01.flac"),
        // a disc folder next to loose files is
        t("F/01.flac"),
        t("F/CD1/01.flac"),
    ]);
    assert!(unit(&units, "A").is_multi_disc);
    assert!(unit(&units, "B").is_multi_disc);
    assert!(!unit(&units, "C").is_multi_disc);
    let d = unit(&units, "D");
    assert!(!d.is_multi_disc);
    assert!(d.tracks.iter().all(|t| t.effective_disc == Some(1)));
    let e = unit(&units, "E");
    assert!(e.is_multi_disc);
    assert_eq!(e.tracks[0].effective_disc, Some(2));
    let f = unit(&units, "F");
    assert!(f.is_multi_disc);
    let eff: Vec<Option<u32>> = f.tracks.iter().map(|t| t.effective_disc).collect();
    assert_eq!(eff, vec![Some(1), None]);
}

#[test]
fn multi_disc_agrees_with_disc_count_for_folder_layouts() {
    let units = group_tracks(vec![
        // Review finding 3: loose files plus one CD2 folder, no tags.
        t("A/01.flac"),
        t("A/02.flac"),
        t("A/CD2/01.flac"),
        // Two disc folders.
        t("B/CD1/01.flac"),
        t("B/CD2/01.flac"),
        // One CD1 folder only.
        t("C/CD1/01.flac"),
        // Plain folder.
        t("D/01.flac"),
        // Tag-only multi-disc.
        TrackInfo {
            disc_no: Some(1),
            ..t("E/01.flac")
        },
        TrackInfo {
            disc_no: Some(2),
            ..t("E/02.flac")
        },
    ]);
    for id in ["B", "C", "D", "E"] {
        let u = unit(&units, id);
        assert_eq!(u.is_multi_disc, u.disc_count > 1, "{id}");
    }
    // The loose files' disc is unknown (as when CD2's files carry a disc-2 tag
    // instead), so only one disc is known although the unit is multi-disc.
    let a = unit(&units, "A");
    assert!(a.is_multi_disc);
    assert_eq!(a.disc_count, 1);
}

#[test]
fn disc_count_above_one_always_means_multi_disc() {
    // The converse can fail when only one disc is known (disc 2 alone).
    let layouts = [
        vec![t("A/CD3/01.flac")],
        vec![
            TrackInfo {
                disc_no: Some(2),
                ..t("A/01.flac")
            },
            t("A/02.flac"),
        ],
        vec![t("A/01.flac"), t("A/Disc 1/01.flac")],
        vec![
            TrackInfo {
                album: Some("X".into()),
                disc_no: Some(2),
                ..t("A/01.flac")
            },
            TrackInfo {
                album: Some("Y".into()),
                ..t("A/02.flac")
            },
        ],
    ];
    for tracks in layouts {
        for u in group_tracks(tracks) {
            assert!(u.disc_count <= 1 || u.is_multi_disc, "{u:#?}");
        }
    }
}

#[test]
fn partitions_decide_multi_disc_independently() {
    let units = group_tracks(vec![
        TrackInfo {
            album: Some("Alpha".into()),
            ..t("D/a1.flac")
        },
        TrackInfo {
            album: Some("Beta".into()),
            disc_no: Some(2),
            ..t("D/b1.flac")
        },
        TrackInfo {
            album: Some("beta".into()),
            ..t("D/b2.flac")
        },
        t("D/none.flac"),
    ]);
    let d = unit(&units, "D");
    assert!(d.is_multi_disc);
    let parts = d.partitions();
    let labels: Vec<Option<&str>> = parts.iter().map(|p| p.label.as_deref()).collect();
    assert_eq!(
        labels,
        vec![
            Some("Tracks without an album tag"),
            Some("Album 'Alpha'"),
            Some("Album 'Beta'")
        ]
    );
    let multi: Vec<bool> = parts.iter().map(|p| p.is_multi_disc).collect();
    assert_eq!(multi, vec![false, false, true]);
    let eff = |p: &str| {
        d.tracks
            .iter()
            .find(|t| t.info.rel_path == p)
            .unwrap()
            .effective_disc
    };
    assert_eq!(eff("D/a1.flac"), Some(1));
    assert_eq!(eff("D/none.flac"), Some(1));
    assert_eq!(eff("D/b1.flac"), Some(2));
    assert_eq!(eff("D/b2.flac"), None);

    // A unit with one album title (plus untagged tracks) is one unlabelled partition.
    let units = group_tracks(vec![
        TrackInfo {
            album: Some("Alpha".into()),
            ..t("E/1.flac")
        },
        t("E/2.flac"),
    ]);
    let parts = units[0].partitions();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].label, None);
    assert_eq!(parts[0].tracks.len(), 2);
}

#[test]
fn album_keys_are_per_partition() {
    // Codex finding: crossed majorities. Unit-wide most common title is A and album
    // artist Y, but no track is A by Y.
    let mut tracks = Vec::new();
    for (i, (album, aa)) in [
        ("A", "X"),
        ("A", "X"),
        ("A", "X"),
        ("B", "Y"),
        ("B", "Y"),
        ("C", "Y"),
        ("C", "Y"),
    ]
    .into_iter()
    .enumerate()
    {
        tracks.push(TrackInfo {
            album: Some(album.into()),
            album_artist: Some(aa.into()),
            ..t(&format!("Mixed/{i}.flac"))
        });
    }
    let units = group_tracks(tracks);
    let mixed = unit(&units, "Mixed");
    assert_eq!(mixed.album_key(), None);
    let keys: Vec<Option<(String, String)>> =
        mixed.partitions().iter().map(|p| p.album_key()).collect();
    assert_eq!(
        keys,
        vec![
            Some(("x".into(), "a".into())),
            Some(("y".into(), "b".into())),
            Some(("y".into(), "c".into())),
        ]
    );
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
fn album_key_skips_various_artists_and_compilations() {
    let mk = |rel: &str, aa: &str, compilation: bool| TrackInfo {
        album: Some("Greatest Hits".into()),
        album_artist: Some(aa.into()),
        compilation,
        ..t(rel)
    };
    let units = group_tracks(vec![
        mk("A/1.flac", "Various Artists", false),
        mk("B/1.flac", " VA ", false),
        mk("C/1.flac", "VARIOUS", false),
        mk("D/1.flac", "Queen", true),
        mk("D/2.flac", "Queen", true),
        mk("E/1.flac", "Queen", true),
        mk("E/2.flac", "Queen", false),
    ]);
    for id in ["A", "B", "C", "D"] {
        assert_eq!(unit(&units, id).album_key(), None, "{id}");
    }
    assert_eq!(
        unit(&units, "E").album_key(),
        Some(("queen".into(), "greatest hits".into()))
    );
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

// ---------------------------------------------------------------------------
// Album partitions keyed by (album, album artist)

fn tagged(rel: &str, album: Option<&str>, album_artist: Option<&str>, artist: &str) -> TrackInfo {
    TrackInfo {
        album: album.map(Into::into),
        album_artist: album_artist.map(Into::into),
        artist: Some(artist.into()),
        ..t(rel)
    }
}

fn labels(u: &AlbumUnit) -> Vec<Option<String>> {
    u.partitions().into_iter().map(|p| p.label).collect()
}

#[test]
fn same_title_by_different_album_artists_is_two_partitions() {
    let mut tracks: Vec<TrackInfo> = (1..=3)
        .map(|n| {
            tagged(
                &format!("Misc/a{n}.flac"),
                Some("Greatest Hits"),
                Some("Artist A"),
                "Artist A",
            )
        })
        .collect();
    tracks.extend((1..=3).map(|n| {
        tagged(
            &format!("Misc/b{n}.flac"),
            Some("Greatest Hits"),
            Some("Artist B"),
            "Artist B",
        )
    }));
    let units = group_tracks(tracks);
    let parts = unit(&units, "Misc").partitions();
    assert_eq!(parts.len(), 2);
    assert!(
        parts
            .iter()
            .all(|p| p.tracks.len() == 3 && !p.is_untitled())
    );
    assert_eq!(
        labels(unit(&units, "Misc")),
        [
            Some("Album 'Greatest Hits' by 'Artist A'".to_owned()),
            Some("Album 'Greatest Hits' by 'Artist B'".to_owned()),
        ]
    );
    // A track without an album artist falls back to its track artist.
    let units = group_tracks(vec![
        tagged("M/1.flac", Some("Hits"), Some("A"), "A"),
        tagged("M/2.flac", Some("Hits"), None, "A"),
        tagged("M/3.flac", Some("Hits"), Some("B"), "B"),
    ]);
    let sizes: Vec<usize> = unit(&units, "M")
        .partitions()
        .iter()
        .map(|p| p.tracks.len())
        .collect();
    assert_eq!(sizes, [2, 1]);
}

#[test]
fn compilations_stay_one_partition() {
    // One album artist, many track artists.
    let va: Vec<TrackInfo> = ["X", "Y", "Z"]
        .iter()
        .enumerate()
        .map(|(i, a)| {
            tagged(
                &format!("VA/{i}.flac"),
                Some("Hits"),
                Some("Various Artists"),
                a,
            )
        })
        .collect();
    // No album artist at all, many track artists.
    let bare: Vec<TrackInfo> = ["X", "Y", "Z"]
        .iter()
        .enumerate()
        .map(|(i, a)| tagged(&format!("Bare/{i}.flac"), Some("Hits"), None, a))
        .collect();
    let units = group_tracks(va.into_iter().chain(bare).collect());
    for id in ["VA", "Bare"] {
        assert_eq!(labels(unit(&units, id)), [None], "{id}");
    }
}

#[test]
fn tracks_without_album_form_an_untitled_partition() {
    let units = group_tracks(vec![
        tagged("Singles/a.flac", None, None, "A"),
        tagged("Singles/b.flac", None, None, "B"),
        tagged("Mixed/a.flac", None, None, "A"),
        tagged("Mixed/b.flac", Some("One"), None, "B"),
        tagged("Mixed/c.flac", Some("Two"), None, "C"),
    ]);
    let singles = unit(&units, "Singles").partitions();
    assert_eq!(singles.len(), 1);
    assert!(singles[0].is_untitled());
    let mixed = unit(&units, "Mixed").partitions();
    assert_eq!(mixed.len(), 3);
    assert!(mixed[0].is_untitled());
    assert_eq!(
        mixed[0].label.as_deref(),
        Some("Tracks without an album tag")
    );
}

// ---------------------------------------------------------------------------
// Identity by the real (possibly non-UTF-8) directory path

#[cfg(unix)]
fn raw(rel: &[u8]) -> TrackInfo {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let rel_os = std::path::Path::new(OsStr::from_bytes(rel));
    let rel_lossy = String::from_utf8_lossy(rel).into_owned();
    TrackInfo {
        track_no: Some(2),
        ..TrackInfo::new(
            std::path::Path::new("/lib").join(rel_os),
            rel_lossy,
            AudioFormat::Flac,
        )
    }
}

#[cfg(unix)]
#[test]
fn directories_differing_only_in_invalid_bytes_are_separate_units() {
    let units = group_tracks(vec![raw(b"Album\xff/02.flac"), raw(b"Album\xfe/02.flac")]);
    let ids: Vec<&str> = units.iter().map(|u| u.id.as_str()).collect();
    // Raw byte order: \xfe keeps the plain id.
    assert_eq!(ids, ["Album\u{FFFD}", "Album\u{FFFD} #2"]);
    assert!(units.iter().all(|u| u.tracks.len() == 1));
    assert_eq!(units[0].tracks[0].info.path, raw(b"Album\xfe/02.flac").path);
}

#[cfg(unix)]
#[test]
fn disambiguated_ids_skip_existing_names() {
    let units = group_tracks(vec![
        raw(b"Album\xfe/02.flac"),
        raw(b"Album\xff/02.flac"),
        raw("Album\u{FFFD} #2/02.flac".as_bytes()),
    ]);
    let ids: BTreeSet<&str> = units.iter().map(|u| u.id.as_str()).collect();
    assert_eq!(
        ids,
        ["Album\u{FFFD}", "Album\u{FFFD} #2", "Album\u{FFFD} #3"].into()
    );
    let real = units.iter().find(|u| u.id == "Album\u{FFFD} #2").unwrap();
    assert_eq!(real.tracks[0].info.rel_path, "Album\u{FFFD} #2/02.flac");
}

#[cfg(unix)]
#[test]
fn disc_folders_differing_only_in_invalid_bytes_are_two_discs() {
    let units = group_tracks(vec![
        raw(b"A/CD 1 \xfe/02.flac"),
        raw(b"A/CD 1 \xff/02.flac"),
    ]);
    assert_eq!(units.len(), 1);
    let a = unit(&units, "A");
    assert!(a.is_multi_disc);
    let folders: BTreeSet<&str> = a
        .tracks
        .iter()
        .filter_map(|t| t.disc_folder.as_deref())
        .collect();
    assert_eq!(folders, ["A/CD 1 \u{FFFD}", "A/CD 1 \u{FFFD} #2"].into());
}
