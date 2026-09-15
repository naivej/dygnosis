//! Expand view plus origin map (`expand_report`).

use crate::equations::equations;
use crate::lexer::{tokenize, Token};
use crate::macro_expand::{expand_macros_traced, FrameRec, TokenTrace};
use crate::parser::{join_lexemes, normalize_newlines, parse_expanded};
use crate::span::Span;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpandReport {
    pub effective_text: String,
    pub n_equations: usize,
    pub origins: Vec<EquationOrigin>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EquationOrigin {
    pub index: usize,
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
    debug_assert_eq!(model.equations.len(), ranges.len());
    let effective_text = join_lexemes(&source, &tokens);
    let counted = equations(&model);
    let mut origins = Vec::with_capacity(counted.len());
    let mut counted_i = 0usize;
    for (eq_index, eq) in model.equations.iter().enumerate() {
        if eq.is_local || eq.static_tag {
            continue;
        }
        let range = &ranges[eq_index];
        let origin = origin_for_row(
            counted_i,
            &tokens[range.start..range.end],
            &traces[range.start..range.end],
            &arena,
            map,
        );
        origins.push(origin);
        counted_i += 1;
    }
    debug_assert_eq!(counted.len(), origins.len());
    ExpandReport {
        effective_text,
        n_equations: counted.len(),
        origins,
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
