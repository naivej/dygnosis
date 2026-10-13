//! LSP source acquisition and versioned presentation of shared Rename decisions.

use super::*;
use crate::engine::analysis::symbols::{self, Selector, SemanticModel};

pub(super) const SOURCE: &str = symbols::SOURCE.message;
pub(super) const MAPPING: &str = symbols::MAPPING.message;
pub(super) const STALE: &str =
    "Model inputs changed during Rename. Refresh the model and try again.";
pub(super) const IDENTIFIER: &str = symbols::IDENTIFIER.message;
pub(super) const WRITABLE: &str = "A Rename target is unavailable or read-only. Make every affected source writable and try again.";

pub(super) struct Decision {
    pub range: Range,
    pub edit: WorkspaceEdit,
    roots: Vec<Url>,
}

fn capture(inner: &mut Inner, root: &Url) -> std::result::Result<SemanticModel, &'static str> {
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
    let byte = LineIndex::new(&text).offset_utf16(&text, span_pos(pos.position));
    let mut roots = inner.known_owner_roots(uri);
    if roots.is_empty() && (is_model_root(uri) || uri.scheme() == "untitled") {
        roots.push(uri.clone());
    }
    if roots.is_empty() || !matches!(uri.scheme(), "file" | "untitled") {
        return Err("Rename requires a file or untitled source with a known model owner.");
    }
    let file = crate::engine::workspace::include_resolver::normalize_uri(uri.as_str());
    let mut views = Vec::new();
    for root in &roots {
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
        let proposal =
            symbols::rename(&views, 0, &binding, replacement).map_err(|reason| reason.message)?;
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
                            return Err(WRITABLE)
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
                (target_uri, LineIndex::new(&source), source),
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
        range: span_range(&LineIndex::new(&text), &text, selected_span),
        edit: versioned_workspace_edit(inner, changes),
        roots,
    })
}

/// Ordinary references use accepted sites; callers convert the shared byte spans.
pub(super) fn references(
    inner: &mut Inner,
    pos: &TextDocumentPositionParams,
    include_declaration: bool,
) -> Option<Vec<Location>> {
    let text = inner.document(&pos.text_document.uri)?.text.clone();
    let byte = LineIndex::new(&text).offset_utf16(&text, span_pos(pos.position));
    let file =
        crate::engine::workspace::include_resolver::normalize_uri(pos.text_document.uri.as_str());
    let mut roots = inner.known_owner_roots(&pos.text_document.uri);
    if roots.is_empty() {
        roots.push(pos.text_document.uri.clone());
    }
    let mut locations = Vec::new();
    for root in roots {
        let view = capture(inner, &root).ok()?;
        let binding = view.select(Selector::Target { file: &file, byte }).ok()??;
        for site in view.references(&binding, include_declaration) {
            let Some((file, span)) = site.written else {
                continue;
            };
            let Some(uri) = naming_edit_url(inner, &root, &file) else {
                continue;
            };
            let Some(source) = view.sources.get(&file) else {
                continue;
            };
            if source.get(span.start as usize..span.end as usize).is_none() {
                continue;
            }
            let location = Location::new(uri, span_range(&LineIndex::new(source), source, span));
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
