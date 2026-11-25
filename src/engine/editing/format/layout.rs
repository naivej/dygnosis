use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use crate::engine::language::lexer::{Token, TokenKind};
use crate::engine::language::model::{ExecutionStep, Model};
use crate::engine::language::span::Span;
use crate::engine::workspace::EffectiveOrigins;

pub(super) struct Owner<'a> {
    pub model: &'a Model,
    pub origins: Option<&'a EffectiveOrigins>,
}

impl Owner<'_> {
    pub(super) fn written(&self, file: &str, span: Span) -> Option<Span> {
        self.origins
            .map_or(Some(span), |map| map.copied_in(file, span))
    }

    pub(super) fn literal(&self, file: &str, token: &Token) -> Option<Span> {
        if token.lexeme.is_some() {
            return None;
        }
        if let Some(emitted) = &token.emitted {
            let origins = &emitted.source.origins;
            let origin = origins
                .get(origins.partition_point(|origin| origin.emitted.end <= emitted.span.start))?;
            if !origin.copied || emitted.span.end > origin.emitted.end {
                return None;
            }
        }
        self.written(file, token.span)
    }

    pub(super) fn for_written_segments(&self, file: &str, span: Span, mut visit: impl FnMut(Span)) {
        if let Some(origins) = self.origins {
            origins.visit_written_in(file, span, visit);
        } else if !span.is_empty() {
            visit(span);
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Edge {
    #[default]
    Row,
    Open,
    Close,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Cut {
    pub gap: RangeBytes,
    pub opens: bool,
    pub closes: bool,
    pub left_closes: bool,
    pub right_opens: bool,
    depth: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct RangeBytes {
    pub start: usize,
    pub end: usize,
}

pub(super) struct Splits {
    pub cuts: BTreeMap<usize, Vec<Cut>>,
    pub withheld: usize,
    pub blocked_lines: BTreeSet<usize>,
}

fn offset_line(starts: &[usize], offset: usize) -> usize {
    starts.partition_point(|start| *start <= offset) - 1
}

pub(super) fn protected(owner: &Owner<'_>, file: &str, mut visit: impl FnMut(Span)) {
    let model = owner.model;
    for step in &model.execution_steps {
        let span = match *step {
            ExecutionStep::Opaque(span) => span,
            ExecutionStep::Statement(id) => {
                let statement = &model.statements[id];
                if !statement.native && statement.name != "verbatim" {
                    continue;
                }
                statement.span
            }
        };
        owner.for_written_segments(file, span, &mut visit);
    }
    for span in model
        .ms_unparsed_spans
        .iter()
        .chain(model.inactive_macro_spans())
    {
        owner.for_written_segments(file, *span, &mut visit);
    }
    for span in model
        .macro_directives
        .iter()
        .map(|directive| directive.span)
        .chain(model.includes.iter().map(|directive| directive.span))
    {
        owner.for_written_segments(file, span, &mut visit);
    }
    protected_literals(owner, file, visit);
}

pub(super) fn protected_literals(owner: &Owner<'_>, file: &str, mut visit: impl FnMut(Span)) {
    for token in &owner.model.expanded_tokens {
        if matches!(token.kind, TokenKind::String | TokenKind::Latex)
            && token
                .lexeme
                .as_deref()
                .or_else(|| {
                    owner
                        .model
                        .source
                        .get(token.span.start as usize..token.span.end as usize)
                })
                .is_some_and(|value| value.contains(['\r', '\n']))
        {
            owner.for_written_segments(file, token.span, &mut visit);
        }
    }
    copy_conflicts(owner, file, visit);
}

/// Copied bytes can be swallowed by a string or comment in one macro
/// execution. Looking only for a matching token pair misses that execution.
fn copy_conflicts(owner: &Owner<'_>, file: &str, mut visit: impl FnMut(Span)) {
    let tokens = &owner.model.expanded_tokens;
    let mut events = Vec::new();
    let mut add = |span: Span| {
        owner.for_written_segments(file, span, |span| {
            events.push((span.start, 1i64));
            events.push((span.end, -1i64));
        });
    };
    if let Some(emitted) = tokens.first().and_then(|token| token.emitted.as_ref()) {
        for origin in emitted.source.origins.iter().filter(|origin| origin.copied) {
            if crate::engine::control::cancelled() {
                return;
            }
            add(origin.written);
        }
    } else if owner.origins.is_some() {
        add(Span::new(0, owner.model.source.len()));
    } else {
        return;
    }
    events.sort_unstable_by_key(|&(offset, _)| offset);
    let mut active = 0;
    let mut coverage = Vec::<(u32, i64)>::new();
    for (offset, change) in events {
        active += change;
        if let Some(last) = coverage.last_mut().filter(|last| last.0 == offset) {
            last.1 = active;
        } else {
            coverage.push((offset, active));
        }
    }
    if coverage.iter().all(|&(_, count)| count <= 1) {
        return;
    }
    let count_at = |offset| {
        coverage
            .partition_point(|&(at, _)| at <= offset)
            .checked_sub(1)
            .map_or(0, |i| coverage[i].1)
    };
    let mut observed = BTreeMap::new();
    for token in tokens {
        if crate::engine::control::cancelled() {
            return;
        }
        if let Some(span) = owner.literal(file, token)
            && !span.is_empty()
            && count_at(span.start).max(count_at(span.end - 1)) > 1
        {
            *observed
                .entry((span.start, span.end, token.kind as u8))
                .or_insert(0) += 1;
        }
    }
    for ((start, end, _), count) in observed {
        if count != count_at(start).max(count_at(end - 1)) {
            visit(Span { start, end });
        }
    }
}

/// Join adjacent parser units in emitted order, then intersect the edits across
/// owners. Every observed copy of the written separator must give the same cut.
pub(super) fn splits(
    text: &str,
    file: &str,
    line_starts: &[usize],
    selection: Range<usize>,
    owners: &[Owner<'_>],
) -> Splits {
    let mut accepted = BTreeMap::new();
    let mut candidates = BTreeMap::new();
    let mut blocked_lines = BTreeSet::new();
    for (index, owner) in owners.iter().enumerate() {
        let view = owner_splits(text, file, line_starts, &selection, owner);
        blocked_lines.extend(
            view.iter()
                .filter(|(_, cut)| cut.is_none())
                .map(|(gap, _)| offset_line(line_starts, gap.start)),
        );
        candidates.extend(
            view.keys()
                .map(|key| (*key, offset_line(line_starts, key.start))),
        );
        if index == 0 {
            accepted = view;
        } else {
            accepted.retain(|key, hint| view.get(key) == Some(hint));
        }
    }
    let mut cuts = BTreeMap::<usize, Vec<Cut>>::new();
    let mut withheld = 0;
    for (key, line) in candidates {
        if let Some(Some(cut)) = accepted.get(&key)
            && !blocked_lines.contains(&line)
        {
            cuts.entry(line).or_default().push(*cut);
        } else {
            withheld += 1;
        }
    }
    Splits {
        cuts,
        withheld,
        blocked_lines,
    }
}

fn owner_splits(
    text: &str,
    file: &str,
    line_starts: &[usize],
    selection: &Range<usize>,
    owner: &Owner<'_>,
) -> BTreeMap<RangeBytes, Option<Cut>> {
    let model = owner.model;
    let tokens = &model.expanded_tokens;
    let mut starts = BTreeMap::new();
    let mut ends = BTreeMap::new();
    let mut add = |range: Range<usize>, kind| {
        if !range.is_empty() {
            starts.insert(range.start, (range.end, kind));
            ends.insert(range.end - 1, (range.start, kind));
        }
    };
    if !model.macro_incomplete() {
        for (range, edge) in super::accepted_units(model) {
            add(range, edge);
        }
    }
    let mut result = BTreeMap::new();
    let mut interiors = BTreeSet::new();
    for (i, pair) in tokens.windows(2).enumerate() {
        if crate::engine::control::cancelled() {
            return BTreeMap::new();
        }
        let [separator, next] = pair else {
            unreachable!()
        };
        if !matches!(separator.kind, TokenKind::Semi | TokenKind::Comma)
            || next.kind == TokenKind::Eof
        {
            continue;
        }
        let Some(left) = owner.literal(file, separator) else {
            continue;
        };
        let Some(right) = owner.written(file, next.span) else {
            continue;
        };
        let gap = RangeBytes {
            start: left.end as usize,
            end: right.start as usize,
        };
        if gap.start < selection.start || gap.end > selection.end {
            continue;
        }
        let Some(between) = text.get(gap.start..gap.end) else {
            continue;
        };
        if between.contains(['\r', '\n']) {
            continue;
        }
        let previous = ends.get(&i);
        let following = starts.get(&(i + 1));
        // Protect a separator that is inside a statement in one copy and
        // between statements in another. Ordinary inner separators stay quiet.
        if previous.is_none()
            && following.is_none()
            && (separator.kind == TokenKind::Comma
                || starts
                    .range(..=i)
                    .next_back()
                    .is_some_and(|(_, (end, _))| *end > i + 1))
        {
            interiors.insert(gap);
            continue;
        }
        let cut = previous
            .zip(following)
            .and_then(|(&(from, left_edge), &(to, right_edge))| {
                let cover = literal_unit_pair(owner, file, &tokens[from..to], text)?;
                let line = offset_line(line_starts, gap.start);
                let start = line_starts[line];
                let end = line_starts.get(line + 1).map_or(text.len(), |&next| {
                    text[..next].strip_suffix("\r\n").map_or(next - 1, str::len)
                });
                if !(between.bytes().all(|b| matches!(b, b' ' | b'\t'))
                    && selection.start <= cover.start as usize
                    && cover.end as usize <= selection.end
                    && selection.start <= start
                    && end <= selection.end)
                {
                    return None;
                }
                Some(Cut {
                    gap,
                    opens: left_edge == Edge::Open,
                    closes: right_edge == Edge::Close,
                    left_closes: left_edge == Edge::Close && cover.start as usize >= start,
                    right_opens: right_edge == Edge::Open && cover.end as usize <= end,
                    depth: body_depth(model, i + 1),
                })
            });
        result
            .entry(gap)
            .and_modify(|existing| {
                if *existing != cut {
                    *existing = None;
                }
            })
            .or_insert(cut);
    }
    for gap in interiors {
        if let Some(cut) = result.get_mut(&gap) {
            *cut = None;
        }
    }
    result
}

pub(super) fn body_depth(model: &Model, token_index: usize) -> usize {
    let statements = &model.statements;
    let before = statements.partition_point(|statement| statement.token_range.start <= token_index);
    usize::from(before.checked_sub(1).is_some_and(|index| {
        let statement = &statements[index];
        statement.kind == crate::engine::language::model::StatementKind::Block
            && statement.name != "verbatim"
            && statement.opener_range.end <= token_index
            && token_index < statement.token_range.end.saturating_sub(2)
    }))
}

fn literal_unit_pair(owner: &Owner<'_>, file: &str, tokens: &[Token], text: &str) -> Option<Span> {
    let first = tokens.first()?;
    let last = tokens.last()?;
    let written = owner.written(
        file,
        Span {
            start: first.span.start,
            end: last.span.end,
        },
    )?;
    let raw = text.get(written.start as usize..written.end as usize)?;
    match (first.emitted.as_ref(), last.emitted.as_ref()) {
        (Some(a), Some(b)) if std::sync::Arc::ptr_eq(&a.source, &b.source) => {
            let emitted = a
                .source
                .text
                .get(a.span.start as usize..b.span.end as usize)?;
            let origins = &a.source.origins;
            let first = origins.partition_point(|origin| origin.emitted.end <= a.span.start);
            if raw != emitted
                || origins[first..]
                    .iter()
                    .take_while(|origin| origin.emitted.start < b.span.end)
                    .any(|origin| !origin.copied)
            {
                return None;
            }
        }
        (None, None) => {}
        _ => return None,
    }
    Some(written)
}
