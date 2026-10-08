//! Literal equation tags and per-symbol long names for editor metadata edits.
//!
//! The language server turns the plan into one workspace edit. This module
//! does not write a file.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::diagnostic::{WritingContext, WritingRows};
use crate::equations::{equations, heterogeneous_equations, EquationRow};
use crate::expand::{EquationOrigin, ExpandReport};
use crate::expr::ExprKind;
use crate::lexer::{tokenize, TokenKind};
use crate::model::Model;
use crate::span::Span;
use crate::workspace::Workspace;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NameTagEdit {
    pub file: String,
    pub span: Span,
    pub new_text: String,
}

pub(crate) struct NameTagPlan {
    pub title: String,
    pub edits: Vec<NameTagEdit>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MetadataCompletion {
    pub label: &'static str,
    pub edit: NameTagEdit,
    /// Byte range of the whole placeholder in new_text.
    pub value: std::ops::Range<usize>,
}

pub(crate) fn equation_name_plan(
    ws: &mut Workspace,
    uri: &str,
    context: &WritingContext,
) -> Option<NameTagPlan> {
    let WritingRows::Equations(rows) = &context.rows else {
        return None;
    };
    if context.statement_ids.is_empty()
        || !crate::check_writing::context_is_current(ws, uri, "I208", context)
    {
        return None;
    }
    equation_name_plan_rows(ws, uri, rows)
}

fn equation_name_plan_rows(ws: &mut Workspace, uri: &str, rows: &[usize]) -> Option<NameTagPlan> {
    let model = ws.get_effective_model(uri)?.clone();
    let report = ws.expand_report(uri)?.clone();
    let mut sources = BTreeMap::<String, String>::new();
    for origin in &report.origins {
        let Some(file) = origin.origin_uri.as_deref() else {
            continue;
        };
        if sources.contains_key(file) {
            continue;
        }
        if let Some(text) = ws.get_source(file) {
            sources.insert(file.to_string(), text.to_string());
        }
    }
    let plan = plan_edits(&model, &report, &sources, rows)?;
    collision_safe(ws, uri, &plan.edits).then_some(plan)
}

fn plan_edits(
    model: &Model,
    report: &ExpandReport,
    sources: &BTreeMap<String, String>,
    row_ids: &[usize],
) -> Option<NameTagPlan> {
    let (aggregate, heterogeneous) = counted_rows(model, report)?;
    let selected: HashSet<_> = row_ids.iter().copied().collect();
    let mut shared: HashMap<(String, u32, u32), usize> = HashMap::new();
    for row in aggregate.iter().chain(&heterogeneous) {
        *shared.entry(span_key(row.origin)).or_insert(0) += 1;
    }
    let mut sites = Vec::new();
    let mut skipped = 0usize;
    for row in aggregate.iter().chain(&heterogeneous) {
        if !row.unnamed || !selected.contains(&row.id) {
            continue;
        }
        if !can_edit(row.origin, &shared) {
            skipped += 1;
            continue;
        }
        let file = row.origin.origin_uri.as_deref()?;
        let Some(source) = sources.get(file) else {
            skipped += 1;
            continue;
        };
        if tag_edit(source, row.origin.written_span, "eq1").is_none() {
            skipped += 1;
            continue;
        }
        sites.push((row, file, source));
    }
    let edited: HashSet<_> = sites.iter().map(|(row, _, _)| row.id).collect();
    let mut taken = reserved_names(model, &edited);
    let mut edits = Vec::new();
    for (row, file, source) in sites {
        let name = allocate(&taken);
        let (span, new_text) = tag_edit(source, row.origin.written_span, &name)?;
        taken.insert(name);
        edits.push(NameTagEdit {
            file: file.to_string(),
            span,
            new_text,
        });
    }
    if edits.is_empty() {
        return None;
    }
    let title = if skipped == 0 {
        "Add equation tags".to_string()
    } else {
        format!("Add equation tags ({skipped} skipped)")
    };
    Some(NameTagPlan { title, edits })
}

struct Counted<'a> {
    id: usize,
    unnamed: bool,
    origin: &'a EquationOrigin,
}

fn counted_rows<'a>(
    model: &Model,
    report: &'a ExpandReport,
) -> Option<(Vec<Counted<'a>>, Vec<Counted<'a>>)> {
    let aggregate_rows = equations(model);
    if aggregate_rows.len() != report.aggregate_origins.len() {
        return None;
    }
    let aggregate = aggregate_rows
        .iter()
        .zip(&report.aggregate_origins)
        .zip(
            model
                .equations
                .iter()
                .filter(|equation| !equation.is_local && !equation.static_tag),
        )
        .map(|((row, origin), equation)| counted(row, origin, equation.parse_order))
        .collect();

    let blocks = heterogeneous_equations(model);
    let mut heterogeneous = Vec::new();
    for block in &blocks {
        let origins = report.heterogeneous_origins.get(block.block_index)?;
        if origins.len() != block.equations.len() {
            return None;
        }
        for ((row, origin), equation) in block.equations.iter().zip(origins).zip(
            model.heterogeneous_models[block.block_index]
                .equations
                .iter()
                .filter(|equation| !equation.is_local && !equation.static_tag),
        ) {
            heterogeneous.push((
                block.dimension.clone(),
                counted(row, origin, equation.parse_order),
            ));
        }
    }
    heterogeneous.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then(left.1.origin.scope_index.cmp(&right.1.origin.scope_index))
    });
    let heterogeneous = heterogeneous.into_iter().map(|(_, row)| row).collect();
    Some((aggregate, heterogeneous))
}

fn counted<'a>(row: &EquationRow, origin: &'a EquationOrigin, id: usize) -> Counted<'a> {
    Counted {
        id,
        unnamed: is_unnamed(row),
        origin,
    }
}

fn is_unnamed(row: &EquationRow) -> bool {
    let named = row.tags.get("name").is_some_and(|value| !value.is_empty());
    !named && row.name.is_empty()
}

fn taken_names(model: &Model) -> BTreeSet<String> {
    let mut taken = BTreeSet::new();
    let equations = model.equations.iter().chain(
        model
            .heterogeneous_models
            .iter()
            .flat_map(|block| block.equations.iter()),
    );
    for equation in equations {
        if let Some(value) = equation.tag_map.get("name") {
            if !value.is_empty() {
                taken.insert(value.clone());
            }
        }
        if !equation.name.is_empty() {
            taken.insert(equation.name.clone());
        }
    }
    taken
}

/// Explicit names are root-wide. Untouched rows also keep the defaults that
/// expandEqTags would assign in execution order. An empty explicit key has no
/// implicit fallback.
fn reserved_names(model: &Model, edited: &HashSet<usize>) -> BTreeSet<String> {
    let mut taken = taken_names(model);
    for rows in std::iter::once(model.equations.as_slice()).chain(
        model
            .heterogeneous_models
            .iter()
            .map(|block| block.equations.as_slice()),
    ) {
        let mut index = 0;
        for equation in rows.iter().filter(|equation| !equation.is_local) {
            index += 1;
            if edited.contains(&equation.parse_order) || equation.tag_map.contains_key("name") {
                continue;
            }
            let lhs = equation
                .lhs_expr
                .and_then(|id| match model.exprs.get(id).kind {
                    ExprKind::Ident { name, .. } => Some(model.name(name).to_string()),
                    _ => None,
                });
            if let Some(lhs) = lhs.filter(|lhs| !taken.contains(lhs)) {
                taken.insert(lhs);
            } else {
                taken.insert(index.to_string());
            }
        }
    }
    taken
}

fn allocate(taken: &BTreeSet<String>) -> String {
    let mut n = 1u32;
    loop {
        let candidate = format!("eq{n}");
        if !taken.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

fn apply_edits(ws: &mut Workspace, edits: &[NameTagEdit]) -> Option<()> {
    let mut files = BTreeMap::<String, Vec<&NameTagEdit>>::new();
    for edit in edits {
        files.entry(edit.file.clone()).or_default().push(edit);
    }
    for (file, mut edits) in files {
        let mut source = ws.get_source(&file)?.to_string();
        edits.sort_by_key(|edit| std::cmp::Reverse((edit.span.start, edit.span.end)));
        for edit in edits {
            source.get(edit.span.start as usize..edit.span.end as usize)?;
            source.replace_range(
                edit.span.start as usize..edit.span.end as usize,
                &edit.new_text,
            );
        }
        ws.update_document(&file, source);
    }
    Some(())
}

fn collision_safe(ws: &mut Workspace, root: &str, edits: &[NameTagEdit]) -> bool {
    let before = collision_counts(ws, root);
    let mut candidate = ws.snapshot_with_root_settings(root);
    if apply_edits(&mut candidate, edits).is_none() {
        return false;
    }
    collision_counts(&mut candidate, root)
        .iter()
        .all(|(message, count)| *count <= before.get(message).copied().unwrap_or(0))
}

fn collision_counts(ws: &mut Workspace, root: &str) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for diagnostic in crate::diagnostic::check_in_workspace_with_origins(ws, root).diagnostics {
        if diagnostic.code == "E257" {
            *counts.entry(diagnostic.message).or_insert(0) += 1;
        }
    }
    counts
}

pub(crate) fn long_name_plan(
    ws: &mut Workspace,
    root: &str,
    context: &WritingContext,
) -> Option<NameTagPlan> {
    let WritingRows::Declarations(rows) = &context.rows else {
        return None;
    };
    if context.statement_ids.is_empty()
        || !crate::check_writing::context_is_current(ws, root, "I209", context)
    {
        return None;
    }
    let model = ws.get_effective_model(root)?.clone();
    let report = ws.expand_report(root)?.clone();
    let selected: HashSet<_> = rows.iter().copied().collect();
    let mut edits = Vec::new();
    let mut skipped = 0;
    for (written, occurrence) in model
        .written_declarations
        .iter()
        .zip(&report.model_map.declarations)
    {
        if !selected.contains(&written.declaration.parse_order) {
            continue;
        }
        let Some(anchor) = occurrence.anchor.as_ref() else {
            skipped += 1;
            continue;
        };
        let Some(file) = anchor.file.as_deref() else {
            skipped += 1;
            continue;
        };
        let Some(source) = ws.get_source(file) else {
            skipped += 1;
            continue;
        };
        let name = model.name(written.declaration.name);
        // A declaration token produced by @{...} is not a literal name site.
        if source.get(anchor.span.start as usize..anchor.span.end as usize) != Some(name) {
            skipped += 1;
            continue;
        }
        let Some((span, new_text)) = declaration_edit(source, anchor.span, name) else {
            skipped += 1;
            continue;
        };
        let same = model
            .written_declarations
            .iter()
            .zip(&report.model_map.declarations)
            .filter(|(_, other)| other.anchor == occurrence.anchor)
            .all(|(other, _)| {
                model.name(other.declaration.name) == name
                    && !other
                        .declaration
                        .long_name
                        .as_ref()
                        .is_some_and(|value| !value.is_empty())
            });
        if !same {
            skipped += 1;
            continue;
        }
        let edit = NameTagEdit {
            file: file.to_string(),
            span,
            new_text,
        };
        if !edits.contains(&edit) {
            edits.push(edit);
        }
    }
    if edits.is_empty() {
        return None;
    }
    let title = if skipped == 0 {
        "Add long names".to_string()
    } else {
        format!("Add long names ({skipped} skipped)")
    };
    Some(NameTagPlan { title, edits })
}

fn span_key(origin: &EquationOrigin) -> (String, u32, u32) {
    (
        origin.origin_uri.clone().unwrap_or_default(),
        origin.written_span.start,
        origin.written_span.end,
    )
}

fn can_edit(origin: &EquationOrigin, shared: &HashMap<(String, u32, u32), usize>) -> bool {
    if origin.ambiguous || origin.loop_copy || origin.origin_uri.is_none() {
        return false;
    }
    shared.get(&span_key(origin)).copied() == Some(1)
}

enum ExistingName {
    None,
    Empty { at: u32, text: String },
    Kept,
}

fn tag_edit(source: &str, span: Span, name: &str) -> Option<(Span, String)> {
    let start = span.start as usize;
    let end = span.end as usize;
    if start >= end || end > source.len() {
        return None;
    }
    let groups = leading_groups(source, start, end);
    if groups.is_empty() {
        if source.as_bytes().get(start) == Some(&b'[') {
            return None;
        }
        return Some((
            Span {
                start: span.start,
                end: span.start,
            },
            format!("[name='{name}'] "),
        ));
    }
    let mut empty = None;
    for &(open, close) in &groups {
        match name_in_group(source, open, close, name) {
            ExistingName::Kept => return None,
            ExistingName::Empty { at, text } => {
                if empty.is_none() {
                    empty = Some((at, text));
                }
            }
            ExistingName::None => {}
        }
    }
    if let Some((at, text)) = empty {
        return Some((Span { start: at, end: at }, text));
    }
    let (open, close) = groups[0];
    let interior = source.get(open + 1..close)?;
    let text = if tokenize(interior).len() == 1 {
        format!("name='{name}'")
    } else {
        format!(", name='{name}'")
    };
    let close = u32::try_from(close).ok()?;
    Some((
        Span {
            start: close,
            end: close,
        },
        text,
    ))
}

fn leading_groups(source: &str, start: usize, end: usize) -> Vec<(usize, usize)> {
    let mut at = start;
    let mut groups = Vec::new();
    while at < end {
        let tokens = tokenize(&source[at..end]);
        let Some(token) = tokens
            .first()
            .filter(|token| token.kind == TokenKind::LBrack)
        else {
            break;
        };
        let open = at + token.span.start as usize;
        let Some(close) = group_close(source, open, b']').filter(|close| *close < end) else {
            break;
        };
        groups.push((open, close));
        at = close + 1;
    }
    groups
}

fn name_in_group(source: &str, open: usize, close: usize, name: &str) -> ExistingName {
    literal_value(source, open, close, "name", name)
}

fn declaration_edit(source: &str, symbol: Span, name: &str) -> Option<(Span, String)> {
    let tokens = tokenize(source.get(symbol.end as usize..)?);
    let mut at = symbol.end as usize;
    let mut next = 0;
    if tokens.first()?.kind == TokenKind::Latex {
        let tex = &tokens[0];
        if !tex.text(&source[symbol.end as usize..]).ends_with('$') {
            return None;
        }
        at += tex.span.end as usize;
        next += 1;
    }
    if tokens.get(next)?.kind != TokenKind::LParen {
        return Some((Span::new(at, at), format!(" (long_name='{name}')")));
    }
    let open = symbol.end as usize + tokens[next].span.start as usize;
    let close = group_close(source, open, b')')?;
    match literal_value(source, open, close, "long_name", name) {
        ExistingName::Kept => None,
        ExistingName::Empty { at, text } => Some((Span { start: at, end: at }, text)),
        ExistingName::None => Some((
            Span::new(close, close),
            format!(
                "{}long_name='{name}'",
                if tokenize(&source[open + 1..close]).len() == 1 {
                    ""
                } else {
                    ", "
                }
            ),
        )),
    }
}

fn group_close(source: &str, open: usize, delimiter: u8) -> Option<usize> {
    let close_kind = if delimiter == b']' {
        TokenKind::RBrack
    } else {
        TokenKind::RParen
    };
    let open_kind = if delimiter == b']' {
        TokenKind::LBrack
    } else {
        TokenKind::LParen
    };
    let mut depth = 0;
    for token in tokenize(source.get(open..)?) {
        if token.kind == open_kind {
            depth += 1;
        }
        if token.kind == close_kind {
            depth -= 1;
            if depth == 0 {
                return Some(open + token.span.start as usize);
            }
        }
    }
    None
}

/// Read keys as lexer tokens so comments and unrelated quoted values cannot
/// become metadata. A nonempty written macro string is never flattened.
fn literal_value(source: &str, open: usize, close: usize, key: &str, value: &str) -> ExistingName {
    let interior = &source[open + 1..close];
    let tokens = tokenize(interior);
    let mut empty = None;
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Ident || token.text(interior) != key {
            continue;
        }
        let at = open + 1 + token.span.end as usize;
        let Some(next) = tokens.get(index + 1) else {
            return ExistingName::Kept;
        };
        if matches!(next.kind, TokenKind::Comma | TokenKind::Eof) {
            empty = Some(ExistingName::Empty {
                at: at as u32,
                text: format!("='{value}'"),
            });
            continue;
        }
        if next.kind != TokenKind::Eq {
            return ExistingName::Kept;
        }
        let Some(next_value) = tokens.get(index + 2) else {
            return ExistingName::Kept;
        };
        if matches!(next_value.kind, TokenKind::Comma | TokenKind::Eof) {
            let at = open + 1 + next.span.end as usize;
            empty = Some(ExistingName::Empty {
                at: at as u32,
                text: format!("'{value}'"),
            });
        } else if next_value.kind == TokenKind::String
            && matches!(next_value.text(interior), "''" | "\"\"")
        {
            let at = open + 1 + next_value.span.start as usize + 1;
            empty = Some(ExistingName::Empty {
                at: at as u32,
                text: value.to_string(),
            });
        } else {
            return ExistingName::Kept;
        }
    }
    empty.unwrap_or(ExistingName::None)
}

const COMPLETION_PROOF: &str = "__dygnosis_metadata_site__";

#[derive(Clone, Copy)]
enum MetadataKind {
    Equation,
    Declaration,
}

struct CompletionSite {
    kind: MetadataKind,
    group: Option<usize>,
    span: Span,
    prefix: String,
    suffix: String,
}

impl CompletionSite {
    fn completion(&self, file: &str, value: &str) -> MetadataCompletion {
        let value_at = self.prefix.len();
        MetadataCompletion {
            label: match self.kind {
                MetadataKind::Equation => "name",
                MetadataKind::Declaration => "long_name",
            },
            edit: NameTagEdit {
                file: file.to_string(),
                span: self.span,
                new_text: format!("{}{value}{}", self.prefix, self.suffix),
            },
            value: value_at..value_at + value.len(),
        }
    }
}

/// Repair only one recognised metadata group in a snapshot. The parser and
/// source map must then prove that it is a counted equation or a declaration.
/// This permits unfinished prefixes without guessing through other recovery.
pub(crate) fn metadata_completion(
    ws: &mut Workspace,
    root: &str,
    file: &str,
    cursor: u32,
) -> Option<MetadataCompletion> {
    ws.input_revision(root)?;
    let file = crate::include_resolver::normalize_uri(file);
    let source = ws.get_source(&file)?.to_string();
    for site in completion_sites(&source, cursor as usize) {
        let proof = site.completion(&file, COMPLETION_PROOF);
        let mut candidate = ws.snapshot_with_root_settings(root);
        apply_edits(&mut candidate, std::slice::from_ref(&proof.edit))?;
        let Some(mut model) = candidate.get_effective_model(root).cloned() else {
            continue;
        };
        let Some(mut report) = candidate.expand_report(root).cloned() else {
            continue;
        };
        let proof_value = Span::new(
            proof.edit.span.start as usize + proof.value.start,
            proof.edit.span.start as usize + proof.value.end,
        );
        let selected_declarations = declaration_proof_rows(
            &model,
            &report,
            &file,
            candidate.get_source(&file)?,
            proof_value,
        );
        // A declaration being typed at EOF has no semicolon yet. Complete
        // only that selected statement in the proof; acceptance still inserts
        // metadata alone. A later statement or any other recovery stays unsafe.
        if matches!(site.kind, MetadataKind::Declaration)
            && matches!(model.parse_issues.as_slice(), [issue]
                if matches!(&issue.kind, crate::model::ParseIssueKind::MissingDeclSemi { next_span: None, .. }))
            && !selected_declarations.is_empty()
            && selected_declarations.iter().all(|&index| {
                let written = &model.written_declarations[index];
                written.statement_id + 1 == model.statements.len()
                    && !model.statements[written.statement_id].complete
            })
        {
            let repaired = format!("{};", candidate.get_source(&file)?);
            candidate.update_document(&file, repaired);
            model = candidate.get_effective_model(root)?.clone();
            report = candidate.expand_report(root)?.clone();
        }
        if !report.model_map.complete
            || crate::check_writing::model_structure_incomplete(&model)
            || !candidate.find_unresolved_includes(root).is_empty()
            || candidate
                .include_records(root)
                .is_some_and(|records| !records.cycles.is_empty())
        {
            continue;
        }
        let value = match site.kind {
            MetadataKind::Equation => {
                let (aggregate, heterogeneous) = counted_rows(&model, &report)?;
                let rows: Vec<_> = aggregate.iter().chain(&heterogeneous).collect();
                let mut shared = HashMap::new();
                for row in &rows {
                    *shared.entry(span_key(row.origin)).or_insert(0) += 1;
                }
                let matching: Vec<_> = rows
                    .into_iter()
                    .filter(|row| {
                        row.origin.origin_uri.as_deref() == Some(file.as_str())
                            && row.origin.written_span.start <= site.span.start
                            && site.span.start < row.origin.written_span.end
                            && model.written_equations.iter().any(|written| {
                                written.token_range.start == row.id
                                    && written.equation.tag_map.get("name").map(String::as_str)
                                        == Some(COMPLETION_PROOF)
                            })
                    })
                    .collect();
                if matching.len() != 1 || !can_edit(matching[0].origin, &shared) {
                    continue;
                }
                let origin = matching[0].origin;
                let delta = proof.edit.new_text.len() as i64
                    - (proof.edit.span.end - proof.edit.span.start) as i64;
                let Ok(original_end) = usize::try_from(origin.written_span.end as i64 - delta)
                else {
                    continue;
                };
                if original_end > source.len() {
                    continue;
                }
                let original_groups =
                    leading_groups(&source, origin.written_span.start as usize, original_end);
                if original_groups.iter().any(|&(open, close)| {
                    match literal_value(&source, open, close, "name", COMPLETION_PROOF) {
                        ExistingName::Kept => true,
                        ExistingName::Empty { .. } => site.group != Some(open),
                        ExistingName::None => false,
                    }
                }) {
                    continue;
                }
                let mut taken = reserved_names(&model, &HashSet::from([matching[0].id]));
                taken.remove(COMPLETION_PROOF);
                allocate(&taken)
            }
            MetadataKind::Declaration => {
                let matching: Vec<_> = declaration_proof_rows(
                    &model,
                    &report,
                    &file,
                    candidate.get_source(&file)?,
                    proof_value,
                )
                .into_iter()
                .map(|index| {
                    (
                        &model.written_declarations[index],
                        &report.model_map.declarations[index],
                    )
                })
                .collect();
                let Some((first, occurrence)) = matching.first() else {
                    continue;
                };
                let name = model.name(first.declaration.name);
                if matching.iter().any(|(written, other)| {
                    other.anchor != occurrence.anchor
                        || model.name(written.declaration.name) != name
                }) {
                    continue;
                }
                // Reject an insertion before an existing TeX name or before
                // an option group. Dynare orders TeX before per-name options.
                let anchor = occurrence.anchor.as_ref()?;
                let old_suffix = tokenize(source.get(anchor.span.end as usize..)?);
                let after_tex = usize::from(
                    old_suffix
                        .first()
                        .is_some_and(|token| token.kind == TokenKind::Latex),
                );
                if site.group.is_none()
                    && old_suffix
                        .get(after_tex)
                        .is_some_and(|token| token.kind == TokenKind::LParen)
                {
                    continue;
                }
                if old_suffix.first().is_some_and(|token| {
                    matches!(token.kind, TokenKind::Latex | TokenKind::LParen)
                        && anchor.span.end as usize + token.span.start as usize
                            >= site.span.start as usize
                        && matches!(
                            source.as_bytes().get(site.span.start as usize),
                            Some(b' ') | Some(b';') | None
                        )
                }) {
                    continue;
                }
                name.to_string()
            }
        };
        let completion = site.completion(&file, &value);
        if collision_safe(ws, root, std::slice::from_ref(&completion.edit)) {
            return Some(completion);
        }
    }
    None
}

/// The marker alone is not a proof: a user can already have that literal
/// label elsewhere. Bind it to the inserted value in the mapped symbol's
/// immediate per-name group and its owning statement.
fn declaration_proof_rows(
    model: &Model,
    report: &ExpandReport,
    file: &str,
    source: &str,
    value: Span,
) -> Vec<usize> {
    model
        .written_declarations
        .iter()
        .zip(&report.model_map.declarations)
        .enumerate()
        .filter(|(_, (written, occurrence))| {
            matches!(
                written.written_kind.as_str(),
                "var" | "varexo" | "varexo_det" | "parameters"
            ) && written.declaration.long_name.as_deref() == Some(COMPLETION_PROOF)
                && occurrence.anchor.as_ref().is_some_and(|anchor| {
                    anchor.file.as_deref() == Some(file)
                        && source.get(anchor.span.start as usize..anchor.span.end as usize)
                            == Some(model.name(written.declaration.name))
                        && declaration_value_span(source, anchor.span) == Some(value)
                })
                && report
                    .model_map
                    .statements
                    .get(written.statement_id)
                    .is_some_and(|statement| {
                        statement.segments.iter().any(|segment| {
                            segment.file.as_deref() == Some(file)
                                && segment.span.start <= value.start
                                && value.end <= segment.span.end
                        })
                    })
        })
        .map(|(index, _)| index)
        .collect()
}

fn declaration_value_span(source: &str, symbol: Span) -> Option<Span> {
    let suffix = source.get(symbol.end as usize..)?;
    let tokens = tokenize(suffix);
    let next = usize::from(tokens.first()?.kind == TokenKind::Latex);
    let token = tokens.get(next)?;
    if token.kind != TokenKind::LParen {
        return None;
    }
    let open = symbol.end as usize + token.span.start as usize;
    let close = group_close(source, open, b')')?;
    let interior = &source[open + 1..close];
    tokenize(interior).windows(3).find_map(|entry| {
        (entry[0].kind == TokenKind::Ident
            && entry[0].text(interior) == "long_name"
            && entry[1].kind == TokenKind::Eq
            && entry[2].kind == TokenKind::String)
            .then(|| {
                Span::new(
                    open + 1 + entry[2].span.start as usize + 1,
                    open + 1 + entry[2].span.end as usize - 1,
                )
            })
    })
}

fn completion_sites(source: &str, cursor: usize) -> Vec<CompletionSite> {
    if cursor > source.len()
        || !source.is_char_boundary(cursor)
        || cursor_in_comment(source, cursor)
    {
        return Vec::new();
    }
    let tokens = tokenize(source);
    let mut sites = Vec::new();
    let mut inside_group = false;
    for token in tokens
        .iter()
        .rev()
        .filter(|token| token.span.start < cursor as u32)
    {
        let (kind, key, delimiter) = match token.kind {
            TokenKind::LBrack => (MetadataKind::Equation, "name", b']'),
            TokenKind::LParen => (MetadataKind::Declaration, "long_name", b')'),
            _ => continue,
        };
        let open = token.span.start as usize;
        let close = group_close(source, open, delimiter);
        if close.is_some_and(|close| cursor > close + 1) {
            continue;
        }
        inside_group |= close.is_none_or(|close| cursor <= close);
        if let Some(site) = group_completion_site(source, cursor, open, close, kind, key, delimiter)
        {
            sites.push(site);
        }
    }
    // Ordinary insertion requires a token boundary. Strings, TeX, macro
    // expressions, and identifiers being typed do not become metadata sites.
    if inside_group
        || tokens
            .iter()
            .any(|token| token.span.start < cursor as u32 && (cursor as u32) < token.span.end)
    {
        return sites;
    }
    sites.push(CompletionSite {
        kind: MetadataKind::Equation,
        group: None,
        span: Span::new(cursor, cursor),
        prefix: "[name='".into(),
        suffix: "'] ".into(),
    });
    sites.push(CompletionSite {
        kind: MetadataKind::Declaration,
        group: None,
        span: Span::new(cursor, cursor),
        prefix: " (long_name='".into(),
        suffix: "')".into(),
    });
    sites
}

fn group_completion_site(
    source: &str,
    cursor: usize,
    open: usize,
    close: Option<usize>,
    kind: MetadataKind,
    key: &str,
    delimiter: u8,
) -> Option<CompletionSite> {
    let end = close.unwrap_or(cursor);
    if end < open + 1 || cursor < open + 1 {
        return None;
    }
    if let Some(close) = close {
        match literal_value(source, open, close, key, COMPLETION_PROOF) {
            ExistingName::Empty { at, text } => {
                let value_at = text.find(COMPLETION_PROOF)?;
                // The cursor must be on the metadata key or its empty value.
                let tokens = tokenize(&source[open + 1..close]);
                let key_token = tokens.iter().find(|token| {
                    token.kind == TokenKind::Ident && token.text(&source[open + 1..close]) == key
                })?;
                let key_at = open + 1 + key_token.span.start as usize;
                let entry_end = tokens
                    .iter()
                    .find(|token| {
                        token.kind == TokenKind::Comma && token.span.start >= key_token.span.end
                    })
                    .map(|token| open + 1 + token.span.start as usize)
                    .unwrap_or(close);
                if cursor < key_at || cursor > entry_end {
                    return None;
                }
                let at = at as usize;
                // Completion ranges contain the request position. Whitespace
                // after an empty key can stay before a value inserted there;
                // a typed quote or key prefix is copied into the replacement.
                let (span, before, after) = if cursor >= at && source[at..cursor].trim().is_empty()
                {
                    (Span::new(cursor, cursor), "", "")
                } else {
                    let start = cursor.min(at);
                    let end = cursor.max(at);
                    if source[start..end].contains(['\r', '\n']) {
                        return None;
                    }
                    (
                        Span::new(start, end),
                        if cursor < at { &source[cursor..at] } else { "" },
                        if cursor > at { &source[at..cursor] } else { "" },
                    )
                };
                return Some(CompletionSite {
                    kind,
                    group: Some(open),
                    span,
                    prefix: format!("{before}{}", &text[..value_at]),
                    suffix: format!("{}{after}", &text[value_at + COMPLETION_PROOF.len()..]),
                });
            }
            ExistingName::Kept => return None,
            ExistingName::None => {}
        }
    }
    let interior = source.get(open + 1..end)?;
    let tokens = tokenize(interior);
    let relative_cursor = cursor.saturating_sub(open + 1);
    let start = tokens
        .iter()
        .filter(|token| {
            token.kind == TokenKind::Comma && token.span.end as usize <= relative_cursor
        })
        .map(|token| token.span.end as usize)
        .max()
        .unwrap_or(0);
    let next_comma = tokens
        .iter()
        .find(|token| {
            token.kind == TokenKind::Comma && token.span.start as usize >= relative_cursor
        })
        .map(|token| token.span.start as usize)
        .unwrap_or(interior.len());
    let entry = interior.get(start..next_comma)?;
    let entry_tokens: Vec<_> = tokenize(entry)
        .into_iter()
        .filter(|token| token.kind != TokenKind::Eof)
        .collect();
    let base = open + 1 + start;
    let (replace, prefix) = if entry_tokens.is_empty() {
        (
            Span::new(cursor.min(end), cursor.min(end)),
            format!("{key}='"),
        )
    } else {
        let first = &entry_tokens[0];
        if first.kind != TokenKind::Ident || !key.starts_with(first.text(entry)) {
            return None;
        }
        if entry_tokens
            .get(1)
            .is_some_and(|token| token.kind != TokenKind::Eq)
        {
            return None;
        }
        if entry_tokens.len() > 3 {
            return None;
        }
        if let Some(value) = entry_tokens.get(2) {
            if value.kind != TokenKind::String
                || !matches!(value.text(entry), "'" | "\"" | "''" | "\"\"")
            {
                return None;
            }
        }
        let last = entry_tokens.last()?;
        // Replacing a typed prefix must preserve comments; decline a prefix
        // containing them rather than deleting that source text.
        for pair in entry_tokens.windows(2) {
            if !entry[pair[0].span.end as usize..pair[1].span.start as usize]
                .trim()
                .is_empty()
            {
                return None;
            }
        }
        (
            Span::new(
                base + first.span.start as usize,
                base + last.span.end as usize,
            ),
            format!("{key}='"),
        )
    };
    if cursor < replace.start as usize || cursor > next_comma + base {
        return None;
    }
    let mut suffix = if close.is_some() {
        "'".to_string()
    } else {
        format!("'{}", delimiter as char)
    };
    let mut replace = replace;
    if (replace.end as usize) < cursor {
        let tail = source.get(replace.end as usize..cursor)?;
        if tail.contains(['\r', '\n']) {
            return None;
        }
        suffix.push_str(tail);
        replace.end = cursor as u32;
    }
    Some(CompletionSite {
        kind,
        group: Some(open),
        span: replace,
        prefix,
        suffix,
    })
}

fn cursor_in_comment(source: &str, cursor: usize) -> bool {
    let bytes = source.as_bytes();
    let mut at = 0;
    let mut quote = None;
    while at < cursor {
        if let Some(delimiter) = quote {
            if bytes[at] == delimiter {
                quote = None;
            }
            at += 1;
        } else if matches!(bytes[at], b'\'' | b'"' | b'$') {
            quote = Some(bytes[at]);
            at += 1;
        } else if bytes.get(at..at + 2) == Some(b"/*") {
            let end = source[at + 2..]
                .find("*/")
                .map(|length| at + 4 + length)
                .unwrap_or(source.len());
            if cursor < end {
                return true;
            }
            at = end;
        } else if bytes.get(at..at + 2) == Some(b"//") || bytes[at] == b'%' {
            let end = source[at..]
                .find('\n')
                .map(|length| at + length)
                .unwrap_or(source.len());
            if cursor <= end {
                return true;
            }
            at = end;
        } else {
            at += 1;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    // Allocator tests exercise all rows; the editor entry point always validates
    // a single note's revision-bound scope before calling the row planner.
    fn all_rows_plan(ws: &mut Workspace, uri: &str) -> Option<NameTagPlan> {
        let model = ws.get_effective_model(uri)?;
        let rows: Vec<_> = model
            .equations
            .iter()
            .chain(
                model
                    .heterogeneous_models
                    .iter()
                    .flat_map(|block| &block.equations),
            )
            .map(|equation| equation.parse_order)
            .collect();
        equation_name_plan_rows(ws, uri, &rows)
    }

    fn apply(source: &str, edits: &[(Span, &str)]) -> String {
        let mut ordered: Vec<(Span, &str)> = edits.to_vec();
        ordered.sort_by_key(|(span, _)| std::cmp::Reverse(span.start));
        let mut out = source.to_string();
        for (span, text) in ordered {
            out.replace_range(span.start as usize..span.end as usize, text);
        }
        out
    }

    fn planned(source: &str) -> Option<(String, String)> {
        let mut ws = Workspace::new();
        let uri = r"C:\dygnosis-equation-names\plan.mod";
        ws.update_document(uri, source);
        let plan = all_rows_plan(&mut ws, uri)?;
        let edits: Vec<(Span, String)> = plan
            .edits
            .iter()
            .map(|edit| (edit.span, edit.new_text.clone()))
            .collect();
        let pairs: Vec<(Span, &str)> = edits
            .iter()
            .map(|(span, text)| (*span, text.as_str()))
            .collect();
        Some((plan.title, apply(source, &pairs)))
    }

    #[test]
    fn fills_an_empty_name_and_keeps_other_tags() {
        let source = include_str!("../tests/fixtures/equation_names/tags.mod");
        let (title, edited) = planned(source).expect("action");
        assert_eq!(title, "Add equation tags");
        assert_eq!(
            edited,
            include_str!("../tests/fixtures/equation_names/tags.named.mod")
        );
    }

    #[test]
    fn an_if_branch_names_each_equation() {
        let source = "\
var y, z;
@#if 1
model;
y = 1;
z = 2;
end;
@#endif
";
        let (title, edited) = planned(source).expect("action");
        assert_eq!(title, "Add equation tags");
        let model_at = edited.find("model;").unwrap();
        let first = edited.find("[name='eq1']").unwrap();
        let second = edited.find("[name='eq2']").unwrap();
        assert!(model_at < first && first < second, "{edited}");
        assert!(edited.contains("y = 1;"));
        assert!(edited.contains("z = 2;"));
    }

    #[test]
    fn an_if_branch_with_a_local_names_the_counted_equation() {
        let source = "\
var y;
@#if 1
model;
# z = 1;
y = z;
end;
@#endif
";
        let (title, edited) = planned(source).expect("action");
        assert_eq!(title, "Add equation tags");
        let local = edited.find("# z = 1;").unwrap();
        let named = edited.find("[name='eq1']").unwrap();
        assert!(local < named, "{edited}");
        assert!(
            edited.contains("[name='eq1'] y = z;") || edited.contains("[name='eq1']\ny = z;"),
            "{edited}"
        );
        assert!(!edited[..=local].contains("[name="), "{edited}");
    }

    #[test]
    fn a_for_with_an_inner_if_is_a_copy() {
        let source = "\
var y;
@#define once = 1:1
model;
@#for i in once
@#if 1
y = @{i};
@#else
y = 0;
@#endif
@#endfor
end;
";
        assert!(
            planned(source).is_none(),
            "a one-iteration loop stays unedited"
        );
    }

    #[test]
    fn a_for_copy_is_skipped_and_keeps_its_number() {
        let source = include_str!("../tests/fixtures/equation_names/for_copy.mod");
        let (title, edited) = planned(source).expect("action");
        assert_eq!(title, "Add equation tags (2 skipped)");
        assert_eq!(
            edited,
            include_str!("../tests/fixtures/equation_names/for_copy.named.mod")
        );
    }

    #[test]
    fn a_name_on_a_skipped_loop_is_still_taken() {
        let source = include_str!("../tests/fixtures/equation_names/taken.mod");
        let (title, edited) = planned(source).expect("action");
        assert_eq!(title, "Add equation tags");
        assert_eq!(
            edited,
            include_str!("../tests/fixtures/equation_names/taken.named.mod")
        );
    }

    #[test]
    fn cross_scope_names_visit_aggregate_then_dimensions() {
        let source = include_str!("../tests/fixtures/equation_names/collision.mod");
        let (title, edited) = planned(source).expect("action");
        assert_eq!(title, "Add equation tags");
        assert_eq!(
            edited,
            include_str!("../tests/fixtures/equation_names/collision.named.mod")
        );
    }

    #[test]
    fn one_iteration_loop_is_skipped() {
        let source = "\
var y;
@#define is = 1:1
model;
y = y(-1);
@#for i in is
y = y(-1);
@#endfor
end;
";
        let (title, edited) = planned(source).expect("action");
        assert_eq!(title, "Add equation tags (1 skipped)");
        assert!(edited.contains("[name='eq1'] y = y(-1);"));
        assert!(edited.contains("@#for i in is\ny = y(-1);"));
    }

    #[test]
    fn a_file_of_only_loop_copies_has_no_plan() {
        let source = include_str!("../tests/fixtures/equation_names/only_for.mod");
        assert!(planned(source).is_none());
    }

    #[test]
    fn a_shared_include_is_skipped_and_keeps_its_number() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/equation_names/shared_root.mod");
        let inc = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/equation_names/shared_eq.inc");
        let root_text = std::fs::read_to_string(&root)
            .unwrap()
            .replace("\r\n", "\n");
        let inc_text = std::fs::read_to_string(&inc).unwrap().replace("\r\n", "\n");
        let mut ws = Workspace::new();
        let root_uri = root.to_string_lossy();
        ws.update_document(&root_uri, &root_text);
        ws.update_document(&inc.to_string_lossy(), &inc_text);
        let plan = all_rows_plan(&mut ws, &root_uri).expect("action");
        assert_eq!(plan.title, "Add equation tags (2 skipped)");
        assert_eq!(plan.edits.len(), 1);
        let edit = &plan.edits[0];
        assert_eq!(
            crate::include_resolver::normalize_uri(&edit.file),
            crate::include_resolver::normalize_uri(&root_uri)
        );
        let edited = apply(&root_text, &[(edit.span, edit.new_text.as_str())]);
        let expected = include_str!("../tests/fixtures/equation_names/shared_root.named.mod")
            .replace("\r\n", "\n");
        assert_eq!(edited, expected);
        assert_eq!(
            ws.get_source(&inc.to_string_lossy()).unwrap(),
            inc_text.as_str()
        );
    }

    #[test]
    fn a_unique_include_is_edited_in_that_file() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/equation_names/one_inc.mod");
        let inc = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/equation_names/one_eq.inc");
        let root_text = std::fs::read_to_string(&root)
            .unwrap()
            .replace("\r\n", "\n");
        let inc_text = std::fs::read_to_string(&inc).unwrap().replace("\r\n", "\n");
        let mut ws = Workspace::new();
        let root_uri = root.to_string_lossy();
        let inc_uri = inc.to_string_lossy();
        ws.update_document(&root_uri, &root_text);
        ws.update_document(&inc_uri, &inc_text);
        let plan = all_rows_plan(&mut ws, &root_uri).expect("action");
        assert_eq!(plan.title, "Add equation tags");
        assert_eq!(plan.edits.len(), 1);
        let edit = &plan.edits[0];
        assert_eq!(
            crate::include_resolver::normalize_uri(&edit.file),
            crate::include_resolver::normalize_uri(&inc_uri)
        );
        let edited = apply(&inc_text, &[(edit.span, edit.new_text.as_str())]);
        let expected =
            include_str!("../tests/fixtures/equation_names/one_eq.named.inc").replace("\r\n", "\n");
        assert_eq!(edited, expected);
    }

    #[test]
    fn an_equation_from_two_files_is_skipped() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/equation_names/mixed.mod");
        let inc = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/equation_names/piece.inc");
        let root_text = std::fs::read_to_string(&root)
            .unwrap()
            .replace("\r\n", "\n");
        let inc_text = std::fs::read_to_string(&inc).unwrap().replace("\r\n", "\n");
        let mut ws = Workspace::new();
        let root_uri = root.to_string_lossy();
        ws.update_document(&root_uri, &root_text);
        ws.update_document(&inc.to_string_lossy(), &inc_text);
        let plan = all_rows_plan(&mut ws, &root_uri).expect("action");
        assert_eq!(plan.title, "Add equation tags (1 skipped)");
        assert_eq!(plan.edits.len(), 1);
        let edit = &plan.edits[0];
        let edited = apply(&root_text, &[(edit.span, edit.new_text.as_str())]);
        assert!(edited.contains("[name='eq1'] y = y(-1);"));
        assert!(edited.contains("y =\n@#include \"piece.inc\""));
        assert_eq!(ws.get_source(&inc.to_string_lossy()).unwrap(), inc_text);
    }

    #[test]
    fn rejected_local_tag_does_not_reserve_name() {
        let source = "\
var y;
model;
[name='eq1']
# x = 1;
y = y(-1);
end;
";
        let (title, edited) = planned(source).expect("action");
        assert_eq!(title, "Add equation tags");
        assert!(edited.contains("[name='eq1']\n# x = 1;"));
        assert!(edited.contains("[name='eq1'] y = y(-1);"));
    }

    #[test]
    fn a_taken_suffix_moves_to_the_next_free_name() {
        let source = "\
heterogeneity_dimension d;
var y;
var(heterogeneity=d) c;
model;
[name='eq1']
y = y(-1);
[name='eq2']
y = y(-1);
end;
model(heterogeneity=d);
c = c(-1);
end;
";
        let (_, edited) = planned(source).expect("action");
        assert!(edited.contains("[name='eq1']\ny = y(-1);"));
        assert!(edited.contains("[name='eq2']\ny = y(-1);"));
        assert!(edited.contains("[name='eq3'] c = c(-1);"));
    }
}
