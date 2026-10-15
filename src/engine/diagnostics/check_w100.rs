//! W100–W103 optimal policy: Ramsey / discretionary / OSR.

use std::collections::HashSet;

use crate::engine::diagnostics::{Diagnostic, Severity};
use crate::engine::language::intern::Name;
use crate::engine::language::model::{Model, PolicyCommand};
use crate::engine::language::span::Span;

const FALLBACK: Span = Span { start: 0, end: 1 };

const E100_MSG: &str = "A planner_objective statement must be used with a ramsey_model, a ramsey_policy, osr, or a discretionary_policy statement and vice versa.";
const E202_MSG: &str = "You cannot use the discretionary_policy command when you use either ramsey_model or ramsey_policy and vice versa";
const E203_MSG: &str =
    "A ramsey_constraints block requires the presence of a ramsey_model or ramsey_policy statement";
const E204_MSG: &str =
    "The osr statement cannot have both optim_weights and a planner_objective; they are mutually exclusive.";
const E215_MSG: &str = "discretionary_policy: the instruments option is required.";

pub fn check_w100(model: &Model) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let has_ramsey = model
        .policy_commands
        .iter()
        .any(|c| matches!(c, PolicyCommand::RamseyModel | PolicyCommand::RamseyPolicy));
    let has_disc = model
        .policy_commands
        .contains(&PolicyCommand::DiscretionaryPolicy);
    let has_osr = model.policy_commands.contains(&PolicyCommand::Osr);
    let has_planner = model.planner_objective_span.is_some();
    let policy_anchor = model.policy_command_span.unwrap_or(FALLBACK);

    if has_disc && has_ramsey {
        diagnostics.push(Diagnostic::new(
            model.discretionary_policy_span.unwrap_or(policy_anchor),
            Severity::Error,
            "E202",
            E202_MSG,
        ));
    }

    if (has_ramsey || has_disc) && !has_planner {
        diagnostics.push(Diagnostic::new(
            policy_anchor,
            Severity::Error,
            "E100",
            E100_MSG,
        ));
    } else if has_planner && !(has_ramsey || has_disc || has_osr) {
        diagnostics.push(Diagnostic::new(
            model.planner_objective_span.unwrap_or(FALLBACK),
            Severity::Error,
            "E100",
            E100_MSG,
        ));
    }

    if let Some(span) = model.ramsey_constraints_span
        && !has_ramsey
    {
        diagnostics.push(Diagnostic::new(span, Severity::Error, "E203", E203_MSG));
    }

    if has_osr && model.has_optim_weights && has_planner {
        diagnostics.push(Diagnostic::new(
            policy_anchor,
            Severity::Error,
            "E204",
            E204_MSG,
        ));
    }

    if has_disc && !model.discretionary_has_instruments_option {
        diagnostics.push(Diagnostic::new(
            model.discretionary_policy_span.unwrap_or(policy_anchor),
            Severity::Error,
            "E215",
            E215_MSG,
        ));
    }

    if model.osr_params_statement_count >= 2 {
        diagnostics.push(Diagnostic::new(
            model
                .osr_params_second_span
                .or(model.osr_params_span)
                .unwrap_or(FALLBACK),
            Severity::Warning,
            "W203",
            "You have more than one osr_params statement in the .mod file.",
        ));
    }

    if model.osr_params_bounds_span.is_some() {
        let bounds_start = model.osr_params_bounds_span.map(|s| s.start).unwrap_or(0);
        let params_after = match model.osr_params_span {
            None => true,
            Some(s) => s.start > bounds_start,
        };
        if params_after {
            diagnostics.push(Diagnostic::new(
                model.osr_params_bounds_span.unwrap_or(FALLBACK),
                Severity::Error,
                "E254",
                "you must have an osr_params statement before the osr_params_bounds block.",
            ));
        }
    }

    if let Some(id) = model.planner_objective_expr {
        let locals: HashSet<Name> = model
            .equations
            .iter()
            .filter(|eq| eq.is_local)
            .filter_map(|eq| {
                eq.lhs_expr.and_then(|e| match &model.exprs.get(e).kind {
                    crate::engine::language::expr::ExprKind::Ident { name, .. } => Some(*name),
                    _ => None,
                })
            })
            .collect();
        let planner_span = model.planner_objective_span.unwrap_or(FALLBACK);
        let exo_in_planner = model.exprs.walk_idents(id).any(|r| {
            !model.dropped_by_surgery_after(r.name, planner_span.start)
                && !locals.contains(&r.name)
                && !matches!(
                    model.final_kind_or_written_if_excluded(r.name),
                    Some("var" | "parameters")
                )
        });
        let excluded_in_planner = model.exprs.walk_idents(id).any(|r| {
            model
                .var_removed_model_uses
                .iter()
                .any(|(name, span, _)| *name == r.name && *span == r.span)
        });
        if exo_in_planner && !excluded_in_planner {
            diagnostics.push(Diagnostic::new(
                planner_span,
                Severity::Error,
                "E251",
                "You cannot include exogenous variables (or variables of undeclared type) in the planner objective. Please define an auxiliary endogenous variable like eps_aux=epsilon and use it instead of the varexo.",
            ));
        }
    }

    if let Some((n, span)) = model.discretionary_order
        && n > 1
    {
        diagnostics.push(Diagnostic::new(
            span,
            Severity::Error,
            "E235",
            "discretionary_policy: order > 1 is not yet implemented",
        ));
    }

    if model.policy_commands.is_empty() {
        return diagnostics;
    }

    let anchor = policy_anchor;

    diagnostics.extend(
        parse_refusals(model)
            .into_iter()
            .map(|(diagnostic, _)| diagnostic),
    );

    if let Some(d) = model.planner_discount
        && !(0.0 < d && d <= 1.0)
    {
        diagnostics.push(Diagnostic::new(
            anchor,
            Severity::Warning,
            "W102",
            format!(
                "planner_discount = {} should be a discount factor in the interval (0, 1].",
                python_g(d)
            ),
        ));
    }

    if model.policy_commands.contains(&PolicyCommand::Osr) {
        if model.osr_params.is_empty() {
            diagnostics.push(Diagnostic::new(
                anchor,
                Severity::Error,
                "E103",
                "The osr statement requires the osr_params statement",
            ));
        }
        if !model.has_optim_weights && model.planner_objective_span.is_none() {
            diagnostics.push(Diagnostic::new(
                anchor,
                Severity::Error,
                "E103",
                "The osr statement requires either an optim_weights block or a planner_objective",
            ));
        }
    }

    diagnostics
}

/// Driver actions after the complete policy command. Model/planner requirements
/// and discretionary order are later Check actions and do not stop Parse.
pub(crate) fn parse_refusals(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut diagnostics = Vec::new();
    'policy_checks: for (index, statement) in model.policy_command_statements.iter().enumerate() {
        if crate::engine::control::cancelled() {
            return Vec::new();
        }
        let order = super::parse_preflight::statement_callback_order(model, statement.parse_order);
        if !statement.discount_parameter_valid {
            diagnostics.push((
                Diagnostic::new(
                    statement.span,
                    Severity::Error,
                    "E378",
                    "optimal_policy_discount_factor is not a parameter",
                ),
                order,
            ));
            break;
        }
        for instrument in model
            .instrument_uses
            .iter()
            .filter(|use_| use_.command_index == index)
        {
            if instrument.kind == Some("var") {
                continue;
            }
            let name = model.name(instrument.name);
            let (span, code, message) = if instrument.kind.is_none() {
                (
                    instrument.command_span,
                    "E101",
                    format!("Policy instrument '{name}' is not a declared endogenous variable."),
                )
            } else {
                (
                    instrument.span,
                    "E317",
                    format!("{name} is not endogenous."),
                )
            };
            diagnostics.push((Diagnostic::new(span, Severity::Error, code, message), order));
            // The pinned parser stops at the first invalid instrument.
            break 'policy_checks;
        }
    }

    diagnostics
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

#[cfg(test)]
mod parse_receipt_tests {
    use super::*;

    #[test]
    fn policy_callbacks_keep_types_order_and_check_stage_separate() {
        // Guards a refused instrument or discount action from publishing the
        // next statement; planner requirements must remain later Check errors.
        for (source, code) in [
            (
                "ramsey_model(instruments=(missing));database later;",
                "E101",
            ),
            (
                "parameters p;ramsey_model(instruments=(p));database later;",
                "E317",
            ),
            (
                "var optimal_policy_discount_factor;discretionary_policy;database later;",
                "E378",
            ),
        ] {
            let model = crate::parse(source);
            let (diagnostic, order) = parse_refusals(&model).into_iter().next().unwrap();
            assert_eq!(diagnostic.code, code, "{source}");
            assert_eq!(model.expanded_tokens[order].text(&model.source), "database");
            assert!(diagnostic.span.start < model.expanded_tokens[order].span.start);
        }
        let model = crate::parse("parameters p;change_type(var) p;ramsey_model(instruments=(p));");
        assert!(parse_refusals(&model).is_empty());
        assert!(check_w100(&model)
            .iter()
            .any(|diagnostic| diagnostic.code == "E100"));
    }
}
