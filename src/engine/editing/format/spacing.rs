//! Horizontal spacing for already identified expression tokens.

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Binop,
    Unary,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Hash,
    Comma,
    Semi,
    Operand,
}

/// Return the desired gap before each token. Callers must retain original
/// line breaks and use this policy only for proven expression tokens.
pub(crate) fn gaps(tokens: &[&str]) -> Vec<&'static str> {
    let mut prev = None;
    tokens
        .iter()
        .map(|token| {
            let kind = classify(token, prev);
            let gap = separator(prev, kind);
            prev = Some(kind);
            gap
        })
        .collect()
}

fn classify(tok: &str, prev: Option<Kind>) -> Kind {
    if tok == "+" || tok == "-" {
        return match prev {
            None | Some(Kind::Binop) | Some(Kind::Unary) | Some(Kind::LParen)
            | Some(Kind::Comma) => Kind::Unary,
            _ => Kind::Binop,
        };
    }
    if matches!(
        tok,
        "*" | "/" | "^" | "=" | "<" | ">" | "<=" | ">=" | "==" | "!=" | "**"
    ) {
        return Kind::Binop;
    }
    match tok {
        "(" => Kind::LParen,
        ")" => Kind::RParen,
        "[" => Kind::LBracket,
        "]" => Kind::RBracket,
        "#" => Kind::Hash,
        "," => Kind::Comma,
        ";" => Kind::Semi,
        _ => Kind::Operand,
    }
}

fn separator(prev: Option<Kind>, kind: Kind) -> &'static str {
    let Some(prev) = prev else {
        return "";
    };
    if matches!(
        kind,
        Kind::RParen | Kind::RBracket | Kind::Comma | Kind::Semi
    ) {
        return "";
    }
    if matches!(
        prev,
        Kind::LParen | Kind::LBracket | Kind::Unary | Kind::Hash
    ) {
        return "";
    }
    if matches!(prev, Kind::Comma | Kind::Semi) {
        return " ";
    }
    if kind == Kind::LParen {
        return if prev == Kind::Binop { " " } else { "" };
    }
    if kind == Kind::Binop || prev == Kind::Binop {
        return " ";
    }
    " "
}

/// Collect edits for one owner. Written gaps with inconsistent copies are
/// withheld; callers intersect the resulting edits across owners.
pub(super) fn collect(
    source: &str,
    file: &str,
    owner: &super::layout::Owner<'_>,
    starts: &[usize],
    lines: &mut [super::SourceLine],
) {
    use crate::engine::language::lexer::TokenKind;
    use crate::engine::language::span::Span;
    use std::collections::{BTreeMap, HashSet};

    if owner.model.macro_incomplete() {
        return;
    }
    let mut unit_marks = vec![0i64; lines.len() + 1];
    let line_at = |offset| starts.partition_point(|&start| start <= offset) - 1;
    let mut edits = BTreeMap::<(usize, usize), Option<&'static str>>::new();
    let mut covered = vec![false; owner.model.expanded_tokens.len()];
    let model = owner.model;
    let expressions: HashSet<_> = model
        .written_equations
        .iter()
        .map(|eq| (eq.token_range.start, eq.token_range.end))
        .chain(
            model
                .param_assignments
                .iter()
                .chain(&model.initval)
                .chain(&model.endval)
                .filter(|assignment| !assignment.native)
                .map(|assignment| (assignment.active_tokens.start, assignment.active_tokens.end)),
        )
        .chain(
            model
                .histval
                .iter()
                .map(|entry| (entry.active_tokens.start, entry.active_tokens.end)),
        )
        .chain(
            model
                .steady_state_equations
                .iter()
                .map(|eq| (eq.active_tokens.start, eq.active_tokens.end)),
        )
        .collect();
    for range in super::accepted_units(owner.model)
        .map(|(range, _)| range)
        .chain(owner.model.format_options.iter().cloned())
    {
        if crate::engine::control::cancelled() {
            return;
        }
        let expression = expressions.contains(&(range.start, range.end))
            || range
                .end
                .checked_sub(1)
                .is_some_and(|end| expressions.contains(&(range.start, end)));
        let Some(tokens) = owner.model.expanded_tokens.get(range.clone()) else {
            continue;
        };
        // Retain physical line breaks inside a written unit even when a
        // generated token makes horizontal edits unsafe.
        if let (Some(first), Some(last)) = (tokens.first(), tokens.last())
            && first.span.start < last.span.end
        {
            owner.for_written_segments(
                file,
                Span {
                    start: first.span.start,
                    end: last.span.end,
                },
                |span| {
                    if !span.is_empty() && span.end as usize <= source.len() {
                        let first = line_at(span.start as usize);
                        let last = line_at((span.end - 1) as usize);
                        if first + 1 < last {
                            unit_marks[first + 1] += 1;
                            unit_marks[last] -= 1;
                        }
                    }
                },
            );
        }
        let spans: Option<Vec<_>> = tokens.iter().map(|t| owner.literal(file, t)).collect();
        let Some(spans) = spans else {
            continue;
        };
        if tokens.is_empty()
            || spans.iter().any(|s| {
                s.end as usize > source.len()
                    || s.start > s.end
                    || !source.is_char_boundary(s.start as usize)
                    || !source.is_char_boundary(s.end as usize)
            })
        {
            continue;
        }
        for span in &spans {
            let line = line_at(span.start as usize);
            lines[line].spacing.get_or_insert_with(Vec::new);
        }
        let words: Vec<&str> = spans
            .iter()
            .map(|s| &source[s.start as usize..s.end as usize])
            .collect();
        let desired = gaps(&words);
        let mut timing_gaps = vec![false; tokens.len()];
        for j in 1..tokens.len() {
            if tokens[j].kind != TokenKind::LParen || tokens[j - 1].kind != TokenKind::Ident {
                continue;
            }
            let n = if tokens
                .get(j + 1)
                .is_some_and(|t| matches!(t.kind, TokenKind::Plus | TokenKind::Minus))
            {
                j + 2
            } else {
                j + 1
            };
            if tokens.get(n).is_some_and(|t| t.kind == TokenKind::Number)
                && tokens
                    .get(n + 1)
                    .is_some_and(|t| t.kind == TokenKind::RParen)
            {
                for gap in &mut timing_gaps[j..=n + 1] {
                    *gap = true;
                }
            }
        }
        for i in 1..tokens.len() {
            let left = &tokens[i - 1];
            let right = &tokens[i];
            let a = spans[i - 1].end as usize;
            let b = spans[i].start as usize;
            if a > b || !source.is_char_boundary(a) || !source.is_char_boundary(b) {
                continue;
            }
            let gap = &source[a..b];
            if !gap.bytes().all(|c| matches!(c, b' ' | b'\t')) {
                continue;
            }
            // Space can separate list elements, while dots and interpolation
            // have grammar-specific adjacency. Leave those gaps untouched.
            let punctuation = |k| {
                matches!(
                    k,
                    TokenKind::Eq
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
                        | TokenKind::LBrack
                        | TokenKind::RBrack
                        | TokenKind::Hash
                )
            };
            if !punctuation(left.kind) && !punctuation(right.kind) {
                continue;
            }
            // In a non-expression row a sign or parenthesis can begin the
            // next whitespace-separated list element.
            if !expression
                && (matches!(left.kind, TokenKind::Plus | TokenKind::Minus)
                    || matches!(right.kind, TokenKind::Plus | TokenKind::Minus))
            {
                continue;
            }
            if !expression && right.kind == TokenKind::LParen && !gap.is_empty() {
                continue;
            }
            // Preserve every gap in a written signed or unsigned timing suffix.
            if timing_gaps[i] || left.kind == TokenKind::Star && right.kind == TokenKind::Star {
                continue;
            }
            let replacement = if left.kind == TokenKind::Latex && right.kind == TokenKind::LParen {
                " "
            } else {
                desired[i]
            };
            covered[range.start + i] = true;
            edits
                .entry((a, b))
                .and_modify(|prior| {
                    if *prior != Some(replacement) {
                        *prior = None;
                    }
                })
                .or_insert(Some(replacement));
        }
    }
    // A second execution of the same written gap may lack a matching parser
    // receipt or carry a role for which spacing is not safe.
    for (i, pair) in owner.model.expanded_tokens.windows(2).enumerate() {
        if crate::engine::control::cancelled() {
            return;
        }
        if covered[i + 1] {
            continue;
        }
        let (Some(left), Some(right)) =
            (owner.literal(file, &pair[0]), owner.literal(file, &pair[1]))
        else {
            continue;
        };
        let key = (left.end as usize, right.start as usize);
        if let Some(edit) = edits.get_mut(&key) {
            *edit = None;
        }
    }
    for ((a, b), replacement) in edits {
        if let Some(replacement) = replacement
            && &source[a..b] != replacement
        {
            let line = line_at(a);
            lines[line].spacing.get_or_insert_with(Vec::new).push((
                a - starts[line],
                b - starts[line],
                replacement,
            ));
        }
    }
    let mut units = 0;
    for (line, mark) in lines.iter_mut().zip(unit_marks) {
        units += mark;
        line.keep_blank = units > 0;
        if let Some(edits) = &mut line.spacing {
            edits.sort_unstable_by_key(|edit| (edit.0, edit.1));
            edits.dedup();
        }
    }
}
