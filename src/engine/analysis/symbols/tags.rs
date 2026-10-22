//! Verified symbol references inside the two semantic equation-tag values.

use super::SemanticModel;
use crate::engine::editing::refs::is_legal_ident;
use crate::engine::language::lexer::Token;
use crate::engine::language::{lexer::TokenKind, model::Model, span::Span};
use std::collections::HashSet;

#[derive(Clone)]
pub(super) struct Reference {
    pub name: String,
    pub declaration: bool,
    pub order: Option<usize>,
    pub declaration_order: usize,
    pub within_token: Span,
    pub written: Option<(String, Span)>,
}

pub(super) fn semantic_name<'a>(key: &str, value: &'a str) -> Option<(usize, &'a str)> {
    let name = match key {
        "endogenous" => value,
        "mcp" => {
            let separator = value.find('<').or_else(|| value.find('>'))?;
            value[..separator].trim_matches([' ', '\n', '\t'])
        }
        _ => return None,
    };
    is_legal_ident(name).then(|| (value.find(name).unwrap_or(0), name))
}

pub(super) fn content(raw: &str) -> Option<&str> {
    let quote = *raw.as_bytes().first()?;
    if !matches!(quote, b'\'' | b'"') || raw.len() < 2 || raw.as_bytes().last() != Some(&quote) {
        return None;
    }
    raw.get(1..raw.len() - 1)
}

/// Only copied bytes prove an editable reference. Other bytes in the same
/// string may be generated, including an mcp tag's numeric bound.
fn literal(
    view: &SemanticModel,
    model: &Model,
    token: &Token,
    within: Span,
    name: &str,
) -> Option<(String, Span)> {
    if crate::engine::control::cancelled() {
        return None;
    }
    let raw = token.text(&model.source);
    if raw.get(within.start as usize..within.end as usize) != Some(name) {
        return None;
    }
    let mut parts = Vec::new();
    if let Some(emitted) = &token.emitted {
        if emitted
            .source
            .text
            .get(emitted.span.start as usize..emitted.span.end as usize)
            != Some(raw)
        {
            return None;
        }
        let span = Span {
            start: emitted.span.start + within.start,
            end: emitted.span.start + within.end,
        };
        let mut next = span.start;
        let first = emitted
            .source
            .origins
            .partition_point(|origin| origin.emitted.end <= span.start);
        for origin in emitted.source.origins[first..].iter().take_while(|origin| {
            !crate::engine::control::cancelled() && origin.emitted.start < span.end
        }) {
            let start = span.start.max(origin.emitted.start);
            let end = span.end.min(origin.emitted.end);
            if !origin.copied || start != next || end <= start {
                return None;
            }
            let written = Span {
                start: origin.written.start + start - origin.emitted.start,
                end: origin.written.start + end - origin.emitted.start,
            };
            parts.push(view.written(written)?);
            next = end;
        }
        if next != span.end {
            return None;
        }
    } else {
        if token.lexeme.is_some() {
            return None;
        }
        parts.push(view.written(Span {
            start: token.span.start + within.start,
            end: token.span.start + within.end,
        })?);
    }
    let (file, mut span) = parts.first()?.clone();
    for (other_file, other_span) in parts.iter().skip(1) {
        if crate::engine::control::cancelled() {
            break;
        }
        if other_file != &file || other_span.start != span.end {
            return None;
        }
        span.end = other_span.end;
    }
    let source = view.sources.get(&file)?;
    if source.get(span.start as usize..span.end as usize) != Some(name) {
        return None;
    }
    let tokens = view.written_tokens(&file)?;
    let index = tokens.partition_point(|written| written.span.end <= span.start);
    if !tokens.get(index).is_some_and(|written| {
        (written.kind == TokenKind::Ident && written.span == span)
            || written.kind == TokenKind::String
                && written.span.start < span.start
                && span.end < written.span.end
    }) {
        return None;
    }
    Some((file, span))
}

pub(super) fn collect(view: &SemanticModel, model: &Model) -> Vec<Reference> {
    let mut result = Vec::new();
    for row in &model.written_equations {
        if crate::engine::control::cancelled() {
            break;
        }
        let tokens = &model.expanded_tokens;
        let mut order = row.token_range.start;
        let mut found = HashSet::new();
        while order < row.token_range.end && tokens[order].kind == TokenKind::LBrack {
            if crate::engine::control::cancelled() {
                break;
            }
            order += 1;
            while order < row.token_range.end && tokens[order].kind != TokenKind::RBrack {
                if crate::engine::control::cancelled() {
                    break;
                }
                if tokens[order].kind == TokenKind::Ident
                    && tokens
                        .get(order + 1)
                        .is_some_and(|token| token.kind == TokenKind::Eq)
                {
                    let key = tokens[order].text(&model.source).to_ascii_lowercase();
                    let value_order = order + 2;
                    if key == "endogenous"
                        && !model
                            .endogenous_tag_actions
                            .contains(&row.equation.parse_order)
                    {
                        order += 1;
                        continue;
                    }
                    if let Some(token) = tokens
                        .get(value_order)
                        .filter(|token| token.kind == TokenKind::String)
                        && let Some(value) = content(token.text(&model.source))
                        && row
                            .equation
                            .tag_map
                            .get(&key)
                            .is_some_and(|accepted| accepted == value)
                        && let Some((offset, name)) = semantic_name(&key, value)
                    {
                        let within_token = Span::new(offset + 1, offset + 1 + name.len());
                        result.push(Reference {
                            name: name.into(),
                            declaration: key == "endogenous",
                            order: Some(value_order),
                            declaration_order: row.token_range.end,
                            within_token,
                            written: literal(view, model, token, within_token, name),
                        });
                        found.insert(key);
                    }
                }
                order += 1;
            }
            order += 1;
        }
        for key in ["endogenous", "mcp"] {
            if crate::engine::control::cancelled() {
                break;
            }
            if !found.contains(key)
                && (key != "endogenous"
                    || model
                        .endogenous_tag_actions
                        .contains(&row.equation.parse_order))
                && let Some(value) = row.equation.tag_map.get(key)
                && let Some((_, name)) = semantic_name(key, value)
            {
                result.push(Reference {
                    name: name.into(),
                    declaration: key == "endogenous",
                    order: None,
                    declaration_order: row.token_range.end,
                    within_token: Span::default(),
                    written: None,
                });
            }
        }
    }
    result
}

/// Every execution of a changed literal must still be the same semantic
/// reference. A macro copy used as a display label is not an editable use.
pub(super) fn compatible(
    view: &SemanticModel,
    model: &Model,
    references: &[Reference],
    targets: &HashSet<(String, Span)>,
    name: &str,
) -> bool {
    for (order, token) in model
        .expanded_tokens
        .iter()
        .take_while(|_| !crate::engine::control::cancelled())
        .enumerate()
        .filter(|(_, token)| token.kind == TokenKind::String)
    {
        let Some(value) = content(token.text(&model.source)) else {
            continue;
        };
        // A copied reference can be joined to generated label text or placed
        // inside quote/comment-like label characters. Check all exact byte
        // matches, including overlaps, without identifier-token boundaries.
        for (start, _) in value
            .char_indices()
            .take_while(|_| !crate::engine::control::cancelled())
            .filter(|(start, _)| value[*start..].starts_with(name))
        {
            let span = Span::new(start, start + name.len());
            let within = Span {
                start: span.start + 1,
                end: span.end + 1,
            };
            let Some(target) = literal(view, model, token, within, name) else {
                continue;
            };
            if targets.contains(&target)
                && !references.iter().any(|reference| {
                    reference.order == Some(order)
                        && reference.name == name
                        && reference.within_token == within
                        && reference.written.as_ref() == Some(&target)
                })
            {
                return false;
            }
        }
    }
    !crate::engine::control::cancelled()
}

pub(super) fn rewrite(
    model: &Model,
    tokens: &mut [Token],
    references: &[Reference],
    name: &str,
    replacement: &str,
) -> Option<()> {
    for reference in references.iter().filter(|reference| reference.name == name) {
        if crate::engine::control::cancelled() {
            break;
        }
        reference.written.as_ref()?;
        let token = tokens.get_mut(reference.order?)?;
        let mut raw = token.text(&model.source).to_owned();
        if raw.get(reference.within_token.start as usize..reference.within_token.end as usize)
            != Some(name)
        {
            return None;
        }
        raw.replace_range(
            reference.within_token.start as usize..reference.within_token.end as usize,
            replacement,
        );
        token.lexeme = Some(raw);
    }
    (!crate::engine::control::cancelled()).then_some(())
}
