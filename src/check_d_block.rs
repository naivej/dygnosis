//! D-block parse-time Errors on trees we already have.

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, Severity};
use crate::expr::{ExprId, ExprKind};
use crate::intern::Name;
use crate::model::{Equation, Model};
use crate::span::Span;

pub fn check_d_block(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    out.extend(check_histval_lag_dup(model));
    out.extend(check_planner_lead_local(model));
    out.extend(check_osr_bounds_type(model));
    out.extend(check_varexobs(model));
    out.extend(check_option_twice(model));
    out.extend(check_static_tag(model));
    out.extend(check_generate_irfs(model));
    out.extend(check_namespace(model));
    out.extend(check_const_fold(model));
    out.extend(check_matlab_locals(model));
    out
}

fn check_histval_lag_dup(model: &Model) -> Vec<Diagnostic> {
    let mut seen: HashSet<(Name, i32)> = HashSet::new();
    let mut out = Vec::new();
    for entry in &model.histval {
        let name = model.name(entry.name);
        if entry.lag > 0 {
            out.push(err(
                entry.span,
                "E242",
                format!("histval: the lag on {name} should be less than or equal to 0"),
            ));
        }
        if !seen.insert((entry.name, entry.lag)) {
            out.push(err(
                entry.span,
                "E243",
                format!("hist_val: ({name}, {}) declared twice", entry.lag),
            ));
        }
    }
    out
}

fn check_planner_lead_local(model: &Model) -> Vec<Diagnostic> {
    let Some(id) = model.planner_objective_expr else {
        return Vec::new();
    };
    let span = model
        .planner_objective_span
        .unwrap_or(Span { start: 0, end: 1 });
    let locals: HashSet<Name> = model
        .equations
        .iter()
        .filter(|eq| eq.is_local)
        .filter_map(|eq| lhs_ident(model, eq))
        .collect();
    let mut out = Vec::new();
    let mut saw_lead = false;
    for r in model.exprs.walk_idents(id) {
        if !saw_lead && r.timing != 0 {
            out.push(err(
                span,
                "E252",
                "Leads and lags on variables are forbidden in 'planner_objective'.",
            ));
            saw_lead = true;
        }
        if locals.contains(&r.name) {
            let name = model.name(r.name);
            out.push(err(
                span,
                "E253",
                format!("Model local variable {name} cannot be used in 'planner_objective'."),
            ));
        }
    }
    out
}

fn check_osr_bounds_type(model: &Model) -> Vec<Diagnostic> {
    let params: HashSet<Name> = model.parameters.iter().map(|d| d.name).collect();
    let mut out = Vec::new();
    for bound in &model.osr_params_bounds {
        if params.contains(&bound.name) {
            continue;
        }
        let name = model.name(bound.name);
        out.push(err(
            bound.span,
            "E255",
            format!("{name} must be a parameter to be used in the osr_bounds block"),
        ));
    }
    out
}

fn check_varexobs(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if model.varexobs_statement_count >= 2 {
        let span = model
            .varexobs_second_span
            .or(model.varexobs_span)
            .unwrap_or(Span { start: 0, end: 1 });
        out.push(err(
            span,
            "E259",
            "varexobs: you cannot have several 'varexobs' statements in the same MOD file",
        ));
    }
    let exo: HashSet<Name> = model.exogenous.iter().map(|d| d.name).collect();
    for v in &model.varexobs {
        if exo.contains(&v.name) {
            continue;
        }
        let name = model.name(v.name);
        out.push(err(
            v.span,
            "E260",
            format!("varexobs: {name} is not an exogenous variable"),
        ));
    }
    out
}

fn check_option_twice(model: &Model) -> Vec<Diagnostic> {
    model
        .option_twice
        .iter()
        .map(|(name, span)| err(*span, "E271", format!("option {name} declared twice")))
        .collect()
}

fn check_static_tag(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for eq in &model.equations {
        if !eq.static_tag {
            continue;
        }
        if eq.lhs_expr.is_some_and(|id| expr_has_dynamics(model, id))
            || eq.rhs_expr.is_some_and(|id| expr_has_dynamics(model, id))
        {
            out.push(err(
                eq.span,
                "E272",
                "An equation tagged [static] cannot contain leads, lags, expectations, diff or STEADY_STATE operators",
            ));
        }
    }
    out
}

fn check_generate_irfs(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut seen: HashSet<Name> = HashSet::new();
    for el in &model.generate_irfs {
        let name = model.name(el.name);
        if !seen.insert(el.name) {
            out.push(err(
                el.span,
                "E273",
                format!(
                    "Names in the generate_irfs block must be unique but you entered '{name}' more than once."
                ),
            ));
        }
        let mut exo_seen: HashSet<Name> = HashSet::new();
        for (exo, span) in &el.exos {
            if !exo_seen.insert(*exo) {
                out.push(err(
                    *span,
                    "E274",
                    format!(
                        "You have set the exogenous variable {} twice.",
                        model.name(*exo)
                    ),
                ));
            }
        }
    }
    out
}

fn check_namespace(model: &Model) -> Vec<Diagnostic> {
    model
        .namespace_qualified
        .iter()
        .map(|(pair, span)| {
            err(
                *span,
                "E275",
                format!("Namespace-qualified symbol {pair} not allowed in this context"),
            )
        })
        .collect()
}

fn check_const_fold(model: &Model) -> Vec<Diagnostic> {
    model
        .const_fold_errors
        .iter()
        .map(|(span, code, message)| err(*span, code, message.clone()))
        .collect()
}

fn check_matlab_locals(model: &Model) -> Vec<Diagnostic> {
    let externals: HashSet<Name> = model.external_function_names.iter().copied().collect();
    let mod_locals: HashSet<Name> = model.mod_file_locals.iter().copied().collect();
    let pound: HashSet<Name> = model
        .equations
        .iter()
        .filter(|eq| eq.is_local)
        .filter_map(|eq| lhs_ident(model, eq))
        .collect();
    let mut out = Vec::new();
    let mut seen_in: HashSet<(Name, &'static str)> = HashSet::new();
    let mut seen_out: HashSet<(Name, &'static str)> = HashSet::new();

    for eq in &model.equations {
        for r in model.ident_refs(eq) {
            if externals.contains(&r.name) && seen_in.insert((r.name, "E280")) {
                let name = model.name(r.name);
                out.push(err(
                    r.span,
                    "E280",
                    crate::model::external_function_in_model_message(name),
                ));
            }
            if mod_locals.contains(&r.name) && seen_in.insert((r.name, "E281")) {
                let name = model.name(r.name);
                out.push(err(
                    r.span,
                    "E281",
                    crate::model::mod_file_local_in_model_message(name),
                ));
            }
        }
    }

    for (id, span) in outside_ident_uses(model) {
        if externals.contains(&id) && seen_out.insert((id, "E279")) {
            let name = model.name(id);
            out.push(err(
                span,
                "E279",
                format!(
                    "Symbol '{name}' is the name of a MATLAB/Octave function, and cannot be used as a variable."
                ),
            ));
        }
        if pound.contains(&id) && seen_out.insert((id, "E282")) {
            let name = model.name(id);
            out.push(err(
                span,
                "E282",
                format!(
                    "Variable {name} not allowed outside model declaration. Its scope is only inside model."
                ),
            ));
        }
    }
    out
}

fn outside_ident_uses(model: &Model) -> Vec<(Name, Span)> {
    let mut out = Vec::new();
    let push_expr = |out: &mut Vec<(Name, Span)>, id: ExprId| {
        for r in model.exprs.walk_idents(id) {
            out.push((r.name, r.span));
        }
    };
    for a in model
        .param_assignments
        .iter()
        .chain(&model.helper_assignments)
    {
        if let Some(id) = a.expr {
            push_expr(&mut out, id);
        }
    }
    if let Some(id) = model.planner_objective_expr {
        push_expr(&mut out, id);
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

fn expr_has_dynamics(model: &Model, id: ExprId) -> bool {
    fn walk(model: &Model, id: ExprId) -> bool {
        match &model.exprs.get(id).kind {
            ExprKind::Ident { timing, .. } => *timing != 0,
            ExprKind::Expectation { .. } | ExprKind::SteadyState { .. } => true,
            ExprKind::Call { callee, args } => {
                model.name(*callee).eq_ignore_ascii_case("diff")
                    || args.iter().any(|a| walk(model, *a))
            }
            ExprKind::Unary { arg, .. } => walk(model, *arg),
            ExprKind::Binary { lhs, rhs, .. } => walk(model, *lhs) || walk(model, *rhs),
            ExprKind::Number | ExprKind::String | ExprKind::Error => false,
        }
    }
    walk(model, id)
}

fn err(span: Span, code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(span, Severity::Error, code, message)
}
