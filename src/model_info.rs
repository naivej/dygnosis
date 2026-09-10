//! Structural model notes for hover and code lens. No `blocks`, no BK.

use std::collections::{HashMap, HashSet};

use crate::model::Model;

/// Timing class from lead/lag offsets in model equations. No Blanchard-Kahn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimingClass {
    Static,
    Predetermined,
    ForwardLooking,
    Mixed,
}

impl TimingClass {
    pub fn label(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::Predetermined => "predetermined",
            Self::ForwardLooking => "forward-looking",
            Self::Mixed => "mixed",
        }
    }
}

/// Per-endogenous timing from walking equation `ident_refs`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimingInfo {
    pub class: TimingClass,
    pub offsets: Vec<i32>,
}

/// Classify each endogenous variable by dynamic timing in `model.equations`.
pub fn classify_variable_timing(model: &Model) -> HashMap<String, TimingInfo> {
    let mut offsets: HashMap<String, HashSet<i32>> = HashMap::new();
    for eq in &model.equations {
        for r in model.ident_refs(eq) {
            let name = model.name(r.name).to_string();
            offsets.entry(name).or_default().insert(r.timing);
        }
    }
    let mut out = HashMap::new();
    for decl in &model.endogenous {
        let name = model.name(decl.name).to_string();
        let mut offs: Vec<i32> = offsets
            .get(&name)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default();
        offs.sort_unstable();
        let has_lead = offs.iter().any(|&o| o > 0);
        let has_lag = offs.iter().any(|&o| o < 0);
        let class = if has_lead && has_lag {
            TimingClass::Mixed
        } else if has_lead {
            TimingClass::ForwardLooking
        } else if has_lag {
            TimingClass::Predetermined
        } else {
            TimingClass::Static
        };
        if offs.is_empty() {
            offs.push(0);
        }
        out.insert(
            name,
            TimingInfo {
                class,
                offsets: offs,
            },
        );
    }
    out
}

/// Hover line: `Timing: **{label}** · appears at {offsets}`.
pub fn format_timing_line(info: &TimingInfo) -> String {
    let appears = info
        .offsets
        .iter()
        .map(|&o| format_time_offset(o))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Timing: **{}** · appears at {}",
        info.class.label(),
        appears
    )
}

/// Counts for the informational model-block code lens. No BK; no Python labels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructureSummary {
    pub endogenous: usize,
    pub predetermined: usize,
    pub forward_looking: usize,
    pub static_vars: usize,
    pub varexo: usize,
    pub max_lead: i32,
    pub max_lag: i32,
}

/// Predetermined / forward-looking / static counts from [`classify_variable_timing`].
pub fn structure_summary(model: &Model) -> StructureSummary {
    let timing = classify_variable_timing(model);
    let mut predetermined = 0;
    let mut forward_looking = 0;
    let mut static_vars = 0;
    let mut max_lead = 0;
    let mut max_lag = 0;
    for info in timing.values() {
        match info.class {
            TimingClass::Static => static_vars += 1,
            TimingClass::Predetermined => predetermined += 1,
            TimingClass::ForwardLooking => forward_looking += 1,
            TimingClass::Mixed => {
                predetermined += 1;
                forward_looking += 1;
            }
        }
        for &o in &info.offsets {
            if o > max_lead {
                max_lead = o;
            }
            if o < max_lag {
                max_lag = o;
            }
        }
    }
    StructureSummary {
        endogenous: model.endogenous.len(),
        predetermined,
        forward_looking,
        static_vars,
        varexo: model.exogenous.len(),
        max_lead,
        max_lag,
    }
}

/// Informational lens title. No Compute Steady State / solver names.
pub fn format_structure_lens(summary: &StructureSummary) -> String {
    format!(
        "{} endogenous: {} predetermined, {} forward-looking, {} static · {} varexo · max lead {}, max lag {}",
        summary.endogenous,
        summary.predetermined,
        summary.forward_looking,
        summary.static_vars,
        summary.varexo,
        summary.max_lead,
        summary.max_lag
    )
}

fn format_time_offset(offset: i32) -> String {
    if offset == 0 {
        "t".into()
    } else {
        format!("t{offset:+}")
    }
}

/// Fold a numeric assignment for `name` from parameter / helper assignments.
pub fn assigned_number(model: &Model, name: &str) -> Option<f64> {
    use crate::expr::{BinOp, ExprKind, UnOp};
    use crate::intern::Name;

    fn fold(model: &Model, id: crate::expr::ExprId, known: &HashMap<Name, f64>) -> Option<f64> {
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
                let v = fold(model, *arg, known)?;
                Some(match op {
                    UnOp::Pos => v,
                    UnOp::Neg => -v,
                })
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let l = fold(model, *lhs, known)?;
                let r = fold(model, *rhs, known)?;
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

    let mut known = HashMap::new();
    for a in model
        .param_assignments
        .iter()
        .chain(model.helper_assignments.iter())
    {
        let value = a.expr.and_then(|id| fold(model, id, &known));
        match value {
            Some(v) if v.is_finite() => {
                known.insert(a.name, v);
            }
            _ => {
                known.remove(&a.name);
            }
        }
    }
    known.iter().find_map(|(n, v)| {
        if model.name(*n) == name {
            Some(*v)
        } else {
            None
        }
    })
}
