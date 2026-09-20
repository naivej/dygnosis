//! Estimation option clashes, DSGE-VAR check, data gate, prior/posterior `function`.

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, Severity};
use crate::model::{EstimatedParamKind, Model};
use crate::span::Span;

const FALLBACK: Span = Span { start: 0, end: 1 };

const E222_MSG: &str = "When estimating a DSGE-VAR model and estimating the weight of the prior, dsge_prior_weight must be referenced in the estimated_params block.";
const E223_MSG: &str = "If dsge_prior_weight is in the estimated_params block, the prior weight cannot be calibrated via the dsge_var option in the estimation statement.";
const E224_MSG: &str = "If dsge_prior_weight is in the estimated_params block, the dsge_var option must be passed to the estimation statement.";
const E225_MSG: &str = "The estimation statement requires a dsge_var option to be passed if the dsge_varlag option is passed.";
const E226_MSG: &str = "An estimation statement cannot take more than one dsge_var option.";
const E227_MSG: &str = "The estimation statement requires a data file to be supplied via the datafile option.";
const E228_MSG: &str = "The mode_file option of the estimation statement is incompatible with the use_calibration option of the estimated_params_init block.";
const E229_MSG: &str = "The mh_tune_jscale and mh_jscale options of the estimation statement are incompatible.";
const E230_MSG: &str = "The option mh_tune_guess in estimation statement cannot be used without option mh_tune_jscale.";
const E231_MSG: &str = "The filter_algorithm=gmf option is incompatible with proposal_approximation=montecarlo in the estimation statement.";
const E232_MSG: &str = "The filter_algorithm=gmf option is incompatible with distribution_approximation=montecarlo in the estimation statement.";
const E234_MSG: &str = "both the 'prior_function' and 'posterior_function' commands require the 'function' option";

/// True when a `data` statement carrying `file` or `series` is written **before**
/// `at`. 7.1 sets that flag when the statement's own check pass runs, and the
/// `estimation` pass reads it in file order.
fn data_statement_before(model: &Model, at: u32) -> bool {
    model
        .data_statements
        .iter()
        .any(|stmt| stmt.span.start < at && stmt.has_file_or_series())
}

pub fn check_estimation(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let weight_in_est = has_dsge_prior_weight(model);
    let has_dsge_var = model.dsge_var_estimated.is_some() || model.dsge_var_calibrated.is_some();

    if model.dsge_var_estimated.is_some() && !weight_in_est {
        let span = model.dsge_var_estimated.or(model.estimation_span).unwrap_or(FALLBACK);
        push(&mut out, span, "E222", E222_MSG);
    }
    if weight_in_est && model.dsge_var_calibrated.is_some() {
        let span = model.dsge_var_calibrated.unwrap_or(FALLBACK);
        push(&mut out, span, "E223", E223_MSG);
    }
    if weight_in_est && !has_dsge_var {
        let span = dsge_prior_weight_span(model).unwrap_or(FALLBACK);
        push(&mut out, span, "E224", E224_MSG);
    }
    if model.dsge_varlag_span.is_some() && !has_dsge_var {
        let span = model.dsge_varlag_span.unwrap_or(FALLBACK);
        push(&mut out, span, "E225", E225_MSG);
    }
    if let Some(span) = e226_span(model) {
        push(&mut out, span, "E226", E226_MSG);
    }
    for stmt in &model.estimation_statements {
        if stmt.has_datafile {
            continue;
        }
        if data_statement_before(model, stmt.span.start) {
            continue;
        }
        push(&mut out, stmt.span, "E227", E227_MSG);
    }
    if model.estimation_mode_file_span.is_some()
        && model.estimated_params_init_use_calibration.is_some()
    {
        let span = model
            .estimation_mode_file_span
            .or(model.estimated_params_init_use_calibration)
            .unwrap_or(FALLBACK);
        push(&mut out, span, "E228", E228_MSG);
    }
    if model.mh_tune_jscale_span.is_some() && model.mh_jscale_span.is_some() {
        let span = model.mh_jscale_span.or(model.mh_tune_jscale_span).unwrap_or(FALLBACK);
        push(&mut out, span, "E229", E229_MSG);
    }
    if model.mh_tune_guess_span.is_some() && model.mh_tune_jscale_span.is_none() {
        let span = model.mh_tune_guess_span.unwrap_or(FALLBACK);
        push(&mut out, span, "E230", E230_MSG);
    }
    if model.filter_algorithm_gmf_span.is_some()
        && model.proposal_approximation_montecarlo_span.is_some()
    {
        let span = model
            .proposal_approximation_montecarlo_span
            .or(model.filter_algorithm_gmf_span)
            .unwrap_or(FALLBACK);
        push(&mut out, span, "E231", E231_MSG);
    }
    if model.filter_algorithm_gmf_span.is_some()
        && model.distribution_approximation_montecarlo_span.is_some()
    {
        let span = model
            .distribution_approximation_montecarlo_span
            .or(model.filter_algorithm_gmf_span)
            .unwrap_or(FALLBACK);
        push(&mut out, span, "E232", E232_MSG);
    }
    if let Some(expr) = model.planner_discount_expr {
        let estimated: HashSet<_> = model
            .estimated_params
            .iter()
            .filter(|p| p.kind == EstimatedParamKind::Param)
            .map(|p| p.name)
            .collect();
        let mut seen = HashSet::new();
        for r in model.exprs.walk_idents(expr) {
            if estimated.contains(&r.name) && seen.insert(r.name) {
                let name = model.name(r.name);
                out.push(Diagnostic::new(
                    r.span,
                    Severity::Error,
                    "E233",
                    format!(
                        "It is not possible to estimate a parameter ({name}) that appears in the discount factor of the planner (i.e. in the 'planner_discount' option)."
                    ),
                ));
            }
        }
    }
    if model.prior_function_has_parens && !model.prior_function_has_function {
        if let Some(span) = model
            .prior_function_span
            .or(model.posterior_function_span)
        {
            push(&mut out, span, "E234", E234_MSG);
        }
    }
    out
}

fn has_dsge_prior_weight(model: &Model) -> bool {
    model
        .estimated_params
        .iter()
        .any(|p| model.name(p.name) == "dsge_prior_weight")
}

fn dsge_prior_weight_span(model: &Model) -> Option<Span> {
    model
        .estimated_params
        .iter()
        .find(|p| model.name(p.name) == "dsge_prior_weight")
        .map(|p| p.span)
}

fn e226_span(model: &Model) -> Option<Span> {
    let stmts = &model.estimation_dsge_var_stmts;
    for (i, a) in stmts.iter().enumerate() {
        if a.estimated.is_none() {
            continue;
        }
        for (j, b) in stmts.iter().enumerate() {
            if i != j && b.calibrated.is_some() {
                return b.calibrated.or(a.estimated);
            }
        }
    }
    None
}

fn push(out: &mut Vec<Diagnostic>, span: Span, code: &str, message: &str) {
    out.push(Diagnostic::new(span, Severity::Error, code, message));
}
