//! Scoped model-local editor requests from the shared parser facts.

use super::*;
use crate::intern::Name;
use crate::model_locals::{LocalBinding, ModelLocals};

struct LocalView {
    root: Url,
    model: Model,
    facts: ModelLocals,
    revision: String,
    complete: bool,
    standalone: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Binding {
    name: Name,
    dimension: Option<Name>,
    declaration: Option<usize>,
    definition: Option<usize>,
}

impl LocalView {
    fn binding(
        &self,
        definition: Option<usize>,
        declaration: Option<usize>,
        name: Name,
        dimension: Option<Name>,
    ) -> Binding {
        Binding {
            name,
            dimension,
            declaration,
            definition,
        }
    }
}

fn views(inner: &mut Inner, uri: &Url) -> Vec<LocalView> {
    let mut roots = inner.known_owner_roots(uri);
    if roots.is_empty() && is_model_root(uri) {
        roots.push(uri.clone());
    }
    let standalone = roots.is_empty();
    if standalone {
        roots.push(uri.clone());
    }
    roots
        .into_iter()
        .filter_map(|root| {
            let revision = inner.root_revision(&root)?;
            let model = if standalone {
                inner.workspace.get_model(root.as_str())?
            } else {
                inner.workspace.get_effective_model(root.as_str())?
            }
            .clone();
            let complete = if standalone {
                crate::model_map::parser_complete(&model)
            } else {
                let report = inner.workspace.expand_report(root.as_str())?;
                report.complete
                    && report.model_map.complete
                    && inner.workspace.includes_complete(root.as_str())
            };
            let facts = ModelLocals::collect(&model);
            Some(LocalView {
                root,
                model,
                facts,
                revision,
                complete,
                standalone,
            })
        })
        .collect()
}

fn written(inner: &mut Inner, view: &LocalView, span: Span) -> Option<(String, Span)> {
    if view.standalone {
        Some((
            crate::include_resolver::normalize_uri(view.root.as_str()),
            span,
        ))
    } else {
        inner
            .workspace
            .map_effective_origin(view.root.as_str(), span)
    }
}

fn location(inner: &mut Inner, view: &LocalView, span: Span) -> Option<Location> {
    let (file, span) = written(inner, view, span)?;
    let uri = naming_edit_url(inner, &view.root, &file)?;
    let source = inner.workspace.get_source(&file)?;
    source.get(span.start as usize..span.end as usize)?;
    Some(Location::new(
        uri,
        span_range(&LineIndex::new(source), source, span),
    ))
}

fn at(inner: &mut Inner, view: &LocalView, uri: &Url, byte: u32, span: Span) -> bool {
    written(inner, view, span).is_some_and(|(file, span)| {
        file == crate::include_resolver::normalize_uri(uri.as_str())
            && span.start <= byte
            && byte <= span.end
    })
}

fn selected(inner: &mut Inner, view: &LocalView, uri: &Url, byte: u32) -> Vec<Binding> {
    let mut result = Vec::new();
    for (id, declaration) in view.facts.declarations.iter().enumerate() {
        if !at(inner, view, uri, byte, declaration.span) {
            continue;
        }
        let definitions: Vec<_> = view
            .facts
            .definitions
            .iter()
            .enumerate()
            .filter(|(_, definition)| definition.name == declaration.name)
            .collect();
        if definitions.is_empty() {
            result.push(view.binding(None, Some(id), declaration.name, None));
        } else {
            result.extend(definitions.into_iter().map(|(index, definition)| {
                view.binding(Some(index), Some(id), definition.name, definition.dimension)
            }));
        }
    }
    for (id, definition) in view.facts.definitions.iter().enumerate() {
        if at(inner, view, uri, byte, definition.target_span) {
            result.push(view.binding(
                Some(id),
                definition.declaration,
                definition.name,
                definition.dimension,
            ));
        }
    }
    for usage in &view.facts.uses {
        if at(inner, view, uri, byte, usage.span) {
            result.push(view.binding(
                usage.definition,
                usage.declaration,
                usage.name,
                usage.dimension,
            ));
        }
    }
    let mut unique = Vec::new();
    for binding in result {
        if !unique.contains(&binding) {
            unique.push(binding);
        }
    }
    unique
}

fn expression(view: &LocalView, binding: Binding) -> Option<&str> {
    let definition = view.facts.definitions.get(binding.definition?)?;
    Some(
        view.model
            .written_equations
            .get(definition.equation_index)?
            .equation
            .rhs
            .as_str(),
    )
}

fn description(
    inner: &mut Inner,
    view: &LocalView,
    binding: Binding,
    preferences: &PresentationSettings,
) -> String {
    let mut text = format!(
        "**Model-local variable** {}",
        crate::server_names::code(view.model.name(binding.name))
    );
    if preferences.name_details.tex
        && let Some(tex) = binding
            .declaration
            .and_then(|id| view.facts.declarations[id].tex_name.as_ref())
    {
        text.push_str(&format!("\n\nTeX: {}", crate::server_names::code(tex)));
    }
    if let Some(expression) = expression(view, binding) {
        let definition = &view.facts.definitions[binding.definition.unwrap()];
        let row = &view.model.written_equations[definition.equation_index];
        let expanded = row.equation.active_tokens.clone().any(|index| {
            let token = &view.model.expanded_tokens[index];
            token.lexeme.as_ref().is_some_and(|lexeme| {
                written(inner, view, token.span)
                    .and_then(|(file, span)| {
                        inner
                            .workspace
                            .get_source(&file)
                            .and_then(|source| source.get(span.start as usize..span.end as usize))
                            .map(str::to_owned)
                    })
                    .as_deref()
                    != Some(lexeme.as_str())
            })
        });
        text.push_str(if expanded {
            "\n\nExpanded expression: "
        } else {
            "\n\nExpression: "
        });
        text.push_str(&crate::server_names::code(expression));
        if let Some(target) = location(inner, view, definition.target_span) {
            let line = target.range.start.line + 1;
            let column = target.range.start.character + 1;
            text.push_str(&format!(
                "\n\n[Definition]({}#L{line},{column})",
                target.uri
            ));
        }
    }
    text
}

/// The outer option distinguishes a local site from the ordinary request path.
pub(super) fn hover(inner: &mut Inner, pos: &TextDocumentPositionParams) -> Option<Option<Hover>> {
    let text = inner.document(&pos.text_document.uri)?.text.clone();
    let index = LineIndex::new(&text);
    let byte = index.offset_utf16(&text, span_pos(pos.position));
    let (_, span) = ident_at(&text, byte)?;
    let preferences = inner.presentation_for(&pos.text_document.uri);
    let mut descriptions = Vec::new();
    let mut found = false;
    for view in views(inner, &pos.text_document.uri) {
        let bindings = selected(inner, &view, &pos.text_document.uri, byte);
        found |= !bindings.is_empty();
        if bindings.len() != 1 {
            descriptions.push(None);
            continue;
        }
        descriptions.push(Some(description(inner, &view, bindings[0], &preferences)));
    }
    if !found {
        return None;
    }
    let first = descriptions.first().cloned().flatten();
    let range = span_range(&index, &text, span);
    Some(
        first
            .filter(|first| {
                descriptions
                    .iter()
                    .all(|other| other.as_ref() == Some(first))
            })
            .map(|markdown| markdown_hover(markdown, Some(range))),
    )
}

pub(super) fn definition(
    inner: &mut Inner,
    pos: &TextDocumentPositionParams,
    declaration: bool,
) -> Option<Option<GotoDefinitionResponse>> {
    let text = inner.document(&pos.text_document.uri)?.text.clone();
    let byte = LineIndex::new(&text).offset_utf16(&text, span_pos(pos.position));
    let mut found = false;
    let mut targets = Vec::new();
    for view in views(inner, &pos.text_document.uri) {
        for binding in selected(inner, &view, &pos.text_document.uri, byte) {
            found = true;
            let span = if declaration {
                binding
                    .declaration
                    .map(|id| view.facts.declarations[id].span)
                    .or_else(|| {
                        binding
                            .definition
                            .map(|id| view.facts.definitions[id].target_span)
                    })
            } else {
                binding
                    .definition
                    .map(|id| view.facts.definitions[id].target_span)
            };
            if let Some(target) = span.and_then(|span| location(inner, &view, span))
                && !targets.contains(&target)
            {
                targets.push(target);
            }
        }
    }
    found.then(|| match targets.len() {
        0 => None,
        1 => Some(GotoDefinitionResponse::Scalar(targets.remove(0))),
        _ => Some(GotoDefinitionResponse::Array(targets)),
    })
}

fn sites(view: &LocalView, binding: Binding, include_declaration: bool) -> Vec<(Span, bool)> {
    let mut sites = Vec::new();
    if include_declaration {
        if let Some(id) = binding.declaration {
            sites.push((view.facts.declarations[id].span, true));
        }
        if let Some(id) = binding.definition {
            sites.push((view.facts.definitions[id].target_span, true));
        }
    }
    sites.extend(
        view.facts
            .uses
            .iter()
            .filter(|usage| {
                usage.name == binding.name
                    && usage.dimension == binding.dimension
                    && usage.definition == binding.definition
                    && usage.declaration == binding.declaration
            })
            .map(|usage| (usage.span, false)),
    );
    sites
}

pub(super) fn references(
    inner: &mut Inner,
    pos: &TextDocumentPositionParams,
    include_declaration: bool,
) -> Option<Option<Vec<Location>>> {
    let text = inner.document(&pos.text_document.uri)?.text.clone();
    let byte = LineIndex::new(&text).offset_utf16(&text, span_pos(pos.position));
    let mut found = false;
    let mut result = Vec::new();
    let views = views(inner, &pos.text_document.uri);
    for view in &views {
        let bindings = selected(inner, view, &pos.text_document.uri, byte);
        found |= !bindings.is_empty();
        if views.len() != 1 || bindings.len() != 1 {
            continue;
        }
        for (span, _) in sites(view, bindings[0], include_declaration) {
            if let Some(target) = location(inner, view, span)
                && !result.contains(&target)
            {
                result.push(target);
            }
        }
    }
    found.then_some((!result.is_empty()).then_some(result))
}

pub(super) fn highlights(
    inner: &mut Inner,
    pos: &TextDocumentPositionParams,
) -> Option<Option<Vec<DocumentHighlight>>> {
    let text = inner.document(&pos.text_document.uri)?.text.clone();
    let byte = LineIndex::new(&text).offset_utf16(&text, span_pos(pos.position));
    let views = views(inner, &pos.text_document.uri);
    let mut found = false;
    let mut result = Vec::new();
    for view in &views {
        let bindings = selected(inner, view, &pos.text_document.uri, byte);
        found |= !bindings.is_empty();
        if views.len() != 1 || bindings.len() != 1 {
            continue;
        }
        for (span, write) in sites(view, bindings[0], true) {
            if let Some(target) =
                location(inner, view, span).filter(|target| target.uri == pos.text_document.uri)
            {
                let highlight = DocumentHighlight {
                    range: target.range,
                    kind: Some(if write {
                        DocumentHighlightKind::WRITE
                    } else {
                        DocumentHighlightKind::READ
                    }),
                };
                if !result.contains(&highlight) {
                    result.push(highlight);
                }
            }
        }
    }
    found.then_some((!result.is_empty()).then_some(result))
}

fn editable(inner: &mut Inner, view: &LocalView, span: Span, name: &str) -> Option<(Url, Range)> {
    let (file, span) = written(inner, view, span)?;
    let source = inner.workspace.get_source(&file)?;
    if source.get(span.start as usize..span.end as usize) != Some(name)
        || !tokenize(source)
            .iter()
            .any(|token| token.kind == TokenKind::Ident && token.span == span)
    {
        return None;
    }
    let range = span_range(&LineIndex::new(source), source, span);
    Some((naming_edit_url(inner, &view.root, &file)?, range))
}

fn owns_execution(view: &LocalView, binding: Binding, order: usize) -> bool {
    if binding
        .declaration
        .is_some_and(|id| view.facts.declarations[id].parse_order == order + 1)
    {
        return true;
    }
    if binding.definition.is_some_and(|id| {
        let definition = &view.facts.definitions[id];
        let equation = &view.model.written_equations[definition.equation_index].equation;
        equation.active_tokens.contains(&order)
            && view.model.expanded_tokens[order].span == definition.target_span
    }) {
        return true;
    }
    view.facts.uses.iter().any(|usage| {
        usage.parse_order == order
            && usage.name == binding.name
            && usage.dimension == binding.dimension
            && usage.definition == binding.definition
            && usage.declaration == binding.declaration
    })
}

pub(super) fn rename(
    inner: &mut Inner,
    pos: &TextDocumentPositionParams,
    new_name: Option<&str>,
) -> Option<Option<WorkspaceEdit>> {
    let text = inner.document(&pos.text_document.uri)?.text.clone();
    let byte = LineIndex::new(&text).offset_utf16(&text, span_pos(pos.position));
    let views = views(inner, &pos.text_document.uri);
    let selections: Vec<_> = views
        .iter()
        .map(|view| selected(inner, view, &pos.text_document.uri, byte))
        .collect();
    if selections.iter().all(Vec::is_empty) {
        return None;
    }
    if views.len() != 1 || selections[0].len() != 1 || !views[0].complete {
        return Some(None);
    }
    let view = &views[0];
    let binding = selections[0][0];
    let old_name = view.model.name(binding.name);
    let new_name = new_name.unwrap_or(old_name);
    if !crate::model_locals::is_local_name(new_name) {
        return Some(None);
    }
    if new_name != old_name
        && (view
            .model
            .symbol_type_events
            .iter()
            .any(|event| view.model.name(event.name) == new_name)
            || view.facts.definitions.iter().any(|definition| {
                view.model.name(definition.name) == new_name
                    && definition.dimension == binding.dimension
            }))
    {
        return Some(None);
    }
    if view
        .facts
        .declarations
        .iter()
        .filter(|decl| decl.name == binding.name)
        .count()
        > 1
        || binding.declaration.is_some()
            && view.facts.definitions.iter().any(|definition| {
                definition.name == binding.name && definition.dimension != binding.dimension
            })
    {
        return Some(None);
    }
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    for (span, _) in sites(view, binding, true) {
        let Some((uri, range)) = editable(inner, view, span, old_name) else {
            return Some(None);
        };
        // One literal macro site can supply several different bindings. Every
        // execution changed by this written edit must belong to this rename.
        let Some(edit_byte) = inner
            .workspace
            .get_source(uri.as_str())
            .map(|source| LineIndex::new(source).offset_utf16(source, span_pos(range.start)))
        else {
            return Some(None);
        };
        if selected(inner, view, &uri, edit_byte)
            .iter()
            .any(|other| *other != binding)
        {
            return Some(None);
        }
        for (order, token) in view.model.expanded_tokens.iter().enumerate() {
            if location(inner, view, token.span)
                .is_some_and(|target| target.uri == uri && target.range == range)
                && !owns_execution(view, binding, order)
            {
                return Some(None);
            }
        }
        let owners = inner.known_owner_roots(&uri);
        if owners.iter().any(|owner| owner != &view.root) {
            return Some(None);
        }
        let edit = TextEdit {
            range,
            new_text: new_name.to_string(),
        };
        let edits = changes.entry(uri).or_default();
        if !edits.contains(&edit) {
            edits.push(edit);
        }
    }
    if changes.is_empty()
        || inner.root_revision(&view.root).as_ref() != Some(&view.revision)
        || !inner
            .workspace
            .input_snapshot_is_current(view.root.as_str())
    {
        return Some(None);
    }
    Some(Some(versioned_workspace_edit(inner, changes)))
}

/// Map the cursor to every parser execution at that written site.
fn expression_orders(
    inner: &mut Inner,
    view: &LocalView,
    uri: &Url,
    byte: u32,
) -> Vec<(Option<Name>, usize)> {
    let Some(text) = inner.workspace.get_source(uri.as_str()).map(str::to_owned) else {
        return Vec::new();
    };
    // The written lexer proves that the cursor is outside comments and strings.
    let tokens = tokenize(&text);
    if tokens.iter().any(|token| {
        matches!(
            token.kind,
            TokenKind::String | TokenKind::Latex | TokenKind::MacroDir | TokenKind::MacroInterp
        ) && token.span.start <= byte
            && byte < token.span.end
    }) {
        return Vec::new();
    }
    let previous = tokens
        .iter()
        .rev()
        .find(|token| token.kind != TokenKind::Eof && token.span.start < byte);
    if let Some(token) = previous {
        let tail = text
            .get(token.span.end as usize..byte as usize)
            .unwrap_or("");
        if !tail.chars().all(char::is_whitespace) {
            return Vec::new();
        }
    } else {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    for (order, token) in view.model.expanded_tokens.iter().enumerate() {
        if token.kind == TokenKind::Eof {
            continue;
        }
        let Some((file, span)) = written(inner, view, token.span) else {
            continue;
        };
        if file != crate::include_resolver::normalize_uri(uri.as_str()) {
            continue;
        }
        let direct = span.start < byte && byte <= span.end;
        let adjacent = previous.is_some_and(|previous| previous.span == span && span.end <= byte);
        if !direct && !adjacent {
            continue;
        }
        let Some(dimension) = crate::model_locals::scope_at_order(&view.model, order) else {
            continue;
        };
        // A # target is not a value site. Scan this execution's current row.
        let start = view.model.expanded_tokens[..order]
            .iter()
            .rposition(|token| token.kind == TokenKind::Semi)
            .map_or(0, |index| index + 1);
        let row = &view.model.expanded_tokens[start..=order];
        if row
            .first()
            .is_some_and(|token| token.kind == TokenKind::Hash)
            && !row.iter().any(|token| token.kind == TokenKind::Eq)
        {
            continue;
        }
        if matches!(token.kind, TokenKind::Semi) && !direct {
            continue;
        }
        candidates.push((dimension, order));
    }
    candidates
}

pub(super) fn completions(
    inner: &mut Inner,
    uri: &Url,
    byte: u32,
    preferences: &PresentationSettings,
) -> Vec<CompletionItem> {
    let views = views(inner, uri);
    let mut groups = Vec::new();
    for view in &views {
        let orders = expression_orders(inner, view, uri, byte);
        if orders.is_empty() {
            return Vec::new();
        }
        for (dimension, order) in orders {
            let mut items = Vec::new();
            for LocalBinding {
                name,
                declaration,
                definition,
            } in view.facts.available(dimension, order)
            {
                let binding = view.binding(definition, declaration, name, dimension);
                let name = view.model.name(name).to_string();
                let item = CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::VARIABLE),
                    detail: Some("model-local variable".into()),
                    filter_text: Some(name.clone()),
                    insert_text: Some(name),
                    documentation: Some(Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: description(inner, view, binding, preferences),
                    })),
                    ..CompletionItem::default()
                };
                if !items.contains(&item) {
                    items.push(item);
                }
            }
            groups.push(items);
        }
    }
    let Some(mut items) = groups.pop() else {
        return Vec::new();
    };
    items.retain(|item| groups.iter().all(|group| group.contains(item)));
    items
}

pub(super) fn info_origins(
    view: &WrittenView<'_>,
    model: &Model,
    map: &crate::model_map::WrittenModelMap,
    result: &mut Value,
) {
    let facts = ModelLocals::collect(model);
    let Some(locals) = result.get_mut("model_locals") else {
        return;
    };
    for (field, sources) in [
        (
            "declarations",
            facts
                .declarations
                .iter()
                .map(|row| map.declarations.get(row.declaration_index))
                .collect::<Vec<_>>(),
        ),
        (
            "definitions",
            facts
                .definitions
                .iter()
                .map(|row| map.equations.get(row.equation_index).map(|row| &row.source))
                .collect::<Vec<_>>(),
        ),
    ] {
        let Some(rows) = locals[field].as_array_mut() else {
            continue;
        };
        for (row, source) in rows.iter_mut().zip(sources) {
            let Some(source) = source else {
                continue;
            };
            let source = view.source_json(source);
            if !source["location"].is_null() {
                row["origin"] = source["location"].clone();
            }
            row["origin_frames"] = source["origin_frames"].clone();
        }
    }
}
