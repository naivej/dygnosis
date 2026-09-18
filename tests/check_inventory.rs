use std::collections::HashSet;
use std::path::PathBuf;

use dygnosis::explain::{explain, known_codes, ExplainKind};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Inventory {
    rows: Vec<Row>,
}

#[derive(Debug, Deserialize)]
struct Row {
    id: String,
    codes: Vec<String>,
    kind: String,
    fixture: String,
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn inventory() -> Inventory {
    let path = root().join("tests/fixtures/inventory.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("inventory missing at {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("inventory.json: {e}"))
}

fn fixture_exists(rel: &str) -> bool {
    let path = root().join(rel);
    path.is_file() || path.is_dir()
}

#[test]
fn check_inventory() {
    let inv = inventory();
    assert!(!inv.rows.is_empty(), "inventory has no rows");

    let mut ids = HashSet::new();
    for row in &inv.rows {
        assert!(
            ids.insert(row.id.clone()),
            "duplicate inventory id {}",
            row.id
        );
        assert!(
            matches!(
                row.kind.as_str(),
                "fire" | "quiet" | "archive" | "harness" | "documented-only"
            ),
            "{}: bad kind {}",
            row.id,
            row.kind
        );
    }

    let mut codes_in_rows: HashSet<String> = HashSet::new();
    for row in &inv.rows {
        codes_in_rows.extend(row.codes.iter().cloned());
    }
    let mut missing_shared_added: Vec<&str> = Vec::new();
    let mut skipped_with_rows: Vec<&str> = Vec::new();
    for code in known_codes() {
        let kind = explain(code)
            .unwrap_or_else(|| panic!("{code} in known_codes() without explain"))
            .kind;
        let has_row = codes_in_rows.contains(code);
        match kind {
            ExplainKind::Skipped => {
                if has_row {
                    skipped_with_rows.push(code);
                }
            }
            ExplainKind::Shared | ExplainKind::Added => {
                if !has_row {
                    missing_shared_added.push(code);
                }
            }
        }
    }
    assert!(
        missing_shared_added.is_empty(),
        "shared/added known_codes() with zero inventory rows: {missing_shared_added:?}"
    );
    assert!(
        skipped_with_rows.is_empty(),
        "skipped keys must have zero inventory rows: {skipped_with_rows:?}"
    );
    for code in [
        "E040", "W040", "W041", "I041", "W071", "I070", "I071", "W080", "W081", "DYNR",
    ] {
        assert!(
            !codes_in_rows.contains(code),
            "Out code {code} must have no inventory row"
        );
    }

    let template = inv
        .rows
        .iter()
        .find(|r| r.id == "e010_extra")
        .expect("wave-a template row e010_extra");
    assert_eq!(template.kind, "fire");
    assert_eq!(template.fixture, "tests/fixtures/e010/e010_extra.mod");
    assert!(
        fixture_exists(&template.fixture),
        "template missing: {}",
        template.fixture
    );

    for row in &inv.rows {
        if row.kind != "archive" {
            continue;
        }
        assert!(
            !row.fixture.is_empty(),
            "archive row {} has empty fixture",
            row.id
        );
        assert!(
            fixture_exists(&row.fixture),
            "archive fixture missing for {}: {}",
            row.id,
            row.fixture
        );
    }

    let mut missing: Vec<&str> = Vec::new();
    for row in &inv.rows {
        if row.kind != "fire" && row.kind != "quiet" {
            continue;
        }
        if row.fixture.is_empty() {
            missing.push(&row.id);
            continue;
        }
        assert!(
            fixture_exists(&row.fixture),
            "{} fixture missing for {}: {}",
            row.kind,
            row.id,
            row.fixture
        );
    }
    missing.sort();
    if !missing.is_empty() {
        eprintln!("missing fire/quiet fixtures ({}):", missing.len());
        for id in &missing {
            eprintln!("  {id}");
        }
    }
    assert!(
        missing.is_empty(),
        "{} fire/quiet rows have an empty fixture (list printed above)",
        missing.len()
    );
}

#[test]
fn inventory_file_is_lf() {
    let path = root().join("tests/fixtures/inventory.json");
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        !bytes.windows(2).any(|w| w == b"\r\n"),
        "inventory.json must be LF only"
    );
}
