//! Equation surgery (0.5.3): the statement half of the catalog row.
//!
//! `Model::equation_surgery` records what each `model_remove` / `model_replace`
//! statement listed and removed. 7.1 refuses these while parsing — before
//! `checkPass` — so honesty for every code here runs at `json=check`. One code per
//! distinct official string; the option half (`exclude_eqs` / `include_eqs`) is Omit.

use crate::diagnostic::{Diagnostic, Severity};
use crate::model::{EquationSurgery, Model};
use crate::span::Span;

const STATEMENTS: &str = "model_remove/model_replace/exclude_eqs/include_eqs";

pub fn check_d_surgery(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for surgery in &model.equation_surgery {
        // 7.1 refuses a repeated key while parsing the tag list, so the statement never
        // gets as far as matching equations.
        if !surgery.tag_twice.is_empty() {
            check_tag_twice(surgery, &mut out);
            continue;
        }
        check_not_found(surgery, &mut out);
        check_excluded_twice(surgery, &mut out);
        if !surgery.replace {
            check_no_single_variable(surgery, &mut out);
        }
    }
    out
}

/// A key listed twice inside one bracketed set — **E256**'s official string.
fn check_tag_twice(surgery: &EquationSurgery, out: &mut Vec<Diagnostic>) {
    for (key, span) in &surgery.tag_twice {
        push(
            out,
            *span,
            "E256",
            format!("Tag '{key}' cannot be used twice for the same equation"),
        );
    }
}

/// A tag set that matched no equation. Their text repeats the unmatched list, pairs
/// sorted by key, a multi-pair set in brackets.
fn check_not_found(surgery: &EquationSurgery, out: &mut Vec<Diagnostic>) {
    if surgery.unmatched.is_empty() {
        return;
    }
    let list: Vec<String> = surgery
        .unmatched
        .iter()
        .map(|set| render_tag_set(set))
        .collect();
    push(
        out,
        surgery.span,
        "E335",
        format!(
            "{STATEMENTS}: The equations specified by {} were not found.",
            list.join(", ")
        ),
    );
}

/// One endogenous excluded twice by the same statement. 7.1 looks the printed name up
/// at the loop index, so it can name a symbol that was not excluded; the editor names
/// the variable that was excluded twice.
fn check_excluded_twice(surgery: &EquationSurgery, out: &mut Vec<Diagnostic>) {
    let mut seen: Vec<&str> = Vec::new();
    for row in &surgery.removed {
        let Some(name) = row.endogenous.as_deref() else {
            continue;
        };
        if seen.contains(&name) {
            push(
                out,
                surgery.span,
                "E337",
                format!(
                    "Variable {name} was excluded twice via a model_remove or model_replace \
                     statement, or via the include_eqs or exclude_eqs option"
                ),
            );
        }
        seen.push(name);
    }
}

/// An excluded equation with no `endogenous` tag and not one endogenous variable on
/// its left side. `model_replace` does not gate on this; `model_remove` does.
fn check_no_single_variable(surgery: &EquationSurgery, out: &mut Vec<Diagnostic>) {
    for row in &surgery.removed {
        if row.endogenous.is_none() {
            push(
                out,
                row.equation.span,
                "E336",
                format!(
                    "Equation {} has been excluded but it does not have a single variable on \
                     its left-hand side or an `endogenous` tag",
                    row.number
                ),
            );
        }
    }
}

fn render_tag_set(set: &[(String, String)]) -> String {
    let mut pairs: Vec<String> = set
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    pairs.sort();
    if pairs.len() > 1 {
        format!("[ {} ]", pairs.join(", "))
    } else {
        pairs.join(", ")
    }
}

fn push(out: &mut Vec<Diagnostic>, span: Span, code: &str, message: String) {
    out.push(Diagnostic::new(span, Severity::Error, code, message));
}
