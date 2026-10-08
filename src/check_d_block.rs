//! D-block parse-time Errors on trees we already have.

use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, RelatedDiagnostic, Severity};
use crate::expr::{ExprId, ExprKind};
use crate::intern::Name;
use crate::model::{Model, SymbolContext};
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
    out.extend(check_outside_assignment_roles(model));
    out.extend(check_model_expression_roles(model));
    out.extend(check_ss_rhs_roles(model));
    let mut existing = HashMap::new();
    for row in &out {
        *existing
            .entry((row.span, row.code.clone(), row.message.clone()))
            .or_insert(0usize) += 1;
    }
    let mut captured = HashMap::new();
    for &(name, span, context) in &model.outside_expression_uses {
        if let Some(row) = outside_expression_role_refusal(model, name, span, context) {
            // Retain executed-copy counts where legacy role readers collapse
            // equal written spans. Other open/removed/heterogeneous readers
            // keep their existing ownership.
            if !matches!(row.code.as_str(), "E279" | "E282" | "E294") {
                continue;
            }
            let key = (row.span, row.code.clone(), row.message.clone());
            let count = captured.entry(key.clone()).or_insert(0usize);
            *count += 1;
            // Overlapping readers describe the same executions. Retain the
            // larger count; equal written spans can be independent macro copies.
            if *count > existing.get(&key).copied().unwrap_or(0) {
                out.push(row);
            }
        }
    }
    out
}

fn check_histval_lag_dup(model: &Model) -> Vec<Diagnostic> {
    let mut seen: HashMap<(Name, i32), Span> = HashMap::new();
    let mut out = Vec::new();
    for (index, entry) in model.histval.iter().enumerate() {
        if model.histval_block_starts.binary_search(&index).is_ok() {
            seen.clear();
        }
        let name = model.name(entry.name);
        if entry.lag > 0 {
            out.push(err(
                entry.span,
                "E242",
                format!("histval: the lag on {name} should be less than or equal to 0"),
            ));
        }
        if let Some(&first) = seen.get(&(entry.name, entry.lag)) {
            out.push(
                err(
                    entry.span,
                    "E243",
                    format!("histval: {name}({}) declared twice", entry.lag),
                )
                .with_related(RelatedDiagnostic::new(first, "Earlier history entry")),
            );
        } else {
            seen.insert((entry.name, entry.lag), entry.span);
        }
    }
    out
}

fn check_planner_lead_local(model: &Model) -> Vec<Diagnostic> {
    if model.planner_objective_expr.is_none() {
        return Vec::new();
    }
    let span = model
        .planner_objective_span
        .unwrap_or(Span { start: 0, end: 1 });
    let mut out = Vec::new();
    for r in model
        .model_expression_uses
        .iter()
        .filter(|use_| use_.command == "planner_objective")
    {
        let kind = model.symbol_kind_in_context(r.name, r.context);
        if matches!(
            kind,
            Some("external_function" | "mod_file_local" | "excluded")
        ) || kind == Some("varexo_det") && r.timing != 0
        {
            continue;
        }
        if r.timing != 0 {
            out.push(err(
                span,
                "E252",
                "Leads and lags on variables are forbidden in 'planner_objective'.",
            ));
            break;
        }
        if kind == Some("model_local_variable") {
            let name = model.name(r.name);
            out.push(err(
                span,
                "E253",
                format!("Model local variable {name} cannot be used in 'planner_objective'."),
            ));
            break;
        }
    }
    out
}

fn check_osr_bounds_type(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for bound in &model.osr_params_bounds {
        if model.final_kind_or_written_if_excluded(bound.name) == Some("parameters") {
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
        .map(|(name, span)| {
            let shock_option = model
                .shock_paths
                .iter()
                .any(|block| block.span.start <= span.start && span.end <= block.span.end)
                || model.shock_blocks.iter().any(|block| {
                    block.kind == crate::model::ShockBlockKind::Multiplicative
                        && block.span.start <= span.start
                        && span.end <= block.span.end
                });
            let message = if shock_option {
                format!("The '{name}' option is declared multiple times")
            } else {
                format!("option {name} declared twice")
            };
            err(*span, "E271", message)
        })
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
    for (index, el) in model.generate_irfs.iter().enumerate() {
        if model
            .generate_irfs_block_starts
            .binary_search(&index)
            .is_ok()
        {
            seen.clear();
        }
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

fn check_outside_assignment_roles(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for a in model
        .param_assignments
        .iter()
        .chain(&model.helper_assignments)
        .filter(|assignment| !assignment.native)
    {
        if let Some(id) = a.expr {
            for usage in model.exprs.walk_idents(id) {
                if let Some(diagnostic) = outside_expression_role_refusal(
                    model,
                    usage.name,
                    usage.span,
                    a.symbol_type_context,
                ) {
                    if seen.insert((usage.name, diagnostic.code.clone())) {
                        out.push(diagnostic);
                    }
                }
            }
        }
    }
    out
}

/// ParsingDriver::add_model_variable checks these roles before the consumer's
/// own restrictions. Read the captured symbol table, including macro order.
fn check_model_expression_roles(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for usage in &model.model_expression_uses {
        let name = model.name(usage.name);
        let kind = model.symbol_kind_in_context(usage.name, usage.context);
        let refusal = match kind {
            Some("external_function") => Some(("E280", crate::model::external_function_in_model_message(name))),
            Some("mod_file_local") => Some(("E281", crate::model::mod_file_local_in_model_message(name))),
            Some("epilogue") if usage.command != "epilogue" => Some(("E294", format!("Symbol '{name}' cannot be used outside the epilogue block."))),
            None if usage.command == "occbin_constraints" => Some(("E182", format!("Exogenous variable {name} cannot be used in 'occbin_constraints'."))),
            None if matches!(usage.command, "model_replace" | "planner_objective" | "trend_var" | "log_trend_var" | "var")
                && !model.constructor_refused_statements.contains(&usage.statement_id) => Some(("E020", format!("Undeclared identifier '{name}' in equation. Fix: add '{name}' to a var, varexo, or parameters declaration."))),
            _ => None,
        };
        if let Some((code, message)) = refusal {
            if seen.insert((usage.name, usage.span, code)) {
                out.push(err(usage.span, code, message));
            }
        }
    }
    out
}

fn check_ss_rhs_roles(model: &Model) -> Vec<Diagnostic> {
    model
        .steady_state_rhs_uses
        .iter()
        .filter_map(|&(name, span, context)| {
            outside_expression_role_refusal(model, name, span, context)
        })
        .collect()
}

/// ParsingDriver::add_variable checks the RHS role before init_param checks
/// the target. Use the symbol history captured at this expression's read.
pub(crate) fn outside_expression_role_refusal(
    model: &Model,
    name: Name,
    span: Span,
    context: SymbolContext,
) -> Option<Diagnostic> {
    if model.outside_expression_symbol_is_valid(name, context) {
        return None;
    }
    let spelling = model.name(name);
    let (code, message) = if model.heterogeneous_in_context(name, context) {
        ("E463", format!("Symbol '{spelling}' cannot be used outside model declaration, because it is heterogeneous."))
    } else {
        match model.symbol_kind_in_context(name, context)? {
        "model_local_variable" => ("E282", format!("Variable {spelling} not allowed outside model declaration. Its scope is only inside model.")),
        "external_function" => ("E279", format!("Symbol '{spelling}' is the name of a MATLAB/Octave function, and cannot be used as a variable.")),
        "epilogue" => ("E294", format!("Symbol '{spelling}' cannot be used outside the epilogue block.")),
        "trend_var" | "log_trend_var" => ("E310", format!("Variable {spelling} not allowed outside model declaration, because it is a trend variable.")),
        "excluded" => ("E426", format!("Variable '{spelling}' can no longer be used since it has been excluded by a previous 'model_remove' or 'var_remove' statement")),
        _ => return None,
    }
    };
    Some(err(span, code, message))
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
            ExprKind::Number
            | ExprKind::String
            | ExprKind::Error
            | ExprKind::PathNamespace { .. } => false,
        }
    }
    walk(model, id)
}

fn err(span: Span, code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(span, Severity::Error, code, message)
}
