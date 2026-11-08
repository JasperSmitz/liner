//! M1: metadata reading (lofty -> TrackInfo) on synthetic fixtures.

mod common;

use common::{Corrupt, Fixture, Library, Tags};
use liner::metadata::{read_all, read_track};
use liner::model::{AudioFormat, FileErrorKind, TagKind, TrackInfo};
use liner::scan::{ScanOptions, scan};
use lofty::tag::{ItemKey, TagType};

fn full_tags() -> Tags {
    Tags::new()
        .title("Título – One")
        .artist("Artist Ä")
        .album("The Album")
        .album_artist("Various Artists")
        .track(3)
        .track_total(12)
        .disc(2)
        .disc_total(3)
        .year(2003)
        .genre("Jazz")
        .compilation(true)
}

fn expected_format(f: Fixture) -> AudioFormat {
    match f {
        Fixture::Flac => AudioFormat::Flac,
        Fixture::Mp3 => AudioFormat::Mp3,
        Fixture::OggVorbis => AudioFormat::OggVorbis,
        Fixture::Opus => AudioFormat::Opus,
        Fixture::M4a => AudioFormat::Mp4,
        Fixture::Wav => AudioFormat::Wav,
    }
}

fn expected_tag_kind(f: Fixture) -> TagKind {
    match f {
        Fixture::Flac | Fixture::OggVorbis | Fixture::Opus => TagKind::VorbisComments,
        Fixture::Mp3 | Fixture::Wav => TagKind::Id3v2,
        Fixture::M4a => TagKind::Mp4Ilst,
    }
}

fn read(lib: &Library, rel: &str) -> TrackInfo {
    read_track(&lib.scanned(rel)).unwrap_or_else(|e| panic!("{rel}: {e:?}"))
}

#[test]
fn round_trip_all_fixture_formats() {
    let lib = Library::new();
    let tags = full_tags();
    for fixture in Fixture::ALL {
        let rel = format!("A/01.{}", fixture.extension());
        lib.add(&rel, fixture, &tags);
        let t = read(&lib, &rel);
        let ctx = format!("{fixture:?}");
        assert_eq!(t.rel_path, rel, "{ctx}");
        assert_eq!(t.format, expected_format(fixture), "{ctx}");
        assert_eq!(t.title, tags.title, "{ctx}");
        assert_eq!(t.artist, tags.artist, "{ctx}");
        assert_eq!(t.album, tags.album, "{ctx}");
        assert_eq!(t.album_artist, tags.album_artist, "{ctx}");
        assert_eq!(t.track_no, tags.track, "{ctx}");
        assert_eq!(t.track_total, tags.track_total, "{ctx}");
        assert_eq!(t.disc_no, tags.disc, "{ctx}");
        assert_eq!(t.disc_total, tags.disc_total, "{ctx}");
        assert_eq!(t.year, tags.year, "{ctx}");
        assert_eq!(t.date.as_deref(), Some("2003"), "{ctx}");
        assert_eq!(t.genre, tags.genre, "{ctx}");
        assert!(t.compilation, "{ctx}");
        assert!(
            t.tag_types.contains(&expected_tag_kind(fixture)),
            "{ctx}: {:?}",
            t.tag_types
        );
        assert!(t.duration_ms.is_some_and(|ms| ms > 0), "{ctx}");
    }
}

#[test]
fn untagged_and_empty_tags_read_as_absent() {
    let lib = Library::new();
    for fixture in Fixture::ALL {
        for (rel, tagged) in [
            (format!("u/01.{}", fixture.extension()), false),
            (format!("e/01.{}", fixture.extension()), true),
        ] {
            if tagged {
                lib.add(&rel, fixture, &Tags::new());
            } else {
                lib.add_untagged(&rel, fixture);
            }
            let t = read(&lib, &rel);
            let expected = TrackInfo {
                duration_ms: t.duration_ms,
                tag_types: t.tag_types.clone(),
                ..TrackInfo::new(lib.path(&rel), rel.clone(), expected_format(fixture))
            };
            assert_eq!(t, expected, "{fixture:?} {rel}");
        }
    }
}

#[test]
fn whitespace_text_and_zero_numbers_are_absent() {
    let lib = Library::new();
    let tags = Tags::new()
        .title("   ")
        .artist("\t")
        .album(" ")
        .album_artist("  ")
        .genre(" ")
        .track(0)
        .track_total(0)
        .disc(0)
        .disc_total(0);
    for fixture in Fixture::ALL {
        let rel = format!("A/01.{}", fixture.extension());
        lib.add(&rel, fixture, &tags);
        let t = read(&lib, &rel);
        let ctx = format!("{fixture:?}");
        assert_eq!(t.title, None, "{ctx}");
        assert_eq!(t.artist, None, "{ctx}");
        assert_eq!(t.album, None, "{ctx}");
        assert_eq!(t.album_artist, None, "{ctx}");
        assert_eq!(t.genre, None, "{ctx}");
        assert_eq!(t.track_no, None, "{ctx}");
        assert_eq!(t.track_total, None, "{ctx}");
        assert_eq!(t.disc_no, None, "{ctx}");
        assert_eq!(t.disc_total, None, "{ctx}");
    }
}

#[test]
fn original_spelling_is_preserved() {
    let lib = Library::new();
    lib.add(
        "A/01.flac",
        Fixture::Flac,
        &Tags::new().album("  The  WALL ").artist("pink floyd"),
    );
    let t = read(&lib, "A/01.flac");
    assert_eq!(t.album.as_deref(), Some("  The  WALL "));
    assert_eq!(t.artist.as_deref(), Some("pink floyd"));
}

#[test]
fn slash_numbers_and_full_dates_are_parsed() {
    let lib = Library::new();
    lib.add_with("A/01.flac", Fixture::Flac, &Tags::new(), |tag| {
        tag.insert_text(ItemKey::TrackNumber, "4/9".into());
        tag.insert_text(ItemKey::DiscNumber, "1/2".into());
        tag.insert_text(ItemKey::RecordingDate, "1999-05-12".into());
    });
    let t = read(&lib, "A/01.flac");
    assert_eq!((t.track_no, t.track_total), (Some(4), Some(9)));
    assert_eq!((t.disc_no, t.disc_total), (Some(1), Some(2)));
    assert_eq!(t.year, Some(1999));
    assert_eq!(t.date.as_deref(), Some("1999-05-12"));
}

#[test]
fn year_falls_back_to_release_dates() {
    let lib = Library::new();
    lib.add_with("A/01.flac", Fixture::Flac, &Tags::new(), |tag| {
        tag.insert_text(ItemKey::OriginalReleaseDate, "1971".into());
    });
    assert_eq!(read(&lib, "A/01.flac").year, Some(1971));

    lib.add_with("A/02.flac", Fixture::Flac, &Tags::new(), |tag| {
        tag.insert_text(ItemKey::RecordingDate, "unknown".into());
        tag.insert_text(ItemKey::ReleaseDate, "2001-01-01".into());
    });
    let t = read(&lib, "A/02.flac");
    assert_eq!(t.year, Some(2001));
    assert_eq!(t.date.as_deref(), Some("2001-01-01"));

    lib.add_with("A/03.flac", Fixture::Flac, &Tags::new(), |tag| {
        tag.insert_text(ItemKey::RecordingDate, "unknown".into());
    });
    let t = read(&lib, "A/03.flac");
    assert_eq!(t.year, None);
    assert_eq!(t.date.as_deref(), Some("unknown"));
}

#[test]
fn compilation_false_and_absent() {
    let lib = Library::new();
    lib.add_with("A/01.flac", Fixture::Flac, &Tags::new(), |tag| {
        tag.insert_text(ItemKey::FlagCompilation, "0".into());
    });
    lib.add("A/02.flac", Fixture::Flac, &Tags::new().album("x"));
    assert!(!read(&lib, "A/01.flac").compilation);
    assert!(!read(&lib, "A/02.flac").compilation);
}

#[test]
fn secondary_tags_fill_missing_fields_and_are_listed() {
    let lib = Library::new();
    lib.add("A/01.mp3", Fixture::Mp3, &Tags::new().title("From ID3v2"));
    lib.add_tag(
        "A/01.mp3",
        TagType::Id3v1,
        &Tags::new().title("From ID3v1").album("V1 Album"),
    );
    let t = read(&lib, "A/01.mp3");
    assert_eq!(t.title.as_deref(), Some("From ID3v2"), "primary tag wins");
    assert_eq!(
        t.album.as_deref(),
        Some("V1 Album"),
        "fallback to other tags"
    );
    assert_eq!(t.tag_types, vec![TagKind::Id3v1, TagKind::Id3v2]);
}

#[test]
fn each_corrupt_fixture_yields_one_corrupt_error() {
    let lib = Library::new();
    for (i, kind) in Corrupt::ALL.into_iter().enumerate() {
        lib.add_corrupt(&format!("bad/{i}.flac"), kind);
    }
    let scan = scan(lib.root(), &ScanOptions::default()).unwrap();
    assert_eq!(scan.audio_files.len(), Corrupt::ALL.len());
    let res = read_all(&scan.audio_files);
    assert!(res.tracks.is_empty());
    assert_eq!(res.errors.len(), Corrupt::ALL.len());
    for (i, err) in res.errors.iter().enumerate() {
        assert_eq!(err.rel_path, format!("bad/{i}.flac"));
        assert_eq!(err.kind, FileErrorKind::Corrupt, "{err:?}");
        assert!(!err.message.is_empty());
    }
}

#[test]
fn text_renamed_to_audio_is_unsupported_or_corrupt() {
    let lib = Library::new();
    lib.add_raw("A/notes.mp3", b"this is just a text file\n");
    lib.add_raw("A/notes.flac", b"this is just a text file\n");
    lib.add_raw("A/notes.m4a", b"this is just a text file\n");
    for rel in ["A/notes.mp3", "A/notes.flac", "A/notes.m4a"] {
        let err = read_track(&lib.scanned(rel)).expect_err(rel);
        assert!(
            matches!(
                err.kind,
                FileErrorKind::Unsupported | FileErrorKind::Corrupt
            ),
            "{rel}: {err:?}"
        );
    }
}

#[test]
fn audio_with_wrong_extension_is_detected_by_content() {
    let lib = Library::new();
    lib.add("A/01.mp3", Fixture::Flac, &Tags::new().title("really flac"));
    let t = read(&lib, "A/01.mp3");
    assert_eq!(t.format, AudioFormat::Flac);
    assert_eq!(t.title.as_deref(), Some("really flac"));
}

#[test]
fn missing_file_is_io_error() {
    let lib = Library::new();
    let err = read_track(&lib.scanned("gone.flac")).unwrap_err();
    assert_eq!(err.kind, FileErrorKind::Io);
    assert_eq!(err.rel_path, "gone.flac");
}

#[cfg(unix)]
#[test]
fn unreadable_file_is_io_error() {
    let lib = Library::new();
    lib.add("A/01.flac", Fixture::Flac, &Tags::new());
    lib.chmod("A/01.flac", 0o000);
    if std::fs::File::open(lib.path("A/01.flac")).is_ok() {
        eprintln!("skipping: running with permission override (root?)");
        return;
    }
    let err = read_track(&lib.scanned("A/01.flac")).unwrap_err();
    assert_eq!(err.kind, FileErrorKind::Io, "{err:?}");
}

#[cfg(unix)]
#[test]
fn non_utf8_file_name_is_read() {
    use std::path::Path;
    let lib = Library::new();
    let name = common::non_utf8_name("Caf", ".flac");
    let rel = Path::new("A").join(&name);
    lib.add_at(&rel, Fixture::Flac, &Tags::new().title("odd name"));
    let t = read_track(&lib.scanned(&rel)).unwrap();
    assert_eq!(t.title.as_deref(), Some("odd name"));
    assert_eq!(t.rel_path, "A/Caf\u{FFFD}.flac");
}

fn mixed_library() -> Library {
    let lib = Library::new();
    for (i, fixture) in Fixture::ALL.into_iter().enumerate() {
        let base = Tags::new().album(&format!("Album {i}")).artist("Artist");
        lib.add_album(&format!("Artist/Album {i}"), fixture, &base, 5);
    }
    lib.add_album(
        "Box/CD1",
        Fixture::Flac,
        &Tags::new().album("Box").disc(1),
        4,
    );
    lib.add_album(
        "Box/CD2",
        Fixture::Flac,
        &Tags::new().album("Box").disc(2),
        4,
    );
    for (i, kind) in Corrupt::ALL.into_iter().enumerate() {
        lib.add_corrupt(&format!("Broken/{i}.flac"), kind);
    }
    lib.add_raw("Broken/text.mp3", b"nope");
    lib
}

#[test]
fn read_all_is_sorted_and_deterministic() {
    let lib = mixed_library();
    let scan = scan(lib.root(), &ScanOptions::default()).unwrap();
    let first = read_all(&scan.audio_files);
    assert_eq!(first.tracks.len(), 6 * 5 + 8);
    assert_eq!(first.errors.len(), 4);
    assert!(first.tracks.is_sorted_by(|a, b| a.rel_path <= b.rel_path));
    assert!(first.errors.is_sorted_by(|a, b| a.rel_path <= b.rel_path));

    for _ in 0..5 {
        assert_eq!(read_all(&scan.audio_files), first);
    }
    // Input order must not matter either.
    let mut reversed = scan.audio_files.clone();
    reversed.reverse();
    assert_eq!(read_all(&reversed), first);
    // Same as sequential reading.
    let sequential: Vec<_> = scan.audio_files.iter().map(read_track).collect();
    let ok: Vec<_> = sequential.iter().filter_map(|r| r.clone().ok()).collect();
    assert_eq!(ok, first.tracks);
}

#[test]
fn read_all_empty_input() {
    let res = read_all(&[]);
    assert!(res.tracks.is_empty() && res.errors.is_empty());
}
