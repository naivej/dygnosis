//! Fixed value help from retained written tokens and the shared reference.
use super::*;
use crate::engine::analysis::source_inventory::SourceInventory;
use crate::engine::catalog::syntax::{self, SyntaxValueDomain};
use crate::engine::language::lexer::Token;
use std::borrow::Cow;

struct ValueSite<'a> {
    command: &'a str,
    option: &'a str,
    domain: &'static SyntaxValueDomain,
    span: Span,
    literal: Cow<'a, str>,
    form: &'static str,
}

fn site<'a>(
    inventory: &'a SourceInventory,
    text: &'a str,
    byte: u32,
    completion: bool,
) -> Option<ValueSite<'a>> {
    let source = inventory.option_value_at(text, byte)?;
    let domain = syntax::option_record(source.command, source.option)?
        .value_domain
        .as_ref()?;
    let mut tokens = source.tokens;
    let mut start = source.start;
    let mut position = "scalar";
    if tokens
        .first()
        .is_some_and(|token| matches!(token.kind, TokenKind::LParen | TokenKind::LBrack))
    {
        let opener = tokens[0].kind;
        let form = if opener == TokenKind::LParen {
            "paren_list"
        } else {
            "bracket_list"
        };
        position = form;
        if !domain.forms.iter().any(|allowed| allowed == form) {
            return None;
        }
        start = tokens[0].span.end;
        tokens = &tokens[1..];
        let close = if opener == TokenKind::LParen {
            TokenKind::RParen
        } else {
            TokenKind::RBrack
        };
        if tokens.last().is_some_and(|token| token.kind == close) {
            tokens = &tokens[..tokens.len() - 1];
        }
        if opener == TokenKind::LBrack {
            (tokens, start) = bracket_element(tokens, text, byte, completion, start)?;
        } else {
            let mut begin = 0;
            let mut end = tokens.len();
            for (at, token) in tokens.iter().enumerate() {
                if token.kind == TokenKind::Comma {
                    if token.span.start >= byte {
                        end = at;
                        break;
                    }
                    begin = at + 1;
                    start = token.span.end;
                } else if matches!(
                    token.kind,
                    TokenKind::LParen | TokenKind::RParen | TokenKind::LBrack | TokenKind::RBrack
                ) {
                    return None;
                }
            }
            tokens = &tokens[begin..end];
        }
    } else if !domain.forms.iter().any(|form| form == "scalar") {
        return None;
    }
    let span = match tokens {
        [] if completion
            && text
                .get(start as usize..byte as usize)?
                .chars()
                .all(char::is_whitespace) =>
        {
            Span {
                start: byte,
                end: byte,
            }
        }
        [token]
            if literal_token(token)
                && token.span.start <= byte
                && (byte < token.span.end || completion && byte == token.span.end) =>
        {
            token.span
        }
        [quote, prefix]
            if completion
                && quote.kind == TokenKind::String
                && quote.text(text) == "'"
                && prefix.kind == TokenKind::Ident
                && quote.span.end == prefix.span.start
                && prefix.span.start <= byte
                && byte <= prefix.span.end =>
        {
            Span {
                start: quote.span.start,
                end: prefix.span.end,
            }
        }
        [sign, number]
            if matches!(sign.kind, TokenKind::Minus | TokenKind::Plus)
                && matches!(number.kind, TokenKind::Number | TokenKind::Ident)
                && text
                    .get(sign.span.end as usize..number.span.start as usize)?
                    .chars()
                    .all(char::is_whitespace)
                && sign.span.start <= byte
                && (byte < number.span.end || completion && byte == number.span.end) =>
        {
            Span {
                start: sign.span.start,
                end: number.span.end,
            }
        }
        _ => return None,
    };
    let written = text.get(span.start as usize..span.end as usize)?;
    if completion && written.contains(['\r', '\n']) {
        return None;
    }
    let literal = if let [sign, number] = tokens
        && matches!(sign.kind, TokenKind::Minus | TokenKind::Plus)
        && sign.span.end < number.span.start
    {
        Cow::Owned(format!("{}{}", sign.text(text), number.text(text)))
    } else {
        Cow::Borrowed(written)
    };
    Some(ValueSite {
        command: source.command,
        option: source.option,
        domain,
        span,
        literal,
        form: position,
    })
}

/// Pinned bracket vectors permit whitespace as well as comma separators.
fn bracket_element<'a>(
    tokens: &'a [Token],
    text: &str,
    byte: u32,
    completion: bool,
    mut start: u32,
) -> Option<(&'a [Token], u32)> {
    let mut at = 0;
    while at < tokens.len() {
        if byte < tokens[at].span.start {
            return (completion
                && text
                    .get(start as usize..byte as usize)?
                    .chars()
                    .all(char::is_whitespace))
            .then_some((&tokens[at..at], byte));
        }
        if tokens[at].kind == TokenKind::Comma {
            start = tokens[at].span.end;
            at += 1;
            continue;
        }
        let begin = at;
        if matches!(tokens[at].kind, TokenKind::Minus | TokenKind::Plus) {
            at += 1;
            if !tokens
                .get(at)
                .is_some_and(|token| matches!(token.kind, TokenKind::Number | TokenKind::Ident))
            {
                return None;
            }
        } else if tokens[at].kind == TokenKind::String
            && tokens[at].text(text) == "'"
            && completion
            && tokens.get(at + 1).is_some_and(|token| {
                token.kind == TokenKind::Ident && tokens[at].span.end == token.span.start
            })
        {
            at += 1;
        } else if !literal_token(&tokens[at]) {
            return None;
        }
        at += 1;
        let last = tokens[at - 1].span.end;
        if byte < last || completion && byte == last {
            return Some((&tokens[begin..at], start));
        }
        start = last;
    }
    (completion
        && text
            .get(start as usize..byte as usize)?
            .chars()
            .all(char::is_whitespace))
    .then_some((&tokens[tokens.len()..], byte))
}

fn literal_token(token: &Token) -> bool {
    matches!(
        token.kind,
        TokenKind::Ident | TokenKind::Number | TokenKind::String
    )
}

pub(super) fn hover(
    inventory: &SourceInventory,
    text: &str,
    byte: u32,
    index: &LineIndex,
) -> Option<Hover> {
    let site = site(inventory, text, byte, false)?;
    let choice = site.domain.choices.iter().find(|value| {
        value.forms.iter().any(|form| form == site.form) && value.matches_literal(&site.literal)
    })?;
    Some(markdown_hover(
        format!(
            "**`{}: {} = {}`**\n\n{}",
            site.command,
            site.option,
            choice.insert_text,
            choice.help()
        ),
        Some(span_range(index, text, site.span)),
    ))
}

/// Closed domains return only their choices. Open domains retain symbol providers.
pub(super) fn completions(
    inventory: &SourceInventory,
    text: &str,
    byte: u32,
    index: &LineIndex,
) -> Option<(bool, Vec<CompletionItem>)> {
    let site = site(inventory, text, byte, true)?;
    let quoted = site.literal.starts_with('\'');
    let items = site
        .domain
        .choices
        .iter()
        .filter(|choice| {
            !choice.description.is_empty()
                && !matches!(choice.status.as_str(), "unused" | "undocumented")
                && choice.forms.iter().any(|form| form == site.form)
                && (!quoted || choice.token_kind == "quoted")
        })
        .map(|choice| CompletionItem {
            label: choice.value.clone(),
            kind: Some(CompletionItemKind::ENUM_MEMBER),
            detail: Some(format!("{} {} value", site.command, site.option)),
            documentation: Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: choice.help(),
            })),
            filter_text: Some(choice.insert_text.clone()),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range: span_range(index, text, site.span),
                new_text: choice.insert_text.clone(),
            })),
            ..Default::default()
        })
        .collect();
    Some((site.domain.kind == "closed", items))
}
