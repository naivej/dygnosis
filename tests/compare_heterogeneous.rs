//! Heterogeneous equation compare, one dimension at a time.

use std::fs;
use std::path::PathBuf;

use dygnosis::{compare_models, parse};
use serde_json::{json, Value};

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

fn blocks(diff: &Value) -> &[Value] {
    diff["heterogeneous_equations"].as_array().unwrap()
}

fn block<'a>(diff: &'a Value, dimension: &str) -> &'a Value {
    blocks(diff)
        .iter()
        .find(|block| block["dimension"] == dimension)
        .unwrap_or_else(|| panic!("missing dimension {dimension}: {diff}"))
}

fn assert_aggregate_equations_unchanged(diff: &Value) {
    assert!(
        diff["added_equations"].as_array().unwrap().is_empty(),
        "{diff}"
    );
    assert!(
        diff["removed_equations"].as_array().unwrap().is_empty(),
        "{diff}"
    );
    assert!(
        diff["changed_equations"].as_array().unwrap().is_empty(),
        "{diff}"
    );
    assert!(
        diff["unmatched_same_name"].as_array().unwrap().is_empty(),
        "{diff}"
    );
}

fn assert_list_row(row: &Value, dimension: &str) {
    assert_eq!(row["domain"], "heterogeneous", "{row}");
    assert_eq!(row["dimension"], dimension, "{row}");
    assert!(row["index"].is_u64(), "{row}");
    assert!(row["text"].is_string(), "{row}");
    assert!(row.get("name").is_some(), "{row}");
    assert!(row["tags"].is_object(), "{row}");
}

fn assert_change_row(row: &Value, dimension: &str) {
    assert_eq!(row["domain"], "heterogeneous", "{row}");
    assert_eq!(row["dimension"], dimension, "{row}");
    for key in ["index", "name", "tags", "text"] {
        assert!(row.get(key).is_none(), "single-side {key}: {row}");
    }
}

fn lists_empty(block: &Value) -> bool {
    block["added"].as_array().unwrap().is_empty()
        && block["removed"].as_array().unwrap().is_empty()
        && block["changed"].as_array().unwrap().is_empty()
        && block["unmatched_same_name"].as_array().unwrap().is_empty()
}

#[test]
fn reorder_inside_one_dimension_is_unchanged() {
    let diff = diff("het_reorder_before.mod", "het_reorder_after.mod");
    assert_aggregate_equations_unchanged(&diff);
    assert_eq!(blocks(&diff).len(), 1, "{diff}");
    let block = block(&diff, "h");
    assert!(lists_empty(block), "{diff}");
    let md = diff["markdown"].as_str().unwrap();
    assert!(!md.contains("Heterogeneous equations"), "{md}");
}

#[test]
fn unique_name_pairs_when_text_differs() {
    let diff = diff("het_rewrite_before.mod", "het_rewrite_after.mod");
    assert_aggregate_equations_unchanged(&diff);
    let block = block(&diff, "h");
    assert!(block["added"].as_array().unwrap().is_empty(), "{diff}");
    assert!(block["removed"].as_array().unwrap().is_empty(), "{diff}");
    assert!(block["unmatched_same_name"].as_array().unwrap().is_empty());
    let changed = block["changed"].as_array().unwrap();
    assert_eq!(changed.len(), 1, "{diff}");
    assert_change_row(&changed[0], "h");
    assert_eq!(changed[0]["name_old"], "euler");
    assert_eq!(changed[0]["name_new"], "euler");
    assert_eq!(changed[0]["index_old"], 0);
    assert_eq!(changed[0]["index_new"], 0);
    assert_ne!(changed[0]["text_old"], changed[0]["text_new"]);
}

#[test]
fn tag_only_edit_on_a_unique_name_is_a_change() {
    let diff = diff("het_tag_before.mod", "het_tag_after.mod");
    assert_aggregate_equations_unchanged(&diff);
    let block = block(&diff, "h");
    assert!(block["added"].as_array().unwrap().is_empty(), "{diff}");
    assert!(block["removed"].as_array().unwrap().is_empty(), "{diff}");
    let changed = block["changed"].as_array().unwrap();
    assert_eq!(changed.len(), 1, "{diff}");
    assert_change_row(&changed[0], "h");
    assert_eq!(changed[0]["text_old"], changed[0]["text_new"]);
    assert_eq!(changed[0]["name_old"], "euler");
    assert_eq!(changed[0]["name_new"], "euler");
    assert!(changed[0]["tags_old"].get("bind").is_none());
    assert_eq!(changed[0]["tags_new"]["bind"], "ELB");
    let md = diff["markdown"].as_str().unwrap();
    assert!(md.contains("## Heterogeneous equations (`h`)"), "{md}");
    assert!(
        md.lines().any(|line| line == "### Changed equations"),
        "{md}"
    );
    assert!(
        !md.lines().any(|line| line == "## Changed equations"),
        "{md}"
    );
    assert!(md.contains("`euler`"), "{md}");
    assert!(md.contains("[bind=ELB]"), "{md}");
    assert!(md.contains("(dimension `h`)"), "{md}");
    let text = changed[0]["text_old"].as_str().unwrap();
    assert_eq!(md.matches(&format!("`{text}`")).count(), 2, "{md}");
    let old_at = md.find(&format!("`{text}`")).unwrap();
    let new_at = md.rfind(&format!("`{text}`")).unwrap();
    assert!(old_at != new_at, "{md}");
    assert!(md[old_at..new_at].contains('\n'), "{md}");
}

#[test]
fn repeated_name_pairs_only_on_exact_text_and_tags() {
    let diff = diff("het_repeat_before.mod", "het_repeat_after.mod");
    assert_aggregate_equations_unchanged(&diff);
    let block = block(&diff, "h");
    assert!(block["changed"].as_array().unwrap().is_empty(), "{diff}");
    let removed = block["removed"].as_array().unwrap();
    let added = block["added"].as_array().unwrap();
    assert_eq!(removed.len(), 1, "{diff}");
    assert_eq!(added.len(), 1, "{diff}");
    assert_list_row(&removed[0], "h");
    assert_list_row(&added[0], "h");
    assert_eq!(removed[0]["name"], "policy");
    assert_eq!(added[0]["name"], "policy");
    assert_eq!(removed[0]["tags"]["bind"], "ELB");
    assert_eq!(added[0]["tags"]["bind"], "ELB");
    assert_ne!(removed[0]["text"], added[0]["text"]);
    let groups = block["unmatched_same_name"].as_array().unwrap();
    assert_eq!(groups.len(), 1, "{diff}");
    assert_eq!(groups[0]["name"], "policy");
    assert_eq!(groups[0]["dimension"], "h");
    assert_eq!(groups[0]["removed"], json!([removed[0]]));
    assert_eq!(groups[0]["added"], json!([added[0]]));
    let md = diff["markdown"].as_str().unwrap();
    assert!(md.contains("## Heterogeneous equations (`h`)"), "{md}");
    assert!(md.contains("### Unmatched same name"), "{md}");
    assert!(md.contains("`policy`"), "{md}");
    assert!(md.contains("[bind=ELB]"), "{md}");
}

#[test]
fn different_nonempty_names_do_not_pair() {
    let diff = diff("het_names_before.mod", "het_names_after.mod");
    assert_aggregate_equations_unchanged(&diff);
    let block = block(&diff, "h");
    assert!(block["changed"].as_array().unwrap().is_empty(), "{diff}");
    assert!(block["unmatched_same_name"].as_array().unwrap().is_empty());
    let removed = &block["removed"][0];
    let added = &block["added"][0];
    assert_list_row(removed, "h");
    assert_list_row(added, "h");
    assert_eq!(removed["name"], "alpha");
    assert_eq!(added["name"], "beta");
    assert_eq!(removed["text"], added["text"]);
}

#[test]
fn unnamed_row_may_text_match_a_named_row() {
    let diff = diff("het_unnamed_before.mod", "het_unnamed_after.mod");
    assert_aggregate_equations_unchanged(&diff);
    let block = block(&diff, "h");
    assert!(block["added"].as_array().unwrap().is_empty(), "{diff}");
    assert!(block["removed"].as_array().unwrap().is_empty(), "{diff}");
    let changed = block["changed"].as_array().unwrap();
    assert_eq!(changed.len(), 1, "{diff}");
    assert_change_row(&changed[0], "h");
    assert!(changed[0]["name_old"].is_null());
    assert_eq!(changed[0]["name_new"], "euler");
    assert_eq!(changed[0]["text_old"], changed[0]["text_new"]);
    assert!(changed[0]["tags_old"].as_object().unwrap().is_empty());
    assert_eq!(changed[0]["tags_new"]["name"], "euler");
}

#[test]
fn new_and_deleted_dimensions_do_not_cross_pair() {
    let diff = diff("het_dims_before.mod", "het_dims_after.mod");
    assert_aggregate_equations_unchanged(&diff);
    assert_eq!(blocks(&diff).len(), 2, "{diff}");
    assert_eq!(blocks(&diff)[0]["dimension"], "a");
    assert_eq!(blocks(&diff)[1]["dimension"], "b");

    let gone = block(&diff, "a");
    let fresh = block(&diff, "b");
    assert!(gone["added"].as_array().unwrap().is_empty(), "{diff}");
    assert!(gone["changed"].as_array().unwrap().is_empty(), "{diff}");
    assert!(gone["unmatched_same_name"].as_array().unwrap().is_empty());
    assert!(fresh["removed"].as_array().unwrap().is_empty(), "{diff}");
    assert!(fresh["changed"].as_array().unwrap().is_empty(), "{diff}");
    assert!(fresh["unmatched_same_name"].as_array().unwrap().is_empty());

    let removed = gone["removed"].as_array().unwrap();
    let added = fresh["added"].as_array().unwrap();
    assert_eq!(removed.len(), 2, "{diff}");
    assert_eq!(added.len(), 2, "{diff}");
    for (index, row) in removed.iter().enumerate() {
        assert_list_row(row, "a");
        assert_eq!(row["index"], index as u64);
        assert_eq!(row["name"], "law");
        assert_eq!(row["text"], added[index]["text"]);
        assert_list_row(&added[index], "b");
        assert_eq!(added[index]["index"], index as u64);
    }
}

#[test]
fn repeated_blocks_keep_continuing_indexes() {
    let diff = diff("het_blocks_before.mod", "het_blocks_after.mod");
    assert_aggregate_equations_unchanged(&diff);
    let block = block(&diff, "h");
    assert!(block["added"].as_array().unwrap().is_empty(), "{diff}");
    assert!(block["removed"].as_array().unwrap().is_empty(), "{diff}");
    let changed = block["changed"].as_array().unwrap();
    assert_eq!(changed.len(), 1, "{diff}");
    assert_change_row(&changed[0], "h");
    assert_eq!(changed[0]["name_old"], "second");
    assert_eq!(changed[0]["name_new"], "second");
    assert_eq!(changed[0]["index_old"], 1);
    assert_eq!(changed[0]["index_new"], 1);
    assert_ne!(changed[0]["text_old"], changed[0]["text_new"]);
}

#[test]
fn aggregate_and_heterogeneous_rows_do_not_pair() {
    let diff = diff("het_scope_before.mod", "het_scope_after.mod");
    assert!(
        diff["added_equations"].as_array().unwrap().is_empty(),
        "{diff}"
    );
    assert!(
        diff["changed_equations"].as_array().unwrap().is_empty(),
        "{diff}"
    );
    assert!(diff["unmatched_same_name"].as_array().unwrap().is_empty());
    let removed = diff["removed_equations"].as_array().unwrap();
    assert_eq!(removed.len(), 1, "{diff}");
    assert_eq!(removed[0]["domain"], "aggregate");
    assert!(removed[0]["dimension"].is_null());
    assert_eq!(removed[0]["name"], "law");
    assert_eq!(removed[0]["index"], 1);

    let block = block(&diff, "h");
    assert!(block["removed"].as_array().unwrap().is_empty(), "{diff}");
    assert!(block["changed"].as_array().unwrap().is_empty(), "{diff}");
    assert!(block["unmatched_same_name"].as_array().unwrap().is_empty());
    let added = block["added"].as_array().unwrap();
    assert_eq!(added.len(), 1, "{diff}");
    assert_list_row(&added[0], "h");
    assert_eq!(added[0]["name"], "law");
    assert_eq!(added[0]["index"], 0);
    assert_eq!(added[0]["text"], removed[0]["text"]);
}

#[test]
fn markdown_section_only_for_a_nonempty_dimension() {
    let diff = diff("het_md_before.mod", "het_md_after.mod");
    assert_aggregate_equations_unchanged(&diff);
    assert_eq!(blocks(&diff).len(), 2, "{diff}");
    assert_eq!(blocks(&diff)[0]["dimension"], "edit");
    assert_eq!(blocks(&diff)[1]["dimension"], "quiet");
    assert!(lists_empty(block(&diff, "quiet")), "{diff}");
    let changed = block(&diff, "edit")["changed"].as_array().unwrap();
    assert_eq!(changed.len(), 1, "{diff}");
    assert_change_row(&changed[0], "edit");
    assert_eq!(changed[0]["name_old"], "move");
    assert_eq!(changed[0]["tags_new"]["bind"], "ELB");

    let md = diff["markdown"].as_str().unwrap();
    assert_eq!(md.matches("## Heterogeneous equations").count(), 1, "{md}");
    assert!(md.contains("## Heterogeneous equations (`edit`)"), "{md}");
    assert!(!md.contains("quiet"), "{md}");
    assert!(!md.contains("`stay`"), "{md}");
    assert!(md.contains("`move`"), "{md}");
    assert!(md.contains("[bind=ELB]"), "{md}");
    assert!(md.contains("(dimension `edit`)"), "{md}");
    assert!(!md.contains("(dimension `quiet`)"), "{md}");
    let old = changed[0]["text_old"].as_str().unwrap();
    let new = changed[0]["text_new"].as_str().unwrap();
    let old_at = md.find(&format!("`{old}`")).unwrap();
    let new_at = md.find(&format!("`{new}`")).unwrap();
    assert!(old_at != new_at, "{md}");
    let (start, end) = if old_at < new_at {
        (old_at, new_at)
    } else {
        (new_at, old_at)
    };
    assert!(md[start..end].contains('\n'), "{md}");
}
