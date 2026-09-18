//! Written-clash Errors they refuse at transform (`json=transform`).

use crate::diagnostic::{Diagnostic, Severity};
use crate::model::{Model, PolicyCommand};
use crate::span::Span;

const FALLBACK: Span = Span { start: 0, end: 1 };

const E104_MSG: &str = "there can only be one planner_objective statement";
const E026_MSG: &str = "A .mod file cannot contain both one of {perfect_foresight_solver, simul, perfect_foresight_with_expectation_errors_solver} and varexo_det declaration (all exogenous variables are deterministic in this case)";
const E027_MSG: &str =
    "ramsey_model and ramsey_policy are incompatible with deterministic exogenous variables";
const E028_MSG: &str = "identification is incompatible with deterministic exogenous variables";
const E219_MSG: &str = "dsge_prior_weight should not be declared as a model variable / parameter when the dsge_var option is passed to the estimation statement.";
const E220_MSG: &str = "When estimating a DSGE-Var and the bayesian_irf option is passed to the estimation statement, the number of shocks must equal the number of observed variables.";
const E221_MSG: &str = "When estimating a DSGE-Var, the number of shocks must be greater than or equal to the number of observed variables.";
const E179_MSG: &str = "the 'occbin_constraints' block is not compatible with commands other than 'estimation', 'stoch_simul', and 'calib_smoother'.";
const E178_MSG: &str = "the 'shocks(surprise)' block can only be used in conjunction with the 'occbin_constraints' block.";
const E113_MSG: &str = "the 'shock_paths' block cannot be used in conjunction with either 'shocks', 'mshocks', 'endval' or 'perfect_foresight_controlled_paths' blocks.";

pub fn check_clash(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let ramsey = has_ramsey(model);
    let varexo_det = model.deterministic_exogenous.first().map(|d| d.span);

    if ramsey && model.planner_objective_spans.len() > 1 {
        push(&mut out, model.planner_objective_spans[1], "E104", E104_MSG);
    }

    let pf_solver = !model.simul_spans.is_empty()
        || model.perfect_foresight_solver_span.is_some()
        || model.pfee_solver_span.is_some();
    if let Some(span) = varexo_det {
        if pf_solver {
            push(&mut out, span, "E026", E026_MSG);
        }
        if ramsey {
            push(&mut out, span, "E027", E027_MSG);
        }
        if let Some(id_span) = model
            .identification_span
            .or(model.sensitivity_identification_eq_1)
        {
            push(&mut out, id_span, "E028", E028_MSG);
        }
    }

    if !model.occbin_constraints_blocks.is_empty() {
        if let Some(span) = e179_span(model) {
            push(&mut out, span, "E179", E179_MSG);
        }
    }

    if model.shocks_surprise && model.occbin_constraints_blocks.is_empty() {
        let span = model
            .shocks_surprise_span
            .or(model.shocks_block)
            .unwrap_or(FALLBACK);
        push(&mut out, span, "E178", E178_MSG);
    }

    if let Some(span) = model.shock_paths_span {
        if model.shocks_block.is_some()
            || model.endval_block.is_some()
            || model.perfect_foresight_controlled_paths_span.is_some()
        {
            push(&mut out, span, "E113", E113_MSG);
        }
    }

    if model.dsge_var_estimated.is_some() || model.dsge_var_calibrated.is_some() {
        if let Some(span) = dsge_prior_weight_decl(model) {
            push(&mut out, span, "E219", E219_MSG);
        }
    }

    if is_dsge_var(model) {
        let shocks = shock_count(model);
        let observed = model.varobs.len();
        if model.bayesian_irf_span.is_some() && shocks != observed {
            let span = model
                .bayesian_irf_span
                .or(model.estimation_span)
                .unwrap_or(FALLBACK);
            push(&mut out, span, "E220", E220_MSG);
        } else if model.bayesian_irf_span.is_none() && shocks < observed {
            let span = model
                .dsge_var_estimated
                .or(model.dsge_var_calibrated)
                .or(model.estimation_span)
                .unwrap_or(FALLBACK);
            push(&mut out, span, "E221", E221_MSG);
        }
    }

    out
}

fn dsge_prior_weight_decl(model: &Model) -> Option<Span> {
    model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.parameters)
        .find(|d| model.name(d.name) == "dsge_prior_weight")
        .map(|d| d.span)
}

fn is_dsge_var(model: &Model) -> bool {
    model.dsge_var_estimated.is_some()
        || model.dsge_var_calibrated.is_some()
        || model
            .estimated_params
            .iter()
            .any(|p| model.name(p.name) == "dsge_prior_weight")
}

fn shock_count(model: &Model) -> usize {
    model
        .exogenous
        .iter()
        .filter(|d| {
            !model
                .deterministic_exogenous
                .iter()
                .any(|det| det.name == d.name)
        })
        .count()
}

fn has_ramsey(model: &Model) -> bool {
    model
        .policy_commands
        .iter()
        .any(|c| matches!(c, PolicyCommand::RamseyModel | PolicyCommand::RamseyPolicy))
}

fn e179_span(model: &Model) -> Option<Span> {
    let policy_clash = model.policy_commands.iter().any(|c| {
        matches!(
            c,
            PolicyCommand::Osr
                | PolicyCommand::RamseyModel
                | PolicyCommand::RamseyPolicy
                | PolicyCommand::DiscretionaryPolicy
        )
    });
    if policy_clash {
        return model
            .policy_command_span
            .or(model.occbin_constraints_blocks.first().copied());
    }
    model
        .method_of_moments_span
        .or(model.extended_path_span)
        .or(model.identification_span)
        .or(model.sensitivity_span)
}

fn push(out: &mut Vec<Diagnostic>, span: Span, code: &str, message: &str) {
    out.push(Diagnostic::new(span, Severity::Error, code, message));
}
