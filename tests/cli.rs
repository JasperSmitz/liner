//! End-to-end smoke tests of the binary. More in M3/M4.

mod common;

use assert_cmd::Command;
use predicates::prelude::*;

fn liner() -> Command {
    let mut cmd = Command::cargo_bin("liner").unwrap();
    // Never pick up the developer's real config (and thus never their library).
    cmd.env("XDG_CONFIG_HOME", "/nonexistent-liner-test-config");
    cmd.env("HOME", "/nonexistent-liner-test-home");
    cmd
}

#[test]
fn rules_lists_codes() {
    liner()
        .arg("rules")
        .assert()
        .success()
        .stdout(predicate::str::contains("duplicate-track-position"));
}

#[test]
fn schema_is_json() {
    let out = liner()
        .arg("schema")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["title"], "Liner report");
}

#[test]
fn scan_empty_dir_json_has_header() {
    let lib = common::Library::new();
    let out = liner()
        .args(["scan", "--format", "json"])
        .arg(lib.root())
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["schema_version"], liner::report::SCHEMA_VERSION);
    assert_eq!(v["tool"]["name"], "liner");
}

#[test]
fn output_to_audio_extension_is_refused() {
    let lib = common::Library::new();
    liner()
        .arg("scan")
        .arg(lib.root())
        .arg("--output")
        .arg(lib.root().join("report.flac"))
        .assert()
        .code(2)
        .stderr(predicate::str::contains("audio file extension"));
    assert!(!lib.root().join("report.flac").exists());
}

#[test]
fn usage_error_exits_2() {
    liner().args(["scan", "--format", "xml"]).assert().code(2);
}
