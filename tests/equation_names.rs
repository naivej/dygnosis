//! Editor lock for the I208 bulk naming action.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

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
        .filter(|action| action.title.starts_with("Name counted equations"))
        .collect()
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

fn edits_for<'a>(action: &'a CodeAction, uri: &Url) -> &'a [TextEdit] {
    let changes = action
        .edit
        .as_ref()
        .and_then(|edit| edit.changes.as_ref())
        .expect("workspace edit");
    changes.get(uri).map(Vec::as_slice).unwrap_or(&[])
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
    assert_eq!(action.title, "Name counted equations");
    let edited = apply_edits(&text, edits_for(&action, &uri));
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
async fn cross_scope_eq_1_gets_a_suffix() {
    let (uri, text) = read_fixture("collision.mod");
    let path = uri.to_file_path().unwrap();
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let action = naming_on_i208(&service, &uri).await.expect("naming action");
    assert_eq!(action.title, "Name counted equations");
    let edits = edits_for(&action, &uri);
    assert_eq!(
        edits.len(),
        4,
        "one workspace edit covers every safe equation"
    );
    let edited = apply_edits(&text, edits);
    assert_eq!(edited, expected("collision.named.mod"));
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
    assert_eq!(action.title, "Name counted equations (2 skipped)");
    let edited = apply_edits(&text, edits_for(&action, &uri));
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
    assert_eq!(action.title, "Name counted equations");
    let edited = apply_edits(&text, edits_for(&action, &uri));
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
    let action = naming_on_i208(&service, &root_uri)
        .await
        .expect("naming action");
    assert_eq!(action.title, "Name counted equations (2 skipped)");
    let changes = action
        .edit
        .as_ref()
        .and_then(|edit| edit.changes.as_ref())
        .expect("workspace edit");
    assert!(changes.get(&inc_uri).is_none(), "the include is not edited");
    let edited = apply_edits(&root_text, edits_for(&action, &root_uri));
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
    let action = naming_on_i208(&service, &root_uri)
        .await
        .expect("naming action");
    assert_eq!(action.title, "Name counted equations");
    let changes = action
        .edit
        .as_ref()
        .and_then(|edit| edit.changes.as_ref())
        .expect("workspace edit");
    assert!(changes.get(&root_uri).is_none(), "the root is not edited");
    let edited = apply_edits(&inc_text, edits_for(&action, &inc_uri));
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
    let edited = apply_edits(&text, edit);
    assert!(edited.contains("/*😀*/[name='eq_1'] y = y(-1);"));
    assert_no_new_error(
        &text,
        &edited,
        &uri.to_file_path().unwrap().to_string_lossy(),
    );
}
