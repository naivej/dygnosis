//! W100–W103 optimal policy: Ramsey / discretionary / OSR.

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, Severity};
use crate::intern::Name;
use crate::model::{Decl, Model, PolicyCommand};
use crate::span::Span;

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
    let has_ramsey = model.policy_commands.iter().any(|c| {
        matches!(
            c,
            PolicyCommand::RamseyModel | PolicyCommand::RamseyPolicy
        )
    });
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

    if let Some(span) = model.ramsey_constraints_span {
        if !has_ramsey {
            diagnostics.push(Diagnostic::new(span, Severity::Error, "E203", E203_MSG));
        }
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
        let endo: HashSet<Name> = model.endogenous.iter().map(|d| d.name).collect();
        let params: HashSet<Name> = model.parameters.iter().map(|d| d.name).collect();
        let locals: HashSet<Name> = model
            .equations
            .iter()
            .filter(|eq| eq.is_local)
            .filter_map(|eq| {
                eq.lhs_expr.and_then(|e| match &model.exprs.get(e).kind {
                    crate::expr::ExprKind::Ident { name, .. } => Some(*name),
                    _ => None,
                })
            })
            .collect();
        let exo_in_planner = model.exprs.walk_idents(id).any(|r| {
            !endo.contains(&r.name) && !params.contains(&r.name) && !locals.contains(&r.name)
        });
        if exo_in_planner {
            diagnostics.push(Diagnostic::new(
                model.planner_objective_span.unwrap_or(FALLBACK),
                Severity::Error,
                "E251",
                "You cannot include exogenous variables (or variables of undeclared type) in the planner objective. Please define an auxiliary endogenous variable like eps_aux=epsilon and use it instead of the varexo.",
            ));
        }
    }

    if let Some((n, span)) = model.discretionary_order {
        if n > 1 {
            diagnostics.push(Diagnostic::new(
                span,
                Severity::Error,
                "E235",
                "discretionary_policy: order > 1 is not yet implemented",
            ));
        }
    }

    if model.policy_commands.is_empty() {
        return diagnostics;
    }

    let anchor = policy_anchor;

    let endogenous = names(&model.endogenous);
    for instrument in &model.instruments {
        if !endogenous.contains(instrument) {
            let name = model.name(*instrument);
            diagnostics.push(Diagnostic::new(
                anchor,
                Severity::Error,
                "E101",
                format!("Policy instrument '{name}' is not a declared endogenous variable."),
            ));
        }
    }

    if let Some(d) = model.planner_discount {
        if !(0.0 < d && d <= 1.0) {
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

fn names(decls: &[Decl]) -> HashSet<Name> {
    decls.iter().map(|d| d.name).collect()
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
