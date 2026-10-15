//! E030 different-types / duplicate `#`, and W031 same-kind duplicate declaration.

use std::collections::HashMap;

use crate::engine::diagnostics::{Diagnostic, RelatedDiagnostic, Severity};
use crate::engine::language::expr::ExprKind;
use crate::engine::language::intern::Name;
use crate::engine::language::model::Model;

pub fn check_e030(model: &Model) -> Vec<Diagnostic> {
    parse_receipts(model)
        .into_iter()
        .map(|(diagnostic, _)| diagnostic)
        .collect()
}

pub(crate) fn parse_receipts(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut orders = Vec::new();
    let mut out = check_duplicate_declarations(model, &mut orders, &mut Vec::new());
    out.extend(check_model_local_dups(model, &mut orders));
    out.into_iter().zip(orders).collect()
}

pub(crate) fn declaration_callback_order(model: &Model, event_index: usize) -> usize {
    if let Some(order) = model.type_event_callback_orders.get(&event_index) {
        return *order;
    }
    if let Some(written) = model
        .written_declarations
        .iter()
        .find(|row| row.declaration.symbol_type_context.index() == event_index + 1)
        && let Some(statement) = model.statements.get(written.statement_id)
    {
        return statement.token_range.end;
    }
    model
        .type_event_occurrences
        .iter()
        .find(|(index, _)| *index == event_index)
        .map_or(0, |(_, range)| range.end)
}

/// The failing declaration event is part of the same sequential driver loop
/// as earlier successful names, even when they share one statement boundary.
pub(crate) fn conflicting_declaration_events(model: &Model) -> Vec<(usize, usize)> {
    let mut orders = Vec::new();
    let mut events = Vec::new();
    let diagnostics = check_duplicate_declarations(model, &mut orders, &mut events);
    diagnostics
        .into_iter()
        .zip(orders)
        .zip(events)
        .filter_map(|((diagnostic, order), event)| {
            (diagnostic.severity == Severity::Error).then_some((event, order))
        })
        .collect()
}

fn check_duplicate_declarations(
    model: &Model,
    orders: &mut Vec<usize>,
    events: &mut Vec<usize>,
) -> Vec<Diagnostic> {
    // Expanded tokens retain their written spans, so two macro iterations can
    // have the same span. Use the event order captured during parsing instead.
    let mut seen: HashMap<
        Name,
        (
            crate::engine::language::model::SymbolKind,
            bool,
            crate::engine::language::span::Span,
            usize,
        ),
    > = HashMap::new();
    let mut diagnostics = Vec::new();
    for (event_index, event) in model.symbol_type_events.iter().enumerate() {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if event.changed {
            if let Some(previous) = seen.get_mut(&event.name) {
                previous.0 = event.kind;
                previous.1 = false;
            }
            continue;
        }
        let heterogeneous = model
            .written_declarations
            .iter()
            .find(|row| row.declaration.symbol_type_context.index() == event_index + 1)
            .is_some_and(|row| row.declaration.heterogeneity.is_some());
        let Some(&(prev_kind, previous_heterogeneous, first_span, first_index)) =
            seen.get(&event.name)
        else {
            seen.insert(
                event.name,
                (event.kind, heterogeneous, event.span, event_index),
            );
            continue;
        };
        let name = model.name(event.name);
        if prev_kind == event.kind && previous_heterogeneous == heterogeneous {
            orders.push(declaration_callback_order(model, event_index));
            events.push(event_index);
            diagnostics.push(Diagnostic {
                span: event.span,
                severity: Severity::Warning,
                code: "W031".to_string(),
                message: format!("Symbol {name} declared twice."),
                fix: None,
                related: vec![RelatedDiagnostic::type_event(first_span, first_index)],
                tags: Vec::new(),
                model_dimension: None,
                display_keyword: None,
                writing: None,
            });
            continue;
        }
        orders.push(declaration_callback_order(model, event_index));
        events.push(event_index);
        diagnostics.push(Diagnostic {
            span: event.span,
            severity: Severity::Error,
            code: "E030".to_string(),
            message: format!("Symbol {name} declared twice with different types!"),
            fix: None,
            related: vec![RelatedDiagnostic::type_event(first_span, first_index)],
            tags: Vec::new(),
            model_dimension: None,
            display_keyword: None,
            writing: None,
        });
    }
    diagnostics
}

fn check_model_local_dups(model: &Model, orders: &mut Vec<usize>) -> Vec<Diagnostic> {
    // `AddLocalVariable` is per data tree. A second `#` of the same name inside
    // one tree refuses (`Local model variable a declared twice.`); the same
    // name in the aggregate model and a heterogeneous block, or in two
    // dimensions, is accepted.
    let mut diagnostics = Vec::new();
    for tree in crate::engine::diagnostics::check_e020::equation_trees(model) {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let mut seen = HashMap::new();
        for eq in tree {
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            if !eq.model_local || !model.model_local_action_attempted(eq) {
                continue;
            }
            let Some(id) = eq.lhs_expr else {
                continue;
            };
            let ExprKind::Ident {
                name, ident_span, ..
            } = &model.exprs.get(id).kind
            else {
                continue;
            };
            if let Some(&(first_equation, first_span)) = seen.get(name) {
                let name = model.name(*name);
                orders.push(eq.active_tokens.end);
                diagnostics.push(Diagnostic {
                    span: *ident_span,
                    severity: Severity::Error,
                    code: "E030".to_string(),
                    message: format!("Local model variable {name} declared twice."),
                    fix: None,
                    related: vec![RelatedDiagnostic::equation(
                        model,
                        first_equation,
                        first_span,
                    )],
                    tags: Vec::new(),
                    model_dimension: None,
                    display_keyword: None,
                    writing: None,
                });
            } else {
                seen.insert(*name, (eq, *ident_span));
            }
        }
    }
    diagnostics
}
