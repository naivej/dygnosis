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
    names::typed_label(kind).unwrap_or("Name")
}

pub(super) fn root_view(inner: &mut Inner, root: &Url) -> Option<Arc<SemanticModel>> {
    let revision = inner.root_revision(root)?;
    if let Some(view) = inner.semantic_views.get(root)
        && view.revision == revision
    {
        return Some(Arc::clone(view));
    }
    inner.semantic_views.remove(root);
    let view =
        Arc::new(SemanticModel::capture(&mut inner.workspace, root.as_str(), revision).ok()?);
    inner.semantic_views.insert(root.clone(), Arc::clone(&view));
    inner.bound_workspace_cache();
    Some(view)
}

fn views(inner: &mut Inner, uri: &Url) -> Option<Vec<Arc<SemanticModel>>> {
    if inner.context_ambiguous(uri) {
        return None;
    }
    let mut roots = inner.context_roots(uri);
    if roots.is_empty() {
        roots.push(uri.clone());
    }
    roots
        .into_iter()
        .map(|root| root_view(inner, &root))
        .collect()
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    fn snapshot(inner: &mut Inner, uri: &Url) -> Arc<SemanticModel> {
        views(inner, uri).unwrap().remove(0)
    }

    #[test]
    fn semantic_cache_tracks_saved_includes_missing_inputs_and_settings() {
        // Guards reusing a snapshot after a saved include is created, changed
        // or removed, or after the root's macro allowance changes.
        let directory = std::env::temp_dir().join(format!(
            "dygnosis-semantic-cache-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let child = directory.join("decl.inc");
        let root = Url::from_file_path(directory.join("root.mod")).unwrap();
        let mut inner = Inner::default();
        inner
            .workspace
            .update_document(root.as_str(), "@#include \"decl.inc\"\n");
        let missing = snapshot(&mut inner, &root);
        assert!(missing.select(Selector::Name("first")).unwrap().is_none());
        std::fs::write(&child, "database first;").unwrap();
        let first = snapshot(&mut inner, &root);
        assert!(!Arc::ptr_eq(&missing, &first));
        assert!(first.select(Selector::Name("first")).unwrap().is_some());
        assert!(Arc::ptr_eq(&first, &snapshot(&mut inner, &root)));
        std::fs::write(&child, "database second_name;").unwrap();
        let second = snapshot(&mut inner, &root);
        assert!(second.select(Selector::Name("first")).unwrap().is_none());
        assert!(
            second
                .select(Selector::Name("second_name"))
                .unwrap()
                .is_some()
        );
        inner.settings.apply(&json!({"macroWorkLimit": 1}));
        let limited = snapshot(&mut inner, &root);
        assert!(!Arc::ptr_eq(&second, &limited));
        assert!(
            limited
                .select(Selector::Name("second_name"))
                .unwrap()
                .is_none()
        );
        inner.settings.apply(&json!({"macroWorkLimit": 2_000_000}));
        assert!(
            snapshot(&mut inner, &root)
                .select(Selector::Name("second_name"))
                .unwrap()
                .is_some()
        );
        std::fs::remove_file(child).unwrap();
        assert!(
            snapshot(&mut inner, &root)
                .select(Selector::Name("second_name"))
                .unwrap()
                .is_none()
        );
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn semantic_cache_uses_the_workspace_retention_policy() {
        // Guards keeping every closed model's captured source and bindings.
        let mut inner = Inner::default();
        let first = Url::parse("file:///semantic-cache-0.mod").unwrap();
        inner
            .workspace
            .update_document(first.as_str(), "database first;");
        let old = snapshot(&mut inner, &first);
        for index in 1..=super::super::workspace_checks::RETAINED_REQUEST_ROOTS {
            let root = Url::parse(&format!("file:///semantic-cache-{index}.mod")).unwrap();
            inner
                .workspace
                .update_document(root.as_str(), "database later;");
            snapshot(&mut inner, &root);
        }
        assert!(!inner.semantic_views.contains_key(&first));
        assert_eq!(Arc::strong_count(&old), 1);
    }

    #[tokio::test]
    async fn named_hover_reuses_current_semantics_and_refreshes_after_edit() {
        // Guards rebuilding the same binding snapshot for each hover, and
        // returning an obsolete named binding after an unsaved edit.
        let (service, _socket) = new_service();
        let server = service.inner();
        let uri = Url::parse("file:///semantic-cache.mod").unwrap();
        let text = "var y;var_model(model_name=forecast,eqtags=['eq']);model;[name='eq']y=0;end;";
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "dynare".into(),
                    version: 1,
                    text: text.into(),
                },
            })
            .await;
        let pos = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position::new(0, text.find("forecast").unwrap() as u32),
        };
        let before = views(&mut server.lock_inner(), &uri).unwrap().remove(0);
        let result = server.hover_at(&pos).expect("named hover");
        let after = views(&mut server.lock_inner(), &uri).unwrap().remove(0);
        assert!(
            Arc::ptr_eq(&before, &after),
            "unchanged hover rebuilt the semantic snapshot"
        );
        assert!(
            matches!(result.contents, HoverContents::Markup(ref text) if text.value.contains("forecast"))
        );
        server
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 2,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: text.replace("forecast", "new_forecast"),
                }],
            })
            .await;
        let result = server.hover_at(&pos).expect("updated named hover");
        let updated = views(&mut server.lock_inner(), &uri).unwrap().remove(0);
        assert!(!Arc::ptr_eq(&before, &updated));
        assert!(
            matches!(result.contents, HoverContents::Markup(ref text) if text.value.contains("new_forecast"))
        );
    }
}

pub(super) struct TypedSites {
    sites: HashMap<Span, (NameRole, bool)>,
    source_owner: Arc<SemanticModel>,
}

impl TypedSites {
    pub fn raw_source_tokens(
        &self,
        file: &str,
    ) -> Option<&[crate::engine::language::lexer::Token]> {
        self.source_owner.raw_source_tokens(file)
    }
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
    sites_in_range(inner, uri, None)
}

pub(super) fn sites_in_range(
    inner: &mut Inner,
    uri: &Url,
    range: Option<Span>,
) -> Option<TypedSites> {
    let selected =
        |span: Span| range.is_none_or(|range| range.start <= span.start && span.start < range.end);
    let file = crate::engine::workspace::include_resolver::normalize_uri(uri.as_str());
    let views = views(inner, uri)?;
    let mut groups = views.iter().map(|view| {
        let mut sites: HashMap<Span, Option<(NameRole, bool)>> = HashMap::new();
        let ordinary: HashMap<_, _> = view
            .binding_inventory()
            .iter()
            .filter_map(|binding| {
                let role = match binding.kind.as_str() {
                    "var" => NameRole::Endogenous,
                    "varexo" | "varexo_det" => NameRole::Exogenous,
                    "parameters" => NameRole::Parameter,
                    _ => return None,
                };
                Some((binding.name.as_str(), role))
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
                && selected(*span)
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
        for binding in view.binding_inventory() {
            let Some(role) = role(&binding.kind) else {
                continue;
            };
            if binding.declaration.is_none() {
                continue;
            }
            for reference in view.references(binding, true) {
                let Some((target, span)) = reference.written else {
                    continue;
                };
                if target != file
                    || !selected(span)
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
    Some(TypedSites {
        sites,
        source_owner: Arc::clone(&views[0]),
    })
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
    context: &syntax_intel::Context,
) -> Vec<CompletionItem> {
    if context.language != "model_expression" {
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

fn slot(context: &syntax_intel::Context, text: &str, byte: u32) -> Option<Slot> {
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
pub(super) fn completions(
    inner: &mut Inner,
    uri: &Url,
    byte: u32,
    context: &syntax_intel::Context,
) -> Option<Vec<CompletionItem>> {
    let text = inner.document(uri)?.text.clone();
    let slot = slot(context, &text, byte)?;
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
