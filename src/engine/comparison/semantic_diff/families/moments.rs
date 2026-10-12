use super::*;

pub(super) fn collect(
    model: &Model,
    facts: &mut Vec<CapturedFact>,
    work: &mut RetainedExpressionWork,
) {
    for (index, row) in model.mom_statements.iter().enumerate() {
        let mut value = fact(
            model,
            SemanticFamily::Moments,
            "method_of_moments",
            "method_of_moments",
            vec![],
            row.options.first().map(|option| option.parse_order),
            index,
        );
        field(
            &mut value,
            "has_option_list",
            FieldState::boolean(row.has_option_list),
            ChangeFacet::Options,
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
    for (index, row) in model.matched_moments.iter().enumerate() {
        let mut value = fact(
            model,
            SemanticFamily::Moments,
            "matched_moment",
            "matched moment",
            vec![],
            Some(row.active_tokens.start),
            index,
        );
        expr(&mut value, "expression", FieldState::text(&row.text));
        value.claims.push(row.active_tokens.clone());
        facts.push(value);
    }
    for (index, block) in model.matched_irfs.iter().enumerate() {
        let mut value = fact(
            model,
            SemanticFamily::Moments,
            "matched_irfs",
            "matched_irfs",
            vec![],
            Some(block.parse_order),
            index,
        );
        field(
            &mut value,
            "overwrite",
            FieldState::boolean(block.overwrite),
            ChangeFacet::Operation,
        );
        let mut rows = Vec::new();
        for row in &block.rows {
            let periods = FieldState::present(FieldValue::List(
                row.period_tokens
                    .iter()
                    .map(|period| {
                        let first = range_text(
                            model,
                            &mut value,
                            "rows_periods",
                            period.first.clone(),
                            work,
                        );
                        let last = period
                            .last
                            .as_ref()
                            .map(|range| {
                                range_text(model, &mut value, "rows_periods", range.clone(), work)
                            })
                            .unwrap_or_else(FieldState::absent);
                        record([("first", state_value(first)), ("last", state_value(last))])
                    })
                    .collect(),
            ));
            let values = token_list(model, &mut value, "rows_values", &row.value_tokens, work);
            let weights = token_list(model, &mut value, "rows_weights", &row.weight_tokens, work);
            let parsed_values = FieldValue::List(
                row.value_exprs
                    .iter()
                    .map(|id| {
                        state_value(expression(
                            model,
                            &mut value,
                            "rows_parenthesized_values",
                            Some(*id),
                            work,
                        ))
                    })
                    .collect(),
            );
            let parsed_weights = FieldValue::List(
                row.weight_exprs
                    .iter()
                    .map(|id| {
                        state_value(expression(
                            model,
                            &mut value,
                            "rows_parenthesized_weights",
                            Some(*id),
                            work,
                        ))
                    })
                    .collect(),
            );
            rows.push(record([
                (
                    "endogenous",
                    FieldValue::Text(model.name(row.endogenous).into()),
                ),
                (
                    "exogenous",
                    FieldValue::Text(model.name(row.exogenous).into()),
                ),
                ("periods", state_value(periods)),
                ("values", state_value(values)),
                ("weights", state_value(weights)),
                ("parenthesized_value_expressions", parsed_values),
                ("parenthesized_weight_expressions", parsed_weights),
            ]));
            value.claims.extend(row.represented_tokens.iter().cloned());
        }
        field(
            &mut value,
            "rows",
            FieldState::present(FieldValue::List(rows)),
            ChangeFacet::Options,
        );
        claim_opener(
            model,
            &mut value,
            &block.opener_tokens,
            &["matched_irfs", "overwrite"],
        );
        gap(&mut value, "matched_irf_expression_positions", "Direct accepted item receipts retain every bare/parenthesized value and weight in order, and period range endpoints. Parenthesized-only expression vectors have no positional link to mixed lists; their ordered supporting trees are kept separately and never zipped to items.", "parser_matched_irf_retention");
        facts.push(value);
    }
    for (index, block) in model.matched_irfs_weights.iter().enumerate() {
        let mut value = fact(
            model,
            SemanticFamily::Moments,
            "matched_irfs_weights",
            "matched_irfs_weights",
            vec![],
            Some(block.parse_order),
            index,
        );
        field(
            &mut value,
            "overwrite",
            FieldState::boolean(block.overwrite),
            ChangeFacet::Operation,
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
                            (
                                "left",
                                record([
                                    (
                                        "endogenous",
                                        FieldValue::Text(model.name(row.left_endo).into()),
                                    ),
                                    ("periods", FieldValue::Text(row.left_periods.clone())),
                                    (
                                        "exogenous",
                                        FieldValue::Text(model.name(row.left_exo).into()),
                                    ),
                                ]),
                            ),
                            (
                                "right",
                                record([
                                    (
                                        "endogenous",
                                        FieldValue::Text(model.name(row.right_endo).into()),
                                    ),
                                    ("periods", FieldValue::Text(row.right_periods.clone())),
                                    (
                                        "exogenous",
                                        FieldValue::Text(model.name(row.right_exo).into()),
                                    ),
                                ]),
                            ),
                            ("weight", FieldValue::Text(row.weight_text.clone())),
                        ])
                    })
                    .collect(),
            )),
            ChangeFacet::Expression,
        );
        claim_opener(
            model,
            &mut value,
            &block.opener_tokens,
            &["matched_irfs_weights", "overwrite"],
        );
        value
            .claims
            .extend(block.rows.iter().map(|row| row.active_tokens.clone()));
        facts.push(value);
    }
    for (index, block) in model.moment_calibration.iter().enumerate() {
        let mut value = fact(
            model,
            SemanticFamily::Moments,
            "moment_calibration",
            "moment_calibration",
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
                            ("first", FieldValue::Text(model.name(row.first).into())),
                            ("second", FieldValue::Text(model.name(row.second).into())),
                            (
                                "lags",
                                state_value(FieldState::optional_text(row.lags.as_deref())),
                            ),
                            ("range", calibration_range(&row.range)),
                        ])
                    })
                    .collect(),
            )),
            ChangeFacet::Options,
        );
        claim_opener(
            model,
            &mut value,
            &block.opener_tokens,
            &["moment_calibration"],
        );
        value
            .claims
            .extend(block.rows.iter().map(|row| row.active_tokens.clone()));
        facts.push(value);
    }
    for (index, block) in model.irf_calibration.iter().enumerate() {
        let mut value = fact(
            model,
            SemanticFamily::Moments,
            "irf_calibration",
            "irf_calibration",
            vec![],
            Some(block.parse_order),
            index,
        );
        field(
            &mut value,
            "relative_irf",
            FieldState::boolean(block.relative_irf),
            ChangeFacet::Options,
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
                            (
                                "endogenous",
                                FieldValue::Text(model.name(row.endogenous).into()),
                            ),
                            (
                                "exogenous",
                                FieldValue::Text(model.name(row.exogenous).into()),
                            ),
                            (
                                "periods",
                                state_value(FieldState::optional_text(row.periods.as_deref())),
                            ),
                            ("range", calibration_range(&row.range)),
                        ])
                    })
                    .collect(),
            )),
            ChangeFacet::Options,
        );
        claim_opener(
            model,
            &mut value,
            &block.opener_tokens,
            &["irf_calibration", "relative_irf"],
        );
        value
            .claims
            .extend(block.rows.iter().map(|row| row.active_tokens.clone()));
        facts.push(value);
    }
    for (index, row) in model.generate_irfs.iter().enumerate() {
        let name = model.name(row.name);
        let mut value = fact(
            model,
            SemanticFamily::Moments,
            "generate_irfs",
            name,
            vec![name.into()],
            None,
            index,
        );
        text(&mut value, "label", name, ChangeFacet::Label);
        field(
            &mut value,
            "exogenous",
            FieldState::present(names(model, row.exos.iter().map(|(name, _)| *name))),
            ChangeFacet::Target,
        );
        field(
            &mut value,
            "coefficients",
            FieldState::unknown(),
            ChangeFacet::Options,
        );
        gap(&mut value, "generate_irf_coefficients_not_retained", "Signed coefficients are consumed by the parser but are not retained. The ordered label and exogenous names are compared; coefficient edits remain accepted statement context and Source.", "parser_generate_irf_retention");
        facts.push(value);
    }
    ms(model, facts, work);
}

fn token_list(
    model: &Model,
    fact: &mut CapturedFact,
    name: &str,
    ranges: &[std::ops::Range<usize>],
    work: &mut RetainedExpressionWork,
) -> FieldState {
    FieldState::present(FieldValue::List(
        ranges
            .iter()
            .map(|range| state_value(range_text(model, fact, name, range.clone(), work)))
            .collect(),
    ))
}

fn calibration_range(range: &CalibrationRange) -> FieldValue {
    match range {
        CalibrationRange::Bracket { lower, upper, .. } => record([
            ("form", FieldValue::Text("bracket".into())),
            ("lower", FieldValue::Text(lower.clone())),
            ("upper", FieldValue::Text(upper.clone())),
        ]),
        CalibrationRange::Plus { .. } => record([("form", FieldValue::Text("plus".into()))]),
        CalibrationRange::Minus { .. } => record([("form", FieldValue::Text("minus".into()))]),
    }
}

fn ms(model: &Model, facts: &mut Vec<CapturedFact>, work: &mut RetainedExpressionWork) {
    for (index, row) in model.ms_statements.iter().enumerate() {
        let mut value = fact(
            model,
            SemanticFamily::MsSbvar,
            "command",
            &row.command,
            vec![],
            Some(row.parse_order),
            index,
        );
        text(&mut value, "command", &row.command, ChangeFacet::Role);
        field(
            &mut value,
            "options",
            FieldState::present(options(model, &row.options)),
            ChangeFacet::Options,
        );
        full_claim(model, &mut value);
        facts.push(value);
    }
    for block in &model.svar_identifications {
        for (index, element) in block.elements.iter().enumerate() {
            let order = block.element_parse_orders.get(index).copied().flatten();
            let mut value = fact(
                model,
                SemanticFamily::MsSbvar,
                "svar_identification",
                "SVAR identification",
                vec![],
                order,
                index,
            );
            match element {
                SvarIdentificationElement::ExclusionLag { lag, equations, .. } => {
                    text(&mut value, "form", "exclusion_lag", ChangeFacet::Role);
                    field(
                        &mut value,
                        "lag",
                        optional_integer(*lag),
                        ChangeFacet::Options,
                    );
                    field(
                        &mut value,
                        "equations",
                        FieldState::present(FieldValue::List(
                            equations
                                .iter()
                                .map(|equation| {
                                    let mut entries = BTreeMap::new();
                                    if let Some(number) = equation.number {
                                        entries.insert(
                                            "number".into(),
                                            FieldValue::Integer(number.into()),
                                        );
                                    }
                                    entries.insert(
                                        "names".into(),
                                        names(model, equation.names.iter().map(|(name, _)| *name)),
                                    );
                                    FieldValue::Record(entries)
                                })
                                .collect(),
                        )),
                        ChangeFacet::Target,
                    );
                }
                SvarIdentificationElement::ExclusionConstants { .. } => {
                    text(&mut value, "form", "exclusion_constants", ChangeFacet::Role)
                }
                SvarIdentificationElement::UpperCholesky { .. } => {
                    text(&mut value, "form", "upper_cholesky", ChangeFacet::Role)
                }
                SvarIdentificationElement::LowerCholesky { .. } => {
                    text(&mut value, "form", "lower_cholesky", ChangeFacet::Role)
                }
                SvarIdentificationElement::Restriction {
                    number, expr_span, ..
                } => {
                    text(&mut value, "form", "restriction", ChangeFacet::Role);
                    field(
                        &mut value,
                        "equation_number",
                        optional_integer(*number),
                        ChangeFacet::Target,
                    );
                    let expression = span_text(model, &mut value, "restriction", *expr_span, work);
                    expr(&mut value, "restriction", expression);
                }
            }
            facts.push(value);
        }
    }
    for block in &model.conditional_forecast_paths {
        for (index, row) in block.rows.iter().enumerate() {
            let name = model.name(row.name);
            let mut value = fact(
                model,
                SemanticFamily::MsSbvar,
                "conditional_forecast_path",
                name,
                vec![name.into()],
                row.parse_order,
                index,
            );
            if let Some(proof) = &mut value.side.provenance {
                proof.parse_order = row.parse_order;
            }
            text(&mut value, "target", name, ChangeFacet::Target);
            field(
                &mut value,
                "has_periods",
                FieldState::boolean(row.has_periods),
                ChangeFacet::Options,
            );
            field(
                &mut value,
                "has_values",
                FieldState::boolean(row.has_values),
                ChangeFacet::Options,
            );
            let periods = span_list(model, &mut value, "periods", &row.periods, work);
            let values = span_list(model, &mut value, "values", &row.values, work);
            field(&mut value, "periods", periods, ChangeFacet::Options);
            field(&mut value, "values", values, ChangeFacet::Expression);
            facts.push(value);
        }
    }
}
