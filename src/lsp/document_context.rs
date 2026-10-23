//! Current compilation context for an open editor document.

use super::*;
use crate::engine::workspace::include_resolver::normalize_uri;

impl Inner {
    pub(super) fn document_context_snapshot(&self) -> HashMap<Url, Vec<Url>> {
        self.docs
            .keys()
            .map(|document| (document.clone(), self.context_roots(document)))
            .collect()
    }

    /// A completed include walk can establish or remove an editor's owner.
    /// The completed root already has current facts. Other newly required
    /// units need their normal input validation before their reports are used.
    pub(super) fn document_context_changes_after_check(
        &mut self,
        completed: &Url,
        previous: HashMap<Url, Vec<Url>>,
    ) -> Vec<Url> {
        self.revalidate_selected_contexts(Some(completed));
        let completed_key = normalize_uri(completed.as_str());
        let mut changed = HashSet::new();
        for (document, before) in previous {
            if !self.docs.contains_key(&document) {
                continue;
            }
            let mut roots = self.context_roots(&document);
            if roots == before {
                continue;
            }
            if roots.is_empty() && !self.context_ambiguous(&document) {
                roots.push(document);
            }
            changed.extend(
                roots
                    .into_iter()
                    .filter(|root| normalize_uri(root.as_str()) != completed_key),
            );
        }
        let mut changed: Vec<_> = changed.into_iter().collect();
        changed.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        changed
    }

    pub(super) fn browsing_documents(&self, root: &Url) -> Vec<Url> {
        self.document_contexts
            .iter()
            .filter(|(_, owner)| *owner == root)
            .filter_map(|(document, _)| {
                file_url_from_path_key(document).or_else(|| Url::parse(document).ok())
            })
            .collect()
    }
    pub(super) fn analysis_browsing_contexts(&self, root: &Url) -> Vec<String> {
        self.docs
            .keys()
            .filter(|document| *document != root && self.context_roots(document) == [root.clone()])
            .map(|document| normalize_uri(document.as_str()))
            .collect()
    }

    /// A written literal include proves a browsing relation, not execution.
    /// Only acquired files are inspected; dormant bodies are never loaded here.
    pub(super) fn source_browsing_relation(&mut self, root: &Url, document: &Url) -> bool {
        let key = normalize_uri(document.as_str());
        if key == normalize_uri(root.as_str()) {
            return true;
        }
        if self
            .workspace
            .owner_roots(document.as_str())
            .contains(&normalize_uri(root.as_str()))
        {
            return true;
        }
        let Ok(mut lookup) = self.workspace.source_include_lookup(root.as_str()) else {
            return false;
        };
        let Ok(files) = self.workspace.source_include_sources(&mut lookup) else {
            return false;
        };
        for (including, source) in files {
            for token in source
                .tokens
                .iter()
                .filter(|token| token.kind == TokenKind::MacroDir && !token.comment_context)
            {
                let Some(literal) = crate::engine::language::parser::literal_include_filename(
                    token.text(&source.normalized),
                ) else {
                    continue;
                };
                match self
                    .workspace
                    .source_include_target(&mut lookup, &including, &literal)
                {
                    Ok(Some(target)) if target == key => return true,
                    Err(_) => return false,
                    _ => {}
                }
            }
        }
        false
    }

    /// Keep ownership discovery separate from an editor's selected view.
    pub(super) fn context_roots(&self, document: &Url) -> Vec<Url> {
        let key = normalize_uri(document.as_str());
        if let Some(root) = self.document_contexts.get(&key) {
            return vec![root.clone()];
        }
        let owners: Vec<_> = self
            .known_owner_roots(document)
            .into_iter()
            .filter(|root| normalize_uri(root.as_str()) != key)
            .collect();
        match owners.len() {
            0 if is_model_root(document) => vec![document.clone()],
            1 => owners,
            // Older clients cannot choose a context. Preserve their common-fact
            // navigation and merged diagnostics when several owners are known.
            _ if !self.document_context_enabled => owners,
            _ => Vec::new(),
        }
    }

    pub(super) fn context_ambiguous(&self, document: &Url) -> bool {
        self.document_context_enabled
            && self.context_roots(document).is_empty()
            && !self.known_owner_roots(document).is_empty()
    }

    pub(super) fn revalidate_document_contexts(&mut self) {
        self.revalidate_selected_contexts(None);
    }

    fn revalidate_selected_contexts(&mut self, completed: Option<&Url>) {
        let completed = completed.map(|root| normalize_uri(root.as_str()));
        let selections: Vec<_> = self
            .document_contexts
            .iter()
            .filter(|(_, root)| {
                completed
                    .as_ref()
                    .is_none_or(|completed| normalize_uri(root.as_str()) == *completed)
            })
            .map(|(document, root)| (document.clone(), root.clone()))
            .collect();
        for (document, root) in selections {
            let identity = normalize_uri(root.as_str());
            if completed.is_none() {
                self.root_revision(&root);
            }
            if matches!(
                self.workspace.analysis_selection(root.as_str()),
                crate::engine::analysis_configuration::AnalysisSelectionState::Stale { .. }
            ) {
                continue;
            }
            let browsing = file_url_from_path_key(&document)
                .or_else(|| Url::parse(&document).ok())
                .is_some_and(|document| self.source_browsing_relation(&root, &document));
            if document != identity && !browsing {
                self.document_contexts.remove(&document);
            }
        }
    }
}

impl Backend {
    pub(super) async fn set_document_context(&self, arguments: &[Value]) -> Value {
        let output = self.output_gate.lock().await;
        let (result, publications) = {
            let mut inner = self.lock_inner();
            match set_context(&mut inner, arguments) {
                Ok(result) => {
                    inner.document_analysis.cancel();
                    let publications = inner.refresh_diagnostics(None);
                    (result, publications)
                }
                Err(result) => (result, Vec::new()),
            }
        };
        ordering::committed();
        for (uri, version, diagnostics) in publications {
            self.client
                .publish_diagnostics(uri, diagnostics, version)
                .await;
        }
        drop(output);
        self.refresh_presentation(true).await;
        result
    }
}

fn set_context(inner: &mut Inner, arguments: &[Value]) -> std::result::Result<Value, Value> {
    let refusal = |code: &str, message: &str| json!({"code":code,"error":message});
    if !inner.document_context_enabled {
        return Err(refusal(
            "UNSUPPORTED_CAPABILITY",
            "This client did not enable model-context selection. Update the client and restart before choosing a model owner.",
        ));
    }
    let argument = arguments.first().and_then(Value::as_object)
        .ok_or_else(|| refusal("INVALID_ARGUMENTS", "setDocumentContext requires document_uri, document_version and root_uri (a model root or null)."))?;
    let document = argument
        .get("document_uri")
        .and_then(Value::as_str)
        .and_then(|raw| Url::parse(raw).ok())
        .ok_or_else(|| {
            refusal(
                "INVALID_ARGUMENTS",
                "setDocumentContext requires a valid document_uri.",
            )
        })?;
    let version = inner
        .document(&document)
        .map(|doc| doc.version)
        .ok_or_else(|| {
            refusal(
                "DOCUMENT_NOT_OPEN",
                "The document is no longer open. Open it before choosing its model context.",
            )
        })?;
    if argument.get("document_version").and_then(Value::as_i64) != Some(i64::from(version)) {
        return Err(refusal(
            "STALE_DOCUMENT",
            "The document changed. Choose its model context again.",
        ));
    }
    let key = normalize_uri(document.as_str());
    let Some(value) = argument.get("root_uri") else {
        return Err(refusal(
            "INVALID_ARGUMENTS",
            "root_uri must identify a model root, or be null to restore automatic context.",
        ));
    };
    if value.is_null() {
        inner.document_contexts.remove(&key);
    } else {
        let root = value
            .as_str()
            .and_then(|raw| Url::parse(raw).ok())
            .filter(|root| is_model_root(root) && matches!(root.scheme(), "file" | "untitled"))
            .ok_or_else(|| {
                refusal(
                    "INVALID_ARGUMENTS",
                    "The model context must be a file or untitled .mod or .dyn root.",
                )
            })?;
        let revision = inner
            .root_revision(&root)
            .or_else(|| analysis_configuration::context_revision(inner, &root))
            .ok_or_else(|| {
                refusal(
                    "ROOT_NOT_FOUND",
                    "The model root is unavailable. Open the root before choosing it.",
                )
            })?;
        if argument.get("expected_revision").and_then(Value::as_str) != Some(revision.as_str()) {
            return Err(refusal(
                "STALE_MODEL",
                "The model inputs changed. Choose its model context again.",
            ));
        }
        let root_key = normalize_uri(root.as_str());
        if key != root_key && !inner.source_browsing_relation(&root, &document) {
            return Err(refusal(
                "DOCUMENT_NOT_OWNED",
                "The chosen model does not include this document. Open its model root and choose a current owner.",
            ));
        }
        inner.document_contexts.insert(key, root);
    }
    Ok(
        json!({"schema_version":1,"document_uri":document,"document_version":version,"root_uris":inner.context_roots(&document),"automatic":value.is_null()}),
    )
}
