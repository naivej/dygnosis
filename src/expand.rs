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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginFrame {
    pub origin_span: Span,
    pub origin_uri: Option<String>,
    pub kind: String,
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
        if eq.is_local || eq.static_tag {
            continue;
        }
        let range = &ranges.aggregate[eq_index];
        let origin = origin_for_row(
            counted_i,
            &tokens[range.start..range.end],
            &traces[range.start..range.end],
            &arena,
            map,
        );
        pending.push((range.start, origin));
        counted_i += 1;
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
        for (eq, range) in block.equations.iter().zip(block_ranges.iter()) {
            if eq.is_local || eq.static_tag {
                continue;
            }
            let mut origin = origin_for_row(
                *scope_index,
                &tokens[range.start..range.end],
                &traces[range.start..range.end],
                &arena,
                map,
            );
            origin.dimension = Some(model.name(block.dimension).to_string());
            origin.block_index = Some(block_index);
            pending.push((range.start, origin));
            *scope_index += 1;
        }
    }
    pending.sort_by_key(|(token_start, _)| *token_start);
    let mut origins = Vec::with_capacity(pending.len());
    let mut aggregate_origins = Vec::with_capacity(counted.len());
    let mut heterogeneous_origins = vec![Vec::new(); model.heterogeneous_models.len()];
    for (_, mut origin) in pending {
        origin.index = origins.len();
        if let Some(block_index) = origin.block_index {
            heterogeneous_origins[block_index].push(origin.clone());
        } else {
            aggregate_origins.push(origin.clone());
        }
        origins.push(origin);
    }
    debug_assert_eq!(counted.len(), aggregate_origins.len());
    ExpandReport {
        effective_text,
        n_equations: origins.len(),
        origins,
        aggregate_origins,
        heterogeneous_origins,
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
    for (file, _) in &mapped {
        if let Some(f) = file {
            if !files.iter().any(|e| e == f) {
                files.push(f.clone());
            }
        }
    }

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

    let origin_frames = if best.len() > 1 {
        best.iter()
            .map(|&id| {
                let rec = &arena[id];
                let (uri, span) = map_span(map, rec.body_span);
                OriginFrame {
                    origin_span: span,
                    origin_uri: uri,
                    kind: rec.kind.to_string(),
                }
            })
            .collect()
    } else {
        Vec::new()
    };

    let origin_span = match best.last() {
        None => covering(&mapped, origin_uri.as_deref()),
        Some(&id) => {
            let innermost = &arena[id];
            let has_shorter = traces.iter().any(|t| t.frames.len() < best.len());
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
