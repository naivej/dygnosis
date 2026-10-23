//! Signature help for catalog command options and function arguments.
//!
//! Names and descriptions come from the command catalog. `{` and `}` are not
//! lexer tokens, so this scan reads the source. A comma inside parentheses,
//! brackets, braces, a string, or a comment does not start the next option.

use tower_lsp::lsp_types::{
    Documentation, ParameterInformation, ParameterLabel, SignatureHelp, SignatureInformation,
};

use crate::engine::catalog::syntax;
use crate::engine::catalog::{HET_SHOCKS_OVERWRITE, is_known_command, option_doc};
use crate::engine::language::lexer::{Token, TokenKind};

#[cfg(test)]
thread_local! {
    static PREFIX_WALKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Function help takes precedence over command options for the same cursor.
pub(crate) fn combined_signature_help(
    src: &str,
    byte: u32,
    context: &str,
    macro_tokens: &[Token],
) -> Option<SignatureHelp> {
    let cursor = byte as usize;
    let prefix = src.get(..cursor)?;
    if let Some(start) = macro_expression_start(macro_tokens, byte) {
        // Macro interpolation also runs inside comments and strings. Its expression
        // has a different lexical context from the command containing that text.
        let function = prefix.get(start..).and_then(|expression| {
            let (_, functions) = prefix_frames(expression, expression.len(), false);
            function_help(&functions, context)
        });
        return function.or_else(|| command_help(src, cursor, &open_frames(src, cursor)));
    }
    let (commands, functions) = prefix_frames(src, cursor, true);
    function_help(&functions, context).or_else(|| command_help(src, cursor, &commands))
}

fn macro_expression_start(tokens: &[Token], byte: u32) -> Option<usize> {
    let first = tokens.partition_point(|token| token.span.end < byte);
    tokens[first..]
        .iter()
        .take_while(|token| token.span.start <= byte)
        .find(|token| matches!(token.kind, TokenKind::MacroDir | TokenKind::MacroInterp))
        .map(|token| token.span.start as usize + 2)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Paren,
    Bracket,
    Brace,
}

#[derive(Clone, Copy)]
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
#[cfg(test)]
pub(crate) fn signature_help(src: &str, byte: u32) -> Option<SignatureHelp> {
    let cursor = (byte as usize).min(src.len());
    let frames = open_frames(src, cursor);
    command_help(src, cursor, &frames)
}

fn command_help(src: &str, cursor: usize, frames: &[Frame]) -> Option<SignatureHelp> {
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
    for option in crate::lsp::syntax_intel::options(command) {
        let name = option.name.as_str();
        let doc = option.summary.as_str();
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
    if fallback.is_empty() { doc } else { fallback }
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
    let mut commands = CommandFrames::new();
    walk_prefix(src, end, |atom, offset| commands.push(atom, offset));
    commands.frames
}

struct CommandFrames {
    frames: Vec<Frame>,
    pending: Option<String>,
    statement: Option<String>,
    at_statement_head: bool,
    in_block: bool,
}

impl CommandFrames {
    fn new() -> Self {
        Self {
            frames: Vec::new(),
            pending: None,
            statement: None,
            at_statement_head: true,
            in_block: false,
        }
    }

    fn push(&mut self, atom: Atom<'_>, offset: usize) {
        match atom {
            Atom::Ident(name) => {
                if self.at_statement_head {
                    self.statement = Some(name.to_ascii_lowercase());
                    self.pending = (!self.in_block).then(|| name.to_ascii_lowercase());
                    self.at_statement_head = false;
                } else {
                    self.pending = None;
                }
            }
            Atom::Open(Kind::Paren) => {
                let command = self.pending.take();
                self.frames.push(Frame {
                    kind: Kind::Paren,
                    command,
                    content_start: offset,
                });
            }
            Atom::Open(kind) => {
                self.pending = None;
                self.frames.push(Frame {
                    kind,
                    command: None,
                    content_start: offset,
                });
            }
            Atom::Close(kind) => {
                self.pending = None;
                if let Some(pos) = self.frames.iter().rposition(|frame| frame.kind == kind) {
                    self.frames.truncate(pos);
                }
            }
            Atom::Semi => {
                self.pending = None;
                self.frames.clear();
                if self.statement.as_deref() == Some("end") {
                    self.in_block = false;
                } else if !self.in_block && self.statement.as_deref().is_some_and(is_block_opener) {
                    self.in_block = true;
                }
                self.statement = None;
                self.at_statement_head = true;
            }
            Atom::Comma | Atom::Other => self.pending = None,
        }
    }
}

fn walk_prefix<'a>(src: &'a str, end: usize, mut visit: impl FnMut(Atom<'a>, usize)) {
    #[cfg(test)]
    PREFIX_WALKS.with(|walks| walks.set(walks.get() + 1));
    let mut i = 0;
    while let Some(atom) = next_atom(src, &mut i, end) {
        visit(atom, i);
    }
}

fn prefix_frames(src: &str, end: usize, commands: bool) -> (Vec<Frame>, FunctionFrames<'_>) {
    let mut command_frames = CommandFrames::new();
    let mut function_frames = FunctionFrames::default();
    walk_prefix(src, end, |atom, offset| {
        if commands {
            command_frames.push(atom, offset);
        }
        function_frames.push(atom);
    });
    (command_frames.frames, function_frames)
}

fn is_block_opener(word: &str) -> bool {
    syntax::is_block(word)
}

/// Key/value selection uses the same balanced scan as option signature help.
/// A value expression must not receive the list of option keys.
pub(crate) fn option_context(src: &str, byte: u32) -> Option<(String, Option<String>)> {
    let cursor = byte as usize;
    let frames = open_frames(src, cursor);
    let (command, start) = if let Some(frame) = frames
        .iter()
        .rev()
        .find(|frame| frame.command.as_deref().is_some_and(is_known_command))
    {
        (frame.command.clone()?, frame.content_start)
    } else {
        let command = crate::engine::editing::refs::option_owner_at(src, byte)?;
        // Qualified prior/options heads are written after a user-name slot.
        if !matches!(command.as_str(), "prior" | "options" | "subsamples") {
            return None;
        }
        let frame = frames
            .iter()
            .rev()
            .find(|frame| frame.kind == Kind::Paren)?;
        (command, frame.content_start)
    };
    let segment = segment_starts(src, start, cursor).last().copied()?;
    let prefix = &src[segment..cursor];
    let key = first_ident(src, segment, cursor);
    let value = crate::engine::language::lexer::tokenize(prefix)
        .iter()
        .any(|token| token.kind == crate::engine::language::lexer::TokenKind::Eq);
    Some((command, value.then_some(key)))
}

/// Function argument help shares the reference's arities and accepted contexts.
#[cfg(test)]
pub(crate) fn function_signature_help(
    src: &str,
    byte: u32,
    context: &str,
) -> Option<SignatureHelp> {
    let cursor = byte as usize;
    let tokens = crate::engine::language::macro_expand::scan_macro_tokens(src);
    let start = macro_expression_start(&tokens, byte).unwrap_or(0);
    let prefix = src.get(start..cursor)?;
    let (_, functions) = prefix_frames(prefix, prefix.len(), false);
    function_help(&functions, context)
}

#[derive(Default)]
struct FunctionFrames<'a> {
    frames: Vec<(Kind, Option<&'a str>, usize, usize)>,
    pending: Option<&'a str>,
    closed: Option<(Option<&'a str>, usize)>,
}

impl<'a> FunctionFrames<'a> {
    fn push(&mut self, atom: Atom<'a>) {
        match atom {
            Atom::Ident(name) => {
                self.pending = Some(name);
                self.closed = None;
            }
            Atom::Open(Kind::Paren) => {
                let (name, group) = self
                    .pending
                    .take()
                    .map(|name| (Some(name), 0))
                    .or_else(|| self.closed.take())
                    .unwrap_or((None, 0));
                self.frames.push((Kind::Paren, name, 0, group));
            }
            Atom::Open(kind) => {
                self.frames.push((kind, None, 0, 0));
                self.pending = None;
                self.closed = None;
            }
            Atom::Close(kind) => {
                self.closed = self
                    .frames
                    .iter()
                    .rposition(|frame| frame.0 == kind)
                    .and_then(|index| {
                        let (_, name, _, group) = self.frames.remove(index);
                        self.frames.truncate(index);
                        (kind == Kind::Paren).then_some((name, group + 1))
                    });
                self.pending = None;
            }
            Atom::Comma => {
                if let Some(frame) = self.frames.last_mut()
                    && frame.0 == Kind::Paren
                {
                    frame.2 += 1;
                }
                self.pending = None;
                self.closed = None;
            }
            Atom::Semi => {
                if !self
                    .frames
                    .iter()
                    .any(|frame| matches!(frame.0, Kind::Bracket | Kind::Brace))
                {
                    self.frames.clear();
                }
                self.pending = None;
                self.closed = None;
            }
            Atom::Other => {
                self.pending = None;
                self.closed = None;
            }
        }
    }
}

fn function_help(functions: &FunctionFrames<'_>, context: &str) -> Option<SignatureHelp> {
    let (_, name, argument, group) = functions
        .frames
        .iter()
        .rev()
        .find(|frame| frame.1.is_some())?;
    let name = (*name)?;
    let record = syntax::in_context(context, None, None).find(|record| {
        matches!(
            record.kind.as_str(),
            "function" | "operator" | "macro_function"
        ) && record.name.eq_ignore_ascii_case(name)
    })?;
    let signatures: Vec<_> = record
        .signatures
        .iter()
        .filter_map(|label| {
            let mut from = 0;
            for _ in 0..=*group {
                from = label[from..].find('(')? + from + 1;
            }
            let end = label[from..].find(')')? + from;
            let mut offset = from;
            let parameters: Vec<_> = label[from..end]
                .split(',')
                .filter(|value| !value.is_empty())
                .map(|value| {
                    let begin = offset + value.len() - value.trim_start().len();
                    let last = offset + value.trim_end().len();
                    offset += value.len() + 1;
                    ParameterInformation {
                        label: ParameterLabel::LabelOffsets([
                            utf16_len(&label[..begin]),
                            utf16_len(&label[..last]),
                        ]),
                        documentation: None,
                    }
                })
                .collect();
            Some(SignatureInformation {
                label: label.clone(),
                documentation: Some(Documentation::String(record.summary.clone())),
                active_parameter: (*argument < parameters.len()).then_some(*argument as u32),
                parameters: Some(parameters),
            })
        })
        .collect();
    if signatures.is_empty() {
        return None;
    }
    let active = signatures
        .iter()
        .position(|signature| signature.active_parameter.is_some())
        .unwrap_or(0) as u32;
    let active_parameter = signatures[active as usize].active_parameter;
    Some(SignatureHelp {
        signatures,
        active_signature: Some(active),
        active_parameter,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    // An unavailable function must not cause command fallback to walk the prefix again.
    #[test]
    fn combined_signature_walks_common_prefix_once() {
        for (source, context, label) in [
            (
                "var y; model; y=y(-1); end;\n// cursor",
                "model_expression",
                None,
            ),
            ("stoch_simul(order=", "top_level", Some("stoch_simul(")),
            (
                "stoch_simul(order=normcdf(1,",
                "expression",
                Some("normcdf("),
            ),
        ] {
            let tokens = crate::engine::language::macro_expand::scan_macro_tokens(source);
            PREFIX_WALKS.with(|walks| walks.set(0));
            let help = combined_signature_help(source, source.len() as u32, context, &tokens);
            assert_eq!(help.is_some(), label.is_some(), "{source}");
            if let Some(label) = label {
                assert!(
                    help.unwrap().signatures[0].label.starts_with(label),
                    "{source}"
                );
            }
            PREFIX_WALKS.with(|walks| assert_eq!(walks.get(), 1, "{source}"));
        }
    }

    // The shared lexical walk must keep each provider's delimiter/statement rules.
    #[test]
    fn combined_signature_matches_separate_providers_at_source_boundaries() {
        for (source, context) in [
            (
                "stoch_simul(graph_format=(pdf,fig), order={1,2}, irf=1);",
                "top_level",
            ),
            (
                "model; y=normcdf(y,0,1)+EXPECTATION(-1)(y); end;",
                "model_expression",
            ),
            ("stoch_simul(order=normcdf([1;2],(0),1));", "expression"),
            (
                "stoch_simul(datafile='é,(x)', order=1 /* (, */); // (\n",
                "top_level",
            ),
            (
                "@#define v = max([1,2],length([3;4]))\n",
                "macro_expression",
            ),
            (
                "stoch_simul(datafile='@{length([1,2])}');",
                "macro_expression",
            ),
            ("// @{length([1,2])}\n", "macro_expression"),
            ("stoch_simul(order=1, /* unmatched", "top_level"),
            (
                "model; y=normcdf(y,0,1]; end; stoch_simul(irf=1);",
                "model_expression",
            ),
        ] {
            let tokens = crate::engine::language::macro_expand::scan_macro_tokens(source);
            for byte in source
                .char_indices()
                .map(|(byte, _)| byte)
                .chain([source.len()])
            {
                let byte = byte as u32;
                let separate = function_signature_help(source, byte, context)
                    .or_else(|| signature_help(source, byte));
                assert_eq!(
                    combined_signature_help(source, byte, context, &tokens),
                    separate,
                    "{source} at {byte}"
                );
            }
        }
    }

    // Literal arguments stay eligible, including macro expressions inside quotes/comments.
    #[test]
    fn combined_signature_keeps_literal_and_macro_arguments() {
        for (marked, context, function, argument) in [
            (
                "model; y=normcdf('a,b|',0,1); end;",
                "model_expression",
                "normcdf",
                0,
            ),
            (
                "stoch_simul(datafile='a,b|');",
                "top_level",
                "stoch_simul",
                0,
            ),
            ("'@{length([1,2|])}'", "macro_expression", "length", 0),
            ("// @{max([1,2],[3;4|])}", "macro_expression", "max", 1),
            (
                "model; y=EXPECTATION(-1)(y|); end;",
                "model_expression",
                "EXPECTATION",
                0,
            ),
        ] {
            let byte = marked.find('|').unwrap() as u32;
            let source = marked.replace('|', "");
            let tokens = crate::engine::language::macro_expand::scan_macro_tokens(&source);
            let help = combined_signature_help(&source, byte, context, &tokens).unwrap();
            assert!(
                help.signatures[0]
                    .label
                    .to_ascii_lowercase()
                    .starts_with(&function.to_ascii_lowercase()),
                "{marked}: {help:?}"
            );
            if function == "stoch_simul" {
                let (_, Some(key)) = option_context(&source, byte).unwrap() else {
                    panic!("literal value must keep its option context");
                };
                assert_eq!(key, "datafile");
            } else {
                assert_eq!(help.active_parameter, Some(argument), "{marked}");
            }
        }
    }

    // Completion must distinguish an option key from its value after sharing frame state.
    #[test]
    fn option_context_keeps_key_value_and_block_boundaries() {
        for (source, expected) in [
            ("stoch_simul(ord", Some(("stoch_simul", None))),
            (
                "stoch_simul(order=normcdf(1,",
                Some(("stoch_simul", Some("order"))),
            ),
            ("model; stoch_simul(order=", None),
            (
                "model; y=1; end; stoch_simul(order=",
                Some(("stoch_simul", Some("order"))),
            ),
        ] {
            assert_eq!(
                option_context(source, source.len() as u32),
                expected.map(|(command, option)| (command.to_string(), option.map(str::to_string))),
                "{source}"
            );
        }
    }
}
