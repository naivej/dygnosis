//! W090–W095 estimation: varobs, estimated_params, observation_trends.

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, Severity};
use crate::intern::Name;
use crate::model::{Decl, EstimatedParamKind, Model};
use crate::span::Span;

const FALLBACK: Span = Span { start: 0, end: 1 };

pub fn check_w090(model: &Model) -> Vec<Diagnostic> {
    let has_context = !model.varobs.is_empty()
        || !model.observation_trends.is_empty()
        || !model.estimated_params.is_empty()
        || model.estimated_params_span.is_some()
        || model.varobs_span.is_some();
    if !has_context {
        return Vec::new();
    }

    let endogenous = names(&model.endogenous);
    let exogenous = names(&model.exogenous);
    let parameters = names(&model.parameters);
    let det: HashSet<Name> = model
        .deterministic_exogenous
        .iter()
        .map(|d| d.name)
        .collect();
    let stochastic_exo: HashSet<Name> = model
        .exogenous
        .iter()
        .map(|d| d.name)
        .filter(|n| !det.contains(n))
        .collect();

    let mut diagnostics = Vec::new();
    let mut seen: HashSet<Name> = HashSet::new();
    for v in &model.varobs {
        let name = model.name(v.name);
        if !seen.insert(v.name) {
            diagnostics.push(Diagnostic::new(
                nonempty_or(v.span, model.varobs_span),
                Severity::Warning,
                "W091",
                format!("Observed variable '{name}' is listed more than once in varobs."),
            ));
            continue;
        }
        if !endogenous.contains(&v.name) {
            let why = if exogenous.contains(&v.name) {
                " (it is an exogenous variable)"
            } else if parameters.contains(&v.name) {
                " (it is a parameter)"
            } else {
                ""
            };
            diagnostics.push(Diagnostic::new(
                nonempty_or(v.span, model.varobs_span),
                Severity::Error,
                "E090",
                format!(
                    "varobs variable '{name}' is not a declared endogenous variable{why}. Observed variables must be endogenous."
                ),
            ));
        }
    }

    let mut obs_seen = HashSet::new();
    let mut n_obs = 0usize;
    for v in &model.varobs {
        if !obs_seen.insert(v.name) {
            continue;
        }
        if endogenous.contains(&v.name) {
            n_obs += 1;
        }
    }
    if n_obs > 0 {
        let mut measurement_errors: HashSet<Name> = HashSet::new();
        for e in &model.estimated_params {
            if e.kind == EstimatedParamKind::Stderr && endogenous.contains(&e.name) {
                measurement_errors.insert(e.name);
            }
        }
        for s in &model.shocks_vars {
            if endogenous.contains(s) {
                measurement_errors.insert(*s);
            }
        }
        let n_shocks = stochastic_exo.len() + measurement_errors.len();
        if n_obs > n_shocks {
            diagnostics.push(Diagnostic::new(
                span_or_fallback(model.varobs_span),
                Severity::Warning,
                "W092",
                format!(
                    "Stochastic singularity: {n_obs} observed variable(s) but only {n_shocks} shock(s) (structural shocks plus measurement errors). The likelihood is stochastically singular; add measurement errors or shocks, or reduce the number of observed variables."
                ),
            ));
        }
    }

    for entry in &model.estimated_params {
        let span = span_or_fallback(Some(entry.span));
        let name = model.name(entry.name);
        match entry.kind {
            EstimatedParamKind::Param => {
                if !parameters.contains(&entry.name) {
                    let where_ = if endogenous.contains(&entry.name) {
                        " (it is an endogenous variable)"
                    } else if exogenous.contains(&entry.name) {
                        " (it is an exogenous variable)"
                    } else {
                        ""
                    };
                    diagnostics.push(Diagnostic::new(
                        span,
                        Severity::Error,
                        "E093",
                        format!("estimated_params: '{name}' is not a declared parameter{where_}."),
                    ));
                }
            }
            EstimatedParamKind::Stderr => {
                if !exogenous.contains(&entry.name) && !endogenous.contains(&entry.name) {
                    diagnostics.push(Diagnostic::new(
                        span,
                        Severity::Error,
                        "E093",
                        format!(
                            "estimated_params: stderr '{name}' is not a declared shock or observed variable."
                        ),
                    ));
                }
            }
            EstimatedParamKind::Corr => {
                for symbol in [Some(entry.name), entry.corr_with].into_iter().flatten() {
                    if !exogenous.contains(&symbol) && !endogenous.contains(&symbol) {
                        let symbol = model.name(symbol);
                        diagnostics.push(Diagnostic::new(
                            span,
                            Severity::Error,
                            "E093",
                            format!(
                                "estimated_params: corr references '{symbol}', which is not a declared shock or variable."
                            ),
                        ));
                    }
                }
            }
        }

        if let (Some(lower), Some(upper)) = (entry.lower, entry.upper) {
            if lower >= upper {
                diagnostics.push(Diagnostic::new(
                    span,
                    Severity::Warning,
                    "W094",
                    format!(
                        "estimated_params: '{name}' has lower bound {} >= upper bound {}.",
                        python_g(lower),
                        python_g(upper)
                    ),
                ));
            }
        }
        if let (Some(init), Some(lower), Some(upper)) = (entry.init, entry.lower, entry.upper) {
            if !(lower <= init && init <= upper) {
                diagnostics.push(Diagnostic::new(
                    span,
                    Severity::Warning,
                    "W094",
                    format!(
                        "estimated_params: '{name}' initial value {} is outside its bounds [{}, {}].",
                        python_g(init),
                        python_g(lower),
                        python_g(upper)
                    ),
                ));
            }
        }
    }

    let varobs_set: HashSet<Name> = model.varobs.iter().map(|v| v.name).collect();
    for (name, span) in &model.observation_trends {
        if !varobs_set.contains(name) {
            diagnostics.push(Diagnostic::new(
                nonempty_or(*span, model.varobs_span),
                Severity::Error,
                "E095",
                format!(
                    "observation_trends: '{}' is not listed in varobs.",
                    model.name(*name)
                ),
            ));
        }
    }

    diagnostics
}

fn names(decls: &[Decl]) -> HashSet<Name> {
    decls.iter().map(|d| d.name).collect()
}

fn span_or_fallback(span: Option<Span>) -> Span {
    span.unwrap_or(FALLBACK)
}

fn nonempty_or(span: Span, fallback: Option<Span>) -> Span {
    if span.is_empty() {
        span_or_fallback(fallback)
    } else {
        span
    }
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
