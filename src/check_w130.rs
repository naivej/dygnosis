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
    diagnostics.extend(check_static_dynamic_tags(model));
    diagnostics.extend(check_w200(model));
    diagnostics.extend(check_linear_ops(model));
    diagnostics.extend(check_w150(model));
    diagnostics.extend(check_e238(model));
    diagnostics.extend(check_w201(model));
    diagnostics
}

fn check_e238(model: &Model) -> Vec<Diagnostic> {
    let n = [
        model.stoch_simul_hp_filter,
        model.stoch_simul_one_sided_hp_filter,
        model.stoch_simul_bandpass_filter,
    ]
    .into_iter()
    .flatten()
    .count();
    if n <= 1 {
        return Vec::new();
    }
    let span = model
        .stoch_simul_hp_filter
        .or(model.stoch_simul_one_sided_hp_filter)
        .or(model.stoch_simul_bandpass_filter)
        .or(model.stoch_simul_span)
        .unwrap_or(Span { start: 0, end: 1 });
    vec![Diagnostic::new(
        span,
        Severity::Error,
        "E238",
        "stoch_simul: can only use one of HP, one-sided HP, and bandpass filters",
    )]
}

fn check_w201(model: &Model) -> Vec<Diagnostic> {
    let Some(span) = model.restriction_fname_span else {
        return Vec::new();
    };
    vec![Diagnostic::new(
        span,
        Severity::Warning,
        "W201",
        "restriction_fname is now deprecated, and may be removed in a future version of Dynare. Use svar_identification instead.",
    )]
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
                    let lhs = ss_lhs_ident(model, eq)
                        .map(|n| model.name(n).to_string())
                        .unwrap_or_else(|| name.to_string());
                    diagnostics.push(Diagnostic::new(
                        r.span,
                        Severity::Error,
                        "E130",
                        format!(
                            "variable '{name}' is undefined in the declaration of variable '{lhs}'"
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
                            "in the 'steady_state_model' block, variable '{name}' is declared twice"
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

const E208_MSG: &str =
    "the number of equations marked [static] must be equal to the number of equations marked [dynamic]";
const E209_MSG: &str = "marking equations as [static] or [dynamic] is not possible with ramsey_model, ramsey_policy or discretionary_policy";
const W200_MSG: &str = r#"you are using a function (max, min, abs, sign) or an operator (<, >, <=, >=, ==, !=) which is unsuitable for a stochastic context; see the reference manual, section about "Expressions", for more details."#;
const E210_MSG: &str = "you have declared your model 'linear' but you are using a function (max, min, abs, sign) or an operator (<, >, <=, >=, ==, !=) on an endogenous variable.";
const E211_MSG: &str = "you have declared your model 'linear' but you are using a function (max, min, abs, sign) or an operator (<, >, <=, >=, ==, !=) on an exogenous variable in a non-perfect-foresight context.";

fn check_static_dynamic_tags(model: &Model) -> Vec<Diagnostic> {
    let n_static = model.equations.iter().filter(|eq| eq.static_tag).count();
    let n_dynamic = model.equations.iter().filter(|eq| eq.dynamic_tag).count();
    let mut diagnostics = Vec::new();
    if n_static != n_dynamic {
        let span = model
            .equations
            .iter()
            .find(|eq| eq.static_tag || eq.dynamic_tag)
            .map(|eq| eq.span)
            .or(model.model_block)
            .unwrap_or(Span { start: 0, end: 1 });
        diagnostics.push(Diagnostic::new(span, Severity::Error, "E208", E208_MSG));
    }
    if n_static > 0 || n_dynamic > 0 {
        let ramsey_or_disc = model.policy_commands.iter().any(|c| {
            matches!(
                c,
                crate::model::PolicyCommand::RamseyModel
                    | crate::model::PolicyCommand::RamseyPolicy
                    | crate::model::PolicyCommand::DiscretionaryPolicy
            )
        });
        if ramsey_or_disc {
            let span = model
                .policy_command_span
                .or(model.model_block)
                .unwrap_or(Span { start: 0, end: 1 });
            diagnostics.push(Diagnostic::new(span, Severity::Error, "E209", E209_MSG));
        }
    }
    diagnostics
}

fn check_w200(model: &Model) -> Vec<Diagnostic> {
    if !model.is_stochastic_context() {
        return Vec::new();
    }
    // Official `isUnaryOpUsed` / `isBinaryOpUsed` walk the whole DataTree,
    // including `#` model-local RHS nodes.
    for eq in &model.equations {
        for id in [eq.lhs_expr, eq.rhs_expr].into_iter().flatten() {
            if let Some(span) = first_nonsmooth(model, id) {
                return vec![Diagnostic::new(span, Severity::Warning, "W200", W200_MSG)];
            }
        }
    }
    Vec::new()
}

fn check_linear_ops(model: &Model) -> Vec<Diagnostic> {
    if !model.is_linear {
        return Vec::new();
    }
    let endo: HashSet<Name> = model.endogenous.iter().map(|d| d.name).collect();
    let exo: HashSet<Name> = model
        .exogenous
        .iter()
        .chain(&model.deterministic_exogenous)
        .map(|d| d.name)
        .collect();
    let mut vars: HashSet<Name> = endo.union(&exo).copied().collect();
    if vars.is_empty() {
        return Vec::new();
    }

    let pf = model.is_pf_solver_context();
    let index = LineIndex::new(&model.source);
    let mut seen: HashSet<(u32, String)> = HashSet::new();
    let mut diagnostics = Vec::new();
    for eq in &model.equations {
        if let Some((kind, operator, span)) = linear_hit(model, eq, &endo, &exo, &vars) {
            let line = index.position(&model.source, eq.span.start).line;
            if !seen.insert((line, operator.clone())) {
                continue;
            }
            match kind {
                LinearHit::EndoNonsmooth => diagnostics.push(Diagnostic::new(
                    span,
                    Severity::Error,
                    "E210",
                    E210_MSG,
                )),
                LinearHit::ExoNonsmooth if !pf => diagnostics.push(Diagnostic::new(
                    span,
                    Severity::Error,
                    "E211",
                    E211_MSG,
                )),
                LinearHit::ExoNonsmooth | LinearHit::Extra => diagnostics.push(Diagnostic::new(
                    span,
                    Severity::Warning,
                    "W140",
                    format!(
                        "Model is declared 'linear' but applies the nonlinear operator '{operator}' to a variable. Dynare requires the equations of a 'linear' model to be linear in the variables; drop 'linear' or linearise the equation."
                    ),
                )),
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

enum LinearHit {
    EndoNonsmooth,
    ExoNonsmooth,
    Extra,
}

fn linear_hit(
    model: &Model,
    eq: &Equation,
    endo: &HashSet<Name>,
    exo: &HashSet<Name>,
    vars: &HashSet<Name>,
) -> Option<(LinearHit, String, Span)> {
    for id in [eq.lhs_expr, eq.rhs_expr].into_iter().flatten() {
        if let Some((target, operator, span)) = first_nonsmooth_on_type(model, id, endo, exo) {
            let kind = if target == NonsmoothOn::Endo {
                LinearHit::EndoNonsmooth
            } else {
                LinearHit::ExoNonsmooth
            };
            return Some((kind, operator, span));
        }
        if let Some((operator, span)) = equation_operator_at(model, id, vars) {
            return Some((LinearHit::Extra, operator, span));
        }
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NonsmoothOn {
    Endo,
    Exo,
}

fn first_nonsmooth(model: &Model, id: ExprId) -> Option<Span> {
    first_nonsmooth_on_type(model, id, &HashSet::new(), &HashSet::new()).map(|(_, _, span)| span)
}

fn first_nonsmooth_on_type(
    model: &Model,
    id: ExprId,
    endo: &HashSet<Name>,
    exo: &HashSet<Name>,
) -> Option<(NonsmoothOn, String, Span)> {
    let expr = model.exprs.get(id);
    match &expr.kind {
        ExprKind::Call { callee, args } => {
            let c = model.name(*callee);
            if is_special(c) {
                let operator = c.to_ascii_lowercase();
                if endo.is_empty() && exo.is_empty() {
                    return Some((NonsmoothOn::Endo, operator, expr.span));
                }
                if args.iter().any(|a| has_variable(model, *a, endo)) {
                    return Some((NonsmoothOn::Endo, operator, expr.span));
                }
                if args.iter().any(|a| has_variable(model, *a, exo)) {
                    return Some((NonsmoothOn::Exo, operator, expr.span));
                }
            }
            for a in args {
                if let Some(hit) = first_nonsmooth_on_type(model, *a, endo, exo) {
                    return Some(hit);
                }
            }
            None
        }
        ExprKind::Binary { op, lhs, rhs } if is_nonsmooth_cmp(*op) => {
            let operator = nonsmooth_cmp_op(*op).to_string();
            if endo.is_empty() && exo.is_empty() {
                return Some((NonsmoothOn::Endo, operator, expr.span));
            }
            if has_variable(model, *lhs, endo) || has_variable(model, *rhs, endo) {
                return Some((NonsmoothOn::Endo, operator, expr.span));
            }
            if has_variable(model, *lhs, exo) || has_variable(model, *rhs, exo) {
                return Some((NonsmoothOn::Exo, operator, expr.span));
            }
            first_nonsmooth_on_type(model, *lhs, endo, exo)
                .or_else(|| first_nonsmooth_on_type(model, *rhs, endo, exo))
        }
        ExprKind::Unary { arg, .. } | ExprKind::Expectation { arg, .. } => {
            first_nonsmooth_on_type(model, *arg, endo, exo)
        }
        ExprKind::Binary { lhs, rhs, .. } => first_nonsmooth_on_type(model, *lhs, endo, exo)
            .or_else(|| first_nonsmooth_on_type(model, *rhs, endo, exo)),
        ExprKind::SteadyState { .. }
        | ExprKind::Ident { .. }
        | ExprKind::Number
        | ExprKind::String
        | ExprKind::Error => None,
    }
}

fn is_nonsmooth_cmp(op: BinOp) -> bool {
    matches!(
        op,
        BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::EqEq | BinOp::Ne
    )
}

fn nonsmooth_cmp_op(op: BinOp) -> &'static str {
    match op {
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::Le => "<=",
        BinOp::Ge => ">=",
        BinOp::EqEq => "==",
        BinOp::Ne => "!=",
        _ => "comparison",
    }
}

fn equation_operator_at(model: &Model, id: ExprId, vars: &HashSet<Name>) -> Option<(String, Span)> {
    first_special_call(model, id, vars).or_else(|| first_operator(model, id, vars))
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
            "'simul' is deprecated. Please use 'perfect_foresight_setup' and 'perfect_foresight_solver' instead.",
        ));
    }
    if let Some(span) = model.ramsey_policy_span {
        diagnostics.push(deprecated(
            span,
            "'ramsey_policy' is deprecated. Please use 'ramsey_model', 'stoch_simul', and 'evaluate_planner_objective' instead.",
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
                "The 'aim_solver' option is deprecated. It has been superseded by the 'dr=aim' option."
            }
            DeprecatedOption::Bytecode => {
                "the 'bytecode' option is deprecated and will be removed in a future release of Dynare."
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
