//! Readable editor display copy. Only proven gaps between tokens can change.

use std::collections::BTreeMap;

use crate::expand::ExpandReport;
use crate::lexer::TokenKind;
use crate::model::{ExecutionStep, Model, StatementKind};
use crate::parser::join_lexemes_recorded;
use crate::span::Span;

pub(crate) struct PreviewLayout {
    pub text: String,
    pub ranges: Vec<Span>,
}

pub(crate) fn readable(report: &ExpandReport, model: &Model) -> Option<PreviewLayout> {
    let original = &report.effective_text;
    // Use the tokens that produced the statement facts. Re-execution from
    // joined text would omit private include-expression replay points.
    let tokens = &model.expanded_tokens;
    let mut emitted = vec![None; tokens.len()];
    let copy = join_lexemes_recorded(&model.source, tokens, |index, span| {
        emitted[index] = Some(span);
    });
    if copy != *original {
        return None;
    }
    let mut starts = BTreeMap::new();
    let mut next_token = 0;
    for (index, step) in model.execution_steps.iter().enumerate() {
        match step {
            ExecutionStep::Statement(id) => next_token = model.statements[*id].token_range.end,
            ExecutionStep::Opaque(_) => {
                starts.insert(next_token, 0);
                next_token += model.opaque_tokens.get(&index)?.len();
            }
        }
    }
    for statement in &model.statements {
        if !statement.complete && !statement.native {
            continue;
        }
        starts.insert(statement.token_range.start, 0);
        if statement.complete {
            starts.entry(statement.token_range.end).or_insert(0);
        }
        if statement.kind != StatementKind::Block {
            continue;
        }
        let body = statement.opener_range.end;
        let end = statement.token_range.end - 2;
        starts.insert(end, 0);
        if body >= end {
            continue;
        }
        starts.insert(body, 1);
        if statement.name == "verbatim" {
            continue;
        }
        let mut parentheses = 0usize;
        let mut brackets = 0usize;
        for (index, token) in tokens.iter().enumerate().take(end).skip(body) {
            match token.kind {
                TokenKind::LParen => parentheses += 1,
                TokenKind::RParen => parentheses = parentheses.saturating_sub(1),
                TokenKind::LBrack => brackets += 1,
                TokenKind::RBrack => brackets = brackets.saturating_sub(1),
                TokenKind::Semi if parentheses == 0 && brackets == 0 && index + 1 < end => {
                    starts.insert(index + 1, 1);
                }
                _ => {}
            }
        }
    }
    // In incomplete input, complete statements and complete parsed model rows
    // still have safe boundaries; incomplete tails remain as emitted.
    for row in &model.written_equations {
        if tokens
            .get(row.token_range.end)
            .is_some_and(|token| token.kind == TokenKind::Semi)
        {
            starts.entry(row.token_range.start).or_insert(1);
        }
    }
    let mut text = String::with_capacity(original.len());
    let mut mapped = Vec::with_capacity(tokens.len());
    let mut previous_end = 0;
    for (index, span) in emitted.iter().enumerate() {
        let Some(span) = span else { continue };
        let start = span.start as usize;
        let end = span.end as usize;
        let gap = &original[previous_end..start];
        if let Some(&depth) = starts.get(&index).filter(|_| gap.trim().is_empty()) {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&"    ".repeat(depth));
        } else {
            text.push_str(gap);
        }
        let emitted_start = text.len();
        text.push_str(&original[start..end]);
        mapped.push((*span, Span::new(emitted_start, text.len())));
        previous_end = end;
    }
    text.push_str(&original[previous_end..]);
    let map = |span: Span| {
        let start = mapped.binary_search_by_key(&span.start, |(old, _)| old.start);
        let end = mapped.binary_search_by_key(&span.end, |(old, _)| old.end);
        match (start, end) {
            (Ok(start), Ok(end)) => Some(Span {
                start: mapped[start].1.start,
                end: mapped[end].1.end,
            }),
            _ => None,
        }
    };
    let ranges = report
        .navigation
        .iter()
        .map(|row| map(row.effective_span))
        .collect::<Option<Vec<_>>>()?;
    Some(PreviewLayout { text, ranges })
}
