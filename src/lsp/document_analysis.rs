//! Coalesce open-document checks outside the editor input and request lock.

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
pub(super) struct DocumentAnalysis {
    generation: u64,
    pending: HashMap<Url, u64>,
    prior: HashMap<Url, workspace_checks::CachedReport>,
    running: bool,
    active_root: Option<Url>,
    pub(super) cancellation: Option<Arc<AtomicBool>>,
    #[cfg(test)]
    pub(super) completed_snapshot: Option<Url>,
}

impl DocumentAnalysis {
    pub(super) fn retire_aliases(&mut self, document: &Url) {
        let key = crate::engine::workspace::include_resolver::normalize_uri(document.as_str());
        self.pending.retain(|root, _| {
            root == document
                || crate::engine::workspace::include_resolver::normalize_uri(root.as_str()) != key
        });
        self.prior.retain(|root, _| self.pending.contains_key(root));
        if self.active_root.as_ref().is_some_and(|root| {
            root != document
                && crate::engine::workspace::include_resolver::normalize_uri(root.as_str()) == key
        }) && let Some(flag) = &self.cancellation
        {
            flag.store(true, Ordering::Relaxed);
        }
    }
    pub(super) fn has_pending(&self, root: &Url) -> bool {
        self.pending.contains_key(root)
    }
    /// Configuration and explicit context changes can complete synchronously.
    /// Their results must not be overwritten by an earlier editor snapshot.
    pub(super) fn cancel(&mut self) {
        self.generation += 1;
        self.pending.clear();
        self.prior.clear();
        if let Some(flag) = self.cancellation.take() {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

impl Inner {
    /// Written trivia starts in root lexical context only when this document
    /// owns its current view and no queued different root can acquire it.
    pub(super) fn source_has_root_context(&self, document: &Url) -> bool {
        let key = crate::engine::workspace::include_resolver::normalize_uri(document.as_str());
        let roots = self.context_roots(document);
        roots.len() == 1
            && crate::engine::workspace::include_resolver::normalize_uri(roots[0].as_str()) == key
            && self.document_analysis.pending.keys().all(|root| {
                crate::engine::workspace::include_resolver::normalize_uri(root.as_str()) == key
                    || !self
                        .workspace
                        .root_may_use(root.as_str(), document.as_str())
            })
    }

    fn document_analysis_root_is_current(&self, root: &Url) -> bool {
        let contexts = self.context_roots(root);
        contexts.is_empty() || contexts == [root.clone()]
    }

    pub(super) fn document_analysis_pending(&self, document: &Url) -> bool {
        if self.document_analysis.pending.is_empty() {
            return false;
        }
        let roots = self.context_roots(document);
        self.document_analysis.pending.keys().any(|root| {
            let related = roots.contains(root)
                || roots.is_empty() && root == document
                || is_model_root(root)
                    && (self
                        .workspace
                        .root_may_use(root.as_str(), document.as_str())
                        || !is_model_root(document)
                            && !self.workspace.has_joined_source(root.as_str()));
            related && self.document_analysis_root_is_current(root)
        })
    }

    pub(super) fn queue_document_context_changes(
        &mut self,
        root: &Url,
        previous: HashMap<Url, Vec<Url>>,
    ) {
        for required_root in self.document_context_changes_after_check(root, previous) {
            if self.document_analysis.has_pending(&required_root) {
                continue;
            }
            if let Some(report) = self.reports.remove(&required_root)
                && let Some(prior) = workspace_checks::CachedReport::capture(report)
            {
                self.document_analysis
                    .prior
                    .insert(required_root.clone(), prior);
            }
            self.document_analysis.generation += 1;
            let next = self.document_analysis.generation;
            self.document_analysis.pending.insert(required_root, next);
        }
    }

    pub(super) fn queue_document_analysis(&mut self, changed: &Url) {
        let mut roots = HashSet::new();
        for document in self.docs.keys() {
            let contexts = self.context_roots(document);
            if contexts.is_empty() && !self.context_ambiguous(document) {
                roots.insert(document.clone());
            } else {
                roots.extend(contexts);
            }
        }
        for root in roots {
            if self.workspace.root_may_use(root.as_str(), changed.as_str()) {
                self.document_analysis.generation += 1;
                if self.document_analysis.active_root.as_ref() == Some(&root)
                    && let Some(flag) = &self.document_analysis.cancellation
                {
                    flag.store(true, Ordering::Relaxed);
                }
                self.semantic_views.remove(&root);
                if let Some(report) = self.reports.remove(&root)
                    && let Some(prior) = workspace_checks::CachedReport::capture(report)
                {
                    self.document_analysis.prior.insert(root.clone(), prior);
                }
                self.document_analysis
                    .pending
                    .insert(root, self.document_analysis.generation);
            }
        }
    }
}

impl Backend {
    pub(super) fn kick_document_analysis(&self) {
        let mut inner = self.lock_inner();
        if inner.document_analysis.running
            || inner.document_analysis.pending.is_empty()
            || inner.workspace_checks.shutdown
        {
            return;
        }
        inner.document_analysis.running = true;
        let state = Arc::clone(&self.inner);
        let done = Arc::clone(&self.document_analysis_done);
        let workspace_wake = Arc::clone(&self.workspace_wake);
        let output = Arc::clone(&self.output_gate);
        let client = self.client.clone();
        let presentation = Arc::clone(&self.presentation_refresh);
        tokio::spawn(async move {
            loop {
                let work = {
                    let mut inner = state.lock().unwrap_or_else(|error| error.into_inner());
                    let root = inner
                        .document_analysis
                        .pending
                        .keys()
                        .min_by(|a, b| {
                            (!is_model_root(a), a.as_str()).cmp(&(!is_model_root(b), b.as_str()))
                        })
                        .cloned();
                    if inner.workspace_checks.shutdown || root.is_none() {
                        inner.document_analysis.running = false;
                        inner.document_analysis.cancellation = None;
                        inner.document_analysis.active_root = None;
                        done.notify_waiters();
                        return;
                    }
                    let root = root.unwrap();
                    if !inner.document_analysis_root_is_current(&root) {
                        inner.document_analysis.pending.remove(&root);
                        inner.document_analysis.prior.remove(&root);
                        done.notify_waiters();
                        continue;
                    }
                    let generation = inner.document_analysis.pending[&root];
                    let prior = inner.document_analysis.prior.get(&root).cloned();
                    let settings = inner.prepare_root(&root);
                    let overlays = inner.overlay_versions();
                    let mut workspace = inner.workspace.snapshot_for_root_with_limit(
                        root.as_str(),
                        settings.search_paths,
                        settings.macro_work_limit,
                    );
                    let cancellation = Arc::new(AtomicBool::new(false));
                    workspace.set_cancellation(Arc::clone(&cancellation));
                    inner.document_analysis.cancellation = Some(cancellation);
                    inner.document_analysis.active_root = Some(root.clone());
                    #[cfg(test)]
                    {
                        inner.document_analysis.completed_snapshot = None;
                    }
                    (root, generation, workspace, overlays, prior)
                };
                let (root, generation, workspace, overlays, prior) = work;
                let target = root.clone();
                let result = tokio::task::spawn_blocking(move || {
                    workspace_checks::compute(&target, workspace, prior)
                })
                .await;
                #[cfg(test)]
                {
                    state.lock().unwrap().document_analysis.completed_snapshot = Some(root.clone());
                }
                let gate = output.lock().await;
                let (publications, change, failure, tokens, hints) = {
                    let mut inner = state.lock().unwrap_or_else(|error| error.into_inner());
                    if inner.document_analysis.pending.get(&root) != Some(&generation)
                        || inner.workspace_checks.shutdown
                    {
                        continue;
                    }
                    if result.as_ref().is_ok_and(|result| {
                        result.as_ref().is_ok_and(|checked| {
                            !checked.current || !checked.overlays_match(&inner, &overlays)
                        })
                    }) {
                        continue;
                    }
                    inner.document_analysis.pending.remove(&root);
                    inner.document_analysis.prior.remove(&root);
                    inner.document_analysis.cancellation = None;
                    let (change, failure) = match result {
                        Ok(Ok(checked)) if checked.current => {
                            let contexts = inner.document_context_snapshot();
                            let revision = checked.report.revision.clone();
                            inner.commit_document_check(&root, checked);
                            inner.queue_document_context_changes(&root, contexts);
                            let mut hash = std::collections::hash_map::DefaultHasher::new();
                            revision.hash(&mut hash);
                            let context_revision = format!("{:016x}", hash.finish());
                            let previous = inner
                                .tracked_roots
                                .insert(root.clone(), context_revision.clone());
                            (
                                (inner.model_info_notifications
                                    && previous.as_ref() != Some(&context_revision))
                                .then(|| {
                                    json!({
                                        "schema_version":MODEL_INFO_SCHEMA_VERSION,"root_uri":root,
                                        "revision":context_revision,"input_revision":revision,
                                    })
                                }),
                                None,
                            )
                        }
                        failure => {
                            let message = match failure {
                                Ok(Err(message)) => message,
                                Err(error) => format!("The check worker failed: {error}"),
                                Ok(Ok(_)) => unreachable!(),
                            };
                            (
                                None,
                                Some(format!(
                                    "Model checks for {root} could not finish. {message} No new diagnostic report was produced."
                                )),
                            )
                        }
                    };
                    // Newly discovered includes replace standalone helper reports.
                    let obsolete: Vec<_> = inner
                        .reports
                        .keys()
                        .filter(|uri| {
                            !is_model_root(uri) && !inner.known_owner_roots(uri).is_empty()
                        })
                        .cloned()
                        .collect();
                    for uri in obsolete {
                        inner.reports.remove(&uri);
                    }
                    let publications = inner.merge_diagnostics(Some(&root));
                    (
                        publications,
                        change,
                        failure,
                        inner.token_refresh,
                        inner.hint_refresh,
                    )
                };
                for (uri, version, diagnostics) in publications {
                    client.publish_diagnostics(uri, diagnostics, version).await;
                }
                drop(gate);
                if let Some(change) = change {
                    client.send_notification::<ModelInfoChanged>(change).await;
                }
                if let Some(failure) = failure {
                    client.log_message(MessageType::WARNING, failure).await;
                }
                presentation.request(&client, tokens, hints);
                done.notify_waiters();
                workspace_wake.notify_one();
            }
        });
    }

    pub(super) async fn wait_document_analysis(&self, document: Option<&Url>) {
        self.kick_document_analysis();
        loop {
            let notified = self.document_analysis_done.notified();
            // Register before observing state, so a completion cannot be lost.
            tokio::pin!(notified);
            notified.as_mut().enable();
            let pending = {
                let inner = self.lock_inner();
                match document {
                    None => !inner.document_analysis.pending.is_empty(),
                    Some(document) => inner.document_analysis_pending(document),
                }
            };
            if !pending {
                return;
            }
            notified.await;
        }
    }
}
