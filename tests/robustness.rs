//! Robustness through the binary: odd libraries must produce a valid report and a
//! sensible exit code, never a crash. Every JSON report is validated against the
//! committed schema (`schema/liner-report.v1.schema.json`) by a small validator
//! for the keywords the schema uses (`jsonschema` is not available offline).

mod common;

use assert_cmd::Command;
use common::{Corrupt, Fixture, Library, Tags};
use serde_json::Value;

fn liner() -> Command {
    let mut cmd = Command::cargo_bin("liner").unwrap();
    // Never pick up the developer's real config (and thus never their library).
    cmd.env("XDG_CONFIG_HOME", "/nonexistent-liner-test-config");
    cmd.env("HOME", "/nonexistent-liner-test-home");
    cmd.env_remove("NO_COLOR");
    cmd
}

fn schema() -> Value {
    let text = include_str!("../schema/liner-report.v1.schema.json");
    serde_json::from_str(text).unwrap()
}

/// Validate `value` against `node` of `root` (a JSON Schema). Supports `$ref` (to
/// `#/$defs/..`), `type` (string or list), `enum`, `const`, `required`,
/// `properties`, `additionalProperties`, `items`, `anyOf`, `oneOf`, `minimum`.
/// `format` and annotations are ignored. Errors are pushed as `path: message`.
fn validate(root: &Value, node: &Value, value: &Value, at: &str, errs: &mut Vec<String>) {
    let Some(obj) = node.as_object() else {
        return; // `true` schema
    };
    if let Some(r) = obj.get("$ref").and_then(Value::as_str) {
        let target = root
            .pointer(r.strip_prefix('#').expect("local $ref"))
            .unwrap_or_else(|| panic!("unresolved $ref {r}"));
        validate(root, target, value, at, errs);
    }
    if let Some(t) = obj.get("type") {
        let types: Vec<&str> = match t {
            Value::String(s) => vec![s.as_str()],
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            _ => panic!("unexpected type keyword {t}"),
        };
        let ok = types.iter().any(|t| match *t {
            "null" => value.is_null(),
            "boolean" => value.is_boolean(),
            "string" => value.is_string(),
            "integer" => value.is_u64() || value.is_i64(),
            "number" => value.is_number(),
            "array" => value.is_array(),
            "object" => value.is_object(),
            other => panic!("unknown type {other}"),
        });
        if !ok {
            errs.push(format!("{at}: {value} is not of type {types:?}"));
            return;
        }
    }
    if let Some(e) = obj.get("enum").and_then(Value::as_array)
        && !e.contains(value)
    {
        errs.push(format!("{at}: {value} not in enum {e:?}"));
    }
    if let Some(c) = obj.get("const")
        && c != value
    {
        errs.push(format!("{at}: {value} != const {c}"));
    }
    if let (Some(min), Some(n)) = (obj.get("minimum").and_then(Value::as_f64), value.as_f64())
        && n < min
    {
        errs.push(format!("{at}: {n} < minimum {min}"));
    }
    for (kw, need_exactly_one) in [("anyOf", false), ("oneOf", true)] {
        if let Some(alts) = obj.get(kw).and_then(Value::as_array) {
            let passing = alts
                .iter()
                .filter(|alt| {
                    let mut sub = Vec::new();
                    validate(root, alt, value, at, &mut sub);
                    sub.is_empty()
                })
                .count();
            if passing == 0 || (need_exactly_one && passing != 1) {
                errs.push(format!("{at}: {passing} alternatives of {kw} match"));
            }
        }
    }
    if let Some(map) = value.as_object() {
        for key in obj
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let key = key.as_str().unwrap();
            if !map.contains_key(key) {
                errs.push(format!("{at}: missing required key {key:?}"));
            }
        }
        let props = obj.get("properties").and_then(Value::as_object);
        for (k, v) in map {
            let sub_at = format!("{at}/{k}");
            match props.and_then(|p| p.get(k)) {
                Some(s) => validate(root, s, v, &sub_at, errs),
                None => match obj.get("additionalProperties") {
                    Some(Value::Bool(false)) => errs.push(format!("{sub_at}: unexpected key")),
                    Some(s) => validate(root, s, v, &sub_at, errs),
                    None => {}
                },
            }
        }
    }
    if let (Some(items), Some(arr)) = (obj.get("items"), value.as_array()) {
        for (i, v) in arr.iter().enumerate() {
            validate(root, items, v, &format!("{at}/{i}"), errs);
        }
    }
}

fn assert_valid_report(report: &Value) {
    let schema = schema();
    let mut errs = Vec::new();
    validate(&schema, &schema, report, "", &mut errs);
    assert!(
        errs.is_empty(),
        "report violates schema:\n{}",
        errs.join("\n")
    );
}

/// Run `liner scan <lib> --format json [extra]`, check the exit code, parse and
/// validate the report. Human output is produced too and must not fail.
fn scan_json(lib: &Library, extra: &[&str], code: i32) -> Value {
    let out = liner()
        .arg("scan")
        .arg(lib.root())
        .args(["--format", "json"])
        .args(extra)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(code),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    assert_valid_report(&report);

    let human = liner()
        .arg("scan")
        .arg(lib.root())
        .args(extra)
        .output()
        .unwrap();
    assert_eq!(human.status.code(), Some(code));
    assert!(
        String::from_utf8(human.stdout).is_ok(),
        "human output is UTF-8"
    );
    report
}

#[test]
fn validator_rejects_broken_reports() {
    let lib = Library::new();
    let good = scan_json(&lib, &[], 0);
    let schema = schema();
    let check = |v: &Value| {
        let mut errs = Vec::new();
        validate(&schema, &schema, v, "", &mut errs);
        errs
    };
    let mut missing = good.clone();
    missing["summary"]
        .as_object_mut()
        .unwrap()
        .remove("issues_hidden");
    assert!(!check(&missing).is_empty());
    let mut wrong_type = good.clone();
    wrong_type["options"]["follow_symlinks"] = Value::from("no");
    assert!(!check(&wrong_type).is_empty());
    let mut bad_enum = good;
    bad_enum["options"]["min_severity"] = Value::from("fatal");
    assert!(!check(&bad_enum).is_empty());
}

#[test]
fn empty_library() {
    let lib = Library::new();
    let r = scan_json(&lib, &[], 0);
    assert_eq!(r["summary"]["files_scanned"], 0);
    assert_eq!(r["albums"], Value::Array(vec![]));
    assert_eq!(r["issues"], Value::Array(vec![]));
    assert_eq!(r["errors"], Value::Array(vec![]));
}

#[test]
fn only_non_audio_files() {
    let lib = Library::new();
    lib.add_raw("cover.jpg", b"\xff\xd8\xff");
    lib.add_raw("Artist/Album/notes.txt", b"hello");
    lib.add_raw("Artist/Album/playlist.m3u", b"#EXTM3U\n");
    lib.add_raw("Artist/no-extension", b"");
    lib.mkdir("Empty/Dir");
    let r = scan_json(&lib, &[], 0);
    assert_eq!(r["summary"]["audio_files"], 0);
    assert_eq!(r["summary"]["albums"], 0);
    assert_eq!(r["albums"], Value::Array(vec![]));
    assert_eq!(r["issues"], Value::Array(vec![]));
    assert_eq!(r["errors"], Value::Array(vec![]));
}

#[test]
fn deep_nesting() {
    let lib = Library::new();
    let dir: Vec<String> = (0..40).map(|i| format!("d{i:02}")).collect();
    let dir = dir.join("/");
    let tags = Tags::new()
        .album("Deep")
        .artist("Diver")
        .album_artist("Diver");
    lib.add_album(&dir, Fixture::Flac, &tags, 2);
    let r = scan_json(&lib, &[], 0);
    assert_eq!(r["summary"]["audio_files"], 2);
    assert_eq!(r["albums"][0]["id"], Value::from(dir.clone()));
    assert_eq!(r["issues"], Value::Array(vec![]));
}

#[cfg(target_os = "linux")]
#[test]
fn non_utf8_names_are_reported_lossily() {
    use common::non_utf8_name;
    let lib = Library::new();
    let dir = std::path::PathBuf::from(non_utf8_name("Caf", " Album"));
    let tags = Tags::album_track("Album", "Artist", 1).album_artist("Artist");
    lib.add_at(dir.join("01.flac"), Fixture::Flac, &tags);
    lib.add_at(
        dir.join(non_utf8_name("02 Tr", ".flac")),
        Fixture::Flac,
        &tags.clone().track(2),
    );
    lib.add_raw_at(dir.join(non_utf8_name("cover", ".jpg")), b"jpeg");
    lib.add_raw_at(non_utf8_name("broken", ".mp3"), b"text, not audio");

    let r = scan_json(&lib, &[], 1);
    assert_eq!(r["summary"]["audio_files"], 3);
    let album = &r["albums"][0];
    assert_eq!(album["id"], "Caf\u{FFFD} Album");
    let paths: Vec<&str> = album["discs"][0]["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        [
            "Caf\u{FFFD} Album/01.flac",
            "Caf\u{FFFD} Album/02 Tr\u{FFFD}.flac"
        ]
    );
    assert_eq!(r["errors"][0]["path"], "broken\u{FFFD}.mp3");
}

#[test]
fn corrupt_files_are_errors_not_crashes() {
    let lib = Library::new();
    for kind in Corrupt::ALL {
        lib.add_corrupt(&format!("Broken/{kind:?}.flac"), kind);
    }
    let overflow =
        std::fs::read(common::fixtures_dir().join("corrupt/opus-overflow.opus")).unwrap();
    lib.add_raw("Broken/overflow.opus", &overflow);
    lib.add_raw("Broken/renamed.mp3", b"just some text\nrenamed to .mp3\n");
    lib.add_raw("Broken/renamed.m4a", b"");
    let good = Tags::album_track("Fine", "Artist", 1).album_artist("Artist");
    lib.add("Fine/01.ogg", Fixture::OggVorbis, &good);

    let r = scan_json(&lib, &[], 1);
    let errors = r["errors"].as_array().unwrap();
    let paths: Vec<&str> = errors.iter().map(|e| e["path"].as_str().unwrap()).collect();
    assert_eq!(
        paths,
        [
            "Broken/Empty.flac",
            "Broken/Garbage.flac",
            "Broken/Truncated.flac",
            "Broken/overflow.opus",
            "Broken/renamed.m4a",
            "Broken/renamed.mp3",
        ]
    );
    for e in errors {
        let kind = e["kind"].as_str().unwrap();
        assert!(matches!(kind, "corrupt" | "unsupported"), "{e}");
    }
    assert_eq!(r["summary"]["unreadable_files"], 6);
    // The readable file is still analysed.
    assert_eq!(r["albums"].as_array().unwrap().len(), 1);
    assert_eq!(r["albums"][0]["id"], "Fine");

    // Errors only affect the exit code through --fail-on.
    scan_json(&lib, &["--fail-on", "never"], 0);
}
