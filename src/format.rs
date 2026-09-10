//! Semantics-preserving formatter. Whitespace only; comments and `@#` stay verbatim.

use crate::lexer::{tokenize, TokenKind};

const BLOCK_OPENERS: &[&str] = &[
    "estimated_params_init",
    "estimated_params_bounds",
    "steady_state_model",
    "observation_trends",
    "ramsey_constraints",
    "estimated_params",
    "moment_calibration",
    "irf_calibration",
    "homotopy_setup",
    "optim_weights",
    "histval",
    "endval",
    "initval",
    "shocks",
    "verbatim",
    "model",
];

const UNSAFE_CHARS: &[char] = &['\'', '"', '[', ']', '{', '}', '@', '#', '%', ':', '\\', '!'];

/// Reformat whole-file text, or `None` to leave it unchanged.
pub fn format_text(text: &str, indent_unit: &str) -> Option<String> {
    if text.trim().is_empty() {
        return None;
    }
    let formatted = reformat(text, indent_unit)?;
    if formatted == text {
        return None;
    }
    if canonical(&formatted) != canonical(text) {
        return None;
    }
    Some(formatted)
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
    if text.trim().is_empty() {
        return None;
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
    if orig_lines.is_empty() {
        return None;
    }
    let start = start_line as usize;
    let end = (end_line as usize).min(orig_lines.len() - 1);
    if start > end {
        return None;
    }
    let depth = depth_before(&stripped_lines, start);
    let formatted = format_lines(
        &orig_lines[start..=end],
        &stripped_lines[start..=end],
        indent_unit,
        depth,
    )?;
    let original_slice = raw_lines[start..=end].join("\n");
    let mut replacement = formatted.join(eol);
    if raw_lines[end].ends_with('\r') {
        replacement.push('\r');
    }
    if replacement == original_slice {
        return None;
    }
    if canonical(&replacement) != canonical(&original_slice) {
        return None;
    }
    Some((start as u32, end as u32, replacement))
}

fn reformat(text: &str, indent_unit: &str) -> Option<String> {
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
    let mut out = format_lines(&orig_owned, &stripped_owned, indent_unit, 0)?;
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

fn structural_line(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

fn depth_before(stripped_lines: &[String], upto: usize) -> usize {
    let mut depth = 0usize;
    for stripped in stripped_lines.iter().take(upto) {
        let structural = structural_line(stripped);
        if is_end_line(structural) {
            depth = depth.saturating_sub(1);
        } else if is_opener(structural) {
            depth += 1;
        }
    }
    depth
}

fn format_lines(
    orig_lines: &[String],
    stripped_lines: &[String],
    indent_unit: &str,
    start_depth: usize,
) -> Option<Vec<String>> {
    if orig_lines.len() != stripped_lines.len() {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    let mut depth = start_depth;
    let mut assign_run: Vec<(usize, String, String)> = Vec::new();

    for (orig, stripped) in orig_lines.iter().zip(stripped_lines.iter()) {
        let code = stripped.trim_end();
        let structural = structural_line(stripped);

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

        if is_end_line(structural) {
            flush_run(&mut out, &mut assign_run);
            depth = depth.saturating_sub(1);
            out.push(format!("{}{}", indent_unit.repeat(depth), orig.trim()));
            continue;
        }

        let indent = indent_unit.repeat(depth);
        let code_end = char_len(code);
        let body = prefix_chars(orig, code_end);
        let trailing = suffix_chars(orig, code_end).trim();

        if !trailing.is_empty() && !comment_prefix(trailing) {
            flush_run(&mut out, &mut assign_run);
            out.push(format!("{}{}", indent, orig.trim()));
            if is_opener(structural) {
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
            if !is_opener(structural) {
                assign_run.push((line_index, lhs.to_string(), rest.to_string()));
            } else {
                flush_run(&mut out, &mut assign_run);
            }
        } else {
            flush_run(&mut out, &mut assign_run);
        }

        if is_opener(structural) {
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

fn is_end_line(structural: &str) -> bool {
    let s = structural.trim_start();
    let rest = match strip_prefix_ci(s, "end") {
        Some(r) => r,
        None => return false,
    };
    let rest = rest.trim_start();
    match rest.strip_prefix(';') {
        Some(tail) => tail.chars().all(char::is_whitespace),
        None => false,
    }
}

fn is_opener(structural: &str) -> bool {
    let mut s = structural.trim_start();
    let mut matched = false;
    for kw in BLOCK_OPENERS {
        if let Some(rest) = strip_prefix_ci(s, kw) {
            if word_boundary(rest) {
                s = rest;
                matched = true;
                break;
            }
        }
    }
    if !matched {
        return false;
    }
    s = s.trim_start();
    if let Some(inner) = s.strip_prefix('(') {
        let Some(close) = inner.find(')') else {
            return false;
        };
        s = inner[close + 1..].trim_start();
    }
    match s.strip_prefix(';') {
        Some(tail) => tail.chars().all(char::is_whitespace),
        None => false,
    }
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let plen = prefix.len();
    if s.len() >= plen && s.as_bytes()[..plen].eq_ignore_ascii_case(prefix.as_bytes()) {
        Some(&s[plen..])
    } else {
        None
    }
}

fn word_boundary(rest: &str) -> bool {
    match rest.chars().next() {
        None => true,
        Some(c) => !(c.is_ascii_alphanumeric() || c == '_'),
    }
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
        if let Some(end) = match_interp_atom(&chars, i) {
            out.push(chars[i..end].iter().collect());
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

fn match_interp_atom(chars: &[char], start: usize) -> Option<usize> {
    let mut word_end = start;
    while word_end < chars.len() && is_word(chars[word_end]) {
        word_end += 1;
    }
    for prefix_end in (start..=word_end).rev() {
        if let Some(end) = match_interp_groups(chars, prefix_end) {
            return Some(end);
        }
    }
    None
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
    if n > 0 {
        Some(i)
    } else {
        None
    }
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
    fn empty_and_ws_decline() {
        assert_eq!(format_text("", "\t"), None);
        assert_eq!(format_text("   \n", "\t"), None);
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
