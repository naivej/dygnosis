//! Earlier occurrences retained by checks, then mapped independently to written source.

use std::collections::HashMap;
use std::sync::Arc;

use crate::diagnostic::{Diagnostic, DiagnosticOrigin};
use crate::model::{Equation, Model};
use crate::model_map::{SourceOccurrence, WrittenSegment};
use crate::span::Span;
use crate::workspace::Workspace;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelatedOccurrence {
    TypeEvent(usize),
    Equation(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelatedFrame {
    pub kind: String,
    pub variable: Option<String>,
    pub value: Option<String>,
    pub locations: Vec<DiagnosticOrigin>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelatedDiagnostic {
    /// Effective-source span, or written span when `file` is explicit.
    pub span: Span,
    pub message: String,
    pub file: Option<String>,
    pub occurrence: Option<RelatedOccurrence>,
    pub locations: Vec<DiagnosticOrigin>,
    pub origin_frames: Vec<RelatedFrame>,
    pub mapped: bool,
}

impl RelatedDiagnostic {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            file: None,
            occurrence: None,
            locations: Vec::new(),
            origin_frames: Vec::new(),
            mapped: false,
        }
    }

    pub fn written(file: String, span: Span, message: impl Into<String>) -> Self {
        Self {
            file: Some(file),
            ..Self::new(span, message)
        }
    }

    pub fn type_event(span: Span, index: usize) -> Self {
        Self {
            occurrence: Some(RelatedOccurrence::TypeEvent(index)),
            ..Self::new(span, "Earlier declaration or creating occurrence")
        }
    }

    pub fn equation(model: &Model, equation: &Equation, span: Span) -> Self {
        // Expression IDs identify the parser occurrence even when macros reuse
        // identical written spans. Do not recover an occurrence by its text.
        let occurrence = model
            .written_equations
            .iter()
            .find(|row| {
                (equation.lhs_expr.is_some() || equation.rhs_expr.is_some())
                    && row.equation.lhs_expr == equation.lhs_expr
                    && row.equation.rhs_expr == equation.rhs_expr
            })
            .map(|row| RelatedOccurrence::Equation(row.token_range.start));
        Self {
            occurrence,
            ..Self::new(span, "Earlier equation or local definition")
        }
    }
}

pub(crate) fn map_related(
    workspace: &mut Workspace,
    root: &str,
    diagnostics: &mut [Diagnostic],
    texts: &mut HashMap<String, Arc<str>>,
) {
    if !diagnostics
        .iter()
        .any(|diagnostic| !diagnostic.related.is_empty())
    {
        return;
    }
    let map = workspace
        .expand_report(root)
        .map(|report| report.model_map.clone());
    for related in diagnostics
        .iter_mut()
        .flat_map(|diagnostic| &mut diagnostic.related)
    {
        related.mapped = true;
        let segments = if let Some(file) = &related.file {
            vec![WrittenSegment {
                file: Some(file.clone()),
                span: related.span,
            }]
        } else {
            workspace.map_effective_segments(root, related.span)
        };
        related.locations = locations(workspace, segments, texts);
        let source = occurrence_source(map.as_ref(), &related.occurrence);
        if let Some(source) = source {
            related.origin_frames = source
                .origin_frames
                .iter()
                .map(|frame| RelatedFrame {
                    kind: frame.kind.clone(),
                    variable: frame.variable.clone(),
                    value: frame.value.clone(),
                    locations: locations(workspace, frame.segments.clone(), texts),
                })
                .collect();
        }
    }
}

fn occurrence_source<'a>(
    map: Option<&'a crate::model_map::WrittenModelMap>,
    occurrence: &Option<RelatedOccurrence>,
) -> Option<&'a SourceOccurrence> {
    match (map, occurrence) {
        (Some(map), Some(RelatedOccurrence::TypeEvent(index))) => map
            .type_events
            .iter()
            .find(|(event, _)| event == index)
            .map(|(_, source)| source),
        (Some(map), Some(RelatedOccurrence::Equation(id))) => map
            .equations
            .iter()
            .find(|row| row.id == *id)
            .map(|row| &row.source),
        _ => None,
    }
}

/// Free-text MCP analysis keeps its existing no-workspace/no-disk route.
pub(crate) fn map_free_text(text: &str, diagnostics: &mut [Diagnostic]) {
    if !diagnostics
        .iter()
        .any(|diagnostic| !diagnostic.related.is_empty())
    {
        return;
    }
    let report = crate::expand::expand_report(text);
    let snapshot: Arc<str> = Arc::from(text);
    for related in diagnostics
        .iter_mut()
        .flat_map(|diagnostic| &mut diagnostic.related)
    {
        related.mapped = true;
        if text
            .get(related.span.start as usize..related.span.end as usize)
            .is_some()
            && related.file.is_none()
        {
            related.locations = vec![DiagnosticOrigin {
                file: String::new(),
                text: snapshot.clone(),
                span: related.span,
            }];
        }
        if let Some(source) = occurrence_source(Some(&report.model_map), &related.occurrence) {
            related.origin_frames = source
                .origin_frames
                .iter()
                .map(|frame| RelatedFrame {
                    kind: frame.kind.clone(),
                    variable: frame.variable.clone(),
                    value: frame.value.clone(),
                    locations: frame
                        .segments
                        .iter()
                        .filter(|segment| segment.file.is_none())
                        .filter_map(|segment| {
                            text.get(segment.span.start as usize..segment.span.end as usize)?;
                            Some(DiagnosticOrigin {
                                file: String::new(),
                                text: snapshot.clone(),
                                span: segment.span,
                            })
                        })
                        .collect(),
                })
                .collect();
        }
    }
}

fn locations(
    workspace: &mut Workspace,
    segments: Vec<WrittenSegment>,
    texts: &mut HashMap<String, Arc<str>>,
) -> Vec<DiagnosticOrigin> {
    segments
        .into_iter()
        .filter_map(|segment| {
            let file = segment.file?;
            let text = if let Some(text) = texts.get(&file) {
                text.clone()
            } else {
                let text: Arc<str> = Arc::from(workspace.get_source(&file)?);
                texts.insert(file.clone(), text.clone());
                text
            };
            text.get(segment.span.start as usize..segment.span.end as usize)?;
            Some(DiagnosticOrigin {
                file,
                text,
                span: segment.span,
            })
        })
        .collect()
}
