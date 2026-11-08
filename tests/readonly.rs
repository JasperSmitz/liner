//! Read-only guarantee, end to end (ARCHITECTURE.md "Read-only guarantees" #5):
//! snapshot every entry of a synthetic library (bytes, size, mtime, permissions,
//! file type, symlink target) and the directory listing, run `liner scan` through
//! the binary in every output mode, and require an identical snapshot afterwards.
#![cfg(unix)]

mod common;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use common::{Corrupt, Fixture, Library, Tags};

fn liner() -> Command {
    let mut cmd = Command::cargo_bin("liner").unwrap();
    // Never pick up the developer's real config (and thus never their library).
    cmd.env("XDG_CONFIG_HOME", "/nonexistent-liner-test-config");
    cmd.env("HOME", "/nonexistent-liner-test-home");
    cmd
}

/// Everything observable about one directory entry, without following symlinks.
#[derive(Debug, PartialEq, Eq)]
struct Entry {
    kind: &'static str,
    size: u64,
    mode: u32,
    mtime: (i64, i64),
    ctime: (i64, i64),
    /// File contents, or the symlink target.
    content: Vec<u8>,
}

type Snapshot = BTreeMap<OsString, Entry>;

/// Recursive snapshot of `root` (including `root` itself under the key `""`).
fn snapshot(root: &Path) -> Snapshot {
    let mut snap = Snapshot::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        let path = root.join(&rel);
        let meta = std::fs::symlink_metadata(&path).unwrap();
        let ft = meta.file_type();
        let (kind, content) = if ft.is_symlink() {
            let target = std::fs::read_link(&path).unwrap();
            ("symlink", target.into_os_string().into_encoded_bytes())
        } else if ft.is_dir() {
            let mut children: Vec<OsString> = std::fs::read_dir(&path)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            children.sort();
            for c in &children {
                stack.push(rel.join(c));
            }
            // The listing itself is part of the snapshot: no entry may appear or vanish.
            let listing = children.join(std::ffi::OsStr::new("\0"));
            ("dir", listing.into_encoded_bytes())
        } else {
            ("file", std::fs::read(&path).unwrap())
        };
        snap.insert(
            rel.into_os_string(),
            Entry {
                kind,
                size: meta.size(),
                mode: meta.mode(),
                mtime: (meta.mtime(), meta.mtime_nsec()),
                ctime: (meta.ctime(), meta.ctime_nsec()),
                content,
            },
        );
    }
    snap
}

/// Several albums (one with disc folders), all six formats, corrupt files, a
/// renamed text file, non-audio files, symlinks (dir, file, loop) and a hidden dir.
fn build_library() -> Library {
    let lib = Library::new();
    let base = Tags::new()
        .album("Two Discs")
        .artist("Band")
        .album_artist("Band")
        .year(2001);
    lib.add_album(
        "Band/Two Discs/CD1",
        Fixture::Flac,
        &base.clone().disc(1),
        3,
    );
    lib.add_album(
        "Band/Two Discs/CD2",
        Fixture::Flac,
        &base.clone().disc(2),
        2,
    );
    lib.add_raw("Band/Two Discs/cover.jpg", b"not really a jpeg");
    for (i, fixture) in Fixture::ALL.into_iter().enumerate() {
        let n = i as u32 + 1;
        let tags = Tags::album_track("Formats", "Various", n).title(&format!("Song {n}"));
        lib.add(
            &format!("Various/Formats/{n:02}.{}", fixture.extension()),
            fixture,
            &tags,
        );
    }
    lib.add_album(
        "Solo/Untidy",
        Fixture::Mp3,
        &Tags::new().album("Untidy").artist("Solo"),
        4,
    );
    lib.add_untagged("Solo/Untidy/05.mp3", Fixture::Mp3);
    for kind in Corrupt::ALL {
        lib.add_corrupt(&format!("Broken/{kind:?}.flac"), kind);
    }
    lib.add_raw("Broken/notes.mp3", b"this is a text file, not an mp3\n");
    lib.add_album(
        ".hidden/Secret",
        Fixture::OggVorbis,
        &Tags::album_track("Secret", "Ghost", 1),
        2,
    );
    lib.add_raw(".DS_Store", b"\0\0\0\x01Bud1");
    lib.symlink("Linked Album", "Band/Two Discs");
    lib.symlink("Solo/link.flac", "../Band/Two Discs/CD1/01.flac");
    lib.symlink("Loop/again", "..");
    lib
}

fn assert_unchanged(lib: &Library, before: &Snapshot, what: &str) {
    let after = snapshot(lib.root());
    let added: Vec<_> = after.keys().filter(|k| !before.contains_key(*k)).collect();
    let removed: Vec<_> = before.keys().filter(|k| !after.contains_key(*k)).collect();
    assert!(
        added.is_empty() && removed.is_empty(),
        "{what}: entries added {added:?}, removed {removed:?}"
    );
    for (k, b) in before {
        assert!(after[k] == *b, "{what}: {k:?} changed");
    }
}

#[test]
fn scan_never_modifies_the_library() {
    let lib = build_library();
    let before = snapshot(lib.root());
    assert!(before.len() > 30, "fixture library unexpectedly small");
    let out_dir = tempfile::tempdir().unwrap();

    let runs: Vec<(&str, Vec<OsString>)> = vec![
        ("json stdout", vec!["--format".into(), "json".into()]),
        ("human stdout", vec!["--format".into(), "human".into()]),
        (
            "json --output",
            vec![
                "--format".into(),
                "json".into(),
                "--output".into(),
                out_dir.path().join("report.json").into(),
            ],
        ),
        (
            "human --output",
            vec![
                "--color".into(),
                "always".into(),
                "--output".into(),
                out_dir.path().join("report.txt").into(),
            ],
        ),
        (
            "follow symlinks + hidden",
            vec![
                "--format".into(),
                "json".into(),
                "--follow-symlinks".into(),
                "--include-hidden".into(),
                "--min-severity".into(),
                "error".into(),
            ],
        ),
    ];

    for (what, args) in runs {
        let out = liner()
            .arg("scan")
            .arg(lib.root())
            .args(&args)
            .output()
            .unwrap();
        // Corrupt files are in errors[], so a healthy run exits 1 (never 2).
        assert_eq!(
            out.status.code(),
            Some(1),
            "{what}: stderr {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_unchanged(&lib, &before, what);
    }

    let json = std::fs::read(out_dir.path().join("report.json")).unwrap();
    let report: serde_json::Value = serde_json::from_slice(&json).unwrap();
    assert!(report["summary"]["audio_files"].as_u64().unwrap() >= 15);
    assert!(!report["errors"].as_array().unwrap().is_empty());
    let human = std::fs::read_to_string(out_dir.path().join("report.txt")).unwrap();
    assert!(!human.is_empty());
}

#[test]
fn refused_output_inside_library_leaves_it_untouched() {
    let lib = build_library();
    let before = snapshot(lib.root());
    for target in [
        lib.root().join("report.json"),
        lib.root().join("Band/Two Discs/CD1/01.flac"),
        lib.root().join("Linked Album/report.json"),
    ] {
        liner()
            .arg("scan")
            .arg(lib.root())
            .arg("--output")
            .arg(&target)
            .assert()
            .code(2);
        assert_unchanged(&lib, &before, &format!("--output {target:?}"));
    }
}

/// Guard against a vacuous snapshot: a content change with the same size and a
/// new file are both detected.
#[test]
#[allow(clippy::disallowed_methods)] // test-only: deliberately modifies the temp library
fn snapshot_detects_changes() {
    let lib = build_library();
    let before = snapshot(lib.root());
    let path = lib.root().join("Broken/notes.mp3");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[0] ^= 1;
    std::fs::write(&path, &bytes).unwrap();
    let after = snapshot(lib.root());
    assert!(
        after[std::ffi::OsStr::new("Broken/notes.mp3")]
            != before[std::ffi::OsStr::new("Broken/notes.mp3")]
    );

    lib.add_raw("Broken/new.txt", b"x");
    let after = snapshot(lib.root());
    assert!(after.len() == before.len() + 1);
    assert!(after[std::ffi::OsStr::new("Broken")] != before[std::ffi::OsStr::new("Broken")]);
}

/// Codex final-review #1: the output parent is swapped for a symlink into the
/// library after validation (i.e. during the scan). The write goes through the
/// directory handle opened at validation time, so it lands in the original
/// directory and the library is untouched.
#[test]
#[allow(clippy::disallowed_methods)] // test-only: renames temp directories
fn output_parent_swapped_after_validation_cannot_redirect_the_write() {
    let lib = build_library();
    let before = snapshot(lib.root());
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    std::fs::create_dir(&out).unwrap();

    let target = liner::render::check_output_path(&out.join("report.json"), lib.root()).unwrap();
    std::fs::rename(&out, tmp.path().join("original-out")).unwrap();
    std::os::unix::fs::symlink(lib.root(), &out).unwrap();
    target.write(b"report\n").unwrap();

    assert_unchanged(&lib, &before, "swapped output parent");
    assert_eq!(
        std::fs::read(tmp.path().join("original-out/report.json")).unwrap(),
        b"report\n"
    );
}

/// The output directory itself is moved into the library during the scan: the
/// library check is repeated before writing.
#[test]
#[allow(clippy::disallowed_methods)] // test-only: moves a temp directory into the temp library
fn output_directory_moved_into_library_is_refused() {
    let lib = build_library();
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let target = liner::render::check_output_path(&out.join("report.json"), lib.root()).unwrap();
    let moved = lib.root().join("Band/moved");
    std::fs::rename(&out, &moved).unwrap();
    let err = target.write(b"report\n").unwrap_err();
    assert!(err.to_string().contains("inside the library"), "{err}");
    assert_eq!(std::fs::read_dir(&moved).unwrap().count(), 0);
}

/// A failing write removes the temporary file and keeps the old report.
#[test]
#[allow(clippy::disallowed_methods)] // test-only: writes the old report into a temp dir
fn failed_write_removes_temporary_file() {
    let lib = build_library();
    let tmp = tempfile::tempdir().unwrap();
    let report = tmp.path().join("report.json");
    std::fs::write(&report, "old\n").unwrap();
    let target = liner::render::check_output_path(&report, lib.root()).unwrap();
    let err = target
        .write_with(b"new\n", |file, bytes| {
            use std::io::Write;
            file.write_all(&bytes[..2])?;
            Err(std::io::ErrorKind::StorageFull.into())
        })
        .unwrap_err();
    assert!(
        matches!(err, liner::error::LinerError::Output(_)),
        "{err:?}"
    );
    assert_eq!(std::fs::read_to_string(&report).unwrap(), "old\n");
    let names: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, [std::ffi::OsString::from("report.json")]);
}
