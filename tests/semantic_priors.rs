use dygnosis::model_diff::{compare_models, compare_models_with_budgets, ModelDiff};
use dygnosis::parser::parse;
use dygnosis::semantic_diff::{
    Availability, ChangeFacet, ChangeKind, ComparisonBudgets, FieldChange, FieldState, FieldValue,
    HighlightBasis, SemanticFamily, SemanticRow, ValueState,
};

fn compare(before: &str, after: &str) -> ModelDiff {
    compare_models(&parse(before), &parse(after))
}
fn prior_rows(diff: &ModelDiff) -> Vec<&SemanticRow> {
    diff.semantic
        .rows
        .iter()
        .filter(|row| row.family == SemanticFamily::Priors)
        .collect()
}
fn field<'a>(row: &'a SemanticRow, name: &str) -> &'a FieldChange {
    row.fields
        .iter()
        .find(|field| field.name == name)
        .expect("named prior field")
}
fn entry(row: &str) -> String {
    format!("parameters p q; estimated_params; {row} end;")
}
fn reconstruct(row: &SemanticRow) {
    for detail in &row.expressions {
        for side in detail.before.iter().chain(&detail.after) {
            assert_eq!(
                side.runs
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<String>(),
                side.text
            );
        }
    }
}

#[test]
fn mean_std_and_full_distribution_edits_keep_other_fields_plain() {
    let before = entry("p, normal_pdf,0.8,0.1;");
    for (after, changed) in [
        (before.replace("0.8", "0.9"), "prior.mean"),
        (before.replace("0.1", "0.2"), "prior.standard_deviation"),
        (before.replace("normal_pdf", "gamma_pdf"), "distribution"),
    ] {
        let diff = compare(&before, &after);
        let rows = prior_rows(&diff);
        assert_eq!(rows.len(), 1);
        let row = rows[0];
        assert!(field(row, changed).changed);
        for name in ["distribution", "prior.mean", "prior.standard_deviation"] {
            if name != changed {
                assert!(!field(row, name).changed);
            }
        }
        reconstruct(row);
    }
}

#[test]
fn all_existing_shapes_retain_spelling_without_distribution_equivalence() {
    for shape in [
        "beta_pdf",
        "gamma_pdf",
        "normal_pdf",
        "uniform_pdf",
        "inv_gamma_pdf",
        "inv_gamma1_pdf",
        "inv_gamma2_pdf",
        "weibull_pdf",
    ] {
        let model = parse(&entry(&format!("p,{shape},0.4,0.1;")));
        let retained = &model.estimated_params[0].retained;
        assert_eq!(model.name(retained.distribution.unwrap()), shape);
        assert!(retained.positions_complete);
    }
    let diff = compare(
        &entry("p,inv_gamma_pdf,0.4,0.1;"),
        &entry("p,inv_gamma1_pdf,0.4,0.1;"),
    );
    assert!(field(prior_rows(&diff)[0], "distribution").changed);
}

#[test]
fn equal_known_values_do_not_hide_written_expression_change() {
    let diff = compare(
        &entry("p,normal_pdf,2,0.1;"),
        &entry("p,normal_pdf,1+1,0.1;"),
    );
    let rows = prior_rows(&diff);
    let row = rows[0];
    assert!(field(row, "prior.mean").changed);
    let value = field(row, "prior.mean.value");
    assert!(!value.changed);
    assert_eq!(value.before, FieldState::number(Some(2.0)));
    assert_eq!(value.numeric_difference, Some(0.0));
}

#[test]
fn known_expression_value_can_change_while_written_prior_text_stays_equal() {
    let before = "parameters p q; q=1; estimated_params; p,normal_pdf,q,0.1; end;";
    let diff = compare(before, &before.replace("q=1", "q=2"));
    let rows = prior_rows(&diff);
    assert_eq!(rows.len(), 1);
    assert!(!field(rows[0], "prior.mean").changed);
    let value = field(rows[0], "prior.mean.value");
    assert_eq!(value.before, FieldState::number(Some(1.0)));
    assert_eq!(value.after, FieldState::number(Some(2.0)));
    assert_eq!(value.numeric_difference, Some(1.0));
}

#[test]
fn empty_initial_value_never_shifts_a_written_optimizer_bound() {
    let before = entry("p,,0.1,0.9,beta_pdf,0.5,0.1;");
    let diff = compare(&before, &before.replace("0.1,0.9", "0.2,0.9"));
    let rows = prior_rows(&diff);
    let row = rows[0];
    assert_eq!(
        field(row, "optimizer.initial").before.state,
        ValueState::Empty
    );
    assert_eq!(
        field(row, "optimizer.lower_bound").before,
        FieldState::text("0.1")
    );
    assert_eq!(
        field(row, "optimizer.lower_bound").after,
        FieldState::text("0.2")
    );
    assert!(!field(row, "optimizer.upper_bound").changed);
    // The legacy compact diagnostic input is deliberately unchanged.
    let old = parse(&before);
    assert_eq!(old.estimated_params[0].init, Some(0.1));
    assert_eq!(old.estimated_params[0].lower, Some(0.9));
    assert!(old.estimated_params[0].upper.is_none());
}

#[test]
fn empty_mean_never_shifts_written_standard_deviation() {
    let before = entry("p,normal_pdf,,0.1;");
    let diff = compare(&before, &before.replace("0.1", "0.2"));
    let rows = prior_rows(&diff);
    let row = rows[0];
    assert_eq!(field(row, "prior.mean").before.state, ValueState::Empty);
    assert_eq!(
        field(row, "prior.standard_deviation").before,
        FieldState::text("0.1")
    );
    assert_eq!(
        field(row, "prior.standard_deviation.value").before,
        FieldState::number(Some(0.1))
    );
    let old = parse(&before);
    assert!(old.estimated_params[0].mean_expr.is_some());
    assert!(old.estimated_params[0].std_expr.is_none());
}

#[test]
fn empty_support_slots_keep_proposal_scale_in_its_original_position() {
    let before = entry("p,normal_pdf,0.5,0.1,,,0.2;");
    let diff = compare(&before, &before.replace("0.2", "0.3"));
    let rows = prior_rows(&diff);
    let row = rows[0];
    for name in ["prior.support_parameter_3", "prior.support_parameter_4"] {
        assert_eq!(field(row, name).before.state, ValueState::Empty);
    }
    assert_eq!(field(row, "proposal.scale").before, FieldState::text("0.2"));
    assert_eq!(
        field(row, "proposal.scale.value").before.state,
        ValueState::Unknown
    );
    assert!(row
        .limits
        .iter()
        .any(|limit| limit.code == "proposal_scale_evaluation_unavailable"));
    assert!(field(row, "optimizer.lower_bound").before.state == ValueState::Absent);
}

#[test]
fn support_parameters_change_separately_from_optimizer_bounds() {
    let before = entry("p,0.4,0,1,beta_pdf,0.5,0.1,-1,2,0.2;");
    for (after, changed) in [
        (
            before.replace("0.1,-1,2", "0.1,-2,2"),
            "prior.support_parameter_3",
        ),
        (
            before.replace("-1,2,0.2", "-1,3,0.2"),
            "prior.support_parameter_4",
        ),
    ] {
        let diff = compare(&before, &after);
        let rows = prior_rows(&diff);
        assert_eq!(rows.len(), 1);
        assert!(field(rows[0], changed).changed);
        for name in [
            "optimizer.initial",
            "optimizer.lower_bound",
            "optimizer.upper_bound",
            "prior.mean",
            "proposal.scale",
        ] {
            assert!(!field(rows[0], name).changed);
        }
    }
}

#[test]
fn nested_expression_commas_are_not_optional_slot_separators() {
    let before = entry("p,f(1,2),0,1,normal_pdf,0.4,0.1;");
    let diff = compare(&before, &before.replace("f(1,2)", "f(1,3)"));
    let rows = prior_rows(&diff);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        field(rows[0], "optimizer.initial").before,
        FieldState::text("f(1, 2)")
    );
    assert_eq!(
        field(rows[0], "optimizer.lower_bound").before,
        FieldState::text("0")
    );
    assert_eq!(
        field(rows[0], "optimizer.upper_bound").before,
        FieldState::text("1")
    );
}

#[test]
fn prior_function_summary_retains_sticky_flags_with_history_limit() {
    let diff = compare("prior_function;", "prior_function(function='f');");
    let rows = prior_rows(&diff);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        field(rows[0], "prior_function_has_function").before,
        FieldState::boolean(false)
    );
    assert_eq!(
        field(rows[0], "prior_function_has_function").after,
        FieldState::boolean(true)
    );
    assert!(rows[0]
        .limits
        .iter()
        .any(|limit| limit.code == "prior_function_history_unavailable"));
}

#[test]
fn omitted_empty_zero_and_unknown_states_remain_distinct() {
    let before = entry("p,normal_pdf,0.5,0.1;");
    let after = entry("p,,normal_pdf,0.5,0.1;");
    let diff = compare(&before, &after);
    let rows = prior_rows(&diff);
    assert_eq!(
        field(rows[0], "optimizer.initial").before.state,
        ValueState::Absent
    );
    assert_eq!(
        field(rows[0], "optimizer.initial").after.state,
        ValueState::Empty
    );
    let diff = compare(&entry("p,0;"), &entry("p,q;"));
    let rows = prior_rows(&diff);
    assert_eq!(
        field(rows[0], "optimizer.initial.value").before,
        FieldState::number(Some(0.0))
    );
    assert_eq!(
        field(rows[0], "optimizer.initial.value").after.state,
        ValueState::Unknown
    );
}

#[test]
fn override_blocks_keep_initial_and_bounds_distinct() {
    for (kind, row, after, name) in [
        (
            "estimated_params_init",
            "p,0.2;",
            "p,0.3;",
            "optimizer.initial",
        ),
        (
            "estimated_params_bounds",
            "p,0,1;",
            "p,-1,1;",
            "optimizer.lower_bound",
        ),
    ] {
        let before = format!("parameters p; {kind}; {row} end;");
        let diff = compare(&before, &before.replace(row, after));
        let rows = prior_rows(&diff);
        assert_eq!(rows.len(), 1);
        assert!(field(rows[0], name).changed);
        assert_eq!(
            field(rows[0], "prior.mean").before.state,
            ValueState::Absent
        );
        if kind == "estimated_params_bounds" {
            assert_eq!(
                field(rows[0], "optimizer.initial").before.state,
                ValueState::Absent
            );
            assert_eq!(
                field(rows[0], "optimizer.upper_bound").before,
                FieldState::text("1")
            );
        }
    }
}

#[test]
fn target_kinds_and_ordered_correlation_names_are_retained() {
    for target in ["p", "stderr e", "corr e,f", "skew e"] {
        let before = format!(
            "parameters p; varexo e f; estimated_params; {target},normal_pdf,0.4,0.1; end;"
        );
        let diff = compare(&before, &before.replace("0.4", "0.5"));
        let rows = prior_rows(&diff);
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0]
                .after
                .as_ref()
                .unwrap()
                .context
                .as_ref()
                .unwrap()
                .name,
            "estimated_params"
        );
        assert!(field(rows[0], "target_kind").before.value.is_some());
        assert!(field(rows[0], "target_symbol_kinds").before.value.is_some());
    }
}

#[test]
fn overwrite_and_empty_use_calibration_have_named_block_operations() {
    let diff = compare(
        &entry("p,normal_pdf,0.4,0.1;"),
        &entry("p,normal_pdf,0.4,0.1;")
            .replace("estimated_params;", "estimated_params(overwrite);"),
    );
    let rows = prior_rows(&diff);
    assert_eq!(rows.len(), 1);
    assert_eq!(field(rows[0], "overwrite").after, FieldState::boolean(true));
    assert_eq!(
        field(rows[0], "block_operation").after,
        FieldState::text("overwrite")
    );
    let diff = compare(
        "parameters p;",
        "parameters p; estimated_params_init(use_calibration); end;",
    );
    let rows = prior_rows(&diff);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        field(rows[0], "use_calibration").after,
        FieldState::boolean(true)
    );
}

#[test]
fn removal_is_a_written_operation_without_inferred_effective_prior() {
    let diff = compare(
        "parameters p q; estimated_params_remove; p; end;",
        "parameters p q; estimated_params_remove; q; end;",
    );
    let rows = prior_rows(&diff);
    assert_eq!(rows.len(), 2);
    assert!(rows
        .iter()
        .all(|row| row.before.is_none() || row.after.is_none()));
    assert!(rows
        .iter()
        .all(|row| field(row, "distribution").before.state == ValueState::Absent));
}

#[test]
fn dotted_prior_and_optimizer_options_keep_named_values_separate() {
    for (statement, changed, label) in [
        (
            "p.prior(shape=beta,mean=0.4,stdev=0.1,domain=[0,1]);",
            "mean=0.4",
            "option.mean",
        ),
        (
            "p.options(init=0.4,bounds=[0,1],jscale=0.2);",
            "init=0.4",
            "option.init",
        ),
        (
            "std(e).prior(shape=gamma,mean=0.4,stdev=0.1);",
            "mean=0.4",
            "option.mean",
        ),
        (
            "corr(e,f).prior(shape=normal,mean=0.4,stdev=0.1);",
            "mean=0.4",
            "option.mean",
        ),
    ] {
        let before = format!("parameters p; varexo e f; {statement}");
        let after = before.replace(changed, &changed.replace("0.4", "0.5"));
        let diff = compare(&before, &after);
        let rows = prior_rows(&diff);
        assert_eq!(rows.len(), 1);
        assert!(field(rows[0], label).changed);
        reconstruct(rows[0]);
    }
}

#[test]
fn joint_prior_keeps_ordered_targets_and_matrix_values() {
    let before = "parameters p q; [p,q].prior(shape=normal,mean=[0.4,0.5],variance=[[1,0],[0,1]]);";
    let diff = compare(before, &before.replace("[0.4,0.5]", "[0.4,0.6]"));
    let rows = prior_rows(&diff);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        field(rows[0], "target_kind").before,
        FieldState::text("joint_parameters")
    );
    assert!(field(rows[0], "option.mean").changed);
    assert!(!field(rows[0], "option.variance").changed);
}

#[test]
fn copy_and_subsample_context_do_not_resolve_inherited_values() {
    let before = "parameters p q; p.subsamples(first=1990Q1:1999Q4,second=2000Q1:2009Q4); p.first.prior=q.prior;";
    let diff = compare(before, &before.replace("q.prior;", "p.second.prior;"));
    let rows = prior_rows(&diff);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        field(rows[0], "subsample").before,
        FieldState::text("first")
    );
    assert!(field(rows[0], "copy_source").changed);
    assert_eq!(
        field(rows[0], "copy_source_subsample").before.state,
        ValueState::Absent
    );
    assert_eq!(
        field(rows[0], "copy_source_subsample").after,
        FieldState::text("second")
    );
    assert!(rows[0]
        .limits
        .iter()
        .any(|limit| limit.code == "prior_copy_effective_unavailable"));
}

#[test]
fn repeated_targets_and_blocks_never_pair_by_name_or_list_index() {
    for before in [
        entry("p,normal_pdf,0.4,0.1; p,normal_pdf,0.5,0.1;"),
        "parameters p; estimated_params; p,normal_pdf,0.5,0.1; end; estimated_params; p,normal_pdf,0.5,0.1; end;".into(),
    ] {
        let diff = compare(&before, &before.replace("0.5", "0.6"));
        let rows = prior_rows(&diff);
        assert!(rows.len() >= 2);
        for row in rows {
            if row.fields.iter().any(|field| field.name == "distribution") {
                assert!(row.before.is_none() || row.after.is_none());
                assert_eq!(row.change, ChangeKind::Unpaired);
                assert!(row.expressions.iter().all(|detail| detail.highlight_basis == HighlightBasis::None));
            }
        }
    }
}

#[test]
fn unchanged_unique_statement_anchor_can_prove_a_later_block_context() {
    let before = "parameters p; estimated_params; p,normal_pdf,0.4,0.1; end; estimated_params; p,normal_pdf,0.5,0.1; end;";
    let diff = compare(before, &before.replace("0.5", "0.6"));
    let rows = prior_rows(&diff);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].change, ChangeKind::Changed);
    assert!(rows[0].before.is_some() && rows[0].after.is_some());
    assert_eq!(field(rows[0], "prior.mean").before, FieldState::text("0.5"));
    assert_eq!(field(rows[0], "prior.mean").after, FieldState::text("0.6"));
}

#[test]
fn repeated_macro_executions_keep_distinct_side_occurrences() {
    let before = "parameters p;\n@#for item in 1:2\nestimated_params; p,normal_pdf,0.4,0.1; end;\n@#endfor\n";
    let diff = compare(before, &before.replace("0.4", "0.5"));
    let rows: Vec<_> = prior_rows(&diff)
        .into_iter()
        .filter(|row| row.fields.iter().any(|field| field.name == "distribution"))
        .collect();
    assert_eq!(rows.len(), 4);
    let sides: Vec<_> = rows.iter().filter_map(|row| row.after.as_ref()).collect();
    assert_eq!(sides.len(), 2);
    assert_eq!(
        sides[0].provenance.as_ref().unwrap().span,
        sides[1].provenance.as_ref().unwrap().span
    );
    assert_ne!(sides[0].occurrence, sides[1].occurrence);
}

#[test]
fn included_prior_edit_retains_semantic_detail_and_private_proof() {
    use std::collections::HashMap;
    let root = "@#include \"prior.inc\"\n";
    let before = entry("p,normal_pdf,0.4,0.1;");
    let old_files = HashMap::from([
        ("before/root.mod".into(), root.into()),
        ("before/prior.inc".into(), before.clone()),
    ]);
    let new_files = HashMap::from([
        ("after/root.mod".into(), root.into()),
        ("after/prior.inc".into(), before.replace("0.4", "0.5")),
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
    assert!(rows.iter().any(|row| row["family"] == "priors"));
    assert!(rows
        .iter()
        .all(|row| row["after"].get("provenance").is_none()));
}

#[test]
fn legacy_prior_alias_remains_context_without_invented_named_prior() {
    let diff = compare(
        "parameters p; priors; p,0.4; end;",
        "parameters p; priors; p,0.5; end;",
    );
    assert!(prior_rows(&diff).is_empty());
}

#[test]
fn ignored_support_expressions_add_no_diagnostic_inputs() {
    let before = "parameters p q; var y; model; y=p; end; estimated_params; p,normal_pdf,0.4,0.1,1,1,1; end;";
    let after = before.replace("0.1,1,1,1", "0.1,q,q,q");
    let old = parse(before);
    let new = parse(&after);
    assert_eq!(dygnosis::analyze(&old), dygnosis::analyze(&new));
    assert_eq!(
        old.outside_expression_uses.len(),
        new.outside_expression_uses.len()
    );
    for slot in new.estimated_params[0].retained.slots[5..].iter().flatten() {
        assert!(slot.expr.is_none());
        assert!(slot.known_value.is_none());
    }
}

#[test]
fn existing_empty_slot_diagnostic_inputs_and_fire_controls_are_unchanged() {
    let source = "parameters p q; var y; model; y=p+q; end; estimated_params; p,normal_pdf,,q; q,normal_pdf,0.4,0.1; end;";
    let model = parse(source);
    assert!(model.estimated_params[0].mean_expr.is_some());
    assert!(model.estimated_params[0].std_expr.is_none());
    assert!(dygnosis::check_estimated_params(&model)
        .iter()
        .any(|diagnostic| diagnostic.code == "E248"));
    let beta = parse(&entry("p,beta_pdf,0.5,0.5;"));
    assert!(dygnosis::check_estimated_params(&beta)
        .iter()
        .any(|diagnostic| diagnostic.code == "E250"));
}

#[test]
fn alignment_limit_keeps_exact_prior_text_and_partial_coverage() {
    let diff = compare_models_with_budgets(
        &parse(&entry("p,normal_pdf,0.4,0.1;")),
        &parse(&entry("p,normal_pdf,0.5,0.1;")),
        None,
        None,
        ComparisonBudgets {
            token_alignment_cells: 0,
            ..ComparisonBudgets::default()
        },
    );
    let rows = prior_rows(&diff);
    let mean = rows[0]
        .expressions
        .iter()
        .find(|detail| detail.field == "prior.mean")
        .unwrap();
    assert_eq!(mean.availability, Availability::LimitExceeded);
    assert_eq!(mean.before.as_ref().unwrap().text, "0.4");
    assert_eq!(mean.after.as_ref().unwrap().text, "0.5");
    assert_eq!(diff.coverage.availability, Availability::Partial);
    reconstruct(rows[0]);
}

#[test]
fn unsupported_retained_layout_has_a_specific_field_gap() {
    let before = entry("p,0.1,0.2;");
    let diff = compare(&before, &before.replace("0.2", "0.3"));
    let rows = prior_rows(&diff);
    let row = rows[0];
    assert_eq!(
        field(row, "optimizer.initial").before.state,
        ValueState::Unknown
    );
    assert!(row
        .limits
        .iter()
        .any(|limit| limit.code == "prior_positional_fields_unavailable"));
    assert!(field(row, "written_row").changed);
}

#[test]
fn unchanged_unevaluated_support_text_keeps_coverage_gap_without_change_rows() {
    let source = entry("p,normal_pdf,0.4,0.1,-1,1,0.2;");
    let diff = compare(&source, &source);
    assert!(prior_rows(&diff).is_empty());
    let coverage = diff
        .coverage
        .families
        .iter()
        .find(|family| family.family == SemanticFamily::Priors)
        .unwrap();
    assert_eq!(coverage.availability, Availability::Partial);
    assert!(coverage
        .limits
        .iter()
        .any(|limit| limit.code == "proposal_scale_evaluation_unavailable"));
    for limits in [
        &coverage.limits,
        &diff.semantic.limits,
        &diff.coverage.limits,
    ] {
        assert_eq!(
            limits
                .iter()
                .filter(|limit| limit.code == "proposal_scale_evaluation_unavailable")
                .count(),
            1,
            "One retained field limit must have one entry per coverage level"
        );
    }
}

#[test]
fn unknown_existing_expression_is_a_value_state_not_a_retention_gap() {
    let source = entry("p,normal_pdf,q,0.1;");
    let diff = compare(&source, &source);
    assert!(prior_rows(&diff).is_empty());
    let coverage = diff
        .coverage
        .families
        .iter()
        .find(|family| family.family == SemanticFamily::Priors)
        .unwrap();
    assert_eq!(coverage.availability, Availability::Complete);
    assert!(coverage.limits.is_empty());
}

#[test]
fn retained_prior_slots_use_parser_indices_after_string_coalescing() {
    let before =
        "var y(long_name='a\n{b}'); parameters p; estimated_params; p,normal_pdf,0.4,0.1; end;";
    let diff = compare(before, &before.replace("0.4", "0.5"));
    let rows = prior_rows(&diff);
    assert_eq!(rows.len(), 1);
    let mean = field(rows[0], "prior.mean");
    assert_eq!(mean.before.value, Some(FieldValue::Text("0.4".into())));
    assert_eq!(mean.after.value, Some(FieldValue::Text("0.5".into())));
    assert!(mean.changed);
    assert!(!field(rows[0], "prior.standard_deviation").changed);
    reconstruct(rows[0]);
}

#[test]
fn reordered_unique_entries_preserve_occurrence_order_without_value_edits() {
    let before = entry("p,normal_pdf,0.4,0.1; q,normal_pdf,0.5,0.1;");
    let after = entry("q,normal_pdf,0.5,0.1; p,normal_pdf,0.4,0.1;");
    let diff = compare(&before, &after);
    let rows = prior_rows(&diff);
    assert_eq!(rows.len(), 2);
    assert!(rows
        .iter()
        .all(|row| row.facets.contains(&ChangeFacet::Order)));
    assert!(rows.iter().all(|row| !field(row, "prior.mean").changed));
}
