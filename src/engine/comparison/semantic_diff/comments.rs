//! Comment boundaries for comparison display; parsing and captured text stay unchanged.

use std::ops::Range;

pub(super) enum CodeRange {
    Tokens(Range<usize>),
    Literal(Range<usize>),
}

fn line_end(text: &str, start: usize) -> usize {
    text[start..]
        .find('\n')
        .map_or(text.len(), |end| start + end)
}

fn transpose(bytes: &[u8], position: usize) -> bool {
    position.checked_sub(1).is_some_and(|previous| {
        let byte = bytes[previous];
        byte.is_ascii_alphanumeric()
            || byte >= 0x80
            || matches!(byte, b'_' | b')' | b']' | b'}' | b'\'' | b'"' | b'.')
    })
}

fn quoted_end(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut position = start + 1;
    while position < bytes.len() {
        if bytes[position] == quote {
            position += 1;
            if quote != b'$' && bytes.get(position) == Some(&quote) {
                position += 1;
            } else {
                break;
            }
        } else {
            position += 1;
        }
    }
    position
}

/// Macro directive arguments accept trailing // comments and double-quoted strings.
pub(super) fn macro_argument(text: &str) -> &str {
    let bytes = text.as_bytes();
    let mut position = 0;
    while position < bytes.len() {
        if bytes[position..].starts_with(b"//") {
            return text[..position].trim_end();
        }
        position = if bytes[position] == b'"' {
            quoted_end(bytes, position)
        } else {
            position + 1
        };
    }
    text
}

/// Keep original byte offsets and strings without allocating a masked source copy.
pub(super) fn code_ranges(text: &str) -> impl Iterator<Item = CodeRange> + '_ {
    let bytes = text.as_bytes();
    let (mut position, mut start) = (0, 0);
    let mut pending_literal = None;
    std::iter::from_fn(move || {
        if let Some(literal) = pending_literal.take() {
            return Some(CodeRange::Literal(literal));
        }
        while position < bytes.len() {
            let tail = &bytes[position..];
            let comment = if tail.starts_with(b"/*") {
                Some((
                    position,
                    text[position + 2..]
                        .find("*/")
                        .map_or(text.len(), |end| position + 2 + end + 2),
                ))
            } else if tail.starts_with(b"//") || bytes[position] == b'%' {
                // Model cards contain Dynare text: %{ and %} each comment one line.
                Some((position, line_end(text, position)))
            } else if tail.starts_with(b"...") {
                Some((position + 3, line_end(text, position)))
            } else {
                None
            };
            if let Some((comment_start, end)) = comment {
                let code = start..comment_start;
                (position, start) = (end, end);
                if !code.is_empty() {
                    return Some(CodeRange::Tokens(code));
                }
            } else if matches!(bytes[position], b'"' | b'$')
                || bytes[position] == b'\'' && !transpose(bytes, position)
            {
                let code = start..position;
                let literal = position..quoted_end(bytes, position);
                (position, start) = (literal.end, literal.end);
                if code.is_empty() {
                    return Some(CodeRange::Literal(literal));
                }
                pending_literal = Some(literal);
                return Some(CodeRange::Tokens(code));
            } else {
                position += 1;
            }
        }
        let code = start..text.len();
        start = text.len();
        (!code.is_empty()).then_some(CodeRange::Tokens(code))
    })
}
