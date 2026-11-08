//! docs/RULES.md and rules::CATALOG must list the same codes.

use std::collections::BTreeSet;

#[test]
fn rules_doc_matches_catalog() {
    let doc = include_str!("../docs/RULES.md");
    // Table rows look like: | `code` | sev | ...
    let documented: BTreeSet<&str> = doc
        .lines()
        .filter_map(|l| l.strip_prefix("| `"))
        .filter_map(|l| l.split('`').next())
        .collect();
    let catalog: BTreeSet<&str> = liner::rules::CATALOG.iter().map(|r| r.code).collect();
    assert_eq!(documented, catalog);

    for r in liner::rules::CATALOG {
        let row = doc
            .lines()
            .find(|l| l.starts_with(&format!("| `{}` |", r.code)))
            .unwrap();
        assert!(
            row.contains(&format!("| {} |", r.severity.as_str())),
            "severity of {} differs between docs and catalog",
            r.code
        );
    }
}
