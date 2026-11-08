//! M2: lint rules over hand-built `TrackInfo`s (no file reading).

use std::collections::BTreeSet;

use liner::group::group_tracks;
use liner::model::{AudioFormat, Issue, Scope, TrackInfo};
use liner::rules::{CATALOG, codes, run_all};
use serde_json::json;

// ---------------------------------------------------------------------------
// Builders

/// A fully and cleanly tagged track (single-disc, no disc tags).
fn clean(rel: &str, track_no: u32, track_total: u32) -> TrackInfo {
    TrackInfo {
        title: Some(format!("Song {track_no}")),
        artist: Some("Artist".into()),
        album: Some("Album".into()),
        album_artist: Some("Artist".into()),
        track_no: Some(track_no),
        track_total: Some(track_total),
        year: Some(2000),
        ..TrackInfo::new(format!("/lib/{rel}"), rel, AudioFormat::Flac)
    }
}

/// Clean single-disc album `dir` with tracks `1..=n` (total `n`).
fn album(dir: &str, n: u32) -> Vec<TrackInfo> {
    (1..=n)
        .map(|i| clean(&format!("{dir}/{i:02}.flac"), i, n))
        .collect()
}

/// Clean disc `disc` of `total_discs`, `n` tracks, in `folder` (e.g. `A/CD1` or `A`).
fn disc(folder: &str, disc: u32, total_discs: u32, n: u32) -> Vec<TrackInfo> {
    (1..=n)
        .map(|i| TrackInfo {
            title: Some(format!("Song {disc}-{i}")),
            disc_no: Some(disc),
            disc_total: Some(total_discs),
            ..clean(&format!("{folder}/{disc}-{i:02}.flac"), i, n)
        })
        .collect()
}

/// CD1 with tracks 1-3 (1-2 of 2, track 3 of 3), CD2 with tracks 1-2 of 3. Moving
/// CD1's track 3 to disc 2 by tag leaves both discs complete by tags, so only the
/// folder mismatch remains.
fn scenario_d_base() -> Vec<TrackInfo> {
    let cd1 = disc("A/CD1", 1, 2, 3).into_iter().map(|t| TrackInfo {
        track_total: if t.track_no == Some(3) {
            Some(3)
        } else {
            Some(2)
        },
        ..t
    });
    let cd2 = disc("A/CD2", 2, 2, 2).into_iter().map(|t| TrackInfo {
        track_total: Some(3),
        ..t
    });
    cd1.chain(cd2).collect()
}

/// Strip disc tags (keeps file names).
fn no_disc_tags(tracks: Vec<TrackInfo>) -> Vec<TrackInfo> {
    tracks
        .into_iter()
        .map(|t| TrackInfo {
            disc_no: None,
            disc_total: None,
            ..t
        })
        .collect()
}

fn concat(parts: Vec<Vec<TrackInfo>>) -> Vec<TrackInfo> {
    parts.into_iter().flatten().collect()
}

/// Apply `f` to the track with `rel_path`.
fn edit(mut tracks: Vec<TrackInfo>, rel: &str, f: impl FnOnce(&mut TrackInfo)) -> Vec<TrackInfo> {
    let t = tracks
        .iter_mut()
        .find(|t| t.rel_path == rel)
        .unwrap_or_else(|| panic!("no track {rel}"));
    f(t);
    tracks
}

fn run(tracks: Vec<TrackInfo>) -> Vec<Issue> {
    run_all(&group_tracks(tracks))
}

fn codes_of(issues: &[Issue]) -> BTreeSet<&str> {
    issues.iter().map(|i| i.code.as_str()).collect()
}

fn only(issues: &[Issue], code: &str) -> Vec<Issue> {
    issues.iter().filter(|i| i.code == code).cloned().collect()
}

fn one(issues: &[Issue], code: &str) -> Issue {
    let found = only(issues, code);
    assert_eq!(found.len(), 1, "expected one {code}, got {issues:#?}");
    found.into_iter().next().unwrap()
}

fn assert_codes(issues: &[Issue], expected: &[&str]) {
    let expected: BTreeSet<&str> = expected.iter().copied().collect();
    assert_eq!(codes_of(issues), expected, "issues: {issues:#?}");
}

// ---------------------------------------------------------------------------
// Scenario tables: one positive and one near-miss negative per code (at least).

/// (code that must be emitted, library)
fn positives() -> Vec<(&'static str, Vec<TrackInfo>)> {
    vec![
        (
            codes::MISSING_ALBUM,
            edit(album("A", 3), "A/02.flac", |t| t.album = None),
        ),
        (
            codes::MISSING_ARTIST,
            edit(album("A", 3), "A/02.flac", |t| {
                t.artist = None;
                t.album_artist = None;
            }),
        ),
        (
            codes::MISSING_TITLE,
            edit(album("A", 3), "A/02.flac", |t| t.title = None),
        ),
        (
            codes::MISSING_TRACK_NUMBER,
            edit(album("A", 3), "A/02.flac", |t| t.track_no = None),
        ),
        (
            codes::TRACK_NUMBER_EXCEEDS_TOTAL,
            edit(album("A", 3), "A/03.flac", |t| t.track_total = Some(2)),
        ),
        (
            codes::DISC_NUMBER_EXCEEDS_TOTAL,
            edit(
                concat(vec![disc("A/CD1", 1, 2, 2), disc("A/CD2", 2, 2, 2)]),
                "A/CD2/2-01.flac",
                |t| t.disc_total = Some(1),
            ),
        ),
        (
            codes::DUPLICATE_TRACK_POSITION,
            edit(album("A", 4), "A/04.flac", |t| t.track_no = Some(3)),
        ),
        (
            codes::LIKELY_MISSING_DISC_NUMBERS,
            concat(vec![album("A", 10), {
                (1..=10)
                    .map(|i| TrackInfo {
                        title: Some(format!("Other {i}")),
                        ..clean(&format!("A/b{i:02}.flac"), i, 10)
                    })
                    .collect()
            }]),
        ),
        (
            codes::MISSING_DISC_NUMBER,
            no_disc_tags(concat(vec![disc("A/CD1", 1, 2, 3), disc("A/CD2", 2, 2, 3)])),
        ),
        (
            codes::DISC_FOLDER_MISMATCH,
            edit(scenario_d_base(), "A/CD1/1-03.flac", |t| {
                t.disc_no = Some(2)
            }),
        ),
        (
            codes::TRACK_NUMBER_GAP,
            album("A", 5)
                .into_iter()
                .filter(|t| t.track_no != Some(3))
                .collect(),
        ),
        (
            codes::DISC_NUMBER_GAP,
            concat(vec![disc("A", 1, 3, 2), disc("A", 3, 3, 2)]),
        ),
        (
            codes::INCONSISTENT_TRACK_TOTAL,
            edit(album("A", 3), "A/02.flac", |t| t.track_total = Some(4)),
        ),
        (
            codes::INCONSISTENT_DISC_TOTAL,
            edit(
                concat(vec![disc("A", 1, 2, 2), disc("A", 2, 2, 2)]),
                "A/2-01.flac",
                |t| t.disc_total = Some(3),
            ),
        ),
        (
            codes::MIXED_ALBUM_DIRECTORY,
            concat(vec![
                album("A", 2),
                (1..=2)
                    .map(|i| TrackInfo {
                        album: Some("Other".into()),
                        ..clean(&format!("A/o{i}.flac"), i, 2)
                    })
                    .collect(),
            ]),
        ),
        (
            codes::INCONSISTENT_TAG_SPELLING,
            edit(album("A", 3), "A/02.flac", |t| {
                t.album = Some("ALBUM".into())
            }),
        ),
        (
            codes::PARTIAL_ALBUM_ARTIST,
            edit(album("A", 3), "A/02.flac", |t| t.album_artist = None),
        ),
        (codes::MISSING_ALBUM_ARTIST, compilation("A", None)),
        (
            codes::INCONSISTENT_YEAR,
            edit(album("A", 3), "A/02.flac", |t| t.year = Some(2001)),
        ),
        (
            codes::SPLIT_ALBUM,
            concat(vec![album("A", 2), album("B", 2)]),
        ),
    ]
}

/// (code that must NOT be emitted, near-miss library)
fn negatives() -> Vec<(&'static str, Vec<TrackInfo>)> {
    vec![
        // Album artist alone is enough for the artist rule; album present.
        (
            codes::MISSING_ARTIST,
            edit(album("A", 3), "A/02.flac", |t| t.artist = None),
        ),
        (
            codes::MISSING_ALBUM,
            edit(album("A", 3), "A/02.flac", |t| t.album_artist = None),
        ),
        (
            codes::MISSING_TITLE,
            edit(album("A", 3), "A/02.flac", |t| t.title = Some(" x ".into())),
        ),
        (
            codes::MISSING_TRACK_NUMBER,
            edit(album("A", 3), "A/02.flac", |t| t.track_total = None),
        ),
        // Equal to the total is fine.
        (codes::TRACK_NUMBER_EXCEEDS_TOTAL, album("A", 3)),
        (
            codes::DISC_NUMBER_EXCEEDS_TOTAL,
            concat(vec![disc("A/CD1", 1, 2, 2), disc("A/CD2", 2, 2, 2)]),
        ),
        // Same track number on different discs.
        (
            codes::DUPLICATE_TRACK_POSITION,
            concat(vec![disc("A", 1, 2, 3), disc("A", 2, 2, 3)]),
        ),
        // Repeats, but disc tags exist (then it is a duplicate position instead).
        (
            codes::LIKELY_MISSING_DISC_NUMBERS,
            edit(
                concat(vec![disc("A", 1, 2, 3), disc("A", 2, 2, 3)]),
                "A/2-01.flac",
                |t| t.disc_no = Some(1),
            ),
        ),
        // The same album in two formats in one folder: copies, not merged discs.
        (
            codes::LIKELY_MISSING_DISC_NUMBERS,
            concat(vec![
                album("A", 4),
                album("A", 4)
                    .into_iter()
                    .map(|t| TrackInfo {
                        rel_path: t.rel_path.replace(".flac", ".mp3"),
                        format: AudioFormat::Mp3,
                        ..t
                    })
                    .collect(),
            ]),
        ),
        // A single copy of track 1 without titles (`01 (1).flac`): same file stem.
        (
            codes::LIKELY_MISSING_DISC_NUMBERS,
            concat(vec![album("A", 5), vec![clean("A/01 (1).flac", 1, 5)]])
                .into_iter()
                .map(|t| TrackInfo { title: None, ..t })
                .collect(),
        ),
        // Only track 1 repeats with distinct tracks: one repeat is not a disc signature.
        (
            codes::LIKELY_MISSING_DISC_NUMBERS,
            concat(vec![
                album("A", 5),
                vec![TrackInfo {
                    title: Some("Bonus".into()),
                    ..clean("A/bonus.flac", 1, 5)
                }],
            ]),
        ),
        // One duplicated file (track 1 not repeated) is a duplicate, not merged discs.
        (
            codes::LIKELY_MISSING_DISC_NUMBERS,
            edit(album("A", 4), "A/04.flac", |t| t.track_no = Some(3)),
        ),
        // Folder hints but every track tagged.
        (
            codes::MISSING_DISC_NUMBER,
            concat(vec![disc("A/CD1", 1, 2, 3), disc("A/CD2", 2, 2, 3)]),
        ),
        // Single-disc album without disc tags.
        (codes::MISSING_DISC_NUMBER, album("A", 3)),
        (
            codes::DISC_FOLDER_MISMATCH,
            concat(vec![disc("A/CD1", 1, 2, 3), disc("A/CD2", 2, 2, 3)]),
        ),
        // Missing tracks beyond the highest number without a total are not visible.
        (
            codes::TRACK_NUMBER_GAP,
            album("A", 5)
                .into_iter()
                .filter(|t| t.track_no != Some(5))
                .map(|t| TrackInfo {
                    track_total: None,
                    ..t
                })
                .collect(),
        ),
        // Continuous numbering across discs is not a gap.
        (
            codes::TRACK_NUMBER_GAP,
            concat(vec![
                disc("A", 1, 2, 3),
                (4..=6)
                    .map(|i| TrackInfo {
                        disc_no: Some(2),
                        disc_total: Some(2),
                        ..clean(&format!("A/2-{i:02}.flac"), i, 3)
                    })
                    .map(|t| TrackInfo {
                        track_total: None,
                        ..t
                    })
                    .collect(),
            ]),
        ),
        (
            codes::DISC_NUMBER_GAP,
            concat(vec![disc("A", 1, 2, 2), disc("A", 2, 2, 2)]),
        ),
        // Missing totals do not count as a distinct value.
        (
            codes::INCONSISTENT_TRACK_TOTAL,
            edit(album("A", 3), "A/02.flac", |t| t.track_total = None),
        ),
        // Different totals on different discs are fine.
        (
            codes::INCONSISTENT_TRACK_TOTAL,
            concat(vec![disc("A", 1, 2, 3), disc("A", 2, 2, 4)]),
        ),
        (
            codes::INCONSISTENT_DISC_TOTAL,
            edit(
                concat(vec![disc("A", 1, 2, 2), disc("A", 2, 2, 2)]),
                "A/2-01.flac",
                |t| t.disc_total = None,
            ),
        ),
        // Case/whitespace variants are spelling issues, not mixed albums.
        (
            codes::MIXED_ALBUM_DIRECTORY,
            edit(album("A", 3), "A/02.flac", |t| {
                t.album = Some(" album ".into())
            }),
        ),
        (
            codes::INCONSISTENT_TAG_SPELLING,
            concat(vec![
                album("A", 2),
                (1..=2)
                    .map(|i| TrackInfo {
                        album: Some("Other".into()),
                        ..clean(&format!("A/o{i}.flac"), i, 2)
                    })
                    .collect(),
            ]),
        ),
        (
            codes::PARTIAL_ALBUM_ARTIST,
            album("A", 3)
                .into_iter()
                .map(|t| TrackInfo {
                    album_artist: None,
                    ..t
                })
                .collect(),
        ),
        (
            codes::MISSING_ALBUM_ARTIST,
            compilation("A", Some("Various Artists")),
        ),
        // Artist differing only in case is the same artist.
        (
            codes::MISSING_ALBUM_ARTIST,
            edit(
                album("A", 3)
                    .into_iter()
                    .map(|t| TrackInfo {
                        album_artist: None,
                        ..t
                    })
                    .collect(),
                "A/02.flac",
                |t| t.artist = Some("ARTIST".into()),
            ),
        ),
        (
            codes::INCONSISTENT_YEAR,
            edit(album("A", 3), "A/02.flac", |t| t.year = None),
        ),
        // Same album title, different artist.
        (
            codes::SPLIT_ALBUM,
            concat(vec![
                album("A", 2),
                album("B", 2)
                    .into_iter()
                    .map(|t| TrackInfo {
                        artist: Some("Other".into()),
                        album_artist: Some("Other".into()),
                        ..t
                    })
                    .collect(),
            ]),
        ),
        // Disc folders of one album are merged, not split.
        (
            codes::SPLIT_ALBUM,
            concat(vec![disc("A/CD1", 1, 2, 2), disc("A/CD2", 2, 2, 2)]),
        ),
    ]
}

/// Compilation in `dir`: 4 tracks with different artists.
fn compilation(dir: &str, album_artist: Option<&str>) -> Vec<TrackInfo> {
    (1..=4)
        .map(|i| TrackInfo {
            artist: Some(format!("Artist {i}")),
            album: Some("Hits".into()),
            album_artist: album_artist.map(str::to_owned),
            compilation: true,
            ..clean(&format!("{dir}/{i:02}.flac"), i, 4)
        })
        .collect()
}

/// Every scenario in this file, for the catalogue coverage test.
fn all_scenarios() -> Vec<Vec<TrackInfo>> {
    let mut out: Vec<Vec<TrackInfo>> = positives().into_iter().map(|(_, t)| t).collect();
    out.extend(negatives().into_iter().map(|(_, t)| t));
    out.extend(multi_disc_scenarios().into_iter().map(|(_, t, _)| t));
    out
}

#[test]
fn positive_scenarios_emit_their_code() {
    for (code, tracks) in positives() {
        let issues = run(tracks);
        assert!(
            issues.iter().any(|i| i.code == code),
            "{code} not emitted: {issues:#?}"
        );
    }
}

#[test]
fn negative_scenarios_do_not_emit_their_code() {
    for (code, tracks) in negatives() {
        let issues = run(tracks);
        assert!(
            !issues.iter().any(|i| i.code == code),
            "{code} emitted for a near miss: {issues:#?}"
        );
    }
}

#[test]
fn every_code_has_a_positive_and_a_negative_scenario() {
    let pos: BTreeSet<&str> = positives().into_iter().map(|(c, _)| c).collect();
    let neg: BTreeSet<&str> = negatives().into_iter().map(|(c, _)| c).collect();
    for r in CATALOG {
        assert!(pos.contains(r.code), "no positive scenario for {}", r.code);
        assert!(neg.contains(r.code), "no negative scenario for {}", r.code);
    }
}

#[test]
fn every_catalog_code_is_emitted_by_some_scenario() {
    let mut emitted = BTreeSet::new();
    for tracks in all_scenarios() {
        for issue in run(tracks) {
            emitted.insert(issue.code);
        }
    }
    for r in CATALOG {
        assert!(emitted.contains(r.code), "{} never emitted", r.code);
    }
}

#[test]
fn issues_use_catalog_severity_scope_and_sorted_paths() {
    for tracks in all_scenarios() {
        let issues = run(tracks);
        let mut sorted = issues.clone();
        sorted.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
        assert_eq!(issues, sorted, "run_all output not sorted");
        for i in &issues {
            let r = liner::rules::rule_info(&i.code).expect("code in catalog");
            assert_eq!((i.severity, i.scope), (r.severity, r.scope), "{}", i.code);
            assert!(!i.paths.is_empty(), "{} has no paths", i.code);
            assert!(i.paths.windows(2).all(|w| w[0] < w[1]), "{}", i.code);
            assert_eq!(i.album_id.is_none(), i.scope == Scope::Library);
            assert!(!i.message.contains('\n'), "{}", i.message);
        }
    }
}

// ---------------------------------------------------------------------------
// Multi-disc acceptance scenarios (exact issue sets).

fn multi_disc_scenarios() -> Vec<(&'static str, Vec<TrackInfo>, Vec<&'static str>)> {
    vec![
        (
            "(a) CD1/CD2 folders, no disc tags",
            no_disc_tags(concat(vec![disc("A/CD1", 1, 2, 4), disc("A/CD2", 2, 2, 3)])),
            vec![codes::MISSING_DISC_NUMBER],
        ),
        (
            "(b) flat folder, tracks 1-10 twice, no disc tags",
            no_disc_tags(concat(vec![disc("A", 1, 2, 10), disc("A", 2, 2, 10)])),
            vec![codes::LIKELY_MISSING_DISC_NUMBERS],
        ),
        (
            "(b') flat folder, discs of different length, no disc tags",
            no_disc_tags(concat(vec![disc("A", 1, 2, 12), disc("A", 2, 2, 8)])),
            vec![codes::LIKELY_MISSING_DISC_NUMBERS],
        ),
        (
            "(c) disc tags 1 and 3",
            concat(vec![disc("A", 1, 0, 3), disc("A", 3, 0, 3)])
                .into_iter()
                .map(|t| TrackInfo {
                    disc_total: None,
                    ..t
                })
                .collect(),
            vec![codes::DISC_NUMBER_GAP],
        ),
        (
            "(d) file tagged disc 2 in CD1",
            edit(scenario_d_base(), "A/CD1/1-03.flac", |t| {
                t.disc_no = Some(2)
            }),
            vec![codes::DISC_FOLDER_MISMATCH],
        ),
        (
            "(e) clean 2-disc album in disc folders",
            concat(vec![disc("A/CD1", 1, 2, 4), disc("A/Disc 2", 2, 2, 5)]),
            vec![],
        ),
        (
            "(e') clean 2-disc album, flat",
            concat(vec![disc("A", 1, 2, 4), disc("A", 2, 2, 5)]),
            vec![],
        ),
        (
            "clean 2-disc album with continuous numbering",
            concat(vec![
                disc("A", 1, 2, 3)
                    .into_iter()
                    .map(|t| TrackInfo {
                        track_total: Some(6),
                        ..t
                    })
                    .collect(),
                (4..=6)
                    .map(|i| TrackInfo {
                        disc_no: Some(2),
                        disc_total: Some(2),
                        ..clean(&format!("A/2-{i:02}.flac"), i, 6)
                    })
                    .collect(),
            ]),
            vec![],
        ),
        (
            "only disc 2 tagged in disc folders",
            edit(
                no_disc_tags(concat(vec![disc("A/CD1", 1, 2, 2), disc("A/CD2", 2, 2, 2)])),
                "A/CD2/2-01.flac",
                |t| t.disc_no = Some(2),
            ),
            vec![codes::MISSING_DISC_NUMBER],
        ),
    ]
}

#[test]
fn multi_disc_scenarios_produce_exactly_the_expected_codes() {
    for (name, tracks, expected) in multi_disc_scenarios() {
        let issues = run(tracks);
        let expected: BTreeSet<&str> = expected.into_iter().collect();
        assert_eq!(codes_of(&issues), expected, "{name}: {issues:#?}");
    }
}

#[test]
fn scenario_a_details() {
    let issues = run(no_disc_tags(concat(vec![
        disc("A/CD1", 1, 2, 4),
        disc("A/CD2", 2, 2, 3),
    ])));
    let i = one(&issues, codes::MISSING_DISC_NUMBER);
    assert_eq!(i.album_id.as_deref(), Some("A"));
    assert_eq!(i.paths.len(), 7);
    assert_eq!(i.details["count"], json!(7));
    assert_eq!(
        i.message,
        "7 of 7 tracks in this multi-disc album have no disc number tag"
    );
}

#[test]
fn scenario_b_details() {
    let issues = run(no_disc_tags(concat(vec![
        disc("A", 1, 2, 10),
        disc("A", 2, 2, 10),
    ])));
    assert_codes(&issues, &[codes::LIKELY_MISSING_DISC_NUMBERS]);
    let i = &issues[0];
    assert_eq!(i.paths.len(), 20);
    assert_eq!(
        i.details["duplicated_tracks"],
        json!((1..=10).collect::<Vec<u32>>())
    );
    assert!(
        i.message
            .contains("Track numbers 1-10 occur more than once"),
        "{}",
        i.message
    );
}

#[test]
fn scenario_c_details() {
    let issues = run(concat(vec![disc("A", 1, 3, 2), disc("A", 3, 3, 2)]));
    let i = one(&issues, codes::DISC_NUMBER_GAP);
    assert_eq!(i.details["missing"], json!([2]));
    assert_eq!(i.details["expected_total"], json!(3));
    assert_eq!(i.message, "Missing disc 2 (found discs 1, 3, disc total 3)");
}

#[test]
fn disc_number_gap_uses_agreed_disc_total() {
    let issues = run(concat(vec![disc("A", 1, 4, 2), disc("A", 2, 4, 2)]));
    let i = one(&issues, codes::DISC_NUMBER_GAP);
    assert_eq!(i.details["missing"], json!([3, 4]));
    assert_eq!(i.details["expected_total"], json!(4));
    assert_eq!(i.paths.len(), 4);

    // Disagreeing totals: only the highest found disc counts -> no gap.
    let issues = run(edit(
        concat(vec![disc("A", 1, 4, 2), disc("A", 2, 4, 2)]),
        "A/2-01.flac",
        |t| t.disc_total = Some(5),
    ));
    assert!(
        only(&issues, codes::DISC_NUMBER_GAP).is_empty(),
        "{issues:#?}"
    );
    assert_eq!(
        one(&issues, codes::INCONSISTENT_DISC_TOTAL).details["values"],
        json!([4, 5])
    );
}

#[test]
fn scenario_d_details() {
    let issues = run(edit(scenario_d_base(), "A/CD1/1-03.flac", |t| {
        t.disc_no = Some(2)
    }));
    let i = one(&issues, codes::DISC_FOLDER_MISMATCH);
    assert_eq!(i.paths, vec!["A/CD1/1-03.flac"]);
    assert_eq!(i.details["tag_disc"], json!(2));
    assert_eq!(i.details["folder_disc"], json!(1));
    assert_eq!(i.details["folder"], json!("A/CD1"));
    assert_eq!(
        i.message,
        "1 file in folder 'A/CD1' (disc 1) is tagged disc 2"
    );
}

#[test]
fn disc_folder_mismatch_one_issue_per_folder_and_tag_disc() {
    let mut tracks = concat(vec![disc("A/CD1", 1, 3, 4), disc("A/CD2", 2, 3, 4)]);
    for (rel, d) in [
        ("A/CD1/1-01.flac", 2),
        ("A/CD1/1-02.flac", 2),
        ("A/CD1/1-03.flac", 3),
        ("A/CD2/2-01.flac", 1),
    ] {
        tracks = edit(tracks, rel, |t| t.disc_no = Some(d));
    }
    let issues = only(&run(tracks), codes::DISC_FOLDER_MISMATCH);
    let combos: Vec<(String, u64, usize)> = issues
        .iter()
        .map(|i| {
            (
                i.details["folder"].as_str().unwrap().to_owned(),
                i.details["tag_disc"].as_u64().unwrap(),
                i.paths.len(),
            )
        })
        .collect();
    assert_eq!(
        combos,
        vec![
            ("A/CD1".into(), 2, 2),
            ("A/CD1".into(), 3, 1),
            ("A/CD2".into(), 1, 1),
        ]
    );
    assert!(
        issues[0]
            .message
            .contains("2 files in folder 'A/CD1' (disc 1) are tagged disc 2")
    );
}

// ---------------------------------------------------------------------------
// Compilations.

#[test]
fn compilation_with_various_artists_is_clean() {
    assert_codes(&run(compilation("Comp", Some("Various Artists"))), &[]);
}

#[test]
fn compilation_without_album_artist_only_missing_album_artist() {
    let issues = run(compilation("Comp", None));
    assert_codes(&issues, &[codes::MISSING_ALBUM_ARTIST]);
    let i = &issues[0];
    assert_eq!(
        i.details["artists"],
        json!(["Artist 1", "Artist 2", "Artist 3", "Artist 4"])
    );
    assert_eq!(i.paths.len(), 4);
    assert_eq!(
        i.message,
        "No album artist tag, but the tracks have 4 different artists: 'Artist 1', 'Artist 2', 'Artist 3', 'Artist 4'"
    );
}

// ---------------------------------------------------------------------------
// Details and messages per rule.

#[test]
fn clean_albums_have_no_issues() {
    assert_codes(
        &run(concat(vec![album("A", 12), album("B/C", 1)])
            .into_iter()
            .map(|mut t| {
                if t.rel_path.starts_with("B/") {
                    t.album = Some("Single".into());
                }
                t
            })
            .collect()),
        &[],
    );
    assert!(run(Vec::new()).is_empty());
}

#[test]
fn file_rules_details_and_messages() {
    let issues = run(edit(album("A", 3), "A/03.flac", |t| {
        t.track_no = Some(5);
        t.disc_no = Some(3);
        t.disc_total = Some(2);
    }));
    let i = one(&issues, codes::TRACK_NUMBER_EXCEEDS_TOTAL);
    assert_eq!(i.details["track_no"], json!(5));
    assert_eq!(i.details["track_total"], json!(3));
    assert_eq!(i.paths, vec!["A/03.flac"]);
    assert_eq!(i.message, "'03.flac': track number 5 exceeds track total 3");
    let i = one(&issues, codes::DISC_NUMBER_EXCEEDS_TOTAL);
    assert_eq!(i.details["disc_no"], json!(3));
    assert_eq!(i.details["disc_total"], json!(2));

    let bare = TrackInfo::new("/lib/x.flac", "x.flac", AudioFormat::Flac);
    let issues = run(vec![bare]);
    assert_codes(
        &issues,
        &[
            codes::MISSING_ALBUM,
            codes::MISSING_ARTIST,
            codes::MISSING_TITLE,
            codes::MISSING_TRACK_NUMBER,
        ],
    );
    for i in &issues {
        assert_eq!(i.album_id.as_deref(), Some("."));
        assert!(i.details.is_empty());
        assert!(i.message.starts_with("'x.flac' has "), "{}", i.message);
    }
}

#[test]
fn duplicate_track_position_details() {
    // Multi-disc: names the disc.
    let issues = run(edit(
        concat(vec![disc("A", 1, 2, 3), disc("A", 2, 2, 4)]),
        "A/2-04.flac",
        |t| t.track_no = Some(3),
    ));
    let i = one(&issues, codes::DUPLICATE_TRACK_POSITION);
    assert_eq!(i.details["disc"], json!(2));
    assert_eq!(i.details["track"], json!(3));
    assert_eq!(i.paths, vec!["A/2-03.flac", "A/2-04.flac"]);
    assert_eq!(i.message, "Track 3 on disc 2 appears 2 times");

    // Single-disc: one issue per position.
    let mut tracks = album("A", 6);
    tracks = edit(tracks, "A/05.flac", |t| t.track_no = Some(2));
    tracks = edit(tracks, "A/06.flac", |t| t.track_no = Some(2));
    tracks = edit(tracks, "A/04.flac", |t| t.track_no = Some(3));
    let dups = only(&run(tracks), codes::DUPLICATE_TRACK_POSITION);
    let msgs: Vec<&str> = dups.iter().map(|i| i.message.as_str()).collect();
    assert_eq!(
        msgs,
        vec!["Track 2 appears 3 times", "Track 3 appears 2 times"]
    );
}

#[test]
fn repeats_with_only_disc_totals_are_likely_missing_disc_numbers() {
    // Multi-disc via disc total, no disc tags/folders: effective disc unknown, so
    // no positions; the repeats point at missing disc numbers.
    let tracks = album("A", 2)
        .into_iter()
        .chain(album("A", 2).into_iter().map(|t| TrackInfo {
            rel_path: t.rel_path.replace(".flac", "b.flac"),
            title: t.title.as_ref().map(|s| format!("{s} (disc 2)")),
            ..t
        }))
        .map(|t| TrackInfo {
            disc_total: Some(2),
            ..t
        })
        .collect();
    let issues = run(tracks);
    assert_codes(
        &issues,
        &[
            codes::MISSING_DISC_NUMBER,
            codes::LIKELY_MISSING_DISC_NUMBERS,
        ],
    );
}

#[test]
fn likely_missing_disc_numbers_suppresses_duplicates_and_folder_hints_prevent_it() {
    let flat = no_disc_tags(concat(vec![disc("A", 1, 2, 3), disc("A", 2, 2, 3)]));
    let issues = run(flat);
    assert!(only(&issues, codes::DUPLICATE_TRACK_POSITION).is_empty());
    one(&issues, codes::LIKELY_MISSING_DISC_NUMBERS);

    // A disc folder next to loose files means disc info exists and the unit is
    // multi-disc: the loose files lack a disc number, nothing is merged onto disc 1.
    let hinted = no_disc_tags(concat(vec![disc("A", 1, 2, 3), disc("A/CD1", 2, 2, 3)]));
    let issues = run(hinted);
    assert!(only(&issues, codes::LIKELY_MISSING_DISC_NUMBERS).is_empty());
    assert!(only(&issues, codes::DUPLICATE_TRACK_POSITION).is_empty());
    let i = one(&issues, codes::MISSING_DISC_NUMBER);
    assert_eq!(i.details["count"], json!(6));
}

#[test]
fn track_number_gap_details() {
    // Gap inside the range, agreed total extends the range.
    let tracks: Vec<TrackInfo> = album("A", 10)
        .into_iter()
        .filter(|t| ![3, 5, 6, 7, 10].contains(&t.track_no.unwrap()))
        .collect();
    let i = one(&run(tracks), codes::TRACK_NUMBER_GAP);
    assert_eq!(i.details["disc"], json!(1));
    assert_eq!(i.details["missing"], json!([3, 5, 6, 7, 10]));
    assert_eq!(i.details["expected_total"], json!(10));
    assert_eq!(i.paths.len(), 5);
    assert_eq!(i.message, "Missing tracks 3, 5-7, 10 (track total 10)");

    // Disagreeing totals: null expected_total, range up to highest number.
    let mut tracks: Vec<TrackInfo> = album("A", 4)
        .into_iter()
        .filter(|t| t.track_no != Some(2))
        .collect();
    tracks = edit(tracks, "A/04.flac", |t| t.track_total = Some(9));
    let i = one(&run(tracks), codes::TRACK_NUMBER_GAP);
    assert_eq!(i.details["missing"], json!([2]));
    assert!(i.details["expected_total"].is_null());
    assert_eq!(i.message, "Missing track 2 (highest track 4)");

    // Multi-disc: one issue per disc, message names the disc.
    let tracks: Vec<TrackInfo> = concat(vec![disc("A", 1, 2, 3), disc("A", 2, 2, 3)])
        .into_iter()
        .filter(|t| t.rel_path != "A/2-02.flac")
        .collect();
    let i = one(&run(tracks), codes::TRACK_NUMBER_GAP);
    assert_eq!(i.details["disc"], json!(2));
    assert_eq!(i.message, "Disc 2 is missing track 2 (track total 3)");
}

#[test]
fn track_number_gap_ignores_absurd_ranges_and_numberless_discs() {
    let tracks = edit(album("A", 3), "A/03.flac", |t| {
        t.track_no = Some(2003);
        t.track_total = None;
    });
    let issues = run(tracks);
    assert!(only(&issues, codes::TRACK_NUMBER_GAP).is_empty());

    let tracks = album("A", 3)
        .into_iter()
        .map(|t| TrackInfo {
            track_no: None,
            ..t
        })
        .collect();
    let issues = run(tracks);
    assert!(only(&issues, codes::TRACK_NUMBER_GAP).is_empty());
    assert_eq!(only(&issues, codes::MISSING_TRACK_NUMBER).len(), 3);
}

#[test]
fn inconsistent_totals_details() {
    let issues = run(edit(
        concat(vec![disc("A", 1, 2, 3), disc("A", 2, 2, 3)]),
        "A/2-02.flac",
        |t| t.track_total = Some(4),
    ));
    let i = one(&issues, codes::INCONSISTENT_TRACK_TOTAL);
    assert_eq!(i.details["disc"], json!(2));
    assert_eq!(i.details["values"], json!([3, 4]));
    assert_eq!(i.paths.len(), 3);
    assert_eq!(
        i.message,
        "Tracks on disc 2 disagree on the track total: 3, 4"
    );

    let issues = run(edit(
        concat(vec![disc("A", 1, 2, 2), disc("A", 2, 2, 2)]),
        "A/2-01.flac",
        |t| t.disc_total = Some(3),
    ));
    let i = one(&issues, codes::INCONSISTENT_DISC_TOTAL);
    assert_eq!(i.details["values"], json!([2, 3]));
    assert_eq!(i.message, "Tracks disagree on the disc total: 2, 3");
}

#[test]
fn mixed_album_directory_details() {
    let mut tracks = album("A", 2);
    tracks.extend((1..=2).map(|i| TrackInfo {
        album: Some("Other".into()),
        album_artist: Some("Someone".into()),
        ..clean(&format!("A/o{i}.flac"), i, 2)
    }));
    tracks = edit(tracks, "A/o2.flac", |t| t.album = Some("OTHER".into()));
    let issues = run(tracks);
    let mixed = only(&issues, codes::MIXED_ALBUM_DIRECTORY);
    assert_eq!(mixed.len(), 2);
    let album = mixed
        .iter()
        .find(|i| i.details["field"] == json!("album"))
        .unwrap();
    // One spelling per normalised value (ties -> smallest).
    assert_eq!(album.details["values"], json!(["Album", "OTHER"]));
    assert_eq!(album.paths.len(), 4);
    assert_eq!(
        album.message,
        "Directory mixes 2 album titles: 'Album', 'OTHER'"
    );
    let aa = mixed
        .iter()
        .find(|i| i.details["field"] == json!("album_artist"))
        .unwrap();
    assert_eq!(aa.details["values"], json!(["Artist", "Someone"]));
    // The case variant is also a spelling issue.
    let s = one(&issues, codes::INCONSISTENT_TAG_SPELLING);
    assert_eq!(s.details["values"], json!(["OTHER", "Other"]));
}

#[test]
fn mixed_directory_does_not_cross_check_numbering_between_albums() {
    // A flat folder of unrelated singles: only the mixed directory is reported
    // (no duplicate track 1, no likely-missing-disc-numbers, no missing album artist).
    let tracks: Vec<TrackInfo> = (1..=4)
        .map(|i| TrackInfo {
            artist: Some(format!("Band {i}")),
            album: Some(format!("Single {i}")),
            album_artist: None,
            year: Some(1990 + i),
            ..clean(&format!("Singles/{i}.flac"), 1, 1)
        })
        .collect();
    let issues = run(tracks);
    assert_codes(&issues, &[codes::MIXED_ALBUM_DIRECTORY]);

    // But a real duplicate inside one of the albums is still found, labelled.
    let mut tracks = album("A", 3);
    tracks.extend((1..=3).map(|i| TrackInfo {
        album: Some("Other".into()),
        ..clean(&format!("A/o{i}.flac"), i, 3)
    }));
    tracks = edit(tracks, "A/o3.flac", |t| t.track_no = Some(2));
    let issues = run(tracks);
    let i = one(&issues, codes::DUPLICATE_TRACK_POSITION);
    assert_eq!(i.paths, vec!["A/o2.flac", "A/o3.flac"]);
    assert_eq!(i.message, "Album 'Other': Track 2 appears 2 times");
}

#[test]
fn spelling_details() {
    let issues = run(edit(
        edit(album("A", 3), "A/02.flac", |t| {
            t.album_artist = Some("artist ".into())
        }),
        "A/03.flac",
        |t| t.album_artist = Some("ARTIST".into()),
    ));
    assert_codes(&issues, &[codes::INCONSISTENT_TAG_SPELLING]);
    let i = &issues[0];
    assert_eq!(i.details["field"], json!("album_artist"));
    assert_eq!(i.details["values"], json!(["ARTIST", "Artist", "artist "]));
    assert_eq!(i.paths.len(), 3);
    assert_eq!(
        i.message,
        "Album artist is spelled 3 ways: 'ARTIST', 'Artist', 'artist '"
    );
}

#[test]
fn partial_album_artist_details() {
    let issues = run(edit(
        edit(album("A", 4), "A/02.flac", |t| t.album_artist = None),
        "A/03.flac",
        |t| t.album_artist = None,
    ));
    assert_codes(&issues, &[codes::PARTIAL_ALBUM_ARTIST]);
    let i = &issues[0];
    assert_eq!(i.paths, vec!["A/02.flac", "A/03.flac"]);
    assert_eq!(i.details["album_artist"], json!("Artist"));
    assert_eq!(
        i.message,
        "2 of 4 tracks have no album artist, the others have 'Artist'"
    );
}

#[test]
fn inconsistent_year_details() {
    let issues = run(edit(
        edit(album("A", 3), "A/02.flac", |t| t.year = Some(1999)),
        "A/03.flac",
        |t| t.year = Some(2003),
    ));
    let i = one(&issues, codes::INCONSISTENT_YEAR);
    assert_eq!(i.details["values"], json!([1999, 2000, 2003]));
    assert_eq!(i.message, "Tracks have 3 different years: 1999, 2000, 2003");
}

#[test]
fn split_album_details() {
    let mut tracks = concat(vec![album("Z/Part 2", 2), album("A/Part 1", 2)]);
    tracks = edit(tracks, "Z/Part 2/01.flac", |t| {
        t.album = Some(" album".into())
    });
    let issues = run(tracks);
    let i = one(&issues, codes::SPLIT_ALBUM);
    assert_eq!(i.album_id, None);
    assert_eq!(i.paths.len(), 4);
    assert_eq!(i.details["album"], json!("Album"));
    assert_eq!(i.details["album_artist"], json!("Artist"));
    assert_eq!(i.details["album_ids"], json!(["A/Part 1", "Z/Part 2"]));
    assert_eq!(
        i.message,
        "Album 'Album' by 'Artist' is split over 2 directories: 'A/Part 1', 'Z/Part 2'"
    );

    // Falls back to the shared track artist when there is no album artist.
    let tracks = concat(vec![album("A", 2), album("B", 2)])
        .into_iter()
        .map(|t| TrackInfo {
            album_artist: None,
            ..t
        })
        .collect();
    let i = one(&run(tracks), codes::SPLIT_ALBUM);
    assert_eq!(i.details["album_artist"], json!("Artist"));
}

#[test]
fn run_all_is_deterministic_regardless_of_input_order() {
    for tracks in all_scenarios() {
        let mut reversed = tracks.clone();
        reversed.reverse();
        assert_eq!(run(tracks), run(reversed));
    }
}

#[test]
fn continuous_numbering_still_reports_real_gaps() {
    // Disc 1: 1-3, disc 2: 4-7 without 6 (album-wide total 7).
    let tracks: Vec<TrackInfo> = (1..=7)
        .filter(|&i| i != 6)
        .map(|i| {
            let d = if i <= 3 { 1 } else { 2 };
            TrackInfo {
                disc_no: Some(d),
                disc_total: Some(2),
                ..clean(&format!("A/{d}-{i:02}.flac"), i, 7)
            }
        })
        .collect();
    let issues = run(tracks);
    assert_codes(&issues, &[codes::TRACK_NUMBER_GAP]);
    let i = &issues[0];
    assert_eq!(i.details["disc"], json!(2));
    assert_eq!(i.details["missing"], json!([6]));
    assert_eq!(i.message, "Disc 2 is missing track 6 (track total 7)");
}

#[test]
fn disc_number_gap_ignores_absurd_disc_numbers() {
    let issues = run(edit(album("A", 2), "A/02.flac", |t| t.disc_no = Some(2003)));
    assert!(
        only(&issues, codes::DISC_NUMBER_GAP).is_empty(),
        "{issues:#?}"
    );
}

#[test]
fn copies_of_the_same_tracks_are_duplicates_not_merged_discs() {
    // FLAC + MP3 of one album in one folder.
    let tracks = concat(vec![
        album("A", 3),
        album("A", 3)
            .into_iter()
            .map(|t| TrackInfo {
                rel_path: t.rel_path.replace(".flac", ".mp3"),
                format: AudioFormat::Mp3,
                ..t
            })
            .collect(),
    ]);
    let issues = run(tracks);
    assert_codes(&issues, &[codes::DUPLICATE_TRACK_POSITION]);
    assert_eq!(only(&issues, codes::DUPLICATE_TRACK_POSITION).len(), 3);

    // A single `01 (1).flac` copy, no titles.
    let tracks: Vec<TrackInfo> = concat(vec![album("A", 5), vec![clean("A/01 (1).flac", 1, 5)]])
        .into_iter()
        .map(|t| TrackInfo { title: None, ..t })
        .collect();
    let issues = run(tracks);
    let i = one(&issues, codes::DUPLICATE_TRACK_POSITION);
    assert_eq!(i.paths, vec!["A/01 (1).flac", "A/01.flac"]);
    assert!(only(&issues, codes::LIKELY_MISSING_DISC_NUMBERS).is_empty());

    // Without titles, distinct file stems still reveal merged discs.
    let tracks: Vec<TrackInfo> = no_disc_tags(concat(vec![disc("A", 1, 2, 3), disc("A", 2, 2, 3)]))
        .into_iter()
        .map(|t| TrackInfo { title: None, ..t })
        .collect();
    let i = one(&run(tracks), codes::LIKELY_MISSING_DISC_NUMBERS);
    assert_eq!(i.details["duplicated_tracks"], json!([1, 2, 3]));
}

/// Track in flat folder `D` with album `album`.
fn flat(rel: &str, album: &str, track_no: u32) -> TrackInfo {
    TrackInfo {
        album: Some(album.into()),
        title: Some(format!("{album} {track_no}")),
        ..clean(rel, track_no, 3)
    }
}

#[test]
fn one_albums_disc_tag_does_not_affect_another_album_in_the_directory() {
    // Review finding 2: Alpha has a real duplicate track 2 and no disc tags; one
    // Beta track is tagged disc 2/2.
    let tracks = vec![
        flat("D/a1.flac", "Alpha", 1),
        flat("D/a2.flac", "Alpha", 2),
        flat("D/a2b.flac", "Alpha", 2),
        flat("D/a3.flac", "Alpha", 3),
        TrackInfo {
            disc_no: Some(2),
            disc_total: Some(2),
            track_total: Some(1),
            ..flat("D/b1.flac", "Beta", 1)
        },
    ];
    let units = group_tracks(tracks.clone());
    let d = &units[0];
    let parts = d.partitions();
    assert_eq!(parts.len(), 2);
    assert!(!parts[0].is_multi_disc, "Alpha");
    assert!(parts[1].is_multi_disc, "Beta");
    for t in &d.tracks {
        let expected = if t.info.rel_path == "D/b1.flac" { 2 } else { 1 };
        assert_eq!(t.effective_disc, Some(expected), "{}", t.info.rel_path);
    }

    let issues = run(tracks);
    let dup = one(&issues, codes::DUPLICATE_TRACK_POSITION);
    assert_eq!(dup.message, "Album 'Alpha': Track 2 appears 2 times");
    assert_eq!(dup.paths, vec!["D/a2.flac", "D/a2b.flac"]);
    let gap = one(&issues, codes::DISC_NUMBER_GAP);
    assert_eq!(
        gap.message,
        "Album 'Beta': Missing disc 1 (found disc 2, disc total 2)"
    );
    assert_eq!(gap.paths, vec!["D/b1.flac"]);
    assert!(
        only(&issues, codes::MISSING_DISC_NUMBER).is_empty(),
        "{issues:#?}"
    );

    // Beta's disc info does not suppress merged-disc detection for Alpha.
    let tracks = vec![
        flat("D/a1.flac", "Alpha", 1),
        flat("D/a2.flac", "Alpha", 2),
        TrackInfo {
            title: Some("Alpha 1 again".into()),
            ..flat("D/a1b.flac", "Alpha", 1)
        },
        TrackInfo {
            title: Some("Alpha 2 again".into()),
            ..flat("D/a2b.flac", "Alpha", 2)
        },
        TrackInfo {
            disc_no: Some(1),
            ..flat("D/b1.flac", "Beta", 1)
        },
    ];
    let i = one(&run(tracks), codes::LIKELY_MISSING_DISC_NUMBERS);
    assert!(i.message.starts_with("Album 'Alpha': "), "{}", i.message);
}

/// Track tagged `album` / `album_artist` in `dir`.
fn tagged(dir: &str, n: u32, album: &str, album_artist: &str) -> TrackInfo {
    TrackInfo {
        album: Some(album.into()),
        album_artist: Some(album_artist.into()),
        artist: Some(album_artist.into()),
        ..clean(&format!("{dir}/{album}-{n:02}.flac"), n, 3)
    }
}

#[test]
fn split_album_keys_come_from_each_partitions_own_consensus() {
    // Codex finding: Mixed holds A/X x3, B/Y x2, C/Y x2. Unit-wide majorities are
    // title A and album artist Y, but no track is A by Y.
    let mut tracks: Vec<TrackInfo> = (1..=3).map(|n| tagged("Mixed", n, "A", "X")).collect();
    tracks.extend((1..=2).map(|n| tagged("Mixed", n, "B", "Y")));
    tracks.extend((1..=2).map(|n| tagged("Mixed", n, "C", "Y")));
    tracks.extend((1..=3).map(|n| tagged("Clean", n, "A", "Y")));
    let issues = run(tracks.clone());
    assert!(only(&issues, codes::SPLIT_ALBUM).is_empty(), "{issues:#?}");

    // A per-partition match is found and lists only that partition's files.
    tracks.extend((1..=2).map(|n| tagged("Other", n, "B", "Y")));
    let i = one(&run(tracks), codes::SPLIT_ALBUM);
    assert_eq!(i.details["album"], json!("B"));
    assert_eq!(i.details["album_artist"], json!("Y"));
    assert_eq!(i.details["album_ids"], json!(["Mixed", "Other"]));
    assert_eq!(
        i.paths,
        vec![
            "Mixed/B-01.flac",
            "Mixed/B-02.flac",
            "Other/B-01.flac",
            "Other/B-02.flac"
        ]
    );
}

#[test]
fn split_album_needs_an_unambiguous_artist_in_the_partition() {
    let no_album_artist = |t: TrackInfo| TrackInfo {
        album_artist: None,
        ..t
    };
    let clean_y: Vec<TrackInfo> = (1..=2).map(|n| tagged("Q", n, "Album", "Y")).collect();

    // No album artist and two track artists in one partition: no key.
    let ambiguous = vec![
        no_album_artist(tagged("P", 1, "Album", "X")),
        no_album_artist(tagged("P", 2, "Album", "Y")),
    ];
    let issues = run(concat(vec![ambiguous, clean_y.clone()]));
    assert!(only(&issues, codes::SPLIT_ALBUM).is_empty(), "{issues:#?}");

    // No album artist but one shared track artist: the track artist is the key.
    let shared: Vec<TrackInfo> = (1..=2)
        .map(|n| no_album_artist(tagged("P", n, "Album", "Y")))
        .collect();
    let i = one(
        &run(concat(vec![shared, clean_y.clone()])),
        codes::SPLIT_ALBUM,
    );
    assert_eq!(i.details["album_ids"], json!(["P", "Q"]));

    // Two album artists in one directory are two partitions (ADR 0002), so the
    // 'Album' by Y in P matches Q, listing only that partition's file.
    let two_artists = vec![tagged("P", 1, "Album", "X"), tagged("P", 2, "Album", "Y")];
    let i = one(&run(concat(vec![two_artists, clean_y])), codes::SPLIT_ALBUM);
    assert_eq!(i.details["album_ids"], json!(["P", "Q"]));
    assert_eq!(
        i.paths,
        vec!["P/Album-02.flac", "Q/Album-01.flac", "Q/Album-02.flac"]
    );
}

#[test]
fn split_album_skips_various_artists_compilations() {
    for va in ["Various Artists", "various", "VA"] {
        let tracks: Vec<TrackInfo> = ["A", "B"]
            .into_iter()
            .flat_map(|dir| (1..=2).map(move |n| tagged(dir, n, "Greatest Hits", va)))
            .collect();
        let issues = run(tracks);
        assert!(
            only(&issues, codes::SPLIT_ALBUM).is_empty(),
            "{va}: {issues:#?}"
        );
    }
    // All tracks flagged as compilations, whatever the album artist.
    let tracks: Vec<TrackInfo> = ["A", "B"]
        .into_iter()
        .flat_map(|dir| (1..=2).map(move |n| tagged(dir, n, "Hits", "DJ Mix")))
        .map(|t| TrackInfo {
            compilation: true,
            ..t
        })
        .collect();
    assert!(only(&run(tracks.clone()), codes::SPLIT_ALBUM).is_empty());
    // One non-compilation track per partition: the key is used again.
    let tracks = edit(tracks, "A/Hits-01.flac", |t| t.compilation = false);
    let tracks = edit(tracks, "B/Hits-01.flac", |t| t.compilation = false);
    one(&run(tracks), codes::SPLIT_ALBUM);
}

#[test]
fn unicode_form_and_quote_differences_are_spelling_not_mixed() {
    for (a, b) in [
        ("Caf\u{00E9}", "Cafe\u{0301}"),
        ("Don\u{2019}t Stop", "Don't Stop"),
        ("\u{201C}Heroes\u{201D}", "\"Heroes\""),
    ] {
        let tracks = edit(
            album("A", 2)
                .into_iter()
                .map(|t| TrackInfo {
                    album: Some(a.into()),
                    ..t
                })
                .collect(),
            "A/02.flac",
            |t| t.album = Some(b.into()),
        );
        let issues = run(tracks);
        assert_codes(&issues, &[codes::INCONSISTENT_TAG_SPELLING]);
    }
    // Split-album also matches across forms.
    let tracks = concat(vec![album("A", 2), album("B", 2)])
        .into_iter()
        .map(|t| TrackInfo {
            album_artist: Some(if t.rel_path.starts_with("A/") {
                "Bjo\u{0308}rk".into()
            } else {
                "Bj\u{00F6}rk".into()
            }),
            ..t
        })
        .collect();
    one(&run(tracks), codes::SPLIT_ALBUM);
}

/// Track `i` on disc `d` of 2, continuous numbering, album-wide total `total`.
fn continuous(d: u32, i: u32, total: u32) -> TrackInfo {
    TrackInfo {
        disc_no: Some(d),
        disc_total: Some(2),
        title: Some(format!("Song {i}")),
        ..clean(&format!("A/{d}-{i:02}.flac"), i, total)
    }
}

#[test]
fn continuous_numbering_reports_a_missing_boundary_track_once() {
    // Last track of disc 1 (12) missing: disc 1 has 1-11, disc 2 has 13-20.
    for total in [None, Some(20)] {
        let tracks: Vec<TrackInfo> = (1..=11)
            .map(|i| continuous(1, i, 20))
            .chain((13..=20).map(|i| continuous(2, i, 20)))
            .map(|t| TrackInfo {
                track_total: total,
                ..t
            })
            .collect();
        let issues = run(tracks);
        let i = one(&issues, codes::TRACK_NUMBER_GAP);
        assert_eq!(i.details["disc"], json!(2), "{total:?}");
        assert_eq!(i.details["missing"], json!([12]), "{total:?}");
        assert!(
            i.message.contains("numbering continues from disc 1"),
            "{}",
            i.message
        );
    }

    // First track of disc 2 (13) missing: disc 1 has 1-12, disc 2 has 14-20.
    let tracks: Vec<TrackInfo> = (1..=12)
        .map(|i| continuous(1, i, 20))
        .chain((14..=20).map(|i| continuous(2, i, 20)))
        .collect();
    let i = one(&run(tracks), codes::TRACK_NUMBER_GAP);
    assert_eq!(i.details["disc"], json!(2));
    assert_eq!(i.details["missing"], json!([13]));
    assert_eq!(
        i.message,
        "Disc 2 is missing track 13 (track total 20, numbering continues from disc 1)"
    );
}

#[test]
fn single_disc_folder_with_number_above_one_is_multi_disc() {
    // Loose 01-03 plus A/CD2/01-02, no disc tags: same meaning as tagging disc 2.
    let tracks = concat(vec![
        album("A", 3),
        (1..=2)
            .map(|i| TrackInfo {
                title: Some(format!("Disc 2 song {i}")),
                ..clean(&format!("A/CD2/{i:02}.flac"), i, 2)
            })
            .collect(),
    ]);
    let units = group_tracks(tracks.clone());
    assert!(units[0].is_multi_disc);
    let issues = run(tracks);
    let i = one(&issues, codes::MISSING_DISC_NUMBER);
    assert_eq!(i.details["count"], json!(5));
    one(&issues, codes::DISC_NUMBER_GAP);
}

#[test]
fn u32_max_numbers_do_not_panic() {
    let max = u32::MAX;
    let tracks: Vec<TrackInfo> = [(1, "A/1.flac"), (max, "A/2.flac"), (max - 1, "A/3.flac")]
        .into_iter()
        .flat_map(|(disc, rel)| {
            [
                TrackInfo {
                    track_no: Some(max),
                    track_total: Some(max),
                    disc_no: Some(disc),
                    disc_total: Some(max),
                    ..clean(rel, 1, 1)
                },
                TrackInfo {
                    track_no: Some(max - 1),
                    track_total: Some(max),
                    disc_no: Some(disc),
                    disc_total: Some(max),
                    ..clean(&rel.replace(".flac", "b.flac"), 1, 1)
                },
            ]
        })
        .collect();
    run(tracks.clone());
    // And with no disc numbers at all.
    run(no_disc_tags(tracks));
}

// ---------------------------------------------------------------------------
// Album partitions by album artist; tracks without an album (final review F1)

fn errors(issues: &[Issue]) -> Vec<&Issue> {
    issues
        .iter()
        .filter(|i| i.severity == liner::model::Severity::Error)
        .collect()
}

#[test]
fn same_title_by_two_album_artists_in_one_folder_is_mixed_only() {
    let by = |artist: &str, n: u32| TrackInfo {
        album: Some("Greatest Hits".into()),
        artist: Some(artist.into()),
        album_artist: Some(artist.into()),
        title: Some(format!("{artist} {n}")),
        ..clean(&format!("Misc/{artist}-{n:02}.flac"), n, 3)
    };
    let tracks: Vec<TrackInfo> = ["Artist A", "Artist B"]
        .into_iter()
        .flat_map(|a| (1..=3).map(move |n| by(a, n)))
        .collect();
    let issues = run(tracks);
    assert_codes(&issues, &[codes::MIXED_ALBUM_DIRECTORY]);
    assert!(errors(&issues).is_empty(), "{issues:#?}");
    let mixed = one(&issues, codes::MIXED_ALBUM_DIRECTORY);
    assert_eq!(mixed.details["field"], json!("album_artist"));
}

#[test]
fn singles_folder_without_album_tags_has_no_position_issues() {
    let single = |name: &str| TrackInfo {
        album: None,
        album_artist: None,
        artist: Some(format!("Artist {name}")),
        title: Some(format!("Song {name}")),
        ..clean(&format!("Singles/{name}.flac"), 1, 1)
    };
    let issues = run(vec![single("a"), single("b"), single("c")]);
    assert!(errors(&issues).is_empty(), "{issues:#?}");
    for code in [
        codes::DUPLICATE_TRACK_POSITION,
        codes::LIKELY_MISSING_DISC_NUMBERS,
        codes::TRACK_NUMBER_GAP,
        codes::INCONSISTENT_TRACK_TOTAL,
        codes::MISSING_ALBUM_ARTIST,
        codes::PARTIAL_ALBUM_ARTIST,
        codes::INCONSISTENT_YEAR,
    ] {
        assert!(only(&issues, code).is_empty(), "{code}: {issues:#?}");
    }
    // File-level rules still apply.
    assert_eq!(only(&issues, codes::MISSING_ALBUM).len(), 3);

    // Untagged tracks next to two albums: their numbers are not compared either.
    let mut tracks = vec![single("a"), single("b")];
    tracks.extend(album("Singles", 2).into_iter().map(|t| TrackInfo {
        album: Some("One".into()),
        ..t
    }));
    tracks.extend(album("Singles", 2).into_iter().map(|t| TrackInfo {
        album: Some("Two".into()),
        rel_path: t.rel_path.replace(".flac", "-two.flac"),
        ..t
    }));
    let issues = run(tracks);
    assert!(errors(&issues).is_empty(), "{issues:#?}");
}

#[cfg(unix)]
#[test]
fn directories_differing_only_in_invalid_bytes_do_not_share_positions() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let track = |dir: &[u8]| {
        let mut rel = dir.to_vec();
        rel.extend_from_slice(b"/02.flac");
        TrackInfo {
            path: std::path::Path::new("/lib").join(OsStr::from_bytes(&rel)),
            rel_path: String::from_utf8_lossy(&rel).into_owned(),
            ..clean("unused/02.flac", 2, 10)
        }
    };
    let units = group_tracks(vec![track(b"Album\xfe"), track(b"Album\xff")]);
    assert_eq!(units.len(), 2);
    let issues = run_all(&units);
    assert!(
        only(&issues, codes::DUPLICATE_TRACK_POSITION).is_empty(),
        "{issues:#?}"
    );
    let ids: BTreeSet<&str> = issues
        .iter()
        .filter_map(|i| i.album_id.as_deref())
        .collect();
    assert_eq!(ids, ["Album\u{FFFD}", "Album\u{FFFD} #2"].into());
}
