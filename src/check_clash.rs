//! Written-clash Errors they refuse at transform (`json=transform`).

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, Severity};
use crate::expr::ExprKind;
use crate::intern::Name;
use crate::model::{Equation, Model, PeriodPoint, PolicyCommand, ShockBlockKind};
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
const LEARNT_GATE_SUFFIX: &str = "block can only be used in conjunction with the 'perfect_foresight_with_expectation_errors_setup' and 'perfect_foresight_with_expectation_errors_solver' commands.";

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

    let learnt_gate = model.perfect_foresight_setup_span.is_some()
        || model.perfect_foresight_solver_span.is_some()
        || model.pfee_setup_span.is_none()
        || model.pfee_solver_span.is_none();
    if learnt_gate {
        for block in &model.shock_blocks {
            if !matches!(
                block.kind,
                ShockBlockKind::LearntIn | ShockBlockKind::Multiplicative
            ) || !nondefault_learnt_in(block.options.learnt_in.as_ref())
            {
                continue;
            }
            push(
                &mut out,
                block.options.learnt_in_span.unwrap_or(block.span),
                "E422",
                &format!("the 'shocks(learnt_in=…)' {LEARNT_GATE_SUFFIX}"),
            );
        }
        for block in &model.endval_instructions {
            if nondefault_learnt_in(block.learnt_in.as_ref()) {
                push(
                    &mut out,
                    block.learnt_in_span.unwrap_or(block.span),
                    "E423",
                    &format!("the 'endval(learnt_in=…)' {LEARNT_GATE_SUFFIX}"),
                );
            }
        }
        for block in &model.controlled_paths {
            if nondefault_learnt_in(block.options.learnt_in.as_ref()) {
                push(
                    &mut out,
                    block.options.learnt_in_span.unwrap_or(block.span),
                    "E424",
                    &format!(
                        "the 'perfect_foresight_controlled_paths(learnt_in=…)' {LEARNT_GATE_SUFFIX}"
                    ),
                );
            }
        }
        for block in &model.shock_paths {
            if nondefault_learnt_in(block.options.learnt_in.as_ref()) {
                push(
                    &mut out,
                    block.options.learnt_in_span.unwrap_or(block.span),
                    "E425",
                    &format!("the 'shock_paths(learnt_in=…)' {LEARNT_GATE_SUFFIX}"),
                );
            }
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

    out.extend(check_default_eq_tag(model));

    out
}

fn nondefault_learnt_in(point: Option<&PeriodPoint>) -> bool {
    matches!(point, Some(PeriodPoint::Date(_)))
        || matches!(point, Some(PeriodPoint::Integer(n)) if *n > 1)
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

fn check_default_eq_tag(model: &Model) -> Vec<Diagnostic> {
    let endo: HashSet<Name> = model.endogenous.iter().map(|d| d.name).collect();
    let mut used: HashSet<String> = HashSet::new();
    for eq in &model.equations {
        if eq.is_local {
            continue;
        }
        if let Some(n) = eq.tag_map.get("name") {
            if !n.is_empty() {
                used.insert(n.clone());
            }
        }
    }
    let mut out = Vec::new();
    let mut index = 0usize;
    for eq in &model.equations {
        if eq.is_local {
            continue;
        }
        index += 1;
        if eq.tag_map.get("name").is_some_and(|n| !n.is_empty()) {
            continue;
        }
        let lhs = lhs_ident(model, eq)
            .filter(|n| endo.contains(n))
            .map(|n| model.name(n).to_string());
        let lhs_ok = lhs.as_ref().is_some_and(|s| !used.contains(s));
        if lhs_ok {
            if let Some(s) = lhs {
                used.insert(s);
            }
            continue;
        }
        let idx = index.to_string();
        if !used.contains(&idx) {
            used.insert(idx);
            continue;
        }
        out.push(Diagnostic::new(
            eq.span,
            Severity::Error,
            "E257",
            format!(
                "Error creating default equation tag: cannot assign default tag to equation number {index} because it is already in use"
            ),
        ));
    }
    out
}

fn lhs_ident(model: &Model, eq: &Equation) -> Option<Name> {
    let id = eq.lhs_expr?;
    match &model.exprs.get(id).kind {
        ExprKind::Ident { name, .. } => Some(*name),
        _ => None,
    }
}
