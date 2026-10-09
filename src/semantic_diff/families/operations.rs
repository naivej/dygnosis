use super::*;

pub(super) fn collect(
    model: &Model,
    facts: &mut Vec<CapturedFact>,
    work: &mut RetainedExpressionWork,
) {
    let mut owned_type_results = BTreeSet::new();
    for (index, row) in model.epilogue.iter().filter(|row| !row.native).enumerate() {
        let name = model.name(row.name);
        let mut value = fact(
            model,
            SemanticFamily::Operations,
            "epilogue_assignment",
            name,
            vec![name.into()],
            (!row.active_tokens.is_empty()).then_some(row.active_tokens.start),
            index,
        );
        value.count_unit = CountUnit::Operation;
        text(&mut value, "target", name, ChangeFacet::Target);
        expr(&mut value, "expression", FieldState::text(&row.expression));
        value.claims.push(row.active_tokens.clone());
        facts.push(value);
    }
    for (index, row) in model.filter_initial_state.iter().enumerate() {
        if !row.accepted_assignment {
            continue;
        }
        let name = model.name(row.name);
        let mut value = fact(
            model,
            SemanticFamily::Operations,
            "filter_initial_state",
            name,
            vec![name.into()],
            (!row.active_tokens.is_empty()).then_some(row.active_tokens.start),
            index,
        );
        text(&mut value, "target", name, ChangeFacet::Target);
        field(
            &mut value,
            "lag",
            FieldState::present(FieldValue::Integer(row.lag.into())),
            ChangeFacet::Timing,
        );
        field(
            &mut value,
            "captured_kind",
            FieldState::optional_text(
                model.symbol_kind_in_context(row.name, row.symbol_type_context),
            ),
            ChangeFacet::SymbolKind,
        );
        let expression = if parent_id(&value).is_some() {
            row.active_tokens
                .clone()
                .find(|&index| model.expanded_tokens[index].kind == crate::lexer::TokenKind::Eq)
                .map(|equal| {
                    super::super::expression_values::range_text(
                        model,
                        &mut value,
                        "expression",
                        equal + 1..row.active_tokens.end - 1,
                        work,
                    )
                })
                .unwrap_or_else(FieldState::unknown)
        } else {
            expression(model, &mut value, "expression", row.expr, work)
        };
        expr(&mut value, "expression", expression);
        value.claims.push(row.active_tokens.clone());
        facts.push(value);
    }
    for (index, row) in model.homotopy_rows.iter().enumerate() {
        let name = model.name(row.name);
        let mut value = fact(
            model,
            SemanticFamily::Operations,
            "homotopy_setup",
            name,
            vec![name.into()],
            None,
            index,
        );
        text(&mut value, "target", name, ChangeFacet::Target);
        field(
            &mut value,
            "captured_kind",
            FieldState::optional_text(
                model.symbol_kind_in_context(row.name, row.symbol_type_context),
            ),
            ChangeFacet::SymbolKind,
        );
        field(
            &mut value,
            "initial",
            FieldState::unknown(),
            ChangeFacet::Expression,
        );
        field(
            &mut value,
            "final",
            FieldState::unknown(),
            ChangeFacet::Expression,
        );
        gap(&mut value, "homotopy_endpoints_not_retained", "The target is retained but initial/final expressions are discarded by the parser. Endpoint edits remain accepted statement context and Source.", "parser_homotopy_retention");
        facts.push(value);
    }
    for (index, row) in model.change_type_statements.iter().enumerate() {
        let mut value = fact(
            model,
            SemanticFamily::Operations,
            "change_type",
            "change_type",
            vec![],
            Some(row.parse_order),
            index,
        );
        value.count_unit = CountUnit::Operation;
        text(
            &mut value,
            "new_type",
            match row.new_type {
                ChangeTypeKind::Parameters => "parameters",
                ChangeTypeKind::Var => "var",
                ChangeTypeKind::Varexo => "varexo",
                ChangeTypeKind::VarexoDet => "varexo_det",
            },
            ChangeFacet::SymbolKind,
        );
        field(
            &mut value,
            "targets",
            FieldState::present(names(model, row.names.iter().map(|(name, _)| *name))),
            ChangeFacet::Target,
        );
        full_claim(model, &mut value);
        facts.push(value);
    }
    for (index, row) in model.var_removed.iter().enumerate() {
        let name = model.name(row.name);
        let mut value = fact(
            model,
            SemanticFamily::Operations,
            "var_remove",
            name,
            vec![name.into()],
            Some(row.parse_order),
            index,
        );
        value.count_unit = CountUnit::Operation;
        text(&mut value, "target", name, ChangeFacet::Target);
        text(
            &mut value,
            "resulting_kind",
            "excluded",
            ChangeFacet::SymbolKind,
        );
        if let Some(event) = model
            .symbol_type_events
            .get(row.symbol_type_context.index())
            .filter(|event| {
                event.changed && event.name == row.name && event.kind == SymbolKind::Excluded
            })
        {
            field(
                &mut value,
                "successful_result",
                FieldState::present(type_result(model, event)),
                ChangeFacet::Operation,
            );
            owned_type_results.insert(row.symbol_type_context.index());
        }
        facts.push(value);
    }
    for (index, row) in model.equation_surgery.iter().enumerate() {
        let mut value = fact(
            model,
            SemanticFamily::Operations,
            "equation_surgery",
            if row.replace {
                "model_replace"
            } else {
                "model_remove"
            },
            vec![],
            Some(row.parse_order),
            index,
        );
        value.count_unit = CountUnit::Operation;
        value.claims.push(row.opener_tokens.clone());
        field(
            &mut value,
            "replace",
            FieldState::boolean(row.replace),
            ChangeFacet::Operation,
        );
        field(
            &mut value,
            "tag_sets",
            FieldState::present(FieldValue::List(
                row.tag_sets
                    .iter()
                    .map(|set| {
                        FieldValue::List(
                            set.iter()
                                .map(|(key, value)| {
                                    record([
                                        ("key", FieldValue::Text(key.clone())),
                                        ("value", FieldValue::Text(value.clone())),
                                    ])
                                })
                                .collect(),
                        )
                    })
                    .collect(),
            )),
            ChangeFacet::Tags,
        );
        field(
            &mut value,
            "removed_equations",
            FieldState::present(FieldValue::List(
                row.removed
                    .iter()
                    .map(|removed| {
                        let mut entries = BTreeMap::new();
                        entries.insert(
                            "expression".into(),
                            FieldValue::Text(removed.equation.text.clone()),
                        );
                        entries.insert(
                            "name".into(),
                            FieldValue::Text(removed.equation.name.clone()),
                        );
                        entries.insert(
                            "tag_map".into(),
                            FieldValue::Record(
                                removed
                                    .equation
                                    .tag_map
                                    .iter()
                                    .map(|(key, value)| {
                                        (key.clone(), FieldValue::Text(value.clone()))
                                    })
                                    .collect(),
                            ),
                        );
                        entries.insert(
                            "static_tag".into(),
                            FieldValue::Boolean(removed.equation.static_tag),
                        );
                        entries.insert(
                            "dynamic_tag".into(),
                            FieldValue::Boolean(removed.equation.dynamic_tag),
                        );
                        entries.insert(
                            "complementarity".into(),
                            state_value(
                                removed
                                    .equation
                                    .complementarity
                                    .as_ref()
                                    .map(|condition| {
                                        FieldState::present(record([
                                            ("text", FieldValue::Text(condition.text.clone())),
                                            (
                                                "matched",
                                                state_value(
                                                    condition
                                                        .matched
                                                        .as_ref()
                                                        .map(|matched| {
                                                            FieldState::present(record([
                                                                (
                                                                    "variable",
                                                                    FieldValue::Text(
                                                                        matched.variable.clone(),
                                                                    ),
                                                                ),
                                                                (
                                                                    "lower_bound",
                                                                    state_value(
                                                                        FieldState::optional_text(
                                                                            matched
                                                                                .lower_bound
                                                                                .as_deref(),
                                                                        ),
                                                                    ),
                                                                ),
                                                                (
                                                                    "upper_bound",
                                                                    state_value(
                                                                        FieldState::optional_text(
                                                                            matched
                                                                                .upper_bound
                                                                                .as_deref(),
                                                                        ),
                                                                    ),
                                                                ),
                                                            ]))
                                                        })
                                                        .unwrap_or_else(FieldState::unknown),
                                                ),
                                            ),
                                        ]))
                                    })
                                    .unwrap_or_else(FieldState::absent),
                            ),
                        );
                        if let Some(name) = &removed.endogenous {
                            entries.insert("endogenous".into(), FieldValue::Text(name.clone()));
                        }
                        FieldValue::Record(entries)
                    })
                    .collect(),
            )),
            ChangeFacet::Operation,
        );
        field(
            &mut value,
            "unmatched_tag_sets",
            FieldState::present(FieldValue::List(
                row.unmatched
                    .iter()
                    .map(|set| {
                        FieldValue::List(
                            set.iter()
                                .map(|(key, value)| {
                                    record([
                                        ("key", FieldValue::Text(key.clone())),
                                        ("value", FieldValue::Text(value.clone())),
                                    ])
                                })
                                .collect(),
                        )
                    })
                    .collect(),
            )),
            ChangeFacet::Operation,
        );
        facts.push(value);
    }
    for (index, row) in model.pruned_initializations.iter().enumerate() {
        let order = model
            .type_event_occurrences
            .iter()
            .find(|(event, _)| *event == row.removal_event)
            .map(|(_, range)| range.start);
        let name = model.name(row.name);
        let mut value = fact(
            model,
            SemanticFamily::Operations,
            "pruned_initialization",
            name,
            vec![name.into(), row.block.into()],
            order,
            index,
        );
        value.count_unit = CountUnit::Operation;
        text(&mut value, "target", name, ChangeFacet::Target);
        text(&mut value, "block", row.block, ChangeFacet::Role);
        text(
            &mut value,
            "removal_kind",
            row.removal_kind,
            ChangeFacet::Operation,
        );
        field(
            &mut value,
            "expression",
            FieldState::unknown(),
            ChangeFacet::Expression,
        );
        gap(&mut value, "pruned_initialization_rhs_not_retained", "The pruned initialization retains target, block and removal operation identity, but not its former RHS. The removed assignment is not a live initialization fact.", "parser_pruned_initialization_retention");
        facts.push(value);
    }
    let results: Vec<_> = model
        .symbol_type_events
        .iter()
        .enumerate()
        .filter(|(index, event)| event.changed && !owned_type_results.contains(index))
        .map(|(_, event)| type_result(model, event))
        .collect();
    let mut history = fact(
        model,
        SemanticFamily::Operations,
        "successful_type_history",
        "Successful type change history",
        vec![],
        None,
        0,
    );
    history.count_unit = CountUnit::Operation;
    field(
        &mut history,
        "successful_type_results",
        FieldState::present(FieldValue::List(results)),
        ChangeFacet::Operation,
    );
    field(
        &mut history,
        "surgery_exits",
        FieldState::present(FieldValue::List(
            model
                .surgery_exits
                .iter()
                .map(|exit| {
                    record([
                        ("target", FieldValue::Text(model.name(exit.name).into())),
                        (
                            "exit_kind",
                            FieldValue::Text(
                                match exit.kind {
                                    SurgeryKind::Exogenous => "exogenous",
                                    SurgeryKind::Dropped => "dropped",
                                }
                                .into(),
                            ),
                        ),
                    ])
                })
                .collect(),
        )),
        ChangeFacet::Operation,
    );
    gap(&mut history, "type_result_history_association", "This ordered success-event history excludes results directly owned by var_remove. Other results have no direct retained operation link, so the history is one supporting operation fact, not one extra counted operation per event. It does not infer event-to-surgery or event-to-change_type correspondence.", "parser_type_result_occurrence_retention");
    facts.push(history);
    macros(model, facts);
    markers(model, facts);
}

fn type_result(model: &Model, event: &SymbolTypeEvent) -> FieldValue {
    record([
        ("target", FieldValue::Text(model.name(event.name).into())),
        ("kind", FieldValue::Text(event.kind.as_str().into())),
        ("change_operation", FieldValue::Boolean(event.changed)),
    ])
}

fn macros(model: &Model, facts: &mut Vec<CapturedFact>) {
    // One retained written-context sequence is a semantic identity at the
    // captured root. It does not pair expanded macro copies or included files.
    let mut value = CapturedFact::new(
        SemanticFamily::MacroContext,
        "retained_written_macro_context",
        vec![],
        RowSide::named("Written macro context", ComparisonScope::global()),
    );
    value.count_unit = CountUnit::FinalFact;
    field(
        &mut value,
        "literal_includes",
        FieldState::present(FieldValue::List(
            model
                .includes
                .iter()
                .map(|row| FieldValue::Text(row.filename.clone()))
                .collect(),
        )),
        ChangeFacet::Options,
    );
    field(
        &mut value,
        "include_paths",
        FieldState::present(FieldValue::List(
            model
                .includepaths
                .iter()
                .map(|row| FieldValue::Text(row.argument.clone()))
                .collect(),
        )),
        ChangeFacet::Options,
    );
    field(
        &mut value,
        "directives",
        FieldState::present(FieldValue::List(
            model
                .macro_directives
                .iter()
                .map(|row| {
                    let mut entries = BTreeMap::new();
                    entries.insert("kind".into(), FieldValue::Text(row.kind.clone()));
                    entries.insert(
                        "has_argument".into(),
                        FieldValue::Boolean(row.argument.is_some()),
                    );
                    if let Some(argument) = &row.argument {
                        entries.insert("argument".into(), FieldValue::Text(argument.clone()));
                    }
                    FieldValue::Record(entries)
                })
                .collect(),
        )),
        ChangeFacet::Options,
    );
    field(
        &mut value,
        "interpolations",
        FieldState::present(FieldValue::List(
            model
                .macro_interps
                .iter()
                .map(|row| FieldValue::Text(row.inner.clone()))
                .collect(),
        )),
        ChangeFacet::Expression,
    );
    gap(&mut value, "macro_context_written_boundary", "These pre-expansion records describe retained written root context. They do not prove executed branches, include variable expressions, included-file history, or expanded occurrence correspondence.", "parser_macro_context_retention");
    facts.push(value);
}

fn markers(model: &Model, facts: &mut Vec<CapturedFact>) {
    let markers = [
        (SemanticFamily::Operations, "is_linear", model.is_linear),
        (
            SemanticFamily::Operations,
            "differentiate_forward_vars",
            model.differentiate_forward_vars,
        ),
        (
            SemanticFamily::Operations,
            "partial_information",
            model.partial_information,
        ),
        (
            SemanticFamily::Operations,
            "model_block_option",
            model.model_block_option.is_some(),
        ),
        (
            SemanticFamily::Operations,
            "use_dll",
            model.use_dll_span.is_some(),
        ),
        (
            SemanticFamily::Operations,
            "no_static",
            model.no_static_span.is_some(),
        ),
        (
            SemanticFamily::Operations,
            "extended_path_has_periods",
            model.extended_path_has_periods,
        ),
        (
            SemanticFamily::Operations,
            "initval_all_values_required",
            model.initval_all_values_required,
        ),
        (
            SemanticFamily::Operations,
            "endval_all_values_required",
            model.endval_all_values_required,
        ),
        (
            SemanticFamily::Operations,
            "histval_all_values_required",
            model.histval_all_values_required,
        ),
        (
            SemanticFamily::Operations,
            "with_epilogue",
            model.with_epilogue_span.is_some(),
        ),
        (
            SemanticFamily::Policy,
            "discretionary_has_instruments_option",
            model.discretionary_has_instruments_option,
        ),
        (
            SemanticFamily::Policy,
            "has_optim_weights",
            model.has_optim_weights,
        ),
        (
            SemanticFamily::Data,
            "dsge_prior_weight_parameter",
            model.dsge_prior_weight_param.is_some(),
        ),
        (
            SemanticFamily::Data,
            "dsge_var_estimated",
            model.dsge_var_estimated.is_some(),
        ),
        (
            SemanticFamily::Data,
            "dsge_var_calibrated",
            model.dsge_var_calibrated.is_some(),
        ),
        (
            SemanticFamily::Data,
            "dsge_varlag",
            model.dsge_varlag_span.is_some(),
        ),
        (
            SemanticFamily::Data,
            "bayesian_irf",
            model.bayesian_irf_span.is_some(),
        ),
        (
            SemanticFamily::Data,
            "estimation_datafile",
            model.estimation_datafile_span.is_some(),
        ),
        (
            SemanticFamily::Data,
            "estimation_dataseries",
            model.estimation_dataseries_span.is_some(),
        ),
        (
            SemanticFamily::Data,
            "estimation_mode_file",
            model.estimation_mode_file_span.is_some(),
        ),
        (
            SemanticFamily::Data,
            "mh_tune_jscale",
            model.mh_tune_jscale_span.is_some(),
        ),
        (
            SemanticFamily::Data,
            "mh_jscale",
            model.mh_jscale_span.is_some(),
        ),
        (
            SemanticFamily::Data,
            "mh_tune_guess",
            model.mh_tune_guess_span.is_some(),
        ),
        (
            SemanticFamily::Data,
            "filter_algorithm_gmf",
            model.filter_algorithm_gmf_span.is_some(),
        ),
        (
            SemanticFamily::Data,
            "proposal_approximation_montecarlo",
            model.proposal_approximation_montecarlo_span.is_some(),
        ),
        (
            SemanticFamily::Data,
            "distribution_approximation_montecarlo",
            model.distribution_approximation_montecarlo_span.is_some(),
        ),
        (
            SemanticFamily::Operations,
            "sensitivity_identification_eq_1",
            model.sensitivity_identification_eq_1.is_some(),
        ),
        (
            SemanticFamily::Operations,
            "stoch_simul_hp_filter",
            model.stoch_simul_hp_filter.is_some(),
        ),
        (
            SemanticFamily::Operations,
            "stoch_simul_one_sided_hp_filter",
            model.stoch_simul_one_sided_hp_filter.is_some(),
        ),
        (
            SemanticFamily::Operations,
            "stoch_simul_bandpass_filter",
            model.stoch_simul_bandpass_filter.is_some(),
        ),
        (
            SemanticFamily::MsSbvar,
            "restriction_fname",
            model.restriction_fname_span.is_some(),
        ),
        (SemanticFamily::MsSbvar, "bvar_present", model.bvar_present),
    ];
    for (family, name, present) in markers {
        facts.push(marker(model, family, name, FieldState::boolean(present)));
    }
    for (name, value) in [
        ("identification_order", model.identification_order),
        ("max_dim_cova_group", model.max_dim_cova_group),
        ("discretionary_order", model.discretionary_order),
    ] {
        facts.push(marker(
            model,
            if name == "discretionary_order" {
                SemanticFamily::Policy
            } else {
                SemanticFamily::MsSbvar
            },
            name,
            optional_integer(value.map(|(value, _)| value)),
        ));
    }
    // Slice 04 owns prior-function/estimated-init markers together with priors;
    // slice 01 owns surprise and all retained shock command setup fields.
}

fn marker(model: &Model, family: SemanticFamily, name: &str, state: FieldState) -> CapturedFact {
    let mut value = CapturedFact::new(
        family,
        "retained_setting",
        vec![name.into()],
        RowSide::named(name, ComparisonScope::global()),
    );
    value.count_unit = CountUnit::FinalFact;
    if let Some(receipt) = model.setting_receipts.get(name) {
        attach_setting_context(model, &mut value, receipt.tokens.start);
        if let Some(options) = &receipt.option_list {
            claim_options(
                model,
                &mut value,
                options.clone(),
                std::slice::from_ref(&receipt.tokens),
            );
        } else {
            value.claims.push(receipt.tokens.clone());
        }
    }
    field(&mut value, name, state, ChangeFacet::Options);
    gap(&mut value, &format!("{name}_retained_history"), &format!("The parser retains {name} as a selected setting or presence marker, not every option value and occurrence. Complete accepted statement context retains its written history."), "parser_command_setting_retention");
    value
}
