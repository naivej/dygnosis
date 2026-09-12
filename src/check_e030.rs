//! E030 duplicate-declaration and duplicate model-local `#` diagnostics.

use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, Severity};
use crate::expr::ExprKind;
use crate::intern::Name;
use crate::model::{Decl, Equation, Model};
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

    let mut all_vars: Vec<(&Decl, &'static str)> = Vec::new();
    for d in &model.endogenous {
        all_vars.push((d, "var"));
    }
    for d in &model.exogenous {
        if !det_keys.contains(&(d.name, d.span)) {
            all_vars.push((d, "varexo"));
        }
    }
    for d in &model.deterministic_exogenous {
        all_vars.push((d, "varexo_det"));
    }
    for d in &model.parameters {
        all_vars.push((d, "parameters"));
    }
    all_vars.sort_by_key(|(d, _)| (d.span.start, d.span.end));

    let assigned_params: HashSet<Name> = model.param_assignments.iter().map(|a| a.name).collect();
    let shocks: HashSet<Name> = model.shocks_vars.iter().copied().collect();

    let mut timed = HashSet::new();
    let mut lhs_untimed = HashSet::new();
    let mut in_eqs = HashSet::new();
    for eq in &model.equations {
        for r in model.ident_refs(eq) {
            in_eqs.insert(r.name);
            if r.timing != 0 {
                timed.insert(r.name);
            }
        }
        if let Some(name) = lhs_untimed_ident(model, eq) {
            lhs_untimed.insert(name);
        }
    }

    let mut seen: HashMap<Name, &'static str> = HashMap::new();
    let mut diagnostics = Vec::new();
    for (decl, kind) in all_vars {
        let Some(&prev_kind) = seen.get(&decl.name) else {
            seen.insert(decl.name, kind);
            continue;
        };
        let name = model.name(decl.name);
        let (severity, message) = if prev_kind == kind {
            (
                Severity::Error,
                format!(
                    "'{name}' is declared more than once in '{kind}'. Fix: remove the redundant '{name}' from the {kind} declaration."
                ),
            )
        } else {
            let hint = cross_kind_hint(
                name,
                prev_kind,
                kind,
                decl.name,
                CrossKindUse {
                    assigned_params: &assigned_params,
                    timed: &timed,
                    lhs_untimed: &lhs_untimed,
                    shocks: &shocks,
                    in_eqs: &in_eqs,
                },
            );
            (
                Severity::Error,
                format!("'{name}' is declared in both '{prev_kind}' and '{kind}'.{hint}"),
            )
        };
        diagnostics.push(Diagnostic {
            span: decl.span,
            severity,
            code: "E030".to_string(),
            message,
            fix: None,
            tags: Vec::new(),
        });
    }
    diagnostics
}

struct CrossKindUse<'a> {
    assigned_params: &'a HashSet<Name>,
    timed: &'a HashSet<Name>,
    lhs_untimed: &'a HashSet<Name>,
    shocks: &'a HashSet<Name>,
    in_eqs: &'a HashSet<Name>,
}

fn cross_kind_hint(
    name: &str,
    prev_kind: &str,
    kind: &str,
    id: Name,
    uses: CrossKindUse<'_>,
) -> String {
    let pair = kinds_pair(prev_kind, kind);
    if uses.assigned_params.contains(&id) {
        let wrong_kind = if prev_kind == "parameters" && kind != "parameters" {
            kind
        } else if kind == "parameters" && prev_kind != "parameters" {
            prev_kind
        } else if prev_kind == "var" || kind == "var" {
            "var"
        } else {
            prev_kind
        };
        return format!(
            " Since '{name}' has a value assignment (like a parameter), it likely belongs in 'parameters'. Fix: remove '{name}' from the '{wrong_kind}' declaration."
        );
    }
    if uses.timed.contains(&id) && pair == ("var", "varexo") {
        return format!(
            " Since '{name}' appears with time subscripts in equations (e.g. {name}(-1) or {name}(+1)), it is likely an endogenous variable. Fix: remove '{name}' from the 'varexo' declaration."
        );
    }
    if pair == ("var", "varexo") {
        if uses.lhs_untimed.contains(&id) && !uses.shocks.contains(&id) {
            return format!(
                " Since '{name}' appears on the LHS of a model equation (it is solved for), it is likely an endogenous variable. Fix: remove '{name}' from the 'varexo' declaration."
            );
        }
        if uses.shocks.contains(&id) {
            return format!(
                " Since '{name}' is referenced in the shocks block, it is likely an exogenous variable. Fix: remove '{name}' from the 'var' declaration."
            );
        }
        if uses.in_eqs.contains(&id) {
            return format!(
                " Since '{name}' appears in model equations and is not referenced in the shocks block, it is likely an endogenous variable. Fix: remove '{name}' from the 'varexo' declaration."
            );
        }
        return generic_exact_one(name, prev_kind, kind);
    }
    if pair == ("varexo", "varexo_det") {
        return format!(
            " Fix: '{name}' cannot appear in both stochastic and deterministic exogenous declarations. Remove the duplicate from the '{kind}' declaration."
        );
    }
    generic_exact_one(name, prev_kind, kind)
}

fn kinds_pair<'a>(a: &'a str, b: &'a str) -> (&'a str, &'a str) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

fn generic_exact_one(name: &str, prev_kind: &str, kind: &str) -> String {
    format!(
        " Fix: '{name}' must appear in exactly one of '{prev_kind}' or '{kind}'. Remove it from whichever declaration is incorrect."
    )
}

fn lhs_untimed_ident(model: &Model, eq: &Equation) -> Option<Name> {
    let id = eq.lhs_expr?;
    match &model.exprs.get(id).kind {
        ExprKind::Ident { name, timing, .. } if *timing == 0 => Some(*name),
        _ => None,
    }
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
                message: format!(
                    "Model-local variable '{name}' is declared twice. Fix: remove the duplicate # definition or give it a different name."
                ),
                fix: None,
                tags: Vec::new(),
            });
        }
    }
    diagnostics
}
