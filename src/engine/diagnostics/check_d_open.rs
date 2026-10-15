//! D-open parse-time Errors on named openers with no later owner.
//!
//! `check_d_open` is the library family (`analyze`); `check_workspace_d_open`
//! adds the file-relative checks (`@#includepath` directory, `load_params` file).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::parse_preflight::{callback_order, statement_callback_order};
use crate::engine::diagnostics::check_d_ms::PriorHeadVerdict;
use crate::engine::diagnostics::{Diagnostic, Severity};
use crate::engine::language::constructor::ComplementarityMatch;
use crate::engine::language::expr::{ExprId, ExprKind};
use crate::engine::language::intern::Name;
use crate::engine::language::model::{DerivSpec, ExternalFunctionStmt, Model, PolicyCommand};
use crate::engine::language::span::Span;
use crate::engine::workspace::Workspace;

pub fn check_d_open(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    out.extend(check_epilogue(model));
    out.extend(check_ramsey_statements(model));
    out.extend(check_dsge_prior_weight(model));
    out.extend(check_includepath_not_string(model));
    out.extend(check_trend_vars(model));
    out.extend(check_filter_initial_state(model));
    out.extend(check_optim_weights(model));
    out.extend(check_ramsey_constraints(model));
    out.extend(check_external_functions(model));
    out.extend(check_pair_lists(model));
    out.extend(check_ms_symbols(model));
    out.extend(check_shock_group_labels(model));
    out
}

/// Only pinned Parse actions participate in the accepted-name frontier.
/// Epilogue duplicate E287, init2shocks duplicate E329 and workspace I/O
/// retain their later-stage owners and do not stop parsing here.
pub(crate) fn parse_refusals(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut out = epilogue_parse_refusals(model);
    out.extend(check_ramsey_statements_with_orders(model));
    out.extend(check_trend_vars_with_orders(model));
    out.extend(check_filter_initial_state_with_orders(model));
    out.extend(check_optim_weights_with_orders(model));
    out.extend(check_ramsey_constraints_with_orders(model));
    out.extend(check_external_functions_with_orders(model));
    out.extend(
        check_pair_lists_with_orders(model)
            .into_iter()
            .filter(|(diagnostic, _)| diagnostic.code != "E329"),
    );
    if let Some(diagnostic) = check_dsge_prior_weight(model).into_iter().next() {
        let order = model
            .written_declarations
            .iter()
            .find(|row| {
                row.written_kind == "parameters"
                    && model.name(row.declaration.name) == "dsge_prior_weight"
            })
            .and_then(|row| model.statements.get(row.statement_id))
            .map_or(0, |statement| statement.token_range.end);
        out.push((diagnostic, order));
    }
    out
}

/// The 0.5.4 family surfaces whose undeclared names are the shipped **E058**
/// sentence, plus the one wrong-type row the shipped **E317** carries there.
///
/// Their parsing checks these names in this order: a `conditional_forecast_paths`
/// `var` row wants an endogenous (**E317** for anything else), while the
/// `svar_identification` body and the `std(…)` / `corr(…)` prior heads only ask
/// that the name exist (**E058**). The prior heads' other checks —
/// `neither endogenous or exogenous` and `is an exogenous deterministic` — are
/// **E059**, in `diag_shape`. Every shape here stops their run at one name, so
/// each loop breaks at its first hit: one Error, exactly as 7.1 prints one.
fn check_ms_symbols(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for block in &model.conditional_forecast_paths {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let Some((span, name, undeclared)) =
            crate::engine::diagnostics::check_d_ms::cfp_first_bad_row(model, &block.rows)
        else {
            continue;
        };
        let message = if undeclared {
            format!(
                "Variable '{}' in conditional_forecast_paths is not declared.",
                model.name(name)
            )
        } else {
            format!("{} is not endogenous.", model.name(name))
        };
        let code = if undeclared { "E058" } else { "E317" };
        out.push(err(span, code, message));
        break;
    }
    for block in &model.svar_identifications {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if let Some((span, name)) =
            crate::engine::diagnostics::check_d_ms::identification_undeclared_name(model, block)
        {
            out.push(err(
                span,
                "E058",
                format!("Variable '{name}' in svar_identification is not declared."),
            ));
            break;
        }
    }
    for row in prior_std_corr_rows(model) {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        match row.verdict {
            PriorHeadVerdict::Undeclared => {
                out.push(err(
                    row.span,
                    "E058",
                    format!(
                        "Variable '{}' in prior is not declared.",
                        model.name(row.name)
                    ),
                ));
                break;
            }
            // Their `check_symbol_is_endogenous_or_exogenous` reaches the
            // `exogenousDet` arm and prints its own sentence, which **E317**
            // already carries the `is not …` shape of.
            PriorHeadVerdict::ExogenousDeterministic => {
                out.push(err(
                    row.span,
                    "E317",
                    format!("{} is an exogenous deterministic.", model.name(row.name)),
                ));
                break;
            }
            _ => {}
        }
    }
    out
}

/// Every `std(…)` / `corr(…)` prior head name, in source order.
fn prior_std_corr_rows(
    model: &Model,
) -> Vec<crate::engine::diagnostics::check_d_ms::PriorHeadName> {
    model
        .dotted_statements
        .iter()
        .flat_map(|stmt| {
            crate::engine::diagnostics::check_d_ms::prior_std_corr_head_names(model, stmt)
        })
        .collect()
}

/// File-relative D-open checks: `@#includepath` directories, `load_params` files.
pub fn check_workspace_d_open(
    ws: &mut Workspace,
    model: &Model,
    abs_path: &str,
) -> Vec<Diagnostic> {
    if ws.is_virtual_root(abs_path) {
        return Vec::new();
    }
    let key = if ws.is_overlay_only() {
        abs_path.to_string()
    } else {
        crate::engine::workspace::include_resolver::make_absolute(
            &crate::engine::workspace::include_resolver::uri_to_path(abs_path),
        )
        .to_string_lossy()
        .into_owned()
    };
    let mut out = check_includepath_dirs(ws, model, &key);
    out.extend(check_load_params(ws, model, &key));
    out
}

fn err(span: Span, code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(span, Severity::Error, code, message)
}

fn warn(span: Span, code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(span, Severity::Warning, code, message)
}

/// Interned declarations of any symbol type, with their identifier spans.
fn declared_names(model: &Model) -> Vec<(Name, Span)> {
    model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.deterministic_exogenous)
        .chain(&model.parameters)
        .chain(&model.retyped_trend_decls)
        .map(|d| (d.name, d.span))
        .collect()
}

/// Expression ids parsed in model context: model equations, `var(deflator=…)`
/// and `ramsey_constraints` bodies. Trend names are legal here.
fn model_context_expr_ids(model: &Model) -> Vec<ExprId> {
    let mut out = Vec::new();
    for eq in &model.equations {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        out.extend(eq.lhs_expr);
        out.extend(eq.rhs_expr);
    }
    for var in &model.nonstationary_vars {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        out.extend(var.deflator);
    }
    for row in &model.ramsey_constraints {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        out.extend(row.expr);
    }
    out
}

/// Expression ids parsed outside the model tree (statement expressions).
fn statement_expr_ids(model: &Model) -> Vec<ExprId> {
    let mut out = Vec::new();
    for a in model
        .param_assignments
        .iter()
        .chain(&model.helper_assignments)
    {
        out.extend(a.expr);
    }
    out.extend(model.planner_objective_expr);
    for a in model.initval.iter().chain(&model.endval) {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        out.extend(a.expr);
    }
    for entry in model.histval.iter().chain(&model.filter_initial_state) {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        out.extend(entry.expr);
    }
    for stmt in &model.shock_stmts {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        out.extend(stmt.rhs_expr);
    }
    for row in &model.optim_weights {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        out.extend(row.expr);
    }
    out
}

fn outside_expr_ids(model: &Model) -> Vec<ExprId> {
    let mut out = model_context_expr_ids(model);
    out.extend(statement_expr_ids(model));
    out
}

/// Every identifier use in `ids`, deduplicated by identifier start offset.
fn ident_uses(model: &Model, ids: &[ExprId]) -> Vec<(Name, Span)> {
    let mut seen: HashSet<u32> = HashSet::new();
    let mut out = Vec::new();
    for id in ids {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        for r in model.exprs.walk_idents(*id) {
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            if seen.insert(r.span.start) {
                out.push((r.name, r.span));
            }
        }
    }
    out
}

fn check_epilogue(model: &Model) -> Vec<Diagnostic> {
    let mut out: Vec<_> = epilogue_parse_refusals(model)
        .into_iter()
        .map(|(diagnostic, _)| diagnostic)
        .collect();
    if model.epilogue_block.is_none() {
        if let Some(span) = model.with_epilogue_span {
            out.push(err(
                span,
                "E286",
                "the 'with_epilogue' option cannot be specified when there is no 'epilogue' block",
            ));
        }
        return out;
    }

    let mut seen: HashSet<Name> = HashSet::new();
    for assignment in &model.epilogue {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if !seen.insert(assignment.name) {
            out.push(err(
                assignment.span,
                "E287",
                format!(
                    "in the 'epilogue' block, variable '{}' is declared twice",
                    model.name(assignment.name)
                ),
            ));
        }
    }

    let mut decl_start: HashMap<Name, u32> = HashMap::new();
    for assignment in &model.epilogue {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        decl_start
            .entry(assignment.name)
            .and_modify(|start| *start = (*start).min(assignment.span.start))
            .or_insert(assignment.span.start);
    }
    if decl_start.is_empty() {
        return out;
    }
    for (name, span) in ident_uses(model, &outside_expr_ids(model)) {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if model
            .model_expression_uses
            .iter()
            .any(|usage| usage.name == name && usage.span == span)
        {
            continue;
        }
        let Some(&start) = decl_start.get(&name) else {
            continue;
        };
        // Before the declaration the name is a plain unknown symbol, not E294.
        if span.start < start {
            continue;
        }
        out.push(err(
            span,
            "E294",
            format!(
                "Symbol '{}' cannot be used outside the epilogue block.",
                model.name(name)
            ),
        ));
    }
    out
}

/// Constructor-stage epilogue reads; E286/E287 remain in Check.
fn epilogue_parse_refusals(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut out = Vec::new();
    for usage in &model.model_expression_uses {
        if crate::engine::control::cancelled() {
            return Vec::new();
        }
        if usage.command != "epilogue" {
            continue;
        }
        let name = model.name(usage.name);
        let (code, message) = match model.symbol_kind_in_context(usage.name, usage.context) {
            None => ("E288", format!("Variable {name} used in the epilogue block but was not declared.")),
            Some("varexo") if !model.heterogeneous_in_context(usage.name, usage.context) => ("E289", format!("Symbol '{name}' cannot be used inside the epilogue block, because it is an exogenous variable.")),
            Some("varexo_det") if !model.heterogeneous_in_context(usage.name, usage.context) => ("E290", format!("Symbol '{name}' cannot be used inside the epilogue block, because it is an exogenous deterministic variable.")),
            _ => continue,
        };
        out.push((err(usage.span, code, message), usage.parse_order + 1));
    }
    for (index, &(name, span)) in model.epilogue_undeclared_calls.iter().enumerate() {
        out.push((
            err(
                span,
                "E288",
                format!(
                    "Variable {} used in the epilogue block but was not declared.",
                    model.name(name)
                ),
            ),
            callback_order(model, "epilogue_unknown_call", index),
        ));
    }
    for (&id, &order) in &model.constructor_orders {
        if model.constructor_scopes.get(&id)
            != Some(&crate::engine::language::constructor::DataTreeScope::Epilogue)
        {
            continue;
        }
        let expression = model.exprs.get(id);
        let (code, message) = match &expression.kind {
            ExprKind::Expectation { .. } => (
                "E291",
                "The 'expectation' operator is forbidden in 'epilogue'.",
            ),
            ExprKind::SteadyState { .. } => (
                "E292",
                "The STEADY_STATE() operator is forbidden in epilogue block",
            ),
            ExprKind::Call { callee, .. } if model.name(*callee).eq_ignore_ascii_case("sum") => {
                ("E293", "The SUM() operator is forbidden in epilogue block")
            }
            _ => continue,
        };
        out.push((err(expression.span, code, message), order));
    }
    out.sort_by_key(|(_, order)| *order);
    out
}

pub(crate) fn check_ramsey_statements(model: &Model) -> Vec<Diagnostic> {
    check_ramsey_statements_with_orders(model)
        .into_iter()
        .map(|(diagnostic, _)| diagnostic)
        .collect()
}

pub(crate) fn check_ramsey_statements_with_orders(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut out = Vec::new();
    let mut saw_model = false;
    let mut saw_policy = false;
    for stmt in &model.policy_command_statements {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        match stmt.command {
            PolicyCommand::RamseyModel => {
                if saw_policy {
                    out.push((
                        err(
                            stmt.span,
                            "E298",
                            "A 'ramsey_model' statement cannot follow a 'ramsey_policy' statement.",
                        ),
                        statement_callback_order(model, stmt.parse_order),
                    ));
                } else if saw_model {
                    out.push((
                        err(
                            stmt.span,
                            "E297",
                            "Several 'ramsey_model' statements cannot appear in a given .mod file.",
                        ),
                        statement_callback_order(model, stmt.parse_order),
                    ));
                }
                saw_model = true;
            }
            PolicyCommand::RamseyPolicy => {
                if saw_model {
                    out.push((
                        err(
                            stmt.span,
                            "E299",
                            "A 'ramsey_policy' statement cannot follow a 'ramsey_model' statement.",
                        ),
                        statement_callback_order(model, stmt.parse_order),
                    ));
                } else if saw_policy {
                    out.push((err(
                        stmt.span,
                        "E300",
                        "Several 'ramsey_policy' statements cannot appear in a given .mod file.",
                    ), statement_callback_order(model, stmt.parse_order)));
                }
                saw_policy = true;
            }
            _ => {}
        }
    }
    for stmt in &model.policy_command_statements {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let Some(option_span) = stmt.planner_discount else {
            continue;
        };
        if !stmt.discount_symbol_existed {
            continue;
        }
        let (code, prefix) = match stmt.command {
            PolicyCommand::RamseyModel => ("E301", "ramsey_model"),
            PolicyCommand::RamseyPolicy => ("E302", "ramsey_policy"),
            _ => continue,
        };
        out.push((err(
            option_span,
            code,
            format!(
                "{prefix}: the 'planner_discount' option cannot be used when the 'optimal_policy_discount_factor' parameter is explicitly declared."
            ),
        ), statement_callback_order(model, stmt.parse_order)));
    }
    out
}

pub(crate) fn check_dsge_prior_weight(model: &Model) -> Vec<Diagnostic> {
    let Some(span) = model.dsge_prior_weight_param else {
        return Vec::new();
    };
    vec![err(
        span,
        "E303",
        "dsge_prior_weight cannot be declared as a parameter. Use the dsge_var option in the estimation statement instead.",
    )]
}

/// E305 is decided by the shared macro evaluator, including variable/function arguments.
fn check_includepath_not_string(model: &Model) -> Vec<Diagnostic> {
    model
        .macro_type_errors
        .iter()
        .filter(|(_, code, _)| *code == "E305")
        .map(|(span, code, message)| err(*span, code, message.clone()))
        .collect()
}

/// E304: `@#includepath` argument that is not a directory (resolved against the
/// invocation root's parent, including directives written in child files).
fn check_includepath_dirs(ws: &mut Workspace, model: &Model, abs_path: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if model.includepaths.is_empty() {
        return out;
    }
    let executed = crate::engine::language::macro_expand::executed_includepaths(
        &model.source,
        model.macro_work_limit,
    );
    for (span, literal) in executed {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let Some((_owner, _)) = ws.map_effective_origin(abs_path, span) else {
            continue;
        };
        let is_directory = if literal.is_empty() {
            false
        } else if ws.is_overlay_only() {
            ws.overlay_directory_exists(abs_path, &literal)
        } else {
            crate::engine::workspace::resolve_includepath(abs_path, &literal).is_dir()
        };
        if is_directory {
            continue;
        }
        out.push(err(
            span,
            "E304",
            format!("{literal} does not evaluate to a valid directory"),
        ));
        break;
    }
    out
}

/// E306 / W204 / E380: the `load_params_and_steady_state` file next to the `.mod`.
///
/// Their gate reads the symbol table the constructor built while parsing: a name
/// not in the table warns **W204** (their `Unknown symbol`), a name in the table
/// but not one of the four allowed slots errors **E380** at the writer
/// (`NumericalInitialization.cc:697`, the `default:` arm of the type switch).
/// The table is finished at parse, so the positional rule is ours: a name
/// declared only *after* the statement is unknown to the constructor, and 7.1
/// warns `Unknown symbol` on it (probed: epilogue after the load statement).
///
/// A trend name is in the slot set like the other two kinds: a *used* trend is
/// refused by the compute-stage balanced-growth test only when the use is itself
/// balanced-growth-incompatible (probed). A compatible use is quiet at check,
/// transform **and** compute, and the write run still prints the sentence, so
/// there is no shadow to defer to.
fn check_load_params(ws: &Workspace, model: &Model, abs_path: &str) -> Vec<Diagnostic> {
    let Some((file, span)) = &model.load_params_file else {
        return Vec::new();
    };
    let text = if ws.is_overlay_only() {
        ws.overlay_beside_source(abs_path, file).map(str::to_string)
    } else {
        std::fs::read_to_string(resolve_beside(abs_path, file)).ok()
    };
    let Some(text) = text else {
        return vec![err(*span, "E306", format!("Can't open {file}"))];
    };
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for name in load_file_names(&text) {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if !seen.insert(name.clone()) {
            continue;
        }
        let declared = declared_names(model)
            .into_iter()
            .any(|(id, decl_span)| model.name(id) == name && decl_span.start < span.start);
        if declared {
            continue;
        }
        if is_unsupported_slot(model, &name, span.start) {
            out.push(err(
                *span,
                "E380",
                format!("Unsupported variable type for {name} in load_params_and_steady_state"),
            ));
            continue;
        }
        out.push(warn(
            *span,
            "W204",
            format!("Unknown symbol {name} in {file}"),
        ));
    }
    out
}

/// True when a declared name sits in a slot the loader's four-way type switch
/// does not accept: an `epilogue` helper, an `external_function` name (the
/// `name=` value or a value named by `first_deriv_provided` /
/// `second_deriv_provided`), or a trend variable.
///
/// All three are positional, like `declared_names`: the loader's constructor
/// reads the symbol table as parsing reaches the statement, so a name declared
/// only *after* it is still unknown and 7.1 warns `Unknown symbol` (probed on
/// the epilogue and the `external_function` shapes).
fn is_unsupported_slot(model: &Model, name: &str, pos: u32) -> bool {
    if model
        .epilogue
        .iter()
        .any(|a| model.name(a.name) == name && a.span.start < pos)
    {
        return true;
    }
    if model.external_functions.iter().any(|stmt| {
        let own = stmt
            .name
            .is_some_and(|(id, span)| model.name(id) == name && span.start < pos);
        let derived = [stmt.first_deriv, stmt.second_deriv].iter().any(|spec| {
            matches!(spec, Some(DerivSpec::Named(id, span)) if model.name(*id) == name && span.start < pos)
        });
        own || derived
    }) {
        return true;
    }
    model
        .trend_vars
        .iter()
        .any(|t| model.name(t.name) == name && t.span.start < pos)
}

/// `name value` pairs, whitespace separated: `f >> symb_name >> value` reads both.
fn load_file_names(text: &str) -> Vec<String> {
    text.split_whitespace()
        .step_by(2)
        .map(str::to_string)
        .collect()
}

fn resolve_beside(abs_path: &str, name: &str) -> std::path::PathBuf {
    let path = Path::new(name);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    Path::new(abs_path)
        .parent()
        .map(|dir| dir.join(path))
        .unwrap_or_else(|| path.to_path_buf())
}

fn check_trend_vars(model: &Model) -> Vec<Diagnostic> {
    check_trend_vars_with_orders(model)
        .into_iter()
        .map(|(diagnostic, _)| diagnostic)
        .collect()
}

fn check_trend_vars_with_orders(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut out = Vec::new();
    let mut seen: HashSet<(Name, bool)> = HashSet::new();
    for trend in &model.trend_vars {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if seen.insert((trend.name, trend.log_trend)) {
            continue;
        }
        out.push((
            err(
                trend.span,
                "E307",
                format!(
                    "Trend variable {} was declared twice.",
                    model.name(trend.name)
                ),
            ),
            statement_callback_order(model, trend.parse_order.saturating_sub(1)),
        ));
    }
    out.extend(check_nonstationary_vars_with_orders(model));

    for (index, &(name, span)) in model.trend_outside_uses.iter().enumerate() {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        out.push((err(
            span,
            "E310",
            format!(
                "Variable {} not allowed outside model declaration, because it is a trend variable.",
                model.name(name)
            ),
        ), callback_order(model, "trend_outside_callback", index)));
    }
    out
}

/// E308 / E309: `var(deflator=…)` lists and deflator expressions.
fn check_nonstationary_vars_with_orders(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut out = Vec::new();
    let mut nonstationary: HashSet<Name> = HashSet::new();
    let mut group_from = 0;
    while group_from < model.nonstationary_vars.len() {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let order = callback_order(model, "nonstationary_callback", group_from);
        let group_end = (group_from + 1..model.nonstationary_vars.len())
            .find(|&index| callback_order(model, "nonstationary_callback", index) != order)
            .unwrap_or(model.nonstationary_vars.len());
        // end_nonstationary_var inserts this complete declaration list before
        // inspecting its deflator. Later declarations are not in that table.
        for var in &model.nonstationary_vars[group_from..group_end] {
            if !nonstationary.insert(var.name) {
                out.push((
                    err(
                        var.span,
                        "E308",
                        format!(
                            "Variable {} was listed more than once as following a trend.",
                            model.name(var.name)
                        ),
                    ),
                    order,
                ));
            }
        }
        let var = &model.nonstationary_vars[group_from];
        if var.deflator.is_some_and(|deflator| {
            model
                .deflator_variables
                .get(&deflator)
                .is_some_and(|names| names.iter().any(|name| nonstationary.contains(name)))
        }) {
            out.push((err(
                var.span,
                "E309",
                "The deflator contains a non-stationary endogenous variable. This is not allowed. Please use only stationary endogenous and/or {log_}trend_vars.",
            ), order));
        }
        group_from = group_end;
    }
    out
}

fn check_filter_initial_state(model: &Model) -> Vec<Diagnostic> {
    check_filter_initial_state_with_orders(model)
        .into_iter()
        .map(|(diagnostic, _)| diagnostic)
        .collect()
}

fn check_filter_initial_state_with_orders(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut seen: HashSet<(Name, i32)> = HashSet::new();
    let mut out = Vec::new();
    for (index, entry) in model.filter_initial_state.iter().enumerate() {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if model
            .filter_initial_state_block_starts
            .binary_search(&index)
            .is_ok()
        {
            seen.clear();
        }
        let name = model.name(entry.name);
        let kind = model.symbol_kind_in_context(entry.name, entry.symbol_type_context);
        if kind.is_none() {
            out.push((
                err(
                    entry.span,
                    "E058",
                    format!("Variable '{name}' in filter_initial_state is not declared."),
                ),
                entry.active_tokens.end,
            ));
            continue;
        }
        let ordinary = !model.heterogeneous_in_context(entry.name, entry.symbol_type_context);
        let is_endo = ordinary && kind == Some("var");
        let is_exo = ordinary && matches!(kind, Some("varexo" | "varexo_det"));
        if !is_endo && !is_exo && kind.is_some() {
            out.push((
                err(
                    entry.span,
                    "E311",
                    format!(
                    "filter_initial_state: {name} should be an endogenous or exogenous variable"
                ),
                ),
                entry.active_tokens.end,
            ));
            continue;
        }
        if is_exo && entry.lag == 0 {
            out.push((
                err(
                    entry.span,
                    "E312",
                    format!(
                    "filter_initial_state: exogenous variable {name} must be provided with a lag"
                ),
                ),
                entry.active_tokens.end,
            ));
            continue;
        }
        if !seen.insert((entry.name, entry.lag)) {
            out.push((
                err(
                    entry.span,
                    "E313",
                    format!(
                        "filter_initial_state: ({name}, {}) declared twice",
                        entry.lag
                    ),
                ),
                entry.active_tokens.end,
            ));
            continue;
        }
        let min_lag = min_model_lag(model, entry.name, entry.active_tokens.end);
        if min_lag > entry.lag - 1 {
            out.push((err(
                entry.span,
                "E314",
                format!(
                    "filter_initial_state: variable {name} does not appear in the model with the lag {} (see the reference manual for the timing convention in 'filter_initial_state')",
                    entry.lag - 1
                ),
            ), entry.active_tokens.end));
        }
    }
    out
}

/// Minimum lag a symbol carries in the model tree (`DataTree::minLagForSymbol`).
fn min_model_lag(model: &Model, name: Name, at: usize) -> i32 {
    // DataTree::minLagForSymbol reads already allocated variable nodes. Keep
    // earlier removed equations, exclude later nodes and other private trees.
    model
        .model_expression_uses
        .iter()
        .filter(|use_| {
            use_.name == name
                && use_.parse_order < at
                && use_.scope == crate::engine::language::constructor::DataTreeScope::Dynamic
        })
        .fold(0, |lag, use_| lag.min(use_.timing))
}

fn check_optim_weights(model: &Model) -> Vec<Diagnostic> {
    check_optim_weights_with_orders(model)
        .into_iter()
        .map(|(diagnostic, _)| diagnostic)
        .collect()
}

fn check_optim_weights_with_orders(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut singles: HashSet<Name> = HashSet::new();
    let mut pairs: HashSet<(Name, Name)> = HashSet::new();
    let mut out = Vec::new();
    for (index, row) in model.optim_weights.iter().enumerate() {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if model
            .optim_weights_block_starts
            .binary_search(&index)
            .is_ok()
        {
            singles.clear();
            pairs.clear();
        }
        if row.second.is_none() && !singles.insert(row.first) {
            out.push((
                err(
                    row.span,
                    "E315",
                    format!("optim_weights: {} declared twice", model.name(row.first)),
                ),
                row.active_tokens.end,
            ));
        }
        if let Some(second) = row.second
            && !pairs.insert((row.first, second))
        {
            out.push((
                err(
                    row.span,
                    "E316",
                    format!(
                        "optim_weights: pair of variables ({}, {}) declared twice",
                        model.name(row.first),
                        model.name(second)
                    ),
                ),
                row.active_tokens.end,
            ));
        }
        for (name, span, kind) in [(row.first, row.first_span, row.first_kind)]
            .into_iter()
            .chain(
                row.second
                    .zip(row.second_span)
                    .map(|(name, span)| (name, span, row.second_kind)),
            )
        {
            if kind == Some("var") {
                continue;
            }
            if kind.is_none() {
                out.push((
                    err(
                        span,
                        "E058",
                        format!("Unknown symbol: {}.", model.name(name)),
                    ),
                    row.active_tokens.end,
                ));
            } else {
                out.push((
                    err(
                        span,
                        "E317",
                        format!("{} is not endogenous.", model.name(name)),
                    ),
                    row.active_tokens.end,
                ));
            }
        }
    }
    out
}

fn check_ramsey_constraints(model: &Model) -> Vec<Diagnostic> {
    check_ramsey_constraints_with_orders(model)
        .into_iter()
        .map(|(diagnostic, _)| diagnostic)
        .collect()
}

fn check_ramsey_constraints_with_orders(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut seen: HashSet<Name> = HashSet::new();
    let mut out = Vec::new();
    for row in &model.ramsey_constraints {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let Some(id) = row.expr else {
            continue;
        };
        let Some(callback_order) = row.callback_order else {
            continue;
        };
        // The matcher reads each name as of this row's position (close call 1a).
        if model.exprs.walk_idents(id).any(|r| {
            model
                .var_removed_model_uses
                .iter()
                .any(|(name, span, _)| *name == r.name && *span == r.span)
        }) {
            continue;
        }
        let Some(result) = row.parse_match else {
            continue;
        };
        match result {
            ComplementarityMatch::Triple(name) => {
                if !seen.insert(name) {
                    out.push((err(
                        row.span,
                        "E318",
                        format!(
                            "The ramsey_constraints block contains two constraints for variable {}",
                            model.name(name)
                        ),
                    ), callback_order));
                }
            }
            ComplementarityMatch::NotAnInequality => out.push((err(
                row.span,
                "E319",
                "Ramsey constraint has an incorrect form: This expression is not an inequality",
            ), callback_order)),
            ComplementarityMatch::BoundsNotConstant => out.push((err(
                row.span,
                "E320",
                "Ramsey constraint has an incorrect form: Bounds must not contain any endogenous or exogenous variable",
            ), callback_order)),
            ComplementarityMatch::Malformed => out.push((err(
                row.span,
                "E321",
                "Ramsey constraint has an incorrect form:",
            ), callback_order)),
        }
    }
    out
}

/// Effective derivative option, mirroring `ExternalFunctionsTable` IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DerivId {
    NotSet,
    Top,
    Named(Name),
}

#[derive(Clone, Copy, Debug)]
struct EffectiveOptions {
    nargs: i32,
    first: DerivId,
    second: DerivId,
}

fn effective_options(stmt: &ExternalFunctionStmt, own: Name) -> EffectiveOptions {
    EffectiveOptions {
        nargs: stmt.nargs.unwrap_or(1),
        first: deriv_id(stmt.first_deriv, own),
        second: deriv_id(stmt.second_deriv, own),
    }
}

fn deriv_id(spec: Option<DerivSpec>, own: Name) -> DerivId {
    match spec {
        None => DerivId::NotSet,
        Some(DerivSpec::Bare(_)) => DerivId::Top,
        Some(DerivSpec::Named(id, _)) if id == own => DerivId::Top,
        Some(DerivSpec::Named(id, _)) => DerivId::Named(id),
    }
}

fn check_external_functions(model: &Model) -> Vec<Diagnostic> {
    check_external_functions_with_orders(model)
        .into_iter()
        .map(|(diagnostic, _)| diagnostic)
        .collect()
}

fn check_external_functions_with_orders(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut out = Vec::new();
    let mut table: HashMap<Name, EffectiveOptions> = HashMap::new();
    for stmt in &model.external_functions {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let Some((own, _)) = stmt.name else {
            out.push((
                err(
                    stmt.span,
                    "E322",
                    "The 'name' option must be passed to external_function().",
                ),
                statement_callback_order(model, stmt.parse_order),
            ));
            continue;
        };
        if model.name(own).is_empty() {
            out.push((err(
                stmt.span,
                "E323",
                "An argument must be passed to the 'name' option of the external_function() statement.",
            ), statement_callback_order(model, stmt.parse_order)));
            continue;
        }
        if matches!(stmt.second_deriv, Some(DerivSpec::Named(..))) && stmt.first_deriv.is_none() {
            out.push((err(
                stmt.span,
                "E324",
                "If the second derivative is provided to the external_function command, the first derivative must also be provided.",
            ), statement_callback_order(model, stmt.parse_order)));
            continue;
        }
        if matches!(stmt.second_deriv, Some(DerivSpec::Bare(_)))
            && !matches!(stmt.first_deriv, Some(DerivSpec::Bare(_)))
        {
            out.push((err(
                stmt.span,
                "E325",
                "If the second derivative is provided in the top-level function, the first derivative must also be provided in that function.",
            ), statement_callback_order(model, stmt.parse_order)));
            continue;
        }
        let options = effective_options(stmt, own);
        if options.first == DerivId::Top
            && let DerivId::Named(id) = options.second
            && id != own
        {
            out.push((err(
                        stmt.span,
                        "E328",
                        "If the first derivative is provided by the top-level function, the second derivative cannot be provided by any other external function.",
                    ), statement_callback_order(model, stmt.parse_order)));
        }
        // `deriv_id` maps a derivative name equal to `own` to `Top`, so both
        // options are `Named` only when neither is the top-level function.
        if let (DerivId::Named(first), DerivId::Named(second)) = (options.first, options.second)
            && first == second
        {
            out.push((err(
                    stmt.span,
                    "E334",
                    "If the Jacobian and Hessian are provided by the same function, that function must be the top-level function.",
                ), statement_callback_order(model, stmt.parse_order)));
        }
        if let Some(previous) = table.get(&own) {
            if options.nargs != previous.nargs {
                out.push((err(
                    stmt.span,
                    "E326",
                    "The number of arguments passed to the external_function() statement do not match the number of arguments passed to a previous call or declaration of the top-level function.",
                ), statement_callback_order(model, stmt.parse_order)));
            } else if options.first != previous.first {
                out.push((err(
                    stmt.span,
                    "E327",
                    "The first derivative function passed to the external_function() statement does not match the first derivative function passed to a previous call or declaration of the top-level function.",
                ), statement_callback_order(model, stmt.parse_order)));
            }
        }
        table.insert(own, options);
    }
    out
}

fn check_pair_lists(model: &Model) -> Vec<Diagnostic> {
    check_pair_lists_with_orders(model)
        .into_iter()
        .map(|(diagnostic, _)| diagnostic)
        .collect()
}

fn check_pair_lists_with_orders(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut out = Vec::new();
    let mut pair_cursor = 0;
    let mut member_cursor = 0;

    for block in &model.init2shocks_blocks {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let mut seen: HashSet<Name> = HashSet::new();
        for row in &block.rows {
            let pair_index = pair_cursor;
            pair_cursor += 1;
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            if model
                .symbol_kind_in_context(row.endo, row.symbol_type_context)
                .is_none()
            {
                out.push((
                    err(
                        row.endo_span,
                        "E058",
                        format!(
                            "Variable '{}' in init2shocks is not declared.",
                            model.name(row.endo)
                        ),
                    ),
                    callback_order(model, "init2shocks_callback", pair_index),
                ));
            } else if model.symbol_kind_in_context(row.endo, row.symbol_type_context) != Some("var")
            {
                out.push((
                    err(
                        row.endo_span,
                        "E330",
                        format!(
                            "init2shocks: {} should be an endogenous variable",
                            model.name(row.endo)
                        ),
                    ),
                    callback_order(model, "init2shocks_callback", pair_index),
                ));
            }
            if model
                .symbol_kind_in_context(row.exo, row.symbol_type_context)
                .is_none()
            {
                out.push((
                    err(
                        row.exo_span,
                        "E058",
                        format!(
                            "Variable '{}' in init2shocks is not declared.",
                            model.name(row.exo)
                        ),
                    ),
                    callback_order(model, "init2shocks_callback", pair_index),
                ));
            } else if model.symbol_kind_in_context(row.exo, row.symbol_type_context)
                != Some("varexo")
            {
                out.push((
                    err(
                        row.exo_span,
                        "E331",
                        format!(
                            "init2shocks: {} should be an exogenous variable",
                            model.name(row.exo)
                        ),
                    ),
                    callback_order(model, "init2shocks_callback", pair_index),
                ));
            }
            if !seen.insert(row.endo) {
                out.push((err(
                    row.span,
                    "E329",
                    format!(
                        "Init2shocks({}): enogenous variable '{}' appears more than once in the init2shocks statement",
                        block.group,
                        model.name(row.endo)
                    ),
                ), 0));
            }
        }
    }

    for (index, row) in model.homotopy_rows.iter().enumerate() {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if model
            .symbol_kind_in_context(row.name, row.symbol_type_context)
            .is_none()
        {
            out.push((
                err(
                    row.span,
                    "E058",
                    format!(
                        "Variable '{}' in homotopy_setup is not declared.",
                        model.name(row.name)
                    ),
                ),
                callback_order(model, "homotopy_callback", index),
            ));
            continue;
        }
        if matches!(
            model.symbol_kind_in_context(row.name, row.symbol_type_context),
            Some("parameters" | "varexo" | "varexo_det")
        ) {
            continue;
        }
        if model.symbol_kind_in_context(row.name, row.symbol_type_context) == Some("var") {
            out.push((
                err(
                    row.span,
                    "E332",
                    format!(
                        "homotopy_val: {} should be a parameter or exogenous variable",
                        model.name(row.name)
                    ),
                ),
                callback_order(model, "homotopy_callback", index),
            ));
        }
    }

    for group in &model.shock_groups {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        for (name, span) in &group.members {
            let member_order = callback_order(model, "shock_group_member_callback", member_cursor);
            member_cursor += 1;
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            if model
                .symbol_kind_in_context(*name, group.symbol_type_context)
                .is_none()
            {
                out.push((
                    err(
                        *span,
                        "E058",
                        format!(
                            "Variable '{}' in shock_groups is not declared.",
                            model.name(*name)
                        ),
                    ),
                    member_order,
                ));
                continue;
            }
            if model.symbol_kind_in_context(*name, group.symbol_type_context) == Some("varexo") {
                continue;
            }
            if model
                .symbol_kind_in_context(*name, group.symbol_type_context)
                .is_some()
            {
                out.push((
                    err(
                        *span,
                        "E333",
                        format!(
                            "shock_groups: {} should be an exogenous variable",
                            model.name(*name)
                        ),
                    ),
                    member_order,
                ));
            }
        }
    }
    out
}

/// W205: two rows of one `shock_groups` block reuse a label.
///
/// Their comparison is within one block only — each block is its own
/// `ShockGroupsStatement` and the vector clears at `end_shock_groups`, so two
/// blocks may share a label silently (probed on 7.1). The `name=group` opener
/// option is skipped by `bump_plain_opener` and never recorded, so it cannot
/// fire this either. Their text, trailing period included.
///
/// Theirs warns on every row that has a later twin
/// (`for (auto it1 = it + 1; …)`, `Shocks.cc:1192`; message at `:1196`, pin
/// `9c61fb6e`), so the span points at the definition that "only using the last
/// definition" discards. A three-row block with one label reports twice.
fn check_shock_group_labels(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let rows = &model.shock_groups;
    let mut bounds = model.shock_group_block_starts.clone();
    bounds.push(rows.len());
    for window in bounds.windows(2) {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let (start, end) = (window[0], window[1]);
        for (i, group) in rows[start..end].iter().enumerate() {
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            let rest = &rows[start + i + 1..end];
            if !rest.iter().any(|later| later.label == group.label) {
                continue;
            }
            out.push(warn(
                group.label_span,
                "W205",
                format!(
                    "shock group label '{}' has been reused. Only using the last definition.",
                    group.label
                ),
            ));
        }
    }
    out
}

#[cfg(test)]
mod parse_receipt_tests {
    use super::*;

    #[test]
    fn epilogue_constructor_refusals_keep_parse_stage_and_lhs_scope() {
        // Guards constructor errors from being deferred to Check, while the
        // declared LHS and Check-only duplicate remain valid Parse facts.
        for (text, code) in [
            ("epilogue;a=missing;end;database later;", "E288"),
            ("varexo e;epilogue;a=e;end;database later;", "E289"),
            ("varexo_det e;epilogue;a=e;end;database later;", "E290"),
            (
                "var y;epilogue;a=EXPECTATION(0)(y);end;database later;",
                "E291",
            ),
            (
                "var y;epilogue;a=STEADY_STATE(y);end;database later;",
                "E292",
            ),
            ("var y;epilogue;a=SUM(y);end;database later;", "E293"),
        ] {
            let model = crate::parse(text);
            assert!(
                parse_refusals(&model)
                    .iter()
                    .any(|(row, _)| row.code == code),
                "{text}"
            );
            assert!(
                crate::engine::analysis::named_callbacks::collect(&model)
                    .slots
                    .is_empty(),
                "{text}"
            );
        }
        for text in [
            "epilogue;a=a;end;database kept;",
            "epilogue;a=1;a=2;end;database kept;",
        ] {
            let model = crate::parse(text);
            assert!(parse_refusals(&model).is_empty(), "{text}");
            assert!(
                !crate::engine::analysis::named_callbacks::collect(&model)
                    .slots
                    .is_empty(),
                "{text}"
            );
        }
    }

    #[test]
    fn nonstationary_deflator_reads_only_its_completed_declaration_groups() {
        // Guards later nonstationary declarations changing earlier acceptance,
        // while requiring every name in the current declaration to be visible.
        let model = crate::parse("var y;var(deflator=y) x;database kept;var(deflator=1) y;");
        assert!(!parse_refusals(&model)
            .iter()
            .any(|(row, _)| row.code == "E309"));
        let model = crate::parse("var y;var(deflator=y) x y;");
        assert!(parse_refusals(&model)
            .iter()
            .any(|(row, _)| row.code == "E309"));
        let model = crate::parse("var y;var(deflator=y-y+1) x;database kept;var(deflator=1) y;");
        assert!(!parse_refusals(&model)
            .iter()
            .any(|(row, _)| row.code == "E309"));
    }

    #[test]
    fn filter_lag_uses_prior_dynamic_constructor_nodes() {
        // Guards future equations and private heterogeneous trees from making
        // an earlier filter row valid. Removed dynamic nodes remain allocated.
        let model = crate::parse(
            "var y;filter_initial_state;y(0)=0;end;database leaked;model;y=y(-1);end;",
        );
        assert!(parse_refusals(&model)
            .iter()
            .any(|(row, _)| row.code == "E314"));
        let model = crate::parse("heterogeneity_dimension h;var(heterogeneity=h) y;model(heterogeneity=h);y=y(-1);end;filter_initial_state;y(0)=0;end;");
        assert_eq!(
            min_model_lag(
                &model,
                model.filter_initial_state[0].name,
                model.expanded_tokens.len()
            ),
            0
        );
        let model = crate::parse(
            "var y;model;[name='Y']y=y(-1);end;model_remove('Y');filter_initial_state;y(0)=0;end;",
        );
        assert!(parse_refusals(&model)
            .iter()
            .any(|(row, _)| row.code == "E311"));
        assert!(!parse_refusals(&model)
            .iter()
            .any(|(row, _)| row.code == "E314"));
        let name = model.filter_initial_state[0].name;
        assert_eq!(min_model_lag(&model, name, model.expanded_tokens.len()), -1);

        let model = crate::parse("heterogeneity_dimension h;var(heterogeneity=h) x;filter_initial_state;x(1)=0;end;database leaked;");
        assert!(parse_refusals(&model)
            .iter()
            .any(|(row, _)| row.code == "E311"));
    }

    #[test]
    fn ramsey_match_uses_captured_types_and_end_block_action() {
        // Guards change_type acceptance and preserves child constructors that
        // complete before the end-block complementarity matcher refuses.
        let model = crate::parse(
            "parameters p;change_type(var) p;ramsey_constraints;p>0;end;database kept;",
        );
        assert!(parse_refusals(&model).is_empty());
        let model = crate::parse("heterogeneity_dimension h;var(heterogeneity=h) x;ramsey_constraints;x>0;end;database leaked;");
        assert!(parse_refusals(&model)
            .iter()
            .any(|(row, _)| row.code == "E321"));
        let model = crate::parse("heterogeneity_dimension h;var(heterogeneity=h) x;var y;ramsey_constraints;y>x;end;database kept;");
        assert!(parse_refusals(&model).is_empty());
        for text in [
            "var y;ramsey_constraints;(y+0)>1;end;database kept;",
            "var y;ramsey_constraints;y>(y-y);end;database kept;",
            "parameters p;var y;model;#a=p;y=1;end;ramsey_constraints;y>a;end;database kept;",
        ] {
            assert!(parse_refusals(&crate::parse(text)).is_empty(), "{text}");
        }
        let model =
            crate::parse("var y;model;#a=y;y=1;end;ramsey_constraints;y>a;end;database leaked;");
        assert!(parse_refusals(&model)
            .iter()
            .any(|(row, _)| row.code == "E320"));
        let model =
            crate::parse("var y;ramsey_constraints;1;y>pac_expectation(m);end;database later;");
        let (_, order) = parse_refusals(&model)
            .into_iter()
            .find(|(row, _)| row.code == "E319")
            .unwrap();
        assert_eq!(model.expanded_tokens[order - 1].text(&model.source), ";");
        assert_eq!(model.expanded_tokens[order - 2].text(&model.source), "end");
        assert!(model.named_model_operators[0].callback_order < order);
    }

    #[test]
    fn parse_receipts_exclude_check_only_rows_and_keep_original_locations() {
        // Guards a later Check refusal from stopping accepted Parse actions.
        let model = crate::parse("var y;varexo e;init2shocks;y e;y e;end;database later;");
        assert!(check_d_open(&model).iter().any(|row| row.code == "E329"));
        assert!(parse_refusals(&model).is_empty());

        // Guards an exclusive callback order being mistaken for its location.
        let model = crate::parse("parameters dsge_prior_weight;database later;");
        let raw = check_dsge_prior_weight(&model).pop().unwrap();
        let (diagnostic, order) = parse_refusals(&model)
            .into_iter()
            .find(|(row, _)| row.code == "E303")
            .unwrap();
        assert_eq!(diagnostic.span, raw.span);
        assert_eq!(diagnostic.message, raw.message);
        assert_eq!(model.expanded_tokens[order - 1].text(&model.source), ";");
        assert_eq!(model.expanded_tokens[order].text(&model.source), "database");
    }
}
