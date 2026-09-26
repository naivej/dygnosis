//! Signature help for a catalog command's option list.
//!
//! Names and descriptions come from the command catalog. `{` and `}` are not
//! lexer tokens, so this scan reads the source. A comma inside parentheses,
//! brackets, braces, a string, or a comment does not start the next option.

use tower_lsp::lsp_types::{
    Documentation, ParameterInformation, ParameterLabel, SignatureHelp, SignatureInformation,
};

use crate::catalog::{command_options, is_known_command, option_doc, HET_SHOCKS_OVERWRITE};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Paren,
    Bracket,
    Brace,
}

enum Atom<'a> {
    Ident(&'a str),
    Open(Kind),
    Close(Kind),
    Comma,
    Semi,
    Other,
}

struct Frame {
    kind: Kind,
    command: Option<String>,
    content_start: usize,
}

/// Catalog option signature at `byte`, or nothing outside a known option list.
pub(crate) fn signature_help(src: &str, byte: u32) -> Option<SignatureHelp> {
    let cursor = (byte as usize).min(src.len());
    let frames = open_frames(src, cursor);
    for frame in frames.iter().rev() {
        if frame.kind != Kind::Paren {
            continue;
        }
        let Some(command) = frame.command.as_deref() else {
            continue;
        };
        if !is_known_command(command) {
            continue;
        }
        let list_end = list_end(src, frame.content_start);
        let heterogeneous =
            command == "shocks" && has_name(src, frame.content_start, list_end, "heterogeneity");
        let options = option_list(command, heterogeneous);
        if options.is_empty() {
            continue;
        }
        let typed = current_name(src, frame.content_start, cursor.min(list_end));
        let active = active_index(&options, &typed);
        return Some(build(command, &options, active));
    }
    None
}

fn option_list(command: &str, heterogeneous: bool) -> Vec<(&'static str, &'static str)> {
    let mut out = Vec::new();
    for &(name, doc) in command_options(command) {
        if heterogeneous && name != "heterogeneity" && name != "overwrite" {
            continue;
        }
        out.push((name, describe(name, doc, heterogeneous)));
    }
    out
}

fn describe(name: &str, doc: &'static str, heterogeneous: bool) -> &'static str {
    if heterogeneous && name == "overwrite" {
        return HET_SHOCKS_OVERWRITE;
    }
    if !doc.is_empty() {
        return doc;
    }
    let fallback = option_doc(name);
    if fallback.is_empty() {
        doc
    } else {
        fallback
    }
}

fn active_index(options: &[(&str, &str)], typed: &str) -> Option<u32> {
    if typed.is_empty() {
        return None;
    }
    if let Some(index) = options
        .iter()
        .position(|(name, _)| name.eq_ignore_ascii_case(typed))
    {
        return Some(index as u32);
    }
    let mut matches = options.iter().enumerate().filter(|(_, (name, _))| {
        name.len() > typed.len()
            && name.as_bytes()[..typed.len()].eq_ignore_ascii_case(typed.as_bytes())
    });
    let (index, _) = matches.next()?;
    if matches.next().is_some() {
        None
    } else {
        Some(index as u32)
    }
}

fn build(command: &str, options: &[(&str, &str)], active: Option<u32>) -> SignatureHelp {
    let mut label = format!("{command}(");
    let mut parameters = Vec::with_capacity(options.len());
    for (i, (name, doc)) in options.iter().enumerate() {
        if i > 0 {
            label.push_str(", ");
        }
        let start = utf16_len(&label);
        label.push_str(name);
        let end = utf16_len(&label);
        parameters.push(ParameterInformation {
            label: ParameterLabel::LabelOffsets([start, end]),
            documentation: (!doc.is_empty()).then(|| Documentation::String((*doc).to_string())),
        });
    }
    label.push(')');
    SignatureHelp {
        signatures: vec![SignatureInformation {
            label,
            documentation: None,
            parameters: Some(parameters),
            active_parameter: active,
        }],
        active_signature: Some(0),
        active_parameter: active,
    }
}

fn utf16_len(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

fn open_frames(src: &str, end: usize) -> Vec<Frame> {
    let mut frames = Vec::new();
    let mut pending: Option<String> = None;
    let mut i = 0;
    while i < end {
        match next_atom(src, &mut i, end) {
            Some(Atom::Ident(name)) => pending = Some(name.to_ascii_lowercase()),
            Some(Atom::Open(Kind::Paren)) => {
                let command = pending.take();
                frames.push(Frame {
                    kind: Kind::Paren,
                    command,
                    content_start: i,
                });
            }
            Some(Atom::Open(kind)) => {
                pending = None;
                frames.push(Frame {
                    kind,
                    command: None,
                    content_start: i,
                });
            }
            Some(Atom::Close(kind)) => {
                pending = None;
                if let Some(pos) = frames.iter().rposition(|frame| frame.kind == kind) {
                    frames.truncate(pos);
                }
            }
            Some(Atom::Semi) => {
                pending = None;
                frames.clear();
            }
            Some(Atom::Comma | Atom::Other) => pending = None,
            None => break,
        }
    }
    frames
}

/// End of the option list: the matching `)`, a `;` at list depth, or the end of the file.
fn list_end(src: &str, start: usize) -> usize {
    let mut stack = Vec::new();
    let mut i = start;
    let end = src.len();
    while i < end {
        let before = i;
        match next_atom(src, &mut i, end) {
            Some(Atom::Open(kind)) => stack.push(kind),
            Some(Atom::Close(kind)) => {
                if let Some(pos) = stack.iter().rposition(|open| *open == kind) {
                    stack.truncate(pos);
                } else if kind == Kind::Paren {
                    return before;
                }
            }
            Some(Atom::Semi) if stack.is_empty() => return before,
            Some(_) => {}
            None => break,
        }
    }
    end
}

fn has_name(src: &str, start: usize, end: usize, name: &str) -> bool {
    segment_starts(src, start, end)
        .iter()
        .any(|&seg| first_ident(src, seg, end).eq_ignore_ascii_case(name))
}

fn current_name(src: &str, start: usize, end: usize) -> String {
    let starts = segment_starts(src, start, end);
    let seg = starts.last().copied().unwrap_or(start);
    first_ident(src, seg, end)
}

fn segment_starts(src: &str, start: usize, end: usize) -> Vec<usize> {
    let mut starts = vec![start];
    let mut stack = Vec::new();
    let mut i = start;
    while i < end {
        match next_atom(src, &mut i, end) {
            Some(Atom::Comma) if stack.is_empty() => starts.push(i),
            Some(Atom::Open(kind)) => stack.push(kind),
            Some(Atom::Close(kind)) => {
                if let Some(pos) = stack.iter().rposition(|open| *open == kind) {
                    stack.truncate(pos);
                }
            }
            _ => {}
        }
    }
    starts
}

fn first_ident(src: &str, start: usize, end: usize) -> String {
    let mut i = start;
    match next_atom(src, &mut i, end) {
        Some(Atom::Ident(name)) => name.to_string(),
        _ => String::new(),
    }
}

fn next_atom<'a>(src: &'a str, i: &mut usize, end: usize) -> Option<Atom<'a>> {
    while *i < end {
        let rest = &src[*i..end];
        if rest.starts_with("//") || rest.starts_with('%') {
            *i = line_end(src, *i, end);
            continue;
        }
        if rest.starts_with("/*") {
            *i = block_end(src, *i, end);
            continue;
        }
        if rest.starts_with("@#") {
            *i = macro_dir_end(src, *i, end);
            return Some(Atom::Other);
        }
        if rest.starts_with("@{") {
            *i = macro_interp_end(src, *i, end);
            return Some(Atom::Other);
        }
        let ch = src[*i..].chars().next()?;
        if matches!(ch, ' ' | '\t' | '\n' | '\r') {
            *i += ch.len_utf8();
            continue;
        }
        if ch == '"' || ch == '\'' {
            *i = string_end(src, *i, end, ch);
            return Some(Atom::Other);
        }
        if ch == '$' {
            *i = latex_end(src, *i, end);
            return Some(Atom::Other);
        }
        if ch.is_ascii_alphabetic() {
            return Some(Atom::Ident(read_ident(src, i, end)));
        }
        *i += ch.len_utf8();
        return Some(match ch {
            '(' => Atom::Open(Kind::Paren),
            ')' => Atom::Close(Kind::Paren),
            '[' => Atom::Open(Kind::Bracket),
            ']' => Atom::Close(Kind::Bracket),
            '{' => Atom::Open(Kind::Brace),
            '}' => Atom::Close(Kind::Brace),
            ',' => Atom::Comma,
            ';' => Atom::Semi,
            _ => Atom::Other,
        });
    }
    None
}

fn read_ident<'a>(src: &'a str, i: &mut usize, end: usize) -> &'a str {
    let start = *i;
    *i += src[*i..].chars().next().map(char::len_utf8).unwrap_or(0);
    while *i < end {
        let b = src.as_bytes()[*i];
        if b.is_ascii_alphanumeric() || b == b'_' {
            *i += 1;
        } else {
            break;
        }
    }
    &src[start..*i]
}

fn line_end(src: &str, i: usize, end: usize) -> usize {
    src[i..end].find('\n').map_or(end, |rel| i + rel)
}

fn block_end(src: &str, i: usize, end: usize) -> usize {
    src[i + 2..end]
        .find("*/")
        .map_or(end, |rel| i + 2 + rel + 2)
}

fn string_end(src: &str, i: usize, end: usize, quote: char) -> usize {
    let mut j = i + quote.len_utf8();
    while j < end {
        let ch = src[j..].chars().next().unwrap();
        j += ch.len_utf8();
        if ch == quote || ch == '\n' {
            break;
        }
    }
    j
}

fn latex_end(src: &str, i: usize, end: usize) -> usize {
    let mut j = i + 1;
    while j < end {
        let ch = src[j..].chars().next().unwrap();
        j += ch.len_utf8();
        if ch == '$' || ch == '\n' {
            break;
        }
    }
    j
}

fn macro_dir_end(src: &str, mut i: usize, end: usize) -> usize {
    loop {
        let line_start = i;
        let nl = line_end(src, i, end);
        let continued = src[line_start..nl].trim_end().ends_with('\\');
        i = nl;
        if !continued || i >= end || src.as_bytes()[i] != b'\n' {
            break;
        }
        i += 1;
    }
    i
}

fn macro_interp_end(src: &str, i: usize, end: usize) -> usize {
    src[i + 2..end].find('}').map_or(end, |rel| i + 2 + rel + 1)
}
