//! W060 / W110–W112 shocks: missing block, duplicate specs, variance sign, corr range.

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, Severity};
use crate::intern::Name;
use crate::model::{Decl, Model, ShockKind};
use crate::span::Span;

const FALLBACK: Span = Span { start: 0, end: 1 };

pub fn check_w110(model: &Model) -> Vec<Diagnostic> {
    let mut diagnostics = check_w060(model);
    diagnostics.extend(check_shock_stmts(model));
    diagnostics
}

fn check_w060(model: &Model) -> Vec<Diagnostic> {
    let stochastic = stochastic_exo(model);
    if stochastic.is_empty() {
        return Vec::new();
    }
    if model.shocks_block.is_some() || !model.shocks_vars.is_empty() {
        return Vec::new();
    }
    let names: Vec<&str> = stochastic.iter().map(|d| model.name(d.name)).collect();
    let shown = names.iter().take(5).copied().collect::<Vec<_>>().join(", ");
    let span = nonempty(stochastic[0].span);
    vec![Diagnostic::new(
        span,
        Severity::Warning,
        "W060",
        format!(
            "Exogenous variable(s) declared ({shown}) but no 'shocks' block found. Add a shocks block to define the shock processes."
        ),
    )]
}

fn check_shock_stmts(model: &Model) -> Vec<Diagnostic> {
    let mut seen: HashSet<SeenKey> = HashSet::new();
    let mut diagnostics = Vec::new();
    for stmt in &model.shock_stmts {
        let span = nonempty(stmt.span);
        match &stmt.kind {
            ShockKind::Var(name) => {
                let key = SeenKey::Var(*name);
                if seen.contains(&key) {
                    let n = model.name(*name);
                    diagnostics.push(Diagnostic::new(
                        span,
                        Severity::Warning,
                        "W111",
                        format!(
                            "Shock '{n}' has its variance/standard error specified more than once in the shocks block."
                        ),
                    ));
                }
                seen.insert(key);
                if let Some(v) = stmt.rhs {
                    if v < 0.0 {
                        let n = model.name(*name);
                        diagnostics.push(Diagnostic::new(
                            span,
                            Severity::Warning,
                            "W112",
                            format!(
                                "Shock '{n}' is given a negative variance ({}). A variance must be non-negative.",
                                python_g(v)
                            ),
                        ));
                    }
                }
            }
            ShockKind::Cov(names) => {
                let key = SeenKey::Cov(sorted_names(model, names));
                if seen.contains(&key) && names.len() >= 2 {
                    let first = model.name(names[0]);
                    let second = model.name(names[1]);
                    diagnostics.push(Diagnostic::new(
                        span,
                        Severity::Warning,
                        "W111",
                        format!(
                            "Covariance between '{first}' and '{second}' is specified more than once in the shocks block."
                        ),
                    ));
                }
                seen.insert(key);
            }
            ShockKind::Corr { a, b } => {
                let key = SeenKey::Corr(sorted_names(model, &[*a, *b]));
                if seen.contains(&key) {
                    let first = model.name(*a);
                    let second = model.name(*b);
                    diagnostics.push(Diagnostic::new(
                        span,
                        Severity::Warning,
                        "W111",
                        format!(
                            "Correlation between '{first}' and '{second}' is specified more than once in the shocks block."
                        ),
                    ));
                }
                seen.insert(key);
                if let Some(v) = stmt.rhs {
                    if v.abs() > 1.0 {
                        let first = model.name(*a);
                        let second = model.name(*b);
                        diagnostics.push(Diagnostic::new(
                            span,
                            Severity::Warning,
                            "W110",
                            format!(
                                "Correlation between '{first}' and '{second}' is {}, which is outside the valid range [-1, 1].",
                                python_g(v)
                            ),
                        ));
                    }
                }
            }
        }
    }
    diagnostics
}

fn stochastic_exo(model: &Model) -> Vec<&Decl> {
    let det: HashSet<Name> = model
        .deterministic_exogenous
        .iter()
        .map(|d| d.name)
        .collect();
    model
        .exogenous
        .iter()
        .filter(|d| !det.contains(&d.name))
        .collect()
}

fn sorted_names(model: &Model, names: &[Name]) -> Vec<Name> {
    let mut v = names.to_vec();
    v.sort_by(|a, b| model.name(*a).cmp(model.name(*b)));
    v
}

fn nonempty(span: Span) -> Span {
    if span.is_empty() {
        FALLBACK
    } else {
        span
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum SeenKey {
    Var(Name),
    Cov(Vec<Name>),
    Corr(Vec<Name>),
}

/// Python 3 default `{value:g}` (precision 6).
fn python_g(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value.is_sign_positive() {
            "inf".into()
        } else {
            "-inf".into()
        };
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0".into()
        } else {
            "0".into()
        };
    }

    const P: i32 = 6;
    let sign = if value.is_sign_negative() { "-" } else { "" };
    let abs = value.abs();
    let mut exp = abs.log10().floor() as i32;
    let mut rounded = (abs * 10f64.powi(P - 1 - exp)).round();
    if rounded >= 10f64.powi(P) {
        rounded /= 10.0;
        exp += 1;
    }
    let mantissa = rounded / 10f64.powi(P - 1);

    if !(-4..P).contains(&exp) {
        let mut digits = format!("{mantissa:.5}");
        trim_trailing_zeros(&mut digits);
        format!("{sign}{digits}e{exp:+03}")
    } else {
        let decimals = (P - 1 - exp).max(0) as usize;
        let mut digits = format!("{:.*}", decimals, mantissa * 10f64.powi(exp));
        trim_trailing_zeros(&mut digits);
        format!("{sign}{digits}")
    }
}

fn trim_trailing_zeros(s: &mut String) {
    if !s.contains('.') {
        return;
    }
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
}
