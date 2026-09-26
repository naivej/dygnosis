//! Heterogeneity refusals for Dynare 7.2.
//!
//! Parse-stage errors (`E459`–`E464`, `E475`–`E478`) stop later check-stage
//! errors. `E479` is also a parse refusal, but it is reported with the obsolete
//! `mcp` warning. `W207` is ours: it stays on an accepted two-dimension file
//! and beside check-stage errors.

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, Severity};
use crate::expr::{BinOp, ExprId, ExprKind};
use crate::intern::Name;
use crate::model::{
    Decl, Equation, HeterogeneousModelBlock, Model, PolicyCommand, ShockBlock, ShockBlockKind,
    ShockKind,
};
use crate::span::{LineIndex, Span};

const W207: &str = "Dynare 7.2 cannot load or compute a heterogeneous steady state with more than one heterogeneity dimension.";

pub fn check_parse(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    unknown_and_repeated_dimensions(model, &mut out);
    forbidden_symbols(model, &mut out);
    sum_operators(model, &mut out);
    out
}

pub fn check_mcp(model: &Model) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for block in &model.heterogeneous_models {
        for eq in &block.equations {
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
            out.push(error(
                eq.span,
                "E479",
                "'mcp' tags are not allowed in heterogeneous model blocks",
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
    if crate::check_writing::model_structure_incomplete(model) {
        return Vec::new();
    }
    struct Side {
        count: usize,
        span: Span,
    }
    let mut seen = HashSet::new();
    let mut endogenous: Vec<(String, Side)> = Vec::new();
    for decl in &model.endogenous {
        let Some((dim, _)) = decl.heterogeneity else {
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
        if !names.iter().any(|have| have == name) {
            names.push(name.clone());
        }
    }
    let mut out = Vec::new();
    for name in names {
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
        ));
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

fn unknown_and_repeated_dimensions(model: &Model, out: &mut Vec<Diagnostic>) {
    let mut seen = HashSet::new();
    for dim in &model.heterogeneity_dimensions {
        if !seen.insert(dim.name) {
            out.push(error(
                dim.name_span,
                "E460",
                format!(
                    "Heterogeneity dimension '{}' already declared",
                    model.name(dim.name)
                ),
            ));
            return;
        }
    }
    let known: HashSet<Name> = model
        .heterogeneity_dimensions
        .iter()
        .map(|dim| dim.name)
        .collect();
    for decl in model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.parameters)
    {
        if let Some((name, span)) = decl.heterogeneity {
            if !known.contains(&name) {
                out.push(error(
                    span,
                    "E459",
                    format!("Unknown heterogeneity dimension: {}", model.name(name)),
                ));
                return;
            }
        }
    }
    for block in &model.heterogeneous_models {
        if !known.contains(&block.dimension) {
            out.push(error(
                block.dimension_span,
                "E459",
                format!(
                    "Unknown heterogeneity dimension: {}",
                    model.name(block.dimension)
                ),
            ));
            return;
        }
    }
    for shock in &model.shock_blocks {
        if shock.kind != ShockBlockKind::Heterogeneous {
            continue;
        }
        if heterogeneous_shock_row_unknown(model, shock) {
            // The binary reports `Unknown symbol` and exits before the
            // dimension sentence when the row name is also unknown.
            continue;
        }
        if let Some((name, span)) = shock.options.heterogeneity {
            if !known.contains(&name) {
                out.push(error(
                    span,
                    "E459",
                    format!("Unknown heterogeneity dimension: {}", model.name(name)),
                ));
                return;
            }
        }
    }
}

fn heterogeneous_shock_row_unknown(model: &Model, block: &ShockBlock) -> bool {
    for stmt in &block.stochastic {
        let names: Vec<Name> = match &stmt.kind {
            ShockKind::Var(name) | ShockKind::Stderr(name) => vec![*name],
            ShockKind::Cov(names) | ShockKind::Skew(names) => names.clone(),
            ShockKind::Corr { a, b } => vec![*a, *b],
        };
        for name in names {
            let declared = model
                .endogenous
                .iter()
                .chain(&model.exogenous)
                .chain(&model.deterministic_exogenous)
                .chain(&model.parameters)
                .any(|decl| decl.name == name && decl.span.start < stmt.span.start);
            if !declared {
                return true;
            }
        }
    }
    false
}

fn forbidden_symbols(model: &Model, out: &mut Vec<Diagnostic>) {
    if let Some(expr) = model.planner_objective_expr {
        if let Some((span, name)) = first_het_ident(model, expr) {
            out.push(error(
                span,
                "E461",
                format!(
                    "Symbol '{name}' cannot be used in 'planner_objective', because it is heterogeneous."
                ),
            ));
            return;
        }
    }
    for constraint in &model.occbin_constraints {
        for piece in [
            constraint.bind.as_ref(),
            constraint.relax.as_ref(),
            constraint.error_bind.as_ref(),
            constraint.error_relax.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(expr) = piece.expr {
                if let Some((span, name)) = first_het_ident(model, expr) {
                    out.push(error(
                        span,
                        "E462",
                        format!(
                            "Symbol '{name}' cannot be used in 'occbin_constraints', because it is heterogeneous."
                        ),
                    ));
                    return;
                }
            }
        }
    }
    for assignment in &model.epilogue {
        if let Some(expr) = assignment.expr {
            if let Some((span, name)) = first_het_ident(model, expr) {
                out.push(error(
                    span,
                    "E464",
                    format!(
                        "Symbol '{name}' cannot be used in epilogue block, because it is heterogeneous."
                    ),
                ));
                return;
            }
        }
    }
    let outside = outside_exprs(model);
    for expr in outside {
        if let Some((span, name)) = first_het_ident(model, expr) {
            out.push(error(
                span,
                "E463",
                format!(
                    "Symbol '{name}' cannot be used outside model declaration, because it is heterogeneous."
                ),
            ));
            return;
        }
    }
}

fn outside_exprs(model: &Model) -> Vec<ExprId> {
    let mut out = Vec::new();
    for assignment in model
        .param_assignments
        .iter()
        .chain(&model.helper_assignments)
        .chain(&model.initval)
        .chain(&model.endval)
    {
        if let Some(expr) = assignment.expr {
            out.push(expr);
        }
    }
    for eq in &model.steady_state_equations {
        if let Some(expr) = eq.lhs_expr {
            out.push(expr);
        }
        if let Some(expr) = eq.rhs_expr {
            out.push(expr);
        }
    }
    out
}

fn sum_operators(model: &Model, out: &mut Vec<Diagnostic>) {
    for block in &model.heterogeneous_models {
        for eq in &block.equations {
            if let Some(span) = first_sum_call(model, eq) {
                out.push(error(
                    span,
                    "E475",
                    "The SUM() operator cannot be used inside a model(heterogeneity=...) block",
                ));
                return;
            }
        }
    }
    for eq in &model.equations {
        if let Some(diag) = aggregate_sum(model, eq) {
            out.push(diag);
            return;
        }
    }
}

fn first_sum_call(model: &Model, eq: &Equation) -> Option<Span> {
    let mut found = None;
    if let Some(expr) = eq.lhs_expr {
        walk_sum_span(model, expr, &mut found);
    }
    if found.is_none() {
        if let Some(expr) = eq.rhs_expr {
            walk_sum_span(model, expr, &mut found);
        }
    }
    found
}

fn walk_sum_span(model: &Model, id: ExprId, found: &mut Option<Span>) {
    if found.is_some() {
        return;
    }
    let expr = model.exprs.get(id);
    match &expr.kind {
        ExprKind::Call { callee, args } if model.name(*callee).eq_ignore_ascii_case("sum") => {
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
                walk_sum_span(model, *arg, found);
            }
        }
        ExprKind::Ident { .. } | ExprKind::Number | ExprKind::String | ExprKind::Error => {}
    }
}

fn aggregate_sum(model: &Model, eq: &Equation) -> Option<Diagnostic> {
    let mut found = None;
    if let Some(expr) = eq.lhs_expr {
        walk_aggregate_sum(model, expr, &mut found);
    }
    if found.is_none() {
        if let Some(expr) = eq.rhs_expr {
            walk_aggregate_sum(model, expr, &mut found);
        }
    }
    found
}

fn walk_aggregate_sum(model: &Model, id: ExprId, found: &mut Option<Diagnostic>) {
    if found.is_some() {
        return;
    }
    let expr = model.exprs.get(id);
    match &expr.kind {
        ExprKind::Call { callee, args } if model.name(*callee).eq_ignore_ascii_case("sum") => {
            let diag = if args.len() != 1 || !is_plain_ident(model, args[0]) {
                error(
                    expr.span,
                    "E476",
                    "The argument to the SUM() operator must be a single variable",
                )
            } else if ident_timing(model, args[0]) != 0 {
                error(
                    expr.span,
                    "E477",
                    "The argument to the SUM() operator must not have a lead or lag",
                )
            } else if !is_het_endo(model, args[0]) {
                error(
                    expr.span,
                    "E478",
                    "The argument to the SUM() operator must be a heterogeneous endogenous variable",
                )
            } else {
                return;
            };
            *found = Some(diag);
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
                walk_aggregate_sum(model, *arg, found);
            }
        }
        ExprKind::Ident { .. } | ExprKind::Number | ExprKind::String | ExprKind::Error => {}
    }
}

fn shock_types(model: &Model, out: &mut Vec<Diagnostic>) {
    for block in &model.shock_blocks {
        if block.kind != ShockBlockKind::Heterogeneous {
            continue;
        }
        for stmt in &block.stochastic {
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
        let mut number = 0_u32;
        for eq in &block.equations {
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

fn first_het_ident(model: &Model, id: ExprId) -> Option<(Span, String)> {
    let mut idents = Vec::new();
    collect_idents(model, id, &mut idents);
    idents.into_iter().find_map(|(name, _, span)| {
        if is_het_symbol(model, name) {
            Some((span, model.name(name).to_string()))
        } else {
            None
        }
    })
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
                collect_idents(model, *arg, out);
            }
        }
        ExprKind::Number | ExprKind::String | ExprKind::Error => {}
    }
}

fn nonseparable(model: &Model, id: ExprId, dim: Name) -> Option<(Span, String)> {
    let expr = model.exprs.get(id);
    match &expr.kind {
        ExprKind::Ident { .. } | ExprKind::Number | ExprKind::String | ExprKind::Error => None,
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
        ExprKind::Number | ExprKind::String | ExprKind::Error => 0,
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
        ExprKind::String | ExprKind::Error => String::new(),
    }
}

fn is_plain_ident(model: &Model, id: ExprId) -> bool {
    matches!(model.exprs.get(id).kind, ExprKind::Ident { .. })
}

fn ident_timing(model: &Model, id: ExprId) -> i32 {
    match &model.exprs.get(id).kind {
        ExprKind::Ident { timing, .. } => *timing,
        _ => 0,
    }
}

fn is_het_endo(model: &Model, id: ExprId) -> bool {
    match &model.exprs.get(id).kind {
        ExprKind::Ident { name, .. } => model
            .endogenous
            .iter()
            .any(|decl| decl.name == *name && decl.heterogeneity.is_some()),
        _ => false,
    }
}

fn is_het_symbol(model: &Model, name: Name) -> bool {
    decl_with_het(model, name).is_some()
}

fn is_het_exo_name(model: &Model, name: Name) -> bool {
    model
        .exogenous
        .iter()
        .any(|decl| decl.name == name && decl.heterogeneity.is_some())
}

fn het_endo_in(model: &Model, name: Name, dim: Name) -> bool {
    model.endogenous.iter().any(|decl| {
        decl.name == name && decl.heterogeneity.map(|(dimension, _)| dimension) == Some(dim)
    })
}

fn het_exo_in(model: &Model, name: Name, dim: Name) -> bool {
    model.exogenous.iter().any(|decl| {
        decl.name == name && decl.heterogeneity.map(|(dimension, _)| dimension) == Some(dim)
    })
}

fn decl_with_het(model: &Model, name: Name) -> Option<&Decl> {
    model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.parameters)
        .find(|decl| decl.name == name && decl.heterogeneity.is_some())
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
