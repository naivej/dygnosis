//! E030 different-types / duplicate `#`, and W031 same-kind duplicate declaration.

use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, Severity};
use crate::expr::ExprKind;
use crate::intern::Name;
use crate::model::{Equation, Model};
use crate::span::Span;

pub fn check_e030(model: &Model) -> Vec<Diagnostic> {
    let mut out = check_duplicate_declarations(model);
    out.extend(check_model_local_dups(model));
    out
}

fn check_duplicate_declarations(model: &Model) -> Vec<Diagnostic> {
    let det_keys: HashSet<(Name, Span)> = model
        .deterministic_exogenous
        .iter()
        .map(|d| (d.name, d.span))
        .collect();

    // Every named symbol the file declares, tagged with its declaration kind:
    // the tag decides W031 (same kind) from E030 (different kinds).
    // Trend names, epilogue helpers and external function names are not `Decl`s
    // but collide the same way.
    let mut all_vars: Vec<(Name, Span, &'static str)> = Vec::new();
    for d in &model.endogenous {
        all_vars.push((d.name, d.span, "var"));
    }
    for d in &model.exogenous {
        if !det_keys.contains(&(d.name, d.span)) {
            all_vars.push((d.name, d.span, "varexo"));
        }
    }
    for d in &model.deterministic_exogenous {
        all_vars.push((d.name, d.span, "varexo_det"));
    }
    for d in &model.parameters {
        all_vars.push((d.name, d.span, "parameters"));
    }
    for trend in &model.trend_vars {
        let kind = if trend.log_trend {
            "log_trend_var"
        } else {
            "trend_var"
        };
        all_vars.push((trend.name, trend.span, kind));
    }
    for assignment in &model.epilogue {
        all_vars.push((assignment.name, assignment.span, "epilogue"));
    }
    for stmt in &model.external_functions {
        if let Some((name, span)) = stmt.name {
            all_vars.push((name, span, "external_function"));
        }
    }
    all_vars.sort_by_key(|(_, span, _)| (span.start, span.end));

    let mut seen: HashMap<Name, &'static str> = HashMap::new();
    let mut diagnostics = Vec::new();
    for (name_id, span, kind) in all_vars {
        let Some(&prev_kind) = seen.get(&name_id) else {
            seen.insert(name_id, kind);
            continue;
        };
        let name = model.name(name_id);
        if prev_kind == kind {
            diagnostics.push(Diagnostic {
                span,
                severity: Severity::Warning,
                code: "W031".to_string(),
                message: format!("Symbol {name} declared twice."),
                fix: None,
                tags: Vec::new(),
            });
            continue;
        }
        diagnostics.push(Diagnostic {
            span,
            severity: Severity::Error,
            code: "E030".to_string(),
            message: format!("Symbol {name} declared twice with different types!"),
            fix: None,
            tags: Vec::new(),
        });
    }
    diagnostics
}

fn check_model_local_dups(model: &Model) -> Vec<Diagnostic> {
    let mut eqs: Vec<&Equation> = model.equations.iter().collect();
    eqs.sort_by_key(|eq| (eq.span.start, eq.span.end));

    let mut seen = HashSet::new();
    let mut diagnostics = Vec::new();
    for eq in eqs {
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
        if !seen.insert(*name) {
            let name = model.name(*name);
            diagnostics.push(Diagnostic {
                span: *ident_span,
                severity: Severity::Error,
                code: "E030".to_string(),
                message: format!("Local model variable {name} declared twice."),
                fix: None,
                tags: Vec::new(),
            });
        }
    }
    diagnostics
}
