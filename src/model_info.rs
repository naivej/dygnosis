//! Structural model notes for hover and model counts. No `blocks`, no BK.

use std::collections::{HashMap, HashSet};

use crate::equations::count_gap;
use crate::intern::Name;
use crate::model::Model;
use serde_json::{json, Value};

use crate::timing::classify_aggregate_variable_timing;
pub use crate::timing::{
    classify_variable_timing, structure_summary, StructureSummary, TimingClass, TimingInfo,
};

/// Hover line names the offset convention when the written uses were shifted.
pub fn format_timing_line(info: &TimingInfo) -> String {
    let offsets = info
        .offsets
        .iter()
        .map(|&o| format_time_offset(o))
        .collect::<Vec<_>>()
        .join(", ");
    if info.predetermined_conversion {
        format!(
            "Timing: **{}** · Dynare end-of-period offsets: {} (one period earlier than written offsets)",
            info.class.label(), offsets
        )
    } else {
        format!(
            "Timing: **{}** · written offsets: {}",
            info.class.label(),
            offsets
        )
    }
}

/// One-line form of [`StructureSummary`]. No Compute Steady State or solver names.
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

/// Shared arithmetic operations. Strict mode uses retained numeric tokens and
/// requires finite intermediates; legacy mode retains the written-number reader.
pub(crate) fn arithmetic_number(
    model: &Model,
    id: crate::expr::ExprId,
    known: &HashMap<Name, f64>,
    strict: bool,
) -> Option<f64> {
    use crate::expr::{BinOp, ExprKind, UnOp};
    let value = match &model.exprs.get(id).kind {
        ExprKind::Number => {
            if strict {
                return model
                    .numeric_literals
                    .get(&id)
                    .copied()
                    .filter(|value| value.is_finite());
            }
            let span = model.exprs.get(id).span;
            let raw = model.source.get(span.start as usize..span.end as usize)?;
            raw.parse().ok()
        }
        ExprKind::Ident {
            name,
            timing,
            timing_span,
            ..
        } => {
            if *timing != 0 || (strict && timing_span.is_some()) {
                return None;
            }
            known.get(name).copied()
        }
        ExprKind::Unary { op, arg } => {
            let v = arithmetic_number(model, *arg, known, strict)?;
            Some(match op {
                UnOp::Pos => v,
                UnOp::Neg => -v,
            })
        }
        ExprKind::Binary { op, lhs, rhs } => {
            let l = arithmetic_number(model, *lhs, known, strict)?;
            let r = arithmetic_number(model, *rhs, known, strict)?;
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
    };
    value.filter(|value| !strict || value.is_finite())
}

/// Legacy final-value walk and numeric source reader remain unchanged.
pub fn assigned_number(model: &Model, name: &str) -> Option<f64> {
    let mut known = HashMap::new();
    for a in model
        .param_assignments
        .iter()
        .chain(model.helper_assignments.iter())
    {
        let value = a
            .expr
            .and_then(|id| arithmetic_number(model, id, &known, false));
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

/// Shared count fields for MCP and LSP. Locations are added by each transport.
pub fn model_info_json(model: &Model) -> Value {
    let endogenous: Vec<String> = model
        .final_endogenous()
        .into_iter()
        .map(|d| model.name(d.name).to_string())
        .collect();
    let exogenous: Vec<String> = model
        .final_decls(&["varexo", "varexo_det"])
        .into_iter()
        .filter(|decl| model.final_heterogeneity(decl).is_none())
        .map(|d| model.name(d.name).to_string())
        .collect();
    let parameters: Vec<String> = model
        .final_parameters()
        .into_iter()
        .filter(|decl| model.final_heterogeneity(decl).is_none())
        .map(|d| model.name(d.name).to_string())
        .collect();
    let timing = classify_aggregate_variable_timing(model);
    let mut static_vars = Vec::new();
    let mut predetermined = Vec::new();
    let mut forward_looking = Vec::new();
    let mut mixed = Vec::new();
    for name in &endogenous {
        match timing.get(name).map(|t| t.class) {
            Some(TimingClass::Mixed) => mixed.push(name.clone()),
            Some(TimingClass::ForwardLooking) => forward_looking.push(name.clone()),
            Some(TimingClass::Predetermined) => predetermined.push(name.clone()),
            _ => static_vars.push(name.clone()),
        }
    }
    let summary = model.summary();
    let heterogeneous_timing = classify_variable_timing(model);
    let heterogeneous_dimensions: Vec<Value> = heterogeneous_dimension_names(model)
        .into_iter()
        .map(|dimension| {
            let names: Vec<String> = model
                .final_decls(&["var"])
                .into_iter()
                .filter(|decl| model.final_heterogeneity(decl) == Some(dimension))
                .map(|decl| model.name(decl.name).to_string())
                .collect();
            let shocks: Vec<String> = model
                .final_decls(&["varexo", "varexo_det"])
                .into_iter()
                .filter(|decl| model.final_heterogeneity(decl) == Some(dimension))
                .map(|decl| model.name(decl.name).to_string())
                .collect();
            let params: Vec<String> = model
                .final_parameters()
                .into_iter()
                .filter(|decl| model.final_heterogeneity(decl) == Some(dimension))
                .map(|decl| model.name(decl.name).to_string())
                .collect();
            let mut static_vars = Vec::new();
            let mut predetermined = Vec::new();
            let mut forward_looking = Vec::new();
            let mut mixed = Vec::new();
            for name in &names {
                match heterogeneous_timing.get(name).map(|info| info.class) {
                    Some(TimingClass::Mixed) => mixed.push(name.clone()),
                    Some(TimingClass::ForwardLooking) => forward_looking.push(name.clone()),
                    Some(TimingClass::Predetermined) => predetermined.push(name.clone()),
                    _ => static_vars.push(name.clone()),
                }
            }
            let n_equations = model
                .heterogeneous_models
                .iter()
                .filter(|block| block.dimension == dimension)
                .flat_map(|block| block.equations.iter())
                .filter(|eq| !eq.is_local && !eq.static_tag)
                .count();
            json!({
                "dimension": model.name(dimension),
                "n_endogenous": names.len(),
                "endogenous": names,
                "n_exogenous": shocks.len(),
                "exogenous": shocks,
                "n_parameters": params.len(),
                "parameters": params,
                "n_equations": n_equations,
                "static": static_vars,
                "predetermined": predetermined,
                "forward_looking": forward_looking,
                "mixed": mixed,
            })
        })
        .collect();
    json!({
        "n_endogenous": endogenous.len(),
        "endogenous": endogenous,
        "n_exogenous": exogenous.len(),
        "exogenous": exogenous,
        "n_parameters": parameters.len(),
        "parameters": parameters,
        "n_equations": count_gap(model).n_equations,
        "static": static_vars,
        "predetermined": predetermined,
        "forward_looking": forward_looking,
        "mixed": mixed,
        "n_static": static_vars.len(),
        "n_predetermined": predetermined.len(),
        "n_forward_looking": forward_looking.len(),
        "n_mixed": mixed.len(),
        "n_state_variables": predetermined.len() + mixed.len(),
        "n_jumpers": forward_looking.len() + mixed.len(),
        "n_model_equations": summary.n_model_equations,
        "n_steady_state_equations": summary.n_steady_state_equations,
        "n_initval_entries": summary.n_initval_entries,
        "is_linear": summary.is_linear,
        "has_model_block": summary.has_model_block,
        "has_steady_state_model_block": summary.has_steady_state_model_block,
        "has_initval_block": summary.has_initval_block,
        "has_shocks_block": summary.has_shocks_block,
        "heterogeneity_dimensions": heterogeneous_dimensions,
    })
}

pub(crate) fn model_incomplete_status() -> Value {
    json!({"status": "incomplete", "message": "Model expansion is incomplete"})
}

/// The same include/companion rows, with each transport retaining its path keys.
pub(crate) fn related_files_json(
    includes: &crate::workspace::IncludeRecords,
    companions: &[crate::companion::CompanionRecord],
    path: impl Fn(&std::path::Path) -> String,
) -> Value {
    let row = |kind: &str, filename: &str, resolved: Option<&std::path::Path>| {
        let mut result =
            json!({ "kind": kind, "filename": filename, "resolved": resolved.is_some() });
        if let Some(resolved) = resolved {
            result["path"] = json!(path(resolved));
        }
        result
    };
    let rows = includes
        .resolved
        .iter()
        .map(|include| row("include", &include.filename, Some(&include.path)))
        .chain(
            includes
                .unresolved
                .iter()
                .map(|include| row("include", &include.filename, None)),
        )
        .chain(
            companions
                .iter()
                .map(|record| row(record.kind.as_str(), &record.name, record.path.as_deref())),
        )
        .collect();
    Value::Array(rows)
}

pub(crate) fn heterogeneous_dimension_names(model: &Model) -> Vec<Name> {
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    for name in model
        .heterogeneity_dimensions
        .iter()
        .map(|dimension| dimension.name)
        .chain(
            model
                .endogenous
                .iter()
                .chain(model.exogenous.iter())
                .chain(model.parameters.iter())
                .filter_map(|decl| decl.heterogeneity.map(|(name, _)| name)),
        )
        .chain(
            model
                .heterogeneous_models
                .iter()
                .map(|block| block.dimension),
        )
    {
        if seen.insert(name) {
            names.push(name);
        }
    }
    names
}
