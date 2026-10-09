use super::*;

pub(super) fn collect(
    model: &Model,
    facts: &mut Vec<CapturedFact>,
    work: &mut RetainedExpressionWork,
) {
    occbin(model, facts);
    policy(model, facts, work);
    semi_structural(model, facts);
    heterogeneity(model, facts);
    external_and_trends(model, facts, work);
}

fn occbin(model: &Model, facts: &mut Vec<CapturedFact>) {
    for (index, row) in model.occbin_constraints.iter().enumerate() {
        let mut value = fact(
            model,
            SemanticFamily::Occbin,
            "constraint",
            &row.name,
            vec![row.name.clone()],
            Some(row.parse_order),
            index,
        );
        text(&mut value, "name", &row.name, ChangeFacet::Label);
        value.claims.push(row.name_tokens.clone());
        for (name, expression) in [
            ("bind", &row.bind),
            ("relax", &row.relax),
            ("error_bind", &row.error_bind),
            ("error_relax", &row.error_relax),
        ] {
            expr(
                &mut value,
                name,
                FieldState::optional_text(
                    expression
                        .as_ref()
                        .map(|expression| expression.text.as_str()),
                ),
            );
            if let Some(expression) = expression {
                value.claims.push(expression.active_tokens.clone());
            }
        }
        facts.push(value);
    }
}

fn policy(model: &Model, facts: &mut Vec<CapturedFact>, work: &mut RetainedExpressionWork) {
    for (index, row) in model.policy_command_statements.iter().enumerate() {
        let mut value = fact(
            model,
            SemanticFamily::Policy,
            "policy_command",
            row.command.as_str(),
            vec![],
            Some(row.parse_order),
            index,
        );
        text(
            &mut value,
            "command",
            row.command.as_str(),
            ChangeFacet::Role,
        );
        field(
            &mut value,
            "planner_discount_present",
            FieldState::boolean(row.planner_discount.is_some()),
            ChangeFacet::Options,
        );
        let instruments = model
            .instrument_uses
            .iter()
            .filter(|instrument| instrument.command_index == index)
            .map(|instrument| {
                record([
                    ("name", FieldValue::Text(model.name(instrument.name).into())),
                    (
                        "kind",
                        FieldValue::Text(instrument.kind.unwrap_or("unknown").into()),
                    ),
                ])
            })
            .collect();
        value.claims.extend(row.owned_option_words.iter().cloned());
        value.claims.extend(
            model
                .instrument_uses
                .iter()
                .filter(|instrument| instrument.command_index == index)
                .map(|instrument| instrument.parse_order..instrument.parse_order + 1),
        );
        field(
            &mut value,
            "instruments",
            FieldState::present(FieldValue::List(instruments)),
            ChangeFacet::Target,
        );
        gap(&mut value, "policy_options_context", "Policy records retain command form, instrument occurrences and discount presence. Other accepted options remain statement context.", "parser_policy_option_retention");
        facts.push(value);
    }
    let mut value = CapturedFact::new(
        SemanticFamily::Policy,
        "retained_planner_objective",
        vec![],
        RowSide::named("Retained planner objective", ComparisonScope::global()),
    );
    value.count_unit = CountUnit::FinalFact;
    if let Some(range) = &model.planner_objective_tokens {
        attach_setting_context(model, &mut value, range.start);
        value.claims.push(range.clone());
    }
    let objective = expression(
        model,
        &mut value,
        "planner_objective",
        model.planner_objective_expr,
        work,
    );
    expr(&mut value, "planner_objective", objective);
    gap(&mut value, "planner_objective_first_only", "The parser retains the first nonempty planner objective expression. Later accepted objectives remain statement context.", "parser_planner_objective_retention");
    facts.push(value);
    let mut value = CapturedFact::new(
        SemanticFamily::Policy,
        "retained_planner_discount",
        vec![],
        RowSide::named("Retained planner discount", ComparisonScope::global()),
    );
    value.count_unit = CountUnit::FinalFact;
    if let Some(receipt) = model.setting_receipts.get("planner_discount_expression") {
        attach_setting_context(model, &mut value, receipt.tokens.start);
        if model
            .setting_receipts
            .get("planner_discount_value")
            .is_some_and(|number| number.tokens != receipt.tokens)
        {
            value.side.context = None;
            value.side.provenance = None;
            gap(&mut value, "planner_discount_selected_origins", "First expression and first finite value were retained from different option occurrences; there is no single supporting statement context for both fields.", "parser_planner_discount_retention");
        }
    }
    let discount = expression(
        model,
        &mut value,
        "planner_discount_expression",
        model.planner_discount_expr,
        work,
    );
    expr(&mut value, "planner_discount_expression", discount);
    field(
        &mut value,
        "planner_discount_value",
        if model.planner_discount_expr.is_some() {
            FieldState::number(model.planner_discount)
        } else {
            FieldState::absent()
        },
        ChangeFacet::ParameterValue,
    );
    gap(&mut value, "planner_discount_first_only", "The expression and first folded discount are retained separately with first-wins behavior. Later option values remain statement context.", "parser_planner_discount_retention");
    facts.push(value);
    let mut value = CapturedFact::new(
        SemanticFamily::Policy,
        "retained_instrument_union",
        vec![],
        RowSide::named("Retained instrument union", ComparisonScope::global()),
    );
    value.count_unit = CountUnit::FinalFact;
    field(
        &mut value,
        "instrument_union",
        FieldState::present(names(model, model.instruments.iter().copied())),
        ChangeFacet::Target,
    );
    gap(&mut value, "instrument_union_first_seen", "This retained union keeps each distinct instrument in first-seen order. Accepted per-command instrument lists retain written duplicates and their own association.", "parser_policy_option_retention");
    facts.push(value);
    for (index, row) in model.osr_params_bounds.iter().enumerate() {
        let name = model.name(row.name);
        let mut value = fact(
            model,
            SemanticFamily::Policy,
            "osr_bounds",
            name,
            vec![name.into()],
            Some(row.active_tokens.start),
            index,
        );
        text(&mut value, "target", name, ChangeFacet::Target);
        let lower = expression(model, &mut value, "lower", row.lower, work);
        let upper = expression(model, &mut value, "upper", row.upper, work);
        expr(&mut value, "lower", lower);
        expr(&mut value, "upper", upper);
        value.claims.push(row.active_tokens.clone());
        facts.push(value);
    }
    for (index, row) in model.optim_weights.iter().enumerate() {
        let name = model.name(row.first);
        let mut value = fact(
            model,
            SemanticFamily::Policy,
            "optim_weight",
            name,
            vec![
                name.into(),
                row.second.map(|name| model.name(name)).unwrap_or("").into(),
            ],
            Some(row.active_tokens.start),
            index,
        );
        text(&mut value, "first", name, ChangeFacet::Target);
        field(
            &mut value,
            "second",
            optional_name(model, row.second),
            ChangeFacet::Target,
        );
        field(
            &mut value,
            "first_kind",
            FieldState::optional_text(row.first_kind),
            ChangeFacet::SymbolKind,
        );
        field(
            &mut value,
            "second_kind",
            FieldState::optional_text(row.second_kind),
            ChangeFacet::SymbolKind,
        );
        let weight = expression(model, &mut value, "weight", row.expr, work);
        expr(&mut value, "weight", weight);
        value.claims.push(row.active_tokens.clone());
        facts.push(value);
    }
    for (index, row) in model.ramsey_constraints.iter().enumerate() {
        let mut value = fact(
            model,
            SemanticFamily::Policy,
            "ramsey_constraint",
            "ramsey constraint",
            vec![],
            Some(row.active_tokens.start),
            index,
        );
        let expression = expression(model, &mut value, "constraint", row.expr, work);
        expr(&mut value, "constraint", expression);
        value.claims.push(row.active_tokens.clone());
        facts.push(value);
    }
    // command_fields preserves osr_params per-command lists in this slice. Its
    // union is a cache and does not add a second accepted operation here.
}

fn semi_structural(model: &Model, facts: &mut Vec<CapturedFact>) {
    for (index, row) in model.semi_structural_commands.iter().enumerate() {
        let command = match row.kind {
            SemiStructuralKind::VarModel => "var_model",
            SemiStructuralKind::TrendComponentModel => "trend_component_model",
            SemiStructuralKind::VarExpectationModel => "var_expectation_model",
            SemiStructuralKind::PacModel => "pac_model",
        };
        let mut value = fact(
            model,
            SemanticFamily::SemiStructural,
            "model_command",
            command,
            vec![],
            Some(row.parse_order),
            index,
        );
        text(&mut value, "command", command, ChangeFacet::Role);
        field(
            &mut value,
            "options",
            FieldState::present(FieldValue::List(
                row.options
                    .iter()
                    .map(|option| {
                        record([
                            ("name", FieldValue::Text(option.name.clone())),
                            ("value", semi_value(model, &option.value)),
                        ])
                    })
                    .collect(),
            )),
            ChangeFacet::Options,
        );
        value.claims.push(row.active_tokens.clone());
        facts.push(value);
    }
    for (index, block) in model.pac_target_info.iter().enumerate() {
        let name = model.name(block.name);
        let mut value = fact(
            model,
            SemanticFamily::SemiStructural,
            "pac_target_info",
            name,
            vec![name.into()],
            Some(block.parse_order),
            index,
        );
        text(&mut value, "model", name, ChangeFacet::Target);
        field(
            &mut value,
            "rows",
            FieldState::present(FieldValue::List(
                block
                    .rows
                    .iter()
                    .map(|row| match row {
                        PacTargetInfoRow::Target(expression) => record([
                            ("role", FieldValue::Text("target".into())),
                            ("expression", FieldValue::Text(expression.text.clone())),
                        ]),
                        PacTargetInfoRow::AuxnameTargetNonstationary { name, .. } => record([
                            (
                                "role",
                                FieldValue::Text("auxname_target_nonstationary".into()),
                            ),
                            ("name", FieldValue::Text(model.name(*name).into())),
                        ]),
                        PacTargetInfoRow::Component(component) => record([
                            ("role", FieldValue::Text("component".into())),
                            (
                                "expression",
                                FieldValue::Text(component.component.text.clone()),
                            ),
                            (
                                "rows",
                                FieldValue::List(
                                    component
                                        .rows
                                        .iter()
                                        .map(|row| match row {
                                            PacTargetComponentRow::Growth(expression) => record([
                                                ("role", FieldValue::Text("growth".into())),
                                                (
                                                    "expression",
                                                    FieldValue::Text(expression.text.clone()),
                                                ),
                                            ]),
                                            PacTargetComponentRow::Auxname { name, .. } => {
                                                record([
                                                    ("role", FieldValue::Text("auxname".into())),
                                                    (
                                                        "name",
                                                        FieldValue::Text(model.name(*name).into()),
                                                    ),
                                                ])
                                            }
                                            PacTargetComponentRow::Kind { text, .. } => record([
                                                ("role", FieldValue::Text("kind".into())),
                                                ("value", FieldValue::Text(text.clone())),
                                            ]),
                                        })
                                        .collect(),
                                ),
                            ),
                        ]),
                    })
                    .collect(),
            )),
            ChangeFacet::Options,
        );
        value.claims.push(block.active_tokens.clone());
        facts.push(value);
    }
}

fn semi_value(model: &Model, value: &SemiStructuralValue) -> FieldValue {
    match value {
        SemiStructuralValue::Flag => record([("shape", FieldValue::Text("flag".into()))]),
        SemiStructuralValue::Symbol { name, .. } => record([
            ("shape", FieldValue::Text("symbol".into())),
            ("name", FieldValue::Text(model.name(*name).into())),
        ]),
        SemiStructuralValue::Tags(tags) => record([
            ("shape", FieldValue::Text("tags".into())),
            (
                "tags",
                FieldValue::List(
                    tags.iter()
                        .map(|(tag, _)| FieldValue::Text(tag.clone()))
                        .collect(),
                ),
            ),
        ]),
        SemiStructuralValue::Expression(expression) => record([
            ("shape", FieldValue::Text("expression".into())),
            ("text", FieldValue::Text(expression.text.clone())),
        ]),
        SemiStructuralValue::Integer { text, .. } => record([
            ("shape", FieldValue::Text("integer".into())),
            ("text", FieldValue::Text(text.clone())),
        ]),
        SemiStructuralValue::Horizon { first, last, .. } => record([
            ("shape", FieldValue::Text("horizon".into())),
            ("first", FieldValue::Text(first.clone())),
            ("last", FieldValue::Text(last.clone())),
        ]),
        SemiStructuralValue::Kind { text, .. } => record([
            ("shape", FieldValue::Text("kind".into())),
            ("text", FieldValue::Text(text.clone())),
        ]),
    }
}

fn heterogeneity(model: &Model, facts: &mut Vec<CapturedFact>) {
    for (index, row) in model.heterogeneity_dimensions.iter().enumerate() {
        let name = model.name(row.name);
        let mut value = fact(
            model,
            SemanticFamily::Heterogeneity,
            "dimension",
            name,
            vec![],
            Some(row.parse_order),
            index,
        );
        text(&mut value, "dimension", name, ChangeFacet::Scope);
        facts.push(value);
    }
    for (index, row) in model.heterogeneity_commands.iter().enumerate() {
        let mut value = fact(
            model,
            SemanticFamily::Heterogeneity,
            "heterogeneity_command",
            &row.command,
            vec![],
            None,
            index,
        );
        text(&mut value, "command", &row.command, ChangeFacet::Role);
        text(
            &mut value,
            "command_kind",
            match row.kind {
                HeterogeneityCommandKind::LoadSteadyState => "load_steady_state",
                HeterogeneityCommandKind::ComputeSteadyState => "compute_steady_state",
                HeterogeneityCommandKind::Solve => "solve",
                HeterogeneityCommandKind::Simulate => "simulate",
            },
            ChangeFacet::Role,
        );
        field(
            &mut value,
            "options",
            FieldState::present(FieldValue::List(
                row.options
                    .iter()
                    .map(|option| {
                        let mut entries = BTreeMap::new();
                        entries.insert("name".into(), FieldValue::Text(option.name.clone()));
                        entries.insert(
                            "has_value".into(),
                            FieldValue::Boolean(option.value.is_some()),
                        );
                        if let Some((text, _)) = &option.value {
                            entries.insert("text".into(), FieldValue::Text(text.clone()));
                        }
                        FieldValue::Record(entries)
                    })
                    .collect(),
            )),
            ChangeFacet::Options,
        );
        field(
            &mut value,
            "simulate_names",
            FieldState::present(names(
                model,
                row.simulate_names.iter().map(|(name, _)| *name),
            )),
            ChangeFacet::Target,
        );
        gap(&mut value, "heterogeneity_numerical_data_not_captured", "Options are written settings. Referenced MAT files, grids, policies, distributions and numerical solutions are not captured model facts.", "source_capture_boundary");
        facts.push(value);
    }
}

fn external_and_trends(
    model: &Model,
    facts: &mut Vec<CapturedFact>,
    work: &mut RetainedExpressionWork,
) {
    for (index, row) in model.external_functions.iter().enumerate() {
        let name = row
            .name
            .map(|(name, _)| model.name(name))
            .unwrap_or("external_function");
        let mut value = fact(
            model,
            SemanticFamily::ExternalFunctions,
            "interface",
            name,
            vec![],
            Some(row.parse_order),
            index,
        );
        field(
            &mut value,
            "name",
            optional_name(model, row.name.map(|(name, _)| name)),
            ChangeFacet::Target,
        );
        field(
            &mut value,
            "nargs",
            optional_integer(row.nargs),
            ChangeFacet::Options,
        );
        field(
            &mut value,
            "first_derivative",
            derivative(model, row.first_deriv.as_ref()),
            ChangeFacet::Options,
        );
        field(
            &mut value,
            "second_derivative",
            derivative(model, row.second_deriv.as_ref()),
            ChangeFacet::Options,
        );
        gap(&mut value, "external_function_body_not_captured", "The declared external interface is compared. External M/MEX function bodies and numerical evaluations are not captured.", "source_capture_boundary");
        full_claim(model, &mut value);
        facts.push(value);
    }
    for (index, row) in model.trend_vars.iter().enumerate() {
        let name = model.name(row.name);
        let mut value = fact(
            model,
            SemanticFamily::Trends,
            "trend_variable",
            name,
            vec![name.into()],
            Some(row.parse_order),
            index,
        );
        text(&mut value, "target", name, ChangeFacet::Target);
        field(
            &mut value,
            "log_trend",
            FieldState::boolean(row.log_trend),
            ChangeFacet::Options,
        );
        let growth = expression(model, &mut value, "growth", row.growth, work);
        expr(&mut value, "growth", growth);
        facts.push(value);
    }
    for (index, row) in model.nonstationary_vars.iter().enumerate() {
        let name = model.name(row.name);
        let mut value = fact(
            model,
            SemanticFamily::Trends,
            "nonstationary_variable",
            name,
            vec![name.into()],
            None,
            index,
        );
        text(&mut value, "target", name, ChangeFacet::Target);
        field(
            &mut value,
            "log_deflator",
            FieldState::boolean(row.log_deflator),
            ChangeFacet::Options,
        );
        field(
            &mut value,
            "log_option",
            FieldState::boolean(row.log_option),
            ChangeFacet::Options,
        );
        let deflator = expression(model, &mut value, "deflator", row.deflator, work);
        expr(&mut value, "deflator", deflator);
        facts.push(value);
    }
    for (index, block) in model.deterministic_trends.iter().enumerate() {
        let mut value = fact(
            model,
            SemanticFamily::Trends,
            "deterministic_trends",
            "deterministic_trends",
            vec![],
            Some(block.parse_order),
            index,
        );
        field(
            &mut value,
            "rows",
            FieldState::present(FieldValue::List(
                block
                    .rows
                    .iter()
                    .map(|row| {
                        record([
                            ("name", FieldValue::Text(model.name(row.name).into())),
                            ("expression", FieldValue::Text(row.expression.text.clone())),
                        ])
                    })
                    .collect(),
            )),
            ChangeFacet::Expression,
        );
        value.claims.push(block.active_tokens.clone());
        facts.push(value);
    }
}

fn derivative(model: &Model, value: Option<&DerivSpec>) -> FieldState {
    match value {
        None => FieldState::absent(),
        Some(DerivSpec::Bare(_)) => FieldState::present(record([(
            "form",
            FieldValue::Text("provided_by_function".into()),
        )])),
        Some(DerivSpec::Named(name, _)) => FieldState::present(record([
            ("form", FieldValue::Text("named_function".into())),
            ("name", FieldValue::Text(model.name(*name).into())),
        ])),
    }
}
