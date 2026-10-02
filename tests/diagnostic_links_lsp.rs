use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use dygnosis::server::{new_service, Backend};
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dygnosis-related-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, text: &str) -> Url {
        let file = self.0.join(name);
        fs::write(&file, text).unwrap();
        Url::from_file_path(file).unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let path = self.0.canonicalize().unwrap();
        let temp = std::env::temp_dir().canonicalize().unwrap();
        assert!(path.starts_with(&temp) && path != temp);
        fs::remove_dir_all(path).unwrap();
    }
}
async fn open(server: &Backend, uri: &Url, text: &str, version: i32) {
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version,
                text: text.into(),
            },
        })
        .await;
}
async fn pull(server: &Backend, uri: &Url) -> Vec<Diagnostic> {
    match server
        .diagnostic(DocumentDiagnosticParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            identifier: None,
            previous_result_id: None,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
    {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
            full.full_document_diagnostic_report.items
        }
        other => panic!("{other:?}"),
    }
}
async fn actions(
    server: &Backend,
    uri: &Url,
    range: Range,
    diagnostics: Vec<Diagnostic>,
) -> Vec<CodeAction> {
    server
        .code_action(CodeActionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range,
            context: CodeActionContext {
                diagnostics,
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|action| {
            if let CodeActionOrCommand::CodeAction(action) = action {
                Some(action)
            } else {
                None
            }
        })
        .collect()
}
fn is_code(row: &Diagnostic, code: &str) -> bool {
    row.code == Some(NumberOrString::String(code.into()))
}
fn edits(action: &CodeAction) -> &[TextDocumentEdit] {
    match action
        .edit
        .as_ref()
        .unwrap()
        .document_changes
        .as_ref()
        .unwrap()
    {
        DocumentChanges::Edits(edits) => edits,
        _ => panic!("expected versioned edits"),
    }
}

#[tokio::test]
async fn related_locations_use_independent_utf16_sources_and_keep_root_merge_sets() {
    let temp = Scratch::new();
    let left = temp.file("left.inc", "/* 🧮 */ var y;\r\n");
    let right = temp.file("right.inc", "/* β */ var y;\n");
    let child = temp.file("shared.inc", "/* 🚀 */ var y;\r\n");
    let a_text = "@#include \"left.inc\"\n@#include \"shared.inc\"\nmodel; y=0; end;";
    let b_text = "@#include \"right.inc\"\n@#include \"shared.inc\"\nmodel; y=0; end;";
    let a = temp.file("a.mod", a_text);
    let b = temp.file("b.mod", b_text);
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &a, a_text, 3).await;
    open(server, &b, b_text, 4).await;
    let rows = pull(server, &child).await;
    let duplicates: Vec<_> = rows.iter().filter(|row| is_code(row, "W031")).collect();
    assert_eq!(duplicates.len(), 2, "{rows:?}");
    assert_eq!(duplicates[0].range.start, Position::new(0, 13));
    let related: Vec<_> = duplicates
        .iter()
        .flat_map(|row| row.related_information.as_ref().unwrap())
        .collect();
    assert!(related
        .iter()
        .any(|row| row.location.uri == left && row.location.range.start == Position::new(0, 13)));
    assert!(related
        .iter()
        .any(|row| row.location.uri == right && row.location.range.start == Position::new(0, 12)));
}

#[tokio::test]
async fn cycles_link_earlier_and_closing_include_edges_in_their_own_files() {
    let temp = Scratch::new();
    let root_text = "@#include \"b.inc\"\nvar y; model; y=0; end;";
    let root = temp.file("a.mod", root_text);
    let b = temp.file("b.inc", "@#include \"c.inc\"\n");
    let c = temp.file("c.inc", "/* 🚀 */\n@#include \"b.inc\"\n");
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &root, root_text, 1).await;
    let rows = pull(server, &root).await;
    let row = rows.iter().find(|row| is_code(row, "W062")).unwrap();
    let sites = row.related_information.as_ref().unwrap();
    assert!(sites
        .iter()
        .any(|site| site.location.uri == root && site.location.range.start == Position::new(0, 0)));
    assert!(sites
        .iter()
        .any(|site| site.location.uri == c && site.location.range.start == Position::new(1, 0)));
    assert!(!sites.iter().any(|site| site.location.uri == b));
}

#[tokio::test]
async fn unopened_include_fix_uses_checked_source_and_rejects_disk_changes() {
    let temp = Scratch::new();
    let child = temp.file("declarations.inc", "var y");
    let text = "@#include \"declarations.inc\"\n";
    let root = temp.file("root.mod", text);
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &root, text, 7).await;
    let rows = pull(server, &child).await;
    let note = rows
        .iter()
        .find(|row| is_code(row, "E001"))
        .unwrap()
        .clone();
    let fixes = actions(server, &child, note.range, vec![note.clone()]).await;
    let fix = fixes.first().unwrap_or_else(|| panic!("{fixes:?}"));
    assert_eq!(edits(fix)[0].text_document.uri, child);
    assert_eq!(edits(fix)[0].text_document.version, None);
    fs::write(child.to_file_path().unwrap(), "var other;\n").unwrap();
    assert!(actions(server, &child, note.range, vec![note])
        .await
        .is_empty());
}

#[tokio::test]
async fn naming_from_unopened_include_preserves_roots_versions_and_stale_context() {
    let temp = Scratch::new();
    let child = temp.file("body.inc", "y=0;\n");
    let text = "var y; model;\n@#include \"body.inc\"\n[name='root'] y=1; end;\n";
    let root = temp.file("root.mod", text);
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &root, text, 7).await;
    let rows = pull(server, &child).await;
    let note = rows
        .iter()
        .find(|row| is_code(row, "I208"))
        .unwrap()
        .clone();
    assert_eq!(note.data.as_ref().unwrap()["root"], root.as_str());
    let named = actions(server, &child, note.range, vec![note.clone()]).await;
    assert_eq!(named.len(), 1, "{named:?}");
    assert_eq!(edits(&named[0])[0].text_document.uri, child);
    assert_eq!(edits(&named[0])[0].text_document.version, None);
    open(server, &child, "y=0;\n", 9).await;
    let current = pull(server, &child)
        .await
        .into_iter()
        .find(|row| is_code(row, "I208"))
        .unwrap();
    let named = actions(server, &child, current.range, vec![current.clone()]).await;
    assert!(edits(&named[0])
        .iter()
        .any(|edit| edit.text_document.uri == child && edit.text_document.version == Some(9)));
    open(
        server,
        &root,
        &format!("{text}\nparameters new_parameter;"),
        8,
    )
    .await;
    assert!(actions(server, &child, note.range, vec![note])
        .await
        .is_empty());
}

#[tokio::test]
async fn unnecessary_tags_are_only_on_unused_warnings_and_macro_context_is_additive() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let uri = Url::from_file_path(Path::new("C:/dygnosis-related/tags.mod")).unwrap();
    let text = "var y unused_y; varexo unused_e; parameters unused_p; model; y=y(-1); end;";
    open(server, &uri, text, 1).await;
    let rows = pull(server, &uri).await;
    for code in ["W020", "W022"] {
        assert_eq!(
            rows.iter().find(|row| is_code(row, code)).unwrap().tags,
            Some(vec![DiagnosticTag::UNNECESSARY])
        );
    }
    assert!(rows
        .iter()
        .find(|row| is_code(row, "E021"))
        .unwrap()
        .tags
        .is_none());
    let text = "@#for k in 1:2\nvar y;\n@#endfor\nmodel; y=0; end;";
    open(server, &uri, text, 2).await;
    let rows = pull(server, &uri).await;
    let duplicate = rows.iter().find(|row| is_code(row, "W031")).unwrap();
    assert_eq!(duplicate.message, "Symbol y declared twice.");
    assert_eq!(
        duplicate.data.as_ref().unwrap()["related_context"][0]["origin_frames"][0]["value"],
        "1"
    );
    let virtual_uri = Url::parse("untitled:related.mod").unwrap();
    let text = "// comment\r/* 🚀 */ var y;\rvar y;\rmodel; y=0; end;";
    open(server, &virtual_uri, text, 1).await;
    let rows = pull(server, &virtual_uri).await;
    let duplicate = rows.iter().find(|row| is_code(row, "W031")).unwrap();
    let site = &duplicate.related_information.as_ref().unwrap()[0];
    assert_eq!(site.location.uri, virtual_uri);
    assert_eq!(site.location.range.start, Position::new(1, 13));
}
