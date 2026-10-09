//! Retained family facts, with conservative expanded-occurrence proof.
//! This view never reads new syntax or infers a solver result.

mod advanced;
mod data;
mod declarations;
mod moments;
mod operations;

use std::collections::{BTreeMap, BTreeSet};

use super::expression_values::{
    expression, gap, parent_id, range_text, span_text, RetainedExpressionWork,
};
use crate::intern::Name;
use crate::model::*;
use crate::model_diff::ModelDiff;
use crate::span::Span;

use super::occurrences::{self, CapturedFact, FactField, TokenClaims};
use super::*;

pub(super) fn populate(
    before: &Model,
    after: &Model,
    diff: &mut ModelDiff,
    claims: &mut TokenClaims,
) {
    let owned = declarations::enrich_primary(before, after, diff, claims);
    claim_surgery_sources(before, Side::Before, claims);
    claim_surgery_sources(after, Side::After, claims);
    let old = collect(before, &mut claims.expression_work, &owned[0]);
    let new = collect(after, &mut claims.expression_work, &owned[1]);
    coverage(&old, &new, diff);
    occurrences::compare_facts(before, after, old, new, diff, claims);
    finalize_option_claims(before, Side::Before, claims);
    finalize_option_claims(after, Side::After, claims);
}

fn claim_surgery_sources(model: &Model, side: Side, claims: &mut TokenClaims) {
    let mut written = BTreeMap::new();
    for equation in &model.written_equations {
        written
            .entry(equation.equation.parse_order)
            .and_modify(|value| *value = None)
            .or_insert(Some(equation));
    }
    for removed in model
        .equation_surgery
        .iter()
        .flat_map(|operation| &operation.removed)
    {
        if let Some(Some(written)) = written.get(&removed.equation.parse_order) {
            if written.equation.active_tokens == removed.equation.active_tokens {
                if let Some(statement) = model.statements.get(written.statement_id) {
                    if statement.token_range.start <= written.token_range.start
                        && written.token_range.end <= statement.token_range.end
                    {
                        claims.claim(side, written.statement_id, written.token_range.clone());
                    }
                }
            }
        }
    }
}

fn finalize_option_claims(model: &Model, side: Side, claims: &mut TokenClaims) {
    let mut lists = BTreeSet::new();
    for receipt in model.setting_receipts.values() {
        if let Some(id) = parent_at_order(model, receipt.tokens.start) {
            if let Some(list) = &receipt.option_list {
                lists.insert((list.start, list.end));
            }
            // This selected receipt names an implemented setting field.
            claims.claim(side, id, receipt.tokens.clone());
        }
    }
    lists.extend(
        model
            .stoch_simul_requests
            .iter()
            .filter_map(|request| request.option_tokens.as_ref())
            .map(|range| (range.start, range.end)),
    );
    lists.extend(
        model
            .irf_shocks_options
            .iter()
            .map(|option| (option.option_tokens.start, option.option_tokens.end)),
    );
    lists.extend(
        model
            .policy_command_statements
            .iter()
            .filter_map(|command| command.option_tokens.as_ref())
            .map(|range| (range.start, range.end)),
    );
    for (start, end) in lists {
        if end <= start + 1 || model.expanded_tokens.get(start..end).is_none() {
            continue;
        }
        if let Some(id) = parent_at_order(model, start) {
            if (start + 1..end - 1).all(|index| {
                model.expanded_tokens[index].kind == crate::lexer::TokenKind::Comma
                    || claims.contains(side, id, index)
            }) {
                claims.claim(side, id, start..end);
            }
        }
    }
}

fn collect(
    model: &Model,
    work: &mut RetainedExpressionWork,
    owned_dimensions: &BTreeSet<usize>,
) -> Vec<CapturedFact> {
    let mut facts = Vec::new();
    declarations::collect(model, &mut facts, owned_dimensions);
    data::collect(model, &mut facts);
    advanced::collect(model, &mut facts, work);
    moments::collect(model, &mut facts, work);
    operations::collect(model, &mut facts, work);
    facts
}

fn fact(
    model: &Model,
    family: SemanticFamily,
    role: &str,
    label: &str,
    key: Vec<String>,
    order: Option<usize>,
    ordinal: usize,
) -> CapturedFact {
    let receipt = model
        .fact_receipts
        .get(role)
        .and_then(|receipts| receipts.get(ordinal));
    let order = order.or_else(|| receipt.map(|receipt| receipt.parse_order));
    let parent = order.and_then(|order| parent_at_order(model, order));
    let mut side = parent
        .map(|id| {
            let statement = &model.statements[id];
            let scope = ComparisonScope {
                domain: if statement.dimension.is_some() {
                    "heterogeneous"
                } else {
                    "aggregate"
                }
                .into(),
                dimension: statement.dimension.clone(),
                block: (statement.kind == StatementKind::Block).then(|| statement.name.clone()),
            };
            occurrences::statement_side(model, id, label, scope)
        })
        .unwrap_or_else(|| RowSide::named(label, ComparisonScope::global()));
    side.occurrence = Some(ordinal);
    let mut fact = CapturedFact::new(family, role, key, side);
    if let Some(receipt) = receipt {
        fact.claims.extend(receipt.claims.iter().cloned());
    }
    if parent.is_none() {
        gap(&mut fact, "producer_occurrence_context", "This retained producer has no proven expanded parent. Written spans do not prove command context, cross-side correspondence or navigation.", "parser_occurrence_retention");
    }
    fact
}

fn parent_at_order(model: &Model, order: usize) -> Option<usize> {
    occurrences::parent_at_order(model, order)
}

fn full_claim(model: &Model, fact: &mut CapturedFact) {
    if let Some(statement) = parent_id(fact).and_then(|id| model.statements.get(id)) {
        fact.claims.push(statement.token_range.clone());
    }
}

fn claim_opener(
    model: &Model,
    fact: &mut CapturedFact,
    range: &std::ops::Range<usize>,
    words: &[&str],
) {
    use crate::lexer::TokenKind;
    for index in range.clone() {
        if let Some(token) = model.expanded_tokens.get(index) {
            if matches!(
                token.kind,
                TokenKind::LParen | TokenKind::RParen | TokenKind::Comma | TokenKind::Semi
            ) || words
                .iter()
                .any(|word| token.text(&model.source).eq_ignore_ascii_case(word))
            {
                fact.claims.push(index..index + 1);
            }
        }
    }
}

/// A selected final setting has global identity and a directly proven origin.
fn attach_setting_context(model: &Model, fact: &mut CapturedFact, order: usize) {
    if let Some(id) = parent_at_order(model, order) {
        let side = occurrences::statement_side(model, id, &fact.side.name, fact.side.scope.clone());
        fact.side.context = side.context;
        fact.side.provenance = side.provenance;
    } else {
        gap(fact, "setting_origin_unavailable", "The retained setting has no directly proven expanded parent; its semantic value remains available.", "parser_setting_origin_retention");
    }
}

/// Claims use direct option-reader ranges. Independent options remain context.
fn claim_options(
    model: &Model,
    fact: &mut CapturedFact,
    list: std::ops::Range<usize>,
    owned: &[std::ops::Range<usize>],
) {
    use crate::lexer::TokenKind;
    if list.end <= list.start + 1 || model.expanded_tokens.get(list.clone()).is_none() {
        return;
    }
    let interior = list.start + 1..list.end - 1;
    let complete = interior.clone().all(|index| {
        model.expanded_tokens[index].kind == TokenKind::Comma
            || owned.iter().any(|range| range.contains(&index))
    });
    if complete && !owned.is_empty() {
        fact.claims.push(list);
        return;
    }
    for range in owned {
        if interior.start <= range.start && range.end <= interior.end {
            let mut range = range.clone();
            if range.start > interior.start
                && model.expanded_tokens[range.start - 1].kind == TokenKind::Comma
            {
                range.start -= 1;
            } else if range.end < interior.end
                && model.expanded_tokens[range.end].kind == TokenKind::Comma
            {
                range.end += 1;
            }
            fact.claims.push(range);
        }
    }
}

fn field(fact: &mut CapturedFact, name: &str, value: FieldState, facet: ChangeFacet) {
    let field = FactField::new(name, &label(name), value, facet);
    fact.fields.push(if budget_unavailable(fact, name) {
        field.unavailable()
    } else {
        field
    });
}

fn text(fact: &mut CapturedFact, name: &str, value: &str, facet: ChangeFacet) {
    field(fact, name, FieldState::text(value), facet);
}

fn expr(fact: &mut CapturedFact, name: &str, value: FieldState) {
    let field = FactField::new(name, &label(name), value, ChangeFacet::Expression).expression();
    fact.fields.push(if budget_unavailable(fact, name) {
        field.unavailable()
    } else {
        field
    });
}

fn label(name: &str) -> String {
    let text = name.replace('_', " ");
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => text,
    }
}

fn budget_unavailable(fact: &CapturedFact, name: &str) -> bool {
    fact.limits.iter().any(|limit| {
        limit.code.starts_with(&format!("{name}_"))
            && [
                "_tree_node_limit",
                "_tree_materialization_limit",
                "_selector_visit_limit",
                "_selector_token_limit",
                "_text_materialization_limit",
            ]
            .iter()
            .any(|suffix| limit.code.ends_with(suffix))
    })
}

fn record(entries: impl IntoIterator<Item = (&'static str, FieldValue)>) -> FieldValue {
    FieldValue::Record(
        entries
            .into_iter()
            .map(|(key, value)| (key.into(), value))
            .collect(),
    )
}

/// Nested fields carry the same absent/empty/unknown/present distinction.
fn state_value(state: FieldState) -> FieldValue {
    let mut entries = BTreeMap::new();
    entries.insert(
        "state".into(),
        FieldValue::Text(
            match state.state {
                ValueState::Absent => "absent",
                ValueState::Empty => "empty",
                ValueState::Unknown => "unknown",
                ValueState::Present => "present",
            }
            .into(),
        ),
    );
    if let Some(value) = state.value {
        entries.insert("value".into(), value);
    }
    FieldValue::Record(entries)
}

fn names(model: &Model, values: impl IntoIterator<Item = Name>) -> FieldValue {
    FieldValue::List(
        values
            .into_iter()
            .map(|name| FieldValue::Text(model.name(name).into()))
            .collect(),
    )
}

fn optional_name(model: &Model, name: Option<Name>) -> FieldState {
    name.map(|name| FieldState::text(model.name(name)))
        .unwrap_or_else(FieldState::absent)
}

fn optional_integer(value: Option<i32>) -> FieldState {
    value
        .map(|value| FieldState::present(FieldValue::Integer(value.into())))
        .unwrap_or_else(FieldState::absent)
}

fn span_list(
    model: &Model,
    fact: &mut CapturedFact,
    name: &str,
    spans: &[Span],
    work: &mut RetainedExpressionWork,
) -> FieldState {
    let mut values = Vec::new();
    for span in spans {
        let value = span_text(model, fact, name, *span, work);
        let Some(value) = value.value else {
            return FieldState::unknown();
        };
        values.push(value);
    }
    FieldState::present(FieldValue::List(values))
}

fn options(model: &Model, options: &[FamilyOption]) -> FieldValue {
    FieldValue::List(
        options
            .iter()
            .map(|option| {
                record([
                    ("name", FieldValue::Text(option.name.clone())),
                    ("has_value", FieldValue::Boolean(option.has_value)),
                    (
                        "shape",
                        FieldValue::Text(
                            match option.value_kind {
                                FamilyValueKind::Flag => "flag",
                                FamilyValueKind::Scalar => "scalar",
                                FamilyValueKind::NameList => "name_list",
                                FamilyValueKind::Vector => "vector",
                                FamilyValueKind::Matrix => "matrix",
                                FamilyValueKind::Date => "date",
                                FamilyValueKind::Range => "range",
                            }
                            .into(),
                        ),
                    ),
                    ("text", FieldValue::Text(option.value_text.clone())),
                    (
                        "names",
                        names(model, option.names.iter().map(|(name, _)| *name)),
                    ),
                ])
            })
            .collect(),
    )
}

fn coverage(old: &[CapturedFact], new: &[CapturedFact], diff: &mut ModelDiff) {
    let families = [
        SemanticFamily::Symbols,
        SemanticFamily::Observables,
        SemanticFamily::Data,
        SemanticFamily::Occbin,
        SemanticFamily::Policy,
        SemanticFamily::SemiStructural,
        SemanticFamily::Moments,
        SemanticFamily::MsSbvar,
        SemanticFamily::Heterogeneity,
        SemanticFamily::ExternalFunctions,
        SemanticFamily::Trends,
        SemanticFamily::Operations,
        SemanticFamily::MacroContext,
    ];
    for family in families {
        let mut fields = BTreeSet::new();
        let mut limits = Vec::new();
        for fact in old.iter().chain(new).filter(|fact| fact.family == family) {
            fields.extend(fact.fields.iter().map(|field| field.name.clone()));
            for limit in &fact.limits {
                if !limits.contains(limit) {
                    limits.push(limit.clone());
                }
            }
        }
        // No syntax added here: the coverage describes retained facts, not all
        // official grammar, referenced data, or external function bodies.
        let availability = if limits.is_empty() {
            Availability::Complete
        } else {
            Availability::Partial
        };
        if let Some(existing) = diff
            .coverage
            .families
            .iter_mut()
            .find(|entry| entry.family == family)
        {
            existing.fields.extend(fields);
            existing.fields.sort();
            existing.fields.dedup();
            for limit in limits {
                if !existing.limits.contains(&limit) {
                    existing.limits.push(limit);
                }
            }
            if availability == Availability::Partial
                && existing.availability != Availability::LimitExceeded
            {
                existing.availability = Availability::Partial;
            }
        } else {
            diff.coverage.families.push(FamilyCoverage {
                family,
                availability,
                fields: fields.into_iter().collect(),
                limits,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_retained_fact_is_not_an_edit_after_asymmetric_budget_exhaustion() {
        let model = crate::parser::parse("var y; planner_objective y;");
        let mut claims = TokenClaims::with_expression_budget(400);
        let mut diff = crate::model_diff::compare_models(&model, &model);
        diff.semantic.rows.clear();
        populate(&model, &model, &mut diff, &mut claims);
        assert!(diff.semantic.rows.is_empty(), "{}", diff.to_json());
        assert!(diff
            .coverage
            .families
            .iter()
            .flat_map(|family| &family.limits)
            .any(|limit| limit.code == "planner_objective_tree_materialization_limit"));
    }
}
