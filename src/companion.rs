//! Companion file mentions harvested from a root `.mod` (path resolve only).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::command_skip::{command_stmt_spans, is_parse_skip_command};
use crate::expr::{ExprId, ExprKind};
use crate::include_resolver::{normalize_separators, path_key};
use crate::lexer::{tokenize, Token, TokenKind};
use crate::macro_expand::expand_macros;
use crate::model::Model;
use crate::span::Span;

const RANK_OPTION: u8 = 3;
const RANK_CONVENTION: u8 = 2;
const RANK_LEFTOVER: u8 = 1;

const DATAFILE_SUFFIXES: &[&str] = &[".m", ".mat", ".csv", ".xls", ".xlsx"];
const M_SUFFIXES: &[&str] = &[".m"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompanionKind {
    SteadyStateFile,
    PriorRestrictions,
    Datafile,
    ModeFile,
    IrfMatchingFile,
    HelperM,
    RunScript,
}

impl CompanionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SteadyStateFile => "steady_state_file",
            Self::PriorRestrictions => "prior_restrictions",
            Self::Datafile => "datafile",
            Self::ModeFile => "mode_file",
            Self::IrfMatchingFile => "irf_matching_file",
            Self::HelperM => "helper_m",
            Self::RunScript => "run_script",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanionRecord {
    pub kind: CompanionKind,
    pub named_in: Span,
    pub name: String,
    pub path: Option<PathBuf>,
}

struct Pending {
    kind: CompanionKind,
    named_in: Span,
    name: String,
    path: Option<PathBuf>,
    rank: u8,
}

pub(crate) fn extra_suffixes(kind: CompanionKind) -> &'static [&'static str] {
    match kind {
        CompanionKind::Datafile => DATAFILE_SUFFIXES,
        CompanionKind::HelperM | CompanionKind::ModeFile | CompanionKind::IrfMatchingFile => {
            M_SUFFIXES
        }
        CompanionKind::SteadyStateFile
        | CompanionKind::PriorRestrictions
        | CompanionKind::RunScript => &[],
    }
}

pub(crate) fn first_line_span(src: &str) -> Span {
    Span::new(0, src.find('\n').unwrap_or(src.len()))
}

/// Command that loads the convention file, or the first line if that command is absent.
///
/// `FILENAME_steadystate.m` is used from `steady`. `FILENAME_prior_restrictions.m`
/// is called from `estimation`; 7.1 `method_of_moments` only `isfile`s it.
/// `run_FILENAME.m` is a MATLAB driver, not a `.mod` command — first line.
fn convention_named_in(
    kind: CompanionKind,
    source: &str,
    tokens: &[Token],
    cmd_spans: &[Span],
) -> Span {
    let names: &[&str] = match kind {
        CompanionKind::SteadyStateFile => &["steady"],
        CompanionKind::PriorRestrictions => &["estimation", "method_of_moments"],
        CompanionKind::RunScript
        | CompanionKind::Datafile
        | CompanionKind::ModeFile
        | CompanionKind::IrfMatchingFile
        | CompanionKind::HelperM => return first_line_span(source),
    };
    first_command_ident(source, tokens, cmd_spans, names).unwrap_or_else(|| first_line_span(source))
}

fn first_command_ident(
    source: &str,
    tokens: &[Token],
    cmd_spans: &[Span],
    names: &[&str],
) -> Option<Span> {
    for span in cmd_spans {
        let Some(tok) = tokens.iter().find(|t| {
            t.kind == TokenKind::Ident && t.span.start >= span.start && t.span.end <= span.end
        }) else {
            continue;
        };
        let ident = tok.text(source);
        if names.iter().any(|n| ident.eq_ignore_ascii_case(n)) {
            return Some(tok.span);
        }
    }
    None
}

pub(crate) fn harvest(
    source: &str,
    model: &Model,
    convention: &[(CompanionKind, String)],
    mut resolve: impl FnMut(&str, &[&str]) -> Option<PathBuf>,
) -> Vec<CompanionRecord> {
    let raw = tokenize(source);
    let tokens = expand_macros(source, raw);
    let cmd_spans = command_stmt_spans(&tokens, source);

    let mut pending = Vec::new();
    let mut option_value_spans = HashSet::new();

    for span in &cmd_spans {
        harvest_command_options(
            source,
            &tokens,
            *span,
            &mut resolve,
            &mut pending,
            &mut option_value_spans,
        );
    }

    harvest_leftover_quotes(
        source,
        &tokens,
        &option_value_spans,
        &mut resolve,
        &mut pending,
    );
    harvest_ident_helpers(
        source,
        model,
        &tokens,
        &cmd_spans,
        &mut resolve,
        &mut pending,
    );

    for (kind, name) in convention {
        if let Some(path) = resolve(name, extra_suffixes(*kind)) {
            pending.push(Pending {
                kind: *kind,
                named_in: convention_named_in(*kind, source, &tokens, &cmd_spans),
                name: name.clone(),
                path: Some(path),
                rank: RANK_CONVENTION,
            });
        }
    }

    merge(pending)
}

fn harvest_command_options(
    source: &str,
    tokens: &[Token],
    span: Span,
    resolve: &mut impl FnMut(&str, &[&str]) -> Option<PathBuf>,
    pending: &mut Vec<Pending>,
    option_value_spans: &mut HashSet<(u32, u32)>,
) {
    let inner: Vec<usize> = tokens
        .iter()
        .enumerate()
        .filter(|(_, t)| t.span.start >= span.start && t.span.end <= span.end)
        .map(|(i, _)| i)
        .collect();
    if inner.is_empty() {
        return;
    }
    let cmd_i = inner[0];
    if tokens[cmd_i].kind != TokenKind::Ident {
        return;
    }
    let command = tokens[cmd_i].text(source).to_string();
    let mut depth = 0i32;
    let mut i = 0usize;
    while i < inner.len() {
        let ti = inner[i];
        match tokens[ti].kind {
            TokenKind::LParen => {
                depth += 1;
                i += 1;
                continue;
            }
            TokenKind::RParen => {
                depth -= 1;
                i += 1;
                continue;
            }
            TokenKind::Ident if depth == 1 => {
                let next_kind = inner.get(i + 1).map(|&j| tokens[j].kind);
                if next_kind != Some(TokenKind::Eq) {
                    i += 1;
                    continue;
                }
                let option = tokens[ti].text(source);
                let Some(&val_i) = inner.get(i + 2) else {
                    i += 1;
                    continue;
                };
                let val = &tokens[val_i];
                if let Some(kind) = option_kind(&command, option) {
                    if let Some((name, named_in)) = option_value(source, val, kind) {
                        option_value_spans.insert((named_in.start, named_in.end));
                        let path = resolve(&name, extra_suffixes(kind));
                        pending.push(Pending {
                            kind,
                            named_in,
                            name,
                            path,
                            rank: RANK_OPTION,
                        });
                    }
                }
                i += 3;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
}

fn option_kind(command: &str, option: &str) -> Option<CompanionKind> {
    let cmd = command.to_ascii_lowercase();
    let opt = option.to_ascii_lowercase();
    match opt.as_str() {
        "datafile" => Some(CompanionKind::Datafile),
        "file" if cmd == "data" => Some(CompanionKind::Datafile),
        "filename" if cmd == "initval_file" || cmd == "histval_file" => {
            Some(CompanionKind::Datafile)
        }
        "mode_file" => Some(CompanionKind::ModeFile),
        "irf_matching_file" => Some(CompanionKind::IrfMatchingFile),
        "gsa_sample_file" => Some(CompanionKind::Datafile),
        "function" if cmd == "prior_function" || cmd == "posterior_function" => {
            Some(CompanionKind::HelperM)
        }
        "name" | "first_deriv_provided" | "second_deriv_provided" if cmd == "external_function" => {
            Some(CompanionKind::HelperM)
        }
        _ => None,
    }
}

fn option_value(source: &str, val: &Token, kind: CompanionKind) -> Option<(String, Span)> {
    match val.kind {
        TokenKind::String => {
            let raw = val.text(source);
            let name = strip_quotes(raw).to_string();
            if name.is_empty() {
                return None;
            }
            Some((name, val.span))
        }
        TokenKind::Ident => {
            let name = val.text(source);
            if kind == CompanionKind::Datafile && matches!(name, "0" | "1") {
                return None;
            }
            if name.is_empty() {
                return None;
            }
            Some((name.to_string(), val.span))
        }
        TokenKind::Number => {
            let name = val.text(source);
            if kind == CompanionKind::Datafile && matches!(name, "0" | "1") {
                return None;
            }
            None
        }
        _ => None,
    }
}

fn harvest_leftover_quotes(
    source: &str,
    tokens: &[Token],
    option_value_spans: &HashSet<(u32, u32)>,
    resolve: &mut impl FnMut(&str, &[&str]) -> Option<PathBuf>,
    pending: &mut Vec<Pending>,
) {
    for tok in tokens {
        if tok.kind != TokenKind::String {
            continue;
        }
        if option_value_spans.contains(&(tok.span.start, tok.span.end)) {
            continue;
        }
        let name = strip_quotes(tok.text(source)).to_string();
        let Some(kind) = leftover_kind(&name) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let path = resolve(&name, &[]);
        pending.push(Pending {
            kind,
            named_in: tok.span,
            name,
            path,
            rank: RANK_LEFTOVER,
        });
    }
}

fn leftover_kind(name: &str) -> Option<CompanionKind> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".m") {
        Some(CompanionKind::HelperM)
    } else if lower.ends_with(".mat")
        || lower.ends_with(".csv")
        || lower.ends_with(".xls")
        || lower.ends_with(".xlsx")
    {
        Some(CompanionKind::Datafile)
    } else {
        None
    }
}

fn harvest_ident_helpers(
    source: &str,
    model: &Model,
    tokens: &[Token],
    cmd_spans: &[Span],
    resolve: &mut impl FnMut(&str, &[&str]) -> Option<PathBuf>,
    pending: &mut Vec<Pending>,
) {
    let mut calls = Vec::new();
    for a in model
        .param_assignments
        .iter()
        .chain(&model.helper_assignments)
        .chain(&model.initval)
        .chain(&model.endval)
    {
        if let Some(id) = a.expr {
            collect_calls(model, id, &mut calls);
        }
    }
    for eq in &model.steady_state_equations {
        if let Some(id) = eq.lhs_expr {
            collect_calls(model, id, &mut calls);
        }
        if let Some(id) = eq.rhs_expr {
            collect_calls(model, id, &mut calls);
        }
    }
    for (name, named_in) in calls {
        if let Some(path) = resolve(&name, extra_suffixes(CompanionKind::HelperM)) {
            pending.push(Pending {
                kind: CompanionKind::HelperM,
                named_in,
                name,
                path: Some(path),
                rank: RANK_LEFTOVER,
            });
        }
    }

    for i in 0..tokens.len().saturating_sub(1) {
        if tokens[i].kind != TokenKind::Ident || tokens[i + 1].kind != TokenKind::LParen {
            continue;
        }
        let ident = &tokens[i];
        if let Some(block) = model.model_block {
            if span_contains(block, ident.span) {
                continue;
            }
        }
        if cmd_spans.iter().any(|s| span_contains(*s, ident.span)) {
            continue;
        }
        let name = ident.text(source);
        if is_parse_skip_command(name) {
            continue;
        }
        if is_timing_paren(tokens, source, i + 1) {
            continue;
        }
        if name.is_empty() {
            continue;
        }
        if let Some(path) = resolve(name, extra_suffixes(CompanionKind::HelperM)) {
            pending.push(Pending {
                kind: CompanionKind::HelperM,
                named_in: ident.span,
                name: name.to_string(),
                path: Some(path),
                rank: RANK_LEFTOVER,
            });
        }
    }
}

fn collect_calls(model: &Model, id: ExprId, out: &mut Vec<(String, Span)>) {
    let expr = model.exprs.get(id);
    match &expr.kind {
        ExprKind::Call { callee, args } => {
            let name = model.name(*callee).to_string();
            let start = expr.span.start as usize;
            out.push((name.clone(), Span::new(start, start + name.len())));
            for arg in args {
                collect_calls(model, *arg, out);
            }
        }
        ExprKind::Unary { arg, .. }
        | ExprKind::SteadyState { arg }
        | ExprKind::Expectation { arg, .. } => {
            collect_calls(model, *arg, out);
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            collect_calls(model, *lhs, out);
            collect_calls(model, *rhs, out);
        }
        ExprKind::Ident { .. } | ExprKind::Number | ExprKind::String | ExprKind::Error => {}
    }
}

fn is_timing_paren(tokens: &[Token], source: &str, lparen_i: usize) -> bool {
    let mut k = lparen_i + 1;
    if tokens
        .get(k)
        .is_some_and(|t| matches!(t.kind, TokenKind::Plus | TokenKind::Minus))
    {
        k += 1;
    }
    match (tokens.get(k), tokens.get(k + 1)) {
        (Some(num), Some(rp)) if num.kind == TokenKind::Number && rp.kind == TokenKind::RParen => {
            is_integer_lexeme(num.text(source))
        }
        _ => false,
    }
}

fn is_integer_lexeme(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn span_contains(outer: Span, inner: Span) -> bool {
    inner.start >= outer.start && inner.end <= outer.end
}

fn strip_quotes(s: &str) -> &str {
    let b = s.as_bytes();
    if b.len() >= 2
        && ((b[0] == b'\'' && b[b.len() - 1] == b'\'') || (b[0] == b'"' && b[b.len() - 1] == b'"'))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn merge(pending: Vec<Pending>) -> Vec<CompanionRecord> {
    let mut map: HashMap<String, Pending> = HashMap::new();
    for p in pending {
        let key = record_key(&p);
        match map.get(&key) {
            Some(old) if old.rank >= p.rank => {}
            _ => {
                map.insert(key, p);
            }
        }
    }
    let mut out: Vec<CompanionRecord> = map
        .into_values()
        .map(|p| CompanionRecord {
            kind: p.kind,
            named_in: p.named_in,
            name: p.name,
            path: p.path,
        })
        .collect();
    out.sort_by(|a, b| {
        a.named_in
            .start
            .cmp(&b.named_in.start)
            .then(a.named_in.end.cmp(&b.named_in.end))
            .then(a.name.cmp(&b.name))
    });
    out
}

fn record_key(p: &Pending) -> String {
    match &p.path {
        Some(path) => format!("path:{}", path_key(path)),
        None => {
            let n = normalize_separators(&p.name);
            let n = if cfg!(windows) {
                n.to_ascii_lowercase()
            } else {
                n
            };
            format!("unresolved:{n}")
        }
    }
}
