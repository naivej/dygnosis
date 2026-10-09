//! Written prior and optimizer settings from accepted parser occurrences.
//! Optional positions come from the existing reader, never from display text.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{
    DottedHead, DottedKind, EstimatedNameRole, EstimatedParam, EstimatedParamBlockKind,
    EstimatedParamKind, FamilyValueKind, Model,
};
use crate::model_diff::ModelDiff;

use super::occurrences::{
    accepted_statement, compare_facts, parent_at_order, statement_side, statement_text,
    CapturedFact, FactField, TokenClaims,
};
use super::*;

const SLOTS: [(&str, &str, ChangeFacet); 8] = [
    (
        "optimizer.initial",
        "Optimizer initial value",
        ChangeFacet::Assignment,
    ),
    (
        "optimizer.lower_bound",
        "Optimizer lower bound",
        ChangeFacet::Assignment,
    ),
    (
        "optimizer.upper_bound",
        "Optimizer upper bound",
        ChangeFacet::Assignment,
    ),
    ("prior.mean", "Prior mean", ChangeFacet::Prior),
    (
        "prior.standard_deviation",
        "Prior standard deviation",
        ChangeFacet::Prior,
    ),
    (
        "prior.support_parameter_3",
        "Prior third parameter (support)",
        ChangeFacet::Prior,
    ),
    (
        "prior.support_parameter_4",
        "Prior fourth parameter (support)",
        ChangeFacet::Prior,
    ),
    ("proposal.scale", "Proposal scale", ChangeFacet::Options),
];

pub(crate) fn populate(
    before: &Model,
    after: &Model,
    diff: &mut ModelDiff,
    claims: &mut TokenClaims,
) {
    let old = facts(before);
    let new = facts(after);
    let start = diff.semantic.rows.len();
    compare_facts(before, after, old, new, diff, claims);
    for row in &mut diff.semantic.rows[start..] {
        for field in &mut row.fields {
            if let (Some(FieldValue::Number(a)), Some(FieldValue::Number(b))) =
                (&field.before.value, &field.after.value)
            {
                field.numeric_difference = (b - a).is_finite().then_some(b - a);
            }
        }
    }
    if !diff
        .coverage
        .families
        .iter()
        .any(|coverage| coverage.family == SemanticFamily::Priors)
    {
        diff.coverage.families.push(FamilyCoverage {
            family: SemanticFamily::Priors,
            availability: Availability::Complete,
            fields: Vec::new(),
            limits: Vec::new(),
        });
    }
    if let Some(coverage) = diff
        .coverage
        .families
        .iter_mut()
        .find(|coverage| coverage.family == SemanticFamily::Priors)
    {
        coverage.fields = [
            "target",
            "target_kind",
            "target_symbol_kinds",
            "removal_target_roles",
            "distribution",
            "optimizer.initial",
            "optimizer.lower_bound",
            "optimizer.upper_bound",
            "prior.mean",
            "prior.standard_deviation",
            "prior.support_parameter_3",
            "prior.support_parameter_4",
            "proposal.scale",
            "block_operation",
            "overwrite",
            "use_calibration",
            "subsample",
            "copy_source",
            "copy_source_subsample",
            "options",
            "prior_function_has_function",
            "prior_function_has_parens",
        ]
        .map(str::to_string)
        .to_vec();
        coverage
            .fields
            .extend(SLOTS.iter().map(|(name, _, _)| format!("{name}.value")));
    }
    if diff.semantic.rows[start..].iter().any(|row| {
        row.change == ChangeKind::Unpaired
            || row
                .expressions
                .iter()
                .any(|expression| expression.availability == Availability::LimitExceeded)
    }) {
        super::occurrences::record_limit(
            diff,
            SemanticFamily::Priors,
            ComparisonLimit::new(
                "prior_detail_partial",
                "Some prior occurrences lack proven correspondence or bounded token highlights; separate written facts remain available.",
                "semantic_priors",
            ),
        );
    }
}

fn scope() -> ComparisonScope {
    ComparisonScope {
        domain: "global".into(),
        dimension: None,
        block: None,
    }
}

fn list(names: impl IntoIterator<Item = String>) -> FieldState {
    FieldState::present(FieldValue::List(
        names.into_iter().map(FieldValue::Text).collect(),
    ))
}

fn field(name: &str, label: &str, value: FieldState, facet: ChangeFacet) -> FactField {
    FactField::new(name, label, value, facet)
}

fn facts(model: &Model) -> Vec<CapturedFact> {
    let mut out = Vec::new();
    if model.prior_function_span.is_some() || model.posterior_function_span.is_some() {
        let mut fact = CapturedFact::new(
            SemanticFamily::Priors,
            "prior_function_summary",
            vec!["prior_posterior_function".into()],
            RowSide::named("Prior/posterior function settings", scope()),
        );
        fact.count_unit = CountUnit::FinalFact;
        fact.fields.extend([
            field(
                "prior_function_has_function",
                "Prior/posterior function option present",
                FieldState::boolean(model.prior_function_has_function),
                ChangeFacet::Options,
            ),
            field(
                "prior_function_has_parens",
                "Prior/posterior function option list present",
                FieldState::boolean(model.prior_function_has_parens),
                ChangeFacet::Options,
            ),
        ]);
        fact.limits.push(ComparisonLimit::new("prior_function_history_unavailable", "These retained prior/posterior function flags are shared sticky summaries; accepted Commands own each written occurrence and unavailable argument values.", "semantic_priors"));
        out.push(fact);
    }
    for block in &model.estimated_param_blocks {
        let Some(statement) = model.statements.get(block.statement_id) else {
            continue;
        };
        if !accepted_statement(model, statement) {
            continue;
        }
        let operation = match block.kind {
            EstimatedParamBlockKind::Parameters => "concatenate",
            EstimatedParamBlockKind::Initialization => "initialization_override",
            EstimatedParamBlockKind::Bounds => "bounds_override",
            EstimatedParamBlockKind::Removal => "remove",
        };
        let side = statement_side(model, block.statement_id, &statement.name, scope());
        let mut fact = CapturedFact::new(
            SemanticFamily::Priors,
            "estimated_parameter_block",
            vec![statement.name.clone()],
            side,
        );
        fact.count_unit = CountUnit::Operation;
        fact.fields.extend([
            field(
                "block_operation",
                "Block operation",
                FieldState::text(if block.overwrite {
                    "overwrite"
                } else {
                    operation
                }),
                ChangeFacet::Operation,
            ),
            field(
                "overwrite",
                "Overwrite earlier entries",
                FieldState::boolean(block.overwrite),
                ChangeFacet::Operation,
            ),
            field(
                "use_calibration",
                "Use calibration for initial values",
                FieldState::boolean(block.use_calibration),
                ChangeFacet::Operation,
            ),
        ]);
        // Only the two retained flags are owned. Other opener tokens stay with
        // Commands, which supplies context for any unretained option spelling.
        let represented = model
            .expanded_tokens
            .get(statement.opener_range.clone())
            .is_some_and(|tokens| {
                tokens
                    .iter()
                    .filter(|token| token.kind == crate::lexer::TokenKind::Ident)
                    .all(|token| {
                        [statement.name.as_str(), "overwrite", "use_calibration"]
                            .iter()
                            .any(|name| token.text(&model.source).eq_ignore_ascii_case(name))
                    })
            });
        if represented {
            fact.claims.push(statement.opener_range.clone());
        }
        out.push(fact);
    }
    for (role, entries) in [
        ("estimated_params_entry", &model.estimated_params),
        ("estimated_params_init_entry", &model.estimated_params_init),
        (
            "estimated_params_bounds_entry",
            &model.estimated_params_bounds,
        ),
        (
            "estimated_params_remove_entry",
            &model.estimated_params_remove,
        ),
    ] {
        for entry in entries {
            if let Some(fact) = estimated_fact(model, entry, role) {
                out.push(fact);
            }
        }
    }
    for dotted in &model.dotted_statements {
        if !matches!(dotted.kind, DottedKind::Prior | DottedKind::Options) {
            continue;
        }
        let Some(id) = parent_at_order(model, dotted.parse_order) else {
            continue;
        };
        let statement = &model.statements[id];
        if !accepted_statement(model, statement) {
            continue;
        }
        let (kind, names, subsample) = head(model, &dotted.head);
        let role = if dotted.kind == DottedKind::Prior {
            "dotted_prior"
        } else {
            "dotted_optimizer_options"
        };
        let label = format!("{} {}", kind, names.join(", "));
        let mut key = vec![kind.into()];
        key.extend(names.iter().cloned());
        key.push(subsample.clone().unwrap_or_default());
        let mut fact = CapturedFact::new(
            SemanticFamily::Priors,
            role,
            key,
            statement_side(model, id, &label, scope()),
        );
        fact.fields.extend([
            field(
                "target_kind",
                "Target kind",
                FieldState::text(kind),
                ChangeFacet::Target,
            ),
            field(
                "target",
                "Target names",
                list(names.clone()),
                ChangeFacet::Target,
            ),
            field(
                "target_symbol_kinds",
                "Captured target symbol kinds",
                list(names.iter().map(|name| {
                    model
                        .intern
                        .lookup(name)
                        .and_then(|name| {
                            model.symbol_kind_in_context(name, dotted.symbol_type_context)
                        })
                        .unwrap_or("unknown")
                        .into()
                })),
                ChangeFacet::Target,
            ),
            field(
                "subsample",
                "Subsample",
                FieldState::optional_text(subsample.as_deref()),
                ChangeFacet::Target,
            ),
            field(
                "has_body",
                "Written option body",
                FieldState::boolean(dotted.has_body),
                ChangeFacet::Role,
            ),
            field(
                "options",
                "Ordered options",
                FieldState::present(FieldValue::List(
                    dotted
                        .options
                        .iter()
                        .map(|option| {
                            FieldValue::Record(BTreeMap::from([
                                ("name".into(), FieldValue::Text(option.name.clone())),
                                ("has_value".into(), FieldValue::Boolean(option.has_value)),
                                (
                                    "value_kind".into(),
                                    FieldValue::Text(value_kind(option.value_kind).into()),
                                ),
                                ("value".into(), FieldValue::Text(option.value_text.clone())),
                                (
                                    "names".into(),
                                    FieldValue::List(
                                        option
                                            .names
                                            .iter()
                                            .map(|(name, _)| {
                                                FieldValue::Text(model.name(*name).into())
                                            })
                                            .collect(),
                                    ),
                                ),
                            ]))
                        })
                        .collect(),
                )),
                ChangeFacet::Options,
            ),
        ]);
        let mut seen = BTreeSet::new();
        for option in &dotted.options {
            if dotted
                .options
                .iter()
                .filter(|other| other.name == option.name)
                .count()
                != 1
                || !seen.insert(&option.name)
            {
                continue;
            }
            let name = format!("option.{}", option.name);
            let label = option_label(&option.name);
            fact.fields.push(
                field(
                    &name,
                    label,
                    if option.has_value {
                        FieldState::text(&option.value_text)
                    } else {
                        FieldState::boolean(true)
                    },
                    if dotted.kind == DottedKind::Prior {
                        ChangeFacet::Prior
                    } else {
                        ChangeFacet::Options
                    },
                )
                .expression(),
            );
        }
        if let Some(source) = &dotted.copy_source {
            let (kind, names, subsample) = head(model, source);
            fact.fields.push(field(
                "copy_source",
                "Copy source",
                FieldState::present(FieldValue::Record(BTreeMap::from([
                    ("kind".into(), FieldValue::Text(kind.into())),
                    (
                        "names".into(),
                        FieldValue::List(names.into_iter().map(FieldValue::Text).collect()),
                    ),
                ]))),
                ChangeFacet::Target,
            ));
            fact.fields.push(field(
                "copy_source_subsample",
                "Copy source subsample",
                FieldState::optional_text(subsample.as_deref()),
                ChangeFacet::Target,
            ));
            fact.limits.push(ComparisonLimit::new("prior_copy_effective_unavailable", "The written copy source is retained; inherited prior or optimizer values are not resolved.", "semantic_priors"));
        } else {
            fact.fields.push(field(
                "copy_source",
                "Copy source",
                FieldState::absent(),
                ChangeFacet::Target,
            ));
        }
        fact.claims.push(statement.token_range.clone());
        out.push(fact);
    }
    out.sort_by_key(|fact| {
        fact.side
            .provenance
            .as_ref()
            .and_then(|provenance| provenance.parse_order)
    });
    out
}

fn estimated_fact(model: &Model, entry: &EstimatedParam, role: &str) -> Option<CapturedFact> {
    let retained = &entry.retained;
    let statement = model.statements.get(retained.statement_id)?;
    if !accepted_statement(model, statement) {
        return None;
    }
    let kind = match entry.kind {
        EstimatedParamKind::Param => "parameter",
        EstimatedParamKind::Stderr => "standard_deviation",
        EstimatedParamKind::Corr => "correlation",
        EstimatedParamKind::Skew => "skewness",
    };
    let names: Vec<_> = std::iter::once(entry.name)
        .chain(entry.corr_with)
        .map(|name| model.name(name).to_string())
        .collect();
    let mut key = vec![kind.into()];
    key.extend(names.iter().cloned());
    let label = format!("{} {}", kind, names.join(", "));
    let mut side = statement_side(model, retained.statement_id, &label, scope());
    side.occurrence = Some(retained.parse_order);
    if let Some(provenance) = &mut side.provenance {
        provenance.span = entry.span;
        provenance.parse_order = Some(retained.parse_order);
    }
    let mut fact = CapturedFact::new(SemanticFamily::Priors, role, key, side);
    fact.fields.extend([
        field(
            "target_kind",
            "Target kind",
            FieldState::text(kind),
            ChangeFacet::Target,
        ),
        field("target", "Target names", list(names), ChangeFacet::Target),
        field(
            "target_symbol_kinds",
            "Captured target symbol kinds",
            list(
                std::iter::once(entry.name)
                    .chain(entry.corr_with)
                    .map(|name| {
                        model
                            .symbol_kind_in_context(name, entry.symbol_type_context)
                            .unwrap_or("unknown")
                            .to_string()
                    }),
            ),
            ChangeFacet::Target,
        ),
        field(
            "distribution",
            "Prior distribution",
            FieldState::optional_text(retained.distribution.map(|name| model.name(name))),
            ChangeFacet::Prior,
        ),
    ]);
    if role == "estimated_params_remove_entry" {
        fact.fields.push(field(
            "removal_target_roles",
            "Captured removal roles",
            list(
                std::iter::once(entry.name_role_at_remove)
                    .chain(entry.corr_with.map(|_| entry.corr_role_at_remove))
                    .map(|role| {
                        match role {
                            EstimatedNameRole::Unknown => "unknown",
                            EstimatedNameRole::Endogenous => "endogenous",
                            EstimatedNameRole::Exogenous => "exogenous",
                            EstimatedNameRole::Parameter => "parameter",
                            EstimatedNameRole::Other => "other",
                        }
                        .into()
                    }),
            ),
            ChangeFacet::Target,
        ));
    }
    for (index, (name, label, facet)) in SLOTS.iter().enumerate() {
        let (written, value) = if !retained.positions_complete {
            (FieldState::unknown(), FieldState::unknown())
        } else if let Some(slot) = retained.slots[index] {
            let written = statement_text(model, slot.token_start..slot.token_end)
                .map(|text| FieldState::text(&text))
                .unwrap_or_else(FieldState::unknown);
            let value = if slot.token_start == slot.token_end {
                FieldState::text("")
            } else {
                FieldState::number(slot.known_value)
            };
            if slot.token_start != slot.token_end && slot.expr.is_none() {
                fact.limits.push(ComparisonLimit::new(
                    &format!("{}_evaluation_unavailable", name.replace('.', "_")),
                    &format!("{label} text retains its original slot; the existing reader retains no expression tree for evaluating this slot."),
                    "semantic_priors",
                ));
            }
            (written, value)
        } else {
            (FieldState::absent(), FieldState::absent())
        };
        fact.fields
            .push(field(name, label, written, *facet).expression());
        fact.fields.push(field(
            &format!("{name}.value"),
            &format!("{label}: known value"),
            value,
            *facet,
        ));
    }
    if retained.positions_complete {
        fact.claims.push(retained.token_start..retained.token_end);
    } else {
        fact.fields.push(
            field(
                "written_row",
                "Written estimated setting",
                statement_text(model, retained.token_start..retained.token_end)
                    .map(|text| FieldState::text(&text))
                    .unwrap_or_else(FieldState::unknown),
                ChangeFacet::Prior,
            )
            .expression(),
        );
        fact.limits.push(ComparisonLimit::new("prior_positional_fields_unavailable", "This retained row does not have an audited optional-field layout; named positional fields are unavailable and its written tokens remain available.", "semantic_priors"));
    }
    Some(fact)
}

fn head(model: &Model, head: &DottedHead) -> (&'static str, Vec<String>, Option<String>) {
    let names = |names: Vec<_>| {
        names
            .into_iter()
            .map(|name| model.name(name).to_string())
            .collect()
    };
    match head {
        DottedHead::Param { first, second } => (
            "parameter",
            names(vec![*first]),
            second.map(|name| model.name(name).into()),
        ),
        DottedHead::Std { first, second, .. } => (
            "standard_deviation",
            names(vec![*first]),
            second.map(|name| model.name(name).into()),
        ),
        DottedHead::Corr {
            first,
            second,
            third,
            ..
        } => (
            "correlation",
            names(vec![*first, *second]),
            third.map(|name| model.name(name).into()),
        ),
        DottedHead::Vec { names: targets } => (
            "joint_parameters",
            names(targets.iter().map(|(name, _)| *name).collect()),
            None,
        ),
    }
}

fn value_kind(kind: FamilyValueKind) -> &'static str {
    match kind {
        FamilyValueKind::Flag => "flag",
        FamilyValueKind::Scalar => "scalar",
        FamilyValueKind::NameList => "name_list",
        FamilyValueKind::Vector => "vector",
        FamilyValueKind::Matrix => "matrix",
        FamilyValueKind::Date => "date",
        FamilyValueKind::Range => "range",
    }
}

fn option_label(name: &str) -> &str {
    match name {
        "mean" => "Prior mean",
        "stdev" => "Prior standard deviation",
        "shape" => "Prior distribution",
        "domain" => "Prior support",
        "variance" => "Prior variance",
        "mode" => "Prior mode",
        "interval" => "Prior interval",
        "truncate" => "Prior truncation",
        "bounds" => "Optimizer bounds",
        "init" => "Optimizer initial value",
        "jscale" => "Proposal scale",
        _ => name,
    }
}
