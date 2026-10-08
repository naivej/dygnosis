//! Equation extraction for `dynare_extract`.
//!
//! The engine selects equations from the effective compilation unit and retains
//! the declarations, locals, heterogeneity dimension, static/dynamic partners,
//! and OccBin regime companions those equations need. Aggregate and
//! heterogeneous rows stay separate even when an index or a name matches.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::check_e060::check_e063;
use crate::check_writing::model_structure;
use crate::expand::{expand_report, ExpandReport};
use crate::expr::{ExprId, ExprKind, IdentRef};
use crate::intern::Name;
use crate::model::{Decl, Equation, ExternalFunctionStmt, Model, TrendVar};
use crate::parser::parse;
use crate::span::Span;
use crate::workspace::Workspace;

/// Caller inputs. `dimension` searches one heterogeneity dimension. Omit it to
/// search aggregate and heterogeneous scopes. `files` is the companion map.
#[derive(Clone, Debug, Default)]
pub struct ExtractRequest {
    pub file_content: String,
    pub active_file: Option<String>,
    pub files: BTreeMap<String, String>,
    pub names: Vec<String>,
    pub tags: BTreeMap<String, String>,
    pub dimension: Option<String>,
}

/// Empty `names` and empty `tags`. Not an extraction status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtractError {
    EmptySelector,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtractStatus {
    Ok,
    Empty,
    UnsupportedContext,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EquationDomain {
    Aggregate,
    Heterogeneous,
}

/// `Requested` matched the selector. `Companion` was added because a retained
/// row needs it. A row that is both stays `Requested`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EquationRole {
    Requested,
    Companion,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedEquation {
    pub domain: EquationDomain,
    pub dimension: Option<String>,
    /// Counted index. `None` for a `[static]` equation.
    pub index: Option<usize>,
    pub name: Option<String>,
    pub role: EquationRole,
    pub tags: BTreeMap<String, String>,
}

/// Where a selected row was read. `frames` follows the expand map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractOrigin {
    pub domain: EquationDomain,
    pub dimension: Option<String>,
    /// Same index as the selected-equation entry. `None` for `[static]`.
    pub index: Option<usize>,
    pub role: EquationRole,
    pub span: Span,
    pub file: Option<String>,
    pub frames: Vec<OriginFrame>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginFrame {
    pub kind: String,
    pub span: Span,
    pub file: Option<String>,
    /// `@#for` index name, when this frame is one loop iteration.
    pub variable: Option<String>,
    /// `@#for` index value for this expanded copy.
    pub value: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OmittedContext {
    pub kind: String,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsupportedKind {
    CompilationUnit,
    Pac,
    Include,
    Macro,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsupportedContext {
    pub kind: UnsupportedKind,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractResult {
    pub status: ExtractStatus,
    pub fragment: Option<String>,
    pub selected_equations: Vec<SelectedEquation>,
    pub origins: Vec<ExtractOrigin>,
    pub omitted_context: Vec<OmittedContext>,
    pub explanation: String,
    pub unsupported: Option<UnsupportedContext>,
}

pub fn extract(request: &ExtractRequest) -> Result<ExtractResult, ExtractError> {
    if request.names.is_empty() && request.tags.is_empty() {
        return Err(ExtractError::EmptySelector);
    }

    let Some(unit) = prepare(request) else {
        return Ok(unsupported(
            UnsupportedKind::CompilationUnit,
            "the compilation unit could not be expanded",
        ));
    };
    let model = &unit.model;
    let hits = matching_places(model, &unit.report, request);
    if let Some((kind, detail)) = unresolved_selection(model, unit.missing_include) {
        return Ok(unsupported(kind, detail));
    }
    if hits.is_empty() {
        let explanation = match &request.dimension {
            Some(dimension) => {
                format!("No equation matched the selectors in dimension `{dimension}`.")
            }
            None => "No equation matched the selectors.".to_string(),
        };
        return Ok(ExtractResult {
            status: ExtractStatus::Empty,
            fragment: Some(String::new()),
            selected_equations: Vec::new(),
            origins: Vec::new(),
            omitted_context: Vec::new(),
            explanation,
            unsupported: None,
        });
    }

    let kept = kept_equations(model, &unit.report, &hits);
    let kept_places: Vec<EqRef> = kept.iter().map(|row| row.place).collect();
    if let Some(detail) = required_partner_problem(model, &kept_places) {
        return Ok(unsupported(UnsupportedKind::CompilationUnit, detail));
    }
    let constraints = match required_constraints(model, &kept_places) {
        Ok(constraints) => constraints,
        Err(detail) => return Ok(unsupported(UnsupportedKind::CompilationUnit, detail)),
    };
    let closure = close(model, &kept_places, &constraints);
    if let Some(reason) = refuse(model, &kept_places, &closure, unit.missing_include) {
        return Ok(unsupported(reason.0, reason.1));
    }
    let retained = retained_places(model, &kept_places, &closure);
    let occbin = match occbin_setup(model, &constraints, &retained) {
        Ok(piece) => piece,
        Err(detail) => return Ok(unsupported(UnsupportedKind::Macro, detail)),
    };

    let selected_equations = kept
        .iter()
        .map(|row| selected_row(model, row.place, row.role))
        .collect::<Vec<_>>();
    let origins = kept
        .iter()
        .map(|row| {
            origin_row(
                model,
                &unit.report,
                row.place,
                row.role,
                request.active_file.as_deref(),
            )
        })
        .collect();
    let omitted_context = omitted(model);

    Ok(ExtractResult {
        status: ExtractStatus::Ok,
        fragment: Some(render(
            model,
            &unit.report,
            &kept_places,
            &closure,
            occbin.as_ref(),
        )),
        selected_equations,
        origins,
        omitted_context,
        explanation: String::new(),
        unsupported: None,
    })
}

/// One written equation. `block` is `None` for the aggregate model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct EqRef {
    block: Option<usize>,
    index: usize,
}

struct Unit {
    model: Model,
    report: ExpandReport,
    missing_include: bool,
}

/// Map-free input parses `file_content` alone.
///
/// A nonempty `files` map is the compilation unit. `file_content` is stored
/// under `active_file`, replacing that entry when the map already has one.
/// The other entries resolve includes. Callers do not need a duplicate copy
/// of the active file in the map.
fn prepare(request: &ExtractRequest) -> Option<Unit> {
    if request.files.is_empty() {
        let model = parse(&request.file_content);
        let report = expand_report(&request.file_content);
        return Some(Unit {
            model,
            report,
            missing_include: false,
        });
    }
    let active = request.active_file.as_deref()?;
    let mut files = request.files.clone();
    let active_path = active.replace('\\', "/");
    files.retain(|key, _| key.replace('\\', "/") != active_path);
    files.insert(active.to_string(), request.file_content.clone());
    let mut ws = Workspace::overlay_documents(&files);
    let model = ws.get_effective_model(active).cloned()?;
    let report = ws.expand_report(active).cloned()?;
    let missing_include = !ws.find_unresolved_includes(active).is_empty()
        || !ws.find_circular_includes(active).is_empty();
    Some(Unit {
        model,
        report,
        missing_include,
    })
}

fn origin_row(
    model: &Model,
    report: &ExpandReport,
    place: EqRef,
    role: EquationRole,
    active_file: Option<&str>,
) -> ExtractOrigin {
    let row = selected_row(model, place, role);
    let mapped = row_origin(report, place).map(|row| &row.origin);
    let frames = mapped
        .map(|origin| {
            origin
                .origin_frames
                .iter()
                .map(|frame| OriginFrame {
                    kind: frame.kind.clone(),
                    span: frame.origin_span,
                    file: frame.origin_uri.clone(),
                    variable: frame.variable.clone(),
                    value: frame.value.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    ExtractOrigin {
        domain: row.domain,
        dimension: row.dimension,
        index: row.index,
        role: row.role,
        span: mapped
            .map(|origin| origin.origin_span)
            .unwrap_or(eq_at(model, place).span),
        file: mapped
            .and_then(|origin| origin.origin_uri.clone())
            .or_else(|| active_file.map(str::to_string)),
        frames,
    }
}

fn row_origin(report: &ExpandReport, place: EqRef) -> Option<&crate::expand::RowOrigin> {
    match place.block {
        None => report.aggregate_row_origins.get(place.index)?.as_ref(),
        Some(block) => report
            .heterogeneous_row_origins
            .get(block)?
            .get(place.index)?
            .as_ref(),
    }
}

fn row_order(report: &ExpandReport, place: EqRef) -> usize {
    row_origin(report, place).map_or(usize::MAX, |row| row.order)
}

fn unsupported(kind: UnsupportedKind, detail: impl Into<String>) -> ExtractResult {
    let detail = detail.into();
    ExtractResult {
        status: ExtractStatus::UnsupportedContext,
        fragment: None,
        selected_equations: Vec::new(),
        origins: Vec::new(),
        omitted_context: Vec::new(),
        explanation: detail.clone(),
        unsupported: Some(UnsupportedContext { kind, detail }),
    }
}

/// Selector hits in source order. A set `dimension` searches only that
/// heterogeneity dimension. An omitted dimension searches every scope.
/// An aggregate row and a heterogeneous row are never the same hit.
fn matching_places(model: &Model, report: &ExpandReport, request: &ExtractRequest) -> Vec<EqRef> {
    let mut hits = Vec::new();
    if request.dimension.is_none() {
        for (index, eq) in model.equations.iter().enumerate() {
            if !eq.is_local && matches_selector(eq, request) {
                hits.push(EqRef { block: None, index });
            }
        }
    }
    for (block, model_block) in model.heterogeneous_models.iter().enumerate() {
        let name = model.name(model_block.dimension);
        if request
            .dimension
            .as_deref()
            .is_some_and(|wanted| wanted != name)
        {
            continue;
        }
        for (index, eq) in model_block.equations.iter().enumerate() {
            if !eq.is_local && matches_selector(eq, request) {
                hits.push(EqRef {
                    block: Some(block),
                    index,
                });
            }
        }
    }
    hits.sort_by_key(|place| row_order(report, *place));
    hits
}

fn eq_at(model: &Model, place: EqRef) -> &Equation {
    match place.block {
        None => &model.equations[place.index],
        Some(block) => &model.heterogeneous_models[block].equations[place.index],
    }
}

fn matches_selector(eq: &Equation, request: &ExtractRequest) -> bool {
    let name_ok = request.names.is_empty() || request.names.iter().any(|name| name == &eq.name);
    let tags_ok = request.tags.iter().all(|(key, value)| {
        eq.tag_map
            .get(&key.to_ascii_lowercase())
            .is_some_and(|got| got == value)
    });
    name_ok && tags_ok
}

struct Closure {
    symbols: BTreeSet<String>,
    /// Model-local names, each tied to the aggregate scope or a dimension.
    locals: BTreeSet<(Option<String>, String)>,
    /// The proven definitions required by bound local uses.
    local_places: BTreeSet<EqRef>,
    externals: BTreeSet<String>,
}

fn close(
    model: &Model,
    selected: &[EqRef],
    constraints: &[&crate::model::OccbinConstraint],
) -> Closure {
    let facts = crate::model_locals::ModelLocals::collect(model);
    let mut closure = Closure {
        symbols: BTreeSet::new(),
        locals: BTreeSet::new(),
        local_places: BTreeSet::new(),
        externals: BTreeSet::new(),
    };
    let mut pending: Vec<EqRef> = selected.to_vec();
    let mut seen = HashSet::new();
    while let Some(place) = pending.pop() {
        absorb(model, place, &facts, &mut closure, &mut pending, &mut seen);
    }
    for constraint in constraints {
        for expression in [
            &constraint.bind,
            &constraint.relax,
            &constraint.error_bind,
            &constraint.error_relax,
        ]
        .into_iter()
        .flatten()
        {
            if let Some(id) = expression.expr {
                push_expr_symbols(model, id, &facts, &mut closure);
            }
        }
    }
    loop {
        let locals_before = closure.locals.len();
        grow_declaration_context(model, &facts, &mut closure);
        if pending.is_empty() && closure.locals.len() == locals_before {
            break;
        }
        while let Some(place) = pending.pop() {
            absorb(model, place, &facts, &mut closure, &mut pending, &mut seen);
        }
    }
    closure
}

fn absorb(
    model: &Model,
    place: EqRef,
    facts: &crate::model_locals::ModelLocals,
    closure: &mut Closure,
    pending: &mut Vec<EqRef>,
    seen: &mut HashSet<EqRef>,
) {
    if !seen.insert(place) {
        return;
    }
    let eq = eq_at(model, place);
    let mut idents: Vec<IdentRef> = Vec::new();
    let mut calls = Vec::new();
    if let Some(id) = eq.lhs_expr {
        walk(model, id, &mut idents, &mut calls);
    }
    if let Some(id) = eq.rhs_expr {
        walk(model, id, &mut idents, &mut calls);
    }
    let defined_span = eq
        .is_local
        .then(|| {
            eq.lhs_expr.and_then(|id| match &model.exprs.get(id).kind {
                ExprKind::Ident { ident_span, .. } => Some(*ident_span),
                _ => None,
            })
        })
        .flatten();
    let written_index = written_equation_index(model, place);
    for reference in idents {
        let text = model.name(reference.name).to_string();
        if defined_span == Some(reference.span) {
            continue;
        }
        let local_use = written_index.and_then(|written_index| {
            facts.uses.iter().find(|usage| {
                usage.equation_index == written_index
                    && usage.name == reference.name
                    && usage.span == reference.span
            })
        });
        if let Some(local_use) = local_use {
            closure.locals.insert((
                local_use
                    .dimension
                    .map(|dimension| model.name(dimension).to_string()),
                text,
            ));
            if let Some(definition) = local_use.definition {
                if let Some(local_place) = local_place_for_definition(model, facts, definition) {
                    closure.local_places.insert(local_place);
                    if !seen.contains(&local_place) {
                        pending.push(local_place);
                    }
                }
            }
        } else {
            closure.symbols.insert(text);
        }
    }
    for name in calls {
        let text = model.name(name).to_string();
        if is_pac_name(&text) {
            closure.symbols.insert(text);
        } else if is_external(model, &text) {
            closure.externals.insert(text);
        }
    }
}

fn closure_has_local(closure: &Closure, name: &str) -> bool {
    closure.locals.iter().any(|(_, local)| local == name)
}

/// A global model-local declaration is shared metadata for every model scope.
fn decl_matches_local(model: &Model, closure: &Closure, decl: &Decl) -> bool {
    let name = model.name(decl.name);
    let declared_dimension = decl
        .heterogeneity
        .map(|(dimension, _)| model.name(dimension).to_string());
    closure.locals.iter().any(|(local_dimension, local)| {
        local == name && (declared_dimension.is_none() || *local_dimension == declared_dimension)
    })
}

fn grow_declaration_context(
    model: &Model,
    facts: &crate::model_locals::ModelLocals,
    closure: &mut Closure,
) {
    loop {
        let before = closure.symbols.len() + closure.externals.len();
        for trend in &model.trend_vars {
            if !closure.symbols.contains(model.name(trend.name)) {
                continue;
            }
            if let Some(id) = trend.growth {
                push_expr_symbols(model, id, facts, closure);
            }
        }
        for row in &model.nonstationary_vars {
            if !closure.symbols.contains(model.name(row.name)) {
                continue;
            }
            if let Some(id) = row.deflator {
                push_expr_symbols(model, id, facts, closure);
            }
        }
        let extra: Vec<String> = model
            .external_functions
            .iter()
            .filter(|stmt| {
                stmt.name
                    .as_ref()
                    .is_some_and(|(name, _)| closure.externals.contains(model.name(*name)))
            })
            .flat_map(deriv_names)
            .map(|name| model.name(name).to_string())
            .filter(|name| is_external(model, name))
            .collect();
        for name in extra {
            closure.externals.insert(name);
        }
        if closure.symbols.len() + closure.externals.len() == before {
            break;
        }
    }
}

fn push_expr_symbols(
    model: &Model,
    id: ExprId,
    facts: &crate::model_locals::ModelLocals,
    closure: &mut Closure,
) {
    let mut idents = Vec::new();
    let mut calls = Vec::new();
    walk(model, id, &mut idents, &mut calls);
    for reference in idents {
        let text = model.name(reference.name).to_string();
        if let Some(binding) = facts
            .available(None, usize::MAX)
            .into_iter()
            .find(|binding| model.name(binding.name) == text)
        {
            closure.locals.insert((None, text));
            if let Some(definition) = binding.definition {
                if let Some(place) = local_place_for_definition(model, facts, definition) {
                    closure.local_places.insert(place);
                }
            }
        } else {
            closure.symbols.insert(text);
        }
    }
    for name in calls {
        let text = model.name(name).to_string();
        if is_external(model, &text) {
            closure.externals.insert(text);
        }
    }
}

fn deriv_names(stmt: &ExternalFunctionStmt) -> Vec<Name> {
    let mut names = Vec::new();
    for spec in [stmt.first_deriv, stmt.second_deriv] {
        if let Some(crate::model::DerivSpec::Named(name, _)) = spec {
            names.push(name);
        }
    }
    names
}

fn walk(model: &Model, id: ExprId, idents: &mut Vec<IdentRef>, calls: &mut Vec<Name>) {
    match &model.exprs.get(id).kind {
        ExprKind::Ident {
            name,
            timing,
            ident_span,
            timing_span,
        } => idents.push(IdentRef {
            name: *name,
            span: *ident_span,
            timing: *timing,
            timing_span: *timing_span,
        }),
        ExprKind::Number | ExprKind::String | ExprKind::Error | ExprKind::PathNamespace { .. } => {}
        ExprKind::Unary { arg, .. } => walk(model, *arg, idents, calls),
        ExprKind::Binary { lhs, rhs, .. } => {
            walk(model, *lhs, idents, calls);
            walk(model, *rhs, idents, calls);
        }
        ExprKind::Call { callee, args } => {
            calls.push(*callee);
            for arg in args {
                walk(model, *arg, idents, calls);
            }
        }
        ExprKind::SteadyState { arg } | ExprKind::Expectation { arg, .. } => {
            walk(model, *arg, idents, calls);
        }
    }
}

fn written_equation_index(model: &Model, place: EqRef) -> Option<usize> {
    let equation = eq_at(model, place);
    let dimension = scope_key(model, place);
    model.written_equations.iter().position(|row| {
        row.equation.parse_order == equation.parse_order && row.dimension == dimension
    })
}

fn local_place_for_definition(
    model: &Model,
    facts: &crate::model_locals::ModelLocals,
    definition: usize,
) -> Option<EqRef> {
    let fact = facts.definitions.get(definition)?;
    let row = model.written_equations.get(fact.equation_index)?;
    let parse_order = row.equation.parse_order;
    match row.dimension {
        None => model
            .equations
            .iter()
            .position(|equation| equation.parse_order == parse_order)
            .map(|index| EqRef { block: None, index }),
        Some(dimension) => model
            .heterogeneous_models
            .iter()
            .enumerate()
            .filter(|(_, block)| block.dimension == dimension)
            .find_map(|(block_index, block)| {
                block
                    .equations
                    .iter()
                    .position(|equation| equation.parse_order == parse_order)
                    .map(|index| EqRef {
                        block: Some(block_index),
                        index,
                    })
            }),
    }
}

fn is_external(model: &Model, name: &str) -> bool {
    model.external_functions.iter().any(|stmt| {
        stmt.name
            .as_ref()
            .is_some_and(|(id, _)| model.name(*id) == name)
    })
}

fn is_pac_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "pac_expectation" | "var_expectation" | "pac_target_nonstationary"
    )
}

/// An unresolved include or macro can add, drop, or replace a selected equation
/// even when the equations that did parse already name their symbols.
fn unresolved_selection(model: &Model, missing_include: bool) -> Option<(UnsupportedKind, String)> {
    let structure = model_structure(model);
    if missing_include || structure.includes {
        return Some((
            UnsupportedKind::Include,
            "an unresolved include may add, remove, or replace a selected equation".to_string(),
        ));
    }
    // `@#error` (E064) refuses a directive that did resolve. It does not hide
    // which equations were selected, so it is not an extraction failure.
    if structure.macro_type_errors || structure.macro_incomplete || structure.e062 || structure.e063
    {
        return Some((
            UnsupportedKind::Macro,
            "macro expansion did not resolve, so the selected equations may be incomplete"
                .to_string(),
        ));
    }
    None
}

fn refuse(
    model: &Model,
    selected: &[EqRef],
    closure: &Closure,
    missing_include: bool,
) -> Option<(UnsupportedKind, String)> {
    if let Some(detail) = macro_block(model, selected, closure) {
        return Some((UnsupportedKind::Macro, detail));
    }
    if let Some(detail) = include_block(model, closure, missing_include) {
        return Some((UnsupportedKind::Include, detail));
    }
    if let Some(detail) = pac_block(model, selected, closure) {
        return Some((UnsupportedKind::Pac, detail));
    }
    None
}

fn macro_block(model: &Model, selected: &[EqRef], closure: &Closure) -> Option<String> {
    let retained = retained_places(model, selected, closure);
    for idx in &retained {
        let text = equation_line(model, *idx);
        if contains_macro(&text) {
            return Some("macro expansion failed on text this extract would retain".to_string());
        }
    }
    if declaration_has_macro(model, closure)
        || declaration_lines(model, closure, &retained)
            .iter()
            .any(|line| contains_macro(line))
    {
        return Some("a retained declaration still contains unexpanded macro text".to_string());
    }
    for (span, _, _) in &model.macro_type_errors {
        if model
            .model_block
            .is_some_and(|block| overlaps(*span, block))
            || retained
                .iter()
                .any(|place| overlaps(*span, eq_at(model, *place).span))
        {
            return Some("macro expansion failed on text this extract would retain".to_string());
        }
    }
    for diag in check_e063(model) {
        if retained
            .iter()
            .any(|place| overlaps(diag.span, eq_at(model, *place).span))
            || declaration_statement_overlaps(model, closure, diag.span)
        {
            return Some("macro expansion failed on text this extract would retain".to_string());
        }
    }
    None
}

fn contains_macro(text: &str) -> bool {
    text.contains("@{") || text.contains("@#")
}

fn for_bodies(model: &Model) -> Vec<Span> {
    let mut stack = Vec::new();
    let mut bodies = Vec::new();
    for directive in &model.macro_directives {
        if directive.kind == "for" {
            stack.push(directive.span.end);
        } else if directive.kind == "endfor" {
            if let Some(start) = stack.pop() {
                bodies.push(Span {
                    start,
                    end: directive.span.start,
                });
            }
        }
    }
    bodies
}

fn declaration_has_macro(model: &Model, closure: &Closure) -> bool {
    let statements = declaration_statement_spans(&model.source);
    let lists = [
        &model.endogenous,
        &model.deterministic_exogenous,
        &model.exogenous,
        &model.parameters,
        &model.predetermined,
    ];
    for decls in lists {
        for decl in decls {
            let name = model.name(decl.name);
            if !closure.symbols.contains(name) {
                continue;
            }
            let stmt = statement_in(&statements, decl.span);
            let text = &model.source[stmt.start as usize..stmt.end as usize];
            if !contains_macro(text) {
                continue;
            }
            match expanded_decl_line(model, decl, text) {
                Some(line) if !contains_macro(&line) => {}
                _ => return true,
            }
        }
    }
    model.model_local_variables.iter().any(|decl| {
        if !decl_matches_local(model, closure, decl) {
            return false;
        }
        let stmt = statement_in(&statements, decl.span);
        let text = &model.source[stmt.start as usize..stmt.end as usize];
        if !contains_macro(text) {
            return false;
        }
        !matches!(expanded_decl_line(model, decl, text), Some(line) if !contains_macro(&line))
    })
}

fn declaration_statement_overlaps(model: &Model, closure: &Closure, span: Span) -> bool {
    let statements = declaration_statement_spans(&model.source);
    let lists = [
        &model.endogenous,
        &model.deterministic_exogenous,
        &model.exogenous,
        &model.parameters,
        &model.predetermined,
    ];
    lists.iter().any(|decls| {
        decls.iter().any(|decl| {
            let name = model.name(decl.name);
            if !closure.symbols.contains(name) {
                return false;
            }
            let stmt = statement_in(&statements, decl.span);
            overlaps(span, stmt)
        })
    }) || model.model_local_variables.iter().any(|decl| {
        if !decl_matches_local(model, closure, decl) {
            return false;
        }
        let stmt = statement_in(&statements, decl.span);
        overlaps(span, stmt)
    })
}

fn include_block(model: &Model, closure: &Closure, missing_include: bool) -> Option<String> {
    if model.includes.is_empty() && !missing_include {
        return None;
    }
    let missing = closure
        .symbols
        .iter()
        .find(|name| !declared(model, name) && !closure_has_local(closure, name));
    missing.map(|name| format!("include may declare `{name}`, which this file does not"))
}

fn declared(model: &Model, name: &str) -> bool {
    decl_has(&model.endogenous, model, name)
        || decl_has(&model.exogenous, model, name)
        || decl_has(&model.deterministic_exogenous, model, name)
        || decl_has(&model.parameters, model, name)
        || decl_has(&model.model_local_variables, model, name)
        || model
            .trend_vars
            .iter()
            .any(|row| model.name(row.name) == name)
        || is_external(model, name)
}

fn decl_has(decls: &[Decl], model: &Model, name: &str) -> bool {
    decls.iter().any(|decl| model.name(decl.name) == name)
}

fn pac_block(model: &Model, selected: &[EqRef], closure: &Closure) -> Option<String> {
    let retained = retained_places(model, selected, closure);
    for operator in &model.named_model_operators {
        if retained
            .iter()
            .any(|place| overlaps(operator.span, eq_at(model, *place).span))
        {
            return Some("a retained equation uses a PAC or VAR expectation operator".to_string());
        }
    }
    for place in &retained {
        let eq = eq_at(model, *place);
        if expr_has_pac(model, eq.lhs_expr) || expr_has_pac(model, eq.rhs_expr) {
            return Some("a retained equation uses a PAC or VAR expectation operator".to_string());
        }
    }
    None
}

fn expr_has_pac(model: &Model, id: Option<ExprId>) -> bool {
    let Some(id) = id else {
        return false;
    };
    let mut calls = Vec::new();
    let mut idents = Vec::new();
    walk(model, id, &mut idents, &mut calls);
    calls.iter().any(|name| is_pac_name(model.name(*name)))
}

struct KeptEquation {
    place: EqRef,
    role: EquationRole,
}

/// Selector hits, plus the static/dynamic partners and OccBin same-name
/// regimes of those hits, in source order. Partners stay inside one scope:
/// the aggregate model, or one heterogeneity dimension. A row that matched
/// the selector stays `Requested`. Companions are not walked again.
fn kept_equations(model: &Model, report: &ExpandReport, requested: &[EqRef]) -> Vec<KeptEquation> {
    let mut roles = HashMap::new();
    for &place in requested {
        roles.entry(place).or_insert(EquationRole::Requested);
    }
    for &place in requested {
        for other in related_equations(model, place) {
            roles.entry(other).or_insert(EquationRole::Companion);
        }
    }
    let mut kept: Vec<KeptEquation> = roles
        .into_iter()
        .map(|(place, role)| KeptEquation { place, role })
        .collect();
    kept.sort_by_key(|row| row_order(report, row.place));
    kept
}

fn related_equations(model: &Model, place: EqRef) -> Vec<EqRef> {
    let eq = eq_at(model, place);
    if eq.is_local {
        return Vec::new();
    }
    let scope = scope_key(model, place);
    let partner = aggregate_partner(model, place);
    let mut others = Vec::new();
    if scope.is_none() {
        for (index, other) in model.equations.iter().enumerate() {
            let other_place = EqRef { block: None, index };
            if other_place != place
                && !other.is_local
                && (Some(other_place) == partner || occbin_same_name(eq, other))
            {
                others.push(other_place);
            }
        }
        return others;
    }
    for (block, model_block) in model.heterogeneous_models.iter().enumerate() {
        if Some(model_block.dimension) != scope {
            continue;
        }
        for (index, other) in model_block.equations.iter().enumerate() {
            let other_place = EqRef {
                block: Some(block),
                index,
            };
            if other_place != place && !other.is_local && occbin_same_name(eq, other) {
                others.push(other_place);
            }
        }
    }
    others
}

/// `None` is the aggregate model. `Some` is one heterogeneity dimension.
fn scope_key(model: &Model, place: EqRef) -> Option<crate::intern::Name> {
    place
        .block
        .map(|block| model.heterogeneous_models[block].dimension)
}

/// Dynare's static model consumes the next static-only row for each
/// dynamic-only row, independent of their names or left-hand expressions.
fn aggregate_partner(model: &Model, place: EqRef) -> Option<EqRef> {
    if place.block.is_some() {
        return None;
    }
    let row = eq_at(model, place);
    let own_static = row.static_tag && !row.dynamic_tag;
    let own_dynamic = row.dynamic_tag && !row.static_tag;
    if !own_static && !own_dynamic {
        return None;
    }
    let ordinal = model
        .equations
        .iter()
        .take(place.index + 1)
        .filter(|eq| {
            !eq.is_local
                && if own_static {
                    eq.static_tag && !eq.dynamic_tag
                } else {
                    eq.dynamic_tag && !eq.static_tag
                }
        })
        .count()
        - 1;
    model
        .equations
        .iter()
        .enumerate()
        .filter(|(_, eq)| {
            !eq.is_local
                && if own_static {
                    eq.dynamic_tag && !eq.static_tag
                } else {
                    eq.static_tag && !eq.dynamic_tag
                }
        })
        .nth(ordinal)
        .map(|(index, _)| EqRef { block: None, index })
}

fn required_partner_problem(model: &Model, kept: &[EqRef]) -> Option<String> {
    for &place in kept {
        let eq = eq_at(model, place);
        if place.block.is_some()
            && (eq.tag_map.contains_key("bind") || eq.tag_map.contains_key("relax"))
        {
            return Some(
                "a heterogeneous bind/relax equation has no supported OccBin regime relation"
                    .to_string(),
            );
        }
        if !eq.static_tag && !eq.dynamic_tag {
            continue;
        }
        if place.block.is_some() {
            if eq.static_tag {
                return Some(
                    "a heterogeneous [static] equation has no supported replacement relation"
                        .to_string(),
                );
            }
            continue;
        }
        if aggregate_partner(model, place).is_none() {
            return Some(format!(
                "a [{}] equation has no corresponding [{}] equation",
                if eq.static_tag { "static" } else { "dynamic" },
                if eq.static_tag { "dynamic" } else { "static" }
            ));
        }
    }
    None
}

/// A bind/relax equation keeps every other equation in the same scope with its name.
fn occbin_same_name(left: &Equation, right: &Equation) -> bool {
    !left.name.is_empty()
        && left.name == right.name
        && (left.tag_map.contains_key("bind") || left.tag_map.contains_key("relax"))
}

struct OccbinPiece {
    text: String,
    before_model: bool,
}

/// Named definitions are required context for retained OccBin regimes.
fn required_constraints<'a>(
    model: &'a Model,
    kept: &[EqRef],
) -> Result<Vec<&'a crate::model::OccbinConstraint>, String> {
    let mut names = HashSet::new();
    for &place in kept {
        names.extend(tag_constraint_names(eq_at(model, place)));
    }
    for name in &names {
        if !model.occbin_constraints.iter().any(|row| row.name == *name) {
            return Err(format!(
                "constraint `{name}` has no occbin_constraints definition"
            ));
        }
    }
    Ok(model
        .occbin_constraints
        .iter()
        .filter(|row| names.contains(&row.name))
        .collect())
}

/// Named constraints cited by retained `bind` / `relax` tags, as one block.
/// A definition whose span still contains `@#` or `@{` cannot be rendered.
fn occbin_setup(
    model: &Model,
    defined: &[&crate::model::OccbinConstraint],
    retained: &[EqRef],
) -> Result<Option<OccbinPiece>, String> {
    if defined.is_empty() {
        return Ok(None);
    }
    let mut parts = Vec::new();
    for constraint in defined {
        let text = slice_through_semi(&model.source, constraint.span);
        if contains_macro(&text) {
            return Err(
                "an occbin_constraints entry still contains unexpanded macro text".to_string(),
            );
        }
        parts.push(text);
    }
    let before_model = earliest_model_start(model, retained).is_some_and(|start| {
        defined
            .iter()
            .all(|constraint| constraint.span.start < start)
    });
    Ok(Some(OccbinPiece {
        text: format_occbin_block(&parts),
        before_model,
    }))
}

fn tag_constraint_names(eq: &Equation) -> Vec<String> {
    let mut names = Vec::new();
    for key in ["bind", "relax"] {
        let Some(value) = eq.tag_map.get(key) else {
            continue;
        };
        for piece in value.split(',') {
            let name = piece.trim();
            if !name.is_empty() {
                names.push(name.to_string());
            }
        }
    }
    names
}

fn format_occbin_block(parts: &[String]) -> String {
    let mut lines = vec!["occbin_constraints;".to_string()];
    lines.extend(parts.iter().map(|part| part.trim().to_string()));
    lines.push("end;".to_string());
    lines.join("\n")
}

fn retained_places(model: &Model, selected: &[EqRef], closure: &Closure) -> Vec<EqRef> {
    let mut places = selected.to_vec();
    places.extend(closure.local_places.iter().copied());
    places.sort_by(|left, right| {
        eq_at(model, *left)
            .span
            .start
            .cmp(&eq_at(model, *right).span.start)
            .then(left.block.cmp(&right.block))
            .then(left.index.cmp(&right.index))
    });
    places.dedup();
    places
}

fn earliest_model_start(model: &Model, retained: &[EqRef]) -> Option<u32> {
    let mut start = None;
    let mut consider = |at: u32| {
        start = Some(start.map_or(at, |cur: u32| cur.min(at)));
    };
    if retained.iter().any(|place| place.block.is_none()) {
        if let Some(block) = model.model_block {
            consider(block.start);
        }
    }
    for place in retained {
        if let Some(block) = place.block {
            consider(model.heterogeneous_models[block].span.start);
        }
    }
    start
}

fn overlaps(left: Span, right: Span) -> bool {
    left.start < right.end && right.start < left.end
}

fn selected_row(model: &Model, place: EqRef, role: EquationRole) -> SelectedEquation {
    let eq = eq_at(model, place);
    let name = if eq.name.is_empty() {
        None
    } else {
        Some(eq.name.clone())
    };
    let (domain, dimension) = match place.block {
        None => (EquationDomain::Aggregate, None),
        Some(block) => (
            EquationDomain::Heterogeneous,
            Some(
                model
                    .name(model.heterogeneous_models[block].dimension)
                    .to_string(),
            ),
        ),
    };
    SelectedEquation {
        domain,
        dimension,
        index: public_index(model, place),
        name,
        role,
        tags: eq.tag_map.clone(),
    }
}

/// `[static]` is not a counted equation. Aggregate counted rows keep their
/// counted index. Heterogeneous counted rows keep the continuing index inside
/// that dimension.
fn public_index(model: &Model, place: EqRef) -> Option<usize> {
    if eq_at(model, place).static_tag {
        return None;
    }
    match place.block {
        None => Some(counted_index(model, place.index)),
        Some(block) => Some(heterogeneous_counted_index(model, block, place.index)),
    }
}

fn counted_index(model: &Model, eq_index: usize) -> usize {
    model
        .equations
        .iter()
        .take(eq_index + 1)
        .filter(|eq| !eq.is_local && !eq.static_tag)
        .count()
        - 1
}

fn heterogeneous_counted_index(model: &Model, block: usize, eq_index: usize) -> usize {
    let dimension = model.heterogeneous_models[block].dimension;
    let mut count = 0;
    for (block_index, model_block) in model.heterogeneous_models.iter().enumerate() {
        if model_block.dimension != dimension {
            continue;
        }
        for (index, eq) in model_block.equations.iter().enumerate() {
            if block_index == block && index == eq_index {
                return count;
            }
            if !eq.is_local && !eq.static_tag {
                count += 1;
            }
        }
    }
    count
}

fn render(
    model: &Model,
    report: &ExpandReport,
    selected: &[EqRef],
    closure: &Closure,
    occbin: Option<&OccbinPiece>,
) -> String {
    let mut retained = retained_places(model, selected, closure);
    retained.sort_by_key(|place| row_order(report, *place));
    let mut lines = declaration_lines(model, closure, &retained);
    if let Some(piece) = occbin.filter(|piece| piece.before_model) {
        push_section(&mut lines, &piece.text);
    }
    for text in model_sections(model, report, &retained) {
        push_section(&mut lines, &text);
    }
    if let Some(piece) = occbin.filter(|piece| !piece.before_model) {
        push_section(&mut lines, &piece.text);
    }
    let mut text = lines.join("\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

fn model_sections(model: &Model, report: &ExpandReport, retained: &[EqRef]) -> Vec<String> {
    let mut sections = Vec::new();
    if retained.iter().any(|place| place.block.is_none()) {
        let start = retained
            .iter()
            .filter(|place| place.block.is_none())
            .map(|place| row_order(report, *place))
            .min()
            .unwrap_or(usize::MAX);
        sections.push((start, model_section(model, None, retained)));
    }
    let mut seen_blocks = BTreeSet::new();
    for place in retained {
        let Some(block) = place.block else {
            continue;
        };
        if !seen_blocks.insert(block) {
            continue;
        }
        sections.push((
            retained
                .iter()
                .filter(|place| place.block == Some(block))
                .map(|place| row_order(report, *place))
                .min()
                .unwrap_or(usize::MAX),
            model_section(model, Some(block), retained),
        ));
    }
    sections.sort_by_key(|(start, _)| *start);
    sections.into_iter().map(|(_, text)| text).collect()
}

fn model_section(model: &Model, block: Option<usize>, retained: &[EqRef]) -> String {
    let opener = match block {
        None => model
            .model_block
            .map(|span| model_opener(&model.source, span))
            .unwrap_or_else(|| "model;".to_string()),
        Some(block) => model_opener(&model.source, model.heterogeneous_models[block].span),
    };
    let mut body = vec![opener];
    for place in retained {
        if place.block == block {
            body.push(equation_line(model, *place));
        }
    }
    body.push("end;".to_string());
    body.join("\n")
}

fn push_section(lines: &mut Vec<String>, section: &str) {
    if !lines.is_empty() {
        lines.push(String::new());
    }
    lines.push(section.to_string());
}

fn declaration_lines(model: &Model, closure: &Closure, retained: &[EqRef]) -> Vec<String> {
    let statements = declaration_statement_spans(&model.source);
    let mut chunks: Vec<(u32, String)> = Vec::new();
    push_dimension_lines(&mut chunks, model, closure, retained);
    push_decl_lines(
        &mut chunks,
        model,
        &statements,
        &model.endogenous,
        &closure.symbols,
    );
    push_decl_lines(
        &mut chunks,
        model,
        &statements,
        &model.deterministic_exogenous,
        &closure.symbols,
    );
    push_decl_lines(
        &mut chunks,
        model,
        &statements,
        &model.exogenous,
        &closure.symbols,
    );
    push_decl_lines(
        &mut chunks,
        model,
        &statements,
        &model.parameters,
        &closure.symbols,
    );
    push_local_decl_lines(
        &mut chunks,
        model,
        &statements,
        &model.model_local_variables,
        closure,
    );
    push_decl_lines(
        &mut chunks,
        model,
        &statements,
        &model.predetermined,
        &closure.symbols,
    );
    push_trend_lines(
        &mut chunks,
        model,
        &statements,
        &model.trend_vars,
        &closure.symbols,
    );
    for stmt in &model.external_functions {
        let Some((name, _)) = stmt.name else {
            continue;
        };
        if closure.externals.contains(model.name(name)) {
            chunks.push((
                stmt.span.start,
                slice_through_semi(&model.source, stmt.span),
            ));
        }
    }
    let mut seen = HashSet::new();
    let mut ordered: Vec<_> = chunks
        .into_iter()
        .filter(|(start, line)| seen.insert((*start, line.clone())))
        .map(|(start, line)| {
            (
                declaration_order(model, &statements, start, &line),
                start,
                line,
            )
        })
        .collect();
    // Declarations retain their written metadata. Replay successful type changes
    // among required declarations, in parser order rather than macro span order.
    for change in &model.change_type_statements {
        let names: Vec<&str> = change
            .names
            .iter()
            .filter(|(name, _)| {
                closure.symbols.contains(model.name(*name))
                    && change.known_names.contains(name)
                    && !change.used_names.contains(name)
            })
            .map(|(name, _)| model.name(*name))
            .collect();
        if names.is_empty() {
            continue;
        }
        let kind = match change.new_type {
            crate::model::ChangeTypeKind::Parameters => "parameters",
            crate::model::ChangeTypeKind::Var => "var",
            crate::model::ChangeTypeKind::Varexo => "varexo",
            crate::model::ChangeTypeKind::VarexoDet => "varexo_det",
        };
        ordered.push((
            (change.parse_order, 1, change.span.start),
            change.span.start,
            format!("change_type({kind}) {};", names.join(" ")),
        ));
    }
    ordered.sort_by_key(|(order, _, _)| *order);
    ordered.into_iter().map(|(_, _, line)| line).collect()
}

fn declaration_order(
    model: &Model,
    statements: &[Span],
    start: u32,
    line: &str,
) -> (usize, u8, u32) {
    let mut rest = skip_noise(line.trim().trim_end_matches(';'));
    let keyword = take_ident(&mut rest).unwrap_or_default();
    rest = skip_noise(rest);
    if rest.starts_with('(') {
        let _ = take_balanced(&mut rest, '(', ')');
    }
    let names: HashSet<_> = declaration_pieces(rest)
        .into_iter()
        .filter_map(first_decl_name)
        .collect();
    if keyword == "heterogeneity_dimension" {
        return model
            .heterogeneity_dimensions
            .iter()
            .filter(|row| row.span.start == start && names.contains(model.name(row.name)))
            .map(|row| (row.parse_order, 0, start))
            .min()
            .unwrap_or((usize::MAX, 0, start));
    }
    if keyword == "external_function" {
        return model
            .external_functions
            .iter()
            .find(|row| row.span.start == start)
            .map(|row| (row.parse_order, 0, start))
            .unwrap_or((usize::MAX, 0, start));
    }
    if matches!(keyword.as_str(), "trend_var" | "log_trend_var") {
        return model
            .trend_vars
            .iter()
            .filter(|row| {
                names.contains(model.name(row.name))
                    && (row.span.start == start
                        || statement_in(statements, row.span).start == start)
            })
            .map(|row| (row.parse_order, 1, start))
            .min()
            .unwrap_or((usize::MAX, 1, start));
    }
    model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.deterministic_exogenous)
        .chain(&model.parameters)
        .chain(&model.predetermined)
        .chain(&model.model_local_variables)
        .filter(|decl| {
            names.contains(model.name(decl.name))
                && (decl.span.start == start || statement_in(statements, decl.span).start == start)
        })
        .map(|decl| (decl.parse_order, 0, start))
        .min()
        .unwrap_or((usize::MAX, 1, start))
}

fn equation_line(model: &Model, place: EqRef) -> String {
    let eq = eq_at(model, place);
    let raw = slice_through_semi(&model.source, eq.span);
    if contains_macro(&raw)
        || for_bodies(model)
            .iter()
            .any(|body| overlaps(eq.span, *body))
    {
        return expanded_equation(eq);
    }
    raw
}

fn expanded_equation(eq: &Equation) -> String {
    let mut out = String::new();
    if !eq.tag_map.is_empty() {
        let parts: Vec<String> = eq
            .tag_map
            .iter()
            .map(|(key, value)| {
                if value.is_empty() {
                    key.clone()
                } else {
                    format!("{key}='{value}'")
                }
            })
            .collect();
        out.push('[');
        out.push_str(&parts.join(", "));
        out.push_str("]\n");
    }
    let body = eq.text.trim();
    if contains_macro(body) {
        return body.to_string();
    }
    out.push_str(body);
    if !body.ends_with(';') {
        out.push(';');
    }
    out
}

fn expanded_decl_line(model: &Model, decl: &Decl, stmt: &str) -> Option<String> {
    let mut rest = skip_noise(stmt.trim().trim_end_matches(';').trim());
    let keyword = take_ident(&mut rest)?;
    let name = model.name(decl.name);
    if contains_macro(name) {
        return None;
    }
    if stmt.contains('$') && decl.tex_name.is_none() {
        return None;
    }
    let mut options = Vec::new();
    if let Some((dimension, _)) = decl.heterogeneity {
        // The grammar takes `heterogeneity=` alone. A second option is a syntax error.
        options.push(format!("heterogeneity={}", model.name(dimension)));
    } else {
        if decl.log_transform {
            options.push("log".to_string());
        }
        if let Some(row) = model
            .nonstationary_vars
            .iter()
            .find(|row| row.name == decl.name && row.span == decl.span)
        {
            let id = row.deflator?;
            let text = expr_text(model, id)?;
            let key = if row.log_deflator {
                "log_deflator"
            } else {
                "deflator"
            };
            options.push(format!("{key}={text}"));
        }
    }
    let mut line = keyword;
    if !options.is_empty() {
        line.push('(');
        line.push_str(&options.join(", "));
        line.push(')');
    }
    line.push(' ');
    line.push_str(name);
    if let Some(tex) = &decl.tex_name {
        if contains_macro(tex) {
            return None;
        }
        line.push_str(" $");
        line.push_str(tex);
        line.push('$');
    }
    if let Some(long_name) = &decl.long_name {
        if contains_macro(long_name) {
            return None;
        }
        line.push_str(" (long_name='");
        line.push_str(long_name);
        line.push_str("')");
    }
    line.push(';');
    Some(line)
}

fn expr_text(model: &Model, id: ExprId) -> Option<String> {
    let expr = model.exprs.get(id);
    let start = expr.span.start as usize;
    let end = expr.span.end as usize;
    if start < end && end <= model.source.len() {
        let slice = model.source[start..end].trim();
        if !slice.is_empty() && !contains_macro(slice) {
            return Some(slice.to_string());
        }
    }
    match &expr.kind {
        ExprKind::Ident { name, timing, .. } if *timing == 0 => Some(model.name(*name).to_string()),
        _ => None,
    }
}

fn push_dimension_lines(
    out: &mut Vec<(u32, String)>,
    model: &Model,
    closure: &Closure,
    retained: &[EqRef],
) {
    let mut needed = BTreeSet::new();
    for place in retained {
        if let Some(block) = place.block {
            needed.insert(
                model
                    .name(model.heterogeneous_models[block].dimension)
                    .to_string(),
            );
        }
    }
    for decls in [
        &model.endogenous,
        &model.exogenous,
        &model.deterministic_exogenous,
        &model.parameters,
    ] {
        for decl in decls {
            if !closure.symbols.contains(model.name(decl.name)) {
                continue;
            }
            if let Some((dimension, _)) = decl.heterogeneity {
                needed.insert(model.name(dimension).to_string());
            }
        }
    }
    let mut groups: BTreeMap<u32, Vec<&crate::model::HeterogeneityDimension>> = BTreeMap::new();
    for dimension in &model.heterogeneity_dimensions {
        groups
            .entry(dimension.span.start)
            .or_default()
            .push(dimension);
    }
    for (start, dims) in groups {
        let keep: Vec<_> = dims
            .iter()
            .filter(|dimension| needed.contains(model.name(dimension.name)))
            .collect();
        if keep.is_empty() {
            continue;
        }
        let line = if keep.len() == dims.len() {
            slice_through_semi(&model.source, dims[0].span)
        } else {
            let names = keep
                .iter()
                .map(|dimension| model.name(dimension.name))
                .collect::<Vec<_>>()
                .join(", ");
            format!("heterogeneity_dimension {names};")
        };
        out.push((start, line));
    }
}

fn push_local_decl_lines(
    out: &mut Vec<(u32, String)>,
    model: &Model,
    statements: &[Span],
    decls: &[Decl],
    closure: &Closure,
) {
    let mut seen = HashSet::new();
    for decl in decls {
        if !decl_matches_local(model, closure, decl) {
            continue;
        }
        let needed = BTreeSet::from([model.name(decl.name).to_string()]);
        let stmt = statement_in(statements, decl.span);
        let text = &model.source[stmt.start as usize..stmt.end as usize];
        if contains_macro(text) {
            if let Some(line) = expanded_decl_line(model, decl, text) {
                out.push((decl.span.start, line));
            }
            continue;
        }
        if !seen.insert(stmt.start) {
            continue;
        }
        if let Some(line) = filter_declaration(text, &needed) {
            out.push((stmt.start, line));
        }
    }
}

fn push_decl_lines(
    out: &mut Vec<(u32, String)>,
    model: &Model,
    statements: &[Span],
    decls: &[Decl],
    needed: &BTreeSet<String>,
) {
    let mut seen = HashSet::new();
    for decl in decls {
        if !needed.contains(model.name(decl.name)) {
            continue;
        }
        let stmt = statement_in(statements, decl.span);
        let text = &model.source[stmt.start as usize..stmt.end as usize];
        if contains_macro(text) {
            if let Some(line) = expanded_decl_line(model, decl, text) {
                out.push((decl.span.start, line));
            }
            continue;
        }
        if !seen.insert(stmt.start) {
            continue;
        }
        if let Some(line) = filter_declaration(text, needed) {
            out.push((stmt.start, line));
        }
    }
}

fn push_trend_lines(
    out: &mut Vec<(u32, String)>,
    model: &Model,
    statements: &[Span],
    rows: &[TrendVar],
    needed: &BTreeSet<String>,
) {
    let mut seen = HashSet::new();
    for row in rows {
        if !needed.contains(model.name(row.name)) {
            continue;
        }
        let stmt = statement_in(statements, row.span);
        if !seen.insert(stmt.start) {
            continue;
        }
        if let Some(line) = filter_declaration(
            &model.source[stmt.start as usize..stmt.end as usize],
            needed,
        ) {
            out.push((stmt.start, line));
        }
    }
}

const DECL_KEYWORDS: &[&str] = &[
    "predetermined_variables",
    "model_local_variable",
    "log_trend_var",
    "varexo_det",
    "trend_var",
    "parameters",
    "varexo",
    "var",
];

fn declaration_statement_spans(source: &str) -> Vec<Span> {
    use crate::lexer::{tokenize, TokenKind};
    let mut spans = Vec::new();
    let mut start = None;
    let mut depth = 0usize;
    for token in tokenize(source) {
        if token.kind == TokenKind::Semi || token.kind == TokenKind::Eof {
            if let Some(start) = start.take() {
                spans.push(Span {
                    start,
                    end: token.span.end,
                });
            }
            depth = 0;
            continue;
        }
        if start.is_none()
            && depth == 0
            && token.kind == TokenKind::Ident
            && DECL_KEYWORDS.contains(&token.text(source))
        {
            start = Some(token.span.start);
        }
        match token.kind {
            TokenKind::LParen | TokenKind::LBrack => depth += 1,
            TokenKind::RParen | TokenKind::RBrack => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    spans
}

fn statement_in(spans: &[Span], name: Span) -> Span {
    let at = spans.partition_point(|span| span.end <= name.start);
    spans
        .get(at)
        .copied()
        .filter(|span| span.start <= name.start && name.end <= span.end)
        .unwrap_or(name)
}

fn filter_declaration(stmt: &str, needed: &BTreeSet<String>) -> Option<String> {
    let mut rest = skip_noise(stmt.trim().trim_end_matches(';').trim());
    let keyword = take_ident(&mut rest)?;
    rest = skip_noise(rest);
    let options = if rest.starts_with('(') {
        let opt = take_balanced(&mut rest, '(', ')')?;
        rest = skip_noise(rest);
        Some(opt)
    } else {
        None
    };
    let pieces = declaration_pieces(rest);
    let mut kept = Vec::new();
    for piece in pieces {
        let trimmed = piece.trim();
        if trimmed.is_empty() {
            continue;
        }
        if first_decl_name(trimmed).is_some_and(|name| needed.contains(&name)) {
            kept.push(trimmed.to_string());
        }
    }
    if kept.is_empty() {
        return None;
    }
    let mut line = keyword;
    if let Some(opt) = options {
        line.push_str(&opt);
    }
    line.push(' ');
    line.push_str(&kept.join(", "));
    line.push(';');
    Some(line)
}

fn skip_noise(input: &str) -> &str {
    let mut rest = input;
    loop {
        rest = rest.trim_start();
        if let Some(stripped) = rest.strip_prefix("//").or_else(|| rest.strip_prefix('%')) {
            rest = stripped
                .split_once('\n')
                .map(|(_, tail)| tail)
                .unwrap_or("");
            continue;
        }
        if let Some(stripped) = rest.strip_prefix("/*") {
            rest = stripped
                .split_once("*/")
                .map(|(_, tail)| tail)
                .unwrap_or("");
            continue;
        }
        return rest;
    }
}

/// Dynare declaration names may be separated by spaces, commas, or comments.
/// TeX and per-name options belong to the preceding name; token boundaries keep
/// commas/parentheses inside quoted metadata from becoming separators.
fn declaration_pieces(input: &str) -> Vec<&str> {
    use crate::lexer::{tokenize, TokenKind};
    let mut pieces = Vec::new();
    let mut depth = 0usize;
    let mut start = None;
    let mut end = 0;
    for token in tokenize(input) {
        if depth == 0 && token.kind == TokenKind::Ident {
            if let Some(start) = start {
                pieces.push(&input[start..end]);
            }
            start = Some(token.span.start as usize);
        }
        if token.kind == TokenKind::Eof || (depth == 0 && token.kind == TokenKind::Comma) {
            continue;
        }
        match token.kind {
            TokenKind::LParen => depth += 1,
            TokenKind::RParen => depth = depth.saturating_sub(1),
            _ => {}
        }
        end = token.span.end as usize;
    }
    if let Some(start) = start {
        pieces.push(&input[start..end]);
    }
    pieces
}

fn take_ident(input: &mut &str) -> Option<String> {
    let bytes = input.as_bytes();
    if bytes.first().is_none_or(|c| !is_ident_start(*c)) {
        return None;
    }
    let mut end = 1;
    while end < bytes.len() && is_ident_continue(bytes[end]) {
        end += 1;
    }
    let word = input[..end].to_string();
    *input = &input[end..];
    Some(word)
}

fn take_balanced(input: &mut &str, open: char, close: char) -> Option<String> {
    if !input.starts_with(open) {
        return None;
    }
    let mut depth = 0;
    for (idx, ch) in input.char_indices() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                let end = idx + ch.len_utf8();
                let taken = input[..end].to_string();
                *input = &input[end..];
                return Some(taken);
            }
        }
    }
    None
}

fn first_decl_name(input: &str) -> Option<String> {
    let mut rest = input;
    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest.starts_with("${") {
            let end = rest.find("$}")? + 2;
            rest = &rest[end..];
            continue;
        }
        if rest.starts_with('(') {
            take_balanced(&mut rest, '(', ')')?;
            continue;
        }
        if is_ident_start(rest.as_bytes()[0]) {
            let word = take_ident(&mut rest)?;
            if !matches!(word.as_str(), "long_name" | "latex_name" | "long") {
                return Some(word);
            }
            continue;
        }
        rest = &rest[rest.chars().next().unwrap().len_utf8()..];
    }
    None
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn model_opener(source: &str, block: Span) -> String {
    let text = &source[block.start as usize..block.end as usize];
    let mut rest = text;
    let Some(keyword) = take_ident(&mut rest) else {
        return "model;".to_string();
    };
    rest = rest.trim_start();
    let options = if rest.starts_with('(') {
        take_balanced(&mut rest, '(', ')').unwrap_or_default()
    } else {
        String::new()
    };
    format!("{keyword}{options};")
}

fn slice_through_semi(source: &str, span: Span) -> String {
    let bytes = source.as_bytes();
    let start = (span.start as usize).min(source.len());
    let mut end = (span.end as usize).min(source.len());
    while end < bytes.len() && bytes[end].is_ascii_whitespace() {
        end += 1;
    }
    if end < bytes.len() && bytes[end] == b';' {
        end += 1;
    }
    source[start..end].trim().to_string()
}

fn omitted(model: &Model) -> Vec<OmittedContext> {
    let mut out = Vec::new();
    if !model.param_assignments.is_empty()
        || !model.helper_assignments.is_empty()
        || model.ss_block.is_some()
        || !model.estimated_params.is_empty()
        || model.load_params_file.is_some()
    {
        out.push(OmittedContext {
            kind: "calibration".to_string(),
            detail: "parameter assignments and steady-state calibration are not in the fragment"
                .to_string(),
        });
    }
    if model.shocks_block.is_some() || !model.shock_blocks.is_empty() {
        out.push(OmittedContext {
            kind: "shocks".to_string(),
            detail: "shock blocks are not in the fragment".to_string(),
        });
    }
    if model.initval_block.is_some()
        || model.endval_block.is_some()
        || model.histval_block.is_some()
        || model.filter_initial_state_block.is_some()
    {
        out.push(OmittedContext {
            kind: "initialization".to_string(),
            detail:
                "initval, endval, histval, and filter_initial_state blocks are not in the fragment"
                    .to_string(),
        });
    }
    let classic_execution = model.stoch_simul_span.is_some()
        || model.estimation_span.is_some()
        || !model.simul_spans.is_empty()
        || model.perfect_foresight_solver_span.is_some()
        || model.perfect_foresight_setup_span.is_some()
        || model.steady_span.is_some()
        || model.check_span.is_some();
    let heterogeneity_execution = !model.heterogeneity_commands.is_empty();
    if classic_execution || heterogeneity_execution {
        let detail = if heterogeneity_execution && !classic_execution {
            "heterogeneity steady-state, solve, and simulate commands are not in the fragment"
        } else if heterogeneity_execution {
            "simulation, estimation, and heterogeneity commands are not in the fragment"
        } else {
            "simulation and estimation commands are not in the fragment"
        };
        out.push(OmittedContext {
            kind: "execution".to_string(),
            detail: detail.to_string(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::registered_tool_names;

    fn req(content: &str, names: &[&str], tags: &[(&str, &str)]) -> ExtractRequest {
        ExtractRequest {
            file_content: content.to_string(),
            names: names.iter().map(|name| (*name).to_string()).collect(),
            tags: tags
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                .collect(),
            ..ExtractRequest::default()
        }
    }

    const CLOSURE: &str = "\
var y, c, k;
varexo e, u;
parameters beta, alpha;
predetermined_variables k;
beta = 0.99;
model;
  # rho = 0.9;
  # phi = rho;
  [name='euler']
  c = beta * c(+1);
  [name='cap']
  k = alpha * k(-1) + phi;
  [name='other']
  y = e;
end;
shocks;
var e; stderr 0.1;
end;
stoch_simul;
";

    #[test]
    fn dependency_closure_keeps_locals_and_drops_unused_equations() {
        let result = extract(&req(CLOSURE, &["cap"], &[])).unwrap();
        assert_eq!(result.status, ExtractStatus::Ok);
        let fragment = result.fragment.unwrap();
        assert!(fragment.contains("var k;"), "{fragment}");
        assert!(fragment.contains("parameters alpha;"), "{fragment}");
        assert!(
            fragment.contains("predetermined_variables k;"),
            "{fragment}"
        );
        assert!(fragment.contains("# rho = 0.9;"), "{fragment}");
        assert!(fragment.contains("# phi = rho;"), "{fragment}");
        assert!(fragment.contains("[name='cap']"), "{fragment}");
        assert!(
            fragment.find("# rho = 0.9;").unwrap() < fragment.find("# phi = rho;").unwrap()
                && fragment.find("# phi = rho;").unwrap() < fragment.find("[name='cap']").unwrap(),
            "{fragment}"
        );
        assert!(!fragment.contains("name='euler'"), "{fragment}");
        assert!(!fragment.contains("name='other'"), "{fragment}");
        assert!(!fragment.contains("var y"), "{fragment}");
        assert!(!fragment.contains("beta"), "{fragment}");
        assert!(!fragment.contains("shocks"), "{fragment}");
        assert!(!fragment.contains("stoch_simul"), "{fragment}");
        assert!(!fragment.contains("0.99"), "{fragment}");
        assert_eq!(result.selected_equations.len(), 1);
        let row = &result.selected_equations[0];
        assert_eq!(row.domain, EquationDomain::Aggregate);
        assert_eq!(row.dimension, None);
        assert_eq!(row.index, Some(1));
        assert_eq!(row.name.as_deref(), Some("cap"));
        assert_eq!(row.role, EquationRole::Requested);
        assert!(result.origins[0].frames.is_empty());
        assert!(result
            .omitted_context
            .iter()
            .any(|item| item.kind == "calibration"));
        assert!(result
            .omitted_context
            .iter()
            .any(|item| item.kind == "shocks"));
        assert!(result
            .omitted_context
            .iter()
            .any(|item| item.kind == "execution"));
    }

    #[test]
    fn simple_equation_keeps_declaration_metadata_in_source_order() {
        let src = "\
parameters beta (long_name='discount'), delta;
var(log) y, c;
model(linear);
  [name='euler']
  c = beta * c(+1);
  [name='out']
  y = 1;
end;
";
        let result = extract(&req(src, &["euler"], &[])).unwrap();
        let fragment = result.fragment.unwrap();
        let beta = fragment
            .find("parameters beta (long_name='discount');")
            .unwrap();
        let var_c = fragment.find("var(log) c;").unwrap();
        assert!(beta < var_c, "{fragment}");
        assert!(fragment.contains("model(linear);"), "{fragment}");
        assert!(!fragment.contains("delta"), "{fragment}");
        assert!(!fragment.contains("name='out'"), "{fragment}");
        assert_eq!(result.selected_equations[0].index, Some(0));
    }

    #[test]
    fn per_name_metadata_and_declaration_locals_stay() {
        let src = "\
var c ${c}$ (long_name='consumption'), y (long_name='output');
trend_var(growth_factor = g) y;
model;
  # g = 1.02;
  [name='eq']
  y = c;
end;
";
        let result = extract(&req(src, &["eq"], &[])).unwrap();
        let fragment = result.fragment.expect("fragment");
        assert!(
            fragment.contains("c ${c}$ (long_name='consumption')"),
            "{fragment}"
        );
        assert!(fragment.contains("y (long_name='output')"), "{fragment}");
        assert!(fragment.contains("# g = 1.02;"), "{fragment}");
        assert!(!fragment.contains("delta"), "{fragment}");

        let macro_decl = "\
var y (long_name=@{lab});
model;
  [name='eq']
  y = 0;
end;
";
        let refused = extract(&req(macro_decl, &["eq"], &[])).unwrap();
        assert_eq!(refused.status, ExtractStatus::UnsupportedContext);
        assert!(refused.fragment.is_none());
    }

    #[test]
    fn names_are_or_and_tags_are_and() {
        let src = "\
var y, c;
model;
  [name='a', group='g']
  y = 0;
  [name='b', group='g']
  c = 0;
  [name='c', group='h']
  y = 1;
end;
";
        let both = extract(&req(src, &["a", "c"], &[("group", "g")])).unwrap();
        assert_eq!(
            both.selected_equations
                .iter()
                .map(|row| row.name.as_deref().unwrap())
                .collect::<Vec<_>>(),
            vec!["a"]
        );
        let tags = extract(&req(src, &[], &[("group", "g")])).unwrap();
        assert_eq!(tags.selected_equations.len(), 2);
        assert_eq!(tags.selected_equations[0].index, Some(0));
        assert_eq!(tags.selected_equations[1].index, Some(1));
    }

    #[test]
    fn empty_selector_is_rejected() {
        let err = extract(&req("var y; model; y = 0; end;", &[], &[])).unwrap_err();
        assert_eq!(err, ExtractError::EmptySelector);
    }

    #[test]
    fn no_match_is_an_empty_fragment() {
        let result = extract(&req(CLOSURE, &["missing"], &[])).unwrap();
        assert_eq!(result.status, ExtractStatus::Empty);
        assert_eq!(result.fragment.as_deref(), Some(""));
        assert!(result.selected_equations.is_empty());
        assert!(result.explanation.contains("No equation matched"));
    }

    #[test]
    fn unknown_dimension_is_empty() {
        let mut request = req(CLOSURE, &["euler"], &[]);
        request.dimension = Some("h".to_string());
        let result = extract(&request).unwrap();
        assert_eq!(result.status, ExtractStatus::Empty);
        assert_eq!(result.fragment.as_deref(), Some(""));
        assert!(result.selected_equations.is_empty());
        assert!(result.explanation.contains("dimension `h`"));
    }

    #[test]
    fn heterogeneous_equation_keeps_its_dimension_and_declaration() {
        let src = "\
heterogeneity_dimension h, g;
parameters beta;
var y;
var(heterogeneity=h) c, k;
var(heterogeneity=g) d;
model;
  [name='agg']
  y = 0;
end;
model(heterogeneity=h);
  # rho = 0.9;
  [name='het']
  c = beta * rho * c(-1);
  [name='other']
  k = 1;
end;
model(heterogeneity=g);
  [name='gone']
  d = 1;
end;
heterogeneity_solve;
";
        let het = extract(&req(src, &["het"], &[])).unwrap();
        assert_eq!(het.status, ExtractStatus::Ok);
        let fragment = het.fragment.expect("fragment");
        assert!(
            fragment.contains("heterogeneity_dimension h;"),
            "{fragment}"
        );
        assert!(
            !fragment.contains("heterogeneity_dimension h, g"),
            "{fragment}"
        );
        assert!(!fragment.contains("heterogeneity=g"), "{fragment}");
        assert!(fragment.contains("parameters beta;"), "{fragment}");
        assert!(fragment.contains("var(heterogeneity=h) c;"), "{fragment}");
        assert!(
            !fragment.contains("var(heterogeneity=h) c, k"),
            "{fragment}"
        );
        assert!(!fragment.contains('k'), "{fragment}");
        assert!(!fragment.contains("var y"), "{fragment}");
        assert!(fragment.contains("# rho = 0.9;"), "{fragment}");
        assert!(fragment.contains("model(heterogeneity=h);"), "{fragment}");
        assert!(fragment.contains("c = beta * rho * c(-1);"), "{fragment}");
        assert!(!fragment.contains("name='agg'"), "{fragment}");
        assert!(!fragment.contains("name='other'"), "{fragment}");
        assert!(!fragment.contains("name='gone'"), "{fragment}");
        assert!(!fragment.contains("\nmodel;"), "{fragment}");
        assert!(!fragment.contains("heterogeneity_solve"), "{fragment}");
        assert_eq!(het.selected_equations.len(), 1);
        let row = &het.selected_equations[0];
        assert_eq!(row.domain, EquationDomain::Heterogeneous);
        assert_eq!(row.dimension.as_deref(), Some("h"));
        assert_eq!(row.index, Some(0));
        assert_eq!(row.name.as_deref(), Some("het"));
        assert_eq!(row.role, EquationRole::Requested);
        assert_eq!(het.origins.len(), 1);
        assert_eq!(het.origins[0].domain, EquationDomain::Heterogeneous);
        assert_eq!(het.origins[0].dimension.as_deref(), Some("h"));
        assert_eq!(het.origins[0].index, Some(0));
        assert_eq!(het.origins[0].role, EquationRole::Requested);
        assert!(het
            .omitted_context
            .iter()
            .any(|item| item.kind == "execution"));

        let agg = extract(&req(src, &["agg"], &[])).unwrap();
        assert_eq!(agg.status, ExtractStatus::Ok);
        let agg_fragment = agg.fragment.expect("fragment");
        assert!(agg_fragment.contains("name='agg'"), "{agg_fragment}");
        assert!(agg_fragment.contains("var y;"), "{agg_fragment}");
        assert!(!agg_fragment.contains("heterogeneity"), "{agg_fragment}");
        assert_eq!(agg.selected_equations[0].domain, EquationDomain::Aggregate);
        assert_eq!(agg.selected_equations[0].dimension, None);
        assert_eq!(agg.selected_equations[0].index, Some(0));

        let mut narrowed = req(src, &["het"], &[]);
        narrowed.dimension = Some("g".to_string());
        let missed = extract(&narrowed).unwrap();
        assert_eq!(missed.status, ExtractStatus::Empty);
        assert_eq!(missed.fragment.as_deref(), Some(""));
        assert!(missed.explanation.contains("dimension `g`"));
    }

    #[test]
    fn aggregate_and_heterogeneous_rows_stay_distinct() {
        let src = "\
heterogeneity_dimension h;
var y;
var(heterogeneity=h) c;
model;
  [name='eq']
  y = 0;
end;
model(heterogeneity=h);
  [name='eq']
  c = 1;
end;
";
        let both = extract(&req(src, &["eq"], &[])).unwrap();
        assert_eq!(both.status, ExtractStatus::Ok);
        assert_eq!(both.selected_equations.len(), 2);
        assert_eq!(both.selected_equations[0].domain, EquationDomain::Aggregate);
        assert_eq!(both.selected_equations[0].dimension, None);
        assert_eq!(both.selected_equations[0].index, Some(0));
        assert_eq!(both.selected_equations[0].role, EquationRole::Requested);
        assert_eq!(
            both.selected_equations[1].domain,
            EquationDomain::Heterogeneous
        );
        assert_eq!(both.selected_equations[1].dimension.as_deref(), Some("h"));
        assert_eq!(both.selected_equations[1].index, Some(0));
        assert_eq!(both.selected_equations[1].role, EquationRole::Requested);
        assert_eq!(both.origins[0].index, Some(0));
        assert_eq!(both.origins[1].index, Some(0));
        assert_eq!(both.origins[0].domain, EquationDomain::Aggregate);
        assert_eq!(both.origins[1].domain, EquationDomain::Heterogeneous);
        let fragment = both.fragment.expect("fragment");
        assert!(fragment.contains("\nmodel;"), "{fragment}");
        assert!(fragment.contains("model(heterogeneity=h);"), "{fragment}");
        assert!(fragment.contains("y = 0;"), "{fragment}");
        assert!(fragment.contains("c = 1;"), "{fragment}");

        let mut only_h = req(src, &["eq"], &[]);
        only_h.dimension = Some("h".to_string());
        let het = extract(&only_h).unwrap();
        assert_eq!(het.selected_equations.len(), 1);
        assert_eq!(
            het.selected_equations[0].domain,
            EquationDomain::Heterogeneous
        );
        assert!(!het.fragment.unwrap().contains("y = 0"));
    }

    #[test]
    fn heterogeneous_partners_stay_inside_the_dimension() {
        let src = "\
heterogeneity_dimension h;
var y;
var(heterogeneity=h) c;
model;
  [name='agg']
  y = 0;
end;
model(heterogeneity=h);
  [name='eq', dynamic]
  c = 0;
  [name='also']
  c = 2;
end;
model(heterogeneity=h);
  [name='later']
  c = 3;
end;
";
        let result = extract(&req(src, &["eq"], &[])).unwrap();
        assert_eq!(result.status, ExtractStatus::Ok);
        assert_eq!(result.selected_equations.len(), 1);
        assert_eq!(result.selected_equations[0].role, EquationRole::Requested);
        assert_eq!(result.selected_equations[0].index, Some(0));
        assert_eq!(result.selected_equations[0].dimension.as_deref(), Some("h"));
        let fragment = result.fragment.expect("fragment");
        assert!(fragment.contains("c = 0;"), "{fragment}");
        assert!(!fragment.contains("c = 2"), "{fragment}");
        assert!(!fragment.contains("c = 3"), "{fragment}");
        assert!(!fragment.contains("name='also'"), "{fragment}");
        assert!(!fragment.contains("name='later'"), "{fragment}");
        assert!(!fragment.contains("y = 0"), "{fragment}");

        let later = extract(&req(src, &["later"], &[])).unwrap();
        assert_eq!(later.selected_equations.len(), 1);
        assert_eq!(later.selected_equations[0].role, EquationRole::Requested);
        assert_eq!(later.selected_equations[0].index, Some(2));
        assert_eq!(
            later.selected_equations[0].domain,
            EquationDomain::Heterogeneous
        );
        let later_fragment = later.fragment.expect("fragment");
        assert!(later_fragment.contains("c = 3;"), "{later_fragment}");
        assert!(!later_fragment.contains("c = 0"), "{later_fragment}");
        assert!(!later_fragment.contains("c = 2"), "{later_fragment}");
        assert!(!later_fragment.contains("name='also'"), "{later_fragment}");

        let cross = "\
heterogeneity_dimension h;
var y;
var(heterogeneity=h) c;
model;
  [name='agg']
  y = 0;
end;
model(heterogeneity=h);
  [static]
  y = 1;
end;
";
        let agg = extract(&req(cross, &["agg"], &[])).unwrap();
        assert_eq!(agg.selected_equations.len(), 1);
        let agg_fragment = agg.fragment.expect("fragment");
        assert!(!agg_fragment.contains("y = 1"), "{agg_fragment}");
        assert!(
            !agg_fragment.contains("model(heterogeneity=h)"),
            "{agg_fragment}"
        );

        let static_result = extract(&req(cross, &[], &[("static", "")])).unwrap();
        assert_eq!(static_result.status, ExtractStatus::UnsupportedContext);
        assert!(static_result.fragment.is_none());

        let regime = "\
heterogeneity_dimension h;
var(heterogeneity=h) c;
model(heterogeneity=h);
  [name='policy', bind='C'] c=0;
end;
";
        let regime_result = extract(&req(regime, &["policy"], &[])).unwrap();
        assert_eq!(regime_result.status, ExtractStatus::UnsupportedContext);
        assert!(regime_result.fragment.is_none());
        assert!(regime_result.explanation.contains("heterogeneous"));
    }

    #[test]
    fn heterogeneous_pac_has_no_fragment() {
        let src = "\
heterogeneity_dimension h;
var(heterogeneity=h) c;
parameters b;
model(heterogeneity=h);
  [name='p']
  c = b*pac_expectation(nope);
  [name='plain']
  c = b;
end;
";
        let pac = extract(&req(src, &["p"], &[])).unwrap();
        assert_eq!(pac.status, ExtractStatus::UnsupportedContext);
        assert!(pac.fragment.is_none());
        assert_eq!(pac.unsupported.unwrap().kind, UnsupportedKind::Pac);
        assert!(pac.explanation.contains("PAC"));

        let plain = extract(&req(src, &["plain"], &[])).unwrap();
        assert_eq!(plain.status, ExtractStatus::Ok);
        assert!(!plain.fragment.unwrap().contains("pac_expectation"));
    }

    #[test]
    fn pac_operator_is_unsupported_and_an_ordinary_neighbour_is_not() {
        let src = "\
var y, z;
varexo e, e2;
parameters b;
b = 0.8;
model;
  [name='Y']
  y = b*y(-1)+e+pac_expectation(nope);
  [name='Z']
  z = z(-1)+e2;
end;
";
        let pac = extract(&req(src, &["Y"], &[])).unwrap();
        assert_eq!(pac.status, ExtractStatus::UnsupportedContext);
        assert!(pac.fragment.is_none());
        assert_eq!(pac.unsupported.unwrap().kind, UnsupportedKind::Pac);

        let plain = extract(&req(src, &["Z"], &[])).unwrap();
        assert_eq!(plain.status, ExtractStatus::Ok);
        let fragment = plain.fragment.unwrap();
        assert!(fragment.contains("name='Z'"), "{fragment}");
        assert!(!fragment.contains("pac_expectation"), "{fragment}");
    }

    #[test]
    fn static_partner_follows_replacement_order_and_does_not_square_the_model() {
        let src = "\
var y, c;
model;
  [name='eq', dynamic]
  y = 0;
  [name='steady', static]
  c = 1;
  [name='other']
  c = 2;
end;
";
        let mut request = req(src, &["eq"], &[]);
        request.active_file = Some("partner.mod".to_string());
        let result = extract(&request).unwrap();
        assert_eq!(result.status, ExtractStatus::Ok);
        let fragment = result.fragment.expect("fragment");
        assert!(fragment.contains("[name='eq', dynamic]"), "{fragment}");
        assert!(fragment.contains("[name='steady', static]"), "{fragment}");
        assert!(fragment.contains("y = 0;"), "{fragment}");
        assert!(fragment.contains("c = 1;"), "{fragment}");
        assert!(!fragment.contains("name='other'"), "{fragment}");
        assert!(!fragment.contains("c = 2"), "{fragment}");
        assert!(fragment.contains("var y, c"), "{fragment}");
        assert_eq!(result.selected_equations.len(), 2);
        assert_eq!(result.selected_equations[0].role, EquationRole::Requested);
        assert_eq!(result.selected_equations[0].index, Some(0));
        assert_eq!(result.selected_equations[0].name.as_deref(), Some("eq"));
        assert_eq!(result.selected_equations[1].role, EquationRole::Companion);
        assert_eq!(result.selected_equations[1].index, None);
        assert_eq!(
            result.selected_equations[1]
                .tags
                .get("static")
                .map(String::as_str),
            Some("")
        );
        assert_eq!(result.origins.len(), 2);
        assert_eq!(result.origins[1].role, EquationRole::Companion);
        assert_eq!(result.origins[1].index, None);
        assert_eq!(result.origins[1].file.as_deref(), Some("partner.mod"));
        let static_src =
            &src[result.origins[1].span.start as usize..result.origins[1].span.end as usize];
        assert!(static_src.contains("c = 1"), "{static_src}");
        assert!(result.origins[1].frames.is_empty());

        let named = "\
var y;
model;
  [name='eq', dynamic]
  y = 0;
  [name='ss', static]
  y = 1;
end;
";
        let only_static = extract(&req(named, &["ss"], &[])).unwrap();
        assert_eq!(only_static.status, ExtractStatus::Ok);
        assert_eq!(only_static.selected_equations.len(), 2);
        assert_eq!(
            only_static.selected_equations[0].role,
            EquationRole::Companion
        );
        assert_eq!(only_static.selected_equations[0].index, Some(0));
        assert_eq!(
            only_static.selected_equations[0].name.as_deref(),
            Some("eq")
        );
        assert_eq!(
            only_static.selected_equations[1].role,
            EquationRole::Requested
        );
        assert_eq!(only_static.selected_equations[1].index, None);
        assert_eq!(only_static.origins[1].index, None);
        assert_eq!(only_static.origins[1].role, EquationRole::Requested);

        let lone = "\
var y;
model;
  [name='ss', static]
  y = 1;
  [name='dyn', dynamic]
  y = 2;
end;
";
        let dynamic_only = extract(&req(lone, &["dyn"], &[])).unwrap();
        assert_eq!(dynamic_only.status, ExtractStatus::Ok);
        assert_eq!(
            dynamic_only.selected_equations[0].role,
            EquationRole::Companion
        );
        assert_eq!(dynamic_only.selected_equations[0].index, None);
        assert_eq!(
            dynamic_only.selected_equations[1].role,
            EquationRole::Requested
        );
        assert_eq!(dynamic_only.selected_equations[1].index, Some(0));
        let dynamic_fragment = dynamic_only.fragment.unwrap();
        assert!(
            dynamic_fragment.contains("[name='dyn', dynamic]"),
            "{dynamic_fragment}"
        );
        assert!(
            dynamic_fragment.contains("[name='ss', static]"),
            "{dynamic_fragment}"
        );

        let third = "\
var y;
model;
  [name='eq', dynamic]
  y = 0;
  [static]
  y = 1;
  [name='also']
  y = 2;
end;
";
        let extra = extract(&req(third, &["eq"], &[])).unwrap();
        assert_eq!(extra.status, ExtractStatus::Ok);
        assert_eq!(extra.selected_equations.len(), 2);
        let extra_fragment = extra.fragment.expect("fragment");
        assert!(extra_fragment.contains("y = 1;"), "{extra_fragment}");
        assert!(!extra_fragment.contains("y = 2"), "{extra_fragment}");
        assert!(!extra_fragment.contains("name='also'"), "{extra_fragment}");
    }

    #[test]
    fn static_partners_follow_tag_order_even_when_names_suggest_other_pairs() {
        let src = "\
var a, b, c;
model;
  [name='first', dynamic] a-a(-1)=0;
  [name='second', dynamic] b-b(-1)=0;
  [name='second', static] c=0;
  [name='first', static] b=0;
  [name='ordinary'] c=a;
end;
";
        let first = extract(&req(src, &["first"], &[("dynamic", "")])).unwrap();
        assert_eq!(first.status, ExtractStatus::Ok);
        assert_eq!(first.selected_equations.len(), 2);
        assert_eq!(first.selected_equations[0].name.as_deref(), Some("first"));
        assert_eq!(first.selected_equations[1].name.as_deref(), Some("second"));
        let fragment = first.fragment.unwrap();
        assert!(fragment.contains("c=0;"), "{fragment}");
        assert!(!fragment.contains("b=0;"), "{fragment}");
        assert!(!fragment.contains("c=a;"), "{fragment}");

        let second = extract(&req(src, &["second"], &[("static", "")])).unwrap();
        assert_eq!(second.selected_equations.len(), 2);
        assert_eq!(second.selected_equations[0].name.as_deref(), Some("first"));
        assert_eq!(second.selected_equations[1].name.as_deref(), Some("second"));
        assert_eq!(second.selected_equations[1].role, EquationRole::Requested);
    }

    #[test]
    fn constraints_close_all_four_expressions_without_unrelated_equations() {
        let src = "\
var y, z;
parameters bindp, relaxp, errorbp, errorrp;
bindp=0; relaxp=0; errorbp=0; errorrp=0;
model;
  [name='policy', relax='C'] y=0;
  [name='policy', bind='C'] y=1;
  z=0;
end;
occbin_constraints;
  name 'C'; bind z<bindp; relax z>=relaxp;
  error_bind errorbp; error_relax errorrp;
end;
";
        let result = extract(&req(src, &["policy"], &[])).unwrap();
        assert_eq!(result.status, ExtractStatus::Ok);
        let fragment = result.fragment.unwrap();
        assert!(fragment.contains("var y, z;"), "{fragment}");
        for name in ["bindp", "relaxp", "errorbp", "errorrp"] {
            assert!(fragment.contains(name), "{fragment}");
        }
        assert!(!fragment.contains("z=0;"), "{fragment}");
        assert!(!fragment.contains("bindp=0;"), "{fragment}");
        assert!(result
            .omitted_context
            .iter()
            .any(|item| item.kind == "calibration"));
    }

    #[test]
    fn static_loop_rows_keep_effective_order_and_iteration_origins() {
        let body = "\
@#define is = 1:2
@#for j in is
[name='law', dynamic] y=y(-1);
[name='ss', static] y=@{j};
@#endfor
";
        let plain = format!("var y;\nmodel;\n{body}end;\n");
        let mut included = req(
            "var y;\nmodel;\n@#include \"body.inc\"\nend;\n",
            &["law"],
            &[],
        );
        included.active_file = Some("root.mod".to_string());
        included
            .files
            .insert("body.inc".to_string(), body.to_string());
        for (request, expected_file) in [
            (req(&plain, &["law"], &[]), None),
            (included, Some("body.inc")),
        ] {
            let result = extract(&request).unwrap();
            assert_eq!(result.status, ExtractStatus::Ok);
            assert_eq!(result.selected_equations.len(), 4);
            assert_eq!(
                result
                    .selected_equations
                    .iter()
                    .map(|row| row.role)
                    .collect::<Vec<_>>(),
                [
                    EquationRole::Requested,
                    EquationRole::Companion,
                    EquationRole::Requested,
                    EquationRole::Companion
                ]
            );
            assert_eq!(
                result
                    .origins
                    .iter()
                    .map(|row| row.frames[0].value.as_deref())
                    .collect::<Vec<_>>(),
                [Some("1"), Some("1"), Some("2"), Some("2")]
            );
            assert!(
                result.origins.iter().all(|row| match expected_file {
                    Some(file) => row.file.as_deref().is_some_and(|path| path.ends_with(file)),
                    None => row.file.is_none(),
                }),
                "{:?}",
                result.origins
            );
            assert_eq!(
                result
                    .origins
                    .iter()
                    .map(|row| row.index)
                    .collect::<Vec<_>>(),
                [Some(0), None, Some(1), None]
            );
            let fragment = result.fragment.unwrap();
            let first = fragment
                .find("y = y(-1);")
                .unwrap_or_else(|| panic!("{fragment}"));
            let static_one = fragment
                .find("y = 1;")
                .unwrap_or_else(|| panic!("{fragment}"));
            let second = fragment[first + 1..]
                .find("y = y(-1);")
                .unwrap_or_else(|| panic!("{fragment}"))
                + first
                + 1;
            let static_two = fragment
                .find("y = 2;")
                .unwrap_or_else(|| panic!("{fragment}"));
            assert!(
                first < static_one && static_one < second && second < static_two,
                "{fragment}"
            );
        }
    }

    #[test]
    fn nested_loop_static_companions_keep_both_iteration_frames() {
        let src = "\
var y;
@#define is = 1:2
model;
@#for i in is
@#for j in is
[name='law', dynamic] y=y(-1);
[name='ss', static] y=@{i}+@{j};
@#endfor
@#endfor
end;
";
        let result = extract(&req(src, &["law"], &[])).unwrap();
        assert_eq!(result.status, ExtractStatus::Ok);
        assert_eq!(result.selected_equations.len(), 8);
        let frames = result
            .origins
            .iter()
            .map(|row| {
                row.frames
                    .iter()
                    .map(|frame| frame.value.as_deref())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            frames,
            [
                vec![Some("1"), Some("1")],
                vec![Some("1"), Some("1")],
                vec![Some("1"), Some("2")],
                vec![Some("1"), Some("2")],
                vec![Some("2"), Some("1")],
                vec![Some("2"), Some("1")],
                vec![Some("2"), Some("2")],
                vec![Some("2"), Some("2")],
            ]
        );
        assert_eq!(
            result
                .origins
                .iter()
                .map(|row| row.index)
                .collect::<Vec<_>>(),
            [Some(0), None, Some(1), None, Some(2), None, Some(3), None,]
        );
    }

    #[test]
    fn expanded_locals_stay_before_each_equation_that_uses_them() {
        let src = "\
var y;
@#define is = 1:2
model;
@#for i in is
# a@{i} = @{i};
[name='eq'] y=a@{i};
@#endfor
end;
";
        let result = extract(&req(src, &["eq"], &[])).unwrap();
        assert_eq!(result.status, ExtractStatus::Ok);
        let fragment = result.fragment.unwrap();
        let first_local = fragment
            .find("#a1 = 1;")
            .unwrap_or_else(|| panic!("{fragment}"));
        let first_eq = fragment
            .find("y = a1;")
            .unwrap_or_else(|| panic!("{fragment}"));
        let second_local = fragment
            .find("#a2 = 2;")
            .unwrap_or_else(|| panic!("{fragment}"));
        let second_eq = fragment
            .find("y = a2;")
            .unwrap_or_else(|| panic!("{fragment}"));
        assert!(
            first_local < first_eq && first_eq < second_local && second_local < second_eq,
            "{fragment}"
        );
    }

    #[test]
    fn occbin_regime_keeps_the_same_name_and_the_named_constraint() {
        let src = "\
var y, i;
model;
  [name='policy', relax='ELB']
  i = y;
  [name='policy', bind='ELB']
  i = 0;
  [name='other']
  y = 1;
end;
occbin_constraints;
  name 'ELB';
  bind i <= 0;
  name 'idle';
  bind y <= 2;
end;
";
        let result = extract(&req(src, &["policy"], &[("bind", "ELB")])).unwrap();
        assert_eq!(result.status, ExtractStatus::Ok);
        let fragment = result.fragment.expect("fragment");
        assert_eq!(result.selected_equations.len(), 2);
        assert_eq!(result.selected_equations[0].role, EquationRole::Companion);
        assert_eq!(result.selected_equations[0].name.as_deref(), Some("policy"));
        assert_eq!(result.selected_equations[0].index, Some(0));
        assert_eq!(
            result.selected_equations[0]
                .tags
                .get("relax")
                .map(String::as_str),
            Some("ELB")
        );
        assert_eq!(result.selected_equations[1].role, EquationRole::Requested);
        assert_eq!(result.selected_equations[1].index, Some(1));
        assert_eq!(
            result.selected_equations[1]
                .tags
                .get("bind")
                .map(String::as_str),
            Some("ELB")
        );
        assert_eq!(result.origins[0].role, EquationRole::Companion);
        assert_eq!(result.origins[0].index, Some(0));
        assert_eq!(result.origins[1].role, EquationRole::Requested);
        assert_eq!(result.origins[1].index, Some(1));
        assert!(fragment.contains("relax='ELB'"), "{fragment}");
        assert!(fragment.contains("bind='ELB'"), "{fragment}");
        assert!(fragment.contains("i = y;"), "{fragment}");
        assert!(fragment.contains("i = 0;"), "{fragment}");
        assert!(fragment.contains("var y, i;"), "{fragment}");
        assert!(!fragment.contains("name='other'"), "{fragment}");
        assert!(fragment.contains("occbin_constraints;"), "{fragment}");
        assert!(fragment.contains("name 'ELB'"), "{fragment}");
        assert!(fragment.contains("bind i <= 0;"), "{fragment}");
        assert!(!fragment.contains("idle"), "{fragment}");
        assert!(!fragment.contains("y <= 2"), "{fragment}");
        let model_at = fragment.find("model;").unwrap();
        let setup_at = fragment.find("occbin_constraints;").unwrap();
        assert!(model_at < setup_at, "{fragment}");

        let both = extract(&req(src, &["policy"], &[])).unwrap();
        assert_eq!(both.selected_equations.len(), 2);
        assert!(both
            .selected_equations
            .iter()
            .all(|row| row.role == EquationRole::Requested));

        let before = "\
occbin_constraints;
  name 'ELB';
  bind i <= 0;
end;
var i;
model;
  [name='policy', bind='ELB']
  i = 0;
  [name='policy', relax='ELB']
  i = 1;
end;
";
        let leading = extract(&req(before, &["policy"], &[("bind", "ELB")])).unwrap();
        let leading_fragment = leading.fragment.expect("fragment");
        let setup_at = leading_fragment.find("occbin_constraints;").unwrap();
        let model_at = leading_fragment.find("model;").unwrap();
        assert!(setup_at < model_at, "{leading_fragment}");
        assert!(leading_fragment.contains("i = 1;"), "{leading_fragment}");
    }

    #[test]
    fn missing_occbin_constraint_has_no_fragment() {
        let src = "\
var y;
model;
  [name='r', bind='c']
  y = 0;
  [name='r', relax='c']
  y = 1;
end;
";
        let result = extract(&req(src, &["r"], &[("bind", "c")])).unwrap();
        assert_eq!(result.status, ExtractStatus::UnsupportedContext);
        assert!(result.fragment.is_none());
        assert!(result.explanation.contains("`c`"));

        let unrelated = "\
var y, z;
model;
  [name='policy', bind='c'] y=0;
  [name='policy', relax='c'] y=1;
  [name='other', bind='missing'] z=0;
end;
occbin_constraints;
  name 'c'; bind y<0;
end;
";
        let selected = extract(&req(unrelated, &["policy"], &[])).unwrap();
        assert_eq!(selected.status, ExtractStatus::Ok);
        let fragment = selected.fragment.unwrap();
        assert!(fragment.contains("name 'c'"), "{fragment}");
        assert!(!fragment.contains("missing"), "{fragment}");
    }

    #[test]
    fn macro_in_an_occbin_constraint_has_no_fragment() {
        let src = "\
var y;
occbin_constraints;
  name 'c';
  // @{kept}
  bind y >= 0;
end;
model;
  [name='r', bind='c']
  y = 0;
end;
";
        let result = extract(&req(src, &["r"], &[])).unwrap();
        assert_eq!(result.status, ExtractStatus::UnsupportedContext);
        assert!(result.fragment.is_none());
        assert_eq!(result.unsupported.unwrap().kind, UnsupportedKind::Macro);
    }

    #[test]
    fn unresolved_macro_and_required_include_have_no_fragment() {
        let hidden = "\
@#if \"nope\"
var y;
model;
  [name='hidden']
  y = 0;
end;
@#endif
";
        let missed = extract(&req(hidden, &["hidden"], &[])).unwrap();
        assert_eq!(missed.status, ExtractStatus::UnsupportedContext);
        assert!(missed.fragment.is_none());
        assert_eq!(missed.unsupported.unwrap().kind, UnsupportedKind::Macro);

        let needed = "\
@#include \"other.mod\"
model;
  [name='eq']
  y = 0;
end;
";
        let include = extract(&req(needed, &["eq"], &[])).unwrap();
        assert_eq!(include.unsupported.unwrap().kind, UnsupportedKind::Include);
        assert!(include.fragment.is_none());

        let unused = "\
@#include \"other.mod\"
var y;
model;
  [name='eq']
  y = 0;
end;
";
        let blocked = extract(&req(unused, &["eq"], &[])).unwrap();
        assert_eq!(blocked.status, ExtractStatus::UnsupportedContext);
        assert!(blocked.fragment.is_none());
        assert_eq!(blocked.unsupported.unwrap().kind, UnsupportedKind::Include);
    }

    #[test]
    fn external_function_and_trend_stay_with_the_equation() {
        let src = "\
external_function(name = foo, nargs = 1);
trend_var(growth_factor = 1.02) A, B;
var y, c;
model;
  [name='eq']
  y = foo(A);
  [name='skip']
  c = B;
end;
";
        let result = extract(&req(src, &["eq"], &[])).unwrap();
        let fragment = result.fragment.unwrap();
        assert!(
            fragment.contains("external_function(name = foo, nargs = 1);"),
            "{fragment}"
        );
        assert!(
            fragment.contains("trend_var(growth_factor = 1.02) A;"),
            "{fragment}"
        );
        assert!(fragment.contains("var y;"), "{fragment}");
        assert!(
            !fragment.contains("var y, c") && !fragment.contains(", c"),
            "{fragment}"
        );
        assert!(!fragment.contains("name='skip'"), "{fragment}");
    }

    #[test]
    fn include_keeps_the_included_equation_and_its_origin() {
        let root = "\
var y;
model;
@#include \"body.inc\"
end;
";
        let body = "\
[name='eq']
y = 1;
";
        let mut request = req(root, &["eq"], &[]);
        request.active_file = Some("root.mod".to_string());
        request
            .files
            .insert("root.mod".to_string(), root.to_string());
        request
            .files
            .insert("body.inc".to_string(), body.to_string());
        let result = extract(&request).unwrap();
        assert_eq!(result.status, ExtractStatus::Ok);
        let fragment = result.fragment.unwrap();
        assert!(fragment.contains("var y;"), "{fragment}");
        assert!(fragment.contains("[name='eq']"), "{fragment}");
        assert!(fragment.contains("y = 1;"), "{fragment}");
        assert!(!fragment.contains("@#include"), "{fragment}");
        let origin = &result.origins[0];
        let file = origin.file.as_deref().unwrap_or("");
        assert!(file.contains("body.inc"), "{file}");
        let slice = &body[origin.span.start as usize..origin.span.end as usize];
        assert!(slice.contains("y = 1"), "{slice}");
    }

    #[test]
    fn macro_loop_writes_instances_and_keeps_the_source_origin() {
        let src = "\
var x;
@#define is = 1:2
@#define js = 1:2
model;
@#for i in is
@#for j in js
[name='row']
x = @{i};
@#endfor
@#endfor
end;
";
        let mut request = req(src, &["row"], &[]);
        request.active_file = Some("loop.mod".to_string());
        let result = extract(&request).unwrap();
        assert_eq!(result.status, ExtractStatus::Ok);
        let fragment = result.fragment.unwrap();
        assert!(fragment.contains("x = 1;"), "{fragment}");
        assert!(fragment.contains("x = 2;"), "{fragment}");
        assert!(fragment.contains("[name='row']"), "{fragment}");
        assert!(!fragment.contains("@{"), "{fragment}");
        assert!(!fragment.contains("@#"), "{fragment}");
        assert_eq!(result.selected_equations.len(), 4);
        assert_eq!(result.origins.len(), 4);
        let span = result.origins[0].span;
        assert!(result.origins.iter().all(|origin| origin.span == span));
        assert!(result.origins.iter().all(|origin| {
            origin.file.as_deref() == Some("loop.mod") && origin.frames.len() == 2
        }));
        let pairs: Vec<Vec<(&str, &str)>> = result
            .origins
            .iter()
            .map(|origin| {
                origin
                    .frames
                    .iter()
                    .map(|frame| {
                        (
                            frame.variable.as_deref().unwrap_or(""),
                            frame.value.as_deref().unwrap_or(""),
                        )
                    })
                    .collect()
            })
            .collect();
        assert_eq!(
            pairs,
            vec![
                vec![("i", "1"), ("j", "1")],
                vec![("i", "1"), ("j", "2")],
                vec![("i", "2"), ("j", "1")],
                vec![("i", "2"), ("j", "2")],
            ]
        );
        let body = &src[span.start as usize..span.end as usize];
        assert!(body.contains("x = @{i}"), "{body}");
        assert!(!body.contains("@#for"), "{body}");
    }

    #[test]
    fn overlay_replaces_the_active_file() {
        let stored = "\
var y;
model;
[name='old']
y = 0;
end;
";
        let overlay = "\
@#include \"decl.inc\"
model;
[name='eq']
y = 1;
end;
";
        let mut request = req(overlay, &["eq"], &[]);
        request.active_file = Some("root.mod".to_string());
        request
            .files
            .insert("root.mod".to_string(), stored.to_string());
        request
            .files
            .insert("decl.inc".to_string(), "var y;\n".to_string());
        let result = extract(&request).unwrap();
        assert_eq!(result.status, ExtractStatus::Ok);
        let fragment = result.fragment.unwrap();
        assert!(fragment.contains("var y;"), "{fragment}");
        assert!(fragment.contains("y = 1;"), "{fragment}");
        assert!(!fragment.contains("name='old'"), "{fragment}");
        let old = extract(&req(stored, &["old"], &[])).unwrap();
        assert_eq!(old.status, ExtractStatus::Ok);

        let mut hidden = request.clone();
        hidden.names = vec!["old".to_string()];
        let missed = extract(&hidden).unwrap();
        assert_eq!(missed.status, ExtractStatus::Empty);
        assert_eq!(missed.fragment.as_deref(), Some(""));
    }

    #[test]
    fn files_map_never_reads_an_unprovided_disk_include() {
        let unique = format!(
            "dygnosis-extract-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root_path = std::env::temp_dir().join(format!("{unique}.mod"));
        let child_name = format!("{unique}.inc");
        let child_path = std::env::temp_dir().join(&child_name);
        let root_key = root_path.to_string_lossy().replace('\\', "/");
        let child_key = child_path.to_string_lossy().replace('\\', "/");
        let root = format!("var y;\nmodel;\n@#include \"{child_name}\"\nend;\n");
        let child = "[name='eq'] y=1;\n";
        let mut request = req(&root, &["eq"], &[]);
        request.active_file = Some(root_key.clone());
        request
            .files
            .insert(root_key, "var y; model; [name='old'] y=0; end;".to_string());

        let before = extract(&request).unwrap();
        assert_eq!(before.status, ExtractStatus::UnsupportedContext);
        assert!(before.fragment.is_none());

        std::fs::write(&child_path, child).unwrap();
        let with_disk = extract(&request).unwrap();
        std::fs::remove_file(&child_path).unwrap();
        assert_eq!(with_disk.status, ExtractStatus::UnsupportedContext);
        assert!(with_disk.fragment.is_none());

        request.files.insert(child_key.clone(), child.to_string());
        let supplied = extract(&request).unwrap();
        assert_eq!(supplied.status, ExtractStatus::Ok);
        assert_eq!(
            supplied.origins[0].file.as_deref(),
            Some(child_key.as_str())
        );
        let fragment = supplied.fragment.unwrap();
        assert!(fragment.contains("y=1;"), "{fragment}");
        assert!(!fragment.contains("name='old'"), "{fragment}");
    }

    #[test]
    fn overlay_keeps_windows_supplied_keys_and_replaces_active_alias() {
        let active = r"C:\models\root.mod";
        let child = r"C:\models\body.inc";
        let mut request = req(
            "var y;\nmodel;\n@#include \"body.inc\"\nend;\n",
            &["eq"],
            &[],
        );
        request.active_file = Some(active.to_string());
        request.files.insert(
            "C:/models/root.mod".to_string(),
            "var y; model; [name='old'] y=0; end;".to_string(),
        );
        request
            .files
            .insert(child.to_string(), "[name='eq'] y=1;\n".to_string());
        let result = extract(&request).unwrap();
        assert_eq!(result.status, ExtractStatus::Ok, "{}", result.explanation);
        assert_eq!(result.origins[0].file.as_deref(), Some(child));
        let fragment = result.fragment.unwrap();
        assert!(fragment.contains("y=1;"), "{fragment}");
        assert!(!fragment.contains("name='old'"), "{fragment}");
    }

    #[test]
    fn unresolved_expansion_and_missing_include_have_no_fragment() {
        let unknown = "\
var y;
model;
[name='eq']
y = @{UNDEF};
end;
";
        let failed = extract(&req(unknown, &["eq"], &[])).unwrap();
        assert_eq!(failed.status, ExtractStatus::UnsupportedContext);
        assert!(failed.fragment.is_none());
        assert_eq!(failed.unsupported.unwrap().kind, UnsupportedKind::Macro);

        let root = "\
@#include \"other.mod\"
model;
[name='eq']
y = 0;
end;
";
        let mut request = req(root, &["eq"], &[]);
        request.active_file = Some("root.mod".to_string());
        request
            .files
            .insert("root.mod".to_string(), root.to_string());
        let include = extract(&request).unwrap();
        assert_eq!(include.status, ExtractStatus::UnsupportedContext);
        assert!(include.fragment.is_none());
        assert_eq!(include.unsupported.unwrap().kind, UnsupportedKind::Include);

        let only_in_include = "\
@#include \"body.inc\"
var y;
model;
[name='other']
y = 0;
end;
";
        let mut hidden = req(only_in_include, &["hidden"], &[]);
        hidden.active_file = Some("root.mod".to_string());
        hidden
            .files
            .insert("root.mod".to_string(), only_in_include.to_string());
        let dropped = extract(&hidden).unwrap();
        assert_eq!(dropped.status, ExtractStatus::UnsupportedContext);
        assert!(dropped.fragment.is_none());
    }

    #[test]
    fn equation_surgery_keeps_the_replacement_only() {
        let src = "\
var y, c;
model;
[name='keep']
y = 0;
[name='drop']
c = 1;
end;
model_remove([name='drop']);
model_replace([name='keep']);
[name='keep']
y = 2;
end;
";
        let kept = extract(&req(src, &["keep"], &[])).unwrap();
        assert_eq!(kept.status, ExtractStatus::Ok);
        let fragment = kept.fragment.unwrap();
        assert!(fragment.contains("y = 2;"), "{fragment}");
        assert!(!fragment.contains("y = 0"), "{fragment}");
        assert!(!fragment.contains("name='drop'"), "{fragment}");
        assert_eq!(kept.selected_equations.len(), 1);

        let dropped = extract(&req(src, &["drop"], &[])).unwrap();
        assert_eq!(dropped.status, ExtractStatus::Empty);
        assert_eq!(dropped.fragment.as_deref(), Some(""));
    }

    #[test]
    fn macro_declaration_keeps_tex_long_name_and_deflator() {
        let src = "\
@#define is = 1:1
@#for i in is
var(log, deflator=z) y@{i} $y$ (long_name='qty');
@#endfor
var z;
model;
[name='eq']
y1 = z;
end;
";
        let result = extract(&req(src, &["eq"], &[])).unwrap();
        assert_eq!(result.status, ExtractStatus::Ok);
        let fragment = result.fragment.expect("fragment");
        assert!(
            fragment.contains("var(log, deflator=z) y1 $y$ (long_name='qty');"),
            "{fragment}"
        );
        assert!(fragment.contains("var z;"), "{fragment}");
    }

    #[test]
    fn single_loop_frames_name_the_iteration() {
        let src = "\
var y;
@#define is = 1:2
model;
@#for i in is
[name='row']
y = @{i};
@#endfor
end;
";
        let result = extract(&req(src, &["row"], &[])).unwrap();
        assert_eq!(result.status, ExtractStatus::Ok);
        assert_eq!(result.origins.len(), 2);
        assert!(result
            .origins
            .iter()
            .all(|origin| origin.span == result.origins[0].span));
        let values: Vec<&str> = result
            .origins
            .iter()
            .map(|origin| {
                assert_eq!(origin.frames.len(), 1);
                assert_eq!(origin.frames[0].variable.as_deref(), Some("i"));
                assert_eq!(origin.frames[0].kind, "for");
                origin.frames[0].value.as_deref().unwrap()
            })
            .collect();
        assert_eq!(values, ["1", "2"]);
    }

    #[test]
    fn include_loop_keeps_file_span_and_iteration() {
        let root = "\
var y;
model;
@#include \"body.inc\"
end;
";
        let body = "\
@#define is = 1:2
@#for i in is
[name='row']
y = @{i};
@#endfor
";
        let mut request = req(root, &["row"], &[]);
        request.active_file = Some("root.mod".to_string());
        request
            .files
            .insert("body.inc".to_string(), body.to_string());
        let result = extract(&request).unwrap();
        assert_eq!(result.status, ExtractStatus::Ok);
        assert_eq!(result.origins.len(), 2);
        assert!(result.origins.iter().all(|origin| {
            origin
                .file
                .as_deref()
                .is_some_and(|file| file.contains("body.inc"))
                && origin.frames.len() == 1
                && origin.frames[0].variable.as_deref() == Some("i")
        }));
        assert_eq!(
            result
                .origins
                .iter()
                .map(|origin| origin.frames[0].value.as_deref().unwrap())
                .collect::<Vec<_>>(),
            ["1", "2"]
        );
        let slice =
            &body[result.origins[0].span.start as usize..result.origins[0].span.end as usize];
        assert!(slice.contains("y = @{i}"), "{slice}");
    }

    #[test]
    fn companion_only_map_uses_current_file_content() {
        let content = "\
@#include \"equations.inc\"
";
        let included = "\
var y;
model;
[name='eq']
y = 1;
end;
";
        let mut request = req(content, &["eq"], &[]);
        request.active_file = Some("main.mod".to_string());
        request
            .files
            .insert("equations.inc".to_string(), included.to_string());
        let result = extract(&request).unwrap();
        assert_eq!(result.status, ExtractStatus::Ok);
        let fragment = result.fragment.unwrap();
        assert!(fragment.contains("y = 1;"), "{fragment}");
        assert!(fragment.contains("var y;"), "{fragment}");

        let stored = "\
var y;
model;
[name='old']
y = 0;
end;
";
        request
            .files
            .insert("main.mod".to_string(), stored.to_string());
        let replaced = extract(&request).unwrap();
        assert_eq!(replaced.status, ExtractStatus::Ok);
        assert!(replaced.fragment.unwrap().contains("y = 1;"));

        let alone = extract(&req(stored, &["old"], &[])).unwrap();
        assert_eq!(alone.status, ExtractStatus::Ok);
        assert!(request.files.contains_key("equations.inc"));
    }

    #[test]
    fn resolved_include_can_be_omitted_when_unused() {
        let root = "\
@#include \"cal.inc\"
var y;
model;
[name='eq']
y = 0;
end;
";
        let mut request = req(root, &["eq"], &[]);
        request.active_file = Some("root.mod".to_string());
        request.files.insert(
            "cal.inc".to_string(),
            "parameters beta;\nbeta = 0.99;\n".to_string(),
        );
        let result = extract(&request).unwrap();
        assert_eq!(result.status, ExtractStatus::Ok);
        let fragment = result.fragment.unwrap();
        assert!(fragment.contains("name='eq'"), "{fragment}");
        assert!(!fragment.contains("beta"), "{fragment}");
    }

    #[test]
    fn tools_list_includes_dynare_extract() {
        let names = registered_tool_names();
        assert!(names.contains(&"dynare_extract"));
        assert!(names.contains(&"dynare_workspace_diagnose"));
    }
}
