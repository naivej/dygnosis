//! Inactive written-source ranges for model information.

use tower_lsp::lsp_types::Range;

use crate::engine::language::lexer::{Token, TokenKind};
use crate::engine::language::span::Span;

use super::Url;
use super::{Inner, span_range};

/// Project exact inactive intervals from the selected root's retained macro
/// execution. This reads the worker's availability summary and does not build
/// a written inventory, parse source, or expand macros on the request path.
pub(super) fn requested_document_ranges(inner: &Inner, root: &Url, document: &Url) -> Vec<Range> {
    let document_key = crate::engine::workspace::include_resolver::normalize_uri(document.as_str());
    let Some(source) = inner
        .workspace
        .source_document(root.as_str(), &document_key)
    else {
        return Vec::new();
    };
    let text = source.source();
    let index = source.source_index();
    let mut inactive = source.inactive_spans().peekable();
    if inactive.peek().is_none() {
        return Vec::new();
    }
    // Execution already prepared this document's macro tokens. Use only the
    // cached scan; a missing cache cannot justify a new scan on ModelInfo.
    let Some(macro_source) = source.cached_macro_source() else {
        return Vec::new();
    };
    project_without_directives(
        inactive,
        macro_source
            .tokens
            .iter()
            .filter(|token| token.kind == TokenKind::MacroDir),
        &index,
        text,
    )
}

fn project_without_directives<'a>(
    inactive: impl Iterator<Item = Span>,
    directives: impl Iterator<Item = &'a Token>,
    index: &crate::engine::language::span::LineIndex,
    text: &str,
) -> Vec<Range> {
    let mut directives = directives.peekable();
    let mut output = Vec::new();
    for span in inactive {
        let mut cursor = span.start;
        while directives
            .peek()
            .is_some_and(|directive| directive.span.end <= cursor)
        {
            directives.next();
        }
        while let Some(directive) = directives.peek() {
            if directive.span.start >= span.end {
                break;
            }
            let directive_span = directive.span;
            if directive_span.end <= span.end {
                let _ = directives.next();
            }
            if directive_span.start > cursor {
                let end = directive_span.start.min(span.end);
                push_range(
                    &mut output,
                    index,
                    text,
                    Span::new(cursor as usize, end as usize),
                );
            }
            cursor = cursor.max(directive_span.end.min(span.end));
            if cursor >= span.end {
                break;
            }
        }
        if cursor < span.end {
            push_range(
                &mut output,
                index,
                text,
                Span::new(cursor as usize, span.end as usize),
            );
        }
    }
    output
}

fn push_range(
    output: &mut Vec<Range>,
    index: &crate::engine::language::span::LineIndex,
    text: &str,
    span: Span,
) {
    if span.start < span.end {
        output.push(span_range(index, text, span));
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::*;
    use crate::engine::language::span::LineIndex;
    use tower_lsp::lsp_types::Position;

    fn projected_spans(text: &str, ranges: &[Range]) -> Vec<Span> {
        let index = LineIndex::new(text);
        ranges
            .iter()
            .map(|range| {
                Span::new(
                    index.offset_utf16(
                        text,
                        crate::engine::language::span::Position {
                            line: range.start.line,
                            character: range.start.character,
                        },
                    ) as usize,
                    index.offset_utf16(
                        text,
                        crate::engine::language::span::Position {
                            line: range.end.line,
                            character: range.end.character,
                        },
                    ) as usize,
                )
            })
            .collect()
    }

    fn assert_directives_are_not_projected(text: &str, spans: &[Span]) {
        let directives = crate::engine::language::macro_expand::scan_macro_tokens(text);
        for directive in directives
            .iter()
            .filter(|token| token.kind == TokenKind::MacroDir)
        {
            assert!(
                spans.iter().all(|span| {
                    span.end <= directive.span.start || directive.span.end <= span.start
                }),
                "macro directive bytes were projected: {:?}",
                directive.span
            );
        }
    }

    #[test]
    fn interval_projection_uses_utf16_coordinates() {
        let text = "😀 hidden\n";
        let index = LineIndex::new(text);
        let hidden_start = text.find("hidden").unwrap();
        let range = span_range(
            &index,
            text,
            Span::new(hidden_start, hidden_start + "hidden".len()),
        );
        assert_eq!(range, Range::new(Position::new(0, 3), Position::new(0, 9)));
    }

    #[test]
    fn projection_removes_macro_directive_bytes_from_inactive_ranges() {
        let text = "@#if 0\nhidden\n@#else\nlive\n@#endif\n";
        let tokens = crate::engine::language::macro_expand::scan_macro_tokens(text);
        let index = LineIndex::new(text);
        let projected = project_without_directives(
            [Span::new(6, text.trim_end().len())].into_iter(),
            tokens
                .iter()
                .filter(|token| token.kind == TokenKind::MacroDir),
            &index,
            text,
        );
        assert_eq!(
            projected,
            [
                Range::new(Position::new(0, 6), Position::new(2, 0)),
                Range::new(Position::new(2, 6), Position::new(4, 0)),
            ]
        );
        assert_directives_are_not_projected(text, &projected_spans(text, &projected));
    }

    #[test]
    fn directive_crossing_disjoint_inactive_intervals_is_not_consumed_early() {
        let text = "0123456789";
        let index = LineIndex::new(text);
        let directive = Token::new(TokenKind::MacroDir, Span::new(2, 7));
        let projected = project_without_directives(
            [Span::new(0, 3), Span::new(5, 10)].into_iter(),
            [&directive].into_iter(),
            &index,
            text,
        );
        assert_eq!(
            projected,
            [
                Range::new(Position::new(0, 0), Position::new(0, 2)),
                Range::new(Position::new(0, 7), Position::new(0, 10)),
            ]
        );
    }

    #[tokio::test]
    async fn model_info_tracks_selected_branch_and_withholds_stale_or_unknown_ranges() {
        let uri = Url::parse("untitled:inactive-ranges.mod").unwrap();
        let source = concat!(
            "@#if 0\n",
            "var hidden; // 😀\n",
            "@#else\n",
            "var visible;\n",
            "@#endif\n",
            "var y;\n",
            "model;\n",
            "y = 0;\n",
            "end;\n",
        );
        let (service, _socket) = super::super::new_service();
        let server = service.inner();
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "dynare".into(),
                    version: 1,
                    text: source.into(),
                },
            })
            .await;
        server.wait_document_analysis(Some(&uri)).await;
        server.lock_inner().analysis_configuration.enabled = true;

        let initial = server.model_info_command(&[json!({"root_uri":uri})]);
        let first_ranges = initial["inactive_ranges"].as_array().unwrap();
        assert_eq!(first_ranges.len(), 1, "{initial}");
        assert_eq!(first_ranges[0]["start"], json!({"line":0,"character":6}));
        assert_eq!(first_ranges[0]["end"], json!({"line":2,"character":0}));

        let context = server.analysis_context_command(&[json!({"root_uri":uri})]);
        let conditional = context["sites"]
            .as_array()
            .unwrap()
            .iter()
            .find(|site| site["kind"] == "conditional")
            .unwrap();
        let accepted = server
            .set_analysis_configuration(&[json!({
                "root_uri":uri,
                "document_uri":uri,
                "document_version":1,
                "expected_revision":context["revision"],
                "expected_selection_generation":context["analysis"]["selection_generation"],
                "include_paths":null,
                "analysis":{"overrides":[{
                    "kind":"branch",
                    "target":conditional["target"],
                    "branch_id":conditional["branches"][0]["branch_id"]
                }]}
            })])
            .await;
        assert_eq!(accepted["accepted"], true, "{accepted}");

        let selected = server.model_info_command(&[json!({"root_uri":uri})]);
        let selected_ranges = selected["inactive_ranges"].as_array().unwrap();
        assert_eq!(selected_ranges.len(), 1, "{selected}");
        assert_eq!(selected_ranges[0]["start"], json!({"line":2,"character":6}));
        assert_eq!(selected_ranges[0]["end"], json!({"line":4,"character":0}));

        server
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 2,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: source.replace("var visible", "var visible2"),
                }],
            })
            .await;
        server.wait_document_analysis(Some(&uri)).await;
        let stale = server.model_info_command(&[json!({"root_uri":uri})]);
        assert_eq!(stale["status"], "stale", "{stale}");
        assert_eq!(stale["inactive_ranges"], json!([]));
    }

    #[tokio::test]
    async fn model_info_does_not_gray_unknown_macro_availability() {
        let uri = Url::parse("untitled:unknown-inactive-ranges.mod").unwrap();
        let source = "@#if MISSING\nvar uncertain;\n@#else\nvar visible;\n@#endif\nvar y; model; y=0; end;\n";
        let (service, _socket) = super::super::new_service();
        let server = service.inner();
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "dynare".into(),
                    version: 1,
                    text: source.into(),
                },
            })
            .await;
        server.wait_document_analysis(Some(&uri)).await;
        let result = server.model_info_command(&[json!({"root_uri":uri})]);
        assert_eq!(result["inactive_ranges"], json!([]), "{result}");
    }

    #[tokio::test]
    async fn repeated_include_loop_withholds_mixed_arms_but_projects_consistent_inactive_body() {
        struct Temporary(std::path::PathBuf);
        impl Drop for Temporary {
            fn drop(&mut self) {
                if let (Ok(parent), Ok(target)) =
                    (std::env::temp_dir().canonicalize(), self.0.canonicalize())
                    && target.parent() == Some(parent.as_path())
                    && target
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("dygnosis-activity-"))
                {
                    let _ = std::fs::remove_dir_all(target);
                }
            }
        }
        let directory = Temporary(std::env::temp_dir().join(format!(
            "dygnosis-activity-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
        std::fs::create_dir_all(&directory.0).unwrap();
        let root = Url::from_file_path(directory.0.join("main.mod")).unwrap();
        let included = Url::from_file_path(directory.0.join("body.inc")).unwrap();
        let root_text = concat!(
            "@#for i in 1:2\n",
            "@#include \"body.inc\"\n",
            "@#endfor\n",
            "var y; model; y=0; end;\n",
        );
        let body_text = concat!(
            "@#if i == 1\n",
            "var first_arm;\n",
            "@#else\n",
            "var second_arm;\n",
            "@#endif\n",
            "@#if 0\n",
            "var always_hidden;\n",
            "@#endif\n",
        );
        std::fs::write(directory.0.join("main.mod"), root_text).unwrap();
        std::fs::write(directory.0.join("body.inc"), body_text).unwrap();

        let (service, _socket) = super::super::new_service();
        let server = service.inner();
        server.upsert(root.clone(), root_text.into(), 1);
        server.upsert(included.clone(), body_text.into(), 1);
        server.wait_document_analysis(Some(&root)).await;

        let result = server.model_info_command(&[json!({
            "root_uri":root,
            "document_uri":included
        })]);
        let ranges = result["inactive_ranges"]
            .as_array()
            .unwrap()
            .iter()
            .cloned()
            .map(serde_json::from_value::<Range>)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        let projected = projected_spans(body_text, &ranges);
        let hidden = body_text.find("always_hidden").unwrap();
        let first = body_text.find("first_arm").unwrap();
        let second = body_text.find("second_arm").unwrap();
        assert!(
            projected
                .iter()
                .any(|span| span.start as usize <= hidden && hidden < span.end as usize),
            "{ranges:?}"
        );
        assert!(
            projected
                .iter()
                .all(|span| span.end as usize <= first || first < span.start as usize),
            "{ranges:?}"
        );
        assert!(
            projected
                .iter()
                .all(|span| span.end as usize <= second || second < span.start as usize),
            "{ranges:?}"
        );
        assert_directives_are_not_projected(body_text, &projected);
    }
}
