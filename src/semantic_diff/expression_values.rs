//! Named retained expression values shared by occurrence families.
//! Iterative traversal uses owned AST nodes; source positions are never values.

use super::occurrences::CapturedFact;
use super::*;
use crate::expr::{BinOp, ExprId, ExprKind, UnOp};
use crate::model::{Model, NamedModelOperatorKind};
use crate::span::Span;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::ops::Range;

/// Before/After share this work account. These units bound tree traversal and
/// materialization; they are separate from the final serialized-detail cap.
pub(crate) struct RetainedExpressionWork {
    nodes: usize,
    bytes: usize,
    selector_visits: usize,
}

impl Default for RetainedExpressionWork {
    fn default() -> Self {
        Self::new(8 * 1024 * 1024)
    }
}

impl RetainedExpressionWork {
    pub(crate) fn new(detail_bytes: usize) -> Self {
        Self {
            nodes: 100_000,
            bytes: detail_bytes.min(8 * 1024 * 1024),
            selector_visits: 1_000_000,
        }
    }
    fn charge(&mut self, bytes: usize) -> bool {
        if self.nodes == 0 || bytes > self.bytes {
            return false;
        }
        self.nodes -= 1;
        self.bytes -= bytes;
        true
    }
}

pub(crate) fn expression(
    model: &Model,
    fact: &mut CapturedFact,
    name: &str,
    id: Option<ExprId>,
    work: &mut RetainedExpressionWork,
) -> FieldState {
    let Some(id) = id else {
        return FieldState::absent();
    };
    // The retained AST is already owned by this field. Comparing that tree
    // needs no source-span matching or second parse. Preorder plus arity keeps
    // operand and call-argument associations without serializing arena IDs.
    let mut pending = vec![id];
    let mut nodes = Vec::new();
    const MAX_EXPRESSION_NODES: usize = 10_000;
    while let Some(id) = pending.pop() {
        if nodes.len() >= MAX_EXPRESSION_NODES || pending.len() > MAX_EXPRESSION_NODES {
            gap(fact, &format!("{name}_tree_node_limit"), "Retained expression tree comparison is limited to 10,000 nodes per field; omitted tree detail has no complete equality claim.", "semantic_family_expression_budget");
            return FieldState::unknown();
        }
        let scalar_bytes = match &model.exprs.get(id).kind {
            ExprKind::Ident { name, .. } => model.name(*name).len(),
            ExprKind::Number => model.numeric_literal_texts.get(&id).map_or(0, String::len),
            ExprKind::Call { callee, .. } => model.name(*callee).len().saturating_add(
                model
                    .named_model_operator_exprs
                    .get(&id)
                    .and_then(|index| model.named_model_operators.get(*index))
                    .map_or(0, |operator| {
                        model.name(operator.name).len() + named_operator_kind(operator.kind).len()
                    }),
            ),
            ExprKind::String => 0,
            _ => 0,
        };
        if !work.charge(scalar_bytes.saturating_add(256)) {
            gap(fact, &format!("{name}_tree_materialization_limit"), "Retained expression trees share 100,000 nodes and at most min(serialized detail budget, 8 MiB) materialization bytes per comparison, charged before node allocation and scalar cloning. Omitted tree equality is unknown.", "semantic_family_expression_budget");
            return FieldState::unknown();
        }
        let mut node = BTreeMap::new();
        let mut children = Vec::new();
        match &model.exprs.get(id).kind {
            ExprKind::Ident {
                name,
                timing,
                timing_span,
                ..
            } => {
                node.insert("kind".into(), FieldValue::Text("identifier".into()));
                node.insert("name".into(), FieldValue::Text(model.name(*name).into()));
                node.insert(
                    "written_offset".into(),
                    FieldValue::Integer((*timing).into()),
                );
                node.insert(
                    "explicit_timing".into(),
                    FieldValue::Boolean(timing_span.is_some()),
                );
            }
            ExprKind::Number => {
                node.insert("kind".into(), FieldValue::Text("number".into()));
                if let Some(text) = model.numeric_literal_texts.get(&id) {
                    node.insert("text".into(), FieldValue::Text(text.clone()));
                } else {
                    if let Some(value) = model
                        .numeric_literals
                        .get(&id)
                        .copied()
                        .filter(|value| value.is_finite())
                    {
                        node.insert("value".into(), FieldValue::Number(value));
                    } else {
                        node.insert("value_known".into(), FieldValue::Boolean(false));
                    }
                    gap(fact, &format!("{name}_number_spelling_unavailable"), "The numeric node has no retained literal spelling; source spans are not used to reconstruct it.", "parser_numeric_literal_retention");
                }
            }
            ExprKind::String => {
                node.insert("kind".into(), FieldValue::Text("string".into()));
                // Exact text still needs producer proof. This is a value leaf,
                // not an empty string or a fabricated reconstructed literal.
                let written = span_text(model, fact, name, model.exprs.get(id).span, work);
                if let Some(value) = written.value {
                    node.insert("text".into(), value);
                } else {
                    node.insert("text_known".into(), FieldValue::Boolean(false));
                }
            }
            ExprKind::Unary { op, arg } => {
                node.insert("kind".into(), FieldValue::Text("unary".into()));
                node.insert(
                    "operator".into(),
                    FieldValue::Text(
                        match op {
                            UnOp::Pos => "+",
                            UnOp::Neg => "-",
                        }
                        .into(),
                    ),
                );
                children.push(*arg);
            }
            ExprKind::Binary { op, lhs, rhs } => {
                node.insert("kind".into(), FieldValue::Text("binary".into()));
                node.insert(
                    "operator".into(),
                    FieldValue::Text(
                        match op {
                            BinOp::Add => "+",
                            BinOp::Sub => "-",
                            BinOp::Mul => "*",
                            BinOp::Div => "/",
                            BinOp::Pow => "^",
                            BinOp::Lt => "<",
                            BinOp::Gt => ">",
                            BinOp::Le => "<=",
                            BinOp::Ge => ">=",
                            BinOp::EqEq => "==",
                            BinOp::Ne => "!=",
                        }
                        .into(),
                    ),
                );
                children.extend([*lhs, *rhs]);
            }
            ExprKind::Call { callee, args } => {
                node.insert("kind".into(), FieldValue::Text("call".into()));
                node.insert(
                    "callee".into(),
                    FieldValue::Text(model.name(*callee).into()),
                );
                if let Some(operator) = model
                    .named_model_operator_exprs
                    .get(&id)
                    .and_then(|index| model.named_model_operators.get(*index))
                {
                    node.insert(
                        "named_operator_kind".into(),
                        FieldValue::Text(named_operator_kind(operator.kind).into()),
                    );
                    node.insert(
                        "model_name".into(),
                        FieldValue::Text(model.name(operator.name).into()),
                    );
                } else if [
                    "var_expectation",
                    "pac_expectation",
                    "pac_target_nonstationary",
                ]
                .iter()
                .any(|name| model.name(*callee).eq_ignore_ascii_case(name))
                {
                    node.insert("model_name_known".into(), FieldValue::Boolean(false));
                    gap(fact, &format!("{name}_named_operator_association_unavailable"), "The named model operator has no retained association to this expression node. Its model name is unknown; source spans do not establish that association.", "parser_named_operator_retention");
                }
                if args.len() > MAX_EXPRESSION_NODES {
                    gap(
                        fact,
                        &format!("{name}_tree_node_limit"),
                        "Retained expression tree comparison is limited to 10,000 nodes per field.",
                        "semantic_family_expression_budget",
                    );
                    return FieldState::unknown();
                }
                children.extend(args.iter().copied());
            }
            ExprKind::SteadyState { arg } => {
                node.insert("kind".into(), FieldValue::Text("steady_state".into()));
                children.push(*arg);
            }
            ExprKind::Expectation { shift, arg } => {
                node.insert("kind".into(), FieldValue::Text("expectation".into()));
                node.insert("shift".into(), FieldValue::Integer((*shift).into()));
                children.push(*arg);
            }
            ExprKind::PathNamespace { .. } => {
                gap(fact, &format!("{name}_path_owner"), "Path namespace values belong to the retained shock/path producer; this family does not reconstruct that field.", "semantic_shock_fields");
                return FieldState::unknown();
            }
            ExprKind::Error => {
                gap(fact, &format!("{name}_tree_recovered"), "The expression tree contains a recovery node; accepted context or Source supplies written text without a fabricated parsed expression.", "parser_family_expression_retention");
                return FieldState::unknown();
            }
        }
        node.insert("arity".into(), FieldValue::Integer(children.len() as i64));
        pending.extend(children.into_iter().rev());
        nodes.push(FieldValue::Record(node));
    }
    if parent_id(fact).is_none() {
        gap(fact, &format!("{name}_written_text_unavailable"), "The owned retained expression tree is compared. Its exact expanded text and navigation are unavailable without producer occurrence proof.", "parser_family_text_retention");
    }
    FieldState::present(FieldValue::List(nodes))
}

fn named_operator_kind(kind: NamedModelOperatorKind) -> &'static str {
    match kind {
        NamedModelOperatorKind::VarExpectation => "var_expectation",
        NamedModelOperatorKind::PacExpectation => "pac_expectation",
        NamedModelOperatorKind::PacTargetNonstationary => "pac_target_nonstationary",
    }
}
pub(crate) fn parent_id(fact: &CapturedFact) -> Option<usize> {
    fact.side.provenance.as_ref()?.statement_id
}

pub(crate) fn gap(fact: &mut CapturedFact, code: &str, reason: &str, owner: &str) {
    let limit = ComparisonLimit::new(code, reason, owner);
    if !fact.limits.contains(&limit) {
        fact.limits.push(limit);
    }
}

/// A span is a selector only after the producer supplies its expanded parent.
/// Repeated/synthesized spans inside that parent cannot prove a unique slice.
fn span_range(
    model: &Model,
    fact: &mut CapturedFact,
    name: &str,
    span: Span,
    work: &mut RetainedExpressionWork,
) -> Option<Range<usize>> {
    let statement = model.statements.get(parent_id(fact)?)?;
    let tokens = model.expanded_tokens.get(statement.token_range.clone())?;
    if tokens.len() > work.selector_visits {
        gap(fact, &format!("{name}_selector_visit_limit"), "Expanded text selectors share 1,000,000 token visits per comparison. Text with no bounded occurrence proof remains unknown; accepted context and Source retain written evidence.", "semantic_family_expression_budget");
        return None;
    }
    work.selector_visits -= tokens.len();
    let mut first = None;
    let mut end = 0;
    let mut count = 0;
    for (index, token) in tokens.iter().enumerate() {
        if span.start <= token.span.start && token.span.end <= span.end && !token.span.is_empty() {
            first.get_or_insert(statement.token_range.start + index);
            end = statement.token_range.start + index + 1;
            count += 1;
        }
    }
    let first = first?;
    if count != end - first {
        return None;
    }
    let selected = model.expanded_tokens.get(first..end)?;
    if selected.first()?.span.start != span.start || selected.last()?.span.end != span.end {
        return None;
    }
    if selected.len() > 10_000 {
        gap(fact, &format!("{name}_selector_token_limit"), "One written field selector is limited to 10,000 tokens before uniqueness-index allocation.", "semantic_family_expression_budget");
        return None;
    }
    let mut spans = BTreeSet::new();
    if selected
        .iter()
        .any(|token| !spans.insert((token.span.start, token.span.end)))
    {
        return None;
    }
    Some(first..end)
}

pub(crate) fn span_text(
    model: &Model,
    fact: &mut CapturedFact,
    name: &str,
    span: Span,
    work: &mut RetainedExpressionWork,
) -> FieldState {
    if let Some(range) = span_range(model, fact, name, span, work) {
        return range_text(model, fact, name, range, work);
    }
    gap(fact, &format!("{name}_text_unavailable"), &format!("The retained {name} span has no unique text slice inside a proven expanded occurrence. Its accepted statement context and captured Source remain available."), "parser_family_text_retention");
    FieldState::unknown()
}

/// Caller supplies an existing accepted producer range, not a range recovered
/// from source offsets. A valid child range proves text even if its parent is
/// unavailable for navigation or cross-side occurrence correspondence.
pub(crate) fn range_text(
    model: &Model,
    fact: &mut CapturedFact,
    name: &str,
    range: Range<usize>,
    work: &mut RetainedExpressionWork,
) -> FieldState {
    if let Some(tokens) = model.expanded_tokens.get(range.clone()) {
        if tokens.len() > 10_000 {
            gap(
                fact,
                &format!("{name}_selector_token_limit"),
                "One retained text field is limited to 10,000 tokens before text allocation.",
                "semantic_family_expression_budget",
            );
            return FieldState::unknown();
        }
        if tokens.len() > work.selector_visits {
            gap(fact, &format!("{name}_selector_visit_limit"), "Retained text selectors share 1,000,000 token visits per comparison, charged before selection or joining.", "semantic_family_expression_budget");
            return FieldState::unknown();
        }
        work.selector_visits -= tokens.len();
        let bytes = tokens.iter().try_fold(0usize, |total, token| {
            total
                .checked_add(token.text(&model.source).len())?
                .checked_add(1)
        });
        let Some(bytes) = bytes.filter(|bytes| *bytes <= work.bytes) else {
            gap(fact, &format!("{name}_text_materialization_limit"), "Retained written text shares at most min(serialized detail budget, 8 MiB) materialization charge per comparison; token bytes and separators are charged before joining text.", "semantic_family_expression_budget");
            return FieldState::unknown();
        };
        work.bytes -= bytes;
        if let Some(value) = super::occurrences::statement_text(model, range.clone()) {
            fact.claims.push(range);
            return FieldState::text(&value);
        }
    }
    gap(fact, &format!("{name}_text_unavailable"), "The retained accepted producer token range is unavailable; no source-span reconstruction is used.", "parser_family_text_retention");
    FieldState::unknown()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact() -> CapturedFact {
        CapturedFact::new(
            SemanticFamily::Operations,
            "test_expression",
            vec![],
            RowSide::named("expression", ComparisonScope::global()),
        )
    }

    #[test]
    fn named_model_targets_use_the_readers_exact_expression_receipt() {
        for kind in [
            "var_expectation",
            "pac_expectation",
            "pac_target_nonstationary",
        ] {
            let before = crate::parser::parse(&format!("planner_objective {kind}(foo);"));
            let after = crate::parser::parse(&format!("planner_objective {kind}(bar);"));
            let mut work = RetainedExpressionWork::default();
            let mut owner = fact();
            let old = expression(
                &before,
                &mut owner,
                "objective",
                before.planner_objective_expr,
                &mut work,
            );
            let new = expression(
                &after,
                &mut owner,
                "objective",
                after.planner_objective_expr,
                &mut work,
            );
            assert_ne!(old, new, "missing {kind} model association");
            let encoded = serde_json::to_string(&old).unwrap();
            assert!(encoded.contains("foo"));
            assert!(encoded.contains("named_operator_kind"));
            assert_eq!(before.named_model_operator_exprs.len(), 1);
            assert_eq!(after.named_model_operator_exprs.len(), 1);
        }
    }

    #[test]
    fn missing_named_operator_receipt_does_not_guess_from_spans() {
        let mut model = crate::parser::parse("planner_objective var_expectation(foo);");
        model.named_model_operator_exprs.clear();
        let mut owner = fact();
        let value = expression(
            &model,
            &mut owner,
            "objective",
            model.planner_objective_expr,
            &mut RetainedExpressionWork::default(),
        );
        let encoded = serde_json::to_string(&value).unwrap();
        assert!(encoded.contains("model_name_known"));
        assert!(!encoded.contains("foo"));
        assert!(owner
            .limits
            .iter()
            .any(|limit| { limit.code == "objective_named_operator_association_unavailable" }));
    }

    #[test]
    fn named_model_name_is_charged_before_scalar_clone() {
        let model = crate::parser::parse(&format!(
            "planner_objective var_expectation({});",
            "x".repeat(1000)
        ));
        let mut owner = fact();
        let mut work = RetainedExpressionWork::new(512);
        assert_eq!(
            expression(
                &model,
                &mut owner,
                "objective",
                model.planner_objective_expr,
                &mut work,
            )
            .state,
            ValueState::Unknown
        );
        assert_eq!(work.bytes, 512);
        assert_eq!(work.nodes, 100_000);
    }

    #[test]
    fn absent_literal_spelling_keeps_finite_numeric_value() {
        let mut model = Model::default();
        let one = model.exprs.alloc(ExprKind::Number, Span::default());
        let two = model.exprs.alloc(ExprKind::Number, Span::default());
        model.numeric_literals.insert(one, 1.0);
        model.numeric_literals.insert(two, 2.0);
        let mut work = RetainedExpressionWork::default();
        let mut owner = fact();
        let old = expression(&model, &mut owner, "rhs", Some(one), &mut work);
        let new = expression(&model, &mut owner, "rhs", Some(two), &mut work);
        assert_ne!(old, new);
        assert!(owner
            .limits
            .iter()
            .any(|limit| limit.code == "rhs_number_spelling_unavailable"));
        assert!(serde_json::to_string(&new).unwrap().contains("2.0"));
    }

    #[test]
    fn materialization_bytes_are_charged_before_scalar_clone() {
        let mut model = Model::default();
        let name = model.intern.intern(&"x".repeat(1000));
        let id = model.exprs.alloc(
            ExprKind::Ident {
                name,
                timing: 0,
                ident_span: Span::default(),
                timing_span: None,
            },
            Span::default(),
        );
        let mut work = RetainedExpressionWork::new(512);
        let mut owner = fact();
        assert_eq!(
            expression(&model, &mut owner, "rhs", Some(id), &mut work).state,
            ValueState::Unknown
        );
        assert_eq!(work.nodes, 100_000);
        assert_eq!(work.bytes, 512);
        assert!(owner
            .limits
            .iter()
            .any(|limit| limit.code == "rhs_tree_materialization_limit"));
    }

    #[test]
    fn before_and_after_share_cumulative_node_account() {
        let mut model = Model::default();
        let id = model.exprs.alloc(ExprKind::Number, Span::default());
        model.numeric_literals.insert(id, 1.0);
        let mut work = RetainedExpressionWork {
            nodes: 1,
            bytes: 1024,
            selector_visits: 1000,
        };
        let mut owner = fact();
        assert_eq!(
            expression(&model, &mut owner, "before", Some(id), &mut work).state,
            ValueState::Present
        );
        assert_eq!(
            expression(&model, &mut owner, "after", Some(id), &mut work).state,
            ValueState::Unknown
        );
    }

    #[test]
    fn deep_tree_uses_iterative_bounded_traversal() {
        let mut model = Model::default();
        let mut id = model.exprs.alloc(ExprKind::Number, Span::default());
        model.numeric_literals.insert(id, 1.0);
        for _ in 0..10_000 {
            id = model.exprs.alloc(
                ExprKind::Unary {
                    op: UnOp::Neg,
                    arg: id,
                },
                Span::default(),
            );
        }
        let mut owner = fact();
        let mut work = RetainedExpressionWork::default();
        assert_eq!(
            expression(&model, &mut owner, "rhs", Some(id), &mut work).state,
            ValueState::Unknown
        );
        assert!(owner
            .limits
            .iter()
            .any(|limit| limit.code == "rhs_tree_node_limit"));
    }

    #[test]
    fn missing_string_text_is_explicit_unknown_leaf() {
        let mut model = Model::default();
        let id = model.exprs.alloc(ExprKind::String, Span::default());
        let mut owner = fact();
        let value = expression(
            &model,
            &mut owner,
            "rhs",
            Some(id),
            &mut RetainedExpressionWork::default(),
        );
        assert!(serde_json::to_string(&value)
            .unwrap()
            .contains("text_known"));
        assert!(owner
            .limits
            .iter()
            .any(|limit| limit.code == "rhs_text_unavailable"));
    }

    #[test]
    fn selectors_charge_comparison_wide_visits_before_scan() {
        let model = crate::parser::parse("external_function(name=f,nargs=2);");
        let mut owner = fact();
        owner.side =
            super::super::occurrences::statement_side(&model, 0, "f", ComparisonScope::global());
        let span = model.external_functions[0].name.unwrap().1;
        let mut work = RetainedExpressionWork {
            selector_visits: 1,
            ..RetainedExpressionWork::default()
        };
        assert_eq!(
            span_text(&model, &mut owner, "name", span, &mut work).state,
            ValueState::Unknown
        );
        assert_eq!(work.selector_visits, 1);
        assert!(owner
            .limits
            .iter()
            .any(|limit| limit.code == "name_selector_visit_limit"));
    }

    #[test]
    fn selected_expanded_text_is_charged_before_join() {
        let model = crate::parser::parse("external_function(name=f,nargs=2);");
        let mut owner = fact();
        owner.side =
            super::super::occurrences::statement_side(&model, 0, "f", ComparisonScope::global());
        let span = model.external_functions[0].name.unwrap().1;
        let mut work = RetainedExpressionWork::new(1);
        assert_eq!(
            span_text(&model, &mut owner, "name", span, &mut work).state,
            ValueState::Unknown
        );
        assert_eq!(work.bytes, 1);
        assert!(owner
            .limits
            .iter()
            .any(|limit| limit.code == "name_text_materialization_limit"));
    }
}
