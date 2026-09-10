//! E001 parse diagnostics. Token/AST walks; Python supplies messages, not regex.

use crate::diagnostic::{Diagnostic, Severity, TextEdit};
use crate::lexer::{tokenize, Token, TokenKind};
use crate::model::{Decl, Model, ParseIssueKind, ShocksSemiFamily};
use crate::span::{LineIndex, Span};

const BLOCK_OPENERS: &[&str] = &["model", "initval", "endval", "shocks", "steady_state_model"];

const DECL_KEYWORDS: &[&str] = &[
    "varexo_det",
    "var",
    "varexo",
    "parameters",
    "predetermined_variables",
];

const DECL_OR_BLOCK: &[&str] = &[
    "varexo_det",
    "var",
    "varexo",
    "parameters",
    "predetermined_variables",
    "model",
    "initval",
    "endval",
    "shocks",
    "steady_state_model",
];

const DYNARE_COMMANDS: &[&str] = &[
    "steady",
    "check",
    "resid",
    "stoch_simul",
    "simul",
    "estimation",
    "osr",
    "calib_smoother",
    "forecast",
    "identification",
    "dynasave",
    "dynatype",
    "model_diagnostics",
    "model_info",
    "perfect_foresight_setup",
    "perfect_foresight_solver",
];

const TERMINAL_COMMANDS: &[&str] = &[
    "stoch_simul",
    "estimation",
    "simul",
    "perfect_foresight_solver",
    "ramsey_policy",
    "discretionary_policy",
    "osr",
    "sensitivity",
    "dynare_sensitivity",
    "send_endogenous_variables_to_workspace",
];

const BUILTINS: &[&str] = &[
    "exp",
    "log",
    "ln",
    "log2",
    "log10",
    "sqrt",
    "cbrt",
    "abs",
    "sign",
    "sin",
    "cos",
    "tan",
    "asin",
    "acos",
    "atan",
    "sinh",
    "cosh",
    "tanh",
    "asinh",
    "acosh",
    "atanh",
    "floor",
    "ceil",
    "round",
    "min",
    "max",
    "normpdf",
    "normcdf",
    "norminv",
    "logncdf",
    "erf",
    "erfc",
    "inf",
    "nan",
    "steady_state",
    "expectation",
    "pac_expectation",
    "diff",
    "adl",
];

const EXPRESSION_OPERATOR_RESERVED: &[&str] = &["var_expectation", "pac_target_nonstationary"];

const RESERVED_BLOCK_KEYWORDS: &[&str] = &[
    "var",
    "varexo",
    "varexo_det",
    "parameters",
    "predetermined_variables",
    "model",
    "end",
    "initval",
    "endval",
    "shocks",
    "steady_state_model",
    "estimated_params",
    "varobs",
];

pub fn check_parse(model: &Model) -> Vec<Diagnostic> {
    let index = LineIndex::new(&model.source);
    let tokens = tokenize(&model.source);
    let mut out = Vec::new();
    out.extend(format_recorded_issues(model, &index));
    out.extend(invalid_ident_diags(model, &tokens, &index));
    out.extend(reserved_ident_diags(model, &index));
    out.extend(merged_equation_diags(model, &tokens, &index, &out));
    out.extend(merged_assignment_diags(model, &tokens, &index, &out));
    out.extend(unbalanced_paren_diags(model, &tokens));
    out
}

pub fn has_structural_error(model: &Model) -> bool {
    !check_parse(model).is_empty()
}

fn e001(span: Span, message: String, fix: Option<TextEdit>) -> Diagnostic {
    Diagnostic {
        span,
        severity: Severity::Error,
        code: "E001".to_string(),
        message,
        fix,
        tags: Vec::new(),
    }
}

fn ident_eq(tok: &Token, src: &str, name: &str) -> bool {
    tok.kind == TokenKind::Ident && tok.text(src).eq_ignore_ascii_case(name)
}

fn ident_in(tok: &Token, src: &str, names: &[&str]) -> bool {
    tok.kind == TokenKind::Ident && names.iter().any(|n| tok.text(src).eq_ignore_ascii_case(n))
}

fn skip_balanced(tokens: &[Token], mut i: usize, open: TokenKind, close: TokenKind) -> usize {
    if i >= tokens.len() || tokens[i].kind != open {
        return i;
    }
    let mut depth = 1;
    i += 1;
    while i < tokens.len() && depth > 0 {
        if tokens[i].kind == open {
            depth += 1;
        } else if tokens[i].kind == close {
            depth -= 1;
        }
        i += 1;
    }
    i
}

fn opener_at(tokens: &[Token], src: &str, i: usize) -> Option<(String, Span, usize)> {
    let tok = tokens.get(i)?;
    if tok.kind != TokenKind::Ident {
        return None;
    }
    let name = tok.text(src);
    let key = BLOCK_OPENERS
        .iter()
        .find(|k| name.eq_ignore_ascii_case(k))?
        .to_string();
    let mut j = i + 1;
    if tokens.get(j).is_some_and(|t| t.kind == TokenKind::LParen) {
        j = skip_balanced(tokens, j, TokenKind::LParen, TokenKind::RParen);
    }
    let semi = tokens.get(j)?;
    if semi.kind != TokenKind::Semi {
        return None;
    }
    Some((
        key,
        Span {
            start: tok.span.start,
            end: semi.span.end,
        },
        j,
    ))
}

fn is_end_semi(tokens: &[Token], src: &str, i: usize) -> bool {
    ident_eq(&tokens[i], src, "end") && tokens.get(i + 1).is_some_and(|t| t.kind == TokenKind::Semi)
}

fn complete_block_ranges(tokens: &[Token], src: &str) -> Vec<Span> {
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some((_, span, semi_i)) = opener_at(tokens, src, i) {
            let mut j = semi_i + 1;
            while j < tokens.len() {
                if is_end_semi(tokens, src, j) {
                    ranges.push(Span {
                        start: span.start,
                        end: tokens[j + 1].span.end,
                    });
                    break;
                }
                if opener_at(tokens, src, j).is_some() {
                    break;
                }
                j += 1;
            }
            i = semi_i + 1;
            continue;
        }
        i += 1;
    }
    ranges
}

fn inside_span(offset: u32, ranges: &[Span]) -> bool {
    ranges.iter().any(|r| offset >= r.start && offset < r.end)
}

fn last_eq_line(src: &str, index: &LineIndex, from: u32, to: u32) -> Option<u32> {
    let to = to.min(src.len() as u32);
    if from >= to {
        return None;
    }
    let body = mask_line_comments(&src[from as usize..to as usize]);
    let mut last = None;
    let mut start = 0;
    for (rel, _) in body.match_indices(';') {
        let stmt = body[start..rel].trim();
        start = rel + 1;
        if stmt.is_empty() || !stmt.chars().any(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        if let Some(first) = leading_ident(stmt) {
            if DYNARE_COMMANDS
                .iter()
                .any(|c| first.eq_ignore_ascii_case(c))
            {
                continue;
            }
        }
        let semi_abs = from + rel as u32;
        last = Some(index.position(src, semi_abs).line + 1);
    }
    last
}

fn mask_line_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for line in s.split_inclusive('\n') {
        let (content, had_nl) = match line.strip_suffix('\n') {
            Some(c) => (c, true),
            None => (line, false),
        };
        let cut = content
            .find("//")
            .or_else(|| content.find('%'))
            .unwrap_or(content.len());
        out.push_str(&content[..cut]);
        out.extend(std::iter::repeat_n(' ', content[cut..].len()));
        if had_nl {
            out.push('\n');
        }
    }
    out
}

fn leading_ident(s: &str) -> Option<&str> {
    let s = s.trim_start();
    let mut end = 0;
    for (i, c) in s.char_indices() {
        if i == 0 {
            if !(c.is_ascii_alphabetic() || c == '_') {
                return None;
            }
            end = i + c.len_utf8();
        } else if c.is_ascii_alphanumeric() || c == '_' {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    if end == 0 {
        None
    } else {
        Some(&s[..end])
    }
}

fn format_recorded_issues(model: &Model, index: &LineIndex) -> Vec<Diagnostic> {
    let src = &model.source;
    let mut out = Vec::new();
    for issue in &model.parse_issues {
        match &issue.kind {
            ParseIssueKind::MissingEnd {
                keyword,
                next_block_label,
                insert_offset,
                ..
            } => {
                let last = last_eq_line(src, index, issue.span.end, *insert_offset);
                let start_line = index.position(src, issue.span.start).line + 1;
                let insert_line = index.position(src, *insert_offset).line + 1;
                let msg = if let (Some(label), Some(last_eq)) = (next_block_label.as_deref(), last)
                {
                    format!(
                        "Missing 'end;' for '{keyword}' block (line {start_line}). \
                         Fix: add a new line containing only 'end;' between \
                         line {last_eq} and line {insert_line} (before '{label};')."
                    )
                } else if let Some(label) = next_block_label.as_deref() {
                    format!(
                        "Missing 'end;' for '{keyword}' block (line {start_line}). \
                         Fix: add a new line containing only 'end;' before \
                         '{label};' on line {insert_line}."
                    )
                } else {
                    format!(
                        "Missing 'end;' for '{keyword}' block (line {start_line}). \
                         Fix: add 'end;' on its own line before line {insert_line}."
                    )
                };
                let fix_line = last
                    .unwrap_or(insert_line.saturating_sub(1))
                    .saturating_sub(1);
                out.push(e001(
                    issue.span,
                    msg,
                    Some(TextEdit {
                        start_line: fix_line,
                        start_char: 999_999,
                        end_line: fix_line,
                        end_char: 999_999,
                        new_text: "\nend;".to_string(),
                    }),
                ));
            }
            ParseIssueKind::MissingDeclSemi {
                keyword,
                next_is_assign,
                next_span,
            } => {
                if let Some(next_span) = next_span {
                    out.push(decl_semi_diagnostic(
                        src,
                        index,
                        keyword,
                        issue.span.start,
                        issue.span.end,
                        *next_is_assign,
                        *next_span,
                    ));
                } else {
                    let lines: Vec<&str> = src.split('\n').collect();
                    let mut fix_line = lines.len().saturating_sub(1) as u32;
                    while fix_line > 0 {
                        let t = lines[fix_line as usize].trim();
                        if t.is_empty() || t.starts_with("@#") {
                            fix_line -= 1;
                            continue;
                        }
                        break;
                    }
                    let code = strip_line_comment(lines[fix_line as usize]);
                    out.push(e001(
                        issue.span,
                        format!(
                            "Declaration '{keyword}' is missing its terminating semicolon. \
                             Add ';' after the last variable name in this declaration."
                        ),
                        Some(TextEdit {
                            start_line: fix_line,
                            start_char: code.chars().count() as u32,
                            end_line: fix_line,
                            end_char: code.chars().count() as u32,
                            new_text: ";".to_string(),
                        }),
                    ));
                }
            }
            ParseIssueKind::MissingAssignSemi { name } => {
                let start = index.position(src, issue.span.start);
                let line = src.split('\n').nth(start.line as usize).unwrap_or("");
                let code = strip_line_comment(line);
                let fix_char = code.chars().count() as u32;
                out.push(e001(
                    issue.span,
                    format!(
                        "Parameter assignment '{name}' is missing its terminating semicolon. \
                         Fix: add ';' at the end of line {}.",
                        start.line + 1
                    ),
                    Some(TextEdit {
                        start_line: start.line,
                        start_char: fix_char,
                        end_line: start.line,
                        end_char: fix_char,
                        new_text: ";".to_string(),
                    }),
                ));
            }
            ParseIssueKind::MissingFinalSemi {
                keyword,
                body_code_end,
            } => {
                let fix_pos = index.position(src, *body_code_end);
                out.push(e001(
                    issue.span,
                    format!(
                        "Statement in '{keyword}' block is missing its terminating semicolon \
                         before 'end;'. Fix: add ';' at the end of line {}.",
                        fix_pos.line + 1
                    ),
                    Some(TextEdit {
                        start_line: fix_pos.line,
                        start_char: fix_pos.character,
                        end_line: fix_pos.line,
                        end_char: fix_pos.character,
                        new_text: ";".to_string(),
                    }),
                ));
            }
            ParseIssueKind::KeywordTypo { found, correct } => {
                let start = index.position(src, issue.span.start);
                let end = index.position(src, issue.span.end);
                out.push(e001(
                    issue.span,
                    format!(
                        "Possible misspelling of '{correct}' keyword: '{found}'. \
                         Fix: replace '{found}' with '{correct}'."
                    ),
                    Some(TextEdit {
                        start_line: start.line,
                        start_char: start.character,
                        end_line: end.line,
                        end_char: end.character,
                        new_text: correct.clone(),
                    }),
                ));
            }
            ParseIssueKind::MissingShocksSemi {
                family,
                label,
                fix_start,
                fix_end,
            } => {
                let fix_start_pos = index.position(src, *fix_start);
                let fix_end_pos = index.position(src, *fix_end);
                let (msg, new_text) = match family {
                    ShocksSemiFamily::BeforeKeyword => (
                        format!(
                            "Missing semicolon in shocks block before '{label}'. \
                             Fix: add ';' before '{label}' on line {}.",
                            fix_end_pos.line + 1
                        ),
                        "; ".to_string(),
                    ),
                    ShocksSemiFamily::AfterVar => (
                        format!(
                            "Missing semicolon after 'var {label}' in shocks block. \
                             Fix: add ';' after '{label}' on line {}.",
                            fix_end_pos.line + 1
                        ),
                        ";".to_string(),
                    ),
                    ShocksSemiFamily::EndOfStmt => (
                        format!(
                            "Missing semicolon at end of '{label}' shock statement. \
                             Fix: add ';' at the end of line {}.",
                            fix_end_pos.line + 1
                        ),
                        ";".to_string(),
                    ),
                };
                out.push(e001(
                    issue.span,
                    msg,
                    Some(TextEdit {
                        start_line: fix_start_pos.line,
                        start_char: fix_start_pos.character,
                        end_line: fix_end_pos.line,
                        end_char: fix_end_pos.character,
                        new_text,
                    }),
                ));
            }
        }
    }
    out
}

fn strip_line_comment(line: &str) -> &str {
    if let Some(i) = line.find("//") {
        return line[..i].trim_end();
    }
    if let Some(i) = line.find('%') {
        return line[..i].trim_end();
    }
    line.trim_end()
}

fn last_ident_in(text: &str) -> Option<&str> {
    let mut last = None;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            last = Some(&text[start..i]);
        } else {
            i += 1;
        }
    }
    last
}

fn decl_semi_diagnostic(
    src: &str,
    index: &LineIndex,
    kw: &str,
    kw_start: u32,
    range_end: u32,
    is_assign: bool,
    next_span: Span,
) -> Diagnostic {
    let next_pos = index.position(src, next_span.start);
    let next_line_1 = next_pos.line + 1;
    let lines: Vec<&str> = src.split('\n').collect();
    let mut insert_line = next_line_1.saturating_sub(1);
    let mut last_var = String::new();
    while insert_line > 0 {
        let idx = (insert_line - 1) as usize;
        if idx >= lines.len() {
            break;
        }
        let line_text = lines[idx].trim();
        if line_text.starts_with("@#") {
            insert_line -= 1;
            continue;
        }
        if line_text.is_empty() {
            insert_line -= 1;
            continue;
        }
        let no_comment = strip_line_comment(line_text).trim();
        if no_comment.is_empty() {
            insert_line -= 1;
            continue;
        }
        if let Some(id) = last_ident_in(no_comment) {
            last_var = id.to_string();
        }
        break;
    }
    let next_label = if is_assign {
        "assignment".to_string()
    } else {
        format!(
            "'{}' declaration",
            src[next_span.start as usize..next_span.end as usize].trim()
        )
    };
    let mut msg = format!(
        "Declaration '{kw}' appears to be missing its terminating semicolon \
         (the next {next_label} starts before a ';' is found). "
    );
    if last_var.is_empty() {
        msg.push_str(&format!(
            "Add ';' after the last variable name in this {kw} declaration."
        ));
    } else {
        msg.push_str(&format!(
            "Fix: add ';' at the end of line {insert_line} (after '{last_var}')."
        ));
    }
    let mut fix_line_0 = insert_line.saturating_sub(1);
    if fix_line_0 >= lines.len() as u32 {
        fix_line_0 = lines.len().saturating_sub(1) as u32;
    }
    let fix_line_text = lines.get(fix_line_0 as usize).copied().unwrap_or("");
    let fix = if next_pos.line == fix_line_0 {
        let mut start_char = next_pos.character as i32;
        if is_assign {
            // Same-line assignment: only auto-fix when a prior decl name exists.
            start_char = -1;
        }
        if start_char >= 0 {
            let mut sc = start_char as usize;
            let bytes = fix_line_text.as_bytes();
            while sc > 0 && matches!(bytes.get(sc - 1), Some(b' ' | b'\t')) {
                sc -= 1;
            }
            Some(TextEdit {
                start_line: fix_line_0,
                start_char: sc as u32,
                end_line: fix_line_0,
                end_char: sc as u32,
                new_text: ";".to_string(),
            })
        } else {
            None
        }
    } else {
        let code = strip_line_comment(fix_line_text);
        let sc = code.chars().count() as u32;
        Some(TextEdit {
            start_line: fix_line_0,
            start_char: sc,
            end_line: fix_line_0,
            end_char: sc,
            new_text: ";".to_string(),
        })
    };
    e001(
        Span {
            start: kw_start,
            end: range_end,
        },
        msg,
        fix,
    )
}

fn trailing_code_line(tokens: &[Token], src: &str, index: &LineIndex) -> Option<u32> {
    for tok in tokens {
        if ident_in(tok, src, TERMINAL_COMMANDS) {
            return Some(index.position(src, tok.span.start).line);
        }
    }
    None
}

fn looks_like_matlab(rhs: &str) -> bool {
    rhs.contains('\'')
        || rhs.contains('"')
        || rhs.contains('[')
        || rhs.contains(']')
        || rhs.contains('{')
        || rhs.contains('}')
        || rhs
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .any(|w| matches!(w, "M_" | "oo_" | "options_"))
}

fn invalid_ident_diags(model: &Model, tokens: &[Token], index: &LineIndex) -> Vec<Diagnostic> {
    let _ = index;
    let src = &model.source;
    let blocks = complete_block_ranges(tokens, src);
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        if inside_span(tok.span.start, &blocks) || !ident_in(tok, src, DECL_KEYWORDS) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        if tokens.get(j).is_some_and(|t| t.kind == TokenKind::LParen) {
            j = skip_balanced(tokens, j, TokenKind::LParen, TokenKind::RParen);
        }
        let body_start_i = j;
        let mut k = j;
        let mut saw_inner_kw = false;
        while k < tokens.len()
            && tokens[k].kind != TokenKind::Semi
            && tokens[k].kind != TokenKind::Eof
        {
            if tokens[k].kind == TokenKind::Latex {
                k += 1;
                continue;
            }
            if tokens[k].kind == TokenKind::LParen {
                k = skip_balanced(tokens, k, TokenKind::LParen, TokenKind::RParen);
                continue;
            }
            if ident_in(&tokens[k], src, DECL_OR_BLOCK) {
                saw_inner_kw = true;
                break;
            }
            k += 1;
        }
        if saw_inner_kw || k >= tokens.len() || tokens[k].kind != TokenKind::Semi {
            i += 1;
            continue;
        }
        let mut kept: Vec<usize> = Vec::new();
        let mut t = body_start_i;
        while t < k {
            if tokens[t].kind == TokenKind::Latex {
                t += 1;
                continue;
            }
            if tokens[t].kind == TokenKind::LParen {
                t = skip_balanced(tokens, t, TokenKind::LParen, TokenKind::RParen);
                continue;
            }
            if tokens[t].kind == TokenKind::Comma {
                flush_invalid_runs(src, tokens, &kept, &mut out);
                kept.clear();
                t += 1;
                continue;
            }
            kept.push(t);
            t += 1;
        }
        flush_invalid_runs(src, tokens, &kept, &mut out);
        i = k + 1;
    }
    out
}

fn is_dynare_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic() && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn flush_invalid_runs(src: &str, tokens: &[Token], kept: &[usize], out: &mut Vec<Diagnostic>) {
    if kept.is_empty() {
        return;
    }
    let mut run: Vec<usize> = vec![kept[0]];
    for &i in &kept[1..] {
        let prev = &tokens[*run.last().unwrap()];
        let cur = &tokens[i];
        let between = &src[prev.span.end as usize..cur.span.start as usize];
        if between.chars().any(char::is_whitespace) {
            push_invalid_run(src, tokens, &run, out);
            run = vec![i];
        } else {
            run.push(i);
        }
    }
    push_invalid_run(src, tokens, &run, out);
}

fn push_invalid_run(src: &str, tokens: &[Token], run: &[usize], out: &mut Vec<Diagnostic>) {
    if run.is_empty() {
        return;
    }
    let start = tokens[run[0]].span.start;
    let end = tokens[*run.last().unwrap()].span.end;
    let token = &src[start as usize..end as usize];
    if token.contains("@{") {
        return;
    }
    if !token.chars().any(|c| c.is_ascii_alphabetic() || c == '_') {
        return;
    }
    if is_dynare_ident(token) {
        return;
    }
    out.push(e001(
        Span { start, end },
        format!(
            "Invalid Dynare identifier '{token}'. \
             Identifiers must start with a letter and contain only \
             letters, digits, and underscores."
        ),
        None,
    ));
}

pub(crate) fn reserved_reason(name: &str) -> Option<&'static str> {
    let lowered = name.to_ascii_lowercase();
    if BUILTINS.iter().any(|b| *b == lowered) {
        return Some("Dynare built-in function or operator");
    }
    if EXPRESSION_OPERATOR_RESERVED.iter().any(|b| *b == lowered) {
        return Some("Dynare reserved model-expression operator");
    }
    if DYNARE_COMMANDS.iter().any(|b| *b == lowered)
        || RESERVED_BLOCK_KEYWORDS.iter().any(|b| *b == lowered)
    {
        return Some("Dynare command or block keyword");
    }
    None
}

fn reserved_ident_diags(model: &Model, index: &LineIndex) -> Vec<Diagnostic> {
    let src = &model.source;
    let mut seen = Vec::new();
    let mut out = Vec::new();
    let decls: Vec<&Decl> = model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.deterministic_exogenous)
        .chain(&model.parameters)
        .chain(&model.predetermined)
        .collect();
    for decl in decls {
        let name = model.name(decl.name);
        let Some(reason) = reserved_reason(name) else {
            continue;
        };
        let key = name.to_ascii_lowercase();
        if seen.iter().any(|s: &String| s == &key) {
            continue;
        }
        let pos = index.position(src, decl.span.start);
        let line = src.split('\n').nth(pos.line as usize).unwrap_or("");
        let leading = line.len() - line.trim_start().len();
        if pos.character == leading as u32 {
            continue;
        }
        seen.push(key);
        out.push(e001(
            decl.span,
            format!(
                "Invalid Dynare identifier '{name}': reserved {}. Choose a different name.",
                reason.to_ascii_lowercase()
            ),
            None,
        ));
    }
    out
}

fn tokens_in_span(tokens: &[Token], span: Span) -> Vec<&Token> {
    tokens
        .iter()
        .filter(|t| {
            t.span.start >= span.start && t.span.end <= span.end && t.kind != TokenKind::Eof
        })
        .collect()
}

fn skip_tag_tokens(toks: &[&Token], mut i: usize) -> usize {
    if i >= toks.len() || toks[i].kind != TokenKind::LBrack {
        return i;
    }
    i += 1;
    while i < toks.len() && toks[i].kind != TokenKind::RBrack {
        i += 1;
    }
    if i < toks.len() {
        i + 1
    } else {
        i
    }
}

fn standalone_eq_indices(toks: &[&Token]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        if toks[i].kind == TokenKind::LBrack {
            i = skip_tag_tokens(toks, i);
            continue;
        }
        if toks[i].kind == TokenKind::Eq {
            out.push(i);
        }
        i += 1;
    }
    out
}

fn merged_equation_diags(
    model: &Model,
    tokens: &[Token],
    index: &LineIndex,
    already: &[Diagnostic],
) -> Vec<Diagnostic> {
    let src = &model.source;
    let mut out = Vec::new();
    for eq in &model.equations {
        if eq.text.trim_start().starts_with('#') {
            continue;
        }
        if already.iter().any(|d| {
            d.message.contains("missing its terminating semicolon")
                && d.span.start <= eq.span.end
                && eq.span.start <= d.span.end
        }) {
            continue;
        }
        let toks = tokens_in_span(tokens, eq.span);
        let eqs = standalone_eq_indices(&toks);
        if eqs.len() < 2 {
            continue;
        }
        let second = eqs[1];
        let mut k = second;
        while k > 0 && !matches!(toks[k - 1].kind, TokenKind::Ident) {
            // walk left skipping non-idents; stop at first ident
            k -= 1;
            if toks[k].kind == TokenKind::Ident {
                break;
            }
        }
        // Walk left from second '=' over whitespace (trivia is already skipped).
        let mut ident_i = None;
        let mut j = second;
        while j > 0 {
            j -= 1;
            if toks[j].kind == TokenKind::Ident {
                ident_i = Some(j);
                break;
            }
            if matches!(
                toks[j].kind,
                TokenKind::RParen | TokenKind::Number | TokenKind::RBrack
            ) {
                break;
            }
        }
        let split_var = ident_i.map(|ii| toks[ii].text(src).to_string());
        let mut fix = None;
        if let Some(ii) = ident_i {
            let split_pos = index.position(src, toks[ii].span.start);
            let line = src.split('\n').nth(split_pos.line as usize).unwrap_or("");
            let prefix = {
                let mut end = 0;
                for (n, c) in line.chars().enumerate() {
                    if n as u32 >= split_pos.character {
                        break;
                    }
                    end += c.len_utf8();
                }
                &line[..end]
            };
            let eq_start_line = index.position(src, eq.span.start).line;
            if split_pos.line > eq_start_line && prefix.trim().is_empty() {
                let mut prev = split_pos.line.saturating_sub(1);
                let lines: Vec<&str> = src.split('\n').collect();
                while prev > eq_start_line
                    && lines
                        .get(prev as usize)
                        .is_some_and(|l| strip_line_comment(l).trim().is_empty())
                {
                    prev -= 1;
                }
                let prev_char = strip_line_comment(lines.get(prev as usize).copied().unwrap_or(""))
                    .chars()
                    .count() as u32;
                // Python uses rstrip of comment-stripped line, which keeps trailing code.
                let prev_char = lines
                    .get(prev as usize)
                    .map(|l| strip_line_comment(l).chars().count() as u32)
                    .unwrap_or(prev_char);
                fix = Some(TextEdit {
                    start_line: prev,
                    start_char: prev_char,
                    end_line: prev,
                    end_char: prev_char,
                    new_text: ";".to_string(),
                });
            } else {
                fix = Some(TextEdit {
                    start_line: split_pos.line,
                    start_char: split_pos.character,
                    end_line: split_pos.line,
                    end_char: split_pos.character,
                    new_text: ";\n".to_string(),
                });
            }
        }
        let mut msg =
            "Equation appears to contain multiple equations merged due to a missing semicolon."
                .to_string();
        if let Some(var) = &split_var {
            msg.push_str(&format!(
                " It looks like '{var} = ...' should be a separate equation. Fix: add ';' before '{var}'."
            ));
        } else {
            msg.push_str(" Fix: add ';' between the two equations.");
        }
        out.push(e001(eq.span, msg, fix));
    }
    out
}

fn merged_assignment_diags(
    model: &Model,
    tokens: &[Token],
    index: &LineIndex,
    already: &[Diagnostic],
) -> Vec<Diagnostic> {
    let src = &model.source;
    let trailing = trailing_code_line(tokens, src, index);
    let mut out = Vec::new();

    let skip_span = |span: Span| {
        already.iter().any(|d| {
            d.message.contains("missing its terminating semicolon")
                && d.span.start <= span.end
                && span.start <= d.span.end
        })
    };

    let mut consider = |span: Span, name: &str, context: &str| {
        if skip_span(span) {
            return;
        }
        if context == "top-level" {
            if trailing.is_some_and(|t| index.position(src, span.start).line > t) {
                return;
            }
            let seg = &src[span.start as usize..span.end as usize];
            if looks_like_matlab(seg) {
                return;
            }
        }
        let toks = tokens_in_span(tokens, span);
        let mut lhs_hits: Vec<usize> = Vec::new();
        for (i, tok) in toks.iter().enumerate() {
            if tok.kind == TokenKind::Ident
                && toks.get(i + 1).is_some_and(|n| n.kind == TokenKind::Eq)
            {
                lhs_hits.push(i);
            }
        }
        if lhs_hits.len() < 2 {
            return;
        }
        let second = lhs_hits[1];
        let second_name = toks[second].text(src);
        let split_pos = index.position(src, toks[second].span.start);
        let mut fix_start = toks[second].span.start;
        while fix_start > span.start {
            let b = src.as_bytes()[(fix_start - 1) as usize];
            if b == b' ' || b == b'\t' {
                fix_start -= 1;
            } else {
                break;
            }
        }
        let fix_start_pos = index.position(src, fix_start);
        let message = if context == "top-level" {
            format!(
                "Parameter/helper assignment '{name}' appears to contain \
                 multiple assignments merged due to a missing semicolon. \
                 Fix: add ';' before '{second_name} = ...'."
            )
        } else {
            format!(
                "Statement in '{context}' appears to contain multiple assignments \
                 merged due to a missing semicolon. \
                 Fix: add ';' before '{second_name} = ...'."
            )
        };
        out.push(e001(
            span,
            message,
            Some(TextEdit {
                start_line: fix_start_pos.line,
                start_char: fix_start_pos.character,
                end_line: split_pos.line,
                end_char: split_pos.character,
                new_text: "; ".to_string(),
            }),
        ));
    };

    for a in model
        .param_assignments
        .iter()
        .chain(&model.helper_assignments)
    {
        consider(a.span, model.name(a.name), "top-level");
    }
    for a in &model.initval {
        consider(a.span, model.name(a.name), "initval");
    }
    for a in &model.endval {
        consider(a.span, model.name(a.name), "endval");
    }
    for eq in &model.steady_state_equations {
        if eq.text.trim_start().starts_with('#') {
            continue;
        }
        let lhs = if eq.lhs.is_empty() {
            eq.text.split('=').next().unwrap_or("").trim()
        } else {
            eq.lhs.as_str()
        };
        consider(eq.span, lhs, "steady_state_model");
    }
    out
}

fn unbalanced_paren_diags(model: &Model, tokens: &[Token]) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for eq in &model.equations {
        let toks = tokens_in_span(tokens, eq.span);
        let mut depth = 0i32;
        let mut orphan = false;
        let mut i = 0;
        while i < toks.len() {
            if toks[i].kind == TokenKind::LBrack {
                i = skip_tag_tokens(&toks, i);
                continue;
            }
            match toks[i].kind {
                TokenKind::LParen => depth += 1,
                TokenKind::RParen => {
                    depth -= 1;
                    if depth < 0 {
                        orphan = true;
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        if orphan || depth != 0 {
            let unmatched = if orphan { "')'" } else { "'('" };
            out.push(e001(
                eq.span,
                format!("Unbalanced parentheses in equation: unmatched {unmatched}."),
                None,
            ));
        }
    }
    out
}
