//! Symbol kind and explicit metadata in compare. Equation pairing stays as it is.

use std::fs;
use std::path::PathBuf;

use dygnosis::{compare_models, parse};
use serde_json::Value;

fn fixture(name: &str) -> String {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/fixtures/compare");
    path.push(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
        .replace("\r\n", "\n")
}

fn diff(before: &str, after: &str) -> Value {
    compare_models(&parse(&fixture(before)), &parse(&fixture(after))).to_json()
}

fn side<'a>(row: &'a Value, key: &str) -> &'a Value {
    &row[key]
}

#[test]
fn metadata_added_changed_and_removed() {
    let diff = diff("meta_before.mod", "meta_after.mod");
    let rows = diff["symbols_changed"].as_array().unwrap();
    let names: Vec<&str> = rows
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["c", "h", "k", "n", "z"], "{diff}");

    let c = &rows[0];
    assert_eq!(side(c, "before")["kind"], "var");
    assert_eq!(side(c, "after")["kind"], "var");
    assert_eq!(side(c, "before")["long_name"], "consumption");
    assert_eq!(side(c, "after")["long_name"], "consumption");
    assert_eq!(side(c, "before")["tex_name"], "c");
    assert_eq!(side(c, "after")["tex_name"], "C");

    let h = &rows[1];
    assert_eq!(side(h, "before")["kind"], "var(heterogeneity=d)");
    assert_eq!(side(h, "after")["kind"], "var(heterogeneity=d)");
    assert_eq!(side(h, "before")["long_name"], "hours");
    assert_eq!(side(h, "after")["long_name"], "labor");
    assert!(side(h, "before")["tex_name"].is_null());
    assert!(side(h, "after")["tex_name"].is_null());

    let k = &rows[2];
    assert_eq!(side(k, "before")["long_name"], "capital");
    assert!(side(k, "after")["long_name"].is_null());
    assert!(side(k, "before")["tex_name"].is_null());

    let n = &rows[3];
    assert!(side(n, "before")["long_name"].is_null());
    assert_eq!(side(n, "after")["long_name"], "hours");

    let z = &rows[4];
    assert!(side(z, "before")["long_name"].is_null());
    assert_eq!(side(z, "after")["long_name"], "z");
    assert!(side(z, "before")["tex_name"].is_null());

    assert!(diff["changed_parameter_values"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["name"] == "beta"));
    let shocks = diff["shock_setup_changes"].as_array().unwrap();
    assert_eq!(shocks.len(), 1, "{diff}");
    assert_eq!(shocks[0]["before"]["values"][0], "0.1");
    assert_eq!(shocks[0]["after"]["values"][0], "0.2");

    let md = diff["markdown"].as_str().unwrap();
    assert!(md.contains("## Changed symbols"), "{md}");
    assert!(md.contains("long_name none"), "{md}");
    assert!(!md.contains("c_hat"), "{md}");
    assert!(!md.contains("c\\_hat"), "{md}");
}

#[test]
fn unchanged_metadata_omits_the_markdown_section() {
    let same = compare_models(
        &parse(&fixture("meta_before.mod")),
        &parse(&fixture("meta_before.mod")),
    )
    .to_json();
    assert!(same["symbols_changed"].as_array().unwrap().is_empty());
    let md = same["markdown"].as_str().unwrap();
    assert!(!md.contains("## Changed symbols"), "{md}");
    assert!(
        md.contains("_No structural or calibration changes detected._"),
        "{md}"
    );
}

#[test]
fn kind_change_keeps_legacy_add_remove_compatibility() {
    let diff = diff("kind_before.mod", "kind_after.mod");
    let rows = diff["symbols_changed"].as_array().unwrap();
    let names: Vec<&str> = rows
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["e", "u", "y"], "{diff}");

    assert_eq!(side(&rows[0], "before")["kind"], "var");
    assert_eq!(side(&rows[0], "after")["kind"], "varexo");
    assert!(side(&rows[0], "before")["long_name"].is_null());
    assert!(side(&rows[0], "after")["tex_name"].is_null());
    assert_eq!(diff["removed_endogenous"], serde_json::json!(["e"]));
    assert_eq!(diff["added_exogenous"], serde_json::json!(["e"]));

    assert_eq!(side(&rows[1], "before")["kind"], "varexo");
    assert_eq!(side(&rows[1], "after")["kind"], "varexo_det");
    assert!(diff["added_exogenous"]
        .as_array()
        .unwrap()
        .iter()
        .all(|name| name != "u"));
    assert!(diff["removed_exogenous"]
        .as_array()
        .unwrap()
        .iter()
        .all(|name| name != "u"));
    assert!(diff["common_exogenous"]
        .as_array()
        .unwrap()
        .iter()
        .any(|name| name == "u"));

    assert_eq!(side(&rows[2], "before")["kind"], "var");
    assert_eq!(side(&rows[2], "after")["kind"], "var(heterogeneity=d)");
    assert!(diff["common_endogenous"]
        .as_array()
        .unwrap()
        .iter()
        .any(|name| name == "y"));
    assert!(diff["added_endogenous"].as_array().unwrap().is_empty());
    assert_eq!(diff["removed_endogenous"], serde_json::json!(["e"]));

    assert!(diff["changed_parameter_values"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["name"] == "beta"));
    assert!(diff["symbols_changed"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["name"] != "beta"));
    assert!(diff["changed_equations"].as_array().unwrap().is_empty());
    assert!(diff["added_equations"].as_array().unwrap().is_empty());
    assert!(diff["removed_equations"].as_array().unwrap().is_empty());
}
