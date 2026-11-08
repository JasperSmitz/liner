//! M1: library walking and classification.

mod common;

use common::{Corrupt, Fixture, Library, Tags};
use liner::error::LinerError;
use liner::model::FileErrorKind;
use liner::scan::{ScanOptions, ScanResult, scan};

fn rels(res: &ScanResult) -> Vec<&str> {
    res.audio_files
        .iter()
        .map(|f| f.rel_path.as_str())
        .collect()
}

fn default_scan(lib: &Library) -> ScanResult {
    scan(lib.root(), &ScanOptions::default()).expect("scan")
}

const FOLLOW: ScanOptions = ScanOptions {
    follow_symlinks: true,
    include_hidden: false,
};
const HIDDEN: ScanOptions = ScanOptions {
    follow_symlinks: false,
    include_hidden: true,
};

#[test]
fn classifies_counts_and_sorts() {
    let lib = Library::new();
    lib.add("B/Album/CD2/01.flac", Fixture::Flac, &Tags::new());
    lib.add("B/Album/CD1/01.flac", Fixture::Flac, &Tags::new());
    lib.add("A/01.mp3", Fixture::Mp3, &Tags::new());
    lib.add_raw("A/02.MP3", b"upper-case extension, junk content");
    lib.add_raw("A/cover.jpg", b"jpg");
    lib.add_raw("A/album.cue", b"cue");
    lib.add_raw("A/README", b"no extension");
    lib.add_raw("root.ogg", b"x");
    lib.add_corrupt("C/empty.flac", Corrupt::Empty);
    lib.mkdir("Empty/Dir");

    let res = default_scan(&lib);
    assert_eq!(
        rels(&res),
        [
            "A/01.mp3",
            "A/02.MP3",
            "B/Album/CD1/01.flac",
            "B/Album/CD2/01.flac",
            "C/empty.flac",
            "root.ogg",
        ]
    );
    assert_eq!(res.other_files, 3);
    assert_eq!(res.files_scanned(), 9);
    assert!(res.errors.is_empty(), "{:?}", res.errors);

    let empty = res
        .audio_files
        .iter()
        .find(|f| f.rel_path == "C/empty.flac");
    assert_eq!(empty.unwrap().size, 0);
    let mp3 = &res.audio_files[0];
    assert_eq!(mp3.path, lib.path("A/01.mp3"));
    assert_eq!(mp3.size, std::fs::metadata(&mp3.path).unwrap().len());
}

#[test]
fn all_audio_extensions_are_recognised() {
    let lib = Library::new();
    for ext in liner::scan::AUDIO_EXTENSIONS {
        lib.add_raw(&format!("x.{ext}"), b"");
        lib.add_raw(&format!("y.{}", ext.to_uppercase()), b"");
    }
    let res = default_scan(&lib);
    assert_eq!(
        res.audio_files.len(),
        2 * liner::scan::AUDIO_EXTENSIONS.len()
    );
    assert_eq!(res.other_files, 0);
}

#[test]
fn empty_library() {
    let lib = Library::new();
    assert_eq!(default_scan(&lib), ScanResult::default());
}

#[test]
fn root_errors_are_fatal() {
    let lib = Library::new();
    let missing = lib.path("nope");
    assert!(matches!(
        scan(&missing, &ScanOptions::default()),
        Err(LinerError::RootNotFound(p)) if p == missing
    ));
    let file = lib.add_raw("file.flac", b"");
    assert!(matches!(
        scan(&file, &ScanOptions::default()),
        Err(LinerError::RootNotDirectory(p)) if p == file
    ));
}

#[test]
fn hidden_entries_skipped_by_default_and_included_on_request() {
    let lib = Library::new();
    lib.add("A/01.flac", Fixture::Flac, &Tags::new());
    lib.add("A/.hidden.flac", Fixture::Flac, &Tags::new());
    lib.add(".Hidden Dir/01.flac", Fixture::Flac, &Tags::new());
    lib.add_raw(".Hidden Dir/cover.jpg", b"jpg");
    lib.add_raw("A/.DS_Store", b"junk");

    let res = default_scan(&lib);
    assert_eq!(rels(&res), ["A/01.flac"]);
    assert_eq!(res.other_files, 0);

    let res = scan(lib.root(), &HIDDEN).unwrap();
    assert_eq!(
        rels(&res),
        [".Hidden Dir/01.flac", "A/.hidden.flac", "A/01.flac"]
    );
    assert_eq!(res.other_files, 2);
}

#[test]
fn hidden_root_is_still_scanned() {
    let lib = Library::new();
    lib.add(".lib/A/01.flac", Fixture::Flac, &Tags::new());
    let res = scan(&lib.path(".lib"), &ScanOptions::default()).unwrap();
    assert_eq!(rels(&res), ["A/01.flac"]);
}

#[cfg(unix)]
#[test]
fn symlinks_not_followed_by_default() {
    let lib = Library::new();
    let outside = Library::new();
    outside.add("Album/01.flac", Fixture::Flac, &Tags::new());
    lib.add("A/01.flac", Fixture::Flac, &Tags::new());
    lib.symlink("Linked", outside.path("Album"));
    lib.symlink("A/link.flac", "01.flac");

    let res = default_scan(&lib);
    assert_eq!(rels(&res), ["A/01.flac"]);
    assert!(res.errors.is_empty());

    let res = scan(lib.root(), &FOLLOW).unwrap();
    assert_eq!(rels(&res), ["A/01.flac", "A/link.flac", "Linked/01.flac"]);
    assert_eq!(
        res.audio_files[2].path,
        lib.root().join("Linked/01.flac"),
        "paths stay under the root, not the link target"
    );
}

#[cfg(unix)]
#[test]
fn symlink_loop_with_following_is_walk_error() {
    let lib = Library::new();
    lib.add("A/B/01.flac", Fixture::Flac, &Tags::new());
    lib.symlink("A/B/loop", "../..");

    let res = default_scan(&lib);
    assert_eq!(rels(&res), ["A/B/01.flac"]);
    assert!(res.errors.is_empty());

    let res = scan(lib.root(), &FOLLOW).unwrap();
    assert_eq!(rels(&res), ["A/B/01.flac"]);
    assert_eq!(res.errors.len(), 1, "{:?}", res.errors);
    let err = &res.errors[0];
    assert_eq!(err.kind, FileErrorKind::Walk);
    assert_eq!(err.rel_path, "A/B/loop");
    assert!(err.message.contains("loop"), "{}", err.message);
}

#[cfg(unix)]
#[test]
fn broken_symlink_with_following_is_walk_error() {
    let lib = Library::new();
    lib.symlink("dangling.flac", "does-not-exist.flac");
    assert_eq!(default_scan(&lib), ScanResult::default());
    let res = scan(lib.root(), &FOLLOW).unwrap();
    assert!(res.audio_files.is_empty());
    assert_eq!(res.errors.len(), 1);
    assert_eq!(res.errors[0].kind, FileErrorKind::Walk);
    assert_eq!(res.errors[0].rel_path, "dangling.flac");
}

#[cfg(unix)]
#[test]
fn unreadable_root_is_fatal_io_error() {
    let lib = Library::new();
    lib.add("Lib/A/01.flac", Fixture::Flac, &Tags::new());
    lib.chmod("Lib", 0o000);
    let root = lib.path("Lib");
    if std::fs::read_dir(&root).is_ok() {
        eprintln!("skipping: running with permission override (root?)");
        return;
    }
    match scan(&root, &ScanOptions::default()) {
        Err(LinerError::Io { path, source }) => {
            assert_eq!(path, root);
            assert_eq!(source.kind(), std::io::ErrorKind::PermissionDenied);
        }
        other => panic!("expected LinerError::Io, got {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn unreadable_directory_is_walk_error_and_scan_continues() {
    let lib = Library::new();
    lib.add("A/01.flac", Fixture::Flac, &Tags::new());
    lib.add("Locked/01.flac", Fixture::Flac, &Tags::new());
    lib.add("Z/01.flac", Fixture::Flac, &Tags::new());
    lib.chmod("Locked", 0o000);
    if std::fs::read_dir(lib.path("Locked")).is_ok() {
        eprintln!("skipping: running with permission override (root?)");
        return;
    }

    let res = default_scan(&lib);
    assert_eq!(rels(&res), ["A/01.flac", "Z/01.flac"]);
    assert_eq!(res.errors.len(), 1, "{:?}", res.errors);
    let err = &res.errors[0];
    assert_eq!(err.kind, FileErrorKind::Walk);
    assert_eq!(err.rel_path, "Locked");
    assert_eq!(err.path, lib.path("Locked"));
    assert!(!err.message.is_empty());
}

#[cfg(unix)]
#[test]
fn non_utf8_names_do_not_panic() {
    use std::path::Path;
    let lib = Library::new();
    let dir = common::non_utf8_name("Dir", "");
    let file = common::non_utf8_name("Caf", ".flac");
    lib.add_at(Path::new(&dir).join(&file), Fixture::Flac, &Tags::new());
    // Two distinct names that collide after lossy conversion.
    lib.add_raw_at(Path::new("X").join(common::non_utf8_name("a", ".mp3")), b"");
    let other = {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(b"a\xFE.mp3".to_vec())
    };
    lib.add_raw_at(Path::new("X").join(other), b"");
    lib.add_raw_at(common::non_utf8_name("cover", ".jpg"), b"");

    let res = default_scan(&lib);
    assert_eq!(
        rels(&res),
        [
            "Dir\u{FFFD}/Caf\u{FFFD}.flac",
            "X/a\u{FFFD}.mp3",
            "X/a\u{FFFD}.mp3"
        ]
    );
    assert_eq!(res.other_files, 1);
    assert!(res.audio_files[0].path.exists());
    // Ties on the lossy rel_path are broken deterministically by the real path.
    assert!(res.audio_files[1].path < res.audio_files[2].path);
}

#[test]
fn deep_nesting() {
    let lib = Library::new();
    let deep = (0..40)
        .map(|i| format!("d{i}"))
        .collect::<Vec<_>>()
        .join("/");
    lib.add(&format!("{deep}/01.flac"), Fixture::Flac, &Tags::new());
    let res = default_scan(&lib);
    assert_eq!(rels(&res), [format!("{deep}/01.flac").as_str()]);
}

#[test]
fn repeated_scans_are_identical() {
    let lib = Library::new();
    for i in 0..20 {
        lib.add_album(
            &format!("Artist {i}/Album"),
            Fixture::ALL[i % Fixture::ALL.len()],
            &Tags::new(),
            3,
        );
        lib.add_raw(&format!("Artist {i}/Album/cover.jpg"), b"jpg");
    }
    let first = default_scan(&lib);
    assert_eq!(first.audio_files.len(), 60);
    assert_eq!(first.other_files, 20);
    for _ in 0..3 {
        assert_eq!(default_scan(&lib), first);
    }
}
