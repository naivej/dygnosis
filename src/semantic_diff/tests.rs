use super::*;
use crate::model_diff::{
    compare_models, compare_models_with_budgets, compare_models_with_sources, CompareSource,
};
use crate::parser::parse;

fn row<'a>(
    diff: &'a crate::model_diff::ModelDiff,
    family: SemanticFamily,
    name: &str,
) -> &'a SemanticRow {
    diff.semantic
        .rows
        .iter()
        .find(|row| row.family == family && row.name == name)
        .expect("semantic row")
}

fn field<'a>(row: &'a SemanticRow, name: &str) -> &'a FieldChange {
    row.fields
        .iter()
        .find(|field| field.name == name)
        .expect("named field")
}

#[test]
fn parameter_expression_value_and_finite_difference_are_separate() {
    let before = parse("parameters p; p=1;");
    let after = parse("parameters p; p=1+0;");
    let diff = compare_models(&before, &after);
    let detail = row(&diff, SemanticFamily::Parameters, "p");
    assert!(field(detail, "expression").changed);
    assert!(!field(detail, "evaluated_value").changed);
    assert_eq!(
        field(detail, "evaluated_value").numeric_difference,
        Some(0.0)
    );
    assert_eq!(detail.pointer, "/changed_parameter_values/0");
    assert_eq!(detail.facets, vec![ChangeFacet::Expression]);

    let diff = compare_models(
        &parse("parameters p q; q=1; p=q;"),
        &parse("parameters p q; q=2; p=q;"),
    );
    let detail = row(&diff, SemanticFamily::Parameters, "p");
    assert!(!field(detail, "expression").changed);
    assert!(field(detail, "evaluated_value").changed);
    assert_eq!(
        field(detail, "evaluated_value").numeric_difference,
        Some(1.0)
    );
}

#[test]
fn numeric_tolerance_unknown_absent_zero_and_overflow_are_preserved() {
    let diff = compare_models(
        &parse("parameters p; p=1;"),
        &parse("parameters p; p=1.0000000000001;"),
    );
    assert!(
        !field(
            row(&diff, SemanticFamily::Parameters, "p"),
            "evaluated_value"
        )
        .changed
    );

    let diff = compare_models(&parse("parameters p;"), &parse("parameters p; p=0;"));
    let value = field(
        row(&diff, SemanticFamily::Parameters, "p"),
        "evaluated_value",
    );
    assert_eq!(value.before.state, ValueState::Absent);
    assert_eq!(value.after, FieldState::number(Some(0.0)));

    let diff = compare_models(
        &parse("parameters p; p=0;"),
        &parse("parameters p; p=unknown_function(1);"),
    );
    assert_eq!(
        field(
            row(&diff, SemanticFamily::Parameters, "p"),
            "evaluated_value"
        )
        .after
        .state,
        ValueState::Unknown
    );
    let diff = compare_models(
        &parse("parameters p; p=1e308;"),
        &parse("parameters p; p=-1e308;"),
    );
    assert_eq!(
        field(
            row(&diff, SemanticFamily::Parameters, "p"),
            "evaluated_value"
        )
        .numeric_difference,
        None
    );
    assert_eq!(
        FieldState::number(Some(f64::INFINITY)).state,
        ValueState::Unknown
    );
}

#[test]
fn log_and_convention_changes_have_independent_rows_without_legacy_changes() {
    let diff = compare_models(
        &parse("var y; model; y=0; end;"),
        &parse("var(log) y; predetermined_variables y; model; y=0; end;"),
    );
    assert!(diff.symbols_changed.is_empty());
    assert!(diff.changed_equations.is_empty());
    let detail = row(&diff, SemanticFamily::Symbols, "y");
    assert!(field(detail, "log_transform").changed);
    assert!(field(detail, "predetermined").changed);
    assert_eq!(detail.pointer, "/semantic/rows/0");
    assert!(detail
        .facets
        .contains(&ChangeFacet::PredeterminedConvention));
}

#[test]
fn metadata_absence_empty_and_duplicate_selection_follow_legacy() {
    let diff = compare_models(&parse("var y;"), &parse("var y (long_name='');"));
    let detail = row(&diff, SemanticFamily::Symbols, "y");
    assert_eq!(field(detail, "long_name").before.state, ValueState::Absent);
    assert_eq!(field(detail, "long_name").after.state, ValueState::Empty);
    assert_eq!(detail.pointer, "/symbols_changed/0");

    let diff = compare_models(
        &parse("var y (long_name='First'); var y (long_name='Later');"),
        &parse("var y (long_name='First'); var y (long_name='Edited');"),
    );
    assert!(diff.symbols_changed.is_empty());
    assert!(diff.semantic.rows.is_empty());
}

#[test]
fn kind_dimension_and_added_parameter_calibration_are_retained() {
    let diff = compare_models(
        &parse("heterogeneity_dimension h; var(heterogeneity=h) y;"),
        &parse(
            "heterogeneity_dimension h; var(heterogeneity=h) y; change_type(parameters) y; y=0;",
        ),
    );
    let detail = row(&diff, SemanticFamily::Symbols, "y");
    assert!(field(detail, "kind").changed);
    assert!(field(detail, "dimension").changed);
    assert_eq!(
        detail.before.as_ref().unwrap().scope.dimension.as_deref(),
        Some("h")
    );
    assert_eq!(detail.after.as_ref().unwrap().scope.dimension, None);
    assert_eq!(
        field(detail, "evaluated_value").after,
        FieldState::number(Some(0.0))
    );

    let diff = compare_models(&parse(""), &parse("parameters p; p=0;"));
    let detail = row(&diff, SemanticFamily::Symbols, "p");
    assert_eq!(detail.pointer, "/added_parameters/0");
    assert_eq!(field(detail, "expression").after, FieldState::text("0"));
    assert_eq!(
        field(detail, "evaluated_value").after,
        FieldState::number(Some(0.0))
    );
}

#[test]
fn tag_fields_include_unchanged_flags_and_side_specific_names() {
    let diff = compare_models(
        &parse("var y; model; [name='Output', foo] y=1; end;"),
        &parse("var y; model; [name='Output', foo, bar='v'] y=1; end;"),
    );
    let detail = row(&diff, SemanticFamily::Equations, "Output");
    assert!(field(detail, "tags").changed);
    assert!(!field(detail, "expression").changed);
    let Some(FieldValue::Record(tags)) = &field(detail, "tags").after.value else {
        panic!("tag record");
    };
    assert_eq!(tags["foo"], FieldValue::Text(String::new()));
    assert_eq!(tags["bar"], FieldValue::Text("v".into()));
    assert_eq!(detail.pointer, "/changed_equations/0");
}

#[test]
fn shock_fields_preserve_period_value_order_and_exclude_locations() {
    let before = "varexo e; shocks; var e; periods 1 3:4; values 0 2; end;";
    let after = "varexo e; shocks(overwrite); var e; periods 1 3:4; values 0 3; end;";
    let old = parse(before);
    let new = parse(after);
    let diff = compare_models_with_sources(
        &old,
        &new,
        Some(CompareSource {
            text: before,
            origin_uri: Some("file:///before.mod"),
        }),
        Some(CompareSource {
            text: after,
            origin_uri: Some("file:///after.mod"),
        }),
    );
    let detail = diff
        .semantic
        .rows
        .iter()
        .find(|row| row.family == SemanticFamily::Shocks && field(row, "values").changed)
        .expect("shock values row");
    let values = field(detail, "values");
    assert_eq!(
        values.before.value,
        Some(FieldValue::List(vec![
            FieldValue::Text("0".into()),
            FieldValue::Text("2".into())
        ]))
    );
    assert_eq!(
        values.after.value,
        Some(FieldValue::List(vec![
            FieldValue::Text("0".into()),
            FieldValue::Text("3".into())
        ]))
    );
    assert!(!field(detail, "periods").changed);
    assert!(!detail.fields.iter().any(|field| [
        "location",
        "group_location",
        "origin_uri",
        "source_span",
        "occurrence_id"
    ]
    .contains(&field.name.as_str())));

    let same = compare_models_with_sources(
        &old,
        &old,
        Some(CompareSource {
            text: before,
            origin_uri: Some("file:///old.mod"),
        }),
        Some(CompareSource {
            text: before,
            origin_uri: Some("file:///new.mod"),
        }),
    );
    assert!(same.semantic.rows.is_empty());
}

#[test]
fn advertised_versions_and_pending_source_boundary_do_not_claim_completeness() {
    let diff = compare_models(&parse(""), &parse(""));
    let wire = diff.to_json();
    assert_eq!(wire["comparison_versions"]["semantic"], 1);
    assert_eq!(wire["semantic"]["schema_version"], 1);
    assert_eq!(wire["source_changes"]["schema_version"], 1);
    assert_eq!(wire["coverage"]["schema_version"], 1);
    assert_eq!(diff.source_changes.availability, Availability::NotAvailable);
    assert_eq!(diff.coverage.availability, Availability::Partial);
    assert_eq!(diff.coverage.source_boundary, "parsed_models_only");
}

#[test]
fn work_limits_accumulate_across_the_comparison_and_reject_overflow() {
    let mut semantic = SemanticDiff::new(ComparisonBudgets {
        token_alignment_cells: 6,
        source_alignment_cells: 4,
        ..ComparisonBudgets::default()
    });
    assert_eq!(
        semantic.charge_token_alignment(2, 2),
        Availability::Complete
    );
    assert_eq!(
        semantic.charge_token_alignment(1, 2),
        Availability::Complete
    );
    assert_eq!(
        semantic.charge_token_alignment(1, 1),
        Availability::LimitExceeded
    );
    assert_eq!(
        semantic.charge_source_alignment(2, 2),
        Availability::Complete
    );
    assert_eq!(
        semantic.charge_source_alignment(1, 1),
        Availability::LimitExceeded
    );
    assert_eq!(
        alignment_availability(usize::MAX, 2, usize::MAX),
        Availability::LimitExceeded
    );
}

fn reference(index: usize, side: Side) -> EquationReference {
    EquationReference {
        pointer: format!("/semantic/references/{index}"),
        symbol: "p".into(),
        side,
        equation_pointer: "/changed_equations/0".into(),
        equation_index: 0,
        label: "Output".into(),
        scope: ComparisonScope::aggregate(),
        occurrence: index,
        timing: TimingSide {
            name: "p".into(),
            class: "parameter".into(),
            written_offset: 0,
            converted_offset: 0,
            occurrence: index,
        },
        provenance: None,
    }
}

#[test]
fn reference_caps_reindex_pointers_and_keep_row_links_valid() {
    let mut diff = compare_models(&parse("parameters p; p=1;"), &parse("parameters p; p=2;"));
    diff.semantic.budgets.references_per_side = 1;
    diff.coverage.availability = Availability::Complete;
    diff.semantic.references = vec![
        reference(0, Side::Before),
        reference(1, Side::Before),
        reference(2, Side::After),
        reference(3, Side::After),
    ];
    diff.semantic.rows[0].references = (0..4)
        .map(|index| format!("/semantic/references/{index}"))
        .collect();
    enforce_output_budget(&mut diff);
    assert_eq!(diff.semantic.references.len(), 2);
    assert_eq!(diff.semantic.references[1].side, Side::After);
    assert_eq!(
        diff.semantic.references[1].pointer,
        "/semantic/references/1"
    );
    assert_eq!(
        diff.semantic.rows[0].references,
        vec!["/semantic/references/0", "/semantic/references/1"]
    );
    assert_eq!(diff.semantic.rows[0].limits[0].omitted, Some(2));
    assert_eq!(diff.coverage.availability, Availability::Partial);
    assert!(diff
        .coverage
        .limits
        .iter()
        .any(|limit| limit.code == "reference_limit" && limit.omitted == Some(2)));
}

#[test]
fn zero_detail_budget_keeps_legacy_rows_and_explicit_omission() {
    let diff = compare_models_with_budgets(
        &parse("parameters p; p=1;"),
        &parse("parameters p; p=2;"),
        None,
        None,
        ComparisonBudgets {
            serialized_output_bytes: 0,
            ..ComparisonBudgets::default()
        },
    );
    assert_eq!(diff.changed_parameter_values.len(), 1);
    assert!(diff.semantic.rows.is_empty());
    assert!(diff
        .semantic
        .limits
        .iter()
        .any(|limit| limit.code == "serialized_output_limit" && limit.omitted == Some(1)));
    assert_eq!(diff.semantic.availability, Availability::Partial);
}

#[test]
fn hunk_caps_keep_exact_captured_file_actions_and_count_omissions() {
    let mut diff = compare_models(&parse(""), &parse(""));
    diff.semantic.budgets.source_hunks = 1;
    let hunk = SourceHunk {
        before_start: 1,
        before_lines: 1,
        after_start: 1,
        after_lines: 1,
        lines: vec![SourceLine {
            role: TokenRole::Added,
            text: "// note".into(),
        }],
    };
    diff.source_changes.files = vec![SourceFileChange {
        pointer: "/source_changes/files/0".into(),
        change: ChangeKind::Changed,
        correspondence: SourceCorrespondence::SelectedRoots,
        before: Some(SourceFileSide {
            input_id: Some("before".into()),
            file_key: "root.mod".into(),
            exact_text_available: true,
        }),
        after: Some(SourceFileSide {
            input_id: Some("after".into()),
            file_key: "root.mod".into(),
            exact_text_available: true,
        }),
        availability: Availability::Complete,
        hunks: vec![hunk.clone(), hunk],
        omitted_hunks: None,
        limits: Vec::new(),
    }];
    enforce_output_budget(&mut diff);
    assert_eq!(diff.source_changes.files[0].hunks.len(), 1);
    assert_eq!(diff.source_changes.files[0].omitted_hunks, Some(1));
    assert!(
        diff.source_changes.files[0]
            .before
            .as_ref()
            .unwrap()
            .exact_text_available
    );
    assert_eq!(diff.source_changes.availability, Availability::Partial);
    diff.semantic.budgets.serialized_output_bytes = 0;
    enforce_output_budget(&mut diff);
    assert!(diff.source_changes.files[0].hunks.is_empty());
    assert_eq!(diff.source_changes.files[0].omitted_hunks, Some(2));
    assert!(
        diff.source_changes.files[0]
            .after
            .as_ref()
            .unwrap()
            .exact_text_available
    );
}

#[test]
fn all_existing_shock_forms_have_named_fields_at_their_legacy_pointer() {
    let cases = [
        ("varexo e; shocks; var e; stderr 1; end;", "varexo e; shocks; var e; stderr 2; end;"),
        ("varexo e; shocks(surprise); var e; periods 1; values 1; end;", "varexo e; shocks(surprise); var e; periods 1; values 2; end;"),
        ("varexo e; mshocks(relative_to_initval); var e; periods 1; values 1; end;", "varexo e; mshocks(relative_to_initval); var e; periods 1; values 2; end;"),
        ("varexo e; heteroskedastic_shocks; var e; periods 1; scales 1; end;", "varexo e; heteroskedastic_shocks; var e; periods 1; scales 2; end;"),
        ("var y; varexo e; shock_paths; var e; periods 1; values 1; end;", "var y; varexo e; shock_paths; var e; periods 1; values 2; end;"),
        ("var y; varexo e; endval(learnt_in=3); e=1; end;", "var y; varexo e; endval(learnt_in=3); e=2; end;"),
        ("varexo e; shock_groups; group=e; end;", "varexo e u; shock_groups; group=e,u; end;"),
        ("heterogeneity_dimension h; varexo(heterogeneity=h) e; shocks(heterogeneity=h); var e; stderr 1; end;", "heterogeneity_dimension h; varexo(heterogeneity=h) e; shocks(heterogeneity=h); var e; stderr 2; end;"),
    ];
    for (before, after) in cases {
        let diff = compare_models(&parse(before), &parse(after));
        assert!(
            !diff.shock_setup_changes.is_empty(),
            "accepted shock fixture: {after}"
        );
        for (index, legacy) in diff.shock_setup_changes.iter().enumerate() {
            let pointer = format!("/shock_setup_changes/{index}");
            let row = diff
                .semantic
                .rows
                .iter()
                .find(|row| row.pointer == pointer)
                .expect("one owned shock detail");
            assert_eq!(
                field(row, "form")
                    .after
                    .value
                    .as_ref()
                    .or(field(row, "form").before.value.as_ref()),
                Some(&FieldValue::Text(legacy.form.clone()))
            );
            assert!(row.fields.iter().any(|field| field.changed));
        }
    }
}

#[test]
fn shock_scope_is_separate_from_target_domain_classification() {
    for (before, after, name, scope, target_domain) in [
        ("varexo e; shocks; var e; stderr 1; end;", "varexo e; shocks; var e; stderr 2; end;", "e", "aggregate", "exogenous"),
        ("var y; varobs y; shocks; var y; stderr 1; end;", "var y; varobs y; shocks; var y; stderr 2; end;", "y", "aggregate", "measurement_error"),
        ("heterogeneity_dimension h; varexo(heterogeneity=h) e; shocks(heterogeneity=h); var e; stderr 1; end;", "heterogeneity_dimension h; varexo(heterogeneity=h) e; shocks(heterogeneity=h); var e; stderr 2; end;", "e", "heterogeneous", "exogenous"),
    ] {
        let diff = compare_models(&parse(before), &parse(after));
        let detail = row(&diff, SemanticFamily::Shocks, name);
        assert_eq!(detail.before.as_ref().unwrap().scope.domain, scope);
        assert_eq!(detail.after.as_ref().unwrap().scope.domain, scope);
        assert_eq!(field(detail, "domain").after, FieldState::text(target_domain));
    }
}

#[test]
fn parser_ids_spans_and_navigation_provenance_do_not_create_value_changes() {
    let before = parse("var y (long_name='Output'); parameters p; p=1;");
    let mut after = before.clone();
    after.intern.intern("unrelated_interned_name");
    after.endogenous[0].span = crate::span::Span { start: 1, end: 2 };
    after.endogenous[0].parse_order += 100;
    after.param_assignments[0].span = crate::span::Span { start: 3, end: 4 };
    let diff = compare_models(&before, &after);
    assert!(diff.semantic.rows.is_empty());
}

#[test]
fn actual_final_detail_json_stays_within_budget_after_reference_limit_metadata() {
    let mut diff = compare_models(&parse("parameters p; p=1;"), &parse("parameters p; p=2;"));
    diff.semantic.references = vec![reference(0, Side::Before), reference(1, Side::Before)];
    diff.semantic.rows[0].references = vec![
        "/semantic/references/0".into(),
        "/semantic/references/1".into(),
    ];
    diff.semantic.budgets.references_per_side = 0;
    diff.semantic.budgets.serialized_output_bytes =
        serde_json::to_vec(&diff.semantic.rows[0]).unwrap().len();
    enforce_output_budget(&mut diff);
    let actual = diff
        .semantic
        .rows
        .iter()
        .map(|row| serde_json::to_vec(row).unwrap().len())
        .sum::<usize>()
        + diff
            .semantic
            .references
            .iter()
            .map(|reference| serde_json::to_vec(reference).unwrap().len())
            .sum::<usize>()
        + diff
            .source_changes
            .files
            .iter()
            .flat_map(|file| &file.hunks)
            .map(|hunk| serde_json::to_vec(hunk).unwrap().len())
            .sum::<usize>();
    assert!(actual <= diff.semantic.budgets.serialized_output_bytes);
    assert!(diff
        .semantic
        .limits
        .iter()
        .any(|limit| limit.code == "reference_limit"));
}
