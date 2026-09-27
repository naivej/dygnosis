//! Expand view plus origin map (`expand_report`).

use std::collections::HashMap;

use crate::equations::equations;
use crate::lexer::{tokenize, Token};
use crate::macro_expand::{expand_macros_traced, FrameRec, TokenTrace};
use crate::parser::{join_lexemes, normalize_newlines, parse_expanded};
use crate::span::Span;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpandReport {
    pub effective_text: String,
    /// Counted equations across the aggregate and all heterogeneous trees.
    pub n_equations: usize,
    /// Equation origins in expanded file order.
    pub origins: Vec<EquationOrigin>,
    /// Aggregate equation origins, indexed by the aggregate equation view.
    pub aggregate_origins: Vec<EquationOrigin>,
    /// Counted equations in each heterogeneous model block, in block order.
    pub heterogeneous_origins: Vec<Vec<EquationOrigin>>,
    /// Every parsed row, including locals and uncounted static rows, for extraction.
    /// Entries follow the parsed aggregate/block vectors.
    pub(crate) aggregate_row_origins: Vec<Option<RowOrigin>>,
    pub(crate) heterogeneous_row_origins: Vec<Vec<Option<RowOrigin>>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RowOrigin {
    pub order: usize,
    pub origin: EquationOrigin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EquationOrigin {
    /// Zero-based position among all counted equations in the expanded file.
    pub index: usize,
    /// Zero-based position in the aggregate tree or named heterogeneity dimension.
    pub scope_index: usize,
    /// `None` for an aggregate equation.
    pub dimension: Option<String>,
    /// Written heterogeneous block index, when `dimension` is set.
    pub block_index: Option<usize>,
    pub origin_span: Span,
    pub origin_uri: Option<String>,
    pub origin_frames: Vec<OriginFrame>,
    /// The equation's own tokens in the origin file. Not the surrounding
    /// `@#if` or `@#for` body.
    pub written_span: Span,
    /// Tokens of this equation come from more than one file.
    pub ambiguous: bool,
    /// This row is one expansion of an `@#for` body.
    pub loop_copy: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginFrame {
    pub origin_span: Span,
    pub origin_uri: Option<String>,
    pub kind: String,
    /// `@#for` index name. Empty for an `@#if` frame.
    pub variable: Option<String>,
    /// `@#for` index value for this expanded copy.
    pub value: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct SpliceSegment {
    pub spliced: Span,
    pub file: Option<String>,
    pub origin: Span,
}

pub fn expand_report(text: &str) -> ExpandReport {
    let source = normalize_newlines(text);
    let map = vec![SpliceSegment {
        spliced: Span::new(0, source.len()),
        file: None,
        origin: Span::new(0, source.len()),
    }];
    expand_report_from_spliced(&source, &map)
}

pub(crate) fn expand_report_from_spliced(spliced: &str, map: &[SpliceSegment]) -> ExpandReport {
    let source = normalize_newlines(spliced);
    let raw = tokenize(&source);
    let (tokens, traces, arena) = expand_macros_traced(&source, raw);
    debug_assert_eq!(tokens.len(), traces.len());
    let (model, ranges) = parse_expanded(&source, tokens.clone());
    debug_assert_eq!(model.equations.len(), ranges.aggregate.len());
    debug_assert_eq!(model.heterogeneous_models.len(), ranges.heterogeneous.len());
    let effective_text = join_lexemes(&source, &tokens);
    let counted = equations(&model);
    let mut pending = Vec::new();
    let mut counted_i = 0usize;
    for (eq_index, eq) in model.equations.iter().enumerate() {
        let range = &ranges.aggregate[eq_index];
        let origin = origin_for_row(
            counted_i,
            &tokens[range.start..range.end],
            &traces[range.start..range.end],
            &arena,
            map,
        );
        pending.push((
            range.start,
            None,
            eq_index,
            !eq.static_tag && !eq.is_local,
            origin,
        ));
        if !eq.static_tag && !eq.is_local {
            counted_i += 1;
        }
    }
    debug_assert_eq!(counted.len(), counted_i);
    let mut dimension_indices = HashMap::new();
    for (block_index, (block, block_ranges)) in model
        .heterogeneous_models
        .iter()
        .zip(ranges.heterogeneous.iter())
        .enumerate()
    {
        debug_assert_eq!(block.equations.len(), block_ranges.len());
        let scope_index = dimension_indices.entry(block.dimension).or_insert(0usize);
        for (eq_index, (eq, range)) in block.equations.iter().zip(block_ranges.iter()).enumerate() {
            let mut origin = origin_for_row(
                *scope_index,
                &tokens[range.start..range.end],
                &traces[range.start..range.end],
                &arena,
                map,
            );
            origin.dimension = Some(model.name(block.dimension).to_string());
            origin.block_index = Some(block_index);
            pending.push((
                range.start,
                Some(block_index),
                eq_index,
                !eq.static_tag && !eq.is_local,
                origin,
            ));
            if !eq.static_tag && !eq.is_local {
                *scope_index += 1;
            }
        }
    }
    pending.sort_by_key(|(token_start, _, _, _, _)| *token_start);
    let mut origins = Vec::with_capacity(pending.len());
    let mut aggregate_origins = Vec::with_capacity(counted.len());
    let mut heterogeneous_origins = vec![Vec::new(); model.heterogeneous_models.len()];
    let mut aggregate_row_origins = vec![None; model.equations.len()];
    let mut heterogeneous_row_origins = model
        .heterogeneous_models
        .iter()
        .map(|block| vec![None; block.equations.len()])
        .collect::<Vec<_>>();
    for (order, (_, block_index, eq_index, is_counted, mut origin)) in
        pending.into_iter().enumerate()
    {
        if is_counted {
            origin.index = origins.len();
            if let Some(block_index) = block_index {
                heterogeneous_origins[block_index].push(origin.clone());
            } else {
                aggregate_origins.push(origin.clone());
            }
            origins.push(origin.clone());
        }
        let row = Some(RowOrigin { order, origin });
        if let Some(block_index) = block_index {
            heterogeneous_row_origins[block_index][eq_index] = row;
        } else {
            aggregate_row_origins[eq_index] = row;
        }
    }
    debug_assert_eq!(counted.len(), aggregate_origins.len());
    ExpandReport {
        effective_text,
        n_equations: origins.len(),
        origins,
        aggregate_origins,
        heterogeneous_origins,
        aggregate_row_origins,
        heterogeneous_row_origins,
    }
}

fn origin_for_row(
    index: usize,
    tokens: &[Token],
    traces: &[TokenTrace],
    arena: &[FrameRec],
    map: &[SpliceSegment],
) -> EquationOrigin {
    debug_assert_eq!(tokens.len(), traces.len());
    let mapped: Vec<(Option<String>, Span)> =
        tokens.iter().map(|t| map_span(map, t.span)).collect();

    let mut files: Vec<String> = Vec::new();
    let mut saw_missing = false;
    for (file, _) in &mapped {
        if let Some(f) = file {
            if !files.iter().any(|e| e == f) {
                files.push(f.clone());
            }
        } else {
            saw_missing = true;
        }
    }
    let ambiguous = files.len() > 1 || (saw_missing && !files.is_empty());

    let mut best: &[usize] = &[];
    for tr in traces {
        if tr.frames.len() > best.len() {
            best = &tr.frames;
        }
    }

    let origin_uri = if files.len() == 1 {
        Some(files[0].clone())
    } else if files.len() > 1 {
        if let Some(&id) = best.last() {
            map_span(map, arena[id].body_span).0
        } else {
            mapped.first().and_then(|(f, _)| f.clone())
        }
    } else {
        None
    };

    let has_shorter = traces.iter().any(|t| t.frames.len() < best.len());
    // Every token sits in a loop, including a one-iteration loop and a loop
    // that also contains an `@#if`. A loop around only part of the equation
    // does not.
    let loop_instance = !traces.is_empty()
        && traces
            .iter()
            .all(|trace| trace.frames.iter().any(|&id| arena[id].kind == "for"));
    let keep_frames = best.len() > 1 || loop_instance;
    let origin_frames = if keep_frames {
        best.iter()
            .map(|&id| {
                let rec = &arena[id];
                let (uri, span) = map_span(map, rec.body_span);
                OriginFrame {
                    origin_span: span,
                    origin_uri: uri,
                    kind: rec.kind.to_string(),
                    variable: rec.variable.clone(),
                    value: rec.value.clone(),
                }
            })
            .collect()
    } else {
        Vec::new()
    };

    let written_span = covering(&mapped, origin_uri.as_deref());
    let origin_span = match best.last() {
        None => covering(&mapped, origin_uri.as_deref()),
        Some(&id) => {
            let innermost = &arena[id];
            if innermost.kind == "for" && has_shorter {
                covering(&mapped, origin_uri.as_deref())
            } else {
                map_span(map, innermost.body_span).1
            }
        }
    };

    EquationOrigin {
        index,
        scope_index: index,
        dimension: None,
        block_index: None,
        origin_span,
        origin_uri,
        origin_frames,
        written_span,
        ambiguous,
        loop_copy: loop_instance,
    }
}

fn covering(mapped: &[(Option<String>, Span)], origin_uri: Option<&str>) -> Span {
    let mut start = u32::MAX;
    let mut end = 0u32;
    let mut any = false;
    for (file, span) in mapped {
        if file.as_deref() != origin_uri {
            continue;
        }
        any = true;
        start = start.min(span.start);
        end = end.max(span.end);
    }
    if any {
        Span { start, end }
    } else {
        Span::default()
    }
}

fn map_span(map: &[SpliceSegment], span: Span) -> (Option<String>, Span) {
    let Some(seg) = lookup(map, span.start) else {
        return (None, span);
    };
    let delta = span.start.saturating_sub(seg.spliced.start);
    let len = span.end.saturating_sub(span.start);
    let origin = Span {
        start: seg.origin.start + delta,
        end: seg.origin.start + delta + len,
    };
    (seg.file.clone(), origin)
}

fn lookup(map: &[SpliceSegment], pos: u32) -> Option<&SpliceSegment> {
    map.iter()
        .find(|s| pos >= s.spliced.start && pos < s.spliced.end)
        .or_else(|| map.last().filter(|s| pos == s.spliced.end))
}
