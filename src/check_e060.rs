//! E061–E065 / W061 / W062 include and macro-file-text diagnostics.
//!
//! Walks recorded include records, `MacroDir` / `MacroInterp` lists, and
//! `ExprKind::SteadyState` — not a regex port of `diagnostics.py`.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::diagnostic::{Diagnostic, Severity};
use crate::expr::{ExprId, ExprKind};
use crate::include_resolver::normalize_uri;
use crate::intern::Name;
use crate::model::{MacroDirective, Model};
use crate::span::Span;
use crate::workspace::{IncludeRecords, Workspace};

const CONDITIONAL_OPENERS: &[&str] = &["if", "ifdef", "ifndef"];

pub fn check_e060(records: &IncludeRecords) -> Vec<Diagnostic> {
    records
        .cycles
        .iter()
        .map(|cycle| {
            let chain = cycle
                .chain
                .iter()
                .map(|p| file_basename(p))
                .collect::<Vec<_>>()
                .join(" -> ");
            let diagnostic = Diagnostic::new(
                cycle.span,
                Severity::Warning,
                "W062",
                format!("Circular @#include detected: {chain}."),
            )
            .with_related(crate::diagnostic::RelatedDiagnostic::written(
                cycle.earlier.file.clone(),
                cycle.earlier.span,
                "Earlier include in this cycle",
            ));
            if cycle.closing != cycle.earlier {
                diagnostic.with_related(crate::diagnostic::RelatedDiagnostic::written(
                    cycle.closing.file.clone(),
                    cycle.closing.span,
                    "Include that closes this cycle",
                ))
            } else {
                diagnostic
            }
        })
        .collect()
}

pub fn check_e061(records: &IncludeRecords) -> Vec<Diagnostic> {
    records
        .unresolved
        .iter()
        .map(|u| {
            Diagnostic::new(u.span, Severity::Error, "E061", {
                let mut msg = format!(
                    "Could not open {}. The following directories were searched",
                    u.filename
                );
                if !u.searched.is_empty() {
                    msg.push(':');
                    for dir in &u.searched {
                        msg.push_str("\n   * ");
                        msg.push_str(dir);
                    }
                }
                msg
            })
        })
        .collect()
}

pub fn check_e062(model: &Model) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut stack: Vec<&MacroDirective> = Vec::new();
    let mut seen_else: Vec<bool> = Vec::new();
    let mut had_mismatch = false;

    for directive in &model.macro_directives {
        let kind = directive.kind.as_str();
        if CONDITIONAL_OPENERS.contains(&kind) || kind == "for" {
            stack.push(directive);
            seen_else.push(false);
        } else if kind == "else" || kind == "elseif" {
            if stack.is_empty() {
                emit_stray(&mut diagnostics, directive, "@#if");
            } else if !CONDITIONAL_OPENERS.contains(&stack.last().unwrap().kind.as_str()) {
                had_mismatch = true;
                emit_mismatch(
                    &mut diagnostics,
                    directive,
                    stack.last().unwrap().kind.as_str(),
                );
            } else if kind == "else" {
                if *seen_else.last().unwrap() {
                    emit_invalid_branch(
                        &mut diagnostics,
                        directive,
                        "Duplicate @#else in @#if block",
                    );
                } else if let Some(flag) = seen_else.last_mut() {
                    *flag = true;
                }
            } else if *seen_else.last().unwrap() {
                emit_invalid_branch(
                    &mut diagnostics,
                    directive,
                    "@#elseif after @#else in @#if block",
                );
            }
        } else if kind == "endif" {
            if stack.is_empty() {
                emit_stray(&mut diagnostics, directive, "@#if");
            } else if CONDITIONAL_OPENERS.contains(&stack.last().unwrap().kind.as_str()) {
                stack.pop();
                seen_else.pop();
            } else {
                had_mismatch = true;
                emit_mismatch(
                    &mut diagnostics,
                    directive,
                    stack.last().unwrap().kind.as_str(),
                );
            }
        } else if kind == "endfor" {
            if stack.is_empty() {
                emit_stray(&mut diagnostics, directive, "@#for");
            } else if stack.last().unwrap().kind == "for" {
                if crate::macro_expand::for_body_is_empty(
                    &model.source,
                    stack.last().unwrap().span,
                    directive.span,
                ) {
                    let written =
                        &model.source[directive.span.start as usize..directive.span.end as usize];
                    let keyword_end = written
                        .to_ascii_lowercase()
                        .find("endfor")
                        .map(|start| start + 6)
                        .unwrap_or(written.len());
                    diagnostics.push(Diagnostic::new(
                        Span {
                            start: directive.span.start,
                            end: directive.span.start + keyword_end as u32,
                        },
                        Severity::Error,
                        "E062",
                        "syntax error, unexpected ENDFOR",
                    ));
                }
                stack.pop();
                seen_else.pop();
            } else {
                had_mismatch = true;
                emit_mismatch(
                    &mut diagnostics,
                    directive,
                    stack.last().unwrap().kind.as_str(),
                );
            }
        }
    }

    if had_mismatch {
        return diagnostics;
    }

    for opener in stack {
        let expected = closer_for(&opener.kind);
        diagnostics.push(Diagnostic::new(
            opener.span,
            Severity::Error,
            "E062",
            format!(
                "Unterminated {} block -- no matching {expected} before end of file. Fix: add {expected} to close this block.",
                label(&opener.kind)
            ),
        ));
    }
    diagnostics
}

/// Unknown names are recorded by the shared evaluator at their executed site.
pub fn check_e063(model: &Model) -> Vec<Diagnostic> {
    model
        .macro_type_errors
        .iter()
        .filter(|(_, code, _)| *code == "E063")
        .map(|(span, code, message)| {
            Diagnostic::new(*span, Severity::Error, *code, message.clone())
        })
        .collect()
}

pub fn check_e064(model: &Model) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut stack = IfStack::default();
    for directive in &model.macro_directives {
        let emitting = stack.emitting();
        if directive.kind == "error" && emitting {
            let raw = directive.argument.as_deref().unwrap_or("");
            let message = strip_error_arg(raw);
            let text = if message.is_empty() {
                "Macro-processing error".to_string()
            } else {
                format!("Macro-processing error: {message}")
            };
            diagnostics.push(Diagnostic::new(
                directive.span,
                Severity::Error,
                "E064",
                text,
            ));
        }
        stack.apply(directive);
    }
    diagnostics
}

pub fn check_e065(model: &Model) -> Vec<Diagnostic> {
    let exo: HashSet<Name> = model.exogenous.iter().map(|d| d.name).collect();
    if exo.is_empty() {
        return Vec::new();
    }
    let mut diagnostics = Vec::new();
    for eq in &model.equations {
        let mut operands = Vec::new();
        if let Some(id) = eq.lhs_expr {
            collect_steady_state_operands(&model.exprs, id, &mut operands);
        }
        if let Some(id) = eq.rhs_expr {
            collect_steady_state_operands(&model.exprs, id, &mut operands);
        }
        for arg in operands {
            let mut names: Vec<String> = model
                .exprs
                .walk_idents(arg)
                .filter(|r| exo.contains(&r.name))
                .map(|r| model.name(r.name).to_string())
                .collect();
            names.sort();
            names.dedup();
            if names.is_empty() {
                continue;
            }
            let joined = names.join(", ");
            diagnostics.push(Diagnostic::new(
                eq.span,
                Severity::Error,
                "E065",
                format!(
                    "Exogenous variables are not allowed in the context of the STEADY_STATE() operator: {joined}."
                ),
            ));
        }
    }
    diagnostics
}

pub fn check_w061(ws: &mut Workspace, uri: &str) -> Vec<Diagnostic> {
    let key = normalize_uri(uri);
    let docs = ws.document_uris();
    let mut parents = Vec::new();
    for parent in &docs {
        if parent == &key {
            continue;
        }
        if ws.resolve_all_includes(parent).contains_key(&key) {
            parents.push(parent.clone());
        }
    }
    if parents.len() <= 1 {
        return Vec::new();
    }
    let mut outermost = Vec::new();
    for parent in &parents {
        let nested_in_other = parents
            .iter()
            .any(|other| other != parent && ws.resolve_all_includes(other).contains_key(parent));
        if !nested_in_other {
            outermost.push(parent.clone());
        }
    }
    if outermost.len() == 1 {
        return Vec::new();
    }
    let span = {
        let source = ws.get_source(uri).unwrap_or("");
        first_scalar_span(source)
    };
    let mut names: Vec<String> = parents.iter().map(|p| file_basename(p)).collect();
    names.sort();
    let joined = names.join(", ");
    vec![Diagnostic::new(
        span,
        Severity::Warning,
        "W061",
        format!(
            "This include is reachable from multiple parent files ({joined}); open or run the intended parent model to get include-scoped diagnostics in the right context."
        ),
    )]
}

pub fn check_e060_family(ws: &mut Workspace, uri: &str) -> Vec<Diagnostic> {
    let records = ws.include_records(uri).cloned().unwrap_or_default();
    let mut out = check_e060(&records);
    out.extend(check_e061(&records));
    let model_diags = ws
        .get_model(uri)
        .map(check_e060_family_on_model)
        .unwrap_or_default();
    out.extend(model_diags);
    out.extend(check_w061(ws, uri));
    out
}

pub fn check_e060_family_on_model(model: &Model) -> Vec<Diagnostic> {
    let syntax = check_e062(model);
    if !syntax.is_empty() {
        return syntax;
    }
    if !model.macro_type_errors.is_empty() {
        return model
            .macro_type_errors
            .iter()
            .map(|(span, code, message)| {
                Diagnostic::new(*span, Severity::Error, *code, message.clone())
            })
            .collect();
    }
    let mut out = syntax;
    out.extend(check_e063(model));
    out.extend(check_e064(model));
    out.extend(check_e065(model));
    out.extend(check_e381(model));
    out
}

/// E381: an `external_function` call inside a `steady_state(…)` operator.
///
/// The external-function analog of **E065**: the same operand set, the same
/// per-equation span. Their needle prints only in the MATLAB-output path
/// (`ExprNode.cc:8095`, `ExternalFunctionNode::writeOutput`); a `json=compute`
/// run aborts with no message, so the honesty row uses the write stage.
pub fn check_e381(model: &Model) -> Vec<Diagnostic> {
    if model.external_function_names.is_empty() {
        return Vec::new();
    }
    let external: HashSet<Name> = model.external_function_names.iter().copied().collect();
    let mut diagnostics = Vec::new();
    for eq in &model.equations {
        let mut operands = Vec::new();
        if let Some(id) = eq.lhs_expr {
            collect_steady_state_operands(&model.exprs, id, &mut operands);
        }
        if let Some(id) = eq.rhs_expr {
            collect_steady_state_operands(&model.exprs, id, &mut operands);
        }
        let mut hit = false;
        for arg in operands {
            walk_call_callees(model, arg, &mut |callee| {
                if external.contains(&callee) {
                    hit = true;
                }
            });
            if hit {
                break;
            }
        }
        if hit {
            diagnostics.push(Diagnostic::new(
                eq.span,
                Severity::Error,
                "E381",
                "The expression inside a steady_state operator cannot contain external functions",
            ));
        }
    }
    diagnostics
}

/// Visits every `Call` node's callee identifier in one expression tree.
/// `walk_idents` skips callees, so the external-function needle needs this walk.
fn walk_call_callees(model: &Model, id: ExprId, f: &mut impl FnMut(Name)) {
    let expr = model.exprs.get(id);
    match &expr.kind {
        ExprKind::Call { callee, args } => {
            f(*callee);
            for arg in args {
                walk_call_callees(model, *arg, f);
            }
        }
        ExprKind::Unary { arg, .. } => walk_call_callees(model, *arg, f),
        ExprKind::Binary { lhs, rhs, .. } => {
            walk_call_callees(model, *lhs, f);
            walk_call_callees(model, *rhs, f);
        }
        ExprKind::SteadyState { arg } | ExprKind::Expectation { arg, .. } => {
            walk_call_callees(model, *arg, f);
        }
        ExprKind::Ident { .. } | ExprKind::Number | ExprKind::String | ExprKind::Error => {}
    }
}

fn label(kind: &str) -> String {
    format!("@#{kind}")
}

fn closer_for(kind: &str) -> &'static str {
    if CONDITIONAL_OPENERS.contains(&kind) {
        "@#endif"
    } else {
        "@#endfor"
    }
}

fn emit_stray(out: &mut Vec<Diagnostic>, directive: &MacroDirective, opener: &str) {
    out.push(Diagnostic::new(
        directive.span,
        Severity::Error,
        "E062",
        format!(
            "Stray {} with no matching {opener}. Fix: remove this directive or add a matching {opener} above.",
            label(&directive.kind)
        ),
    ));
}

fn emit_mismatch(out: &mut Vec<Diagnostic>, directive: &MacroDirective, open_kind: &str) {
    let expected = closer_for(open_kind);
    let found = label(&directive.kind);
    let open = label(open_kind);
    out.push(Diagnostic::new(
        directive.span,
        Severity::Error,
        "E062",
        format!(
            "Mismatched {found} while {open} block is still open. Fix: close the {open} block with {expected} before {found}."
        ),
    ));
}

fn emit_invalid_branch(out: &mut Vec<Diagnostic>, directive: &MacroDirective, message: &str) {
    out.push(Diagnostic::new(
        directive.span,
        Severity::Error,
        "E062",
        format!("{message}. Fix: remove or reorder this macro branch."),
    ));
}

fn file_basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

fn first_scalar_span(source: &str) -> Span {
    match source.chars().next() {
        None => Span { start: 0, end: 0 },
        Some(ch) => Span {
            start: 0,
            end: ch.len_utf8() as u32,
        },
    }
}

fn leading_ident(s: &str) -> Option<&str> {
    let mut chars = s.char_indices();
    let (_, first) = chars.next()?;
    if !first.is_ascii_alphabetic() && first != '_' {
        return None;
    }
    let mut end = first.len_utf8();
    for (i, c) in chars {
        if c.is_ascii_alphanumeric() || c == '_' {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    Some(&s[..end])
}

fn is_false_literal(arg: &str) -> bool {
    let t = arg.trim();
    t.is_empty() || matches!(t.to_ascii_lowercase().as_str(), "0" | "false" | "no")
}

fn if_truth(argument: Option<&str>, defines: &HashMap<String, bool>) -> bool {
    let raw = argument.unwrap_or("").trim();
    if is_false_literal(raw) {
        return false;
    }
    if let Some(ident) = leading_ident(raw) {
        if raw[ident.len()..].trim().is_empty() {
            return defines.get(ident).copied().unwrap_or(false);
        }
    }
    if let Ok(n) = raw.parse::<i64>() {
        return n != 0;
    }
    false
}

fn define_name(arg: &str) -> Option<String> {
    leading_ident(arg.trim()).map(str::to_string)
}

fn define_value_truthy(arg: &str) -> bool {
    let s = arg.trim();
    let Some(name) = leading_ident(s) else {
        return true;
    };
    let rest = s[name.len()..].trim();
    let val = rest.strip_prefix('=').map(str::trim).unwrap_or("");
    !is_false_literal(val)
}

fn strip_error_arg(arg: &str) -> String {
    arg.trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .to_string()
}

#[derive(Default)]
struct IfStack {
    frames: Vec<bool>,
    defines: HashMap<String, bool>,
}

impl IfStack {
    fn emitting(&self) -> bool {
        self.frames.iter().all(|a| *a)
    }

    fn apply(&mut self, d: &MacroDirective) {
        match d.kind.as_str() {
            "if" => {
                let cond = if_truth(d.argument.as_deref(), &self.defines);
                self.frames.push(cond);
            }
            "ifdef" => {
                let name = d.argument.as_deref().unwrap_or("").trim();
                self.frames.push(self.defines.contains_key(name));
            }
            "ifndef" => {
                let name = d.argument.as_deref().unwrap_or("").trim();
                self.frames.push(!self.defines.contains_key(name));
            }
            "else" => {
                if let Some(a) = self.frames.last_mut() {
                    *a = !*a;
                }
            }
            "elseif" => {
                if let Some(a) = self.frames.last_mut() {
                    if *a {
                        *a = false;
                    } else {
                        *a = if_truth(d.argument.as_deref(), &self.defines);
                    }
                }
            }
            "endif" => {
                self.frames.pop();
            }
            "define" if self.emitting() => {
                if let Some(name) = define_name(d.argument.as_deref().unwrap_or("")) {
                    let truth = define_value_truthy(d.argument.as_deref().unwrap_or(""));
                    self.defines.insert(name, truth);
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn collect_steady_state_operands(
    arena: &crate::expr::ExprArena,
    id: ExprId,
    out: &mut Vec<ExprId>,
) {
    match &arena.get(id).kind {
        ExprKind::Ident { .. } | ExprKind::Number | ExprKind::String | ExprKind::Error => {}
        ExprKind::Unary { arg, .. } => collect_steady_state_operands(arena, *arg, out),
        ExprKind::Binary { lhs, rhs, .. } => {
            collect_steady_state_operands(arena, *lhs, out);
            collect_steady_state_operands(arena, *rhs, out);
        }
        ExprKind::Call { args, .. } => {
            for arg in args {
                collect_steady_state_operands(arena, *arg, out);
            }
        }
        ExprKind::SteadyState { arg } => {
            out.push(*arg);
            collect_steady_state_operands(arena, *arg, out);
        }
        ExprKind::Expectation { arg, .. } => {
            collect_steady_state_operands(arena, *arg, out);
        }
    }
}
