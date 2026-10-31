//! Scoped model-local editor requests from the shared parser facts.

use super::*;
use crate::engine::analysis::model_locals::{LocalBinding, ModelLocals};
use crate::engine::language::intern::Name;

struct LocalView {
    root: Url,
    analysis: LocalAnalysis,
}

enum LocalAnalysis {
    Effective(Arc<crate::engine::analysis::symbols::SemanticModel>),
    Standalone {
        model: Box<Model>,
        facts: ModelLocals,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Binding {
    name: Name,
    dimension: Option<Name>,
    declaration: Option<usize>,
    definition: Option<usize>,
}

impl LocalView {
    fn model(&self) -> &Model {
        match &self.analysis {
            LocalAnalysis::Effective(semantic) => &semantic.model,
            LocalAnalysis::Standalone { model, .. } => model,
        }
    }

    fn facts(&self) -> &ModelLocals {
        match &self.analysis {
            LocalAnalysis::Effective(semantic) => semantic.local_facts(),
            LocalAnalysis::Standalone { facts, .. } => facts,
        }
    }

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
    let mut roots = inner.context_roots(uri);
    if inner.context_ambiguous(uri) {
        return Vec::new();
    }
    let standalone = roots.is_empty();
    if standalone {
        roots.push(uri.clone());
    }
    roots
        .into_iter()
        .filter_map(|root| {
            if !standalone {
                let semantic = typed_names::root_view(inner, &root)?;
                return Some(LocalView {
                    root,
                    analysis: LocalAnalysis::Effective(semantic),
                });
            }
            inner.root_revision(&root)?;
            let model = Box::new(inner.workspace.get_model(root.as_str())?.clone());
            let facts = ModelLocals::collect_before(
                &model,
                crate::engine::diagnostics::parse_preflight::first_refusal(&model)
                    .map(|refusal| refusal.order),
            );
            Some(LocalView {
                root,
                analysis: LocalAnalysis::Standalone { model, facts },
            })
        })
        .collect()
}

fn written(view: &LocalView, span: Span) -> Option<(String, Span)> {
    match &view.analysis {
        LocalAnalysis::Standalone { .. } => Some((
            crate::engine::workspace::include_resolver::normalize_uri(view.root.as_str()),
            span,
        )),
        LocalAnalysis::Effective(semantic) => semantic.written(span),
    }
}

fn location(inner: &mut Inner, view: &LocalView, span: Span) -> Option<Location> {
    let (file, span) = written(view, span)?;
    let uri = naming_edit_url(inner, &view.root, &file)?;
    let source = inner.workspace.get_source(&file)?;
    source.get(span.start as usize..span.end as usize)?;
    Some(Location::new(
        uri,
        span_range(&local_source_index(source), source, span),
    ))
}

#[cfg(test)]
thread_local! {
    static LOCAL_SOURCE_INDEX_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn local_source_index(source: &str) -> LineIndex {
    #[cfg(test)]
    LOCAL_SOURCE_INDEX_BUILDS.with(|builds| builds.set(builds.get() + 1));
    LineIndex::new(source)
}

struct DescriptionSource<'a> {
    text: &'a str,
    index: LineIndex,
    uri: Option<Url>,
}

/// Source conversion shared by the local descriptions in one request.
struct DescriptionSources<'a> {
    inner: &'a Inner,
    root: &'a Url,
    sources: HashMap<String, Option<DescriptionSource<'a>>>,
}

impl<'a> DescriptionSources<'a> {
    fn new(inner: &'a Inner, root: &'a Url) -> Self {
        Self {
            inner,
            root,
            sources: HashMap::new(),
        }
    }

    fn source(&mut self, file: &str) -> Option<&DescriptionSource<'a>> {
        let inner = self.inner;
        let root = self.root;
        self.sources
            .entry(file.to_owned())
            .or_insert_with(|| {
                let text = inner.workspace.get_source(file)?;
                Some(DescriptionSource {
                    text,
                    index: local_source_index(text),
                    uri: naming_edit_url(inner, root, file),
                })
            })
            .as_ref()
    }

    fn location(&mut self, view: &LocalView, span: Span) -> Option<Location> {
        let (file, span) = written(view, span)?;
        let source = self.source(&file)?;
        source.text.get(span.start as usize..span.end as usize)?;
        Some(Location::new(
            source.uri.clone()?,
            span_range(&source.index, source.text, span),
        ))
    }
}

fn at(view: &LocalView, file: &str, byte: u32, span: Span) -> bool {
    written(view, span)
        .is_some_and(|(target, span)| target == file && span.start <= byte && byte <= span.end)
}

fn selected(view: &LocalView, uri: &Url, byte: u32) -> Vec<Binding> {
    let file = crate::engine::workspace::include_resolver::normalize_uri(uri.as_str());
    let mut result = Vec::new();
    for (id, declaration) in view.facts().declarations.iter().enumerate() {
        if !at(view, &file, byte, declaration.span) {
            continue;
        }
        let definitions: Vec<_> = view
            .facts()
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
    for (id, definition) in view.facts().definitions.iter().enumerate() {
        if at(view, &file, byte, definition.target_span) {
            result.push(view.binding(
                Some(id),
                definition.declaration,
                definition.name,
                definition.dimension,
            ));
        }
    }
    for usage in &view.facts().uses {
        if at(view, &file, byte, usage.span) {
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
    let definition = view.facts().definitions.get(binding.definition?)?;
    Some(
        view.model()
            .written_equations
            .get(definition.equation_index)?
            .equation
            .rhs
            .as_str(),
    )
}

fn description(
    sources: &mut DescriptionSources<'_>,
    view: &LocalView,
    binding: Binding,
    preferences: &PresentationSettings,
) -> String {
    let mut text = format!(
        "**Model-local variable** {}",
        crate::lsp::names::code(view.model().name(binding.name))
    );
    if preferences.name_details.tex
        && let Some(tex) = binding
            .declaration
            .and_then(|id| view.facts().declarations[id].tex_name.as_ref())
    {
        text.push_str(&format!("\n\nTeX: {}", crate::lsp::names::code(tex)));
    }
    if let Some(expression) = expression(view, binding) {
        let definition = &view.facts().definitions[binding.definition.unwrap()];
        let row = &view.model().written_equations[definition.equation_index];
        let expanded = row.equation.active_tokens.clone().any(|index| {
            let token = &view.model().expanded_tokens[index];
            token.lexeme.as_ref().is_some_and(|lexeme| {
                written(view, token.span).and_then(|(file, span)| {
                    sources.source(&file).map(|source| {
                        source.text.get(span.start as usize..span.end as usize)
                            == Some(lexeme.as_str())
                    })
                }) != Some(true)
            })
        });
        text.push_str(if expanded {
            "\n\nExpanded expression: "
        } else {
            "\n\nExpression: "
        });
        text.push_str(&crate::lsp::names::code(expression));
        if let Some(target) = sources.location(view, definition.target_span) {
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
        let bindings = selected(&view, &pos.text_document.uri, byte);
        found |= !bindings.is_empty();
        if bindings.len() != 1 {
            descriptions.push(None);
            continue;
        }
        let mut sources = DescriptionSources::new(inner, &view.root);
        descriptions.push(Some(description(
            &mut sources,
            &view,
            bindings[0],
            &preferences,
        )));
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
        for binding in selected(&view, &pos.text_document.uri, byte) {
            found = true;
            let span = if declaration {
                binding
                    .declaration
                    .map(|id| view.facts().declarations[id].span)
                    .or_else(|| {
                        binding
                            .definition
                            .map(|id| view.facts().definitions[id].target_span)
                    })
            } else {
                binding
                    .definition
                    .map(|id| view.facts().definitions[id].target_span)
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
            sites.push((view.facts().declarations[id].span, true));
        }
        if let Some(id) = binding.definition {
            sites.push((view.facts().definitions[id].target_span, true));
        }
    }
    sites.extend(
        view.facts()
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
        let bindings = selected(view, &pos.text_document.uri, byte);
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
        let bindings = selected(view, &pos.text_document.uri, byte);
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
    let file = crate::engine::workspace::include_resolver::normalize_uri(uri.as_str());
    let mut candidates = Vec::new();
    for (order, token) in view.model().expanded_tokens.iter().enumerate() {
        if token.kind == TokenKind::Eof {
            continue;
        }
        let Some((target, span)) = written(view, token.span) else {
            continue;
        };
        if target != file {
            continue;
        }
        let direct = span.start < byte && byte <= span.end;
        let adjacent = previous.is_some_and(|previous| previous.span == span && span.end <= byte);
        if !direct && !adjacent {
            continue;
        }
        let Some(dimension) =
            crate::engine::analysis::model_locals::scope_at_order(view.model(), order)
        else {
            continue;
        };
        // A # target is not a value site. Scan this execution's current row.
        let start = view.model().expanded_tokens[..order]
            .iter()
            .rposition(|token| token.kind == TokenKind::Semi)
            .map_or(0, |index| index + 1);
        let row = &view.model().expanded_tokens[start..=order];
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
        let mut sources = DescriptionSources::new(inner, &view.root);
        for (dimension, order) in orders {
            let mut items = Vec::new();
            for LocalBinding {
                name,
                declaration,
                definition,
            } in view.facts().available(dimension, order)
            {
                let binding = view.binding(definition, declaration, name, dimension);
                let name = view.model().name(name).to_string();
                let item = CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::VARIABLE),
                    detail: Some("model-local variable".into()),
                    filter_text: Some(name.clone()),
                    insert_text: Some(name),
                    documentation: Some(Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: description(&mut sources, view, binding, preferences),
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
    facts: &ModelLocals,
    map: &crate::engine::workspace::model_map::WrittenModelMap,
    result: &mut Value,
) {
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

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    #[test]
    fn model_information_shares_accepted_locals_and_keeps_origins_current() {
        // Guards a second accepted-local analysis while adding source origins,
        // and keeps its rows aligned after an included source changes.
        use crate::engine::analysis::model_info::MODEL_INFO_LOCAL_FACT_BUILDS;
        let root = Url::parse("file:///model-info-reuse/main.mod").unwrap();
        let helper = Url::parse("file:///model-info-reuse/locals.inc").unwrap();
        let text = "var y;\r\npredetermined_variables y;\r\nparameters p;\r\nmodel_local_variable first $F$;\r\nmodel;\r\n@#include \"locals.inc\"\r\ny=second;\r\nend;\r\n";
        let local_text = "/* 😀 */ #first=p+y(-1);\r\n#second=first+1;\r\n";
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(root.clone(), text.into(), 1);
        for (version, prefix) in [(1, ""), (2, "// moved\r\n")] {
            let source = format!("{prefix}{local_text}");
            server.upsert(helper.clone(), source.clone(), version);
            MODEL_INFO_LOCAL_FACT_BUILDS.with(|builds| builds.set(0));
            let result = server.model_info_command(&[json!({"root_uri":root})]);
            assert_eq!(result["complete"], true);
            assert_eq!(MODEL_INFO_LOCAL_FACT_BUILDS.with(|builds| builds.get()), 1);
            let locals = &result["model_locals"];
            assert_eq!(locals["declarations"][0]["name"], "first");
            assert_eq!(locals["declarations"][0]["origin"]["uri"], root.as_str());
            let definitions = locals["definitions"].as_array().unwrap();
            assert_eq!(definitions.len(), 2);
            for (row, name) in definitions.iter().zip(["first", "second"]) {
                assert_eq!(row["name"], name);
                assert_eq!(row["origin"]["uri"], helper.as_str());
                let start = source.find(&format!("#{name}=")).unwrap() as u32;
                let expected = LineIndex::new(&source).position_utf16(&source, start);
                assert_eq!(
                    row["origin"]["range"]["start"],
                    json!({"line":expected.line,"character":expected.character})
                );
            }
            let y = definitions[0]["idents"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["name"] == "y")
                .unwrap();
            assert_eq!(y["class"], "endogenous");
            assert_eq!(y["dynare_timing"], -2);
            assert_eq!(definitions[1]["idents"][0]["class"], "model_local");
        }
        server.upsert(
            root.clone(),
            "var y;model;#a=1;#a=2;#late=3;y=a;end;".into(),
            2,
        );
        MODEL_INFO_LOCAL_FACT_BUILDS.with(|builds| builds.set(0));
        let result = server.model_info_command(&[json!({"root_uri":root})]);
        assert_eq!(result["complete"], false);
        assert!(result.get("model_locals").is_none());
        assert_eq!(MODEL_INFO_LOCAL_FACT_BUILDS.with(|builds| builds.get()), 0);
        server.upsert(
            root.clone(),
            "var y;\n@#include \"missing.inc\"\nmodel;y=0;end;".into(),
            3,
        );
        MODEL_INFO_LOCAL_FACT_BUILDS.with(|builds| builds.set(0));
        let result = server.model_info_command(&[json!({"root_uri":root})]);
        assert_eq!(result["complete"], false);
        assert!(result.get("model_locals").is_none());
        assert_eq!(MODEL_INFO_LOCAL_FACT_BUILDS.with(|builds| builds.get()), 0);
    }

    #[test]
    fn many_local_completions_share_source_conversion_and_refresh_documentation() {
        // Guards rebuilding complete source indexes for each local completion,
        // incorrect include/UTF-16 links, and stale source coordinates after edits.
        let root = Url::parse("file:///completion-sources/main.mod").unwrap();
        let helper = Url::parse("file:///completion-sources/helper.inc").unwrap();
        let mut text = String::from("// 😀\r\nvar ");
        for id in 1..=240 {
            text.push_str(&format!("y{id} "));
        }
        text.push_str(";\r\nmodel;\r\n@#include \"helper.inc\"\r\n");
        for id in 1..=240 {
            text.push_str(&format!(
                "/* 😀 */ #local{id}=shared+y{id}(-1);\r\ny{id}=local{id};\r\n"
            ));
        }
        text.push_str("end;\r\n");
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(helper.clone(), "#shared=0.5;\r\n".into(), 1);
        server.upsert(root.clone(), text.clone(), 1);
        for (version, prefix) in [(1, ""), (2, "// second version\r\n")] {
            let source = format!("{prefix}{text}");
            if version > 1 {
                server.upsert(root.clone(), source.clone(), version);
            }
            let byte = source.rfind("local240").unwrap() as u32 + "local240".len() as u32;
            let point = LineIndex::new(&source).position_utf16(&source, byte);
            let pos = TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: root.clone() },
                position: Position::new(point.line, point.character),
            };
            LOCAL_SOURCE_INDEX_BUILDS.with(|builds| builds.set(0));
            SYNTAX_CONTEXT_READS.with(|reads| reads.set(0));
            let Some(CompletionResponse::Array(items)) = server.complete(&pos, None) else {
                panic!("completion items required")
            };
            let locals: Vec<_> = items
                .iter()
                .filter(|item| item.detail.as_deref() == Some("model-local variable"))
                .collect();
            assert_eq!(locals.len(), 241);
            for item in &locals {
                let Some(Documentation::MarkupContent(documentation)) = &item.documentation else {
                    panic!("local documentation required")
                };
                let (uri, line, column, expression) = if item.label == "shared" {
                    (&helper, 1, 2, "0.5".to_owned())
                } else {
                    let id: u32 = item.label.strip_prefix("local").unwrap().parse().unwrap();
                    (
                        &root,
                        5 + 2 * (id - 1) + u32::from(version > 1),
                        11,
                        format!("shared+y{id}(-1)"),
                    )
                };
                assert!(
                    documentation
                        .value
                        .contains(&format!("Expression: `{expression}`")),
                    "{}: {}",
                    item.label,
                    documentation.value
                );
                assert!(
                    documentation
                        .value
                        .contains(&format!("[Definition]({uri}#L{line},{column})")),
                    "{}: {}",
                    item.label,
                    documentation.value
                );
            }
            assert_eq!(
                LOCAL_SOURCE_INDEX_BUILDS.with(std::cell::Cell::get),
                2,
                "one source index per written completion source is sufficient"
            );
            assert_eq!(
                SYNTAX_CONTEXT_READS.with(std::cell::Cell::get),
                1,
                "one resolved source context must serve the whole completion request"
            );
        }
    }

    #[test]
    fn standalone_helper_keeps_its_written_model_with_a_cached_effective_view() {
        // Guards using an expanded snapshot for a helper with no model owner.
        let uri = Url::parse("file:///standalone-local-requests.inc").unwrap();
        let text = "var y; model; #z=1; y=z; end;";
        let mut inner = Inner::default();
        inner.workspace.update_document(uri.as_str(), text);
        let effective = typed_names::root_view(&mut inner, &uri).unwrap();
        let views = views(&mut inner, &uri);
        let [view] = views.as_slice() else {
            panic!("one standalone view required")
        };
        assert!(matches!(view.analysis, LocalAnalysis::Standalone { .. }));
        assert!(!std::ptr::eq(view.model(), &*effective.model));
        let target = text.find("y=z").unwrap() as u32 + 2;
        let bindings = selected(view, &uri, target);
        assert_eq!(bindings.len(), 1);
        assert_eq!(expression(view, bindings[0]), Some("1"));
    }

    fn assert_shared(
        inner: &mut Inner,
        uri: &Url,
    ) -> Arc<crate::engine::analysis::symbols::SemanticModel> {
        let semantic = typed_names::root_view(inner, uri).unwrap();
        let views = views(inner, uri);
        assert_eq!(views.len(), 1);
        assert!(
            std::ptr::eq(views[0].model(), &*semantic.model),
            "local requests cloned the shared semantic model"
        );
        assert!(
            std::ptr::eq(views[0].facts(), semantic.local_facts()),
            "local requests recollected the shared local facts"
        );
        semantic
    }

    #[test]
    fn local_requests_share_semantics_and_refresh_after_edit() {
        // Guards a full model clone and repeated local-fact collection on each
        // hover, completion and navigation request, plus stale local expressions.
        let (service, _socket) = new_service();
        let server = service.inner();
        let uri = Url::parse("file:///shared-local-requests.mod").unwrap();
        let text = "var y; model; #z=1; y=z; end;";
        server.upsert(uri.clone(), text.into(), 1);
        let pos = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position::new(0, text.find("y=z").unwrap() as u32 + 2),
        };
        let original = assert_shared(&mut server.lock_inner(), &uri);
        let hover = server.hover_at(&pos).unwrap();
        assert!(
            matches!(hover.contents, HoverContents::Markup(content) if content.value.contains("Expression: `1`"))
        );
        let Some(GotoDefinitionResponse::Scalar(definition)) = server.definition_at(&pos) else {
            panic!("one local definition required")
        };
        assert_eq!(definition.uri, uri);
        assert_eq!(
            definition.range.start.character,
            text.find("#z").unwrap() as u32 + 1
        );
        let Some(CompletionResponse::Array(items)) = server.complete(&pos, None) else {
            panic!("local completions required")
        };
        assert!(items.iter().any(
            |item| item.label == "z" && item.detail.as_deref() == Some("model-local variable")
        ));
        assert!(Arc::ptr_eq(
            &original,
            &assert_shared(&mut server.lock_inner(), &uri)
        ));
        server.upsert(uri.clone(), text.replace("#z=1", "#z=2"), 2);
        let hover = server.hover_at(&pos).unwrap();
        assert!(
            matches!(hover.contents, HoverContents::Markup(content) if content.value.contains("Expression: `2`"))
        );
        assert!(!Arc::ptr_eq(
            &original,
            &assert_shared(&mut server.lock_inner(), &uri)
        ));
    }
}
