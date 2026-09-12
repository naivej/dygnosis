//! Initval / equation-shape diagnostics (E050–E053, W042, W050–W053, I050).

use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, Severity};
use crate::expr::ExprKind;
use crate::intern::Name;
use crate::lexer::{tokenize, Token, TokenKind};
use crate::model::{Equation, Model};
use crate::span::{LineIndex, Span};

const I050_MESSAGE: &str = "No initval or steady_state_model block. Add an initval block with initial guesses, or a steady_state_model block with closed-form assignments.";

const OP_CHARS: &[char] = &[
    '=', '+', '-', '*', '/', '^', ',', '(', ')', '[', ']', '<', '>',
];

pub(crate) fn check_shape(model: &Model) -> Vec<Diagnostic> {
    let index = LineIndex::new(&model.source);
    let mut out = check_e050(model, &index);
    out.extend(check_e052(model, &index));
    out.extend(check_e051(model));
    out.extend(check_e053(model, &index));
    out.extend(check_w042(model));
    out.extend(check_i050(model));
    out.extend(check_w050_w053(model));
    out.extend(check_w051(model));
    out.extend(check_w052(model));
    out
}

fn check_e050(model: &Model, index: &LineIndex) -> Vec<Diagnostic> {
    if model.equations.is_empty() {
        return Vec::new();
    }
    let mut seen: HashMap<(String, Vec<String>), (Span, u32)> = HashMap::new();
    let mut diagnostics = Vec::new();
    for eq in &model.equations {
        if eq.text.trim().starts_with('#') {
            continue;
        }
        let key = (normalize_eq_text(&eq.text), tag_class(eq));
        if let Some(&(first_span, first_line)) = seen.get(&key) {
            let _ = first_span;
            diagnostics.push(Diagnostic {
                span: eq.span,
                severity: Severity::Warning,
                code: "W054".to_string(),
                message: format!(
                    "Duplicate equation (same as line {first_line}). Fix: remove this duplicate equation."
                ),
                fix: None,
                tags: Vec::new(),
            });
        } else {
            let line = index.position(&model.source, eq.span.start).line + 1;
            seen.insert(key, (eq.span, line));
        }
    }
    diagnostics
}

fn tag_class(eq: &Equation) -> Vec<String> {
    let mut tags: Vec<String> = eq
        .tags
        .iter()
        .filter(|t| *t == "static" || *t == "dynamic")
        .cloned()
        .collect();
    tags.sort();
    tags.dedup();
    tags
}

fn normalize_eq_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if OP_CHARS.contains(&chars[i]) {
            while out.ends_with(' ') {
                out.pop();
            }
            out.push(chars[i]);
            i += 1;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            continue;
        }
        if chars[i].is_whitespace() {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            i += 1;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out.trim().to_string()
}

fn check_e051(model: &Model) -> Vec<Diagnostic> {
    if model.equations.is_empty() {
        return Vec::new();
    }
    let mut diagnostics = Vec::new();
    for eq in &model.equations {
        let text = eq.text.trim();
        if text.starts_with('#') {
            continue;
        }
        if let Some((lhs_val, rhs_val)) = match_number_eq(text) {
            if lhs_val != rhs_val {
                diagnostics.push(Diagnostic {
                    span: eq.span,
                    severity: Severity::Warning,
                    code: "W055".to_string(),
                    message: format!(
                        "Contradictory equation '{text}' (always false). Fix: remove this equation."
                    ),
                    fix: None,
                    tags: Vec::new(),
                });
                continue;
            }
        }
        if let Some((lhs, rhs)) = text.split_once('=') {
            let lhs_norm: String = lhs.chars().filter(|c| !c.is_whitespace()).collect();
            let rhs_norm: String = rhs.chars().filter(|c| !c.is_whitespace()).collect();
            if !lhs_norm.is_empty() && lhs_norm == rhs_norm {
                diagnostics.push(Diagnostic {
                    span: eq.span,
                    severity: Severity::Warning,
                    code: "W055".to_string(),
                    message: format!(
                        "Trivially true equation '{text}' (LHS = RHS). Fix: remove this equation."
                    ),
                    fix: None,
                    tags: Vec::new(),
                });
            }
        }
    }
    diagnostics
}

fn match_number_eq(text: &str) -> Option<(f64, f64)> {
    let (lhs, rhs) = text.split_once('=')?;
    let lhs_val = parse_signed_decimal(lhs.trim())?;
    let rhs_val = parse_signed_decimal(rhs.trim())?;
    if !is_signed_decimal(lhs.trim()) || !is_signed_decimal(rhs.trim()) {
        return None;
    }
    Some((lhs_val, rhs_val))
}

fn is_signed_decimal(s: &str) -> bool {
    parse_signed_decimal(s).is_some() && signed_decimal_consumes_all(s)
}

fn signed_decimal_consumes_all(s: &str) -> bool {
    let mut rest = s;
    if rest.starts_with('+') || rest.starts_with('-') {
        rest = rest[1..].trim_start();
    }
    if rest.is_empty() || !rest.chars().next().unwrap().is_ascii_digit() {
        return false;
    }
    let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    rest = &rest[digits..];
    if let Some(stripped) = rest.strip_prefix('.') {
        if stripped.is_empty() || !stripped.chars().next().unwrap().is_ascii_digit() {
            return false;
        }
        let frac = stripped.chars().take_while(|c| c.is_ascii_digit()).count();
        rest = &stripped[frac..];
    }
    rest.is_empty()
}

fn parse_signed_decimal(s: &str) -> Option<f64> {
    if !signed_decimal_consumes_all(s) {
        return None;
    }
    s.replace(' ', "").parse().ok()
}

fn check_e052(model: &Model, index: &LineIndex) -> Vec<Diagnostic> {
    let mut known: HashMap<String, f64> = HashMap::new();
    let mut seen: HashMap<Name, Vec<(f64, u32, String)>> = HashMap::new();
    let mut diagnostics = Vec::new();
    for a in &model.param_assignments {
        let name = model.name(a.name).to_string();
        let value = fold_numeric(&a.expression, &known);
        if let Some(v) = value {
            known.insert(name.clone(), v);
        }
        if let Some(v) = value {
            if let Some(prev) = seen.get(&a.name) {
                if let Some((_, first_line, _)) = prev.iter().find(|(pv, _, _)| *pv == v) {
                    diagnostics.push(Diagnostic {
                        span: a.span,
                        severity: Severity::Warning,
                        code: "W056".to_string(),
                        message: format!(
                            "Duplicate parameter assignment '{name} = {}' (same value as line {first_line}); the later assignment is redundant. Remove it to avoid confusion.",
                            a.expression
                        ),
                        fix: None,
                        tags: Vec::new(),
                    });
                }
            }
            let line = index.position(&model.source, a.span.start).line + 1;
            seen.entry(a.name)
                .or_default()
                .push((v, line, a.expression.clone()));
        }
    }
    diagnostics
}

fn fold_numeric(expr: &str, known: &HashMap<String, f64>) -> Option<f64> {
    let tokens: Vec<_> = tokenize(expr)
        .into_iter()
        .filter(|t| t.kind != TokenKind::Eof)
        .collect();
    if tokens.is_empty() {
        return None;
    }
    let mut p = FoldParser {
        src: expr,
        tokens: &tokens,
        i: 0,
        known,
    };
    let v = p.fold_bp(0)?;
    if p.i != p.tokens.len() {
        return None;
    }
    Some(v)
}

struct FoldParser<'a> {
    src: &'a str,
    tokens: &'a [crate::lexer::Token],
    i: usize,
    known: &'a HashMap<String, f64>,
}

impl FoldParser<'_> {
    fn fold_bp(&mut self, min_bp: u8) -> Option<f64> {
        let mut lhs = self.fold_prefix()?;
        while let Some((l_bp, r_bp, op)) = self.infix() {
            if l_bp < min_bp {
                break;
            }
            self.i += 1;
            let rhs = self.fold_bp(r_bp)?;
            lhs = match op {
                TokenKind::Plus => lhs + rhs,
                TokenKind::Minus => lhs - rhs,
                TokenKind::Star => lhs * rhs,
                TokenKind::Slash => lhs / rhs,
                TokenKind::Caret => lhs.powf(rhs),
                _ => return None,
            };
        }
        Some(lhs)
    }

    fn fold_prefix(&mut self) -> Option<f64> {
        let tok = self.tokens.get(self.i)?;
        match tok.kind {
            TokenKind::Plus => {
                self.i += 1;
                self.fold_bp(7)
            }
            TokenKind::Minus => {
                self.i += 1;
                Some(-self.fold_bp(7)?)
            }
            TokenKind::LParen => {
                self.i += 1;
                let inner = self.fold_bp(0)?;
                if self.tokens.get(self.i).map(|t| t.kind) != Some(TokenKind::RParen) {
                    return None;
                }
                self.i += 1;
                Some(inner)
            }
            TokenKind::Number => {
                self.i += 1;
                tok.text(self.src).replace(' ', "").parse().ok()
            }
            TokenKind::Ident => {
                if self.tokens.get(self.i + 1).map(|t| t.kind) == Some(TokenKind::LParen) {
                    return None;
                }
                self.i += 1;
                self.known.get(tok.text(self.src)).copied()
            }
            _ => None,
        }
    }

    fn infix(&self) -> Option<(u8, u8, TokenKind)> {
        let kind = self.tokens.get(self.i)?.kind;
        match kind {
            TokenKind::Plus | TokenKind::Minus => Some((1, 2, kind)),
            TokenKind::Star | TokenKind::Slash => Some((3, 4, kind)),
            TokenKind::Caret => Some((6, 5, kind)),
            _ => None,
        }
    }
}

fn check_e053(model: &Model, index: &LineIndex) -> Vec<Diagnostic> {
    let mut block_lines = HashSet::new();
    for span in [
        model.model_block,
        model.ss_block,
        model.initval_block,
        model.endval_block,
        model.shocks_block,
    ]
    .into_iter()
    .flatten()
    {
        let start = index.position(&model.source, span.start).line;
        let end = index
            .position(&model.source, span.end.saturating_sub(1).max(span.start))
            .line;
        for line in start..=end {
            block_lines.insert(line);
        }
    }

    let mut skip_lines = HashSet::new();
    for a in model
        .param_assignments
        .iter()
        .chain(&model.helper_assignments)
    {
        skip_lines.insert(index.position(&model.source, a.span.start).line);
    }
    for d in model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.parameters)
    {
        skip_lines.insert(index.position(&model.source, d.span.start).line);
    }

    let mut diagnostics = Vec::new();
    let mut line: Option<u32> = None;
    let mut line_toks: Vec<Token> = Vec::new();
    for tok in tokenize(&model.source) {
        if tok.kind == TokenKind::Eof {
            break;
        }
        let tok_line = index.position(&model.source, tok.span.start).line;
        if line != Some(tok_line) {
            flush_e053_line(
                &model.source,
                line,
                &line_toks,
                &block_lines,
                &skip_lines,
                &mut diagnostics,
            );
            line = Some(tok_line);
            line_toks.clear();
        }
        line_toks.push(tok);
    }
    flush_e053_line(
        &model.source,
        line,
        &line_toks,
        &block_lines,
        &skip_lines,
        &mut diagnostics,
    );
    diagnostics
}

fn flush_e053_line(
    source: &str,
    line: Option<u32>,
    toks: &[Token],
    block_lines: &HashSet<u32>,
    skip_lines: &HashSet<u32>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(line) = line else {
        return;
    };
    if toks.is_empty() || block_lines.contains(&line) || skip_lines.contains(&line) {
        return;
    }
    let Some(span) = stray_number_eq_span(toks) else {
        return;
    };
    let shown = source[span.start as usize..span.end as usize]
        .trim_end_matches(';')
        .trim();
    diagnostics.push(Diagnostic {
        span,
        severity: Severity::Warning,
        code: "W057".to_string(),
        message: format!(
            "Stray equation '{shown}' outside model block (line {}). This will cause a Dynare syntax error. Fix: remove this line.",
            line + 1
        ),
        fix: None,
        tags: Vec::new(),
    });
}

fn stray_number_eq_span(toks: &[Token]) -> Option<Span> {
    let mut i = 0;
    i = skip_signed_number(toks, i)?;
    if toks.get(i).map(|t| t.kind) != Some(TokenKind::Eq) {
        return None;
    }
    i += 1;
    i = skip_signed_number(toks, i)?;
    let mut end = toks[i - 1].span.end;
    if toks.get(i).map(|t| t.kind) == Some(TokenKind::Semi) {
        end = toks[i].span.end;
        i += 1;
    }
    if i != toks.len() {
        return None;
    }
    Some(Span {
        start: toks[0].span.start,
        end,
    })
}

fn skip_signed_number(toks: &[Token], i: usize) -> Option<usize> {
    let tok = toks.get(i)?;
    match tok.kind {
        TokenKind::Plus | TokenKind::Minus => {
            if toks.get(i + 1).map(|t| t.kind) == Some(TokenKind::Number) {
                Some(i + 2)
            } else {
                None
            }
        }
        TokenKind::Number => Some(i + 1),
        _ => None,
    }
}

fn check_w042(model: &Model) -> Vec<Diagnostic> {
    if model.steady_state_equations.is_empty() {
        return Vec::new();
    }
    let static_eqs: Vec<&Equation> = model
        .equations
        .iter()
        .filter(|eq| is_static_eq(eq))
        .collect();
    if !model.equations.is_empty() && static_eqs.is_empty() {
        return Vec::new();
    }

    let assigned = ss_assigned_names(model);
    let missing: Vec<String> = model
        .endogenous
        .iter()
        .map(|d| model.name(d.name).to_string())
        .filter(|n| !assigned.contains(n))
        .collect();
    if missing.is_empty() {
        return Vec::new();
    }

    if !model.equations.is_empty() {
        let mut alpha = missing;
        alpha.sort();
        let n = alpha.len();
        let listed = alpha.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
        let suffix = if n > 5 {
            format!(" (and {} more)", n - 5)
        } else {
            String::new()
        };
        return vec![Diagnostic {
            span: model.ss_block.unwrap_or_else(|| fallback_span(model)),
            severity: Severity::Warning,
            code: "W042".to_string(),
            message: format!(
                "{n} endogenous variable(s) missing from steady_state_model: {listed}{suffix}"
            ),
            fix: None,
            tags: Vec::new(),
        }];
    }

    let span_block = model.ss_block;
    model
        .endogenous
        .iter()
        .filter(|d| !assigned.contains(model.name(d.name)))
        .map(|d| {
            let name = model.name(d.name);
            Diagnostic {
                span: span_block.unwrap_or(d.span),
                severity: Severity::Warning,
                code: "W042".to_string(),
                message: format!(
                    "Endogenous variable '{name}' has no assignment in the steady_state_model block."
                ),
                fix: None,
                tags: Vec::new(),
            }
        })
        .collect()
}

fn is_static_eq(eq: &Equation) -> bool {
    !eq.text.trim().starts_with('#') && !eq.dynamic_tag && !eq.tags.iter().any(|t| t == "dynamic")
}

fn ss_assigned_names(model: &Model) -> HashSet<String> {
    let mut assigned = HashSet::new();
    for eq in &model.steady_state_equations {
        if eq.is_local || eq.text.trim().starts_with('#') {
            continue;
        }
        if let Some(id) = eq.lhs_expr {
            if let ExprKind::Ident { name, timing, .. } = &model.exprs.get(id).kind {
                if *timing == 0 {
                    assigned.insert(model.name(*name).to_string());
                    continue;
                }
            }
        }
        let lhs = eq.lhs.trim();
        if is_simple_ident(lhs) {
            assigned.insert(lhs.to_string());
            continue;
        }
        if lhs.starts_with('[') && lhs.ends_with(']') {
            assigned.extend(bracket_idents(lhs));
        }
    }
    assigned
}

fn is_simple_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn bracket_idents(lhs: &str) -> Vec<String> {
    let inner = &lhs[1..lhs.len() - 1];
    let mut names = Vec::new();
    let mut cur = String::new();
    for c in inner.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            if cur.is_empty() && c.is_ascii_digit() {
                cur.clear();
                continue;
            }
            cur.push(c);
        } else if !cur.is_empty() {
            if cur
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            {
                names.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        }
    }
    if !cur.is_empty()
        && cur
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
    {
        names.push(cur);
    }
    names
}

fn check_i050(model: &Model) -> Vec<Diagnostic> {
    let static_eqs: Vec<&Equation> = model
        .equations
        .iter()
        .filter(|eq| is_static_eq(eq))
        .collect();
    if static_eqs.is_empty() {
        return Vec::new();
    }
    if !model_local_gate_empty(model) {
        return Vec::new();
    }
    if !model.steady_state_equations.is_empty() || !model.initval.is_empty() {
        return Vec::new();
    }
    if static_eqs.len() != model.endogenous.len() {
        return Vec::new();
    }
    vec![Diagnostic {
        span: model.model_block.unwrap_or_else(|| fallback_span(model)),
        severity: Severity::Information,
        code: "I050".to_string(),
        message: I050_MESSAGE.to_string(),
        fix: None,
        tags: Vec::new(),
    }]
}

fn model_local_gate_empty(model: &Model) -> bool {
    let declared: HashSet<Name> = model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.parameters)
        .map(|d| d.name)
        .collect();

    let mut eqs: Vec<&Equation> = model.equations.iter().collect();
    eqs.sort_by_key(|eq| (eq.span.start, eq.span.end));

    let mut first_definition: HashMap<Name, u32> = HashMap::new();
    for eq in &eqs {
        let Some(name) = model_local_name(model, eq) else {
            continue;
        };
        if first_definition.contains_key(&name) {
            return false;
        }
        first_definition.insert(name, eq.span.start);
        if declared.contains(&name) {
            return false;
        }
    }

    let mut visible_locals = HashSet::new();
    let mut seen_early = HashSet::new();
    for eq in &eqs {
        let local = model_local_name(model, eq);
        for r in model.ident_refs(eq) {
            if local == Some(r.name) {
                continue;
            }
            let Some(&def_start) = first_definition.get(&r.name) else {
                continue;
            };
            if visible_locals.contains(&r.name)
                || declared.contains(&r.name)
                || seen_early.contains(&r.name)
            {
                continue;
            }
            if eq.span.start >= def_start {
                continue;
            }
            seen_early.insert(r.name);
            return false;
        }
        if let Some(n) = local {
            visible_locals.insert(n);
        }
    }

    for eq in &model.steady_state_equations {
        let Some(name) = model_local_name(model, eq) else {
            continue;
        };
        if declared.contains(&name) {
            return false;
        }
    }
    true
}

fn model_local_name(model: &Model, eq: &Equation) -> Option<Name> {
    if !eq.is_local && !eq.text.trim().starts_with('#') {
        return None;
    }
    let id = eq.lhs_expr?;
    model.exprs.walk_idents(id).next().map(|r| r.name)
}

fn check_w050_w053(model: &Model) -> Vec<Diagnostic> {
    let params: HashSet<Name> = model.parameters.iter().map(|d| d.name).collect();
    let declared: HashSet<Name> = model
        .endogenous
        .iter()
        .chain(&model.exogenous)
        .chain(&model.parameters)
        .map(|d| d.name)
        .collect();
    let mut diagnostics = Vec::new();
    for (block_name, entries) in [
        ("initval", model.initval.as_slice()),
        ("endval", model.endval.as_slice()),
    ] {
        for entry in entries {
            let name = model.name(entry.name);
            if params.contains(&entry.name) {
                diagnostics.push(Diagnostic {
                    span: entry.span,
                    severity: Severity::Error,
                    code: "E059".to_string(),
                    message: format!(
                        "Parameter '{name}' assigned in {block_name} is ignored. Assign parameters before the model block or inside steady_state_model instead."
                    ),
                    fix: None,
                    tags: Vec::new(),
                });
                continue;
            }
            if !declared.contains(&entry.name) {
                diagnostics.push(Diagnostic {
                    span: entry.span,
                    severity: Severity::Error,
                    code: "E058".to_string(),
                    message: format!("Variable '{name}' in {block_name} is not declared."),
                    fix: None,
                    tags: Vec::new(),
                });
            }
        }
    }
    diagnostics
}

fn check_w051(model: &Model) -> Vec<Diagnostic> {
    let det: HashSet<Name> = model
        .deterministic_exogenous
        .iter()
        .map(|d| d.name)
        .collect();
    let exo: HashSet<Name> = model
        .exogenous
        .iter()
        .map(|d| d.name)
        .filter(|n| !det.contains(n))
        .collect();
    let mut diagnostics = Vec::new();
    for entry in &model.initval {
        if exo.contains(&entry.name) {
            let name = model.name(entry.name);
            diagnostics.push(Diagnostic {
                span: entry.span,
                severity: Severity::Warning,
                code: "W051".to_string(),
                message: format!(
                    "Exogenous variable '{name}' is set in initval. This is unusual -- exogenous shocks are typically zero at steady state."
                ),
                fix: None,
                tags: Vec::new(),
            });
        }
    }
    diagnostics
}

fn check_w052(model: &Model) -> Vec<Diagnostic> {
    if model.initval.is_empty() {
        return Vec::new();
    }
    let initval_names: HashSet<Name> = model.initval.iter().map(|a| a.name).collect();
    let mut missing: Vec<String> = model
        .endogenous
        .iter()
        .filter(|d| !initval_names.contains(&d.name))
        .map(|d| model.name(d.name).to_string())
        .collect();
    missing.sort();
    if missing.is_empty() {
        return Vec::new();
    }
    let n = missing.len();
    let listed = missing
        .iter()
        .take(10)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let suffix = if n > 10 {
        format!(" (and {} more)", n - 10)
    } else {
        String::new()
    };
    vec![Diagnostic {
        span: model.initval_block.unwrap_or_else(|| fallback_span(model)),
        severity: Severity::Warning,
        code: "W052".to_string(),
        message: format!(
            "{n} endogenous variable(s) missing from initval (will default to 0): {listed}{suffix}"
        ),
        fix: None,
        tags: Vec::new(),
    }]
}

fn fallback_span(model: &Model) -> Span {
    let end = model
        .source
        .chars()
        .next()
        .map(|c| c.len_utf8())
        .unwrap_or(1);
    Span::new(0, end)
}
