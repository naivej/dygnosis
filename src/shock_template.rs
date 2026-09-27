//! Explicit code actions: commented stochastic and deterministic shocks templates.

use std::collections::HashSet;

use crate::lexer::{tokenize, Token, TokenKind};
use crate::model::{Model, ShockBlockKind, SurgeryKind};

/// One name per ordinary aggregate `varexo`, in declaration order.
/// `varexo_det` and `varexo(heterogeneity=…)` are left out. A repeated name is listed once.
pub(crate) fn aggregate_varexo_names(model: &Model) -> Vec<String> {
    aggregate_names(model, false)
}

/// Ordinary aggregate exogenous names for deterministic shocks, including `varexo_det`.
pub(crate) fn aggregate_deterministic_names(model: &Model) -> Vec<String> {
    aggregate_names(model, true)
}

fn aggregate_names(model: &Model, include_deterministic: bool) -> Vec<String> {
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for decl in &model.exogenous {
        if decl.heterogeneity.is_some() {
            continue;
        }
        let removed_endogenous = model
            .surgery_exits
            .iter()
            .any(|exit| exit.name == decl.name && exit.kind == SurgeryKind::Exogenous);
        if removed_endogenous {
            continue;
        }
        let deterministic = model
            .deterministic_exogenous
            .iter()
            .any(|det| det.name == decl.name && det.span == decl.span);
        if deterministic && !include_deterministic {
            continue;
        }
        let name = model.name(decl.name).to_string();
        if name.is_empty() || !seen.insert(name.clone()) {
            continue;
        }
        names.push(name);
    }
    names
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TemplateKind {
    Stochastic,
    Deterministic,
}

impl TemplateKind {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Stochastic => "Insert stochastic shocks template",
            Self::Deterministic => "Insert deterministic shocks template",
        }
    }
}

/// Command context selects one form when clear, and both when mixed or unspecified.
pub(crate) fn available_kinds(model: &Model) -> Vec<TemplateKind> {
    let stochastic = model.is_stochastic_context();
    let deterministic = model.is_pf_solver_context()
        || model.perfect_foresight_setup_span.is_some()
        || model.pfee_setup_span.is_some();
    match (stochastic, deterministic) {
        (true, false) => vec![TemplateKind::Stochastic],
        (false, true) => vec![TemplateKind::Deterministic],
        _ => vec![TemplateKind::Stochastic, TemplateKind::Deterministic],
    }
}

/// A plain `shocks` block, including `shocks(overwrite)`.
/// `shocks(heterogeneity=…)`, `mshocks`, surprise, learnt, and heteroskedastic blocks are not this.
pub(crate) fn has_ordinary_shocks(model: &Model) -> bool {
    model
        .shock_blocks
        .iter()
        .any(|block| block.kind == ShockBlockKind::Regular)
}

pub(crate) struct Insertion {
    pub byte: u32,
    pub new_text: String,
}

/// Where to insert, and the commented template. `None` when no name is eligible or no byte is safe.
pub(crate) fn insertion(text: &str, names: &[String], kind: TemplateKind) -> Option<Insertion> {
    if names.is_empty() {
        return None;
    }
    let scan = scan(text);
    let byte = choose_offset(text, &scan)?;
    Some(Insertion {
        byte,
        new_text: insertion_text(text, byte, names, kind),
    })
}

struct Scan {
    last_decl_end: Option<u32>,
    first_command: Option<u32>,
    block_spans: Vec<(u32, u32)>,
}

fn scan(text: &str) -> Scan {
    let tokens = tokenize(text);
    let mut i = 0;
    let mut last_decl_end = None;
    let mut first_command = None;
    let mut block_spans = Vec::new();
    while i < tokens.len() && tokens[i].kind != TokenKind::Eof {
        if tokens[i].kind != TokenKind::Ident {
            i = if matches!(
                tokens[i].kind,
                TokenKind::MacroDir | TokenKind::MacroInterp | TokenKind::Semi
            ) {
                i + 1
            } else {
                statement_end(&tokens, text, i).0
            };
            continue;
        }
        let word = tokens[i].text(text).to_ascii_lowercase();
        let next_is_eq = tokens.get(i + 1).map(|tok| tok.kind) == Some(TokenKind::Eq);
        if next_is_eq {
            i = statement_end(&tokens, text, i).0;
            continue;
        }
        if is_declaration(&word) {
            let (next, end_byte) = statement_end(&tokens, text, i);
            last_decl_end = Some(end_byte);
            i = next;
            continue;
        }
        if is_block(&word) {
            let start = tokens[i].span.start;
            let (next, end_byte, closed) = skip_block(&tokens, text, i);
            let end = if !closed && end_byte >= text.len() as u32 {
                text.len() as u32 + 1
            } else {
                end_byte
            };
            block_spans.push((start, end));
            i = next;
            continue;
        }
        if word == "end" {
            i += 1;
            if tokens.get(i).map(|tok| tok.kind) == Some(TokenKind::Semi) {
                i += 1;
            }
            continue;
        }
        if first_command.is_none() {
            first_command = Some(tokens[i].span.start);
        }
        i = statement_end(&tokens, text, i).0;
    }
    Scan {
        last_decl_end,
        first_command,
        block_spans,
    }
}

fn choose_offset(text: &str, scan: &Scan) -> Option<u32> {
    let decl_end = scan.last_decl_end.unwrap_or(0);
    let eof = text.len() as u32;
    let safe = |at: u32| !point_is_unsafe(text, at, &scan.block_spans);
    if let Some(cmd) = scan.first_command {
        if decl_end <= cmd {
            if safe(cmd) {
                let trimmed = before_horizontal_space(text, cmd);
                if trimmed >= decl_end && safe(trimmed) {
                    return Some(trimmed);
                }
                return Some(cmd);
            }
            if decl_end < cmd && safe(decl_end) {
                return Some(decl_end);
            }
            return None;
        }
    }
    if safe(eof) {
        return Some(eof);
    }
    if scan.last_decl_end.is_some() && safe(decl_end) {
        Some(decl_end)
    } else {
        None
    }
}

fn before_horizontal_space(text: &str, at: u32) -> u32 {
    let bytes = text.as_bytes();
    let mut i = at as usize;
    while i > 0 && matches!(bytes[i - 1], b' ' | b'\t') {
        i -= 1;
    }
    i as u32
}

fn insertion_text(text: &str, at: u32, names: &[String], kind: TemplateKind) -> String {
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut body = String::new();
    body.push_str("// shocks;");
    body.push_str(nl);
    for name in names {
        body.push_str("// var ");
        body.push_str(name);
        body.push(';');
        body.push_str(nl);
        match kind {
            TemplateKind::Stochastic => {
                body.push_str("// stderr ;");
                body.push_str(nl);
            }
            TemplateKind::Deterministic => {
                body.push_str("// periods ;");
                body.push_str(nl);
                body.push_str("// values ;");
                body.push_str(nl);
            }
        }
    }
    body.push_str("// end;");
    body.push_str(nl);

    let at_us = at as usize;
    let mut out = String::new();
    if at_us > 0 && !text[..at_us].ends_with('\n') {
        out.push_str(nl);
    }
    let followed_by_newline = at_us < text.len()
        && (text[at_us..].starts_with('\n') || text[at_us..].starts_with("\r\n"));
    if followed_by_newline {
        out.push_str(body.strip_suffix(nl).unwrap_or(&body));
    } else {
        out.push_str(&body);
    }
    out
}

fn is_declaration(word: &str) -> bool {
    matches!(
        word,
        "var"
            | "varexo"
            | "varexo_det"
            | "parameters"
            | "predetermined_variables"
            | "model_local_variable"
            | "trend_var"
            | "log_trend_var"
            | "external_function"
            | "heterogeneity_dimension"
            | "change_type"
            | "var_remove"
    )
}

fn is_block(word: &str) -> bool {
    crate::parser::BLOCK_OPENERS.contains(&word)
        || matches!(
            word,
            "mshocks"
                | "estimated_params"
                | "verbatim"
                | "model_replace"
                | "svar_identification"
                | "conditional_forecast_paths"
        )
}

fn statement_end(tokens: &[Token], src: &str, start: usize) -> (usize, u32) {
    let mut i = start;
    let mut paren = 0i32;
    let mut end_byte = tokens.get(start).map(|tok| tok.span.end).unwrap_or(0);
    while i < tokens.len() && tokens[i].kind != TokenKind::Eof {
        if paren == 0 && tokens[i].kind == TokenKind::Ident {
            let word = tokens[i].text(src).to_ascii_lowercase();
            if is_block(&word)
                && statement_semi_after(tokens, i)
                && !ident_seen_before(tokens, src, i, &word)
            {
                return (i, end_byte);
            }
        }
        match tokens[i].kind {
            TokenKind::LParen => paren += 1,
            TokenKind::RParen => paren = paren.saturating_sub(1),
            TokenKind::Semi if paren == 0 => {
                return (i + 1, tokens[i].span.end);
            }
            _ => end_byte = tokens[i].span.end,
        }
        i += 1;
    }
    (i, end_byte)
}

/// Stop at the first `end;` or at the next real block statement.
///
/// A block keyword ends this block only when it starts a statement and is
/// followed by `;`, after an optional argument list. `y = shocks` and
/// `shocks(-1)` inside an equation do not. A name already declared before
/// this block stays a name when the block is an equation body.
/// Returns `(next_index, end_byte, closed)`.
fn skip_block(tokens: &[Token], src: &str, opener: usize) -> (usize, u32, bool) {
    let opener_word = tokens[opener].text(src).to_ascii_lowercase();
    let equation_body = matches!(
        opener_word.as_str(),
        "model" | "model_replace" | "steady_state_model" | "matched_moments"
    );
    let mut i = opener + 1;
    let mut paren = 0i32;
    while i < tokens.len() && tokens[i].kind != TokenKind::Eof {
        match tokens[i].kind {
            TokenKind::LParen => paren += 1,
            TokenKind::RParen => paren = paren.saturating_sub(1),
            TokenKind::Ident if paren == 0 => {
                let word = tokens[i].text(src).to_ascii_lowercase();
                if word == "end" && tokens.get(i + 1).map(|tok| tok.kind) == Some(TokenKind::Semi) {
                    let end_byte = tokens[i + 1].span.end;
                    return (i + 2, end_byte, true);
                }
                if is_block(&word)
                    && starts_statement(tokens, i)
                    && statement_semi_after(tokens, i)
                    && !(equation_body && ident_seen_before(tokens, src, opener, &word))
                {
                    return (i, tokens[i].span.start, false);
                }
            }
            _ => {}
        }
        i += 1;
    }
    (i, src.len() as u32, false)
}

fn starts_statement(tokens: &[Token], index: usize) -> bool {
    let mut k = index;
    while k > 0 && tokens[k - 1].kind == TokenKind::MacroDir {
        k -= 1;
    }
    k == 0 || tokens[k - 1].kind == TokenKind::Semi
}

/// `keyword` or `keyword(...)` and then `;`.
fn statement_semi_after(tokens: &[Token], index: usize) -> bool {
    let mut k = index + 1;
    if tokens.get(k).map(|tok| tok.kind) == Some(TokenKind::LParen) {
        let mut depth = 1i32;
        k += 1;
        while let Some(tok) = tokens.get(k) {
            match tok.kind {
                TokenKind::LParen => depth += 1,
                TokenKind::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        k += 1;
                        break;
                    }
                }
                TokenKind::Eof => return false,
                _ => {}
            }
            k += 1;
        }
    }
    tokens.get(k).map(|tok| tok.kind) == Some(TokenKind::Semi)
}

fn ident_seen_before(tokens: &[Token], src: &str, before: usize, word: &str) -> bool {
    tokens[..before]
        .iter()
        .any(|tok| tok.kind == TokenKind::Ident && tok.text(src).eq_ignore_ascii_case(word))
}

fn point_is_unsafe(text: &str, at: u32, block_spans: &[(u32, u32)]) -> bool {
    block_spans
        .iter()
        .any(|(start, end)| *start <= at && at < *end)
        || in_comment_string_or_directive(text, at)
}

fn in_comment_string_or_directive(text: &str, at: u32) -> bool {
    let bytes = text.as_bytes();
    let at_us = at as usize;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            let start = i;
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            if (start..i).contains(&at_us) {
                return true;
            }
            continue;
        }
        if bytes[i] == b'%' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            if (start..i).contains(&at_us) {
                return true;
            }
            continue;
        }
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let start = i;
            i += 2;
            if let Some(rel) = text[i..].find("*/") {
                i += rel + 2;
                if (start..i).contains(&at_us) {
                    return true;
                }
                continue;
            }
            return at_us >= start;
        }
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            let quote = bytes[i];
            let start = i;
            i += 1;
            while i < bytes.len() && bytes[i] != quote && bytes[i] != b'\n' {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == quote {
                i += 1;
            }
            if start < at_us && at_us < i {
                return true;
            }
            continue;
        }
        if bytes[i] == b'@' && bytes.get(i + 1) == Some(&b'#') {
            let start = i;
            loop {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                let line_start = text[..i].rfind('\n').map(|pos| pos + 1).unwrap_or(0);
                let continued = text[line_start..i].trim_end().ends_with('\\');
                if continued && i < bytes.len() && bytes[i] == b'\n' {
                    i += 1;
                    continue;
                }
                break;
            }
            if start < at_us && at_us < i {
                return true;
            }
            continue;
        }
        if bytes[i] == b'@' && bytes.get(i + 1) == Some(&b'{') {
            let start = i;
            i += 2;
            if let Some(rel) = text[i..].find('}') {
                i += rel + 1;
                if start < at_us && at_us < i {
                    return true;
                }
                continue;
            }
            return at_us >= start;
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn e001_count(text: &str) -> usize {
        crate::check_file(text, "shock-template.mod")
            .into_iter()
            .filter(|diag| diag.code == "E001")
            .count()
    }

    fn apply_at(text: &str, edit: &Insertion) -> String {
        let mut out = text.to_string();
        out.insert_str(edit.byte as usize, &edit.new_text);
        out
    }

    #[test]
    fn live_empty_shocks_block_is_e001_so_the_wrapper_stays_commented() {
        let base = "varexo e;\n";
        let live = "varexo e;\nshocks;\n// var e;\n// stderr ;\nend;\n";
        let commented = "varexo e;\n// shocks;\n// var e;\n// stderr ;\n// end;\n";
        assert!(
            e001_count(live) > e001_count(base),
            "a shocks block whose only rows are comments is a syntax error"
        );
        assert!(e001_count(commented) <= e001_count(base));
    }

    #[test]
    fn model_remove_does_not_list_the_removed_endogenous() {
        let model = parse(
            "var c, y;\nvarexo e;\nmodel;\n[name='drop']\nc = 0;\n[name='keep']\ny = c + e;\nend;\nmodel_remove([name='drop']);\n",
        );
        assert_eq!(
            aggregate_varexo_names(&model),
            vec!["e".to_string()],
            "{:?}",
            aggregate_varexo_names(&model)
        );
    }

    #[test]
    fn names_skip_deterministic_and_heterogeneous_and_keep_order() {
        let model = parse(
            "heterogeneity_dimension d;\nvarexo u, e;\nvarexo_det x;\nvarexo(heterogeneity=d) h;\nvarexo e;\n",
        );
        assert_eq!(
            aggregate_varexo_names(&model),
            vec!["u".to_string(), "e".to_string()]
        );
        assert_eq!(
            aggregate_deterministic_names(&model),
            vec!["u".to_string(), "e".to_string(), "x".to_string()]
        );
    }

    #[test]
    fn ordinary_shocks_are_only_the_regular_block() {
        let ordinary = parse("varexo e;\nshocks;\nvar e; stderr 0.01;\nend;\n");
        let empty = parse("varexo e;\nshocks;\nend;\n");
        let overwrite = parse("varexo e;\nshocks(overwrite);\nend;\n");
        let het = parse(
            "heterogeneity_dimension d;\nvarexo(heterogeneity=d) h;\nshocks(heterogeneity=d);\nvar h = 0.1;\nend;\n",
        );
        let multiplicative = parse("varexo e;\nmshocks;\nvar e; periods 1; values 1; end;\n");
        assert!(has_ordinary_shocks(&ordinary));
        assert!(has_ordinary_shocks(&empty));
        assert!(has_ordinary_shocks(&overwrite));
        assert!(!has_ordinary_shocks(&het));
        assert!(!has_ordinary_shocks(&multiplicative));
    }

    #[test]
    fn template_goes_before_the_first_command_and_adds_no_e001() {
        let text = "var y;\nvarexo e;\nparameters a;\na = 0.9;\nmodel;\ny = a * y(-1) + e;\nend;\nstoch_simul;\n";
        let model = parse(text);
        let edit = insertion(
            text,
            &aggregate_varexo_names(&model),
            TemplateKind::Stochastic,
        )
        .unwrap();
        let edited = apply_at(text, &edit);
        let template = edited.find("// shocks;").unwrap();
        let command = edited.find("stoch_simul").unwrap();
        let model_end = edited.find("end;").unwrap();
        assert!(model_end < template && template < command);
        assert!(e001_count(&edited) <= e001_count(text));
        assert!(!edit.new_text.chars().any(|ch| ch.is_ascii_digit()));
        assert!(edit.new_text.contains("// var e;\n// stderr ;\n"));
    }

    #[test]
    fn no_command_uses_the_end_of_the_file() {
        let text = "varexo e;\n";
        let edit = insertion(text, &["e".to_string()], TemplateKind::Stochastic).unwrap();
        assert_eq!(edit.byte, text.len() as u32);
        let edited = apply_at(text, &edit);
        assert!(edited.starts_with("varexo e;\n// shocks;\n"));
        assert!(e001_count(&edited) <= e001_count(text));
    }

    #[test]
    fn unclosed_model_does_not_take_the_template() {
        assert!(insertion(
            "model;\ny = e;\n",
            &["e".to_string()],
            TemplateKind::Stochastic
        )
        .is_none());
        let text = "varexo e;\nmodel;\ny = e;\n";
        let edit = insertion(text, &["e".to_string()], TemplateKind::Stochastic).unwrap();
        let edited = apply_at(text, &edit);
        let template = edited.find("// shocks;").unwrap();
        let model_at = edited.find("model;").unwrap();
        assert!(template < model_at);

        let missing_semi = "varexo e\nmodel;\ny = e;\n";
        let edit = insertion(missing_semi, &["e".to_string()], TemplateKind::Stochastic).unwrap();
        let edited = apply_at(missing_semi, &edit);
        let template = edited.find("// shocks;").unwrap();
        let model_at = edited.find("model;").unwrap();
        assert!(template < model_at, "{edited}");

        for body in ["y = rho * y(-1) + shocks;", "y = shocks(-1);"] {
            let text = format!("varexo shocks;\nmodel;\n{body}\n");
            let edit = insertion(&text, &["shocks".to_string()], TemplateKind::Stochastic).unwrap();
            let edited = apply_at(&text, &edit);
            let template = edited.find("// shocks;").unwrap();
            let model_at = edited.find("model;").unwrap();
            assert!(template < model_at, "{edited}");
        }
    }

    #[test]
    fn unclosed_comment_with_nothing_to_anchor_offers_nothing() {
        assert!(insertion("/*\n", &["e".to_string()], TemplateKind::Stochastic).is_none());
    }

    #[test]
    fn unfinished_blocks_keep_both_templates_at_top_level() {
        for opener in ["initval", "steady_state_model", "estimated_params"] {
            let text = format!("varexo e;\n{opener};\ne = 0;\n");
            for kind in [TemplateKind::Stochastic, TemplateKind::Deterministic] {
                let edit = insertion(&text, &["e".to_string()], kind).unwrap();
                let edited = apply_at(&text, &edit);
                assert!(
                    edited.find("// shocks;").unwrap() < edited.find(opener).unwrap(),
                    "{edited}"
                );
                assert!(e001_count(&edited) <= e001_count(&text), "{edited}");
            }
        }
        let closed = "varexo e;\ninitval;\ne = 0;\nend;\n";
        let edit = insertion(closed, &["e".to_string()], TemplateKind::Deterministic).unwrap();
        let edited = apply_at(closed, &edit);
        assert!(edited.find("// shocks;").unwrap() > edited.find("end;").unwrap());
        assert!(e001_count(&edited) <= e001_count(closed));
    }

    #[test]
    fn deterministic_rows_have_blank_periods_and_values() {
        let text = "varexo e;\nvarexo_det x;\n";
        let model = parse(text);
        let edit = insertion(
            text,
            &aggregate_deterministic_names(&model),
            TemplateKind::Deterministic,
        )
        .unwrap();
        assert_eq!(
            edit.new_text.lines().collect::<Vec<_>>(),
            vec![
                "// shocks;",
                "// var e;",
                "// periods ;",
                "// values ;",
                "// var x;",
                "// periods ;",
                "// values ;",
                "// end;"
            ]
        );
        assert!(e001_count(&apply_at(text, &edit)) <= e001_count(text));
    }
}
