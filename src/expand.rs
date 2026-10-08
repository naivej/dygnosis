//! Expand view plus origin map (`expand_report`).

use std::collections::{HashMap, HashSet};

use crate::equations::equations;
use crate::lexer::{tokenize, Token};
use crate::macro_expand::{
    expand_macros_traced_with_lines_and_evaluations, FrameRec, MacroMessage, MacroReplay,
    TokenTrace,
};
use crate::model_map::{
    EquationOccurrence, SourceFrame, SourceOccurrence, WrittenModelMap, WrittenSegment,
};
use crate::parser::{join_lexemes_recorded, normalize_newlines, parse_expanded};
use crate::span::Span;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpandReport {
    pub model_map: WrittenModelMap,
    /// False when macro syntax remains because this expander cannot safely evaluate it.
    pub complete: bool,
    /// Separate proof for preview navigation, including macro block termination.
    /// The legacy expansion/count completeness keeps its existing meaning.
    pub navigation_complete: bool,
    pub effective_text: String,
    /// Exact emitted model-row ranges and independently mapped written targets.
    pub navigation: Vec<PreviewRow>,
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
    /// `@#echo` and `@#echomacrovars` results in execution order. Spans are in
    /// the written file named by `file`, or in the expanded buffer when `file`
    /// is absent.
    pub macro_messages: Vec<MacroMessage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewRow {
    pub equation: EquationOccurrence,
    pub effective_span: Span,
    pub macro_frames: Vec<PreviewMacroFrame>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewMacroFrame {
    pub kind: String,
    pub variable: Option<String>,
    pub value: Option<String>,
    pub directive_segments: Vec<WrittenSegment>,
    pub body_segments: Vec<WrittenSegment>,
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
    /// 1-based line of `origin.start` in the written file.
    pub line: u32,
    /// Original macro header evaluated at this position before copied output.
    /// This metadata is neither emitted text nor a written macro statement.
    pub evaluation: Option<String>,
}

pub(crate) fn splice_evaluations(map: &[SpliceSegment]) -> Vec<MacroReplay> {
    map.iter()
        .filter_map(|segment| {
            segment.evaluation.as_ref().map(|directive| MacroReplay {
                span: Span::new(
                    segment.spliced.start as usize,
                    segment.spliced.start as usize,
                ),
                directive: directive.clone(),
            })
        })
        .collect()
}

pub fn expand_report(text: &str) -> ExpandReport {
    let source = normalize_newlines(text);
    let map = vec![SpliceSegment {
        spliced: Span::new(0, source.len()),
        file: None,
        origin: Span::new(0, source.len()),
        line: 1,
        evaluation: None,
    }];
    expand_report_from_spliced(&source, &map, None)
}

/// Normalize an already emitted fatal prefix without running macro processing.
pub(crate) fn compact_emitted_prefix(prefix: &str) -> String {
    let source = std::sync::Arc::new(crate::native_line::EmittedSource {
        text: prefix.to_string(),
        origins: vec![crate::native_line::EmittedOrigin {
            emitted: Span::new(0, prefix.len()),
            written: Span::new(0, prefix.len()),
            copied: true,
            frames: Vec::new(),
        }],
        comment_checkpoints: Vec::new(),
    });
    let mut tokens = tokenize(prefix);
    for token in &mut tokens {
        // These characters reached the model lexer after macro execution.
        // Retain that fact even when the emitted spelling looks like a macro.
        if matches!(
            token.kind,
            crate::lexer::TokenKind::MacroDir | crate::lexer::TokenKind::MacroInterp
        ) {
            token.lexeme = Some(token.text(prefix).to_string());
        }
        token.emitted = Some(crate::native_line::EmittedToken {
            source: source.clone(),
            span: token.span,
        });
    }
    join_lexemes_recorded(prefix, &tokens, |_, _| {})
}

pub(crate) enum NavigationSource {
    Unavailable,
    Mapped {
        text: String,
        segments: Vec<SpliceSegment>,
    },
}

pub(crate) fn expand_report_from_spliced(
    spliced: &str,
    map: &[SpliceSegment],
    written_navigation: Option<&NavigationSource>,
) -> ExpandReport {
    let source = normalize_newlines(spliced);
    let raw = tokenize(&source);
    let line_segments: Vec<(Span, u32)> = map
        .iter()
        .filter(|segment| !segment.spliced.is_empty())
        .map(|segment| (segment.spliced, segment.line))
        .collect();
    let evaluations = splice_evaluations(map);
    let (tokens, traces, arena, incomplete, macro_navigation_complete, messages) =
        expand_macros_traced_with_lines_and_evaluations(&source, raw, &line_segments, &evaluations);
    let macro_messages = locate_macro_messages(map, messages);
    debug_assert_eq!(tokens.len(), traces.len());
    let (model, ranges) = parse_expanded(&source, tokens.clone());
    let map_complete = !incomplete && crate::model_map::parser_complete(&model);
    let parsed_tokens = &model.expanded_tokens;
    let parsed_traces = project_token_traces(parsed_tokens, &ranges, &traces);
    let render_tokens = if map_complete {
        parsed_tokens.as_slice()
    } else {
        &tokens
    };
    let mut emitted = vec![None; render_tokens.len()];
    let effective_text = join_lexemes_recorded(&source, render_tokens, |index, span| {
        emitted[index] = Some(span);
    });
    let model_map = build_model_map(
        &model,
        &ranges,
        parsed_tokens,
        &parsed_traces,
        &arena,
        map,
        map_complete,
    );
    // Compare with an independently proven active-include projection. Matching
    // lexemes alone is insufficient: kinds and order must agree too. It uses
    // the same expander and parser; each projection is parsed once.
    let verified = if let Some(NavigationSource::Mapped { text, segments }) = written_navigation {
        let text = normalize_newlines(text);
        let navigation_lines: Vec<(Span, u32)> = segments
            .iter()
            .filter(|segment| !segment.spliced.is_empty())
            .map(|segment| (segment.spliced, segment.line))
            .collect();
        let navigation_evaluations = splice_evaluations(segments);
        let (actual, actual_traces, actual_arena, incomplete, macro_navigation_complete, _) =
            expand_macros_traced_with_lines_and_evaluations(
                &text,
                tokenize(&text),
                &navigation_lines,
                &navigation_evaluations,
            );
        let matches = !incomplete
            && macro_navigation_complete
            && actual.len() == tokens.len()
            && actual.iter().zip(&tokens).all(|(actual, legacy)| {
                actual.kind == legacy.kind && actual.text(&text) == legacy.text(&source)
            });
        if matches {
            // Raw terminators and Dynare strings can split or join ordinary
            // lexer tokens. Compare completed parser streams, then use the
            // independently parsed projection's exact written positions.
            let (actual_model, actual_ranges) = parse_expanded(&text, actual);
            let parsed_matches = actual_model.expanded_tokens.len() == parsed_tokens.len()
                && actual_model.expanded_tokens.iter().zip(parsed_tokens).all(
                    |(actual, legacy)| {
                        actual.kind == legacy.kind && actual.text(&text) == legacy.text(&source)
                    },
                );
            let actual_traces = project_token_traces(
                &actual_model.expanded_tokens,
                &actual_ranges,
                &actual_traces,
            );
            parsed_matches.then_some((
                actual_model.expanded_tokens,
                actual_traces,
                actual_arena,
                segments.as_slice(),
            ))
        } else {
            None
        }
    } else {
        None
    };
    let navigation_complete = map_complete
        && if written_navigation.is_some() {
            verified.is_some()
        } else {
            macro_navigation_complete
        };
    let (navigation_tokens, navigation_traces, navigation_arena, navigation_map) = verified
        .as_ref()
        .map(|(tokens, traces, arena, map)| {
            (tokens.as_slice(), traces.as_slice(), arena.as_slice(), *map)
        })
        .unwrap_or((parsed_tokens.as_slice(), &parsed_traces, &arena, map));
    let navigation = if navigation_complete {
        model
            .written_equations
            .iter()
            .zip(&model_map.equations)
            .filter_map(|(written, equation)| {
                let range = written.token_range.clone();
                let anchor = written.token_range.start..written.token_range.start + 1;
                let spans = &emitted[range.clone()];
                let start = spans.iter().flatten().next()?.start;
                let end = spans.iter().flatten().next_back()?.end;
                let mut frame_ids = Vec::new();
                let mut seen_frames = HashSet::new();
                for trace in &navigation_traces[range.clone()] {
                    for &id in &trace.frames {
                        if seen_frames.insert(id) {
                            frame_ids.push(id);
                        }
                    }
                }
                let mut equation = equation.clone();
                let anchors = map_token_segments(navigation_map, &navigation_tokens[anchor]);
                equation.source = SourceOccurrence {
                    segments: map_token_segments(navigation_map, &navigation_tokens[range]),
                    anchor: if anchors.len() == 1 {
                        anchors.into_iter().next()
                    } else {
                        None
                    },
                    origin_frames: Vec::new(),
                };
                Some(PreviewRow {
                    equation,
                    effective_span: Span { start, end },
                    macro_frames: frame_ids
                        .into_iter()
                        .map(|id| {
                            let frame = &navigation_arena[id];
                            PreviewMacroFrame {
                                kind: frame.kind.to_string(),
                                variable: frame.variable.clone(),
                                value: frame.value.clone(),
                                directive_segments: map_written_segments(
                                    navigation_map,
                                    frame.directive_span,
                                ),
                                body_segments: map_written_segments(
                                    navigation_map,
                                    frame.body_span,
                                ),
                            }
                        })
                        .collect(),
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    if incomplete {
        return ExpandReport {
            model_map,
            complete: false,
            navigation_complete,
            effective_text,
            navigation,
            n_equations: 0,
            origins: Vec::new(),
            aggregate_origins: Vec::new(),
            heterogeneous_origins: Vec::new(),
            aggregate_row_origins: Vec::new(),
            heterogeneous_row_origins: Vec::new(),
            macro_messages: macro_messages.clone(),
        };
    }
    debug_assert_eq!(model.equations.len(), ranges.aggregate.len());
    debug_assert_eq!(model.heterogeneous_models.len(), ranges.heterogeneous.len());
    let counted = equations(&model);
    let mut pending = Vec::new();
    let mut counted_i = 0usize;
    for (eq_index, eq) in model.equations.iter().enumerate() {
        let range = &ranges.aggregate[eq_index];
        let origin = origin_for_row(
            counted_i,
            &parsed_tokens[range.start..range.end],
            &parsed_traces[range.start..range.end],
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
                &parsed_tokens[range.start..range.end],
                &parsed_traces[range.start..range.end],
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
        model_map,
        complete: true,
        navigation_complete,
        effective_text,
        navigation,
        n_equations: origins.len(),
        origins,
        aggregate_origins,
        heterogeneous_origins,
        aggregate_row_origins,
        heterogeneous_row_origins,
        macro_messages,
    }
}

/// A joined token has only the macro context shared by all its source tokens.
/// This cannot turn a string crossing a loop boundary into one loop occurrence.
fn project_token_traces(
    tokens: &[Token],
    ranges: &crate::parser::EquationTokenRanges,
    traces: &[TokenTrace],
) -> Vec<TokenTrace> {
    ranges
        .original_tokens
        .iter()
        .zip(tokens)
        .map(|(range, token)| {
            if let Some(emitted) = &token.emitted {
                return TokenTrace {
                    frames: emitted.source.common_frames(emitted.span),
                };
            }
            let mut frames = traces[range.start].frames.clone();
            for trace in &traces[range.start + 1..range.end] {
                let shared = frames
                    .iter()
                    .zip(&trace.frames)
                    .take_while(|(a, b)| a == b)
                    .count();
                frames.truncate(shared);
            }
            TokenTrace { frames }
        })
        .collect()
}

fn locate_macro_messages(map: &[SpliceSegment], messages: Vec<MacroMessage>) -> Vec<MacroMessage> {
    messages
        .into_iter()
        .map(|message| {
            let Some(segment) = map
                .iter()
                .find(|segment| {
                    message.span.start >= segment.spliced.start
                        && message.span.end <= segment.spliced.end
                })
                .or_else(|| {
                    // The directive scanner consumes a newline the splicer added
                    // after a child that did not end with one. The written
                    // directive is still the child segment.
                    map.iter().find(|segment| {
                        message.span.start >= segment.spliced.start
                            && message.span.start < segment.spliced.end
                            && message.span.end > segment.spliced.end
                            && message.span.end - segment.spliced.end <= 2
                    })
                })
            else {
                return message;
            };
            let shift = segment.origin.start as i64 - segment.spliced.start as i64;
            let end = message.span.end.min(segment.spliced.end);
            MacroMessage {
                kind: message.kind,
                message: message.message,
                span: Span::new(
                    (message.span.start as i64 + shift) as usize,
                    (end as i64 + shift) as usize,
                ),
                file: segment.file.clone(),
            }
        })
        .collect()
}

fn build_model_map(
    model: &crate::model::Model,
    ranges: &crate::parser::EquationTokenRanges,
    tokens: &[Token],
    traces: &[TokenTrace],
    arena: &[FrameRec],
    map: &[SpliceSegment],
    complete: bool,
) -> WrittenModelMap {
    let source = |range: &std::ops::Range<usize>, anchor: &std::ops::Range<usize>| {
        let frames = traces[anchor.clone()]
            .iter()
            .max_by_key(|trace| trace.frames.len())
            .map(|trace| trace.frames.as_slice())
            .unwrap_or_default();
        let anchors = map_token_segments(map, &tokens[anchor.clone()]);
        SourceOccurrence {
            segments: map_token_segments(map, &tokens[range.clone()]),
            anchor: if anchors.len() == 1 {
                anchors.into_iter().next()
            } else {
                None
            },
            origin_frames: frames
                .iter()
                .map(|&id| {
                    let frame = &arena[id];
                    SourceFrame {
                        kind: frame.kind.to_string(),
                        variable: frame.variable.clone(),
                        value: frame.value.clone(),
                        segments: map_written_segments(map, frame.body_span),
                    }
                })
                .collect(),
        }
    };
    let mut numbered = HashMap::new();
    let mut active = std::collections::HashSet::new();
    let mut number = 0;
    for (equation, range) in model.equations.iter().zip(&ranges.aggregate) {
        active.insert(range.start);
        if !equation.is_local && !equation.static_tag {
            number += 1;
            numbered.insert(range.start, number);
        }
    }
    let mut dimension_numbers = HashMap::new();
    for (block, block_ranges) in model.heterogeneous_models.iter().zip(&ranges.heterogeneous) {
        let number = dimension_numbers.entry(block.dimension).or_insert(0);
        for (equation, range) in block.equations.iter().zip(block_ranges) {
            active.insert(range.start);
            if !equation.is_local && !equation.static_tag {
                *number += 1;
                numbered.insert(range.start, *number);
            }
        }
    }
    WrittenModelMap {
        complete,
        statements: model
            .statements
            .iter()
            .map(|statement| source(&statement.token_range, &statement.opener_range))
            .collect(),
        declarations: model
            .written_declarations
            .iter()
            .map(|decl| source(&decl.token_range, &decl.token_range))
            .collect(),
        equations: model
            .written_equations
            .iter()
            .map(|row| EquationOccurrence {
                id: row.token_range.start,
                statement_id: row.statement_id,
                name: row.equation.name.clone(),
                dimension: row.dimension.map(|name| model.name(name).to_string()),
                number: complete
                    .then(|| numbered.get(&row.token_range.start).copied())
                    .flatten(),
                active: active.contains(&row.token_range.start),
                local: row.equation.is_local,
                static_only: row.equation.static_tag,
                source: source(
                    &row.token_range,
                    &(row.token_range.start..row.token_range.start + 1),
                ),
            })
            .collect(),
        type_events: model
            .type_event_occurrences
            .iter()
            .map(|(index, range)| (*index, source(range, range)))
            .collect(),
        writes: model
            .write_targets
            .iter()
            .map(|write| source(&write.token_range, &write.token_range))
            .collect(),
    }
}

/// Group tokens by the include segment they actually came from. Clipping makes
/// cross-file rows safe; one file's range cannot run into another file's bytes.
fn map_token_segments(map: &[SpliceSegment], tokens: &[Token]) -> Vec<WrittenSegment> {
    let mut grouped: Vec<(usize, WrittenSegment)> = Vec::new();
    for token in tokens {
        for (index, segment) in map.iter().enumerate() {
            let start = token.span.start.max(segment.spliced.start);
            let end = token.span.end.min(segment.spliced.end);
            if start >= end {
                continue;
            }
            let span = Span {
                start: segment.origin.start + start - segment.spliced.start,
                end: segment.origin.start + end - segment.spliced.start,
            };
            if let Some((_, existing)) = grouped.iter_mut().find(|(id, _)| *id == index) {
                existing.span.start = existing.span.start.min(span.start);
                existing.span.end = existing.span.end.max(span.end);
            } else {
                grouped.push((
                    index,
                    WrittenSegment {
                        file: segment.file.clone(),
                        span,
                    },
                ));
            }
        }
    }
    grouped.sort_by_key(|(index, _)| *index);
    grouped.into_iter().map(|(_, segment)| segment).collect()
}

pub(crate) fn map_written_segments(map: &[SpliceSegment], span: Span) -> Vec<WrittenSegment> {
    map.iter()
        .filter_map(|segment| {
            let start = span.start.max(segment.spliced.start);
            let end = span.end.min(segment.spliced.end);
            (start < end).then(|| WrittenSegment {
                file: segment.file.clone(),
                span: Span {
                    start: segment.origin.start + start - segment.spliced.start,
                    end: segment.origin.start + end - segment.spliced.start,
                },
            })
        })
        .collect()
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

#[cfg(test)]
mod prefix_tests {
    use super::*;

    #[test]
    fn fatal_prefix_uses_the_same_compact_join() {
        let text = "var y; model; y=0; end;\n";
        assert_eq!(
            compact_emitted_prefix(text),
            expand_report(text).effective_text
        );
    }

    #[test]
    fn fatal_prefix_does_not_execute_generated_macro_markers() {
        for marker in ["@#echo 123", "@#define x=1", "@{missing}"] {
            let prefix = format!("{marker}\nvar y; model; y=0; end;\n");
            let compact = compact_emitted_prefix(&prefix);
            assert!(
                compact.starts_with(&format!("{marker}\nvar y")),
                "{compact}"
            );
        }
    }
}
