use super::*;

pub(super) fn collect(model: &Model, facts: &mut Vec<CapturedFact>) {
    command_fields(model, facts);
    for (role, rows) in [("varobs", &model.varobs), ("varexobs", &model.varexobs)] {
        for (index, row) in rows.iter().enumerate() {
            let name = model.name(row.name);
            let mut value = fact(
                model,
                SemanticFamily::Observables,
                role,
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
            facts.push(value);
        }
    }
    for (index, (name, _)) in model.observation_trends.iter().enumerate() {
        let name = model.name(*name);
        let mut value = fact(
            model,
            SemanticFamily::Observables,
            "observation_trends",
            name,
            vec![name.into()],
            None,
            index,
        );
        text(&mut value, "target", name, ChangeFacet::Target);
        field(
            &mut value,
            "trend_expression",
            FieldState::unknown(),
            ChangeFacet::Expression,
        );
        gap(&mut value, "observation_trend_expression_not_retained", "Only the first leading name per name and block is retained. Trend expressions and later duplicate rows are available in accepted statement context and Source.", "parser_observation_trends_retention");
        facts.push(value);
    }
    for (index, row) in model.databases.iter().enumerate() {
        let mut value = fact(
            model,
            SemanticFamily::Data,
            "database",
            "database",
            vec![],
            None,
            index,
        );
        field(
            &mut value,
            "variables",
            FieldState::present(names(model, row.names.iter().map(|(name, _)| *name))),
            ChangeFacet::Target,
        );
        facts.push(value);
    }
    for (index, row) in model.set_time.iter().enumerate() {
        let mut value = fact(
            model,
            SemanticFamily::Data,
            "set_time",
            "set_time",
            vec![],
            None,
            index,
        );
        text(&mut value, "date", &row.value.text, ChangeFacet::Options);
        facts.push(value);
    }
    for (index, row) in model.date_options.iter().enumerate() {
        let order = model
            .fact_receipts
            .get("date_option")
            .and_then(|receipts| receipts.get(index))
            .map(|receipt| receipt.parse_order);
        if row.command == "estimation"
            && model
                .estimation_statements
                .iter()
                .flat_map(|statement| &statement.data_options)
                .any(|option| Some(option.active_tokens.start) == order)
        {
            continue;
        }
        let mut value = fact(
            model,
            SemanticFamily::Data,
            "date_option",
            &row.command,
            vec![row.command.clone(), row.name.clone()],
            None,
            index,
        );
        text(&mut value, "command", &row.command, ChangeFacet::Role);
        text(&mut value, "option", &row.name, ChangeFacet::Options);
        text(&mut value, "date", &row.value.text, ChangeFacet::Options);
        facts.push(value);
    }
    for (index, row) in model.subsamples.iter().enumerate() {
        let mut value = fact(
            model,
            SemanticFamily::Data,
            "subsamples",
            "subsamples",
            vec![],
            None,
            index,
        );
        match row {
            SubsampleInstruction::Declare { head, ranges, .. } => {
                text(&mut value, "form", "declaration", ChangeFacet::Operation);
                field(
                    &mut value,
                    "target",
                    FieldState::present(head_value(model, head)),
                    ChangeFacet::Target,
                );
                field(
                    &mut value,
                    "ranges",
                    FieldState::present(FieldValue::List(
                        ranges
                            .iter()
                            .map(|range| {
                                record([
                                    ("label", FieldValue::Text(model.name(range.name).into())),
                                    ("first", FieldValue::Text(range.first.text.clone())),
                                    ("last", FieldValue::Text(range.last.text.clone())),
                                ])
                            })
                            .collect(),
                    )),
                    ChangeFacet::Options,
                );
            }
            SubsampleInstruction::Copy { target, source, .. } => {
                text(&mut value, "form", "copy", ChangeFacet::Operation);
                field(
                    &mut value,
                    "target",
                    FieldState::present(head_value(model, target)),
                    ChangeFacet::Target,
                );
                field(
                    &mut value,
                    "source",
                    FieldState::present(head_value(model, source)),
                    ChangeFacet::Target,
                );
            }
        }
        facts.push(value);
    }
    // Dotted subsamples are the same accepted operation as model.subsamples.
    // That typed range/copy producer owns values; no second counted head row.
    for (index, row) in model.data_statements.iter().enumerate() {
        let mut value = fact(
            model,
            SemanticFamily::Data,
            "data",
            "data",
            vec![],
            row.options.first().map(|option| option.parse_order),
            index,
        );
        field(
            &mut value,
            "options",
            FieldState::present(options(model, &row.options)),
            ChangeFacet::Options,
        );
        full_claim(model, &mut value);
        facts.push(value);
    }
    for (index, row) in model.estimation_statements.iter().enumerate() {
        let mut value = fact(
            model,
            SemanticFamily::Data,
            "estimation_data",
            "estimation",
            vec![],
            row.data_options.first().map(|option| option.parse_order),
            index,
        );
        field(
            &mut value,
            "has_datafile",
            FieldState::boolean(row.has_datafile),
            ChangeFacet::Options,
        );
        field(
            &mut value,
            "data_options",
            FieldState::present(options(model, &row.data_options)),
            ChangeFacet::Options,
        );
        gap(&mut value, "estimation_other_options_context", "Only data locating options have named data fields. Other accepted estimation settings retain statement context.", "parser_command_option_retention");
        value.claims.extend(
            row.data_options
                .iter()
                .map(|option| option.active_tokens.clone()),
        );
        facts.push(value);
    }
    for (index, row) in model.estimation_dsge_var_stmts.iter().enumerate() {
        let mut value = fact(
            model,
            SemanticFamily::Data,
            "estimation_dsge_var",
            "estimation DSGE-VAR",
            vec![],
            None,
            index,
        );
        field(
            &mut value,
            "estimated_form",
            FieldState::boolean(row.estimated.is_some()),
            ChangeFacet::Options,
        );
        field(
            &mut value,
            "calibrated_form",
            FieldState::boolean(row.calibrated.is_some()),
            ChangeFacet::Options,
        );
        gap(&mut value, "dsge_var_calibration_value_context", "The record retains bare versus calibrated form, but not the calibrated value. Accepted statement context owns that value.", "parser_estimation_dsge_var_retention");
        facts.push(value);
    }
    let mut value = CapturedFact::new(
        SemanticFamily::Data,
        "load_params_first",
        vec![],
        RowSide::named("load_params_and_steady_state", ComparisonScope::global()),
    );
    value.count_unit = CountUnit::FinalFact;
    if let Some(receipt) = model.setting_receipts.get("load_params_file") {
        attach_setting_context(model, &mut value, receipt.tokens.start);
        value.claims.push(receipt.tokens.clone());
    }
    field(
        &mut value,
        "filename",
        FieldState::optional_text(
            model
                .load_params_file
                .as_ref()
                .map(|(name, _)| name.as_str()),
        ),
        ChangeFacet::Options,
    );
    gap(&mut value, "load_params_history_first_only", "Only the first nonempty load filename is retained. All accepted load occurrences remain in statement context; loaded data is not captured.", "parser_load_params_retention");
    facts.push(value);
}

fn command_fields(model: &Model, facts: &mut Vec<CapturedFact>) {
    let ms_commands: BTreeSet<_> = model
        .ms_statements
        .iter()
        .map(|statement| statement.command.as_str())
        .collect();
    let mut lists: BTreeMap<u32, Vec<&CommandSymbol>> = BTreeMap::new();
    for row in &model.command_symbols {
        lists.entry(row.list_id).or_default().push(row);
    }
    for (ordinal, list) in lists.values().enumerate() {
        let command = &list[0].command;
        let family = if command == "osr_params" {
            SemanticFamily::Policy
        } else if ms_commands.contains(command.as_str()) {
            SemanticFamily::MsSbvar
        } else {
            SemanticFamily::Commands
        };
        let mut value = fact(
            model,
            family,
            "command_symbol_list",
            command,
            vec![command.clone()],
            list.first().map(|row| row.parse_order),
            ordinal,
        );
        text(&mut value, "command", command, ChangeFacet::Role);
        field(
            &mut value,
            "symbols",
            FieldState::present(names(model, list.iter().map(|row| row.name))),
            ChangeFacet::Target,
        );
        gap(&mut value, "command_list_empty_occurrences_not_retained", "These retained written list records include repeats and identify their per-command grouping. Empty lists have no named-list record and remain accepted statement context. No effective duplicate-removed list is inferred.", "parser_command_list_retention");
        if let Some(parent) = parent_id(&value).and_then(|id| model.statements.get(id)) {
            let start = list[0].parse_order;
            let end = list.last().unwrap().parse_order + 1;
            if parent.token_range.start <= start && end <= parent.token_range.end {
                // Keep skipped recovery tokens available to command context.
                if model.expanded_tokens[start..end].iter().all(|token| {
                    matches!(
                        token.kind,
                        crate::lexer::TokenKind::Ident | crate::lexer::TokenKind::Comma
                    )
                }) {
                    value.claims.push(start..end);
                } else {
                    value
                        .claims
                        .extend(list.iter().map(|row| row.parse_order..row.parse_order + 1));
                }
            }
        }
        facts.push(value);
    }
    for (ordinal, request) in model.stoch_simul_requests.iter().enumerate() {
        let mut value = fact(
            model,
            SemanticFamily::Shocks,
            "stoch_simul_irf_request",
            "stoch_simul IRF request",
            vec![],
            Some(request.parse_order),
            ordinal,
        );
        field(
            &mut value,
            "irf",
            optional_integer(request.irf.map(|(value, _)| value)),
            ChangeFacet::Options,
        );
        field(
            &mut value,
            "irf_shocks",
            request
                .irf_shocks
                .as_ref()
                .map(|rows| FieldState::present(names(model, rows.iter().map(|(name, _)| *name))))
                .unwrap_or_else(FieldState::absent),
            ChangeFacet::Target,
        );
        if let Some(options) = &request.option_tokens {
            let owned: Vec<_> = request
                .irf_tokens
                .iter()
                .chain(&request.irf_shocks_tokens)
                .cloned()
                .collect();
            claim_options(model, &mut value, options.clone(), &owned);
        }
        facts.push(value);
    }
    for (ordinal, option) in model
        .irf_shocks_options
        .iter()
        .filter(|option| !option.command.eq_ignore_ascii_case("stoch_simul"))
        .enumerate()
    {
        let mut value = fact(
            model,
            SemanticFamily::Shocks,
            "command_irf_shocks",
            &option.command,
            vec![option.command.clone()],
            Some(option.parse_order),
            ordinal,
        );
        text(&mut value, "command", &option.command, ChangeFacet::Role);
        field(
            &mut value,
            "irf_shocks",
            FieldState::present(names(model, option.names.iter().map(|(name, _)| *name))),
            ChangeFacet::Target,
        );
        claim_options(
            model,
            &mut value,
            option.option_tokens.clone(),
            std::slice::from_ref(&option.active_tokens),
        );
        facts.push(value);
    }
}

fn head_value(model: &Model, head: &SubsampleHead) -> FieldValue {
    let (kind, targets) = match head {
        SubsampleHead::Symbol(name, _) => ("parameter", vec![*name]),
        SubsampleHead::Std(name, _) => ("std", vec![*name]),
        SubsampleHead::Corr(first, _, second, _) => ("corr", vec![*first, *second]),
    };
    record([
        ("kind", FieldValue::Text(kind.into())),
        ("names", names(model, targets)),
    ])
}
