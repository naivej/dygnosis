//! Heterogeneity refusals for Dynare 7.2.
//!
//! Parse-stage errors (`E459`–`E464`, `E475`–`E478`) stop later check-stage
//! errors. `E479` is also a parse refusal, but it is reported with the obsolete
//! `mcp` warning. `W207` is ours: it stays on an accepted two-dimension file
//! and beside check-stage errors.

use std::collections::HashSet;

use crate::engine::diagnostics::{Diagnostic, Severity};
use crate::engine::language::expr::{BinOp, ExprId, ExprKind};
use crate::engine::language::intern::Name;
use crate::engine::language::model::{
    Equation, HeterogeneousModelBlock, Model, PolicyCommand, ShockBlock, ShockBlockKind, ShockKind,
};
use crate::engine::language::span::{LineIndex, Span};

const W207: &str = "Dynare 7.2 cannot load or compute a heterogeneous steady state with more than one heterogeneity dimension.";

pub fn check_parse(model: &Model) -> Vec<Diagnostic> {
    parse_rows(model)
        .into_iter()
        .map(|(diagnostic, _)| diagnostic)
        .collect()
}

/// Parse receipts retain the callback order, separately from the display span.
pub(crate) fn parse_refusals(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut out = Vec::new();
    unknown_and_repeated_dimensions(model, &mut out, true);
    forbidden_symbols(model, &mut out, true);
    sum_operators(model, &mut out, true);
    for block in &model.shock_blocks {
        if block.kind == ShockBlockKind::Heterogeneous
            && let Some(row) = heterogeneous_shock_row_refusal(model, block)
        {
            out.push(row);
        }
    }
    out.extend(mcp_refusals(model));
    out
}

fn parse_rows(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut out = Vec::new();
    unknown_and_repeated_dimensions(model, &mut out, false);
    forbidden_symbols(model, &mut out, false);
    sum_operators(model, &mut out, false);
    out
}

pub fn check_mcp(model: &Model) -> Vec<Diagnostic> {
    mcp_refusals(model)
        .into_iter()
        .map(|(diagnostic, _)| diagnostic)
        .collect()
}

fn mcp_refusals(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut out = Vec::new();
    for block in &model.heterogeneous_models {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        for eq in &block.equations {
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            if eq.complementarity.is_some() {
                continue;
            }
            let Some(value) = eq.tag_map.get("mcp") else {
                continue;
            };
            if !value.contains('<') && !value.contains('>') {
                continue;
            }
            let lhs = value.split(['<', '>']).next().unwrap_or("").trim();
            let lhs = lhs.split('(').next().unwrap_or(lhs).trim();
            if lhs.is_empty() || !symbol_declared(model, lhs) {
                continue;
            }
            out.push((
                error(
                    eq.span,
                    "E479",
                    "'mcp' tags are not allowed in heterogeneous model blocks",
                ),
                eq.active_tokens.end,
            ));
            return out;
        }
    }
    out
}

pub fn check_second_dimension(model: &Model) -> Vec<Diagnostic> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut first = true;
    for dim in &model.heterogeneity_dimensions {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if !seen.insert(dim.name) {
            continue;
        }
        if first {
            first = false;
            continue;
        }
        out.push(Diagnostic::new(
            dim.name_span,
            Severity::Warning,
            "W207",
            W207,
        ));
    }
    out
}

/// Written equation count versus distinct written endogenous names, per
/// heterogeneity dimension. Same idea as aggregate `W013`: do not count helper
/// variables Dynare inserts later for leads and lags. `#` locals and `[static]`
/// rows are not counted equations. A name written twice counts once; **W031**
/// reports the duplicate. An unresolved include or expansion withholds the
/// count, because the missing text may change it.
pub fn check_square(model: &Model) -> Vec<Diagnostic> {
    if crate::engine::diagnostics::check_writing::model_structure_incomplete(model) {
        return Vec::new();
    }
    struct Side {
        count: usize,
        span: Span,
    }
    let mut seen = HashSet::new();
    let mut endogenous: Vec<(String, Side)> = Vec::new();
    for decl in model.final_decls(&["var"]) {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let Some(dim) = model.final_heterogeneity(decl) else {
            continue;
        };
        if !seen.insert((dim, decl.name)) {
            continue;
        }
        let name = model.name(dim).to_string();
        if let Some((_, side)) = endogenous.iter_mut().find(|(key, _)| key == &name) {
            side.count += 1;
        } else {
            endogenous.push((
                name,
                Side {
                    count: 1,
                    span: decl.span,
                },
            ));
        }
    }
    let mut equations: Vec<(String, Side)> = Vec::new();
    for block in &model.heterogeneous_models {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let name = model.name(block.dimension).to_string();
        let n = block
            .equations
            .iter()
            .filter(|eq| !eq.is_local && !eq.static_tag)
            .count();
        if let Some((_, side)) = equations.iter_mut().find(|(key, _)| key == &name) {
            side.count += n;
        } else {
            equations.push((
                name,
                Side {
                    count: n,
                    span: block.span,
                },
            ));
        }
    }
    let mut names: Vec<String> = endogenous.iter().map(|(name, _)| name.clone()).collect();
    for (name, _) in &equations {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if !names.iter().any(|have| have == name) {
            names.push(name.clone());
        }
    }
    let mut out = Vec::new();
    for name in names {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let n_endo = endogenous
            .iter()
            .find(|(key, _)| key == &name)
            .map(|(_, side)| side.count)
            .unwrap_or(0);
        let eq_side = equations.iter().find(|(key, _)| key == &name);
        let n_eq = eq_side.map(|(_, side)| side.count).unwrap_or(0);
        if n_eq == n_endo {
            continue;
        }
        let span = eq_side
            .map(|(_, side)| side.span)
            .or_else(|| {
                endogenous
                    .iter()
                    .find(|(key, _)| key == &name)
                    .map(|(_, side)| side.span)
            })
            .unwrap_or(Span::new(0, 0));
        out.push(Diagnostic::new(
            span,
            Severity::Warning,
            "W208",
            format!(
                "Equation count mismatch: {n_eq} equation(s) but {n_endo} endogenous variable(s) in heterogeneity dimension '{name}'."
            ),
        ).with_model_dimension(name));
    }
    out
}

pub fn check_check(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if model.heterogeneity_dimensions.is_empty() {
        return out;
    }
    shock_types(model, &mut out);
    if out.is_empty() {
        equation_timing(model, &mut out);
    }
    if out.is_empty() {
        unsupported_command(model, &mut out);
    }
    out
}

fn unknown_and_repeated_dimensions(model: &Model, out: &mut Vec<(Diagnostic, usize)>, all: bool) {
    let mut seen = HashSet::new();
    for dim in &model.heterogeneity_dimensions {
        if crate::engine::control::cancelled() {
            return;
        }
        if !seen.insert(dim.name) {
            out.push((
                error(
                    dim.name_span,
                    "E460",
                    format!(
                        "Heterogeneity dimension '{}' already declared",
                        model.name(dim.name)
                    ),
                ),
                dim.parse_order,
            ));
            if !all {
                return;
            }
        }
    }
    let mut slots: Vec<_> = model
        .named_slots
        .iter()
        .filter(|slot| {
            matches!(
                slot.owner,
                crate::engine::language::model::NamedSlotOwner::DimensionLookup
                    | crate::engine::language::model::NamedSlotOwner::HeterogeneousShock(_)
            )
        })
        .collect();
    slots.sort_by_key(|slot| slot.callback_order);
    let duplicates = crate::engine::diagnostics::check_w110::completed_parse_duplicates(model);
    for slot in slots {
        if let crate::engine::language::model::NamedSlotOwner::HeterogeneousShock(index) =
            slot.owner
            && let Some(block) = model.shock_blocks.get(index)
            && (heterogeneous_shock_row_unknown(model, block)
                || block.stochastic.iter().any(|row| {
                    duplicates
                        .iter()
                        .any(|(_, order)| *order == row.parse_order)
                }))
        {
            continue;
        }
        if !model.heterogeneity_dimensions.iter().any(|dimension| {
            dimension.name == slot.name && dimension.parse_order < slot.callback_order
        }) {
            out.push((
                error(
                    slot.span,
                    "E459",
                    format!("Unknown heterogeneity dimension: {}", model.name(slot.name)),
                ),
                slot.callback_order,
            ));
            if !all {
                return;
            }
        }
    }
}

pub(crate) fn heterogeneous_shock_row_unknown(model: &Model, block: &ShockBlock) -> bool {
    heterogeneous_shock_row_refusal(model, block).is_some()
}

fn heterogeneous_shock_row_refusal(
    model: &Model,
    block: &ShockBlock,
) -> Option<(Diagnostic, usize)> {
    for stmt in &block.stochastic {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let names: Vec<Name> = match &stmt.kind {
            ShockKind::Var(name) | ShockKind::Stderr(name) => vec![*name],
            ShockKind::Cov(names) | ShockKind::Skew(names) => names.clone(),
            ShockKind::Corr { a, b } => vec![*a, *b],
        };
        for name in names {
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            let declared = model
                .symbol_kind_in_context(name, stmt.symbol_type_context)
                .is_some();
            if !declared {
                let order = model.expanded_token_origins.iter().position(|origin| {
                    origin.start <= stmt.parse_order && stmt.parse_order < origin.end
                })?;
                let callback = model
                    .fact_receipts
                    .get("shock_instruction")
                    .and_then(|rows| rows.iter().find(|row| row.parse_order == order))
                    .and_then(|row| row.claims.iter().map(|claim| claim.end).max())
                    .unwrap_or(order);
                return Some((
                    error(
                        stmt.span,
                        "E058",
                        format!("Unknown symbol: {}.", model.name(name)),
                    ),
                    callback,
                ));
            }
        }
    }
    None
}

fn forbidden_symbols(model: &Model, out: &mut Vec<(Diagnostic, usize)>, all: bool) {
    if let Some(expr) = model.planner_objective_expr
        && let Some((span, name, order)) = first_het_ident(model, expr)
    {
        out.push((error(
                span,
                "E461",
                format!(
                    "Symbol '{name}' cannot be used in 'planner_objective', because it is heterogeneous."
                ),
            ), order));
        if !all {
            return;
        }
    }
    for constraint in &model.occbin_constraints {
        if crate::engine::control::cancelled() {
            return;
        }
        for piece in [
            constraint.bind.as_ref(),
            constraint.relax.as_ref(),
            constraint.error_bind.as_ref(),
            constraint.error_relax.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(expr) = piece.expr
                && let Some((span, name, order)) = first_het_ident(model, expr)
            {
                out.push((error(
                        span,
                        "E462",
                        format!(
                            "Symbol '{name}' cannot be used in 'occbin_constraints', because it is heterogeneous."
                        ),
                    ), order));
                if !all {
                    return;
                }
            }
        }
    }
    for assignment in &model.epilogue {
        if crate::engine::control::cancelled() {
            return;
        }
        if let Some(expr) = assignment.expr
            && let Some((span, name, order)) = first_het_ident(model, expr)
        {
            out.push((error(
                    span,
                    "E464",
                    format!(
                        "Symbol '{name}' cannot be used in epilogue block, because it is heterogeneous."
                    ),
                ), order));
            if !all {
                return;
            }
        }
    }
    let outside = outside_exprs(model);
    for (expr, _) in outside {
        if crate::engine::control::cancelled() {
            return;
        }
        if let Some((span, name, order)) = first_het_ident(model, expr) {
            out.push((error(
                span,
                "E463",
                format!(
                    "Symbol '{name}' cannot be used outside model declaration, because it is heterogeneous."
                ),
            ), order));
            if !all {
                return;
            }
        }
    }
}

fn outside_exprs(model: &Model) -> Vec<(ExprId, std::ops::Range<usize>)> {
    let mut out = Vec::new();
    for assignment in model
        .param_assignments
        .iter()
        .chain(&model.helper_assignments)
        .chain(&model.initval)
        .chain(&model.endval)
    {
        if let Some(expr) = assignment.expr {
            out.push((expr, assignment.active_tokens.clone()));
        }
    }
    for eq in &model.steady_state_equations {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        // Output names are checked by the steady-state target action, not by
        // the ordinary-expression heterogeneous-variable rule.
        if let Some(expr) = eq.rhs_expr {
            out.push((expr, eq.active_tokens.clone()));
        }
    }
    out
}

fn sum_operators(model: &Model, out: &mut Vec<(Diagnostic, usize)>, all: bool) {
    if all {
        use crate::engine::language::constructor::DataTreeScope;
        for (id, expr) in model.exprs.iter() {
            if crate::engine::control::cancelled() {
                return;
            }
            let ExprKind::Call { callee, .. } = &expr.kind else {
                continue;
            };
            if !model.name(*callee).eq_ignore_ascii_case("sum") {
                continue;
            }
            let diagnostic = match model.constructor_scopes.get(&id) {
                Some(DataTreeScope::Heterogeneous(_)) => Some(error(
                    expr.span,
                    "E475",
                    "The SUM() operator cannot be used inside a model(heterogeneity=...) block",
                )),
                Some(DataTreeScope::Dynamic) => aggregate_sum_call(model, id),
                _ => None,
            };
            if let Some(diagnostic) = diagnostic {
                out.push((
                    diagnostic,
                    model.constructor_orders.get(&id).copied().unwrap_or(0),
                ));
            }
        }
        return;
    }
    for block in &model.heterogeneous_models {
        if crate::engine::control::cancelled() {
            return;
        }
        for eq in &block.equations {
            if crate::engine::control::cancelled() {
                return;
            }
            if let Some(span) = first_sum_call(model, eq) {
                out.push((
                    error(
                        span,
                        "E475",
                        "The SUM() operator cannot be used inside a model(heterogeneity=...) block",
                    ),
                    crate::engine::diagnostics::parse_preflight::within(
                        model,
                        eq.active_tokens.clone(),
                        span,
                        true,
                    )
                    .unwrap_or(eq.parse_order),
                ));
                if !all {
                    return;
                }
            }
        }
    }
    for eq in &model.equations {
        if crate::engine::control::cancelled() {
            return;
        }
        if let Some(diag) = aggregate_sum(model, eq) {
            out.push((
                diag.clone(),
                crate::engine::diagnostics::parse_preflight::within(
                    model,
                    eq.active_tokens.clone(),
                    diag.span,
                    true,
                )
                .unwrap_or(eq.parse_order),
            ));
            if !all {
                return;
            }
        }
    }
}

fn first_sum_call(model: &Model, eq: &Equation) -> Option<Span> {
    let mut found = None;
    if let Some(expr) = eq.lhs_expr {
        walk_sum_span(model, expr, &mut found);
    }
    if found.is_none()
        && let Some(expr) = eq.rhs_expr
    {
        walk_sum_span(model, expr, &mut found);
    }
    found
}

fn walk_sum_span(model: &Model, id: ExprId, found: &mut Option<Span>) {
    if found.is_some() {
        return;
    }
    let expr = model.exprs.get(id);
    match &expr.kind {
        ExprKind::Call { callee, .. } if model.name(*callee).eq_ignore_ascii_case("sum") => {
            *found = Some(expr.span);
        }
        ExprKind::Unary { arg, .. }
        | ExprKind::SteadyState { arg }
        | ExprKind::Expectation { arg, .. } => {
            walk_sum_span(model, *arg, found);
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            walk_sum_span(model, *lhs, found);
            walk_sum_span(model, *rhs, found);
        }
        ExprKind::Call { args, .. } => {
            for arg in args {
                if crate::engine::control::cancelled() {
                    return Default::default();
                }
                walk_sum_span(model, *arg, found);
            }
        }
        ExprKind::Ident { .. }
        | ExprKind::Number
        | ExprKind::String
        | ExprKind::Error
        | ExprKind::PathNamespace { .. } => {}
    }
}

fn aggregate_sum(model: &Model, eq: &Equation) -> Option<Diagnostic> {
    let mut found = None;
    if let Some(expr) = eq.lhs_expr {
        walk_aggregate_sum(model, expr, &mut found);
    }
    if found.is_none()
        && let Some(expr) = eq.rhs_expr
    {
        walk_aggregate_sum(model, expr, &mut found);
    }
    found
}

fn aggregate_sum_call(model: &Model, id: ExprId) -> Option<Diagnostic> {
    let expr = model.exprs.get(id);
    let ExprKind::Call { args, .. } = &expr.kind else {
        return None;
    };
    let argument = model.sum_arguments.get(&id).copied().unwrap_or_else(|| {
        args.first()
            .and_then(|arg| match model.exprs.get(*arg).kind {
                ExprKind::Ident { name, timing, .. } if args.len() == 1 => {
                    Some((name, timing, is_het_endo(model, *arg)))
                }
                _ => None,
            })
    });
    let (code, message) = if argument.is_none() {
        (
            "E476",
            "The argument to the SUM() operator must be a single variable",
        )
    } else if argument.is_some_and(|(_, timing, _)| timing != 0) {
        (
            "E477",
            "The argument to the SUM() operator must not have a lead or lag",
        )
    } else if !argument.is_some_and(|(_, _, role)| role) {
        (
            "E478",
            "The argument to the SUM() operator must be a heterogeneous endogenous variable",
        )
    } else {
        return None;
    };
    Some(error(expr.span, code, message))
}

fn walk_aggregate_sum(model: &Model, id: ExprId, found: &mut Option<Diagnostic>) {
    if found.is_some() {
        return;
    }
    let expr = model.exprs.get(id);
    match &expr.kind {
        ExprKind::Call { callee, .. } if model.name(*callee).eq_ignore_ascii_case("sum") => {
            *found = aggregate_sum_call(model, id);
        }
        ExprKind::Unary { arg, .. }
        | ExprKind::SteadyState { arg }
        | ExprKind::Expectation { arg, .. } => {
            walk_aggregate_sum(model, *arg, found);
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            walk_aggregate_sum(model, *lhs, found);
            walk_aggregate_sum(model, *rhs, found);
        }
        ExprKind::Call { args, .. } => {
            for arg in args {
                if crate::engine::control::cancelled() {
                    return Default::default();
                }
                walk_aggregate_sum(model, *arg, found);
            }
        }
        ExprKind::Ident { .. }
        | ExprKind::Number
        | ExprKind::String
        | ExprKind::Error
        | ExprKind::PathNamespace { .. } => {}
    }
}

fn shock_types(model: &Model, out: &mut Vec<Diagnostic>) {
    for block in &model.shock_blocks {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if block.kind != ShockBlockKind::Heterogeneous {
            continue;
        }
        for stmt in &block.stochastic {
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            match &stmt.kind {
                ShockKind::Var(name) | ShockKind::Stderr(name)
                    if !is_het_exo_name(model, *name) =>
                {
                    let verb = if matches!(stmt.kind, ShockKind::Var(_)) {
                        "variance"
                    } else {
                        "standard error"
                    };
                    let code = if matches!(stmt.kind, ShockKind::Var(_)) {
                        "E465"
                    } else {
                        "E466"
                    };
                    out.push(error(
                        stmt.span,
                        code,
                        format!(
                            "shocks: setting a {verb} on '{}' is not allowed, because it is not a heterogeneous exogenous variable",
                            model.name(*name)
                        ),
                    ));
                    return;
                }
                ShockKind::Cov(names) => {
                    if names.len() >= 2
                        && !(is_het_exo_name(model, names[0]) && is_het_exo_name(model, names[1]))
                    {
                        out.push(error(
                            stmt.span,
                            "E467",
                            format!(
                                "shocks: setting a covariance between '{}' and '{}'{}",
                                model.name(names[0]),
                                model.name(names[1]),
                                COVAR_TAIL
                            ),
                        ));
                        return;
                    }
                }
                ShockKind::Corr { a, b }
                    if !(is_het_exo_name(model, *a) && is_het_exo_name(model, *b)) =>
                {
                    out.push(error(
                        stmt.span,
                        "E468",
                        format!(
                            "shocks: setting a correlation between '{}' and '{}'{}",
                            model.name(*a),
                            model.name(*b),
                            COVAR_TAIL
                        ),
                    ));
                    return;
                }
                _ => {}
            }
        }
    }
}

const COVAR_TAIL: &str =
    "is not allowed; covariances can only be specified for heterogeneous exogenous variables";

fn equation_timing(model: &Model, out: &mut Vec<Diagnostic>) {
    let lines = LineIndex::new(&model.source);
    for block in &model.heterogeneous_models {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let mut number = 0_u32;
        for eq in &block.equations {
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            if eq.is_local {
                continue;
            }
            number += 1;
            if let Some(diag) = equation_timing_error(model, block, eq, number, &lines) {
                out.push(diag);
                return;
            }
        }
    }
}

fn equation_timing_error(
    model: &Model,
    block: &HeterogeneousModelBlock,
    eq: &Equation,
    number: u32,
    lines: &LineIndex,
) -> Option<Diagnostic> {
    let dim = block.dimension;
    let dim_name = model.name(dim);
    let mut idents = Vec::new();
    if let Some(expr) = eq.lhs_expr {
        collect_idents(model, expr, &mut idents);
    }
    if let Some(expr) = eq.rhs_expr {
        collect_idents(model, expr, &mut idents);
    }
    for (name, timing, span) in &idents {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if het_exo_in(model, *name, dim) && *timing < 0 {
            return Some(error(
                *span,
                "E469",
                format!(
                    "In model(heterogeneity={dim_name}), equation {number}: lagged heterogeneous exogenous variable '{}' is not supported.",
                    model.name(*name)
                ),
            ));
        }
    }
    for (name, timing, span) in &idents {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if het_exo_in(model, *name, dim) && *timing > 0 {
            return Some(error(
                *span,
                "E470",
                format!(
                    "In model(heterogeneity={dim_name}), equation {number}: lead on heterogeneous exogenous variable '{}({})' is not supported.",
                    model.name(*name),
                    showpos(*timing)
                ),
            ));
        }
    }
    for (name, timing, span) in &idents {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if het_endo_in(model, *name, dim) && *timing < -1 {
            return Some(error(
                *span,
                "E471",
                format!(
                    "In model(heterogeneity={dim_name}), equation {number}: heterogeneous endogenous variable '{}' with lag {timing} is not supported (maximum lag is -1).",
                    model.name(*name)
                ),
            ));
        }
    }
    for (name, timing, span) in &idents {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if het_endo_in(model, *name, dim) && *timing > 1 {
            return Some(error(
                *span,
                "E472",
                format!(
                    "In model(heterogeneity={dim_name}), equation {number}: heterogeneous endogenous variable '{}' with lead {timing} is not supported (maximum lead is +1).",
                    model.name(*name)
                ),
            ));
        }
    }
    let root = eq.rhs_expr.or(eq.lhs_expr)?;
    if let Some((span, text)) = nonseparable(model, root, dim) {
        let line = lines.position(&model.source, eq.span.start).line + 1;
        return Some(error(
            span,
            "E473",
            format!(
                "In model(heterogeneity={dim_name}), equation {number} (line {line}):\n  Non-separable expression '{text}'  combines forward-looking variables with lagged states and is not supported."
            ),
        ));
    }
    None
}

fn unsupported_command(model: &Model, out: &mut Vec<Diagnostic>) {
    let Some((span, message)) = first_unsupported(model) else {
        return;
    };
    out.push(error(span, "E474", message));
}

fn first_unsupported(model: &Model) -> Option<(Span, String)> {
    if let Some(span) = model.model_block_option {
        return Some((
            span,
            "the 'block' option of the 'model' block is not supported for heterogeneous models"
                .to_string(),
        ));
    }
    let commands: &[(Option<Span>, &str)] = &[
        (
            model.check_span,
            "The 'check' command is not supported for heterogeneous models",
        ),
        (
            model.steady_span,
            "The 'steady' command is not supported for heterogeneous models",
        ),
        (
            model.perfect_foresight_solver_span,
            "The 'perfect_foresight_solver' command is not supported for heterogeneous models",
        ),
        (
            model.pfee_solver_span,
            "The 'perfect_foresight_with_expectation_errors_solver' command is not supported for heterogeneous models",
        ),
        (
            model.stoch_simul_span,
            "The 'stoch_simul' command is not supported for heterogeneous models",
        ),
        (
            model.estimation_span,
            "The 'estimation' command is not supported for heterogeneous models",
        ),
    ];
    for (span, message) in commands {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if let Some(span) = *span {
            return Some((span, (*message).to_string()));
        }
    }
    if let Some(stmt) = model
        .policy_command_statements
        .iter()
        .find(|stmt| stmt.command == PolicyCommand::Osr)
    {
        return Some((
            keyword_span(&model.source, stmt.span),
            "The 'osr' command is not supported for heterogeneous models".to_string(),
        ));
    }
    if let Some(span) = model.osr_params_span {
        return Some((
            keyword_span(&model.source, span),
            "The 'osr_params' command is not supported for heterogeneous models".to_string(),
        ));
    }
    if let Some(span) = model.optim_weights_span {
        return Some((
            span,
            "The 'optim_weights' block is not supported for heterogeneous models".to_string(),
        ));
    }
    if let Some(stmt) = model
        .policy_command_statements
        .iter()
        .find(|stmt| stmt.command == PolicyCommand::RamseyModel)
    {
        return Some((
            keyword_span(&model.source, stmt.span),
            "The 'ramsey_model' command is not supported for heterogeneous models".to_string(),
        ));
    }
    if let Some(span) = model.discretionary_policy_span {
        return Some((
            span,
            "The 'discretionary_policy' command is not supported for heterogeneous models"
                .to_string(),
        ));
    }
    // `planner_objective`'s heterogeneity sentence is unreachable on this pin:
    // the pairing check exits first, and a legal partner is refused earlier
    // in this same list.
    if let Some(span) = model.extended_path_span {
        return Some((
            span,
            "The 'extended_path' command is not supported for heterogeneous models".to_string(),
        ));
    }
    if let Some(span) = model.identification_span {
        return Some((
            span,
            "The 'identification' command is not supported for heterogeneous models".to_string(),
        ));
    }
    if let Some(span) = model.sensitivity_span {
        return Some((
            span,
            "The 'sensitivity' command is not supported for heterogeneous models".to_string(),
        ));
    }
    if let Some(span) = model.method_of_moments_span {
        return Some((
            span,
            "The 'methods_of_moments' command is not supported for heterogeneous models"
                .to_string(),
        ));
    }
    if let Some(span) = model.occbin_constraints_blocks.first().copied() {
        return Some((
            keyword_span(&model.source, span),
            "The 'occbin_constraints' block is not supported for heterogeneous models".to_string(),
        ));
    }
    None
}

fn keyword_span(source: &str, span: Span) -> Span {
    let start = span.start as usize;
    let mut end = start;
    let bytes = source.as_bytes();
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        end += 1;
    }
    Span {
        start: span.start,
        end: end as u32,
    }
}

fn showpos(timing: i32) -> String {
    if timing > 0 {
        format!("+{timing}")
    } else {
        timing.to_string()
    }
}

fn first_het_ident(model: &Model, id: ExprId) -> Option<(Span, String, usize)> {
    let expression = model.exprs.get(id);
    match &expression.kind {
        ExprKind::Ident {
            name,
            ident_span,
            timing_span,
            ..
        } => model
            .constructor_contexts
            .get(&id)
            .is_some_and(|context| model.heterogeneous_in_context(*name, *context))
            .then(|| {
                (
                    Span {
                        start: ident_span.start,
                        end: timing_span.map_or(ident_span.end, |span| span.end),
                    },
                    model.name(*name).into(),
                    model.constructor_orders.get(&id).copied().unwrap_or(0),
                )
            }),
        ExprKind::Unary { arg, .. }
        | ExprKind::SteadyState { arg }
        | ExprKind::Expectation { arg, .. } => first_het_ident(model, *arg),
        ExprKind::Binary { lhs, rhs, .. } => {
            first_het_ident(model, *lhs).or_else(|| first_het_ident(model, *rhs))
        }
        ExprKind::Call { args, .. } => args.iter().find_map(|arg| first_het_ident(model, *arg)),
        _ => None,
    }
}

fn collect_idents(model: &Model, id: ExprId, out: &mut Vec<(Name, i32, Span)>) {
    let expr = model.exprs.get(id);
    match &expr.kind {
        ExprKind::Ident {
            name,
            timing,
            ident_span,
            timing_span,
        } => {
            let end = timing_span.map(|t| t.end).unwrap_or(ident_span.end);
            out.push((
                *name,
                *timing,
                Span {
                    start: ident_span.start,
                    end,
                },
            ));
        }
        ExprKind::Unary { arg, .. }
        | ExprKind::SteadyState { arg }
        | ExprKind::Expectation { arg, .. } => collect_idents(model, *arg, out),
        ExprKind::Binary { lhs, rhs, .. } => {
            collect_idents(model, *lhs, out);
            collect_idents(model, *rhs, out);
        }
        ExprKind::Call { args, .. } => {
            for arg in args {
                if crate::engine::control::cancelled() {
                    return Default::default();
                }
                collect_idents(model, *arg, out);
            }
        }
        ExprKind::Number | ExprKind::String | ExprKind::Error | ExprKind::PathNamespace { .. } => {}
    }
}

fn nonseparable(model: &Model, id: ExprId, dim: Name) -> Option<(Span, String)> {
    let expr = model.exprs.get(id);
    match &expr.kind {
        ExprKind::Ident { .. }
        | ExprKind::Number
        | ExprKind::String
        | ExprKind::Error
        | ExprKind::PathNamespace { .. } => None,
        ExprKind::Unary { arg, .. }
        | ExprKind::SteadyState { arg }
        | ExprKind::Expectation { arg, .. } => {
            if let Some(found) = nonseparable(model, *arg, dim) {
                return Some(found);
            }
            if max_endo_lead(model, *arg, dim) >= 1 && max_endo_lag(model, *arg, dim) >= 1 {
                Some((expr.span, json_text(model, id)))
            } else {
                None
            }
        }
        ExprKind::Call { args, .. } => {
            for arg in args {
                if crate::engine::control::cancelled() {
                    return Default::default();
                }
                if let Some(found) = nonseparable(model, *arg, dim) {
                    return Some(found);
                }
            }
            let lead = args
                .iter()
                .map(|arg| max_endo_lead(model, *arg, dim))
                .max()
                .unwrap_or(0);
            let lag = args
                .iter()
                .map(|arg| max_endo_lag(model, *arg, dim))
                .max()
                .unwrap_or(0);
            if lead >= 1 && lag >= 1 {
                Some((expr.span, json_text(model, id)))
            } else {
                None
            }
        }
        ExprKind::Binary { op, lhs, rhs } => {
            if let Some(found) = nonseparable(model, *lhs, dim) {
                return Some(found);
            }
            if let Some(found) = nonseparable(model, *rhs, dim) {
                return Some(found);
            }
            let lead1 = max_endo_lead(model, *lhs, dim);
            let lead2 = max_endo_lead(model, *rhs, dim);
            let lag = max_endo_lag(model, *lhs, dim).max(max_endo_lag(model, *rhs, dim));
            if lead1 < 1 && lead2 < 1 {
                return None;
            }
            let separable = match op {
                BinOp::Add | BinOp::Sub | BinOp::EqEq => true,
                BinOp::Mul | BinOp::Div => {
                    (lead1 >= 1 && lead2 == 0 && max_exo_lead(model, *rhs, dim) == 0)
                        || (*op == BinOp::Mul
                            && lead1 == 0
                            && max_exo_lead(model, *lhs, dim) == 0
                            && lead2 >= 1)
                }
                _ => false,
            };
            if separable {
                None
            } else if lag >= 1 {
                Some((expr.span, json_text(model, id)))
            } else {
                None
            }
        }
    }
}

fn max_endo_lead(model: &Model, id: ExprId, dim: Name) -> i32 {
    max_timing(model, id, dim, true, true)
}

fn max_endo_lag(model: &Model, id: ExprId, dim: Name) -> i32 {
    max_timing(model, id, dim, true, false)
}

fn max_exo_lead(model: &Model, id: ExprId, dim: Name) -> i32 {
    max_timing(model, id, dim, false, true)
}

fn max_timing(model: &Model, id: ExprId, dim: Name, endo: bool, lead: bool) -> i32 {
    let expr = model.exprs.get(id);
    match &expr.kind {
        ExprKind::Ident { name, timing, .. } => {
            let mine = if endo {
                het_endo_in(model, *name, dim)
            } else {
                het_exo_in(model, *name, dim)
            };
            if !mine {
                0
            } else if lead {
                (*timing).max(0)
            } else {
                (-timing).max(0)
            }
        }
        ExprKind::Unary { arg, .. }
        | ExprKind::SteadyState { arg }
        | ExprKind::Expectation { arg, .. } => max_timing(model, *arg, dim, endo, lead),
        ExprKind::Binary { lhs, rhs, .. } => {
            max_timing(model, *lhs, dim, endo, lead).max(max_timing(model, *rhs, dim, endo, lead))
        }
        ExprKind::Call { args, .. } => args
            .iter()
            .map(|arg| max_timing(model, *arg, dim, endo, lead))
            .max()
            .unwrap_or(0),
        ExprKind::Number | ExprKind::String | ExprKind::Error | ExprKind::PathNamespace { .. } => 0,
    }
}

fn json_text(model: &Model, id: ExprId) -> String {
    let expr = model.exprs.get(id);
    match &expr.kind {
        ExprKind::Ident { name, timing, .. } => {
            if *timing == 0 {
                model.name(*name).to_string()
            } else {
                format!("{}({})", model.name(*name), timing)
            }
        }
        ExprKind::Number => {
            model.source[expr.span.start as usize..expr.span.end as usize].to_string()
        }
        ExprKind::Call { callee, args } => {
            let args = args
                .iter()
                .map(|arg| json_text(model, *arg))
                .collect::<Vec<_>>()
                .join(",");
            format!("{}({args})", model.name(*callee))
        }
        ExprKind::Binary { op, lhs, rhs } => {
            let symbol = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Pow => "^",
                BinOp::Lt => "<",
                BinOp::Gt => ">",
                BinOp::Le => "<=",
                BinOp::Ge => ">=",
                BinOp::EqEq => "==",
                BinOp::Ne => "!=",
            };
            format!(
                "{}{symbol}{}",
                json_text(model, *lhs),
                json_text(model, *rhs)
            )
        }
        ExprKind::Unary { arg, .. } => format!("-{}", json_text(model, *arg)),
        ExprKind::SteadyState { arg } => format!("STEADY_STATE({})", json_text(model, *arg)),
        ExprKind::Expectation { arg, .. } => json_text(model, *arg),
        ExprKind::String | ExprKind::Error | ExprKind::PathNamespace { .. } => String::new(),
    }
}

fn is_het_endo(model: &Model, id: ExprId) -> bool {
    match &model.exprs.get(id).kind {
        ExprKind::Ident { name, .. } => model.is_heterogeneous_endogenous(*name),
        _ => false,
    }
}

fn is_het_exo_name(model: &Model, name: Name) -> bool {
    model
        .exogenous
        .iter()
        .any(|decl| decl.name == name && model.final_heterogeneity(decl).is_some())
}

fn het_endo_in(model: &Model, name: Name, dim: Name) -> bool {
    model
        .endogenous
        .iter()
        .any(|decl| decl.name == name && model.final_heterogeneity(decl) == Some(dim))
}

fn het_exo_in(model: &Model, name: Name, dim: Name) -> bool {
    model
        .exogenous
        .iter()
        .any(|decl| decl.name == name && model.final_heterogeneity(decl) == Some(dim))
}

fn symbol_declared(model: &Model, name: &str) -> bool {
    model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.deterministic_exogenous)
        .chain(&model.parameters)
        .chain(&model.model_local_variables)
        .any(|decl| model.name(decl.name) == name)
}

fn error(span: Span, code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(span, Severity::Error, code, message)
}

#[cfg(test)]
mod parse_receipt_tests {
    use super::*;
    use crate::engine::language::parser::parse;

    // A later dimension declaration must not validate the earlier lookup;
    // SUM's refusal belongs to its closing call, while Check-only failures
    // must not stop an otherwise accepted Parse prefix.
    #[test]
    fn parse_receipts_keep_callback_order_and_exclude_check_failures() {
        let model = parse("var(heterogeneity=later) x;heterogeneity_dimension later;");
        let rows = parse_refusals(&model);
        let (_, order) = rows.iter().find(|(row, _)| row.code == "E459").unwrap();
        assert!(*order < model.heterogeneity_dimensions[0].parse_order);

        let model = parse(include_str!(
            "../../../tests/fixtures/d_hank/e475_sum_het.mod"
        ));
        let (_, order) = parse_refusals(&model)
            .into_iter()
            .find(|(row, _)| row.code == "E475")
            .unwrap();
        assert_eq!(model.expanded_tokens[order - 1].text(&model.source), ")");

        let model = parse(include_str!(
            "../../../tests/fixtures/d_hank/e474_stoch.mod"
        ));
        assert!(check_check(&model).iter().any(|row| row.code == "E474"));
        assert!(parse_refusals(&model).is_empty());
    }
}
