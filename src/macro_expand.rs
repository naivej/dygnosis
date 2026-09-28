//! Native `@#define` / `@#if` / `@#for` / `@{NAME}` expansion over the token stream.

use std::collections::HashMap;

use crate::lexer::{Token, TokenKind};
use crate::span::Span;

const RANGE_CAP: usize = 10_000;
const MACRO_DEPTH_CAP: usize = 32;

type MacroTypeError = (Span, &'static str, String);
type ExpandTracedFull = (
    Vec<Token>,
    Vec<TokenTrace>,
    Vec<FrameRec>,
    Vec<MacroTypeError>,
    Vec<Span>,
    Option<Span>,
);

#[derive(Clone, Debug)]
enum MacroVal {
    Int(i64),
    Real(f64),
    Bool(bool),
    Range { start: i64, end: i64 },
    Text(String),
    Tuple(Vec<MacroVal>),
    Array(Vec<MacroVal>),
    Function { params: Vec<String>, body: String },
    Unresolved,
}

impl MacroVal {
    fn display(&self) -> String {
        match self {
            MacroVal::Int(n) => n.to_string(),
            MacroVal::Real(n) => n.to_string(),
            MacroVal::Bool(b) => b.to_string(),
            MacroVal::Range { start, end } => format!("{start}:{end}"),
            MacroVal::Text(s) => s.clone(),
            MacroVal::Tuple(values) => format!(
                "({})",
                values
                    .iter()
                    .map(MacroVal::display)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            MacroVal::Array(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(MacroVal::display)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            MacroVal::Function { .. } => String::new(),
            MacroVal::Unresolved => String::new(),
        }
    }

    fn loop_values(&self) -> Option<Vec<MacroVal>> {
        match self {
            MacroVal::Range { start, end }
                if (*end as i128 - *start as i128 + 1) <= RANGE_CAP as i128 =>
            {
                Some(
                    inclusive_range(*start, *end)
                        .into_iter()
                        .map(MacroVal::Int)
                        .collect(),
                )
            }
            MacroVal::Array(values) if values.len() <= RANGE_CAP => Some(values.clone()),
            _ => None,
        }
    }

    fn condition(&self) -> Option<bool> {
        match self {
            MacroVal::Bool(b) => Some(*b),
            MacroVal::Int(n) => Some(*n != 0),
            MacroVal::Real(n) => Some(*n != 0.0),
            _ => None,
        }
    }
}

#[derive(Debug)]
enum MacroEvalError {
    UnknownVariable(String),
    UnknownFunction(String),
    TypeMismatch(&'static str),
    SyntaxEol,
    Unsupported,
}

impl MacroEvalError {
    fn diagnostic(&self) -> Option<(&'static str, String)> {
        match self {
            Self::UnknownVariable(name) => Some(("E063", format!("Unknown variable {name}"))),
            Self::UnknownFunction(name) => Some(("E063", format!("Unknown function {name}"))),
            Self::TypeMismatch(op) => Some((
                "E285",
                format!("Type mismatch for operands of {op} operator"),
            )),
            Self::SyntaxEol => Some(("E062", "syntax error, unexpected EOL".to_string())),
            Self::Unsupported => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dir {
    Define,
    Ifdef,
    Ifndef,
    If,
    Elseif,
    Else,
    Endif,
    For,
    Endfor,
    Unknown,
}

struct IfFrame {
    /// This branch of the chain is the one being emitted.
    active: bool,
    /// An earlier branch in this chain was selected, so later clauses stay inactive.
    taken: bool,
    frame_id: usize,
    body_start: u32,
    /// First byte of this branch, just after the directive that opened it.
    branch_start: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct TokenTrace {
    pub frames: Vec<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct FrameRec {
    pub kind: &'static str,
    pub body_span: Span,
    /// Loop index name. Set on a `@#for` iteration frame.
    pub variable: Option<String>,
    /// Loop index value for this iteration, as written after substitution.
    pub value: Option<String>,
}

struct ExpandState<'a> {
    src: &'a str,
    defines: &'a mut HashMap<String, MacroVal>,
    origin_stack: Vec<usize>,
    arena: &'a mut Vec<FrameRec>,
    type_errors: &'a mut Vec<MacroTypeError>,
    discarded: &'a mut Vec<Span>,
    incomplete: &'a mut Option<Span>,
    include_seen: bool,
}

pub fn expand_macros(src: &str, tokens: Vec<Token>) -> Vec<Token> {
    expand_macros_full(src, tokens).0
}

pub fn expand_macros_full(
    src: &str,
    tokens: Vec<Token>,
) -> (Vec<Token>, Vec<(Span, &'static str, String)>) {
    let (out, _, _, errors, _, _) = expand_macros_traced_full(src, tokens);
    (out, errors)
}

pub(crate) fn expand_macros_with_status(
    src: &str,
    tokens: Vec<Token>,
) -> (Vec<Token>, Vec<MacroTypeError>, Option<Span>) {
    let (out, _, _, errors, _, incomplete) = expand_macros_traced_full(src, tokens);
    (out, errors, incomplete)
}

pub(crate) fn expand_macros_traced_with_status(
    src: &str,
    tokens: Vec<Token>,
) -> (Vec<Token>, Vec<TokenTrace>, Vec<FrameRec>, bool) {
    let (out, traces, arena, _, _, incomplete) = expand_macros_traced_full(src, tokens);
    (out, traces, arena, incomplete.is_some())
}

/// Source ranges of `@#if` / `@#ifndef` branches that expansion discarded.
pub(crate) fn inactive_macro_spans(src: &str) -> Vec<Span> {
    let tokens = crate::lexer::tokenize(src);
    let (_, _, _, _, discarded, _) = expand_macros_traced_full(src, tokens);
    discarded
}

fn expand_macros_traced_full(src: &str, tokens: Vec<Token>) -> ExpandTracedFull {
    let mut defines = HashMap::new();
    let mut arena = Vec::new();
    let mut type_errors = Vec::new();
    let mut discarded = Vec::new();
    let mut incomplete = None;
    let (out, traces) = {
        let mut state = ExpandState {
            src,
            defines: &mut defines,
            origin_stack: Vec::new(),
            arena: &mut arena,
            type_errors: &mut type_errors,
            discarded: &mut discarded,
            incomplete: &mut incomplete,
            include_seen: false,
        };
        expand_seq(&mut state, &tokens)
    };
    (out, traces, arena, type_errors, discarded, incomplete)
}

fn expand_seq(state: &mut ExpandState<'_>, tokens: &[Token]) -> (Vec<Token>, Vec<TokenTrace>) {
    let mut out = Vec::new();
    let mut traces = Vec::new();
    let mut i = 0;
    let mut stack: Vec<IfFrame> = Vec::new();

    while i < tokens.len() {
        let tok = &tokens[i];
        if tok.kind == TokenKind::Eof {
            emit(state, &mut out, &mut traces, tok.clone());
            break;
        }
        if tok.kind == TokenKind::MacroDir {
            match dir_kind(state.src, tok) {
                Dir::Define => {
                    if emitting(&stack) {
                        match parse_define_eval(tok.text(state.src), state.defines) {
                            Ok(Some((name, val))) => {
                                state.defines.insert(name, val);
                            }
                            Ok(None) => {}
                            Err(error) => {
                                if let Some(name) = defined_name(tok.text(state.src)) {
                                    state.defines.insert(name, MacroVal::Unresolved);
                                }
                                if let Some((code, message)) = error.diagnostic() {
                                    state.type_errors.push((tok.span, code, message));
                                    state.incomplete.get_or_insert(tok.span);
                                    emit(state, &mut out, &mut traces, tok.clone());
                                } else {
                                    state.incomplete.get_or_insert(tok.span);
                                    emit(state, &mut out, &mut traces, tok.clone());
                                }
                            }
                        }
                    }
                    i += 1;
                }
                Dir::Ifdef => {
                    let cond = name_is_defined(state, tok, "ifdef");
                    i += 1;
                    push_if_frame(state, &mut stack, tokens, i, tok.span, "ifdef", cond);
                }
                Dir::Ifndef => {
                    let cond = match dir_arg_ident(tok.text(state.src), "ifndef") {
                        Some(name) => !state.defines.contains_key(&name),
                        None => false,
                    };
                    i += 1;
                    push_if_frame(state, &mut stack, tokens, i, tok.span, "ifndef", cond);
                }
                Dir::If => {
                    let cond = if emitting(&stack) {
                        eval_condition(state, tok, "if")
                    } else {
                        Some(false)
                    };
                    if let Some(cond) = cond {
                        i += 1;
                        push_if_frame(state, &mut stack, tokens, i, tok.span, "if", cond);
                    } else {
                        let next = take_if_end(state.src, tokens, i);
                        retain_raw_macro(state, &tokens[i..next], &mut out, &mut traces);
                        i = next;
                    }
                }
                Dir::Elseif => {
                    let boundary = tok.span;
                    let outer_emitting = stack
                        .get(..stack.len().saturating_sub(1))
                        .is_some_and(|outer| outer.iter().all(|frame| frame.active));
                    let active = match stack.last() {
                        Some(frame) if !frame.taken && outer_emitting => {
                            eval_condition(state, tok, "elseif")
                        }
                        _ => Some(false),
                    };
                    let Some(active) = active else {
                        let next = take_if_end(state.src, tokens, i);
                        retain_raw_macro(state, &tokens[i..next], &mut out, &mut traces);
                        if let Some(frame) = stack.pop() {
                            backpatch(
                                state.arena,
                                frame.frame_id,
                                frame.body_start,
                                boundary.start,
                            );
                            state.origin_stack.pop();
                        }
                        i = next;
                        continue;
                    };
                    i += 1;
                    if let Some(frame) = stack.last_mut() {
                        frame.taken = frame.taken || active;
                        open_next_branch(state, frame, tokens, i, boundary, "elseif", active);
                    }
                }
                Dir::Else => {
                    let boundary = tok.span;
                    i += 1;
                    if let Some(frame) = stack.last_mut() {
                        let active = !frame.taken;
                        frame.taken = true;
                        open_next_branch(state, frame, tokens, i, boundary, "else", active);
                    }
                }
                Dir::Endif => {
                    let end_span = tok.span;
                    i += 1;
                    if let Some(frame) = stack.pop() {
                        if !frame.active {
                            push_discarded(state, frame.branch_start, end_span.start);
                        }
                        backpatch(
                            state.arena,
                            frame.frame_id,
                            frame.body_start,
                            end_span.start,
                        );
                        state.origin_stack.pop();
                    }
                }
                Dir::For => {
                    let (body, next) = take_for_body(state.src, tokens, i);
                    if emitting(&stack) {
                        check_for_tuple(state, tok);
                        if !unroll_for(state, tok, body, &mut out, &mut traces) {
                            state.incomplete.get_or_insert(tok.span);
                            for original in &tokens[i..next] {
                                emit(state, &mut out, &mut traces, original.clone());
                            }
                        }
                    }
                    i = next;
                }
                Dir::Endfor | Dir::Unknown => {
                    if directive_name(tok.text(state.src)).eq_ignore_ascii_case("include")
                        && emitting(&stack)
                    {
                        state.include_seen = true;
                    }
                    i += 1;
                }
            }
            continue;
        }
        if tok.kind == TokenKind::MacroInterp {
            if emitting(&stack) {
                match subst_interp(state.src, tok, state.defines) {
                    Ok(replacements) => {
                        for replacement in replacements {
                            emit(state, &mut out, &mut traces, replacement);
                        }
                    }
                    Err(error) => {
                        if state.include_seen
                            && matches!(
                                error,
                                MacroEvalError::UnknownVariable(_)
                                    | MacroEvalError::UnknownFunction(_)
                            )
                        {
                            state.incomplete.get_or_insert(tok.span);
                            emit(state, &mut out, &mut traces, tok.clone());
                        } else if let Some((code, message)) = error.diagnostic() {
                            state.type_errors.push((tok.span, code, message));
                            state.incomplete.get_or_insert(tok.span);
                            emit(state, &mut out, &mut traces, tok.clone());
                        } else {
                            state.incomplete.get_or_insert(tok.span);
                            emit(state, &mut out, &mut traces, tok.clone());
                        }
                    }
                }
            }
            i += 1;
            continue;
        }
        if emitting(&stack) {
            emit(state, &mut out, &mut traces, tok.clone());
        }
        i += 1;
    }
    while let Some(frame) = stack.pop() {
        if !frame.active {
            let end = tokens
                .last()
                .map(|tok| tok.span.end)
                .unwrap_or(frame.branch_start);
            push_discarded(state, frame.branch_start, end);
        }
        state.origin_stack.pop();
    }
    debug_assert_eq!(out.len(), traces.len());
    (out, traces)
}

fn push_discarded(state: &mut ExpandState<'_>, start: u32, end: u32) {
    if end > start {
        state.discarded.push(Span { start, end });
    }
}

fn emit(state: &ExpandState<'_>, out: &mut Vec<Token>, traces: &mut Vec<TokenTrace>, tok: Token) {
    if let Some(prev) = out.last() {
        if let Some(merged) = merge_adjacent(state.src, prev, &tok) {
            *out.last_mut().expect("token just read") = merged;
            return;
        }
    }
    traces.push(TokenTrace {
        frames: state.origin_stack.clone(),
    });
    out.push(tok);
}

/// Glue `x@{i}` into one identifier when the pieces touch in the source.
///
/// Dynare substitutes `@{…}` as text before it lexes, so `x@{i}` with `i = 1`
/// is the identifier `x1`. A space, or a join that is not an identifier, stays
/// two tokens.
fn merge_adjacent(src: &str, prev: &Token, next: &Token) -> Option<Token> {
    if !matches!(prev.kind, TokenKind::Ident | TokenKind::Number)
        || !matches!(next.kind, TokenKind::Ident | TokenKind::Number)
    {
        return None;
    }
    if prev.span.end != next.span.start {
        return None;
    }
    let combined = format!("{}{}", prev.text(src), next.text(src));
    if !is_dynare_ident(&combined) {
        return None;
    }
    Some(Token::with_lexeme(
        TokenKind::Ident,
        Span {
            start: prev.span.start,
            end: next.span.end,
        },
        combined,
    ))
}

fn is_dynare_ident(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn emitting(stack: &[IfFrame]) -> bool {
    stack.iter().all(|f| f.active)
}

fn next_body_start(tokens: &[Token], i: usize, fallback: u32) -> u32 {
    tokens.get(i).map(|t| t.span.start).unwrap_or(fallback)
}

fn alloc_frame(arena: &mut Vec<FrameRec>, kind: &'static str, body_start: u32) -> usize {
    let id = arena.len();
    arena.push(FrameRec {
        kind,
        body_span: Span {
            start: body_start,
            end: body_start,
        },
        variable: None,
        value: None,
    });
    id
}

fn push_if_frame(
    state: &mut ExpandState<'_>,
    stack: &mut Vec<IfFrame>,
    tokens: &[Token],
    next_i: usize,
    dir_span: Span,
    kind: &'static str,
    active: bool,
) {
    let body_start = next_body_start(tokens, next_i, dir_span.end);
    let frame_id = alloc_frame(state.arena, kind, body_start);
    stack.push(IfFrame {
        active,
        taken: active,
        frame_id,
        body_start,
        branch_start: dir_span.end,
    });
    state.origin_stack.push(frame_id);
}

fn backpatch(arena: &mut [FrameRec], frame_id: usize, body_start: u32, body_end: u32) {
    if let Some(rec) = arena.get_mut(frame_id) {
        rec.body_span = Span {
            start: body_start,
            end: body_end.max(body_start),
        };
    }
}

fn tokens_body_span(body: &[Token]) -> Span {
    let mut first = None;
    let mut last_end = 0u32;
    for t in body {
        if t.kind == TokenKind::Eof {
            continue;
        }
        if first.is_none() {
            first = Some(t.span.start);
        }
        last_end = t.span.end;
    }
    match first {
        Some(start) => Span {
            start,
            end: last_end,
        },
        None => Span::default(),
    }
}

fn unroll_for(
    state: &mut ExpandState<'_>,
    for_tok: &Token,
    body: &[Token],
    out: &mut Vec<Token>,
    traces: &mut Vec<TokenTrace>,
) -> bool {
    let Some((vars, collection, condition)) = parse_for(for_tok.text(state.src)) else {
        return false;
    };
    let values = match eval_macro_expr(&collection, state.defines, 0) {
        Ok(values) => values,
        Err(error) => {
            if let Some((code, message)) = error.diagnostic() {
                state.type_errors.push((for_tok.span, code, message));
                return false;
            }
            return false;
        }
    };
    let Some(values) = values.loop_values() else {
        return false;
    };
    let mut planned = Vec::new();
    for value in values {
        let members = match (&vars[..], &value) {
            ([_], _) => vec![value.clone()],
            (_, MacroVal::Tuple(items)) if items.len() == vars.len() => items.clone(),
            _ => return false,
        };
        let mut bindings = state.defines.clone();
        for (name, member) in vars.iter().zip(&members) {
            bindings.insert(name.clone(), member.clone());
        }
        if let Some(condition) = &condition {
            match eval_macro_expr(condition, &bindings, 0) {
                Ok(result) => match result.condition() {
                    Some(true) => {}
                    Some(false) => continue,
                    None => return false,
                },
                Err(error) => {
                    if let Some((code, message)) = error.diagnostic() {
                        state.type_errors.push((for_tok.span, code, message));
                        return false;
                    }
                    return false;
                }
            }
        }
        planned.push((value, members));
    }
    let previous: Vec<_> = vars
        .iter()
        .map(|name| (name.clone(), state.defines.get(name).cloned()))
        .collect();
    let body_span = tokens_body_span(body);
    for (value, members) in planned {
        for (name, member) in vars.iter().zip(members) {
            state.defines.insert(name.clone(), member);
        }
        let frame_id = state.arena.len();
        state.arena.push(FrameRec {
            kind: "for",
            body_span,
            variable: Some(if vars.len() == 1 {
                vars[0].clone()
            } else {
                format!("({})", vars.join(","))
            }),
            value: Some(value.display()),
        });
        state.origin_stack.push(frame_id);
        let (expanded, expanded_traces) = expand_seq(state, body);
        for (tok, trace) in expanded.into_iter().zip(expanded_traces) {
            if tok.kind != TokenKind::Eof {
                if let Some(prev) = out.last() {
                    if let Some(merged) = merge_adjacent(state.src, prev, &tok) {
                        *out.last_mut().expect("token just read") = merged;
                        continue;
                    }
                }
                out.push(tok);
                traces.push(trace);
            }
        }
        state.origin_stack.pop();
    }
    for (name, prior) in previous {
        match prior {
            Some(value) => {
                state.defines.insert(name, value);
            }
            None => {
                state.defines.remove(&name);
            }
        }
    }
    true
}

fn take_for_body<'a>(src: &str, tokens: &'a [Token], for_idx: usize) -> (&'a [Token], usize) {
    let mut depth = 1usize;
    let mut j = for_idx + 1;
    while j < tokens.len() {
        let t = &tokens[j];
        if t.kind == TokenKind::Eof {
            return (&tokens[for_idx + 1..j], j);
        }
        if t.kind == TokenKind::MacroDir {
            match dir_kind(src, t) {
                Dir::For => depth += 1,
                Dir::Endfor => {
                    depth -= 1;
                    if depth == 0 {
                        return (&tokens[for_idx + 1..j], j + 1);
                    }
                }
                _ => {}
            }
        }
        j += 1;
    }
    (&tokens[for_idx + 1..], tokens.len())
}

fn subst_interp(
    src: &str,
    tok: &Token,
    defines: &HashMap<String, MacroVal>,
) -> Result<Vec<Token>, MacroEvalError> {
    let text = tok.text(src);
    let inner = text
        .strip_prefix("@{")
        .and_then(|s| s.strip_suffix('}'))
        .ok_or(MacroEvalError::Unsupported)?
        .trim();
    let val = eval_macro_expr(inner, defines, 0)?;
    let repl = val.display();
    if repl.is_empty() {
        return Err(MacroEvalError::Unsupported);
    }
    // A macro value is substituted as text before the .mod lexer reads it.
    // Retokenize that text, but do not execute newly generated macro syntax.
    // This lexer drops trivia and unknown characters, so only whitespace may
    // lie between its tokens; comments or skipped characters need a wider
    // surrounding-source lexer pass and remain explicitly incomplete.
    let generated = crate::lexer::tokenize(&repl);
    let pieces: Vec<_> = generated
        .iter()
        .filter(|piece| piece.kind != TokenKind::Eof)
        .collect();
    if pieces.is_empty() {
        return Err(MacroEvalError::Unsupported);
    }
    let mut cursor = 0usize;
    for piece in &pieces {
        let start = piece.span.start as usize;
        let end = piece.span.end as usize;
        if !repl[cursor..start].chars().all(char::is_whitespace)
            || matches!(piece.kind, TokenKind::MacroDir | TokenKind::MacroInterp)
        {
            return Err(MacroEvalError::Unsupported);
        }
        cursor = end;
    }
    if !repl[cursor..].chars().all(char::is_whitespace) {
        return Err(MacroEvalError::Unsupported);
    }
    let leading_space = pieces[0].span.start > 0;
    let trailing_space = pieces
        .last()
        .is_some_and(|piece| (piece.span.end as usize) < repl.len());
    let last = pieces.len() - 1;
    pieces
        .into_iter()
        .enumerate()
        .map(|(index, piece)| {
            let mut span = tok.span;
            if index == 0 && leading_space {
                span.start += 1;
            }
            if index == last && trailing_space {
                span.end -= 1;
            }
            if span.start >= span.end {
                return Err(MacroEvalError::Unsupported);
            }
            Ok(Token::with_lexeme(piece.kind, span, piece.text(&repl)))
        })
        .collect()
}

fn dir_kind(src: &str, tok: &Token) -> Dir {
    match directive_name(tok.text(src)).to_ascii_lowercase().as_str() {
        "define" => Dir::Define,
        "ifdef" => Dir::Ifdef,
        "ifndef" => Dir::Ifndef,
        "if" => Dir::If,
        "elseif" => Dir::Elseif,
        "else" => Dir::Else,
        "endif" => Dir::Endif,
        "for" => Dir::For,
        "endfor" => Dir::Endfor,
        _ => Dir::Unknown,
    }
}

fn name_is_defined(state: &ExpandState<'_>, tok: &Token, kw: &str) -> bool {
    dir_arg_ident(tok.text(state.src), kw).is_some_and(|name| state.defines.contains_key(&name))
}

fn open_next_branch(
    state: &mut ExpandState<'_>,
    frame: &mut IfFrame,
    tokens: &[Token],
    next_i: usize,
    boundary: Span,
    kind: &'static str,
    active: bool,
) {
    if !frame.active {
        push_discarded(state, frame.branch_start, boundary.start);
    }
    backpatch(
        state.arena,
        frame.frame_id,
        frame.body_start,
        boundary.start,
    );
    frame.active = active;
    let body_start = next_body_start(tokens, next_i, boundary.end);
    let frame_id = alloc_frame(state.arena, kind, body_start);
    frame.frame_id = frame_id;
    frame.body_start = body_start;
    frame.branch_start = boundary.end;
    if let Some(last) = state.origin_stack.last_mut() {
        *last = frame_id;
    } else {
        state.origin_stack.push(frame_id);
    }
}

fn directive_name(text: &str) -> &str {
    let Some(rest) = text.trim_start().strip_prefix("@#") else {
        return "";
    };
    let rest = rest.trim_start();
    match ident_len(rest) {
        Some(n) => &rest[..n],
        None => "",
    }
}

fn eval_condition(state: &mut ExpandState<'_>, tok: &Token, kw: &str) -> Option<bool> {
    let arg = strip_kw(tok.text(state.src), kw)?;
    let arg = arg.trim();
    if arg.is_empty() {
        return None;
    }
    match eval_macro_expr(arg, state.defines, 0) {
        Ok(value) => match value.condition() {
            Some(condition) => Some(condition),
            None => {
                state.type_errors.push((
                    tok.span,
                    "E283",
                    "The condition must evaluate to a boolean or a double".to_string(),
                ));
                None
            }
        },
        Err(error) => {
            if let Some((code, message)) = error.diagnostic() {
                state.type_errors.push((tok.span, code, message));
            }
            None
        }
    }
}

fn take_if_end(src: &str, tokens: &[Token], start: usize) -> usize {
    let mut depth = 1usize;
    for (index, token) in tokens.iter().enumerate().skip(start + 1) {
        if token.kind != TokenKind::MacroDir {
            continue;
        }
        match dir_kind(src, token) {
            Dir::If | Dir::Ifdef | Dir::Ifndef => depth += 1,
            Dir::Endif => {
                depth -= 1;
                if depth == 0 {
                    return index + 1;
                }
            }
            _ => {}
        }
    }
    tokens.len()
}

fn retain_raw_macro(
    state: &mut ExpandState<'_>,
    tokens: &[Token],
    out: &mut Vec<Token>,
    traces: &mut Vec<TokenTrace>,
) {
    if let Some(first) = tokens.first() {
        state.incomplete.get_or_insert(first.span);
    }
    for token in tokens {
        emit(state, out, traces, token.clone());
    }
}

fn parse_define_eval(
    text: &str,
    defines: &HashMap<String, MacroVal>,
) -> Result<Option<(String, MacroVal)>, MacroEvalError> {
    let Some(rest) = strip_kw(text, "define") else {
        return Ok(None);
    };
    let rest = rest.trim_start();
    let Some(n) = ident_len(rest) else {
        return Ok(None);
    };
    let name = rest[..n].to_string();
    let rest = rest[n..].trim_start();
    if let Some(after_open) = rest.strip_prefix('(') {
        let Some(close) = after_open.find(')') else {
            return Err(MacroEvalError::Unsupported);
        };
        let params: Vec<_> = after_open[..close]
            .split(',')
            .map(str::trim)
            .map(str::to_owned)
            .collect();
        if params.is_empty() || params.iter().any(|param| !is_simple_ident(param)) {
            return Err(MacroEvalError::Unsupported);
        }
        let Some(body) = after_open[close + 1..].trim_start().strip_prefix('=') else {
            return Err(MacroEvalError::Unsupported);
        };
        let body = strip_line_comment(body).trim();
        validate_function_body(body)?;
        return Ok(Some((
            name,
            MacroVal::Function {
                params,
                body: body.to_string(),
            },
        )));
    }
    let Some(body) = rest.strip_prefix('=') else {
        return Ok(Some((name, MacroVal::Bool(true))));
    };
    let body = strip_line_comment(body).trim();
    Ok(Some((name, eval_macro_expr(body, defines, 0)?)))
}

/// Check the function body's expression shape at definition time without
/// looking up its free names. Dynare parses a function body immediately but
/// evaluates names when the function is called. Only an unmistakable missing
/// final operand gets the pinned `unexpected EOL` sentence; uncertain syntax
/// stays incomplete instead of being mislabeled as an official refusal.
fn validate_function_body(body: &str) -> Result<(), MacroEvalError> {
    if body.is_empty() {
        return Err(MacroEvalError::SyntaxEol);
    }
    let chars: Vec<char> = body.chars().collect();
    let mut stack = Vec::new();
    let mut need_operand = true;
    let mut last_open = false;
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        if ch == '"' {
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                i += 1;
            }
            if i == chars.len() {
                return Err(MacroEvalError::SyntaxEol);
            }
            need_operand = false;
            last_open = false;
            i += 1;
            continue;
        }
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric() || matches!(chars[i], '_' | '.'))
            {
                i += 1;
            }
            need_operand = false;
            last_open = false;
            continue;
        }
        match ch {
            '(' | '[' => {
                stack.push(ch);
                need_operand = true;
                last_open = true;
            }
            ')' | ']' => {
                let expected = if ch == ')' { '(' } else { '[' };
                if stack.pop() != Some(expected) || (need_operand && !last_open) {
                    return Err(MacroEvalError::Unsupported);
                }
                need_operand = false;
                last_open = false;
            }
            ',' => {
                if need_operand {
                    return Err(MacroEvalError::Unsupported);
                }
                need_operand = true;
                last_open = false;
            }
            '+' | '-' | '!' if need_operand => {
                // Macro unary operators are legal at an operand position.
                last_open = false;
            }
            '+' | '-' | '*' | '/' | '^' | ':' | '<' | '>' | '!' | '&' | '|' | '=' => {
                if need_operand {
                    return Err(MacroEvalError::Unsupported);
                }
                if ch == '=' && chars.get(i + 1) != Some(&'=') {
                    return Err(MacroEvalError::Unsupported);
                }
                if (matches!(ch, '=' | '!' | '<' | '>' | '&' | '|')
                    && chars.get(i + 1) == Some(&'='))
                    || (matches!(ch, '&' | '|') && chars.get(i + 1) == Some(&ch))
                {
                    i += 1;
                }
                need_operand = true;
                last_open = false;
            }
            _ => return Err(MacroEvalError::Unsupported),
        }
        i += 1;
    }
    if need_operand || !stack.is_empty() {
        Err(MacroEvalError::SyntaxEol)
    } else {
        Ok(())
    }
}

fn eval_macro_expr(
    source: &str,
    defines: &HashMap<String, MacroVal>,
    depth: usize,
) -> Result<MacroVal, MacroEvalError> {
    if depth >= MACRO_DEPTH_CAP {
        return Err(MacroEvalError::Unsupported);
    }
    let source = source.trim();
    if source.is_empty() {
        return Err(MacroEvalError::Unsupported);
    }
    // Pinned macro grammar: `:` binds looser than arithmetic, tighter than comparison.
    for group in [&['|'][..], &['&'][..], &['=', '!', '<', '>'][..]] {
        if let Some((left, op, right)) = split_macro_binary(source, group) {
            let left = eval_macro_expr(left, defines, depth + 1)?;
            let right = eval_macro_expr(right, defines, depth + 1)?;
            return eval_binary(left, op, right);
        }
    }
    if let Some((left, right)) = split_top_level_char(source, ':') {
        let start = eval_macro_expr(left, defines, depth + 1)?;
        let end = eval_macro_expr(right, defines, depth + 1)?;
        if let (MacroVal::Int(start), MacroVal::Int(end)) = (start, end) {
            return Ok(MacroVal::Range { start, end });
        }
        return Err(MacroEvalError::Unsupported);
    }
    for group in [&['+', '-'][..], &['*', '/'][..]] {
        if let Some((left, op, right)) = split_macro_binary(source, group) {
            let left = eval_macro_expr(left, defines, depth + 1)?;
            let right = eval_macro_expr(right, defines, depth + 1)?;
            return eval_binary(left, op, right);
        }
    }
    if let Some(body) = source.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        return Ok(MacroVal::Array(
            split_top_level_list(body)
                .into_iter()
                .map(|item| eval_macro_expr(item, defines, depth + 1))
                .collect::<Result<_, _>>()?,
        ));
    }
    if let Some(body) = source.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        let parts = split_top_level_list(body);
        return if parts.len() > 1 {
            Ok(MacroVal::Tuple(
                parts
                    .into_iter()
                    .map(|item| eval_macro_expr(item, defines, depth + 1))
                    .collect::<Result<_, _>>()?,
            ))
        } else {
            eval_macro_expr(body, defines, depth + 1)
        };
    }
    if let Some(value) = strip_quotes(source) {
        return Ok(MacroVal::Text(value.to_string()));
    }
    if source.eq_ignore_ascii_case("true") || source.eq_ignore_ascii_case("false") {
        return Ok(MacroVal::Bool(source.eq_ignore_ascii_case("true")));
    }
    if let Ok(value) = source.parse::<i64>() {
        return Ok(MacroVal::Int(value));
    }
    if let Ok(value) = source.parse::<f64>() {
        return Ok(MacroVal::Real(value));
    }
    if let Some(rest) = source.strip_prefix('-') {
        return match eval_macro_expr(rest, defines, depth + 1)? {
            MacroVal::Int(value) => value
                .checked_neg()
                .map(MacroVal::Int)
                .ok_or(MacroEvalError::Unsupported),
            MacroVal::Real(value) => Ok(MacroVal::Real(-value)),
            _ => Err(MacroEvalError::TypeMismatch("-")),
        };
    }
    if let Some(rest) = source.strip_prefix('!') {
        return eval_macro_expr(rest, defines, depth + 1)?
            .condition()
            .map(|value| MacroVal::Bool(!value))
            .ok_or(MacroEvalError::TypeMismatch("!"));
    }
    let name_len = ident_len(source).ok_or(MacroEvalError::Unsupported)?;
    let name = &source[..name_len];
    let rest = source[name_len..].trim_start();
    if let Some(args) = rest.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        if name == "defined" {
            let arg = args.trim();
            if !is_simple_ident(arg) {
                return Err(MacroEvalError::Unsupported);
            }
            return match defines.get(arg) {
                Some(MacroVal::Unresolved) => Err(MacroEvalError::Unsupported),
                value => Ok(MacroVal::Bool(value.is_some())),
            };
        }
        let Some(MacroVal::Function { params, body }) = defines.get(name) else {
            return Err(if is_pinned_macro_builtin(name) {
                MacroEvalError::Unsupported
            } else {
                MacroEvalError::UnknownFunction(name.to_string())
            });
        };
        let arguments = split_top_level_list(args);
        if arguments.len() != params.len() {
            return Err(MacroEvalError::Unsupported);
        }
        let mut local = defines.clone();
        for (param, arg) in params.iter().zip(arguments) {
            local.insert(param.clone(), eval_macro_expr(arg, defines, depth + 1)?);
        }
        return eval_macro_expr(body, &local, depth + 1);
    }
    if let Some(index) = rest.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        let value = defines
            .get(name)
            .ok_or_else(|| MacroEvalError::UnknownVariable(name.to_string()))?;
        let MacroVal::Int(index) = eval_macro_expr(index, defines, depth + 1)? else {
            return Err(MacroEvalError::Unsupported);
        };
        return match value {
            MacroVal::Array(items) => items
                .get(index.saturating_sub(1) as usize)
                .cloned()
                .ok_or(MacroEvalError::Unsupported),
            _ => Err(MacroEvalError::Unsupported),
        };
    }
    if rest.is_empty() {
        let value = defines
            .get(name)
            .cloned()
            .ok_or_else(|| MacroEvalError::UnknownVariable(name.to_string()))?;
        return if matches!(value, MacroVal::Unresolved) {
            Err(MacroEvalError::Unsupported)
        } else {
            Ok(value)
        };
    }
    Err(MacroEvalError::Unsupported)
}

fn eval_binary(left: MacroVal, op: &str, right: MacroVal) -> Result<MacroVal, MacroEvalError> {
    // The pinned macro language has array/tuple operators and string ordering
    // that this bounded evaluator does not implement. They remain incomplete,
    // never a fabricated operand-type Error.
    if matches!(
        &left,
        MacroVal::Array(_) | MacroVal::Tuple(_) | MacroVal::Function { .. } | MacroVal::Unresolved
    ) || matches!(
        &right,
        MacroVal::Array(_) | MacroVal::Tuple(_) | MacroVal::Function { .. } | MacroVal::Unresolved
    ) || (matches!((&left, &right), (MacroVal::Text(_), MacroVal::Text(_)))
        && !matches!(op, "+" | "==" | "!="))
    {
        return Err(MacroEvalError::Unsupported);
    }
    if let (Some(a), Some(b)) = (numeric_value(&left), numeric_value(&right)) {
        if let (MacroVal::Int(a), MacroVal::Int(b)) = (&left, &right) {
            match op {
                "+" => {
                    return a
                        .checked_add(*b)
                        .map(MacroVal::Int)
                        .ok_or(MacroEvalError::Unsupported)
                }
                "-" => {
                    return a
                        .checked_sub(*b)
                        .map(MacroVal::Int)
                        .ok_or(MacroEvalError::Unsupported)
                }
                "*" => {
                    return a
                        .checked_mul(*b)
                        .map(MacroVal::Int)
                        .ok_or(MacroEvalError::Unsupported)
                }
                _ => {}
            }
        }
        let result = match op {
            "+" => MacroVal::Real(a + b),
            "-" => MacroVal::Real(a - b),
            "*" => MacroVal::Real(a * b),
            "/" if b != 0.0 => MacroVal::Real(a / b),
            "==" => MacroVal::Bool(a == b),
            "!=" => MacroVal::Bool(a != b),
            "<" => MacroVal::Bool(a < b),
            "<=" => MacroVal::Bool(a <= b),
            ">" => MacroVal::Bool(a > b),
            ">=" => MacroVal::Bool(a >= b),
            "&&" => MacroVal::Bool(a != 0.0 && b != 0.0),
            "||" => MacroVal::Bool(a != 0.0 || b != 0.0),
            _ => return Err(MacroEvalError::Unsupported),
        };
        return match result {
            MacroVal::Real(value) if !value.is_finite() => Err(MacroEvalError::Unsupported),
            other => Ok(other),
        };
    }
    if matches!(op, "&&" | "||") {
        if let (Some(a), Some(b)) = (left.condition(), right.condition()) {
            return Ok(MacroVal::Bool(if op == "&&" { a && b } else { a || b }));
        }
    }
    match (left, op, right) {
        (MacroVal::Text(a), "+", MacroVal::Text(b)) => Ok(MacroVal::Text(a + &b)),
        (MacroVal::Bool(a), "&&", MacroVal::Bool(b)) => Ok(MacroVal::Bool(a && b)),
        (MacroVal::Bool(a), "||", MacroVal::Bool(b)) => Ok(MacroVal::Bool(a || b)),
        (MacroVal::Bool(a), "==", MacroVal::Bool(b)) => Ok(MacroVal::Bool(a == b)),
        (MacroVal::Bool(a), "!=", MacroVal::Bool(b)) => Ok(MacroVal::Bool(a != b)),
        (MacroVal::Text(a), "==", MacroVal::Text(b)) => Ok(MacroVal::Bool(a == b)),
        (MacroVal::Text(a), "!=", MacroVal::Text(b)) => Ok(MacroVal::Bool(a != b)),
        (_, op, _) => Err(MacroEvalError::TypeMismatch(match op {
            "+" => "+",
            "-" => "-",
            "*" => "*",
            "/" => "/",
            "&&" => "&&",
            "||" => "||",
            _ => "comparison",
        })),
    }
}

fn numeric_value(value: &MacroVal) -> Option<f64> {
    match value {
        MacroVal::Int(value) => Some(*value as f64),
        MacroVal::Real(value) => Some(*value),
        _ => None,
    }
}

fn split_macro_binary<'a>(source: &'a str, group: &[char]) -> Option<(&'a str, &'a str, &'a str)> {
    let mut found = None;
    walk_top_level(source, |i, tail| {
        let op = [
            "||", "&&", "==", "!=", "<=", ">=", "+", "-", "*", "/", "<", ">",
        ]
        .into_iter()
        .find(|op| tail.starts_with(op) && group.contains(&op.chars().next().unwrap()));
        if let Some(op) = op {
            let left = source[..i].trim();
            let right = source[i + op.len()..].trim();
            if !left.is_empty() && !right.is_empty() && !(op == "-" && left.ends_with(':')) {
                found = Some((left, op, right));
            }
        }
    });
    found
}

fn split_top_level_char(source: &str, needle: char) -> Option<(&str, &str)> {
    let mut found = None;
    walk_top_level(source, |i, tail| {
        if tail.starts_with(needle) {
            found = Some((&source[..i], &source[i + needle.len_utf8()..]));
        }
    });
    found
}

fn split_top_level_keyword<'a>(source: &'a str, word: &str) -> Option<(&'a str, &'a str)> {
    let mut found = None;
    walk_top_level(source, |i, tail| {
        if tail.starts_with(word)
            && source[..i].chars().last().is_some_and(char::is_whitespace)
            && tail[word.len()..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
        {
            found = Some((&source[..i], &source[i + word.len()..]));
        }
    });
    found
}

fn split_top_level_list(source: &str) -> Vec<&str> {
    if source.trim().is_empty() {
        return Vec::new();
    }
    let mut starts = vec![0];
    walk_top_level(source, |i, tail| {
        if tail.starts_with(',') {
            starts.push(i + 1);
        }
    });
    let mut parts = Vec::new();
    for pair in starts.windows(2) {
        parts.push(source[pair[0]..pair[1] - 1].trim());
    }
    parts.push(source[*starts.last().unwrap()..].trim());
    parts
}

fn walk_top_level(source: &str, mut visit: impl FnMut(usize, &str)) {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (i, ch) in source.char_indices() {
        if let Some(delim) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == delim {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => visit(i, &source[i..]),
            _ => {}
        }
    }
}

fn strip_line_comment(source: &str) -> &str {
    let mut end = source.len();
    walk_top_level(source, |i, tail| {
        if tail.starts_with("//") && end == source.len() {
            end = i;
        }
    });
    &source[..end]
}

fn strip_quotes(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'"' && *bytes.last()? == b'"' {
        Some(&s[1..s.len() - 1])
    } else {
        None
    }
}

fn check_for_tuple(state: &mut ExpandState<'_>, tok: &Token) {
    let Some(rest) = strip_kw(tok.text(state.src), "for") else {
        return;
    };
    let rest = rest.trim_start();
    if !rest.starts_with('(') {
        return;
    }
    let Some(close) = rest.find(')') else {
        return;
    };
    let names = rest[1..close]
        .split(',')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .count();
    let after = rest[close + 1..].trim_start();
    let Some(after) = strip_word(after, "in") else {
        return;
    };
    let after = after.trim_start();
    if !after.starts_with('[') {
        return;
    };
    if let Some(n) = first_tuple_size(after) {
        if n != names {
            state.type_errors.push((
                tok.span,
                "E284",
                format!("Encountered tuple of size {n} but only have {names} index variables"),
            ));
        }
    }
}

fn first_tuple_size(s: &str) -> Option<usize> {
    let inner = s.trim().strip_prefix('[')?.strip_suffix(']')?;
    let inner = inner.trim();
    let start = inner.find('(')?;
    let end = inner[start..].find(')')?;
    let tuple = &inner[start + 1..start + end];
    Some(
        tuple
            .split(',')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .count(),
    )
}

fn parse_for(text: &str) -> Option<(Vec<String>, String, Option<String>)> {
    let rest = strip_kw(text, "for")?;
    let rest = rest.trim_start();
    let (vars, rest) = if let Some(after_open) = rest.strip_prefix('(') {
        let close = after_open.find(')')?;
        let names: Vec<_> = after_open[..close]
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect();
        if names.is_empty() || names.iter().any(|name| !is_simple_ident(name)) {
            return None;
        }
        (names, after_open[close + 1..].trim_start())
    } else {
        let n = ident_len(rest)?;
        (vec![rest[..n].to_string()], rest[n..].trim_start())
    };
    let rest = strip_word(rest, "in")?;
    let rest = rest.trim_start();
    let (collection, condition) = match split_top_level_keyword(rest, "when") {
        Some((collection, condition)) => (collection.trim(), Some(condition.trim().to_string())),
        None => (rest.trim(), None),
    };
    if collection.is_empty() || condition.as_ref().is_some_and(String::is_empty) {
        return None;
    }
    Some((vars, collection.to_string(), condition))
}

fn defined_name(text: &str) -> Option<String> {
    let rest = strip_kw(text, "define")?.trim_start();
    let len = ident_len(rest)?;
    Some(rest[..len].to_string())
}

fn is_pinned_macro_builtin(name: &str) -> bool {
    matches!(
        name,
        "max"
            | "min"
            | "mod"
            | "exp"
            | "log"
            | "ln"
            | "log10"
            | "sin"
            | "cos"
            | "tan"
            | "asin"
            | "acos"
            | "atan"
            | "sqrt"
            | "cbrt"
            | "sign"
            | "floor"
            | "ceil"
            | "trunc"
            | "erf"
            | "erfc"
            | "gamma"
            | "lgamma"
            | "round"
            | "normpdf"
            | "normcdf"
            | "length"
            | "empty"
            | "sum"
            | "isboolean"
            | "isreal"
            | "isstring"
            | "istuple"
            | "isarray"
            | "isempty"
            | "defined"
            | "bool"
            | "real"
            | "string"
            | "tuple"
            | "array"
    )
}

fn dir_arg_ident(text: &str, kw: &str) -> Option<String> {
    let rest = strip_kw(text, kw)?;
    let rest = rest.trim_start();
    let n = ident_len(rest)?;
    Some(rest[..n].to_string())
}

fn inclusive_range(start: i64, end: i64) -> Vec<i64> {
    if start > end {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut n = start;
    while n <= end && out.len() < RANGE_CAP {
        out.push(n);
        if n == end {
            break;
        }
        n += 1;
    }
    out
}

fn strip_kw<'a>(text: &'a str, kw: &str) -> Option<&'a str> {
    let rest = text.trim_start().strip_prefix("@#")?;
    strip_word(rest.trim_start(), kw)
}

fn strip_word<'a>(text: &'a str, word: &str) -> Option<&'a str> {
    if text.len() < word.len() || !text[..word.len()].eq_ignore_ascii_case(word) {
        return None;
    }
    let after = &text[word.len()..];
    if after
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    Some(after)
}

fn ident_len(s: &str) -> Option<usize> {
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
    Some(end)
}

fn is_simple_ident(s: &str) -> bool {
    ident_len(s).is_some_and(|n| n == s.len())
}
