//! `liner scan` through the binary: config resolution, exit codes, output file
//! safety, colour and broken pipes. The full exit-code matrix for issue thresholds
//! is unit-tested on the pure `cli::outcome` (src/cli.rs); here the wiring is
//! checked end to end with unreadable (corrupt) files.

mod common;

use std::error::Error as _;
use std::path::Path;
use std::process::Stdio;

use assert_cmd::Command;
use predicates::prelude::*;

fn liner() -> Command {
    let mut cmd = Command::cargo_bin("liner").unwrap();
    // Never pick up the developer's real config (and thus never their library).
    cmd.env("XDG_CONFIG_HOME", "/nonexistent-liner-test-config");
    cmd.env("HOME", "/nonexistent-liner-test-home");
    cmd.env_remove("NO_COLOR");
    cmd
}

#[allow(clippy::disallowed_methods)] // test-only: writes a config file into a temp dir
fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn json_stdout(cmd: &mut Command) -> serde_json::Value {
    let out = cmd.assert().code(0).get_output().stdout.clone();
    serde_json::from_slice(&out).unwrap()
}

fn abs(p: &Path) -> String {
    std::path::absolute(p)
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

#[test]
fn explicit_config_supplies_library_path() {
    let lib = common::Library::new();
    let cfg_dir = tempfile::tempdir().unwrap();
    let cfg = cfg_dir.path().join("c.toml");
    write(&cfg, &format!("library_path = {:?}\n", lib.root()));
    let v = json_stdout(
        liner()
            .args(["scan", "--format", "json", "--config"])
            .arg(&cfg),
    );
    assert_eq!(v["root"], abs(lib.root()));
}

#[test]
fn cli_path_overrides_config() {
    let lib = common::Library::new();
    let other = common::Library::new();
    let cfg_dir = tempfile::tempdir().unwrap();
    let cfg = cfg_dir.path().join("c.toml");
    write(&cfg, &format!("library_path = {:?}\n", other.root()));
    let v = json_stdout(
        liner()
            .args(["scan", "--format", "json", "--config"])
            .arg(&cfg)
            .arg(lib.root()),
    );
    assert_eq!(v["root"], abs(lib.root()));
}

#[test]
fn default_config_from_xdg_config_home() {
    let lib = common::Library::new();
    let xdg = tempfile::tempdir().unwrap();
    write(
        &xdg.path().join("liner/config.toml"),
        &format!("library_path = {:?}\n", lib.root()),
    );
    let v = json_stdout(
        liner()
            .env("XDG_CONFIG_HOME", xdg.path())
            .args(["scan", "--format", "json"]),
    );
    assert_eq!(v["root"], abs(lib.root()));
}

#[test]
fn default_config_from_home_when_xdg_unset() {
    let lib = common::Library::new();
    let home = tempfile::tempdir().unwrap();
    write(
        &home.path().join(".config/liner/config.toml"),
        &format!("library_path = {:?}\n", lib.root()),
    );
    let v = json_stdout(
        liner()
            .env_remove("XDG_CONFIG_HOME")
            .env("HOME", home.path())
            .args(["scan", "--format", "json"]),
    );
    assert_eq!(v["root"], abs(lib.root()));
}

#[test]
fn missing_default_config_is_ignored() {
    let lib = common::Library::new();
    let empty = tempfile::tempdir().unwrap();
    liner()
        .env("XDG_CONFIG_HOME", empty.path())
        .arg("scan")
        .arg(lib.root())
        .assert()
        .code(0);
}

#[test]
fn explicit_missing_config_exits_2() {
    let lib = common::Library::new();
    liner()
        .arg("scan")
        .arg(lib.root())
        .args(["--config", "/nonexistent/liner.toml"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot read config file"));
}

#[test]
fn unknown_config_key_exits_2() {
    let lib = common::Library::new();
    let xdg = tempfile::tempdir().unwrap();
    write(
        &xdg.path().join("liner/config.toml"),
        "libary_path = \"/x\"\n",
    );
    liner()
        .env("XDG_CONFIG_HOME", xdg.path())
        .arg("scan")
        .arg(lib.root())
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("invalid config file")
                .and(predicate::str::contains("libary_path")),
        );
}

#[test]
fn no_library_path_exits_2() {
    liner()
        .arg("scan")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("no library path"));
}

#[test]
fn bad_root_exits_2() {
    liner()
        .args(["scan", "/nonexistent-liner-test-library"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("does not exist"));
}

#[cfg(unix)]
#[test]
fn unreadable_root_exits_2() {
    let lib = common::Library::new();
    lib.mkdir("Lib");
    lib.chmod("Lib", 0o000);
    if std::fs::read_dir(lib.path("Lib")).is_ok() {
        eprintln!("skipping: running with permission override (root?)");
        return;
    }
    liner()
        .arg("scan")
        .arg(lib.path("Lib"))
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Permission denied"));
}

#[test]
fn unknown_flag_and_bad_values_exit_2() {
    liner().args(["scan", "--frobnicate"]).assert().code(2);
    liner()
        .args(["scan", "--fail-on", "sometimes"])
        .assert()
        .code(2);
    liner()
        .args(["scan", "--color", "rainbow"])
        .assert()
        .code(2);
}

#[test]
fn output_to_existing_directory_is_refused() {
    let lib = common::Library::new();
    let out = tempfile::tempdir().unwrap();
    let mut dir_arg = out.path().as_os_str().to_owned();
    dir_arg.push("/");
    liner()
        .arg("scan")
        .arg(lib.root())
        .arg("--output")
        .arg(&dir_arg)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("not a regular file"));
}

#[test]
fn output_file_receives_report() {
    let lib = common::Library::new();
    let out = tempfile::tempdir().unwrap();
    let file = out.path().join("report.json");
    liner()
        .args(["scan", "--format", "json", "--color", "always"])
        .arg(lib.root())
        .arg("--output")
        .arg(&file)
        .assert()
        .code(0)
        .stdout("");
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(v["root"], abs(lib.root()));
}

#[test]
fn colour_only_on_request_when_piped() {
    let lib = common::Library::new();
    // assert_cmd captures stdout through a pipe: auto => no colour.
    liner().arg("scan").arg(lib.root()).assert().code(0).stdout(
        predicate::str::contains("No issues found.").and(predicate::str::contains("\x1b").not()),
    );
    liner()
        .args(["scan", "--color", "always"])
        .arg(lib.root())
        .assert()
        .code(0)
        .stdout(predicate::str::contains("\x1b[1m"));
    // NO_COLOR does not override an explicit --color always.
    liner()
        .env("NO_COLOR", "1")
        .args(["scan", "--color", "always"])
        .arg(lib.root())
        .assert()
        .stdout(predicate::str::contains("\x1b["));
    // JSON never contains colour.
    liner()
        .args(["scan", "--format", "json", "--color", "always"])
        .arg(lib.root())
        .assert()
        .stdout(predicate::str::contains("\x1b").not());
}

#[test]
fn closed_stdout_exits_quietly() {
    // Smoke test of `liner rules | head -0`; the EPIPE path itself is unit-tested
    // with a failing writer in src/cli.rs (the race here may let the write succeed).
    let bin = assert_cmd::cargo::cargo_bin("liner");
    let lib = common::Library::new();
    lib.add("A/B/01.flac", common::Fixture::Flac, &common::Tags::new());
    let root = lib.root().to_str().unwrap();
    for args in [
        &["rules"][..],
        &["schema"][..],
        &["scan", root][..],
        &["scan", root, "--format", "json"][..],
    ] {
        let mut child = std::process::Command::new(&bin)
            .args(args)
            .env("XDG_CONFIG_HOME", "/nonexistent-liner-test-config")
            .env("HOME", "/nonexistent-liner-test-home")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdout.take());
        let out = child.wait_with_output().unwrap();
        assert_eq!(out.status.code(), Some(0), "{args:?}");
        assert!(
            out.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn error_display_never_repeats_its_source() {
    use liner::config::ConfigError;
    use liner::error::LinerError;
    let io = || std::io::Error::other("disk on fire");
    let errors: Vec<LinerError> = vec![
        LinerError::Io {
            path: "/x".into(),
            source: io(),
        },
        LinerError::Output(io()),
        LinerError::Config(ConfigError::Read {
            path: "/c.toml".into(),
            source: io(),
        }),
        liner::config::parse("bogus = 1", Path::new("/c.toml"))
            .unwrap_err()
            .into(),
    ];
    for err in errors {
        let shown = err.to_string();
        let mut source = err.source();
        while let Some(s) = source {
            assert!(!shown.contains(&s.to_string()), "{shown:?} repeats {s}");
            source = s.source();
        }
    }
}

/// Bytes and inode metadata of a file, to prove liner left it alone.
fn snapshot(path: &Path) -> (Vec<u8>, std::time::SystemTime) {
    let meta = std::fs::metadata(path).unwrap();
    (std::fs::read(path).unwrap(), meta.modified().unwrap())
}

fn library_with_track() -> (common::Library, std::path::PathBuf) {
    let lib = common::Library::new();
    let track = lib.add(
        "A/B/01.flac",
        common::Fixture::Flac,
        &common::Tags::album_track("B", "A", 1),
    );
    (lib, track)
}

#[test]
fn output_hard_link_to_audio_is_refused() {
    let (lib, track) = library_with_track();
    let before = snapshot(&track);
    let out = tempfile::tempdir().unwrap();
    let report = out.path().join("report.json");
    std::fs::hard_link(&track, &report).unwrap();
    liner()
        .args(["scan", "--format", "json"])
        .arg(lib.root())
        .arg("--output")
        .arg(&report)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("hard links"));
    assert_eq!(snapshot(&track), before);
}

#[test]
fn output_symlink_to_audio_is_refused() {
    let (lib, track) = library_with_track();
    let before = snapshot(&track);
    let out = tempfile::tempdir().unwrap();
    let report = out.path().join("report.json");
    std::os::unix::fs::symlink(&track, &report).unwrap();
    liner()
        .args(["scan", "--format", "json"])
        .arg(lib.root())
        .arg("--output")
        .arg(&report)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("not a regular file"));
    assert_eq!(snapshot(&track), before);
}

#[test]
fn fatal_error_leaves_existing_output_untouched() {
    let out = tempfile::tempdir().unwrap();
    let keep = out.path().join("keep.json");
    write(&keep, "precious\n");
    liner()
        .args(["scan", "/nonexistent-liner-test-library", "--output"])
        .arg(&keep)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("does not exist"));
    assert_eq!(std::fs::read_to_string(&keep).unwrap(), "precious\n");
}

#[test]
fn output_inside_library_is_refused() {
    let (lib, _) = library_with_track();
    for rel in ["r.json", "A/B/album.cue", "A/new.txt"] {
        let target = lib.path(rel);
        liner()
            .arg("scan")
            .arg(lib.root())
            .arg("--output")
            .arg(&target)
            .assert()
            .code(2)
            .stderr(predicate::str::contains("inside the library"));
        assert!(!target.exists(), "{rel}");
    }
    // Also through a relative library path and a symlinked output directory.
    let out = tempfile::tempdir().unwrap();
    let link = out.path().join("lib-link");
    std::os::unix::fs::symlink(lib.path("A"), &link).unwrap();
    liner()
        .current_dir(lib.root())
        .args(["scan", ".", "--output"])
        .arg(link.join("x.json"))
        .assert()
        .code(2)
        .stderr(predicate::str::contains("inside the library"));
    assert!(!lib.path("A/x.json").exists());
}

#[test]
fn output_with_audio_extension_is_refused_in_any_case() {
    let lib = common::Library::new();
    let out = tempfile::tempdir().unwrap();
    for name in ["report.flac", "REPORT.FLAC", "r.Mp3"] {
        let target = out.path().join(name);
        liner()
            .arg("scan")
            .arg(lib.root())
            .arg("--output")
            .arg(&target)
            .assert()
            .code(2)
            .stderr(predicate::str::contains("audio file extension"));
        assert!(!target.exists(), "{name}");
    }
}

#[test]
fn output_in_missing_directory_exits_2() {
    let lib = common::Library::new();
    liner()
        .arg("scan")
        .arg(lib.root())
        .args(["--output", "/nonexistent-liner-test-dir/r.json"])
        .assert()
        .code(2);
}

#[test]
fn existing_output_file_is_replaced() {
    let lib = common::Library::new();
    let out = tempfile::tempdir().unwrap();
    let file = out.path().join("report.txt");
    write(&file, &"x".repeat(100_000));
    liner()
        .arg("scan")
        .arg(lib.root())
        .arg("--output")
        .arg(&file)
        .assert()
        .code(0)
        .stdout("");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.starts_with("No issues found.\n"), "{text:.200}");
    assert!(!text.contains('x'));
}

fn corrupt_library() -> common::Library {
    let lib = common::Library::new();
    lib.add_corrupt("A/B/01.flac", common::Corrupt::Truncated);
    lib
}

#[test]
fn unreadable_file_exits_1_through_the_binary() {
    let lib = corrupt_library();
    let out = liner()
        .args(["scan", "--format", "json"])
        .arg(lib.root())
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["summary"]["unreadable_files"], 1);
    assert_eq!(v["errors"][0]["path"], "A/B/01.flac");
    assert_eq!(v["errors"][0]["kind"], "corrupt");

    // Human output says why it failed.
    liner()
        .arg("scan")
        .arg(lib.root())
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Unreadable files (1)"));
}

#[test]
fn fail_on_never_ignores_unreadable_files() {
    let lib = corrupt_library();
    liner()
        .args(["scan", "--fail-on", "never"])
        .arg(lib.root())
        .assert()
        .code(0);
}

#[test]
fn min_severity_does_not_hide_unreadable_files_from_exit_code() {
    let lib = corrupt_library();
    let out = liner()
        .args(["scan", "--format", "json", "--min-severity", "error"])
        .arg(lib.root())
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["options"]["min_severity"], "error");
    assert_eq!(v["summary"]["unreadable_files"], 1);
}

#[test]
fn report_records_scan_options() {
    let lib = common::Library::new();
    let v = json_stdout(
        liner()
            .args(["scan", "--format", "json", "--include-hidden"])
            .arg(lib.root()),
    );
    assert_eq!(
        v["options"],
        serde_json::json!({"min_severity": "info", "follow_symlinks": false, "include_hidden": true})
    );
    assert_eq!(v["summary"]["issues_hidden"], 0);
}

#[test]
fn relative_library_path_is_relative_to_config_file() {
    let cfg_dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(cfg_dir.path().join("music")).unwrap();
    let cfg = cfg_dir.path().join("c.toml");
    write(&cfg, "library_path = \"music\"\n");
    let elsewhere = tempfile::tempdir().unwrap();
    let v = json_stdout(
        liner()
            .current_dir(elsewhere.path())
            .args(["scan", "--format", "json", "--config"])
            .arg(&cfg),
    );
    assert_eq!(v["root"], abs(&cfg_dir.path().join("music")));
}

/// Directory entries other than `keep` (the report must be the only thing left).
fn dir_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn output_leaves_no_temporary_file() {
    let lib = common::Library::new();
    let out = tempfile::tempdir().unwrap();
    let file = out.path().join("report.json");
    for _ in 0..2 {
        // Create, then replace.
        liner()
            .args(["scan", "--format", "json"])
            .arg(lib.root())
            .arg("--output")
            .arg(&file)
            .assert()
            .code(0);
        assert_eq!(dir_names(out.path()), ["report.json"]);
    }
    let mode =
        std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&file).unwrap().permissions());
    assert_eq!(mode & 0o777 & !0o022, 0o644 & !0o022, "{mode:o}");
}

#[test]
fn output_through_symlinked_parent_into_library_is_refused() {
    let (lib, track) = library_with_track();
    let before = snapshot(&track);
    let out = tempfile::tempdir().unwrap();
    // A symlink to the library root, and a chain of symlinks into a sub-directory.
    std::os::unix::fs::symlink(lib.root(), out.path().join("lib")).unwrap();
    std::os::unix::fs::symlink(lib.path("A"), out.path().join("a1")).unwrap();
    std::os::unix::fs::symlink(out.path().join("a1"), out.path().join("a2")).unwrap();
    for target in ["lib/r.json", "lib/A/B/r.json", "a2/r.json", "a2/B/r.json"] {
        liner()
            .args(["scan", "--format", "json"])
            .arg(lib.root())
            .arg("--output")
            .arg(out.path().join(target))
            .assert()
            .code(2)
            .stderr(predicate::str::contains("inside the library"));
    }
    assert!(!lib.path("r.json").exists());
    assert!(!lib.path("A/r.json").exists());
    assert!(!lib.path("A/B/r.json").exists());
    assert_eq!(snapshot(&track), before);
}

#[test]
fn hard_link_destination_is_refused_without_leftovers() {
    let (lib, track) = library_with_track();
    let before = snapshot(&track);
    let out = tempfile::tempdir().unwrap();
    let report = out.path().join("report.txt");
    std::fs::hard_link(&track, &report).unwrap();
    liner()
        .arg("scan")
        .arg(lib.root())
        .arg("--output")
        .arg(&report)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("hard links"));
    assert_eq!(snapshot(&track), before);
    assert_eq!(snapshot(&report), before);
    assert_eq!(dir_names(out.path()), ["report.txt"]);
}

/// A write that fails half-way (here: `RLIMIT_FSIZE`, as on a full disk or quota)
/// leaves an existing report exactly as it was, and no temporary file behind.
#[cfg(target_os = "linux")]
#[test]
fn failed_write_preserves_existing_report() {
    use std::os::unix::process::CommandExt;
    let lib = common::Library::new();
    let out = tempfile::tempdir().unwrap();
    let file = out.path().join("report.json");
    let old = "o".repeat(1500);
    write(&file, &old);

    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_liner"));
    cmd.env("XDG_CONFIG_HOME", "/nonexistent-liner-test-config")
        .env("HOME", "/nonexistent-liner-test-home")
        .args(["scan", "--format", "json"])
        .arg(lib.root())
        .arg("--output")
        .arg(&file);
    // SAFETY: only async-signal-safe libc calls between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            let limit = libc::rlimit {
                rlim_cur: 64,
                rlim_max: 64,
            };
            if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
            Ok(())
        });
    }
    let output = cmd.output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("failed to write output"), "{stderr}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), old);
    assert_eq!(dir_names(out.path()), ["report.json"]);
}
