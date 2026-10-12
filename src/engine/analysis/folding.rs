//! File-local folding boundaries without executing macros or reading includes.

use crate::engine::language::lexer::{tokenize, TokenKind};
use crate::engine::language::macro_expand::scan_macro_tokens;
use crate::engine::language::model::{MacroDirective, Model, StatementKind};
use crate::engine::language::parser::parse_expanded;
use crate::engine::language::span::Span;

pub(crate) struct SourceFold {
    pub span: Span,
    pub comment: bool,
}

struct MacroFrame<'a> {
    opener: &'a MacroDirective,
    parent: usize,
    branches: Vec<&'a MacroDirective>,
}

/// The parser supplies statement boundaries; its temporary facts are discarded.
/// Inert macro text keeps every branch and every byte/line in the written file.
pub(crate) fn source_folds(text: &str, directives: &[MacroDirective]) -> Vec<SourceFold> {
    let mut inert = text.as_bytes().to_vec();
    for token in scan_macro_tokens(text) {
        let replacement = match token.kind {
            TokenKind::MacroDir | TokenKind::MacroPrefix => b' ',
            TokenKind::MacroInterp => b'x',
            _ => continue,
        };
        for byte in &mut inert[token.span.start as usize..token.span.end as usize] {
            if !matches!(*byte, b'\n' | b'\r') {
                *byte = replacement;
            }
        }
    }
    let inert = String::from_utf8(inert).expect("macro masks preserve UTF-8");
    let (model, _) = parse_expanded(&inert, tokenize(&inert));
    let mut statement_spans: Vec<_> = complete_statements(&model).collect();
    // An unfinished block can consume later command rows as its body. Recover
    // at the first written INITIAL keyword on its own line, using the pinned
    // keyword owner and the same parser, without changing diagnostic recovery.
    let mut recovery = inert.as_bytes().to_vec();
    let mut changed = false;
    for statement in &model.statements {
        if statement.kind != StatementKind::Block || statement.complete {
            continue;
        }
        if let Some(head) = model.expanded_tokens
            [statement.opener_range.end..statement.token_range.end]
            .iter()
            .find(|token| {
                token.kind == TokenKind::Ident
                    && crate::engine::language::command_skip::is_pin_statement_keyword(
                        token.text(&inert),
                    )
                    && crate::engine::language::native_line::initial_content_start(
                        &inert,
                        line_start(&inert, token.span.start as usize),
                    ) == token.span.start as usize
            })
        {
            for byte in &mut recovery[statement.span.start as usize..head.span.start as usize] {
                if !matches!(*byte, b'\n' | b'\r') {
                    *byte = b' ';
                }
            }
            changed = true;
        }
    }
    if changed {
        let recovery = String::from_utf8(recovery).expect("recovery masks preserve UTF-8");
        let (model, _) = parse_expanded(&recovery, tokenize(&recovery));
        statement_spans.extend(complete_statements(&model));
    }
    let mut folds = Vec::new();
    let mut contexts = vec![(0, 0)];
    let mut stack: Vec<MacroFrame<'_>> = Vec::new();
    let mut context = 0;
    let mut next_context = 0;
    for directive in directives {
        let kind = directive.kind.as_str();
        if conditional(kind) || kind == "for" {
            stack.push(MacroFrame {
                opener: directive,
                parent: context,
                branches: vec![directive],
            });
            next_context += 1;
            context = next_context;
            contexts.push((directive.span.end, context));
        } else if matches!(kind, "else" | "elseif") {
            if let Some(frame) = stack.last_mut()
                && conditional(&frame.opener.kind)
            {
                frame.branches.push(directive);
                next_context += 1;
                context = next_context;
                contexts.push((directive.span.end, context));
            }
        } else if let Some(frame) = stack.pop_if(|frame| {
            kind == "endif" && conditional(&frame.opener.kind)
                || kind == "endfor" && frame.opener.kind == "for"
        }) {
            folds.push(region(Span {
                start: frame.opener.span.start,
                end: directive.span.end,
            }));
            if conditional(&frame.opener.kind) {
                for (index, branch) in frame.branches.iter().enumerate() {
                    let start = if index == 0 {
                        let end = branch.span.end as usize;
                        if end > 0 && text.as_bytes()[end - 1] == b'\n' {
                            end
                        } else {
                            next_line(text, end)
                        }
                    } else {
                        branch.span.start as usize
                    };
                    let boundary = frame.branches.get(index + 1).unwrap_or(&directive);
                    let end = line_start(text, boundary.span.start as usize);
                    if start < end {
                        folds.push(region(Span::new(start, end)));
                    }
                }
            }
            context = frame.parent;
            contexts.push((directive.span.start, context));
        }
    }
    let context_at = |offset| {
        let index = contexts.partition_point(|&(start, _)| start <= offset);
        contexts[index.saturating_sub(1)].1
    };
    for span in statement_spans {
        // A missing delimiter in one branch cannot borrow another branch's
        // delimiter. Whole constructs around nested macro blocks remain local.
        if context_at(span.start) == context_at(span.end.saturating_sub(1)) {
            folds.push(region(span));
        }
    }
    folds.extend(comment_folds(&inert));
    folds
}

fn complete_statements(model: &Model) -> impl Iterator<Item = Span> + '_ {
    model
        .statements
        .iter()
        .filter(|statement| statement.complete && !statement.native)
        .filter_map(|statement| {
            let opener = model.expanded_tokens.get(statement.token_range.start)?;
            let closer = model
                .expanded_tokens
                .get(statement.token_range.end.checked_sub(1)?)?;
            Some(Span {
                start: opener.span.start,
                end: closer.span.end,
            })
        })
}

fn conditional(kind: &str) -> bool {
    matches!(kind, "if" | "ifdef" | "ifndef")
}

fn region(span: Span) -> SourceFold {
    SourceFold {
        span,
        comment: false,
    }
}

fn line_start(text: &str, offset: usize) -> usize {
    text[..offset].rfind('\n').map_or(0, |index| index + 1)
}

fn next_line(text: &str, offset: usize) -> usize {
    text[offset..]
        .find('\n')
        .map_or(text.len(), |index| offset + index + 1)
}

fn comment_folds(text: &str) -> Vec<SourceFold> {
    let strings: Vec<_> = tokenize(text)
        .into_iter()
        .filter(|token| matches!(token.kind, TokenKind::String | TokenKind::Latex))
        .map(|token| token.span)
        .collect();
    let mut strings = strings.iter().peekable();
    let bytes = text.as_bytes();
    let mut offset = 0;
    let mut folds = Vec::new();
    while offset < bytes.len() {
        while strings
            .peek()
            .is_some_and(|span| span.end as usize <= offset)
        {
            strings.next();
        }
        if let Some(span) = strings.peek()
            && span.start as usize == offset
        {
            offset = span.end as usize;
        } else if bytes[offset..].starts_with(b"//") || bytes[offset] == b'%' {
            offset = next_line(text, offset);
        } else if bytes[offset..].starts_with(b"/*") {
            let start = offset;
            let Some(end) = text[offset + 2..].find("*/") else {
                break;
            };
            offset += end + 4;
            folds.push(SourceFold {
                span: Span::new(start, offset),
                comment: true,
            });
        } else {
            offset += 1;
        }
    }
    folds
}
