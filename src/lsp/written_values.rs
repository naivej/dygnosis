//! Fixed non-option roles projected from bounded retained written context.
use super::*;
use crate::engine::analysis::source_inventory::{
    SourceInventory, SourceItemKind, SourceOptionValue,
};
use crate::engine::catalog::syntax::{self, SyntaxRecord};
use crate::engine::language::lexer::Token;
use std::borrow::Cow;

struct Site<'a> {
    parent: Option<&'a str>,
    slot: &'static str,
    span: Span,
    literal: Cow<'a, str>,
    selector: Option<&'a str>,
    closed: bool,
}

fn literal_span(
    tokens: &[Token],
    text: &str,
    start: u32,
    byte: u32,
    completion: bool,
) -> Option<Span> {
    match tokens {
        [] if completion
            && byte >= start
            && text.get(start as usize..byte as usize)?.trim().is_empty() =>
        {
            Some(Span {
                start: byte,
                end: byte,
            })
        }
        [token]
            if matches!(
                token.kind,
                TokenKind::Ident
                    | TokenKind::Number
                    | TokenKind::String
                    | TokenKind::Plus
                    | TokenKind::Minus
            ) && token.span.start <= byte
                && (byte < token.span.end || completion && byte == token.span.end) =>
        {
            Some(token.span)
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
            Some(Span {
                start: quote.span.start,
                end: prefix.span.end,
            })
        }
        [sign, number]
            if matches!(sign.kind, TokenKind::Plus | TokenKind::Minus)
                && number.kind == TokenKind::Number
                && text
                    .get(sign.span.end as usize..number.span.start as usize)?
                    .trim()
                    .is_empty()
                && sign.span.start <= byte
                && (byte < number.span.end || completion && byte == number.span.end) =>
        {
            Some(Span {
                start: sign.span.start,
                end: number.span.end,
            })
        }
        _ => None,
    }
}

fn site<'a>(
    inventory: &'a SourceInventory,
    source: Option<&SourceOptionValue<'a>>,
    text: &'a str,
    byte: u32,
    completion: bool,
) -> Option<Site<'a>> {
    if !inventory.structure_complete() || crate::engine::control::cancelled() {
        return None;
    }
    if let Some(site) = macro_site(inventory, text, byte, completion) {
        return Some(site);
    }
    if let Some(source) = source
        && let Some(site) = nested_site(inventory, text, byte, completion, source)
    {
        return Some(site);
    }
    let after = inventory
        .items
        .partition_point(|item| item.span.start <= byte);
    let nearby = &inventory.items[after.saturating_sub(64)..after];
    let tokens = &inventory.lexical_tokens;
    if let Some(item) = nearby
        .iter()
        .rev()
        .find(|item| item.name.as_deref() == Some("change_type") && byte <= item.span.end)
    {
        let head = tokens.partition_point(|token| token.span.start < item.selection.end);
        if tokens.get(head)?.kind != TokenKind::LParen {
            return None;
        }
        let begin = head + 1;
        let end = tokens[begin..].iter().take(4).position(|token| {
            matches!(
                token.kind,
                TokenKind::RParen | TokenKind::Semi | TokenKind::Eof
            )
        })? + begin;
        let span = literal_span(
            &tokens[begin..end],
            text,
            tokens[head].span.end,
            byte,
            completion,
        )?;
        if !inventory.literal_value_anchor(
            Span {
                start: item.selection.start,
                end: span.end.max(byte).max(tokens[head].span.end),
            },
            byte.saturating_sub(1).max(item.selection.start),
        ) {
            return None;
        }
        return Some(Site {
            parent: Some("change_type"),
            slot: "target_type",
            span,
            literal: text.get(span.start as usize..span.end as usize)?.into(),
            selector: None,
            closed: true,
        });
    }
    if let Some(item) = nearby
        .iter()
        .rev()
        .find(|item| item.kind == SourceItemKind::Equation && byte < item.header_end)
    {
        let begin = tokens.partition_point(|token| token.span.start < item.span.start);
        let end = tokens.partition_point(|token| token.span.start < item.header_end);
        if end.saturating_sub(begin) > 256 {
            return None;
        }
        let mut at = tokens[begin..end].partition_point(|token| token.span.end <= byte) + begin;
        if completion
            && at > begin
            && tokens[at - 1].kind == TokenKind::Ident
            && tokens[at - 1].span.end == byte
        {
            at -= 1;
        }
        let token = tokens.get(at)?;
        let (span, before, following) = if token.kind == TokenKind::Ident
            && token.span.start <= byte
            && (byte < token.span.end || completion && byte == token.span.end)
        {
            (token.span, at.checked_sub(1)?, at + 1)
        } else if completion && token.span.start >= byte {
            let before = at.checked_sub(1)?;
            let span = literal_span(&[], text, tokens[before].span.end, byte, completion)?;
            (span, before, at)
        } else {
            return None;
        };
        if !matches!(tokens[before].kind, TokenKind::LBrack | TokenKind::Comma)
            || !tokens
                .get(following)
                .is_some_and(|token| matches!(token.kind, TokenKind::RBrack | TokenKind::Comma))
            || !inventory.literal_value_anchor(
                Span {
                    start: item.span.start,
                    end: item.header_end,
                },
                byte,
            )
        {
            return None;
        }
        return Some(Site {
            parent: Some("model"),
            slot: "equation_tag",
            span,
            literal: text.get(span.start as usize..span.end as usize)?.into(),
            selector: None,
            closed: false,
        });
    }
    let block = nearby.iter().rev().find(|item| {
        item.kind == SourceItemKind::Block
            && item.header_end <= byte
            && byte < item.span.end
            && matches!(
                item.name.as_deref(),
                Some("moment_calibration" | "irf_calibration")
            )
    })?;
    let cursor = tokens.partition_point(|token| token.span.end <= byte);
    let begin = tokens[..cursor]
        .iter()
        .rev()
        .take(128)
        .position(|token| token.kind == TokenKind::Semi)
        .map(|n| cursor - n)?;
    if tokens.get(begin)?.span.start < block.header_end {
        return None;
    }
    let end = tokens[begin..]
        .iter()
        .take(128)
        .position(|token| matches!(token.kind, TokenKind::Semi | TokenKind::Eof))?
        + begin;
    let row = &tokens[begin..end];
    let mut depth = 0;
    let mut commas = Vec::with_capacity(2);
    for (at, token) in row.iter().enumerate() {
        match token.kind {
            TokenKind::LParen => depth += 1,
            TokenKind::RParen if depth > 0 => depth -= 1,
            TokenKind::Comma if depth == 0 => commas.push(at),
            _ => {}
        }
    }
    let [first, second] = commas.as_slice() else {
        return None;
    };
    let parent = block.name.as_deref()?;
    if !symbol_operand(&row[..*first], parent == "irf_calibration")
        || !symbol_operand(&row[first + 1..*second], parent == "moment_calibration")
    {
        return None;
    }
    let span = literal_span(
        &row[second + 1..],
        text,
        row[*second].span.end,
        byte,
        completion,
    )?;
    if row.len() > second + 2 {
        return None;
    }
    if !inventory.literal_value_anchor(
        Span {
            start: row[0].span.start,
            end: tokens[end].span.end,
        },
        byte,
    ) {
        return None;
    }
    Some(Site {
        parent: Some(parent),
        slot: "interval",
        span,
        literal: text.get(span.start as usize..span.end as usize)?.into(),
        selector: None,
        closed: false,
    })
}

fn symbol_operand(tokens: &[Token], timing: bool) -> bool {
    if let [token] = tokens {
        return token.kind == TokenKind::Ident;
    }
    timing
        && (4..=7).contains(&tokens.len())
        && tokens[0].kind == TokenKind::Ident
        && tokens[1].kind == TokenKind::LParen
        && tokens
            .last()
            .is_some_and(|token| token.kind == TokenKind::RParen)
        && tokens[2..tokens.len() - 1].iter().all(|token| {
            matches!(
                token.kind,
                TokenKind::Number | TokenKind::Plus | TokenKind::Minus
            )
        })
}

fn macro_site<'a>(
    inventory: &SourceInventory,
    text: &'a str,
    byte: u32,
    completion: bool,
) -> Option<Site<'a>> {
    let at = inventory
        .macro_tokens
        .partition_point(|token| token.span.end < byte);
    let token = inventory.macro_tokens.get(at)?;
    if !matches!(token.kind, TokenKind::MacroDir | TokenKind::MacroInterp)
        || byte < token.span.start
        || byte > token.span.end
    {
        return None;
    }
    let bytes = text.as_bytes();
    let ident = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let mut start = byte as usize;
    let mut end = start;
    while start > token.span.start as usize + 2
        && byte as usize - start < 256
        && ident(bytes[start - 1])
    {
        start -= 1;
    }
    while end < token.span.end as usize && end - start < 256 && ident(bytes[end]) {
        end += 1;
    }
    if start == end && !completion {
        return None;
    }
    let prefix = text.get(token.span.start as usize + 2..start)?;
    if prefix.len() > 256 || end - start >= 256 {
        return None;
    }
    let expression = if token.kind == TokenKind::MacroInterp {
        prefix
    } else {
        let directive = prefix.trim_start();
        let split = directive.find(char::is_whitespace)?;
        let (name, body) = directive.split_at(split);
        match name.to_ascii_lowercase().as_str() {
            "define" => body.split_once('=')?.1,
            "for" => {
                let separator = body.as_bytes().windows(4).position(|window| {
                    window[0].is_ascii_whitespace()
                        && window[1..3].eq_ignore_ascii_case(b"in")
                        && !ident(window[3])
                })?;
                &body[separator + 3..]
            }
            "if" | "elseif" | "echo" | "error" => body,
            _ => return None,
        }
    };
    let mut quoted = false;
    let mut prior = 0;
    for byte in expression.bytes() {
        if !quoted && prior == b'/' && matches!(byte, b'/' | b'*') {
            return None;
        }
        if byte == b'"' {
            quoted = !quoted;
        }
        prior = byte;
    }
    if quoted || expression.trim_end().ends_with('.') {
        return None;
    }
    let span = Span::new(start, end);
    Some(Site {
        parent: None,
        slot: "boolean_literal",
        span,
        literal: (&text[start..end]).into(),
        selector: None,
        closed: false,
    })
}

fn nested_site<'a>(
    inventory: &SourceInventory,
    text: &'a str,
    byte: u32,
    completion: bool,
    source: &SourceOptionValue<'a>,
) -> Option<Site<'a>> {
    let (slot, key) = match source.option.to_ascii_lowercase().as_str() {
        "optim" => ("optim.NumgradAlgorithm", "NumgradAlgorithm"),
        "posterior_sampler_options" => (
            "posterior_sampler_options.proposal_distribution",
            "proposal_distribution",
        ),
        _ => return None,
    };
    let selector = &syntax::slot_values(Some(source.command), slot)
        .next()?
        .selector
        .as_ref()?
        .option;
    if source.tokens.len() > 8192 {
        return None;
    }
    let mut tokens = source.tokens;
    if tokens.first()?.kind != TokenKind::LParen {
        return None;
    }
    tokens = &tokens[1..];
    if tokens
        .last()
        .is_some_and(|token| token.kind == TokenKind::RParen)
    {
        tokens = &tokens[..tokens.len() - 1];
    }
    let mut begin = 0;
    let mut depth = 0;
    let mut pair_key = None;
    let mut value_start = 0;
    for at in 0..=tokens.len() {
        if at % 128 == 0 && crate::engine::control::cancelled() {
            return None;
        }
        if at < tokens.len() {
            match tokens[at].kind {
                TokenKind::LParen | TokenKind::LBrack => {
                    depth += 1;
                    if depth > 32 {
                        return None;
                    }
                }
                TokenKind::RParen | TokenKind::RBrack => {
                    if depth == 0 {
                        return None;
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
        if at == tokens.len() || depth == 0 && tokens[at].kind == TokenKind::Comma {
            let element = &tokens[begin..at];
            if let Some(pair_key) = pair_key.take() {
                if pair_key == key
                    && let Some(span) = literal_span(element, text, value_start, byte, completion)
                {
                    let literal = if let [sign, number] = element
                        && matches!(sign.kind, TokenKind::Plus | TokenKind::Minus)
                    {
                        Cow::Owned(format!("{}{}", sign.text(text), number.text(text)))
                    } else {
                        Cow::Borrowed(text.get(span.start as usize..span.end as usize)?)
                    };
                    return Some(Site {
                        parent: Some(source.command),
                        slot,
                        span,
                        literal,
                        selector: selector_literal(inventory, text, byte, source, selector),
                        closed: false,
                    });
                }
            } else {
                let [token] = element else {
                    return None;
                };
                if token.kind != TokenKind::String {
                    return None;
                }
                pair_key = Some(token.text(text).strip_prefix('\'')?.strip_suffix('\'')?);
                value_start = tokens
                    .get(at)
                    .map_or(token.span.end, |token| token.span.end);
            }
            begin = at + 1;
        }
    }
    None
}

fn selector_literal<'a>(
    inventory: &SourceInventory,
    text: &'a str,
    byte: u32,
    source: &SourceOptionValue<'a>,
    selector: &str,
) -> Option<&'a str> {
    let tokens = source.header;
    if tokens.len() > 8192 {
        return None;
    }
    let mut depth = 0usize;
    let mut begin = 0;
    let mut found = None;
    for (at, token) in tokens.iter().enumerate() {
        if at % 128 == 0 && crate::engine::control::cancelled() {
            return None;
        }
        if depth == 0
            && matches!(
                token.kind,
                TokenKind::Comma | TokenKind::RParen | TokenKind::Semi | TokenKind::Eof
            )
        {
            if tokens
                .get(begin)
                .is_some_and(|key| key.text(text).eq_ignore_ascii_case(selector))
            {
                let [_, equal, value] = &tokens[begin..at] else {
                    return None;
                };
                if found.is_some()
                    || !matches!(
                        value.kind,
                        TokenKind::Ident | TokenKind::Number | TokenKind::String
                    )
                    || equal.kind != TokenKind::Eq
                    || !inventory.literal_value_anchor(
                        Span {
                            start: source.anchor_start,
                            end: value.span.end.max(byte),
                        },
                        byte,
                    )
                {
                    return None;
                }
                found = Some(value.text(text));
            }
            if token.kind != TokenKind::Comma {
                break;
            }
            begin = at + 1;
            continue;
        }
        match token.kind {
            TokenKind::LParen | TokenKind::LBrack => {
                depth += 1;
                if depth > 32 {
                    return None;
                }
            }
            TokenKind::RParen | TokenKind::RBrack => {
                depth = depth.checked_sub(1)?;
            }
            _ => {}
        }
    }
    found
}

fn selector_matches(record: &SyntaxRecord, site: &Site<'_>) -> bool {
    record.selector.as_ref().is_none_or(|selector| {
        site.selector.is_some_and(|literal| {
            syntax::option_record(site.parent.unwrap_or_default(), &selector.option)
                .and_then(|record| record.value_domain.as_ref())
                .is_some_and(|domain| {
                    domain.choices.iter().any(|value| {
                        value.value == selector.value && value.matches_literal(literal)
                    })
                })
        })
    })
}

pub(super) fn hover(
    inventory: &SourceInventory,
    source: Option<&SourceOptionValue<'_>>,
    text: &str,
    byte: u32,
    index: &LineIndex,
) -> Option<Option<Hover>> {
    let site = site(inventory, source, text, byte, false)?;
    let record = syntax::slot_values(site.parent, site.slot).find(|record| {
        record
            .value
            .as_ref()
            .is_some_and(|value| value.matches_literal(&site.literal))
    });
    if let Some(record) = record {
        let value = record.value.as_ref()?;
        let mut help = value.help();
        if let Some(selector) = &record.selector
            && !selector_matches(record, &site)
        {
            if let Some(actual) = site.selector {
                help.push_str(&format!(
                    "\n\nThis command sets `{}={actual}`, so the condition above is not met.",
                    selector.option
                ));
            } else {
                help.push_str(&format!("\n\nThe written command does not establish one literal `{}` value, so the condition above is not established.", selector.option));
            }
        }
        return Some(Some(markdown_hover(
            format!(
                "**`{}: {} = {}`**\n\n{}",
                site.parent.unwrap_or("macro"),
                site.slot,
                value.insert_text,
                help
            ),
            Some(span_range(index, text, site.span)),
        )));
    }
    site.closed.then_some(None)
}

pub(super) fn completions(
    inventory: &SourceInventory,
    source: Option<&SourceOptionValue<'_>>,
    text: &str,
    byte: u32,
    index: &LineIndex,
) -> Option<(bool, Vec<CompletionItem>)> {
    let site = site(inventory, source, text, byte, true)?;
    if text
        .get(site.span.start as usize..site.span.end as usize)?
        .contains(['\r', '\n'])
    {
        return None;
    }
    // An owner/slot has one controlling selector; resolve it once per request.
    let mut records = syntax::slot_values(site.parent, site.slot).peekable();
    if !selector_matches(records.peek()?, &site) {
        return Some((site.closed, Vec::new()));
    }
    let items = records
        .filter_map(|record| {
            let value = record.value.as_ref()?;
            if value.description.is_empty()
                || matches!(
                    value.status.as_str(),
                    "unused" | "undocumented" | "unavailable"
                )
            {
                return None;
            }
            Some(CompletionItem {
                label: value.value.clone(),
                kind: Some(CompletionItemKind::ENUM_MEMBER),
                detail: Some(format!(
                    "{} {} value",
                    site.parent.unwrap_or("macro"),
                    site.slot
                )),
                documentation: Some(Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: value.help(),
                })),
                filter_text: Some(value.insert_text.clone()),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range: span_range(index, text, site.span),
                    new_text: value.insert_text.clone(),
                })),
                ..Default::default()
            })
        })
        .collect();
    Some((site.closed, items))
}
