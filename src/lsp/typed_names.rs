//! Editor presentation of accepted typed names from the shared binding owner.

use super::*;
use crate::engine::analysis::symbols::{Binding, Selector, SemanticModel};

fn role(kind: &str) -> Option<NameRole> {
    match kind {
        "heterogeneity_dimension" => Some(NameRole::Dimension),
        "database" => Some(NameRole::Database),
        "var_model" | "trend_component_model" | "var_expectation_model" | "pac_model" => {
            Some(NameRole::NamedModel)
        }
        "named_subsample" => Some(NameRole::Subsample),
        "trend_var" | "log_trend_var" => Some(NameRole::Trend),
        "external_function" => Some(NameRole::ExternalFunction),
        _ => None,
    }
}

fn label(kind: &str) -> &'static str {
    match kind {
        "heterogeneity_dimension" => "Heterogeneity dimension",
        "database" => "Database",
        "var_model" => "VAR auxiliary model",
        "trend_component_model" => "Trend component auxiliary model",
        "var_expectation_model" => "VAR expectation model",
        "pac_model" => "PAC model",
        "named_subsample" => "Named subsample",
        "trend_var" => "Trend variable",
        "log_trend_var" => "Log trend variable",
        "external_function" => "External function",
        _ => "Name",
    }
}

fn views(inner: &mut Inner, uri: &Url) -> Option<Vec<SemanticModel>> {
    if inner.context_ambiguous(uri) {
        return None;
    }
    let mut roots = inner.context_roots(uri);
    if roots.is_empty() {
        roots.push(uri.clone());
    }
    roots
        .into_iter()
        .map(|root| rename::capture(inner, &root).ok())
        .collect()
}

pub(super) struct TypedSites {
    sites: HashMap<Span, (NameRole, bool)>,
}

impl TypedSites {
    pub fn at(&self, span: Span) -> Option<(NameRole, bool)> {
        self.sites.get(&span).copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = (Span, NameRole, bool)> + '_ {
        self.sites
            .iter()
            .map(|(&span, &(role, declaration))| (span, role, declaration))
    }
}

/// Literal accepted sites only. Several owners must agree on the displayed role.
pub(super) fn sites(inner: &mut Inner, uri: &Url) -> Option<TypedSites> {
    let file = crate::engine::workspace::include_resolver::normalize_uri(uri.as_str());
    let views = views(inner, uri)?;
    let mut groups = views.iter().map(|view| {
        let mut sites: HashMap<Span, Option<(NameRole, bool)>> = HashMap::new();
        let ordinary: HashMap<_, _> = view
            .bindings()
            .into_iter()
            .filter_map(|binding| {
                let role = match binding.kind.as_str() {
                    "var" => NameRole::Endogenous,
                    "varexo" | "varexo_det" => NameRole::Exogenous,
                    "parameters" => NameRole::Parameter,
                    _ => return None,
                };
                Some((binding.name, role))
            })
            .collect();
        for (name, reference) in view.accepted_ordinary_sites() {
            let Some(role) = ordinary.get(name).copied() else {
                continue;
            };
            let Some((target, span)) = &reference.written else {
                continue;
            };
            if target == &file
                && view
                    .sources
                    .get(target)
                    .and_then(|text| text.get(span.start as usize..span.end as usize))
                    == Some(name)
            {
                let value = (role, reference.declaration);
                sites
                    .entry(*span)
                    .and_modify(|existing| {
                        if *existing != Some(value) {
                            *existing = None;
                        }
                    })
                    .or_insert(Some(value));
            }
        }
        for binding in view.bindings() {
            let Some(role) = role(&binding.kind) else {
                continue;
            };
            if binding.declaration.is_none() {
                continue;
            }
            for reference in view.references(&binding, true) {
                let Some((target, span)) = reference.written else {
                    continue;
                };
                if target != file
                    || view
                        .sources
                        .get(&target)
                        .and_then(|text| text.get(span.start as usize..span.end as usize))
                        != Some(binding.name.as_str())
                {
                    continue;
                }
                let value = (role, reference.declaration);
                sites
                    .entry(span)
                    .and_modify(|existing| {
                        if *existing != Some(value) {
                            *existing = None;
                        }
                    })
                    .or_insert(Some(value));
            }
        }
        sites
            .into_iter()
            .filter_map(|(span, value)| value.map(|value| (span, value)))
            .collect::<HashMap<_, _>>()
    });
    let mut sites = groups.next()?;
    for group in groups {
        sites.retain(|span, value| group.get(span) == Some(value));
    }
    Some(TypedSites { sites })
}

fn description(binding: &Binding) -> String {
    let mut text = format!(
        "**{}** {}",
        label(&binding.kind),
        names::code(&binding.name)
    );
    if let Some(namespace) = &binding.namespace {
        text.push_str(&format!("\n\nOwner: {}", names::code(namespace)));
    }
    if binding.declaration.is_none() {
        text.push_str("\n\nNo accepted declaration is available for this name.");
    } else if let Some(record) = crate::engine::catalog::syntax::records()
        .iter()
        .find(|record| {
            record.name == binding.kind
                && matches!(record.kind.as_str(), "command" | "block" | "declaration")
        })
    {
        text.push_str("\n\n");
        text.push_str(&record.summary);
    }
    text
}

fn completion(binding: &Binding) -> CompletionItem {
    CompletionItem {
        label: binding.name.clone(),
        kind: Some(if binding.kind == "external_function" {
            CompletionItemKind::FUNCTION
        } else if matches!(binding.kind.as_str(), "trend_var" | "log_trend_var") {
            CompletionItemKind::VARIABLE
        } else {
            CompletionItemKind::REFERENCE
        }),
        detail: Some(label(&binding.kind).into()),
        documentation: Some(Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: description(binding),
        })),
        ..Default::default()
    }
}

/// Additional declared expression names; caller keeps ordinary/local completion.
pub(super) fn expression_completions(
    inner: &mut Inner,
    uri: &Url,
    byte: u32,
) -> Vec<CompletionItem> {
    let Some(text) = inner.document(uri).map(|document| document.text.clone()) else {
        return Vec::new();
    };
    if !inner
        .syntax_context(uri, &text, byte, false)
        .is_some_and(|context| context.language == "model_expression")
    {
        return Vec::new();
    }
    let Some(views) = views(inner, uri) else {
        return Vec::new();
    };
    let mut groups = views.iter().map(|view| {
        view.bindings()
            .into_iter()
            .filter(|binding| {
                binding.declaration.is_some()
                    && matches!(
                        binding.kind.as_str(),
                        "trend_var" | "log_trend_var" | "external_function"
                    )
            })
            .map(|binding| completion(&binding))
            .collect::<Vec<_>>()
    });
    let Some(mut items) = groups.next() else {
        return Vec::new();
    };
    for group in groups {
        items.retain(|item| group.contains(item));
    }
    items.sort_by(|left, right| left.label.cmp(&right.label));
    items.dedup();
    items
}

/// The outer option marks a typed site, including an ambiguous or unavailable result.
pub(super) fn hover(inner: &mut Inner, pos: &TextDocumentPositionParams) -> Option<Option<Hover>> {
    let uri = &pos.text_document.uri;
    let text = inner.document(uri)?.text.clone();
    let index = LineIndex::new(&crate::engine::language::parser::normalize_newlines(&text));
    let byte = index.offset_utf16(&text, span_pos(pos.position));
    let file = crate::engine::workspace::include_resolver::normalize_uri(uri.as_str());
    let views = views(inner, uri)?;
    let mut selected = Vec::new();
    let mut typed = false;
    for view in &views {
        match view.select(Selector::Target { file: &file, byte }) {
            Ok(Some(binding)) if role(&binding.kind).is_some() => {
                typed = true;
                let span = view
                    .references(&binding, true)
                    .into_iter()
                    .filter_map(|reference| reference.written)
                    .find_map(|(target, span)| {
                        (target == file && span.start <= byte && byte < span.end).then_some(span)
                    });
                selected.push(span.map(|span| (binding, span)));
            }
            Err(error)
                if error
                    .candidates
                    .iter()
                    .any(|binding| role(&binding.kind).is_some()) =>
            {
                typed = true;
                selected.push(None);
            }
            _ => selected.push(None),
        }
    }
    if !typed {
        return None;
    }
    let Some(Some((binding, span))) = selected.first() else {
        return Some(None);
    };
    if !selected.iter().all(|other| {
        other.as_ref().is_some_and(|(other, other_span)| {
            other.kind == binding.kind
                && other.name == binding.name
                && other.namespace == binding.namespace
                && other.declaration == binding.declaration
                && other_span == span
        })
    }) {
        return Some(None);
    }
    Some(Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: description(binding),
        }),
        range: Some(span_range(&index, &text, *span)),
    }))
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Slot {
    Kind(&'static str),
    Auxiliary {
        preferred: &'static str,
        fallback: &'static str,
    },
    Generated,
}

fn slot(inner: &mut Inner, uri: &Url, text: &str, byte: u32) -> Option<Slot> {
    let context = inner.syntax_context(uri, text, byte, false)?;
    if context.language == "macro_expression" {
        return None;
    }
    if let Some((command, Some(option))) = signature_help::option_context(text, byte) {
        return match option.to_ascii_lowercase().as_str() {
            "heterogeneity" => Some(Slot::Kind("heterogeneity_dimension")),
            "auxiliary_model_name"
                if matches!(command.as_str(), "var_expectation_model" | "pac_model") =>
            {
                Some(if command == "pac_model" {
                    Slot::Auxiliary {
                        preferred: "trend_component_model",
                        fallback: "var_model",
                    }
                } else {
                    Slot::Auxiliary {
                        preferred: "var_model",
                        fallback: "trend_component_model",
                    }
                })
            }
            "model_name" | "auxname"
                if matches!(
                    command.as_str(),
                    "var_model" | "trend_component_model" | "var_expectation_model" | "pac_model"
                ) =>
            {
                Some(Slot::Generated)
            }
            _ => None,
        };
    }
    // Incomplete argument names need a lexical slot, but the surrounding
    // source must already have a Dynare context from the parser/source map.
    let tokens = tokenize(text);
    let mut stack = Vec::new();
    for (index, token) in tokens
        .iter()
        .enumerate()
        .take_while(|(_, token)| token.span.start < byte)
    {
        match token.kind {
            TokenKind::LParen => stack.push(index),
            TokenKind::RParen => {
                stack.pop();
            }
            TokenKind::Semi => stack.clear(),
            _ => {}
        }
    }
    let open = *stack.last()?;
    let callee = tokens.get(open.checked_sub(1)?)?;
    if callee.kind != TokenKind::Ident {
        return None;
    }
    if tokens[open + 1..]
        .iter()
        .take_while(|token| token.span.start < byte)
        .any(|token| !matches!(token.kind, TokenKind::Ident | TokenKind::Eof))
    {
        return None;
    }
    match callee.text(text).to_ascii_lowercase().as_str() {
        "var_expectation" if context.language == "model_expression" => {
            Some(Slot::Kind("var_expectation_model"))
        }
        "pac_expectation" | "pac_target_nonstationary"
            if context.language == "model_expression" =>
        {
            Some(Slot::Kind("pac_model"))
        }
        "pac_target_info" => Some(Slot::Kind("pac_model")),
        _ => None,
    }
}

/// Some(empty) is a typed name slot with no valid declared candidates.
pub(super) fn completions(inner: &mut Inner, uri: &Url, byte: u32) -> Option<Vec<CompletionItem>> {
    let text = inner.document(uri)?.text.clone();
    let slot = slot(inner, uri, &text, byte)?;
    let views = views(inner, uri)?;
    let mut groups = views.iter().map(|view| {
        let bindings = view.bindings();
        let preferred_names: HashSet<_> = if let Slot::Auxiliary { preferred, .. } = slot {
            bindings
                .iter()
                .filter(|binding| binding.kind == preferred && binding.declaration.is_some())
                .map(|binding| binding.name.clone())
                .collect()
        } else {
            HashSet::new()
        };
        let mut candidates: Vec<_> = bindings
            .into_iter()
            .filter(|binding| {
                binding.declaration.is_some()
                    && match slot {
                        Slot::Kind(kind) => binding.kind == kind,
                        Slot::Auxiliary {
                            preferred,
                            fallback,
                        } => {
                            binding.kind == preferred
                                || binding.kind == fallback
                                    && !preferred_names.contains(&binding.name)
                        }
                        Slot::Generated => false,
                    }
            })
            .map(|binding| completion(&binding))
            .collect();
        candidates.sort_by(|left, right| {
            left.label
                .cmp(&right.label)
                .then_with(|| left.detail.cmp(&right.detail))
        });
        candidates.dedup();
        candidates
    });
    let mut items = groups.next()?;
    for group in groups {
        items.retain(|item| group.contains(item));
    }
    Some(items)
}
