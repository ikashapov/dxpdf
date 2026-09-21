//! Consistency checks for README.md's OOXML feature-coverage table.
//!
//! Nobody re-reads the whole table on every PR, so a feature added twice — once
//! under its old, superseded status and once under its new one — is invisible
//! in a diff that only shows the lines it touches. That happened here: two
//! rows were appended without removing the ones they superseded, leaving
//! "Bidirectional tab stops and numbering labels" and "`w:bidiVisual`
//! (mirrored table columns)" each listed twice with contradictory ✅/❌
//! statuses, and desyncing the heading's own count of how many rows say what.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn readme() -> String {
    std::fs::read_to_string(repo_root().join("README.md")).expect("README.md is missing")
}

/// The feature matrix's own data rows — `(feature name, status cell)` pairs
/// from every subsection between the `<details>` marker and its close. Skips
/// the `| Feature | Status |` header and `|---|---|` separator each
/// subsection repeats.
///
/// Every row in the table splits cleanly into exactly four `|`-delimited
/// fields (checked below): no cell contains a literal `|`, so this needs
/// nothing more than a plain split.
fn feature_rows(readme: &str) -> Vec<(String, String)> {
    let start = readme
        .find("<summary>Full feature matrix (click to expand)</summary>")
        .expect("README.md's feature-matrix marker is missing or renamed");
    let end = readme[start..]
        .find("</details>")
        .map(|i| start + i)
        .expect("no closing </details> after the feature-matrix marker");

    readme[start..end]
        .lines()
        .filter(|line| line.starts_with('|'))
        .filter_map(|line| {
            let cells: Vec<&str> = line.split('|').collect();
            assert_eq!(
                cells.len(),
                4,
                "feature-matrix row has an unexpected shape \
                 (a literal `|` inside a cell?): {line:?}"
            );
            let (feature, status) = (cells[1].trim(), cells[2].trim());
            if feature == "Feature" || feature.starts_with("---") {
                None
            } else {
                Some((feature.to_string(), status.to_string()))
            }
        })
        .collect()
}

#[test]
fn no_feature_is_listed_twice() {
    let readme = readme();
    let rows = feature_rows(&readme);
    assert!(rows.len() > 50, "the feature-matrix parse found suspiciously few rows ({}) — the marker or table shape probably changed", rows.len());

    let mut seen = std::collections::HashSet::new();
    let duplicates: Vec<&str> = rows
        .iter()
        .map(|(feature, _)| feature.as_str())
        .filter(|feature| !seen.insert(*feature))
        .collect();
    assert!(
        duplicates.is_empty(),
        "these features appear more than once in the table — each entry needs \
         exactly one, current, status: {duplicates:?}"
    );
}

#[test]
fn the_summary_line_matches_the_table_it_summarizes() {
    let readme = readme();
    let rows = feature_rows(&readme);

    let implemented = rows.iter().filter(|(_, s)| s.starts_with('✅')).count();
    let partial = rows.iter().filter(|(_, s)| s.starts_with('⚠')).count();
    let missing = rows.iter().filter(|(_, s)| s.starts_with('❌')).count();
    assert_eq!(
        implemented + partial + missing,
        rows.len(),
        "every status cell should start with ✅, ⚠️ or ❌ — found one that doesn't"
    );

    let marker = "## OOXML Feature Coverage";
    let summary_start = readme
        .find(marker)
        .map(|i| i + marker.len())
        .expect("README.md's feature-coverage heading is missing or renamed");
    let summary_line = readme[summary_start..]
        .lines()
        .find(|l| l.contains("entries fully implemented"))
        .expect("no summary line after the feature-coverage heading");

    let expected = format!(
        "**{implemented} entries fully implemented, {partial} partial, {missing} not yet supported.**"
    );
    assert!(
        summary_line.contains(&expected),
        "the summary line says {summary_line:?}, but the table itself has \
         {implemented} \u{2705}, {partial} \u{26a0}\u{fe0f}, {missing} \u{274c} rows — expected {expected:?}"
    );
}
