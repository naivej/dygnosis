//! W060 / W110–W112 shocks: requested IRFs, duplicate specs, variance sign, corr range.

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, Severity};
use crate::intern::Name;
use crate::lexer::{tokenize, TokenKind};
use crate::model::{Decl, EstimatedParamKind, Model, PeriodPoint, ShockBlockKind, ShockKind};
use crate::span::Span;

const FALLBACK: Span = Span { start: 0, end: 1 };

pub fn check_w110(model: &Model) -> Vec<Diagnostic> {
    let mut diagnostics = check_w060(model);
    diagnostics.extend(check_shock_stmts(model));
    diagnostics.extend(check_shock_types(model));
    diagnostics.extend(check_e212(model));
    diagnostics
}

fn check_w060(model: &Model) -> Vec<Diagnostic> {
    let stochastic = stochastic_exo(model);
    if stochastic.is_empty() || model.source.contains("@#") || model.source.contains("@{") {
        return Vec::new();
    }
    let exo: HashSet<Name> = stochastic.iter().map(|d| d.name).collect();
    let mut specified = HashSet::new();
    let mut next_block = 0;
    let mut out = Vec::new();
    for request in &model.stoch_simul_requests {
        let estimated: HashSet<Name> = model
            .estimated_params
            .iter()
            .filter(|row| {
                row.kind == EstimatedParamKind::Stderr
                    && row.span.start < request.span.start
                    && exo.contains(&row.name)
            })
            .map(|row| row.name)
            .collect();
        while let Some(block) = model.shock_blocks.get(next_block) {
            if block.span.start >= request.span.start {
                break;
            }
            next_block += 1;
            let regular = block.kind == ShockBlockKind::Regular
                || (block.kind == ShockBlockKind::LearntIn
                    && matches!(
                        block.options.learnt_in.as_ref(),
                        Some(PeriodPoint::Integer(1))
                    ));
            if !regular {
                continue;
            }
            if block.options.overwrite {
                specified.clear();
            }
            for stmt in &block.stochastic {
                if let ShockKind::Var(name) | ShockKind::Stderr(name) = &stmt.kind {
                    if exo.contains(name) {
                        specified.insert(*name);
                    }
                }
            }
        }
        let prior = &model.source[..request.span.start as usize];
        if has_verbatim(prior) {
            // Verbatim MATLAB may set M_.Sigma_e or a shock standard error.
            continue;
        }
        if request.irf.is_some_and(|(value, _)| value == 0) {
            continue;
        }
        if let Some(selected) = &request.irf_shocks {
            let mut seen = HashSet::new();
            for &(name, span) in selected {
                if !exo.contains(&name)
                    || specified.contains(&name)
                    || estimated.contains(&name)
                    || !seen.insert(name)
                {
                    continue;
                }
                out.push(Diagnostic::new(
                    nonempty(span),
                    Severity::Warning,
                    "W060",
                    format!(
                        "stoch_simul requests an IRF for '{}', but no stochastic shock size is specified.",
                        model.name(name)
                    ),
                ));
            }
        } else if let Some((irf, span)) = request.irf {
            if irf > 0 && specified.is_empty() && estimated.is_empty() {
                out.push(Diagnostic::new(
                    nonempty(span),
                    Severity::Warning,
                    "W060",
                    "stoch_simul requests IRFs, but no stochastic shock size is specified.",
                ));
            }
        }
    }
    out
}

fn check_shock_stmts(model: &Model) -> Vec<Diagnostic> {
    let mut seen: HashSet<SeenKey> = HashSet::new();
    let mut diagnostics = Vec::new();
    for (index, stmt) in model.shock_stmts.iter().enumerate() {
        if model.shock_stmt_block_starts.binary_search(&index).is_ok() {
            seen.clear();
        }
        let span = nonempty(stmt.span);
        match &stmt.kind {
            ShockKind::Var(name) | ShockKind::Stderr(name) => {
                let key = SeenKey::Var(*name);
                if seen.contains(&key) {
                    let n = model.name(*name);
                    diagnostics.push(Diagnostic::new(
                        span,
                        Severity::Error,
                        "E111",
                        format!("shocks: variance or stderr of shock on {n} declared twice"),
                    ));
                }
                seen.insert(key);
                if let (ShockKind::Var(_), Some(v)) = (&stmt.kind, stmt.rhs) {
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
                let key = SeenKey::Pair(sorted_names(model, names));
                if seen.contains(&key) && names.len() >= 2 {
                    let first = model.name(names[0]);
                    let second = model.name(names[1]);
                    diagnostics.push(Diagnostic::new(
                        span,
                        Severity::Error,
                        "E111",
                        format!(
                            "shocks: covariance or correlation shock on variable pair ({first}, {second}) declared twice"
                        ),
                    ));
                }
                seen.insert(key);
            }
            ShockKind::Skew(names) => {
                let key = if names.len() == 1 {
                    SeenKey::Skew(vec![names[0]; 3])
                } else {
                    SeenKey::Skew(sorted_names(model, names))
                };
                if !seen.insert(key) {
                    if names.len() == 1 {
                        diagnostics.push(Diagnostic::new(
                            span,
                            Severity::Error,
                            "E393",
                            format!(
                                "shocks: skewness of {} declared twice",
                                model.name(names[0])
                            ),
                        ));
                    } else if names.len() == 3 {
                        diagnostics.push(Diagnostic::new(
                            span,
                            Severity::Error,
                            "E394",
                            format!(
                                "shocks: co-skewness of ({}, {}, {}) declared twice",
                                model.name(names[0]),
                                model.name(names[1]),
                                model.name(names[2])
                            ),
                        ));
                    }
                }
            }
            ShockKind::Corr { a, b } => {
                let key = SeenKey::Pair(sorted_names(model, &[*a, *b]));
                if seen.contains(&key) {
                    let first = model.name(*a);
                    let second = model.name(*b);
                    diagnostics.push(Diagnostic::new(
                        span,
                        Severity::Error,
                        "E111",
                        format!(
                            "shocks: covariance or correlation shock on variable pair ({first}, {second}) declared twice"
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

fn check_shock_types(model: &Model) -> Vec<Diagnostic> {
    let det: HashSet<Name> = model
        .deterministic_exogenous
        .iter()
        .map(|d| d.name)
        .collect();
    let exo: HashSet<Name> = model
        .exogenous
        .iter()
        .map(|d| d.name)
        .filter(|n| !det.contains(n))
        .collect();
    let obs: HashSet<Name> = model.varobs.iter().map(|v| v.name).collect();
    let mut out = Vec::new();
    for stmt in &model.shock_stmts {
        let span = nonempty(stmt.span);
        match &stmt.kind {
            ShockKind::Var(name) => {
                if !exo.contains(name) && !obs.contains(name) {
                    let n = model.name(*name);
                    out.push(Diagnostic::new(
                        span,
                        Severity::Error,
                        "E266",
                        format!(
                            "shocks: setting a variance on '{n}' is not allowed, because it is neither an exogenous variable nor an observed endogenous variable"
                        ),
                    ));
                }
            }
            ShockKind::Stderr(name) => {
                if !exo.contains(name) && !obs.contains(name) {
                    let n = model.name(*name);
                    out.push(Diagnostic::new(
                        span,
                        Severity::Error,
                        "E267",
                        format!(
                            "shocks: setting a standard error on '{n}' is not allowed, because it is neither an exogenous variable nor an observed endogenous variable"
                        ),
                    ));
                }
            }
            ShockKind::Cov(names) if names.len() >= 2 => {
                let a = names[0];
                let b = names[1];
                let both_exo = exo.contains(&a) && exo.contains(&b);
                let both_obs = obs.contains(&a) && obs.contains(&b);
                if !both_exo && !both_obs {
                    out.push(Diagnostic::new(
                        span,
                        Severity::Error,
                        "E268",
                        format!(
                            "shocks: setting a covariance between '{}' and '{}'is not allowed; covariances can only be specified for exogenous or observed endogenous variables of same type",
                            model.name(a),
                            model.name(b)
                        ),
                    ));
                }
            }
            ShockKind::Corr { a, b } => {
                let both_exo = exo.contains(a) && exo.contains(b);
                let both_obs = obs.contains(a) && obs.contains(b);
                if !both_exo && !both_obs {
                    out.push(Diagnostic::new(
                        span,
                        Severity::Error,
                        "E269",
                        format!(
                            "shocks: setting a correlation between '{}' and '{}'is not allowed; correlations can only be specified for exogenous or observed endogenous variables of same type",
                            model.name(*a),
                            model.name(*b)
                        ),
                    ));
                }
            }
            ShockKind::Skew(names) if names.iter().any(|n| !exo.contains(n)) => {
                let a = names.first().map(|n| model.name(*n)).unwrap_or("");
                let b = names.get(1).map(|n| model.name(*n)).unwrap_or(a);
                let c = names.get(2).map(|n| model.name(*n)).unwrap_or(a);
                out.push(Diagnostic::new(
                    span,
                    Severity::Error,
                    "E270",
                    format!(
                        "shocks: setting skewness for '{a}', '{b}', '{c}' is not allowed; skewness can only be specified for exogenous variables"
                    ),
                ));
            }
            _ => {}
        }
    }
    out
}

fn check_e212(model: &Model) -> Vec<Diagnostic> {
    let estimated: HashSet<Name> = model
        .estimated_params
        .iter()
        .filter(|p| p.kind == EstimatedParamKind::Param)
        .filter(|p| !model.name(p.name).eq_ignore_ascii_case("dsge_prior_weight"))
        .map(|p| p.name)
        .collect();
    if estimated.is_empty() {
        return Vec::new();
    }
    let param_names: HashSet<Name> = model.parameters.iter().map(|d| d.name).collect();
    let mut hits: Vec<Name> = Vec::new();
    for stmt in &model.shock_stmts {
        let Some(id) = stmt.rhs_expr else {
            continue;
        };
        for r in model.exprs.walk_idents(id) {
            if estimated.contains(&r.name)
                && param_names.contains(&r.name)
                && !hits.contains(&r.name)
            {
                hits.push(r.name);
            }
        }
    }
    if hits.is_empty() {
        return Vec::new();
    }
    hits.sort_by(|a, b| model.name(*a).cmp(model.name(*b)));
    let listed = hits
        .iter()
        .map(|n| model.name(*n))
        .collect::<Vec<_>>()
        .join(", ");
    let span = model
        .estimated_params_span
        .or(model.shocks_block)
        .unwrap_or(FALLBACK);
    vec![Diagnostic::new(
        span,
        Severity::Error,
        "E212",
        format!(
            "some estimated parameters ({listed}) also appear in the expressions defining the variance/covariance matrix of shocks; this is not allowed."
        ),
    )]
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
    Pair(Vec<Name>),
    Skew(Vec<Name>),
}

fn has_verbatim(source: &str) -> bool {
    tokenize(source).windows(2).any(|pair| {
        pair[0].kind == TokenKind::Ident
            && pair[0].text(source).eq_ignore_ascii_case("verbatim")
            && pair[1].kind == TokenKind::Semi
    })
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
