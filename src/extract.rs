//! Private equation extraction. Later slices extend this engine.
//!
//! This slice closes an ordinary aggregate selection on the effective compilation
//! unit: overlays, includes, macros, and equation surgery. It does not register
//! an MCP tool.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::check_e060::check_e063;
use crate::expand::{expand_report, ExpandReport};
use crate::expr::{ExprId, ExprKind};
use crate::intern::Name;
use crate::model::{Decl, Equation, ExternalFunctionStmt, Model, TrendVar};
use crate::parser::parse;
use crate::span::Span;
use crate::workspace::Workspace;

/// Caller inputs. `dimension` is the later heterogeneous scope; this slice
/// refuses it. `files` is the companion map for the MCP compilation unit.
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
    /// Filled when a later slice retains heterogeneous rows.
    #[allow(dead_code)]
    Heterogeneous,
}

/// `Requested` is a selector hit. `Companion` is reserved for a later slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EquationRole {
    Requested,
    /// Filled when a later slice adds a required companion row.
    #[allow(dead_code)]
    Companion,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedEquation {
    pub domain: EquationDomain,
    pub dimension: Option<String>,
    pub index: usize,
    pub name: Option<String>,
    pub role: EquationRole,
    pub tags: BTreeMap<String, String>,
}

/// Where a selected row was read. `frames` follows the expand map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractOrigin {
    pub domain: EquationDomain,
    pub dimension: Option<String>,
    pub index: usize,
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OmittedContext {
    pub kind: String,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsupportedKind {
    HeterogeneousDimension,
    CompilationUnit,
    Heterogeneous,
    Pac,
    Occbin,
    StaticDynamic,
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
    if request.dimension.is_some() {
        return Ok(unsupported(
            UnsupportedKind::HeterogeneousDimension,
            "dimension narrows to a heterogeneous scope this step does not retain",
        ));
    }

    let Some(unit) = prepare(request) else {
        return Ok(unsupported(
            UnsupportedKind::CompilationUnit,
            "the compilation unit could not be expanded",
        ));
    };
    let model = &unit.model;
    let aggregate = matching_indexes(&model.equations, request);
    if heterogeneous_match(model, request) {
        return Ok(unsupported(
            UnsupportedKind::Heterogeneous,
            "a matching equation is in a heterogeneous model block",
        ));
    }
    if aggregate.is_empty() {
        if unit.missing_include {
            return Ok(unsupported(
                UnsupportedKind::Include,
                "a required include did not resolve, so the selection may be incomplete",
            ));
        }
        if !model.macro_type_errors.is_empty() || !check_e063(model).is_empty() {
            return Ok(unsupported(
                UnsupportedKind::Macro,
                "macro expansion failed and no equation matched",
            ));
        }
        return Ok(ExtractResult {
            status: ExtractStatus::Empty,
            fragment: Some(String::new()),
            selected_equations: Vec::new(),
            origins: Vec::new(),
            omitted_context: Vec::new(),
            explanation: "No equation matched the selectors.".to_string(),
            unsupported: None,
        });
    }

    let closure = close(model, &aggregate);
    if let Some(reason) = refuse(model, &aggregate, &closure, unit.missing_include) {
        return Ok(unsupported(reason.0, reason.1));
    }

    let selected_equations = aggregate
        .iter()
        .map(|&idx| selected_row(model, idx))
        .collect::<Vec<_>>();
    let origins = aggregate
        .iter()
        .map(|&idx| origin_row(model, &unit.report, idx, request.active_file.as_deref()))
        .collect();

    Ok(ExtractResult {
        status: ExtractStatus::Ok,
        fragment: Some(render(model, &aggregate, &closure)),
        selected_equations,
        origins,
        omitted_context: omitted(model),
        explanation: String::new(),
        unsupported: None,
    })
}

struct Unit {
    model: Model,
    report: ExpandReport,
    missing_include: bool,
}

/// MCP object-tool rule: splice only when `active_file` is a key in `files`.
fn prepare(request: &ExtractRequest) -> Option<Unit> {
    let Some(active) = request
        .active_file
        .as_deref()
        .filter(|active| request.files.contains_key(*active))
    else {
        let model = parse(&request.file_content);
        let report = expand_report(&request.file_content);
        return Some(Unit {
            model,
            report,
            missing_include: false,
        });
    };
    let mut files = request.files.clone();
    files.insert(active.to_string(), request.file_content.clone());
    let mut ws = Workspace::new();
    for (name, content) in &files {
        ws.update_document(name, content);
    }
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
    eq_index: usize,
    active_file: Option<&str>,
) -> ExtractOrigin {
    let counted = counted_index(model, eq_index);
    let mapped = report.aggregate_origins.get(counted);
    let row = selected_row(model, eq_index);
    let frames = mapped
        .map(|origin| {
            origin
                .origin_frames
                .iter()
                .map(|frame| OriginFrame {
                    kind: frame.kind.clone(),
                    span: frame.origin_span,
                    file: frame.origin_uri.clone(),
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
            .unwrap_or(model.equations[eq_index].span),
        file: mapped
            .and_then(|origin| origin.origin_uri.clone())
            .or_else(|| active_file.map(str::to_string)),
        frames,
    }
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

fn matching_indexes(equations: &[Equation], request: &ExtractRequest) -> Vec<usize> {
    equations
        .iter()
        .enumerate()
        .filter(|(_, eq)| !eq.is_local && matches_selector(eq, request))
        .map(|(idx, _)| idx)
        .collect()
}

fn heterogeneous_match(model: &Model, request: &ExtractRequest) -> bool {
    model.heterogeneous_models.iter().any(|block| {
        block
            .equations
            .iter()
            .any(|eq| !eq.is_local && matches_selector(eq, request))
    })
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
    locals: BTreeSet<String>,
    externals: BTreeSet<String>,
}

fn close(model: &Model, selected: &[usize]) -> Closure {
    let mut closure = Closure {
        symbols: BTreeSet::new(),
        locals: BTreeSet::new(),
        externals: BTreeSet::new(),
    };
    let mut pending_eq: Vec<usize> = selected.to_vec();
    let mut seen_eq = HashSet::new();
    while let Some(idx) = pending_eq.pop() {
        if !seen_eq.insert(idx) {
            continue;
        }
        let eq = &model.equations[idx];
        let mut idents = Vec::new();
        let mut calls = Vec::new();
        if let Some(id) = eq.lhs_expr {
            walk(model, id, &mut idents, &mut calls);
        }
        if let Some(id) = eq.rhs_expr {
            walk(model, id, &mut idents, &mut calls);
        }
        let defined = eq.is_local.then(|| local_name(model, eq)).flatten();
        for name in idents {
            let text = model.name(name).to_string();
            if defined.as_deref() == Some(text.as_str()) {
                continue;
            }
            if local_index(model, &text).is_some() {
                if closure.locals.insert(text.clone()) {
                    if let Some(local_idx) = local_index(model, &text) {
                        pending_eq.push(local_idx);
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
    loop {
        let locals_before = closure.locals.len();
        grow_declaration_context(model, &mut closure);
        for name in &closure.locals {
            if let Some(idx) = local_index(model, name) {
                if !seen_eq.contains(&idx) {
                    pending_eq.push(idx);
                }
            }
        }
        if pending_eq.is_empty() && closure.locals.len() == locals_before {
            break;
        }
        while let Some(idx) = pending_eq.pop() {
            if !seen_eq.insert(idx) {
                continue;
            }
            let eq = &model.equations[idx];
            let mut idents = Vec::new();
            let mut calls = Vec::new();
            if let Some(id) = eq.lhs_expr {
                walk(model, id, &mut idents, &mut calls);
            }
            if let Some(id) = eq.rhs_expr {
                walk(model, id, &mut idents, &mut calls);
            }
            let defined = eq.is_local.then(|| local_name(model, eq)).flatten();
            for name in idents {
                let text = model.name(name).to_string();
                if defined.as_deref() == Some(text.as_str()) {
                    continue;
                }
                if local_index(model, &text).is_some() {
                    if closure.locals.insert(text.clone()) {
                        if let Some(local_idx) = local_index(model, &text) {
                            pending_eq.push(local_idx);
                        }
                    }
                } else {
                    closure.symbols.insert(text);
                }
            }
        }
    }
    closure
}

fn grow_declaration_context(model: &Model, closure: &mut Closure) {
    loop {
        let before = closure.symbols.len() + closure.externals.len();
        for trend in &model.trend_vars {
            if !closure.symbols.contains(model.name(trend.name)) {
                continue;
            }
            if let Some(id) = trend.growth {
                push_expr_symbols(model, id, closure);
            }
        }
        for row in &model.nonstationary_vars {
            if !closure.symbols.contains(model.name(row.name)) {
                continue;
            }
            if let Some(id) = row.deflator {
                push_expr_symbols(model, id, closure);
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

fn push_expr_symbols(model: &Model, id: ExprId, closure: &mut Closure) {
    let mut idents = Vec::new();
    let mut calls = Vec::new();
    walk(model, id, &mut idents, &mut calls);
    for name in idents {
        let text = model.name(name).to_string();
        if local_index(model, &text).is_some() {
            closure.locals.insert(text);
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

fn walk(model: &Model, id: ExprId, idents: &mut Vec<Name>, calls: &mut Vec<Name>) {
    match &model.exprs.get(id).kind {
        ExprKind::Ident { name, .. } => idents.push(*name),
        ExprKind::Number | ExprKind::String | ExprKind::Error => {}
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

fn local_name(model: &Model, eq: &Equation) -> Option<String> {
    let id = eq.lhs_expr?;
    match &model.exprs.get(id).kind {
        ExprKind::Ident { name, .. } => Some(model.name(*name).to_string()),
        _ => None,
    }
}

fn local_index(model: &Model, name: &str) -> Option<usize> {
    model
        .equations
        .iter()
        .position(|eq| eq.is_local && local_name(model, eq).as_deref() == Some(name))
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

fn refuse(
    model: &Model,
    selected: &[usize],
    closure: &Closure,
    missing_include: bool,
) -> Option<(UnsupportedKind, String)> {
    if let Some(detail) = macro_block(model, selected, closure) {
        return Some((UnsupportedKind::Macro, detail));
    }
    if let Some(detail) = include_block(model, closure, missing_include) {
        return Some((UnsupportedKind::Include, detail));
    }
    if let Some(detail) = heterogeneous_decl(model, closure) {
        return Some((UnsupportedKind::Heterogeneous, detail));
    }
    if let Some(detail) = pac_block(model, selected, closure) {
        return Some((UnsupportedKind::Pac, detail));
    }
    if let Some(detail) = occbin_block(model, selected) {
        return Some((UnsupportedKind::Occbin, detail));
    }
    if let Some(detail) = static_block(model, selected) {
        return Some((UnsupportedKind::StaticDynamic, detail));
    }
    None
}

fn macro_block(model: &Model, selected: &[usize], closure: &Closure) -> Option<String> {
    let retained = retained_indexes(model, selected, closure);
    for idx in &retained {
        let text = equation_line(model, *idx);
        if contains_macro(&text) {
            return Some("macro expansion failed on text this extract would retain".to_string());
        }
    }
    if declaration_has_macro(model, closure)
        || declaration_lines(model, closure)
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
                .any(|idx| overlaps(*span, model.equations[*idx].span))
        {
            return Some("macro expansion failed on text this extract would retain".to_string());
        }
    }
    for diag in check_e063(model) {
        if retained
            .iter()
            .any(|idx| overlaps(diag.span, model.equations[*idx].span))
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
            if !closure.symbols.contains(name) && !closure.locals.contains(name) {
                continue;
            }
            let stmt = statement_around(&model.source, decl.span);
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
    false
}

fn declaration_statement_overlaps(model: &Model, closure: &Closure, span: Span) -> bool {
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
            if !closure.symbols.contains(name) && !closure.locals.contains(name) {
                return false;
            }
            let stmt = statement_around(&model.source, decl.span);
            overlaps(span, stmt)
        })
    })
}

fn include_block(model: &Model, closure: &Closure, missing_include: bool) -> Option<String> {
    if model.includes.is_empty() && !missing_include {
        return None;
    }
    let missing = closure
        .symbols
        .iter()
        .find(|name| !declared(model, name) && local_index(model, name).is_none());
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

fn heterogeneous_decl(model: &Model, closure: &Closure) -> Option<String> {
    let lists = [
        &model.endogenous,
        &model.exogenous,
        &model.deterministic_exogenous,
        &model.parameters,
    ];
    for decls in lists {
        for decl in decls {
            if closure.symbols.contains(model.name(decl.name)) && decl.heterogeneity.is_some() {
                return Some(format!(
                    "declaration of `{}` carries a heterogeneity dimension",
                    model.name(decl.name)
                ));
            }
        }
    }
    None
}

fn pac_block(model: &Model, selected: &[usize], closure: &Closure) -> Option<String> {
    let retained = retained_indexes(model, selected, closure);
    for operator in &model.named_model_operators {
        if retained
            .iter()
            .any(|idx| overlaps(operator.span, model.equations[*idx].span))
        {
            return Some("a retained equation uses a PAC or VAR expectation operator".to_string());
        }
    }
    for idx in &retained {
        let eq = &model.equations[*idx];
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

fn occbin_block(model: &Model, selected: &[usize]) -> Option<String> {
    for &idx in selected {
        let eq = &model.equations[idx];
        if eq.tag_map.contains_key("bind") || eq.tag_map.contains_key("relax") {
            return Some("a selected equation carries an OccBin bind or relax tag".to_string());
        }
        if !eq.name.is_empty()
            && model.equations.iter().any(|other| {
                other.name == eq.name
                    && (other.tag_map.contains_key("bind") || other.tag_map.contains_key("relax"))
            })
        {
            return Some(
                "another equation with the same name carries an OccBin bind or relax tag"
                    .to_string(),
            );
        }
    }
    None
}

fn static_block(model: &Model, selected: &[usize]) -> Option<String> {
    for &idx in selected {
        let eq = &model.equations[idx];
        if eq.static_tag || eq.dynamic_tag {
            return Some("a selected equation is tagged static or dynamic".to_string());
        }
        let lhs = lhs_symbol(model, eq);
        if model
            .equations
            .iter()
            .enumerate()
            .any(|(other_idx, other)| {
                other_idx != idx
                    && (other.static_tag || other.dynamic_tag)
                    && ((!eq.name.is_empty() && other.name == eq.name)
                        || lhs_symbol(model, other).is_some_and(|name| lhs == Some(name)))
            })
        {
            return Some(
                "a static or dynamic equation is a partner of a selected equation".to_string(),
            );
        }
    }
    None
}

fn lhs_symbol(model: &Model, eq: &Equation) -> Option<Name> {
    let id = eq.lhs_expr?;
    match &model.exprs.get(id).kind {
        ExprKind::Ident { name, .. } => Some(*name),
        _ => None,
    }
}

fn retained_indexes(model: &Model, selected: &[usize], closure: &Closure) -> Vec<usize> {
    let mut indexes = selected.to_vec();
    for (idx, eq) in model.equations.iter().enumerate() {
        if eq.is_local && local_name(model, eq).is_some_and(|name| closure.locals.contains(&name)) {
            indexes.push(idx);
        }
    }
    indexes.sort_unstable();
    indexes.dedup();
    indexes
}

fn overlaps(left: Span, right: Span) -> bool {
    left.start < right.end && right.start < left.end
}

fn selected_row(model: &Model, eq_index: usize) -> SelectedEquation {
    let eq = &model.equations[eq_index];
    let name = if eq.name.is_empty() {
        None
    } else {
        Some(eq.name.clone())
    };
    SelectedEquation {
        domain: EquationDomain::Aggregate,
        dimension: None,
        index: counted_index(model, eq_index),
        name,
        role: EquationRole::Requested,
        tags: eq.tag_map.clone(),
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

fn render(model: &Model, selected: &[usize], closure: &Closure) -> String {
    let mut lines = declaration_lines(model, closure);
    let opener = model
        .model_block
        .map(|span| model_opener(&model.source, span))
        .unwrap_or_else(|| "model;".to_string());
    let mut body = vec![opener];
    for idx in retained_indexes(model, selected, closure) {
        body.push(equation_line(model, idx));
    }
    body.push("end;".to_string());
    if !lines.is_empty() {
        lines.push(String::new());
    }
    lines.push(body.join("\n"));
    let mut text = lines.join("\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

fn declaration_lines(model: &Model, closure: &Closure) -> Vec<String> {
    let mut chunks: Vec<(u32, String)> = Vec::new();
    push_decl_lines(&mut chunks, model, &model.endogenous, &closure.symbols);
    push_decl_lines(
        &mut chunks,
        model,
        &model.deterministic_exogenous,
        &closure.symbols,
    );
    push_decl_lines(&mut chunks, model, &model.exogenous, &closure.symbols);
    push_decl_lines(&mut chunks, model, &model.parameters, &closure.symbols);
    push_decl_lines(
        &mut chunks,
        model,
        &model.model_local_variables,
        &closure.locals,
    );
    push_decl_lines(&mut chunks, model, &model.predetermined, &closure.symbols);
    push_trend_lines(&mut chunks, model, &model.trend_vars, &closure.symbols);
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
    chunks.sort_by_key(|(start, _)| *start);
    let mut seen = HashSet::new();
    chunks
        .into_iter()
        .filter(|(start, line)| seen.insert((*start, line.clone())))
        .map(|(_, line)| line)
        .collect()
}

fn equation_line(model: &Model, idx: usize) -> String {
    let eq = &model.equations[idx];
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
    let mut line = keyword;
    if decl.log_transform {
        line.push_str("(log)");
    }
    line.push(' ');
    line.push_str(name);
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

fn push_decl_lines(
    out: &mut Vec<(u32, String)>,
    model: &Model,
    decls: &[Decl],
    needed: &BTreeSet<String>,
) {
    let mut seen = HashSet::new();
    for decl in decls {
        if !needed.contains(model.name(decl.name)) {
            continue;
        }
        let stmt = statement_around(&model.source, decl.span);
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
    rows: &[TrendVar],
    needed: &BTreeSet<String>,
) {
    let mut seen = HashSet::new();
    for row in rows {
        if !needed.contains(model.name(row.name)) {
            continue;
        }
        let stmt = statement_around(&model.source, row.span);
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

fn statement_around(source: &str, name: Span) -> Span {
    let bytes = source.as_bytes();
    let mut start = name.start as usize;
    while start > 0 && bytes[start - 1] != b';' {
        start -= 1;
    }
    let mut end = (name.end as usize).min(bytes.len());
    while end < bytes.len() && bytes[end] != b';' {
        end += 1;
    }
    if end < bytes.len() {
        end += 1;
    }
    if let Some(rel) = keyword_start(&source[start..name.start as usize]) {
        start += rel;
    }
    Span::new(start, end)
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

fn keyword_start(region: &str) -> Option<usize> {
    let bytes = region.as_bytes();
    let mut best = None;
    for (i, _) in region.char_indices() {
        if i > 0 && is_ident_continue(bytes[i - 1]) {
            continue;
        }
        for keyword in DECL_KEYWORDS {
            if region[i..].starts_with(keyword) {
                let end = i + keyword.len();
                if end < bytes.len() && is_ident_continue(bytes[end]) {
                    continue;
                }
                best = Some(i);
                break;
            }
        }
    }
    best
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
    let pieces = split_commas_depth0(rest);
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

fn split_commas_depth0(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (idx, ch) in input.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&input[start..idx]);
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&input[start..]);
    parts
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
            detail: "initval, endval, and histval blocks are not in the fragment".to_string(),
        });
    }
    if model.stoch_simul_span.is_some()
        || model.estimation_span.is_some()
        || !model.simul_spans.is_empty()
        || model.perfect_foresight_solver_span.is_some()
        || model.perfect_foresight_setup_span.is_some()
        || model.steady_span.is_some()
        || model.check_span.is_some()
    {
        out.push(OmittedContext {
            kind: "execution".to_string(),
            detail: "simulation and estimation commands are not in the fragment".to_string(),
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
        assert_eq!(row.index, 1);
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
parameters(long_name='discount') beta, delta;
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
            .find("parameters(long_name='discount') beta;")
            .unwrap();
        let var_c = fragment.find("var(log) c;").unwrap();
        assert!(beta < var_c, "{fragment}");
        assert!(fragment.contains("model(linear);"), "{fragment}");
        assert!(!fragment.contains("delta"), "{fragment}");
        assert!(!fragment.contains("name='out'"), "{fragment}");
        assert_eq!(result.selected_equations[0].index, 0);
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
        assert_eq!(tags.selected_equations[0].index, 0);
        assert_eq!(tags.selected_equations[1].index, 1);
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
    fn passed_dimension_is_unsupported() {
        let mut request = req(CLOSURE, &["euler"], &[]);
        request.dimension = Some("h".to_string());
        let result = extract(&request).unwrap();
        assert_eq!(result.status, ExtractStatus::UnsupportedContext);
        assert!(result.fragment.is_none());
        assert_eq!(
            result.unsupported.unwrap().kind,
            UnsupportedKind::HeterogeneousDimension
        );
    }

    #[test]
    fn heterogeneous_match_is_unsupported() {
        let src = "\
heterogeneity_dimension h;
var y;
var(heterogeneity=h) c;
model;
  [name='agg']
  y = 0;
end;
model(heterogeneity=h);
  [name='het']
  c = c(-1);
end;
";
        let het = extract(&req(src, &["het"], &[])).unwrap();
        assert_eq!(het.status, ExtractStatus::UnsupportedContext);
        assert!(het.fragment.is_none());
        assert_eq!(
            het.unsupported.unwrap().kind,
            UnsupportedKind::Heterogeneous
        );

        let agg = extract(&req(src, &["agg"], &[])).unwrap();
        assert_eq!(agg.status, ExtractStatus::Ok);
        assert!(agg.fragment.unwrap().contains("name='agg'"));
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
    fn occbin_and_static_partners_are_unsupported() {
        let occ = "\
var y;
model;
  [name='r', bind='c']
  y = 0;
  [name='other']
  y = 1;
end;
";
        let bound = extract(&req(occ, &["r"], &[])).unwrap();
        assert_eq!(bound.unsupported.unwrap().kind, UnsupportedKind::Occbin);
        let other = extract(&req(occ, &["other"], &[])).unwrap();
        assert_eq!(other.status, ExtractStatus::Ok);

        let partner = "\
var y, c;
model;
  [name='eq']
  y = 0;
  [static]
  y = 1;
  [name='ss', static]
  c = 1;
end;
";
        let shared = extract(&req(partner, &["eq"], &[])).unwrap();
        assert_eq!(
            shared.unsupported.unwrap().kind,
            UnsupportedKind::StaticDynamic
        );
        let alone = extract(&req(partner, &["ss"], &[])).unwrap();
        assert_eq!(
            alone.unsupported.unwrap().kind,
            UnsupportedKind::StaticDynamic
        );
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
        let ok = extract(&req(unused, &["eq"], &[])).unwrap();
        assert_eq!(ok.status, ExtractStatus::Ok);
        assert!(!ok.fragment.unwrap().contains("@#include"));
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
        assert!(result
            .origins
            .iter()
            .all(|origin| origin.frames.iter().all(|frame| frame.kind == "for")));
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
    fn tools_list_has_no_dynare_extract() {
        let names = registered_tool_names();
        assert!(!names.contains(&"dynare_extract"));
    }
}
