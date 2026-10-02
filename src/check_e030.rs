//! E030 different-types / duplicate `#`, and W031 same-kind duplicate declaration.

use std::collections::HashMap;

use crate::diagnostic::{Diagnostic, RelatedDiagnostic, Severity};
use crate::expr::ExprKind;
use crate::intern::Name;
use crate::model::Model;

pub fn check_e030(model: &Model) -> Vec<Diagnostic> {
    let mut out = check_duplicate_declarations(model);
    out.extend(check_model_local_dups(model));
    out
}

fn check_duplicate_declarations(model: &Model) -> Vec<Diagnostic> {
    // Expanded tokens retain their written spans, so two macro iterations can
    // have the same span. Use the event order captured during parsing instead.
    let mut seen: HashMap<Name, (crate::model::SymbolKind, crate::span::Span, usize)> =
        HashMap::new();
    let mut diagnostics = Vec::new();
    for (event_index, event) in model.symbol_type_events.iter().enumerate() {
        if event.changed {
            if let Some(previous) = seen.get_mut(&event.name) {
                previous.0 = event.kind;
            }
            continue;
        }
        let Some(&(prev_kind, first_span, first_index)) = seen.get(&event.name) else {
            seen.insert(event.name, (event.kind, event.span, event_index));
            continue;
        };
        let name = model.name(event.name);
        if prev_kind == event.kind {
            diagnostics.push(Diagnostic {
                span: event.span,
                severity: Severity::Warning,
                code: "W031".to_string(),
                message: format!("Symbol {name} declared twice."),
                fix: None,
                related: vec![RelatedDiagnostic::type_event(first_span, first_index)],
                tags: Vec::new(),
            });
            continue;
        }
        diagnostics.push(Diagnostic {
            span: event.span,
            severity: Severity::Error,
            code: "E030".to_string(),
            message: format!("Symbol {name} declared twice with different types!"),
            fix: None,
            related: vec![RelatedDiagnostic::type_event(first_span, first_index)],
            tags: Vec::new(),
        });
    }
    diagnostics
}

fn check_model_local_dups(model: &Model) -> Vec<Diagnostic> {
    // `AddLocalVariable` is per data tree. A second `#` of the same name inside
    // one tree refuses (`Local model variable a declared twice.`); the same
    // name in the aggregate model and a heterogeneous block, or in two
    // dimensions, is accepted.
    let mut diagnostics = Vec::new();
    for tree in crate::check_e020::equation_trees(model) {
        let mut seen = HashMap::new();
        for eq in tree {
            if !eq.model_local {
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
                });
            } else {
                seen.insert(*name, (eq, *ident_span));
            }
        }
    }
    diagnostics
}
