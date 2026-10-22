//! Expand view plus origin map (`expand_report`).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, atomic::AtomicBool};

use crate::engine::analysis::equations::equations;
use crate::engine::language::lexer::{Token, tokenize};
#[cfg(test)]
use crate::engine::language::macro_expand::expand_macros_traced_with_lines_and_evaluations_budget;
use crate::engine::language::macro_expand::{FrameRec, MacroMessage, MacroReplay, TokenTrace};
#[cfg(test)]
use crate::engine::language::parser::parse_expanded_controlled;
use crate::engine::language::parser::{join_lexemes_recorded, normalize_newlines};
use crate::engine::language::span::Span;
use crate::engine::workspace::model_map::{
    EquationOccurrence, SourceFrame, SourceOccurrence, WrittenModelMap, WrittenSegment,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
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

pub(crate) fn map_execution_inputs(
    analysis: Arc<crate::engine::analysis_configuration::ResolvedAnalysis>,
    map: &[SpliceSegment],
    budget: &mut crate::engine::language::macro_expr::MacroBudget,
) -> Result<
    Arc<crate::engine::analysis_configuration::MacroExecutionInputs>,
    crate::engine::language::macro_expand::MacroEvalError,
> {
    budget.spend_work(map.len().max(1))?;
    let mut sites = HashMap::new();
    for segment in map {
        if segment.spliced.is_empty()
            || segment.spliced.end - segment.spliced.start
                != segment.origin.end - segment.origin.start
        {
            continue;
        }
        if let Some(original) = segment
            .file
            .as_ref()
            .and_then(|file| analysis.by_file.get(file))
        {
            budget.spend_work(original.len().max(1))?;
            for (span, index) in original.iter() {
                if segment.origin.start <= span.start && span.end <= segment.origin.end {
                    sites.insert(
                        Span {
                            start: segment.spliced.start + span.start - segment.origin.start,
                            end: segment.spliced.start + span.end - segment.origin.start,
                        },
                        *index,
                    );
                }
            }
        }
    }
    Ok(Arc::new(
        crate::engine::analysis_configuration::MacroExecutionInputs {
            analysis,
            sites: Arc::new(sites),
        },
    ))
}

pub(crate) fn splice_evaluations_configured(
    map: &[SpliceSegment],
    retain_overrides: bool,
) -> Vec<MacroReplay> {
    use crate::engine::language::macro_expand::{
        FOR_END_ORIGIN_PREFIX, FOR_INPUT_ORIGIN_PREFIX, FOR_INPUT_PREFIX, FOR_ORIGIN_PREFIX,
        directive_name,
    };
    let mut keys: HashMap<(Option<&str>, Span), Vec<usize>> = HashMap::new();
    let mut next = 0usize;
    map.iter()
        .filter_map(|segment| {
            segment.evaluation.as_ref().map(|directive| MacroReplay {
                span: Span::new(
                    segment.spliced.start as usize,
                    segment.spliced.start as usize,
                ),
                directive: if retain_overrides {
                    let identity = (segment.file.as_deref(), segment.origin);
                    if directive_name(directive).eq_ignore_ascii_case("for") {
                        let id = next;
                        next += 1;
                        keys.entry(identity).or_default().push(id);
                        format!("{FOR_ORIGIN_PREFIX}{id}:{directive}")
                    } else if let Some(value) = directive.strip_prefix(FOR_INPUT_PREFIX) {
                        keys.get(&identity).and_then(|ids| ids.last()).map_or_else(
                            || directive.clone(),
                            |id| format!("{FOR_INPUT_ORIGIN_PREFIX}{id}:{value}"),
                        )
                    } else if directive == FOR_END_ORIGIN_PREFIX {
                        keys.get_mut(&identity).and_then(Vec::pop).map_or_else(
                            || directive.clone(),
                            |id| format!("{FOR_END_ORIGIN_PREFIX}{id}"),
                        )
                    } else {
                        directive.clone()
                    }
                } else {
                    directive.clone()
                },
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
    let source = std::sync::Arc::new(crate::engine::language::native_line::EmittedSource {
        text: prefix.to_string(),
        origins: vec![crate::engine::language::native_line::EmittedOrigin {
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
            crate::engine::language::lexer::TokenKind::MacroDir
                | crate::engine::language::lexer::TokenKind::MacroInterp
        ) {
            token.lexeme = Some(token.text(prefix).to_string());
        }
        token.emitted = Some(crate::engine::language::native_line::EmittedToken {
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
    expand_report_from_spliced_controlled(
        spliced,
        map,
        written_navigation,
        None,
        crate::engine::macro_work::MacroWorkLimit::default(),
    )
    .expect("uncontrolled expansion")
}

pub(crate) fn expand_report_from_spliced_controlled(
    spliced: &str,
    map: &[SpliceSegment],
    written_navigation: Option<&NavigationSource>,
    cancellation: Option<Arc<AtomicBool>>,
    work_limit: crate::engine::macro_work::MacroWorkLimit,
) -> Result<ExpandReport, crate::engine::control::Cancelled> {
    expand_report_from_spliced_model_controlled(
        spliced,
        map,
        written_navigation,
        cancellation,
        work_limit,
        None,
    )
}

/// The workspace supplies facts from this same joined input. A fresh macro
/// pass must still prove the exact parser input before those facts are reused.
pub(crate) fn expand_report_from_spliced_model_controlled(
    spliced: &str,
    map: &[SpliceSegment],
    written_navigation: Option<&NavigationSource>,
    cancellation: Option<Arc<AtomicBool>>,
    work_limit: crate::engine::macro_work::MacroWorkLimit,
    parsed_model: Option<&crate::engine::language::model::Model>,
) -> Result<ExpandReport, crate::engine::control::Cancelled> {
    expand_report_from_spliced_configured(
        spliced,
        map,
        written_navigation,
        cancellation,
        work_limit,
        parsed_model,
        None,
        None,
    )
}

// This adapter passes the same explicit replay inputs to the traced renderer.
#[allow(clippy::too_many_arguments)]
pub(crate) fn expand_report_from_spliced_configured(
    spliced: &str,
    map: &[SpliceSegment],
    written_navigation: Option<&NavigationSource>,
    cancellation: Option<Arc<AtomicBool>>,
    work_limit: crate::engine::macro_work::MacroWorkLimit,
    parsed_model: Option<&crate::engine::language::model::Model>,
    inputs: Option<Arc<crate::engine::analysis_configuration::MacroExecutionInputs>>,
    cached: Option<Arc<crate::engine::language::macro_source::MacroSource>>,
) -> Result<ExpandReport, crate::engine::control::Cancelled> {
    crate::engine::control::run(cancellation.clone(), || {
        expand_spliced(
            spliced,
            map,
            written_navigation,
            cancellation,
            work_limit,
            parsed_model,
            inputs,
            cached,
        )
    })
}

#[allow(clippy::too_many_arguments)]
fn expand_spliced(
    spliced: &str,
    map: &[SpliceSegment],
    written_navigation: Option<&NavigationSource>,
    cancellation: Option<Arc<AtomicBool>>,
    work_limit: crate::engine::macro_work::MacroWorkLimit,
    parsed_model: Option<&crate::engine::language::model::Model>,
    inputs: Option<Arc<crate::engine::analysis_configuration::MacroExecutionInputs>>,
    cached: Option<Arc<crate::engine::language::macro_source::MacroSource>>,
) -> ExpandReport {
    let source = normalize_newlines(spliced);
    let raw = tokenize(&source);
    let line_segments: Vec<(Span, u32)> = map
        .iter()
        .filter(|segment| !segment.spliced.is_empty())
        .map(|segment| (segment.spliced, segment.line))
        .collect();
    let retain_overrides = inputs
        .as_ref()
        .is_some_and(|inputs| inputs.analysis.selection.preview);
    let evaluations = splice_evaluations_configured(map, retain_overrides);
    let (tokens, traces, arena, incomplete, macro_navigation_complete, messages, discarded) =
        crate::engine::language::macro_expand::expand_macros_traced_configured(
            &source,
            raw,
            &line_segments,
            &evaluations,
            &mut crate::engine::language::macro_expr::MacroBudget::with_limit(
                work_limit,
                cancellation.clone(),
            ),
            inputs.clone(),
            cached.as_deref(),
        );
    if crate::engine::control::cancelled() {
        return ExpandReport::default();
    }
    let macro_messages = locate_macro_messages(map, messages);
    debug_assert_eq!(tokens.len(), traces.len());
    let reusable = parsed_model.filter(|model| {
        !incomplete
            && macro_navigation_complete
            && same_completed_parser_input(model, &source, &tokens, work_limit)
    });
    let owned_parse = reusable.is_none().then(|| {
        crate::engine::language::parser::parse_expanded_context_controlled(
            &source,
            tokens.clone(),
            cancellation.clone(),
            work_limit,
            inputs.clone(),
            (!discarded.is_empty()).then(|| Arc::from(discarded)),
        )
    });
    let reused_ranges;
    let (model, ranges) = if let Some((model, ranges)) = &owned_parse {
        (model, ranges)
    } else {
        let model = reusable.expect("verified parser input");
        // Each reader stores this exact range on its equation. Surgery keeps
        // the equation and range vectors with the same removal predicate.
        reused_ranges = crate::engine::language::parser::EquationTokenRanges {
            aggregate: model
                .equations
                .iter()
                .map(|eq| eq.active_tokens.clone())
                .collect(),
            heterogeneous: model
                .heterogeneous_models
                .iter()
                .map(|block| {
                    block
                        .equations
                        .iter()
                        .map(|eq| eq.active_tokens.clone())
                        .collect()
                })
                .collect(),
            original_tokens: model.expanded_token_origins.clone(),
        };
        (model, &reused_ranges)
    };
    if crate::engine::control::cancelled() {
        return ExpandReport::default();
    }
    let map_complete = reusable.is_some()
        || (!incomplete && crate::engine::workspace::model_map::parser_complete(model));
    let parsed_tokens = &model.expanded_tokens;
    let parsed_traces =
        project_token_traces(parsed_tokens, ranges, &traces, &arena, retain_overrides);
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
        model,
        ranges,
        parsed_tokens,
        &parsed_traces,
        &arena,
        map,
        map_complete,
    );
    // Compare with an independently proven active-include projection. Matching
    // lexemes alone is insufficient: kinds and order must agree too. It uses
    // the same expander and parser; each projection is parsed once.
    // Identical text and every source segment imply the same traced tokens and
    // parser proof. A different projection retains its independent check.
    let identical_navigation = matches!(written_navigation, Some(NavigationSource::Mapped {text,segments}) if text == &source && segments.as_slice() == map);
    let verified = (|| {
        if identical_navigation {
            None
        } else if let Some(NavigationSource::Mapped { text, segments }) = written_navigation {
            let text = normalize_newlines(text);
            let navigation_lines: Vec<(Span, u32)> = segments
                .iter()
                .filter(|segment| !segment.spliced.is_empty())
                .map(|segment| (segment.spliced, segment.line))
                .collect();
            let navigation_evaluations = splice_evaluations_configured(segments, retain_overrides);
            let mut navigation_budget =
                crate::engine::language::macro_expr::MacroBudget::with_limit(
                    work_limit,
                    cancellation.clone(),
                );
            let navigation_inputs = inputs
                .as_ref()
                .map(|inputs| {
                    map_execution_inputs(
                        Arc::clone(&inputs.analysis),
                        segments,
                        &mut navigation_budget,
                    )
                })
                .transpose()
                .ok()?;
            let (
                actual,
                actual_traces,
                actual_arena,
                incomplete,
                macro_navigation_complete,
                _,
                discarded,
            ) = crate::engine::language::macro_expand::expand_macros_traced_configured(
                &text,
                tokenize(&text),
                &navigation_lines,
                &navigation_evaluations,
                &mut navigation_budget,
                navigation_inputs.clone(),
                None,
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
                let (actual_model, actual_ranges) =
                    crate::engine::language::parser::parse_expanded_context_controlled(
                        &text,
                        actual,
                        cancellation,
                        work_limit,
                        navigation_inputs,
                        (!discarded.is_empty()).then(|| Arc::from(discarded)),
                    );
                if crate::engine::control::cancelled() {
                    return None;
                }
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
                    &actual_arena,
                    retain_overrides,
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
        }
    })();
    let navigation_complete = map_complete
        && if identical_navigation {
            macro_navigation_complete
        } else if written_navigation.is_some() {
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
                            let origin = frame;
                            PreviewMacroFrame {
                                kind: frame.kind.to_string(),
                                variable: origin.variable.clone(),
                                value: origin.value.clone(),
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
    let counted = equations(model);
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
            retain_overrides,
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
                retain_overrides,
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

/// Reuse only an unchanged parser stream. Rewritten streams, macro errors and
/// interrupted parses use the existing parser path.
fn same_completed_parser_input(
    model: &crate::engine::language::model::Model,
    source: &str,
    tokens: &[Token],
    work_limit: crate::engine::macro_work::MacroWorkLimit,
) -> bool {
    if !model.parse_completed
        || model.macro_incomplete()
        || !model.macro_type_errors.is_empty()
        || !model.incomplete_reasons.is_empty()
        || model.macro_work_limit != work_limit
        || model.source != source
        || model.expanded_tokens.len() != tokens.len()
        || model.expanded_token_origins.len() != tokens.len()
    {
        return false;
    }
    // Expansion shares one immutable emitted source across its tokens. Compare
    // that complete source once; another source handle takes the fallback.
    let mut emitted_sources = None;
    for (index, ((old, new), origin)) in model
        .expanded_tokens
        .iter()
        .zip(tokens)
        .zip(&model.expanded_token_origins)
        .enumerate()
    {
        if index % 256 == 0 && crate::engine::control::cancelled() {
            return false;
        }
        if *origin != (index..index + 1)
            || old.kind != new.kind
            || old.span != new.span
            || old.lexeme != new.lexeme
            || old.expanded_adjacent_next != new.expanded_adjacent_next
            || old.glue_left != new.glue_left
            || old.comment_context != new.comment_context
            || old.output_prepaid != new.output_prepaid
        {
            return false;
        }
        match (&old.emitted, &new.emitted) {
            (None, None) => {}
            (Some(old), Some(new)) if old.span == new.span => {
                if let Some((old_source, new_source)) = emitted_sources {
                    if !Arc::ptr_eq(old_source, &old.source)
                        || !Arc::ptr_eq(new_source, &new.source)
                    {
                        return false;
                    }
                } else {
                    if !same_emitted_source(&old.source, &new.source) {
                        return false;
                    }
                    emitted_sources = Some((&old.source, &new.source));
                }
            }
            _ => return false,
        }
    }
    crate::engine::workspace::model_map::parser_complete(model)
}

fn same_emitted_source(
    old: &crate::engine::language::native_line::EmittedSource,
    new: &crate::engine::language::native_line::EmittedSource,
) -> bool {
    old.text == new.text
        && old.comment_checkpoints == new.comment_checkpoints
        && old.origins.len() == new.origins.len()
        && old
            .origins
            .iter()
            .zip(&new.origins)
            .enumerate()
            .all(|(index, (old, new))| {
                (index % 256 != 0 || !crate::engine::control::cancelled())
                    && old.emitted == new.emitted
                    && old.written == new.written
                    && old.copied == new.copied
                    && old.frames == new.frames
            })
}

/// A joined token has only the macro context shared by all its source tokens.
/// This cannot turn a string crossing a loop boundary into one loop occurrence.
fn project_token_traces(
    tokens: &[Token],
    ranges: &crate::engine::language::parser::EquationTokenRanges,
    traces: &[TokenTrace],
    arena: &[FrameRec],
    retain_overrides: bool,
) -> Vec<TokenTrace> {
    ranges
        .original_tokens
        .iter()
        .zip(tokens)
        .map(|(range, token)| {
            let mut frames = if let Some(emitted) = &token.emitted {
                emitted.source.common_frames(emitted.span)
            } else {
                let mut frames = traces[range.start].frames.clone();
                for trace in &traces[range.start + 1..range.end] {
                    let shared = frames
                        .iter()
                        .zip(&trace.frames)
                        .take_while(|(a, b)| a == b)
                        .count();
                    frames.truncate(shared);
                }
                frames
            };
            if retain_overrides {
                let mut append = |ids: &[usize]| {
                    for &id in ids {
                        if arena[id].kind.starts_with("override_") && !frames.contains(&id) {
                            frames.push(id);
                        }
                    }
                };
                if let Some(emitted) = &token.emitted {
                    let start = emitted
                        .source
                        .origins
                        .partition_point(|origin| origin.emitted.end <= emitted.span.start);
                    for origin in emitted
                        .source
                        .origins
                        .iter()
                        .skip(start)
                        .take_while(|origin| origin.emitted.start < emitted.span.end)
                    {
                        append(&origin.frames);
                    }
                } else {
                    for trace in &traces[range.clone()] {
                        append(&trace.frames);
                    }
                }
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
    model: &crate::engine::language::model::Model,
    ranges: &crate::engine::language::parser::EquationTokenRanges,
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
        let mut override_frames = Vec::new();
        if model.is_analysis_preview() {
            for trace in &traces[range.clone()] {
                for &id in &trace.frames {
                    if arena[id].kind.starts_with("override_")
                        && !frames.contains(&id)
                        && !override_frames.contains(&id)
                    {
                        override_frames.push(id);
                    }
                }
            }
        }
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
                .copied()
                .chain(override_frames)
                .map(|id| {
                    let frame = &arena[id];
                    let origin = frame;
                    let segments = if frame.kind.starts_with("override_") {
                        map_written_segments(map, frame.directive_span)
                    } else {
                        map_written_segments(map, frame.body_span)
                    };
                    SourceFrame {
                        kind: frame.kind.to_string(),
                        variable: origin.variable.clone(),
                        value: origin.value.clone(),
                        segments,
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
    let (statement_objects, pac_children) =
        crate::engine::workspace::model_map::outline_occurrences(model, source);
    WrittenModelMap {
        complete,
        statement_objects,
        pac_children,
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
    retain_overrides: bool,
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
    let mut override_frames = Vec::new();
    for tr in traces {
        let control = if retain_overrides {
            tr.frames
                .iter()
                .position(|&id| arena[id].kind == "override_definition")
                .unwrap_or(tr.frames.len())
        } else {
            tr.frames.len()
        };
        if control > best.len() {
            best = &tr.frames[..control];
        }
        if retain_overrides {
            for &id in &tr.frames {
                if arena[id].kind == "override_definition" && !override_frames.contains(&id) {
                    override_frames.push(id);
                }
            }
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

    let has_shorter = traces.iter().any(|t| if retain_overrides {t.frames.iter().take_while(|&&id|arena[id].kind!="override_definition").count()}else{t.frames.len()} < best.len());
    // Every token sits in a loop, including a one-iteration loop and a loop
    // that also contains an `@#if`. A loop around only part of the equation
    // does not.
    let loop_instance = !traces.is_empty()
        && traces
            .iter()
            .all(|trace| trace.frames.iter().any(|&id| arena[id].kind == "for"));
    let keep_frames = best.len() > 1
        || loop_instance
        || best.iter().any(|&id| arena[id].kind == "override_branch");
    let control_frames = if keep_frames { best } else { &[] };
    let origin_frames = control_frames
        .iter()
        .copied()
        .chain(override_frames)
        .map(|id| {
            let rec = &arena[id];
            let origin = rec;
            let (uri, span) = map_span(
                map,
                if rec.kind.starts_with("override_") {
                    rec.directive_span
                } else {
                    rec.body_span
                },
            );
            OriginFrame {
                origin_span: span,
                origin_uri: uri,
                kind: rec.kind.to_string(),
                variable: origin.variable.clone(),
                value: origin.value.clone(),
            }
        })
        .collect();

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
mod parser_reuse_tests {
    use super::*;
    use crate::engine::language::macro_expr::MacroBudget;
    use crate::engine::language::model::Model;
    use crate::engine::language::parser::{PARSE_EXPANDED_BUILDS, parse, parse_with_budget};
    use crate::engine::macro_work::MacroWorkLimit;
    use std::sync::atomic::Ordering;

    fn segments(source: &str) -> Vec<SpliceSegment> {
        vec![SpliceSegment {
            spliced: Span::new(0, source.len()),
            file: Some("root.mod".to_string()),
            origin: Span::new(0, source.len()),
            line: 1,
            evaluation: None,
        }]
    }

    fn assert_fallback(source: &str, model: &Model, limit: MacroWorkLimit) -> ExpandReport {
        let map = segments(source);
        let expected =
            expand_report_from_spliced_controlled(source, &map, None, None, limit).unwrap();
        PARSE_EXPANDED_BUILDS.with(|builds| builds.set(0));
        let actual = expand_report_from_spliced_model_controlled(
            source,
            &map,
            None,
            None,
            limit,
            Some(model),
        )
        .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            PARSE_EXPANDED_BUILDS.with(std::cell::Cell::get),
            1,
            "{source}"
        );
        actual
    }

    #[test]
    fn report_falls_back_for_refused_incomplete_and_rewritten_parser_inputs() {
        // Guards reusing refused/recovered facts as a completed report and
        // comparing post-parser tokens as if they were the original stream.
        for source in [
            "@#define bad=missing\nvar y; model; y=0; end;\n",
            "var y; model; y=; end;\n",
            "var y (long_name='line\nbreak'); model; y=0; end;\n",
        ] {
            let model = parse(source);
            assert_fallback(source, &model, MacroWorkLimit::default());
        }
    }

    #[test]
    fn report_keeps_its_own_macro_budget_when_the_effective_pass_stops() {
        // Guards the extra source reservation in the ordinary parse changing
        // the report's independent work stop or complete emitted result.
        let source = "@#for i in 1:3\nvar y@{i};\n@#endfor\nmodel;\n@#for i in 1:3\ny@{i}=0;\n@#endfor\nend;\n";
        let mut budget = MacroBudget::new();
        expand_macros_traced_with_lines_and_evaluations_budget(
            source,
            tokenize(source),
            &[],
            &[],
            &mut budget,
        );
        let macro_work = budget.work_limit().get() - budget.remaining_work();
        let limit = MacroWorkLimit::new((macro_work + source.len() - 1) as u64).unwrap();
        let model = parse_with_budget(source, &mut MacroBudget::with_limit(limit, None));
        assert!(model.macro_incomplete());
        let report = assert_fallback(source, &model, limit);
        assert!(report.complete);
        assert_eq!(report.n_equations, 3);
        let complete_model = parse(source);
        assert_fallback(source, &complete_model, limit);
    }

    #[test]
    fn equal_source_with_different_replay_context_takes_the_fallback() {
        // Guards source equality bypassing changed macro input, including
        // emitted comments that ordinary lexer tokens do not retain.
        for source in [
            "@#for value in [1]\nvar y; model; y=@{value}; end;\n@#endfor\n",
            "@#for value in [1]\nvar y; model; y=0; end; // @{value}\n@#endfor\n",
        ] {
            let replay = |value| {
                let input = crate::engine::language::macro_expr::encode_replay_value(
                    &crate::engine::language::macro_expand::MacroVal::Array(vec![
                        crate::engine::language::macro_expand::MacroVal::Real(value),
                    ]),
                    &mut MacroBudget::new(),
                )
                .unwrap();
                format!(
                    "{}{input}",
                    crate::engine::language::macro_expand::FOR_INPUT_PREFIX
                )
            };
            let evaluation = crate::engine::language::macro_expand::MacroReplay {
                span: Span::new(0, 0),
                directive: replay(1.0),
            };
            let model = crate::engine::language::parser::parse_with_lines_and_evaluations(
                source,
                &[],
                &[evaluation],
            );
            assert!(model.parse_completed && !model.macro_incomplete());
            let mut map = segments(source);
            map.insert(
                0,
                SpliceSegment {
                    spliced: Span::new(0, 0),
                    file: Some("root.mod".to_string()),
                    origin: Span::new(0, 0),
                    line: 1,
                    evaluation: Some(replay(2.0)),
                },
            );
            let expected = expand_report_from_spliced_controlled(
                source,
                &map,
                None,
                None,
                MacroWorkLimit::default(),
            )
            .unwrap();
            assert!(expected.complete && expected.model_map.complete);
            PARSE_EXPANDED_BUILDS.with(|builds| builds.set(0));
            let actual = expand_report_from_spliced_model_controlled(
                source,
                &map,
                None,
                None,
                MacroWorkLimit::default(),
                Some(&model),
            )
            .unwrap();
            assert_eq!(actual, expected);
            assert_eq!(PARSE_EXPANDED_BUILDS.with(std::cell::Cell::get), 1);
        }
    }

    #[test]
    fn report_rejects_an_interrupted_parse_and_still_obeys_cancellation() {
        // Guards a canceled core parse with no macro stop reason poisoning
        // later report reuse, and reuse bypassing the current stop signal.
        let source = "var y; model; y=y(-1); end;\n";
        let flag = Arc::new(AtomicBool::new(false));
        crate::engine::control::cancel_after(&flag, 0);
        let (interrupted, _) = parse_expanded_controlled(
            source,
            tokenize(source),
            Some(flag.clone()),
            MacroWorkLimit::default(),
        );
        assert!(!interrupted.parse_completed);
        assert!(!interrupted.macro_incomplete());
        flag.store(false, Ordering::Relaxed);
        assert_fallback(source, &interrupted, MacroWorkLimit::default());
        let model = parse(source);
        let map = segments(source);
        flag.store(true, Ordering::Relaxed);
        assert!(
            expand_report_from_spliced_model_controlled(
                source,
                &map,
                None,
                Some(flag.clone()),
                MacroWorkLimit::default(),
                Some(&model)
            )
            .is_err()
        );
        flag.store(false, Ordering::Relaxed);
        let mut budget = MacroBudget::new();
        let (tokens, ..) = expand_macros_traced_with_lines_and_evaluations_budget(
            source,
            tokenize(source),
            &[],
            &[],
            &mut budget,
        );
        let result = crate::engine::control::run(Some(flag.clone()), || {
            crate::engine::control::cancel_after(&flag, 0);
            same_completed_parser_input(&model, source, &tokens, MacroWorkLimit::default())
        });
        assert!(result.is_err());
    }

    #[test]
    fn report_keeps_the_independent_navigation_parse() {
        // Guards a verified core stream skipping the separate navigation
        // projection when its exact text and written segments differ.
        let source = "var y; model; y=y(-1); end;\n";
        let navigation = "var y;\nmodel;\ny = y(-1);\nend;\n";
        let model = parse(source);
        let map = segments(source);
        let written = NavigationSource::Mapped {
            text: navigation.to_string(),
            segments: segments(navigation),
        };
        let expected = expand_report_from_spliced_controlled(
            source,
            &map,
            Some(&written),
            None,
            MacroWorkLimit::default(),
        )
        .unwrap();
        PARSE_EXPANDED_BUILDS.with(|builds| builds.set(0));
        let actual = expand_report_from_spliced_model_controlled(
            source,
            &map,
            Some(&written),
            None,
            MacroWorkLimit::default(),
            Some(&model),
        )
        .unwrap();
        assert_eq!(actual, expected);
        assert!(actual.navigation_complete);
        assert_eq!(PARSE_EXPANDED_BUILDS.with(std::cell::Cell::get), 1);
    }
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
