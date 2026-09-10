//! W100–W103 optimal policy: Ramsey / discretionary / OSR.

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, Severity};
use crate::intern::Name;
use crate::model::{Decl, Model, PolicyCommand};
use crate::span::Span;

const FALLBACK: Span = Span { start: 0, end: 1 };

pub fn check_w100(model: &Model) -> Vec<Diagnostic> {
    if model.policy_commands.is_empty() {
        return Vec::new();
    }

    let anchor = model.policy_command_span.unwrap_or(FALLBACK);
    let mut diagnostics = Vec::new();

    let planner_command = model
        .policy_commands
        .iter()
        .copied()
        .find(|c| c.is_planner());
    if let Some(planner_command) = planner_command {
        if model.planner_objective_span.is_none() {
            diagnostics.push(Diagnostic::new(
                anchor,
                Severity::Warning,
                "W100",
                format!(
                    "{} requires a planner_objective statement, which is missing.",
                    planner_command.as_str()
                ),
            ));
        }
    }

    let endogenous = names(&model.endogenous);
    for instrument in &model.instruments {
        if !endogenous.contains(instrument) {
            let name = model.name(*instrument);
            diagnostics.push(Diagnostic::new(
                anchor,
                Severity::Warning,
                "W101",
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
                Severity::Warning,
                "W103",
                "osr requires an osr_params statement listing the parameters to optimize.",
            ));
        }
        if !model.has_optim_weights {
            diagnostics.push(Diagnostic::new(
                anchor,
                Severity::Warning,
                "W103",
                "osr requires an optim_weights block defining the objective (the weights on the target variables).",
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
