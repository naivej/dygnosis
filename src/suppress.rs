//! One suppression policy for LSP, MCP, CLI, and fixes.
//!
//! A directive is a real `//` comment. It drops an Added Warning whose source
//! start line is covered. Errors, Information, shared Warnings, and skipped
//! codes stay. Unknown codes and malformed comments do nothing.

use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, Severity};
use crate::explain::{explain, ExplainKind};
use crate::lexer::{tokenize, Token, TokenKind};
use crate::model::Model;
use crate::span::{LineIndex, Span};
use crate::workspace::Workspace;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Line,
    NextLine,
    File,
}

#[derive(Default)]
struct Directives {
    file: HashSet<String>,
    lines: HashMap<u32, HashSet<String>>,
}

pub(crate) fn apply_model(model: &Model, diags: Vec<Diagnostic>) -> Vec<Diagnostic> {
    apply_source(&model.source, &model.ms_unparsed_spans, diags)
}

pub(crate) fn apply_source(
    source: &str,
    native: &[Span],
    diags: Vec<Diagnostic>,
) -> Vec<Diagnostic> {
    let directives = collect(source, native);
    let index = LineIndex::new(source);
    diags
        .into_iter()
        .filter(|diag| {
            let line = index.position(source, diag.span.start).line;
            !covered(&directives, diag, line)
        })
        .collect()
}

/// Drop Added Warnings using each physical file's comments.
/// A span that does not map to a file is kept.
pub(crate) fn apply_effective(
    ws: &mut Workspace,
    root: &str,
    diags: Vec<Diagnostic>,
) -> Vec<Diagnostic> {
    let mut located = Vec::with_capacity(diags.len());
    for diag in &diags {
        located.push(locate(ws, root, diag.span));
    }
    let mut cache: HashMap<String, Directives> = HashMap::new();
    for item in &located {
        let Some((file, _)) = item else {
            continue;
        };
        if cache.contains_key(file) {
            continue;
        }
        let directives = match ws.get_model(file) {
            Some(model) => collect(&model.source, &model.ms_unparsed_spans),
            None => Directives::default(),
        };
        cache.insert(file.clone(), directives);
    }
    diags
        .into_iter()
        .zip(located)
        .filter(|(diag, origin)| match origin {
            Some((file, line)) => cache
                .get(file)
                .is_none_or(|directives| !covered(directives, diag, *line)),
            None => true,
        })
        .map(|(diag, _)| diag)
        .collect()
}

fn locate(ws: &mut Workspace, root: &str, span: Span) -> Option<(String, u32)> {
    let (file, origin) = ws.map_effective_origin(root, span)?;
    let model = ws.get_model(&file)?;
    let line = LineIndex::new(&model.source)
        .position(&model.source, origin.start)
        .line;
    Some((file, line))
}

fn covered(directives: &Directives, diag: &Diagnostic, line: u32) -> bool {
    if !suppressible(diag) {
        return false;
    }
    let code = diag.code.to_ascii_uppercase();
    directives.file.contains(&code)
        || directives
            .lines
            .get(&line)
            .is_some_and(|codes| codes.contains(&code))
}

fn suppressible(diag: &Diagnostic) -> bool {
    diag.severity == Severity::Warning
        && explain(&diag.code).is_some_and(|entry| entry.kind == ExplainKind::Added)
}

fn collect(source: &str, native: &[Span]) -> Directives {
    let inactive = inactive_ranges(source);
    let verbatim = verbatim_ranges(source);
    let mut directives = Directives::default();
    let index = LineIndex::new(source);
    let bytes = source.as_bytes();
    let mut pos = 0usize;
    while pos < source.len() {
        if let Some(end) = covering_end(pos as u32, &inactive)
            .or_else(|| covering_end(pos as u32, &verbatim))
            .or_else(|| covering_end(pos as u32, native))
        {
            pos = end as usize;
            continue;
        }
        let ch = source[pos..].chars().next().unwrap_or('\0');
        if ch == ' ' || ch == '\t' || ch == '\n' || ch == '\r' {
            pos += ch.len_utf8();
            continue;
        }
        if source[pos..].starts_with("//") {
            let line = index.position(source, pos as u32).line;
            let start = pos + 2;
            let end = source[start..]
                .find('\n')
                .map(|i| start + i)
                .unwrap_or(source.len());
            if let Some((scope, codes)) = parse_directive(&source[start..end]) {
                match scope {
                    Scope::File => directives.file.extend(codes),
                    Scope::Line => add_line(&mut directives, line, codes),
                    Scope::NextLine => add_line(&mut directives, line.saturating_add(1), codes),
                }
            }
            pos = end;
            continue;
        }
        if ch == '%' {
            pos = source[pos..]
                .find('\n')
                .map(|i| pos + i)
                .unwrap_or(source.len());
            continue;
        }
        if source[pos..].starts_with("/*") {
            pos = source[pos + 2..]
                .find("*/")
                .map(|i| pos + 2 + i + 2)
                .unwrap_or(source.len());
            continue;
        }
        if ch == '\'' || ch == '"' {
            pos = end_string(source, pos, ch);
            continue;
        }
        if source[pos..].starts_with("@#") {
            pos = end_macro_dir(bytes, pos);
            continue;
        }
        pos += ch.len_utf8();
    }
    directives
}

fn add_line(directives: &mut Directives, line: u32, codes: Vec<String>) {
    directives.lines.entry(line).or_default().extend(codes);
}

fn covering_end(pos: u32, ranges: &[Span]) -> Option<u32> {
    ranges
        .iter()
        .find(|span| pos >= span.start && pos < span.end)
        .map(|span| span.end)
}

fn parse_directive(text: &str) -> Option<(Scope, Vec<String>)> {
    let text = text.trim_start_matches([' ', '\t']);
    let (scope, rest) = directive_scope(text)?;
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    let mut codes = Vec::new();
    for part in rest.split(',') {
        let code = part.trim();
        if !is_code(code) {
            return None;
        }
        codes.push(code.to_ascii_uppercase());
    }
    if codes.is_empty() {
        return None;
    }
    Some((scope, codes))
}

fn directive_scope(text: &str) -> Option<(Scope, &str)> {
    const FORMS: &[(&str, Scope)] = &[
        ("dygnosis:disable-next-line", Scope::NextLine),
        ("dygnosis:disable-file", Scope::File),
        ("dygnosis:disable", Scope::Line),
        ("vsd:disable-file", Scope::File),
        ("vsd:disable", Scope::Line),
    ];
    for (name, scope) in FORMS {
        if let Some(rest) = text.strip_prefix(name) {
            return Some((*scope, rest));
        }
    }
    None
}

fn is_code(text: &str) -> bool {
    let mut chars = text.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => chars.all(|c| c.is_ascii_alphanumeric()),
        _ => false,
    }
}

fn end_string(source: &str, start: usize, quote: char) -> usize {
    let mut pos = start + quote.len_utf8();
    while pos < source.len() {
        let ch = source[pos..].chars().next().unwrap_or('\0');
        if ch == quote || ch == '\n' {
            if ch == quote {
                pos += ch.len_utf8();
            }
            return pos;
        }
        pos += ch.len_utf8();
    }
    source.len()
}

fn end_macro_dir(bytes: &[u8], start: usize) -> usize {
    let mut pos = start;
    loop {
        let line_start = pos;
        while pos < bytes.len() && bytes[pos] != b'\n' {
            pos += 1;
        }
        let line = std::str::from_utf8(&bytes[line_start..pos]).unwrap_or("");
        if !line.trim_end().ends_with('\\') {
            break;
        }
        if pos < bytes.len() && bytes[pos] == b'\n' {
            pos += 1;
        }
    }
    pos
}

fn verbatim_ranges(source: &str) -> Vec<Span> {
    let tokens = tokenize(source);
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if is_ident(&tokens[i], source, "verbatim")
            && tokens
                .get(i + 1)
                .is_some_and(|tok| tok.kind == TokenKind::Semi)
        {
            let start = tokens[i + 1].span.end;
            i += 2;
            while i < tokens.len() {
                if is_ident(&tokens[i], source, "end")
                    && tokens
                        .get(i + 1)
                        .is_some_and(|tok| tok.kind == TokenKind::Semi)
                {
                    ranges.push(Span {
                        start,
                        end: tokens[i].span.start,
                    });
                    i += 2;
                    break;
                }
                i += 1;
            }
            continue;
        }
        i += 1;
    }
    ranges
}

struct IfFrame {
    active: bool,
    taken: bool,
    region_start: u32,
}

fn inactive_ranges(source: &str) -> Vec<Span> {
    let tokens = tokenize(source);
    let mut defines: HashMap<String, i64> = HashMap::new();
    let mut stack: Vec<IfFrame> = Vec::new();
    let mut ranges = Vec::new();
    for tok in &tokens {
        if tok.kind != TokenKind::MacroDir {
            continue;
        }
        let body = tok
            .text(source)
            .trim_start()
            .strip_prefix("@#")
            .unwrap_or("")
            .trim_start();
        let name = leading_ident(body).unwrap_or("");
        let arg = body[name.len()..].trim_start();
        match name.to_ascii_lowercase().as_str() {
            "define" if emitting(&stack) => {
                if let Some((key, value)) = parse_define(arg) {
                    defines.insert(key, value);
                }
            }
            "if" => {
                let active = eval_if(arg, &defines);
                stack.push(IfFrame {
                    active,
                    taken: active,
                    region_start: tok.span.end,
                });
            }
            "ifdef" => {
                let flag = leading_ident(arg).unwrap_or("");
                let active = !flag.is_empty() && defines.contains_key(flag);
                stack.push(IfFrame {
                    active,
                    taken: active,
                    region_start: tok.span.end,
                });
            }
            "ifndef" => {
                let flag = leading_ident(arg).unwrap_or("");
                let active = !flag.is_empty() && !defines.contains_key(flag);
                stack.push(IfFrame {
                    active,
                    taken: active,
                    region_start: tok.span.end,
                });
            }
            "elseif" => {
                leave_inactive(&stack, tok.span.start, &mut ranges);
                if let Some(frame) = stack.last_mut() {
                    if frame.taken {
                        frame.active = false;
                    } else {
                        let active = eval_if(arg, &defines);
                        frame.active = active;
                        frame.taken = active;
                    }
                    frame.region_start = tok.span.end;
                }
            }
            "else" => {
                leave_inactive(&stack, tok.span.start, &mut ranges);
                if let Some(frame) = stack.last_mut() {
                    frame.active = !frame.taken;
                    frame.taken = true;
                    frame.region_start = tok.span.end;
                }
            }
            "endif" => {
                leave_inactive(&stack, tok.span.start, &mut ranges);
                stack.pop();
            }
            _ => {}
        }
    }
    ranges
}

fn emitting(stack: &[IfFrame]) -> bool {
    stack.iter().all(|frame| frame.active)
}

fn leave_inactive(stack: &[IfFrame], end: u32, ranges: &mut Vec<Span>) {
    if emitting(stack) {
        return;
    }
    if let Some(frame) = stack.last() {
        if frame.region_start < end {
            ranges.push(Span {
                start: frame.region_start,
                end,
            });
        }
    }
}

fn eval_if(arg: &str, defines: &HashMap<String, i64>) -> bool {
    let arg = arg.trim();
    if arg.is_empty() {
        return false;
    }
    if let Ok(n) = arg.parse::<i64>() {
        return n != 0;
    }
    if let Ok(n) = arg.parse::<f64>() {
        return n != 0.0;
    }
    if arg.eq_ignore_ascii_case("true") {
        return true;
    }
    if arg.eq_ignore_ascii_case("false") {
        return false;
    }
    if let Some(name) = leading_ident(arg) {
        if name.len() == arg.len() {
            return defines.get(name).is_some_and(|n| *n != 0);
        }
    }
    // A condition this pass cannot evaluate stays live, so a real directive is not dropped.
    true
}

fn parse_define(arg: &str) -> Option<(String, i64)> {
    let name = leading_ident(arg)?;
    let rest = arg[name.len()..].trim_start().strip_prefix('=')?.trim();
    let value = rest.parse::<i64>().ok()?;
    Some((name.to_string(), value))
}

fn leading_ident(text: &str) -> Option<&str> {
    let mut end = 0usize;
    for (i, ch) in text.char_indices() {
        let ok = if i == 0 {
            ch.is_ascii_alphabetic() || ch == '_'
        } else {
            ch.is_ascii_alphanumeric() || ch == '_'
        };
        if !ok {
            break;
        }
        end = i + ch.len_utf8();
    }
    if end == 0 {
        None
    } else {
        Some(&text[..end])
    }
}

fn is_ident(tok: &Token, source: &str, word: &str) -> bool {
    tok.kind == TokenKind::Ident && tok.text(source).eq_ignore_ascii_case(word)
}
