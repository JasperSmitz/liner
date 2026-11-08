//! `liner scan` through the binary: config resolution, exit code 2 paths, output
//! file safety, colour and broken pipes. Exit codes 0/1 for issue thresholds are
//! unit-tested on the pure `cli::outcome` (src/cli.rs), since they need no files.

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
    for args in [&["rules"][..], &["schema"][..]] {
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
