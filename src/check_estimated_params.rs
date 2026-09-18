//! Estimated-params* duplicates, value-used, skew type, and beta 0.5/0.5.

use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, Severity};
use crate::expr::ExprId;
use crate::intern::Name;
use crate::model::{EstimatedParam, EstimatedParamKind, Model};
use crate::span::Span;

pub fn check_estimated_params(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    out.extend(check_one_block(
        model,
        "estimated_params",
        &model.estimated_params,
        true,
    ));
    out.extend(check_one_block(
        model,
        "estimated_params_init",
        &model.estimated_params_init,
        false,
    ));
    out.extend(check_one_block(
        model,
        "estimated_params_bounds",
        &model.estimated_params_bounds,
        false,
    ));
    out
}

fn check_one_block(
    model: &Model,
    block: &str,
    entries: &[EstimatedParam],
    main: bool,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let exo: HashSet<Name> = model
        .exogenous
        .iter()
        .map(|d| d.name)
        .filter(|n| {
            !model
                .deterministic_exogenous
                .iter()
                .any(|det| det.name == *n)
        })
        .collect();
    let declared: HashSet<Name> = model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.parameters)
        .map(|d| d.name)
        .collect();
    let mut seen_param: HashMap<Name, Span> = HashMap::new();
    let mut seen_stderr: HashMap<Name, Span> = HashMap::new();
    let mut seen_skew: HashMap<Name, Span> = HashMap::new();
    let mut seen_corr: HashMap<(Name, Name), (String, String)> = HashMap::new();

    for entry in entries {
        let name = model.name(entry.name);
        match entry.kind {
            EstimatedParamKind::Param => {
                if let Some(_first) = seen_param.get(&entry.name) {
                    out.push(err(
                        entry.span,
                        "E244",
                        format!("in `{block}' block, the symbol {name} is declared twice."),
                    ));
                }
                seen_param.insert(entry.name, entry.span);
            }
            EstimatedParamKind::Stderr => {
                if seen_stderr.contains_key(&entry.name) {
                    out.push(err(
                        entry.span,
                        "E245",
                        format!("in `{block}' block, the stderr of {name} is declared twice."),
                    ));
                }
                seen_stderr.insert(entry.name, entry.span);
            }
            EstimatedParamKind::Corr => {
                if let Some(other) = entry.corr_with {
                    let key = sorted_pair(model, entry.name, other);
                    if let Some((a, b)) = seen_corr.get(&key) {
                        out.push(err(
                            entry.span,
                            "E246",
                            format!(
                                "in `{block}' block, the correlation between {a} and {b} is declared twice."
                            ),
                        ));
                    } else {
                        seen_corr.insert(
                            key,
                            (
                                model.name(entry.name).to_string(),
                                model.name(other).to_string(),
                            ),
                        );
                    }
                }
            }
            EstimatedParamKind::Skew => {
                if seen_skew.contains_key(&entry.name) {
                    out.push(err(
                        entry.span,
                        "E247",
                        format!("in `{block}' block, the skewness of {name} is declared twice."),
                    ));
                }
                seen_skew.insert(entry.name, entry.span);
                if main && declared.contains(&entry.name) && !exo.contains(&entry.name) {
                    out.push(err(
                        entry.span,
                        "E249",
                        format!(
                            "in `estimated_params' block, skewness can only be specified for exogenous variables, not for '{name}'."
                        ),
                    ));
                }
            }
        }
        if main
            && entry.kind == EstimatedParamKind::Param
            && entry.prior_beta
            && fold_is_half(model, entry.mean_expr)
            && fold_is_half(model, entry.std_expr)
        {
            out.push(err(
                entry.span,
                "E250",
                "The prior density is not defined for the beta distribution when the mean = standard deviation = 0.5.",
            ));
        }
    }

    let declared_params: HashSet<Name> = entries
        .iter()
        .filter(|e| e.kind == EstimatedParamKind::Param)
        .filter(|e| !model.name(e.name).eq_ignore_ascii_case("dsge_prior_weight"))
        .map(|e| e.name)
        .collect();
    if declared_params.is_empty() {
        return out;
    }
    for entry in entries {
        for id in [
            entry.init_expr,
            entry.lower_expr,
            entry.upper_expr,
            entry.mean_expr,
            entry.std_expr,
        ]
        .into_iter()
        .flatten()
        {
            for r in model.exprs.walk_idents(id) {
                if !declared_params.contains(&r.name) {
                    continue;
                }
                if entry.kind == EstimatedParamKind::Param && r.name == entry.name {
                    continue;
                }
                let used = model.name(r.name);
                let target = decl_target(model, entry);
                out.push(err(
                    entry.span,
                    "E248",
                    format!(
                        "in `{block}' block, the value of estimated parameter {used} is used in the declaration for {target}. This behaviour is undefined."
                    ),
                ));
                break;
            }
        }
    }
    out
}

fn decl_target(model: &Model, entry: &EstimatedParam) -> String {
    match entry.kind {
        EstimatedParamKind::Corr => {
            let a = model.name(entry.name);
            let b = entry
                .corr_with
                .map(|n| model.name(n))
                .unwrap_or("");
            format!("correlation between {a} and {b}")
        }
        EstimatedParamKind::Skew => format!("skewness of {}", model.name(entry.name)),
        EstimatedParamKind::Param | EstimatedParamKind::Stderr => {
            model.name(entry.name).to_string()
        }
    }
}

fn fold_is_half(model: &Model, id: Option<ExprId>) -> bool {
    let Some(id) = id else {
        return false;
    };
    model.exprs.get(id).interned == Some(0.5)
}

fn sorted_pair(model: &Model, a: Name, b: Name) -> (Name, Name) {
    if model.name(a) <= model.name(b) {
        (a, b)
    } else {
        (b, a)
    }
}

fn err(span: Span, code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(span, Severity::Error, code, message)
}
