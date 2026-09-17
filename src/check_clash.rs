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
        if let Some(id_span) = model.identification_span {
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

    out
}

fn has_ramsey(model: &Model) -> bool {
    model
        .policy_commands
        .iter()
        .any(|c| matches!(c, PolicyCommand::RamseyModel | PolicyCommand::RamseyPolicy))
}

fn e179_span(model: &Model) -> Option<Span> {
    if model.policy_commands.contains(&PolicyCommand::Osr) {
        return model
            .policy_command_span
            .or(model.occbin_constraints_blocks.first().copied());
    }
    if has_ramsey(model) {
        return model
            .policy_command_span
            .or(model.occbin_constraints_blocks.first().copied());
    }
    if model
        .policy_commands
        .contains(&PolicyCommand::DiscretionaryPolicy)
    {
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
