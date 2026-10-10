use dygnosis::model_diff::{compare_models_with_budgets, ModelDiff};
use dygnosis::semantic_diff::*;
use dygnosis::{compare_models, parse};

fn role<'a>(diff: &'a ModelDiff, name: &str) -> Vec<&'a SemanticRow> {
    diff.semantic.rows.iter().filter(|row|row.fields.iter().any(|field|field.name=="role" && [&field.before,&field.after].into_iter().any(|value|matches!(value.value.as_ref(),Some(FieldValue::Text(role)) if role==name)))).collect()
}
fn commands(diff: &ModelDiff) -> Vec<&SemanticRow> {
    diff.semantic
        .rows
        .iter()
        .filter(|row| row.family == SemanticFamily::Commands)
        .collect()
}
fn compare(a: &str, b: &str) -> ModelDiff {
    compare_models(&parse(a), &parse(b))
}

#[test]
fn added_predetermined_symbol_keeps_one_owning_symbol_row() {
    let diff = compare(
        "var y; model; y=1; end;",
        "var y xxx; predetermined_variables xxx; model; y=xxx; end;",
    );
    let rows: Vec<_> = diff
        .semantic
        .rows
        .iter()
        .filter(|row| row.family == SemanticFamily::Symbols && row.name == "xxx")
        .collect();
    assert_eq!(
        rows.len(),
        1,
        "predetermined declaration duplicated the primary symbol: {rows:?}"
    );
    assert!(rows[0]
        .fields
        .iter()
        .any(|field| field.name == "predetermined" && field.after == FieldState::boolean(true)));
    assert!(!rows[0].references.is_empty());
    assert!(commands(&diff).is_empty());
}

#[test]
fn shock_instruction_edits_do_not_duplicate_command_changes() {
    let before = "varexo e u; shocks; var e=.1; var u=.2; end;";
    for after in [
        "varexo e u; shocks; var e=.3; var u=.2; end;",
        "varexo e u; shocks; var e=.1; end;",
    ] {
        let diff = compare(before, after);
        assert_eq!(diff.shock_setup_changes.len(), 1);
        assert!(
            commands(&diff).is_empty(),
            "shock edit also appears in Commands: {:?}",
            commands(&diff)
        );
    }
}

#[test]
fn added_local_definition_does_not_change_model_separators() {
    let diff = compare(
        "var y; model; y=1; end;",
        "var y; model; # helper=2; y=1; end;",
    );
    assert_eq!(role(&diff, "model_local_definition").len(), 1);
    assert!(
        commands(&diff).is_empty(),
        "local insertion left a redundant model row: {:?}",
        commands(&diff)
    );
    assert!(diff.added_equations.is_empty());
    assert!(diff.changed_equations.is_empty());
}

#[test]
fn shock_cards_retain_each_instruction_and_its_block_options() {
    let before = "varexo e u; shocks; var e; stderr 1; var u; stderr 3; end;";
    let after = "varexo e u; shocks(overwrite); var e; stderr 2; var u; stderr 3; end;";
    let diff = compare(before, after);
    let row = diff
        .semantic
        .rows
        .iter()
        .find(|row| row.family == SemanticFamily::Shocks && row.name == "e")
        .unwrap();
    let text = row
        .expressions
        .iter()
        .find(|expression| expression.field == "statement_text")
        .expect("shock statement text");
    assert_eq!(
        text.before.as_ref().unwrap().text,
        "shocks;\nvar e; stderr 1;\nend;"
    );
    assert_eq!(
        text.after.as_ref().unwrap().text,
        "shocks(overwrite);\nvar e; stderr 2;\nend;"
    );
    assert!(!text.after.as_ref().unwrap().text.contains("var u"));
    assert!(text
        .after
        .as_ref()
        .unwrap()
        .runs
        .iter()
        .any(|run| run.role == TokenRole::Added && run.text.contains('2')));
    assert!(text
        .after
        .as_ref()
        .unwrap()
        .runs
        .iter()
        .any(|run| run.role == TokenRole::Added && run.text.contains("overwrite")));
}

#[test]
fn local_only_edit_has_one_owner_and_direct_unchanged_equation_references() {
    let before = "var y; model; # a=1; [name='Output'] y=a+a(-1); end;";
    let diff = compare(before, &before.replace("# a=1", "# a=2"));
    assert!(diff.changed_equations.is_empty());
    let rows = role(&diff, "model_local_definition");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].family, SemanticFamily::Symbols);
    assert_eq!(rows[0].change, ChangeKind::Changed);
    assert_eq!(rows[0].references.len(), 4);
    assert!(commands(&diff).is_empty());
    for reference in &diff.semantic.references {
        assert_eq!(reference.symbol, "a");
        assert!(reference
            .provenance
            .as_ref()
            .unwrap()
            .statement_id
            .is_some());
    }
}

#[test]
fn explicit_local_metadata_and_forward_read_keep_declaration_identity() {
    let before = "model_local_variable a $A$; var y; model; [name='Output'] y=a; # a=1; end;";
    let diff = compare(before, &before.replace("$A$", "$B$"));
    let rows = role(&diff, "model_local_declaration");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].references.len(), 2);
    assert!(rows[0].facets.contains(&ChangeFacet::Label));
    assert!(commands(&diff).is_empty());
}

#[test]
fn named_static_alternative_keeps_counted_arrays_unchanged() {
    let before = "var y; model; [name='Dynamic',dynamic] y=y(-1); [name='Static',static] y=1; end;";
    let diff = compare(before, &before.replace("y=1", "y=2"));
    assert!(diff.changed_equations.is_empty());
    let rows = role(&diff, "static_alternative");
    assert_eq!(rows.len(), 1);
    assert!(rows[0].after.as_ref().unwrap().equation_index.is_none());
    assert!(rows[0]
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
fn ordered_steady_outputs_and_temporary_roles_remain_one_assignment() {
    let before = "var y z; steady_state_model; t=1; [y,z]=myfun(t); end;";
    let diff = compare(before, &before.replace("myfun(t)", "myfun(t+1)"));
    let rows = role(&diff, "steady_state_assignment");
    assert_eq!(rows.len(), 1);
    let targets = rows[0]
        .fields
        .iter()
        .find(|field| field.name == "targets")
        .unwrap();
    assert_eq!(
        targets.after.value,
        Some(FieldValue::List(vec![
            FieldValue::Text("y".into()),
            FieldValue::Text("z".into())
        ]))
    );
    assert!(commands(&diff).is_empty());
    let temporary = compare(before, &before.replace("t=1", "t=2"));
    assert_eq!(role(&temporary, "steady_state_assignment").len(), 1);
}

#[test]
fn state_and_history_fields_compare_with_proven_parent() {
    for (before, after, expected) in [
        (
            "var y; initval; y=1; end;",
            "var y; initval; y=2; end;",
            "initval",
        ),
        (
            "var y; endval; y=1; end;",
            "var y; endval; y=2; end;",
            "endval",
        ),
        (
            "var y; histval; y(-1)=1; end;",
            "var y; histval; y(-1)=2; end;",
            "histval_assignment",
        ),
    ] {
        let diff = compare(before, after);
        let rows = role(&diff, expected);
        assert_eq!(rows.len(), 1, "{expected}");
        assert_eq!(rows[0].change, ChangeKind::Changed);
        assert!(rows[0]
            .before
            .as_ref()
            .unwrap()
            .provenance
            .as_ref()
            .unwrap()
            .statement_id
            .is_some());
        assert!(commands(&diff).is_empty());
    }
    let diff = compare(
        "var y; histval; y(-1)=1; end;",
        "var y; histval; y(-2)=1; end;",
    );
    let rows = role(&diff, "histval_assignment");
    assert_eq!(rows.len(), 2);
    assert!(rows
        .iter()
        .all(|row| row.change == ChangeKind::Added || row.change == ChangeKind::Removed));
}

#[test]
fn command_options_and_block_flags_use_complete_accepted_ranges() {
    let diff = compare(
        "var y; stoch_simul(order=1) y;",
        "var y; stoch_simul(order=2) y;",
    );
    let rows = commands(&diff);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].change, ChangeKind::Changed);
    assert!(rows[0].expressions.iter().any(|detail| detail
        .after
        .as_ref()
        .unwrap()
        .runs
        .iter()
        .any(|run| run.role == TokenRole::Added)));
    let diff = compare("var y; model; y=0; end;", "var y; model(linear); y=0; end;");
    assert!(commands(&diff).is_empty());
    let owner = diff
        .semantic
        .rows
        .iter()
        .find(|row| row.fields.iter().any(|field| field.name == "is_linear"))
        .expect("linear field owner");
    assert!(owner
        .fields
        .iter()
        .any(|field| field.name == "is_linear" && field.changed));
}

#[test]
fn native_and_recovered_commands_stay_out_of_semantic_context() {
    let diff = compare(
        "foo = magic(2); plot(foo);",
        "foo = magic(3); plot(foo,'x');",
    );
    assert!(commands(&diff).is_empty());
    assert!(role(&diff, "helper_assignment").is_empty());
    let diff = compare("model = 1;", "model = 2;");
    assert!(commands(&diff).is_empty());
}

#[test]
fn repeated_changed_commands_cancel_equal_controls_without_pairing() {
    let before = "var y; stoch_simul(order=1) y; stoch_simul(order=1) y;";
    let after = "var y; stoch_simul(order=1) y; stoch_simul(order=2) y;";
    let diff = compare(before, after);
    let rows = commands(&diff);
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.change
        == if row.before.is_some() {
            ChangeKind::Removed
        } else {
            ChangeKind::Added
        }));
}

#[test]
fn repeated_state_target_keeps_unchanged_control_out_of_rows() {
    let diff = compare(
        "var y z; initval; y=1; y=2; z=7; end;",
        "var y z; initval; y=1; y=3; z=7; end;",
    );
    let rows = role(&diff, "initval");
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.change
        == if row.before.is_some() {
            ChangeKind::Removed
        } else {
            ChangeKind::Added
        }));
    assert!(rows.iter().all(|row| row.name == "y"));
}

#[test]
fn prior_nonfinal_calibration_writes_remain_operations() {
    let diff = compare("parameters a; a=1; a=3;", "parameters a; a=2; a=3;");
    assert!(diff.changed_parameter_values.is_empty());
    let rows = role(&diff, "parameter_assignment_history");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].count_unit, CountUnit::Operation);
}

#[test]
fn local_references_share_comparison_wide_cap_and_exact_fallback() {
    let before = "var y; model; # a=1; [name='Output'] y=a+a; end;";
    let diff = compare_models_with_budgets(
        &parse(before),
        &parse(&before.replace("a=1", "a=2")),
        None,
        None,
        ComparisonBudgets {
            references_per_side: 1,
            token_alignment_cells: 0,
            ..Default::default()
        },
    );
    assert_eq!(diff.semantic.references.len(), 2);
    let rows = role(&diff, "model_local_definition");
    assert_eq!(rows.len(), 1);
    assert!(rows[0]
        .limits
        .iter()
        .any(|limit| limit.code == "references_partial"));
    assert_eq!(
        rows[0].expressions[0].availability,
        Availability::LimitExceeded
    );
    for detail in &rows[0].expressions {
        for side in detail.before.iter().chain(&detail.after) {
            assert_eq!(
                side.text,
                side.runs
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<String>()
            );
        }
    }
}

#[test]
fn many_local_bindings_keep_exact_reference_omissions_after_indexing() {
    let source = |suffix: &str| {
        format!(
        "var y;\nmodel;\n@#for i in 1:128\n# lc@{{i}} = @{{i}}{suffix};\n@#endfor\n@#for i in 1:128\ny = lc@{{i}} + lc@{{i}} + lc@{{i}} + lc@{{i}};\n@#endfor\nend;\n"
    )
    };
    let diff = compare_models_with_budgets(
        &parse(&source("")),
        &parse(&source("+1")),
        None,
        None,
        ComparisonBudgets {
            references_per_side: 32,
            ..ComparisonBudgets::default()
        },
    );
    assert_eq!(role(&diff, "model_local_definition").len(), 128);
    assert_eq!(diff.semantic.references.len(), 64);
    assert!(diff
        .semantic
        .references
        .iter()
        .all(|reference| reference.occurrence < 4));
    let limit = diff
        .semantic
        .limits
        .iter()
        .find(|limit| limit.code == "reference_limit" && limit.owner == "semantic_surfaces")
        .unwrap();
    assert_eq!(limit.omitted, Some(960));
    assert!(diff.changed_equations.is_empty());
}

#[test]
fn selected_shock_baseline_assignment_has_one_existing_owner() {
    let before="var y; varexo e; model; y=e; end; initval; e=1; end; shock_paths; var e; periods 1; values initval.e; end;";
    let diff = compare(before, &before.replace("e=1", "e=2"));
    assert!(!diff.shock_setup_changes.is_empty());
    assert!(role(&diff, "initval").is_empty());
    let owned: Vec<_> = diff
        .semantic
        .rows
        .iter()
        .filter(|row| row.family == SemanticFamily::Shocks && row.name == "e")
        .collect();
    assert_eq!(owned.len(), 1);
    let side = owned[0].after.as_ref().unwrap();
    assert_eq!(side.context.as_ref().unwrap().name, "initval");
    assert!(side.provenance.as_ref().unwrap().statement_id.is_some());
}

#[test]
fn relative_order_is_separate_from_insertion_shifts_and_repeated_pairing() {
    let diff = compare(
        "var y z; initval; y=1; z=2; end;",
        "var y z; initval; z=2; y=1; end;",
    );
    let rows = role(&diff, "initval");
    assert_eq!(rows.len(), 2);
    assert!(rows
        .iter()
        .all(|row| row.facets == vec![ChangeFacet::Order]));
    assert_eq!(rows[0].name, "z");
    let diff = compare(
        "var y z; initval; y=1; z=2; end;",
        "var y z; initval; y=0; y=1; z=2; end;",
    );
    assert!(role(&diff, "initval")
        .iter()
        .all(|row| !row.facets.contains(&ChangeFacet::Order)));
    let diff = compare("var y; steady; check;", "var y; check; steady;");
    assert_eq!(commands(&diff).len(), 2);
    assert!(commands(&diff)
        .iter()
        .all(|row| row.facets == vec![ChangeFacet::Order]));
    let diff = compare(
        "var y; initval; y=1; y=2; end;",
        "var y; initval; y=2; y=1; end;",
    );
    let rows = role(&diff, "initval");
    assert_eq!(rows.len(), 4);
    assert!(rows.iter().all(|row| row.change
        == if row.before.is_some() {
            ChangeKind::Removed
        } else {
            ChangeKind::Added
        }
        && row.facets.contains(&ChangeFacet::Order)));
}

#[test]
fn aggregate_and_heterogeneous_locals_keep_separate_direct_bindings() {
    let before="heterogeneity_dimension h; var y; var(heterogeneity=h) yh; model_local_variable a; model; #a=1; y=a; end; model(heterogeneity=h); #a=2; yh=a(-1); end;";
    let diff = compare(before, &before.replace("#a=2", "#a=3"));
    let rows = role(&diff, "model_local_definition");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].after.as_ref().unwrap().scope.dimension.as_deref(),
        Some("h")
    );
    assert_eq!(rows[0].references.len(), 2);
    assert!(diff
        .semantic
        .references
        .iter()
        .all(
            |reference| reference.scope.dimension.as_deref() == Some("h")
                && reference.timing.written_offset == -1
        ));
}

#[test]
fn shared_written_spans_do_not_pair_changed_macro_occurrences() {
    let before = "@#for i in 1:2\nstoch_simul(order=1);\n@#endfor\n";
    let diff = compare(before, &before.replace("order=1", "order=2"));
    let rows = commands(&diff);
    assert_eq!(rows.len(), 4);
    assert!(rows.iter().all(|row| row.change
        == if row.before.is_some() {
            ChangeKind::Removed
        } else {
            ChangeKind::Added
        }));
    let orders: std::collections::BTreeSet<_> = rows
        .iter()
        .filter_map(|row| row.after.as_ref())
        .map(|side| side.provenance.as_ref().unwrap().parse_order.unwrap())
        .collect();
    assert_eq!(orders.len(), 2);
}

#[test]
fn unretained_block_body_has_context_beside_owned_fields() {
    let before = "var y; parameters a b; observation_trends; y(a); end;";
    let diff = compare(before, &before.replace("y(a)", "y(b)"));
    assert_eq!(commands(&diff).len(), 1);
    assert_eq!(commands(&diff)[0].name, "observation_trends");
    let before = "var y; model; [name='Output'] y=1; end;";
    let diff = compare(before, &before.replace("y=1", "y=2"));
    assert!(commands(&diff).is_empty());
}

#[test]
fn histval_execution_link_survives_recovery_records_before_its_parent() {
    let before = "var y; histval=0; histval; y(-1)=1; end;";
    let diff = compare(before, &before.replace("y(-1)=1", "y(-1)=2"));
    let rows = role(&diff, "histval_assignment");
    assert_eq!(rows.len(), 1);
    let model = parse(before);
    let proof = rows[0]
        .before
        .as_ref()
        .unwrap()
        .provenance
        .as_ref()
        .unwrap();
    let parent = &model.statements[proof.statement_id.unwrap()];
    assert_eq!(parent.name, "histval");
    assert!(parent.complete);
    assert!(parent.token_range.contains(&proof.parse_order.unwrap()));
    let before = "var y; histval; y(-1)=1; y(-2) 4; end;";
    let diff = compare(before, &before.replace("y(-1)=1", "y(-1)=2"));
    let rows = role(&diff, "histval_assignment");
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.change
        == if row.before.is_some() {
            ChangeKind::Removed
        } else {
            ChangeKind::Added
        }));
    assert!(rows.iter().all(|row| row.name == "y(-1)"));
}

#[test]
fn histval_retention_preserves_diagnostic_control() {
    let before = "var y; model; y=y(-1); end; histval; y(1)=0; end;";
    let after = before.replace("y(1)=0", "y(1)=1");
    let a = dygnosis::analyze(&parse(before));
    let b = dygnosis::analyze(&parse(&after));
    let codes = |diagnostics: Vec<dygnosis::Diagnostic>| {
        diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>()
    };
    assert_eq!(codes(a), codes(b));
    assert!(dygnosis::analyze(&parse(before))
        .iter()
        .any(|diagnostic| diagnostic.code == "E242"));
}
