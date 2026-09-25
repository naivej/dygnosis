//! Dynare 7.2 written shock and path refusals. The parser keeps these forms
//! separate from stochastic `ShockStmt` checks.

use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, Severity};
use crate::intern::Name;
use crate::lag_fold::{fold_lag, LagFold};
use crate::model::{
    Model, PathBlock, PathReference, PathStanza, PathTarget, PeriodPoint, PeriodRange,
    ShockBlockKind, ShockKind, ShockOperation,
};
use crate::span::Span;

fn error(out: &mut Vec<Diagnostic>, span: Span, code: &str, message: impl Into<String>) {
    out.push(Diagnostic::new(span, Severity::Error, code, message));
}

struct Roles {
    exo: HashSet<Name>,
    det: HashSet<Name>,
    endo: HashSet<Name>,
    param: HashSet<Name>,
    declared_at: HashMap<Name, u32>,
}

impl Roles {
    fn new(model: &Model) -> Self {
        let det: HashSet<Name> = model
            .deterministic_exogenous
            .iter()
            .map(|d| d.name)
            .collect();
        let mut declared_at = HashMap::new();
        for decl in model
            .exogenous
            .iter()
            .chain(&model.deterministic_exogenous)
            .chain(&model.endogenous)
            .chain(&model.parameters)
        {
            declared_at
                .entry(decl.name)
                .and_modify(|at: &mut u32| *at = (*at).min(decl.span.start))
                .or_insert(decl.span.start);
        }
        Self {
            exo: model
                .exogenous
                .iter()
                .map(|d| d.name)
                .filter(|name| !det.contains(name))
                .collect(),
            det,
            endo: model.endogenous.iter().map(|d| d.name).collect(),
            param: model.parameters.iter().map(|d| d.name).collect(),
            declared_at,
        }
    }

    fn known(&self, name: Name, span: Span) -> bool {
        self.declared_at
            .get(&name)
            .is_some_and(|at| *at < span.start)
            && (self.exo.contains(&name)
                || self.det.contains(&name)
                || self.endo.contains(&name)
                || self.param.contains(&name))
    }

    fn exogenous(
        &self,
        model: &Model,
        out: &mut Vec<Diagnostic>,
        name: Name,
        span: Span,
        allow_det: bool,
    ) -> bool {
        if self.known(name, span)
            && (self.exo.contains(&name) || (allow_det && self.det.contains(&name)))
        {
            return true;
        }
        let n = model.name(name);
        if !self.known(name, span) {
            error(out, span, "E058", format!("Unknown symbol: {n}."));
        } else if self.det.contains(&name) {
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

    fn endogenous(&self, model: &Model, out: &mut Vec<Diagnostic>, name: Name, span: Span) -> bool {
        if self.known(name, span) && self.endo.contains(&name) {
            return true;
        }
        let n = model.name(name);
        if !self.known(name, span) {
            error(out, span, "E058", format!("Unknown symbol: {n}."));
        } else {
            error(out, span, "E317", format!("{n} is not endogenous."));
        }
        false
    }
}

pub fn check_d_shocks(model: &Model) -> Vec<Diagnostic> {
    let roles = Roles::new(model);
    let mut out = Vec::new();
    for (name, span) in &model.option_twice {
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
    if !out.is_empty() {
        return out;
    }
    check_irf_shocks_options(model, &roles, &mut out);
    check_stochastic_names(model, &roles, &mut out);
    check_scheduled(model, &roles, &mut out);
    check_endval(model, &roles, &mut out);
    check_databases(model, &mut out);
    check_paths(model, &roles, &mut out);
    out
}

fn check_stochastic_names(model: &Model, roles: &Roles, out: &mut Vec<Diagnostic>) {
    for block in &model.shock_blocks {
        if block.kind == ShockBlockKind::Heterogeneous {
            for stmt in &block.stochastic {
                let names: Vec<Name> = match &stmt.kind {
                    ShockKind::Var(name) | ShockKind::Stderr(name) => vec![*name],
                    ShockKind::Cov(names) | ShockKind::Skew(names) => names.clone(),
                    ShockKind::Corr { a, b } => vec![*a, *b],
                };
                for name in names {
                    if !roles.known(name, stmt.span) {
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
            let names: Vec<Name> = match &stmt.kind {
                ShockKind::Var(name) | ShockKind::Stderr(name) => vec![*name],
                ShockKind::Cov(names) | ShockKind::Skew(names) => names.clone(),
                ShockKind::Corr { a, b } => vec![*a, *b],
            };
            for name in names {
                if !roles.known(name, stmt.span) {
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

fn check_irf_shocks_options(model: &Model, roles: &Roles, out: &mut Vec<Diagnostic>) {
    for option in &model.irf_shocks_options {
        for &(name, span) in &option.names {
            if !roles.known(name, span) {
                error(
                    out,
                    span,
                    "E058",
                    format!("Unknown symbol: {}", model.name(name)),
                );
            } else if !roles.exo.contains(&name) {
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

fn check_range(out: &mut Vec<Diagnostic>, range: &PeriodRange) {
    if let (PeriodPoint::Integer(first), Some(PeriodPoint::Integer(last))) =
        (&range.first, &range.last)
    {
        if first > last {
            error(
                out,
                range.span,
                "E395",
                "Can't have first period index greater than second index in range specification",
            );
        }
    }
}

fn check_scheduled(model: &Model, roles: &Roles, out: &mut Vec<Diagnostic>) {
    for block in &model.shock_blocks {
        if block.kind == ShockBlockKind::Heterogeneous {
            continue;
        }
        let mut seen = HashSet::new();
        let mut seen_hetero = HashSet::new();
        let learnt = block.options.learnt_in.as_ref();
        if let Some(PeriodPoint::Integer(n)) = learnt {
            if *n < 1 {
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
        }
        for row in &block.scheduled {
            let allow_det = block.kind != ShockBlockKind::Heteroskedastic
                && row.operation == ShockOperation::Values;
            roles.exogenous(model, out, row.name, row.name_span, allow_det);
            for range in &row.periods {
                check_range(out, range);
            }
            if block.kind == ShockBlockKind::Heteroskedastic {
                if !seen_hetero.insert((row.name, row.operation as u8)) {
                    error(
                        out,
                        row.span,
                        "E402",
                        format!(
                            "heteroskedastic_shocks: variable {} declared twice",
                            model.name(row.name)
                        ),
                    );
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
            if !seen.insert(row.name) {
                error(
                    out,
                    row.span,
                    "E344",
                    format!(
                        "shocks/conditional_forecast_paths: variable {} declared twice",
                        model.name(row.name)
                    ),
                );
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
            if let Some(PeriodPoint::Integer(n)) = learnt {
                if *n > 1 {
                    for range in &row.periods {
                        if let PeriodPoint::Integer(first) = &range.first {
                            if first < n {
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

fn check_endval(model: &Model, roles: &Roles, out: &mut Vec<Diagnostic>) {
    for block in &model.endval_instructions {
        let learnt = block.learnt_in.as_ref();
        if let Some(PeriodPoint::Integer(n)) = learnt {
            if *n < 1 {
                error(
                    out,
                    block.learnt_in_span.unwrap_or(block.span),
                    "E418",
                    format!("endval: value '{n}' is not allowed for 'learnt_in' option"),
                );
            }
        }
        let nondefault = matches!(learnt, Some(PeriodPoint::Date(_)))
            || matches!(learnt, Some(PeriodPoint::Integer(n)) if *n > 1);
        for entry in &block.entries {
            if nondefault
                && roles.known(entry.name, entry.name_span)
                && !roles.exo.contains(&entry.name)
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

fn check_databases(model: &Model, out: &mut Vec<Diagnostic>) {
    let mut seen = HashSet::new();
    for statement in &model.databases {
        for &(name, span) in &statement.names {
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

fn check_paths(model: &Model, roles: &Roles, out: &mut Vec<Diagnostic>) {
    for (block, companion) in model
        .shock_paths
        .iter()
        .map(|block| (block, false))
        .chain(model.controlled_paths.iter().map(|block| (block, true)))
    {
        if let Some(PeriodPoint::Integer(n)) = block.options.learnt_in.as_ref() {
            if *n < 1 {
                error(
                    out,
                    block.options.learnt_in_span.unwrap_or(block.span),
                    "E421",
                    format!("Value '{n}' is not allowed for 'learnt_in' option"),
                );
            }
        }
        for stanza in &block.stanzas {
            match &stanza.target {
                PathTarget::Exogenous { name, span } => {
                    roles.exogenous(model, out, *name, *span, false);
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
                    roles.endogenous(model, out, *exogenize, *exogenize_span);
                    roles.exogenous(model, out, *endogenize, *endogenize_span, false);
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
                check_range(out, range);
            }
            if !companion {
                check_path_values(model, roles, block, stanza, out);
            }
        }
    }
}

fn check_path_values(
    model: &Model,
    roles: &Roles,
    block: &PathBlock,
    stanza: &PathStanza,
    out: &mut Vec<Diagnostic>,
) {
    let databases: HashSet<&str> = model
        .databases
        .iter()
        .flat_map(|decl| decl.names.iter())
        .filter(|(_, span)| span.start < block.span.start)
        .map(|(name, _)| model.name(*name))
        .collect();
    for (index, value) in stanza.values.iter().enumerate() {
        let mut max_lag = 0;
        let mut bad_reference = false;
        for reference in &value.path_refs {
            let before = out.len();
            check_path_reference(model, roles, block, stanza, reference, &databases, out);
            bad_reference |= out.len() != before;
            if let Some(LagFold::Integer(lag)) = reference.lag.as_deref().map(fold_lag) {
                max_lag = max_lag.max(-lag);
            }
        }
        if !bad_reference {
            if let (PathTarget::Exogenous { .. }, Some(period)) =
                (&stanza.target, stanza.periods.get(index))
            {
                if let PeriodPoint::Integer(first) = &period.first {
                    if *first <= max_lag {
                        error(
                            out,
                            value.span,
                            "E405",
                            format!(
                                "shock_paths: a lag of {max_lag} is not allowed at period {first}"
                            ),
                        );
                    }
                }
            }
        }
    }
}

fn check_path_reference(
    model: &Model,
    roles: &Roles,
    block: &PathBlock,
    stanza: &PathStanza,
    reference: &PathReference,
    databases: &HashSet<&str>,
    out: &mut Vec<Diagnostic>,
) {
    let Some(namespace) = reference.namespace.as_deref() else {
        if !reference.call
            && roles.known(reference.name, reference.span)
            && !roles.param.contains(&reference.name)
        {
            error(
                out,
                reference.span,
                "E407",
                "In the shock_paths block, parameters are the only symbols allowed without a namespace-qualifier",
            );
        }
        // An undeclared bare name makes pinned 7.2 crash without an ERROR.
        return;
    };
    let name = model.name(reference.name);
    let syntax = if namespace.eq_ignore_ascii_case("learnt_in") {
        let period = reference
            .learnt_in
            .as_ref()
            .map(period_text)
            .unwrap_or_else(|| "?".to_string());
        format!("learnt_in({period}).{name}")
    } else {
        format!("{namespace}.{name}")
    };
    let controlled = matches!(stanza.target, PathTarget::Controlled { .. });
    if reference
        .lag
        .as_deref()
        .is_some_and(|lag| lag.contains(','))
        && (namespace.eq_ignore_ascii_case("self")
            || namespace.eq_ignore_ascii_case("prev")
            || databases.contains(namespace))
    {
        error(
            out,
            reference.span,
            "E410",
            format!(
                "The parenthesis after {syntax} should only include a lag, since it references a variable inside a namespace"
            ),
        );
        return;
    }
    match namespace.to_ascii_lowercase().as_str() {
        "initval" | "init" => {
            // `initval.x(...)` is routed to the function/lead-lag grammar,
            // not to the bare `initval.x` namespace rule.
            if reference.lag_call {
                return;
            }
            if !roles.known(reference.name, reference.span) {
                error(
                    out,
                    reference.span,
                    "E058",
                    format!("Unknown symbol: {name}."),
                );
            } else if roles.det.contains(&reference.name) {
                error(
                    out,
                    reference.span,
                    "E317",
                    format!("{name} is an exogenous deterministic."),
                );
            } else if !roles.exo.contains(&reference.name) && !roles.endo.contains(&reference.name)
            {
                error(
                    out,
                    reference.span,
                    "E059",
                    format!("{name} is neither endogenous or exogenous."),
                );
            }
            return;
        }
        "self" | "prev" | "learnt_in" => {
            if !roles.exogenous(model, out, reference.name, reference.span, false) {
                return;
            }
        }
        _ => {
            if !databases.contains(namespace) && reference.lag_call {
                // An undeclared `A.x(…)` is parsed as an external function,
                // even in a controlled stanza.
                return;
            }
            if controlled {
                forbidden_controlled(out, reference.span, &syntax);
                return;
            }
            if !databases.contains(namespace) {
                error(
                    out,
                    reference.span,
                    "E415",
                    format!(
                        "Unknown database: {namespace}. You may want to declare it via the 'database' command."
                    ),
                );
                return;
            }
        }
    }
    if namespace.eq_ignore_ascii_case("self") {
        if controlled {
            forbidden_controlled(out, reference.span, &syntax);
            return;
        }
        if let Some(LagFold::Integer(lag)) = reference.lag.as_deref().map(fold_lag) {
            if lag > 0 {
                error(
                    out,
                    reference.span,
                    "E408",
                    format!("The syntax {syntax} cannot be used with a lead"),
                );
                return;
            }
        }
    } else if namespace.eq_ignore_ascii_case("prev") {
        if block.options.learnt_in.is_none()
            || matches!(
                block.options.learnt_in.as_ref(),
                Some(PeriodPoint::Integer(1))
            )
        {
            error(
                out,
                reference.span,
                "E411",
                format!(
                    "The syntax {syntax} is not accepted in a 'shock_paths' block without the 'learnt_in' option or in a 'shock_paths(learnt_in=1)' block"
                ),
            );
            return;
        }
        if controlled {
            forbidden_controlled(out, reference.span, &syntax);
            return;
        }
    } else if namespace.eq_ignore_ascii_case("learnt_in") {
        if let Some(PeriodPoint::Integer(n)) = reference.learnt_in.as_ref() {
            if *n < 1 {
                error(
                    out,
                    reference.span,
                    "E412",
                    format!("The syntax {syntax} is not accepted"),
                );
                return;
            }
            let block_n = match block.options.learnt_in.as_ref() {
                Some(PeriodPoint::Integer(value)) => *value,
                None => 1,
                _ => 0,
            };
            if block_n > 0 && block_n <= *n {
                error(
                    out,
                    reference.span,
                    "E413",
                    format!(
                        "The syntax {syntax} is not accepted in a 'shock_paths' block without the 'learnt_in' option or in a 'shock_paths(learnt_in={block_n})' block"
                    ),
                );
                return;
            }
        }
        if controlled {
            forbidden_controlled(out, reference.span, &syntax);
            return;
        }
    }
    if let Some(lag) = reference.lag.as_deref() {
        if fold_lag(lag) == LagFold::NonInteger {
            error(
                out,
                reference.span,
                "E409",
                format!(
                    "Symbol {syntax} is being treated as if it were a function (i.e., passed an argument that is not an integer)."
                ),
            );
        }
    }
    if namespace.eq_ignore_ascii_case("self") {
        let current = match &stanza.target {
            PathTarget::Exogenous { name, .. } => *name,
            PathTarget::Controlled { .. } => return,
        };
        let zero_lag = reference
            .lag
            .as_deref()
            .map(fold_lag)
            .unwrap_or(LagFold::Integer(0))
            == LagFold::Integer(0);
        if current == reference.name && zero_lag {
            error(
                out,
                reference.span,
                "E420",
                format!(
                    "in the definition of '{name}' in a 'shock_paths' block, the use of 'self.{name}' without a lag is not allowed, since it is a circular reference"
                ),
            );
        }
    }
}

fn forbidden_controlled(out: &mut Vec<Diagnostic>, span: Span, syntax: &str) {
    error(
        out,
        span,
        "E416",
        format!(
            "The syntax {syntax} is not accepted in an 'endogenize' stanza of a 'shock_paths' block"
        ),
    );
}

fn period_text(point: &PeriodPoint) -> String {
    match point {
        PeriodPoint::Integer(n) => n.to_string(),
        PeriodPoint::Date(date) => date.text.clone(),
        PeriodPoint::End => "end".to_string(),
    }
}
