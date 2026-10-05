use std::fs;
use std::path::PathBuf;

use dygnosis::server::new_service;
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "dygnosis-origin-lsp-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn uri(path: &std::path::Path) -> Url {
    Url::from_file_path(path).unwrap()
}

fn open(uri: Url, text: &str) -> DidOpenTextDocumentParams {
    DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri,
            language_id: "dynare".into(),
            version: 1,
            text: text.into(),
        },
    }
}

fn pull(uri: Url) -> DocumentDiagnosticParams {
    DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier { uri },
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

fn items(result: DocumentDiagnosticReportResult) -> Vec<Diagnostic> {
    match result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
            full.full_document_diagnostic_report.items
        }
        other => panic!("unexpected report {other:?}"),
    }
}

fn has_code(items: &[Diagnostic], code: &str) -> bool {
    items
        .iter()
        .any(|item| item.code == Some(NumberOrString::String(code.into())))
}

fn count_code(items: &[Diagnostic], code: &str) -> usize {
    items
        .iter()
        .filter(|item| item.code == Some(NumberOrString::String(code.into())))
        .count()
}

#[tokio::test]
async fn block_openers_match_mcp_ownership_with_utf16_and_split_includes() {
    let dir = scratch("block-openers");
    let root = dir.join("root.mod");
    let opener = dir.join("opener.inc");
    let body = dir.join("body.inc");
    let root_text = "var x y z;\n@#include \"opener.inc\"\n";
    let opener_text =
        "/*😀中*/model;\n@#include \"body.inc\"\nend;\n/*😀中*/steady_state_model; x=0; end;\n";
    let body_text = "#p = 1;\nx = y(-1);\ny = x;\n";
    for (file, text) in [
        (&root, root_text),
        (&opener, opener_text),
        (&body, body_text),
    ] {
        fs::write(file, text).unwrap();
    }
    let files = std::collections::HashMap::from([
        (root.to_string_lossy().to_string(), root_text.to_string()),
        (
            opener.to_string_lossy().to_string(),
            opener_text.to_string(),
        ),
        (body.to_string_lossy().to_string(), body_text.to_string()),
    ]);
    let mcp = dygnosis::dynare_diagnose(root_text, Some(&root.to_string_lossy()), Some(&files));
    let (service, _socket) = new_service();
    service.inner().did_open(open(uri(&root), root_text)).await;
    let root_rows = items(service.inner().diagnostic(pull(uri(&root))).await.unwrap());
    let opener_rows = items(
        service
            .inner()
            .diagnostic(pull(uri(&opener)))
            .await
            .unwrap(),
    );
    for (code, keyword, line, count) in [
        ("W013", "model", 0, 1),
        ("W042", "steady_state_model", 3, 2),
    ] {
        assert!(!has_code(&root_rows, code));
        let wire: Vec<_> = mcp
            .iter()
            .filter(|diagnostic| diagnostic.code == code)
            .collect();
        let lsp: Vec<_> = opener_rows
            .iter()
            .filter(|diagnostic| diagnostic.code == Some(NumberOrString::String(code.into())))
            .collect();
        assert_eq!(wire.len(), count);
        assert_eq!(lsp.len(), count);
        for diagnostic in wire {
            assert_eq!(
                diagnostic.file.as_deref(),
                Some(opener.to_string_lossy().as_ref())
            );
            assert_eq!(
                (
                    diagnostic.line,
                    diagnostic.column,
                    diagnostic.end_line,
                    diagnostic.end_column
                ),
                (line + 1, 7, line + 1, 7 + keyword.len() as u32)
            );
        }
        for diagnostic in lsp {
            assert_eq!(
                diagnostic.range,
                Range::new(
                    Position::new(line, 7),
                    Position::new(line, 7 + keyword.len() as u32)
                )
            );
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn first_child_openers_match_mcp_owners_when_the_child_is_in_an_include() {
    let dir = scratch("first-child-openers");
    let declaration = "var y;\n/*😀中*/varexo_det\n@#include \"det.inc\"\n;\nmodel; y=y(-1)+tau; end; initval; y=0; tau=0; end; simul;\n";
    let occbin = include_str!("fixtures/occbin/e171_three.mod").replace("\r\n", "\n");
    let (before, rest) = occbin.split_once("occbin_constraints;\n").unwrap();
    let (constraints, after) = rest.split_once("end;").unwrap();
    let occbin =
        format!("{before}/*😀中*/occbin_constraints;\n@#include \"constraints.inc\"\nend;{after}");
    let cases = [
        ("a.mod", declaration.to_string(), "E026", "varexo_det"),
        (
            "b.mod",
            declaration.replace(
                "simul;",
                "planner_objective y; ramsey_model(instruments=(y));",
            ),
            "E027",
            "varexo_det",
        ),
        ("occbin.mod", occbin, "E171", "occbin_constraints"),
        (
            "tags.mod",
            "var y;\n/*😀中*/model; y=y(-1); end;\nmodel;\n@#include \"tags.inc\"\nend;\n"
                .to_string(),
            "E208",
            "model",
        ),
    ];
    for (name, text) in [
        ("det.inc", "/*😀中*/tau"),
        ("constraints.inc", constraints),
        ("tags.inc", "/*😀中*/[dynamic] y=y(-1);\n"),
    ] {
        fs::write(dir.join(name), text).unwrap();
    }
    for (name, text, code, keyword) in cases {
        let root = dir.join(name);
        fs::write(&root, &text).unwrap();
        let mut files =
            std::collections::HashMap::from([(root.to_string_lossy().to_string(), text.clone())]);
        for name in ["det.inc", "constraints.inc", "tags.inc"] {
            let child = dir.join(name);
            files.insert(
                child.to_string_lossy().to_string(),
                fs::read_to_string(child).unwrap(),
            );
        }
        let mcp = dygnosis::dynare_diagnose(&text, Some(&root.to_string_lossy()), Some(&files));
        let wire = mcp
            .iter()
            .find(|diagnostic| diagnostic.code == code)
            .unwrap();
        assert_eq!(wire.file, None, "{code} belongs to the root opener");
        let start =
            text.find(&format!("/*😀中*/{keyword}")).unwrap() as u32 + "/*😀中*/".len() as u32;
        let end = start + keyword.len() as u32;
        let index = dygnosis::span::LineIndex::new(&text);
        let scalar_start = index.position(&text, start);
        let scalar_end = index.position(&text, end);
        assert_eq!(
            (wire.line, wire.column, wire.end_line, wire.end_column),
            (
                scalar_start.line + 1,
                scalar_start.character + 1,
                scalar_end.line + 1,
                scalar_end.character + 1
            )
        );
        let (service, _socket) = new_service();
        service.inner().did_open(open(uri(&root), &text)).await;
        let lsp = items(service.inner().diagnostic(pull(uri(&root))).await.unwrap());
        let diagnostic = lsp
            .iter()
            .find(|diagnostic| diagnostic.code == Some(NumberOrString::String(code.into())))
            .unwrap();
        let utf16_start = index.position_utf16(&text, start);
        let utf16_end = index.position_utf16(&text, end);
        assert_eq!(
            diagnostic.range,
            Range::new(
                Position::new(utf16_start.line, utf16_start.character),
                Position::new(utf16_end.line, utf16_end.character)
            )
        );
    }
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn included_error_is_published_on_child_uri() {
    let dir = scratch("diagnostic");
    let root = dir.join("root.mod");
    let child = dir.join("child.inc");
    let root_text = "@#include \"child.inc\"\n";
    let child_text = "var y; model; y=zz; end;\n";
    fs::write(&root, root_text).unwrap();
    fs::write(&child, child_text).unwrap();
    let root_uri = uri(&root);
    let child_uri = uri(&child);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open(root_uri.clone(), root_text))
        .await;
    let root_items = items(service.inner().diagnostic(pull(root_uri)).await.unwrap());
    let child_items = items(service.inner().diagnostic(pull(child_uri)).await.unwrap());
    assert!(!has_code(&root_items, "E020"), "root: {root_items:?}");
    let error = child_items
        .iter()
        .find(|item| item.code == Some(NumberOrString::String("E020".into())))
        .expect("child E020");
    assert_eq!(error.range.start, Position::new(0, 16));
    assert_eq!(error.range.end, Position::new(0, 18));
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn reserved_block_token_is_published_on_child_uri() {
    let dir = scratch("reserved-block-token");
    let root = dir.join("root.mod");
    let child = dir.join("child.inc");
    let root_text = "@#include \"child.inc\"\n";
    let child_text = "var y; model; y=dsge_prior_weight; end;\n";
    fs::write(&root, root_text).unwrap();
    fs::write(&child, child_text).unwrap();
    let (service, _socket) = new_service();
    service.inner().did_open(open(uri(&root), root_text)).await;
    let root_items = items(service.inner().diagnostic(pull(uri(&root))).await.unwrap());
    let child_items = items(service.inner().diagnostic(pull(uri(&child))).await.unwrap());
    assert!(!has_code(&root_items, "E001"), "{root_items:?}");
    let error = child_items
        .iter()
        .find(|item| item.code == Some(NumberOrString::String("E001".into())))
        .unwrap();
    assert_eq!(error.range.start, Position::new(0, 16));
    assert_eq!(error.range.end, Position::new(0, 33));
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn references_follow_known_include_from_root() {
    let dir = scratch("references");
    let root = dir.join("root.mod");
    let child = dir.join("child.inc");
    let root_text = "var y;\nmodel;\n@#include \"child.inc\"\nend;\n";
    let child_text = "y=y(-1); // y in comment\n";
    fs::write(&root, root_text).unwrap();
    fs::write(&child, child_text).unwrap();
    let root_uri = uri(&root);
    let child_uri = uri(&child);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open(root_uri.clone(), root_text))
        .await;
    let refs = service
        .inner()
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: root_uri.clone(),
                },
                position: Position::new(0, 4),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap()
        .expect("references");
    assert_eq!(refs.len(), 3, "{refs:?}");
    assert_eq!(refs.iter().filter(|loc| loc.uri == root_uri).count(), 1);
    let child_ranges: Vec<_> = refs
        .iter()
        .filter(|loc| loc.uri == child_uri)
        .map(|loc| loc.range)
        .collect();
    assert_eq!(child_ranges.len(), 2, "{refs:?}");
    assert!(child_ranges.contains(&Range::new(Position::new(0, 0), Position::new(0, 1))));
    assert!(child_ranges.contains(&Range::new(Position::new(0, 2), Position::new(0, 3))));
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn nested_child_overlay_keeps_utf16_error_position() {
    let dir = scratch("unicode");
    let root = dir.join("root.mod");
    let middle = dir.join("middle.inc");
    let child = dir.join("child.inc");
    let root_text = "@#include \"middle.inc\"\n";
    let disk_child = "var y; model; y=0; end;\n";
    let overlay_child = "/*😀*/var y; model; y=zz; end;\n";
    fs::write(&root, root_text).unwrap();
    fs::write(&middle, "@#include \"child.inc\"\n").unwrap();
    fs::write(&child, disk_child).unwrap();
    let root_uri = uri(&root);
    let child_uri = uri(&child);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open(child_uri.clone(), overlay_child))
        .await;
    service
        .inner()
        .did_open(open(root_uri.clone(), root_text))
        .await;
    let child_items = items(service.inner().diagnostic(pull(child_uri)).await.unwrap());
    let error = child_items
        .iter()
        .find(|item| item.code == Some(NumberOrString::String("E020".into())))
        .expect("overlay E020");
    assert_eq!(error.range.start, Position::new(0, 22));
    assert_eq!(error.range.end, Position::new(0, 24));
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn shared_child_diagnostic_deduplicates_and_clears_by_root() {
    let dir = scratch("shared");
    let first = dir.join("first.mod");
    let second = dir.join("second.mod");
    let child = dir.join("child.inc");
    let root_text = "@#include \"child.inc\"\n";
    fs::write(&first, root_text).unwrap();
    fs::write(&second, root_text).unwrap();
    fs::write(&child, "var y; model; y=zz; end;\n").unwrap();
    let first_uri = uri(&first);
    let second_uri = uri(&second);
    let child_uri = uri(&child);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open(first_uri.clone(), root_text))
        .await;
    service
        .inner()
        .did_open(open(second_uri.clone(), root_text))
        .await;
    let before = items(
        service
            .inner()
            .diagnostic(pull(child_uri.clone()))
            .await
            .unwrap(),
    );
    assert_eq!(count_code(&before, "E020"), 1, "{before:?}");
    service
        .inner()
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: first_uri },
        })
        .await;
    let after_first = items(
        service
            .inner()
            .diagnostic(pull(child_uri.clone()))
            .await
            .unwrap(),
    );
    assert_eq!(count_code(&after_first, "E020"), 1, "{after_first:?}");
    service
        .inner()
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: second_uri },
        })
        .await;
    let after_both = items(service.inner().diagnostic(pull(child_uri)).await.unwrap());
    assert_eq!(count_code(&after_both, "E020"), 0, "{after_both:?}");
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn child_reference_query_finds_open_parent_but_not_unrelated_file() {
    let dir = scratch("parent-scope");
    let root = dir.join("root.mod");
    let child = dir.join("child.inc");
    let unrelated = dir.join("unrelated.mod");
    let root_text = "var y;\nmodel;\n@#include \"child.inc\"\nend;\n";
    let child_text = "y=y(-1);\n";
    let unrelated_text = "var y; model; y=y; end;\n";
    fs::write(&root, root_text).unwrap();
    fs::write(&child, child_text).unwrap();
    fs::write(&unrelated, unrelated_text).unwrap();
    let root_uri = uri(&root);
    let child_uri = uri(&child);
    let unrelated_uri = uri(&unrelated);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open(root_uri.clone(), root_text))
        .await;
    service
        .inner()
        .did_open(open(child_uri.clone(), child_text))
        .await;
    service
        .inner()
        .did_open(open(unrelated_uri.clone(), unrelated_text))
        .await;
    let refs = service
        .inner()
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: child_uri.clone(),
                },
                position: Position::new(0, 0),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap()
        .expect("references");
    assert_eq!(refs.len(), 3, "{refs:?}");
    assert_eq!(refs.iter().filter(|loc| loc.uri == root_uri).count(), 1);
    assert_eq!(refs.iter().filter(|loc| loc.uri == child_uri).count(), 2);
    assert!(!refs.iter().any(|loc| loc.uri == unrelated_uri));
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn included_parse_fix_cannot_edit_include_directive() {
    let dir = scratch("fix-owner");
    let root = dir.join("root.mod");
    let child = dir.join("child.inc");
    let root_text = "@#include \"child.inc\"\n";
    let child_text = "var y";
    fs::write(&root, root_text).unwrap();
    fs::write(&child, child_text).unwrap();
    let root_uri = uri(&root);
    let child_uri = uri(&child);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open(root_uri.clone(), root_text))
        .await;
    let actions = service
        .inner()
        .code_action(CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: root_uri.clone(),
            },
            range: Range::new(Position::new(0, 0), Position::new(0, 25)),
            context: CodeActionContext {
                diagnostics: Vec::new(),
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .unwrap_or_default();
    for action in actions {
        if let CodeActionOrCommand::CodeAction(action) = action {
            assert!(
                !action
                    .edit
                    .and_then(|edit| edit.document_changes)
                    .is_some_and(|changes| matches!(changes, DocumentChanges::Edits(edits)
                        if edits.iter().any(|edit| edit.text_document.uri == root_uri))),
                "child fix must not edit the include directive"
            );
        }
    }
    let child_items = items(service.inner().diagnostic(pull(child_uri)).await.unwrap());
    assert!(has_code(&child_items, "E001"), "{child_items:?}");
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn root_change_drops_its_child_diagnostic() {
    let dir = scratch("change-scope");
    let root = dir.join("root.mod");
    let child = dir.join("child.inc");
    let root_text = "@#include \"child.inc\"\n";
    fs::write(&root, root_text).unwrap();
    fs::write(&child, "var y; model; y=zz; end;\n").unwrap();
    let root_uri = uri(&root);
    let child_uri = uri(&child);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open(root_uri.clone(), root_text))
        .await;
    let before = items(
        service
            .inner()
            .diagnostic(pull(child_uri.clone()))
            .await
            .unwrap(),
    );
    assert!(has_code(&before, "E020"));
    service
        .inner()
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: root_uri,
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "var y; model; y=0; end;\n".into(),
            }],
        })
        .await;
    let after = items(service.inner().diagnostic(pull(child_uri)).await.unwrap());
    assert!(!has_code(&after, "E020"), "{after:?}");
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn executed_include_definition_overlay_turns_child_diagnostics_off_and_on() {
    let dir = scratch("active-include-definition");
    let root = dir.join("root.mod");
    let flags = dir.join("flags.inc");
    let child = dir.join("child.inc");
    let source = "@#include \"flags.inc\"\nvar y; model;\n@#if ENABLED\n@#include \"child.inc\"\n@#else\ny=0;\n@#endif\nend;\n";
    fs::write(&root, source).unwrap();
    fs::write(&flags, "@#define ENABLED=1\n").unwrap();
    fs::write(&child, "/*😀*/ y=missing;\n").unwrap();
    let (service, _socket) = new_service();
    let backend = service.inner();
    backend.did_open(open(uri(&root), source)).await;
    let before = items(backend.diagnostic(pull(uri(&child))).await.unwrap());
    let error = before
        .iter()
        .find(|item| item.code == Some(NumberOrString::String("E020".into())))
        .unwrap();
    assert_eq!(
        error.range,
        Range::new(Position::new(0, 9), Position::new(0, 16))
    );
    backend
        .did_open(open(uri(&flags), "@#define ENABLED=0\n"))
        .await;
    let dormant = items(backend.diagnostic(pull(uri(&child))).await.unwrap());
    assert!(!has_code(&dormant, "E020"), "{dormant:?}");
    let root_items = items(backend.diagnostic(pull(uri(&root))).await.unwrap());
    assert!(
        !root_items
            .iter()
            .any(|item| item.severity == Some(DiagnosticSeverity::ERROR)),
        "{root_items:?}"
    );
    backend
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri(&flags),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "@#define ENABLED=1\n".into(),
            }],
        })
        .await;
    let after = items(backend.diagnostic(pull(uri(&child))).await.unwrap());
    assert_eq!(count_code(&after, "E020"), 1, "{after:?}");
    let path = dir.canonicalize().unwrap();
    let temporary = std::env::temp_dir().canonicalize().unwrap();
    assert!(path.starts_with(&temporary) && path != temporary);
    fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn evaluated_include_path_updates_lsp_child_owner_and_model_facts() {
    let dir = scratch("evaluated-path");
    let root = dir.join("root.mod");
    let flags = dir.join("paths.inc");
    let first = dir.join("visible/chosen.inc");
    let second = dir.join("other/chosen.inc");
    fs::create_dir(dir.join("visible")).unwrap();
    fs::create_dir(dir.join("other")).unwrap();
    let source = "@#include \"paths.inc\"\n@#includepath P\n@#include \"chosen.inc\"\n";
    fs::write(&root, source).unwrap();
    fs::write(&flags, "@#define P=\"visible\"\n").unwrap();
    fs::write(&first, "var y; model; y=missing; end;\n").unwrap();
    fs::write(&second, "var y; model; y=0; end;\n").unwrap();
    let (service, _socket) = new_service();
    let backend = service.inner();
    backend.did_open(open(uri(&root), source)).await;
    assert!(has_code(
        &items(backend.diagnostic(pull(uri(&first))).await.unwrap()),
        "E020"
    ));
    let info = backend
        .execute_command(ExecuteCommandParams {
            command: "dynare/modelInfo".to_string(),
            arguments: vec![serde_json::json!({"root_uri": uri(&root)})],
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(info["n_equations"], 1, "{info}");
    backend
        .did_open(open(uri(&flags), "@#define P=\"other\"\n"))
        .await;
    let old = items(backend.diagnostic(pull(uri(&first))).await.unwrap());
    assert!(!has_code(&old, "E020"), "{old:?}");
    let root_items = items(backend.diagnostic(pull(uri(&root))).await.unwrap());
    assert!(
        !root_items
            .iter()
            .any(|item| item.severity == Some(DiagnosticSeverity::ERROR)),
        "{root_items:?}"
    );
    let changed = backend
        .execute_command(ExecuteCommandParams {
            command: "dynare/modelInfo".to_string(),
            arguments: vec![serde_json::json!({"root_uri": uri(&root)})],
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(changed["n_equations"], 1, "{changed}");
    assert_ne!(changed["revision"], info["revision"]);
    let path = dir.canonicalize().unwrap();
    let temporary = std::env::temp_dir().canonicalize().unwrap();
    assert!(path.starts_with(&temporary) && path != temporary);
    fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn different_root_context_messages_remain_on_shared_child() {
    let dir = scratch("context");
    let first = dir.join("first.mod");
    let second = dir.join("second.mod");
    let child = dir.join("child.inc");
    let first_text = "var y; parameters ab;\n@#include \"child.inc\"\n";
    let second_text = "var y; parameters ac;\n@#include \"child.inc\"\n";
    fs::write(&first, first_text).unwrap();
    fs::write(&second, second_text).unwrap();
    fs::write(&child, "model; y=aa; end;\n").unwrap();
    let child_uri = uri(&child);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open(uri(&first), first_text))
        .await;
    service
        .inner()
        .did_open(open(uri(&second), second_text))
        .await;
    let found = items(service.inner().diagnostic(pull(child_uri)).await.unwrap());
    let messages: Vec<_> = found
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("E020".into())))
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert_ne!(messages[0], messages[1]);
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn shared_child_keeps_maximum_macro_copy_multiplicity_across_roots() {
    let dir = scratch("multiplicity");
    let first = dir.join("a.mod");
    let later = dir.join("c.mod");
    let child = dir.join("child.inc");
    let once = "@#include \"child.inc\"\n";
    let thrice = once.repeat(3);
    fs::write(&first, once).unwrap();
    fs::write(&later, &thrice).unwrap();
    fs::write(&child, "var y; model; y=1; y=1; end;\n").unwrap();
    let first_uri = uri(&first);
    let later_uri = uri(&later);
    let child_uri = uri(&child);

    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open(first_uri.clone(), once))
        .await;
    service
        .inner()
        .did_open(open(later_uri.clone(), &thrice))
        .await;
    let before = items(
        service
            .inner()
            .diagnostic(pull(child_uri.clone()))
            .await
            .unwrap(),
    );
    let before_count = count_code(&before, "W054");
    assert!(
        before_count >= 2,
        "expected repeated child warnings: {before:?}"
    );
    service
        .inner()
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: first_uri.clone(),
            },
        })
        .await;
    let after = items(
        service
            .inner()
            .diagnostic(pull(child_uri.clone()))
            .await
            .unwrap(),
    );
    assert_eq!(
        count_code(&after, "W054"),
        before_count,
        "before={before:?}; after={after:?}"
    );

    let (reverse, _socket) = new_service();
    reverse.inner().did_open(open(later_uri, &thrice)).await;
    reverse.inner().did_open(open(first_uri, once)).await;
    let reversed = items(reverse.inner().diagnostic(pull(child_uri)).await.unwrap());
    assert_eq!(count_code(&reversed, "W054"), before_count, "{reversed:?}");
    fs::remove_dir_all(dir).unwrap();
}
