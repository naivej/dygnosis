//! Editor lock for the I208 bulk naming action.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use dygnosis::server::new_service;
use dygnosis::span::LineIndex;
use dygnosis::{check_file, Severity};
use tower_lsp::lsp_types::*;
use tower_lsp::{LanguageServer, LspService};

fn fixture_path(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/equation_names")
        .join(name);
    path.canonicalize()
        .unwrap_or_else(|err| panic!("canonicalize {}: {err}", path.display()))
}

fn read_fixture(name: &str) -> (Url, String) {
    let path = fixture_path(name);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
        .replace("\r\n", "\n");
    (file_url(&path), text)
}

fn expected(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/equation_names")
        .join(name);
    fs::read_to_string(path).unwrap().replace("\r\n", "\n")
}

fn file_url(path: &Path) -> Url {
    Url::from_file_path(path).unwrap_or_else(|_| panic!("file url for {}", path.display()))
}

fn open_params(uri: Url, text: String, version: i32) -> DidOpenTextDocumentParams {
    DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri,
            language_id: "dynare".into(),
            version,
            text,
        },
    }
}

fn change_params(uri: Url, text: String, version: i32) -> DidChangeTextDocumentParams {
    DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier { uri, version },
        content_changes: vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text,
        }],
    }
}

fn pull_params(uri: Url) -> DocumentDiagnosticParams {
    DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier { uri },
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

fn diag_code(diag: &Diagnostic) -> String {
    match &diag.code {
        Some(NumberOrString::String(code)) => code.clone(),
        Some(NumberOrString::Number(code)) => code.to_string(),
        None => String::new(),
    }
}

fn full_range(text: &str) -> Range {
    let lines: Vec<&str> = text.split('\n').collect();
    let last = lines.last().copied().unwrap_or("");
    Range::new(
        Position::new(0, 0),
        Position::new(
            lines.len().saturating_sub(1) as u32,
            last.encode_utf16().count() as u32,
        ),
    )
}

fn apply_edits(text: &str, edits: &[TextEdit]) -> String {
    let mut spans: Vec<(u32, u32, &str)> = edits
        .iter()
        .map(|edit| {
            let index = LineIndex::new(text);
            let start = index.offset_utf16(
                text,
                dygnosis::span::Position {
                    line: edit.range.start.line,
                    character: edit.range.start.character,
                },
            );
            let end = index.offset_utf16(
                text,
                dygnosis::span::Position {
                    line: edit.range.end.line,
                    character: edit.range.end.character,
                },
            );
            (start, end, edit.new_text.as_str())
        })
        .collect();
    spans.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));
    let mut out = text.to_string();
    for (start, end, new) in spans {
        out.replace_range(start as usize..end as usize, new);
    }
    out
}

fn error_codes(text: &str, path: &str) -> BTreeSet<String> {
    check_file(text, path)
        .into_iter()
        .filter(|diag| diag.severity == Severity::Error)
        .map(|diag| diag.code)
        .collect()
}

fn assert_no_new_error(before: &str, after: &str, path: &str) {
    let old = error_codes(before, path);
    let new = error_codes(after, path);
    let added: Vec<_> = new.difference(&old).cloned().collect();
    assert!(added.is_empty(), "edit added {added:?}");
    assert!(
        !new.contains("E001") || old.contains("E001"),
        "edit added E001"
    );
}

async fn items(service: &LspService<dygnosis::server::Backend>, uri: &Url) -> Vec<Diagnostic> {
    match service
        .inner()
        .diagnostic(pull_params(uri.clone()))
        .await
        .expect("pull")
    {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
            full.full_document_diagnostic_report.items
        }
        other => panic!("expected a full diagnostic report, got {other:?}"),
    }
}

async fn actions_for(
    service: &LspService<dygnosis::server::Backend>,
    uri: &Url,
    range: Range,
) -> Vec<CodeAction> {
    let response = service
        .inner()
        .code_action(CodeActionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range,
            context: CodeActionContext::default(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("code action");
    response
        .unwrap_or_default()
        .into_iter()
        .filter_map(|action| match action {
            CodeActionOrCommand::CodeAction(action) => Some(action),
            CodeActionOrCommand::Command(_) => None,
        })
        .collect()
}

fn naming_actions(actions: &[CodeAction]) -> Vec<&CodeAction> {
    actions
        .iter()
        .filter(|action| action.title.starts_with("Add equation tags"))
        .collect()
}

async fn actions_for_note(
    service: &LspService<dygnosis::server::Backend>,
    uri: &Url,
    note: &Diagnostic,
) -> Vec<CodeAction> {
    service
        .inner()
        .code_action(CodeActionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range: note.range,
            context: CodeActionContext {
                diagnostics: vec![note.clone()],
                ..CodeActionContext::default()
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|action| match action {
            CodeActionOrCommand::CodeAction(action) => Some(action),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn every_note_edits_only_its_exact_equation_rows() {
    let uri = Url::parse("file:///C:/writing-action/statements.mod").unwrap();
    let text = "var x y;\n/*😀中*/model; x=x(-1); end;\n/*😀中*/model; y=x; end;";
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.into(), 1))
        .await;
    let notes: Vec<_> = items(&service, &uri)
        .await
        .into_iter()
        .filter(|note| diag_code(note) == "I208")
        .collect();
    assert_eq!(notes.len(), 2);
    for (note, kept, named) in [
        (&notes[0], "y=x;", "x=x(-1);"),
        (&notes[1], "x=x(-1);", "y=x;"),
    ] {
        let context: dygnosis::WritingContext =
            serde_json::from_value(note.data.as_ref().unwrap()["writing_context"].clone()).unwrap();
        assert_eq!(context.statement_ids.len(), 1);
        assert_eq!(context.rows.ids().len(), 1);
        assert!(matches!(context.rows, dygnosis::WritingRows::Equations(_)));
        let actions = actions_for_note(&service, &uri, note).await;
        let action = naming_actions(&actions);
        assert_eq!(action.len(), 1);
        let edits = edits_for(action[0], &uri);
        assert_eq!(edits.len(), 1);
        let edited = apply_edits(text, &edits);
        assert!(edited.contains(kept), "{edited}");
        assert!(edited.contains(&format!("'] {named}")), "{edited}");
    }
}

#[tokio::test]
async fn stale_forged_and_missing_note_contexts_cannot_select_other_rows() {
    let uri = Url::parse("file:///C:/writing-action/context.mod").unwrap();
    let text = "var x y; model; x=x(-1); end; model; y=x; end;";
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.into(), 1))
        .await;
    let old = items(&service, &uri)
        .await
        .into_iter()
        .find(|note| diag_code(note) == "I208")
        .unwrap();
    service
        .inner()
        .did_change(change_params(uri.clone(), format!("{text}\n// changed"), 2))
        .await;
    assert!(naming_actions(&actions_for_note(&service, &uri, &old).await).is_empty());
    let notes: Vec<_> = items(&service, &uri)
        .await
        .into_iter()
        .filter(|note| diag_code(note) == "I208")
        .collect();
    for case in 0..4 {
        let mut forged = notes[0].clone();
        match case {
            0 => {
                forged.data.as_mut().unwrap()["writing_context"]["rows"] =
                    notes[1].data.as_ref().unwrap()["writing_context"]["rows"].clone()
            }
            1 => {
                forged.data.as_mut().unwrap()["writing_context"]["statement_ids"] =
                    notes[1].data.as_ref().unwrap()["writing_context"]["statement_ids"].clone()
            }
            2 => {
                forged.data.as_mut().unwrap()["root"] =
                    serde_json::json!("file:///C:/writing-action/other.mod")
            }
            _ => forged.data = None,
        }
        assert!(
            naming_actions(&actions_for_note(&service, &uri, &forged).await).is_empty(),
            "forged case {case}"
        );
    }
    assert_eq!(
        naming_actions(&actions_for_note(&service, &uri, &notes[0]).await).len(),
        1
    );
}

#[tokio::test]
async fn replacement_note_names_only_the_surviving_replacement_rows() {
    let uri = Url::parse("file:///C:/writing-action/replacement.mod").unwrap();
    let text =
        "var x y;\nmodel; [name='old'] x=y(-1); y=y(-1); end;\nmodel_replace('old'); x=y; end;";
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.into(), 1))
        .await;
    let notes: Vec<_> = items(&service, &uri)
        .await
        .into_iter()
        .filter(|note| diag_code(note) == "I208")
        .collect();
    assert_eq!(notes.len(), 2);
    let replacement = notes
        .iter()
        .find(|note| note.range.start.line == 2)
        .unwrap();
    assert_eq!(replacement.range.end.character, 13);
    let actions = actions_for_note(&service, &uri, replacement).await;
    let naming = naming_actions(&actions);
    assert_eq!(naming.len(), 1);
    let edits = edits_for(naming[0], &uri);
    assert_eq!(edits.len(), 1);
    let edited = apply_edits(text, &edits);
    assert!(edited.contains("[name='old'] x=y(-1); y=y(-1);"));
    assert!(
        edited.contains("model_replace('old'); [name='eq1'] x=y;"),
        "{edited}"
    );
}

#[tokio::test]
async fn shared_declaration_notes_publish_separate_explicit_root_contexts() {
    let dir = std::env::temp_dir().join(format!(
        "dygnosis-i209-roots-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let child = dir.join("decl.inc");
    let a = dir.join("a.mod");
    let b = dir.join("b.mod");
    let text = "@#include \"decl.inc\"\nmodel; x=x(-1); end;";
    fs::write(&child, "/*😀中*/var x;").unwrap();
    fs::write(&a, text).unwrap();
    fs::write(&b, text).unwrap();
    let (service, _socket) = new_service();
    for root in [&a, &b] {
        service
            .inner()
            .did_open(open_params(file_url(root), text.into(), 1))
            .await;
    }
    let notes: Vec<_> = items(&service, &file_url(&child))
        .await
        .into_iter()
        .filter(|note| diag_code(note) == "I209")
        .collect();
    assert_eq!(notes.len(), 2);
    let mut roots = BTreeSet::new();
    for note in notes {
        assert_eq!(
            note.range,
            Range::new(Position::new(0, 7), Position::new(0, 10))
        );
        let context: dygnosis::WritingContext =
            serde_json::from_value(note.data.unwrap()["writing_context"].clone()).unwrap();
        roots.insert(context.root);
        assert!(!context.input_revision.is_empty());
        assert_eq!(context.statement_ids.len(), 1);
        assert!(matches!(
            context.rows,
            dygnosis::WritingRows::Declarations(_)
        ));
        assert_eq!(context.rows.ids().len(), 1);
    }
    assert_eq!(roots.len(), 2);
    fs::remove_dir_all(dir).unwrap();
}

async fn naming_on_i208(
    service: &LspService<dygnosis::server::Backend>,
    uri: &Url,
) -> Option<CodeAction> {
    let note = items(service, uri)
        .await
        .into_iter()
        .find(|diag| diag_code(diag) == "I208")?;
    let actions = actions_for(service, uri, note.range).await;
    let found = naming_actions(&actions);
    assert!(
        found.len() <= 1,
        "expected one naming action, got {}",
        found.len()
    );
    found.into_iter().next().cloned()
}

#[tokio::test]
async fn included_writing_notes_route_to_their_files_and_clear_after_edits() {
    let dir = std::env::temp_dir().join(format!(
        "dygnosis-writing-owner-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let root = dir.join("root.mod");
    let decl = dir.join("decl.inc");
    let eq = dir.join("eq.inc");
    let root_text = "// 😀 root\n@#include \"decl.inc\"\nmodel;\n@#include \"eq.inc\"\nend;\n";
    let decl_text = "/*😀*/var y;\n";
    let eq_text = "/*😀*/ y = 2;\n";
    fs::write(&root, root_text).unwrap();
    fs::write(&decl, decl_text).unwrap();
    fs::write(&eq, eq_text).unwrap();
    let root_uri = file_url(&root);
    let decl_uri = file_url(&decl);
    let eq_uri = file_url(&eq);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(root_uri.clone(), root_text.into(), 1))
        .await;

    let root_notes = items(&service, &root_uri).await;
    let unnamed = root_notes
        .iter()
        .find(|diag| diag_code(diag) == "I208")
        .unwrap();
    assert_eq!(
        unnamed.range,
        Range::new(Position::new(2, 0), Position::new(2, 5))
    );
    assert!(root_notes
        .iter()
        .all(|diag| !matches!(diag_code(diag).as_str(), "I209" | "I210")));
    let declaration = items(&service, &decl_uri).await;
    let long_name = declaration
        .iter()
        .find(|diag| diag_code(diag) == "I209")
        .unwrap();
    assert_eq!(
        long_name.range,
        Range::new(Position::new(0, 6), Position::new(0, 9))
    );
    let equations = items(&service, &eq_uri).await;
    assert!(equations.iter().all(|diag| diag_code(diag) != "I208"));
    let literal = equations
        .iter()
        .find(|diag| diag_code(diag) == "I210")
        .unwrap();
    assert_eq!(literal.range.start, Position::new(0, 11));

    service
        .inner()
        .did_open(open_params(eq_uri.clone(), eq_text.into(), 1))
        .await;
    let action = naming_on_i208(&service, &root_uri)
        .await
        .expect("included naming action");
    assert_eq!(action.title, "Add equation tags");
    assert_eq!(edits_for(&action, &eq_uri).len(), 1);
    assert!(edits_for(&action, &root_uri).is_empty());

    let without_literal = "/*😀*/ y = 0;\n";
    service
        .inner()
        .did_change(change_params(eq_uri.clone(), without_literal.into(), 2))
        .await;
    assert!(items(&service, &root_uri)
        .await
        .iter()
        .all(|diag| diag_code(diag) != "I210"));
    let named = "/*😀*/ [name='law'] y = 0;\n";
    service
        .inner()
        .did_change(change_params(eq_uri.clone(), named.into(), 3))
        .await;
    assert!(items(&service, &eq_uri)
        .await
        .iter()
        .all(|diag| diag_code(diag) != "I208"));
    service
        .inner()
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: root_uri },
        })
        .await;
    assert!(items(&service, &decl_uri)
        .await
        .iter()
        .all(|diag| diag_code(diag) != "I209"));
    fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn shared_included_i208_offers_one_action_per_root() {
    let dir = std::env::temp_dir().join(format!(
        "dygnosis-shared-writing-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let a = dir.join("a.mod");
    let b = dir.join("b.mod");
    let child = dir.join("shared.inc");
    let a_text = "var y, a;\n@#include \"shared.inc\"\na = 0;\nend;\n";
    let b_text = "var y, b;\n@#include \"shared.inc\"\nb = 0;\nend;\n";
    let child_text = "model;\ny = 2;\n";
    fs::write(&a, a_text).unwrap();
    fs::write(&b, b_text).unwrap();
    fs::write(&child, child_text).unwrap();
    let a_uri = file_url(&a);
    let b_uri = file_url(&b);
    let child_uri = file_url(&child);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(child_uri.clone(), child_text.into(), 1))
        .await;
    service
        .inner()
        .did_open(open_params(a_uri.clone(), a_text.into(), 1))
        .await;
    service
        .inner()
        .did_open(open_params(b_uri.clone(), b_text.into(), 1))
        .await;
    let notes: Vec<_> = items(&service, &child_uri)
        .await
        .into_iter()
        .filter(|d| diag_code(d) == "I208")
        .collect();
    assert_eq!(notes.len(), 2, "{notes:?}");
    let roots: BTreeSet<_> = notes
        .iter()
        .map(|note| note.data.as_ref().unwrap()["root"].as_str().unwrap())
        .collect();
    assert_eq!(roots, BTreeSet::from([a_uri.as_str(), b_uri.as_str()]));
    assert_eq!(notes[0].range, notes[1].range);

    let actions = naming_actions(&actions_for(&service, &child_uri, notes[0].range).await)
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(actions.len(), 2);
    for action in &actions {
        let root = action.diagnostics.as_ref().unwrap()[0]
            .data
            .as_ref()
            .unwrap()["root"]
            .as_str()
            .unwrap();
        let (own, other) = if root == a_uri.as_str() {
            (&a_uri, &b_uri)
        } else {
            (&b_uri, &a_uri)
        };
        assert!(action
            .title
            .contains(own.to_file_path().unwrap().to_string_lossy().as_ref()));
        assert_eq!(edits_for(action, own).len(), 1);
        assert_eq!(edits_for(action, &child_uri).len(), 1);
        assert!(edits_for(action, other).is_empty());
    }
    let b_note = notes
        .iter()
        .find(|note| note.data.as_ref().unwrap()["root"] == b_uri.as_str())
        .unwrap();
    let response = service
        .inner()
        .code_action(CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: child_uri.clone(),
            },
            range: b_note.range,
            context: CodeActionContext {
                diagnostics: vec![b_note.clone()],
                ..CodeActionContext::default()
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .unwrap();
    let selected: Vec<_> = response
        .into_iter()
        .filter_map(|item| match item {
            CodeActionOrCommand::CodeAction(action)
                if action.title.starts_with("Add equation tags") =>
            {
                Some(action)
            }
            _ => None,
        })
        .collect();
    assert_eq!(selected.len(), 1);
    assert_eq!(
        selected[0].diagnostics.as_ref().unwrap()[0]
            .data
            .as_ref()
            .unwrap()["root"],
        b_uri.as_str()
    );

    service
        .inner()
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: a_uri },
        })
        .await;
    let remaining: Vec<_> = items(&service, &child_uri)
        .await
        .into_iter()
        .filter(|d| diag_code(d) == "I208")
        .collect();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].data.as_ref().unwrap()["root"], b_uri.as_str());
    let single = naming_actions(&actions_for(&service, &child_uri, remaining[0].range).await)
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(single.len(), 1);
    assert_eq!(single[0].title, "Add equation tags");
    assert_eq!(edits_for(&single[0], &b_uri).len(), 1);
    fs::remove_dir_all(&dir).unwrap();
}

fn edits_for(action: &CodeAction, uri: &Url) -> Vec<TextEdit> {
    let edit = action.edit.as_ref().expect("workspace edit");
    let Some(DocumentChanges::Edits(changes)) = edit.document_changes.as_ref() else {
        panic!("expected versioned document changes: {edit:?}");
    };
    changes
        .iter()
        .filter(|change| change.text_document.uri == *uri)
        .flat_map(|change| change.edits.iter())
        .map(|edit| match edit {
            OneOf::Left(edit) => edit.clone(),
            OneOf::Right(_) => panic!("unexpected annotation"),
        })
        .collect()
}
#[tokio::test]
async fn tags_are_kept_and_a_second_apply_does_nothing() {
    let (uri, text) = read_fixture("tags.mod");
    let path = uri.to_file_path().unwrap();
    let path = path.to_string_lossy();
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let action = naming_on_i208(&service, &uri).await.expect("naming action");
    assert_eq!(action.title, "Add equation tags");
    let edited = apply_edits(&text, &edits_for(&action, &uri));
    assert_eq!(edited, expected("tags.named.mod"));
    assert_no_new_error(&text, &edited, &path);
    service
        .inner()
        .did_change(change_params(uri.clone(), edited.clone(), 2))
        .await;
    let again = items(&service, &uri).await;
    assert!(again.iter().all(|diag| diag_code(diag) != "I208"));
    assert!(naming_actions(&actions_for(&service, &uri, full_range(&edited)).await).is_empty());
}

#[tokio::test]
async fn one_note_names_only_its_model_block() {
    let (uri, text) = read_fixture("collision.mod");
    let path = uri.to_file_path().unwrap();
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let action = naming_on_i208(&service, &uri).await.expect("naming action");
    assert_eq!(action.title, "Add equation tags");
    let edits = edits_for(&action, &uri);
    assert_eq!(
        edits.len(),
        1,
        "the note owns only the aggregate model block"
    );
    let edited = apply_edits(&text, &edits);
    assert_eq!(
        edited,
        text.replacen("y = y(-1);", "[name='eq1'] y = y(-1);", 1)
    );
    assert_no_new_error(&text, &edited, &path.to_string_lossy());
}

#[tokio::test]
async fn a_for_copy_is_skipped_and_reported() {
    let (uri, text) = read_fixture("for_copy.mod");
    let path = uri.to_file_path().unwrap();
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let action = naming_on_i208(&service, &uri).await.expect("naming action");
    assert_eq!(action.title, "Add equation tags (2 skipped)");
    let edited = apply_edits(&text, &edits_for(&action, &uri));
    assert_eq!(edited, expected("for_copy.named.mod"));
    assert_no_new_error(&text, &edited, &path.to_string_lossy());
    service
        .inner()
        .did_change(change_params(uri.clone(), edited, 2))
        .await;
    let note = items(&service, &uri)
        .await
        .into_iter()
        .find(|diag| diag_code(diag) == "I208")
        .expect("the copies are still unnamed");
    assert_eq!(note.message, "2 counted equations have no name tag.");
    assert!(naming_actions(&actions_for(&service, &uri, note.range).await).is_empty());
}

#[tokio::test]
async fn only_loop_copies_offer_nothing() {
    let (uri, text) = read_fixture("only_for.mod");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let note = items(&service, &uri)
        .await
        .into_iter()
        .find(|diag| diag_code(diag) == "I208")
        .expect("I208");
    assert!(naming_actions(&actions_for(&service, &uri, note.range).await).is_empty());
    assert!(naming_actions(&actions_for(&service, &uri, full_range(&text)).await).is_empty());
}

#[tokio::test]
async fn an_unresolved_include_offers_nothing() {
    let (uri, text) = read_fixture("unresolved.mod");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let published = items(&service, &uri).await;
    assert!(published.iter().all(|diag| diag_code(diag) != "I208"));
    assert!(naming_actions(&actions_for(&service, &uri, full_range(&text)).await).is_empty());
}

#[tokio::test]
async fn a_name_on_a_skipped_copy_is_still_taken() {
    let (uri, text) = read_fixture("taken.mod");
    let path = uri.to_file_path().unwrap();
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let action = naming_on_i208(&service, &uri).await.expect("naming action");
    assert_eq!(action.title, "Add equation tags");
    let edited = apply_edits(&text, &edits_for(&action, &uri));
    assert_eq!(edited, expected("taken.named.mod"));
    assert_no_new_error(&text, &edited, &path.to_string_lossy());
}

#[tokio::test]
async fn a_shared_include_is_skipped_and_reported() {
    let (root_uri, root_text) = read_fixture("shared_root.mod");
    let (inc_uri, inc_text) = read_fixture("shared_eq.inc");
    let path = root_uri.to_file_path().unwrap();
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(inc_uri.clone(), inc_text.clone(), 1))
        .await;
    service
        .inner()
        .did_open(open_params(root_uri.clone(), root_text.clone(), 1))
        .await;
    assert!(items(&service, &inc_uri)
        .await
        .iter()
        .all(|diag| diag_code(diag) != "I208"));
    let action = naming_on_i208(&service, &root_uri)
        .await
        .expect("naming action");
    assert_eq!(action.title, "Add equation tags (2 skipped)");
    assert!(
        edits_for(&action, &inc_uri).is_empty(),
        "the include is not edited"
    );
    let edited = apply_edits(&root_text, &edits_for(&action, &root_uri));
    assert_eq!(edited, expected("shared_root.named.mod"));
    assert_no_new_error(&root_text, &edited, &path.to_string_lossy());
}

#[tokio::test]
async fn a_unique_include_is_edited_in_that_file() {
    let (root_uri, root_text) = read_fixture("one_inc.mod");
    let (inc_uri, inc_text) = read_fixture("one_eq.inc");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(inc_uri.clone(), inc_text.clone(), 1))
        .await;
    service
        .inner()
        .did_open(open_params(root_uri.clone(), root_text.clone(), 1))
        .await;
    let before = items(&service, &root_uri).await;
    assert!(items(&service, &inc_uri)
        .await
        .iter()
        .all(|diag| diag_code(diag) != "I208"));
    let action = naming_on_i208(&service, &root_uri)
        .await
        .expect("naming action");
    assert_eq!(action.title, "Add equation tags");
    assert!(
        edits_for(&action, &root_uri).is_empty(),
        "the root is not edited"
    );
    let edited = apply_edits(&inc_text, &edits_for(&action, &inc_uri));
    assert_eq!(edited, expected("one_eq.named.inc"));
    service
        .inner()
        .did_change(change_params(inc_uri, edited, 2))
        .await;
    service
        .inner()
        .did_change(change_params(root_uri.clone(), root_text, 2))
        .await;
    let after = items(&service, &root_uri).await;
    let old = error_set(&before);
    let new = error_set(&after);
    let added: Vec<_> = new.difference(&old).cloned().collect();
    assert!(added.is_empty(), "edit added {added:?}");
    assert!(!new.contains("E001"));
}

fn error_set(diags: &[Diagnostic]) -> BTreeSet<String> {
    diags
        .iter()
        .filter(|diag| diag.severity == Some(DiagnosticSeverity::ERROR))
        .map(diag_code)
        .collect()
}

async fn metadata_at(
    service: &LspService<dygnosis::server::Backend>,
    uri: &Url,
    text: &str,
    byte: usize,
) -> Option<CompletionItem> {
    let position = LineIndex::new(text).position_utf16(text, byte as u32);
    let response = service
        .inner()
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position::new(position.line, position.character),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .unwrap()?;
    let items = match response {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    items.into_iter().find(|item| {
        matches!(item.label.as_str(), "name" | "long_name") && item.text_edit.is_some()
    })
}

fn completion_edit(item: &CompletionItem) -> TextEdit {
    match item.text_edit.as_ref().unwrap() {
        CompletionTextEdit::Edit(edit) => edit.clone(),
        _ => panic!("expected replacement"),
    }
}

#[tokio::test]
async fn metadata_completions_repair_only_the_typed_group_and_select_the_whole_value() {
    for snippets in [false, true] {
        for (marked, label, expected) in [
            ("var y; model; |y=0; end;", "name", "[name='eq1'] y=0"),
            ("var y; model; [na|] y=0; end;", "name", "[name='eq1'] y=0"),
            (
                "var y; model; [name=|] y=0; end;",
                "name",
                "[name='eq1'] y=0",
            ),
            (
                "var y; model; [name=  |] y=0; end;",
                "name",
                "[name=  'eq1'] y=0",
            ),
            (
                "var y; model; [name=''|] y=0; end;",
                "name",
                "[name='eq1'] y=0",
            ),
            (
                "var y; model; [name='|'] y=0; end;",
                "name",
                "[name='eq1'] y=0",
            ),
            (
                "var y; model; [name='|\ny=0; end;",
                "name",
                "[name='eq1']\ny=0",
            ),
            (
                "var y|; model; y=0; end;",
                "long_name",
                "y (long_name='y');",
            ),
            ("var y|", "long_name", "var y (long_name='y')"),
            (
                "var y|; parameters p(long_name='__dygnosis_metadata_site__');",
                "long_name",
                "var y (long_name='y');",
            ),
            (
                "parameters p(long_name='__dygnosis_metadata_site__'); var y|;",
                "long_name",
                "var y (long_name='y');",
            ),
            (
                "parameters p(long_name='__dygnosis_metadata_site__'); var y|",
                "long_name",
                "var y (long_name='y')",
            ),
            (
                "var y| z(long_name='__dygnosis_metadata_site__')",
                "long_name",
                "var y (long_name='y') z(long_name='__dygnosis_metadata_site__')",
            ),
            ("var y (long_name=|", "long_name", "var y (long_name='y')"),
            (
                "var y (long_n|); model; y=0; end;",
                "long_name",
                "y (long_name='y');",
            ),
            (
                "var y (long_name=|); model; y=0; end;",
                "long_name",
                "y (long_name='y');",
            ),
            (
                "var y (long_name='|'); model; y=0; end;",
                "long_name",
                "y (long_name='y');",
            ),
            (
                "var y (country='US', long_name=|); model; y=0; end;",
                "long_name",
                "country='US', long_name='y'",
            ),
        ] {
            let uri = Url::parse("file:///C:/metadata/completion.mod").unwrap();
            let byte = marked.find('|').unwrap();
            let text = marked.replacen('|', "", 1);
            let (service, _socket) = new_service();
            service
                .inner()
                .initialize(InitializeParams {
                    capabilities: ClientCapabilities {
                        text_document: Some(TextDocumentClientCapabilities {
                            completion: Some(CompletionClientCapabilities {
                                completion_item: Some(CompletionItemCapability {
                                    snippet_support: Some(snippets),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .await
                .unwrap();
            service
                .inner()
                .did_open(open_params(uri.clone(), text.clone(), 1))
                .await;
            let item = metadata_at(&service, &uri, &text, byte)
                .await
                .unwrap_or_else(|| panic!("missing completion: {marked}"));
            assert_eq!(item.label, label, "{marked}");
            let mut edit = completion_edit(&item);
            let value = if label == "name" { "eq1" } else { "y" };
            if snippets {
                let placeholder = format!("${{1:{value}}}");
                assert!(edit.new_text.contains(&placeholder), "{marked}: {edit:?}");
                edit.new_text = edit.new_text.replace(&placeholder, value);
            } else {
                assert!(!edit.new_text.contains("${"));
            }
            let edited = apply_edits(&text, &[edit]);
            assert!(edited.contains(expected), "{marked}: {edited}");
            assert_no_new_error(&text, &edited, "completion.mod");
            service
                .inner()
                .did_change(change_params(uri.clone(), edited.clone(), 2))
                .await;
            let value_at = edited.find(&format!("'{value}'")).unwrap() + 1;
            assert!(
                metadata_at(&service, &uri, &edited, value_at)
                    .await
                    .is_none(),
                "existing value: {edited}"
            );
        }
    }
}

#[tokio::test]
async fn metadata_completion_withholds_unsafe_or_unrelated_sites() {
    for marked in [
        "var y; model; y=|0; end;",
        "var y; model; y(|-1)=0; end;",
        "var y; model; /* [name=|] */ y=0; end;",
        "var y; model; // [name=|]\ny=0; end;",
        "var y; model; [name='law|'] y=0; end;",
        "var y; model; [name='law'] |y=0; end;",
        "var y; model; [group=|] y=0; end;",
        "var y (long_name='Kept')|; model; y=0; end;",
        "var y (country='|'); model; y=0; end;",
        "var y; model; y=0; end; stoch_simul(order=|);",
        "var y| $Y$; model; y=0; end;",
        "@#define EMPTY=\"\"\nvar y (long_name='@{EMPTY}|'); model; y=0; end;",
        "@#define EMPTY=\"\"\nvar y; model; [name='@{EMPTY}|'] y=0; end;",
        "var y; model; @#for i in 1:1\n|y=0;\n@#endfor\nend;",
        "var y; model; @#if 0\n|y=0;\n@#endif\ny=0; end;",
        "@#include \"missing.inc\"\nvar y; model; |y=0; end;",
        "var y; model; [name=|] y=0\nend;", // Unrelated missing equation semicolon.
        "var y|; parameters p",             // Another declaration owns the EOF recovery.
        "var y|; parameters p(long_name='__dygnosis_metadata_site__')",
        "var y|; model; y=0;", // A missing block end is not metadata recovery.
    ] {
        let uri = Url::parse("file:///C:/metadata/unsafe.mod").unwrap();
        let byte = marked.find('|').unwrap();
        let text = marked.replacen('|', "", 1);
        let (service, _socket) = new_service();
        service
            .inner()
            .did_open(open_params(uri.clone(), text.clone(), 1))
            .await;
        assert!(
            metadata_at(&service, &uri, &text, byte).await.is_none(),
            "unsafe completion: {marked}"
        );
    }
}

#[tokio::test]
async fn a_partial_action_reserves_untouched_lhs_defaults_and_does_not_renumber_tags() {
    let uri = Url::parse("file:///C:/metadata/defaults.mod").unwrap();
    let text = "var y eq1 x; model; y=eq1; end; model; [name='3'] x=y; eq1=x; end;";
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.into(), 1))
        .await;
    let action = naming_on_i208(&service, &uri).await.unwrap();
    let edited = apply_edits(text, &edits_for(&action, &uri));
    assert!(edited.contains("[name='eq2'] y=eq1;"), "{edited}");
    assert_no_new_error(text, &edited, "defaults.mod");
    let pp = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    if pp.is_file() {
        let bad = text.replacen("y=eq1;", "[name='eq1'] y=eq1;", 1);
        for (source, accepted) in [(text, true), (bad.as_str(), false), (edited.as_str(), true)] {
            let result = dygnosis::run_preprocessor(
                source,
                &pp,
                None,
                std::time::Duration::from_secs(30),
                dygnosis::JsonStage::Transform,
            );
            assert_eq!(
                result.success, accepted,
                "{}{}",
                result.raw_stdout, result.raw_stderr
            );
            if !accepted {
                assert!(format!("{}{}", result.raw_stdout, result.raw_stderr)
                    .contains("number 3 because it is already in use"));
            }
        }
    } else {
        eprintln!("Dynare 7.2 absent: partial metadata collision probes skipped");
    }
    service
        .inner()
        .did_change(change_params(uri.clone(), edited.clone(), 2))
        .await;
    let reordered = edited.replace(
        "model; [name='eq2'] y=eq1; end; model; [name='3'] x=y; eq1=x; end;",
        "model; [name='3'] x=y; end; model; [name='eq2'] y=eq1; eq1=x; end;",
    );
    service
        .inner()
        .did_change(change_params(uri.clone(), reordered.clone(), 3))
        .await;
    let actions = actions_for(&service, &uri, full_range(&reordered)).await;
    for action in naming_actions(&actions) {
        let next = apply_edits(&reordered, &edits_for(action, &uri));
        assert!(next.contains("[name='eq2'] y=eq1;"), "{next}");
    }
    // Undo restores the original scope and the same safe allocation.
    service
        .inner()
        .did_change(change_params(uri.clone(), text.into(), 4))
        .await;
    let action = naming_on_i208(&service, &uri).await.unwrap();
    assert_eq!(apply_edits(text, &edits_for(&action, &uri)), edited);
}

#[tokio::test]
async fn long_name_actions_keep_tex_partitions_comments_and_nonempty_values() {
    let uri = Url::parse("file:///C:/metadata/long.mod").unwrap();
    let text = "/*😀中*/var(log) y $Y$ (country='US', /*keep*/ long_name='') z $Z$ q (long_name='Kept');\nvarexo e; varexo_det d; parameters p; model; [group='g', /*keep ]*/ name=''] y=z+q+e+d+p; z=0; q=0; end;";
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.into(), 7))
        .await;
    let equation_action = naming_on_i208(&service, &uri).await.unwrap();
    let tagged = apply_edits(text, &edits_for(&equation_action, &uri));
    assert!(
        tagged.contains("[group='g', /*keep ]*/ name='eq1'] y=z+q+e+d+p;"),
        "{tagged}"
    );
    assert_no_new_error(text, &tagged, "long.mod");
    let notes: Vec<_> = items(&service, &uri)
        .await
        .into_iter()
        .filter(|note| diag_code(note) == "I209")
        .collect();
    assert_eq!(notes.len(), 4);
    let mut all_edits = Vec::new();
    for note in &notes {
        let actions = actions_for_note(&service, &uri, note).await;
        let action = actions
            .iter()
            .find(|action| action.title == "Add long names")
            .unwrap();
        let Some(DocumentChanges::Edits(changes)) =
            action.edit.as_ref().unwrap().document_changes.as_ref()
        else {
            panic!("versioned")
        };
        assert_eq!(changes[0].text_document.version, Some(7));
        all_edits.extend(edits_for(action, &uri));
    }
    let edited = apply_edits(text, &all_edits);
    assert!(edited.contains("y $Y$ (country='US', /*keep*/ long_name='y') z $Z$ (long_name='z') q (long_name='Kept')"), "{edited}");
    for name in ["e", "d", "p"] {
        assert!(
            edited.contains(&format!("{name} (long_name='{name}')")),
            "{edited}"
        );
    }
    assert_no_new_error(text, &edited, "long.mod");
    service
        .inner()
        .did_change(change_params(uri.clone(), edited.clone(), 8))
        .await;
    assert!(items(&service, &uri)
        .await
        .iter()
        .all(|note| diag_code(note) != "I209"));
    assert!(actions_for(&service, &uri, full_range(&edited))
        .await
        .iter()
        .all(|action| !action.title.starts_with("Add long names")));
    assert!(actions_for_note(&service, &uri, &notes[0])
        .await
        .iter()
        .all(|action| !action.title.starts_with("Add long names")));
}

#[tokio::test]
async fn repeated_declarations_need_one_literal_edit_and_macro_values_stay_written() {
    let uri = Url::parse("file:///C:/metadata/repeated.mod").unwrap();
    let text = "@#define EMPTY=\"\"\n@#for j in 1:2\nvar literal generated@{j} (long_name='@{EMPTY}');\n@#endfor\nmodel; literal=0; generated1=0; generated2=0; end;";
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.into(), 1))
        .await;
    let note = items(&service, &uri)
        .await
        .into_iter()
        .find(|note| diag_code(note) == "I209")
        .unwrap();
    let actions = actions_for_note(&service, &uri, &note).await;
    let action = actions
        .iter()
        .find(|action| action.title.starts_with("Add long names"))
        .unwrap();
    assert_eq!(action.title, "Add long names (2 skipped)");
    let edits = edits_for(action, &uri);
    assert_eq!(edits.len(), 1);
    let edited = apply_edits(text, &edits);
    assert!(
        edited.contains("var literal (long_name='literal') generated@{j} (long_name='@{EMPTY}')"),
        "{edited}"
    );
    service
        .inner()
        .did_change(change_params(uri.clone(), edited.clone(), 2))
        .await;
    let notes: Vec<_> = items(&service, &uri)
        .await
        .into_iter()
        .filter(|note| diag_code(note) == "I209")
        .collect();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].message, "2 symbols have no long_name.");
    assert!(actions_for_note(&service, &uri, &notes[0])
        .await
        .iter()
        .all(|action| !action.title.starts_with("Add long names")));
}

#[tokio::test]
async fn included_completions_require_owner_agreement_and_long_name_actions_keep_root_contexts() {
    let dir = std::env::temp_dir().join(format!(
        "dygnosis-metadata-roots-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let child = dir.join("shared.inc");
    let a = dir.join("a.mod");
    let b = dir.join("b.mod");
    let child_text = "var y; model; [name=''] y=0;";
    let a_text = "@#include \"shared.inc\"\n[name='eq1'] y=1; end;";
    let b_text = "@#include \"shared.inc\"\ny=1; end;";
    fs::write(&child, child_text).unwrap();
    fs::write(&a, a_text).unwrap();
    fs::write(&b, b_text).unwrap();
    let child_uri = file_url(&child);
    let a_uri = file_url(&a);
    let b_uri = file_url(&b);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(child_uri.clone(), child_text.into(), 4))
        .await;
    service
        .inner()
        .did_open(open_params(a_uri.clone(), a_text.into(), 1))
        .await;
    service
        .inner()
        .did_open(open_params(b_uri.clone(), b_text.into(), 1))
        .await;
    let notes: Vec<_> = items(&service, &child_uri)
        .await
        .into_iter()
        .filter(|note| diag_code(note) == "I209")
        .collect();
    assert_eq!(notes.len(), 2);
    for note in &notes {
        let actions = actions_for_note(&service, &child_uri, note).await;
        let actions: Vec<_> = actions
            .iter()
            .filter(|action| action.title.starts_with("Add long names"))
            .collect();
        assert_eq!(actions.len(), 1);
        assert!(actions[0].title.contains(" in "), "{}", actions[0].title);
        let edits = edits_for(actions[0], &child_uri);
        assert_eq!(edits.len(), 1);
        assert!(apply_edits(child_text, &edits).contains("var y (long_name='y');"));
    }
    let at = child_text.find("name=''").unwrap() + "name='".len();
    assert!(
        metadata_at(&service, &child_uri, child_text, at)
            .await
            .is_none(),
        "owners allocate eq2 and eq1"
    );
    let decl_at = child_text.find("var y").unwrap() + "var y".len();
    assert!(
        metadata_at(&service, &child_uri, child_text, decl_at)
            .await
            .is_some(),
        "identical long names across roots"
    );
    service
        .inner()
        .did_change(change_params(
            a_uri.clone(),
            a_text.replace("eq1", "kept"),
            2,
        ))
        .await;
    let item = metadata_at(&service, &child_uri, child_text, at)
        .await
        .unwrap();
    assert_eq!(completion_edit(&item).new_text, "eq1");
    let incomplete = "var y; model; [na y=0;";
    service
        .inner()
        .did_change(change_params(child_uri.clone(), incomplete.into(), 5))
        .await;
    let cursor = incomplete.find("[na").unwrap() + 3;
    let item = metadata_at(&service, &child_uri, incomplete, cursor)
        .await
        .unwrap();
    let edited = apply_edits(incomplete, &[completion_edit(&item)]);
    assert!(edited.contains("[name='eq1'] y=0"), "{edited}");
    assert!(actions_for_note(&service, &child_uri, &notes[1])
        .await
        .iter()
        .all(|action| !action.title.starts_with("Add long names")));
    // Literal labels equal to the proof marker do not identify the selected
    // site, including when several roots own that same written declaration.
    let include_only = "@#include \"shared.inc\"\n";
    service
        .inner()
        .did_change(change_params(a_uri, include_only.into(), 3))
        .await;
    service
        .inner()
        .did_change(change_params(b_uri, include_only.into(), 2))
        .await;
    for (version, marked, expected) in [
        (
            6,
            "parameters p(long_name='__dygnosis_metadata_site__'); var y|;",
            true,
        ),
        (
            7,
            "var y|; parameters p(long_name='__dygnosis_metadata_site__')",
            false,
        ),
        (
            8,
            "parameters p(long_name='__dygnosis_metadata_site__'); var y|",
            true,
        ),
    ] {
        let cursor = marked.find('|').unwrap();
        let text = marked.replacen('|', "", 1);
        service
            .inner()
            .did_change(change_params(child_uri.clone(), text.clone(), version))
            .await;
        let item = metadata_at(&service, &child_uri, &text, cursor).await;
        assert_eq!(item.is_some(), expected, "shared owners: {marked}");
        if let Some(item) = item {
            assert_eq!(completion_edit(&item).new_text, " (long_name='y')");
        }
    }
    fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn quoted_macro_names_are_reserved_and_empty_macro_values_are_skipped() {
    let uri = Url::parse("file:///C:/metadata/quoted.mod").unwrap();
    let text = "@#define EMPTY=\"\"\nvar y z; model;\n@#for j in 1:2\n[name='eq@{j}'] y=0;\n@#endfor\n[name='@{EMPTY}'] z=0; y=z; end;";
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.into(), 1))
        .await;
    let action = naming_on_i208(&service, &uri).await.unwrap();
    assert_eq!(action.title, "Add equation tags (1 skipped)");
    let edited = apply_edits(text, &edits_for(&action, &uri));
    assert!(edited.contains("[name='eq@{j}'] y=0;"));
    assert!(
        edited.contains("[name='@{EMPTY}'] z=0; [name='eq3'] y=z;"),
        "{edited}"
    );
    let cursor = text.rfind("y=z;").unwrap();
    let item = metadata_at(&service, &uri, text, cursor).await.unwrap();
    assert_eq!(completion_edit(&item).new_text, "[name='eq3'] ");
    assert_no_new_error(text, &edited, "quoted.mod");
}

#[tokio::test]
async fn the_edit_uses_a_utf16_column() {
    let (uri, text) = read_fixture("utf16.mod");
    let byte = text.find("y = y(-1);").expect("equation") as u32;
    let index = LineIndex::new(&text);
    let utf16 = index.position_utf16(&text, byte);
    let scalar = index.position(&text, byte);
    assert_ne!(
        utf16.character, scalar.character,
        "the fixture must put a wide character before the equation"
    );
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let action = naming_on_i208(&service, &uri).await.expect("naming action");
    let edit = edits_for(&action, &uri);
    assert_eq!(edit.len(), 1);
    assert_eq!(edit[0].range.start.line, utf16.line);
    assert_eq!(edit[0].range.start.character, utf16.character);
    assert_eq!(edit[0].range.start, edit[0].range.end);
    let edited = apply_edits(&text, &edit);
    assert!(edited.contains("/*😀*/[name='eq1'] y = y(-1);"));
    assert_no_new_error(
        &text,
        &edited,
        &uri.to_file_path().unwrap().to_string_lossy(),
    );
}
