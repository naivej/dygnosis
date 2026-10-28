//! Source-context projections of the shared pinned syntax reference.

use crate::engine::analysis::source_inventory::SourceInventory;
use crate::engine::catalog::syntax::{self, SyntaxRecord};
use crate::engine::language::lexer::{Token, TokenKind, tokenize};
use crate::engine::language::model::{ExecutionStep, Model, StatementKind};
use crate::engine::language::span::Span;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, Documentation};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Context {
    pub language: &'static str,
    pub parent: Option<String>,
}

/// A point lookup only; retained source facts must prove its lexical context.
pub(crate) fn candidate_word_at(text: &str, byte: u32) -> Option<(&str, Span)> {
    let bytes = text.as_bytes();
    let at = byte as usize;
    let ident = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    if !bytes.get(at).is_some_and(|byte| ident(*byte)) {
        return None;
    }
    let mut start = at;
    while start > 0 && ident(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = at + 1;
    while end < bytes.len() && ident(bytes[end]) {
        end += 1;
    }
    (!bytes[start].is_ascii_digit()).then(|| (&text[start..end], Span::new(start, end)))
}

pub(crate) fn word_at(text: &str, byte: u32) -> Option<(String, Span)> {
    word_at_with_macros(
        text,
        byte,
        &crate::engine::language::macro_expand::scan_macro_tokens(text),
    )
}

pub(crate) fn word_at_with_macros(
    text: &str,
    byte: u32,
    tokens: &[Token],
) -> Option<(String, Span)> {
    if let Some(token) = tokens.iter().find(|token| {
        matches!(token.kind, TokenKind::MacroDir | TokenKind::MacroInterp)
            && token.span.start <= byte
            && byte < token.span.end
    }) {
        let start = token.span.start + 2;
        let end = token.span.end;
        let (word, span) = crate::engine::editing::refs::ident_at(
            text.get(start as usize..end as usize)?,
            byte.checked_sub(start)?,
        )?;
        return Some((
            word,
            Span {
                start: span.start + start,
                end: span.end + start,
            },
        ));
    }
    crate::engine::editing::refs::ident_at(text, byte)
}

pub(crate) fn context(model: &Model, text: &str, byte: u32) -> Option<Context> {
    context_impl(model, text, byte, false)
}

pub(crate) fn signature_context(model: &Model, text: &str, byte: u32) -> Option<Context> {
    context_impl(model, text, byte, true)
}

/// Written context is shared with folding and Outline, including dormant code.
/// It does not borrow semantic facts from a different macro branch.
pub(crate) fn source_context(
    inventory: &SourceInventory,
    text: &str,
    byte: u32,
    allow_literals: bool,
) -> Option<Context> {
    if macro_at(&inventory.macro_tokens, byte) {
        return Some(Context {
            language: "macro_expression",
            parent: None,
        });
    }
    if !lexical_context(&inventory.lexical_tokens, text, byte, allow_literals) {
        return None;
    }
    inventory.context(byte).map(|context| Context {
        language: context.language,
        parent: context.parent,
    })
}

/// A negative proof from the retained written lexer, before any model lookup.
/// Unknown strings and incomplete structure stay eligible: quoted external
/// names and semantic equation tags must still reach their binding owner.
pub(crate) fn hover_may_need_model(inventory: &SourceInventory, text: &str, byte: u32) -> bool {
    use crate::engine::analysis::source_inventory::SourceItemKind;
    // A quoted literal include target has only its document link. The macro
    // directive name remains eligible for syntax help, as do computed inputs.
    let macro_index = inventory
        .macro_tokens
        .partition_point(|token| token.span.end <= byte);
    if let Some(token) = inventory.macro_tokens.get(macro_index).filter(|token| {
        token.kind == TokenKind::MacroDir && token.span.start <= byte && byte < token.span.end
    }) {
        let directive = token.text(text);
        if let Some(filename) = crate::engine::language::parser::literal_include_filename(directive)
            && !filename.contains("@{")
            && let Some(quote) = directive.find(['\"', '\''])
        {
            let start = token.span.start as usize + quote;
            let end = start + filename.len() + 2;
            if start <= byte as usize && (byte as usize) < end {
                return false;
            }
        }
    }
    if !inventory.structure_complete() || macro_at(&inventory.macro_tokens, byte) {
        return true;
    }
    let at = inventory
        .lexical_tokens
        .partition_point(|token| token.span.end <= byte);
    let Some(token) = inventory
        .lexical_tokens
        .get(at)
        .filter(|token| token.span.start <= byte && byte < token.span.end)
    else {
        // Comments and whitespace are lexer trivia. No name occurrence owns them.
        return false;
    };
    if inventory.is_proven_native(token.span) {
        return false;
    }
    if token.kind == TokenKind::Ident {
        return true;
    }
    // Source items are ordered by start, with outer items first at equal
    // starts. A local point lookup is sufficient for the exclusions below;
    // uncertain nesting falls through to the existing model path.
    let item = inventory
        .items
        .partition_point(|item| item.span.start <= byte)
        .checked_sub(1)
        .map(|at| &inventory.items[at]);
    if token.kind == TokenKind::String {
        let macro_at = inventory
            .macro_tokens
            .partition_point(|part| part.span.end <= token.span.start);
        if inventory.macro_tokens.get(macro_at).is_some_and(|part| {
            matches!(part.kind, TokenKind::MacroDir | TokenKind::MacroInterp)
                && part.span.start < token.span.end
        }) {
            return true;
        }
        let key = at.checked_sub(2).and_then(|key| {
            (inventory.lexical_tokens[key].kind == TokenKind::Ident
                && inventory.lexical_tokens[key + 1].kind == TokenKind::Eq)
                .then(|| inventory.lexical_tokens[key].text(text))
        });
        if let Some(key) = key {
            if key.eq_ignore_ascii_case("long_name")
                && item.is_some_and(|item| {
                    item.kind == SourceItemKind::Declaration
                        && item
                            .declaration_metadata
                            .as_ref()
                            .is_some_and(|metadata| metadata.long_name.is_some())
                })
            {
                return false;
            }
            if item.is_some_and(|item| {
                item.kind == SourceItemKind::Equation
                    && !item.template
                    && item.span.start <= token.span.start
                    && token.span.end <= item.span.end
                    && item.tags.keys().any(|tag| tag.eq_ignore_ascii_case(key))
            }) && !key.eq_ignore_ascii_case("endogenous")
                && !key.eq_ignore_ascii_case("mcp")
            {
                return false;
            }
        }
        return true;
    }
    // Source inspection can describe the selection of a dormant item even
    // when its selection is punctuation (for example an equation's opening [).
    item.is_some_and(|item| item.selection.start <= byte && byte < item.selection.end)
}

fn macro_at(tokens: &[Token], byte: u32) -> bool {
    let index = tokens.partition_point(|token| token.span.end < byte);
    tokens.get(index).is_some_and(|token| {
        matches!(token.kind, TokenKind::MacroDir | TokenKind::MacroInterp)
            && token.span.start <= byte
            && byte <= token.span.end
    })
}

fn context_impl(model: &Model, text: &str, byte: u32, allow_literals: bool) -> Option<Context> {
    if macro_at(
        &crate::engine::language::macro_expand::scan_macro_tokens(text),
        byte,
    ) {
        return Some(Context {
            language: "macro_expression",
            parent: None,
        });
    }
    let tokens = tokenize(text);
    if !lexical_context(&tokens, text, byte, allow_literals) {
        return None;
    }
    let statement = model
        .statements
        .iter()
        .filter(|statement| {
            statement.span.start <= byte
                && (byte < statement.span.end
                    || (!statement.complete && byte == statement.span.end))
        })
        .min_by_key(|statement| statement.span.end - statement.span.start);
    if let Some(statement) = statement {
        if statement.name == "verbatim" {
            return None;
        }
        if statement.native {
            return head_recovery(text, byte);
        }
        let header_end = statement
            .opener_range
            .end
            .checked_sub(1)
            .and_then(|index| model.expanded_tokens.get(index))
            .map(|token| token.span.end);
        let body =
            statement.kind == StatementKind::Block && header_end.is_some_and(|end| byte >= end);
        let expression_scope = model
            .exprs
            .iter()
            .filter(|(_, expression)| expression.span.start <= byte && byte <= expression.span.end)
            .filter_map(|(id, expression)| {
                model
                    .constructor_scopes
                    .get(&id)
                    .map(|scope| (expression, scope))
            })
            .min_by_key(|(expression, _)| expression.span.end - expression.span.start)
            .map(|(_, scope)| scope);
        use crate::engine::language::constructor::DataTreeScope;
        let language = if expression_scope.is_some_and(|scope| {
            matches!(
                scope,
                DataTreeScope::Dynamic
                    | DataTreeScope::Planner(_)
                    | DataTreeScope::Epilogue
                    | DataTreeScope::Occbin(_)
                    | DataTreeScope::Heterogeneous(_)
            )
        }) || (body
            && matches!(
                statement.name.as_str(),
                "model" | "model_replace" | "epilogue"
            )) {
            "model_expression"
        } else if body || statement.kind == StatementKind::Assignment {
            "expression"
        } else {
            "top_level"
        };
        return Some(Context {
            language,
            parent: Some(statement.name.clone()),
        });
    }
    if model.execution_steps.iter().any(|step| {
        matches!(step, ExecutionStep::Opaque(span)
        if span.start <= byte && byte < span.end)
    }) {
        return head_recovery(text, byte);
    }
    Some(Context {
        language: "top_level",
        parent: None,
    })
}

fn lexical_context(tokens: &[Token], text: &str, byte: u32, allow_literals: bool) -> bool {
    if allow_literals {
        return true;
    }
    let next = tokens.partition_point(|token| token.span.end <= byte);
    if tokens.get(next).is_some_and(|token| {
        token.kind == TokenKind::String && token.span.start <= byte && byte < token.span.end
    }) {
        return false;
    }
    let previous = tokens[..next]
        .iter()
        .rev()
        .find(|token| token.kind != TokenKind::Eof)
        .map_or(0, |token| token.span.end);
    let gap = text
        .get(previous as usize..byte as usize)
        .unwrap_or_default();
    let line = gap.rsplit(['\r', '\n']).next().unwrap_or_default();
    let in_block_comment = gap
        .rfind("/*")
        .is_some_and(|open| gap.rfind("*/").is_none_or(|close| open > close));
    !line.contains("//") && !line.contains('%') && !in_block_comment
}

fn head_recovery(text: &str, byte: u32) -> Option<Context> {
    let before = text.get(..byte as usize)?;
    let start = before.rfind(['\n', ';']).map_or(0, |at| at + 1);
    let prefix = before[start..].trim_start();
    let tail = text.get(byte as usize..)?.split(['\n', ';']).next()?;
    if !tail
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch.is_whitespace())
    {
        return None;
    }
    if !prefix
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return None;
    }
    syntax::in_context("top_level", None, None)
        .any(|record| {
            record
                .name
                .to_ascii_lowercase()
                .starts_with(&prefix.to_ascii_lowercase())
        })
        .then_some(Context {
            language: "top_level",
            parent: None,
        })
}

pub(crate) fn documentation(record: &SyntaxRecord) -> String {
    let mut text = record.summary.clone();
    if !record.signatures.is_empty() {
        text.push_str("\n\n```dynare\n");
        text.push_str(&record.signatures.join("\n"));
        text.push_str("\n```");
    }
    text
}

/// Catalog help only when retained written context proves the token's role.
pub(crate) fn written_syntax_help(
    inventory: &SourceInventory,
    text: &str,
    byte: u32,
    word: &str,
    span: Span,
    context: &Context,
) -> Option<String> {
    use crate::engine::analysis::source_inventory::SourceItemKind;
    if context.language == "top_level" {
        return None;
    }
    let macro_expression = context.language == "macro_expression";
    let (call, body_start, after_comma) = if macro_expression {
        let item = inventory
            .items
            .iter()
            .filter(|item| item.span.start <= byte && byte < item.span.end)
            .min_by_key(|item| item.span.end - item.span.start)?;
        if item.kind == SourceItemKind::MacroDefinition && item.selection == span {
            return None;
        }
        // Directives are retained as whole tokens. Check only a bounded local
        // prefix and track quoted strings and comments before this word.
        let prefix = text.get(item.span.start as usize..span.start as usize)?;
        if prefix.len() > 256 {
            return None;
        }
        let mut quoted = false;
        let mut previous = 0;
        for byte in prefix.bytes() {
            if !quoted && previous == b'/' && byte == b'/' {
                return None;
            }
            if byte == b'"' {
                quoted = !quoted;
            }
            previous = byte;
        }
        if quoted {
            return None;
        }
        let call = text
            .get(span.end as usize..item.span.end as usize)?
            .bytes()
            .take(256)
            .find(|byte| !byte.is_ascii_whitespace())
            == Some(b'(');
        (call, false, false)
    } else {
        let tokens = &inventory.lexical_tokens;
        let at = tokens.partition_point(|token| token.span.end <= byte);
        if tokens
            .get(at)
            .is_none_or(|token| token.kind != TokenKind::Ident || token.span != span)
        {
            return None;
        }
        let previous = at
            .checked_sub(1)
            .and_then(|index| tokens.get(index))
            .map(|token| token.kind);
        if previous == Some(TokenKind::Dot) {
            return None;
        }
        (
            tokens
                .get(at + 1)
                .is_some_and(|token| token.kind == TokenKind::LParen),
            previous == Some(TokenKind::Semi),
            previous == Some(TokenKind::Comma),
        )
    };
    let docs: Vec<_> = records(context)
        .into_iter()
        .filter(|record| {
            (record.name.eq_ignore_ascii_case(word)
                || record
                    .aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(word)))
                && ((record.kind == "body_keyword"
                    && (body_start
                        || after_comma && context.parent.as_deref() == Some("estimated_params")))
                    || matches!(
                        record.kind.as_str(),
                        "function" | "operator" | "macro_function"
                    ) && call)
        })
        .map(documentation)
        .collect();
    (!docs.is_empty()).then(|| format!("**`{word}`**\n\n{}", docs.join("\n\n")))
}

pub(crate) fn options(command: &str) -> impl Iterator<Item = &'static SyntaxRecord> + '_ {
    syntax::records().iter().filter(move |record| {
        record.kind == "option"
            && record
                .parent
                .as_deref()
                .is_some_and(|parent| parent.eq_ignore_ascii_case(command))
    })
}

pub(crate) fn records(context: &Context) -> Vec<&'static SyntaxRecord> {
    syntax::records()
        .iter()
        .filter(|record| {
            (record
                .contexts
                .iter()
                .any(|language| language == context.language)
                && match context.language {
                    "top_level" => {
                        matches!(record.kind.as_str(), "command" | "block" | "declaration")
                    }
                    "macro_expression" => record.kind == "macro_function",
                    _ => {
                        matches!(record.kind.as_str(), "function" | "operator")
                            && record
                                .name
                                .chars()
                                .next()
                                .is_some_and(|ch| ch.is_ascii_alphabetic())
                    }
                })
                || (record.kind == "body_keyword"
                    && record.parent.as_ref() == context.parent.as_ref()
                    && context.parent.is_some()
                    && context.language != "top_level")
        })
        .collect()
}

pub(crate) fn completions(context: &Context) -> Vec<CompletionItem> {
    records(context)
        .into_iter()
        .map(|record| CompletionItem {
            label: record.name.clone(),
            kind: Some(
                if matches!(
                    record.kind.as_str(),
                    "function" | "macro_function" | "operator"
                ) {
                    CompletionItemKind::FUNCTION
                } else {
                    CompletionItemKind::KEYWORD
                },
            ),
            detail: Some(format!("Dynare {}", record.kind.replace('_', " "))),
            documentation: Some(Documentation::String(documentation(record))),
            ..CompletionItem::default()
        })
        .collect()
}

pub(crate) fn option_values(command: &str, name: &str) -> Vec<CompletionItem> {
    let mut values: Vec<_> = syntax::records()
        .iter()
        .filter(|record| {
            record.kind == "option"
                && record
                    .parent
                    .as_deref()
                    .is_some_and(|parent| parent.eq_ignore_ascii_case(command))
                && record.name.eq_ignore_ascii_case(name)
        })
        .flat_map(|record| &record.value_rules)
        .flat_map(|rule| rule.split(" | "))
        .filter(|value| {
            !value.is_empty() && value.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_')
        })
        .map(str::to_owned)
        .collect();
    values.sort();
    values.dedup();
    values
        .into_iter()
        .map(|value| CompletionItem {
            label: value,
            kind: Some(CompletionItemKind::ENUM_MEMBER),
            detail: Some(format!("{command} {name} value")),
            ..CompletionItem::default()
        })
        .collect()
}

/// Literal written macro targets are useful even in inactive source branches.
/// This supplies source navigation/completion, not an executed macro binding.
pub(crate) fn macro_definitions(text: &str) -> Vec<(String, Span, Span, bool)> {
    crate::engine::language::macro_expand::scan_macro_tokens(text)
        .into_iter()
        .filter(|token| token.kind == TokenKind::MacroDir)
        .filter_map(|token| {
            let source = token.text(text);
            if crate::engine::language::macro_expand::directive_name(source) != "define" {
                return None;
            }
            let directive = source.strip_prefix("@#")?.trim_start();
            let after = directive.get("define".len()..)?;
            let name_source = after.trim_start();
            let start = token.span.start as usize + source.len() - name_source.len();
            let end = name_source
                .bytes()
                .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                .count();
            if end == 0 || name_source.as_bytes()[0].is_ascii_digit() {
                return None;
            }
            let function = name_source[end..].trim_start().starts_with('(');
            Some((
                name_source[..end].into(),
                Span::new(start, start + end),
                token.span,
                function,
            ))
        })
        .collect()
}
