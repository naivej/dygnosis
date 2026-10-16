//! D-block parse-time Errors on trees we already have.

use std::collections::{HashMap, HashSet};

use super::parse_preflight::callback_order;
use crate::engine::diagnostics::{Diagnostic, RelatedDiagnostic, Severity};
use crate::engine::language::expr::{ExprId, ExprKind};
use crate::engine::language::intern::Name;
use crate::engine::language::model::{Model, SymbolContext};
use crate::engine::language::span::Span;

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
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        *existing
            .entry((row.span, row.code.clone(), row.message.clone()))
            .or_insert(0usize) += 1;
    }
    let mut captured = HashMap::new();
    for &(name, span, context) in &model.outside_expression_uses {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
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

/// Parse-only receipts over the same predicates used by diagnostics. Check-only
/// OSR bounds are excluded; epilogue role checks here are constructor actions.
pub(crate) fn parse_refusals(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut out = check_histval_lag_dup_with_orders(model);
    out.extend(check_planner_lead_local_with_orders(model));
    out.extend(check_varexobs_with_orders(model));
    out.extend(check_static_tag_with_orders(model));
    out.extend(check_generate_irfs_with_orders(model));
    out.extend(model_expression_refusals(model));
    out.extend(
        check_option_twice(model)
            .into_iter()
            .enumerate()
            .map(|(index, diagnostic)| {
                (
                    diagnostic,
                    callback_order(model, "option_duplicate_callback", index),
                )
            }),
    );
    out.extend(
        check_namespace(model)
            .into_iter()
            .enumerate()
            .map(|(index, diagnostic)| {
                (
                    diagnostic,
                    callback_order(model, "namespace_callback", index),
                )
            }),
    );
    out.extend(
        check_const_fold(model)
            .into_iter()
            .enumerate()
            .map(|(index, diagnostic)| {
                (
                    diagnostic,
                    callback_order(model, "constant_fold_callback", index),
                )
            }),
    );
    for receipt in &model.outside_expression_callbacks {
        if crate::engine::control::cancelled() {
            return Vec::new();
        }
        if let Some(diagnostic) =
            outside_expression_role_refusal(model, receipt.name, receipt.span, receipt.context)
        {
            out.push((diagnostic, receipt.order));
        }
    }
    out
}

fn check_histval_lag_dup(model: &Model) -> Vec<Diagnostic> {
    check_histval_lag_dup_with_orders(model)
        .into_iter()
        .map(|(diagnostic, _)| diagnostic)
        .collect()
}

fn check_histval_lag_dup_with_orders(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut seen: HashMap<(Name, i32), Span> = HashMap::new();
    let mut out = Vec::new();
    for (index, entry) in model.histval.iter().enumerate() {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if model.histval_block_starts.binary_search(&index).is_ok() {
            seen.clear();
        }
        let name = model.name(entry.name);
        if entry.lag > 0 {
            out.push((
                err(
                    entry.span,
                    "E242",
                    format!("histval: the lag on {name} should be less than or equal to 0"),
                ),
                entry.active_tokens.end,
            ));
        }
        if let Some(&first) = seen.get(&(entry.name, entry.lag)) {
            out.push((
                err(
                    entry.span,
                    "E243",
                    format!("histval: {name}({}) declared twice", entry.lag),
                )
                .with_related(RelatedDiagnostic::new(first, "Earlier history entry")),
                entry.active_tokens.end,
            ));
        } else {
            seen.insert((entry.name, entry.lag), entry.span);
        }
    }
    out
}

fn check_planner_lead_local(model: &Model) -> Vec<Diagnostic> {
    check_planner_lead_local_with_orders(model)
        .into_iter()
        .map(|(diagnostic, _)| diagnostic)
        .collect()
}

fn check_planner_lead_local_with_orders(model: &Model) -> Vec<(Diagnostic, usize)> {
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
            out.push((
                err(
                    span,
                    "E252",
                    "Leads and lags on variables are forbidden in 'planner_objective'.",
                ),
                r.parse_order,
            ));
            break;
        }
        if kind == Some("model_local_variable") {
            let name = model.name(r.name);
            out.push((
                err(
                    span,
                    "E253",
                    format!("Model local variable {name} cannot be used in 'planner_objective'."),
                ),
                r.parse_order,
            ));
            break;
        }
    }
    out
}

fn check_osr_bounds_type(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for bound in &model.osr_params_bounds {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
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
    check_varexobs_with_orders(model)
        .into_iter()
        .map(|(diagnostic, _)| diagnostic)
        .collect()
}

fn check_varexobs_with_orders(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut out = Vec::new();
    if model.varexobs_statement_count >= 2 {
        let span = model
            .varexobs_second_span
            .or(model.varexobs_span)
            .unwrap_or(Span { start: 0, end: 1 });
        out.push((
            err(
                span,
                "E259",
                "varexobs: you cannot have several 'varexobs' statements in the same MOD file",
            ),
            callback_order(model, "varexobs_header_callback", 1),
        ));
    }
    for (index, v) in model.varexobs.iter().enumerate() {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if model.symbol_kind_in_context(v.name, v.symbol_type_context) == Some("varexo")
            && !model.heterogeneous_in_context(v.name, v.symbol_type_context)
        {
            continue;
        }
        let name = model.name(v.name);
        out.push((
            err(
                v.span,
                "E260",
                format!("varexobs: {name} is not an exogenous variable"),
            ),
            callback_order(model, "varexobs", index) + 1,
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
                    block.kind == crate::engine::language::model::ShockBlockKind::Multiplicative
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
    check_static_tag_with_orders(model)
        .into_iter()
        .map(|(diagnostic, _)| diagnostic)
        .collect()
}

fn check_static_tag_with_orders(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut out = Vec::new();
    for eq in &model.equations {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if !eq.static_tag {
            continue;
        }
        if eq.lhs_expr.is_some_and(|id| expr_has_dynamics(model, id))
            || eq.rhs_expr.is_some_and(|id| expr_has_dynamics(model, id))
        {
            out.push((err(
                eq.span,
                "E272",
                "An equation tagged [static] cannot contain leads, lags, expectations, diff or STEADY_STATE operators",
            ), eq.active_tokens.end));
        }
    }
    out
}

fn check_generate_irfs(model: &Model) -> Vec<Diagnostic> {
    check_generate_irfs_with_orders(model)
        .into_iter()
        .map(|(diagnostic, _)| diagnostic)
        .collect()
}

fn check_generate_irfs_with_orders(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut out = Vec::new();
    let mut seen: HashSet<Name> = HashSet::new();
    let mut exo_index = 0;
    for (index, el) in model.generate_irfs.iter().enumerate() {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if model
            .generate_irfs_block_starts
            .binary_search(&index)
            .is_ok()
        {
            seen.clear();
        }
        let name = model.name(el.name);
        if !seen.insert(el.name) {
            out.push((err(
                el.span,
                "E273",
                format!(
                    "Names in the generate_irfs block must be unique but you entered '{name}' more than once."
                ),
            ), callback_order(model, "generate_irfs_callback", index)));
        }
        let mut exo_seen: HashSet<Name> = HashSet::new();
        for (exo, span) in &el.exos {
            let exo_order = callback_order(model, "generate_irfs_exog_callback", exo_index);
            exo_index += 1;
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            if !exo_seen.insert(*exo) {
                out.push((
                    err(
                        *span,
                        "E274",
                        format!(
                            "You have set the exogenous variable {} twice.",
                            model.name(*exo)
                        ),
                    ),
                    exo_order,
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
                if crate::engine::control::cancelled() {
                    return Default::default();
                }
                if let Some(diagnostic) = outside_expression_role_refusal(
                    model,
                    usage.name,
                    usage.span,
                    a.symbol_type_context,
                ) && seen.insert((usage.name, diagnostic.code.clone()))
                {
                    out.push(diagnostic);
                }
            }
        }
    }
    out
}

/// ParsingDriver::add_model_variable checks these roles before the consumer's
/// own restrictions. Read the captured symbol table, including macro order.
fn check_model_expression_roles(model: &Model) -> Vec<Diagnostic> {
    model_expression_refusals(model)
        .into_iter()
        .map(|(diagnostic, _)| diagnostic)
        .collect()
}

pub(crate) fn model_expression_refusals(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for usage in &model.model_expression_uses {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let name = model.name(usage.name);
        let kind = model.symbol_kind_in_context(usage.name, usage.context);
        let refusal = match kind {
            Some("external_function") => Some((
                "E280",
                crate::engine::language::model::external_function_in_model_message(name),
            )),
            Some("mod_file_local") => Some((
                "E281",
                crate::engine::language::model::mod_file_local_in_model_message(name),
            )),
            Some("epilogue") if usage.command != "epilogue" => Some((
                "E294",
                format!("Symbol '{name}' cannot be used outside the epilogue block."),
            )),
            None if usage.command == "occbin_constraints" => Some((
                "E182",
                format!("Exogenous variable {name} cannot be used in 'occbin_constraints'."),
            )),
            None if matches!(
                usage.command,
                "model_replace" | "planner_objective" | "trend_var" | "log_trend_var" | "var"
            ) && !model
                .constructor_refused_statements
                .contains(&usage.statement_id) =>
            {
                Some((
                    "E020",
                    format!(
                        "Undeclared identifier '{name}' in equation. Fix: add '{name}' to a var, varexo, or parameters declaration."
                    ),
                ))
            }
            _ => None,
        };
        if let Some((code, message)) = refusal
            && seen.insert((usage.name, usage.span, code))
        {
            out.push((err(usage.span, code, message), usage.parse_order));
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
        (
            "E463",
            format!(
                "Symbol '{spelling}' cannot be used outside model declaration, because it is heterogeneous."
            ),
        )
    } else {
        match model.symbol_kind_in_context(name, context)? {
            "model_local_variable" => (
                "E282",
                format!(
                    "Variable {spelling} not allowed outside model declaration. Its scope is only inside model."
                ),
            ),
            "external_function" => (
                "E279",
                format!(
                    "Symbol '{spelling}' is the name of a MATLAB/Octave function, and cannot be used as a variable."
                ),
            ),
            "epilogue" => (
                "E294",
                format!("Symbol '{spelling}' cannot be used outside the epilogue block."),
            ),
            "trend_var" | "log_trend_var" => (
                "E310",
                format!(
                    "Variable {spelling} not allowed outside model declaration, because it is a trend variable."
                ),
            ),
            "excluded" => (
                "E426",
                format!(
                    "Variable '{spelling}' can no longer be used since it has been excluded by a previous 'model_remove' or 'var_remove' statement"
                ),
            ),
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

#[cfg(test)]
mod parse_receipt_tests {
    use super::*;

    #[test]
    fn ordinary_constructor_receipts_keep_identity_after_row_rollback() {
        // Guards a later refusal being joined to a discarded accepted-use row.
        let model = crate::parse(
            "parameters p;trend_var(growth_factor=1) A;var y;observation_trends;y(p) garbage;y(A);end;database after;",
        );
        let (diagnostic, order) = parse_refusals(&model)
            .into_iter()
            .find(|(row, _)| row.code == "E310")
            .unwrap();
        let receipt = model
            .outside_expression_callbacks
            .iter()
            .find(|receipt| model.name(receipt.name) == "A")
            .unwrap();
        assert_eq!(diagnostic.span, receipt.span);
        assert_eq!(order, receipt.order);
        assert_eq!(model.expanded_tokens[order - 1].text(&model.source), "A");

        // A genuine constructor refusal survives a later malformed row tail.
        let model =
            crate::parse("trend_var(growth_factor=1) A;var y;observation_trends;y(A) garbage;end;");
        assert!(model.outside_expression_uses.is_empty());
        assert!(
            parse_refusals(&model)
                .iter()
                .any(|(row, _)| row.code == "E310")
        );
    }

    #[test]
    fn parse_receipts_exclude_osr_check_and_keep_history_callback_order() {
        // Guards OSR's later parameter-type Check from becoming a Parse stop.
        let model = crate::parse("var y;osr_params_bounds;y,0,1;end;database later;");
        assert!(check_d_block(&model).iter().any(|row| row.code == "E255"));
        assert!(parse_refusals(&model).is_empty());

        // Guards a duplicate history callback being attached to the first row.
        let model = crate::parse("var y;histval;y(0)=0;y(0)=1;end;database later;");
        let (diagnostic, order) = parse_refusals(&model)
            .into_iter()
            .find(|(row, _)| row.code == "E243")
            .unwrap();
        assert_eq!(diagnostic.span, model.histval[1].span);
        assert_eq!(order, model.histval[1].active_tokens.end);
        assert!(order > model.histval[0].active_tokens.end);
    }

    #[test]
    fn varexobs_uses_each_callback_type_instead_of_later_declarations() {
        // Guards later declarations/retypes from retroactively accepting an
        // earlier Parse role, including copies of one written macro row.
        for source in [
            "varexobs e;varexo e;",
            "parameters e;varexobs e;change_type(varexo)e;",
            "varexo_det e;varexobs e;",
            "heterogeneity_dimension h;varexo(heterogeneity=h) e;varexobs e;",
        ] {
            let model = crate::parse(source);
            assert!(
                parse_refusals(&model)
                    .iter()
                    .any(|(row, _)| row.code == "E260")
            );
        }
        let model = crate::parse("varexo e;varexobs e;");
        assert!(parse_refusals(&model).is_empty());
        let model = crate::parse(
            "@#for i in 1:2\n@#if i == 1\nparameters e;\n@#else\nchange_type(varexo)e;\n@#endif\nvarexobs e;\n@#endfor\n",
        );
        let rows: Vec<_> = parse_refusals(&model)
            .into_iter()
            .filter(|(row, _)| row.code == "E260")
            .collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].1,
            model.fact_receipts["varexobs"][0].parse_order + 1
        );
    }
}
