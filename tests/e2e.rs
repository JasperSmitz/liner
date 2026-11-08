//! End-to-end tests: synthetic libraries (real audio template copies with written
//! tags, mixed formats) scanned through the `liner` binary, checked in both output
//! formats. Together the scenarios emit every code in `rules::CATALOG`
//! (`scenarios_cover_every_catalog_code`).

mod common;

use std::collections::{BTreeMap, BTreeSet};

use assert_cmd::Command;
use common::{Corrupt, Fixture, Library, Tags};
use serde_json::{Value, json};
use unicode_normalization::UnicodeNormalization;

/// Key used in expectations for issues with `album_id: null` (library scope).
const LIBRARY: &str = "<library>";

fn liner() -> Command {
    let mut cmd = Command::cargo_bin("liner").unwrap();
    // Never pick up the developer's real config (and thus never their library).
    cmd.env("XDG_CONFIG_HOME", "/nonexistent-liner-test-config");
    cmd.env("HOME", "/nonexistent-liner-test-home");
    cmd.env_remove("NO_COLOR");
    cmd
}

/// Run `liner scan <root> --format json <extra>`; returns (exit code, report).
fn scan_json(lib: &Library, extra: &[&str]) -> (i32, Value) {
    let out = liner()
        .arg("scan")
        .arg(lib.root())
        .args(["--format", "json"])
        .args(extra)
        .output()
        .unwrap();
    assert!(
        out.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    (out.status.code().unwrap(), v)
}

/// Run `liner scan <root> --color never <extra>` (human); returns (exit code, stdout).
fn scan_human(lib: &Library, extra: &[&str]) -> (i32, String) {
    let out = liner()
        .arg("scan")
        .arg(lib.root())
        .args(["--color", "never"])
        .args(extra)
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
    )
}

/// album_id (or [`LIBRARY`]) -> set of codes.
fn codes_by_album(v: &Value) -> BTreeMap<String, BTreeSet<String>> {
    let mut map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for i in v["issues"].as_array().unwrap() {
        let id = i["album_id"].as_str().unwrap_or(LIBRARY).to_owned();
        map.entry(id)
            .or_default()
            .insert(i["code"].as_str().unwrap().to_owned());
    }
    map
}

fn issues<'a>(v: &'a Value, code: &str) -> Vec<&'a Value> {
    v["issues"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["code"] == code)
        .collect()
}

fn one<'a>(v: &'a Value, code: &str) -> &'a Value {
    let found = issues(v, code);
    assert_eq!(found.len(), 1, "expected one {code} issue, got {found:#?}");
    found[0]
}

fn album<'a>(v: &'a Value, id: &str) -> &'a Value {
    v["albums"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == id)
        .unwrap_or_else(|| panic!("no album {id}"))
}

/// `(disc number, [track paths in order])` per disc of an album.
fn disc_layout(v: &Value, id: &str) -> Vec<(Value, Vec<String>)> {
    album(v, id)["discs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            let paths = d["tracks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["path"].as_str().unwrap().to_owned())
                .collect();
            (d["number"].clone(), paths)
        })
        .collect()
}

fn strs(v: &Value) -> Vec<&str> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect()
}

fn base(album: &str, artist: &str) -> Tags {
    Tags::new()
        .album(album)
        .artist(artist)
        .album_artist(artist)
        .year(2001)
}

/// One synthetic library and the exact codes expected per album_id. Albums that
/// are not listed must have no issues.
struct Scenario {
    name: &'static str,
    build: fn(&Library),
    expect: &'static [(&'static str, &'static [&'static str])],
}

impl Scenario {
    /// Build, scan as JSON, assert the exact code sets and an empty `errors[]`.
    fn run(&self) -> (Library, Value) {
        let lib = Library::new();
        (self.build)(&lib);
        let (code, v) = scan_json(&lib, &["--fail-on", "never"]);
        assert_eq!(code, 0, "{}", self.name);
        let expected: BTreeMap<String, BTreeSet<String>> = self
            .expect
            .iter()
            .map(|(id, codes)| {
                (
                    (*id).to_owned(),
                    codes.iter().map(|c| (*c).to_owned()).collect(),
                )
            })
            .collect();
        assert_eq!(
            codes_by_album(&v),
            expected,
            "scenario {}: {:#}",
            self.name,
            v["issues"]
        );
        assert_eq!(v["errors"], json!([]), "scenario {}", self.name);
        (lib, v)
    }
}

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

/// Clean 2-disc album: CD1/CD2 folders and matching disc tags, FLAC.
fn build_clean_two_disc(lib: &Library) {
    let b = base("Night Drive", "The Examples").disc_total(2);
    lib.add_album(
        "The Examples/Night Drive/CD1",
        Fixture::Flac,
        &b.clone().disc(1),
        4,
    );
    lib.add_album(
        "The Examples/Night Drive/CD2",
        Fixture::Flac,
        &b.clone().disc(2),
        3,
    );
}

/// Disc tags, no disc folders; file names deliberately interleave the discs when
/// sorted by path.
fn build_disc_tags_flat(lib: &Library) {
    let b = base("Tidal", "Moon Unit");
    for (disc, n) in [(1, 3), (2, 2)] {
        for t in 1..=n {
            let tags = b
                .clone()
                .disc(disc)
                .disc_total(2)
                .track(t)
                .track_total(n)
                .title(&format!("D{disc}T{t}"));
            // "01 a" (disc 1), "01 b" (disc 2), "02 a", ...: path order != disc order
            let suffix = if disc == 1 { "a" } else { "b" };
            lib.add(
                &format!("Moon Unit/Tidal/{t:02} {suffix}.m4a"),
                Fixture::M4a,
                &tags,
            );
        }
    }
}

/// CD1/CD2 folders, no disc tags at all.
fn build_folders_no_disc_tags(lib: &Library) {
    let b = base("Folded", "Paper Band");
    lib.add_album("Paper Band/Folded/CD1", Fixture::Mp3, &b, 3);
    lib.add_album("Paper Band/Folded/CD2", Fixture::Mp3, &b, 2);
}

/// Two discs merged flat into one folder, no disc tags, different formats per disc.
fn build_flat_merged(lib: &Library) {
    let b = base("Merged", "Flat Earth");
    for t in 1..=4 {
        let tags = b.clone().track(t).title(&format!("Side A {t}"));
        lib.add(
            &format!("Flat Earth/Merged/a{t:02}.flac"),
            Fixture::Flac,
            &tags,
        );
    }
    for t in 1..=3 {
        let tags = b.clone().track(t).title(&format!("Side B {t}"));
        lib.add(
            &format!("Flat Earth/Merged/b{t:02}.mp3"),
            Fixture::Mp3,
            &tags,
        );
    }
}

/// Disc folders with tags, but CD2/02 is tagged disc 1 (so it also collides with
/// CD1/02 and leaves a hole on disc 2).
fn build_disc_folder_mismatch(lib: &Library) {
    let b = base("Mislabel", "Wrong Way").disc_total(2);
    lib.add_album(
        "Wrong Way/Mislabel/CD1",
        Fixture::OggVorbis,
        &b.clone().disc(1),
        3,
    );
    lib.add_album(
        "Wrong Way/Mislabel/CD2",
        Fixture::OggVorbis,
        &b.clone().disc(2),
        3,
    );
    lib.add(
        "Wrong Way/Mislabel/CD2/02.ogg",
        Fixture::OggVorbis,
        &b.clone().disc(1).track(2).track_total(3).title("Track 02"),
    );
}

/// Continuous numbering across disc folders (1-5 then 6-9): clean.
fn build_continuous(lib: &Library) {
    let b = base("Long Play", "Endless").disc_total(2);
    for (disc, range) in [(1u32, 1..=5u32), (2, 6..=9)] {
        for t in range {
            let tags = b.clone().disc(disc).track(t).title(&format!("Part {t}"));
            lib.add(
                &format!("Endless/Long Play/CD{disc}/{t:02}.opus"),
                Fixture::Opus,
                &tags,
            );
        }
    }
}

/// Continuous numbering with the first track of disc 2 missing (1-5, then 7-9).
fn build_continuous_gap(lib: &Library) {
    let b = base("Short Play", "Endless").disc_total(2);
    for (disc, range) in [(1u32, 1..=5u32), (2, 7..=9)] {
        for t in range {
            let tags = b.clone().disc(disc).track(t).title(&format!("Part {t}"));
            lib.add(
                &format!("Endless/Short Play/CD{disc}/{t:02}.flac"),
                Fixture::Flac,
                &tags,
            );
        }
    }
}

/// The same album as FLAC and MP3 copies in one folder.
fn build_flac_mp3_copies(lib: &Library) {
    let b = base("Twice", "Echo");
    lib.add_album("Echo/Twice", Fixture::Flac, &b, 3);
    lib.add_album("Echo/Twice", Fixture::Mp3, &b, 3);
}

/// Various Artists compilations: two with the same generic title in different
/// folders (no split-album), plus one without album artist.
fn build_various_artists(lib: &Library) {
    for (dir, fixture) in [
        ("Compilations/Greatest Hits 1", Fixture::Flac),
        ("Compilations/Greatest Hits 2", Fixture::M4a),
    ] {
        for (t, artist) in ["Alpha", "Beta", "Gamma"].iter().enumerate() {
            let t = t as u32 + 1;
            let tags = Tags::new()
                .album("Greatest Hits")
                .album_artist("Various Artists")
                .artist(artist)
                .title(&format!("Hit {t}"))
                .track(t)
                .track_total(3)
                .year(1999)
                .compilation(true);
            lib.add(
                &format!("{dir}/{t:02}.{}", fixture.extension()),
                fixture,
                &tags,
            );
        }
    }
    for (t, artist) in ["Delta", "Epsilon"].iter().enumerate() {
        let t = t as u32 + 1;
        let tags = Tags::new()
            .album("Summer Mix")
            .artist(artist)
            .title(&format!("Mix {t}"))
            .track(t)
            .track_total(2);
        lib.add(
            &format!("Compilations/Summer Mix/{t:02}.mp3"),
            Fixture::Mp3,
            &tags,
        );
    }
}

/// One album whose title is NFC on some tracks and NFD on others.
fn build_nfc_nfd(lib: &Library) {
    let nfc: String = "Café Society".nfc().collect();
    let nfd: String = "Café Society".nfd().collect();
    assert_ne!(nfc, nfd);
    for (t, title) in [(1, &nfc), (2, &nfd), (3, &nfc)] {
        let tags = base(title, "Unicode Trio")
            .track(t)
            .track_total(3)
            .title(&format!("Song {t}"));
        lib.add(
            &format!("Unicode Trio/Cafe/{t:02}.flac"),
            Fixture::Flac,
            &tags,
        );
    }
}

/// The same album split over two directories (one FLAC, one MP3).
fn build_split(lib: &Library) {
    let b = base("Scattered", "Split Personality").track_total(4);
    for t in 1..=2 {
        lib.add(
            &format!("Split Personality/Scattered/{t:02}.flac"),
            Fixture::Flac,
            &b.clone().track(t).title(&format!("Bit {t}")),
        );
    }
    for t in 3..=4 {
        lib.add(
            &format!("Downloads/Scattered (rest)/{t:02}.mp3"),
            Fixture::Mp3,
            &b.clone().track(t).title(&format!("Bit {t}")),
        );
    }
}

/// An untagged WAV file in a folder of its own.
fn build_untagged(lib: &Library) {
    lib.add_untagged("Inbox/untitled.wav", Fixture::Wav);
}

/// Totals exceeded and a missing disc (tags 1 and 3, disc total 2 on disc 3).
fn build_exceeds_and_disc_gap(lib: &Library) {
    let b = base("Overflow", "Counters");
    lib.add_album(
        "Counters/Overflow/CD1",
        Fixture::Flac,
        &b.clone().disc(1).disc_total(3),
        2,
    );
    lib.add(
        "Counters/Overflow/CD3/01.flac",
        Fixture::Flac,
        &b.clone()
            .disc(3)
            .disc_total(3)
            .track(1)
            .track_total(1)
            .title("A"),
    );
    lib.add(
        "Counters/Overflow/CD3/02.flac",
        Fixture::Flac,
        &b.clone()
            .disc(3)
            .disc_total(2)
            .track(2)
            .track_total(1)
            .title("B"),
    );
}

/// Inconsistent totals and years, one track without album artist.
fn build_inconsistent(lib: &Library) {
    let tags = |t: u32| {
        Tags::new()
            .album("Loose Ends")
            .artist("Tangle")
            .title(&format!("Knot {t}"))
            .track(t)
            .disc(1)
    };
    lib.add(
        "Tangle/Loose Ends/01.flac",
        Fixture::Flac,
        &tags(1)
            .album_artist("Tangle")
            .track_total(3)
            .disc_total(1)
            .year(2001),
    );
    lib.add(
        "Tangle/Loose Ends/02.mp3",
        Fixture::Mp3,
        &tags(2)
            .album_artist("Tangle")
            .track_total(3)
            .disc_total(1)
            .year(2001),
    );
    lib.add(
        "Tangle/Loose Ends/03.m4a",
        Fixture::M4a,
        &tags(3).track_total(4).disc_total(2).year(2003),
    );
}

/// A flat "Singles" folder with two unrelated albums.
fn build_mixed(lib: &Library) {
    lib.add(
        "Singles/one.flac",
        Fixture::Flac,
        &base("First Single", "Solo").track(1).title("One"),
    );
    lib.add(
        "Singles/two.mp3",
        Fixture::Mp3,
        &base("Second Single", "Duo").track(1).title("Two"),
    );
}

const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "clean two-disc album with CD folders and disc tags",
        build: build_clean_two_disc,
        expect: &[],
    },
    Scenario {
        name: "disc tags without disc folders",
        build: build_disc_tags_flat,
        expect: &[],
    },
    Scenario {
        name: "CD folders without disc tags",
        build: build_folders_no_disc_tags,
        expect: &[("Paper Band/Folded", &["missing-disc-number"])],
    },
    Scenario {
        name: "flat folder with two discs merged, no disc tags",
        build: build_flat_merged,
        expect: &[("Flat Earth/Merged", &["likely-missing-disc-numbers"])],
    },
    Scenario {
        name: "disc tag disagrees with disc folder",
        build: build_disc_folder_mismatch,
        expect: &[(
            "Wrong Way/Mislabel",
            &[
                "disc-folder-mismatch",
                "duplicate-track-position",
                "track-number-gap",
            ],
        )],
    },
    Scenario {
        name: "continuous numbering across discs",
        build: build_continuous,
        expect: &[],
    },
    Scenario {
        name: "continuous numbering with a gap at the disc boundary",
        build: build_continuous_gap,
        expect: &[("Endless/Short Play", &["track-number-gap"])],
    },
    Scenario {
        name: "FLAC and MP3 copies in one folder",
        build: build_flac_mp3_copies,
        expect: &[("Echo/Twice", &["duplicate-track-position"])],
    },
    Scenario {
        name: "various artists compilations",
        build: build_various_artists,
        expect: &[("Compilations/Summer Mix", &["missing-album-artist"])],
    },
    Scenario {
        name: "NFC vs NFD album titles",
        build: build_nfc_nfd,
        expect: &[("Unicode Trio/Cafe", &["inconsistent-tag-spelling"])],
    },
    Scenario {
        name: "album split over two directories",
        build: build_split,
        // Each half also misses the other half's tracks.
        expect: &[
            (LIBRARY, &["split-album"]),
            ("Downloads/Scattered (rest)", &["track-number-gap"]),
            ("Split Personality/Scattered", &["track-number-gap"]),
        ],
    },
    Scenario {
        name: "untagged file",
        build: build_untagged,
        expect: &[(
            "Inbox",
            &[
                "missing-album",
                "missing-artist",
                "missing-title",
                "missing-track-number",
            ],
        )],
    },
    Scenario {
        name: "totals exceeded and a missing disc",
        build: build_exceeds_and_disc_gap,
        expect: &[(
            "Counters/Overflow",
            &[
                "disc-number-exceeds-total",
                "disc-number-gap",
                "inconsistent-disc-total",
                "track-number-exceeds-total",
            ],
        )],
    },
    Scenario {
        name: "inconsistent totals, years and album artist",
        build: build_inconsistent,
        expect: &[(
            "Tangle/Loose Ends",
            &[
                "inconsistent-disc-total",
                "inconsistent-track-total",
                "inconsistent-year",
                "partial-album-artist",
            ],
        )],
    },
    Scenario {
        name: "flat folder of unrelated singles",
        build: build_mixed,
        expect: &[("Singles", &["mixed-album-directory"])],
    },
];

fn scenario(name: &str) -> &'static Scenario {
    SCENARIOS
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("no scenario {name}"))
}

// ---------------------------------------------------------------------------
// Coverage
// ---------------------------------------------------------------------------

#[test]
fn scenarios_cover_every_catalog_code() {
    let mut seen = BTreeSet::new();
    for s in SCENARIOS {
        let (_lib, v) = s.run();
        for set in codes_by_album(&v).into_values() {
            seen.extend(set);
        }
    }
    let catalog: BTreeSet<String> = liner::rules::CATALOG
        .iter()
        .map(|r| r.code.to_owned())
        .collect();
    assert_eq!(seen, catalog);
}

#[test]
fn whole_library_combines_all_scenarios() {
    // All scenarios in one library (they use disjoint directories, and only the
    // split scenario shares an album key across directories): the per-album
    // results must be the union, the summary must match the issue list.
    let lib = Library::new();
    for s in SCENARIOS {
        (s.build)(&lib);
    }
    lib.add_raw("The Examples/Night Drive/cover.jpg", b"jpeg");
    lib.add_corrupt("Broken/01.flac", Corrupt::Truncated);

    let (code, v) = scan_json(&lib, &[]);
    assert_eq!(code, 1, "error issues and a corrupt file fail by default");

    let mut expected: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for s in SCENARIOS {
        for (id, codes) in s.expect {
            expected
                .entry((*id).to_owned())
                .or_default()
                .extend(codes.iter().map(|c| (*c).to_owned()));
        }
    }
    assert_eq!(codes_by_album(&v), expected);

    let all = v["issues"].as_array().unwrap();
    let count = |sev: &str| all.iter().filter(|i| i["severity"] == sev).count() as u64;
    let s = &v["summary"];
    assert_eq!(s["issues_by_severity"]["error"], count("error"));
    assert_eq!(s["issues_by_severity"]["warning"], count("warning"));
    assert_eq!(s["issues_by_severity"]["info"], count("info"));
    let mut by_code: BTreeMap<String, u64> = BTreeMap::new();
    for i in all {
        *by_code
            .entry(i["code"].as_str().unwrap().to_owned())
            .or_default() += 1;
    }
    assert_eq!(s["issues_by_code"], json!(by_code));
    assert_eq!(s["issues_hidden"], 0);
    assert_eq!(v["options"]["min_severity"], "info");

    let audio = v["albums"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["track_count"].as_u64().unwrap())
        .sum::<u64>();
    assert_eq!(s["audio_files"], audio + 1, "+1 corrupt file");
    assert_eq!(s["files_scanned"], audio + 2, "+ corrupt + cover.jpg");
    assert_eq!(s["unreadable_files"], 1);
    assert_eq!(s["albums"], v["albums"].as_array().unwrap().len() as u64);
    assert_eq!(v["errors"][0]["path"], "Broken/01.flac");
    assert_eq!(v["errors"][0]["kind"], "corrupt");
}

// ---------------------------------------------------------------------------
// Multi-disc scenarios
// ---------------------------------------------------------------------------

#[test]
fn clean_two_disc_album_has_no_issues() {
    let (_lib, v) = scenario("clean two-disc album with CD folders and disc tags").run();
    let a = album(&v, "The Examples/Night Drive");
    assert_eq!(a["disc_count"], 2);
    assert_eq!(a["track_count"], 7);
    assert_eq!(a["title"], "Night Drive");
    assert_eq!(a["discs"][0]["folder"], "The Examples/Night Drive/CD1");
    assert_eq!(a["discs"][1]["folder"], "The Examples/Night Drive/CD2");
    assert_eq!(v["summary"]["issues_by_code"], json!({}));
}

#[test]
fn disc_tags_without_folders_order_tracks_by_disc() {
    let (_lib, v) = scenario("disc tags without disc folders").run();
    let p = |n: &str| format!("Moon Unit/Tidal/{n}.m4a");
    assert_eq!(
        disc_layout(&v, "Moon Unit/Tidal"),
        vec![
            (json!(1), vec![p("01 a"), p("02 a"), p("03 a")]),
            (json!(2), vec![p("01 b"), p("02 b")]),
        ]
    );
    let a = album(&v, "Moon Unit/Tidal");
    assert_eq!(a["discs"][0]["folder"], Value::Null);
    assert_eq!(a["discs"][1]["tracks"][1]["track_no"], 2);
    assert_eq!(a["discs"][1]["tracks"][1]["disc_no"], 2);
}

#[test]
fn disc_folders_without_disc_tags() {
    let (_lib, v) = scenario("CD folders without disc tags").run();
    let i = one(&v, "missing-disc-number");
    assert_eq!(i["severity"], "warning");
    assert_eq!(i["details"], json!({"count": 5}));
    assert_eq!(i["paths"].as_array().unwrap().len(), 5);
    // Folder hints still give every track a disc and a sane order.
    let layout = disc_layout(&v, "Paper Band/Folded");
    assert_eq!(layout[0].0, 1);
    assert_eq!(layout[1].0, 2);
    assert_eq!(
        layout[1].1,
        vec![
            "Paper Band/Folded/CD2/01.mp3",
            "Paper Band/Folded/CD2/02.mp3"
        ]
    );
}

#[test]
fn flat_folder_with_merged_discs() {
    let (_lib, v) = scenario("flat folder with two discs merged, no disc tags").run();
    let i = one(&v, "likely-missing-disc-numbers");
    assert_eq!(i["severity"], "error");
    assert_eq!(i["details"], json!({"duplicated_tracks": [1, 2, 3]}));
    assert_eq!(
        strs(&i["paths"]),
        [
            "Flat Earth/Merged/a01.flac",
            "Flat Earth/Merged/a02.flac",
            "Flat Earth/Merged/a03.flac",
            "Flat Earth/Merged/b01.mp3",
            "Flat Earth/Merged/b02.mp3",
            "Flat Earth/Merged/b03.mp3",
        ]
    );
}

#[test]
fn disc_tag_disagreeing_with_folder() {
    let (_lib, v) = scenario("disc tag disagrees with disc folder").run();
    let i = one(&v, "disc-folder-mismatch");
    assert_eq!(
        i["details"],
        json!({"tag_disc": 1, "folder_disc": 2, "folder": "Wrong Way/Mislabel/CD2"})
    );
    assert_eq!(strs(&i["paths"]), ["Wrong Way/Mislabel/CD2/02.ogg"]);
    // The tag wins for the position: it collides with CD1/02 and leaves disc 2
    // without track 2.
    let dup = one(&v, "duplicate-track-position");
    assert_eq!(dup["details"], json!({"disc": 1, "track": 2}));
    assert_eq!(
        strs(&dup["paths"]),
        [
            "Wrong Way/Mislabel/CD1/02.ogg",
            "Wrong Way/Mislabel/CD2/02.ogg"
        ]
    );
    let gap = one(&v, "track-number-gap");
    assert_eq!(
        gap["details"],
        json!({"disc": 2, "missing": [2], "expected_total": 3})
    );
}

#[test]
fn continuous_numbering_across_discs_is_clean() {
    let (_lib, v) = scenario("continuous numbering across discs").run();
    let layout = disc_layout(&v, "Endless/Long Play");
    assert_eq!(layout.len(), 2);
    assert_eq!(layout[1].1[0], "Endless/Long Play/CD2/06.opus");
}

#[test]
fn continuous_numbering_gap_reported_once_on_later_disc() {
    let (_lib, v) = scenario("continuous numbering with a gap at the disc boundary").run();
    let gap = one(&v, "track-number-gap");
    assert_eq!(
        gap["details"],
        json!({"disc": 2, "missing": [6], "expected_total": null})
    );
}

#[test]
fn flac_and_mp3_copies_are_duplicates_not_discs() {
    let (_lib, v) = scenario("FLAC and MP3 copies in one folder").run();
    let dups = issues(&v, "duplicate-track-position");
    assert_eq!(dups.len(), 3);
    for (n, i) in dups.iter().enumerate() {
        let t = n + 1;
        assert_eq!(i["details"], json!({"disc": 1, "track": t}));
        assert_eq!(
            strs(&i["paths"]),
            [
                format!("Echo/Twice/{t:02}.flac"),
                format!("Echo/Twice/{t:02}.mp3")
            ]
        );
    }
}

// ---------------------------------------------------------------------------
// Album tags and library scope
// ---------------------------------------------------------------------------

#[test]
fn various_artists_compilations() {
    let (_lib, v) = scenario("various artists compilations").run();
    let i = one(&v, "missing-album-artist");
    assert_eq!(i["details"], json!({"artists": ["Delta", "Epsilon"]}));
    // Same generic title in two folders, but compilations have no split key.
    assert!(issues(&v, "split-album").is_empty());
}

#[test]
fn nfc_and_nfd_titles_are_a_spelling_issue_not_a_mix() {
    let (_lib, v) = scenario("NFC vs NFD album titles").run();
    let i = one(&v, "inconsistent-tag-spelling");
    assert_eq!(i["details"]["field"], "album");
    let mut want = vec![
        "Café Society".nfc().collect::<String>(),
        "Café Society".nfd().collect::<String>(),
    ];
    want.sort();
    assert_eq!(i["details"]["values"], json!(want));
    assert_eq!(i["paths"].as_array().unwrap().len(), 3);
}

#[test]
fn split_album_across_directories() {
    let (_lib, v) = scenario("album split over two directories").run();
    let i = one(&v, "split-album");
    assert_eq!(i["scope"], "library");
    assert_eq!(i["album_id"], Value::Null);
    assert_eq!(
        i["details"],
        json!({
            "album": "Scattered",
            "album_artist": "Split Personality",
            "album_ids": ["Downloads/Scattered (rest)", "Split Personality/Scattered"],
        })
    );
    assert_eq!(i["paths"].as_array().unwrap().len(), 4);
}

#[test]
fn untagged_file_gets_every_file_rule() {
    let (_lib, v) = scenario("untagged file").run();
    for code in [
        "missing-album",
        "missing-artist",
        "missing-title",
        "missing-track-number",
    ] {
        assert_eq!(strs(&one(&v, code)["paths"]), ["Inbox/untitled.wav"]);
    }
    assert_eq!(one(&v, "missing-title")["severity"], "info");
}

#[test]
fn exceeded_totals_and_disc_gap() {
    let (_lib, v) = scenario("totals exceeded and a missing disc").run();
    assert_eq!(
        one(&v, "track-number-exceeds-total")["details"],
        json!({"track_no": 2, "track_total": 1})
    );
    let d = one(&v, "disc-number-exceeds-total");
    assert_eq!(d["details"], json!({"disc_no": 3, "disc_total": 2}));
    assert_eq!(strs(&d["paths"]), ["Counters/Overflow/CD3/02.flac"]);
    assert_eq!(
        one(&v, "disc-number-gap")["details"],
        json!({"missing": [2], "expected_total": null})
    );
    assert_eq!(
        one(&v, "inconsistent-disc-total")["details"],
        json!({"values": [2, 3]})
    );
}

#[test]
fn inconsistent_album_tags() {
    let (_lib, v) = scenario("inconsistent totals, years and album artist").run();
    assert_eq!(
        one(&v, "inconsistent-track-total")["details"],
        json!({"disc": 1, "values": [3, 4]})
    );
    assert_eq!(
        one(&v, "inconsistent-disc-total")["details"],
        json!({"values": [1, 2]})
    );
    let y = one(&v, "inconsistent-year");
    assert_eq!(y["severity"], "info");
    assert_eq!(y["details"], json!({"values": [2001, 2003]}));
    let p = one(&v, "partial-album-artist");
    assert_eq!(p["details"], json!({"album_artist": "Tangle"}));
    assert_eq!(strs(&p["paths"]), ["Tangle/Loose Ends/03.m4a"]);
}

#[test]
fn flat_singles_folder_is_mixed_only() {
    let (_lib, v) = scenario("flat folder of unrelated singles").run();
    let mut mixed: Vec<Value> = issues(&v, "mixed-album-directory")
        .into_iter()
        .map(|i| i["details"].clone())
        .collect();
    mixed.sort_by_key(|d| d["field"].as_str().unwrap().to_owned());
    assert_eq!(
        mixed,
        [
            json!({"field": "album", "values": ["First Single", "Second Single"]}),
            json!({"field": "album_artist", "values": ["Duo", "Solo"]}),
        ]
    );
}

// ---------------------------------------------------------------------------
// Human output, filtering and exit codes
// ---------------------------------------------------------------------------

#[test]
fn human_output_spot_check() {
    let lib = Library::new();
    build_flat_merged(&lib);
    build_split(&lib);
    build_clean_two_disc(&lib);
    lib.add_corrupt("Broken/01.flac", Corrupt::Garbage);
    let (code, out) = scan_human(&lib, &[]);
    assert_eq!(code, 1);

    assert!(
        out.contains("Flat Earth/Merged  \"Merged\" by Flat Earth"),
        "{out}"
    );
    assert!(
        out.contains("error     likely-missing-disc-numbers"),
        "{out}"
    );
    assert!(out.contains("- a01.flac"), "{out}");
    assert!(out.contains("warning   split-album"), "{out}");
    assert!(out.contains("Unreadable files (1)"), "{out}");
    assert!(out.contains("Broken/01.flac"), "{out}");
    // Clean albums are not listed.
    assert!(!out.contains("Night Drive"), "{out}");
    assert!(out.contains("Missing tracks 1, 2 (track total 4)"), "{out}");
    // 7 merged + 4 split + 7 clean + 1 corrupt; albums: merged, 2 split, clean.
    assert_eq!(
        out.lines().last().unwrap(),
        "19 files scanned, 19 audio, 1 unreadable, 4 albums; 1 errors, 3 warnings, 0 info",
        "{out}"
    );
}

#[test]
fn min_severity_hides_info_but_fail_on_info_still_fails() {
    let lib = Library::new();
    build_inconsistent(&lib); // 3 warnings + 1 info (inconsistent-year)

    let (code, v) = scan_json(&lib, &["--min-severity", "warning", "--fail-on", "info"]);
    assert_eq!(code, 1);
    assert_eq!(v["options"]["min_severity"], "warning");
    assert_eq!(v["summary"]["issues_hidden"], 1);
    assert_eq!(
        v["summary"]["issues_by_severity"],
        json!({"error": 0, "warning": 3, "info": 0})
    );
    assert!(issues(&v, "inconsistent-year").is_empty());
    assert!(
        v["summary"]["issues_by_code"]
            .get("inconsistent-year")
            .is_none()
    );

    // Without the info threshold the hidden info issue does not fail the run.
    let (code, _) = scan_json(&lib, &["--min-severity", "warning", "--fail-on", "error"]);
    assert_eq!(code, 0);

    let (code, out) = scan_human(&lib, &["--min-severity", "warning", "--fail-on", "info"]);
    assert_eq!(code, 1);
    assert!(!out.contains("inconsistent-year"), "{out}");
    assert!(
        out.lines()
            .last()
            .unwrap()
            .ends_with("(1 below warning hidden)"),
        "{out}"
    );
}
