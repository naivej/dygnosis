//! Source-layout editor display copy. Keeps written spaces and line breaks.

use crate::expand::ExpandReport;
use crate::lexer::{tokenize, Token, TokenKind};
use crate::macro_expand::{expand_macros_with_source_layout, SourceFragment};
use crate::model::Model;
use crate::parser::join_lexemes_recorded;
use crate::span::Span;

#[derive(Debug)]
pub(crate) struct SourcePreview {
    pub text: String,
    pub ranges: Vec<Span>,
    /// False when active tokens disagree with the stored expansion.
    pub proven: bool,
    /// Private until slice 03 publishes `source_navigation`.
    ///
    /// Each fragment is contiguous display text with a written span in the
    /// expander source (include-spliced), a copy/substitution tag, and whether
    /// it was emitted under an active macro frame. Slice 03 maps these through
    /// include segments and coalesces identifier regions.
    #[allow(dead_code)]
    pub(crate) fragments: Vec<SourceFragment>,
}

/// Build the source-layout preview. `None` only when the stored expansion
/// cannot be reproduced from `model.source`.
pub(crate) fn source(report: &ExpandReport, model: &Model) -> Option<SourcePreview> {
    let original = &report.effective_text;
    let (tokens, _, _, layout) =
        expand_macros_with_source_layout(&model.source, tokenize(&model.source));
    let copy = join_lexemes_recorded(&model.source, &tokens, |_, _| {});
    if copy != *original {
        return None;
    }
    let proven = token_stream_agrees(&layout.text, &model.source, &tokens);
    let ranges = if proven {
        map_navigation(report, &layout.text, &model.source, &tokens).unwrap_or_default()
    } else {
        Vec::new()
    };
    let navigation_ok = ranges.len() == report.navigation.len();
    Some(SourcePreview {
        text: layout.text,
        ranges,
        proven: proven && navigation_ok,
        fragments: layout.fragments,
    })
}

fn token_stream_agrees(display: &str, src: &str, expanded: &[Token]) -> bool {
    let proof = tokenize(display);
    let mut proof = proof.iter().filter(|token| token.kind != TokenKind::Eof);
    for token in expanded.iter().filter(|token| token.kind != TokenKind::Eof) {
        let Some(got) = proof.next() else {
            return false;
        };
        if got.kind != token.kind || got.text(display) != token.text(src) {
            return false;
        }
    }
    proof.next().is_none()
}

fn map_navigation(
    report: &ExpandReport,
    display: &str,
    src: &str,
    expanded: &[Token],
) -> Option<Vec<Span>> {
    let proof = tokenize(display);
    let proof: Vec<_> = proof
        .iter()
        .filter(|token| token.kind != TokenKind::Eof)
        .collect();
    let mut display_of = vec![None; expanded.len()];
    let mut next = 0usize;
    for (index, token) in expanded.iter().enumerate() {
        if token.kind == TokenKind::Eof {
            continue;
        }
        display_of[index] = Some(proof.get(next)?.span);
        next += 1;
    }
    if next != proof.len() {
        return None;
    }
    let mut join_of = vec![None; expanded.len()];
    let _ = join_lexemes_recorded(src, expanded, |index, span| {
        join_of[index] = Some(span);
    });
    let map = |span: Span| -> Option<Span> {
        let start = join_of
            .iter()
            .position(|join| join.is_some_and(|j| j.start == span.start))?;
        let end = join_of
            .iter()
            .position(|join| join.is_some_and(|j| j.end == span.end))?;
        Some(Span {
            start: display_of[start]?.start,
            end: display_of[end]?.end,
        })
    };
    report
        .navigation
        .iter()
        .map(|row| map(row.effective_span))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expand::expand_report;
    use crate::parser::parse;

    fn preview(text: &str) -> SourcePreview {
        let report = expand_report(text);
        let model = parse(text);
        source(&report, &model).expect("source layout")
    }

    #[test]
    fn keeps_compact_assignment_spacing() {
        let layout = preview("var y;\nmodel;\ny=c;\nend;\n");
        assert!(layout.proven, "{layout:?}");
        assert!(layout.text.contains("y=c;"), "{}", layout.text);
        assert!(!layout.text.contains("y = c"), "{}", layout.text);
    }

    #[test]
    fn keeps_tabs_blanks_trailing_spaces_and_comments() {
        let src = "var y;\nmodel;\n\t// keep\ny=c;   \n\nend;\n";
        let layout = preview(src);
        assert!(layout.proven, "{}", layout.text);
        assert!(layout.text.contains("\t// keep\n"), "{}", layout.text);
        assert!(layout.text.contains("y=c;   \n\nend;"), "{}", layout.text);
    }

    #[test]
    fn substitutes_only_interpolation_inside_quotes() {
        let src = "@#for j in 1:2\nparameters beta_@{j};\n@#endfor\nvar y;\nmodel;\n[name='eq@{j}'] y=1;\nend;\n";
        // j remains from the loop; last value is 2
        let layout = preview(src);
        assert!(layout.proven, "{}", layout.text);
        assert!(layout.text.contains("parameters beta_1;"), "{}", layout.text);
        assert!(layout.text.contains("parameters beta_2;"), "{}", layout.text);
        assert!(!layout.text.contains("@#"), "{}", layout.text);
    }

    #[test]
    fn discards_inactive_branch_text() {
        let src = "var y;\n@#if 0\nbad=1;\n@#else\ny=c;\n@#endif\nmodel;\ny=1;\nend;\n";
        let layout = preview(src);
        assert!(layout.proven, "{}", layout.text);
        assert!(layout.text.contains("y=c;"), "{}", layout.text);
        assert!(!layout.text.contains("bad"), "{}", layout.text);
        assert!(!layout.text.contains("@#"), "{}", layout.text);
    }

    #[test]
    fn normalizes_crlf_to_lf() {
        let layout = preview("var y;\r\nmodel;\r\ny=c;\r\nend;\r\n");
        assert!(!layout.text.contains('\r'), "{:?}", layout.text.as_bytes());
        assert!(layout.text.contains("y=c;"), "{}", layout.text);
        assert_eq!(layout.text, preview("var y;\nmodel;\ny=c;\nend;\n").text);
        assert!(layout.proven, "{layout:?}");
    }

    #[test]
    fn preserves_quoted_native_and_matrix_text() {
        let src = "var y; verbatim; A=[1 2;3 4]; fprintf('a;b😀'); end; model; y=1; end;";
        let layout = preview(src);
        assert!(layout.proven, "{}", layout.text);
        assert!(layout.text.contains("A=[1 2;3 4]"), "{}", layout.text);
        assert!(layout.text.contains("fprintf('a;b😀')"), "{}", layout.text);
    }
}
