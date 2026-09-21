//! D-open parse-time Errors on named openers with no later owner.
//!
//! `check_d_open` is the library family (`analyze`); `check_workspace_d_open`
//! adds the file-relative checks (`@#includepath` directory, `load_params` file).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::check_d_ms::PriorHeadVerdict;
use crate::diagnostic::{Diagnostic, Severity};
use crate::expr::{BinOp, ExprId, ExprKind};
use crate::intern::Name;
use crate::model::{DerivSpec, ExternalFunctionStmt, Model, PolicyCommand};
use crate::span::Span;

pub fn check_d_open(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    out.extend(check_epilogue(model));
    out.extend(check_change_type(model));
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
        let Some((span, name, undeclared)) =
            crate::check_d_ms::cfp_first_bad_row(model, &block.rows)
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
        if let Some((span, name)) = crate::check_d_ms::identification_undeclared_name(model, block)
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
fn prior_std_corr_rows(model: &Model) -> Vec<crate::check_d_ms::PriorHeadName> {
    model
        .dotted_statements
        .iter()
        .flat_map(|stmt| crate::check_d_ms::prior_std_corr_head_names(model, stmt))
        .collect()
}

/// File-relative D-open checks: `@#includepath` directories, `load_params` files.
pub fn check_workspace_d_open(model: &Model, abs_path: &str) -> Vec<Diagnostic> {
    let key =
        crate::include_resolver::make_absolute(&crate::include_resolver::uri_to_path(abs_path))
            .to_string_lossy()
            .into_owned();
    let mut out = check_includepath_dirs(model, &key);
    out.extend(check_load_params(model, &key));
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
        .map(|d| (d.name, d.span))
        .collect()
}

/// True when `name` is declared by a declaration before `pos`.
fn declared_before(model: &Model, name: Name, pos: u32) -> bool {
    declared_names(model)
        .into_iter()
        .any(|(candidate, span)| candidate == name && span.start < pos)
}

fn name_set<'a>(decls: impl Iterator<Item = &'a crate::model::Decl>) -> HashSet<Name> {
    decls.map(|d| d.name).collect()
}

/// Every declared symbol, for the "is not declared" guards. Trend variables are
/// not `Decl`s but are still symbols the opener name slots may name.
fn declared_set(model: &Model) -> HashSet<Name> {
    let mut set = name_set(
        model
            .endogenous
            .iter()
            .chain(&model.exogenous)
            .chain(&model.deterministic_exogenous)
            .chain(&model.parameters),
    );
    set.extend(model.trend_vars.iter().map(|t| t.name));
    set
}

/// Expression ids parsed in model context: model equations, `var(deflator=…)`
/// and `ramsey_constraints` bodies. Trend names are legal here.
fn model_context_expr_ids(model: &Model) -> Vec<ExprId> {
    let mut out = Vec::new();
    for eq in &model.equations {
        out.extend(eq.lhs_expr);
        out.extend(eq.rhs_expr);
    }
    for var in &model.nonstationary_vars {
        out.extend(var.deflator);
    }
    for row in &model.ramsey_constraints {
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
        out.extend(a.expr);
    }
    for entry in model.histval.iter().chain(&model.filter_initial_state) {
        out.extend(entry.expr);
    }
    for stmt in &model.shock_stmts {
        out.extend(stmt.rhs_expr);
    }
    for row in &model.optim_weights {
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
        for r in model.exprs.walk_idents(*id) {
            if seen.insert(r.span.start) {
                out.push((r.name, r.span));
            }
        }
    }
    out
}

/// True when `name` appears in an expression parsed before `pos`.
fn used_in_expression_before(model: &Model, name: Name, pos: u32) -> bool {
    ident_uses(model, &outside_expr_ids(model))
        .into_iter()
        .any(|(candidate, span)| candidate == name && span.start < pos)
}

fn check_epilogue(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
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

    for assignment in &model.epilogue {
        let Some(id) = assignment.expr else {
            continue;
        };
        out.extend(check_epilogue_expr(model, id));
    }

    let mut decl_start: HashMap<Name, u32> = HashMap::new();
    for assignment in &model.epilogue {
        decl_start
            .entry(assignment.name)
            .and_modify(|start| *start = (*start).min(assignment.span.start))
            .or_insert(assignment.span.start);
    }
    if decl_start.is_empty() {
        return out;
    }
    for (name, span) in ident_uses(model, &outside_expr_ids(model)) {
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

/// Epilogue-body Errors for one assignment: unknown name, exo, or a banned operator.
fn check_epilogue_expr(model: &Model, id: ExprId) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for r in model.exprs.walk_idents(id) {
        let name = model.name(r.name);
        if model
            .epilogue
            .iter()
            .any(|a| a.name == r.name && a.span.start < r.span.start)
        {
            continue;
        }
        if !declared_before(model, r.name, r.span.start) {
            out.push(err(
                r.span,
                "E288",
                format!("Variable {name} used in the epilogue block but was not declared."),
            ));
            continue;
        }
        if model
            .deterministic_exogenous
            .iter()
            .any(|d| d.name == r.name)
        {
            out.push(err(
                r.span,
                "E290",
                format!(
                    "Symbol '{name}' cannot be used inside the epilogue block, because it is an exogenous deterministic variable."
                ),
            ));
        } else if model.exogenous.iter().any(|d| d.name == r.name) {
            out.push(err(
                r.span,
                "E289",
                format!(
                    "Symbol '{name}' cannot be used inside the epilogue block, because it is an exogenous variable."
                ),
            ));
        }
    }
    out.extend(check_epilogue_operators(model, id));
    out
}

fn check_epilogue_operators(model: &Model, id: ExprId) -> Vec<Diagnostic> {
    let mut first: HashMap<&'static str, Span> = HashMap::new();
    walk_nodes(model, id, &mut |node| match &node.kind {
        ExprKind::Expectation { .. } => {
            first.entry("E291").or_insert(node.span);
        }
        ExprKind::SteadyState { .. } => {
            first.entry("E292").or_insert(node.span);
        }
        ExprKind::Call { callee, .. } if model.name(*callee).eq_ignore_ascii_case("sum") => {
            first.entry("E293").or_insert(node.span);
        }
        _ => {}
    });
    let mut out = Vec::new();
    for (code, message) in [
        (
            "E291",
            "The 'expectation' operator is forbidden in 'epilogue'.",
        ),
        (
            "E292",
            "The STEADY_STATE() operator is forbidden in epilogue block",
        ),
        ("E293", "The SUM() operator is forbidden in epilogue block"),
    ] {
        if let Some(span) = first.get(code) {
            out.push(err(*span, code, message));
        }
    }
    out
}

/// Depth-first walk over one expression tree, node by node.
fn walk_nodes<'a>(model: &'a Model, id: ExprId, f: &mut impl FnMut(&'a crate::expr::Expr)) {
    let expr = model.exprs.get(id);
    f(expr);
    match &expr.kind {
        ExprKind::Unary { arg, .. } => walk_nodes(model, *arg, f),
        ExprKind::Binary { lhs, rhs, .. } => {
            walk_nodes(model, *lhs, f);
            walk_nodes(model, *rhs, f);
        }
        ExprKind::Call { args, .. } => {
            for arg in args {
                walk_nodes(model, *arg, f);
            }
        }
        ExprKind::SteadyState { arg } => walk_nodes(model, *arg, f),
        ExprKind::Expectation { arg, .. } => walk_nodes(model, *arg, f),
        ExprKind::Ident { .. } | ExprKind::Number | ExprKind::String | ExprKind::Error => {}
    }
}

fn check_change_type(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for stmt in &model.change_type_statements {
        for (name, span) in &stmt.names {
            if !declared_before(model, *name, span.start) {
                out.push(err(
                    *span,
                    "E295",
                    format!("Unknown variable {}", model.name(*name)),
                ));
                continue;
            }
            if used_in_expression_before(model, *name, stmt.span.start) {
                out.push(err(
                    stmt.span,
                    "E296",
                    format!(
                        "You cannot modify the type of symbol {} after having used it in an expression",
                        model.name(*name)
                    ),
                ));
            }
        }
    }
    out
}

fn check_ramsey_statements(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut saw_model = false;
    let mut saw_policy = false;
    for stmt in &model.policy_command_statements {
        match stmt.command {
            PolicyCommand::RamseyModel => {
                if saw_policy {
                    out.push(err(
                        stmt.span,
                        "E298",
                        "A 'ramsey_model' statement cannot follow a 'ramsey_policy' statement.",
                    ));
                } else if saw_model {
                    out.push(err(
                        stmt.span,
                        "E297",
                        "Several 'ramsey_model' statements cannot appear in a given .mod file.",
                    ));
                }
                saw_model = true;
            }
            PolicyCommand::RamseyPolicy => {
                if saw_model {
                    out.push(err(
                        stmt.span,
                        "E299",
                        "A 'ramsey_policy' statement cannot follow a 'ramsey_model' statement.",
                    ));
                } else if saw_policy {
                    out.push(err(
                        stmt.span,
                        "E300",
                        "Several 'ramsey_policy' statements cannot appear in a given .mod file.",
                    ));
                }
                saw_policy = true;
            }
            _ => {}
        }
    }
    for stmt in &model.policy_command_statements {
        let Some(option_span) = stmt.planner_discount else {
            continue;
        };
        let declared = model.parameters.iter().any(|d| {
            model.name(d.name) == "optimal_policy_discount_factor" && d.span.start < stmt.span.start
        });
        if !declared {
            continue;
        }
        let (code, prefix) = match stmt.command {
            PolicyCommand::RamseyModel => ("E301", "ramsey_model"),
            PolicyCommand::RamseyPolicy => ("E302", "ramsey_policy"),
            _ => continue,
        };
        out.push(err(
            option_span,
            code,
            format!(
                "{prefix}: the 'planner_discount' option cannot be used when the 'optimal_policy_discount_factor' parameter is explicitly declared."
            ),
        ));
    }
    out
}

fn check_dsge_prior_weight(model: &Model) -> Vec<Diagnostic> {
    let Some(span) = model.dsge_prior_weight_param else {
        return Vec::new();
    };
    vec![err(
        span,
        "E303",
        "dsge_prior_weight cannot be declared as a parameter. Use the dsge_var option in the estimation statement instead.",
    )]
}

/// E305: `@#includepath` argument that cannot be a string (a number literal).
fn check_includepath_not_string(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for directive in &model.includepaths {
        let argument = directive.argument.trim();
        if argument.parse::<f64>().is_ok() {
            out.push(err(
                directive.span,
                "E305",
                "File name does not evaluate to a string",
            ));
        }
    }
    out
}

/// E304: `@#includepath` argument that is not a directory (resolved against the
/// directive file's parent).
fn check_includepath_dirs(model: &Model, abs_path: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for directive in &model.includepaths {
        let Some(literal) = quoted_literal(&directive.argument) else {
            continue;
        };
        let path = crate::workspace::resolve_includepath(abs_path, literal);
        if path.is_dir() {
            continue;
        }
        out.push(err(
            directive.span,
            "E304",
            format!("{literal} does not evaluate to a valid directory"),
        ));
    }
    out
}

/// The inner text when `raw` is exactly one quoted literal.
fn quoted_literal(raw: &str) -> Option<&str> {
    let s = raw.trim();
    let bytes = s.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let quote = bytes[0];
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    if *bytes.last()? != quote {
        return None;
    }
    let inner = &s[1..s.len() - 1];
    if inner.as_bytes().contains(&quote) {
        return None;
    }
    Some(inner)
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
fn check_load_params(model: &Model, abs_path: &str) -> Vec<Diagnostic> {
    let Some((file, span)) = &model.load_params_file else {
        return Vec::new();
    };
    let path = resolve_beside(abs_path, file);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return vec![err(*span, "E306", format!("Can't open {file}"))];
    };
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for name in load_file_names(&text) {
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
    let mut out = Vec::new();
    let mut seen: HashSet<(Name, bool)> = HashSet::new();
    for trend in &model.trend_vars {
        if seen.insert((trend.name, trend.log_trend)) {
            continue;
        }
        out.push(err(
            trend.span,
            "E307",
            format!(
                "Trend variable {} was declared twice.",
                model.name(trend.name)
            ),
        ));
    }
    out.extend(check_nonstationary_vars(model));

    let mut trend_decl: HashMap<Name, u32> = HashMap::new();
    for trend in &model.trend_vars {
        trend_decl
            .entry(trend.name)
            .and_modify(|start| *start = (*start).min(trend.span.start))
            .or_insert(trend.span.start);
    }
    if trend_decl.is_empty() {
        return out;
    }
    for (name, span) in ident_uses(model, &statement_expr_ids(model)) {
        let Some(&decl_start) = trend_decl.get(&name) else {
            continue;
        };
        if span.start < decl_start {
            continue;
        }
        out.push(err(
            span,
            "E310",
            format!(
                "Variable {} not allowed outside model declaration, because it is a trend variable.",
                model.name(name)
            ),
        ));
    }
    out
}

/// E308 / E309: `var(deflator=…)` lists and deflator expressions.
fn check_nonstationary_vars(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut nonstationary: HashSet<Name> = HashSet::new();
    for var in &model.nonstationary_vars {
        if !nonstationary.insert(var.name) {
            out.push(err(
                var.span,
                "E308",
                format!(
                    "Variable {} was listed more than once as following a trend.",
                    model.name(var.name)
                ),
            ));
        }
    }
    let mut seen: HashSet<ExprId> = HashSet::new();
    for var in &model.nonstationary_vars {
        let Some(deflator) = var.deflator else {
            continue;
        };
        if !seen.insert(deflator) {
            continue;
        }
        let contains_nonstationary = model
            .exprs
            .walk_idents(deflator)
            .any(|r| nonstationary.contains(&r.name));
        if contains_nonstationary {
            out.push(err(
                var.span,
                "E309",
                "The deflator contains a non-stationary endogenous variable. This is not allowed. Please use only stationary endogenous and/or {log_}trend_vars.",
            ));
        }
    }
    out
}

fn check_filter_initial_state(model: &Model) -> Vec<Diagnostic> {
    let endo = name_set(model.endogenous.iter());
    let exo = name_set(model.exogenous.iter());
    let exo_det = name_set(model.deterministic_exogenous.iter());
    let params = name_set(model.parameters.iter());
    let trends: HashSet<Name> = model.trend_vars.iter().map(|t| t.name).collect();
    let declared = declared_set(model);
    let excluded: HashSet<Name> = model.excluded_endogenous.iter().map(|d| d.name).collect();
    let mut seen: HashSet<(Name, i32)> = HashSet::new();
    let mut out = Vec::new();
    for entry in &model.filter_initial_state {
        let name = model.name(entry.name);
        // A symbol `model_remove` dropped was declared when this entry was written, and
        // 7.1 still refuses the entry — with the timing message, not the undeclared one.
        if !declared.contains(&entry.name) && !excluded.contains(&entry.name) {
            out.push(err(
                entry.span,
                "E058",
                format!("Variable '{name}' in filter_initial_state is not declared."),
            ));
            continue;
        }
        let is_exo = exo.contains(&entry.name) || exo_det.contains(&entry.name);
        if !endo.contains(&entry.name)
            && !is_exo
            && (params.contains(&entry.name) || trends.contains(&entry.name))
        {
            out.push(err(
                entry.span,
                "E311",
                format!(
                    "filter_initial_state: {name} should be an endogenous or exogenous variable"
                ),
            ));
            continue;
        }
        if is_exo && entry.lag == 0 {
            out.push(err(
                entry.span,
                "E312",
                format!(
                    "filter_initial_state: exogenous variable {name} must be provided with a lag"
                ),
            ));
            continue;
        }
        if !seen.insert((entry.name, entry.lag)) {
            out.push(err(
                entry.span,
                "E313",
                format!(
                    "filter_initial_state: ({name}, {}) declared twice",
                    entry.lag
                ),
            ));
            continue;
        }
        let min_lag = min_model_lag(model, entry.name);
        if min_lag > entry.lag - 1 {
            out.push(err(
                entry.span,
                "E314",
                format!(
                    "filter_initial_state: variable {name} does not appear in the model with the lag {} (see the reference manual for the timing convention in 'filter_initial_state')",
                    entry.lag - 1
                ),
            ));
        }
    }
    out
}

/// Minimum lag a symbol carries in the model tree (`DataTree::minLagForSymbol`).
fn min_model_lag(model: &Model, name: Name) -> i32 {
    let mut min_lag = 0;
    for id in model_context_expr_ids(model) {
        for r in model.exprs.walk_idents(id) {
            if r.name == name {
                min_lag = min_lag.min(r.timing);
            }
        }
    }
    min_lag
}

fn check_optim_weights(model: &Model) -> Vec<Diagnostic> {
    let endo = name_set(model.endogenous.iter());
    let params = name_set(model.parameters.iter());
    let exo = name_set(model.exogenous.iter());
    let exo_det = name_set(model.deterministic_exogenous.iter());
    let mut singles: HashSet<Name> = HashSet::new();
    let mut pairs: HashSet<(Name, Name)> = HashSet::new();
    let mut out = Vec::new();
    for row in &model.optim_weights {
        if row.second.is_none() && !singles.insert(row.first) {
            out.push(err(
                row.span,
                "E315",
                format!("optim_weights: {} declared twice", model.name(row.first)),
            ));
        }
        if let Some(second) = row.second {
            if !pairs.insert((row.first, second)) {
                out.push(err(
                    row.span,
                    "E316",
                    format!(
                        "optim_weights: pair of variables ({}, {}) declared twice",
                        model.name(row.first),
                        model.name(second)
                    ),
                ));
            }
        }
        for (name, span) in [(row.first, row.first_span)]
            .into_iter()
            .chain(row.second.zip(row.second_span))
        {
            if endo.contains(&name) {
                continue;
            }
            // 7.1 read this row while the symbol was still endogenous (close call 1a).
            if model.surgery_exit_after(name, span.start) {
                continue;
            }
            if params.contains(&name) || exo.contains(&name) || exo_det.contains(&name) {
                out.push(err(
                    span,
                    "E317",
                    format!("{} is not endogenous.", model.name(name)),
                ));
            }
        }
    }
    out
}

/// How a `ramsey_constraints` expression fails the complementarity matcher.
enum RamseyMatch {
    Triple(Name),
    NotAnInequality,
    BoundsNotConstant,
    Malformed,
}

fn match_ramsey_constraint(model: &Model, id: ExprId, at: u32) -> RamseyMatch {
    let ExprKind::Binary { op, lhs, rhs } = &model.exprs.get(id).kind else {
        return RamseyMatch::NotAnInequality;
    };
    let (op, lhs, rhs) = (*op, *lhs, *rhs);
    let greater = match op {
        BinOp::Gt | BinOp::Ge => true,
        BinOp::Lt | BinOp::Le => false,
        _ => return RamseyMatch::NotAnInequality,
    };
    if let Some(name) = contemporaneous_endo(model, lhs, at) {
        if !is_constant_bound(model, rhs, at) {
            return RamseyMatch::BoundsNotConstant;
        }
        return RamseyMatch::Triple(name);
    }
    if let Some(name) = contemporaneous_endo(model, rhs, at) {
        if !is_constant_bound(model, lhs, at) {
            return RamseyMatch::BoundsNotConstant;
        }
        return RamseyMatch::Triple(name);
    }
    // Chained form `bound < endo < bound` / `bound > endo > bound`.
    let ExprKind::Binary {
        op: inner_op,
        lhs: inner_lhs,
        rhs: inner_rhs,
    } = &model.exprs.get(lhs).kind
    else {
        return RamseyMatch::Malformed;
    };
    let inner_op = *inner_op;
    let inner_lhs = *inner_lhs;
    let inner_rhs = *inner_rhs;
    let same_dir = match inner_op {
        BinOp::Gt | BinOp::Ge => greater,
        BinOp::Lt | BinOp::Le => !greater,
        _ => false,
    };
    if !same_dir {
        return RamseyMatch::Malformed;
    }
    let Some(name) = contemporaneous_endo(model, inner_rhs, at) else {
        return RamseyMatch::Malformed;
    };
    if !is_constant_bound(model, inner_lhs, at) || !is_constant_bound(model, rhs, at) {
        return RamseyMatch::BoundsNotConstant;
    }
    RamseyMatch::Triple(name)
}

fn contemporaneous_endo(model: &Model, id: ExprId, at: u32) -> Option<Name> {
    let ExprKind::Ident { name, timing, .. } = &model.exprs.get(id).kind else {
        return None;
    };
    if *timing != 0 {
        return None;
    }
    // A name a later `model_remove` took out of the model was still endogenous when
    // this constraint was written, and 7.1 read it that way.
    let was_endogenous =
        model.endogenous.iter().any(|d| d.name == *name) || model.surgery_exit_after(*name, at);
    was_endogenous.then_some(*name)
}

/// True when the expression holds no endogenous, exogenous, or `varexo_det` symbol.
fn is_constant_bound(model: &Model, id: ExprId, at: u32) -> bool {
    !model.exprs.walk_idents(id).any(|r| {
        model.surgery_exit_after(r.name, at)
            || model.endogenous.iter().any(|d| d.name == r.name)
            || model.exogenous.iter().any(|d| d.name == r.name)
            || model
                .deterministic_exogenous
                .iter()
                .any(|d| d.name == r.name)
    })
}

fn check_ramsey_constraints(model: &Model) -> Vec<Diagnostic> {
    let mut seen: HashSet<Name> = HashSet::new();
    let mut out = Vec::new();
    for row in &model.ramsey_constraints {
        let Some(id) = row.expr else {
            continue;
        };
        // The matcher reads each name as of this row's position (close call 1a).
        match match_ramsey_constraint(model, id, row.span.start) {
            RamseyMatch::Triple(name) => {
                if !seen.insert(name) {
                    out.push(err(
                        row.span,
                        "E318",
                        format!(
                            "The ramsey_constraints block contains two constraints for variable {}",
                            model.name(name)
                        ),
                    ));
                }
            }
            RamseyMatch::NotAnInequality => out.push(err(
                row.span,
                "E319",
                "Ramsey constraint has an incorrect form: This expression is not an inequality",
            )),
            RamseyMatch::BoundsNotConstant => out.push(err(
                row.span,
                "E320",
                "Ramsey constraint has an incorrect form: Bounds must not contain any endogenous or exogenous variable",
            )),
            RamseyMatch::Malformed => out.push(err(
                row.span,
                "E321",
                "Ramsey constraint has an incorrect form:",
            )),
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
    let mut out = Vec::new();
    let mut table: HashMap<Name, EffectiveOptions> = HashMap::new();
    for stmt in &model.external_functions {
        let Some((own, _)) = stmt.name else {
            out.push(err(
                stmt.span,
                "E322",
                "The 'name' option must be passed to external_function().",
            ));
            continue;
        };
        if model.name(own).is_empty() {
            out.push(err(
                stmt.span,
                "E323",
                "An argument must be passed to the 'name' option of the external_function() statement.",
            ));
            continue;
        }
        if matches!(stmt.second_deriv, Some(DerivSpec::Named(..))) && stmt.first_deriv.is_none() {
            out.push(err(
                stmt.span,
                "E324",
                "If the second derivative is provided to the external_function command, the first derivative must also be provided.",
            ));
            continue;
        }
        if matches!(stmt.second_deriv, Some(DerivSpec::Bare(_)))
            && !matches!(stmt.first_deriv, Some(DerivSpec::Bare(_)))
        {
            out.push(err(
                stmt.span,
                "E325",
                "If the second derivative is provided in the top-level function, the first derivative must also be provided in that function.",
            ));
            continue;
        }
        let options = effective_options(stmt, own);
        if options.first == DerivId::Top {
            if let DerivId::Named(id) = options.second {
                if id != own {
                    out.push(err(
                        stmt.span,
                        "E328",
                        "If the first derivative is provided by the top-level function, the second derivative cannot be provided by any other external function.",
                    ));
                }
            }
        }
        // `deriv_id` maps a derivative name equal to `own` to `Top`, so both
        // options are `Named` only when neither is the top-level function.
        if let (DerivId::Named(first), DerivId::Named(second)) = (options.first, options.second) {
            if first == second {
                out.push(err(
                    stmt.span,
                    "E334",
                    "If the Jacobian and Hessian are provided by the same function, that function must be the top-level function.",
                ));
            }
        }
        if let Some(previous) = table.get(&own) {
            if options.nargs != previous.nargs {
                out.push(err(
                    stmt.span,
                    "E326",
                    "The number of arguments passed to the external_function() statement do not match the number of arguments passed to a previous call or declaration of the top-level function.",
                ));
            } else if options.first != previous.first {
                out.push(err(
                    stmt.span,
                    "E327",
                    "The first derivative function passed to the external_function() statement does not match the first derivative function passed to a previous call or declaration of the top-level function.",
                ));
            }
        }
        table.insert(own, options);
    }
    out
}

fn check_pair_lists(model: &Model) -> Vec<Diagnostic> {
    let endo = name_set(model.endogenous.iter());
    let exo_det = name_set(model.deterministic_exogenous.iter());
    // `Model::exogenous` also holds `varexo_det`; these two slots want plain `varexo`.
    let exo: HashSet<Name> = model
        .exogenous
        .iter()
        .map(|d| d.name)
        .filter(|name| !exo_det.contains(name))
        .collect();
    let params = name_set(model.parameters.iter());
    let declared = declared_set(model);
    let mut out = Vec::new();

    for block in &model.init2shocks_blocks {
        let mut seen: HashSet<Name> = HashSet::new();
        for row in &block.rows {
            if !declared.contains(&row.endo) {
                out.push(err(
                    row.endo_span,
                    "E058",
                    format!(
                        "Variable '{}' in init2shocks is not declared.",
                        model.name(row.endo)
                    ),
                ));
            } else if !endo.contains(&row.endo)
                && (exo.contains(&row.endo)
                    || exo_det.contains(&row.endo)
                    || params.contains(&row.endo))
            {
                out.push(err(
                    row.endo_span,
                    "E330",
                    format!(
                        "init2shocks: {} should be an endogenous variable",
                        model.name(row.endo)
                    ),
                ));
            }
            if !declared.contains(&row.exo) {
                out.push(err(
                    row.exo_span,
                    "E058",
                    format!(
                        "Variable '{}' in init2shocks is not declared.",
                        model.name(row.exo)
                    ),
                ));
            } else if !exo.contains(&row.exo)
                && (exo_det.contains(&row.exo)
                    || params.contains(&row.exo)
                    || endo.contains(&row.exo))
            {
                out.push(err(
                    row.exo_span,
                    "E331",
                    format!(
                        "init2shocks: {} should be an exogenous variable",
                        model.name(row.exo)
                    ),
                ));
            }
            if !seen.insert(row.endo) {
                out.push(err(
                    row.span,
                    "E329",
                    format!(
                        "Init2shocks({}): enogenous variable '{}' appears more than once in the init2shocks statement",
                        block.group,
                        model.name(row.endo)
                    ),
                ));
            }
        }
    }

    for row in &model.homotopy_rows {
        if !declared.contains(&row.name) {
            out.push(err(
                row.span,
                "E058",
                format!(
                    "Variable '{}' in homotopy_setup is not declared.",
                    model.name(row.name)
                ),
            ));
            continue;
        }
        if params.contains(&row.name) || exo.contains(&row.name) || exo_det.contains(&row.name) {
            continue;
        }
        if endo.contains(&row.name) {
            out.push(err(
                row.span,
                "E332",
                format!(
                    "homotopy_val: {} should be a parameter or exogenous variable",
                    model.name(row.name)
                ),
            ));
        }
    }

    for group in &model.shock_groups {
        for (name, span) in &group.members {
            if !declared.contains(name) {
                out.push(err(
                    *span,
                    "E058",
                    format!(
                        "Variable '{}' in shock_groups is not declared.",
                        model.name(*name)
                    ),
                ));
                continue;
            }
            if exo.contains(name) {
                continue;
            }
            if exo_det.contains(name) || params.contains(name) || endo.contains(name) {
                out.push(err(
                    *span,
                    "E333",
                    format!(
                        "shock_groups: {} should be an exogenous variable",
                        model.name(*name)
                    ),
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
        let (start, end) = (window[0], window[1]);
        for (i, group) in rows[start..end].iter().enumerate() {
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
