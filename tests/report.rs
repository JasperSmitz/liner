//! Report building, JSON golden file and human rendering, from hand-built inputs
//! (no audio files involved).
//!
//! Regenerate the golden file deliberately with:
//!   LINER_UPDATE_GOLDEN=1 cargo test --test report

use std::collections::BTreeMap;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use liner::group::{AlbumUnit, GroupedTrack};
use liner::model::{AudioFormat, FileError, FileErrorKind, Issue, Severity, TagKind, TrackInfo};
use liner::render::{self, HumanOptions, OutputFormat};
use liner::report::{Report, ReportInput, build_report};
use liner::rules::{codes, rule_info};
use liner::scan::{ScanResult, ScannedFile};
use serde_json::json;

const GOLDEN: &str = "tests/golden/report.json";
const ROOT: &str = "/library";

struct Inputs {
    scan: ScanResult,
    read_errors: Vec<FileError>,
    albums: Vec<AlbumUnit>,
    issues: Vec<Issue>,
}

fn track(rel: &str, f: impl FnOnce(TrackInfo) -> TrackInfo) -> TrackInfo {
    f(TrackInfo::new(
        Path::new(ROOT).join(rel),
        rel,
        AudioFormat::Flac,
    ))
}

fn grouped(info: TrackInfo, folder: Option<(u32, &str)>, effective: Option<u32>) -> GroupedTrack {
    GroupedTrack {
        info,
        folder_disc: folder.map(|(n, _)| n),
        disc_folder: folder.map(|(_, f)| f.to_owned()),
        effective_disc: effective,
    }
}

fn file_error(rel: &str, kind: FileErrorKind, message: &str) -> FileError {
    FileError {
        path: Path::new(ROOT).join(rel),
        rel_path: rel.to_owned(),
        kind,
        message: message.to_owned(),
    }
}

fn issue(
    code: &str,
    message: &str,
    album: Option<&str>,
    paths: &[&str],
    details: serde_json::Value,
) -> Issue {
    let details: BTreeMap<String, serde_json::Value> = match details {
        serde_json::Value::Object(m) => m.into_iter().collect(),
        _ => BTreeMap::new(),
    };
    rule_info(code).expect("known code").issue(
        message,
        album,
        paths.iter().map(|p| (*p).to_owned()).collect(),
        details,
    )
}

fn inputs() -> Inputs {
    let base = |album: &str, artist: &str| {
        let album = album.to_owned();
        let artist = artist.to_owned();
        move |t: TrackInfo| TrackInfo {
            album: Some(album),
            artist: Some(artist.clone()),
            album_artist: Some(artist),
            year: Some(1999),
            tag_types: vec![TagKind::VorbisComments],
            duration_ms: Some(200),
            ..t
        }
    };

    // Two-disc album: CD1 tagged, CD2 untagged (folder hint), one loose untagged file.
    let a = "Artist/Album";
    let a_tracks = vec![
        grouped(
            track("Artist/Album/CD1/02.flac", |t| TrackInfo {
                title: Some("Two".into()),
                track_no: Some(2),
                track_total: Some(2),
                disc_no: Some(1),
                disc_total: Some(2),
                ..base("Album", "Artist")(t)
            }),
            Some((1, "Artist/Album/CD1")),
            Some(1),
        ),
        grouped(
            track("Artist/Album/CD1/01.flac", |t| TrackInfo {
                title: Some("One".into()),
                track_no: Some(1),
                track_total: Some(2),
                disc_no: Some(1),
                disc_total: Some(2),
                ..base("Album", "Artist")(t)
            }),
            Some((1, "Artist/Album/CD1")),
            Some(1),
        ),
        grouped(
            track("Artist/Album/CD2/01.flac", |t| TrackInfo {
                track_no: Some(1),
                ..base("Album", "Artist")(t)
            }),
            Some((2, "Artist/Album/CD2")),
            Some(2),
        ),
        grouped(
            track("Artist/Album/bonus.flac", |t| TrackInfo {
                title: Some("Bonus".into()),
                ..base("Album", "Artist")(t)
            }),
            None,
            None,
        ),
    ];
    let album_a = AlbumUnit {
        id: a.to_owned(),
        tracks: a_tracks,
        is_multi_disc: true,
        title: Some("Album".into()),
        album_artist: Some("Artist".into()),
        disc_count: 2,
    };

    // Compilation in MP3, single disc, no album artist.
    let c = "Various/Comp";
    let c_tracks = ["Ann", "Bob"]
        .iter()
        .enumerate()
        .map(|(i, artist)| {
            let n = i as u32 + 1;
            let mut info = track(&format!("{c}/{n:02} {artist}.mp3"), |t| TrackInfo {
                title: Some(format!("Song {n}")),
                artist: Some((*artist).to_owned()),
                album: Some("Comp".into()),
                track_no: Some(n),
                year: Some(2000 + n),
                date: Some(format!("{}-01-01", 2000 + n)),
                genre: Some("Pop".into()),
                compilation: true,
                tag_types: vec![TagKind::Id3v1, TagKind::Id3v2],
                ..t
            });
            info.format = AudioFormat::Mp3;
            grouped(info, None, Some(1))
        })
        .collect();
    let album_c = AlbumUnit {
        id: c.to_owned(),
        tracks: c_tracks,
        is_multi_disc: false,
        title: Some("Comp".into()),
        album_artist: None,
        disc_count: 1,
    };

    // A loose track in the root unit, with a duplicate of album A elsewhere.
    let album_root = AlbumUnit {
        id: ".".to_owned(),
        tracks: vec![grouped(
            track("loose.flac", |t| TrackInfo {
                album: Some("Album".into()),
                album_artist: Some("Artist".into()),
                ..t
            }),
            None,
            Some(1),
        )],
        is_multi_disc: false,
        title: Some("Album".into()),
        album_artist: Some("Artist".into()),
        disc_count: 1,
    };

    let issues = vec![
        issue(
            codes::MISSING_DISC_NUMBER,
            "2 tracks of a multi-disc album have no disc number",
            Some(a),
            &["Artist/Album/CD2/01.flac", "Artist/Album/bonus.flac"],
            json!({"count": 2}),
        ),
        issue(
            codes::MISSING_TITLE,
            "Track has no title",
            Some(a),
            &["Artist/Album/CD2/01.flac"],
            json!({}),
        ),
        issue(
            codes::MISSING_TRACK_NUMBER,
            "Track has no track number",
            Some(a),
            &["Artist/Album/bonus.flac"],
            json!({}),
        ),
        issue(
            codes::MISSING_ALBUM_ARTIST,
            "Tracks have 2 different artists but no album artist",
            Some(c),
            &["Various/Comp/01 Ann.mp3", "Various/Comp/02 Bob.mp3"],
            json!({"artists": ["Ann", "Bob"]}),
        ),
        issue(
            codes::INCONSISTENT_YEAR,
            "Tracks have different years: 2001, 2002",
            Some(c),
            &["Various/Comp/01 Ann.mp3", "Various/Comp/02 Bob.mp3"],
            json!({"values": [2001, 2002]}),
        ),
        issue(
            codes::DUPLICATE_TRACK_POSITION,
            "Disc 1 track 1 appears in 2 files",
            Some(a),
            &["Artist/Album/CD1/01.flac", "loose.flac"],
            json!({"disc": 1, "track": 1}),
        ),
        issue(
            codes::SPLIT_ALBUM,
            "Album \"Album\" by Artist is spread over 2 directories",
            None,
            &[
                "Artist/Album/CD1/01.flac",
                "Artist/Album/CD1/02.flac",
                "Artist/Album/CD2/01.flac",
                "Artist/Album/bonus.flac",
                "loose.flac",
            ],
            json!({"album": "Album", "album_artist": "Artist", "album_ids": [".", a]}),
        ),
    ];

    let audio: Vec<ScannedFile> = [
        "Artist/Album/CD1/01.flac",
        "Artist/Album/CD1/02.flac",
        "Artist/Album/CD2/01.flac",
        "Artist/Album/bad.flac",
        "Artist/Album/bonus.flac",
        "Various/Comp/01 Ann.mp3",
        "Various/Comp/02 Bob.mp3",
        "loose.flac",
    ]
    .iter()
    .map(|rel| ScannedFile {
        path: Path::new(ROOT).join(rel),
        rel_path: (*rel).to_owned(),
        size: 1000,
    })
    .collect();

    Inputs {
        scan: ScanResult {
            audio_files: audio,
            other_files: 3,
            errors: vec![file_error(
                "Locked",
                FileErrorKind::Walk,
                "permission denied",
            )],
        },
        read_errors: vec![file_error(
            "Artist/Album/bad.flac",
            FileErrorKind::Corrupt,
            "truncated STREAMINFO block",
        )],
        albums: vec![album_root, album_a, album_c],
        issues,
    }
}

fn timestamp() -> jiff::Timestamp {
    "2026-11-08T10:00:00Z".parse().unwrap()
}

fn build(i: &Inputs) -> Report {
    let mut report = build_report(ReportInput {
        root: Path::new(ROOT),
        generated_at: timestamp(),
        scan_options: Default::default(),
        scan: &i.scan,
        read_errors: &i.read_errors,
        albums: &i.albums,
        issues: &i.issues,
    });
    // Keep the golden file independent of the crate version.
    report.tool.version = "0.0.0-test".to_owned();
    report
}

fn json_bytes(r: &Report) -> Vec<u8> {
    let mut out = Vec::new();
    render::render(r, OutputFormat::Json, &mut out).unwrap();
    out
}

fn human(r: &Report, color: bool) -> String {
    let mut out = Vec::new();
    render::render_with(r, OutputFormat::Human, HumanOptions { color }, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

/// Deterministically scramble every input list.
fn shuffled(mut i: Inputs) -> Inputs {
    i.albums.reverse();
    i.albums.rotate_left(1);
    for a in &mut i.albums {
        a.tracks.reverse();
    }
    i.issues.reverse();
    i.issues.rotate_left(2);
    std::mem::swap(&mut i.scan.errors, &mut i.read_errors);
    i
}

#[test]
fn golden_json() {
    let report = build(&inputs());
    let bytes = json_bytes(&report);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(GOLDEN);
    if std::env::var_os("LINER_UPDATE_GOLDEN").is_some() {
        #[allow(clippy::disallowed_methods)] // dev-time regeneration of a repo file
        std::fs::write(&path, &bytes).unwrap();
    }
    let committed = std::fs::read(&path).unwrap_or_default();
    assert!(
        committed == bytes,
        "{GOLDEN} differs from the generated report:\n{}\n\
         If intended, run `LINER_UPDATE_GOLDEN=1 cargo test --test report`.",
        String::from_utf8_lossy(&bytes)
    );
    let back: Report = serde_json::from_slice(&committed).unwrap();
    assert_eq!(back, report);
}

#[test]
fn same_inputs_give_identical_bytes_regardless_of_order() {
    let a = json_bytes(&build(&inputs()));
    let b = json_bytes(&build(&inputs()));
    let c = json_bytes(&build(&shuffled(inputs())));
    assert_eq!(a, b);
    assert_eq!(a, c);
    assert_eq!(
        human(&build(&inputs()), false),
        human(&build(&shuffled(inputs())), false)
    );
}

#[test]
fn albums_discs_and_tracks_are_converted_and_sorted() {
    let r = build(&inputs());
    let ids: Vec<&str> = r.albums.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, [".", "Artist/Album", "Various/Comp"]);

    let a = &r.albums[1];
    assert_eq!(a.path, a.id);
    assert_eq!(a.track_count, 4);
    assert_eq!(a.disc_count, 2);
    let discs: Vec<(Option<u32>, Option<&str>)> = a
        .discs
        .iter()
        .map(|d| (d.number, d.folder.as_deref()))
        .collect();
    assert_eq!(
        discs,
        [
            (Some(1), Some("Artist/Album/CD1")),
            (Some(2), Some("Artist/Album/CD2")),
            (None, None),
        ]
    );
    let d1: Vec<&str> = a.discs[0].tracks.iter().map(|t| t.path.as_str()).collect();
    assert_eq!(d1, ["Artist/Album/CD1/01.flac", "Artist/Album/CD1/02.flac"]);
    // disc_no in the track entry is the tag value, not the effective disc.
    assert_eq!(a.discs[1].tracks[0].disc_no, None);
    let comp = &r.albums[2];
    assert!(comp.title.as_deref() == Some("Comp") && comp.album_artist.is_none());
    assert_eq!(comp.discs[0].tracks[0].format, AudioFormat::Mp3);
}

#[test]
fn disc_folder_is_null_when_tracks_come_from_several_places() {
    let mut i = inputs();
    // Tag says disc 1 but the file lives in CD2: disc 1 now spans two folders.
    let a = &mut i.albums[1];
    a.tracks[2].effective_disc = Some(1);
    let r = build(&i);
    let a = &r.albums[1];
    assert_eq!(a.discs[0].number, Some(1));
    assert_eq!(a.discs[0].folder, None);
    assert_eq!(a.discs[0].tracks.len(), 3);
}

#[test]
fn errors_merge_scan_and_read_failures_sorted() {
    let r = build(&inputs());
    let errors: Vec<(&str, FileErrorKind)> =
        r.errors.iter().map(|e| (e.path.as_str(), e.kind)).collect();
    assert_eq!(
        errors,
        [
            ("Artist/Album/bad.flac", FileErrorKind::Corrupt),
            ("Locked", FileErrorKind::Walk),
        ]
    );
}

#[test]
fn summary_counts_and_issue_order() {
    let mut r = build(&inputs());
    let s = &r.summary;
    assert_eq!(s.files_scanned, 11);
    assert_eq!(s.audio_files, 8);
    assert_eq!(s.unreadable_files, 2);
    assert_eq!(s.albums, 3);
    assert_eq!(
        (
            s.issues_by_severity.error,
            s.issues_by_severity.warning,
            s.issues_by_severity.info
        ),
        (1, 4, 2)
    );
    assert_eq!(s.issues_by_code[codes::MISSING_TITLE], 1);
    assert_eq!(s.issues_by_code.len(), 7);
    let severities: Vec<Severity> = r.issues.iter().map(|i| i.severity).collect();
    assert!(severities.windows(2).all(|w| w[0] >= w[1]), "error first");
    assert_eq!(r.max_severity(), Some(Severity::Error));

    r.retain_min_severity(Severity::Warning);
    assert_eq!(r.summary.issues_by_severity.info, 0);
    assert_eq!(r.summary.issues_by_severity.warning, 4);
    assert!(!r.summary.issues_by_code.contains_key(codes::MISSING_TITLE));
    // errors[] is not affected by --min-severity.
    assert_eq!(r.errors.len(), 2);
    // The filter is recorded.
    assert_eq!(r.summary.issues_hidden, 2);
    assert_eq!(r.options.min_severity, Severity::Warning);
    r.retain_min_severity(Severity::Error);
    assert_eq!(r.summary.issues_hidden, 6);
    assert_eq!(r.options.min_severity, Severity::Error);
}

#[test]
fn json_keeps_null_optionals() {
    let v: serde_json::Value = serde_json::from_slice(&json_bytes(&build(&inputs()))).unwrap();
    let comp = &v["albums"][2];
    assert!(comp.as_object().unwrap().contains_key("album_artist"));
    assert!(comp["album_artist"].is_null());
    let split = v["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["code"] == codes::SPLIT_ALBUM)
        .unwrap();
    assert!(split["album_id"].is_null());
    assert!(split.as_object().unwrap().contains_key("details"));
    let track = &v["albums"][0]["discs"][0]["tracks"][0];
    for key in ["title", "track_no", "duration_ms", "genre"] {
        assert!(track.as_object().unwrap().contains_key(key), "{key}");
    }
}

/// Every object property in the schema is `required` (ADR 0003: keys are always
/// present), and the open enums accept unknown strings.
#[test]
fn schema_requires_every_key_and_keeps_enums_open() {
    let schema = liner::report::json_schema();
    let mut objects = vec![&schema];
    objects.extend(schema["$defs"].as_object().unwrap().values());
    for obj in objects {
        let Some(props) = obj.get("properties").and_then(|p| p.as_object()) else {
            continue;
        };
        let required = obj["required"].as_array().unwrap();
        for key in props.keys() {
            assert!(
                required.contains(&key.as_str().into()),
                "{key} not required"
            );
        }
    }
    for open in ["AudioFormat", "TagKind", "FileErrorKind"] {
        let any_of = schema["$defs"][open]["anyOf"].as_array().unwrap();
        assert!(
            any_of.contains(&json!({
                "type": "string",
                "description": "A value added in a later 1.x minor version."
            })),
            "{open}"
        );
    }
    for closed in ["Severity", "Scope"] {
        assert!(schema["$defs"][closed].get("anyOf").is_none(), "{closed}");
    }
    assert_eq!(schema["properties"]["generated_at"]["format"], "date-time");
}

#[test]
fn human_output_layout() {
    let text = human(&build(&inputs()), false);
    // Albums with issues, sorted by id, with a header; then library, errors, summary.
    let order = [
        "Artist/Album  \"Album\" by Artist  (2 discs, 4 tracks)",
        "Various/Comp  \"Comp\"  (1 disc, 2 tracks)",
        "Library",
        "Unreadable files (2)",
        "files scanned",
    ];
    let mut at = 0;
    for needle in order {
        let pos = text[at..]
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} missing or out of order in:\n{text}"));
        at += pos + needle.len();
    }
    // The root unit "." has no album-scoped issue here, so no section for it.
    assert!(!text.lines().any(|l| l.starts_with(". ")), "{text}");
    // Issue lines: severity marker + code, message and album-relative paths.
    assert!(
        text.contains("  error     duplicate-track-position\n"),
        "{text}"
    );
    assert!(text.contains("            - CD1/01.flac\n"), "{text}");
    // A path outside the album stays root-relative.
    assert!(text.contains("            - loose.flac\n"), "{text}");
    assert!(
        text.contains("  corrupt   Artist/Album/bad.flac\n"),
        "{text}"
    );
    assert!(text.contains("1 errors, 4 warnings, 2 info"), "{text}");
    assert!(!text.contains('\x1b'));
    for line in text.lines() {
        // Only path lines may exceed 80 columns (they cannot be wrapped).
        assert!(
            line.chars().count() <= 80 || line.trim_start().starts_with("- "),
            "{line}"
        );
    }
    assert!(text.ends_with('\n'));
}

#[test]
fn human_output_colour_only_when_asked() {
    let text = human(&build(&inputs()), true);
    assert!(text.contains("\x1b[31merror\x1b[0m"));
    assert!(text.contains("\x1b[1m"));
    // Stripping escapes yields the plain rendering.
    let mut plain = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for c in chars.by_ref() {
                if c == 'm' {
                    break;
                }
            }
        } else {
            plain.push(c);
        }
    }
    assert_eq!(plain, human(&build(&inputs()), false));
}

#[test]
fn human_output_long_path_lists_are_elided() {
    let mut i = inputs();
    let paths: Vec<String> = (1..=12)
        .map(|n| format!("Artist/Album/{n:02}.flac"))
        .collect();
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    i.issues = vec![issue(
        codes::TRACK_NUMBER_GAP,
        "Disc 1 is missing tracks 13, 14",
        Some("Artist/Album"),
        &refs,
        json!({"disc": 1, "missing": [13, 14], "expected_total": 14}),
    )];
    let text = human(&build(&i), false);
    assert!(text.contains("- 08.flac"));
    assert!(!text.contains("- 09.flac"));
    assert!(text.contains("... and 4 more"), "{text}");
}

#[test]
fn human_output_clean_library() {
    let mut i = inputs();
    i.issues.clear();
    i.scan.errors.clear();
    i.read_errors.clear();
    let text = human(&build(&i), false);
    assert!(text.starts_with("No issues found.\n"), "{text}");
    assert_eq!(text.lines().count(), 2);
}

#[test]
fn human_output_mentions_hidden_issues() {
    let mut i = inputs();
    i.scan.errors.clear();
    i.read_errors.clear();
    i.issues.retain(|x| x.severity != Severity::Error);
    let mut r = build(&i);
    r.retain_min_severity(Severity::Error);
    let text = human(&r, false);
    assert!(
        text.starts_with("No issues at or above error (6 lower-severity issues hidden).\n"),
        "{text}"
    );
    assert!(
        text.contains("0 errors, 0 warnings, 0 info (6 below error hidden)"),
        "{text}"
    );

    // With issues left, the summary line still reports the hidden ones.
    let mut r = build(&inputs());
    r.retain_min_severity(Severity::Warning);
    let text = human(&r, false);
    assert!(!text.contains("No issues"), "{text}");
    assert!(
        text.contains("1 errors, 4 warnings, 0 info (2 below warning hidden)"),
        "{text}"
    );
}

/// Tag values and file names are untrusted: control characters (ESC, newline, ...)
/// must never reach the terminal raw, and JSON messages stay single-line.
#[test]
fn control_characters_are_escaped() {
    let id = "Evil\x1b[2J/Al\nbum";
    let title = "Title\x1b]0;pwned\x07";
    let path = "Evil\x1b[2J/Al\nbum/0\x1b[31m1\nfake line.flac";
    let album = AlbumUnit {
        id: id.to_owned(),
        tracks: vec![grouped(
            track(path, |t| TrackInfo {
                album: Some(title.to_owned()),
                ..t
            }),
            None,
            Some(1),
        )],
        is_multi_disc: false,
        title: Some(title.to_owned()),
        album_artist: Some("Art\u{9b}ist".to_owned()),
        disc_count: 1,
    };
    let issues = vec![issue(
        codes::INCONSISTENT_TAG_SPELLING,
        &format!("Album title is spelled 2 ways: '{title}', 'x\ny'"),
        Some(id),
        &[path],
        json!({}),
    )];
    let inputs = Inputs {
        scan: ScanResult {
            audio_files: vec![],
            other_files: 0,
            errors: vec![file_error(
                "bad\x1b[1m\nname.flac",
                FileErrorKind::Corrupt,
                "broken\x1b[0m\nreason",
            )],
        },
        read_errors: vec![],
        albums: vec![album],
        issues,
    };
    let report = build(&inputs);

    // JSON: the message is sanitised at the source; paths stay exact (JSON-escaped).
    let message = &report.issues[0].message;
    assert!(!message.chars().any(char::is_control), "{message:?}");
    assert!(message.contains("Title\\u{1b}]0;pwned\\u{7}"), "{message}");
    assert!(message.contains("'x\\u{a}y'"), "{message}");
    assert_eq!(report.issues[0].paths, vec![path.to_owned()]);

    for color in [false, true] {
        let text = human(&report, color);
        let plain: String = text.replace("\x1b[1m", "").replace("\x1b[0m", "");
        let plain = plain.replace("\x1b[31m", "").replace("\x1b[33m", "");
        assert!(
            !plain.chars().any(|c| c.is_control() && c != '\n'),
            "raw control character in:\n{text:?}"
        );
        assert!(
            text.contains(
                "Evil\\u{1b}[2J/Al\\u{a}bum  \"Title\\u{1b}]0;pwned\\u{7}\" by Art\\u{9b}ist"
            ),
            "{text}"
        );
        assert!(
            text.contains("- 0\\u{1b}[31m1\\u{a}fake line.flac\n"),
            "{text}"
        );
        assert!(text.contains("bad\\u{1b}[1m\\u{a}name.flac\n"), "{text}");
        assert!(text.contains("broken\\u{1b}[0m\\u{a}reason\n"), "{text}");
        assert!(!text.lines().any(|l| l.starts_with("fake line")), "{text}");
    }
}

/// Two distinct files whose names collide under lossy UTF-8 conversion are two
/// entries in `errors[]`, even with the same kind and message.
#[test]
fn colliding_lossy_error_paths_are_not_merged() {
    let error = |real: &[u8]| FileError {
        path: Path::new(ROOT).join(std::ffi::OsStr::from_bytes(real)),
        rel_path: "a\u{fffd}.flac".to_owned(),
        kind: FileErrorKind::Corrupt,
        message: "file is empty".to_owned(),
    };
    let inputs = Inputs {
        scan: ScanResult {
            audio_files: vec![],
            other_files: 0,
            errors: vec![],
        },
        read_errors: vec![
            error(b"a\xff.flac"),
            error(b"a\xfe.flac"),
            error(b"a\xff.flac"),
        ],
        albums: vec![],
        issues: vec![],
    };
    let report = build(&inputs);
    // The exact duplicate is dropped, the colliding distinct file is kept.
    assert_eq!(report.errors.len(), 2, "{:?}", report.errors);
    assert_eq!(report.summary.unreadable_files, 2);
}
