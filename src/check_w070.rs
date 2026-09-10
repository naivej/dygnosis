//! W070 conventional parameter bounds.

use std::collections::HashMap;

use crate::diagnostic::{Diagnostic, Severity};
use crate::expr::{BinOp, ExprId, ExprKind, UnOp};
use crate::intern::Name;
use crate::model::Model;

struct Bound {
    pattern: &'static str,
    low: Option<f64>,
    high: Option<f64>,
    strict_low: bool,
    strict_high: bool,
    rationale: &'static str,
}

/// Copy of `python_dynare_lsp/bounds.py` `_BOUNDS`. First-match, case-insensitive
/// substring — not “narrowest”.
const BOUNDS: &[Bound] = &[
    Bound {
        pattern: "beta",
        low: Some(0.0),
        high: Some(1.0),
        strict_low: true,
        strict_high: true,
        rationale: "discount factor must lie in (0,1)",
    },
    Bound {
        pattern: "betta",
        low: Some(0.0),
        high: Some(1.0),
        strict_low: true,
        strict_high: true,
        rationale: "discount factor must lie in (0,1)",
    },
    Bound {
        pattern: "alpha",
        low: Some(0.0),
        high: Some(1.0),
        strict_low: true,
        strict_high: true,
        rationale: "capital share / output elasticity is typically in (0,1)",
    },
    Bound {
        pattern: "delta",
        low: Some(0.0),
        high: Some(1.0),
        strict_low: false,
        strict_high: true,
        rationale: "depreciation rate must lie in [0,1)",
    },
    Bound {
        pattern: "frisch",
        low: Some(0.0),
        high: Some(20.0),
        strict_low: false,
        strict_high: false,
        rationale: "inverse Frisch elasticity is typically in [0,20]",
    },
    Bound {
        pattern: "sigma_c",
        low: Some(0.0),
        high: Some(20.0),
        strict_low: true,
        strict_high: false,
        rationale: "risk aversion / 1/IES is typically in (0,20]",
    },
    Bound {
        pattern: "sigma_",
        low: Some(0.0),
        high: None,
        strict_low: false,
        strict_high: false,
        rationale: "standard deviation must be non-negative",
    },
    Bound {
        pattern: "std_",
        low: Some(0.0),
        high: None,
        strict_low: false,
        strict_high: false,
        rationale: "standard deviation must be non-negative",
    },
    Bound {
        pattern: "var_",
        low: Some(0.0),
        high: None,
        strict_low: false,
        strict_high: false,
        rationale: "variance must be non-negative",
    },
    Bound {
        pattern: "rho",
        low: Some(-1.0),
        high: Some(1.0),
        strict_low: true,
        strict_high: true,
        rationale: "AR(1) persistence must lie in (-1,1) for stationarity",
    },
    Bound {
        pattern: "phi_pi",
        low: Some(0.0),
        high: Some(10.0),
        strict_low: false,
        strict_high: false,
        rationale: "Taylor rule inflation coefficient should be non-negative; >1 satisfies the Taylor principle",
    },
    Bound {
        pattern: "phi_y",
        low: Some(-1.0),
        high: Some(5.0),
        strict_low: false,
        strict_high: false,
        rationale: "Taylor rule output-gap coefficient is typically in [-1,5]",
    },
    Bound {
        pattern: "theta",
        low: Some(0.0),
        high: Some(1.0),
        strict_low: false,
        strict_high: true,
        rationale: "Calvo / share parameter must lie in [0,1)",
    },
    Bound {
        pattern: "habit",
        low: Some(0.0),
        high: Some(1.0),
        strict_low: false,
        strict_high: true,
        rationale: "habit persistence is typically in [0,1)",
    },
    Bound {
        pattern: "iota",
        low: Some(0.0),
        high: Some(1.0),
        strict_low: false,
        strict_high: false,
        rationale: "indexation parameter is typically in [0,1]",
    },
    Bound {
        pattern: "epsilon",
        low: Some(1.0),
        high: None,
        strict_low: true,
        strict_high: false,
        rationale: "demand elasticity must exceed 1 for positive markup",
    },
    Bound {
        pattern: "pi_bar",
        low: Some(-0.1),
        high: Some(1.5),
        strict_low: false,
        strict_high: false,
        rationale: "steady-state inflation rate is typically in [-10%,50%] (net) or [0.9,1.5] (gross); accept both conventions.",
    },
    Bound {
        pattern: "r_bar",
        low: Some(-0.05),
        high: Some(1.5),
        strict_low: false,
        strict_high: false,
        rationale: "steady-state real interest rate is typically in [-5%,50%] (net) or [0.9,1.5] (gross); accept both.",
    },
];

pub fn check_w070(model: &Model) -> Vec<Diagnostic> {
    let mut known: HashMap<Name, f64> = HashMap::new();
    let mut latest: HashMap<Name, usize> = HashMap::new();
    let mut order: Vec<Name> = Vec::new();
    let mut folded: Vec<Option<f64>> = Vec::with_capacity(model.param_assignments.len());

    for (i, a) in model.param_assignments.iter().enumerate() {
        if !latest.contains_key(&a.name) {
            order.push(a.name);
        }
        latest.insert(a.name, i);
        let value = a
            .expr
            .and_then(|id| fold_expr(model, id, &known))
            .filter(|v| v.is_finite());
        match value {
            Some(v) => {
                known.insert(a.name, v);
            }
            None => {
                known.remove(&a.name);
            }
        }
        folded.push(value);
    }

    let mut diagnostics = Vec::new();
    for name in order {
        let i = latest[&name];
        let Some(value) = folded[i] else {
            continue;
        };
        let a = &model.param_assignments[i];
        let pname = model.name(a.name);
        let Some(bound) = lookup(pname) else {
            continue;
        };
        if is_in_bounds(value, bound) {
            continue;
        }
        let range = format_range(bound);
        diagnostics.push(Diagnostic::new(
            a.span,
            Severity::Warning,
            "W070",
            format!(
                "Parameter '{pname}' = {} is outside the conventional range {range}: {}. This is a warning, not an error \u{2014} override if intentional.",
                python_g(value),
                bound.rationale
            ),
        ));
    }
    diagnostics
}

fn lookup(name: &str) -> Option<&'static Bound> {
    if name.is_empty() {
        return None;
    }
    let lower = name.to_ascii_lowercase();
    BOUNDS.iter().find(|b| lower.contains(b.pattern))
}

fn is_in_bounds(value: f64, bound: &Bound) -> bool {
    if let Some(low) = bound.low {
        if bound.strict_low && value <= low {
            return false;
        }
        if !bound.strict_low && value < low {
            return false;
        }
    }
    if let Some(high) = bound.high {
        if bound.strict_high && value >= high {
            return false;
        }
        if !bound.strict_high && value > high {
            return false;
        }
    }
    true
}

fn format_range(bound: &Bound) -> String {
    let (left_bracket, left_val) = match bound.low {
        None => ("(-inf", String::new()),
        Some(low) => (if bound.strict_low { "(" } else { "[" }, python_g(low)),
    };
    let (right_bracket, right_val) = match bound.high {
        None => ("", "+inf)".to_string()),
        Some(high) => (if bound.strict_high { ")" } else { "]" }, python_g(high)),
    };
    format!("{left_bracket}{left_val},{right_val}{right_bracket}")
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

fn fold_expr(model: &Model, id: ExprId, known: &HashMap<Name, f64>) -> Option<f64> {
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
            known.get(name).copied()
        }
        ExprKind::Unary { op, arg } => {
            let v = fold_expr(model, *arg, known)?;
            Some(match op {
                UnOp::Pos => v,
                UnOp::Neg => -v,
            })
        }
        ExprKind::Binary { op, lhs, rhs } => {
            let l = fold_expr(model, *lhs, known)?;
            let r = fold_expr(model, *rhs, known)?;
            match op {
                BinOp::Add => Some(l + r),
                BinOp::Sub => Some(l - r),
                BinOp::Mul => Some(l * r),
                BinOp::Div => Some(l / r),
                BinOp::Pow => Some(l.powf(r)),
                BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::EqEq | BinOp::Ne => None,
            }
        }
        ExprKind::Call { .. }
        | ExprKind::String
        | ExprKind::Error
        | ExprKind::SteadyState { .. }
        | ExprKind::Expectation { .. } => None,
    }
}
