//! W010–W012 / W020–W022 usage, timing, and calibration diagnostics.

use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, Severity};
use crate::expr::{BinOp, ExprId, ExprKind, UnOp};
use crate::intern::Name;
use crate::lexer::{tokenize, TokenKind};
use crate::model::{Assignment, Equation, Model};
use crate::span::{LineIndex, Span};

const RUN_COMMANDS: &[&str] = &[
    "steady",
    "check",
    "stoch_simul",
    "simul",
    "estimation",
    "perfect_foresight_setup",
    "perfect_foresight_solver",
    "ramsey_policy",
    "discretionary_policy",
    "osr",
    "calib_smoother",
    "forecast",
];

pub fn check_w010(model: &Model) -> Vec<Diagnostic> {
    let mut assigned: HashSet<Name> = model.param_assignments.iter().map(|a| a.name).collect();
    for eq in &model.steady_state_equations {
        if let Some(name) = ss_lhs_ident(model, eq) {
            assigned.insert(name);
        }
    }
    let mut diagnostics = Vec::new();
    for p in &model.parameters {
        if assigned.contains(&p.name) {
            continue;
        }
        let name = model.name(p.name);
        diagnostics.push(Diagnostic::new(
            p.span,
            Severity::Warning,
            "W010",
            format!("Parameter '{name}' is declared but never assigned a value."),
        ));
    }
    diagnostics
}

pub fn check_w011(model: &Model) -> Vec<Diagnostic> {
    let param_names: HashSet<Name> = model.parameters.iter().map(|d| d.name).collect();
    let subjects = unevaluable_subjects(model, &param_names);
    if subjects.is_empty() {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    let mut reported: HashSet<(Name, u32)> = HashSet::new();
    let commands = run_command_positions(&model.source);

    for cmd in &commands {
        let mut latest: HashMap<Name, usize> = HashMap::new();
        for (i, row) in subjects.iter().enumerate() {
            if row.span.start < *cmd {
                latest.insert(row.name, i);
            }
        }
        for i in latest.values() {
            push_w011(&mut diagnostics, &mut reported, model, &subjects[*i]);
        }
    }

    let mut latest: HashMap<Name, usize> = HashMap::new();
    for (i, row) in subjects.iter().enumerate() {
        latest.insert(row.name, i);
    }
    for i in latest.values() {
        push_w011(&mut diagnostics, &mut reported, model, &subjects[*i]);
    }
    diagnostics
}

pub fn check_w012(model: &Model) -> Vec<Diagnostic> {
    let declared = declared_names(model);
    let index = LineIndex::new(&model.source);
    let first_block_line = [
        model.model_block,
        model.initval_block,
        model.ss_block,
        model.shocks_block,
    ]
    .into_iter()
    .flatten()
    .map(|span| index.position(&model.source, span.start).line)
    .min();

    let mut diagnostics = Vec::new();
    for a in &model.helper_assignments {
        if declared.contains(&a.name) {
            continue;
        }
        if let Some(line) = first_block_line {
            let assign_line = index.position(&model.source, a.span.start).line;
            if assign_line >= line {
                continue;
            }
        }
        let name = model.name(a.name);
        diagnostics.push(Diagnostic::new(
            a.span,
            Severity::Information,
            "W012",
            format!(
                "'{name}' is assigned but not declared as a parameter. If it is used to compute other parameters, consider declaring it in the parameters block."
            ),
        ));
    }
    diagnostics
}

pub fn check_w020(model: &Model) -> Vec<Diagnostic> {
    let has_static = model
        .equations
        .iter()
        .any(|eq| !eq.is_local && !eq.dynamic_tag);
    if !has_static {
        return Vec::new();
    }
    let referenced = model_eq_refs(model);
    unused_decls(
        model,
        &model.endogenous,
        &referenced,
        "W020",
        "Endogenous variable",
        "the model block",
    )
}

pub fn check_w021(model: &Model) -> Vec<Diagnostic> {
    if model.equations.is_empty() {
        return Vec::new();
    }
    let referenced = model_eq_refs(model);
    unused_decls(
        model,
        &model.exogenous,
        &referenced,
        "W021",
        "Exogenous variable",
        "the model block",
    )
}

pub fn check_w022(model: &Model) -> Vec<Diagnostic> {
    if model.equations.is_empty() {
        return Vec::new();
    }
    let mut referenced = model_eq_refs(model);
    for eq in &model.steady_state_equations {
        for r in model.ident_refs(eq) {
            referenced.insert(r.name);
        }
    }
    extend_shocks_idents(model, &mut referenced);
    walk_assignment_idents(model, &model.param_assignments, &mut referenced);
    walk_assignment_idents(model, &model.helper_assignments, &mut referenced);
    walk_assignment_idents(model, &model.initval, &mut referenced);

    let mut diagnostics = Vec::new();
    for p in &model.parameters {
        if referenced.contains(&p.name) {
            continue;
        }
        let name = model.name(p.name);
        diagnostics.push(Diagnostic::new(
            p.span,
            Severity::Information,
            "W022",
            format!("Parameter '{name}' is declared but never referenced in model equations."),
        ));
    }
    diagnostics
}

pub fn check_w010_family(model: &Model) -> Vec<Diagnostic> {
    let mut out = check_w010(model);
    out.extend(check_w011(model));
    out.extend(check_w012(model));
    out.extend(check_w020(model));
    out.extend(check_w021(model));
    out.extend(check_w022(model));
    out
}

struct CalibRow {
    name: Name,
    expression: String,
    span: Span,
    value: Option<f64>,
}

fn unevaluable_subjects(model: &Model, param_names: &HashSet<Name>) -> Vec<CalibRow> {
    let mut known: HashMap<Name, f64> = HashMap::new();
    let mut indexed: Vec<(usize, &Assignment)> = model
        .param_assignments
        .iter()
        .chain(model.helper_assignments.iter())
        .enumerate()
        .collect();
    indexed.sort_by_key(|(i, a)| (a.span.start, *i));

    let mut subjects = Vec::new();
    for (_, a) in indexed {
        let value = a.expr.and_then(|id| eval_expr(model, id, &known));
        bind(&mut known, a.name, value);
        if param_names.contains(&a.name) {
            subjects.push(CalibRow {
                name: a.name,
                expression: a.expression.trim().to_string(),
                span: a.span,
                value,
            });
        }
    }

    for exo in &model.exogenous {
        known.insert(exo.name, 0.0);
    }

    for eq in &model.steady_state_equations {
        let Some(name) = ss_lhs_ident(model, eq) else {
            continue;
        };
        let value = eq.rhs_expr.and_then(|id| eval_expr(model, id, &known));
        bind(&mut known, name, value);
        if param_names.contains(&name) {
            subjects.push(CalibRow {
                name,
                expression: eq.rhs.trim().to_string(),
                span: eq.span,
                value,
            });
        }
    }
    subjects
}

fn bind(known: &mut HashMap<Name, f64>, name: Name, value: Option<f64>) {
    match value {
        Some(v) => {
            known.insert(name, v);
        }
        None => {
            known.remove(&name);
        }
    }
}

fn push_w011(
    diagnostics: &mut Vec<Diagnostic>,
    reported: &mut HashSet<(Name, u32)>,
    model: &Model,
    row: &CalibRow,
) {
    if row.value.is_some() {
        return;
    }
    if !reported.insert((row.name, row.span.start)) {
        return;
    }
    let name = model.name(row.name);
    diagnostics.push(Diagnostic::new(
        row.span,
        Severity::Warning,
        "W011",
        format!(
            "Parameter '{name}' assignment could not be evaluated: {name} = {}. Check that all referenced names are declared and assigned.",
            row.expression
        ),
    ));
}

fn run_command_positions(source: &str) -> Vec<u32> {
    tokenize(source)
        .into_iter()
        .filter(|tok| {
            tok.kind == TokenKind::Ident
                && RUN_COMMANDS
                    .iter()
                    .any(|cmd| tok.text(source).eq_ignore_ascii_case(cmd))
        })
        .map(|tok| tok.span.start)
        .collect()
}

fn ss_lhs_ident(model: &Model, eq: &Equation) -> Option<Name> {
    if eq.is_local {
        return None;
    }
    let id = eq.lhs_expr?;
    match &model.exprs.get(id).kind {
        ExprKind::Ident { name, .. } => Some(*name),
        _ => None,
    }
}

fn declared_names(model: &Model) -> HashSet<Name> {
    model
        .endogenous
        .iter()
        .chain(model.exogenous.iter())
        .chain(model.parameters.iter())
        .map(|d| d.name)
        .collect()
}

fn model_eq_refs(model: &Model) -> HashSet<Name> {
    let mut refs = HashSet::new();
    for eq in &model.equations {
        for r in model.ident_refs(eq) {
            refs.insert(r.name);
        }
    }
    refs
}

fn unused_decls(
    model: &Model,
    decls: &[crate::model::Decl],
    referenced: &HashSet<Name>,
    code: &str,
    kind: &str,
    where_: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for d in decls {
        if referenced.contains(&d.name) {
            continue;
        }
        let name = model.name(d.name);
        diagnostics.push(Diagnostic::new(
            d.span,
            Severity::Warning,
            code,
            format!("{kind} '{name}' is declared but never referenced in {where_}."),
        ));
    }
    diagnostics
}

fn extend_shocks_idents(model: &Model, referenced: &mut HashSet<Name>) {
    let Some(span) = model.shocks_block else {
        return;
    };
    for tok in tokenize(&model.source) {
        if tok.kind != TokenKind::Ident {
            continue;
        }
        if tok.span.start < span.start || tok.span.end > span.end {
            continue;
        }
        let text = tok.text(&model.source);
        if let Some(d) = model.parameters.iter().find(|d| model.name(d.name) == text) {
            referenced.insert(d.name);
        }
    }
}

fn walk_assignment_idents(model: &Model, rows: &[Assignment], referenced: &mut HashSet<Name>) {
    for a in rows {
        let Some(id) = a.expr else {
            continue;
        };
        for r in model.exprs.walk_idents(id) {
            referenced.insert(r.name);
        }
    }
}

fn eval_expr(model: &Model, id: ExprId, known: &HashMap<Name, f64>) -> Option<f64> {
    match &model.exprs.get(id).kind {
        ExprKind::Number => {
            let span = model.exprs.get(id).span;
            let raw = model.source.get(span.start as usize..span.end as usize)?;
            raw.parse().ok()
        }
        ExprKind::Ident { name, timing, .. } => {
            if *timing != 0 {
                return None;
            }
            if let Some(&v) = known.get(name) {
                return Some(v);
            }
            let s = model.name(*name);
            if s.eq_ignore_ascii_case("pi") {
                Some(std::f64::consts::PI)
            } else if s.eq_ignore_ascii_case("inf") {
                Some(f64::INFINITY)
            } else if s.eq_ignore_ascii_case("nan") {
                Some(f64::NAN)
            } else {
                None
            }
        }
        ExprKind::Unary { op, arg } => {
            let v = eval_expr(model, *arg, known)?;
            Some(match op {
                UnOp::Pos => v,
                UnOp::Neg => -v,
            })
        }
        ExprKind::Binary { op, lhs, rhs } => {
            let l = eval_expr(model, *lhs, known)?;
            let r = eval_expr(model, *rhs, known)?;
            match op {
                BinOp::Add => Some(l + r),
                BinOp::Sub => Some(l - r),
                BinOp::Mul => Some(l * r),
                BinOp::Div => Some(l / r),
                BinOp::Pow => Some(l.powf(r)),
                BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::EqEq | BinOp::Ne => None,
            }
        }
        ExprKind::Call { callee, args } => {
            let fname = model.name(*callee);
            eval_call(model, fname, args, known)
        }
        ExprKind::String
        | ExprKind::Error
        | ExprKind::SteadyState { .. }
        | ExprKind::Expectation { .. } => None,
    }
}

fn eval_call(
    model: &Model,
    fname: &str,
    args: &[ExprId],
    known: &HashMap<Name, f64>,
) -> Option<f64> {
    let mut vals = Vec::with_capacity(args.len());
    for a in args {
        vals.push(eval_expr(model, *a, known)?);
    }
    let f = fname.to_ascii_lowercase();
    match f.as_str() {
        "exp" => unary(&vals, f64::exp),
        "log" | "ln" => unary(&vals, f64::ln),
        "log2" => unary(&vals, f64::log2),
        "log10" => unary(&vals, f64::log10),
        "sqrt" => unary(&vals, f64::sqrt),
        "cbrt" => unary(&vals, |x| x.abs().powf(1.0 / 3.0).copysign(x)),
        "abs" => unary(&vals, f64::abs),
        "sign" => unary(&vals, |x| {
            if x > 0.0 {
                1.0
            } else if x < 0.0 {
                -1.0
            } else {
                0.0
            }
        }),
        "sin" => unary(&vals, f64::sin),
        "cos" => unary(&vals, f64::cos),
        "tan" => unary(&vals, f64::tan),
        "asin" => unary(&vals, f64::asin),
        "acos" => unary(&vals, f64::acos),
        "atan" => unary(&vals, f64::atan),
        "sinh" => unary(&vals, f64::sinh),
        "cosh" => unary(&vals, f64::cosh),
        "tanh" => unary(&vals, f64::tanh),
        "asinh" => unary(&vals, f64::asinh),
        "acosh" => unary(&vals, f64::acosh),
        "atanh" => unary(&vals, f64::atanh),
        "floor" => unary(&vals, f64::floor),
        "ceil" => unary(&vals, f64::ceil),
        "round" => unary(&vals, dynare_round),
        "erf" => unary(&vals, erf),
        "erfc" => unary(&vals, |x| 1.0 - erf(x)),
        "min" => {
            if vals.is_empty() {
                None
            } else {
                vals.into_iter().reduce(f64::min)
            }
        }
        "max" => {
            if vals.is_empty() {
                None
            } else {
                vals.into_iter().reduce(f64::max)
            }
        }
        "normpdf" => {
            let x = *vals.first()?;
            let mu = vals.get(1).copied().unwrap_or(0.0);
            let sigma = vals.get(2).copied().unwrap_or(1.0);
            if vals.len() > 3 {
                return None;
            }
            Some(
                (-0.5 * ((x - mu) / sigma).powi(2)).exp()
                    / (sigma * (2.0 * std::f64::consts::PI).sqrt()),
            )
        }
        "normcdf" => {
            let x = *vals.first()?;
            let mu = vals.get(1).copied().unwrap_or(0.0);
            let sigma = vals.get(2).copied().unwrap_or(1.0);
            if vals.len() > 3 {
                return None;
            }
            Some(0.5 * (1.0 + erf((x - mu) / (sigma * 2.0_f64.sqrt()))))
        }
        "norminv" => {
            let p = *vals.first()?;
            let mu = vals.get(1).copied().unwrap_or(0.0);
            let sigma = vals.get(2).copied().unwrap_or(1.0);
            if vals.len() > 3 {
                return None;
            }
            Some(norminv(p, mu, sigma))
        }
        "logncdf" => {
            let x = *vals.first()?;
            let mu = vals.get(1).copied().unwrap_or(0.0);
            let sigma = vals.get(2).copied().unwrap_or(1.0);
            if vals.len() > 3 {
                return None;
            }
            if sigma <= 0.0 {
                return Some(f64::NAN);
            }
            if x <= 0.0 {
                return Some(0.0);
            }
            Some(0.5 * (1.0 + erf((x.ln() - mu) / (sigma * 2.0_f64.sqrt()))))
        }
        _ => None,
    }
}

fn unary(vals: &[f64], f: fn(f64) -> f64) -> Option<f64> {
    if vals.len() == 1 {
        Some(f(vals[0]))
    } else {
        None
    }
}

fn dynare_round(value: f64) -> f64 {
    if value >= 0.0 {
        (value + 0.5).floor()
    } else {
        (value - 0.5).ceil()
    }
}

/// Abramowitz & Stegun 7.1.26.
fn erf(x: f64) -> f64 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let ans = t * f64::exp(
        -z * z - 1.26551223
            + t * (1.00002368
                + t * (0.37409196
                    + t * (0.09678418
                        + t * (-0.18628806
                            + t * (0.27886807
                                + t * (-1.13520398
                                    + t * (1.48851587 + t * (-0.82215223 + t * 0.17087277)))))))),
    );
    if x >= 0.0 {
        1.0 - ans
    } else {
        ans - 1.0
    }
}

/// Acklam's inverse normal CDF, then affine scale.
fn norminv(p: f64, mu: f64, sigma: f64) -> f64 {
    if sigma <= 0.0 || !(0.0 < p && p < 1.0) {
        return f64::NAN;
    }
    mu + sigma * standard_norminv(p)
}

fn standard_norminv(p: f64) -> f64 {
    const A: [f64; 6] = [
        -3.969683028665376e+01,
        2.209460984245205e+02,
        -2.759285104469687e+02,
        1.383577459006915e+02,
        -3.066479806614716e+01,
        2.506628277459239e+00,
    ];
    const B: [f64; 5] = [
        -5.447609879822406e+01,
        1.615858368580409e+02,
        -1.556989798598866e+02,
        6.680131188771972e+01,
        -1.328068155288572e+01,
    ];
    const C: [f64; 6] = [
        -7.784894002430293e-03,
        -3.223964580411365e-01,
        -2.400758277161838e+00,
        -2.549732539343734e+00,
        4.374664141464968e+00,
        2.938163982698783e+00,
    ];
    const D: [f64; 4] = [
        7.784695709041462e-03,
        3.224671290700398e-01,
        2.445134137142996e+00,
        3.754408661907416e+00,
    ];
    const P_LOW: f64 = 0.02425;
    const P_HIGH: f64 = 1.0 - P_LOW;
    if p < P_LOW {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= P_HIGH {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    }
}
