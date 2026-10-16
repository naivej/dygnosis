//! W090–W095 estimation: varobs, estimated_params, observation_trends.

use std::collections::{HashMap, HashSet};

use crate::engine::diagnostics::{Diagnostic, RelatedDiagnostic, Severity};
use crate::engine::language::intern::Name;
use crate::engine::language::model::{EstimatedParam, EstimatedParamKind, Model, ObservedVar};
use crate::engine::language::span::Span;

const FALLBACK: Span = Span { start: 0, end: 1 };

pub fn check_w090(model: &Model) -> Vec<Diagnostic> {
    let has_context = !model.varobs.is_empty()
        || !model.observation_trends.is_empty()
        || !model.estimated_params.is_empty()
        || !model.deterministic_trends_dups.is_empty()
        || model.estimated_params_span.is_some()
        || model.varobs_span.is_some();
    if !has_context {
        return Vec::new();
    }

    let endogenous: HashSet<Name> = model.final_endogenous().iter().map(|d| d.name).collect();
    let stochastic_exo: HashSet<Name> = model
        .final_decls(&["varexo"])
        .iter()
        .map(|d| d.name)
        .collect();

    let mut diagnostics = Vec::new();
    let mut seen: HashMap<Name, Span> = HashMap::new();
    for v in &model.varobs {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let name = model.name(v.name);
        if let Some(&first) = seen.get(&v.name) {
            diagnostics.push(
                Diagnostic::new(
                    nonempty_or(v.span, model.varobs_span),
                    Severity::Warning,
                    "W091",
                    format!("Observed variable '{name}' is listed more than once in varobs."),
                )
                .with_related(RelatedDiagnostic::new(first, "Earlier observed variable")),
            );
            continue;
        }
        seen.insert(v.name, v.span);
        diagnostics.extend(observed_variable_refusals(model, v));
    }

    diagnostics.extend(
        head_parse_refusals(model)
            .into_iter()
            .map(|(diagnostic, _)| diagnostic),
    );

    let mut obs_seen = HashSet::new();
    let mut n_obs = 0usize;
    for v in &model.varobs {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
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
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            if e.kind == EstimatedParamKind::Stderr && endogenous.contains(&e.name) {
                measurement_errors.insert(e.name);
            }
        }
        for s in &model.shocks_vars {
            if crate::engine::control::cancelled() {
                return Default::default();
            }
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
                    "{n_obs} observed variable(s) but only {n_shocks} shock source(s) found (structural shocks plus measurement errors). Review the observed variables and shock sources before estimation."
                ),
            ));
        }
    }

    for entry in &model.estimated_params {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let span = span_or_fallback(Some(entry.span));
        let name = model.name(entry.name);
        diagnostics.extend(estimated_parameter_refusals(model, entry));

        if let (Some(lower), Some(upper)) = (entry.lower, entry.upper)
            && lower >= upper
        {
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
        if let (Some(init), Some(lower), Some(upper)) = (entry.init, entry.lower, entry.upper)
            && !(lower <= init && init <= upper)
        {
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

    let varobs_set: HashSet<Name> = model.varobs.iter().map(|v| v.name).collect();
    for (name, span) in &model.observation_trends {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if !varobs_set.contains(name) {
            diagnostics.push(Diagnostic::new(
                nonempty_or(*span, model.varobs_span),
                Severity::Error,
                "E095",
                format!(
                    "variable {} in observation_trends block is not an observed variable",
                    model.name(*name)
                ),
            ));
        }
    }

    diagnostics
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
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
}

/// The row's type action is a Parse callback; bounds and estimation checks are not.
fn estimated_parameter_refusals(model: &Model, entry: &EstimatedParam) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let span = span_or_fallback(Some(entry.span));
    let name = model.name(entry.name);
    match entry.kind {
        EstimatedParamKind::Param => {
            if model.symbol_kind_in_context(entry.name, entry.symbol_type_context)
                != Some("parameters")
                && !name.eq_ignore_ascii_case("dsge_prior_weight")
            {
                let where_ = if model.symbol_kind_in_context(entry.name, entry.symbol_type_context)
                    == Some("var")
                {
                    " (it is an endogenous variable)"
                } else if matches!(
                    model.symbol_kind_in_context(entry.name, entry.symbol_type_context),
                    Some("varexo" | "varexo_det")
                ) {
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
            if !matches!(
                model.symbol_kind_in_context(entry.name, entry.symbol_type_context),
                Some("var" | "varexo")
            ) {
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
            let first_kind = model.symbol_kind_in_context(entry.name, entry.symbol_type_context);
            let second_kind = entry
                .corr_with
                .and_then(|name| model.symbol_kind_in_context(name, entry.symbol_type_context));
            let mismatch = first_kind.is_some()
                && second_kind.is_some()
                && (!matches!(first_kind, Some("var" | "varexo")) || first_kind != second_kind);
            if let Some(other) = entry.corr_with.filter(|_| mismatch) {
                diagnostics.push(Diagnostic::new(
                    span,
                    Severity::Error,
                    "E093",
                    format!(
                        "{name} and {} must either be both endogenous variables or both exogenous",
                        model.name(other)
                    ),
                ));
            } else {
                for symbol in [Some(entry.name), entry.corr_with].into_iter().flatten() {
                    if crate::engine::control::cancelled() {
                        return Default::default();
                    }
                    if !matches!(
                        model.symbol_kind_in_context(symbol, entry.symbol_type_context),
                        Some("var" | "varexo")
                    ) {
                        diagnostics.push(Diagnostic::new(span, Severity::Error, "E093", format!("estimated_params: corr references '{}', which is not a declared shock or variable.", model.name(symbol))));
                    }
                }
            }
        }
        EstimatedParamKind::Skew => {
            let declared = model
                .symbol_kind_in_context(entry.name, entry.symbol_type_context)
                .is_some();
            if !declared {
                diagnostics.push(Diagnostic::new(
                        span,
                        Severity::Error,
                        "E093",
                        format!(
                            "estimated_params: skew '{name}' is not a declared shock or observed variable."
                        ),
                    ));
            }
        }
    }

    diagnostics
}

/// Parse-only receipts, emitted by the same type and duplicate predicates as
/// the diagnostic family. Check-stage observation matching and estimation bounds stay outside.
pub(crate) fn parse_refusals(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut rows = head_parse_refusals(model);
    let mut observed = HashSet::new();
    for (row, receipt) in model
        .varobs
        .iter()
        .zip(model.fact_receipts.get("varobs").into_iter().flatten())
    {
        if observed.insert(row.name) {
            rows.extend(
                observed_variable_refusals(model, row)
                    .into_iter()
                    .map(|diagnostic| (diagnostic, receipt.parse_order + 1)),
            );
        }
    }
    for entry in &model.estimated_params {
        if crate::engine::control::cancelled() {
            return Vec::new();
        }
        rows.extend(
            estimated_parameter_refusals(model, entry)
                .into_iter()
                .map(|diagnostic| (diagnostic, entry.retained.token_end)),
        );
    }
    rows
}

fn head_parse_refusals(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut rows = Vec::new();
    if model.varobs_statement_count >= 2
        && let Some(statement) = model
            .statements
            .iter()
            .filter(|row| row.name == "varobs")
            .nth(1)
    {
        rows.push((
            Diagnostic::new(
                model
                    .varobs_second_span
                    .unwrap_or(span_or_fallback(model.varobs_span)),
                Severity::Error,
                "E258",
                "varobs: you cannot have several 'varobs' statements in the same MOD file",
            )
            .with_related(RelatedDiagnostic::new(
                span_or_fallback(model.varobs_span),
                "Earlier varobs statement",
            )),
            statement.token_range.start + 1,
        ));
    }
    for family in ["observation_trends", "deterministic_trends"] {
        for statement in model.statements.iter().filter(|row| row.name == family) {
            if crate::engine::control::cancelled() {
                return Vec::new();
            }
            let mut seen = HashSet::new();
            let mut targets = Vec::new();
            if family == "observation_trends" {
                if let Some(receipts) = model.fact_receipts.get("observation_trends") {
                    for (row, receipt) in model.observation_trend_rows.iter().zip(receipts) {
                        if statement.token_range.contains(&receipt.parse_order) {
                            targets.push((
                                row.name,
                                row.name_span,
                                receipt
                                    .claims
                                    .iter()
                                    .map(|range| range.end)
                                    .max()
                                    .unwrap_or(receipt.parse_order),
                            ));
                        }
                    }
                }
            } else {
                for row in model
                    .deterministic_trends
                    .iter()
                    .flat_map(|block| &block.rows)
                {
                    if statement.token_range.start < row.callback_order
                        && row.callback_order <= statement.token_range.end
                    {
                        targets.push((row.name, row.name_span, row.callback_order));
                    }
                }
            }
            for (name, span, order) in targets {
                if !seen.insert(name) {
                    rows.push((
                        Diagnostic::new(
                            nonempty_or(span, model.observation_trends_span),
                            Severity::Error,
                            "E261",
                            format!("observation_trends: {} declared twice", model.name(name)),
                        ),
                        order,
                    ));
                }
            }
        }
    }
    rows
}

fn observed_variable_refusals(model: &Model, v: &ObservedVar) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let name = model.name(v.name);
    if model.symbol_kind_in_context(v.name, v.symbol_type_context) != Some("var") {
        let why = if matches!(
            model.symbol_kind_in_context(v.name, v.symbol_type_context),
            Some("varexo" | "varexo_det")
        ) {
            " (it is an exogenous variable)"
        } else if model.symbol_kind_in_context(v.name, v.symbol_type_context) == Some("parameters")
        {
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
    diagnostics
}

#[cfg(test)]
mod parse_receipt_tests {
    use super::*;
    use crate::engine::language::parser::parse;

    // Parse-time observed/estimated roles and trend-map insertions are not
    // interchangeable with Check-time observation membership and bounds.
    #[test]
    fn parse_receipts_separate_type_actions_from_estimation_checks() {
        let model = parse("parameters p;varobs p;database later;");
        let rows = parse_refusals(&model);
        let (diagnostic, order) = rows.iter().find(|(row, _)| row.code == "E090").unwrap();
        assert_eq!(
            diagnostic.message,
            check_w090(&model)
                .into_iter()
                .find(|row| row.code == "E090")
                .unwrap()
                .message
        );
        assert_eq!(model.expanded_tokens[order - 1].text(&model.source), "p");
        let model = parse("var y;estimated_params;y,.5;end;");
        assert!(
            parse_refusals(&model)
                .iter()
                .any(|(row, _)| row.code == "E093")
        );
        let model = parse("var y;parameters p;observation_trends;y(p);y(p);end;");
        assert!(
            parse_refusals(&model)
                .iter()
                .any(|(row, _)| row.code == "E261")
        );
        let model = parse("var y;parameters p;observation_trends;y(p);end;");
        assert!(check_w090(&model).iter().any(|row| row.code == "E095"));
        assert!(parse_refusals(&model).is_empty());
    }
}
