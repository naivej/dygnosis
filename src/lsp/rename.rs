//! LSP source acquisition and versioned presentation of shared Rename decisions.

use super::*;
use crate::engine::analysis::symbols::{self, Selector, SemanticModel};

fn source_index(text: &str) -> LineIndex {
    LineIndex::new(&crate::engine::language::parser::normalize_newlines(text))
}

#[cfg(test)]
thread_local! {
    static CAPTURE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static REFERENCE_SOURCE_INDEXES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn reference_source_index(text: &str) -> LineIndex {
    #[cfg(test)]
    REFERENCE_SOURCE_INDEXES.with(|indexes| indexes.set(indexes.get() + 1));
    source_index(text)
}

pub(super) const SOURCE: &str = symbols::SOURCE.message;
pub(super) const MAPPING: &str = symbols::MAPPING.message;
pub(super) const STALE: &str =
    "Model inputs changed during Rename. Refresh the model and try again.";
pub(super) const IDENTIFIER: &str = symbols::IDENTIFIER.message;
pub(super) const WRITABLE: &str = "A Rename target is unavailable or read-only. Make every affected source writable and try again.";
pub(super) const PREVIEW: &str = "ANALYSIS_PREVIEW_EDIT_UNSUPPORTED: Rename is unavailable while a definition or branch analysis preview is selected. No edits were proposed. Reset Analysis Configuration before requesting Rename.";

pub(super) fn refusal_json(reason: &'static str) -> Value {
    json!({"error":reason,"code":if reason==PREVIEW{"ANALYSIS_PREVIEW_EDIT_UNSUPPORTED"}else{"RENAME_REFUSED"}})
}

pub(super) struct Decision {
    pub range: Range,
    pub edit: WorkspaceEdit,
    roots: Vec<Url>,
}

pub(super) fn capture(
    inner: &mut Inner,
    root: &Url,
) -> std::result::Result<SemanticModel, &'static str> {
    #[cfg(test)]
    CAPTURE_COUNT.with(|captures| captures.set(captures.get() + 1));
    let revision = inner.root_revision(root).ok_or(SOURCE)?;
    SemanticModel::capture(&mut inner.workspace, root.as_str(), revision)
        .map_err(|reason| reason.message)
}

pub(super) fn decide(
    inner: &mut Inner,
    pos: &TextDocumentPositionParams,
    new_name: Option<&str>,
) -> std::result::Result<Decision, &'static str> {
    let uri = &pos.text_document.uri;
    let doc = inner.document(uri).ok_or(SOURCE)?;
    let text = doc.text.clone();
    let version = doc.version;
    let byte = source_index(&text).offset_utf16(&text, span_pos(pos.position));
    if inner.context_ambiguous(uri) {
        return Err("Choose a model owner before requesting Rename for this file.");
    }
    let mut roots = inner.context_roots(uri);
    if roots.is_empty() && (is_model_root(uri) || uri.scheme() == "untitled") {
        roots.push(uri.clone());
    }
    if roots.is_empty() || !matches!(uri.scheme(), "file" | "untitled") {
        return Err("Rename requires a file or untitled source with a known model owner.");
    }
    // The displayed context selects the binding and its collision scope. Every
    // known owner still has to accept edits to these same written bytes.
    for owner in inner.known_owner_roots(uri) {
        if !roots.contains(&owner) {
            roots.push(owner);
        }
    }
    let file = crate::engine::workspace::include_resolver::normalize_uri(uri.as_str());
    let mut views = Vec::new();
    for root in &roots {
        if inner
            .workspace
            .root_analysis_configuration(root.as_str())
            .is_preview()
        {
            return Err(PREVIEW);
        }
        views.push(capture(inner, root)?);
    }
    let binding = views[0]
        .select(Selector::Target { file: &file, byte })
        .map_err(|reason| reason.reason.message)?
        .ok_or("Place the cursor on a declared model name before requesting Rename.")?;
    let selected_span = views[0]
        .references(&binding, true)
        .into_iter()
        .find_map(|site| {
            site.written
                .filter(|(target, span)| target == &file && span.start <= byte && byte < span.end)
                .map(|(_, span)| span)
        })
        .ok_or(MAPPING)?;
    let replacement = new_name.unwrap_or(&binding.name);
    if new_name.is_some() && replacement == binding.name {
        return Err("The requested name is already the current name.");
    }
    // The first owner identifies the binding. Each known owner at the selected
    // source must accept it before the shared operation can include that owner.
    for view in views.iter().skip(1) {
        let other = view
            .select(Selector::Target { file: &file, byte })
            .map_err(|reason| reason.reason.message)?
            .ok_or(symbols::BINDING.message)?;
        if binding.is_local() != other.is_local()
            || other.name != binding.name
            || other.kind != binding.kind
            || other.declaration != binding.declaration
            || other.definition != binding.definition
            || other.dimension != binding.dimension
        {
            return Err(symbols::BINDING.message);
        }
    }
    let proposal = loop {
        let proposal = symbols::rename(&views, 0, &binding, replacement).map_err(|reason| {
            if reason.code == "ANALYSIS_PREVIEW_EDIT_UNSUPPORTED" {
                PREVIEW
            } else {
                reason.message
            }
        })?;
        let mut additional = Vec::new();
        for edit in &proposal.edits {
            let target_uri = naming_edit_url(inner, &roots[0], &edit.file).ok_or(MAPPING)?;
            for owner in inner.known_owner_roots(&target_uri) {
                if !roots.contains(&owner) && !additional.contains(&owner) {
                    additional.push(owner);
                }
            }
        }
        if additional.is_empty() {
            break proposal;
        }
        for owner in additional {
            views.push(capture(inner, &owner)?);
            roots.push(owner);
        }
    };
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    let mut sources = HashMap::new();
    for edit in &proposal.edits {
        if !sources.contains_key(&edit.file) {
            let target_uri = naming_edit_url(inner, &roots[0], &edit.file).ok_or(MAPPING)?;
            if !matches!(target_uri.scheme(), "file" | "untitled") {
                return Err(WRITABLE);
            }
            if target_uri.scheme() == "file" {
                match target_uri.to_file_path() {
                    Ok(path) => match std::fs::metadata(&path) {
                        Ok(metadata)
                            if !metadata.is_file() || metadata.permissions().readonly() =>
                        {
                            return Err(WRITABLE);
                        }
                        Err(_) if inner.document(&target_uri).is_none() => return Err(SOURCE),
                        _ => {}
                    },
                    Err(_) if inner.document(&target_uri).is_none() => return Err(MAPPING),
                    _ => {}
                }
            }
            let source = inner
                .workspace
                .source_for_normalized_key(&edit.file)
                .ok_or(MAPPING)?
                .to_owned();
            sources.insert(
                edit.file.clone(),
                (target_uri, source_index(&source), source),
            );
        }
        let (target_uri, index, source) = sources.get(&edit.file).ok_or(MAPPING)?;
        if source.get(edit.span.start as usize..edit.span.end as usize)
            != Some(edit.expected.as_str())
        {
            return Err(STALE);
        }
        changes
            .entry(target_uri.clone())
            .or_default()
            .push(TextEdit {
                range: span_range(index, source, edit.span),
                new_text: edit.replacement.clone(),
            });
    }
    if changes.is_empty()
        || inner.document(uri).ok_or(MAPPING)?.version != version
        || inner.document(uri).ok_or(MAPPING)?.text != text
        || inner.workspace.get_source(uri.as_str()).ok_or(MAPPING)? != text
        || sources.iter().any(|(file, (target, _, _))| {
            crate::engine::workspace::include_resolver::normalize_uri(target.as_str()) != *file
        })
    {
        return Err(STALE);
    }
    for (root, view) in roots.iter().zip(&views) {
        if inner.root_revision(root).as_ref() != Some(&view.revision)
            || !inner.workspace.input_snapshot_is_current(root.as_str())
        {
            return Err(STALE);
        }
    }
    Ok(Decision {
        range: span_range(&source_index(&text), &text, selected_span),
        edit: versioned_workspace_edit(inner, changes),
        roots,
    })
}

/// Ordinary references use accepted sites; callers convert the shared byte spans.
/// Ordinary names follow the displayed model; older multi-owner clients only
/// get a target when every known owner agrees on its written declaration.
pub(super) fn definition(inner: &mut Inner, pos: &TextDocumentPositionParams) -> Option<Location> {
    let uri = &pos.text_document.uri;
    let text = inner.document(uri)?.text.clone();
    let byte = source_index(&text).offset_utf16(&text, span_pos(pos.position));
    let file = crate::engine::workspace::include_resolver::normalize_uri(uri.as_str());
    let roots = inner.context_roots(uri);
    let mut agreed = None;
    for root in roots {
        let view = typed_names::root_view(inner, &root)?;
        let binding = view.select(Selector::Target { file: &file, byte }).ok()??;
        let (file, span) = binding.declaration?;
        let target = naming_edit_url(inner, &root, &file)?;
        let source = view.sources.get(&file)?;
        source.get(span.start as usize..span.end as usize)?;
        let location = Location::new(target, span_range(&source_index(source), source, span));
        if agreed.as_ref().is_some_and(|prior| prior != &location) {
            return None;
        }
        agreed = Some(location);
    }
    agreed
}

pub(super) fn references(
    inner: &mut Inner,
    pos: &TextDocumentPositionParams,
    include_declaration: bool,
) -> Option<Vec<Location>> {
    let text = inner.document(&pos.text_document.uri)?.text.clone();
    let byte = source_index(&text).offset_utf16(&text, span_pos(pos.position));
    let file =
        crate::engine::workspace::include_resolver::normalize_uri(pos.text_document.uri.as_str());
    let mut roots = inner.context_roots(&pos.text_document.uri);
    if inner.context_ambiguous(&pos.text_document.uri) {
        return None;
    }
    if roots.is_empty() {
        roots.push(pos.text_document.uri.clone());
    }
    let mut locations = Vec::new();
    for root in roots {
        let view = typed_names::root_view(inner, &root)?;
        let binding = view.select(Selector::Target { file: &file, byte }).ok()??;
        let mut sources = HashMap::new();
        for site in view.references(&binding, include_declaration) {
            let Some((file, span)) = site.written else {
                continue;
            };
            let Some((file, source)) = view.sources.get_key_value(&file) else {
                continue;
            };
            if source.get(span.start as usize..span.end as usize).is_none() {
                continue;
            }
            let Some((uri, index)) = sources
                .entry(file.as_str())
                .or_insert_with(|| {
                    Some((
                        naming_edit_url(inner, &root, file)?,
                        reference_source_index(source),
                    ))
                })
                .as_ref()
            else {
                continue;
            };
            let location = Location::new(uri.clone(), span_range(index, source, span));
            if !locations.contains(&location) {
                locations.push(location);
            }
        }
    }
    (!locations.is_empty()).then_some(locations)
}

/// Native Rename loads saved inputs before applying through workspace.applyEdit.
/// A second capture after those loads proves the complete decision again.
pub(super) fn capture_command(
    inner: &mut Inner,
    arguments: &[Value],
) -> std::result::Result<Value, &'static str> {
    let params: RenameParams = serde_json::from_value(
        arguments
            .first()
            .ok_or("captureRename requires Rename arguments")?
            .clone(),
    )
    .map_err(|_| "captureRename requires valid Rename arguments")?;
    if !is_legal_ident(&params.new_name) {
        return Err(IDENTIFIER);
    }
    let pos = &params.text_document_position;
    let decision = decide(inner, pos, Some(&params.new_name))?;
    let (edit, roots) = (decision.edit, decision.roots);
    let DocumentChanges::Edits(documents) = edit.document_changes.as_ref().ok_or(SOURCE)? else {
        return Err(MAPPING);
    };
    let mut source_uris: HashSet<Url> = documents
        .iter()
        .map(|document| document.text_document.uri.clone())
        .collect();
    for root in &roots {
        source_uris.insert(root.clone());
        let identity = crate::engine::workspace::include_resolver::normalize_uri(root.as_str());
        for file in inner.workspace.input_stamps(root.as_str()).keys() {
            if inner.workspace.source_for_normalized_key(file).is_some()
                && inner.workspace.owner_roots(file).contains(&identity)
            {
                source_uris.insert(naming_edit_url(inner, root, file).ok_or(SOURCE)?);
            }
        }
    }
    let mut source_uris: Vec<_> = source_uris.into_iter().collect();
    source_uris.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    let mut sources = Vec::new();
    let mut bytes = 0usize;
    for uri in source_uris {
        let text = inner.workspace.get_source(uri.as_str()).ok_or(SOURCE)?;
        bytes = bytes.checked_add(text.len()).ok_or(SOURCE)?;
        if bytes > crate::engine::comparison::compare_snapshots::MAX_SNAPSHOT_SOURCE_BYTES {
            return Err("Rename capture exceeds the 16 MiB source limit.");
        }
        sources.push(json!({ "uri": uri, "text": text, "version": inner.document(&uri).map(|document| document.version) }));
    }
    let mut owners = Vec::new();
    for root in roots {
        owners.push(
            json!({ "root_uri": root, "revision": inner.root_revision(&root).ok_or(SOURCE)? }),
        );
        if !inner.workspace.input_snapshot_is_current(root.as_str()) {
            return Err(STALE);
        }
    }
    owners.sort_by(|left, right| left["root_uri"].as_str().cmp(&right["root_uri"].as_str()));
    Ok(json!({ "schema_version": 1, "edit": edit, "sources": sources, "owners": owners }))
}

#[cfg(test)]
mod read_tests {
    use super::*;

    fn position(source: &str, byte: usize) -> Position {
        let prefix = &source[..byte];
        Position::new(
            prefix.bytes().filter(|byte| *byte == b'\n').count() as u32,
            prefix.rsplit('\n').next().unwrap().encode_utf16().count() as u32,
        )
    }

    fn expected(uri: &Url, source: &str) -> Vec<Location> {
        source
            .match_indices("rho")
            .map(|(byte, _)| {
                Location::new(
                    uri.clone(),
                    Range::new(position(source, byte), position(source, byte + 3)),
                )
            })
            .collect()
    }

    async fn check_references(
        server: &Backend,
        root: &Url,
        source: &str,
        helper: &Url,
        helper_source: &str,
    ) {
        let pos = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: root.clone() },
            position: position(source, source.find("rho").unwrap() + 1),
        };
        REFERENCE_SOURCE_INDEXES.with(|indexes| indexes.set(0));
        CAPTURE_COUNT.with(|captures| captures.set(0));
        let result = server
            .references(ReferenceParams {
                text_document_position: pos.clone(),
                context: ReferenceContext {
                    include_declaration: true,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();
        let mut targets = expected(root, source);
        targets.extend(expected(helper, helper_source));
        assert_eq!(result.len(), targets.len());
        assert!(
            targets.iter().all(|target| result.contains(target)),
            "references must preserve every exact written UTF-16 range"
        );
        assert_eq!(
            REFERENCE_SOURCE_INDEXES.with(std::cell::Cell::get),
            2,
            "references must build one source index per captured written file"
        );
        assert_eq!(
            CAPTURE_COUNT.with(std::cell::Cell::get),
            0,
            "read-only references must reuse the current semantic snapshot"
        );
        let before = typed_names::root_view(&mut server.lock_inner(), root).unwrap();
        let Some(GotoDefinitionResponse::Scalar(definition)) = server.definition_at(&pos) else {
            panic!("one ordinary declaration required")
        };
        let root_targets = expected(root, source);
        assert_eq!(definition, root_targets[0]);
        assert!(!server.doc_symbols(root).unwrap().is_empty());
        let highlights = server.ident_highlights(&pos).unwrap();
        assert_eq!(highlights.len(), root_targets.len());
        for highlight in &highlights {
            assert!(
                root_targets
                    .iter()
                    .any(|target| target.range == highlight.range)
            );
            assert_eq!(
                highlight.kind,
                Some(if highlight.range == definition.range {
                    DocumentHighlightKind::WRITE
                } else {
                    DocumentHighlightKind::READ
                })
            );
        }
        assert_eq!(
            CAPTURE_COUNT.with(std::cell::Cell::get),
            0,
            "read-only navigation and Outline must share the same snapshot"
        );
        let after = typed_names::root_view(&mut server.lock_inner(), root).unwrap();
        assert!(Arc::ptr_eq(&before, &after));
    }

    #[tokio::test]
    async fn ordinary_reads_share_snapshots_and_project_each_current_source_once() {
        // Guards repeated source indexing and semantic capture for high-fanout
        // reads, stale included/settings facts, and shifted editor coordinates.
        let directory = std::env::temp_dir().join(format!(
            "dygnosis-read-sources-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let helper_path = directory.join("shared.inc");
        let helper = Url::from_file_path(&helper_path).unwrap();
        let root = Url::from_file_path(directory.join("main.mod")).unwrap();
        let mut helper_source = String::from("/* 😀 */ rho=0.9;\r\n");
        std::fs::write(&helper_path, &helper_source).unwrap();
        let mut source = String::from("/* 😀 */ parameters rho;\r\nvar ");
        for index in 1..=240 {
            source.push_str(&format!("y{index} "));
        }
        source.push_str(";\r\n@#include \"shared.inc\"\r\nmodel;\r\n");
        for index in 1..=240 {
            source.push_str(&format!(
                "#local{index}=rho*y{index}(-1);\r\ny{index}=local{index};\r\n"
            ));
        }
        source.push_str("end;\r\n");
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(root.clone(), source.clone(), 1);
        check_references(server, &root, &source, &helper, &helper_source).await;
        source.insert_str(0, "// shifted source\r\n");
        server.upsert(root.clone(), source.clone(), 2);
        check_references(server, &root, &source, &helper, &helper_source).await;
        helper_source.push_str("rho=rho+0.01;\r\n");
        std::fs::write(&helper_path, &helper_source).unwrap();
        check_references(server, &root, &source, &helper, &helper_source).await;
        server
            .lock_inner()
            .settings
            .apply(&json!({"macroWorkLimit": 1}));
        let pos = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: root.clone() },
            position: position(&source, source.find("rho").unwrap() + 1),
        };
        assert!(
            server
                .references(ReferenceParams {
                    text_document_position: pos,
                    context: ReferenceContext {
                        include_declaration: true
                    },
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default()
                })
                .await
                .unwrap()
                .is_none(),
            "a work-limit change must not retain the previous accepted references"
        );
        server
            .lock_inner()
            .settings
            .apply(&json!({"macroWorkLimit": 2_000_000}));
        check_references(server, &root, &source, &helper, &helper_source).await;
        let pos = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: root.clone() },
            position: position(&source, source.find("rho").unwrap() + 1),
        };
        CAPTURE_COUNT.with(|captures| captures.set(0));
        assert!(server.prepare_rename(pos).await.unwrap().is_some());
        assert!(
            CAPTURE_COUNT.with(std::cell::Cell::get) > 0,
            "Rename must retain its fresh current-source proof"
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
