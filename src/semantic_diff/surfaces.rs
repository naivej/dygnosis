//! Accepted local, static, state and helper surfaces plus statement context.

use std::collections::BTreeSet;

use crate::expr::ExprKind;
use crate::model::{Assignment, Model, StatementKind};
use crate::model_diff::{
    final_symbol_declarations, last_assignment, normalize_equation, ModelDiff,
};
use crate::model_locals::ModelLocals;

use super::occurrences::*;
use super::*;

pub(crate) fn populate(
    before: &Model,
    after: &Model,
    diff: &mut ModelDiff,
    claims: &mut TokenClaims,
) {
    primary_context(before, after, diff, claims);
    let owned = |side: Side| {
        diff.shock_setup_changes
            .iter()
            .filter_map(|change| match side {
                Side::Before => change.before.as_ref(),
                Side::After => change.after.as_ref(),
            })
            .filter_map(|setting| {
                setting
                    .assignment_tokens
                    .as_ref()
                    .map(|range| (range.start, range.end))
            })
            .collect::<BTreeSet<_>>()
    };
    let old = facts(before, &owned(Side::Before), &mut claims.expression_work);
    let new = facts(after, &owned(Side::After), &mut claims.expression_work);
    compare_facts(before, after, old, new, diff, claims);
    super::equations::populate_local_references(before, after, diff);
    surface_coverage(diff);
}

fn scope(
    model: &Model,
    dimension: Option<crate::intern::Name>,
    block: Option<&str>,
) -> ComparisonScope {
    ComparisonScope {
        domain: if dimension.is_some() {
            "heterogeneous"
        } else {
            "aggregate"
        }
        .into(),
        dimension: dimension.map(|name| model.name(name).into()),
        block: block.map(str::to_owned),
    }
}

fn field(name: &str, label: &str, value: FieldState, facet: ChangeFacet) -> FactField {
    FactField::new(name, label, value, facet)
}
fn expression(name: &str, text: &str) -> FactField {
    field(
        name,
        "Expression",
        FieldState::text(text),
        ChangeFacet::Expression,
    )
    .expression()
}
fn names(values: impl IntoIterator<Item = String>) -> FieldState {
    FieldState::present(FieldValue::List(
        values.into_iter().map(FieldValue::Text).collect(),
    ))
}

fn side_at(model: &Model, order: usize, label: &str, scope: ComparisonScope) -> RowSide {
    parent_at_order(model, order)
        .map(|id| statement_side(model, id, label, scope.clone()))
        .unwrap_or_else(|| RowSide::named(label, scope))
}

fn facts(
    model: &Model,
    owned_assignments: &BTreeSet<(usize, usize)>,
    work: &mut super::expression_values::RetainedExpressionWork,
) -> Vec<CapturedFact> {
    let mut facts = Vec::new();
    let locals = ModelLocals::collect(model);
    for declaration in &locals.declarations {
        let written = &model.written_declarations[declaration.declaration_index];
        let name = model.name(declaration.name);
        let mut fact = CapturedFact::new(
            SemanticFamily::Symbols,
            "model_local_declaration",
            vec![name.into()],
            statement_side(model, written.statement_id, name, ComparisonScope::global()),
        );
        if let Some(provenance) = &mut fact.side.provenance {
            provenance.span = declaration.span;
            provenance.parse_order = Some(declaration.parse_order);
        }
        fact.fields = vec![
            field(
                "target",
                "Local name",
                FieldState::text(name),
                ChangeFacet::Target,
            ),
            field(
                "tex_name",
                "TeX label",
                FieldState::optional_text(declaration.tex_name.as_deref()),
                ChangeFacet::Label,
            ),
        ];
        fact.claims.push(written.token_range.clone());
        // The existing per-object producer retains the TeX value but not its
        // expanded range. Leave residual declaration text in Source rather than
        // turn one metadata edit into another Commands count.
        facts.push(fact);
    }
    for definition in &locals.definitions {
        let written = &model.written_equations[definition.equation_index];
        let name = model.name(definition.name);
        let mut side = statement_side(
            model,
            written.statement_id,
            name,
            scope(model, definition.dimension, Some("model")),
        );
        side.occurrence = Some(definition.parse_order);
        side.provenance = Some(OccurrenceProvenance {
            span: definition.target_span,
            parse_order: Some(definition.parse_order),
            equation_id: Some(definition.equation_index),
            statement_id: Some(written.statement_id),
        });
        let mut fact = CapturedFact::new(
            SemanticFamily::Symbols,
            "model_local_definition",
            vec![name.into()],
            side,
        );
        fact.fields = vec![
            field(
                "target",
                "Local name",
                FieldState::text(name),
                ChangeFacet::Target,
            ),
            expression("expression", &written.equation.rhs),
        ];
        fact.claims.push(written.token_range.clone());
        facts.push(fact);
    }
    for (written_id, written) in model.written_equations.iter().enumerate() {
        let equation = &written.equation;
        if !equation.static_tag
            || equation.is_local
            || !live_static(model, equation.parse_order, written.dimension)
        {
            continue;
        }
        let label = if equation.name.is_empty() {
            "Static alternative"
        } else {
            &equation.name
        };
        let mut side = statement_side(
            model,
            written.statement_id,
            label,
            scope(model, written.dimension, Some("model")),
        );
        side.occurrence = Some(equation.parse_order);
        side.provenance = Some(OccurrenceProvenance {
            span: equation.span,
            parse_order: Some(equation.parse_order),
            equation_id: Some(written_id),
            statement_id: Some(written.statement_id),
        });
        let key = if equation.name.is_empty() {
            vec![normalize_equation(&equation.text)]
        } else {
            vec![equation.name.clone()]
        };
        let mut fact =
            CapturedFact::new(SemanticFamily::Equations, "static_alternative", key, side);
        fact.fields = vec![
            expression("expression", &equation.text),
            field(
                "tags",
                "Tags",
                FieldState::present(FieldValue::Record(
                    equation
                        .tag_map
                        .iter()
                        .map(|(key, value)| (key.clone(), FieldValue::Text(value.clone())))
                        .collect(),
                )),
                ChangeFacet::Tags,
            ),
            field(
                "static",
                "Static alternative",
                FieldState::present(FieldValue::Boolean(equation.static_tag)),
                ChangeFacet::Role,
            ),
            field(
                "dynamic",
                "Dynamic role",
                FieldState::present(FieldValue::Boolean(equation.dynamic_tag)),
                ChangeFacet::Role,
            ),
        ];
        fact.claims.push(written.token_range.clone());
        facts.push(fact);
    }
    for equation in &model.steady_state_equations {
        if equation
            .rhs_expr
            .is_none_or(|id| matches!(model.exprs.get(id).kind, ExprKind::Error))
            || equation.steady_state_targets.is_empty()
            || equation.steady_state_targets.iter().any(|target| {
                !target.action_attempted || !model.steady_state_target_is_valid(target)
            })
        {
            continue;
        }
        let targets: Vec<_> = equation
            .steady_state_targets
            .iter()
            .map(|target| model.name(target.name).to_string())
            .collect();
        let label = targets.join(", ");
        let roles = equation
            .steady_state_targets
            .iter()
            .map(|target| {
                model
                    .symbol_kind_in_context(target.name, target.symbol_type_context)
                    .unwrap_or("temporary")
                    .to_string()
            })
            .collect::<Vec<_>>();
        let mut fact = CapturedFact::new(
            SemanticFamily::SteadyState,
            "steady_state_assignment",
            targets.clone(),
            side_at(
                model,
                equation.parse_order,
                &label,
                scope(model, None, Some("steady_state_model")),
            ),
        );
        fact.side.occurrence = Some(equation.parse_order);
        fact.fields = vec![
            field(
                "targets",
                "Ordered outputs",
                names(targets),
                ChangeFacet::Target,
            ),
            field(
                "target_roles",
                "Output roles",
                names(roles),
                ChangeFacet::Role,
            ),
            expression("expression", &equation.rhs),
        ];
        fact.claims.push(equation.active_tokens.clone());
        facts.push(fact);
    }
    for (role, assignments) in [("initval", &model.initval), ("endval", &model.endval)] {
        for assignment in assignments {
            if assignment.native
                || owned_assignments
                    .contains(&(assignment.active_tokens.start, assignment.active_tokens.end))
            {
                continue;
            }
            facts.push(assignment_fact(
                model,
                assignment,
                role,
                SemanticFamily::SteadyState,
            ));
        }
    }
    for assignment in &model.helper_assignments {
        if assignment.native {
            continue;
        }
        facts.push(assignment_fact(
            model,
            assignment,
            "helper_assignment",
            SemanticFamily::Operations,
        ));
    }
    // Final calibration has an existing primary row. Earlier accepted writes
    // are separate operations, including a change with equal final calibration.
    for assignment in &model.param_assignments {
        if assignment.native
            || last_assignment(model, model.name(assignment.name))
                .is_some_and(|last| std::ptr::eq(last, assignment))
        {
            continue;
        }
        facts.push(assignment_fact(
            model,
            assignment,
            "parameter_assignment_history",
            SemanticFamily::Operations,
        ));
    }
    history_facts(model, &mut facts, work);
    facts.sort_by_key(|fact| fact.side.occurrence.unwrap_or(usize::MAX));
    facts
}

fn live_static(model: &Model, order: usize, dimension: Option<crate::intern::Name>) -> bool {
    match dimension {
        None => model
            .equations
            .iter()
            .any(|equation| equation.parse_order == order && equation.static_tag),
        Some(dimension) => model
            .heterogeneous_models
            .iter()
            .filter(|block| block.dimension == dimension)
            .flat_map(|block| &block.equations)
            .any(|equation| equation.parse_order == order && equation.static_tag),
    }
}

fn assignment_fact(
    model: &Model,
    assignment: &Assignment,
    role: &str,
    family: SemanticFamily,
) -> CapturedFact {
    let name = model.name(assignment.name);
    let mut side = if assignment.active_tokens.is_empty() {
        RowSide::named(name, scope(model, None, Some(role)))
    } else {
        side_at(
            model,
            assignment.active_tokens.start,
            name,
            scope(model, None, Some(role)),
        )
    };
    side.occurrence =
        (!assignment.active_tokens.is_empty()).then_some(assignment.active_tokens.start);
    if let Some(provenance) = &mut side.provenance {
        provenance.span = assignment.span;
    }
    let mut fact = CapturedFact::new(family, role, vec![name.into()], side);
    fact.fields = vec![
        field(
            "target",
            "Target",
            FieldState::text(name),
            ChangeFacet::Target,
        ),
        field(
            "target_role",
            "Captured target role",
            FieldState::optional_text(
                model.symbol_kind_in_context(assignment.name, assignment.symbol_type_context),
            ),
            ChangeFacet::Role,
        ),
        expression("expression", &assignment.expression),
    ];
    fact.claims.push(assignment.active_tokens.clone());
    if family == SemanticFamily::Operations {
        fact.count_unit = CountUnit::Operation;
    }
    fact
}

fn history_facts(
    model: &Model,
    facts: &mut Vec<CapturedFact>,
    work: &mut super::expression_values::RetainedExpressionWork,
) {
    for entry in &model.histval {
        if !entry.accepted_assignment {
            continue;
        }
        let name = model.name(entry.name);
        let label = format!("{name}({})", entry.lag);
        let parent = parent_at_order(model, entry.active_tokens.start).filter(|&id| {
            let statement = &model.statements[id];
            statement.name == "histval" && entry.active_tokens.end <= statement.token_range.end
        });
        let mut side = parent
            .map(|id| statement_side(model, id, &label, scope(model, None, Some("histval"))))
            .unwrap_or_else(|| RowSide::named(&label, scope(model, None, Some("histval"))));
        side.occurrence = Some(entry.active_tokens.start);
        if let Some(provenance) = &mut side.provenance {
            provenance.span = entry.span;
            provenance.parse_order = Some(entry.active_tokens.start);
        }
        let mut fact = CapturedFact::new(
            SemanticFamily::SteadyState,
            "histval_assignment",
            vec![name.into()],
            side,
        );
        let value = if parent.is_some() {
            let equal = entry
                .active_tokens
                .clone()
                .find(|&index| model.expanded_tokens[index].kind == crate::lexer::TokenKind::Eq);
            equal
                .map(|equal| {
                    super::expression_values::range_text(
                        model,
                        &mut fact,
                        "expression",
                        equal + 1..entry.active_tokens.end - 1,
                        work,
                    )
                })
                .unwrap_or_else(FieldState::unknown)
        } else {
            super::expression_values::expression(model, &mut fact, "expression", entry.expr, work)
        };
        let available = !fact.limits.iter().any(|limit| {
            limit.code == "expression_tree_node_limit"
                || limit.code == "expression_tree_materialization_limit"
                || limit.code == "expression_selector_visit_limit"
                || limit.code == "expression_selector_token_limit"
                || limit.code == "expression_text_materialization_limit"
        });
        fact.fields = vec![
            field(
                "target",
                "Historical target",
                FieldState::text(name),
                ChangeFacet::Target,
            ),
            field(
                "lag",
                "Written history period",
                FieldState::present(FieldValue::Integer(entry.lag.into())),
                ChangeFacet::Timing,
            ),
            field("expression", "Expression", value, ChangeFacet::Expression).expression(),
        ];
        if !available && let Some(field) = fact.fields.last_mut() {
            field.comparison_available = false;
        }
        if parent.is_some() {
            fact.claims.push(entry.active_tokens.clone());
        } else {
            fact.limits.push(ComparisonLimit::new("history_identity_unavailable", "This retained history row has no proven expanded parent; target, period and owned expression tree remain available, written text and navigation are withheld.", "semantic_surfaces"));
        }
        facts.push(fact);
    }
}
fn primary_context(before: &Model, after: &Model, diff: &mut ModelDiff, claims: &mut TokenClaims) {
    for (model, side) in [(before, Side::Before), (after, Side::After)] {
        // The shock reader owns these complete instructions on both sides,
        // including unchanged rows. Written spans cannot identify macro copies.
        for receipt in model
            .fact_receipts
            .get("shock_instruction")
            .into_iter()
            .flatten()
        {
            if let Some(id) = parent_at_order(model, receipt.parse_order)
                && accepted_statement(model, &model.statements[id])
            {
                for range in &receipt.claims {
                    claims.claim(side, id, range.clone());
                }
            }
        }
        for (name, _) in final_symbol_declarations(model) {
            // Per-object declarations retain target tokens separately. Metadata
            // is represented by the primary symbol fields; declarations never
            // create an aggregate Commands row for these same object edits.
            for written in &model.written_declarations {
                if model.name(written.declaration.name) == name {
                    claims.claim(side, written.statement_id, written.token_range.clone());
                }
            }
        }
        for assignment in &model.param_assignments {
            if last_assignment(model, model.name(assignment.name))
                .is_some_and(|last| std::ptr::eq(last, assignment))
                && let Some(parent) = parent_at_order(model, assignment.active_tokens.start)
            {
                claims.claim(side, parent, assignment.active_tokens.clone());
            }
        }
        for written in &model.written_equations {
            let equation = &written.equation;
            let live = match written.dimension {
                None => model
                    .equations
                    .iter()
                    .any(|live| live.parse_order == equation.parse_order),
                Some(dimension) => model
                    .heterogeneous_models
                    .iter()
                    .filter(|block| block.dimension == dimension)
                    .flat_map(|block| &block.equations)
                    .any(|live| live.parse_order == equation.parse_order),
            };
            if live && !equation.is_local && !equation.static_tag {
                claims.claim(side, written.statement_id, written.token_range.clone());
            }
        }
        for assignment in model
            .initval
            .iter()
            .chain(&model.endval)
            .filter(|assignment| assignment.native && !assignment.active_tokens.is_empty())
        {
            if let Some(id) = parent_at_order(model, assignment.active_tokens.start) {
                claims.claim(side, id, assignment.active_tokens.clone());
            }
        }
        for row in &mut diff.semantic.rows {
            let row_side = match side {
                Side::Before => &mut row.before,
                Side::After => &mut row.after,
            };
            if let Some(row_side) = row_side
                && let Some(id) = row_side
                    .provenance
                    .as_ref()
                    .and_then(|provenance| provenance.statement_id)
            {
                row_side.context =
                    statement_side(model, id, &row_side.name, row_side.scope.clone()).context;
            }
        }
        for (index, change) in diff.shock_setup_changes.iter().enumerate() {
            let setting = match side {
                Side::Before => change.before.as_ref(),
                Side::After => change.after.as_ref(),
            };
            let Some(range) = setting.and_then(|setting| setting.assignment_tokens.as_ref()) else {
                continue;
            };
            let Some(id) = parent_at_order(model, range.start) else {
                continue;
            };
            claims.claim(side, id, range.clone());
            let Some(row) = diff
                .semantic
                .rows
                .iter_mut()
                .find(|row| row.pointer == format!("/shock_setup_changes/{index}"))
            else {
                continue;
            };
            let row_side = match side {
                Side::Before => &mut row.before,
                Side::After => &mut row.after,
            };
            if let Some(row_side) = row_side {
                row_side.context =
                    statement_side(model, id, &row_side.name, row_side.scope.clone()).context;
                if let Some(provenance) = &mut row_side.provenance {
                    provenance.statement_id = Some(id);
                    provenance.parse_order = Some(range.start);
                }
            }
        }
    }
}

pub(crate) fn populate_commands(
    before: &Model,
    after: &Model,
    diff: &mut ModelDiff,
    claims: &TokenClaims,
) {
    let pairs = ParentPairs::new(before, after);
    let mut old_order: Vec<_> = pairs
        .before_to_after
        .iter()
        .map(|(&a, &b)| (a, b))
        .collect();
    old_order.sort_by_key(|(a, _)| execution_order(before, *a));
    let mut new_order = old_order.clone();
    new_order.sort_by_key(|(_, b)| execution_order(after, *b));
    let new_ranks: std::collections::BTreeMap<_, _> = new_order
        .iter()
        .enumerate()
        .map(|(rank, &pair)| (pair, rank))
        .collect();
    let mut old_changes = std::collections::BTreeMap::new();
    let mut new_changes = std::collections::BTreeMap::new();
    for (rank, &(a, b)) in old_order.iter().enumerate() {
        let after_rank = new_ranks[&(a, b)];
        if rank != after_rank {
            old_changes.insert(a, rank);
            new_changes.insert(b, after_rank);
        }
    }
    let old = command_facts(before, Side::Before, claims, &old_changes);
    let new = command_facts(after, Side::After, claims, &new_changes);
    compare_facts(before, after, old, new, diff, &mut TokenClaims::default());
    let mut coverage = FamilyCoverage {
        family: SemanticFamily::Commands,
        availability: Availability::Complete,
        fields: vec![
            "statement_tokens".into(),
            "statement_kind".into(),
            "statement_name".into(),
            "execution_order".into(),
            "block_opener_tokens".into(),
        ],
        limits: Vec::new(),
    };
    if before
        .statements
        .iter()
        .any(|statement| !accepted_statement(before, statement))
        || after
            .statements
            .iter()
            .any(|statement| !accepted_statement(after, statement))
    {
        coverage.availability = Availability::Partial;
        coverage.limits.push(ComparisonLimit::new("statement_acceptance_unavailable","Native text and recovery records do not establish an accepted Dynare command; available accepted child facts are compared separately.","semantic_surfaces"));
    }
    merge_coverage(diff, coverage);
}

fn command_facts(
    model: &Model,
    side: Side,
    claims: &TokenClaims,
    order_changes: &std::collections::BTreeMap<usize, usize>,
) -> Vec<CapturedFact> {
    let mut facts = Vec::new();
    for statement in &model.statements {
        if !matches!(
            statement.kind,
            StatementKind::Command | StatementKind::Block
        ) || !accepted_statement(model, statement)
        {
            continue;
        }
        let tokens = residual(model, statement, side, claims);
        let order = order_changes.get(&statement.id);
        if tokens.is_empty() && order.is_none() {
            continue;
        }
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
        let role = if statement.kind == StatementKind::Block {
            "block_context"
        } else {
            "command"
        };
        let mut fact = CapturedFact::new(
            SemanticFamily::Commands,
            role,
            vec![statement.name.clone()],
            statement_side(model, statement.id, &statement.name, scope),
        );
        fact.fields.push(
            field(
                "statement_tokens",
                "Statement",
                FieldState::text(&tokens),
                ChangeFacet::Options,
            )
            .expression(),
        );
        if let Some(order) = order {
            fact.fields.push(field(
                "execution_order",
                "Relative accepted execution order",
                FieldState::present(FieldValue::Integer(*order as i64)),
                ChangeFacet::Order,
            ));
        }
        if !tokens.is_empty() {
            fact.limits.push(ComparisonLimit::new("statement_options_text_only","These remaining accepted tokens have no complete named option facts; token detail makes no numerical-effect claim.","semantic_surfaces"));
        }
        facts.push(fact);
    }
    facts
}

fn residual(
    model: &Model,
    statement: &crate::model::Statement,
    side: Side,
    claims: &TokenClaims,
) -> String {
    let range = &statement.token_range;
    let tokens: Vec<_> = range
        .clone()
        .filter(|&index| !claims.contains(side, statement.id, index))
        // Written equations retain their body up to, but not including, `;`.
        // A terminator after an owned body belongs to that same fact.
        .filter(|&index| {
            !(matches!(statement.name.as_str(), "model" | "model_replace")
                && model.expanded_tokens[index].kind == crate::lexer::TokenKind::Semi
                && index >= statement.opener_range.end
                && claims.contains(side, statement.id, index - 1))
        })
        .map(|index| model.expanded_tokens[index].clone())
        .collect();
    // Named fields can own all domain tokens while leaving separators behind.
    // Punctuation alone does not establish an additional accepted fact. Keep
    // every residual identifier, value, operator and unowned block-body token.
    let closer = (statement.kind == StatementKind::Block && statement.complete)
        .then(|| model.expanded_tokens.get(range.end.checked_sub(2)?))
        .flatten()
        .filter(|token| token.text(&model.source).eq_ignore_ascii_case("end"));
    if tokens.iter().all(|token| {
        matches!(
            token.kind,
            crate::lexer::TokenKind::Semi
                | crate::lexer::TokenKind::Comma
                | crate::lexer::TokenKind::LParen
                | crate::lexer::TokenKind::RParen
                | crate::lexer::TokenKind::LBrack
                | crate::lexer::TokenKind::RBrack
        ) || closer.is_some_and(|closer| token.span == closer.span)
    }) {
        return String::new();
    }
    crate::parser::join_lexemes(&model.source, &tokens)
}

fn surface_coverage(diff: &mut ModelDiff) {
    let steady = FamilyCoverage {
        family: SemanticFamily::SteadyState,
        availability: Availability::Complete,
        fields: vec![
            "targets".into(),
            "target_roles".into(),
            "expression".into(),
            "lag".into(),
            "accepted_assignment_order".into(),
        ],
        limits: Vec::new(),
    };
    merge_coverage(diff, steady);
    if let Some(equations) = diff
        .coverage
        .families
        .iter_mut()
        .find(|coverage| coverage.family == SemanticFamily::Equations)
    {
        equations.fields.push("static_alternative".into());
    }
    if let Some(symbols) = diff
        .coverage
        .families
        .iter_mut()
        .find(|coverage| coverage.family == SemanticFamily::Symbols)
    {
        symbols
            .fields
            .extend(["model_local_declaration", "model_local_definition"].map(str::to_string));
    }
}

fn merge_coverage(diff: &mut ModelDiff, coverage: FamilyCoverage) {
    if let Some(existing) = diff
        .coverage
        .families
        .iter_mut()
        .find(|existing| existing.family == coverage.family)
    {
        existing.fields.extend(coverage.fields);
        for limit in coverage.limits {
            if !existing.limits.contains(&limit) {
                existing.limits.push(limit);
            }
        }
        if coverage.availability != Availability::Complete {
            existing.availability = Availability::Partial;
        }
    } else {
        diff.coverage.families.push(coverage);
    }
}
