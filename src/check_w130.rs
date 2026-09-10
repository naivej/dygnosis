//! W130–W131 steady-state order, W140 linear-model operators, W150 deprecations.

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, Severity};
use crate::expr::{BinOp, ExprId, ExprKind};
use crate::intern::Name;
use crate::model::{DeprecatedOption, Equation, Model};
use crate::span::{LineIndex, Span};

const SPECIAL_CALLS: &[&str] = &["abs", "max", "min", "sign"];

const MATH_BUILTINS: &[&str] = &[
    "exp", "log", "ln", "log2", "log10", "sqrt", "cbrt", "abs", "sign", "sin", "cos", "tan",
    "asin", "acos", "atan", "sinh", "cosh", "tanh", "asinh", "acosh", "atanh", "floor", "ceil",
    "round", "min", "max", "normpdf", "normcdf", "norminv", "logncdf", "erf", "erfc",
];

pub fn check_w130(model: &Model) -> Vec<Diagnostic> {
    let mut diagnostics = check_ss_order(model);
    diagnostics.extend(check_w140(model));
    diagnostics.extend(check_w150(model));
    diagnostics
}

fn check_ss_order(model: &Model) -> Vec<Diagnostic> {
    if model.steady_state_equations.is_empty() {
        return Vec::new();
    }

    let endogenous: HashSet<Name> = model.endogenous.iter().map(|d| d.name).collect();
    let mut assigned_anywhere = HashSet::new();
    for eq in &model.steady_state_equations {
        if let Some(name) = ss_lhs_ident(model, eq) {
            assigned_anywhere.insert(name);
        }
    }

    let mut assigned_so_far: HashSet<Name> = model
        .parameters
        .iter()
        .chain(&model.exogenous)
        .chain(&model.deterministic_exogenous)
        .map(|d| d.name)
        .collect();
    let mut assigned_once: HashSet<Name> = HashSet::new();
    let mut flagged_use_before: HashSet<Name> = HashSet::new();
    let mut diagnostics = Vec::new();

    for eq in &model.steady_state_equations {
        if let Some(rhs) = eq.rhs_expr {
            for r in model.exprs.walk_idents(rhs) {
                if endogenous.contains(&r.name)
                    && !assigned_so_far.contains(&r.name)
                    && assigned_anywhere.contains(&r.name)
                    && flagged_use_before.insert(r.name)
                {
                    let name = model.name(r.name);
                    diagnostics.push(Diagnostic::new(
                        r.span,
                        Severity::Warning,
                        "W130",
                        format!(
                            "'{name}' is used in the steady_state_model block before it is assigned. The block is evaluated top to bottom, so each variable must be assigned before use."
                        ),
                    ));
                }
            }
        }

        if let Some(lhs) = ss_lhs_ident(model, eq) {
            if assigned_once.contains(&lhs) && endogenous.contains(&lhs) {
                let reuses_self = eq
                    .rhs_expr
                    .is_some_and(|rhs| model.exprs.walk_idents(rhs).any(|r| r.name == lhs));
                if !reuses_self {
                    let name = model.name(lhs);
                    diagnostics.push(Diagnostic::new(
                        eq.span,
                        Severity::Warning,
                        "W131",
                        format!(
                            "'{name}' is assigned more than once in the steady_state_model block; the later assignment silently overrides the earlier one."
                        ),
                    ));
                }
            }
            assigned_once.insert(lhs);
            assigned_so_far.insert(lhs);
        }
    }
    diagnostics
}

fn ss_lhs_ident(model: &Model, eq: &Equation) -> Option<Name> {
    let id = eq.lhs_expr?;
    match &model.exprs.get(id).kind {
        ExprKind::Ident { name, .. } => Some(*name),
        _ => None,
    }
}

fn check_w140(model: &Model) -> Vec<Diagnostic> {
    if !model.is_linear {
        return Vec::new();
    }
    let mut vars: HashSet<Name> = model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.deterministic_exogenous)
        .map(|d| d.name)
        .collect();
    if vars.is_empty() {
        return Vec::new();
    }

    let index = LineIndex::new(&model.source);
    let mut seen: HashSet<(u32, String)> = HashSet::new();
    let mut diagnostics = Vec::new();
    for eq in &model.equations {
        if let Some((operator, span)) = equation_operator(model, eq, &vars) {
            let line = index.position(&model.source, eq.span.start).line;
            if seen.insert((line, operator.clone())) {
                diagnostics.push(Diagnostic::new(
                    span,
                    Severity::Warning,
                    "W140",
                    format!(
                        "Model is declared 'linear' but applies the nonlinear operator '{operator}' to a variable. Dynare requires the equations of a 'linear' model to be linear in the variables; drop 'linear' or linearise the equation."
                    ),
                ));
            }
        }
        if eq.is_local {
            if let (Some(name), Some(rhs)) = (ss_lhs_ident(model, eq), eq.rhs_expr) {
                if has_variable(model, rhs, &vars) {
                    vars.insert(name);
                }
            }
        }
    }
    diagnostics
}

fn equation_operator(model: &Model, eq: &Equation, vars: &HashSet<Name>) -> Option<(String, Span)> {
    for id in [eq.lhs_expr, eq.rhs_expr].into_iter().flatten() {
        if let Some(hit) = first_special_call(model, id, vars) {
            return Some(hit);
        }
        if let Some(hit) = first_operator(model, id, vars) {
            return Some(hit);
        }
    }
    None
}

fn first_special_call(model: &Model, id: ExprId, vars: &HashSet<Name>) -> Option<(String, Span)> {
    let expr = model.exprs.get(id);
    match &expr.kind {
        ExprKind::Call { callee, args } => {
            let c = model.name(*callee);
            if !c.eq_ignore_ascii_case("diff")
                && is_special(c)
                && args.iter().any(|a| has_variable(model, *a, vars))
            {
                return Some((c.to_ascii_lowercase(), expr.span));
            }
            for a in args {
                if let Some(hit) = first_special_call(model, *a, vars) {
                    return Some(hit);
                }
            }
            None
        }
        ExprKind::Unary { arg, .. } => first_special_call(model, *arg, vars),
        ExprKind::Binary { lhs, rhs, .. } => {
            first_special_call(model, *lhs, vars).or_else(|| first_special_call(model, *rhs, vars))
        }
        ExprKind::Expectation { arg, .. } => first_special_call(model, *arg, vars),
        ExprKind::SteadyState { .. }
        | ExprKind::Ident { .. }
        | ExprKind::Number
        | ExprKind::String
        | ExprKind::Error => None,
    }
}

fn first_operator(model: &Model, id: ExprId, vars: &HashSet<Name>) -> Option<(String, Span)> {
    let expr = model.exprs.get(id);
    match &expr.kind {
        ExprKind::Call { callee, args } => {
            let c = model.name(*callee);
            if !c.eq_ignore_ascii_case("diff") && args.iter().any(|a| has_variable(model, *a, vars))
            {
                let operator = if is_math_builtin(c) {
                    c.to_ascii_lowercase()
                } else {
                    c.to_string()
                };
                return Some((operator, expr.span));
            }
            for a in args {
                if let Some(hit) = first_operator(model, *a, vars) {
                    return Some(hit);
                }
            }
            None
        }
        ExprKind::Binary { op, lhs, rhs } => {
            let left_has = has_variable(model, *lhs, vars);
            let right_has = has_variable(model, *rhs, vars);
            let node_op = match op {
                BinOp::Mul if left_has && right_has => Some("*"),
                BinOp::Div if right_has => Some("/"),
                BinOp::Pow if left_has || right_has => {
                    if left_has && !right_has && is_linear_safe_exponent(model, *rhs) {
                        None
                    } else {
                        Some("^")
                    }
                }
                BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::EqEq | BinOp::Ne
                    if left_has || right_has =>
                {
                    Some("comparison")
                }
                _ => None,
            };
            if let Some(operator) = node_op {
                return Some((operator.to_string(), expr.span));
            }
            first_operator(model, *lhs, vars).or_else(|| first_operator(model, *rhs, vars))
        }
        ExprKind::Unary { arg, .. } | ExprKind::Expectation { arg, .. } => {
            first_operator(model, *arg, vars)
        }
        ExprKind::SteadyState { .. }
        | ExprKind::Ident { .. }
        | ExprKind::Number
        | ExprKind::String
        | ExprKind::Error => None,
    }
}

fn has_variable(model: &Model, id: ExprId, vars: &HashSet<Name>) -> bool {
    let expr = model.exprs.get(id);
    match &expr.kind {
        ExprKind::Ident { name, .. } => vars.contains(name),
        ExprKind::SteadyState { .. } => false,
        ExprKind::Expectation { arg, .. } => has_variable(model, *arg, vars),
        ExprKind::Call { callee, args } => {
            if model.name(*callee).eq_ignore_ascii_case("diff") {
                return false;
            }
            args.iter().any(|a| has_variable(model, *a, vars))
        }
        ExprKind::Unary { arg, .. } => has_variable(model, *arg, vars),
        ExprKind::Binary { lhs, rhs, .. } => {
            has_variable(model, *lhs, vars) || has_variable(model, *rhs, vars)
        }
        ExprKind::Number | ExprKind::String | ExprKind::Error => false,
    }
}

fn is_linear_safe_exponent(model: &Model, id: ExprId) -> bool {
    let expr = model.exprs.get(id);
    if !matches!(expr.kind, ExprKind::Number) {
        return false;
    }
    let Some(raw) = model
        .source
        .get(expr.span.start as usize..expr.span.end as usize)
    else {
        return false;
    };
    raw.parse::<f64>()
        .ok()
        .is_some_and(|v| v == 0.0 || v == 1.0)
}

fn is_special(name: &str) -> bool {
    SPECIAL_CALLS.iter().any(|s| name.eq_ignore_ascii_case(s))
}

fn is_math_builtin(name: &str) -> bool {
    MATH_BUILTINS.iter().any(|s| name.eq_ignore_ascii_case(s))
}

fn check_w150(model: &Model) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for &span in &model.simul_spans {
        diagnostics.push(deprecated(
            span,
            "'simul' is deprecated. Use 'perfect_foresight_setup' followed by 'perfect_foresight_solver'.",
        ));
    }
    if let Some(span) = model.ramsey_policy_span {
        diagnostics.push(deprecated(
            span,
            "'ramsey_policy' is deprecated. Use 'ramsey_model' followed by 'stoch_simul'.",
        ));
    }

    let mut declared: HashSet<String> = model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.parameters)
        .map(|d| model.name(d.name).to_string())
        .collect();
    for eq in &model.equations {
        if eq.is_local {
            if let Some(name) = ss_lhs_ident(model, eq) {
                declared.insert(model.name(name).to_string());
            }
        }
    }

    for (option, span) in &model.deprecated_option_spans {
        let lex = model
            .source
            .get(span.start as usize..span.end as usize)
            .unwrap_or("");
        if declared.contains(lex) {
            continue;
        }
        let message = match option {
            DeprecatedOption::AimSolver => {
                "The 'aim_solver' option is deprecated; use 'dr = aim' instead."
            }
            DeprecatedOption::Bytecode => {
                "The 'bytecode' option is deprecated and will be removed in a future Dynare release."
            }
        };
        diagnostics.push(deprecated(*span, message));
    }
    diagnostics
}

fn deprecated(span: Span, message: &str) -> Diagnostic {
    let mut d = Diagnostic::new(span, Severity::Warning, "W150", message);
    d.tags.push(2);
    d
}
