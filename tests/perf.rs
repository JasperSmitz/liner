//! Performance smoke test (M4): 2 000 generated files must be analysed by the
//! binary in under 10 s. Ignored by default because building the library takes a
//! while; the time limit is only asserted in release builds. Run with:
//!
//!   cargo test --release --test perf -- --ignored --nocapture

mod common;

use std::time::{Duration, Instant};

use assert_cmd::Command;
use common::{Fixture, Library, Tags};

const ALBUMS: u32 = 200;
const TRACKS: u32 = 10;
const LIMIT: Duration = Duration::from_secs(10);

#[test]
#[ignore = "slow; run with --release -- --ignored"]
fn two_thousand_files_under_ten_seconds() {
    let lib = Library::new();
    let setup = Instant::now();
    for a in 0..ALBUMS {
        let fixture = Fixture::ALL[a as usize % Fixture::ALL.len()];
        let artist = format!("Artist {:02}", a / 4);
        let album = format!("Album {a:03}");
        let tags = Tags::new()
            .album(&album)
            .artist(&artist)
            .album_artist(&artist)
            .year(1970 + a % 50)
            .genre("Test");
        // Every fifth album is split over two disc folders.
        if a % 5 == 0 {
            let half = TRACKS / 2;
            for d in 1..=2 {
                let dir = format!("{artist}/{album}/CD{d}");
                lib.add_album(&dir, fixture, &tags.clone().disc(d).disc_total(2), half);
            }
        } else {
            lib.add_album(&format!("{artist}/{album}"), fixture, &tags, TRACKS);
        }
    }
    eprintln!("built library in {:.2?}", setup.elapsed());

    let start = Instant::now();
    let out = Command::cargo_bin("liner")
        .unwrap()
        .env("XDG_CONFIG_HOME", "/nonexistent-liner-test-config")
        .env("HOME", "/nonexistent-liner-test-home")
        .arg("scan")
        .arg(lib.root())
        .args(["--format", "json", "--fail-on", "never"])
        .output()
        .unwrap();
    let elapsed = start.elapsed();
    eprintln!("liner scan of {} files took {elapsed:.2?}", ALBUMS * TRACKS);

    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["summary"]["audio_files"], u64::from(ALBUMS * TRACKS));
    assert_eq!(report["errors"], serde_json::json!([]));
    if cfg!(debug_assertions) {
        eprintln!("debug build: time limit not asserted (use --release)");
    } else {
        assert!(
            elapsed < LIMIT,
            "analysis took {elapsed:.2?} (limit {LIMIT:?})"
        );
    }
}
