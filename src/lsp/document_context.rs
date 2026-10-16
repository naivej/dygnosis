//! Current compilation context for an open editor document.

use super::*;
use crate::engine::workspace::include_resolver::normalize_uri;

impl Inner {
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
        let selections: Vec<_> = self
            .document_contexts
            .iter()
            .map(|(document, root)| (document.clone(), root.clone()))
            .collect();
        for (document, root) in selections {
            let identity = normalize_uri(root.as_str());
            self.root_revision(&root);
            if document != identity && !self.workspace.owner_roots(&document).contains(&identity) {
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
        let revision = inner.root_revision(&root).ok_or_else(|| {
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
        if key != root_key
            && !inner
                .workspace
                .owner_roots(document.as_str())
                .contains(&root_key)
        {
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
