//! W120–W122: stochastic commands, timed parameters, and non-finite deep params.

use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, Severity};
use crate::expr::{BinOp, ExprId, ExprKind, UnOp};
use crate::intern::Name;
use crate::lexer::{tokenize, TokenKind};
use crate::model::{Assignment, Model};
use crate::span::{LineIndex, Span};

const STOCH_COMMANDS: &[&str] = &["stoch_simul", "estimation"];

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

const AUTO_PARAM: &str = "optimal_policy_discount_factor";

pub fn check_w120(model: &Model) -> Vec<Diagnostic> {
    let det: HashSet<Name> = model
        .deterministic_exogenous
        .iter()
        .map(|d| d.name)
        .collect();
    let has_stochastic = model.exogenous.iter().any(|d| !det.contains(&d.name));
    if has_stochastic {
        return Vec::new();
    }
    let Some((name, span)) = first_stoch_command(&model.source) else {
        return Vec::new();
    };
    vec![Diagnostic::new(
        span,
        Severity::Warning,
        "W120",
        format!(
            "'{name}' is a stochastic command but the model declares no stochastic exogenous variable. Dynare requires at least one 'varexo'; add a (dummy) shock and a shocks-block entry for it."
        ),
    )]
}

pub fn check_w121(model: &Model) -> Vec<Diagnostic> {
    let params: HashSet<Name> = model.parameters.iter().map(|d| d.name).collect();
    if params.is_empty() {
        return Vec::new();
    }
    let index = LineIndex::new(&model.source);
    let mut seen: HashSet<(Name, u32)> = HashSet::new();
    let mut diagnostics = Vec::new();
    for eq in &model.equations {
        if eq.is_local || eq.static_tag {
            continue;
        }
        let line = index.position(&model.source, eq.span.start).line;
        for r in model.ident_refs(eq) {
            if r.timing == 0 || !params.contains(&r.name) {
                continue;
            }
            let Some(timing_span) = r.timing_span else {
                continue;
            };
            if !seen.insert((r.name, line)) {
                continue;
            }
            let name = model.name(r.name);
            let index_text = timing_index(&model.source, timing_span);
            diagnostics.push(Diagnostic::new(
                Span {
                    start: r.span.start,
                    end: timing_span.end,
                },
                Severity::Warning,
                "W121",
                format!(
                    "Parameter '{name}' is used with a lead/lag ('{name}({index_text})'). Parameters are time-invariant; this is usually a variable mis-declared as a parameter, or a stray time index."
                ),
            ));
        }
    }
    diagnostics
}

pub fn check_w122(model: &Model) -> Vec<Diagnostic> {
    let commands = run_commands(&model.source);
    if commands.is_empty() {
        return Vec::new();
    }
    let param_names: HashSet<Name> = model.parameters.iter().map(|d| d.name).collect();
    let used = used_in_model(model);
    let rows = folded_param_assignments(model, &param_names);
    let mut diagnostics = Vec::new();
    let mut reported: HashSet<(Name, u32)> = HashSet::new();
    for cmd in &commands {
        let mut latest: HashMap<Name, usize> = HashMap::new();
        for (i, row) in rows.iter().enumerate() {
            if row.span.start < cmd.span.start {
                latest.insert(row.name, i);
            }
        }
        for i in latest.values().copied() {
            let row = &rows[i];
            let name = model.name(row.name);
            if name == AUTO_PARAM || !used.contains(&row.name) {
                continue;
            }
            let Some(kind) = nonfinite_kind(&row.expression, row.value) else {
                continue;
            };
            if !reported.insert((row.name, row.span.start)) {
                continue;
            }
            diagnostics.push(Diagnostic::new(
                row.span,
                Severity::Warning,
                "W122",
                format!(
                    "Parameter '{name}' is assigned {kind}. Dynare requires every deep parameter used in the model to be finite before running '{}'.",
                    cmd.lexeme
                ),
            ));
        }
    }
    diagnostics
}

pub fn check_w120_family(model: &Model) -> Vec<Diagnostic> {
    let mut out = check_w120(model);
    out.extend(check_w121(model));
    out.extend(check_w122(model));
    out
}

struct CommandTok {
    lexeme: String,
    span: Span,
}

struct FoldedAssign {
    name: Name,
    expression: String,
    span: Span,
    value: Option<f64>,
}

fn first_stoch_command(source: &str) -> Option<(String, Span)> {
    tokenize(source).into_iter().find_map(|tok| {
        if tok.kind != TokenKind::Ident {
            return None;
        }
        let text = tok.text(source);
        if STOCH_COMMANDS
            .iter()
            .any(|cmd| text.eq_ignore_ascii_case(cmd))
        {
            Some((text.to_string(), tok.span))
        } else {
            None
        }
    })
}

fn run_commands(source: &str) -> Vec<CommandTok> {
    tokenize(source)
        .into_iter()
        .filter(|tok| {
            tok.kind == TokenKind::Ident
                && RUN_COMMANDS
                    .iter()
                    .any(|cmd| tok.text(source).eq_ignore_ascii_case(cmd))
        })
        .map(|tok| CommandTok {
            lexeme: tok.text(source).to_string(),
            span: tok.span,
        })
        .collect()
}

fn timing_index(source: &str, timing_span: Span) -> String {
    let raw = source
        .get(timing_span.start as usize..timing_span.end as usize)
        .unwrap_or("");
    let inner = raw
        .trim()
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(raw);
    inner.trim().to_string()
}

fn used_in_model(model: &Model) -> HashSet<Name> {
    let mut used = HashSet::new();
    for eq in &model.equations {
        if eq.is_local {
            continue;
        }
        for r in model.ident_refs(eq) {
            used.insert(r.name);
        }
    }
    used
}

fn folded_param_assignments(model: &Model, param_names: &HashSet<Name>) -> Vec<FoldedAssign> {
    let mut known: HashMap<Name, f64> = HashMap::new();
    let mut indexed: Vec<(usize, &Assignment)> = model
        .param_assignments
        .iter()
        .chain(model.helper_assignments.iter())
        .enumerate()
        .collect();
    indexed.sort_by_key(|(i, a)| (a.span.start, *i));

    let mut rows = Vec::new();
    for (_, a) in indexed {
        let value = a.expr.and_then(|id| eval_expr(model, id, &known));
        bind(&mut known, a.name, value);
        if param_names.contains(&a.name) {
            rows.push(FoldedAssign {
                name: a.name,
                expression: a.expression.trim().to_string(),
                span: a.span,
                value,
            });
        }
    }
    rows
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

fn nonfinite_kind(expression: &str, value: Option<f64>) -> Option<&'static str> {
    if let Some(kind) = literal_kind(expression) {
        return Some(kind);
    }
    match value {
        Some(v) if !v.is_finite() => {
            if v.is_infinite() {
                Some("Inf")
            } else {
                Some("a non-finite value")
            }
        }
        _ => None,
    }
}

fn literal_kind(expression: &str) -> Option<&'static str> {
    let t = expression.trim();
    let t = t.strip_prefix(['+', '-']).unwrap_or(t);
    match t.to_ascii_lowercase().as_str() {
        "nan" => Some("NaN"),
        "inf" | "infinity" => Some("Inf"),
        _ => None,
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
