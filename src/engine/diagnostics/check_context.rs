//! Check-class Errors for model options, 0-equation files, PF/stochastic mix, and command order.

use crate::engine::diagnostics::{Diagnostic, Severity};
use crate::engine::language::model::{DeprecatedOption, Model};
use crate::engine::language::span::Span;

const FALLBACK: Span = Span { start: 0, end: 1 };

const E200_MSG: &str = "You cannot have a write_latex_steady_state_model statement without a steady_state_model block.";
const E201_MSG: &str = "At least one model equation must be declared!";
const E205_MSG: &str = "A .mod file cannot contain both one of {perfect_foresight_solver, simul, perfect_foresight_with_expectation_errors_solver} and one of {stoch_simul, estimation, osr, ramsey_policy, discretionary_policy}. This is not possible: one cannot mix perfect foresight context with stochastic context in the same file.";
const E206_MSG: &str = "In 'model' block, 'use_dll' option is not compatible with 'bytecode'";
const E207_MSG: &str = "no_static option is incompatible with stoch_simul, estimation, osr, ramsey_policy, discretionary_policy, steady and check commands";
const E213_MSG: &str =
    "A 'perfect_foresight_setup' command must come before 'perfect_foresight_solver'";
const E214_MSG: &str = "A 'perfect_foresight_with_expectation_errors_setup' command must come before 'perfect_foresight_with_expectation_errors_solver'";
const E216_MSG: &str = "the 'periods' option of 'extended_path' is mandatory";
const E237_MSG: &str = "The max_dim_cova_group option to identification only accepts integers > 0.";

/// IdentificationStatement checks this option in its constructor. Its `order`
/// option and all other checks in this module belong to the later Check pass.
pub(crate) fn parse_refusals(model: &Model) -> Vec<(Diagnostic, usize)> {
    model
        .identification_max_dimensions
        .iter()
        .filter(|&&(value, _, _)| value == 0)
        .map(|&(_, span, at)| {
            (
                Diagnostic::new(span, Severity::Error, "E237", E237_MSG),
                super::parse_preflight::statement_callback_order(model, at),
            )
        })
        .collect()
}

pub fn check_context(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();

    if let Some(span) = model.write_latex_steady_state_model_span
        && model.ss_block.is_none()
    {
        push(&mut out, span, "E200", E200_MSG);
    }

    if model.non_local_equation_count() == 0 && zero_eq_trigger(model) {
        let span = zero_eq_span(model);
        push(&mut out, span, "E201", E201_MSG);
    }

    if model.is_pf_solver_context()
        && model.is_stochastic_context()
        && let Some(span) = mix_span(model)
    {
        push(&mut out, span, "E205", E205_MSG);
    }

    if model.use_dll_span.is_some()
        && model
            .deprecated_option_spans
            .iter()
            .any(|(opt, _)| *opt == DeprecatedOption::Bytecode)
    {
        let span = model.use_dll_span.unwrap_or(FALLBACK);
        push(&mut out, span, "E206", E206_MSG);
    }

    if model.no_static_span.is_some()
        && (model.is_stochastic_context()
            || model.check_span.is_some()
            || model.steady_span.is_some())
    {
        let span = model.no_static_span.unwrap_or(FALLBACK);
        push(&mut out, span, "E207", E207_MSG);
    }

    if let Some(solver) = model.perfect_foresight_solver_span
        && solver_before_setup(model.perfect_foresight_setup_span, solver)
    {
        push(&mut out, solver, "E213", E213_MSG);
    }

    if let Some(solver) = model.pfee_solver_span
        && solver_before_setup(model.pfee_setup_span, solver)
    {
        push(&mut out, solver, "E214", E214_MSG);
    }

    if model.extended_path_span.is_some() && !model.extended_path_has_periods {
        let span = model.extended_path_span.unwrap_or(FALLBACK);
        push(&mut out, span, "E216", E216_MSG);
    }

    if let Some((n, span)) = model.identification_order
        && !(1..=3).contains(&n)
    {
        push(
            &mut out,
            span,
            "E236",
            "the order option of identification command must be between 1 and 3",
        );
    }

    out.extend(
        parse_refusals(model)
            .into_iter()
            .map(|(diagnostic, _)| diagnostic),
    );

    out
}

fn zero_eq_trigger(model: &Model) -> bool {
    model.check_span.is_some()
        || model.perfect_foresight_solver_span.is_some()
        || model.pfee_solver_span.is_some()
        || model.is_stochastic_context()
}

fn zero_eq_span(model: &Model) -> Span {
    model
        .check_span
        .or(model.perfect_foresight_solver_span)
        .or(model.pfee_solver_span)
        .or(model.stoch_simul_span)
        .or(model.estimation_span)
        .or(model.model_block)
        .unwrap_or(FALLBACK)
}

fn mix_span(model: &Model) -> Option<Span> {
    let mut pf = Vec::new();
    pf.extend(model.simul_spans.iter().copied());
    if let Some(s) = model.perfect_foresight_solver_span {
        pf.push(s);
    }
    if let Some(s) = model.pfee_solver_span {
        pf.push(s);
    }
    let mut stoch = Vec::new();
    for span in [
        model.stoch_simul_span,
        model.estimation_span,
        model.calib_smoother_span,
        model.identification_span,
        model.method_of_moments_span,
        model.sensitivity_span,
        model.extended_path_span,
        model.ramsey_policy_span,
        model.discretionary_policy_span,
    ]
    .into_iter()
    .flatten()
    {
        stoch.push(span);
    }
    if model
        .policy_commands
        .contains(&crate::engine::language::model::PolicyCommand::Osr)
        && let Some(s) = model.policy_command_span
    {
        stoch.push(s);
    }
    let last_pf = pf.iter().max_by_key(|s| s.start)?;
    let last_stoch = stoch.iter().max_by_key(|s| s.start)?;
    if last_pf.start > last_stoch.start {
        Some(*last_pf)
    } else {
        Some(*last_stoch)
    }
}

fn solver_before_setup(setup: Option<Span>, solver: Span) -> bool {
    match setup {
        None => true,
        Some(setup) => setup.start > solver.start,
    }
}

fn push(out: &mut Vec<Diagnostic>, span: Span, code: &str, message: &str) {
    out.push(Diagnostic::new(span, Severity::Error, code, message));
}

#[cfg(test)]
mod parse_receipt_tests {
    use super::*;

    #[test]
    fn identification_constructor_keeps_each_execution_and_check_order_separate() {
        // Guards the first-option summary from hiding a later constructor
        // refusal, and an order Check error from stopping the Parse prefix.
        let model = crate::parse("identification(max_dim_cova_group=1);identification(max_dim_cova_group=0);database later;");
        let (diagnostic, order) = parse_refusals(&model).into_iter().next().unwrap();
        assert_eq!(diagnostic.code, "E237");
        assert_eq!(model.expanded_tokens[order].text(&model.source), "database");
        assert_eq!(
            &model.source[diagnostic.span.start as usize..diagnostic.span.end as usize],
            "0"
        );
        let model = crate::parse(
            "identification(max_dim_cova_group=0);identification(max_dim_cova_group=1);",
        );
        assert!(check_context(&model)
            .iter()
            .any(|diagnostic| diagnostic.code == "E237"));
        let model = crate::parse("identification(order=0);database kept;");
        assert!(parse_refusals(&model).is_empty());
        assert!(check_context(&model)
            .iter()
            .any(|diagnostic| diagnostic.code == "E236"));
    }
}
