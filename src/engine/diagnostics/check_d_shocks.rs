//! Dynare 7.2 written shock and path refusals. The parser keeps these forms
//! separate from stochastic `ShockStmt` checks.

use std::collections::{HashMap, HashSet};

use crate::engine::diagnostics::{Diagnostic, RelatedDiagnostic, Severity};
use crate::engine::language::intern::Name;
use crate::engine::language::model::{
    Model, PathStanza, PathTarget, PeriodPoint, PeriodRange, ShockBlockKind, ShockKind,
    ShockOperation,
};
use crate::engine::language::span::Span;

/// Diagnostics and exact producing execution owners travel together. Display
/// spans stay unchanged; they cannot identify a repeated macro execution.
struct OrderedRows<'a> {
    model: &'a Model,
    diagnostics: Vec<Diagnostic>,
    orders: Vec<usize>,
    owner: std::ops::Range<usize>,
    callback: Option<usize>,
}

impl<'a> OrderedRows<'a> {
    fn new(model: &'a Model) -> Self {
        Self {
            model,
            diagnostics: Vec::new(),
            orders: Vec::new(),
            owner: 0..0,
            callback: None,
        }
    }
    fn own(&mut self, key: &'static str, index: usize) {
        let receipt = self
            .model
            .fact_receipts
            .get(key)
            .and_then(|rows| rows.get(index));
        self.owner = receipt
            .and_then(|row| row.claims.first())
            .cloned()
            .unwrap_or(0..0);
        self.callback = None;
    }
    fn push(&mut self, diagnostic: Diagnostic) {
        let order = self
            .callback
            .or_else(|| {
                crate::engine::diagnostics::parse_preflight::within(
                    self.model,
                    self.owner.clone(),
                    diagnostic.span,
                    true,
                )
            })
            .unwrap_or(self.owner.start);
        self.orders.push(order);
        self.diagnostics.push(diagnostic);
    }
}

impl std::ops::Deref for OrderedRows<'_> {
    type Target = Vec<Diagnostic>;
    fn deref(&self) -> &Self::Target {
        &self.diagnostics
    }
}

fn error(out: &mut OrderedRows<'_>, span: Span, code: &str, message: impl Into<String>) {
    out.push(Diagnostic::new(span, Severity::Error, code, message));
}

fn known(
    model: &Model,
    name: Name,
    _span: Span,
    context: crate::engine::language::model::SymbolContext,
) -> bool {
    model.symbol_kind_in_context(name, context).is_some()
}

fn has_kind(
    model: &Model,
    name: Name,
    context: crate::engine::language::model::SymbolContext,
    kind: &str,
) -> bool {
    model.symbol_kind_in_context(name, context) == Some(kind)
}

fn exogenous(
    model: &Model,
    out: &mut OrderedRows<'_>,
    name: Name,
    at: (Span, crate::engine::language::model::SymbolContext),
    allow_det: bool,
) -> bool {
    let (span, context) = at;
    if known(model, name, span, context)
        && (has_kind(model, name, context, "varexo")
            || (allow_det && has_kind(model, name, context, "varexo_det")))
    {
        return true;
    }
    let n = model.name(name);
    if !known(model, name, span, context) {
        error(out, span, "E058", format!("Unknown symbol: {n}."));
    } else if has_kind(model, name, context, "varexo_det") {
        error(
            out,
            span,
            "E317",
            format!("{n} is an exogenous deterministic."),
        );
    } else {
        error(out, span, "E387", format!("{n} is not exogenous."));
    }
    false
}

fn endogenous(
    model: &Model,
    out: &mut OrderedRows<'_>,
    name: Name,
    at: (Span, crate::engine::language::model::SymbolContext),
) -> bool {
    let (span, context) = at;
    if known(model, name, span, context) && has_kind(model, name, context, "var") {
        return true;
    }
    let n = model.name(name);
    if !known(model, name, span, context) {
        error(out, span, "E058", format!("Unknown symbol: {n}."));
    } else {
        error(out, span, "E317", format!("{n} is not endogenous."));
    }
    false
}

pub fn check_d_shocks(model: &Model) -> Vec<Diagnostic> {
    ordered_rows(model, false).diagnostics
}

pub(crate) fn parse_refusals(model: &Model) -> Vec<(Diagnostic, usize)> {
    let rows = ordered_rows(model, true);
    rows.diagnostics
        .into_iter()
        .zip(rows.orders)
        .filter(|(diagnostic, _)| !matches!(diagnostic.code.as_str(), "E420" | "E271"))
        .collect()
}

fn ordered_rows(model: &Model, all_owners: bool) -> OrderedRows<'_> {
    let mut out = OrderedRows::new(model);
    for (index, (span, code, message)) in model.path_parse_errors.iter().enumerate() {
        out.callback = Some(crate::engine::diagnostics::parse_preflight::callback_order(
            model,
            "path_error_callback",
            index,
        ));
        if crate::engine::control::cancelled() {
            return OrderedRows::new(model);
        }
        error(&mut out, *span, code, message);
    }
    for (name, span) in &model.option_twice {
        if crate::engine::control::cancelled() {
            return OrderedRows::new(model);
        }
        let shock_option = model
            .shock_paths
            .iter()
            .any(|block| block.span.start <= span.start && span.end <= block.span.end)
            || model.shock_blocks.iter().any(|block| {
                block.kind == ShockBlockKind::Multiplicative
                    && block.span.start <= span.start
                    && span.end <= block.span.end
            });
        if shock_option {
            error(
                &mut out,
                *span,
                "E271",
                format!("The '{name}' option is declared multiple times"),
            );
        }
        if model
            .stoch_simul_requests
            .iter()
            .any(|request| request.span.start <= span.start && span.end <= request.span.end)
        {
            error(
                &mut out,
                *span,
                "E271",
                format!("option {name} declared twice"),
            );
        }
    }
    if !out.is_empty() && !all_owners {
        return out;
    }
    check_irf_shocks_options(model, &mut out);
    check_stochastic_names(model, &mut out);
    check_scheduled(model, &mut out);
    check_endval(model, &mut out);
    check_databases(model, &mut out);
    check_paths(model, &mut out);
    out
}

fn check_stochastic_names(model: &Model, out: &mut OrderedRows<'_>) {
    for (index, block) in model.shock_blocks.iter().enumerate() {
        out.own("shock_block_callback", index);
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if block.kind == ShockBlockKind::Heterogeneous {
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
                    if !known(model, name, stmt.span, stmt.symbol_type_context) {
                        error(
                            out,
                            stmt.span,
                            "E058",
                            format!("Unknown symbol: {}.", model.name(name)),
                        );
                        return;
                    }
                }
            }
            continue;
        }
        if block.kind != ShockBlockKind::Regular {
            continue;
        }
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
                if !known(model, name, stmt.span, stmt.symbol_type_context) {
                    error(
                        out,
                        stmt.span,
                        "E058",
                        format!("Unknown symbol: {}.", model.name(name)),
                    );
                    break;
                }
            }
        }
    }
}

/// Completed regular rows whose symbol action refuses during Parse. Keep the
/// row's execution position; a written span alone cannot identify a macro copy.
pub(crate) fn completed_regular_parse_unknowns(model: &Model) -> Vec<(Diagnostic, usize)> {
    let mut out = Vec::new();
    for row in &model.shock_stmts {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let names: Vec<Name> = match &row.kind {
            ShockKind::Var(name) | ShockKind::Stderr(name) => vec![*name],
            ShockKind::Cov(names) | ShockKind::Skew(names) => names.clone(),
            ShockKind::Corr { a, b } => vec![*a, *b],
        };
        for name in names {
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            if !known(model, name, row.span, row.symbol_type_context) {
                out.push((
                    Diagnostic::new(
                        row.span,
                        Severity::Error,
                        "E058",
                        format!("Unknown symbol: {}.", model.name(name)),
                    ),
                    row.parse_order,
                ));
                break;
            }
        }
    }
    out
}

fn check_irf_shocks_options(model: &Model, out: &mut OrderedRows<'_>) {
    for option in &model.irf_shocks_options {
        out.owner = option.active_tokens.clone();
        out.callback = None;
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        for &(name, span) in &option.names {
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            if !known(model, name, span, option.symbol_type_context) {
                error(
                    out,
                    span,
                    "E058",
                    format!("Unknown symbol: {}", model.name(name)),
                );
            } else if !has_kind(model, name, option.symbol_type_context, "varexo") {
                error(
                    out,
                    span,
                    "E240",
                    format!(
                        "Variables passed to irf_shocks must be exogenous. Caused by: {}",
                        model.name(name)
                    ),
                );
            }
        }
    }
}

fn check_range(out: &mut OrderedRows<'_>, range: &PeriodRange) {
    if let (PeriodPoint::Integer(first), Some(PeriodPoint::Integer(last))) =
        (&range.first, &range.last)
        && first > last
    {
        error(
            out,
            range.span,
            "E395",
            "Can't have first period index greater than second index in range specification",
        );
    }
}

fn check_scheduled(model: &Model, out: &mut OrderedRows<'_>) {
    for (index, block) in model.shock_blocks.iter().enumerate() {
        out.own("shock_block_callback", index);
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if block.kind == ShockBlockKind::Heterogeneous {
            continue;
        }
        let mut seen = HashMap::new();
        let mut seen_hetero = HashMap::new();
        let learnt = block.options.learnt_in.as_ref();
        if let Some(PeriodPoint::Integer(n)) = learnt
            && *n < 1
        {
            out.callback = Some(crate::engine::diagnostics::parse_preflight::callback_order(
                model,
                "shock_block_callback",
                index,
            ));
            let command = if block.kind == ShockBlockKind::Multiplicative {
                "mshocks"
            } else {
                "shocks"
            };
            error(
                out,
                block.options.learnt_in_span.unwrap_or(block.span),
                "E400",
                format!("{command}: value '{n}' is not allowed for 'learnt_in' option"),
            );
        }
        out.callback = None;
        for row in &block.scheduled {
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            let allow_det = block.kind != ShockBlockKind::Heteroskedastic
                && row.operation == ShockOperation::Values;
            exogenous(
                model,
                out,
                row.name,
                (row.name_span, row.symbol_type_context),
                allow_det,
            );
            for range in &row.periods {
                if crate::engine::control::cancelled() {
                    return Default::default();
                }
                check_range(out, range);
            }
            if block.kind == ShockBlockKind::Heteroskedastic {
                if let Some(&first) = seen_hetero.get(&(row.name, row.operation as u8)) {
                    out.push(
                        Diagnostic::new(
                            row.span,
                            Severity::Error,
                            "E402",
                            format!(
                                "heteroskedastic_shocks: variable {} declared twice",
                                model.name(row.name)
                            ),
                        )
                        .with_related(RelatedDiagnostic::new(
                            first,
                            "Earlier heteroskedastic shock entry",
                        )),
                    );
                } else {
                    seen_hetero.insert((row.name, row.operation as u8), row.span);
                }
                if row.periods.len() != row.values.len() {
                    error(
                        out,
                        row.span,
                        "E403",
                        format!(
                            "heteroskedastic_shocks: variable {}: number of periods is different from number of shock values",
                            model.name(row.name)
                        ),
                    );
                }
                continue;
            }
            if let Some(&first) = seen.get(&row.name) {
                out.push(
                    Diagnostic::new(
                        row.span,
                        Severity::Error,
                        "E344",
                        format!(
                            "shocks/conditional_forecast_paths: variable {} declared twice",
                            model.name(row.name)
                        ),
                    )
                    .with_related(RelatedDiagnostic::new(
                        first,
                        "Earlier scheduled shock entry",
                    )),
                );
            } else {
                seen.insert(row.name, row.span);
            }
            if row.periods.len() != row.values.len() {
                error(
                    out,
                    row.span,
                    "E343",
                    format!(
                        "shocks/conditional_forecast_paths: variable {}: number of periods is different from number of shock values",
                        model.name(row.name)
                    ),
                );
            }
            if block.kind == ShockBlockKind::Surprise
                && row.periods.iter().any(|range| {
                    matches!(&range.first, PeriodPoint::Date(_))
                        || matches!(&range.last, Some(PeriodPoint::Date(_)))
                })
            {
                error(
                    out,
                    row.span,
                    "E399",
                    "shocks(surprise): dates are not allowed in the 'periods' keyword",
                );
            }
            if let Some(PeriodPoint::Integer(n)) = learnt
                && *n > 1
            {
                for range in &row.periods {
                    if crate::engine::control::cancelled() {
                        return Default::default();
                    }
                    if let PeriodPoint::Integer(first) = &range.first
                        && first < n
                    {
                        let command = if block.kind == ShockBlockKind::Multiplicative {
                            "mshocks"
                        } else {
                            "shocks"
                        };
                        error(
                            out,
                            range.span,
                            "E401",
                            format!(
                                "{command}: for variable {}, shock period ({first}) is earlier than the period in which the shock is learnt ({n})",
                                model.name(row.name)
                            ),
                        );
                        break;
                    }
                }
            }
            let command = match block.kind {
                ShockBlockKind::Regular if row.operation != ShockOperation::Values => Some((
                    "E396",
                    format!(
                        "shocks: '{}' keyword not allowed unless 'learnt_in' option with value >1 is passed",
                        operation_word(row.operation)
                    ),
                )),
                ShockBlockKind::LearntIn
                    if matches!(learnt, Some(PeriodPoint::Integer(1)))
                        && row.operation != ShockOperation::Values =>
                {
                    Some((
                        "E396",
                        format!(
                            "shocks: '{}' keyword not allowed unless 'learnt_in' option with value >1 is passed",
                            operation_word(row.operation)
                        ),
                    ))
                }
                ShockBlockKind::Multiplicative if row.operation != ShockOperation::Values => {
                    Some((
                        "E397",
                        format!(
                            "mshocks: '{}' keyword not allowed",
                            operation_word(row.operation)
                        ),
                    ))
                }
                ShockBlockKind::Surprise if row.operation != ShockOperation::Values => Some((
                    "E398",
                    format!(
                        "shocks(surprise): '{}' keyword not allowed",
                        operation_word(row.operation)
                    ),
                )),
                _ => None,
            };
            if let Some((code, message)) = command {
                error(out, row.span, code, message);
            }
        }
    }
}

fn operation_word(operation: ShockOperation) -> &'static str {
    match operation {
        ShockOperation::Add => "add",
        ShockOperation::Multiply => "multiply",
        ShockOperation::Values => "values",
        ShockOperation::Scales => "scales",
    }
}

fn check_endval(model: &Model, out: &mut OrderedRows<'_>) {
    for (index, block) in model.endval_instructions.iter().enumerate() {
        out.own("endval_instruction_callback", index);
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let learnt = block.learnt_in.as_ref();
        if let Some(PeriodPoint::Integer(n)) = learnt
            && *n < 1
        {
            out.callback = Some(crate::engine::diagnostics::parse_preflight::callback_order(
                model,
                "endval_instruction_callback",
                index,
            ));
            error(
                out,
                block.learnt_in_span.unwrap_or(block.span),
                "E418",
                format!("endval: value '{n}' is not allowed for 'learnt_in' option"),
            );
        }
        out.callback = None;
        let nondefault = matches!(learnt, Some(PeriodPoint::Date(_)))
            || matches!(learnt, Some(PeriodPoint::Integer(n)) if *n > 1);
        for entry in &block.entries {
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            if nondefault
                && known(
                    model,
                    entry.name,
                    entry.name_span,
                    entry.symbol_type_context,
                )
                && !has_kind(model, entry.name, entry.symbol_type_context, "varexo")
            {
                error(
                    out,
                    entry.name_span,
                    "E419",
                    format!(
                        "endval(learnt_in=...): {} is not an exogenous variable",
                        model.name(entry.name)
                    ),
                );
            }
            if !nondefault && entry.operation != ShockOperation::Values {
                let name = model.name(entry.name);
                let operator = if entry.operation == ShockOperation::Add {
                    "+="
                } else {
                    "*="
                };
                error(
                    out,
                    entry.span,
                    "E417",
                    format!(
                        "endval: '{name} {operator} ...' line not allowed unless 'learnt_in' option with value >1 or date is passed"
                    ),
                );
            }
        }
    }
}

fn check_databases(model: &Model, out: &mut OrderedRows<'_>) {
    let mut seen = HashSet::new();
    for (index, statement) in model.databases.iter().enumerate() {
        out.own("database", index);
        out.callback = Some(
            crate::engine::diagnostics::parse_preflight::statement_callback_order(
                model,
                crate::engine::diagnostics::parse_preflight::callback_order(
                    model, "database", index,
                ),
            ),
        );
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        for &(name, span) in &statement.names {
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            if !seen.insert(name) {
                error(
                    out,
                    span,
                    "E414",
                    format!("Database '{}' already declared", model.name(name)),
                );
            }
        }
    }
}

fn check_paths(model: &Model, out: &mut OrderedRows<'_>) {
    for (index, block) in model.shock_paths.iter().enumerate() {
        out.own("shock_path_callback", index);
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if !block.completed {
            continue;
        }
        for stanza in &block.stanzas {
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            check_path_self_variables(model, stanza, out);
        }
    }
    // The companion block has its separate value grammar and callback checks.
    for (index, block) in model.controlled_paths.iter().enumerate() {
        out.own("controlled_path_callback", index);
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        if let Some(PeriodPoint::Integer(n)) = block.options.learnt_in.as_ref()
            && *n < 1
        {
            error(
                out,
                block.options.learnt_in_span.unwrap_or(block.span),
                "E421",
                format!("Value '{n}' is not allowed for 'learnt_in' option"),
            );
        }
        for stanza in &block.stanzas {
            if crate::engine::control::cancelled() {
                return Default::default();
            }
            if !stanza.callback_completed {
                continue;
            }
            match &stanza.target {
                PathTarget::Exogenous { name, span } => {
                    exogenous(
                        model,
                        out,
                        *name,
                        (*span, stanza.symbol_type_context),
                        false,
                    );
                    if stanza.periods.len() != stanza.values.len() {
                        error(
                            out,
                            stanza.span,
                            "E404",
                            format!(
                                "shock_paths: variable {}: number of periods is different from number of shock values",
                                model.name(*name)
                            ),
                        );
                    }
                }
                PathTarget::Controlled {
                    exogenize,
                    exogenize_span,
                    endogenize,
                    endogenize_span,
                } => {
                    endogenous(
                        model,
                        out,
                        *exogenize,
                        (*exogenize_span, stanza.symbol_type_context),
                    );
                    exogenous(
                        model,
                        out,
                        *endogenize,
                        (*endogenize_span, stanza.symbol_type_context),
                        false,
                    );
                    if stanza.periods.len() != stanza.values.len() {
                        error(
                            out,
                            stanza.span,
                            "E406",
                            "The number of periods is different from the number of values",
                        );
                    }
                }
            }
            for range in &stanza.periods {
                if crate::engine::control::cancelled() {
                    return Default::default();
                }
                check_range(out, range);
            }
        }
    }
}

fn check_path_self_variables(model: &Model, stanza: &PathStanza, out: &mut OrderedRows<'_>) {
    let PathTarget::Exogenous { name, .. } = stanza.target else {
        return;
    };
    if !stanza.callback_completed {
        return;
    }
    for value in &stanza.values {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let Some(facts) = value.expr.and_then(|id| model.path_value_facts.get(&id)) else {
            continue;
        };
        if !facts.self_variables.contains(&(name, 0)) {
            continue;
        }
        let span = value
            .path_refs
            .iter()
            .find(|reference| {
                reference.namespace.as_deref() == Some("self")
                    && reference.name == name
                    && reference.constructed_lag == Some(0)
            })
            .map(|reference| reference.span)
            .unwrap_or(value.span);
        let name = model.name(name);
        error(
            out,
            span,
            "E420",
            format!(
                "in the definition of '{name}' in a 'shock_paths' block, the use of 'self.{name}' without a lag is not allowed, since it is a circular reference"
            ),
        );
    }
}
