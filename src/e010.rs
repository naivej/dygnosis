//! E010 equation-count diagnostics.

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, Severity};
use crate::model::{Equation, Model};
use crate::span::Span;

pub fn check_e010(model: &Model) -> Vec<Diagnostic> {
    if model.model_block.is_none() && model.equations.is_empty() {
        return Vec::new();
    }

    let n_eq = model.equations.iter().filter(|eq| is_dynamic(eq)).count();
    let n_endo = model.endogenous.len();
    if n_eq == 0 && n_endo == 0 {
        return Vec::new();
    }
    if n_eq == n_endo {
        return Vec::new();
    }

    let span = mismatch_span(model);
    if n_endo > n_eq {
        if let Some(linked) = link_unused_endo(model, span, n_eq, n_endo) {
            return vec![linked];
        }
    }
    vec![generic_e010(span, n_eq, n_endo)]
}

fn is_dynamic(eq: &Equation) -> bool {
    !eq.is_local && !eq.static_tag
}

fn is_static(eq: &Equation) -> bool {
    !eq.is_local && !eq.dynamic_tag
}

fn mismatch_span(model: &Model) -> Span {
    match model.model_block {
        Some(span) => span,
        None => {
            let end = model
                .source
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(1);
            Span::new(0, end)
        }
    }
}

fn generic_e010(span: Span, n_eq: usize, n_endo: usize) -> Diagnostic {
    let fix_msg = if n_eq > n_endo {
        format!(
            "Fix: remove {} duplicate/extra equation(s) from the model block, or add {} missing variable(s) to the 'var' declaration.",
            n_eq - n_endo,
            n_eq - n_endo,
        )
    } else {
        format!(
            "Fix: add {} missing equation(s) to the model block, or remove {} extra variable(s) from the 'var' declaration.",
            n_endo - n_eq,
            n_endo - n_eq,
        )
    };
    Diagnostic {
        span,
        severity: Severity::Error,
        code: "E010".to_string(),
        message: format!(
            "Equation count mismatch: {n_eq} equation(s) but {n_endo} endogenous variable(s). {fix_msg}"
        ),
        fix: None,
        tags: Vec::new(),
    }
}

fn link_unused_endo(model: &Model, span: Span, n_eq: usize, n_endo: usize) -> Option<Diagnostic> {
    if !model.equations.iter().any(is_static) {
        return None;
    }

    let mut referenced = HashSet::new();
    for eq in &model.equations {
        for r in model.ident_refs(eq) {
            referenced.insert(r.name);
        }
    }

    let unreferenced: Vec<&str> = model
        .endogenous
        .iter()
        .filter(|d| !referenced.contains(&d.name))
        .map(|d| model.name(d.name))
        .collect();

    let n_missing = n_endo - n_eq;
    if unreferenced.is_empty() || unreferenced.len() > n_missing + 2 {
        return None;
    }

    let names = unreferenced.join(", ");
    let message = if unreferenced.len() == n_missing {
        format!(
            "Equation count mismatch: {n_eq} equation(s) but {n_endo} endogenous variable(s) ({n_missing} extra variable(s)). The unreferenced variable(s) {names} should be removed from the 'var' declaration. Fix: remove {names} from 'var'."
        )
    } else {
        format!(
            "Equation count mismatch: {n_eq} equation(s) but {n_endo} endogenous variable(s) ({n_missing} equation(s) missing). The unreferenced variable(s) {names} likely need equation(s). Fix: look for commented-out or deleted equations involving {names}, and restore or re-add them to the model block."
        )
    };

    Some(Diagnostic {
        span,
        severity: Severity::Error,
        code: "E010".to_string(),
        message,
        fix: None,
        tags: Vec::new(),
    })
}
