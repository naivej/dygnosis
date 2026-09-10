//! E060–E065 / W061 include and macro-file-text diagnostics.
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

const MACRO_BUILTIN_NAMES: &[&str] = &[
    "true",
    "false",
    "inf",
    "nan",
    "length",
    "isempty",
    "isboolean",
    "isreal",
    "isstring",
    "isarray",
    "istuple",
    "isdefined",
    "defined",
    "exp",
    "log",
    "ln",
    "log10",
    "sin",
    "cos",
    "tan",
    "asin",
    "acos",
    "atan",
    "sqrt",
    "cbrt",
    "sign",
    "floor",
    "ceil",
    "trunc",
    "round",
    "mod",
    "max",
    "min",
    "sum",
    "erf",
    "erfc",
    "gamma",
    "lgamma",
    "abs",
    "normpdf",
    "normcdf",
];

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
            Diagnostic::new(
                cycle.span,
                Severity::Error,
                "E060",
                format!(
                    "Circular @#include detected: {chain}. Fix: break the cycle by removing one of the @#include directives along this chain."
                ),
            )
        })
        .collect()
}

pub fn check_e061(records: &IncludeRecords) -> Vec<Diagnostic> {
    records
        .unresolved
        .iter()
        .map(|u| {
            let target = match &u.included_from {
                Some(parent) => format!("{} (included from {parent})", u.filename),
                None => u.filename.clone(),
            };
            Diagnostic::new(
                u.span,
                Severity::Error,
                "E061",
                format!(
                    "Cannot resolve @#include target '{target}'. Searched the directory of the including file and the workspace search paths. Fix: correct the path, add the missing file, or add its containing directory to the language server's search paths."
                ),
            )
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

pub fn check_e063(model: &Model) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let has_includes = !model.includes.is_empty();
    for interp in &model.macro_interps {
        let line = line_text(&model.source, interp.span.start);
        if line.trim_start().starts_with("@#") {
            continue;
        }
        let before_line = line_of(&model.source, interp.span.start);
        let known = known_names_before(model, before_line);
        let expr = interp.inner.trim();
        if expr.is_empty() {
            continue;
        }
        let unknown = unknown_names(expr, &known);
        if unknown.is_empty() {
            continue;
        }
        let message = if is_simple_ident(expr) {
            format!(
                "Undefined macro interpolation '@{{{expr}}}'. Fix: define '{expr}' with @#define before this line, or remove the macro interpolation."
            )
        } else {
            if has_includes {
                continue;
            }
            let unknown_joined = unknown.join(", ");
            format!(
                "Undefined macro name(s) in interpolation '@{{{expr}}}': {unknown_joined}. Fix: define them with @#define before this line, or remove the macro interpolation."
            )
        };
        diagnostics.push(Diagnostic::new(
            interp.span,
            Severity::Error,
            "E063",
            message,
        ));
    }
    diagnostics
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
                "Macro @#error triggered.".to_string()
            } else {
                format!("Macro @#error triggered: {message}.")
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
                    "Invalid steady_state() operand: exogenous variable(s) {joined} cannot be used inside steady_state()."
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
    let mut out = check_e062(model);
    out.extend(check_e063(model));
    out.extend(check_e064(model));
    out.extend(check_e065(model));
    out
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

fn line_of(source: &str, byte: u32) -> usize {
    let end = (byte as usize).min(source.len());
    source[..end].bytes().filter(|&b| b == b'\n').count()
}

fn line_text(source: &str, byte: u32) -> &str {
    let b = (byte as usize).min(source.len());
    let start = source[..b].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = source[b..]
        .find('\n')
        .map(|i| b + i)
        .unwrap_or(source.len());
    &source[start..end]
}

fn is_simple_ident(s: &str) -> bool {
    leading_ident(s).is_some_and(|n| n == s)
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

fn for_vars(arg: &str) -> Vec<String> {
    let mut s = arg.trim();
    if let Some(rest) = s.strip_prefix('(') {
        s = rest.trim_start();
    }
    let mut vars = Vec::new();
    loop {
        s = s.trim_start();
        if s.is_empty() {
            break;
        }
        if s.len() >= 2 && s[..2].eq_ignore_ascii_case("in") {
            let after = &s[2..];
            if after
                .chars()
                .next()
                .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_')
            {
                break;
            }
        }
        let Some(ident) = leading_ident(s) else {
            break;
        };
        vars.push(ident.to_string());
        s = s[ident.len()..].trim_start();
        if let Some(rest) = s.strip_prefix(',') {
            s = rest;
            continue;
        }
        break;
    }
    vars
}

fn strip_error_arg(arg: &str) -> String {
    arg.trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .to_string()
}

fn is_builtin(name: &str) -> bool {
    MACRO_BUILTIN_NAMES
        .iter()
        .any(|b| b.eq_ignore_ascii_case(name))
}

fn unknown_names(expr: &str, known: &HashSet<String>) -> Vec<String> {
    let (scannable, guarded) = strip_defined_calls(expr);
    let scannable = mask_strings(&scannable);
    let mut idents = Vec::new();
    let mut i = 0usize;
    while i < scannable.len() {
        let rest = &scannable[i..];
        if let Some(ident) = leading_ident(rest) {
            if !is_builtin(ident) && !idents.iter().any(|x| x == ident) {
                idents.push(ident.to_string());
            }
            i += ident.len();
        } else {
            let ch = rest.chars().next().unwrap();
            i += ch.len_utf8();
        }
    }
    idents
        .into_iter()
        .filter(|n| !known.contains(n) && !guarded.contains(n))
        .collect()
}

fn mask_strings(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '"' || c == '\'' {
            out.push(' ');
            for d in chars.by_ref() {
                out.push(' ');
                if d == c {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn strip_defined_calls(expr: &str) -> (String, HashSet<String>) {
    let lower = expr.to_ascii_lowercase();
    let bytes = expr.as_bytes();
    let mut out = bytes.to_vec();
    let mut guarded = HashSet::new();
    let mut i = 0usize;
    while i < lower.len() {
        let rest = &lower[i..];
        let kw = if rest.starts_with("isdefined") {
            Some("isdefined")
        } else if rest.starts_with("defined") {
            Some("defined")
        } else {
            None
        };
        let Some(kw) = kw else {
            i += 1;
            continue;
        };
        if i > 0 {
            let prev = bytes[i - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                i += 1;
                continue;
            }
        }
        let after_kw = i + kw.len();
        let after = expr.get(after_kw..).unwrap_or("");
        let trimmed = after.trim_start();
        let skip = after.len() - trimmed.len();
        if !trimmed.starts_with('(') {
            i += 1;
            continue;
        }
        let inner = trimmed[1..].trim_start();
        let Some(ident) = leading_ident(inner) else {
            i += 1;
            continue;
        };
        let after_ident = inner[ident.len()..].trim_start();
        if !after_ident.starts_with(')') {
            i += 1;
            continue;
        }
        let close_rel = expr[after_kw..].len() - after_ident.len() + 1;
        let end = after_kw + close_rel;
        guarded.insert(ident.to_string());
        for b in out.iter_mut().take(end).skip(i) {
            *b = b' ';
        }
        let _ = skip;
        i = end;
    }
    let scannable = String::from_utf8(out).unwrap_or_else(|_| expr.to_string());
    (scannable, guarded)
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

fn known_names_before(model: &Model, before_line: usize) -> HashSet<String> {
    let mut stack = IfStack::default();
    let mut known = HashSet::new();
    let mut for_stack: Vec<Vec<String>> = Vec::new();
    for d in &model.macro_directives {
        let line = line_of(&model.source, d.span.start);
        let emitting = stack.emitting();
        if line < before_line {
            match d.kind.as_str() {
                "define" if emitting => {
                    if let Some(name) = define_name(d.argument.as_deref().unwrap_or("")) {
                        known.insert(name);
                    }
                }
                "for" => {
                    let vars = for_vars(d.argument.as_deref().unwrap_or(""));
                    if emitting {
                        for v in &vars {
                            known.insert(v.clone());
                        }
                    }
                    for_stack.push(vars);
                }
                "endfor" => {
                    if let Some(vars) = for_stack.pop() {
                        for v in vars {
                            if !for_stack.iter().any(|open| open.contains(&v)) {
                                known.remove(&v);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        stack.apply(d);
    }
    known
}

fn collect_steady_state_operands(
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
