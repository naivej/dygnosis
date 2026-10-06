//! Source-layout editor display copy. Keeps written spaces and line breaks.

use crate::expand::ExpandReport;
use crate::lexer::{tokenize, Token, TokenKind};
use crate::macro_expand::{expand_macros_with_source_layout, SourceFragment, SourceLayoutGap};
use crate::model::Model;
use crate::parser::join_lexemes_recorded;
use crate::span::Span;

#[derive(Debug)]
pub(crate) struct SourcePreview {
    pub text: String,
    pub ranges: Vec<Span>,
    /// False when active tokens disagree with the stored expansion.
    pub proven: bool,
    /// Contiguous display pieces with written spans in include-spliced source.
    pub(crate) fragments: Vec<SourceFragment>,
}

/// Build the source-layout preview. `None` only when the stored expansion
/// cannot be reproduced from `model.source`.
///
/// `gaps` comes from the include splice: leftover directive indent and line
/// endings that must not appear as written display text.
pub(crate) fn source(
    report: &ExpandReport,
    model: &Model,
    gaps: &[SourceLayoutGap],
) -> Option<SourcePreview> {
    let original = &report.effective_text;
    let (tokens, _, _, layout) =
        expand_macros_with_source_layout(&model.source, tokenize(&model.source), gaps);
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
    use crate::workspace::Workspace;

    fn preview(text: &str) -> SourcePreview {
        let report = expand_report(text);
        let model = parse(text);
        source(&report, &model, &[]).expect("source layout")
    }

    fn preview_with_includes(root: &str, root_text: &str, files: &[(&str, &str)]) -> SourcePreview {
        let mut ws = Workspace::default();
        for (path, text) in files {
            ws.update_document(path, *text);
        }
        ws.update_document(root, root_text);
        let model = ws.get_effective_model(root).unwrap().clone();
        let report = ws.expand_report(root).unwrap().clone();
        let gaps = ws.source_layout_gaps(root);
        source(&report, &model, &gaps).expect("source layout")
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
    fn quoted_interpolation_keeps_surrounding_quotes() {
        let layout = preview("@#define j=2\nvar y;\nmodel;\n[name='eq@{j}'] y=1;\nend;\n");
        assert!(layout.proven, "{}", layout.text);
        assert!(
            layout.text.contains("name='eq2'"),
            "expected quoted name, got {}",
            layout.text
        );
        assert!(!layout.text.contains("@{"), "{}", layout.text);
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

    #[test]
    fn empty_for_leaves_no_directive_line() {
        let layout = preview("var y;\nmodel;\n@#for i in []\ny=1;\n@#endfor\nend;\n");
        assert_eq!(layout.text, "var y;\nmodel;\nend;\n");
        assert!(layout.proven, "{layout:?}");
    }

    #[test]
    fn nested_for_repeats_written_body_spacing() {
        let layout = preview("var y;\nmodel;\n@#for i in 1:2\n  y=@{i};\n@#endfor\nend;\n");
        assert_eq!(layout.text, "var y;\nmodel;\n  y=1;\n  y=2;\nend;\n");
        assert!(layout.proven, "{layout:?}");
    }

    #[test]
    fn indented_active_include_drops_directive_indent() {
        let layout = preview_with_includes(
            "C:/probe/root.mod",
            "model;\n  @#include \"inner.mod\"\nend;\n",
            &[("C:/probe/inner.mod", "y=1;\n")],
        );
        assert_eq!(layout.text, "model;\ny=1;\nend;\n");
        assert!(layout.proven, "{layout:?}");
    }

    #[test]
    fn include_without_final_newline_keeps_unwritten_separator() {
        let layout = preview_with_includes(
            "C:/probe/root.mod",
            "model;\n@#include \"inner.mod\"\nend;\n",
            &[("C:/probe/inner.mod", "y=1;")],
        );
        assert_eq!(layout.text, "model;\ny=1;\nend;\n");
        assert!(layout.proven, "{layout:?}");
        let separator = layout
            .fragments
            .iter()
            .find(|fragment| {
                fragment.written.is_none()
                    && &layout.text
                        [fragment.display.start as usize..fragment.display.end as usize]
                        == "\n"
            })
            .expect("boundary newline fragment");
        assert_eq!(
            &layout.text[separator.display.start as usize..separator.display.end as usize],
            "\n"
        );
        // The unwritten separator sits between the include body and `end`.
        assert!(layout.text[..separator.display.start as usize].ends_with("y=1;"));
        assert!(layout.text[separator.display.end as usize..].starts_with("end;"));
    }

    #[test]
    fn dormant_include_contributes_no_text() {
        let layout = preview_with_includes(
            "C:/probe/root.mod",
            "var y;\n@#if 0\n  @#include \"inner.mod\"\n@#endif\nmodel;\ny=1;\nend;\n",
            &[("C:/probe/inner.mod", "  leaked=1;\n")],
        );
        assert_eq!(layout.text, "var y;\nmodel;\ny=1;\nend;\n");
        assert!(!layout.text.contains("leaked"), "{}", layout.text);
        assert!(layout.proven, "{layout:?}");
    }

    #[test]
    fn included_file_leading_spaces_stay() {
        let layout = preview_with_includes(
            "C:/probe/root.mod",
            "model;\n  @#include \"inner.mod\"\nend;\n",
            &[("C:/probe/inner.mod", "  y=1;\n")],
        );
        assert_eq!(layout.text, "model;\n  y=1;\nend;\n");
        assert!(layout.proven, "{layout:?}");
    }

    #[test]
    fn fewer_ranges_than_rows_keep_preview_unproven() {
        let text = "var y;\nmodel;\ny=1;\nend;\n";
        let mut report = expand_report(text);
        let model = parse(text);
        let ok = source(&report, &model, &[]).expect("source layout");
        assert!(!report.navigation.is_empty());
        assert!(ok.proven, "{ok:?}");
        assert_eq!(ok.ranges.len(), report.navigation.len());
        // Unmappable effective spans make map_navigation fail; source() then
        // keeps an empty range list and clears proven so showEffectiveModel
        // returns incomplete with empty navigation instead of indexing past it.
        report.navigation[0].effective_span = Span::new(usize::MAX / 2, usize::MAX / 2 + 1);
        let short = source(&report, &model, &[]).expect("source layout");
        assert!(
            short.ranges.len() < report.navigation.len(),
            "{:?} vs {}",
            short.ranges.len(),
            report.navigation.len()
        );
        assert!(!short.proven, "{short:?}");
    }
}
