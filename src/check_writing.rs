//! Writing-preference summaries I208, I209, and I210.
//!
//! One note per code for the effective compilation unit. Not a Dynare refusal.

use crate::expr::ExprKind;
use crate::intern::Name;
use crate::model::{Decl, Equation, Model};
use crate::span::Span;

use crate::diagnostic::{Diagnostic, Severity};

pub(crate) fn writing_summaries(model: &Model) -> Vec<Diagnostic> {
    if model_structure_incomplete(model) {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Some(diag) = unnamed_equations(model) {
        out.push(diag);
    }
    if let Some(diag) = missing_long_names(model) {
        out.push(diag);
    }
    if let Some(diag) = literal_numbers(model) {
        out.push(diag);
    }
    out
}

pub(crate) fn is_writing_code(code: &str) -> bool {
    matches!(code, "I208" | "I209" | "I210")
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
    pub e062: bool,
    pub e063: bool,
    pub e064: bool,
}

pub(crate) fn model_structure(model: &Model) -> ModelStructure {
    ModelStructure {
        parse_issues: !model.parse_issues.is_empty(),
        includes: !model.includes.is_empty(),
        macro_type_errors: !model.macro_type_errors.is_empty(),
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
        || structure.e062
        || structure.e063
        || structure.e064
}

fn unnamed_equations(model: &Model) -> Option<Diagnostic> {
    let sites = equation_sites(model, true);
    let n = sites.len();
    let span = sites.into_iter().next()?;
    let message = if n == 1 {
        "1 counted equation has no name tag.".to_string()
    } else {
        format!("{n} counted equations have no name tag.")
    };
    Some(note(span, "I208", message))
}

fn missing_long_names(model: &Model) -> Option<Diagnostic> {
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
    rows.sort_by_key(|row| (row.span.start, row.span.end));

    let mut seen = Vec::new();
    let mut missing: Vec<Span> = Vec::new();
    for row in rows {
        if seen.iter().any(|key| key == &row.key) {
            continue;
        }
        seen.push(row.key);
        if !row.long_name.as_ref().is_some_and(|text| !text.is_empty()) {
            missing.push(row.span);
        }
    }
    let n = missing.len();
    let span = missing.into_iter().next()?;
    let message = if n == 1 {
        "1 symbol has no long_name.".to_string()
    } else {
        format!("{n} symbols have no long_name.")
    };
    Some(note(span, "I209", message))
}

fn literal_numbers(model: &Model) -> Option<Diagnostic> {
    let mut sites = Vec::new();
    for eq in equation_sites_full(model, false) {
        if let Some(id) = eq.lhs_expr {
            collect_numbers(model, id, &mut sites);
        }
        if let Some(id) = eq.rhs_expr {
            collect_numbers(model, id, &mut sites);
        }
    }
    let n = sites.len();
    let span = sites.into_iter().next()?;
    let message = if n == 1 {
        "1 number is written directly in equations. Consider named parameters.".to_string()
    } else {
        format!("{n} numbers are written directly in equations. Consider named parameters.")
    };
    Some(note(span, "I210", message))
}

fn collect_numbers(model: &Model, id: crate::expr::ExprId, out: &mut Vec<Span>) {
    let expr = model.exprs.get(id);
    match &expr.kind {
        ExprKind::Number => {
            let quiet = expr
                .interned
                .is_some_and(|value| value.abs() == 0.0 || value.abs() == 1.0);
            if !quiet {
                out.push(expr.span);
            }
        }
        ExprKind::String | ExprKind::Error | ExprKind::Ident { .. } => {}
        ExprKind::Unary { arg, .. }
        | ExprKind::SteadyState { arg }
        | ExprKind::Expectation { arg, .. } => collect_numbers(model, *arg, out),
        ExprKind::Binary { lhs, rhs, .. } => {
            collect_numbers(model, *lhs, out);
            collect_numbers(model, *rhs, out);
        }
        ExprKind::Call { args, .. } => {
            for arg in args {
                collect_numbers(model, *arg, out);
            }
        }
    }
}

fn equation_sites(model: &Model, counted_only: bool) -> Vec<Span> {
    equation_sites_full(model, counted_only)
        .into_iter()
        .filter(|eq| !has_name(eq))
        .map(|eq| eq.span)
        .collect()
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
}
