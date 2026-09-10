//! Native `@#define` / `@#if` / `@#for` / `@{NAME}` expansion over the token stream.

use std::collections::HashMap;

use crate::lexer::{Token, TokenKind};

const RANGE_CAP: usize = 10_000;

#[derive(Clone, Debug)]
enum MacroVal {
    Int(i64),
    Range { start: i64, end: i64 },
    Text(String),
}

impl MacroVal {
    fn display(&self) -> String {
        match self {
            MacroVal::Int(n) => n.to_string(),
            MacroVal::Range { start, end } => format!("{start}:{end}"),
            MacroVal::Text(s) => s.clone(),
        }
    }

    fn truthy(&self) -> bool {
        match self {
            MacroVal::Int(n) => *n != 0,
            MacroVal::Range { start, end } => start <= end,
            MacroVal::Text(s) => {
                let t = s.trim();
                if t.is_empty() {
                    return false;
                }
                let lower = t.to_ascii_lowercase();
                if matches!(lower.as_str(), "0" | "false" | "no") {
                    return false;
                }
                if let Ok(n) = t.parse::<i64>() {
                    return n != 0;
                }
                if let Ok(f) = t.parse::<f64>() {
                    return f != 0.0;
                }
                true
            }
        }
    }

    fn range_values(&self) -> Option<Vec<i64>> {
        match self {
            MacroVal::Range { start, end } => Some(inclusive_range(*start, *end)),
            MacroVal::Int(n) => Some(vec![*n]),
            MacroVal::Text(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dir {
    Define,
    Ifndef,
    If,
    Else,
    Endif,
    For,
    Endfor,
    Unknown,
}

struct IfFrame {
    active: bool,
}

pub fn expand_macros(src: &str, tokens: Vec<Token>) -> Vec<Token> {
    let mut defines = HashMap::new();
    expand_seq(src, &tokens, &mut defines)
}

fn expand_seq(src: &str, tokens: &[Token], defines: &mut HashMap<String, MacroVal>) -> Vec<Token> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut stack: Vec<IfFrame> = Vec::new();

    while i < tokens.len() {
        let tok = &tokens[i];
        if tok.kind == TokenKind::Eof {
            out.push(tok.clone());
            break;
        }
        if tok.kind == TokenKind::MacroDir {
            match dir_kind(src, tok) {
                Dir::Define => {
                    if emitting(&stack) {
                        if let Some((name, val)) = parse_define(tok.text(src)) {
                            defines.insert(name, val);
                        }
                    }
                    i += 1;
                }
                Dir::Ifndef => {
                    let cond = match dir_arg_ident(tok.text(src), "ifndef") {
                        Some(name) => !defines.contains_key(&name),
                        None => false,
                    };
                    stack.push(IfFrame { active: cond });
                    i += 1;
                }
                Dir::If => {
                    let cond = match dir_arg_ident(tok.text(src), "if") {
                        Some(name) => defines.get(&name).is_some_and(MacroVal::truthy),
                        None => false,
                    };
                    stack.push(IfFrame { active: cond });
                    i += 1;
                }
                Dir::Else => {
                    if let Some(frame) = stack.last_mut() {
                        frame.active = !frame.active;
                    }
                    i += 1;
                }
                Dir::Endif => {
                    stack.pop();
                    i += 1;
                }
                Dir::For => {
                    let (body, next) = take_for_body(src, tokens, i);
                    if emitting(&stack) {
                        unroll_for(src, tok, body, defines, &mut out);
                    }
                    i = next;
                }
                Dir::Endfor | Dir::Unknown => {
                    i += 1;
                }
            }
            continue;
        }
        if tok.kind == TokenKind::MacroInterp {
            if emitting(&stack) {
                if let Some(repl) = subst_interp(src, tok, defines) {
                    out.push(repl);
                }
            }
            i += 1;
            continue;
        }
        if emitting(&stack) {
            out.push(tok.clone());
        }
        i += 1;
    }
    out
}

fn emitting(stack: &[IfFrame]) -> bool {
    stack.iter().all(|f| f.active)
}

fn unroll_for(
    src: &str,
    for_tok: &Token,
    body: &[Token],
    defines: &mut HashMap<String, MacroVal>,
    out: &mut Vec<Token>,
) {
    let Some((var, collection)) = parse_for(for_tok.text(src)) else {
        return;
    };
    let Some(values) = defines.get(&collection).and_then(MacroVal::range_values) else {
        return;
    };
    let previous = defines.get(&var).cloned();
    for n in values {
        defines.insert(var.clone(), MacroVal::Int(n));
        let expanded = expand_seq(src, body, defines);
        out.extend(expanded.into_iter().filter(|t| t.kind != TokenKind::Eof));
    }
    match previous {
        Some(prev) => {
            defines.insert(var, prev);
        }
        None => {
            defines.remove(&var);
        }
    }
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

fn subst_interp(src: &str, tok: &Token, defines: &HashMap<String, MacroVal>) -> Option<Token> {
    let text = tok.text(src);
    let inner = text.strip_prefix("@{")?.strip_suffix('}')?.trim();
    if !is_simple_ident(inner) {
        return None;
    }
    let val = defines.get(inner)?;
    let repl = val.display();
    let kind = kind_for_interpolated(&repl);
    Some(Token::with_lexeme(kind, tok.span, repl))
}

fn kind_for_interpolated(text: &str) -> TokenKind {
    if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) {
        TokenKind::Number
    } else {
        TokenKind::Ident
    }
}

fn dir_kind(src: &str, tok: &Token) -> Dir {
    match directive_name(tok.text(src)).to_ascii_lowercase().as_str() {
        "define" => Dir::Define,
        "ifndef" => Dir::Ifndef,
        "if" => Dir::If,
        "else" => Dir::Else,
        "endif" => Dir::Endif,
        "for" => Dir::For,
        "endfor" => Dir::Endfor,
        _ => Dir::Unknown,
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

fn parse_define(text: &str) -> Option<(String, MacroVal)> {
    let rest = strip_kw(text, "define")?;
    let rest = rest.trim_start();
    let n = ident_len(rest)?;
    let name = rest[..n].to_string();
    let rest = rest[n..].trim_start().strip_prefix('=')?.trim_start();
    let val = parse_macro_value(rest)?;
    Some((name, val))
}

fn parse_for(text: &str) -> Option<(String, String)> {
    let rest = strip_kw(text, "for")?;
    let rest = rest.trim_start();
    let n = ident_len(rest)?;
    let var = rest[..n].to_string();
    let rest = rest[n..].trim_start();
    let rest = strip_word(rest, "in")?;
    let rest = rest.trim_start();
    let m = ident_len(rest)?;
    Some((var, rest[..m].to_string()))
}

fn dir_arg_ident(text: &str, kw: &str) -> Option<String> {
    let rest = strip_kw(text, kw)?;
    let rest = rest.trim_start();
    let n = ident_len(rest)?;
    Some(rest[..n].to_string())
}

fn parse_macro_value(s: &str) -> Option<MacroVal> {
    let s = s.trim();
    if s.is_empty() {
        return Some(MacroVal::Text(String::new()));
    }
    if let Some((a, b)) = s.split_once(':') {
        let start: i64 = a.trim().parse().ok()?;
        let end: i64 = b.trim().parse().ok()?;
        return Some(MacroVal::Range { start, end });
    }
    if let Ok(n) = s.parse::<i64>() {
        return Some(MacroVal::Int(n));
    }
    Some(MacroVal::Text(s.to_string()))
}

fn inclusive_range(start: i64, end: i64) -> Vec<i64> {
    if start > end {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut n = start;
    while n <= end && out.len() < RANGE_CAP {
        out.push(n);
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
