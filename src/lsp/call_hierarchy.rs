//! Configured equation dependencies with revision-bound written locations.
use super::*;
use crate::engine::analysis::symbols::{Binding, Selector, SemanticModel};
use crate::engine::language::expand::ExpandReport;
use crate::engine::workspace::include_resolver::normalize_uri;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tower_lsp::jsonrpc::Error;

const UNAVAILABLE: &str = "Call Hierarchy is unavailable because the model or its source locations could not be verified.";
const OWNER: &str = "Choose a model owner before using Call Hierarchy for this file.";
const STALE: &str = "The model changed. Run Call Hierarchy again.";
const MAPPING: &str = "Call Hierarchy is unavailable because some model names or equation uses have no verified source location.";
const FRAGMENT_DETAIL: &str = "equation fragment";
const INCOMPLETE: &str =
    "Call Hierarchy is unavailable because the model could not be fully analyzed: ";

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "dynare", rename_all = "snake_case")]
enum Subject {
    Variable { binding: String },
    Equation { equation: usize },
}

#[derive(Deserialize, Serialize)]
struct Target {
    root_uri: Url,
    document_uri: Url,
    revision: String,
    #[serde(flatten)]
    subject: Subject,
}

fn refusal(message: impl Into<String>) -> Error {
    Error {
        message: message.into().into(),
        ..Error::invalid_request()
    }
}

struct Context {
    root: Url,
    document: Url,
    view: Arc<SemanticModel>,
    report: Arc<ExpandReport>,
}

impl Context {
    fn current(inner: &mut Inner, uri: &Url) -> Result<Self> {
        let mut roots = inner.context_roots(uri);
        if inner.context_ambiguous(uri) || roots.len() > 1 {
            return Err(refusal(OWNER));
        }
        if roots.is_empty() && is_model_root(uri) {
            roots.push(uri.clone());
        }
        let root = roots.pop().ok_or_else(|| refusal(UNAVAILABLE))?;
        let view = typed_names::root_view(inner, &root).ok_or_else(|| refusal(UNAVAILABLE))?;
        if !view.complete {
            let causes = crate::engine::analysis::model_info::incomplete_causes(
                &mut inner.workspace,
                root.as_str(),
                &view.model,
            );
            let cause = causes
                .into_iter()
                .map(|cause| cause.message)
                .collect::<Vec<_>>()
                .join(" ");
            return Err(refusal(if cause.is_empty() {
                UNAVAILABLE.to_owned()
            } else {
                format!("{INCOMPLETE}{cause}")
            }));
        }
        let report = inner
            .workspace
            .expand_report_snapshot(root.as_str())
            .ok_or_else(|| refusal(UNAVAILABLE))?;
        Ok(Self {
            root,
            document: uri.clone(),
            view,
            report,
        })
    }

    fn restore(inner: &mut Inner, item: &CallHierarchyItem) -> Result<(Self, Subject)> {
        let target: Target =
            serde_json::from_value(item.data.clone().ok_or_else(|| refusal(STALE))?)
                .map_err(|_| refusal(STALE))?;
        let context = Self::current(inner, &target.document_uri)?;
        if context.root != target.root_uri || context.view.revision != target.revision {
            return Err(refusal(STALE));
        }
        Ok((context, target.subject))
    }

    fn data(&self, subject: Subject) -> Value {
        serde_json::to_value(Target {
            root_uri: self.root.clone(),
            document_uri: self.document.clone(),
            revision: self.view.revision.clone(),
            subject,
        })
        .expect("hierarchy target serialization")
    }

    fn equation_at_order(&self, order: usize) -> Option<usize> {
        let rows = &self.view.model.written_equations;
        let index = rows
            .partition_point(|row| row.token_range.start <= order)
            .checked_sub(1)?;
        let occurrence = self.report.model_map.equations.get(index)?;
        (rows[index].token_range.contains(&order) && occurrence.active && !occurrence.local)
            .then_some(index)
    }
}

struct Locations<'a> {
    inner: &'a Inner,
    context: &'a Context,
    indexes: HashMap<String, (Url, Arc<LineIndex>)>,
}

impl<'a> Locations<'a> {
    fn new(inner: &'a Inner, context: &'a Context) -> Self {
        Self {
            inner,
            context,
            indexes: HashMap::new(),
        }
    }

    fn location(&mut self, file: &str, span: Span) -> Option<Location> {
        let text = self.context.view.sources.get(file)?;
        text.get(span.start as usize..span.end as usize)?;
        if !self.indexes.contains_key(file) {
            let uri = naming_edit_url(self.inner, &self.context.root, file)?;
            let source = self
                .inner
                .workspace
                .source_document(self.context.root.as_str(), file)?;
            self.indexes
                .insert(file.to_owned(), (uri, source.source_index()));
        }
        let (uri, index) = self.indexes.get(file)?;
        Some(Location::new(uri.clone(), span_range(index, text, span)))
    }

    fn variable(&mut self, binding: &Binding) -> Option<CallHierarchyItem> {
        let (file, span) = binding.declaration.as_ref()?;
        let location = self.location(file, *span)?;
        Some(CallHierarchyItem {
            name: binding.name.clone(),
            kind: SymbolKind::VARIABLE,
            tags: None,
            detail: Some("variable".into()),
            uri: location.uri,
            range: location.range,
            selection_range: location.range,
            data: Some(self.context.data(Subject::Variable {
                binding: binding.id.clone(),
            })),
        })
    }

    fn equation(&mut self, row: usize, segment: usize) -> Option<CallHierarchyItem> {
        let occurrence = self.context.report.model_map.equations.get(row)?;
        let part = occurrence.source.segments.get(segment)?;
        let file = part.file.as_deref()?;
        let location = self.location(file, part.span)?;
        let selection_range = occurrence
            .source
            .anchor
            .as_ref()
            .filter(|anchor| {
                anchor.file == part.file
                    && part.span.start <= anchor.span.start
                    && anchor.span.end <= part.span.end
            })
            .and_then(|anchor| self.location(file, anchor.span))
            .map(|anchor| anchor.range)
            .unwrap_or_else(|| Range::new(location.range.start, location.range.start));
        let equation = &self.context.view.model.written_equations.get(row)?.equation;
        let name = if !equation.name.is_empty() {
            equation.name.clone()
        } else {
            format!("{} = ...", equation.lhs.trim())
        };
        Some(CallHierarchyItem {
            name,
            kind: SymbolKind::FUNCTION,
            tags: None,
            detail: Some(if occurrence.source.segments.len() > 1 {
                FRAGMENT_DETAIL.into()
            } else {
                "equation".into()
            }),
            uri: location.uri,
            range: location.range,
            selection_range,
            data: Some(self.context.data(Subject::Equation {
                equation: occurrence.id,
            })),
        })
    }
}

pub(super) fn prepare(
    inner: &mut Inner,
    pos: &TextDocumentPositionParams,
) -> Result<Option<Vec<CallHierarchyItem>>> {
    controlled(|| prepare_current(inner, pos))
}

pub(super) fn incoming(
    inner: &mut Inner,
    item: &CallHierarchyItem,
) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
    controlled(|| incoming_current(inner, item))
}

pub(super) fn outgoing(
    inner: &mut Inner,
    item: &CallHierarchyItem,
) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
    controlled(|| outgoing_current(inner, item))
}

fn controlled<T>(work: impl FnOnce() -> Result<T>) -> Result<T> {
    crate::engine::control::run(None, work).unwrap_or_else(|_| Err(Error::request_cancelled()))
}

fn prepare_current(
    inner: &mut Inner,
    pos: &TextDocumentPositionParams,
) -> Result<Option<Vec<CallHierarchyItem>>> {
    let context = Context::current(inner, &pos.text_document.uri)?;
    let file = normalize_uri(pos.text_document.uri.as_str());
    let source = inner
        .workspace
        .source_document(context.root.as_str(), &file)
        .ok_or_else(|| refusal(UNAVAILABLE))?;
    let byte = source
        .source_index()
        .offset_utf16(source.source(), span_pos(pos.position));
    let mut locations = Locations::new(inner, &context);
    let mut items = Vec::new();
    if let Some(binding) = context
        .view
        .select(Selector::Target { file: &file, byte })
        .map_err(|error| refusal(error.reason.message))?
        .filter(Binding::is_ordinary)
    {
        items.push(
            locations
                .variable(&binding)
                .ok_or_else(|| refusal(MAPPING))?,
        );
    }
    for (row, occurrence) in context.report.model_map.equations.iter().enumerate() {
        if crate::engine::control::cancelled() {
            return Err(Error::request_cancelled());
        }
        if !occurrence.active || occurrence.local {
            continue;
        }
        for (segment, part) in occurrence.source.segments.iter().enumerate() {
            if part.file.as_deref() == Some(file.as_str())
                && part.span.start <= byte
                && byte < part.span.end
            {
                items.push(
                    locations
                        .equation(row, segment)
                        .ok_or_else(|| refusal(MAPPING))?,
                );
            }
        }
    }
    Ok((!items.is_empty()).then_some(items))
}

fn incoming_current(
    inner: &mut Inner,
    item: &CallHierarchyItem,
) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
    let (context, subject) = Context::restore(inner, item)?;
    let Subject::Variable { binding } = subject else {
        return Ok(Some(Vec::new()));
    };
    let binding = context
        .view
        .select(Selector::Id(&binding))
        .map_err(|error| refusal(error.reason.message))?
        .ok_or_else(|| refusal(STALE))?;
    let mut locations = Locations::new(inner, &context);
    let mut grouped: BTreeMap<(usize, usize), Vec<Range>> = BTreeMap::new();
    for site in context.view.references(&binding, false) {
        if crate::engine::control::cancelled() {
            return Err(Error::request_cancelled());
        }
        let Some(row) = context.equation_at_order(site.order) else {
            continue;
        };
        let (file, span) = site.written.ok_or_else(|| refusal(MAPPING))?;
        let segment = context.report.model_map.equations[row]
            .source
            .segments
            .iter()
            .position(|part| {
                part.file.as_deref() == Some(file.as_str())
                    && part.span.start <= span.start
                    && span.end <= part.span.end
            })
            .ok_or_else(|| refusal(MAPPING))?;
        let location = locations
            .location(&file, span)
            .ok_or_else(|| refusal(MAPPING))?;
        grouped
            .entry((row, segment))
            .or_default()
            .push(location.range);
    }
    let calls = grouped
        .into_iter()
        .map(|((row, segment), mut ranges)| {
            ranges.sort_by_key(|range| (range.start, range.end));
            ranges.dedup();
            Ok(CallHierarchyIncomingCall {
                from: locations
                    .equation(row, segment)
                    .ok_or_else(|| refusal(MAPPING))?,
                from_ranges: ranges,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(calls))
}

fn outgoing_current(
    inner: &mut Inner,
    item: &CallHierarchyItem,
) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
    let (context, subject) = Context::restore(inner, item)?;
    let Subject::Equation { equation } = subject else {
        return Ok(Some(Vec::new()));
    };
    let row = context
        .report
        .model_map
        .equations
        .iter()
        .position(|row| row.id == equation && row.active && !row.local)
        .ok_or_else(|| refusal(STALE))?;
    let tokens = &context.view.model.written_equations[row].token_range;
    let bindings: HashMap<_, _> = context
        .view
        .binding_inventory()
        .iter()
        .filter(|binding| binding.is_ordinary())
        .map(|binding| (binding.name.as_str(), binding))
        .collect();
    let mut grouped: BTreeMap<&str, Vec<Range>> = BTreeMap::new();
    let mut locations = Locations::new(inner, &context);
    for (name, site) in context.view.accepted_ordinary_sites() {
        if crate::engine::control::cancelled() {
            return Err(Error::request_cancelled());
        }
        if site.declaration || !tokens.contains(&site.order) || !bindings.contains_key(name) {
            continue;
        }
        let (file, span) = site.written.as_ref().ok_or_else(|| refusal(MAPPING))?;
        let location = locations
            .location(file, *span)
            .ok_or_else(|| refusal(MAPPING))?;
        if location.uri == item.uri
            && item.range.start <= location.range.start
            && location.range.end <= item.range.end
        {
            grouped.entry(name).or_default().push(location.range);
        }
    }
    let calls = grouped
        .into_iter()
        .map(|(name, mut ranges)| {
            ranges.sort_by_key(|range| (range.start, range.end));
            ranges.dedup();
            Ok(CallHierarchyOutgoingCall {
                to: locations
                    .variable(bindings[name])
                    .ok_or_else(|| refusal(MAPPING))?,
                from_ranges: ranges,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(calls))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::control;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn cancellation_cannot_publish_empty_or_partial_hierarchy_results() {
        // Guards lazy accepted-site iteration stopping before its consumer's
        // loop body can observe cancellation.
        let uri = Url::parse("file:///hierarchy-cancellation.mod").unwrap();
        let text = "var y; parameters beta; model; y=beta*y(-1); end;";
        let mut inner = Inner::default();
        inner.workspace.update_document(uri.as_str(), text);
        let params = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position::new(0, text.find("y=beta").unwrap() as u32),
        };
        let items = prepare(&mut inner, &params).unwrap().unwrap();
        let variable = items.iter().find(|item| item.name == "y").unwrap();
        let equation = items
            .iter()
            .find(|item| item.detail.as_deref() == Some("equation"))
            .unwrap();
        incoming(&mut inner, variable).unwrap();
        outgoing(&mut inner, equation).unwrap();
        for request in 0..3 {
            let mut interrupted = 0;
            for checkpoint in 0..128 {
                let flag = Arc::new(AtomicBool::new(false));
                control::cancel_after(&flag, checkpoint);
                let mut observed = None;
                let _ = control::run(Some(Arc::clone(&flag)), || {
                    let result = match request {
                        0 => prepare(&mut inner, &params).map(|_| ()),
                        1 => incoming(&mut inner, variable).map(|_| ()),
                        _ => outgoing(&mut inner, equation).map(|_| ()),
                    };
                    observed = Some((result, flag.load(Ordering::Relaxed)));
                });
                if let Some((result, true)) = observed {
                    interrupted += 1;
                    assert_eq!(
                        result.expect_err("cancelled hierarchy result").code,
                        tower_lsp::jsonrpc::ErrorCode::RequestCancelled
                    );
                }
            }
            assert!(
                interrupted > 0,
                "request {request} reached cancellation checkpoints"
            );
        }
        let flag = Arc::new(AtomicBool::new(false));
        control::cancel_after(&flag, 0);
        control::cancelled();
    }
}
