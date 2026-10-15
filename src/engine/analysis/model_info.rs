//! Structural model notes for hover and model counts. No `blocks`, no BK.

use std::collections::{HashMap, HashSet};

use crate::engine::analysis::equations::count_gap;
use crate::engine::analysis::equations::{identifier_class, IdentClass};
use crate::engine::analysis::model_locals::ModelLocals;
use crate::engine::language::intern::Name;
use crate::engine::language::model::Model;
use crate::engine::language::span::Span;
use serde_json::{json, Value};

use crate::engine::analysis::timing::classify_aggregate_variable_timing;
use crate::engine::analysis::timing::TimingAnalysis;
pub use crate::engine::analysis::timing::{
    classify_variable_timing, structure_summary, StructureSummary, TimingClass, TimingInfo,
};

/// Hover shows the timing class and the declared convention when applicable.
pub fn format_timing_line(info: &TimingInfo) -> String {
    if info.predetermined_convention {
        format!(
            "Timing: **{}**, end-of-period convention",
            info.class.label()
        )
    } else {
        format!("Timing: **{}**", info.class.label())
    }
}

/// Shared arithmetic operations. Strict mode uses retained numeric tokens and
/// requires finite intermediates; legacy mode retains the written-number reader.
pub(crate) fn arithmetic_number(
    model: &Model,
    id: crate::engine::language::expr::ExprId,
    known: &HashMap<Name, f64>,
    strict: bool,
) -> Option<f64> {
    use crate::engine::language::expr::{BinOp, ExprKind, UnOp};
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
        | ExprKind::PathNamespace { .. }
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
        if crate::engine::control::cancelled() {
            break;
        }
        match timing.get(name).map(|t| t.class) {
            Some(TimingClass::Mixed) => mixed.push(name.clone()),
            Some(TimingClass::ForwardLooking) => forward_looking.push(name.clone()),
            Some(TimingClass::Predetermined) => predetermined.push(name.clone()),
            _ => static_vars.push(name.clone()),
        }
    }
    let summary = model.summary();
    let model_locals = model_locals_json(model);
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
                if crate::engine::control::cancelled() {
                    break;
                }
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
        "model_locals": model_locals,
    })
}

/// Parsed model-local declarations and definitions shared by MCP and LSP.
/// Source origins are attached by the transport that owns the source map.
pub(crate) fn model_locals_json(model: &Model) -> Value {
    let facts = ModelLocals::collect(model);
    let declarations: Vec<Value> = facts
        .declarations
        .iter()
        .map(|declaration| {
            let mut row = json!({ "name": model.name(declaration.name) });
            if let Some(tex_name) = &declaration.tex_name {
                row["tex_name"] = json!(tex_name);
            }
            row
        })
        .collect();
    let timing = TimingAnalysis::new(model);
    let definitions: Vec<Value> = facts
        .definitions
        .iter()
        .map(|definition| {
            let written = &model.written_equations[definition.equation_index];
            let equation = &written.equation;
            let expression_text = equation.rhs.trim();
            let idents: Vec<Value> = model
                .exprs
                .walk_idents(definition.rhs_expr)
                .map(|reference| {
                    let is_local = facts.uses.iter().any(|usage| {
                        usage.equation_index == definition.equation_index
                            && usage.name == reference.name
                            && usage.span == reference.span
                    });
                    let class = identifier_class(model, reference.name, is_local);
                    let mut ident = json!({
                        "name": model.name(reference.name),
                        "timing": reference.timing,
                        "dynare_timing": if definition.dimension.is_some() {
                            reference.timing
                        } else {
                            timing.aggregate_dynare_offset(reference.name, reference.timing)
                        },
                        "class": class.as_str(),
                    });
                    if class == IdentClass::Endogenous {
                        let timing_classes = if definition.dimension.is_some() {
                            &timing.all
                        } else {
                            &timing.aggregate
                        };
                        if let Some(info) = timing_classes.get(model.name(reference.name)) {
                            ident["timing_class"] = json!(info.class.label());
                        }
                    }
                    ident
                })
                .collect();
            json!({
                "name": model.name(definition.name),
                "dimension": definition.dimension.map(|dimension| model.name(dimension)),
                "text": equation.text,
                "expression": expression_text,
                "idents": idents,
            })
        })
        .collect();
    json!({ "declarations": declarations, "definitions": definitions })
}

const MACRO_INCOMPLETE_MESSAGE: &str = "Macro expansion is incomplete";

/// Shared by MCP and the LSP commands so both transports report the same status.
pub(crate) fn macro_incomplete_status() -> Value {
    json!({"status": "incomplete", "message": MACRO_INCOMPLETE_MESSAGE})
}

pub(crate) fn incomplete_model_status(model: &Model, includes_complete: bool) -> Option<Value> {
    if !includes_complete {
        Some(model_incomplete_status())
    } else if model.macro_incomplete() {
        Some(macro_incomplete_status())
    } else if !crate::engine::workspace::model_map::parser_complete(model) {
        Some(model_incomplete_status())
    } else {
        None
    }
}

pub(crate) fn model_incomplete_status() -> Value {
    json!({"status": "incomplete", "message": "Model expansion is incomplete"})
}

/// Applied work limits, from engine receipts rather than diagnostic prose.
pub(crate) fn macro_work_limits(model: &Model, root: Option<&str>) -> Vec<Value> {
    let mut seen = HashSet::new();
    model
        .incomplete_reasons
        .iter()
        .filter_map(|reason| {
            let name = reason.limit_name?;
            if name == "request cancelled" || !seen.insert(name) {
                return None;
            }
            let ceiling = crate::engine::language::macro_expr::limit_ceiling(name);
            Some(
                json!({"code":"MACRO_LIMIT","limit":name,"ceiling":ceiling.map(|bound|bound.0),
            "unit":ceiling.map(|bound|bound.1),"root":root,
            "affected":"macro expansion and dependent model analysis",
            "omitted":"authoritative model counts and complete navigation"}),
            )
        })
        .collect()
}

/// Optional `incomplete_reasons` retain shared causes and diagnostic sentences.
pub(crate) fn model_incomplete_status_with_reasons(reasons: &[Value]) -> Value {
    let mut status = model_incomplete_status();
    if !reasons.is_empty() {
        status["incomplete_reasons"] = Value::Array(reasons.to_vec());
    }
    status
}

fn e061_reasons(
    records: &crate::engine::workspace::IncludeRecords,
) -> Vec<(Span, &'static str, String)> {
    crate::engine::diagnostics::check_e060::check_e061(records)
        .into_iter()
        .map(|diagnostic| (diagnostic.span, "E061", diagnostic.message))
        .collect()
}

/// Diagnostic incomplete causes plus captured unfinished Dynare statements.
pub fn incomplete_reason_records(
    model: &Model,
    includes: Option<&crate::engine::workspace::IncludeRecords>,
) -> Vec<(Span, &'static str, String)> {
    // Missing includes suppress possibly downstream unknown names and I211,
    // as in workspace diagnostics. Independent proven macro Errors remain.
    if let Some(records) = includes
        && !records.unresolved.is_empty()
    {
        let mut reasons = incomplete_reason_records(model, None);
        reasons.retain(|(_, code, _)| !matches!(*code, "E001" | "E063" | "I211"));
        reasons.extend(e061_reasons(records));
        return reasons;
    }
    // Match analyze_positions: official macro Errors before I211.
    let e062 = crate::engine::diagnostics::check_e060::check_e062(model);
    if !e062.is_empty() {
        return e062
            .into_iter()
            .map(|diagnostic| (diagnostic.span, "E062", diagnostic.message))
            .collect();
    }
    if !model.macro_type_errors.is_empty() {
        let mut seen = HashSet::new();
        return model
            .macro_type_errors
            .iter()
            .filter_map(|(span, code, message)| {
                seen.insert((*span, *code, message.clone())).then_some((
                    *span,
                    *code,
                    message.clone(),
                ))
            })
            .collect();
    }
    if !model.incomplete_reasons.is_empty() || model.macro_incomplete_span.is_some() {
        let e064 = crate::engine::diagnostics::check_e060::check_e064(model);
        if !e064.is_empty() {
            return e064
                .into_iter()
                .map(|diagnostic| (diagnostic.span, "E064", diagnostic.message))
                .collect();
        }
        if !model.incomplete_reasons.is_empty() {
            return model
                .incomplete_reasons
                .iter()
                .map(|reason| (reason.span, reason.code, reason.message.clone()))
                .collect();
        }
        if let Some(span) = model.macro_incomplete_span {
            return vec![(
                span,
                "I211",
                "Macro expansion is incomplete; some model checks were withheld.".to_string(),
            )];
        }
    }
    let mut reasons: Vec<_> = crate::engine::diagnostics::check_parse::check_parse(model)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "E001")
        .map(|diagnostic| (diagnostic.span, "E001", diagnostic.message))
        .collect();
    // Presentation completeness also observes accepted but unfinished command
    // and block records. These causes are not new language diagnostics.
    reasons.extend(
        model
            .statements
            .iter()
            .filter(|row| !row.native && !row.complete)
            .map(|row| {
                let block = row.kind == crate::engine::language::model::StatementKind::Block;
                (
                    row.keyword_span,
                    if block {
                        "INCOMPLETE_BLOCK"
                    } else {
                        "INCOMPLETE_STATEMENT"
                    },
                    format!(
                        "{} '{}' was not completely parsed.",
                        if block { "Block" } else { "Statement" },
                        row.name
                    ),
                )
            }),
    );
    reasons
}

/// An incomplete cause with verified written locations. Both transports use
/// this conversion; no caller derives a resource name from diagnostic prose.
pub(crate) struct IncompleteCause {
    pub code: &'static str,
    pub message: String,
    pub origin: Option<crate::engine::workspace::IncludeSite>,
    pub include_chain: Vec<crate::engine::workspace::IncludeSite>,
    pub limit: Option<Value>,
}

impl IncompleteCause {
    pub(crate) fn outcome_json(&self) -> Value {
        let mut result = json!({"code":self.code,"message":self.message,
            "available":"verified emitted prefix and already established facts",
            "omitted":"authoritative model counts, complete source navigation and edits that need the complete model"});
        if let Some(limit) = &self.limit {
            result["limit"] = limit.clone();
        }
        result
    }
}

pub(crate) fn incomplete_causes(
    workspace: &mut crate::engine::workspace::Workspace,
    root: &str,
    model: &Model,
) -> Vec<IncompleteCause> {
    let includes = workspace.include_records(root).cloned();
    let limits = macro_work_limits(model, Some(root));
    incomplete_reason_records(model, includes.as_ref())
        .into_iter()
        .map(|(span, code, message)| {
            let (origin, include_chain) = workspace.incomplete_cause_origin(root, span, code);
            let limit = model
                .incomplete_reasons
                .iter()
                .find(|reason| {
                    reason.span == span && reason.code == code && reason.message == message
                })
                .and_then(|reason| reason.limit_name)
                .and_then(|name| limits.iter().find(|limit| limit["limit"] == name))
                .cloned();
            IncompleteCause {
                code,
                message,
                origin,
                include_chain,
                limit,
            }
        })
        .collect()
}

/// The same include/companion rows, with each transport retaining its path keys.
pub(crate) fn related_files_json(
    includes: &crate::engine::workspace::IncludeRecords,
    companions: &[crate::engine::workspace::companion::CompanionRecord],
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
