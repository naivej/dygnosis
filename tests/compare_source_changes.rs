//! Captured-file comparison and bounded-output acceptance through the shared API.

use std::collections::BTreeMap;

use dygnosis::model_diff::{compare_models, compare_models_with_budgets, ModelDiff};
use dygnosis::parser::parse;
use dygnosis::semantic_diff::{
    enforce_output_budget, populate_captured_sources, Availability, CaptureBoundary,
    CapturedSourceInput, ChangeKind, ComparisonBudgets, SourceCorrespondence, SourceFilePair,
    SourceIdentityProof, TokenRole,
};

fn files(items: &[(&str, &str)]) -> BTreeMap<String, String> {
    items
        .iter()
        .map(|(key, text)| ((*key).into(), (*text).into()))
        .collect()
}

fn captured<'a>(
    id: &'a str,
    root: &'a str,
    sources: &'a BTreeMap<String, String>,
) -> CapturedSourceInput<'a> {
    CapturedSourceInput {
        input_id: Some(id),
        root_key: root,
        sources,
        boundary: CaptureBoundary::RootAndExecutedIncludes,
    }
}

fn same_key(key: &str) -> SourceFilePair<'_> {
    SourceFilePair {
        before_key: key,
        after_key: key,
        proof: SourceIdentityProof::SameRepositoryKey,
    }
}

fn comparison() -> ModelDiff {
    compare_models(&parse("parameters p; p=1;"), &parse("parameters p; p=1;"))
}

#[test]
fn include_only_edit_retains_semantic_overlap_and_exact_file_identity() {
    let root = "@#include \"parts/calibration.mod\"\n";
    let before = files(&[
        ("model.mod", root),
        ("parts/calibration.mod", "parameters p; p=1;\n"),
    ]);
    let after = files(&[
        ("model.mod", root),
        ("parts/calibration.mod", "parameters p; p=2;\n"),
    ]);
    let mut diff = compare_models(&parse("parameters p; p=1;"), &parse("parameters p; p=2;"));
    populate_captured_sources(
        &mut diff,
        captured("before", "model.mod", &before),
        captured("after", "model.mod", &after),
        &[same_key("parts/calibration.mod")],
    )
    .unwrap();
    assert_eq!(diff.changed_parameter_values.len(), 1);
    assert_eq!(diff.source_changes.files.len(), 1);
    let file = &diff.source_changes.files[0];
    assert_eq!(
        file.correspondence,
        SourceCorrespondence::ProvenFileIdentity
    );
    assert_eq!(
        file.before.as_ref().unwrap().file_key,
        "parts/calibration.mod"
    );
    assert_eq!(
        file.after.as_ref().unwrap().input_id.as_deref(),
        Some("after")
    );
    assert_eq!(file.hunks[0].before_start, 1);
    assert_eq!(file.hunks[0].after_start, 1);
    assert_eq!(file.hunks[0].lines[0].role, TokenRole::Removed);
    assert_eq!(file.hunks[0].lines[1].role, TokenRole::Added);
    assert!(file.before.as_ref().unwrap().exact_text_available);
    assert_eq!(diff.source_changes.availability, Availability::Complete);
}

#[test]
fn roots_pair_explicitly_but_unrelated_includes_never_pair_by_name_or_similarity() {
    let before = files(&[
        ("old/root.mod", "old root\n"),
        ("old/shared.mod", "same\n"),
        ("same.mod", "same\n"),
    ]);
    let after = files(&[
        ("new/root.mod", "new root\n"),
        ("new/shared.mod", "same\n"),
        ("same.mod", "same\n"),
    ]);
    let mut diff = comparison();
    populate_captured_sources(
        &mut diff,
        captured("before", "old/root.mod", &before),
        captured("after", "new/root.mod", &after),
        &[],
    )
    .unwrap();
    assert_eq!(diff.source_changes.files.len(), 5);
    assert_eq!(
        diff.source_changes.files[0].correspondence,
        SourceCorrespondence::SelectedRoots
    );
    for file in &diff.source_changes.files[1..] {
        assert_eq!(file.correspondence, SourceCorrespondence::Unpaired);
        assert!(file.before.is_none() != file.after.is_none());
    }
    assert_eq!(
        diff.source_changes
            .files
            .iter()
            .filter(|file| file.change == ChangeKind::Removed)
            .count(),
        2
    );
    assert_eq!(
        diff.source_changes
            .files
            .iter()
            .filter(|file| file.change == ChangeKind::Added)
            .count(),
        2
    );
}

#[test]
fn normalized_written_identity_proof_pairs_different_registry_keys() {
    let before = files(&[("root.mod", "root\n"), ("file:///repo/inc.mod", "a\n")]);
    let after = files(&[("root.mod", "root\n"), ("C:/repo/inc.mod", "b\n")]);
    let pair = SourceFilePair {
        before_key: "file:///repo/inc.mod",
        after_key: "C:/repo/inc.mod",
        proof: SourceIdentityProof::SameWrittenFileIdentity,
    };
    let mut diff = comparison();
    populate_captured_sources(
        &mut diff,
        captured("before", "root.mod", &before),
        captured("after", "root.mod", &after),
        &[pair],
    )
    .unwrap();
    assert_eq!(diff.source_changes.files.len(), 1);
    assert_eq!(
        diff.source_changes.files[0].correspondence,
        SourceCorrespondence::ProvenFileIdentity
    );
}

#[test]
fn branch_switch_retains_empty_added_and_removed_captured_includes() {
    let before = files(&[("root.mod", "@#include \"a.mod\"\n"), ("a.mod", "")]);
    let after = files(&[("root.mod", "@#include \"b.mod\"\n"), ("b.mod", "")]);
    let mut diff = comparison();
    populate_captured_sources(
        &mut diff,
        captured("before", "root.mod", &before),
        captured("after", "root.mod", &after),
        &[],
    )
    .unwrap();
    assert_eq!(diff.source_changes.files.len(), 3);
    assert_eq!(diff.source_changes.files[1].change, ChangeKind::Removed);
    assert_eq!(diff.source_changes.files[2].change, ChangeKind::Added);
    for file in &diff.source_changes.files[1..] {
        assert!(file.hunks.is_empty());
        assert_eq!(file.availability, Availability::Complete);
        assert!(
            file.before
                .as_ref()
                .or(file.after.as_ref())
                .unwrap()
                .exact_text_available
        );
    }
}

#[test]
fn empty_paired_files_are_equal_but_empty_to_nonempty_has_a_hunk() {
    let before = files(&[("root.mod", ""), ("empty.mod", "")]);
    let after = files(&[("root.mod", "x\n"), ("empty.mod", "")]);
    let mut diff = comparison();
    populate_captured_sources(
        &mut diff,
        captured("before", "root.mod", &before),
        captured("after", "root.mod", &after),
        &[same_key("empty.mod")],
    )
    .unwrap();
    assert_eq!(diff.source_changes.files.len(), 1);
    let hunk = &diff.source_changes.files[0].hunks[0];
    assert_eq!(
        (
            hunk.before_start,
            hunk.before_lines,
            hunk.after_start,
            hunk.after_lines
        ),
        (1, 0, 1, 1)
    );
    assert_eq!(hunk.lines[0].text, "x\n");
}

#[test]
fn source_retains_comments_spacing_inactive_macro_and_native_edits() {
    let before = files(&[(
        "root.mod",
        "// comment\n@#if 0\nparameters ignored;\n@#endif\nplot(x, 'old');\np = 1;\n",
    )]);
    let after = files(&[(
        "root.mod",
        "// changed\n@#if 0\nparameters edited;\n@#endif\nplot(x, 'new');\np=1;\n",
    )]);
    let mut diff = comparison();
    populate_captured_sources(
        &mut diff,
        captured("before", "root.mod", &before),
        captured("after", "root.mod", &after),
        &[],
    )
    .unwrap();
    assert!(diff.semantic.rows.is_empty());
    let text: String = diff.source_changes.files[0]
        .hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .map(|line| line.text.as_str())
        .collect();
    for fragment in [
        "// changed",
        "parameters edited;",
        "plot(x, 'new');",
        "p=1;",
    ] {
        assert!(text.contains(fragment), "missing {fragment}");
    }
}

#[test]
fn parser_newline_rule_preserves_crlf_and_final_newline_changes() {
    for (old, new, changed) in [
        ("α\rβ\r", "α\nβ\n", false),
        ("α\r\n", "α\n", true),
        ("α", "α\n", true),
    ] {
        let before = files(&[("root.mod", old)]);
        let after = files(&[("root.mod", new)]);
        let mut diff = comparison();
        populate_captured_sources(
            &mut diff,
            captured("before", "root.mod", &before),
            captured("after", "root.mod", &after),
            &[],
        )
        .unwrap();
        assert_eq!(!diff.source_changes.files.is_empty(), changed);
        if changed {
            let file = &diff.source_changes.files[0];
            let lines = &file.hunks[0].lines;
            assert_eq!(
                lines
                    .iter()
                    .filter(|line| line.role != TokenRole::Added)
                    .map(|line| line.text.as_str())
                    .collect::<String>(),
                old
            );
            assert_eq!(
                lines
                    .iter()
                    .filter(|line| line.role != TokenRole::Removed)
                    .map(|line| line.text.as_str())
                    .collect::<String>(),
                new
            );
        }
    }
}

#[test]
fn separated_hunks_have_independent_one_based_line_coordinates() {
    let old: String = (1..=30).map(|line| format!("line {line}\n")).collect();
    let new = old
        .replace("line 4\n", "edited four\n")
        .replace("line 25\n", "edited twentyfive\n");
    let before = files(&[("root.mod", &old)]);
    let after = files(&[("root.mod", &new)]);
    let mut diff = comparison();
    populate_captured_sources(
        &mut diff,
        captured("before", "root.mod", &before),
        captured("after", "root.mod", &after),
        &[],
    )
    .unwrap();
    let hunks = &diff.source_changes.files[0].hunks;
    assert_eq!(hunks.len(), 2);
    assert_eq!(hunks[0].before_start, 1);
    assert_eq!(hunks[1].before_start, 22);
    assert_eq!(hunks[1].after_start, 22);
    assert_eq!(hunks[1].before_lines, 7);
    assert_eq!(hunks[1].after_lines, 7);
}

#[test]
fn inserted_lines_move_only_the_after_hunk_coordinate() {
    let old: String = (1..=25).map(|line| format!("line {line}\n")).collect();
    let new = format!("inserted\n{}", old.replace("line 20\n", "changed twenty\n"));
    let before = files(&[("root.mod", &old)]);
    let after = files(&[("root.mod", &new)]);
    let mut diff = comparison();
    populate_captured_sources(
        &mut diff,
        captured("before", "root.mod", &before),
        captured("after", "root.mod", &after),
        &[],
    )
    .unwrap();
    let hunks = &diff.source_changes.files[0].hunks;
    assert_eq!(hunks.len(), 2);
    assert_eq!((hunks[1].before_start, hunks[1].after_start), (17, 18));
}

#[test]
fn alignment_charge_is_comparison_wide_and_fallback_keeps_registry_actions() {
    let before = files(&[("root.mod", "a\nb\n"), ("include.mod", "a\nb\n")]);
    let after = files(&[("root.mod", "c\nd\n"), ("include.mod", "c\nd\n")]);
    let mut diff = compare_models_with_budgets(
        &parse(""),
        &parse(""),
        None,
        None,
        ComparisonBudgets {
            source_alignment_cells: 4,
            ..ComparisonBudgets::default()
        },
    );
    populate_captured_sources(
        &mut diff,
        captured("before", "root.mod", &before),
        captured("after", "root.mod", &after),
        &[same_key("include.mod")],
    )
    .unwrap();
    assert_eq!(
        diff.source_changes.files[0].availability,
        Availability::Complete
    );
    let file = &diff.source_changes.files[1];
    assert_eq!(file.availability, Availability::LimitExceeded);
    assert!(file.hunks.is_empty());
    assert_eq!(file.omitted_hunks, None);
    assert_eq!(file.limits[0].code, "source_alignment_limit");
    assert!(file.before.as_ref().unwrap().exact_text_available);
    assert!(file.after.as_ref().unwrap().exact_text_available);
    assert_eq!(diff.source_changes.availability, Availability::Partial);
}

#[test]
fn hunk_cap_is_comparison_wide_with_exact_omitted_counts() {
    let before = files(&[("root.mod", "a\n"), ("include.mod", "a\n")]);
    let after = files(&[("root.mod", "b\n"), ("include.mod", "b\n")]);
    let mut diff = compare_models_with_budgets(
        &parse(""),
        &parse(""),
        None,
        None,
        ComparisonBudgets {
            source_hunks: 1,
            ..ComparisonBudgets::default()
        },
    );
    populate_captured_sources(
        &mut diff,
        captured("before", "root.mod", &before),
        captured("after", "root.mod", &after),
        &[same_key("include.mod")],
    )
    .unwrap();
    enforce_output_budget(&mut diff);
    assert_eq!(diff.source_changes.files[0].hunks.len(), 1);
    let file = &diff.source_changes.files[1];
    assert!(file.hunks.is_empty());
    assert_eq!(file.omitted_hunks, Some(1));
    assert_eq!(file.limits[0].omitted, Some(1));
    assert!(file.after.as_ref().unwrap().exact_text_available);
}

#[test]
fn serialized_output_cap_omits_hunks_but_retains_complete_text_actions() {
    let before = files(&[("root.mod", "old\n")]);
    let after = files(&[("root.mod", "new\n")]);
    let mut diff = compare_models_with_budgets(
        &parse(""),
        &parse(""),
        None,
        None,
        ComparisonBudgets {
            serialized_output_bytes: 10,
            ..ComparisonBudgets::default()
        },
    );
    populate_captured_sources(
        &mut diff,
        captured("before", "root.mod", &before),
        captured("after", "root.mod", &after),
        &[],
    )
    .unwrap();
    enforce_output_budget(&mut diff);
    let file = &diff.source_changes.files[0];
    assert!(file.hunks.is_empty());
    assert_eq!(file.omitted_hunks, Some(1));
    assert!(file.before.as_ref().unwrap().exact_text_available);
    assert_eq!(diff.source_changes.availability, Availability::Partial);
}

#[test]
fn long_equal_context_does_not_spend_quadratic_alignment_work() {
    let old: String = (0..5000).map(|line| format!("line {line}\n")).collect();
    let new = old.replace("line 2500\n", "changed\n");
    let before = files(&[("root.mod", &old)]);
    let after = files(&[("root.mod", &new)]);
    let mut diff = compare_models_with_budgets(
        &parse(""),
        &parse(""),
        None,
        None,
        ComparisonBudgets {
            source_alignment_cells: 1,
            ..ComparisonBudgets::default()
        },
    );
    populate_captured_sources(
        &mut diff,
        captured("before", "root.mod", &before),
        captured("after", "root.mod", &after),
        &[],
    )
    .unwrap();
    assert_eq!(diff.source_changes.availability, Availability::Complete);
    assert_eq!(diff.semantic.work.source_alignment_cells, 1);
    assert_eq!(diff.source_changes.files[0].hunks.len(), 1);
}

#[test]
fn supplied_boundary_names_unexamined_files_without_downgrading_captured_file_detail() {
    let before = files(&[("root.mod", "a\n")]);
    let after = files(&[("root.mod", "a\n")]);
    let mut old = captured("before", "root.mod", &before);
    let mut new = captured("after", "root.mod", &after);
    old.boundary = CaptureBoundary::SuppliedRootAndExecutedIncludes;
    new.boundary = CaptureBoundary::SuppliedRootAndExecutedIncludes;
    let mut diff = comparison();
    populate_captured_sources(&mut diff, old, new, &[]).unwrap();
    assert_eq!(diff.source_changes.availability, Availability::Complete);
    assert!(diff.source_changes.files.is_empty());
    assert_eq!(
        diff.coverage.source_boundary,
        "supplied_roots_and_executed_includes"
    );
    assert!(diff
        .coverage
        .limits
        .iter()
        .any(|limit| limit.code == "sources_not_captured"));
    assert!(diff
        .coverage
        .limits
        .iter()
        .any(|limit| limit.code == "supplied_source_boundary"));
    assert!(!diff
        .coverage
        .limits
        .iter()
        .any(|limit| limit.code == "capture_boundary_unavailable"));
}

#[test]
fn missing_captured_root_withholds_source_rows_and_actions() {
    let before = files(&[("child.mod", "captured prefix\n")]);
    let after = files(&[("root.mod", "root\n")]);
    let mut diff = comparison();
    assert!(populate_captured_sources(
        &mut diff,
        captured("before", "root.mod", &before),
        captured("after", "root.mod", &after),
        &[]
    )
    .is_err());
    assert_eq!(diff.source_changes.availability, Availability::NotAvailable);
    assert!(diff.source_changes.files.is_empty());
    assert_eq!(diff.coverage.source_boundary, "capture_unavailable");
}

#[test]
fn duplicate_or_missing_or_root_or_mismatched_key_proofs_are_rejected() {
    let before = files(&[("root.mod", ""), ("a.mod", "a\n")]);
    let after = files(&[("root.mod", ""), ("a.mod", "b\n"), ("b.mod", "b\n")]);
    for pairs in [
        vec![same_key("a.mod"), same_key("a.mod")],
        vec![same_key("missing.mod")],
        vec![same_key("root.mod")],
        vec![SourceFilePair {
            before_key: "a.mod",
            after_key: "b.mod",
            proof: SourceIdentityProof::SameRepositoryKey,
        }],
    ] {
        let mut diff = comparison();
        assert!(populate_captured_sources(
            &mut diff,
            captured("before", "root.mod", &before),
            captured("after", "root.mod", &after),
            &pairs
        )
        .is_err());
        assert!(diff.source_changes.files.is_empty());
    }
}

#[test]
fn duplicate_input_ids_reject_a_source_registry_ambiguity() {
    let sources = files(&[("root.mod", "")]);
    let mut diff = comparison();
    assert!(populate_captured_sources(
        &mut diff,
        captured("same", "root.mod", &sources),
        captured("same", "root.mod", &sources),
        &[],
    )
    .is_err());
    assert_eq!(diff.source_changes.availability, Availability::NotAvailable);
    assert!(diff.source_changes.files.is_empty());
}

#[test]
fn default_work_budget_omits_large_alignment_before_allocating_a_matrix() {
    let old: String = (0..1200).map(|line| format!("old {line}\n")).collect();
    let new: String = (0..1200).map(|line| format!("new {line}\n")).collect();
    let before = files(&[("root.mod", &old)]);
    let after = files(&[("root.mod", &new)]);
    let mut diff = comparison();
    populate_captured_sources(
        &mut diff,
        captured("before", "root.mod", &before),
        captured("after", "root.mod", &after),
        &[],
    )
    .unwrap();
    assert_eq!(diff.semantic.work.source_alignment_cells, 0);
    assert_eq!(
        diff.source_changes.files[0].availability,
        Availability::LimitExceeded
    );
    assert!(diff.source_changes.files[0].hunks.is_empty());
    assert!(
        diff.source_changes.files[0]
            .after
            .as_ref()
            .unwrap()
            .exact_text_available
    );
}

#[test]
fn newline_dense_added_file_is_limited_before_source_line_materialization() {
    let before = files(&[("root.mod", "")]);
    let text = "\n".repeat(100_001);
    let after = files(&[("root.mod", ""), ("added.mod", &text)]);
    let mut diff = comparison();
    populate_captured_sources(
        &mut diff,
        captured("before", "root.mod", &before),
        captured("after", "root.mod", &after),
        &[],
    )
    .unwrap();
    let file = &diff.source_changes.files[0];
    assert_eq!(file.change, ChangeKind::Added);
    assert_eq!(file.availability, Availability::LimitExceeded);
    assert!(file.hunks.is_empty());
    assert_eq!(file.omitted_hunks, None);
    assert_eq!(file.limits[0].code, "source_materialization_limit");
    assert_eq!(diff.semantic.work.source_alignment_cells, 0);
    assert!(file.after.as_ref().unwrap().exact_text_available);
}

#[test]
fn lone_cr_line_count_is_bounded_before_normalization_allocation() {
    let before = files(&[("root.mod", "")]);
    let text = "\r".repeat(100_001);
    let after = files(&[("root.mod", &text)]);
    let mut diff = comparison();
    populate_captured_sources(
        &mut diff,
        captured("before", "root.mod", &before),
        captured("after", "root.mod", &after),
        &[],
    )
    .unwrap();
    assert_eq!(
        diff.source_changes.files[0].limits[0].code,
        "source_materialization_limit"
    );
    assert!(diff.source_changes.files[0].hunks.is_empty());
}

#[test]
fn long_single_line_is_limited_before_hunk_text_allocation() {
    let before = files(&[("root.mod", "")]);
    let text = "x".repeat(8 * 1024 * 1024 + 1);
    let after = files(&[("root.mod", &text)]);
    let mut diff = comparison();
    populate_captured_sources(
        &mut diff,
        captured("before", "root.mod", &before),
        captured("after", "root.mod", &after),
        &[],
    )
    .unwrap();
    let file = &diff.source_changes.files[0];
    assert_eq!(file.limits[0].code, "source_materialization_limit");
    assert!(file.hunks.is_empty());
    assert!(file.after.as_ref().unwrap().exact_text_available);
}

#[test]
fn line_materialization_budget_is_shared_across_files() {
    let before = files(&[("root.mod", "")]);
    let text = "\n".repeat(50_001);
    let after = files(&[("root.mod", ""), ("a.mod", &text), ("b.mod", &text)]);
    let mut diff = comparison();
    populate_captured_sources(
        &mut diff,
        captured("before", "root.mod", &before),
        captured("after", "root.mod", &after),
        &[],
    )
    .unwrap();
    assert_eq!(
        diff.source_changes.files[0].availability,
        Availability::Complete
    );
    assert_eq!(
        diff.source_changes.files[1].availability,
        Availability::LimitExceeded
    );
    assert_eq!(
        diff.source_changes.files[1].limits[0].code,
        "source_materialization_limit"
    );
}

#[test]
fn file_output_order_and_pointers_do_not_depend_on_proof_input_order() {
    let before = files(&[("root.mod", ""), ("a.mod", "old\n"), ("b.mod", "old\n")]);
    let after = files(&[("root.mod", ""), ("a.mod", "new\n"), ("b.mod", "new\n")]);
    let mut first = comparison();
    let mut second = comparison();
    populate_captured_sources(
        &mut first,
        captured("before", "root.mod", &before),
        captured("after", "root.mod", &after),
        &[same_key("a.mod"), same_key("b.mod")],
    )
    .unwrap();
    populate_captured_sources(
        &mut second,
        captured("before", "root.mod", &before),
        captured("after", "root.mod", &after),
        &[same_key("b.mod"), same_key("a.mod")],
    )
    .unwrap();
    assert_eq!(first.source_changes, second.source_changes);
    assert_eq!(
        first.source_changes.files[0].pointer,
        "/source_changes/files/0"
    );
    assert_eq!(
        first.source_changes.files[1].pointer,
        "/source_changes/files/1"
    );
}
