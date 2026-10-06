//! Writing-preference summaries I208 and I209.
//!
//! I208/I209 notes retain the exact rows of their owning statement executions.
//! Neither is a Dynare refusal.

use crate::intern::Name;
use crate::model::{Decl, Equation, Model};
use crate::span::Span;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::diagnostic::{Diagnostic, Severity};

pub(crate) fn writing_summaries(model: &Model) -> Vec<Diagnostic> {
    if model_structure_incomplete(model) {
        return Vec::new();
    }
    let mut out = Vec::new();
    out.extend(unnamed_equations(model));
    out.extend(missing_long_names(model));
    out
}

pub(crate) fn is_writing_code(code: &str) -> bool {
    matches!(code, "I208" | "I209")
}

/// Facts about whether the parsed model is complete enough to count.
///
/// A missing or cyclic include that the workspace has already removed from the
/// text is not one of these facts. That check stays on the include records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ModelStructure {
    pub parse_issues: bool,
    pub includes: bool,
    pub macro_type_errors: bool,
    pub macro_incomplete: bool,
    pub e062: bool,
    pub e063: bool,
    pub e064: bool,
}

pub(crate) fn model_structure(model: &Model) -> ModelStructure {
    ModelStructure {
        parse_issues: !model.parse_issues.is_empty(),
        includes: !model.includes.is_empty(),
        macro_type_errors: !model.macro_type_errors.is_empty(),
        macro_incomplete: model.macro_incomplete(),
        e062: !crate::check_e060::check_e062(model).is_empty(),
        e063: !crate::check_e060::check_e063(model).is_empty(),
        e064: !crate::check_e060::check_e064(model).is_empty(),
    }
}

/// Syntax problems and unfinished expansion. Callers decide what to withhold.
pub(crate) fn model_structure_incomplete(model: &Model) -> bool {
    let structure = model_structure(model);
    structure.parse_issues
        || structure.includes
        || structure.macro_type_errors
        || structure.macro_incomplete
        || structure.e062
        || structure.e063
        || structure.e064
}

/// Revision-bound ownership of one writing note. Row ids are expanded token
/// positions, so repeated written spans remain distinct executions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WritingContext {
    pub root: String,
    pub input_revision: String,
    pub statement_ids: Vec<usize>,
    pub rows: WritingRows,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "row_ids", rename_all = "snake_case")]
pub enum WritingRows {
    /// Starts of the surviving counted equations' token ranges.
    Equations(Vec<usize>),
    /// Declaration `parse_order` values, each assigned to its first statement.
    Declarations(Vec<usize>),
}

impl WritingRows {
    pub fn ids(&self) -> &[usize] {
        match self {
            Self::Equations(rows) | Self::Declarations(rows) => rows,
        }
    }

    fn append(&mut self, other: &Self) {
        match (self, other) {
            (Self::Equations(rows), Self::Equations(more))
            | (Self::Declarations(rows), Self::Declarations(more)) => rows.extend(more),
            _ => unreachable!("one code has one writing row kind"),
        }
    }
}

fn scoped_note(
    span: Span,
    code: &str,
    statement_id: Option<usize>,
    rows: WritingRows,
) -> Diagnostic {
    let mut diagnostic = note(span, code, summary_message(code, rows.ids().len()));
    diagnostic.writing = Some(WritingContext {
        root: String::new(),
        input_revision: String::new(),
        statement_ids: statement_id.into_iter().collect(),
        rows,
    });
    diagnostic
}

fn summary_message(code: &str, count: usize) -> String {
    match (code, count) {
        ("I208", 1) => "1 counted equation has no name tag.".to_string(),
        ("I208", count) => format!("{count} counted equations have no name tag."),
        ("I209", 1) => "1 symbol has no long_name.".to_string(),
        ("I209", count) => format!("{count} symbols have no long_name."),
        _ => unreachable!("only I208/I209 have per-statement summaries"),
    }
}

fn unnamed_equations(model: &Model) -> Vec<Diagnostic> {
    let active: HashSet<_> = equation_sites_full(model, true)
        .into_iter()
        .filter(|equation| !has_name(equation))
        .map(|equation| equation.parse_order)
        .collect();
    let mut scopes: BTreeMap<usize, (Span, Vec<usize>)> = BTreeMap::new();
    for written in &model.written_equations {
        if active.contains(&written.token_range.start) {
            scopes
                .entry(written.statement_id)
                .or_insert_with(|| (written.equation.span, Vec::new()))
                .1
                .push(written.token_range.start);
        }
    }
    scopes
        .into_iter()
        .map(|(statement, (span, rows))| {
            scoped_note(span, "I208", Some(statement), WritingRows::Equations(rows))
        })
        .collect()
}

fn missing_long_names(model: &Model) -> Vec<Diagnostic> {
    let mut rows = Vec::new();
    push_decls(&mut rows, &model.endogenous, DeclKind::Var);
    push_decls(
        &mut rows,
        &model.deterministic_exogenous,
        DeclKind::VarexoDet,
    );
    for decl in &model.exogenous {
        if model
            .deterministic_exogenous
            .iter()
            .any(|det| det.name == decl.name && det.span == decl.span)
        {
            continue;
        }
        rows.push(decl_row(decl, DeclKind::Varexo));
    }
    push_decls(&mut rows, &model.parameters, DeclKind::Parameters);
    rows.sort_by_key(|row| row.parse_order);

    let mut seen = Vec::new();
    let mut scopes: BTreeMap<Option<usize>, (Span, Vec<usize>)> = BTreeMap::new();
    for row in rows {
        if seen.iter().any(|key| key == &row.key) {
            continue;
        }
        seen.push(row.key);
        if !row.long_name.as_ref().is_some_and(|text| !text.is_empty()) {
            let statement_id = model
                .written_declarations
                .iter()
                .find(|written| written.declaration.parse_order == row.parse_order)
                .filter(|written| {
                    matches!(
                        written.written_kind.as_str(),
                        "var" | "varexo" | "varexo_det" | "parameters"
                    )
                })
                .map(|written| written.statement_id);
            // Basic declarations and their change_type rehomes retain this
            // record. A Model with missing ownership metadata keeps its prior
            // range and count rather than inventing a declaration keyword.
            scopes
                .entry(statement_id)
                .or_insert_with(|| (row.span, Vec::new()))
                .1
                .push(row.parse_order);
        }
    }
    scopes
        .into_iter()
        .map(|(statement, (span, rows))| {
            scoped_note(span, "I209", statement, WritingRows::Declarations(rows))
        })
        .collect()
}

/// Group repeated executions of the same written opener after analysis.
/// The mapping returns the written identity and whether the exact keyword is
/// safe to display. An uncertain token keeps the original first-row range.
pub(crate) fn group_summaries(
    model: &Model,
    diagnostics: &mut Vec<Diagnostic>,
    mut keyword_site: impl FnMut(Span, &str) -> Option<(String, Span, bool)>,
) {
    let mut groups = HashMap::<(String, String, Span), usize>::new();
    let mut grouped: Vec<Diagnostic> = Vec::new();
    for mut diagnostic in std::mem::take(diagnostics) {
        let site = diagnostic
            .writing
            .as_ref()
            .and_then(|context| context.statement_ids.first())
            .and_then(|id| model.statements.get(*id))
            .and_then(|statement| {
                let (file, span, safe) = keyword_site(statement.keyword_span, &statement.name)?;
                if safe {
                    diagnostic.span = statement.keyword_span;
                }
                Some((diagnostic.code.clone(), file, span))
            });
        if let Some(site) = site {
            if let Some(&index) = groups.get(&site) {
                let context = diagnostic.writing.as_ref().expect("writing scope");
                let prior = &mut grouped[index];
                let merged = prior.writing.as_mut().expect("writing scope");
                merged.statement_ids.extend(&context.statement_ids);
                merged.rows.append(&context.rows);
                prior.message = summary_message(&prior.code, merged.rows.ids().len());
                continue;
            }
            groups.insert(site, grouped.len());
        }
        grouped.push(diagnostic);
    }
    *diagnostics = grouped;
}

/// Actions must match a complete current note, including its exact row set.
pub(crate) fn context_is_current(
    workspace: &mut crate::workspace::Workspace,
    root: &str,
    code: &str,
    context: &WritingContext,
) -> bool {
    if context.statement_ids.is_empty()
        || context.rows.ids().is_empty()
        || context.root != crate::include_resolver::normalize_uri(root)
        || workspace.input_revision(root).as_deref() != Some(&context.input_revision)
    {
        return false;
    }
    crate::diagnostic::check_in_workspace_with_origins(workspace, root)
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == code && diagnostic.writing.as_ref() == Some(context))
}

fn equation_sites_full(model: &Model, counted_only: bool) -> Vec<&Equation> {
    let mut rows: Vec<&Equation> = model
        .equations
        .iter()
        .filter(|eq| equation_in_scope(eq, counted_only))
        .collect();
    for block in &model.heterogeneous_models {
        rows.extend(
            block
                .equations
                .iter()
                .filter(|eq| equation_in_scope(eq, counted_only)),
        );
    }
    rows.sort_by_key(|eq| (eq.span.start, eq.span.end));
    rows
}

fn equation_in_scope(eq: &Equation, counted_only: bool) -> bool {
    if eq.is_local {
        return false;
    }
    if counted_only && eq.static_tag {
        return false;
    }
    true
}

fn has_name(eq: &Equation) -> bool {
    eq.tag_map
        .get("name")
        .is_some_and(|value| !value.is_empty())
        || !eq.name.is_empty()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeclKind {
    Var,
    Varexo,
    VarexoDet,
    Parameters,
}

struct DeclRow {
    parse_order: usize,
    key: (Name, Option<Name>, DeclKind),
    span: Span,
    long_name: Option<String>,
}

fn push_decls(out: &mut Vec<DeclRow>, decls: &[Decl], kind: DeclKind) {
    for decl in decls {
        out.push(decl_row(decl, kind));
    }
}

fn decl_row(decl: &Decl, kind: DeclKind) -> DeclRow {
    DeclRow {
        parse_order: decl.parse_order,
        key: (decl.name, decl.heterogeneity.map(|(name, _)| name), kind),
        span: decl.span,
        long_name: decl.long_name.clone(),
    }
}

fn note(span: Span, code: &str, message: String) -> Diagnostic {
    Diagnostic::new(span, Severity::Information, code, message)
}

#[cfg(test)]
mod tests {
    use super::model_structure_incomplete;
    use crate::parser::parse;

    #[test]
    fn a_resolved_file_is_complete() {
        let src = "var y;\nmodel;\ny = 0;\nend;\n";
        assert!(!model_structure_incomplete(&parse(src)));
    }

    #[test]
    fn syntax_and_unfinished_expansion_are_incomplete() {
        let cases = [
            "parameters betta;\nbetta = 0.99\nmodel;\nend;\n",
            "@#include \"missing.inc\"\nvar y;\nmodel;\ny = 0;\nend;\n",
            "var y;\nmodel;\ny = @{UNDEF};\nend;\n",
            "@#if 1\nvar y;\nmodel;\ny = 0;\nend;\n",
            "@#error \"stop\"\nvar y;\nmodel;\ny = 0;\nend;\n",
        ];
        for src in cases {
            assert!(model_structure_incomplete(&parse(src)), "{src}");
        }
    }

    #[test]
    fn both_writing_contexts_validate_exact_rows_root_and_revision() {
        let root = "C:/writing-context/current.mod";
        let source = "var x y; model; x=x(-1); end; model; y=x; end;";
        let mut workspace = crate::workspace::Workspace::new();
        workspace.update_document(root, source);
        let set = crate::diagnostic::check_in_workspace_with_origins(&mut workspace, root);
        for code in ["I208", "I209"] {
            let context = set
                .diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code == code)
                .unwrap()
                .writing
                .as_ref()
                .unwrap()
                .clone();
            assert!(super::context_is_current(
                &mut workspace,
                root,
                code,
                &context
            ));
            let mut forged = context.clone();
            forged.statement_ids.push(999);
            assert!(!super::context_is_current(
                &mut workspace,
                root,
                code,
                &forged
            ));
            let mut forged = context.clone();
            forged.statement_ids.clear();
            assert!(!super::context_is_current(
                &mut workspace,
                root,
                code,
                &forged
            ));
            let mut forged = context.clone();
            match &mut forged.rows {
                super::WritingRows::Equations(rows) | super::WritingRows::Declarations(rows) => {
                    rows.push(999)
                }
            }
            assert!(!super::context_is_current(
                &mut workspace,
                root,
                code,
                &forged
            ));
            let mut forged = context.clone();
            forged.root = "C:/writing-context/other.mod".to_string();
            assert!(!super::context_is_current(
                &mut workspace,
                root,
                code,
                &forged
            ));
            let mut forged = context.clone();
            forged.input_revision = "stale".to_string();
            assert!(!super::context_is_current(
                &mut workspace,
                root,
                code,
                &forged
            ));
            workspace.update_document(root, format!("{source}\n// changed"));
            assert!(!super::context_is_current(
                &mut workspace,
                root,
                code,
                &context
            ));
            workspace.update_document(root, source);
        }
    }
}
