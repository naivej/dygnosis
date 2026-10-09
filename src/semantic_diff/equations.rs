//! Written token detail, timing and direct references over existing equation
//! pairing. Display offsets never supply a navigation location.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use crate::equations::{self as counted, EquationRow, IdentClass};
use crate::expr::ExprKind;
use crate::lexer::{tokenize, TokenKind};
use crate::model::{Equation, Model};
use crate::model_diff::{normalize_equation, ModelDiff};
use crate::model_locals::ModelLocals;

use super::*;

/// Populate the optional equation detail before final output limits run.
pub fn populate(before: &Model, after: &Model, diff: &mut ModelDiff) {
    let old = catalog(before);
    let new = catalog(after);
    let old_locals = BindingFacts::new(before, &ModelLocals::collect(before));
    let new_locals = BindingFacts::new(after, &ModelLocals::collect(after));
    populate_condition_rows(&old, &new, diff);
    populate_rows(before, after, &old, &new, &old_locals, &new_locals, diff);
    populate_references(before, after, &old, &new, &old_locals, &new_locals, diff);
    let unmatched_conditions = old
        .values()
        .chain(new.values())
        .filter(|equation| {
            equation
                .equation
                .complementarity
                .as_ref()
                .is_some_and(|condition| condition.matched.is_none())
        })
        .count();
    if let Some(coverage) = diff
        .coverage
        .families
        .iter_mut()
        .find(|coverage| coverage.family == SemanticFamily::Equations)
    {
        coverage.fields.extend(
            [
                "tokens",
                "written_timing",
                "converted_timing",
                "direct_references",
                "complementarity.text",
                "complementarity.variable",
                "complementarity.lower_bound",
                "complementarity.upper_bound",
            ]
            .map(str::to_string),
        );
        coverage
            .limits
            .retain(|limit| limit.code != "equation_detail_pending");
        let limited = diff
            .semantic
            .rows
            .iter()
            .filter(|row| row.family == SemanticFamily::Equations)
            .flat_map(|row| &row.expressions)
            .filter(|expression| expression.availability == Availability::LimitExceeded)
            .count();
        coverage.availability = if limited == 0 {
            Availability::Complete
        } else {
            Availability::Partial
        };
        if limited > 0 {
            let mut limit = ComparisonLimit::new(
                "token_alignment_limit",
                "Equation highlights are unavailable for some expressions; exact text is retained.",
                "semantic_equations",
            );
            limit.omitted = Some(limited);
            coverage.limits.push(limit.clone());
            diff.semantic.limits.push(limit.clone());
            diff.coverage.limits.push(limit);
            diff.semantic.availability = Availability::Partial;
            diff.coverage.availability = Availability::Partial;
        }
        if unmatched_conditions > 0 {
            let mut limit = ComparisonLimit::new(
                "complementarity_unmatched",
                "Retained complementarity variable, lower_bound and upper_bound fields are unavailable for unmatched conditions; exact text remains available. The count is affected side occurrences.",
                "semantic_equations",
            );
            limit.omitted = Some(unmatched_conditions);
            coverage.limits.push(limit.clone());
            diff.semantic.limits.push(limit.clone());
            diff.coverage.limits.push(limit);
            coverage.availability = Availability::Partial;
            diff.semantic.availability = Availability::Partial;
            diff.coverage.availability = Availability::Partial;
        }
        for code in [
            "condition_correspondence_unpaired",
            "timing_correspondence_limit",
            "identifier_correspondence_unpaired",
            "equation_correspondence_unpaired",
        ] {
            let limits: Vec<_> = diff
                .semantic
                .rows
                .iter()
                .filter(|row| row.family == SemanticFamily::Equations)
                .flat_map(|row| &row.limits)
                .filter(|limit| limit.code == code)
                .collect();
            if let Some(first) = limits.first() {
                let mut limit = (*first).clone();
                limit.omitted = Some(limits.len());
                coverage.limits.push(limit.clone());
                diff.semantic.limits.push(limit.clone());
                diff.coverage.limits.push(limit);
                coverage.availability = Availability::Partial;
                diff.semantic.availability = Availability::Partial;
                diff.coverage.availability = Availability::Partial;
            }
        }
    }
}

struct CapturedEquation<'a> {
    row: EquationRow,
    equation: &'a Equation,
    dimension: Option<crate::intern::Name>,
    dimension_name: Option<String>,
    written_id: Option<usize>,
    statement_id: Option<usize>,
}

type EquationKey = (Option<String>, usize);
type Catalog<'a> = BTreeMap<EquationKey, CapturedEquation<'a>>;

fn catalog(model: &Model) -> Catalog<'_> {
    let mut out = BTreeMap::new();
    for (row, equation) in counted::equations(model).into_iter().zip(
        model
            .equations
            .iter()
            .filter(|equation| !equation.is_local && !equation.static_tag),
    ) {
        out.insert((None, row.index), captured(model, row, equation, None));
    }
    let blocks = counted::heterogeneous_equations(model);
    for block in blocks {
        let source = &model.heterogeneous_models[block.block_index];
        for (row, equation) in block.equations.into_iter().zip(
            source
                .equations
                .iter()
                .filter(|equation| !equation.is_local && !equation.static_tag),
        ) {
            out.insert(
                (Some(block.dimension.clone()), row.index),
                captured(model, row, equation, Some(source.dimension)),
            );
        }
    }
    out
}

fn captured<'a>(
    model: &'a Model,
    row: EquationRow,
    equation: &'a Equation,
    dimension: Option<crate::intern::Name>,
) -> CapturedEquation<'a> {
    let written_id = model.written_equations.iter().position(|written| {
        written.dimension == dimension && written.equation.parse_order == equation.parse_order
    });
    CapturedEquation {
        row,
        equation,
        dimension,
        dimension_name: dimension.map(|dimension| model.name(dimension).into()),
        written_id,
        statement_id: written_id.map(|index| model.written_equations[index].statement_id),
    }
}

fn provenance(equation: &CapturedEquation<'_>) -> OccurrenceProvenance {
    OccurrenceProvenance {
        span: equation.equation.span,
        parse_order: Some(equation.equation.parse_order),
        equation_id: equation.written_id,
        statement_id: equation.statement_id,
    }
}

type ConditionKey = Option<(String, Option<(String, Option<String>, Option<String>)>)>;
type BodyKey = (Option<String>, String, BTreeMap<String, String>);

fn condition_key(equation: &CapturedEquation<'_>) -> ConditionKey {
    equation.equation.complementarity.as_ref().map(|condition| {
        (
            condition.text.clone(),
            condition.matched.as_ref().map(|matched| {
                (
                    matched.variable.clone(),
                    matched.lower_bound.clone(),
                    matched.upper_bound.clone(),
                )
            }),
        )
    })
}

fn condition_row_side(equation: &CapturedEquation<'_>) -> RowSide {
    let mut side = RowSide::named(
        &equation_label(equation),
        ComparisonScope {
            domain: if equation.dimension.is_some() {
                "heterogeneous"
            } else {
                "aggregate"
            }
            .into(),
            dimension: equation.dimension_name.clone(),
            block: None,
        },
    );
    side.equation_index = Some(equation.row.index);
    side
}

fn equation_label(equation: &CapturedEquation<'_>) -> String {
    if equation.row.name.is_empty() {
        format!("Equation {}", equation.row.index + 1)
    } else {
        equation.row.name.clone()
    }
}

fn push_condition_row(
    before: Option<&CapturedEquation<'_>>,
    after: Option<&CapturedEquation<'_>>,
    unpaired: bool,
    diff: &mut ModelDiff,
) {
    let mut row = SemanticRow::new(
        SemanticFamily::Equations,
        if unpaired {
            ChangeKind::Unpaired
        } else {
            ChangeKind::Changed
        },
        &equation_label(after.or(before).expect("one condition side")),
    );
    row.before = before.map(condition_row_side);
    row.after = after.map(condition_row_side);
    row.fields.push(FieldChange::new(
        "expression",
        "Expression",
        before
            .map(|equation| FieldState::text(&equation.row.text))
            .unwrap_or_else(FieldState::absent),
        after
            .map(|equation| FieldState::text(&equation.row.text))
            .unwrap_or_else(FieldState::absent),
    ));
    if unpaired {
        row.count_unit = CountUnit::AcceptedOccurrence;
        row.limits.push(ComparisonLimit::new(
            "condition_correspondence_unpaired",
            "Repeated equation candidates have no proven condition correspondence; each condition remains a separate side fact.",
            "semantic_equations",
        ));
    }
    diff.semantic.push_row(row);
}

/// Supplement legacy rows only for retained conditions that legacy body/tag
/// comparison does not inspect. Unique names have the existing engine meaning;
/// other unchanged bodies pair only when their exact scoped candidate is unique.
/// Cancelling equal repeated facts establishes no cross-side occurrence pair.
fn populate_condition_rows(old: &Catalog<'_>, new: &Catalog<'_>, diff: &mut ModelDiff) {
    let mut used_old = BTreeSet::new();
    let mut used_new = BTreeSet::new();
    for row in diff
        .semantic
        .rows
        .iter()
        .filter(|row| row.family == SemanticFamily::Equations)
    {
        used_old.extend(row.before.as_ref().and_then(key));
        used_new.extend(row.after.as_ref().and_then(key));
    }
    let mut names: BTreeMap<(Option<String>, String), [Vec<EquationKey>; 2]> = BTreeMap::new();
    for (catalog, side) in [(old, 0), (new, 1)] {
        for (key, equation) in catalog {
            if !equation.row.name.is_empty() {
                names
                    .entry((key.0.clone(), equation.row.name.clone()))
                    .or_default()[side]
                    .push(key.clone());
            }
        }
    }
    for candidates in names.values() {
        if let ([before], [after]) = (&candidates[0][..], &candidates[1][..]) {
            if used_old.contains(before) || used_new.contains(after) {
                continue;
            }
            if condition_key(&old[before]) != condition_key(&new[after]) {
                push_condition_row(Some(&old[before]), Some(&new[after]), false, diff);
            }
            used_old.insert(before.clone());
            used_new.insert(after.clone());
        }
    }
    let mut bodies: BTreeMap<BodyKey, [Vec<EquationKey>; 2]> = BTreeMap::new();
    let mut body_counts: BTreeMap<BodyKey, [usize; 2]> = BTreeMap::new();
    for (catalog, used, side) in [(old, &used_old, 0), (new, &used_new, 1)] {
        for (key, equation) in catalog {
            let body = (
                key.0.clone(),
                normalize_equation(&equation.row.text),
                equation.row.tags.clone(),
            );
            body_counts.entry(body.clone()).or_default()[side] += 1;
            if !used.contains(key) {
                bodies.entry(body).or_default()[side].push(key.clone());
            }
        }
    }
    for (body, candidates) in &bodies {
        if body_counts[body] == [1, 1]
            && let ([before], [after]) = (&candidates[0][..], &candidates[1][..])
        {
            if condition_key(&old[before]) != condition_key(&new[after]) {
                push_condition_row(Some(&old[before]), Some(&new[after]), false, diff);
            }
            continue;
        }
        let mut unchanged: BTreeMap<ConditionKey, VecDeque<&EquationKey>> = BTreeMap::new();
        for after in &candidates[1] {
            unchanged
                .entry(condition_key(&new[after]))
                .or_default()
                .push_back(after);
        }
        let mut remaining_old = Vec::new();
        let mut cancelled_new = BTreeSet::new();
        for before in &candidates[0] {
            if let Some(after) = unchanged
                .get_mut(&condition_key(&old[before]))
                .and_then(VecDeque::pop_front)
            {
                cancelled_new.insert(after);
            } else {
                remaining_old.push(before);
            }
        }
        for before in remaining_old {
            push_condition_row(Some(&old[before]), None, true, diff);
        }
        for after in &candidates[1] {
            if !cancelled_new.contains(after) {
                push_condition_row(None, Some(&new[after]), true, diff);
            }
        }
    }
}

fn key(side: &RowSide) -> Option<EquationKey> {
    Some((side.scope.dimension.clone(), side.equation_index?))
}

fn attach_provenance(side: &mut Option<RowSide>, catalog: &Catalog<'_>) {
    let Some(side) = side else {
        return;
    };
    let Some(equation) = key(side).and_then(|key| catalog.get(&key)) else {
        return;
    };
    side.occurrence = Some(equation.equation.parse_order);
    side.provenance = Some(provenance(equation));
}

fn text(field: &FieldState) -> Option<&str> {
    match field.value.as_ref()? {
        FieldValue::Text(text) => Some(text),
        _ => None,
    }
}

#[derive(Clone)]
struct LexToken {
    start: usize,
    end: usize,
    key: String,
    kind: TokenKind,
}

fn tokens(text: &str) -> Vec<LexToken> {
    tokenize(text)
        .into_iter()
        .filter(|token| token.kind != TokenKind::Eof)
        .map(|token| LexToken {
            start: token.span.start as usize,
            end: token.span.end as usize,
            key: token.text(text).into(),
            kind: token.kind,
        })
        .collect()
}

fn append_run(runs: &mut Vec<TokenRun>, text: &str, role: TokenRole) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = runs.last_mut().filter(|last| last.role == role) {
        last.text.push_str(text);
    } else {
        runs.push(TokenRun {
            text: text.into(),
            role,
        });
    }
}

fn runs(text: &str, tokens: &[LexToken], roles: &[TokenRole]) -> ExpressionSide {
    let mut runs = Vec::new();
    let mut position = 0;
    for (index, token) in tokens.iter().enumerate() {
        let gap_role = if index > 0 && roles[index - 1] == roles[index] {
            roles[index]
        } else {
            TokenRole::Unchanged
        };
        append_run(&mut runs, &text[position..token.start], gap_role);
        append_run(&mut runs, &text[token.start..token.end], roles[index]);
        position = token.end;
    }
    append_run(&mut runs, &text[position..], TokenRole::Unchanged);
    ExpressionSide {
        text: text.into(),
        runs,
    }
}

fn plain(
    field: &str,
    before: Option<&str>,
    after: Option<&str>,
    availability: Availability,
    reason: Option<&str>,
) -> ExpressionDetail {
    ExpressionDetail {
        field: field.into(),
        before: before.map(ExpressionSide::plain),
        after: after.map(ExpressionSide::plain),
        highlight_basis: HighlightBasis::None,
        availability,
        reason: reason.map(str::to_string),
    }
}

/// Shared written-token comparison for downstream retained expression fields.
/// Lexing reuses the existing lexer; this does not read a second syntax tree.
/// Matrix allocation starts only after the comparison-wide work charge passes.
pub fn expression_detail(
    semantic: &mut SemanticDiff,
    field: &str,
    before: Option<&str>,
    after: Option<&str>,
) -> ExpressionDetail {
    if before == after {
        return plain(field, before, after, Availability::Complete, None);
    }
    let old = before.map(tokens).unwrap_or_default();
    let new = after.map(tokens).unwrap_or_default();
    let old_size = old.len().saturating_add(1);
    let new_size = new.len().saturating_add(1);
    if semantic.charge_token_alignment(old_size, new_size) != Availability::Complete {
        return plain(
            field,
            before,
            after,
            Availability::LimitExceeded,
            Some("Token alignment limit; exact expression text is retained without highlights."),
        );
    }
    let mut old_roles = vec![TokenRole::Removed; old.len()];
    let mut new_roles = vec![TokenRole::Added; new.len()];
    let mut matrix = vec![0_u32; old_size * new_size];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            matrix[i * new_size + j] = if old[i].key == new[j].key {
                1 + matrix[(i + 1) * new_size + j + 1]
            } else {
                matrix[(i + 1) * new_size + j].max(matrix[i * new_size + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < old.len() && j < new.len() {
        if old[i].key == new[j].key {
            old_roles[i] = TokenRole::Unchanged;
            new_roles[j] = TokenRole::Unchanged;
            i += 1;
            j += 1;
        } else if matrix[(i + 1) * new_size + j] >= matrix[i * new_size + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    ExpressionDetail {
        field: field.into(),
        before: before.map(|text| runs(text, &old, &old_roles)),
        after: after.map(|text| runs(text, &new, &new_roles)),
        highlight_basis: HighlightBasis::PairedExpression,
        availability: Availability::Complete,
        reason: None,
    }
}

struct CandidateTokens {
    lexemes: BTreeSet<String>,
    pairs: BTreeSet<(String, String)>,
}

fn candidate_tokens(candidates: &[&str]) -> CandidateTokens {
    let mut lexemes = BTreeSet::new();
    let mut pairs = BTreeSet::new();
    for text in candidates {
        let tokens = tokens(text);
        for token in &tokens {
            lexemes.insert(token.key.clone());
        }
        for pair in tokens.windows(2) {
            pairs.insert((pair[0].key.clone(), pair[1].key.clone()));
        }
    }
    CandidateTokens { lexemes, pairs }
}

fn is_operator(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Eq
            | TokenKind::EqEq
            | TokenKind::Ne
            | TokenKind::Lt
            | TokenKind::Gt
            | TokenKind::Le
            | TokenKind::Ge
            | TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Star
            | TokenKind::Slash
            | TokenKind::Caret
            | TokenKind::Comma
    )
}

fn unpaired_runs(text: &str, opposite: &CandidateTokens, role: TokenRole) -> ExpressionSide {
    let tokens = tokens(text);
    let mut roles: Vec<_> = tokens
        .iter()
        .map(|token| {
            if opposite.lexemes.contains(&token.key) {
                TokenRole::Unchanged
            } else {
                role
            }
        })
        .collect();
    for (index, pair) in tokens.windows(2).enumerate() {
        if (is_operator(pair[0].kind) || is_operator(pair[1].kind))
            && !opposite
                .pairs
                .contains(&(pair[0].key.clone(), pair[1].key.clone()))
        {
            roles[index] = role;
            roles[index + 1] = role;
        }
    }
    runs(text, &tokens, &roles)
}

fn unpaired_detail(
    semantic: &mut SemanticDiff,
    before: Option<&str>,
    after: Option<&str>,
    before_candidates: &[&str],
    after_candidates: &[&str],
) -> ExpressionDetail {
    let total_bytes = before_candidates
        .iter()
        .chain(after_candidates)
        .fold(0_usize, |sum, text| sum.saturating_add(text.len()));
    // A linear candidate-set pass has a conservative one-cell-per-byte charge.
    if semantic.charge_token_alignment(1, total_bytes.max(1)) != Availability::Complete {
        return plain("expression", before, after, Availability::LimitExceeded, Some("Unpaired text highlight limit; exact candidate text is retained without highlights."));
    }
    let old = candidate_tokens(before_candidates);
    let new = candidate_tokens(after_candidates);
    ExpressionDetail {
        field: "expression".into(),
        before: before.map(|text| unpaired_runs(text, &new, TokenRole::Removed)),
        after: after.map(|text| unpaired_runs(text, &old, TokenRole::Added)),
        highlight_basis: HighlightBasis::UnpairedTextOnly,
        availability: Availability::Complete,
        reason: None,
    }
}

fn candidate_group<'a>(
    diff: &'a ModelDiff,
    side: Option<&RowSide>,
) -> Option<(Vec<&'a str>, Vec<&'a str>)> {
    let side = side?;
    let index = side.equation_index?;
    let groups = if let Some(dimension) = &side.scope.dimension {
        &diff
            .heterogeneous_equations
            .iter()
            .find(|group| &group.dimension == dimension)?
            .unmatched_same_name
    } else {
        &diff.unmatched_same_name
    };
    let group = groups.iter().find(|group| {
        group.name == side.name
            && group
                .removed
                .iter()
                .chain(&group.added)
                .any(|equation| equation.index == index)
    })?;
    Some((
        group
            .removed
            .iter()
            .map(|equation| equation.text.as_str())
            .collect(),
        group
            .added
            .iter()
            .map(|equation| equation.text.as_str())
            .collect(),
    ))
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum BindingKey {
    Global(String, String, Option<String>),
    Local(String, Option<String>),
    Unknown(usize),
}

struct UseFact {
    side: TimingSide,
    binding: BindingKey,
}

struct BindingFacts {
    dimensions: HashMap<String, Option<String>>,
    locals: HashMap<(String, Option<String>), bool>,
}

impl BindingFacts {
    fn new(model: &Model, locals: &ModelLocals) -> Self {
        let dimensions = model
            .final_decls(&["var", "varexo", "varexo_det", "parameters"])
            .into_iter()
            .map(|decl| {
                (
                    model.name(decl.name).into(),
                    model
                        .final_heterogeneity(decl)
                        .map(|dimension| model.name(dimension).into()),
                )
            })
            .collect();
        let mut definitions = HashMap::new();
        let mut declarations = HashMap::new();
        for definition in &locals.definitions {
            *definitions
                .entry((
                    model.name(definition.name).to_string(),
                    definition
                        .dimension
                        .map(|dimension| model.name(dimension).to_string()),
                ))
                .or_insert(0_usize) += 1;
        }
        for declaration in &locals.declarations {
            *declarations
                .entry(model.name(declaration.name).to_string())
                .or_insert(0_usize) += 1;
        }
        let mut local_bindings = HashMap::new();
        for ((name, dimension), count) in definitions {
            local_bindings.insert(
                (name.clone(), dimension),
                count == 1 && declarations.get(&name).copied().unwrap_or(0) <= 1,
            );
        }
        Self {
            dimensions,
            locals: local_bindings,
        }
    }
}

fn uses(model: &Model, equation: &CapturedEquation<'_>, bindings: &BindingFacts) -> Vec<UseFact> {
    equation
        .row
        .idents
        .iter()
        .enumerate()
        .map(|(occurrence, ident)| {
            let binding = if ident.class == IdentClass::ModelLocal {
                let key = (
                    ident.name.clone(),
                    equation
                        .dimension
                        .map(|dimension| model.name(dimension).into()),
                );
                if bindings.locals.get(&key).copied().unwrap_or(false) {
                    BindingKey::Local(key.0, key.1)
                } else {
                    BindingKey::Unknown(occurrence)
                }
            } else if ident.class == IdentClass::Undeclared {
                BindingKey::Unknown(occurrence)
            } else {
                let dimension = bindings.dimensions.get(&ident.name).cloned().flatten();
                BindingKey::Global(ident.name.clone(), ident.class.as_str().into(), dimension)
            };
            UseFact {
                side: TimingSide {
                    name: ident.name.clone(),
                    class: ident.class.as_str().into(),
                    written_offset: ident.timing,
                    converted_offset: ident.dynare_timing,
                    occurrence,
                },
                binding,
            }
        })
        .collect()
}

fn equation_shape(model: &Model, equation: &Equation) -> Option<Vec<String>> {
    let bound = equation.active_tokens.len().checked_add(2)?;
    if equation.active_tokens.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for root in [equation.lhs_expr, equation.rhs_expr] {
        let mut stack = vec![root];
        while let Some(expr) = stack.pop() {
            if out.len() >= bound || stack.len() >= bound {
                return None;
            }
            let Some(expr) = expr else {
                out.push("absent".into());
                continue;
            };
            let node = model.exprs.get(expr);
            let shape = match &node.kind {
                ExprKind::Ident { name, .. } => format!("ident:{}", model.name(*name)),
                ExprKind::Number => {
                    let literal = model
                        .numeric_literal_texts
                        .get(&expr)
                        .cloned()
                        .or_else(|| {
                            model
                                .source
                                .get(node.span.start as usize..node.span.end as usize)
                                .filter(|text| text.parse::<f64>().is_ok())
                                .map(str::to_string)
                        })
                        .or_else(|| model.numeric_literals.get(&expr).map(f64::to_string))?;
                    format!("number:{literal}")
                }
                ExprKind::String => {
                    let literal = model
                        .source
                        .get(node.span.start as usize..node.span.end as usize)
                        .filter(|text| text.starts_with('\'') || text.starts_with('"'))?;
                    format!("string:{literal}")
                }
                ExprKind::Unary { op, arg } => {
                    stack.push(Some(*arg));
                    format!("unary:{op:?}")
                }
                ExprKind::Binary { op, lhs, rhs } => {
                    stack.push(Some(*rhs));
                    stack.push(Some(*lhs));
                    format!("binary:{op:?}")
                }
                ExprKind::Call { callee, args } => {
                    stack.extend(args.iter().rev().map(|arg| Some(*arg)));
                    format!("call:{}:{}", model.name(*callee), args.len())
                }
                ExprKind::SteadyState { arg } => {
                    stack.push(Some(*arg));
                    "steady_state".into()
                }
                ExprKind::Expectation { shift, arg } => {
                    stack.push(Some(*arg));
                    format!("expectation:{shift}")
                }
                ExprKind::PathNamespace { .. } | ExprKind::Error => return None,
            };
            out.push(shape);
        }
    }
    Some(out)
}

fn timing_changes(
    before: &Model,
    after: &Model,
    old: &CapturedEquation<'_>,
    new: &CapturedEquation<'_>,
    old_locals: &BindingFacts,
    new_locals: &BindingFacts,
    semantic: &mut SemanticDiff,
) -> (Vec<TimingChange>, bool) {
    let old_uses = uses(before, old, old_locals);
    let new_uses = uses(after, new, new_locals);
    let mut old_counts = BTreeMap::new();
    let mut new_counts = BTreeMap::new();
    let mut new_positions: BTreeMap<_, VecDeque<_>> = BTreeMap::new();
    for usage in &old_uses {
        *old_counts.entry(usage.binding.clone()).or_insert(0_usize) += 1;
    }
    for (index, usage) in new_uses.iter().enumerate() {
        *new_counts.entry(usage.binding.clone()).or_insert(0_usize) += 1;
        new_positions
            .entry(usage.binding.clone())
            .or_default()
            .push_back(index);
    }
    let repeated = old_counts
        .iter()
        .any(|(binding, count)| *count > 1 && new_counts.get(binding) == Some(count));
    let work = old
        .equation
        .active_tokens
        .len()
        .saturating_add(new.equation.active_tokens.len())
        .saturating_add(4);
    let limited = repeated && semantic.charge_token_alignment(1, work) != Availability::Complete;
    let same_shape = repeated
        && !limited
        && match (
            equation_shape(before, old.equation),
            equation_shape(after, new.equation),
        ) {
            (Some(old), Some(new)) => old == new,
            _ => false,
        };
    let mut used = vec![false; new_uses.len()];
    let mut changes = Vec::new();
    for old in old_uses {
        let old_count = old_counts[&old.binding];
        let new_count = new_counts.get(&old.binding).copied().unwrap_or(0);
        let pairable = !matches!(old.binding, BindingKey::Unknown(_))
            && ((old_count == 1 && new_count == 1) || (same_shape && old_count == new_count));
        let paired = if pairable {
            new_positions
                .get_mut(&old.binding)
                .and_then(VecDeque::pop_front)
        } else {
            None
        };
        if let Some(index) = paired {
            let new = &new_uses[index];
            used[index] = true;
            changes.push(TimingChange {
                before: Some(old.side),
                after: Some(new.side.clone()),
            });
        } else {
            changes.push(TimingChange {
                before: Some(old.side),
                after: None,
            });
        }
    }
    for (index, new) in new_uses.into_iter().enumerate() {
        if !used[index] {
            changes.push(TimingChange {
                before: None,
                after: Some(new.side),
            });
        }
    }
    (changes, limited)
}

fn populate_rows(
    before: &Model,
    after: &Model,
    old: &Catalog<'_>,
    new: &Catalog<'_>,
    old_locals: &BindingFacts,
    new_locals: &BindingFacts,
    diff: &mut ModelDiff,
) {
    for index in 0..diff.semantic.rows.len() {
        let mut row = diff.semantic.rows[index].clone();
        if row.family == SemanticFamily::Equations {
            attach_provenance(&mut row.before, old);
            attach_provenance(&mut row.after, new);
        }
        let Some(expression) = row.fields.iter().find(|field| field.name == "expression") else {
            continue;
        };
        let before_text = text(&expression.before);
        let after_text = text(&expression.after);
        if row.family == SemanticFamily::Equations {
            let side = row.before.as_ref().or(row.after.as_ref());
            if row
                .limits
                .iter()
                .any(|limit| limit.code == "condition_correspondence_unpaired")
            {
                row.expressions.push(plain(
                    "expression",
                    before_text,
                    after_text,
                    Availability::Complete,
                    None,
                ));
            } else if let Some((old_candidates, new_candidates)) = candidate_group(diff, side) {
                let old_candidates: Vec<_> =
                    old_candidates.into_iter().map(str::to_string).collect();
                let new_candidates: Vec<_> =
                    new_candidates.into_iter().map(str::to_string).collect();
                row.change = ChangeKind::Unpaired;
                row.expressions.push(unpaired_detail(
                    &mut diff.semantic,
                    before_text,
                    after_text,
                    &old_candidates
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>(),
                    &new_candidates
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>(),
                ));
                row.limits.push(ComparisonLimit::new("equation_correspondence_unpaired", "Text-only highlights do not establish equation, reference or timing correspondence.", "semantic_equations"));
            } else {
                row.expressions.push(expression_detail(
                    &mut diff.semantic,
                    "expression",
                    before_text,
                    after_text,
                ));
                let sides = (
                    row.before
                        .as_ref()
                        .and_then(key)
                        .and_then(|key| old.get(&key)),
                    row.after
                        .as_ref()
                        .and_then(key)
                        .and_then(|key| new.get(&key)),
                );
                if let (Some(old), Some(new)) = sides {
                    let (timing, limited) = timing_changes(
                        before,
                        after,
                        old,
                        new,
                        old_locals,
                        new_locals,
                        &mut diff.semantic,
                    );
                    row.timing = timing;
                    if limited {
                        row.limits.push(ComparisonLimit::new("timing_correspondence_limit", "Timing correspondence for repeated uses exceeded the remaining work budget; separate side facts remain available.", "semantic_equations"));
                    }
                    if row
                        .timing
                        .iter()
                        .any(|change| match (&change.before, &change.after) {
                            (Some(before), Some(after)) => {
                                before.written_offset != after.written_offset
                                    || before.converted_offset != after.converted_offset
                            }
                            _ => true,
                        })
                    {
                        row.facets.push(ChangeFacet::Timing);
                    }
                    if row
                        .timing
                        .iter()
                        .any(|timing| timing.before.is_none() || timing.after.is_none())
                    {
                        row.limits.push(ComparisonLimit::new("identifier_correspondence_unpaired", "Some identifier uses have no proven correspondence; they remain separate Before and After occurrences.", "semantic_equations"));
                    }
                } else {
                    row.timing = match sides {
                        (Some(old), None) => uses(before, old, old_locals)
                            .into_iter()
                            .map(|usage| TimingChange {
                                before: Some(usage.side),
                                after: None,
                            })
                            .collect(),
                        (None, Some(new)) => uses(after, new, new_locals)
                            .into_iter()
                            .map(|usage| TimingChange {
                                before: None,
                                after: Some(usage.side),
                            })
                            .collect(),
                        _ => Vec::new(),
                    };
                }
            }
        } else if matches!(
            row.family,
            SemanticFamily::Parameters | SemanticFamily::Symbols
        ) {
            row.expressions.push(expression_detail(
                &mut diff.semantic,
                "expression",
                before_text,
                after_text,
            ));
        }
        if row.family == SemanticFamily::Equations {
            append_conditions(&mut row, old, new, &mut diff.semantic);
        }
        diff.semantic.rows[index] = row;
    }
}

fn condition_values(equation: Option<&CapturedEquation<'_>>) -> [FieldState; 4] {
    let Some(condition) = equation.and_then(|equation| equation.equation.complementarity.as_ref())
    else {
        return std::array::from_fn(|_| FieldState::absent());
    };
    let [variable, lower, upper] = if let Some(matched) = &condition.matched {
        [
            FieldState::text(&matched.variable),
            FieldState::optional_text(matched.lower_bound.as_deref()),
            FieldState::optional_text(matched.upper_bound.as_deref()),
        ]
    } else {
        std::array::from_fn(|_| FieldState::unknown())
    };
    [FieldState::text(&condition.text), variable, lower, upper]
}

fn append_conditions(
    row: &mut SemanticRow,
    old: &Catalog<'_>,
    new: &Catalog<'_>,
    semantic: &mut SemanticDiff,
) {
    let before = row
        .before
        .as_ref()
        .and_then(key)
        .and_then(|key| old.get(&key));
    let after = row
        .after
        .as_ref()
        .and_then(key)
        .and_then(|key| new.get(&key));
    if before
        .into_iter()
        .chain(after)
        .all(|equation| equation.equation.complementarity.is_none())
    {
        return;
    }
    let before_values = condition_values(before);
    let after_values = condition_values(after);
    let before_text = text(&before_values[0]);
    let after_text = text(&after_values[0]);
    row.expressions.push(if row.change == ChangeKind::Unpaired {
        plain(
            "complementarity.text",
            before_text,
            after_text,
            Availability::Complete,
            None,
        )
    } else {
        expression_detail(semantic, "complementarity.text", before_text, after_text)
    });
    let mut changed = false;
    for (((name, label), before), after) in [
        ("complementarity.text", "Complementarity condition"),
        ("complementarity.variable", "Constrained variable"),
        ("complementarity.lower_bound", "Lower bound"),
        ("complementarity.upper_bound", "Upper bound"),
    ]
    .into_iter()
    .zip(before_values)
    .zip(after_values)
    {
        let field = FieldChange::new(name, label, before, after);
        changed |= field.changed;
        row.fields.push(field);
    }
    if changed {
        row.facets.push(ChangeFacet::Complementarity);
    }
    if before.into_iter().chain(after).any(|equation| {
        equation
            .equation
            .complementarity
            .as_ref()
            .is_some_and(|condition| condition.matched.is_none())
    }) {
        row.limits.push(ComparisonLimit::new(
            "complementarity_unmatched",
            "The retained condition has no matched variable and bounds; its exact text remains available.",
            "semantic_equations",
        ));
    }
}

fn equation_pointers(diff: &ModelDiff, side: Side) -> BTreeMap<EquationKey, String> {
    diff.semantic
        .rows
        .iter()
        .filter(|row| row.family == SemanticFamily::Equations)
        .filter_map(|row| {
            let side = if side == Side::Before {
                row.before.as_ref()
            } else {
                row.after.as_ref()
            }?;
            Some((key(side)?, row.pointer.clone()))
        })
        .collect()
}

fn reference_index(
    model: &Model,
    equations: &Catalog<'_>,
    bindings: &BindingFacts,
) -> HashMap<String, Vec<(EquationKey, TimingSide)>> {
    let mut index: HashMap<String, Vec<_>> = HashMap::new();
    for (key, equation) in equations {
        for usage in uses(model, equation, bindings)
            .into_iter()
            .filter(|usage| matches!(usage.binding, BindingKey::Global(..)))
        {
            index
                .entry(usage.side.name.clone())
                .or_default()
                .push((key.clone(), usage.side));
        }
    }
    index
}

fn populate_references(
    before: &Model,
    after: &Model,
    old: &Catalog<'_>,
    new: &Catalog<'_>,
    old_locals: &BindingFacts,
    new_locals: &BindingFacts,
    diff: &mut ModelDiff,
) {
    let old_references = reference_index(before, old, old_locals);
    let new_references = reference_index(after, new, new_locals);
    let old_pointers = equation_pointers(diff, Side::Before);
    let new_pointers = equation_pointers(diff, Side::After);
    let mut counts = [0_usize; 2];
    let mut omitted = 0_usize;
    // One direct written use can support several changed facts about its
    // symbol. Those rows share one reference entry and one budget charge.
    let mut emitted: HashMap<(usize, EquationKey, usize), Option<String>> = HashMap::new();
    for row_index in 0..diff.semantic.rows.len() {
        if !matches!(
            diff.semantic.rows[row_index].family,
            SemanticFamily::Parameters | SemanticFamily::Symbols
        ) {
            continue;
        }
        let name = diff.semantic.rows[row_index].name.clone();
        for (side, equations, references, pointers, count_index) in [
            (Side::Before, old, &old_references, &old_pointers, 0),
            (Side::After, new, &new_references, &new_pointers, 1),
        ] {
            let row_side = if side == Side::Before {
                &diff.semantic.rows[row_index].before
            } else {
                &diff.semantic.rows[row_index].after
            };
            if row_side.is_none() {
                continue;
            }
            let mut row_omitted = 0;
            for (key, usage) in references.get(&name).into_iter().flatten() {
                let equation = &equations[key];
                let identity = (count_index, key.clone(), usage.occurrence);
                if let Some(pointer) = emitted.get(&identity) {
                    if let Some(pointer) = pointer {
                        diff.semantic.rows[row_index]
                            .references
                            .push(pointer.clone());
                    } else {
                        row_omitted += 1;
                    }
                    continue;
                }
                if counts[count_index] >= diff.semantic.budgets.references_per_side {
                    omitted += 1;
                    row_omitted += 1;
                    emitted.insert(identity, None);
                    continue;
                }
                counts[count_index] += 1;
                let pointer = format!("/semantic/references/{}", diff.semantic.references.len());
                let scope = ComparisonScope {
                    domain: if equation.dimension.is_some() {
                        "heterogeneous"
                    } else {
                        "aggregate"
                    }
                    .into(),
                    dimension: key.0.clone(),
                    block: None,
                };
                let equation_pointer = pointers
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| pointer.clone());
                diff.semantic.references.push(EquationReference {
                    pointer: pointer.clone(),
                    symbol: name.clone(),
                    side,
                    equation_pointer,
                    equation_index: equation.row.index,
                    label: equation_label(equation),
                    scope,
                    occurrence: usage.occurrence,
                    timing: usage.clone(),
                    provenance: Some(provenance(equation)),
                });
                diff.semantic.rows[row_index].references.push(pointer);
                emitted.insert(
                    identity,
                    Some(
                        diff.semantic
                            .references
                            .last()
                            .expect("just appended")
                            .pointer
                            .clone(),
                    ),
                );
            }
            if row_omitted > 0 {
                let mut limit = ComparisonLimit::new(
                    "references_partial",
                    "Direct equation references for this row were omitted by the reference limit.",
                    "semantic_equations",
                );
                limit.omitted = Some(row_omitted);
                diff.semantic.rows[row_index].limits.push(limit);
            }
        }
    }
    if omitted > 0 {
        let mut limit = ComparisonLimit::new(
            "reference_limit",
            "Direct equation references were omitted; the reference list is partial.",
            "semantic_equations",
        );
        limit.omitted = Some(omitted);
        diff.semantic.limits.push(limit.clone());
        diff.coverage.limits.push(limit);
        diff.semantic.availability = Availability::Partial;
        diff.coverage.availability = Availability::Partial;
    }
}

/// Local rows use ModelLocals' exact accepted binding links. These are direct
/// written counted uses, not uses reached through expanding the local's RHS.
struct LocalReferenceIndex {
    definition_ids: HashMap<usize, usize>,
    declaration_ids: HashMap<usize, usize>,
    by_definition: HashMap<usize, Vec<(EquationKey, usize, usize)>>,
    by_declaration: HashMap<usize, Vec<(EquationKey, usize, usize)>>,
}

impl LocalReferenceIndex {
    fn new(catalog: &Catalog<'_>, locals: &ModelLocals) -> Self {
        let mut index = Self {
            definition_ids: locals
                .definitions
                .iter()
                .enumerate()
                .map(|(id, definition)| (definition.equation_index, id))
                .collect(),
            declaration_ids: locals
                .declarations
                .iter()
                .enumerate()
                .map(|(id, declaration)| (declaration.parse_order, id))
                .collect(),
            by_definition: HashMap::new(),
            by_declaration: HashMap::new(),
        };
        let counted: HashMap<_, _> = catalog
            .iter()
            .filter_map(|(key, equation)| equation.written_id.map(|id| (id, key)))
            .collect();
        let mut occurrences = HashMap::new();
        for (usage_id, usage) in locals.uses.iter().enumerate() {
            let Some(key) = counted.get(&usage.equation_index) else {
                continue;
            };
            let occurrence = occurrences.entry(usage.equation_index).or_insert(0);
            let entry = ((*key).clone(), *occurrence, usage_id);
            *occurrence += 1;
            if let Some(id) = usage.definition {
                index
                    .by_definition
                    .entry(id)
                    .or_default()
                    .push(entry.clone());
            }
            if let Some(id) = usage.declaration {
                index.by_declaration.entry(id).or_default().push(entry);
            }
        }
        for entries in index
            .by_definition
            .values_mut()
            .chain(index.by_declaration.values_mut())
        {
            entries.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
        }
        index
    }
}

pub(crate) fn populate_local_references(before: &Model, after: &Model, diff: &mut ModelDiff) {
    let old = catalog(before);
    let new = catalog(after);
    let old_locals = ModelLocals::collect(before);
    let new_locals = ModelLocals::collect(after);
    let old_index = LocalReferenceIndex::new(&old, &old_locals);
    let new_index = LocalReferenceIndex::new(&new, &new_locals);
    let old_pointers = equation_pointers(diff, Side::Before);
    let new_pointers = equation_pointers(diff, Side::After);
    let mut counts = [0_usize; 2];
    for reference in &diff.semantic.references {
        counts[usize::from(reference.side == Side::After)] += 1;
    }
    let mut emitted: HashMap<(usize, EquationKey, usize), Option<String>> = HashMap::new();
    let mut omitted = 0;
    for row_index in 0..diff.semantic.rows.len() {
        let role = diff.semantic.rows[row_index]
            .fields
            .iter()
            .find(|field| field.name == "role");
        let is_local=role.is_some_and(|field|[&field.before,&field.after].into_iter().any(|value|matches!(value.value.as_ref(),Some(FieldValue::Text(role)) if role=="model_local_definition" || role=="model_local_declaration")));
        if !is_local {
            continue;
        }
        for (side, model, catalog, locals, index, pointers, count_index) in [
            (
                Side::Before,
                before,
                &old,
                &old_locals,
                &old_index,
                &old_pointers,
                0,
            ),
            (
                Side::After,
                after,
                &new,
                &new_locals,
                &new_index,
                &new_pointers,
                1,
            ),
        ] {
            let row_side = match side {
                Side::Before => &diff.semantic.rows[row_index].before,
                Side::After => &diff.semantic.rows[row_index].after,
            };
            let Some(proof) = row_side.as_ref().and_then(|side| side.provenance.as_ref()) else {
                continue;
            };
            let definition = proof
                .equation_id
                .and_then(|id| index.definition_ids.get(&id).copied());
            let declaration = if definition.is_none() {
                proof
                    .parse_order
                    .and_then(|order| index.declaration_ids.get(&order).copied())
            } else {
                None
            };
            let mut row_omitted = 0;
            let uses = definition
                .and_then(|id| index.by_definition.get(&id))
                .or_else(|| declaration.and_then(|id| index.by_declaration.get(&id)));
            for (key, occurrence, usage_id) in uses.into_iter().flatten() {
                let equation = &catalog[key];
                let usage = &locals.uses[*usage_id];
                let occurrence = *occurrence;
                let identity = (count_index, key.clone(), occurrence);
                if let Some(pointer) = emitted.get(&identity) {
                    if let Some(pointer) = pointer {
                        diff.semantic.rows[row_index]
                            .references
                            .push(pointer.clone());
                    } else {
                        row_omitted += 1;
                    }
                    continue;
                }
                if counts[count_index] >= diff.semantic.budgets.references_per_side {
                    row_omitted += 1;
                    omitted += 1;
                    emitted.insert(identity, None);
                    continue;
                }
                counts[count_index] += 1;
                let pointer = format!("/semantic/references/{}", diff.semantic.references.len());
                let timing = TimingSide {
                    name: model.name(usage.name).into(),
                    class: "model_local".into(),
                    written_offset: usage.timing,
                    converted_offset: usage.timing,
                    occurrence,
                };
                diff.semantic.references.push(EquationReference {
                    pointer: pointer.clone(),
                    symbol: timing.name.clone(),
                    side,
                    equation_pointer: pointers
                        .get(key)
                        .cloned()
                        .unwrap_or_else(|| pointer.clone()),
                    equation_index: equation.row.index,
                    label: equation_label(equation),
                    scope: ComparisonScope {
                        domain: if equation.dimension.is_some() {
                            "heterogeneous"
                        } else {
                            "aggregate"
                        }
                        .into(),
                        dimension: key.0.clone(),
                        block: None,
                    },
                    occurrence,
                    timing,
                    provenance: Some(provenance(equation)),
                });
                diff.semantic.rows[row_index]
                    .references
                    .push(pointer.clone());
                emitted.insert(identity, Some(pointer));
            }
            if row_omitted > 0 {
                let mut limit = ComparisonLimit::new(
                    "references_partial",
                    "Direct local references were omitted by the comparison-wide reference limit.",
                    "semantic_surfaces",
                );
                limit.omitted = Some(row_omitted);
                diff.semantic.rows[row_index].limits.push(limit);
            }
        }
    }
    if omitted > 0 {
        let mut limit = ComparisonLimit::new(
            "reference_limit",
            "Direct local references were omitted; the reference list is partial.",
            "semantic_surfaces",
        );
        limit.omitted = Some(omitted);
        super::occurrences::record_limit(diff, SemanticFamily::Equations, limit);
    }
}
