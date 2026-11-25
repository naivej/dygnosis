//! Semantics-preserving formatter. Whitespace only; comments and `@#` stay verbatim.

use serde_json::Value;
use std::ops::Range;

use crate::engine::language::lexer::{TokenKind, tokenize};
use crate::engine::language::model::{Model, StatementKind};
use crate::engine::workspace::EffectiveOrigins;

mod layout;
pub(crate) mod spacing;

const UNSAFE_CHARS: &[char] = &['\'', '"', '[', ']', '{', '}', '@', '#', '%', ':', '\\', '!'];

#[derive(Clone, Default)]
struct SourceLine {
    preserve: bool,
    keep_blank: bool,
    opens: bool,
    closes: bool,
    spacing: Option<Vec<(usize, usize, &'static str)>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct FormatStyle {
    pub split_statements: bool,
    pub align_assignments: bool,
}

pub(crate) struct FormatOwner<'a> {
    pub model: &'a Model,
    pub origins: &'a EffectiveOrigins,
}

pub(crate) struct SourceFormat {
    pub replacement: Option<(Range<usize>, String)>,
    pub splits: usize,
    pub withheld: usize,
}

pub(crate) const SPLIT_REFUSED: &str = "No formatting edits are available. The formatter could not verify the statement boundaries, source ownership or selection.";

pub(crate) fn split_notice(splits: usize) -> String {
    format!(
        "Formatting edits include {splits} statement splits. The formatter skipped possible splits where it could not verify boundaries, source ownership or the selection."
    )
}

fn accepted_units(model: &Model) -> impl Iterator<Item = (Range<usize>, layout::Edge)> {
    model
        .format_rows
        .iter()
        .cloned()
        .map(|range| (range, layout::Edge::Row))
        .chain(model.format_blocks.iter().flat_map(|(open, close)| {
            [
                (open.clone(), layout::Edge::Open),
                (close.clone(), layout::Edge::Close),
            ]
        }))
}

/// Format the selected source over the current owner analyses. The optional
/// byte range is the original selection, before expanding ordinary layout to
/// whole lines.
pub(crate) fn format_source(
    text: &str,
    file: &str,
    selection: Option<Range<usize>>,
    indent: &str,
    style: FormatStyle,
    owners: &[FormatOwner<'_>],
    work_limit: crate::engine::macro_work::MacroWorkLimit,
) -> Result<SourceFormat, &'static str> {
    let mut result = SourceFormat {
        replacement: None,
        splits: 0,
        withheld: 0,
    };
    if text.trim().is_empty() {
        return Ok(result);
    }
    const UNSAFE: &str =
        "The formatter cannot preserve the selected source while changing whitespace.";
    let normalised = normalise_line_breaks(text);
    let written_lines = split_line_breaks(text);
    let starts: Vec<_> = written_lines
        .iter()
        .map(|line| line.as_ptr() as usize - text.as_ptr() as usize)
        .collect();
    let stripped = blank_comments_macros(&normalised);
    let stripped_lines: Vec<_> = stripped.split('\n').collect();
    if written_lines.len() != stripped_lines.len() {
        return Err(UNSAFE);
    }
    let selected = selection.clone().unwrap_or(0..text.len());
    if selected.start > selected.end || selected.end > text.len() {
        return Err("The formatting range is outside the document or reversed.");
    }
    let first = starts.partition_point(|at| *at <= selected.start) - 1;
    let mut last = starts.partition_point(|at| *at <= selected.end) - 1;
    if selection.is_some() && last > first && starts[last] == selected.end {
        last -= 1;
    }
    let views: Vec<_> = owners
        .iter()
        .map(|owner| layout::Owner {
            model: owner.model,
            origins: Some(owner.origins),
        })
        .collect();
    let complete = !views.is_empty()
        && views
            .iter()
            .all(|view| !view.model.macro_incomplete() && !view.model.is_analysis_preview());
    let (mut structure, initial_depth) = if complete {
        retained_lines(text, file, &starts, &views)
    } else {
        (source_lines(&normalised, work_limit), 0)
    };
    let splits = (style.split_statements && complete)
        .then(|| layout::splits(text, file, &starts, selected, &views));
    if style.split_statements && !complete {
        result.withheld = 1;
    }
    if let Some(splits) = &splits {
        result.withheld = splits.withheld;
        for index in &splits.blocked_lines {
            structure[*index].preserve = true;
        }
    }
    let depth = depth_before_from(&structure, first, initial_depth);
    let mut original = Vec::new();
    let mut mask = Vec::new();
    let mut lines = Vec::new();
    let mut endings = Vec::new();
    let default_eol = line_ending(text);
    for index in first..=last {
        let source = written_lines[index];
        let stripped = stripped_lines[index];
        let line = &structure[index];
        let ending = source_ending(text, &starts, index, source.len());
        let cuts = splits.as_ref().and_then(|splits| splits.cuts.get(&index));
        if let Some(cuts) = cuts.filter(|_| !line.preserve) {
            let mut start = 0;
            let mut stripped = stripped;
            let mut closes = cuts[0].left_closes;
            for cut in cuts {
                let end = cut.gap.start - starts[index];
                let matched = prefix_chars(stripped, source[start..end].chars().count());
                push_fragment(
                    &mut original,
                    &mut mask,
                    &mut lines,
                    source,
                    matched,
                    line,
                    start..end,
                    cut.opens,
                    closes,
                );
                endings.push(if ending.is_empty() {
                    default_eol
                } else {
                    ending
                });
                start = cut.gap.end - starts[index];
                stripped = stripped.get(matched.len() + start - end..).ok_or(UNSAFE)?;
                closes = cut.closes;
            }
            push_fragment(
                &mut original,
                &mut mask,
                &mut lines,
                source,
                stripped,
                line,
                start..source.len(),
                cuts.last().is_some_and(|cut| cut.right_opens),
                closes,
            );
            result.splits += cuts.len();
        } else {
            result.withheld += cuts.map_or(0, Vec::len);
            original.push(source.to_owned());
            mask.push(stripped.to_owned());
            lines.push(line.clone());
        }
        endings.push(ending);
    }
    let mut formatted = format_lines(
        &original,
        &mask,
        &lines,
        indent,
        depth,
        style.align_assignments,
    )
    .ok_or(UNSAFE)?;
    let document = selection.is_none();
    let finish_document = document
        && !structure
            .iter()
            .zip(&written_lines)
            .rev()
            .take_while(|(line, source)| line.preserve || source.is_empty())
            .any(|(line, _)| line.preserve);
    if finish_document {
        while formatted.last().is_some_and(|(_, text)| text.is_empty()) {
            formatted.pop();
        }
    }
    let span = if document {
        0..text.len()
    } else {
        starts[first]..starts[last] + written_lines[last].len()
    };
    let replacement = join_formatted(
        &formatted,
        &endings,
        document,
        finish_document.then_some(default_eol),
    );
    if canonical(&replacement) != canonical(&text[span.clone()]) {
        return Err(UNSAFE);
    }
    if replacement != text[span.clone()] {
        result.replacement = Some((span, replacement));
    }
    Ok(result)
}

fn source_ending<'a>(text: &'a str, starts: &[usize], line: usize, line_len: usize) -> &'a str {
    &text[starts[line] + line_len..starts.get(line + 1).copied().unwrap_or(text.len())]
}

fn join_formatted(
    lines: &[(usize, String)],
    endings: &[&str],
    include_last_ending: bool,
    default_last_ending: Option<&str>,
) -> String {
    let mut output = String::new();
    for (index, (source, text)) in lines.iter().enumerate() {
        output.push_str(text);
        let ending = endings[*source];
        if index + 1 < lines.len() {
            output.push_str(ending);
        } else if include_last_ending {
            output.push_str(if ending.is_empty() {
                default_last_ending.unwrap_or("")
            } else {
                ending
            });
        }
    }
    output
}

#[allow(clippy::too_many_arguments)]
fn push_fragment(
    original: &mut Vec<String>,
    mask: &mut Vec<String>,
    lines: &mut Vec<SourceLine>,
    source: &str,
    stripped: &str,
    line: &SourceLine,
    bytes: Range<usize>,
    opens: bool,
    closes: bool,
) {
    let fragment = &source[bytes.clone()];
    original.push(fragment.to_owned());
    mask.push(stripped.to_owned());
    let spacing = line.spacing.as_ref().map(|edits| {
        let first = edits.partition_point(|&(start, _, _)| start < bytes.start);
        edits[first..]
            .iter()
            .take_while(|&&(start, _, _)| start <= bytes.end)
            .filter(|&&(start, end, _)| start >= bytes.start && end <= bytes.end)
            .map(|&(start, end, gap)| (start - bytes.start, end - bytes.start, gap))
            .collect()
    });
    lines.push(SourceLine {
        opens,
        closes,
        spacing,
        preserve: false,
        keep_blank: false,
    });
}

fn retained_lines(
    text: &str,
    file: &str,
    starts: &[usize],
    views: &[layout::Owner<'_>],
) -> (Vec<SourceLine>, usize) {
    let line_at = |offset: u32| starts.partition_point(|&at| at <= offset as usize) - 1;
    let mut depths = Vec::new();
    let mut actual = Vec::new();
    for owner in views {
        let model = owner.model;
        let tokens = &model.expanded_tokens;
        let mut lines = vec![SourceLine::default(); starts.len()];
        spacing::collect(text, file, owner, starts, &mut lines);
        let first_token = tokens.iter().position(|token| {
            token.kind != TokenKind::Eof && owner.written(file, token.span).is_some()
        });
        let depth = first_token.map_or(0, |index| layout::body_depth(model, index));
        depths.push(depth);
        for statement in &model.statements {
            if statement.kind != StatementKind::Block
                || statement.native
                || statement.name == "verbatim"
            {
                continue;
            }
            let open = statement
                .opener_range
                .end
                .checked_sub(1)
                .and_then(|index| tokens.get(index))
                .filter(|token| token.kind == TokenKind::Semi)
                .and_then(|token| owner.literal(file, token))
                .map(|span| line_at(span.start));
            let close = statement
                .complete
                .then_some(statement.token_range.end)
                .and_then(|end| end.checked_sub(2))
                .and_then(|index| tokens.get(index))
                .filter(|token| token.kind == TokenKind::Ident)
                .and_then(|token| owner.literal(file, token))
                .map(|span| line_at(span.start));
            if open != close {
                if let Some(open) = open {
                    lines[open].opens = true;
                }
                if let Some(close) = close {
                    lines[close].closes = true;
                }
            }
        }
        let mut marks = vec![0i64; starts.len() + 1];
        layout::protected(owner, file, |span| {
            if !span.is_empty() && span.end as usize <= text.len() {
                marks[line_at(span.start)] += 1;
                marks[line_at(span.end - 1) + 1] -= 1;
            }
        });
        let mut active = 0;
        for (line, mark) in lines.iter_mut().zip(marks) {
            active += mark;
            line.preserve = active > 0;
        }
        if actual.is_empty() {
            actual = lines;
        } else {
            for (current, next) in actual.iter_mut().zip(lines) {
                current.preserve |=
                    next.preserve || current.opens != next.opens || current.closes != next.closes;
                current.keep_blank |= next.keep_blank;
                if current.spacing != next.spacing {
                    current.spacing = Some(Vec::new());
                }
            }
        }
    }
    let depth = depths[0];
    let ambiguous_depth = depths.iter().any(|other| *other != depth);
    for (index, line) in actual.iter_mut().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(text.len());
        line.preserve |= ambiguous_depth || text[starts[index]..end].contains("@#");
    }
    (actual, depth)
}

/// Whole-file format result. `format_text` is `Some` only for [`FormatOutcome::Changed`].
pub enum FormatOutcome {
    Changed(String),
    Unchanged,
    Unsupported(&'static str),
}

/// Same decision as [`format_text`], with unchanged and unsupported kept apart.
pub fn format_outcome(text: &str, indent_unit: &str) -> FormatOutcome {
    format_outcome_with_limit(text, indent_unit, Default::default())
}

pub(crate) fn format_outcome_with_limit(
    text: &str,
    indent_unit: &str,
    work_limit: crate::engine::macro_work::MacroWorkLimit,
) -> FormatOutcome {
    if text.trim().is_empty() {
        return FormatOutcome::Unchanged;
    }
    let Some(formatted) = reformat(text, indent_unit, work_limit) else {
        return FormatOutcome::Unsupported("formatter cannot reformat this file");
    };
    if formatted == text {
        return FormatOutcome::Unchanged;
    }
    if canonical(&formatted) != canonical(text) {
        return FormatOutcome::Unsupported("formatting would change more than whitespace");
    }
    FormatOutcome::Changed(formatted)
}

/// Reformat whole-file text, or `None` to leave it unchanged.
pub fn format_text(text: &str, indent_unit: &str) -> Option<String> {
    match format_outcome(text, indent_unit) {
        FormatOutcome::Changed(formatted) => Some(formatted),
        FormatOutcome::Unchanged | FormatOutcome::Unsupported(_) => None,
    }
}

/// LSP `formatIndent`: `tab`, or a whole number of spaces from 1 to 8.
pub fn parse_format_indent(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        let s = s.trim();
        if s.eq_ignore_ascii_case("tab") {
            return Some("\t".into());
        }
        if s.chars().all(|c| c.is_ascii_digit())
            && let Ok(n) = s.parse::<usize>()
            && (1..=8).contains(&n)
        {
            return Some(" ".repeat(n));
        }
        return None;
    }
    if let Some(n) = value.as_u64()
        && (1..=8).contains(&n)
    {
        return Some(" ".repeat(n as usize));
    }
    if let Some(n) = value.as_i64()
        && (1..=8).contains(&n)
    {
        return Some(" ".repeat(n as usize));
    }
    None
}

/// Format inclusive line range `[start_line, end_line]`.
///
/// Returns `(start_line, end_line, replacement)` for a whole-line edit, or `None`.
pub fn format_range(
    text: &str,
    start_line: u32,
    end_line: u32,
    indent_unit: &str,
) -> Option<(u32, u32, String)> {
    if text.trim().is_empty() {
        return None;
    }
    let raw_lines: Vec<&str> = text.split('\n').collect();
    let orig_lines: Vec<String> = raw_lines
        .iter()
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect();
    let stripped = blank_comments_macros(text);
    let stripped_lines: Vec<String> = stripped
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect();
    let start = start_line as usize;
    let end = (end_line as usize).min(orig_lines.len() - 1);
    if start > end {
        return None;
    }
    let structure = source_lines(text, Default::default());
    let depth = depth_before(&structure, start);
    let formatted = format_lines(
        &orig_lines[start..=end],
        &stripped_lines[start..=end],
        &structure[start..=end],
        indent_unit,
        depth,
        false,
    )?;
    let original_slice = raw_lines[start..=end].join("\n");
    let endings: Vec<_> = raw_lines[start..=end]
        .iter()
        .map(|line| if line.ends_with('\r') { "\r\n" } else { "\n" })
        .collect();
    let mut replacement = join_formatted(&formatted, &endings, false, None);
    if raw_lines[end].ends_with('\r') {
        replacement.push('\r');
    }
    (replacement != original_slice && canonical(&replacement) == canonical(&original_slice))
        .then_some((start as u32, end as u32, replacement))
}

fn reformat(
    text: &str,
    indent_unit: &str,
    work_limit: crate::engine::macro_work::MacroWorkLimit,
) -> Option<String> {
    let orig_lines = split_line_breaks(text);
    let normalised = normalise_line_breaks(text);
    let stripped = blank_comments_macros(&normalised);
    let stripped_lines: Vec<&str> = stripped.split('\n').collect();
    if orig_lines.len() != stripped_lines.len() {
        return None;
    }
    let orig_owned: Vec<String> = orig_lines.iter().map(|s| s.to_string()).collect();
    let stripped_owned: Vec<String> = stripped_lines.iter().map(|s| s.to_string()).collect();
    let structure = source_lines(&normalised, work_limit);
    let mut out = format_lines(
        &orig_owned,
        &stripped_owned,
        &structure,
        indent_unit,
        0,
        false,
    )?;
    while out.last().is_some_and(|(_, text)| text.is_empty()) {
        out.pop();
    }
    let starts: Vec<_> = orig_lines
        .iter()
        .map(|line| line.as_ptr() as usize - text.as_ptr() as usize)
        .collect();
    let endings: Vec<_> = orig_lines
        .iter()
        .enumerate()
        .map(|(line, source)| source_ending(text, &starts, line, source.len()))
        .collect();
    Some(join_formatted(
        &out,
        &endings,
        true,
        Some(line_ending(text)),
    ))
}

fn line_ending(text: &str) -> &str {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            return "\r\n";
        }
        if bytes[i] == b'\r' {
            return "\r";
        }
        if bytes[i] == b'\n' {
            return "\n";
        }
        i += 1;
    }
    "\n"
}

fn split_line_breaks(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            out.push(&text[start..i]);
            i += 2;
            start = i;
        } else if bytes[i] == b'\r' || bytes[i] == b'\n' {
            out.push(&text[start..i]);
            i += 1;
            start = i;
        } else {
            i += 1;
        }
    }
    out.push(&text[start..]);
    out
}

fn normalise_line_breaks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            out.push('\n');
            i += 2;
        } else if bytes[i] == b'\r' {
            out.push('\n');
            i += 1;
        } else {
            // SAFETY: i is on a char boundary because we only skip ASCII CR/LF.
            let ch = text[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// Spaces separate arguments in MATLAB command syntax. The shared parser owns
/// native line boundaries, including continuations and macro execution. Preserve
/// whole physical lines when any part is native, verbatim, or opaque recovery.
/// Block transitions use the same parser's accepted token boundaries, including
/// multiline headers. Equal written macro copies produce one transition.
fn source_lines(
    text: &str,
    work_limit: crate::engine::macro_work::MacroWorkLimit,
) -> Vec<SourceLine> {
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(at, _)| at + 1));
    let model = crate::engine::language::parser::parse_with_budget(
        text,
        &mut crate::engine::language::macro_expr::MacroBudget::with_limit(work_limit, None),
    );
    let fallback;
    let structure = if model.macro_incomplete() {
        // Source presentation uses an inert parse when execution stops. This
        // does not create accepted language facts or execute a failed branch.
        let inert = crate::engine::analysis::folding::inert_source(text);
        fallback = crate::engine::language::parser::parse_expanded_controlled(
            &inert,
            tokenize(&inert),
            None,
            work_limit,
        )
        .0;
        &fallback
    } else {
        &model
    };
    let mut lines = vec![SourceLine::default(); starts.len()];
    let owner = layout::Owner {
        model: &model,
        origins: None,
    };
    spacing::collect(text, "", &owner, &starts, &mut lines);
    let line_at = |offset: u32| starts.partition_point(|&at| at <= offset as usize) - 1;
    for statement in &structure.statements {
        if statement.kind != crate::engine::language::model::StatementKind::Block
            || statement.native
            || statement.name == "verbatim"
        {
            continue;
        }
        let Some(opener) = statement
            .opener_range
            .end
            .checked_sub(1)
            .and_then(|index| structure.expanded_tokens.get(index))
            .filter(|token| token.kind == TokenKind::Semi)
        else {
            continue;
        };
        let open_line = line_at(opener.span.start);
        lines[open_line].opens = true;
        if statement.complete
            && let Some(closer) = statement
                .token_range
                .end
                .checked_sub(2)
                .and_then(|index| structure.expanded_tokens.get(index))
            && closer.kind == TokenKind::Ident
            && closer.text(text).eq_ignore_ascii_case("end")
        {
            let close_line = line_at(closer.span.start);
            if close_line == open_line {
                lines[open_line].opens = false;
            } else {
                lines[close_line].closes = true;
            }
        }
    }
    // Difference marks avoid rescanning long spans repeated by macro execution.
    let mut marks = vec![0i64; starts.len() + 1];
    let mut spans: Vec<_> = model
        .execution_steps
        .iter()
        .filter_map(|step| match step {
            crate::engine::language::model::ExecutionStep::Opaque(span) => Some(*span),
            crate::engine::language::model::ExecutionStep::Statement(id) => {
                let statement = &model.statements[*id];
                (statement.native || statement.name == "verbatim").then_some(statement.span)
            }
        })
        .chain(
            model
                .ms_unparsed_spans
                .iter()
                .copied()
                .filter(|_| !model.macro_incomplete()),
        )
        .chain(model.inactive_macro_spans().iter().copied())
        .collect();
    layout::protected_literals(&owner, "", |span| spans.push(span));
    if model.macro_incomplete() {
        // Failed expansion cannot prove block ownership. Preserve possible
        // command arguments even in text whose execution could not be read.
        let tokens = tokenize(text);
        for (index, pair) in tokens.windows(2).enumerate() {
            let head = &pair[0];
            let next = &pair[1];
            if head.kind != TokenKind::Ident
                || crate::engine::language::command_skip::is_pin_statement_keyword(head.text(text))
                || matches!(
                    next.kind,
                    TokenKind::Eq | TokenKind::LParen | TokenKind::Eof
                )
            {
                continue;
            }
            let gap = &text[head.span.end as usize..next.span.start as usize];
            let boundary = index == 0
                || matches!(tokens[index - 1].kind, TokenKind::Semi | TokenKind::Comma)
                || text[tokens[index - 1].span.end as usize..head.span.start as usize]
                    .contains('\n');
            if boundary
                && !gap.is_empty()
                && !gap.contains('\n')
                && gap.chars().all(char::is_whitespace)
            {
                spans.push(crate::engine::language::span::Span::new(
                    head.span.start as usize,
                    crate::engine::language::native_line::native_region_end(
                        text,
                        head.span.start as usize,
                    ),
                ));
            }
        }
    }
    for span in spans {
        let first = starts.partition_point(|&at| at <= span.start as usize) - 1;
        let last = starts.partition_point(|&at| at <= span.end.saturating_sub(1) as usize) - 1;
        if first <= last {
            marks[first] += 1;
            marks[last + 1] -= 1;
        }
    }
    let mut active = 0;
    for (line, mark) in lines.iter_mut().zip(&marks) {
        active += mark;
        line.preserve = active > 0;
    }
    lines
}

fn depth_before(structure: &[SourceLine], upto: usize) -> usize {
    depth_before_from(structure, upto, 0)
}

fn depth_before_from(structure: &[SourceLine], upto: usize, mut depth: usize) -> usize {
    for line in structure.iter().take(upto) {
        if line.preserve {
            continue;
        }
        if line.closes {
            depth = depth.saturating_sub(1);
        }
        if line.opens {
            depth += 1;
        }
    }
    depth
}

fn format_lines(
    orig_lines: &[String],
    stripped_lines: &[String],
    structure: &[SourceLine],
    indent_unit: &str,
    start_depth: usize,
    align_assignments: bool,
) -> Option<Vec<(usize, String)>> {
    if orig_lines.len() != stripped_lines.len() || orig_lines.len() != structure.len() {
        return None;
    }
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut depth = start_depth;
    let mut assign_run: Vec<(usize, String, String)> = Vec::new();

    for (index, ((orig, stripped), source_line)) in orig_lines
        .iter()
        .zip(stripped_lines)
        .zip(structure)
        .enumerate()
    {
        if source_line.preserve || source_line.keep_blank && orig.trim().is_empty() {
            flush_run(&mut out, &mut assign_run);
            out.push((index, orig.clone()));
            continue;
        }
        let code = stripped.trim_end();

        if code.trim().is_empty() {
            flush_run(&mut out, &mut assign_run);
            if orig.trim().is_empty() {
                if out.last().is_some_and(|(_, text)| text.is_empty()) {
                    continue;
                }
                out.push((index, String::new()));
            } else {
                out.push((index, orig.trim_end().to_string()));
            }
            continue;
        }

        if source_line.closes {
            flush_run(&mut out, &mut assign_run);
            depth = depth.saturating_sub(1);
            out.push((
                index,
                format!("{}{}", indent_unit.repeat(depth), orig.trim()),
            ));
            if source_line.opens {
                depth += 1;
            }
            continue;
        }

        let indent = indent_unit.repeat(depth);
        let code_end = char_len(code);
        let body = prefix_chars(orig, code_end);
        let trailing = suffix_chars(orig, code_end).trim();

        if !trailing.is_empty() && !comment_prefix(trailing) {
            flush_run(&mut out, &mut assign_run);
            out.push((index, format!("{}{}", indent, orig.trim())));
            if source_line.opens {
                depth += 1;
            }
            continue;
        }

        let no_midline_comment = prefix_chars(orig, code_end) == prefix_chars(stripped, code_end);
        let spaced = if no_midline_comment {
            if let Some(edits) = &source_line.spacing {
                let mut result = body.to_string();
                for &(start, end, replacement) in edits.iter().rev() {
                    if end > result.len()
                        || !result.is_char_boundary(start)
                        || !result.is_char_boundary(end)
                    {
                        return None;
                    }
                    result.replace_range(start..end, replacement);
                }
                Some(result.trim().to_string())
            } else {
                space_line(body.trim())
            }
        } else {
            None
        };
        let line_body = spaced.as_deref().unwrap_or_else(|| body.trim()).to_string();
        let mut line = format!("{indent}{line_body}");
        if !trailing.is_empty() {
            line.push(' ');
            line.push_str(trailing);
        }
        let line_index = out.len();
        out.push((index, line));

        let assign = if align_assignments && spaced.is_some() && trailing.is_empty() {
            assignment_parts(&line_body)
        } else {
            None
        };
        if let Some((lhs, rest)) = assign {
            if !source_line.opens {
                assign_run.push((line_index, lhs.to_string(), rest.to_string()));
            } else {
                flush_run(&mut out, &mut assign_run);
            }
        } else {
            flush_run(&mut out, &mut assign_run);
        }

        if source_line.opens {
            depth += 1;
        }
    }

    flush_run(&mut out, &mut assign_run);
    Some(out)
}

fn comment_prefix(trailing: &str) -> bool {
    trailing.starts_with("//") || trailing.starts_with('%') || trailing.starts_with("/*")
}

fn flush_run(out: &mut [(usize, String)], assign_run: &mut Vec<(usize, String, String)>) {
    if assign_run.len() > 1 {
        let width = assign_run
            .iter()
            .map(|(_, lhs, _)| lhs.len())
            .max()
            .unwrap_or(0);
        for (idx, lhs, rest) in assign_run.iter() {
            let line = &mut out[*idx].1;
            let Some(at) = line.find(lhs) else {
                continue;
            };
            let prefix = &line[..at];
            *line = format!("{prefix}{:<width$} = {rest}", lhs);
        }
    }
    assign_run.clear();
}

fn space_line(code: &str) -> Option<String> {
    if code.chars().any(|c| UNSAFE_CHARS.contains(&c)) {
        return None;
    }
    let tokens = simple_tokens(code)?;
    let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
    let gaps = spacing::gaps(&refs);
    Some(
        gaps.into_iter()
            .zip(tokens)
            .map(|(gap, token)| format!("{gap}{token}"))
            .collect(),
    )
}

fn simple_tokens(code: &str) -> Option<Vec<String>> {
    let toks = tokenize(code);
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let tok = &toks[i];
        if tok.kind == TokenKind::Eof {
            break;
        }
        if !simple_kind(tok.kind) {
            return None;
        }
        if tok.kind == TokenKind::Star
            && i + 1 < toks.len()
            && toks[i + 1].kind == TokenKind::Star
            && tok.span.end == toks[i + 1].span.start
        {
            out.push("**".to_string());
            i += 2;
            continue;
        }
        out.push(tok.text(code).to_string());
        i += 1;
    }
    let joined: String = out.concat();
    let nospace: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if joined != nospace {
        return None;
    }
    Some(out)
}

fn simple_kind(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Ident
            | TokenKind::Number
            | TokenKind::Eq
            | TokenKind::EqEq
            | TokenKind::Ne
            | TokenKind::Lt
            | TokenKind::Gt
            | TokenKind::Le
            | TokenKind::Ge
            | TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Star
            | TokenKind::Slash
            | TokenKind::Caret
            | TokenKind::Comma
            | TokenKind::Semi
            | TokenKind::LParen
            | TokenKind::RParen
    )
}

fn assignment_parts(line_body: &str) -> Option<(&str, &str)> {
    let ident_end = ident_prefix_len(line_body)?;
    let after_ident = &line_body[ident_end..];
    let after_ws = after_ident.trim_start();
    let rest = after_ws.strip_prefix('=')?;
    if rest.starts_with('=') {
        return None;
    }
    let rhs = rest.trim_start();
    if !rhs.chars().any(|c| !c.is_whitespace()) {
        return None;
    }
    if rhs.chars().last().is_some_and(char::is_whitespace) {
        return None;
    }
    Some((&line_body[..ident_end], rhs))
}

fn ident_prefix_len(s: &str) -> Option<usize> {
    let mut chars = s.char_indices();
    let (_, first) = chars.next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    let mut end = first.len_utf8();
    for (i, c) in chars {
        if c.is_ascii_alphanumeric() || c == '_' {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    Some(end)
}

fn canonical(text: &str) -> String {
    let normalised = normalise_line_breaks(text);
    let blanked = blank_comments_macros(&normalised);
    let tokens = canonical_tokens(&blanked);
    let atoms = interp_atoms(&normalised);
    format!("{}\u{1}{}", tokens.join("\u{0}"), atoms.join("\u{0}"))
}

fn canonical_tokens(blanked: &str) -> Vec<String> {
    let chars: Vec<char> = blanked.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        if let Some(end) = match_ident(&chars, i)
            .or_else(|| match_number(&chars, i))
            .or_else(|| match_multi_op(&chars, i))
        {
            out.push(chars[i..end].iter().collect());
            i = end;
        } else {
            out.push(chars[i].to_string());
            i += 1;
        }
    }
    out
}

fn match_ident(chars: &[char], i: usize) -> Option<usize> {
    let c = *chars.get(i)?;
    if !(c.is_ascii_alphabetic() || c == '_') {
        return None;
    }
    let mut j = i + 1;
    while j < chars.len() && is_word(chars[j]) {
        j += 1;
    }
    Some(j)
}

fn match_number(chars: &[char], i: usize) -> Option<usize> {
    if chars[i].is_ascii_digit() {
        let mut j = i;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
        if j < chars.len() && chars[j] == '.' {
            j += 1;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
        }
        return Some(match_exponent(chars, j));
    }
    if chars[i] == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
        let mut j = i + 1;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
        return Some(match_exponent(chars, j));
    }
    None
}

fn match_exponent(chars: &[char], i: usize) -> usize {
    if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
        let mut j = i + 1;
        if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
            j += 1;
        }
        if j < chars.len() && chars[j].is_ascii_digit() {
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            return j;
        }
    }
    i
}

fn match_multi_op(chars: &[char], i: usize) -> Option<usize> {
    if i + 1 >= chars.len() {
        return None;
    }
    let a = chars[i];
    let b = chars[i + 1];
    match (a, b) {
        ('=', '=')
        | ('!', '=')
        | ('<', '=')
        | ('>', '=')
        | ('&', '&')
        | ('|', '|')
        | ('*', '*') => Some(i + 2),
        _ => None,
    }
}

fn interp_atoms(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let start = i;
        while i < chars.len() && is_word(chars[i]) {
            i += 1;
        }
        // '@' cannot occur inside the word prefix. Check its end once instead
        // of rescanning every suffix of a long identifier or quoted value.
        if let Some(end) = match_interp_groups(&chars, i) {
            out.push(chars[start..end].iter().collect());
            i = end;
        } else if i == start {
            i += 1;
        }
    }
    out
}

fn match_interp_groups(chars: &[char], mut i: usize) -> Option<usize> {
    let mut n = 0usize;
    loop {
        if i + 1 >= chars.len() || chars[i] != '@' || chars[i + 1] != '{' {
            break;
        }
        let save = i;
        i += 2;
        while i < chars.len() && chars[i] != '}' && chars[i] != '\n' {
            i += 1;
        }
        if i >= chars.len() || chars[i] != '}' {
            i = save;
            break;
        }
        i += 1;
        while i < chars.len() && is_word(chars[i]) {
            i += 1;
        }
        n += 1;
    }
    if n > 0 { Some(i) } else { None }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn blank_comments_macros(text: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if c == '"' || c == '\'' {
            let quote = c;
            i += 1;
            while i < n && chars[i] != quote && chars[i] != '\n' {
                i += 1;
            }
            if i < n && chars[i] == quote {
                i += 1;
            }
            continue;
        }
        if c == '/' && i + 1 < n && chars[i + 1] == '/' {
            while i < n && chars[i] != '\n' {
                if chars[i] != '\r' {
                    chars[i] = ' ';
                }
                i += 1;
            }
            continue;
        }
        if c == '%' {
            while i < n && chars[i] != '\n' {
                if chars[i] != '\r' {
                    chars[i] = ' ';
                }
                i += 1;
            }
            continue;
        }
        if c == '/' && i + 1 < n && chars[i + 1] == '*' {
            chars[i] = ' ';
            chars[i + 1] = ' ';
            i += 2;
            while i + 1 < n && !(chars[i] == '*' && chars[i + 1] == '/') {
                if chars[i] != '\n' {
                    chars[i] = ' ';
                }
                i += 1;
            }
            if i + 1 < n {
                chars[i] = ' ';
                chars[i + 1] = ' ';
                i += 2;
            }
            continue;
        }
        if c == '@' && i + 1 < n && chars[i + 1] == '#' {
            let end = macro_directive_end(&chars, i);
            for ch in chars.iter_mut().take(end).skip(i) {
                if *ch != '\r' && *ch != '\n' {
                    *ch = ' ';
                }
            }
            i = end;
            continue;
        }
        i += 1;
    }
    let mut i = 0;
    let n = chars.len();
    while i < n {
        if chars[i] == '@' && i + 1 < n && chars[i + 1] == '{' {
            let start = i;
            i += 2;
            while i < n && chars[i] != '}' {
                i += 1;
            }
            if i < n && chars[i] == '}' {
                i += 1;
            }
            for ch in chars.iter_mut().take(i).skip(start) {
                if *ch != '\n' {
                    *ch = ' ';
                }
            }
            continue;
        }
        i += 1;
    }
    chars.into_iter().collect()
}

fn macro_directive_end(chars: &[char], start: usize) -> usize {
    let mut line_start = start;
    while line_start < chars.len() {
        let mut j = line_start;
        while j < chars.len() && chars[j] != '\n' && chars[j] != '\r' {
            j += 1;
        }
        if j >= chars.len() {
            return chars.len();
        }
        let line_end = j;
        if line_ends_with_backslash(&chars[line_start..line_end]) {
            if chars[j] == '\r' && j + 1 < chars.len() && chars[j + 1] == '\n' {
                line_start = j + 2;
            } else {
                line_start = j + 1;
            }
        } else {
            return line_end;
        }
    }
    chars.len()
}

fn line_ends_with_backslash(line: &[char]) -> bool {
    let mut k = line.len();
    while k > 0 && (line[k - 1] == ' ' || line[k - 1] == '\t') {
        k -= 1;
    }
    k > 0 && line[k - 1] == '\\'
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

fn prefix_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

fn suffix_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[i..],
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolation_atoms_keep_word_affixes_and_adjacent_groups() {
        // Guards a forward scan dropping an identifier prefix/suffix or joining
        // atoms across a word boundary, including Unicode word characters.
        let source = "plain _pre2@{i}尾@{j}_tail + @{k}@{l} + αβ_2@{n}ζ";
        assert_eq!(
            interp_atoms(source),
            ["_pre2@{i}尾@{j}_tail", "@{k}@{l}", "αβ_2@{n}ζ"]
        );
        for (joined, separated) in [
            ("pre@{i}", "pre @{i}"),
            ("@{i}tail", "@{i} tail"),
            ("@{i}@{j}", "@{i} @{j}"),
            ("αβ@{i}尾", "αβ @{i}尾"),
        ] {
            assert_ne!(canonical(joined), canonical(separated));
        }
    }

    #[test]
    fn interpolation_atoms_keep_completed_groups_before_malformed_text() {
        // Guards advancing past a rejected group and losing a later atom, or
        // changing the existing first-closing-brace treatment of nested text.
        for (source, expected) in [
            ("prefix@{missing\nnext@{ok}suffix", vec!["next@{ok}suffix"]),
            (
                "prefix@{a}@{missing\nnext@{b}",
                vec!["prefix@{a}", "next@{b}"],
            ),
            ("prefix@{outer@{inner}}tail", vec!["prefix@{outer@{inner}"]),
            ("prefix.@{i}suffix", vec!["@{i}suffix"]),
        ] {
            assert_eq!(interp_atoms(source), expected, "{source}");
        }
    }

    #[test]
    fn empty_and_ws_is_unchanged() {
        for text in ["", "   \n", " \n\t\n"] {
            assert_eq!(format_text(text, "\t"), None);
            assert!(matches!(
                format_outcome(text, "\t"),
                FormatOutcome::Unchanged
            ));
        }
    }

    #[test]
    fn join_lead_lag_and_call() {
        assert_eq!(
            space_line("y=exp(z)*c(-1)+e").as_deref(),
            Some("y = exp(z) * c(-1) + e")
        );
    }

    #[test]
    fn starstar_stays_one_token() {
        assert_eq!(space_line("a**b;").as_deref(), Some("a ** b;"));
        assert_eq!(canonical("a**b;"), canonical("a ** b;"));
    }

    #[test]
    fn formats_accepted_tags_locals_and_metadata_without_changing_values() {
        // Guards rejecting whole Dynare lines solely because they contain a
        // quoted tag or TeX label, and guards changing their written values.
        let source = "var y $y$ (long_name='Output level');\nmodel;\n#lagged=rho*y(-1);\n[name='Demand',endogenous='y'] y=lagged+eps;\nend;\nestimation(datafile='observations.mat',first_obs=5,nobs=100);\n";
        let formatted = format_text(source, "    ").unwrap();
        assert!(
            formatted.contains("long_name = 'Output level'"),
            "{formatted}"
        );
        assert!(formatted.contains("#lagged = rho * y(-1);"), "{formatted}");
        assert!(
            formatted.contains("[name = 'Demand', endogenous = 'y'] y = lagged + eps;"),
            "{formatted}"
        );
        assert!(
            formatted.contains("datafile = 'observations.mat', first_obs = 5, nobs = 100"),
            "{formatted}"
        );
    }

    #[test]
    fn preserves_written_timing_and_list_separators() {
        // Guards interpreting a timing suffix or a signed list value as an
        // ordinary binary expression while formatting accepted rows.
        let source = "model;\n[name='timing'] y=x( - 1 )+z;\nend;\nshocks;\nvar e;\nperiods 1 2;\nvalues 1 -2;\nend;\n";
        let formatted = format_text(source, "    ").unwrap();
        assert!(formatted.contains("x( - 1 )"), "{formatted}");
        assert!(formatted.contains("values 1 -2;"), "{formatted}");
    }

    fn format_overlay(
        files: &[(&str, &str)],
        file: &str,
        selection: Option<Range<usize>>,
    ) -> SourceFormat {
        let sources = files
            .iter()
            .map(|(key, text)| (key.to_string(), text.to_string()))
            .collect();
        let mut workspace = crate::engine::workspace::Workspace::overlay_documents(&sources);
        let model = workspace.effective_model_snapshot("run.mod").unwrap();
        let origins = workspace.retained_effective_origins("run.mod").unwrap();
        let builds = &crate::engine::language::parser::PARSE_EXPANDED_BUILDS;
        let before = builds.with(std::cell::Cell::get);
        let result = format_source(
            &sources[file],
            file,
            selection,
            "  ",
            FormatStyle {
                split_statements: true,
                align_assignments: false,
            },
            &[FormatOwner {
                model: &model,
                origins: &origins,
            }],
            Default::default(),
        )
        .unwrap();
        assert_eq!(builds.with(std::cell::Cell::get), before);
        result
    }

    #[test]
    fn splits_one_written_copy_when_active_executions_agree() {
        let input = "parameters a longer;\n@#for i in 1:2\na=1; longer=2;\n@#endfor\n";
        let output = format_overlay(&[("run.mod", input)], "run.mod", None);
        assert_eq!(output.splits, 1);
        assert_eq!(output.withheld, 0);
        let (_, text) = output.replacement.unwrap();
        assert_eq!(
            text,
            "parameters a longer;\n@#for i in 1:2\na = 1;\nlonger = 2;\n@#endfor\n"
        );
    }

    #[test]
    fn preserves_written_line_when_another_macro_copy_uses_it_as_a_string() {
        let input = concat!(
            "parameters a b;\n@#for mode in [0,1]\n@#if mode\n",
            "@{\"var y (long_name='\"}\n@#endif\n  a=1; b=2;\n",
            "@#if mode\n@{\"');\"}\n@#endif\n@#endfor\n",
        );
        let result = format_overlay(&[("run.mod", input)], "run.mod", None);
        assert_eq!(result.splits, 0);
        assert!(result.withheld > 0);
        assert!(
            result
                .replacement
                .is_none_or(|(_, text)| text.contains("\n  a=1; b=2;\n"))
        );
    }

    #[test]
    fn preserves_every_physical_line_of_multiline_dynare_text() {
        let input = "var y(long_name='first\n  a=1; b=2;  \nlast');\nmodel; y=0; end;\n";
        let result = format_overlay(&[("run.mod", input)], "run.mod", None);
        let (_, text) = result.replacement.unwrap();
        assert!(text.contains("long_name='first\n  a=1; b=2;  \nlast')"));
        assert!(text.contains("model;\n  y = 0;\nend;"));
        assert!(format_text(input, "  ").is_none_or(|text| text.contains("\n  a=1; b=2;  \n")));
    }

    #[test]
    fn keeps_mixed_line_endings_inside_literals_and_native_text() {
        let input = "var y(long_name='top\r\n  middle  \nlast');\r\nmodel; y=0; end;\r\n  disp( ...\n  'native');";
        let expected = "var y(long_name='top\r\n  middle  \nlast');\r\nmodel;\r\n  y = 0;\r\nend;\r\n  disp( ...\n  'native');";
        let result = format_overlay(&[("run.mod", input)], "run.mod", None);
        assert_eq!(result.replacement.unwrap().1, expected);
        assert!(
            format_overlay(&[("run.mod", expected)], "run.mod", None)
                .replacement
                .is_none()
        );
    }

    #[test]
    fn keeps_blank_lines_inside_a_multiline_equation_after_splitting() {
        let input = "var y;\nmodel; y=1\n\n\n+2; end;\n";
        let result = format_overlay(&[("run.mod", input)], "run.mod", None);
        assert_eq!(
            result.replacement.unwrap().1,
            "var y;\nmodel;\n  y = 1\n\n\n  + 2;\nend;\n"
        );
    }

    #[test]
    fn keeps_blank_lines_inside_an_equation_with_macro_interpolation() {
        let input = "@#define X = 1\nvar y;\nmodel;\ny=@{X}\n\n\n+2;\nend;\n";
        let result = format_overlay(&[("run.mod", input)], "run.mod", None);
        assert!(
            result
                .replacement
                .is_none_or(|(_, text)| text.contains("y=@{X}\n\n\n"))
        );
    }

    #[test]
    fn formats_signs_only_in_verified_scalar_expressions() {
        let input = "parameters p; var y; varexo e;\np=1+-2;\nmodel; y=e; end;\ninitval; y=1-2; end;\nshocks; var e; periods 1 2; values 1 -2; end;\n";
        let result = format_overlay(&[("run.mod", input)], "run.mod", None);
        let text = result.replacement.unwrap().1;
        assert!(text.contains("\np = 1 + -2;\n"), "{text}");
        assert!(text.contains("\n  y = 1 - 2;\n"), "{text}");
        assert!(text.contains("\n  values 1 -2;\n"), "{text}");
    }

    #[test]
    fn keeps_indent_across_block_openers_and_closers_on_the_same_line() {
        let input = "var y;\nmodel;\ny=1;\nend; model; y=2; end; initval;\ny=3;\nend;\n";
        let result = format_overlay(&[("run.mod", input)], "run.mod", None);
        assert_eq!(result.withheld, 0);
        let (_, text) = result.replacement.unwrap();
        assert_eq!(
            text,
            "var y;\nmodel;\n  y = 1;\nend;\nmodel;\n  y = 2;\nend;\ninitval;\n  y = 3;\nend;\n"
        );
        let repeated = format_overlay(&[("run.mod", &text)], "run.mod", None);
        assert!(repeated.replacement.is_none());
    }

    #[test]
    fn splits_body_boundaries_with_zero_model_changes() {
        use crate::engine::comparison::compare_models;
        use crate::engine::language::parser::parse;

        let input = "var y; varexo e;\nparameters a; a=0.5;\nmodel; [name='transition'] y=a*y(-1)\n+e; end;\ninitval; y=0; end;\nendval; y=1; end;\nshocks; var e; stderr 0.1; end;\n";
        let result = format_overlay(&[("run.mod", input)], "run.mod", None);
        assert_eq!(result.withheld, 0);
        let (_, text) = result.replacement.unwrap();
        assert!(
            text.contains("  [name = 'transition'] y = a * y(-1)\n  + e;\nend;"),
            "{text}"
        );
        assert!(
            text.contains("shocks;\n  var e;\n  stderr 0.1;\nend;"),
            "{text}"
        );
        assert!(text.contains("endval;\n  y = 1;\nend;"), "{text}");
        let comparison = compare_models(&parse(input), &parse(&text));
        assert!(
            comparison.semantic.rows.is_empty(),
            "{:?}",
            comparison.semantic.rows
        );
        assert_ne!(input, text);
    }

    #[test]
    fn splits_verified_boundaries_across_dynare_block_readers() {
        let prefix =
            "var y; varexo e; parameters a; a=.5;\nmodel; [name='main'] y=a*y(-1)+e; end;\n";
        for (suffix, expected) in [
            (
                "steady_state_model; y=1; end; steady;",
                "\n  y = 1;\nend;\nsteady;",
            ),
            ("histval; y(-1)=1; end;", "\n  y(-1) = 1;\nend;"),
            (
                "model_replace('main'); [name='main'] y=2*e; end;",
                "\n  [name = 'main'] y = 2 * e;\nend;",
            ),
            (
                "shocks(surprise); var e; periods 1; values .1; end;",
                "\n  var e;\n  periods 1;\n  values .1;\nend;",
            ),
            (
                "mshocks; var e; periods 1:3; values .1; end;",
                "\n  var e;\n  periods 1:3;\n  values .1;\nend;",
            ),
            (
                "conditional_forecast_paths; var y; periods 1:3; values 1; end;",
                "\n  var y;\n  periods 1:3;\n  values 1;\nend;",
            ),
            (
                "occbin_constraints; name 'lower'; bind y<0; relax y>0; end;",
                "\n  name 'lower';\n  bind y < 0;\n  relax y > 0;\nend;",
            ),
            ("estimated_params; a,.5,0,1; end;", "\n  a, .5, 0, 1;\nend;"),
            (
                "estimated_params_init(use_calibration); a,.5; end;",
                "\n  a, .5;\nend;",
            ),
            ("estimated_params_bounds; a,0,1; end;", "\n  a, 0, 1;\nend;"),
            ("homotopy_setup; a,.1,.9; end;", "\n  a, .1, .9;\nend;"),
        ] {
            let input = format!("{prefix}{suffix}\n");
            let result = format_overlay(&[("run.mod", &input)], "run.mod", None);
            assert_eq!(result.withheld, 0, "{suffix}");
            let (_, formatted) = result.replacement.unwrap();
            assert!(formatted.contains(expected), "{suffix}\n{formatted}");
            let again = format_overlay(&[("run.mod", &formatted)], "run.mod", None);
            assert!(again.replacement.is_none(), "{suffix}");
        }
    }
}
