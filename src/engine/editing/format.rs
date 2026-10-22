//! Semantics-preserving formatter. Whitespace only; comments and `@#` stay verbatim.

use serde_json::Value;

use crate::engine::language::lexer::{TokenKind, tokenize};

const UNSAFE_CHARS: &[char] = &['\'', '"', '[', ']', '{', '}', '@', '#', '%', ':', '\\', '!'];

#[derive(Clone, Copy, Default)]
struct SourceLine {
    preserve: bool,
    opens: bool,
    closes: bool,
}

/// Whole-file format result. `format_text` is `Some` only for [`FormatOutcome::Changed`].
pub enum FormatOutcome {
    Changed(String),
    Unchanged,
    Unsupported(&'static str),
}

pub(crate) enum RangeFormatOutcome {
    Changed(u32, u32, String),
    Unchanged,
    Unsupported(&'static str),
}

/// Same decision as [`format_text`], with unchanged and unsupported kept apart.
pub fn format_outcome(text: &str, indent_unit: &str) -> FormatOutcome {
    format_outcome_with_limit(text, indent_unit, Default::default())
}

pub(crate) fn format_outcome_with_limit(
    text: &str,
    indent_unit: &str,
    work_limit: crate::engine::macro_work::MacroWorkLimit,
) -> FormatOutcome {
    if text.trim().is_empty() {
        return FormatOutcome::Unchanged;
    }
    let Some(formatted) = reformat(text, indent_unit, work_limit) else {
        return FormatOutcome::Unsupported("formatter cannot reformat this file");
    };
    if formatted == text {
        return FormatOutcome::Unchanged;
    }
    if canonical(&formatted) != canonical(text) {
        return FormatOutcome::Unsupported("formatting would change more than whitespace");
    }
    FormatOutcome::Changed(formatted)
}

/// Reformat whole-file text, or `None` to leave it unchanged.
pub fn format_text(text: &str, indent_unit: &str) -> Option<String> {
    match format_outcome(text, indent_unit) {
        FormatOutcome::Changed(formatted) => Some(formatted),
        FormatOutcome::Unchanged | FormatOutcome::Unsupported(_) => None,
    }
}

/// LSP `formatIndent`: `tab`, or a whole number of spaces from 1 to 8.
pub fn parse_format_indent(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        let s = s.trim();
        if s.eq_ignore_ascii_case("tab") {
            return Some("\t".into());
        }
        if s.chars().all(|c| c.is_ascii_digit())
            && let Ok(n) = s.parse::<usize>()
            && (1..=8).contains(&n)
        {
            return Some(" ".repeat(n));
        }
        return None;
    }
    if let Some(n) = value.as_u64()
        && (1..=8).contains(&n)
    {
        return Some(" ".repeat(n as usize));
    }
    if let Some(n) = value.as_i64()
        && (1..=8).contains(&n)
    {
        return Some(" ".repeat(n as usize));
    }
    None
}

/// Format inclusive line range `[start_line, end_line]`.
///
/// Returns `(start_line, end_line, replacement)` for a whole-line edit, or `None`.
pub fn format_range(
    text: &str,
    start_line: u32,
    end_line: u32,
    indent_unit: &str,
) -> Option<(u32, u32, String)> {
    match format_range_outcome(text, start_line, end_line, indent_unit) {
        RangeFormatOutcome::Changed(start, end, text) => Some((start, end, text)),
        RangeFormatOutcome::Unchanged | RangeFormatOutcome::Unsupported(_) => None,
    }
}

pub(crate) fn format_range_outcome(
    text: &str,
    start_line: u32,
    end_line: u32,
    indent_unit: &str,
) -> RangeFormatOutcome {
    format_range_outcome_with_limit(text, start_line, end_line, indent_unit, Default::default())
}

pub(crate) fn format_range_outcome_with_limit(
    text: &str,
    start_line: u32,
    end_line: u32,
    indent_unit: &str,
    work_limit: crate::engine::macro_work::MacroWorkLimit,
) -> RangeFormatOutcome {
    if text.trim().is_empty() {
        return RangeFormatOutcome::Unchanged;
    }
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let raw_lines: Vec<&str> = text.split('\n').collect();
    let orig_lines: Vec<String> = raw_lines
        .iter()
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect();
    let stripped = blank_comments_macros(text);
    let stripped_lines: Vec<String> = stripped
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect();
    let start = start_line as usize;
    let end = (end_line as usize).min(orig_lines.len() - 1);
    if start > end {
        return RangeFormatOutcome::Unsupported(
            "The formatting range is outside the document or reversed.",
        );
    }
    let structure = source_lines(text, work_limit);
    let depth = depth_before(&structure, start);
    let Some(formatted) = format_lines(
        &orig_lines[start..=end],
        &stripped_lines[start..=end],
        &structure[start..=end],
        indent_unit,
        depth,
    ) else {
        return RangeFormatOutcome::Unsupported(
            "The formatter cannot preserve the selected source while changing whitespace.",
        );
    };
    let original_slice = raw_lines[start..=end].join("\n");
    let mut replacement = formatted.join(eol);
    if raw_lines[end].ends_with('\r') {
        replacement.push('\r');
    }
    if replacement == original_slice {
        return RangeFormatOutcome::Unchanged;
    }
    if canonical(&replacement) != canonical(&original_slice) {
        return RangeFormatOutcome::Unsupported(
            "Formatting would change more than whitespace in the selected source.",
        );
    }
    RangeFormatOutcome::Changed(start as u32, end as u32, replacement)
}

fn reformat(
    text: &str,
    indent_unit: &str,
    work_limit: crate::engine::macro_work::MacroWorkLimit,
) -> Option<String> {
    let eol = line_ending(text);
    let orig_lines = split_line_breaks(text);
    let normalised = normalise_line_breaks(text);
    let stripped = blank_comments_macros(&normalised);
    let stripped_lines: Vec<&str> = stripped.split('\n').collect();
    if orig_lines.len() != stripped_lines.len() {
        return None;
    }
    let orig_owned: Vec<String> = orig_lines.iter().map(|s| s.to_string()).collect();
    let stripped_owned: Vec<String> = stripped_lines.iter().map(|s| s.to_string()).collect();
    let structure = source_lines(&normalised, work_limit);
    let mut out = format_lines(&orig_owned, &stripped_owned, &structure, indent_unit, 0)?;
    while out.last().is_some_and(|s| s.is_empty()) {
        out.pop();
    }
    Some(out.join(eol) + eol)
}

fn line_ending(text: &str) -> &str {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            return "\r\n";
        }
        if bytes[i] == b'\r' {
            return "\r";
        }
        if bytes[i] == b'\n' {
            return "\n";
        }
        i += 1;
    }
    "\n"
}

fn split_line_breaks(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            out.push(&text[start..i]);
            i += 2;
            start = i;
        } else if bytes[i] == b'\r' || bytes[i] == b'\n' {
            out.push(&text[start..i]);
            i += 1;
            start = i;
        } else {
            i += 1;
        }
    }
    out.push(&text[start..]);
    out
}

fn normalise_line_breaks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            out.push('\n');
            i += 2;
        } else if bytes[i] == b'\r' {
            out.push('\n');
            i += 1;
        } else {
            // SAFETY: i is on a char boundary because we only skip ASCII CR/LF.
            let ch = text[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// Spaces separate arguments in MATLAB command syntax. The shared parser owns
/// native line boundaries, including continuations and macro execution. Preserve
/// whole physical lines when any part is native, verbatim, or opaque recovery.
/// Block transitions use the same parser's accepted token boundaries, including
/// multiline headers. Equal written macro copies produce one transition.
fn source_lines(
    text: &str,
    work_limit: crate::engine::macro_work::MacroWorkLimit,
) -> Vec<SourceLine> {
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(at, _)| at + 1));
    let model = crate::engine::language::parser::parse_with_budget(
        text,
        &mut crate::engine::language::macro_expr::MacroBudget::with_limit(work_limit, None),
    );
    let fallback;
    let structure = if model.macro_incomplete() {
        // Source presentation uses an inert parse when execution stops. This
        // does not create accepted language facts or execute a failed branch.
        let inert = crate::engine::analysis::folding::inert_source(text);
        fallback = crate::engine::language::parser::parse_expanded_controlled(
            &inert,
            tokenize(&inert),
            None,
            work_limit,
        )
        .0;
        &fallback
    } else {
        &model
    };
    let mut lines = vec![SourceLine::default(); starts.len()];
    let line_at = |offset: u32| starts.partition_point(|&at| at <= offset as usize) - 1;
    for statement in &structure.statements {
        if statement.kind != crate::engine::language::model::StatementKind::Block
            || statement.native
            || statement.name == "verbatim"
        {
            continue;
        }
        let Some(opener) = statement
            .opener_range
            .end
            .checked_sub(1)
            .and_then(|index| structure.expanded_tokens.get(index))
            .filter(|token| token.kind == TokenKind::Semi)
        else {
            continue;
        };
        let open_line = line_at(opener.span.start);
        lines[open_line].opens = true;
        if statement.complete
            && let Some(closer) = statement
                .token_range
                .end
                .checked_sub(2)
                .and_then(|index| structure.expanded_tokens.get(index))
            && closer.kind == TokenKind::Ident
            && closer.text(text).eq_ignore_ascii_case("end")
        {
            let close_line = line_at(closer.span.start);
            if close_line == open_line {
                lines[open_line].opens = false;
            } else {
                lines[close_line].closes = true;
            }
        }
    }
    // Difference marks avoid rescanning long spans repeated by macro execution.
    let mut marks = vec![0i64; starts.len() + 1];
    let mut spans: Vec<_> = model
        .execution_steps
        .iter()
        .filter_map(|step| match step {
            crate::engine::language::model::ExecutionStep::Opaque(span) => Some(*span),
            crate::engine::language::model::ExecutionStep::Statement(id) => {
                let statement = &model.statements[*id];
                (statement.native || statement.name == "verbatim").then_some(statement.span)
            }
        })
        .chain(
            model
                .ms_unparsed_spans
                .iter()
                .copied()
                .filter(|_| !model.macro_incomplete()),
        )
        .chain(model.inactive_macro_spans().iter().copied())
        .collect();
    if model.macro_incomplete() {
        // Failed expansion cannot prove block ownership. Preserve possible
        // command arguments even in text whose execution could not be read.
        let tokens = tokenize(text);
        for (index, pair) in tokens.windows(2).enumerate() {
            let head = &pair[0];
            let next = &pair[1];
            if head.kind != TokenKind::Ident
                || crate::engine::language::command_skip::is_pin_statement_keyword(head.text(text))
                || matches!(
                    next.kind,
                    TokenKind::Eq | TokenKind::LParen | TokenKind::Eof
                )
            {
                continue;
            }
            let gap = &text[head.span.end as usize..next.span.start as usize];
            let boundary = index == 0
                || matches!(tokens[index - 1].kind, TokenKind::Semi | TokenKind::Comma)
                || text[tokens[index - 1].span.end as usize..head.span.start as usize]
                    .contains('\n');
            if boundary
                && !gap.is_empty()
                && !gap.contains('\n')
                && gap.chars().all(char::is_whitespace)
            {
                spans.push(crate::engine::language::span::Span::new(
                    head.span.start as usize,
                    crate::engine::language::native_line::native_region_end(
                        text,
                        head.span.start as usize,
                    ),
                ));
            }
        }
    }
    for span in spans {
        let first = starts.partition_point(|&at| at <= span.start as usize) - 1;
        let last = starts.partition_point(|&at| at <= span.end.saturating_sub(1) as usize) - 1;
        if first <= last {
            marks[first] += 1;
            marks[last + 1] -= 1;
        }
    }
    let mut active = 0;
    for (line, mark) in lines.iter_mut().zip(&marks) {
        active += mark;
        line.preserve = active > 0;
    }
    lines
}

fn depth_before(structure: &[SourceLine], upto: usize) -> usize {
    let mut depth = 0usize;
    for line in structure.iter().take(upto) {
        if line.preserve {
            continue;
        }
        if line.closes {
            depth = depth.saturating_sub(1);
        }
        if line.opens {
            depth += 1;
        }
    }
    depth
}

fn format_lines(
    orig_lines: &[String],
    stripped_lines: &[String],
    structure: &[SourceLine],
    indent_unit: &str,
    start_depth: usize,
) -> Option<Vec<String>> {
    if orig_lines.len() != stripped_lines.len() || orig_lines.len() != structure.len() {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    let mut depth = start_depth;
    let mut assign_run: Vec<(usize, String, String)> = Vec::new();

    for ((orig, stripped), source_line) in orig_lines.iter().zip(stripped_lines).zip(structure) {
        if source_line.preserve {
            flush_run(&mut out, &mut assign_run);
            out.push(orig.clone());
            continue;
        }
        let code = stripped.trim_end();

        if code.trim().is_empty() {
            flush_run(&mut out, &mut assign_run);
            if orig.trim().is_empty() {
                if out.last().is_some_and(|s| s.is_empty()) {
                    continue;
                }
                out.push(String::new());
            } else {
                out.push(orig.trim_end().to_string());
            }
            continue;
        }

        if source_line.closes {
            flush_run(&mut out, &mut assign_run);
            depth = depth.saturating_sub(1);
            out.push(format!("{}{}", indent_unit.repeat(depth), orig.trim()));
            if source_line.opens {
                depth += 1;
            }
            continue;
        }

        let indent = indent_unit.repeat(depth);
        let code_end = char_len(code);
        let body = prefix_chars(orig, code_end);
        let trailing = suffix_chars(orig, code_end).trim();

        if !trailing.is_empty() && !comment_prefix(trailing) {
            flush_run(&mut out, &mut assign_run);
            out.push(format!("{}{}", indent, orig.trim()));
            if source_line.opens {
                depth += 1;
            }
            continue;
        }

        let no_midline_comment = prefix_chars(orig, code_end) == prefix_chars(stripped, code_end);
        let spaced = if no_midline_comment {
            space_line(body.trim())
        } else {
            None
        };
        let line_body = spaced.as_deref().unwrap_or_else(|| body.trim()).to_string();
        let mut line = format!("{indent}{line_body}");
        if !trailing.is_empty() {
            line.push(' ');
            line.push_str(trailing);
        }
        let line_index = out.len();
        out.push(line);

        let assign = if spaced.is_some() && trailing.is_empty() {
            assignment_parts(&line_body)
        } else {
            None
        };
        if let Some((lhs, rest)) = assign {
            if !source_line.opens {
                assign_run.push((line_index, lhs.to_string(), rest.to_string()));
            } else {
                flush_run(&mut out, &mut assign_run);
            }
        } else {
            flush_run(&mut out, &mut assign_run);
        }

        if source_line.opens {
            depth += 1;
        }
    }

    flush_run(&mut out, &mut assign_run);
    Some(out)
}

fn comment_prefix(trailing: &str) -> bool {
    trailing.starts_with("//") || trailing.starts_with('%') || trailing.starts_with("/*")
}

fn flush_run(out: &mut [String], assign_run: &mut Vec<(usize, String, String)>) {
    if assign_run.len() > 1 {
        let width = assign_run
            .iter()
            .map(|(_, lhs, _)| lhs.len())
            .max()
            .unwrap_or(0);
        for (idx, lhs, rest) in assign_run.iter() {
            let line = &out[*idx];
            let Some(at) = line.find(lhs) else {
                continue;
            };
            let prefix = &line[..at];
            out[*idx] = format!("{prefix}{:<width$} = {rest}", lhs);
        }
    }
    assign_run.clear();
}

fn space_line(code: &str) -> Option<String> {
    if code.chars().any(|c| UNSAFE_CHARS.contains(&c)) {
        return None;
    }
    let tokens = simple_tokens(code)?;
    Some(join_tokens(&tokens))
}

fn simple_tokens(code: &str) -> Option<Vec<String>> {
    let toks = tokenize(code);
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let tok = &toks[i];
        if tok.kind == TokenKind::Eof {
            break;
        }
        if !simple_kind(tok.kind) {
            return None;
        }
        if tok.kind == TokenKind::Star
            && i + 1 < toks.len()
            && toks[i + 1].kind == TokenKind::Star
            && tok.span.end == toks[i + 1].span.start
        {
            out.push("**".to_string());
            i += 2;
            continue;
        }
        out.push(tok.text(code).to_string());
        i += 1;
    }
    let joined: String = out.concat();
    let nospace: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if joined != nospace {
        return None;
    }
    Some(out)
}

fn simple_kind(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Ident
            | TokenKind::Number
            | TokenKind::Eq
            | TokenKind::EqEq
            | TokenKind::Ne
            | TokenKind::Lt
            | TokenKind::Gt
            | TokenKind::Le
            | TokenKind::Ge
            | TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Star
            | TokenKind::Slash
            | TokenKind::Caret
            | TokenKind::Comma
            | TokenKind::Semi
            | TokenKind::LParen
            | TokenKind::RParen
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Binop,
    Unary,
    LParen,
    RParen,
    Comma,
    Semi,
    Operand,
}

fn join_tokens(tokens: &[String]) -> String {
    let mut result = String::new();
    let mut prev: Option<Kind> = None;
    for tok in tokens {
        let kind = classify(tok, prev);
        result.push_str(separator(prev, kind));
        result.push_str(tok);
        prev = Some(kind);
    }
    result
}

fn classify(tok: &str, prev: Option<Kind>) -> Kind {
    if tok == "+" || tok == "-" {
        return match prev {
            None | Some(Kind::Binop) | Some(Kind::Unary) | Some(Kind::LParen)
            | Some(Kind::Comma) => Kind::Unary,
            _ => Kind::Binop,
        };
    }
    if matches!(
        tok,
        "*" | "/" | "^" | "=" | "<" | ">" | "<=" | ">=" | "==" | "!=" | "**"
    ) {
        return Kind::Binop;
    }
    match tok {
        "(" => Kind::LParen,
        ")" => Kind::RParen,
        "," => Kind::Comma,
        ";" => Kind::Semi,
        _ => Kind::Operand,
    }
}

fn separator(prev: Option<Kind>, kind: Kind) -> &'static str {
    let Some(prev) = prev else {
        return "";
    };
    if matches!(kind, Kind::RParen | Kind::Comma | Kind::Semi) {
        return "";
    }
    if matches!(prev, Kind::LParen | Kind::Unary) {
        return "";
    }
    if matches!(prev, Kind::Comma | Kind::Semi) {
        return " ";
    }
    if kind == Kind::LParen {
        return if prev == Kind::Binop { " " } else { "" };
    }
    if kind == Kind::Binop || prev == Kind::Binop {
        return " ";
    }
    " "
}

fn assignment_parts(line_body: &str) -> Option<(&str, &str)> {
    let ident_end = ident_prefix_len(line_body)?;
    let after_ident = &line_body[ident_end..];
    let after_ws = after_ident.trim_start();
    let rest = after_ws.strip_prefix('=')?;
    if rest.starts_with('=') {
        return None;
    }
    let rhs = rest.trim_start();
    if !rhs.chars().any(|c| !c.is_whitespace()) {
        return None;
    }
    if rhs.chars().last().is_some_and(char::is_whitespace) {
        return None;
    }
    Some((&line_body[..ident_end], rhs))
}

fn ident_prefix_len(s: &str) -> Option<usize> {
    let mut chars = s.char_indices();
    let (_, first) = chars.next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
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

fn canonical(text: &str) -> String {
    let normalised = normalise_line_breaks(text);
    let blanked = blank_comments_macros(&normalised);
    let tokens = canonical_tokens(&blanked);
    let atoms = interp_atoms(&normalised);
    format!("{}\u{1}{}", tokens.join("\u{0}"), atoms.join("\u{0}"))
}

fn canonical_tokens(blanked: &str) -> Vec<String> {
    let chars: Vec<char> = blanked.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        if let Some(end) = match_ident(&chars, i)
            .or_else(|| match_number(&chars, i))
            .or_else(|| match_multi_op(&chars, i))
        {
            out.push(chars[i..end].iter().collect());
            i = end;
        } else {
            out.push(chars[i].to_string());
            i += 1;
        }
    }
    out
}

fn match_ident(chars: &[char], i: usize) -> Option<usize> {
    let c = *chars.get(i)?;
    if !(c.is_ascii_alphabetic() || c == '_') {
        return None;
    }
    let mut j = i + 1;
    while j < chars.len() && is_word(chars[j]) {
        j += 1;
    }
    Some(j)
}

fn match_number(chars: &[char], i: usize) -> Option<usize> {
    if chars[i].is_ascii_digit() {
        let mut j = i;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
        if j < chars.len() && chars[j] == '.' {
            j += 1;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
        }
        return Some(match_exponent(chars, j));
    }
    if chars[i] == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
        let mut j = i + 1;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
        return Some(match_exponent(chars, j));
    }
    None
}

fn match_exponent(chars: &[char], i: usize) -> usize {
    if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
        let mut j = i + 1;
        if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
            j += 1;
        }
        if j < chars.len() && chars[j].is_ascii_digit() {
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            return j;
        }
    }
    i
}

fn match_multi_op(chars: &[char], i: usize) -> Option<usize> {
    if i + 1 >= chars.len() {
        return None;
    }
    let a = chars[i];
    let b = chars[i + 1];
    match (a, b) {
        ('=', '=')
        | ('!', '=')
        | ('<', '=')
        | ('>', '=')
        | ('&', '&')
        | ('|', '|')
        | ('*', '*') => Some(i + 2),
        _ => None,
    }
}

fn interp_atoms(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let start = i;
        while i < chars.len() && is_word(chars[i]) {
            i += 1;
        }
        // '@' cannot occur inside the word prefix. Check its end once instead
        // of rescanning every suffix of a long identifier or quoted value.
        if let Some(end) = match_interp_groups(&chars, i) {
            out.push(chars[start..end].iter().collect());
            i = end;
        } else if i == start {
            i += 1;
        }
    }
    out
}

fn match_interp_groups(chars: &[char], mut i: usize) -> Option<usize> {
    let mut n = 0usize;
    loop {
        if i + 1 >= chars.len() || chars[i] != '@' || chars[i + 1] != '{' {
            break;
        }
        let save = i;
        i += 2;
        while i < chars.len() && chars[i] != '}' && chars[i] != '\n' {
            i += 1;
        }
        if i >= chars.len() || chars[i] != '}' {
            i = save;
            break;
        }
        i += 1;
        while i < chars.len() && is_word(chars[i]) {
            i += 1;
        }
        n += 1;
    }
    if n > 0 { Some(i) } else { None }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn blank_comments_macros(text: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if c == '"' || c == '\'' {
            let quote = c;
            i += 1;
            while i < n && chars[i] != quote && chars[i] != '\n' {
                i += 1;
            }
            if i < n && chars[i] == quote {
                i += 1;
            }
            continue;
        }
        if c == '/' && i + 1 < n && chars[i + 1] == '/' {
            while i < n && chars[i] != '\n' {
                if chars[i] != '\r' {
                    chars[i] = ' ';
                }
                i += 1;
            }
            continue;
        }
        if c == '%' {
            while i < n && chars[i] != '\n' {
                if chars[i] != '\r' {
                    chars[i] = ' ';
                }
                i += 1;
            }
            continue;
        }
        if c == '/' && i + 1 < n && chars[i + 1] == '*' {
            chars[i] = ' ';
            chars[i + 1] = ' ';
            i += 2;
            while i + 1 < n && !(chars[i] == '*' && chars[i + 1] == '/') {
                if chars[i] != '\n' {
                    chars[i] = ' ';
                }
                i += 1;
            }
            if i + 1 < n {
                chars[i] = ' ';
                chars[i + 1] = ' ';
                i += 2;
            }
            continue;
        }
        if c == '@' && i + 1 < n && chars[i + 1] == '#' {
            let end = macro_directive_end(&chars, i);
            for ch in chars.iter_mut().take(end).skip(i) {
                if *ch != '\r' && *ch != '\n' {
                    *ch = ' ';
                }
            }
            i = end;
            continue;
        }
        i += 1;
    }
    let mut i = 0;
    let n = chars.len();
    while i < n {
        if chars[i] == '@' && i + 1 < n && chars[i + 1] == '{' {
            let start = i;
            i += 2;
            while i < n && chars[i] != '}' {
                i += 1;
            }
            if i < n && chars[i] == '}' {
                i += 1;
            }
            for ch in chars.iter_mut().take(i).skip(start) {
                if *ch != '\n' {
                    *ch = ' ';
                }
            }
            continue;
        }
        i += 1;
    }
    chars.into_iter().collect()
}

fn macro_directive_end(chars: &[char], start: usize) -> usize {
    let mut line_start = start;
    while line_start < chars.len() {
        let mut j = line_start;
        while j < chars.len() && chars[j] != '\n' && chars[j] != '\r' {
            j += 1;
        }
        if j >= chars.len() {
            return chars.len();
        }
        let line_end = j;
        if line_ends_with_backslash(&chars[line_start..line_end]) {
            if chars[j] == '\r' && j + 1 < chars.len() && chars[j + 1] == '\n' {
                line_start = j + 2;
            } else {
                line_start = j + 1;
            }
        } else {
            return line_end;
        }
    }
    chars.len()
}

fn line_ends_with_backslash(line: &[char]) -> bool {
    let mut k = line.len();
    while k > 0 && (line[k - 1] == ' ' || line[k - 1] == '\t') {
        k -= 1;
    }
    k > 0 && line[k - 1] == '\\'
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

fn prefix_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

fn suffix_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[i..],
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolation_atoms_keep_word_affixes_and_adjacent_groups() {
        // Guards a forward scan dropping an identifier prefix/suffix or joining
        // atoms across a word boundary, including Unicode word characters.
        let source = "plain _pre2@{i}尾@{j}_tail + @{k}@{l} + αβ_2@{n}ζ";
        assert_eq!(
            interp_atoms(source),
            ["_pre2@{i}尾@{j}_tail", "@{k}@{l}", "αβ_2@{n}ζ"]
        );
        for (joined, separated) in [
            ("pre@{i}", "pre @{i}"),
            ("@{i}tail", "@{i} tail"),
            ("@{i}@{j}", "@{i} @{j}"),
            ("αβ@{i}尾", "αβ @{i}尾"),
        ] {
            assert_ne!(canonical(joined), canonical(separated));
        }
    }

    #[test]
    fn interpolation_atoms_keep_completed_groups_before_malformed_text() {
        // Guards advancing past a rejected group and losing a later atom, or
        // changing the existing first-closing-brace treatment of nested text.
        for (source, expected) in [
            ("prefix@{missing\nnext@{ok}suffix", vec!["next@{ok}suffix"]),
            (
                "prefix@{a}@{missing\nnext@{b}",
                vec!["prefix@{a}", "next@{b}"],
            ),
            ("prefix@{outer@{inner}}tail", vec!["prefix@{outer@{inner}"]),
            ("prefix.@{i}suffix", vec!["@{i}suffix"]),
        ] {
            assert_eq!(interp_atoms(source), expected, "{source}");
        }
    }

    #[test]
    fn empty_and_ws_is_unchanged() {
        for text in ["", "   \n", " \n\t\n"] {
            assert_eq!(format_text(text, "\t"), None);
            assert!(matches!(
                format_outcome(text, "\t"),
                FormatOutcome::Unchanged
            ));
        }
    }

    #[test]
    fn join_lead_lag_and_call() {
        assert_eq!(
            space_line("y=exp(z)*c(-1)+e").as_deref(),
            Some("y = exp(z) * c(-1) + e")
        );
    }

    #[test]
    fn starstar_stays_one_token() {
        assert_eq!(space_line("a**b;").as_deref(), Some("a ** b;"));
        assert_eq!(canonical("a**b;"), canonical("a ** b;"));
    }
}
