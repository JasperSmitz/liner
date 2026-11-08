//! The committed JSON Schema must equal the one generated from the report types.
//! Regenerate deliberately (and bump SCHEMA_VERSION per ADR 0003) with:
//!   LINER_UPDATE_SCHEMA=1 cargo test --test schema

use std::path::Path;

const SCHEMA_FILE: &str = "schema/liner-report.v1.schema.json";

#[test]
fn committed_schema_matches_generated() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(SCHEMA_FILE);
    let generated = serde_json::to_string_pretty(&liner::report::json_schema()).unwrap() + "\n";
    if std::env::var_os("LINER_UPDATE_SCHEMA").is_some() {
        #[allow(clippy::disallowed_methods)] // dev-time regeneration of a repo file
        std::fs::write(&path, &generated).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == generated,
        "{SCHEMA_FILE} is out of date with the report types. If the change is intended, \
         bump report::SCHEMA_VERSION per docs/adr/0003 and run \
         `LINER_UPDATE_SCHEMA=1 cargo test --test schema`."
    );
}

#[test]
fn schema_major_matches_file_name() {
    let major = liner::report::SCHEMA_VERSION.split('.').next().unwrap();
    assert!(SCHEMA_FILE.contains(&format!(".v{major}.")));
}
