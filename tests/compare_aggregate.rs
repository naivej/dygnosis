//! Aggregate equation pairing for compare. Heterogeneous rows stay out of the aggregate lists.

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

fn eq_lists_empty(diff: &Value) -> bool {
    diff["added_equations"].as_array().unwrap().is_empty()
        && diff["removed_equations"].as_array().unwrap().is_empty()
        && diff["changed_equations"].as_array().unwrap().is_empty()
        && diff["unmatched_same_name"].as_array().unwrap().is_empty()
}

fn assert_change_shape(row: &Value) {
    for key in [
        "index_old",
        "index_new",
        "text_old",
        "text_new",
        "domain",
        "dimension",
        "name_old",
        "name_new",
        "tags_old",
        "tags_new",
    ] {
        assert!(row.get(key).is_some(), "changed row missing {key}: {row}");
    }
    for key in ["index", "name", "tags"] {
        assert!(
            row.get(key).is_none(),
            "changed row must not use a single-side {key}: {row}"
        );
    }
    assert_eq!(row["domain"], "aggregate");
    assert!(row["dimension"].is_null());
}

fn assert_list_shape(row: &Value) {
    for key in ["index", "text", "domain", "dimension", "name", "tags"] {
        assert!(row.get(key).is_some(), "equation row missing {key}: {row}");
    }
    assert_eq!(row["domain"], "aggregate");
    assert!(row["dimension"].is_null());
    assert!(row["tags"].is_object());
}

#[test]
fn changing_one_regime_equation_with_a_repeated_name_is_a_paired_edit() {
    let before = "var r; parameters rho; rho=.9; model; [name='policy',bind='ELB'] r=1; [name='policy',relax='ELB'] r=rho*r(-1); end;";
    let after = before.replace("r=1", "r=0");
    let diff = compare_models(&parse(before), &parse(&after)).to_json();
    let changed = diff["changed_equations"].as_array().unwrap();
    assert_eq!(changed.len(), 1, "one constant edit must stay paired");
    assert_eq!(changed[0]["index_old"], 0);
    assert_eq!(changed[0]["index_new"], 0);
    assert_eq!(changed[0]["tags_old"]["bind"], "ELB");
    for key in [
        "added_equations",
        "removed_equations",
        "unmatched_same_name",
    ] {
        assert!(diff[key].as_array().unwrap().is_empty(), "{key}: {diff}");
    }
    let row = diff["semantic"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["family"] == "equations" && row["change"] == "changed")
        .unwrap();
    for (side, role, literal) in [("before", "removed", "1"), ("after", "added", "0")] {
        let highlighted: String = row["expressions"][0][side]["runs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|run| run["role"] == role)
            .map(|run| run["text"].as_str().unwrap())
            .collect();
        assert_eq!(highlighted, literal);
    }
}

#[test]
fn regime_pairs_survive_reordering_far_rewrites_and_other_tag_edits() {
    let before = "var r; model; [name='policy',bind='ELB',label='old'] r=1; [name='policy',relax='ELB'] r=2; end;";
    let after = "var r; model; [name='policy',relax='ELB'] r=r(-1)+3; [name='policy',label='new',bind='ELB'] r=r(-2)*r(-3); end;";
    let diff = compare_models(&parse(before), &parse(after)).to_json();
    let changed = diff["changed_equations"].as_array().unwrap();
    assert_eq!(changed.len(), 2);
    for row in changed {
        for role in ["bind", "relax"] {
            assert_eq!(row["tags_old"][role], row["tags_new"][role]);
        }
        assert_ne!(row["index_old"], row["index_new"]);
    }
    assert!(diff["unmatched_same_name"].as_array().unwrap().is_empty());
}

#[test]
fn an_unchanged_copy_cannot_make_a_repeated_regime_pair_unique() {
    let before = "var r; model; [name='policy',bind='ELB'] r=0; [name='policy',bind='ELB'] r=0; [name='policy',relax='ELB'] r=r(-1); end;";
    let after = "var r; model; [name='policy',bind='ELB'] r=0; [name='policy',bind='ELB'] r=1; [name='policy',relax='ELB'] r=r(-2); end;";
    let diff = compare_models(&parse(before), &parse(after)).to_json();
    let changed = diff["changed_equations"].as_array().unwrap();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0]["tags_old"]["relax"], "ELB");
    assert_eq!(
        diff["unmatched_same_name"][0]["removed"][0]["tags"]["bind"],
        "ELB"
    );
    assert_eq!(
        diff["unmatched_same_name"][0]["added"][0]["tags"]["bind"],
        "ELB"
    );
    assert!(diff["semantic"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["family"] == "equations"
            && row["limits"]
                .as_array()
                .unwrap()
                .iter()
                .any(|limit| limit["code"] == "equation_correspondence_unpaired"))
        .all(|row| row["timing"].as_array().unwrap().is_empty()));
}

#[test]
fn reorder_of_unique_names_is_unchanged() {
    let diff = diff("reorder_before.mod", "reorder_after.mod");
    assert!(eq_lists_empty(&diff), "{diff}");
    assert!(diff["heterogeneous_equations"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn unique_name_rewrite_is_a_change() {
    let diff = diff("rewrite_before.mod", "rewrite_after.mod");
    assert!(diff["added_equations"].as_array().unwrap().is_empty());
    assert!(diff["removed_equations"].as_array().unwrap().is_empty());
    assert!(diff["unmatched_same_name"].as_array().unwrap().is_empty());
    let changed = diff["changed_equations"].as_array().unwrap();
    assert_eq!(changed.len(), 1, "{diff}");
    assert_change_shape(&changed[0]);
    assert_eq!(changed[0]["name_old"], "euler");
    assert_eq!(changed[0]["name_new"], "euler");
    assert_eq!(changed[0]["index_old"], 0);
    assert_eq!(changed[0]["index_new"], 0);
    assert_ne!(changed[0]["text_old"], changed[0]["text_new"]);
}

#[test]
fn unique_name_far_rewrite_still_pairs() {
    let diff = diff("far_before.mod", "far_after.mod");
    assert!(
        diff["added_equations"].as_array().unwrap().is_empty(),
        "{diff}"
    );
    assert!(
        diff["removed_equations"].as_array().unwrap().is_empty(),
        "{diff}"
    );
    let changed = diff["changed_equations"].as_array().unwrap();
    assert_eq!(changed.len(), 1, "{diff}");
    assert_eq!(changed[0]["name_old"], "euler");
    assert_eq!(changed[0]["name_new"], "euler");
}

#[test]
fn unique_name_tag_only_edit_is_a_change() {
    let diff = diff("tag_only_before.mod", "tag_only_after.mod");
    assert!(
        diff["added_equations"].as_array().unwrap().is_empty(),
        "{diff}"
    );
    assert!(
        diff["removed_equations"].as_array().unwrap().is_empty(),
        "{diff}"
    );
    let changed = diff["changed_equations"].as_array().unwrap();
    assert_eq!(changed.len(), 1, "{diff}");
    assert_eq!(changed[0]["text_old"], changed[0]["text_new"]);
    assert_eq!(changed[0]["tags_old"]["name"], "euler");
    assert!(changed[0]["tags_old"].get("bind").is_none());
    assert_eq!(changed[0]["tags_new"]["bind"], "ELB");
    let md = diff["markdown"].as_str().unwrap();
    assert!(md.contains("[0 -> 0]"), "{md}");
    assert!(md.contains("`euler`"), "{md}");
    assert!(md.contains("[bind=ELB]"), "{md}");
    let text = changed[0]["text_old"].as_str().unwrap();
    assert_eq!(md.matches(&format!("`{text}`")).count(), 2, "{md}");
    let old_at = md.find(&format!("`{text}`")).unwrap();
    let new_at = md.rfind(&format!("`{text}`")).unwrap();
    assert!(old_at != new_at, "old and new text share one line: {md}");
    assert!(md[old_at..new_at].contains('\n'), "{md}");
}

#[test]
fn tag_order_alone_is_unchanged() {
    let diff = diff("tag_order_before.mod", "tag_order_after.mod");
    assert!(eq_lists_empty(&diff), "{diff}");
}

#[test]
fn repeated_regimes_reorder_when_text_and_tags_match() {
    let diff = diff("regimes_before.mod", "regimes_swap_after.mod");
    assert!(eq_lists_empty(&diff), "{diff}");
}

#[test]
fn repeated_regime_text_edit_keeps_the_regime_and_markdown_pair() {
    let diff = diff("regimes_before.mod", "regimes_edit_after.mod");
    let changed = diff["changed_equations"].as_array().unwrap();
    assert_eq!(changed.len(), 1);
    assert_change_shape(&changed[0]);
    let removed = diff["removed_equations"].as_array().unwrap();
    let added = diff["added_equations"].as_array().unwrap();
    assert!(removed.is_empty());
    assert!(added.is_empty());
    assert_eq!(changed[0]["name_old"], "policy");
    assert_eq!(changed[0]["tags_old"]["bind"], "ELB");
    assert_eq!(changed[0]["tags_new"]["bind"], "ELB");
    assert!(changed[0]["tags_old"].get("relax").is_none());
    let groups = diff["unmatched_same_name"].as_array().unwrap();
    assert!(groups.is_empty());
    let md = diff["markdown"].as_str().unwrap();
    assert!(md.contains("## Changed equations"), "{md}");
    assert!(md.contains("`policy`"), "{md}");
    assert!(md.contains("[bind=ELB]"), "{md}");
}

#[test]
fn duplicate_exact_match_follows_source_order() {
    let diff = diff("order_before.mod", "order_after.mod");
    assert!(
        diff["changed_equations"].as_array().unwrap().is_empty(),
        "{diff}"
    );
    assert_eq!(diff["removed_equations"][0]["index"], 1);
    assert_eq!(diff["added_equations"][0]["index"], 1);
    assert_eq!(diff["unmatched_same_name"][0]["name"], "policy");
}

#[test]
fn different_nonempty_names_do_not_pair() {
    for (before, after) in [
        ("different_names_before.mod", "different_names_after.mod"),
        ("near_names_before.mod", "near_names_after.mod"),
    ] {
        let diff = diff(before, after);
        assert!(
            diff["changed_equations"].as_array().unwrap().is_empty(),
            "{before}: {diff}"
        );
        assert_eq!(
            diff["removed_equations"].as_array().unwrap().len(),
            1,
            "{diff}"
        );
        assert_eq!(
            diff["added_equations"].as_array().unwrap().len(),
            1,
            "{diff}"
        );
        assert!(
            diff["unmatched_same_name"].as_array().unwrap().is_empty(),
            "{diff}"
        );
        assert_ne!(
            diff["removed_equations"][0]["name"],
            diff["added_equations"][0]["name"]
        );
    }
}

#[test]
fn unnamed_text_fallback_reports_tag_and_text_changes() {
    let named = diff("unnamed_before.mod", "unnamed_named_after.mod");
    assert!(
        named["added_equations"].as_array().unwrap().is_empty(),
        "{named}"
    );
    assert!(
        named["removed_equations"].as_array().unwrap().is_empty(),
        "{named}"
    );
    let changed = named["changed_equations"].as_array().unwrap();
    assert_eq!(changed.len(), 1, "{named}");
    assert!(changed[0]["name_old"].is_null());
    assert_eq!(changed[0]["name_new"], "euler");
    assert_eq!(changed[0]["text_old"], changed[0]["text_new"]);
    assert!(changed[0]["tags_old"].as_object().unwrap().is_empty());
    assert_eq!(changed[0]["tags_new"]["name"], "euler");
    let md = named["markdown"].as_str().unwrap();
    assert!(md.contains("`unnamed`"), "{md}");
    assert!(md.contains("`euler`"), "{md}");

    let near = diff("unnamed_before.mod", "unnamed_near_after.mod");
    let changed = near["changed_equations"].as_array().unwrap();
    assert_eq!(changed.len(), 1, "{near}");
    assert!(changed[0]["name_old"].is_null());
    assert_eq!(changed[0]["name_new"], "euler");
    assert_ne!(changed[0]["text_old"], changed[0]["text_new"]);
    assert!(
        near["added_equations"].as_array().unwrap().is_empty(),
        "{near}"
    );
}

#[test]
fn enriched_rows_keep_flag_and_regime_tags() {
    let diff = diff("labels_before.mod", "labels_after.mod");
    let removed = &diff["removed_equations"][0];
    assert_list_shape(removed);
    assert_eq!(removed["name"], "policy");
    assert_eq!(removed["tags"]["dynamic"], "");
    assert_eq!(removed["tags"]["bind"], "ELB");
    assert_eq!(removed["tags"]["name"], "policy");
    assert!(diff["added_equations"][0]["name"].is_null());
    assert!(diff["unmatched_same_name"].as_array().unwrap().is_empty());
    let md = diff["markdown"].as_str().unwrap();
    assert!(md.contains("`policy`"), "{md}");
    assert!(md.contains("[dynamic]"), "{md}");
    assert!(md.contains("[bind=ELB]"), "{md}");
    assert!(!md.contains("## Unmatched same name"), "{md}");
}

#[test]
fn repeated_name_on_one_side_skips_text_matching() {
    let diff = diff("bypass_before.mod", "bypass_after.mod");
    assert!(
        diff["changed_equations"].as_array().unwrap().is_empty(),
        "{diff}"
    );
    assert_eq!(
        diff["removed_equations"].as_array().unwrap().len(),
        2,
        "{diff}"
    );
    assert_eq!(
        diff["added_equations"].as_array().unwrap().len(),
        1,
        "{diff}"
    );
    assert!(
        diff["unmatched_same_name"].as_array().unwrap().is_empty(),
        "{diff}"
    );
}

#[test]
fn unmatched_groups_sort_by_name_and_source_order() {
    let diff = diff("groups_before.mod", "groups_after.mod");
    assert!(
        diff["changed_equations"].as_array().unwrap().is_empty(),
        "{diff}"
    );
    let groups = diff["unmatched_same_name"].as_array().unwrap();
    assert_eq!(groups.len(), 2, "{diff}");
    assert_eq!(groups[0]["name"], "alpha");
    assert_eq!(groups[1]["name"], "zeta");
    let alpha_removed: Vec<u64> = groups[0]["removed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["index"].as_u64().unwrap())
        .collect();
    assert_eq!(alpha_removed, vec![2, 3]);
    let zeta_added: Vec<u64> = groups[1]["added"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["index"].as_u64().unwrap())
        .collect();
    assert_eq!(zeta_added, vec![2, 3]);
    assert_eq!(diff["removed_equations"].as_array().unwrap().len(), 4);
    assert_eq!(diff["added_equations"].as_array().unwrap().len(), 4);
}

#[test]
fn parameter_and_shock_diffs_stay_when_equations_only_reorder() {
    let diff = diff("preserve_before.mod", "preserve_after.mod");
    assert!(eq_lists_empty(&diff), "{diff}");
    assert!(diff["changed_parameter_values"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["name"] == "beta"));
    assert_eq!(diff["added_parameters"].as_array().unwrap().len(), 0);
    let shocks = diff["shock_setup_changes"].as_array().unwrap();
    assert_eq!(shocks.len(), 1, "{diff}");
    assert_eq!(shocks[0]["before"]["periods"][0], "2");
    assert_eq!(shocks[0]["after"]["periods"][0], "3");
    assert!(diff["heterogeneous_equations"]
        .as_array()
        .unwrap()
        .is_empty());
    let md = diff["markdown"].as_str().unwrap();
    assert!(md.contains("beta"), "{md}");
    assert!(md.contains("Shock setup changes"), "{md}");
    assert!(!md.contains("## Changed equations"), "{md}");
}

#[test]
fn heterogeneous_edits_stay_out_of_aggregate_lists() {
    let diff = diff("het_before.mod", "het_after.mod");
    assert!(eq_lists_empty(&diff), "{diff}");
    let blocks = diff["heterogeneous_equations"].as_array().unwrap();
    assert_eq!(blocks.len(), 1, "{diff}");
    assert_eq!(blocks[0]["dimension"], "d");
    assert!(
        !blocks[0]["added"].as_array().unwrap().is_empty()
            || !blocks[0]["removed"].as_array().unwrap().is_empty()
            || !blocks[0]["changed"].as_array().unwrap().is_empty(),
        "{diff}"
    );
    assert!(diff.get("added_endogenous").is_some());
    assert!(diff.get("shock_setup_changes").is_some());
}
