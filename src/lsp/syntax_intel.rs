//! Source-context projections of the shared pinned syntax reference.

use crate::engine::catalog::syntax::{self, SyntaxRecord};
use crate::engine::language::lexer::{TokenKind, tokenize};
use crate::engine::language::model::{ExecutionStep, Model, StatementKind};
use crate::engine::language::span::Span;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, Documentation};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Context {
    pub language: &'static str,
    pub parent: Option<String>,
}

pub(crate) fn word_at(text: &str, byte: u32) -> Option<(String, Span)> {
    if let Some(token) = crate::engine::language::macro_expand::scan_macro_tokens(text)
        .into_iter()
        .find(|token| {
            matches!(token.kind, TokenKind::MacroDir | TokenKind::MacroInterp)
                && token.span.start <= byte
                && byte < token.span.end
        })
    {
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

fn context_impl(model: &Model, text: &str, byte: u32, allow_literals: bool) -> Option<Context> {
    if crate::engine::language::macro_expand::scan_macro_tokens(text)
        .iter()
        .any(|token| {
            matches!(token.kind, TokenKind::MacroDir | TokenKind::MacroInterp)
                && token.span.start <= byte
                && byte <= token.span.end
        })
    {
        return Some(Context {
            language: "macro_expression",
            parent: None,
        });
    }
    let tokens = tokenize(text);
    if !allow_literals
        && tokens.iter().any(|token| {
            token.kind == TokenKind::String && token.span.start <= byte && byte < token.span.end
        })
    {
        return None;
    }
    let previous = tokens
        .iter()
        .filter(|token| token.kind != TokenKind::Eof && token.span.end <= byte)
        .map(|token| token.span.end)
        .max()
        .unwrap_or(0);
    let gap = text
        .get(previous as usize..byte as usize)
        .unwrap_or_default();
    let line = gap.rsplit(['\r', '\n']).next().unwrap_or_default();
    let in_block_comment = gap
        .rfind("/*")
        .is_some_and(|open| gap.rfind("*/").is_none_or(|close| open > close));
    if !allow_literals && (line.contains("//") || line.contains('%') || in_block_comment) {
        return None;
    }
    let fallback;
    let model = if model.macro_incomplete() {
        let inert = crate::engine::analysis::folding::inert_source(text);
        fallback = crate::engine::language::parser::parse_expanded_controlled(
            &inert,
            tokenize(&inert),
            None,
            model.macro_work_limit,
        )
        .0;
        &fallback
    } else {
        model
    };
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
