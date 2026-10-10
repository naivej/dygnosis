//! Semantic token/timing/reference facts preserve the existing pairing contract.

use dygnosis::model_diff::{compare_models_with_budgets, ModelDiff};
use dygnosis::semantic_diff::equations::expression_detail;
use dygnosis::semantic_diff::*;
use dygnosis::{compare_models, parse};

fn changed_equation(diff: &ModelDiff) -> &SemanticRow {
    diff.semantic
        .rows
        .iter()
        .find(|row| row.family == SemanticFamily::Equations && row.change == ChangeKind::Changed)
        .expect("paired equation")
}

fn reconstruction(side: &ExpressionSide) {
    assert_eq!(
        side.runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        side.text
    );
}

fn highlight(side: &ExpressionSide, role: TokenRole) -> String {
    side.runs
        .iter()
        .filter(|run| run.role == role)
        .map(|run| run.text.as_str())
        .collect()
}

#[test]
fn constants_operators_and_timing_suffix_tokens_have_exact_runs() {
    let diff = compare_models(
        &parse("var y; model; [name='Output'] y=0.5*y(-1); end;"),
        &parse("var y; model; [name='Output'] y=0.6+y; end;"),
    );
    let row = changed_equation(&diff);
    let expression = &row.expressions[0];
    assert_eq!(expression.highlight_basis, HighlightBasis::PairedExpression);
    let before = expression.before.as_ref().unwrap();
    let after = expression.after.as_ref().unwrap();
    reconstruction(before);
    reconstruction(after);
    assert!(highlight(before, TokenRole::Removed).contains("0.5"));
    assert!(highlight(before, TokenRole::Removed).contains("(-1)"));
    assert!(highlight(after, TokenRole::Added).contains("0.6"));
    assert!(highlight(after, TokenRole::Added).contains('+'));
    assert!(row.before.as_ref().unwrap().provenance.is_some());
}

#[test]
fn tags_remain_separate_and_reorder_creates_no_equation_rows() {
    let diff = compare_models(
        &parse("var y; model; [name='Output', old] y=1; end;"),
        &parse("var y; model; [name='Output', new] y=1; end;"),
    );
    let row = changed_equation(&diff);
    assert!(row.facets.contains(&ChangeFacet::Tags));
    assert!(row.expressions[0]
        .before
        .as_ref()
        .unwrap()
        .runs
        .iter()
        .all(|run| run.role == TokenRole::Unchanged));
    let diff = compare_models(
        &parse("var y x; model; [name='Y'] y=x; [name='X'] x=1; end;"),
        &parse("var y x; model; [name='X'] x=1; [name='Y'] y=x; end;"),
    );
    assert!(diff.semantic.rows.is_empty());
}

#[test]
fn repeated_uses_pair_only_under_equal_structure_and_counts() {
    let diff = compare_models(
        &parse("var y; model; [name='Output'] y=y(-1)+y(-2); end;"),
        &parse("var y; model; [name='Output'] y=y+y(-3); end;"),
    );
    let timing = &changed_equation(&diff).timing;
    assert_eq!(timing.len(), 3);
    assert!(timing
        .iter()
        .all(|timing| timing.before.is_some() && timing.after.is_some()));
    assert_eq!(timing[1].before.as_ref().unwrap().written_offset, -1);
    assert_eq!(timing[1].after.as_ref().unwrap().written_offset, 0);
    assert_eq!(timing[2].before.as_ref().unwrap().written_offset, -2);
    assert_eq!(timing[2].after.as_ref().unwrap().written_offset, -3);
    let diff = compare_models(
        &parse("var y; model; [name='Output'] y=y(-1)+y(-2); end;"),
        &parse("var y; model; [name='Output'] y=y(-1)+y(-2)+y(-3); end;"),
    );
    let timing = &changed_equation(&diff).timing;
    assert_eq!(timing.len(), 7);
    assert!(timing
        .iter()
        .all(|timing| timing.before.is_none() || timing.after.is_none()));
}

#[test]
fn written_and_converted_timing_changes_are_independent() {
    let diff = compare_models(
        &parse("var y k; model; [name='Output'] y=k(-1); end;"),
        &parse("var y k; predetermined_variables k; model; [name='Output'] y=k; end;"),
    );
    let timing = changed_equation(&diff)
        .timing
        .iter()
        .find(|timing| timing.before.as_ref().is_some_and(|side| side.name == "k"))
        .unwrap();
    assert_eq!(timing.before.as_ref().unwrap().written_offset, -1);
    assert_eq!(timing.after.as_ref().unwrap().written_offset, 0);
    assert_eq!(timing.before.as_ref().unwrap().converted_offset, -1);
    assert_eq!(timing.after.as_ref().unwrap().converted_offset, -1);
}

#[test]
fn convention_only_change_has_symbol_context_and_unchanged_equation_references() {
    let before = "var y k; model; [name='Production'] y=k; end;";
    let after = "var y k; predetermined_variables k; model; [name='Production'] y=k; end;";
    let diff = compare_models(&parse(before), &parse(after));
    assert!(diff.changed_equations.is_empty());
    assert!(diff
        .semantic
        .rows
        .iter()
        .all(|row| row.family != SemanticFamily::Equations));
    let convention = diff
        .semantic
        .rows
        .iter()
        .find(|row| row.name == "k")
        .unwrap();
    assert!(convention
        .facets
        .contains(&ChangeFacet::PredeterminedConvention));
    assert_eq!(convention.references.len(), 2);
    for reference in &diff.semantic.references {
        assert_eq!(reference.symbol, "k");
        assert_eq!(reference.label, "Production");
        assert_eq!(reference.equation_pointer, reference.pointer);
        assert_eq!(reference.timing.written_offset, 0);
        assert_eq!(
            reference.timing.converted_offset,
            if reference.side == Side::Before {
                0
            } else {
                -1
            }
        );
        assert!(reference.provenance.as_ref().unwrap().equation_id.is_some());
    }
}

#[test]
fn parameter_references_include_each_direct_use_in_unchanged_equations() {
    let diff = compare_models(
        &parse("var y; parameters p; p=1; model; [name='Output'] y=p+p; end;"),
        &parse("var y; parameters p; p=2; model; [name='Output'] y=p+p; end;"),
    );
    let row = diff
        .semantic
        .rows
        .iter()
        .find(|row| row.family == SemanticFamily::Parameters)
        .unwrap();
    assert_eq!(row.references.len(), 4);
    assert_eq!(diff.semantic.references[0].occurrence, 1);
    assert_eq!(diff.semantic.references[1].occurrence, 2);
    assert!(diff
        .semantic
        .rows
        .iter()
        .all(|row| row.family != SemanticFamily::Equations));
}

#[test]
fn unpaired_group_keeps_text_only_unique_fragment_and_no_timing_pairs() {
    let diff = compare_models(&parse("var y x; parameters tax; model; [name='Repeated'] y=x; [name='Repeated'] x=y; end;"), &parse("var y x; parameters tax; model; [name='Repeated'] y=x+tax; [name='Repeated'] x=y+tax; end;"));
    assert!(diff.changed_equations.is_empty());
    let rows: Vec<_> = diff
        .semantic
        .rows
        .iter()
        .filter(|row| row.family == SemanticFamily::Equations)
        .collect();
    assert_eq!(rows.len(), 4);
    assert!(rows.iter().all(|row| row.change
        == if row.before.is_some() {
            ChangeKind::Removed
        } else {
            ChangeKind::Added
        }
        && row.timing.is_empty()));
    for row in rows {
        let expression = &row.expressions[0];
        assert_eq!(expression.highlight_basis, HighlightBasis::UnpairedTextOnly);
        if let Some(after) = &expression.after {
            reconstruction(after);
            let added = highlight(after, TokenRole::Added);
            assert!(added.contains('+') && added.contains("tax"), "{added}");
        }
    }
}

#[test]
fn unpaired_repetition_count_cannot_highlight_an_inserted_occurrence() {
    let diff = compare_models(
        &parse("var y x; model; [name='Repeated'] y=x+x; [name='Repeated'] x=y+y; end;"),
        &parse("var y x; model; [name='Repeated'] y=x+x+x; [name='Repeated'] x=y+y+y; end;"),
    );
    for row in diff
        .semantic
        .rows
        .iter()
        .filter(|row| row.family == SemanticFamily::Equations)
    {
        let expression = &row.expressions[0];
        assert_eq!(
            row.change,
            if row.before.is_some() {
                ChangeKind::Removed
            } else {
                ChangeKind::Added
            }
        );
        assert!(expression
            .before
            .iter()
            .chain(&expression.after)
            .flat_map(|side| &side.runs)
            .all(|run| run.role == TokenRole::Unchanged));
    }
}

#[test]
fn heterogeneous_scopes_keep_timing_and_reference_occurrences_separate() {
    let before = "var y; heterogeneity_dimension h; var(heterogeneity=h) c; parameters p; p=1; model; [name='Aggregate'] y=p; end; model(heterogeneity=h); [name='Individual'] c=p+c(-1); end;";
    let after = before.replace("p=1", "p=2").replace("c=p+c(-1)", "c=p+c");
    let diff = compare_models(&parse(before), &parse(&after));
    let equation = changed_equation(&diff);
    assert_eq!(
        equation.after.as_ref().unwrap().scope.dimension.as_deref(),
        Some("h")
    );
    let timing = equation
        .timing
        .iter()
        .find(|timing| {
            timing
                .before
                .as_ref()
                .is_some_and(|side| side.name == "c" && side.written_offset == -1)
        })
        .unwrap();
    assert_eq!(timing.after.as_ref().unwrap().converted_offset, 0);
    let refs: Vec<_> = diff
        .semantic
        .references
        .iter()
        .filter(|reference| reference.symbol == "p")
        .collect();
    assert_eq!(refs.len(), 4);
    assert!(refs
        .iter()
        .any(|reference| reference.scope.dimension.as_deref() == Some("h")));
    assert!(refs
        .iter()
        .any(|reference| reference.scope.dimension.is_none()));
}

#[test]
fn macro_occurrences_with_shared_written_spans_keep_reference_identity() {
    let before = "var y; parameters p; p=1; model;\n@#for item in 1:2\n[name='Repeated'] y=p;\n@#endfor\nend;";
    let after = before.replace("p=1", "p=2");
    let diff = compare_models(&parse(before), &parse(&after));
    let refs: Vec<_> = diff
        .semantic
        .references
        .iter()
        .filter(|reference| reference.side == Side::After)
        .collect();
    assert_eq!(refs.len(), 2);
    assert_eq!(
        refs[0].provenance.as_ref().unwrap().span,
        refs[1].provenance.as_ref().unwrap().span
    );
    assert_ne!(
        refs[0].provenance.as_ref().unwrap().parse_order,
        refs[1].provenance.as_ref().unwrap().parse_order
    );
    assert_ne!(refs[0].equation_index, refs[1].equation_index);
}

#[test]
fn unicode_and_crlf_written_token_reconstruction_uses_no_navigation_offsets() {
    let mut semantic = SemanticDiff::new(ComparisonBudgets::default());
    let detail = expression_detail(
        &mut semantic,
        "expression",
        Some("f('α')\r\n + x(-1)"),
        Some("f('β')\r\n + x"),
    );
    reconstruction(detail.before.as_ref().unwrap());
    reconstruction(detail.after.as_ref().unwrap());
    assert!(highlight(detail.before.as_ref().unwrap(), TokenRole::Removed).contains("'α'"));
    assert!(highlight(detail.after.as_ref().unwrap(), TokenRole::Added).contains("'β'"));
}

#[test]
fn global_token_budget_falls_back_to_exact_plain_expression_text() {
    let diff = compare_models_with_budgets(
        &parse("var y x; model; [name='Y'] y=1; [name='X'] x=2; end;"),
        &parse("var y x; model; [name='Y'] y=3; [name='X'] x=4; end;"),
        None,
        None,
        ComparisonBudgets {
            token_alignment_cells: 16,
            ..ComparisonBudgets::default()
        },
    );
    let expressions: Vec<_> = diff
        .semantic
        .rows
        .iter()
        .filter(|row| row.family == SemanticFamily::Equations)
        .flat_map(|row| &row.expressions)
        .collect();
    assert_eq!(expressions.len(), 2);
    assert_eq!(expressions[0].availability, Availability::Complete);
    assert_eq!(expressions[1].availability, Availability::LimitExceeded);
    for detail in expressions {
        reconstruction(detail.before.as_ref().unwrap());
        reconstruction(detail.after.as_ref().unwrap());
    }
    assert_eq!(diff.semantic.work.token_alignment_cells, 16);
}

#[test]
fn reference_limit_reports_exact_omissions_without_change_row_inflation() {
    let diff = compare_models_with_budgets(
        &parse("var y; parameters p; p=1; model; y=p+p+p; end;"),
        &parse("var y; parameters p; p=2; model; y=p+p+p; end;"),
        None,
        None,
        ComparisonBudgets {
            references_per_side: 1,
            ..ComparisonBudgets::default()
        },
    );
    assert_eq!(diff.semantic.rows.len(), 1);
    assert_eq!(diff.semantic.references.len(), 2);
    assert!(diff
        .semantic
        .references
        .iter()
        .all(|reference| reference.label == "Equation 1" && reference.equation_index == 0));
    assert!(diff
        .semantic
        .limits
        .iter()
        .any(|limit| limit.code == "reference_limit" && limit.omitted == Some(4)));
    assert_eq!(diff.coverage.availability, Availability::Partial);
}

#[test]
fn changed_calibration_and_metadata_share_reference_entries() {
    let diff = compare_models(
        &parse("var y; parameters p (long_name='Before'); p=1; model; y=p; end;"),
        &parse("var y; parameters p (long_name='After'); p=2; model; y=p; end;"),
    );
    assert_eq!(diff.semantic.rows.len(), 2);
    assert_eq!(diff.semantic.references.len(), 2);
    assert_eq!(
        diff.semantic.rows[0].references,
        diff.semantic.rows[1].references
    );
}

#[test]
fn local_bindings_with_the_same_name_stay_in_their_model_scopes() {
    let before = "heterogeneity_dimension h; var y; var(heterogeneity=h) yh; model_local_variable z; model; #z=1; [name='Aggregate'] y=z; end; model(heterogeneity=h); #z=2; [name='Individual'] yh=z(-1); end;";
    let after = before.replace("y=z;", "y=2*z;").replace("yh=z(-1)", "yh=z");
    let diff = compare_models(&parse(before), &parse(&after));
    for row in diff
        .semantic
        .rows
        .iter()
        .filter(|row| row.family == SemanticFamily::Equations)
    {
        let local = row
            .timing
            .iter()
            .find(|timing| timing.before.as_ref().is_some_and(|side| side.name == "z"))
            .unwrap();
        assert_eq!(local.before.as_ref().unwrap().class, "model_local");
        assert_eq!(local.after.as_ref().unwrap().class, "model_local");
        assert!(row.references.is_empty());
    }
    assert!(diff.semantic.references.is_empty());
}

#[test]
fn included_equations_keep_accepted_provenance_and_semantic_wire_detail() {
    use std::collections::HashMap;
    let root = "@#include \"body.inc\"\n";
    let before = "var y; parameters p; p=1; model; [name='Included'] y=p+y(-1); end;";
    let after = before.replace("p=1", "p=2").replace("y(-1)", "y");
    let old_files = HashMap::from([
        ("before/root.mod".into(), root.into()),
        ("before/body.inc".into(), before.into()),
    ]);
    let new_files = HashMap::from([
        ("after/root.mod".into(), root.into()),
        ("after/body.inc".into(), after),
    ]);
    let diff = dygnosis::dynare_compare_models(
        root,
        root,
        Some("before/root.mod"),
        Some("after/root.mod"),
        Some(&old_files),
        Some(&new_files),
        None,
    );
    let rows = diff["semantic"]["rows"].as_array().unwrap();
    assert!(rows.iter().any(|row| row["family"] == "equations"
        && row["expressions"][0]["highlight_basis"] == "paired_expression"));
    let refs = diff["semantic"]["references"].as_array().unwrap();
    assert_eq!(refs.len(), 2);
    assert!(refs.iter().all(|reference| reference["label"] == "Included"
        && reference["equation_pointer"] == "/changed_equations/0"));
}

#[test]
fn large_expression_alignment_is_bounded_before_matrix_allocation() {
    let old = format!("x{}", "+x".repeat(2_000));
    let new = format!("x{}+1", "+x".repeat(2_000));
    let mut semantic = SemanticDiff::new(ComparisonBudgets::default());
    let start = std::time::Instant::now();
    let detail = expression_detail(&mut semantic, "expression", Some(&old), Some(&new));
    assert_eq!(detail.availability, Availability::LimitExceeded);
    assert_eq!(semantic.work.token_alignment_cells, 0);
    reconstruction(detail.before.as_ref().unwrap());
    reconstruction(detail.after.as_ref().unwrap());
    assert!(start.elapsed().as_secs() < 5);
}

#[test]
fn accepted_type_changes_clear_effective_convention_with_equal_equation_text() {
    let before = "var y k; predetermined_variables k; model; [name='Output'] y=k; end;";
    let after = "var y k; predetermined_variables k; change_type(parameters) k; change_type(var) k; model; [name='Output'] y=k; end;";
    let diff = compare_models(&parse(before), &parse(after));
    assert!(diff.symbols_changed.is_empty());
    assert!(diff.changed_equations.is_empty());
    let convention = diff
        .semantic
        .rows
        .iter()
        .find(|row| row.name == "k")
        .unwrap();
    let convention_field = convention
        .fields
        .iter()
        .find(|field| field.name == "predetermined")
        .unwrap();
    assert_eq!(convention_field.before, FieldState::boolean(true));
    assert_eq!(convention_field.after, FieldState::boolean(false));
    assert_eq!(convention.references.len(), 2);
    let restored = after.replace("model;", "predetermined_variables k; model;");
    let diff = compare_models(&parse(before), &parse(&restored));
    assert!(diff.symbols_changed.is_empty());
    assert!(diff.changed_equations.is_empty());
    assert!(!diff.semantic.rows.iter().any(|row| {
        row.family == SemanticFamily::Equations
            || row.family == SemanticFamily::Symbols && row.count_unit == CountUnit::FinalFact
    }));
    // Written operations remain visible after the final convention is restored.
    assert_eq!(
        diff.semantic
            .rows
            .iter()
            .filter(|row| row.family == SemanticFamily::Operations && row.name == "change_type")
            .count(),
        2
    );
}

#[test]
fn later_reference_cap_repairs_self_context_and_preserves_legacy_equation_pointer() {
    let before =
        "var y x; parameters p; p=1; model; [name='Unchanged'] y=p+p; [name='Changed'] x=p; end;";
    let after = before.replace("p=1", "p=2").replace("x=p;", "x=2*p;");
    let mut diff = compare_models(&parse(before), &parse(&after));
    diff.semantic.budgets.references_per_side = 1;
    enforce_output_budget(&mut diff);
    assert_eq!(diff.semantic.references.len(), 2);
    for reference in &diff.semantic.references {
        assert_eq!(reference.equation_pointer, reference.pointer);
    }
    assert_eq!(diff.semantic.references[1].side, Side::After);
    assert_eq!(
        diff.semantic.references[1].pointer,
        "/semantic/references/1"
    );
    let mut diff = compare_models(
        &parse("var y; parameters p; p=1; model; [name='Changed'] y=p+p; end;"),
        &parse("var y; parameters p; p=2; model; [name='Changed'] y=2*p+p; end;"),
    );
    diff.semantic.budgets.references_per_side = 1;
    enforce_output_budget(&mut diff);
    assert_eq!(diff.semantic.references.len(), 2);
    assert!(diff
        .semantic
        .references
        .iter()
        .all(|reference| reference.equation_pointer == "/changed_equations/0"));
}

#[test]
fn repeated_use_literal_reorder_cannot_manufacture_timing_transitions() {
    let before = "var y k; model; [name='Output'] y=k(-1)*1+k*2; end;";
    let after = "var y k; model; [name='Output'] y=k*2+k(-1)*1; end;";
    let diff = compare_models(&parse(before), &parse(after));
    let row = changed_equation(&diff);
    let k_uses: Vec<_> = row
        .timing
        .iter()
        .filter(|timing| {
            timing
                .before
                .as_ref()
                .or(timing.after.as_ref())
                .is_some_and(|side| side.name == "k")
        })
        .collect();
    assert_eq!(k_uses.len(), 4);
    assert!(k_uses
        .iter()
        .all(|timing| timing.before.is_none() || timing.after.is_none()));
    assert!(row
        .limits
        .iter()
        .any(|limit| limit.code == "identifier_correspondence_unpaired"));
}

#[test]
fn full_named_equation_large_sum_uses_iterative_bounded_timing_correspondence() {
    let before = format!(
        "var y x; model; [name='Long'] y=x{}; end;",
        "+x".repeat(600)
    );
    let after = format!(
        "var y x; model; [name='Long'] y=x(-1){}; end;",
        "+x".repeat(600)
    );
    let start = std::time::Instant::now();
    let diff = compare_models_with_budgets(
        &parse(&before),
        &parse(&after),
        None,
        None,
        ComparisonBudgets {
            token_alignment_cells: 0,
            ..ComparisonBudgets::default()
        },
    );
    let row = changed_equation(&diff);
    assert_eq!(row.expressions[0].availability, Availability::LimitExceeded);
    assert!(row
        .limits
        .iter()
        .any(|limit| limit.code == "timing_correspondence_limit"));
    let uses: Vec<_> = row
        .timing
        .iter()
        .filter(|timing| {
            timing
                .before
                .as_ref()
                .or(timing.after.as_ref())
                .is_some_and(|side| side.name == "x")
        })
        .collect();
    assert_eq!(uses.len(), 1_202);
    assert!(uses
        .iter()
        .all(|timing| timing.before.is_none() || timing.after.is_none()));
    assert!(start.elapsed().as_secs() < 5);
}

fn condition_field<'a>(row: &'a SemanticRow, name: &str) -> &'a FieldChange {
    row.fields
        .iter()
        .find(|field| field.name == name)
        .expect("retained condition field")
}

#[test]
fn bound_only_conditions_have_one_semantic_owner_and_preserve_legacy_counts() {
    for tag in ["", "[name='Bounded']"] {
        let before = format!("var y; model; {tag} y=0 _|_ -1 < y < 1; end;");
        let after = before.replace("y < 1", "y < 2");
        let diff = compare_models(&parse(&before), &parse(&after));
        assert!(diff.added_equations.is_empty());
        assert!(diff.removed_equations.is_empty());
        assert!(diff.changed_equations.is_empty());
        assert_eq!(diff.semantic.rows.len(), 1);
        let row = changed_equation(&diff);
        assert_eq!(row.pointer, "/semantic/rows/0");
        assert_eq!(row.facets, vec![ChangeFacet::Complementarity]);
        assert!(!condition_field(row, "expression").changed);
        assert_eq!(
            condition_field(row, "complementarity.variable").after,
            FieldState::text("y")
        );
        assert_eq!(
            condition_field(row, "complementarity.lower_bound").after,
            FieldState::text("-1")
        );
        let upper = condition_field(row, "complementarity.upper_bound");
        assert_eq!(upper.before, FieldState::text("1"));
        assert_eq!(upper.after, FieldState::text("2"));
        assert!(row.before.as_ref().unwrap().provenance.is_some());
        assert!(row.after.as_ref().unwrap().provenance.is_some());
        for detail in &row.expressions {
            reconstruction(detail.before.as_ref().unwrap());
            reconstruction(detail.after.as_ref().unwrap());
        }
    }
}

#[test]
fn body_and_condition_edits_share_the_existing_equation_row() {
    let diff = compare_models(
        &parse("var y; model; [name='Bounded'] y=0 _|_ y > -1; end;"),
        &parse("var y; model; [name='Bounded'] y=1 _|_ y > 0; end;"),
    );
    assert_eq!(diff.changed_equations.len(), 1);
    assert_eq!(diff.semantic.rows.len(), 1);
    let row = changed_equation(&diff);
    assert_eq!(row.pointer, "/changed_equations/0");
    assert!(row.facets.contains(&ChangeFacet::Expression));
    assert!(row.facets.contains(&ChangeFacet::Complementarity));
    assert_eq!(
        condition_field(row, "complementarity.lower_bound").before,
        FieldState::text("-1")
    );
    assert_eq!(
        condition_field(row, "complementarity.upper_bound").after,
        FieldState::absent()
    );
}

#[test]
fn unmatched_condition_text_keeps_unknown_bounds_and_an_explicit_limit() {
    let diff = compare_models(
        &parse("var y; model; [name='Bounded'] y=0 _|_ y+1 > 0; end;"),
        &parse("var y; model; [name='Bounded'] y=0 _|_ y+2 > 0; end;"),
    );
    let row = changed_equation(&diff);
    assert!(condition_field(row, "complementarity.text").changed);
    for name in [
        "complementarity.variable",
        "complementarity.lower_bound",
        "complementarity.upper_bound",
    ] {
        let field = condition_field(row, name);
        assert_eq!(field.before, FieldState::unknown());
        assert_eq!(field.after, FieldState::unknown());
    }
    assert!(row
        .limits
        .iter()
        .any(|limit| limit.code == "complementarity_unmatched"));
    let equation_coverage = diff
        .coverage
        .families
        .iter()
        .find(|family| family.family == SemanticFamily::Equations)
        .unwrap();
    assert_eq!(equation_coverage.availability, Availability::Partial);
    assert!(equation_coverage
        .limits
        .iter()
        .any(|limit| limit.code == "complementarity_unmatched"));
    assert!(diff
        .coverage
        .limits
        .iter()
        .any(|limit| limit.code == "complementarity_unmatched"));
    let absent = compare_models(
        &parse("var y; model; [name='Bounded'] y=0; end;"),
        &parse("var y; model; [name='Bounded'] y=0 _|_ y+1 > 0; end;"),
    );
    assert_eq!(
        condition_field(changed_equation(&absent), "complementarity.variable").before,
        FieldState::absent()
    );
}

#[test]
fn unchanged_conditions_and_equal_repeated_condition_facts_create_no_rows() {
    for source in [
        "var y; model; [name='Bounded'] y=0 _|_ y > -1; end;",
        "var y; model; [name='Repeated'] y=0 _|_ y > -1; [name='Repeated'] y=0 _|_ y > 0; end;",
        "var y; model; y=0 _|_ y > -1; y=0 _|_ y > 0; end;",
    ] {
        let diff = compare_models(&parse(source), &parse(source));
        assert!(diff.semantic.rows.is_empty());
    }
}

#[test]
fn repeated_condition_candidates_stay_separate_without_inferred_pairing() {
    for tag in ["", "[name='Repeated']"] {
        let before = format!("var y; model; {tag} y=0 _|_ y > -1; {tag} y=0 _|_ y > 0; end;");
        let after = before.replace("y > 0", "y > 1");
        let diff = compare_models(&parse(&before), &parse(&after));
        assert!(diff.changed_equations.is_empty());
        assert!(diff.added_equations.is_empty());
        assert!(diff.removed_equations.is_empty());
        assert_eq!(diff.semantic.rows.len(), 2);
        for row in &diff.semantic.rows {
            assert_eq!(
                row.change,
                if row.before.is_some() {
                    ChangeKind::Removed
                } else {
                    ChangeKind::Added
                }
            );
            assert!(row.before.is_none() || row.after.is_none());
            assert!(row.timing.is_empty());
            assert!(row
                .expressions
                .iter()
                .all(|detail| detail.highlight_basis == HighlightBasis::None));
            assert!(row
                .limits
                .iter()
                .any(|limit| limit.code == "condition_correspondence_unpaired"));
        }
    }
}

#[test]
fn heterogeneous_condition_only_row_keeps_dimension_and_exact_source_proof() {
    let before = "heterogeneity_dimension h; var(heterogeneity=h) y; model(heterogeneity=h); [name='Bounded'] y=0 _|_ y > -1; end;";
    let after = before.replace("y > -1", "y > 0");
    let diff = compare_models(&parse(before), &parse(&after));
    let row = changed_equation(&diff);
    assert_eq!(
        row.after.as_ref().unwrap().scope.dimension.as_deref(),
        Some("h")
    );
    assert_eq!(row.after.as_ref().unwrap().scope.domain, "heterogeneous");
    assert!(row
        .after
        .as_ref()
        .unwrap()
        .provenance
        .as_ref()
        .unwrap()
        .equation_id
        .is_some());
}

#[test]
fn removing_a_duplicate_candidate_cannot_create_new_condition_correspondence() {
    let before =
        "var y; model; [name='Repeated'] y=0 _|_ y > -1; [name='Repeated'] y=0 _|_ y > 0; end;";
    let after =
        "var y; model; [name='Repeated'] y=0 _|_ y > 1; [name='Repeated'] y=1 _|_ y > 2; end;";
    let diff = compare_models(&parse(before), &parse(after));
    assert!(diff.changed_equations.is_empty());
    assert_eq!(diff.added_equations.len(), 1);
    assert_eq!(diff.removed_equations.len(), 1);
    assert!(diff
        .semantic
        .rows
        .iter()
        .all(|row| row.before.is_none() || row.after.is_none()));
    let condition_rows: Vec<_> = diff
        .semantic
        .rows
        .iter()
        .filter(|row| row.pointer.starts_with("/semantic/rows/"))
        .collect();
    assert_eq!(condition_rows.len(), 2);
    assert!(condition_rows.iter().all(|row| row.change
        == if row.before.is_some() {
            ChangeKind::Removed
        } else {
            ChangeKind::Added
        }));
    assert!(diff
        .coverage
        .limits
        .iter()
        .any(|limit| limit.code == "condition_correspondence_unpaired"));
}

#[test]
fn unchanged_unmatched_condition_has_no_change_row_but_retains_field_coverage_limit() {
    let source = "var y; model; [name='Bounded'] y=0 _|_ y+1 > 0; end;";
    let diff = compare_models(&parse(source), &parse(source));
    assert!(diff.semantic.rows.is_empty());
    let coverage = diff
        .coverage
        .families
        .iter()
        .find(|family| family.family == SemanticFamily::Equations)
        .unwrap();
    assert_eq!(coverage.availability, Availability::Partial);
    let limits: Vec<_> = coverage
        .limits
        .iter()
        .filter(|limit| limit.code == "complementarity_unmatched")
        .collect();
    assert_eq!(limits.len(), 1);
    assert_eq!(limits[0].omitted, Some(2));
    assert!(diff
        .semantic
        .limits
        .iter()
        .any(|limit| limit.code == "complementarity_unmatched" && limit.omitted == Some(2)));
    assert!(diff
        .coverage
        .limits
        .iter()
        .any(|limit| limit.code == "complementarity_unmatched" && limit.omitted == Some(2)));
}
