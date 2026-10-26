//! Shared preview facts. Each transport verifies targets and converts ranges.

use serde_json::{Value, json};

use crate::engine::language::expand::{ExpandReport, PreviewRow};
use crate::engine::language::span::Span;
use crate::engine::workspace::model_map::WrittenSegment;

pub(crate) const NAVIGATION_SCHEMA_VERSION: u32 = 1;

#[cfg(test)]
thread_local! {
    pub(crate) static NAVIGATION_ROW_PROJECTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) fn navigation_json(
    report: &ExpandReport,
    mut effective_range: impl FnMut(Span) -> Value,
    mut location: impl FnMut(&WrittenSegment) -> Option<Value>,
) -> Value {
    Value::Array(
        report
            .navigation
            .iter()
            .map(|row| navigation_row_json(row, &mut effective_range, &mut location))
            .collect(),
    )
}

pub(crate) fn navigation_row_json(
    row: &PreviewRow,
    mut effective_range: impl FnMut(Span) -> Value,
    mut location: impl FnMut(&WrittenSegment) -> Option<Value>,
) -> Value {
    #[cfg(test)]
    NAVIGATION_ROW_PROJECTIONS.with(|count| count.set(count.get() + 1));
    let equation = &row.equation;
    let written: Vec<_> = equation
        .source
        .segments
        .iter()
        .filter_map(&mut location)
        .collect();
    let frames: Vec<_> = row.macro_frames.iter().map(|frame| json!({
                    "kind": frame.kind,
                    "variable": frame.variable,
                    "value": frame.value,
                    "directive_locations": frame.directive_segments.iter().filter_map(&mut location).collect::<Vec<_>>(),
                    "body_locations": frame.body_segments.iter().filter_map(&mut location).collect::<Vec<_>>()
                })).collect();
    json!({
        "id": format!("e{}", equation.id),
        "statement_id": format!("s{}", equation.statement_id),
        "effective_range": effective_range(row.effective_span),
        "written_locations": written,
        "macro_frames": frames,
        "kind": if equation.local { "local" } else if equation.static_only { "static" } else { "equation" },
        "active": equation.active,
        "number": equation.number,
        "scope": if equation.dimension.is_some() { "heterogeneous" } else { "aggregate" },
        "dimension": equation.dimension
    })
}
