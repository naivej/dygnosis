//! Parse skip for Dynare command statements (option lists are not assignments).

use crate::lexer::{Token, TokenKind};
use crate::span::Span;

/// 0.1 command names that are not in `COMMAND_OPTIONS`.
const PARSE_SKIP_EXTRAS: &[&str] = &["dynasave", "dynatype", "model_diagnostics"];

/// Catalog command or 0.1 extra. A command *statement* is this name plus `(` or `;`, not `=`.
pub(crate) fn is_parse_skip_command(name: &str) -> bool {
    crate::catalog::is_known_command(name)
        || PARSE_SKIP_EXTRAS
            .iter()
            .any(|c| name.eq_ignore_ascii_case(c))
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

/// One span per skip-command token whose next token is `(` or `;` (not `=`),
/// covering optional balanced `(…)` through the closing `;`.
pub(crate) fn command_stmt_spans(tokens: &[Token], src: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        if tok.kind == TokenKind::Ident && is_parse_skip_command(tok.text(src)) {
            let next_kind = tokens.get(i + 1).map(|t| t.kind);
            if next_kind == Some(TokenKind::LParen) || next_kind == Some(TokenKind::Semi) {
                let mut j = i + 1;
                if next_kind == Some(TokenKind::LParen) {
                    j = skip_balanced(tokens, j, TokenKind::LParen, TokenKind::RParen);
                }
                if tokens.get(j).is_some_and(|t| t.kind == TokenKind::Semi) {
                    spans.push(Span {
                        start: tok.span.start,
                        end: tokens[j].span.end,
                    });
                    i = j + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    spans
}
