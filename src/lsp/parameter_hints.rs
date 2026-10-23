//! Reuse complete hint results for current open-document inputs and owner context.
use super::*;
use crate::engine::language::expand::ExpandReport;
use crate::engine::workspace::SourceDocument;
use std::sync::{OnceLock, Weak};

pub(super) struct Entry {
    root: Url,
    revision: String,
    source: SourceDocument,
    file: String,
    model: Weak<Model>,
    report: Weak<ExpandReport>,
    complete: bool,
    hints: OnceLock<Vec<InlayHint>>,
}

impl Entry {
    fn matches(
        &self,
        root: &Url,
        revision: &str,
        source: &SourceDocument,
        model: &Arc<Model>,
        report: &Arc<ExpandReport>,
        complete: bool,
    ) -> bool {
        &self.root == root
            && self.revision == revision
            && self.source.same_source(source)
            && self.model.ptr_eq(&Arc::downgrade(model))
            && self.report.ptr_eq(&Arc::downgrade(report))
            && self.complete == complete
    }

    fn project(&self, model: &Model, report: &ExpandReport) -> Vec<InlayHint> {
        if !self.complete {
            return Vec::new();
        }
        let text = self.source.source();
        let index = value_hint_source_index(&self.source);
        let key = &self.file;
        // All executions at an anchor take part, including unknown values.
        let mut sites: std::collections::BTreeMap<(u32, u32), ValueHintSite> =
            std::collections::BTreeMap::new();
        for value in crate::engine::analysis::assignment_values::assignment_values(model) {
            let source = &report.model_map.statements[value.statement_id];
            let Some(anchor) = source
                .anchor
                .as_ref()
                .filter(|anchor| anchor.file.as_deref() == Some(key.as_str()))
            else {
                continue;
            };
            let range = if source.segments.len() == 1
                && source.segments[0].file.as_deref() == Some(key.as_str())
            {
                value_hint_range(&index, text, source.segments[0].span)
            } else {
                None
            };
            let proof = value.value.filter(|_| range.is_some());
            sites
                .entry((anchor.span.start, anchor.span.end))
                .and_modify(|site| {
                    if site.value.map(f64::to_bits) != proof.map(f64::to_bits)
                        || site.range != range
                    {
                        site.value = None;
                    }
                    site.plain |= value.written_plain_number;
                })
                .or_insert(ValueHintSite {
                    value: proof,
                    range,
                    plain: value.written_plain_number,
                    statement_id: value.statement_id,
                });
        }
        sites.into_values().filter_map(|ValueHintSite {value, range, plain, statement_id:id}| {
            let value = value?;
            if plain { return None; }
            Some(InlayHint {
                position: range?.end, label: InlayHintLabel::String(format!("= {value}")), kind: None,
                text_edits: None, tooltip: Some(InlayHintTooltip::String("Value from this expression and earlier assignments.".into())),
                padding_left: Some(true), padding_right: Some(false),
                data: Some(json!({"root_uri":self.root,"revision":self.revision,"statement_id":format!("s{id}")})),
            })
        }).collect()
    }
}

impl Backend {
    pub(super) fn value_hints(&self, params: &InlayHintParams) -> Vec<InlayHint> {
        let uri = &params.text_document.uri;
        let (entry, model, report, retained) = {
            let mut inner = self.lock_inner();
            if !inner.presentation_for(uri).parameter_value_hints
                || inner.document_analysis_pending(uri)
            {
                return Vec::new();
            }
            let roots = inner.context_roots(uri);
            let [root] = roots.as_slice() else {
                return Vec::new();
            };
            let Some(revision) = inner.root_revision(root) else {
                return Vec::new();
            };
            let Some(model) = inner.workspace.effective_model_snapshot(root.as_str()) else {
                return Vec::new();
            };
            let Some(report) = inner.workspace.expand_report_snapshot(root.as_str()) else {
                return Vec::new();
            };
            let key = crate::engine::workspace::include_resolver::normalize_uri(uri.as_str());
            let Some(source) = inner.workspace.source_document(root.as_str(), &key) else {
                return Vec::new();
            };
            let complete = report.complete
                && report.model_map.complete
                && inner.workspace.includes_complete(root.as_str());
            let retained = inner.docs.contains_key(uri);
            let cached = inner
                .parameter_hints
                .get(uri)
                .filter(|entry| entry.matches(root, &revision, &source, &model, &report, complete));
            let entry = if let Some(entry) = cached {
                // A completed projection needs no unlocked work or second
                // context pass. Return within the current input proof.
                if let Some(hints) = entry.hints.get() {
                    return hints
                        .iter()
                        .filter(|hint| pos_in_range(hint.position, params.range))
                        .cloned()
                        .collect();
                }
                Arc::clone(entry)
            } else {
                let entry = Arc::new(Entry {
                    root: root.clone(),
                    revision,
                    source,
                    file: key,
                    model: Arc::downgrade(&model),
                    report: Arc::downgrade(&report),
                    complete,
                    hints: OnceLock::new(),
                });
                if retained {
                    inner
                        .parameter_hints
                        .insert(uri.clone(), Arc::clone(&entry));
                }
                entry
            };
            (entry, model, report, retained)
        };
        // Coalesce simultaneous requests. Calculation uses immutable snapshots
        // outside Inner so independent requests can proceed.
        let hints = entry.hints.get_or_init(|| entry.project(&model, &report));
        let result = hints
            .iter()
            .filter(|hint| pos_in_range(hint.position, params.range))
            .cloned()
            .collect();
        {
            // Inputs could change while the calculation had released Inner.
            let mut inner = self.lock_inner();
            if inner.document_analysis_pending(uri)
                || !inner.presentation_for(uri).parameter_value_hints
                || inner.context_roots(uri) != [entry.root.clone()]
                || inner.root_revision(&entry.root).as_deref() != Some(&entry.revision)
                || retained
                    && !inner
                        .parameter_hints
                        .get(uri)
                        .is_some_and(|current| Arc::ptr_eq(current, &entry))
            {
                return Vec::new();
            }
        }
        result
    }
}
