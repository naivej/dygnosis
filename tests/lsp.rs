use std::fs;
use std::path::{Path, PathBuf};

use dygnosis::server::{diagnostics_for, initialize_result, new_service};
use dygnosis::Workspace;
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

const OUT: &[&str] = &[
    "E040", "W040", "W041", "I041", "W071", "I070", "I071", "W080", "W081", "DYNR",
];

const I050_MESSAGE: &str = "No initval or steady_state_model block. Add an initval block with initial guesses, or a steady_state_model block with closed-form assignments.";

fn copilot_mod(archive_dir: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/model-archive")
        .join(archive_dir)
        .join(format!("{archive_dir}.mod"));
    // Canonicalize so file URLs have no `..` (Url round-trip must match HashMap keys).
    path.canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize {}: {e}", path.display()))
}

fn read_mod(archive_dir: &str) -> String {
    let path = copilot_mod(archive_dir);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn file_url(path: &Path) -> Url {
    Url::from_file_path(path).unwrap_or_else(|_| panic!("file url for {}", path.display()))
}

fn archive_url(archive_dir: &str) -> Url {
    file_url(&copilot_mod(archive_dir))
}

fn expand_fixture(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/expand")
        .join(name);
    path.canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize {}: {e}", path.display()))
}

fn expand_open(name: &str) -> (Url, String) {
    let path = expand_fixture(name);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        .replace("\r\n", "\n");
    (file_url(&path), text)
}

fn copilot_example(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/examples")
        .join(format!("{name}.mod"));
    path.canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize {}: {e}", path.display()))
}

fn json_range(obj: &serde_json::Value) -> Range {
    serde_json::from_value(obj.get("range").cloned().expect("range")).expect("Range")
}

fn diag_code(d: &Diagnostic) -> String {
    match &d.code {
        Some(NumberOrString::String(s)) => s.clone(),
        Some(NumberOrString::Number(n)) => n.to_string(),
        None => String::new(),
    }
}

fn is_p_digits(code: &str) -> bool {
    let rest = match code.strip_prefix('P') {
        Some(r) => r,
        None => return false,
    };
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

fn assert_thin_codes(diags: &[Diagnostic]) {
    for d in diags {
        let code = diag_code(d);
        assert!(!OUT.contains(&code.as_str()), "Out code published: {code}");
        assert!(!is_p_digits(&code), "preprocessor code published: {code}");
        assert!(
            !d.message.contains("Compute Steady State"),
            "forbidden message: {}",
            d.message
        );
    }
}

fn pull_items(result: DocumentDiagnosticReportResult) -> Vec<Diagnostic> {
    match result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
            full.full_document_diagnostic_report.items
        }
        other => panic!("expected full document diagnostic report, got {other:?}"),
    }
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

fn close_params(uri: Url) -> DidCloseTextDocumentParams {
    DidCloseTextDocumentParams {
        text_document: TextDocumentIdentifier { uri },
    }
}

fn save_params(uri: Url, text: String) -> DidSaveTextDocumentParams {
    DidSaveTextDocumentParams {
        text_document: TextDocumentIdentifier { uri },
        text: Some(text),
    }
}

fn workspace_pull_params() -> WorkspaceDiagnosticParams {
    WorkspaceDiagnosticParams {
        identifier: None,
        previous_result_ids: Vec::new(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

fn workspace_items(result: WorkspaceDiagnosticReportResult) -> Vec<(Url, Vec<Diagnostic>)> {
    match result {
        WorkspaceDiagnosticReportResult::Report(report) => report
            .items
            .into_iter()
            .map(|item| match item {
                WorkspaceDocumentDiagnosticReport::Full(full) => {
                    (full.uri, full.full_document_diagnostic_report.items)
                }
                other => panic!("expected full workspace report, got {other:?}"),
            })
            .collect(),
        other => panic!("expected workspace report, got {other:?}"),
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

fn assignment_names(model: &dygnosis::Model) -> Vec<String> {
    model
        .helper_assignments
        .iter()
        .map(|a| model.name(a.name).to_string())
        .collect()
}

#[test]
fn initialize_capabilities_wave_a() {
    let result = initialize_result();
    assert_eq!(
        result.capabilities.text_document_sync,
        Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL))
    );
    match result.capabilities.diagnostic_provider {
        Some(DiagnosticServerCapabilities::Options(ref opts)) => {
            assert!(opts.inter_file_dependencies);
            assert!(opts.workspace_diagnostics);
        }
        other => panic!("expected diagnostic options, got {other:?}"),
    }
    assert_eq!(
        result.server_info.as_ref().map(|s| s.name.as_str()),
        Some("dygnosis")
    );

    let json = serde_json::to_string(&result).expect("serialize initialize result");
    for forbidden in [
        "dynare/computeSteadyState",
        "dynare/runDynare",
        "Compute Steady State",
    ] {
        assert!(
            !json.contains(forbidden),
            "initialize JSON contains {forbidden:?}: {json}"
        );
    }
}

#[tokio::test]
async fn open_trend_rbc_gov_inv_thin_codes() {
    let text = read_mod("trend_rbc_gov_inv");
    let uri = archive_url("trend_rbc_gov_inv");
    let from_lib = diagnostics_for(uri.as_str(), &text);
    assert_thin_codes(&from_lib);

    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text, 1))
        .await;
    let pulled = pull_items(
        service
            .inner()
            .diagnostic(pull_params(uri))
            .await
            .expect("pull"),
    );
    assert_thin_codes(&pulled);
    assert_eq!(pulled.len(), from_lib.len());
}

#[test]
fn open_mutation_publishes_e001_or_i050() {
    let mutated =
        read_mod("trend_rbc_gov_inv").replacen("log_n = log(n);\n\nend;", "log_n = log(n);\n", 1);
    let e001 = diagnostics_for(archive_url("trend_rbc_gov_inv").as_str(), &mutated);
    assert_thin_codes(&e001);
    assert!(
        e001.iter().any(|d| diag_code(d) == "E001"),
        "expected E001 after delete model end; got {:?}",
        e001.iter().map(diag_code).collect::<Vec<_>>()
    );

    let sims = read_mod("sims_wu_2019");
    let i050 = diagnostics_for(archive_url("sims_wu_2019").as_str(), &sims);
    assert_thin_codes(&i050);
    let info = i050
        .iter()
        .find(|d| diag_code(d) == "I050")
        .expect("I050 on sims_wu_2019");
    assert_eq!(info.message, I050_MESSAGE);
    assert!(!info.message.contains("Compute Steady State"));
}

#[tokio::test]
async fn close_clears_diagnostics() {
    let mutated =
        read_mod("trend_rbc_gov_inv").replacen("log_n = log(n);\n\nend;", "log_n = log(n);\n", 1);
    let uri = archive_url("trend_rbc_gov_inv");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), mutated, 1))
        .await;
    let open_items = pull_items(
        service
            .inner()
            .diagnostic(pull_params(uri.clone()))
            .await
            .expect("pull after open"),
    );
    assert!(
        open_items.iter().any(|d| diag_code(d) == "E001"),
        "open should publish E001"
    );

    service.inner().did_close(close_params(uri.clone())).await;
    let closed = pull_items(
        service
            .inner()
            .diagnostic(pull_params(uri))
            .await
            .expect("pull after close"),
    );
    assert!(closed.is_empty(), "close must clear pull diagnostics");
}

#[tokio::test]
async fn full_change_reanalyzes() {
    let original = read_mod("trend_rbc_gov_inv");
    let mutated = original.replacen("log_n = log(n);\n\nend;", "log_n = log(n);\n", 1);
    let uri = archive_url("trend_rbc_gov_inv");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), original, 1))
        .await;
    service
        .inner()
        .did_change(change_params(uri.clone(), mutated, 2))
        .await;
    let items = pull_items(
        service
            .inner()
            .diagnostic(pull_params(uri))
            .await
            .expect("pull after change"),
    );
    assert_thin_codes(&items);
    assert!(
        items.iter().any(|d| diag_code(d) == "E001"),
        "FULL didChange mutation should publish E001; got {:?}",
        items.iter().map(diag_code).collect::<Vec<_>>()
    );
    assert!(
        !items.iter().any(|d| is_p_digits(&diag_code(d))),
        "didChange must stay own-only; got {:?}",
        items.iter().map(diag_code).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn watched_files_invalidates_overlay() {
    let dir = std::env::temp_dir().join(format!(
        "dygnosis-lsp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let helper = dir.join("helper.inc");
    let parent = dir.join("parent.mod");
    fs::write(&helper, "disk_only = 1;\n").unwrap();
    fs::write(&parent, "@#include \"helper.inc\"\nvar y;\n").unwrap();

    let mut ws = Workspace::new();
    ws.load_from_disk(&parent).unwrap();
    ws.update_document(helper.to_str().unwrap(), "overlay_only = 1;\n");
    let parent_key = parent.to_str().unwrap();
    let names = assignment_names(ws.get_effective_model(parent_key).unwrap());
    assert!(
        names.contains(&"overlay_only".to_string()),
        "overlay should splice, got {names:?}"
    );
    assert!(
        !names.contains(&"disk_only".to_string()),
        "disk helper must not win while overlay is set: {names:?}"
    );

    ws.remove_document(helper.to_str().unwrap());
    let names = assignment_names(ws.get_effective_model(parent_key).unwrap());
    assert!(
        names.contains(&"disk_only".to_string()),
        "remove_document should let disk win, got {names:?}"
    );
    assert!(
        !names.contains(&"overlay_only".to_string()),
        "overlay must not survive remove_document: {names:?}"
    );

    fs::write(&helper, "parameters helper_var;\nhelper_var = 1;\n").unwrap();
    let parent_src = "@#include \"helper.inc\"\nvar y;\nmodel;\ny = helper_var;\nend;\n";
    fs::write(&parent, parent_src).unwrap();
    let parent_uri = file_url(&parent);
    let helper_uri = file_url(&helper);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(parent_uri.clone(), parent_src.to_string(), 1))
        .await;
    let before = pull_items(
        service
            .inner()
            .diagnostic(pull_params(parent_uri.clone()))
            .await
            .expect("pull before watch"),
    );
    assert!(
        !before.iter().any(|d| diag_code(d) == "E020"),
        "included helper_var should not be E020: {:?}",
        before.iter().map(diag_code).collect::<Vec<_>>()
    );

    fs::write(&helper, "parameters other;\nother = 1;\n").unwrap();
    service
        .inner()
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent::new(helper_uri, FileChangeType::CHANGED)],
        })
        .await;
    let after = pull_items(
        service
            .inner()
            .diagnostic(pull_params(parent_uri))
            .await
            .expect("pull after watch"),
    );
    assert!(
        after.iter().any(|d| diag_code(d) == "E020"),
        "watched-file refresh should drop helper_var and emit E020; got {:?}",
        after.iter().map(diag_code).collect::<Vec<_>>()
    );

    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn open_sims_wu_2019_publishes_i050() {
    let text = read_mod("sims_wu_2019");
    let uri = archive_url("sims_wu_2019");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text, 1))
        .await;
    let items = pull_items(
        service
            .inner()
            .diagnostic(pull_params(uri))
            .await
            .expect("pull"),
    );
    let info = items
        .iter()
        .find(|d| diag_code(d) == "I050")
        .expect("I050 on sims_wu_2019");
    assert_eq!(info.message, I050_MESSAGE);
    assert!(!info.message.contains("Compute Steady State"));
}

#[tokio::test]
async fn did_save_reanalyzes() {
    let original = read_mod("trend_rbc_gov_inv");
    let mutated = original.replacen("log_n = log(n);\n\nend;", "log_n = log(n);\n", 1);
    let uri = archive_url("trend_rbc_gov_inv");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), original, 1))
        .await;
    service
        .inner()
        .did_save(save_params(uri.clone(), mutated))
        .await;
    let items = pull_items(
        service
            .inner()
            .diagnostic(pull_params(uri))
            .await
            .expect("pull after save"),
    );
    assert_thin_codes(&items);
    assert!(
        items.iter().any(|d| diag_code(d) == "E001"),
        "didSave mutation should publish own E001 even when the binary is present; got {:?}",
        items.iter().map(diag_code).collect::<Vec<_>>()
    );
    assert!(
        !items.iter().any(|d| is_p_digits(&diag_code(d))),
        "didSave must stay own-only; got {:?}",
        items.iter().map(diag_code).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn workspace_diagnostic_lists_open_docs() {
    let trend = read_mod("trend_rbc_gov_inv");
    let sims = read_mod("sims_wu_2019");
    let trend_uri = archive_url("trend_rbc_gov_inv");
    let sims_uri = archive_url("sims_wu_2019");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(trend_uri.clone(), trend, 1))
        .await;
    service
        .inner()
        .did_open(open_params(sims_uri.clone(), sims, 1))
        .await;
    let items = workspace_items(
        service
            .inner()
            .workspace_diagnostic(workspace_pull_params())
            .await
            .expect("workspace pull"),
    );
    let uris: Vec<Url> = items.iter().map(|(u, _)| u.clone()).collect();
    assert!(uris.contains(&trend_uri), "missing trend: {uris:?}");
    assert!(uris.contains(&sims_uri), "missing sims: {uris:?}");
    let sims_diags = items
        .iter()
        .find(|(u, _)| u == &sims_uri)
        .map(|(_, d)| d)
        .expect("sims row");
    assert!(sims_diags.iter().any(|d| diag_code(d) == "I050"));
}

fn pos_at(text: &str, byte: usize) -> Position {
    let index = dygnosis::span::LineIndex::new(text);
    let p = index.position(text, byte as u32);
    Position::new(p.line, p.character)
}

fn tdp(uri: Url, text: &str, byte: usize) -> TextDocumentPositionParams {
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri },
        position: pos_at(text, byte),
    }
}

fn hover_markdown(h: Hover) -> String {
    match h.contents {
        HoverContents::Markup(m) => m.value,
        HoverContents::Scalar(MarkedString::String(s)) => s,
        other => panic!("unexpected hover contents: {other:?}"),
    }
}

fn completion_labels(resp: CompletionResponse) -> Vec<String> {
    match resp {
        CompletionResponse::Array(items) => items.into_iter().map(|i| i.label).collect(),
        CompletionResponse::List(list) => list.items.into_iter().map(|i| i.label).collect(),
    }
}

#[allow(deprecated)]
fn nested_symbols(resp: DocumentSymbolResponse) -> Vec<DocumentSymbol> {
    match resp {
        DocumentSymbolResponse::Nested(syms) => syms,
        DocumentSymbolResponse::Flat(_) => panic!("expected Nested document symbols, got Flat"),
    }
}

fn top_level_names(syms: &[DocumentSymbol]) -> Vec<&str> {
    syms.iter().map(|s| s.name.as_str()).collect()
}

fn child_names<'a>(syms: &'a [DocumentSymbol], parent: &str) -> Vec<&'a str> {
    let parent_sym = syms
        .iter()
        .find(|s| s.name == parent)
        .unwrap_or_else(|| panic!("missing outline parent {parent:?}"));
    parent_sym
        .children
        .as_ref()
        .map(|c| c.iter().map(|s| s.name.as_str()).collect())
        .unwrap_or_default()
}

fn assert_no_symbol_named(syms: &[DocumentSymbol], forbidden: &str) {
    for s in syms {
        assert_ne!(
            s.name.as_str(),
            forbidden,
            "outline still has {forbidden:?}"
        );
        if let Some(children) = &s.children {
            assert_no_symbol_named(children, forbidden);
        }
    }
}

fn fixture_mod(rel: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel);
    path.canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize {}: {e}", path.display()))
}

fn read_fixture_mod(rel: &str) -> String {
    let path = fixture_mod(rel);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn companion_open(rel: &str) -> (Url, String) {
    let path = fixture_mod(&format!("companions/{rel}"));
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n");
    (file_url(&path), text)
}

fn link_params(uri: Url) -> DocumentLinkParams {
    DocumentLinkParams {
        text_document: TextDocumentIdentifier { uri },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

fn definition_params(uri: Url, text: &str, byte: usize) -> GotoDefinitionParams {
    GotoDefinitionParams {
        text_document_position_params: tdp(uri, text, byte),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

fn link_target_contains(link: &DocumentLink, needle: &str) -> bool {
    link.target
        .as_ref()
        .is_some_and(|u| u.as_str().contains(needle))
}

fn byte_on_trimmed_line(text: &str, trimmed: &str) -> usize {
    let mut off = 0usize;
    for line in text.lines() {
        if line.trim() == trimmed {
            return off + line.find(trimmed).expect("trimmed needle");
        }
        off += line.len() + 1;
    }
    panic!("missing line {trimmed:?}");
}

fn zero_start_range() -> Range {
    Range::new(Position::new(0, 0), Position::new(0, 0))
}

fn slice_range(text: &str, range: Range) -> String {
    let index = dygnosis::span::LineIndex::new(text);
    let start = index.offset(
        text,
        dygnosis::span::Position {
            line: range.start.line,
            character: range.start.character,
        },
    );
    let end = index.offset(
        text,
        dygnosis::span::Position {
            line: range.end.line,
            character: range.end.character,
        },
    );
    text[start as usize..end as usize].to_string()
}

fn apply_edits(text: &str, edits: &[TextEdit]) -> String {
    let mut spans: Vec<(u32, u32, &str)> = edits
        .iter()
        .map(|e| {
            let index = dygnosis::span::LineIndex::new(text);
            let start = index.offset(
                text,
                dygnosis::span::Position {
                    line: e.range.start.line,
                    character: e.range.start.character,
                },
            );
            let end = index.offset(
                text,
                dygnosis::span::Position {
                    line: e.range.end.line,
                    character: e.range.end.character,
                },
            );
            (start, end, e.new_text.as_str())
        })
        .collect();
    spans.sort_by_key(|(s, _, _)| std::cmp::Reverse(*s));
    let mut out = text.to_string();
    for (start, end, new) in spans {
        out.replace_range(start as usize..end as usize, new);
    }
    out
}

fn first_assignment_betta(text: &str) -> usize {
    text.find("betta   =")
        .or_else(|| text.find("betta ="))
        .expect("betta assignment")
}

fn copilot_file(archive_dir: &str, name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/model-archive")
        .join(archive_dir)
        .join(name);
    path.canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize {}: {e}", path.display()))
}

fn read_copilot_file(archive_dir: &str, name: &str) -> String {
    let path = copilot_file(archive_dir, name);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

#[test]
fn initialize_capabilities_wave_b() {
    let result = initialize_result();
    assert!(result.capabilities.hover_provider.is_some());
    assert!(result.capabilities.completion_provider.is_some());
    assert!(result.capabilities.rename_provider.is_some());
    assert!(result.capabilities.code_action_provider.is_some());
    assert!(result.capabilities.document_symbol_provider.is_some());
    assert!(result.capabilities.workspace_symbol_provider.is_some());
    assert!(result.capabilities.definition_provider.is_some());
    assert!(result.capabilities.declaration_provider.is_some());
    assert!(result.capabilities.type_definition_provider.is_some());
    assert!(result.capabilities.references_provider.is_some());
    assert!(result.capabilities.document_highlight_provider.is_some());
    assert!(result.capabilities.linked_editing_range_provider.is_some());
    assert!(result.capabilities.signature_help_provider.is_none());
    assert_eq!(
        result.capabilities.text_document_sync,
        Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL))
    );

    let json = serde_json::to_string(&result).expect("serialize initialize result");
    for forbidden in [
        "dynare/computeSteadyState",
        "dynare/runDynare",
        "Compute Steady State",
    ] {
        assert!(
            !json.contains(forbidden),
            "initialize JSON contains {forbidden:?}: {json}"
        );
    }
}

#[tokio::test]
async fn hover_betta_is_parameter_no_ss_solve() {
    let text = read_mod("trend_rbc_gov_inv");
    let uri = archive_url("trend_rbc_gov_inv");
    let byte = first_assignment_betta(&text);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let hover = service
        .inner()
        .hover(HoverParams {
            text_document_position_params: tdp(uri, &text, byte),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("hover rpc")
        .expect("hover");
    let md = hover_markdown(hover);
    assert!(md.contains("Parameter"), "hover: {md}");
    assert!(md.contains("0.99") || md.contains("Value:"), "hover: {md}");
    assert!(!md.contains("Computed steady state"));
    assert!(!md.contains("Compute Steady State"));
}

#[tokio::test]
async fn hover_endogenous_includes_timing() {
    let text = read_mod("trend_rbc_gov_inv");
    let uri = archive_url("trend_rbc_gov_inv");
    let byte = text.find("k(-1)^alppha").expect("k lag in production");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let hover = service
        .inner()
        .hover(HoverParams {
            text_document_position_params: tdp(uri, &text, byte),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("hover rpc")
        .expect("hover");
    let md = hover_markdown(hover);
    assert!(md.contains("Endogenous"), "hover: {md}");
    assert!(md.contains("Timing:"), "hover: {md}");
    assert!(!md.contains("Computed steady state"));
}

#[tokio::test]
async fn hover_stoch_simul_option() {
    let text = read_mod("trend_rbc_gov_inv");
    let uri = archive_url("trend_rbc_gov_inv");
    let byte = text.find("order=1").expect("order option");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let hover = service
        .inner()
        .hover(HoverParams {
            text_document_position_params: tdp(uri, &text, byte),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("hover rpc")
        .expect("hover");
    let md = hover_markdown(hover);
    assert!(md.contains("stoch_simul"), "hover: {md}");
    assert!(md.contains("option"), "hover: {md}");
    assert!(md.contains("`order`"), "hover: {md}");
}

#[tokio::test]
async fn document_symbols_timing_class_groups() {
    let text = read_mod("trend_rbc_gov_inv");
    let uri = archive_url("trend_rbc_gov_inv");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text, 1))
        .await;
    let resp = service
        .inner()
        .document_symbol(DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("symbols rpc")
        .expect("symbols");
    let nested = nested_symbols(resp);
    assert_eq!(
        top_level_names(&nested),
        [
            "predetermined",
            "forward-looking",
            "static",
            "varexo (exogenous)",
            "parameters",
            "model",
        ]
    );
    assert_no_symbol_named(&nested, "var (endogenous)");
    assert_eq!(
        child_names(&nested, "predetermined"),
        ["k", "ig", "kg", "z"]
    );
    assert_eq!(child_names(&nested, "forward-looking"), ["c", "rk"]);
    assert_eq!(
        child_names(&nested, "static"),
        ["y", "n", "invest", "w", "log_y", "log_c", "log_k", "log_kg", "log_ig", "log_n"]
    );
    assert!(
        nested.iter().all(|s| s.name != "mixed"),
        "empty mixed group must be omitted"
    );
}

#[tokio::test]
async fn document_symbols_omit_empty_timing_groups() {
    let text = read_fixture_mod("lsp/outline_timing.mod");
    let path = fixture_mod("lsp/outline_timing.mod");
    let uri = file_url(&path);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let resp = service
        .inner()
        .document_symbol(DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("symbols rpc")
        .expect("symbols");
    let nested = nested_symbols(resp);
    assert_eq!(
        top_level_names(&nested),
        [
            "predetermined",
            "mixed",
            "static",
            "varexo (exogenous)",
            "parameters",
            "model",
        ]
    );
    assert_no_symbol_named(&nested, "var (endogenous)");
    assert!(
        nested.iter().all(|s| s.name != "forward-looking"),
        "empty forward-looking group must be omitted"
    );
    assert_eq!(child_names(&nested, "predetermined"), ["y"]);
    assert_eq!(child_names(&nested, "mixed"), ["k"]);
    assert_eq!(child_names(&nested, "static"), ["w", "a", "u"]);
    let model = dygnosis::parse(&text);
    let model_children = nested
        .iter()
        .find(|s| s.name == "model")
        .and_then(|s| s.children.as_ref())
        .expect("model children");
    assert_eq!(model_children.len(), model.equations.len());
}

#[tokio::test]
async fn workspace_symbols_across_open_docs() {
    let trend = read_mod("trend_rbc_gov_inv");
    let sims = read_mod("sims_wu_2019");
    let trend_uri = archive_url("trend_rbc_gov_inv");
    let sims_uri = archive_url("sims_wu_2019");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(trend_uri.clone(), trend, 1))
        .await;
    service
        .inner()
        .did_open(open_params(sims_uri.clone(), sims, 1))
        .await;
    let symbols = service
        .inner()
        .symbol(WorkspaceSymbolParams {
            query: String::new(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("workspace symbol rpc")
        .expect("workspace symbols");
    assert!(
        symbols.iter().any(|s| s.location.uri == trend_uri),
        "trend missing from workspace symbols"
    );
    assert!(
        symbols.iter().any(|s| s.location.uri == sims_uri),
        "sims missing from workspace symbols"
    );
}

#[tokio::test]
async fn definition_jumps_to_decl() {
    let text = read_mod("trend_rbc_gov_inv");
    let uri = archive_url("trend_rbc_gov_inv");
    let use_byte = first_assignment_betta(&text);
    let decl_line = text
        .lines()
        .position(|l| {
            l.contains("parameters betta") || l.trim_start().starts_with("parameters betta")
        })
        .or_else(|| {
            text.lines()
                .position(|l| l.contains("parameters") && text.lines().any(|x| x.contains("betta")))
        })
        .expect("parameters line") as u32;
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let loc = match service
        .inner()
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: tdp(uri.clone(), &text, use_byte),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("definition rpc")
        .expect("definition")
    {
        GotoDefinitionResponse::Scalar(loc) => loc,
        GotoDefinitionResponse::Array(mut locs) => locs.remove(0),
        other => panic!("unexpected definition: {other:?}"),
    };
    assert_eq!(loc.uri, uri);
    assert_eq!(
        loc.range.start.line, decl_line,
        "expected parameters-line decl, got line {}",
        loc.range.start.line
    );
}

#[tokio::test]
async fn declaration_and_type_definition_match_definition() {
    let text = read_mod("trend_rbc_gov_inv");
    let uri = archive_url("trend_rbc_gov_inv");
    let byte = first_assignment_betta(&text);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let params = GotoDefinitionParams {
        text_document_position_params: tdp(uri, &text, byte),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let definition = service
        .inner()
        .goto_definition(params.clone())
        .await
        .expect("definition rpc")
        .expect("definition");
    let declaration = service
        .inner()
        .goto_declaration(params.clone())
        .await
        .expect("declaration rpc")
        .expect("declaration");
    let type_definition = service
        .inner()
        .goto_type_definition(params)
        .await
        .expect("type definition rpc")
        .expect("type definition");
    assert_eq!(definition, declaration);
    assert_eq!(definition, type_definition);
}

#[tokio::test]
async fn references_skip_comment() {
    let mut text = read_mod("trend_rbc_gov_inv");
    text.insert_str(0, "// betta\n");
    let uri = archive_url("trend_rbc_gov_inv");
    let comment_byte = text.find("// betta").unwrap() + 3;
    let real_byte = first_assignment_betta(&text);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let refs = service
        .inner()
        .references(ReferenceParams {
            text_document_position: tdp(uri.clone(), &text, real_byte),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .expect("references rpc")
        .expect("references");
    assert!(!refs.is_empty(), "expected at least one real ident hit");
    let comment_range = {
        let start = pos_at(&text, comment_byte);
        let end = pos_at(&text, comment_byte + 5);
        Range::new(start, end)
    };
    for loc in &refs {
        assert_ne!(
            loc.range, comment_range,
            "comment span must not be a reference"
        );
        let slice = slice_range(&text, loc.range);
        assert_eq!(slice, "betta", "reference slice {slice:?}");
        assert!(
            loc.range.start != comment_range.start,
            "reference must not start at comment betta"
        );
    }
}

#[tokio::test]
async fn highlight_betta() {
    let text = read_mod("trend_rbc_gov_inv");
    let uri = archive_url("trend_rbc_gov_inv");
    let byte = first_assignment_betta(&text);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let hits = service
        .inner()
        .document_highlight(DocumentHighlightParams {
            text_document_position_params: tdp(uri, &text, byte),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("highlight rpc")
        .expect("highlights");
    assert!(!hits.is_empty());
    for h in hits {
        assert_eq!(slice_range(&text, h.range), "betta");
    }
}

#[tokio::test]
async fn completion_stoch_simul_options() {
    let text = read_mod("trend_rbc_gov_inv");
    let uri = archive_url("trend_rbc_gov_inv");
    let byte = text.find("stoch_simul(").expect("stoch_simul") + "stoch_simul(".len();
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let labels = completion_labels(
        service
            .inner()
            .completion(CompletionParams {
                text_document_position: tdp(uri, &text, byte),
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: None,
            })
            .await
            .expect("completion rpc")
            .expect("completion"),
    );
    assert!(
        labels.iter().any(|l| l == "order" || l == "irf"),
        "stoch_simul options missing order/irf: {labels:?}"
    );
}

/// The six family names this slice added to `DYNARE_KEYWORDS`, so command-name
/// completion offers them: the list is hand-kept and does not read the catalog.
#[tokio::test]
async fn completion_offers_the_moment_family_keywords() {
    let text = "var y;\n\n".to_string();
    let uri = Url::parse("file:///tmp/moment_keywords_test.mod").unwrap();
    let byte = text.len() - 1;
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let labels = completion_labels(
        service
            .inner()
            .completion(CompletionParams {
                text_document_position: tdp(uri, &text, byte),
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: None,
            })
            .await
            .expect("completion rpc")
            .expect("completion"),
    );
    for name in [
        "method_of_moments",
        "matched_moments",
        "matched_irfs",
        "matched_irfs_weights",
        "irf_calibration",
        "moment_calibration",
    ] {
        assert!(
            labels.iter().any(|l| l == name),
            "keyword {name} missing: {labels:?}"
        );
    }
}

/// The two IRF blocks the catalog gained: `overwrite` reaches hover and
/// completion through the command name before the parenthesis.
#[tokio::test]
async fn matched_irfs_overwrite_reaches_hover_and_completion() {
    let text = "var y c;\nvarexo e;\nparameters a;\na = 0.5;\nmodel;\ny = a*y(-1) + e;\nc = y;\nend;\n\nmatched_irfs(overwrite);\nvar y; varexo e; periods 1; values 1; end;\n";
    let uri = Url::parse("file:///tmp/matched_irfs_overwrite_test.mod").unwrap();
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.to_string(), 1))
        .await;

    let byte = text.find("overwrite").expect("overwrite word");
    let md = hover_markdown(
        service
            .inner()
            .hover(HoverParams {
                text_document_position_params: tdp(uri.clone(), text, byte),
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .expect("hover rpc")
            .expect("hover"),
    );
    assert!(md.contains("matched_irfs"), "hover: {md}");
    assert!(md.contains("`overwrite`"), "hover: {md}");

    let byte = text.find("matched_irfs(").expect("matched_irfs opener") + "matched_irfs(".len();
    let labels = completion_labels(
        service
            .inner()
            .completion(CompletionParams {
                text_document_position: tdp(uri, text, byte),
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: None,
            })
            .await
            .expect("completion rpc")
            .expect("completion"),
    );
    assert!(
        labels.iter().any(|l| l == "overwrite"),
        "matched_irfs option overwrite missing: {labels:?}"
    );
}

#[tokio::test]
async fn shock_overwrite_hover_uses_its_block_description() {
    const BASE: &str = "var y; varexo e; parameters p; p=.5; model; y=p*y(-1)+e; end;\n";
    for (command, option, body, expected, unrelated) in [
        (
            "shocks",
            "overwrite",
            "end;",
            "other skew rows remain",
            "earlier shock_paths blocks",
        ),
        (
            "shocks",
            "OVERWRITE",
            "end;",
            "other skew rows remain",
            "earlier shock_paths blocks",
        ),
        (
            "mshocks",
            "overwrite",
            "var e; periods 2; values 1.1; end;",
            "earlier deterministic shocks",
            "measurement-error settings",
        ),
        (
            "shock_paths",
            "overwrite",
            "var e; periods 2; values 1; end;",
            "perfect_foresight_controlled_paths entries",
            "measurement-error settings",
        ),
    ] {
        let text = format!("{BASE}{command}({option}); {body}\n");
        let uri = Url::parse(&format!("file:///tmp/{command}_{option}_catalog.mod")).unwrap();
        let (service, _socket) = new_service();
        service
            .inner()
            .did_open(open_params(uri.clone(), text.clone(), 1))
            .await;

        let option_byte = text.find(option).unwrap();
        let md = hover_markdown(
            service
                .inner()
                .hover(HoverParams {
                    text_document_position_params: tdp(uri.clone(), &text, option_byte),
                    work_done_progress_params: WorkDoneProgressParams::default(),
                })
                .await
                .expect("hover rpc")
                .expect("hover"),
        );
        assert!(md.contains(expected), "{command} hover: {md}");
        assert!(!md.contains(unrelated), "{command} hover: {md}");
        assert!(md.contains(&format!("`{option}`")), "{command} hover: {md}");

        let labels = completion_labels(
            service
                .inner()
                .completion(CompletionParams {
                    text_document_position: tdp(uri, &text, option_byte),
                    work_done_progress_params: WorkDoneProgressParams::default(),
                    partial_result_params: PartialResultParams::default(),
                    context: None,
                })
                .await
                .expect("completion rpc")
                .expect("completion"),
        );
        assert!(
            labels.contains(&"overwrite".to_string()),
            "{command}: {labels:?}"
        );
        assert!(
            labels.contains(&"learnt_in".to_string()),
            "{command}: {labels:?}"
        );
    }
}

#[tokio::test]
async fn completion_includes_exogenous() {
    let text = read_mod("trend_rbc_gov_inv");
    let uri = archive_url("trend_rbc_gov_inv");
    let byte = text.find("var ").expect("var decl");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let labels = completion_labels(
        service
            .inner()
            .completion(CompletionParams {
                text_document_position: tdp(uri, &text, byte),
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: None,
            })
            .await
            .expect("completion rpc")
            .expect("completion"),
    );
    assert!(
        labels.iter().any(|l| l == "eps_z" || l.starts_with("eps_")),
        "default completion missing varexo: {labels:?}"
    );
}

#[tokio::test]
async fn completion_nsam_spelling() {
    let text = "sensitivity(\n".to_string();
    let uri = Url::parse("file:///tmp/nsam_test.mod").unwrap();
    let byte = text.find('(').unwrap() + 1;
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let labels = completion_labels(
        service
            .inner()
            .completion(CompletionParams {
                text_document_position: tdp(uri, &text, byte),
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: None,
            })
            .await
            .expect("completion rpc")
            .expect("completion"),
    );
    assert!(
        labels.iter().any(|l| l == "Nsam"),
        "expected exact Nsam, got {labels:?}"
    );
}

/// The dotted `prior` statement: its head is a symbol, so the option list is
/// reached through the word after the dot, not through a command name.
#[tokio::test]
async fn dotted_prior_reaches_hover_and_completion() {
    let text = "var y; parameters alpha;\nalpha.prior(shape=beta, mean=0.5, stdev=0.1);\n";
    let uri = Url::parse("file:///tmp/dotted_prior_test.mod").unwrap();
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.to_string(), 1))
        .await;

    let byte = text.find("stdev=").expect("stdev option");
    let md = hover_markdown(
        service
            .inner()
            .hover(HoverParams {
                text_document_position_params: tdp(uri.clone(), text, byte),
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .expect("hover rpc")
            .expect("hover"),
    );
    assert!(md.contains("prior"), "hover: {md}");
    assert!(md.contains("`stdev`"), "hover: {md}");

    let byte = text.find("mean=0.5").expect("mean option");
    let labels = completion_labels(
        service
            .inner()
            .completion(CompletionParams {
                text_document_position: tdp(uri, text, byte),
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: None,
            })
            .await
            .expect("completion rpc")
            .expect("completion"),
    );
    for name in ["shape", "domain", "variance"] {
        assert!(
            labels.iter().any(|l| l == name),
            "prior option {name} missing: {labels:?}"
        );
    }
}

#[tokio::test]
async fn prepare_rename_rejects_comment() {
    let mut text = read_mod("trend_rbc_gov_inv");
    text.insert_str(0, "// betta\n");
    let uri = archive_url("trend_rbc_gov_inv");
    let comment_byte = text.find("// betta").unwrap() + 3;
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let prep = service
        .inner()
        .prepare_rename(tdp(uri, &text, comment_byte))
        .await
        .expect("prepare_rename rpc");
    assert!(
        prep.is_none(),
        "comment cursor must not be renameable: {prep:?}"
    );
}

#[tokio::test]
async fn rename_betta_in_file() {
    let text = read_mod("trend_rbc_gov_inv");
    let uri = archive_url("trend_rbc_gov_inv");
    let byte = first_assignment_betta(&text);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let edit = service
        .inner()
        .rename(RenameParams {
            text_document_position: tdp(uri.clone(), &text, byte),
            new_name: "beta_disc".into(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("rename rpc")
        .expect("rename edit");
    let changes = edit.changes.expect("changes");
    let edits = changes.get(&uri).expect("edits for trend");
    let new_text = apply_edits(&text, edits);
    assert!(new_text.contains("beta_disc"));
    assert!(
        !new_text.contains("betta =") && !new_text.contains("betta   ="),
        "leftover betta assignment in {new_text}"
    );
}

#[tokio::test]
async fn rename_swff_cross_file() {
    let mod_text = read_copilot_file("swff", "swff.mod");
    let inc_text = read_copilot_file("swff", "swff_params.inc");
    let mod_uri = file_url(&copilot_file("swff", "swff.mod"));
    let inc_uri = file_url(&copilot_file("swff", "swff_params.inc"));
    let shared = ["alppha", "rho", "gstar"]
        .into_iter()
        .find(|n| mod_text.contains(n) && inc_text.contains(n))
        .expect("shared ident");
    let byte = mod_text.find(shared).expect("ident in swff.mod");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(mod_uri.clone(), mod_text.clone(), 1))
        .await;
    service
        .inner()
        .did_open(open_params(inc_uri.clone(), inc_text.clone(), 1))
        .await;
    let edit = service
        .inner()
        .rename(RenameParams {
            text_document_position: tdp(mod_uri.clone(), &mod_text, byte),
            new_name: format!("{shared}_renamed"),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("rename rpc")
        .expect("rename edit");
    let changes = edit.changes.expect("changes");
    let mod_edits = changes.get(&mod_uri).map(|e| e.len()).unwrap_or(0);
    let inc_edits = changes.get(&inc_uri).map(|e| e.len()).unwrap_or(0);
    assert!(
        mod_edits > 0 && inc_edits > 0,
        "expected nonempty edits in both open buffers; mod={mod_edits} inc={inc_edits}"
    );
}

#[tokio::test]
async fn code_action_e001_fix() {
    let mutated =
        read_mod("trend_rbc_gov_inv").replacen("log_n = log(n);\n\nend;", "log_n = log(n);\n", 1);
    let uri = archive_url("trend_rbc_gov_inv");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), mutated, 1))
        .await;
    let items = pull_items(
        service
            .inner()
            .diagnostic(pull_params(uri.clone()))
            .await
            .expect("pull"),
    );
    let e001 = items.iter().find(|d| diag_code(d) == "E001").expect("E001");
    let actions = service
        .inner()
        .code_action(CodeActionParams {
            text_document: TextDocumentIdentifier { uri },
            range: e001.range,
            context: CodeActionContext {
                diagnostics: vec![e001.clone()],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("codeAction rpc")
        .expect("actions");
    let action = actions
        .iter()
        .find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => Some(ca),
            CodeActionOrCommand::Command(_) => None,
        })
        .expect("code action");
    assert_eq!(action.kind.as_ref(), Some(&CodeActionKind::QUICKFIX));
    assert!(action.edit.is_some(), "expected WorkspaceEdit");
    assert!(
        !action.title.contains("Compute Steady State"),
        "title: {}",
        action.title
    );
}

#[tokio::test]
async fn linked_editing_betta() {
    let text = read_mod("trend_rbc_gov_inv");
    let uri = archive_url("trend_rbc_gov_inv");
    let byte = first_assignment_betta(&text);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let ranges = service
        .inner()
        .linked_editing_range(LinkedEditingRangeParams {
            text_document_position_params: tdp(uri, &text, byte),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("linked rpc")
        .expect("linked ranges");
    assert!(
        ranges.ranges.len() >= 2,
        "expected ≥2 linked ranges, got {}",
        ranges.ranges.len()
    );
}

fn full_range(text: &str) -> Range {
    let lines: Vec<&str> = text.split('\n').collect();
    let last = lines.last().copied().unwrap_or("");
    Range::new(
        Position::new(0, 0),
        Position::new(
            lines.len().saturating_sub(1) as u32,
            last.chars().count() as u32,
        ),
    )
}

fn fmt_opts() -> FormattingOptions {
    FormattingOptions {
        tab_size: 4,
        insert_spaces: false,
        properties: Default::default(),
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    }
}

fn inlay_label(h: &InlayHint) -> String {
    match &h.label {
        InlayHintLabel::String(s) => s.clone(),
        InlayHintLabel::LabelParts(parts) => parts.iter().map(|p| p.value.as_str()).collect(),
    }
}

fn assert_no_out_json(json: &str) {
    for forbidden in [
        "dynare/computeSteadyState",
        "dynare/runDynare",
        "Compute Steady State",
    ] {
        assert!(
            !json.contains(forbidden),
            "JSON contains {forbidden:?}: {json}"
        );
    }
}

#[test]
fn initialize_capabilities_wave_c() {
    let result = initialize_result();
    assert!(result.capabilities.inlay_hint_provider.is_some());
    assert!(result.capabilities.folding_range_provider.is_some());
    assert!(result.capabilities.selection_range_provider.is_some());
    assert!(result.capabilities.document_link_provider.is_some());
    assert!(result.capabilities.semantic_tokens_provider.is_some());
    assert!(result.capabilities.code_lens_provider.is_some());
    assert!(result.capabilities.call_hierarchy_provider.is_some());
    assert!(result.capabilities.document_formatting_provider.is_some());
    assert!(result
        .capabilities
        .document_range_formatting_provider
        .is_some());
    assert!(result.capabilities.signature_help_provider.is_none());
    assert_eq!(
        result.capabilities.text_document_sync,
        Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL))
    );
    let commands = result
        .capabilities
        .execute_command_provider
        .as_ref()
        .expect("executeCommand")
        .commands
        .clone();
    assert_eq!(commands.len(), 3, "commands: {commands:?}");
    assert!(commands.contains(&"dynare/explainDiagnostic".into()));
    assert!(commands.contains(&"dynare/compareModels".into()));
    assert!(commands.contains(&"dynare/showEffectiveModel".into()));
    assert!(!commands.contains(&"dynare/runPreprocessor".into()));
    assert!(!commands.iter().any(|c| c.contains("computeSteadyState")));
    assert!(!commands.iter().any(|c| c.contains("runDynare")));

    let json = serde_json::to_string(&result).expect("serialize initialize result");
    assert_no_out_json(&json);
}

#[tokio::test]
async fn inlay_betta_assignment_has_arrow_number() {
    let text = read_mod("trend_rbc_gov_inv");
    let uri = archive_url("trend_rbc_gov_inv");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let hints = service
        .inner()
        .inlay_hint(InlayHintParams {
            text_document: TextDocumentIdentifier { uri },
            range: full_range(&text),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("inlay rpc")
        .expect("hints");
    let joined: String = hints.iter().map(inlay_label).collect::<Vec<_>>().join(" ");
    assert!(
        joined.contains('→') && joined.chars().any(|c| c.is_ascii_digit()),
        "inlay labels: {joined}"
    );
    assert!(!joined.contains("SS OK"));
    assert!(!joined.contains("residual"));
    assert!(!joined.contains("Compute"));
}

#[tokio::test]
async fn folding_includes_model_block() {
    let text = read_mod("trend_rbc_gov_inv");
    let uri = archive_url("trend_rbc_gov_inv");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text, 1))
        .await;
    let ranges = service
        .inner()
        .folding_range(FoldingRangeParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("folding rpc")
        .expect("ranges");
    assert!(
        ranges.iter().any(|r| {
            r.collapsed_text
                .as_deref()
                .is_some_and(|t| t.contains("model"))
                || r.kind == Some(FoldingRangeKind::Region)
        }),
        "folding: {ranges:?}"
    );
}

#[tokio::test]
async fn selection_range_at_betta_expands() {
    let text = read_mod("trend_rbc_gov_inv");
    let uri = archive_url("trend_rbc_gov_inv");
    let byte = first_assignment_betta(&text);
    let pos = pos_at(&text, byte);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let sels = service
        .inner()
        .selection_range(SelectionRangeParams {
            text_document: TextDocumentIdentifier { uri },
            positions: vec![pos],
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("selection rpc")
        .expect("selection");
    let first = sels.first().expect("one selection");
    let ident = slice_range(&text, first.range);
    assert!(ident.contains("betta"), "innermost range {ident:?}");
    assert!(first.parent.is_some(), "expected parent expansion");
}

#[tokio::test]
async fn document_links_swff_params() {
    let text = read_mod("swff");
    let uri = archive_url("swff");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text, 1))
        .await;
    let links = service
        .inner()
        .document_link(DocumentLinkParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("links rpc")
        .expect("links");
    assert!(
        links.iter().any(|l| {
            l.target
                .as_ref()
                .map(|u| u.as_str().contains("swff_params"))
                .unwrap_or(false)
        }),
        "links: {links:?}"
    );
}

#[tokio::test]
async fn document_links_swff_run_script() {
    let text = read_mod("swff");
    let uri = archive_url("swff");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let links = service
        .inner()
        .document_link(link_params(uri))
        .await
        .expect("links rpc")
        .expect("links");
    let run = links
        .iter()
        .find(|l| link_target_contains(l, "run_swff.m"))
        .unwrap_or_else(|| panic!("run_script link missing: {links:?}"));
    assert_eq!(run.tooltip.as_deref(), Some("run_script run_swff.m"));
    assert_eq!(run.range.start.line, 0);
    assert_eq!(
        slice_range(&text, run.range),
        text.lines().next().expect("first line")
    );
    assert!(
        links
            .iter()
            .all(|l| !link_target_contains(l, "swff_ff_coeffs")),
        "swff_ff_coeffs.m is not a companion of swff.mod: {links:?}"
    );
}

#[tokio::test]
async fn document_links_ss_present_on_steady() {
    let (uri, text) = companion_open("ss_present/ss_present.mod");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let links = service
        .inner()
        .document_link(link_params(uri))
        .await
        .expect("links rpc")
        .expect("links");
    let ss = links
        .iter()
        .find(|l| link_target_contains(l, "ss_present_steadystate.m"))
        .unwrap_or_else(|| panic!("steady_state_file link missing: {links:?}"));
    assert_eq!(
        ss.tooltip.as_deref(),
        Some("steady_state_file ss_present_steadystate.m")
    );
    assert_eq!(slice_range(&text, ss.range), "steady");
    assert_ne!(
        ss.range.start.line, 0,
        "ss_present named_in is the steady command, not line 0"
    );
}

#[tokio::test]
async fn document_links_ident_helper() {
    let (uri, text) = companion_open("ident_helper/ident_helper.mod");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let links = service
        .inner()
        .document_link(link_params(uri))
        .await
        .expect("links rpc")
        .expect("links");
    let helper = links
        .iter()
        .find(|l| link_target_contains(l, "my_ss_helper.m"))
        .unwrap_or_else(|| panic!("helper_m link missing: {links:?}"));
    assert_eq!(helper.tooltip.as_deref(), Some("helper_m my_ss_helper.m"));
    assert_eq!(slice_range(&text, helper.range), "my_ss_helper");
}

#[tokio::test]
async fn document_links_data_file() {
    let (uri, text) = companion_open("data_file.mod");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let links = service
        .inner()
        .document_link(link_params(uri))
        .await
        .expect("links rpc")
        .expect("links");
    let data = links
        .iter()
        .find(|l| link_target_contains(l, "data_file.csv"))
        .unwrap_or_else(|| panic!("datafile link missing: {links:?}"));
    assert_eq!(data.tooltip.as_deref(), Some("datafile data_file.csv"));
    assert_eq!(slice_range(&text, data.range), "'data_file.csv'");
}

#[tokio::test]
async fn document_links_leftover_csv() {
    let (uri, text) = companion_open("leftover_csv.mod");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let links = service
        .inner()
        .document_link(link_params(uri))
        .await
        .expect("links rpc")
        .expect("links");
    let leftover = links
        .iter()
        .find(|l| link_target_contains(l, "leftover.csv"))
        .unwrap_or_else(|| panic!("leftover datafile link missing: {links:?}"));
    assert_eq!(leftover.tooltip.as_deref(), Some("datafile leftover.csv"));
    assert_eq!(slice_range(&text, leftover.range), "'leftover.csv'");
}

#[tokio::test]
async fn document_links_named_missing_has_no_companion_links() {
    let (uri, text) = companion_open("named_missing.mod");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text, 1))
        .await;
    let links = service
        .inner()
        .document_link(link_params(uri))
        .await
        .expect("links rpc")
        .unwrap_or_default();
    for banned in [
        "missing_data.csv",
        "missing_mode",
        "missing_data_file.csv",
        "missing_gsa.mat",
        "missing_initval.csv",
        "missing_histval.csv",
        "missing_ext",
        "missing_helper.m",
    ] {
        assert!(
            links.iter().all(|l| !link_target_contains(l, banned)),
            "unresolved {banned} must not be a document link: {links:?}"
        );
    }
}

#[tokio::test]
async fn definition_swff_first_line_jumps_to_run_script() {
    let text = read_mod("swff");
    let uri = archive_url("swff");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let loc = match service
        .inner()
        .goto_definition(definition_params(uri, &text, 0))
        .await
        .expect("definition rpc")
        .expect("definition")
    {
        GotoDefinitionResponse::Scalar(loc) => loc,
        other => panic!("expected Scalar run_script definition, got {other:?}"),
    };
    assert!(
        loc.uri.as_str().contains("run_swff.m"),
        "expected run_swff.m, got {}",
        loc.uri
    );
    assert_eq!(loc.range, zero_start_range());
}

#[tokio::test]
async fn definition_ss_present_steady_jumps_to_steadystate() {
    let (uri, text) = companion_open("ss_present/ss_present.mod");
    let byte = byte_on_trimmed_line(&text, "steady;");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let loc = match service
        .inner()
        .goto_definition(definition_params(uri, &text, byte))
        .await
        .expect("definition rpc")
        .expect("definition")
    {
        GotoDefinitionResponse::Scalar(loc) => loc,
        other => panic!("expected Scalar steady_state_file definition, got {other:?}"),
    };
    assert!(
        loc.uri.as_str().contains("ss_present_steadystate.m"),
        "expected ss_present_steadystate.m, got {}",
        loc.uri
    );
    assert_eq!(loc.range, zero_start_range());
}

#[tokio::test]
async fn definition_ident_helper_jumps_to_m() {
    let (uri, text) = companion_open("ident_helper/ident_helper.mod");
    let byte = text.find("my_ss_helper").expect("helper ident");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let loc = match service
        .inner()
        .goto_definition(definition_params(uri, &text, byte))
        .await
        .expect("definition rpc")
        .expect("definition")
    {
        GotoDefinitionResponse::Scalar(loc) => loc,
        other => panic!("expected Scalar helper_m definition, got {other:?}"),
    };
    assert!(
        loc.uri.as_str().contains("my_ss_helper.m"),
        "expected my_ss_helper.m, got {}",
        loc.uri
    );
    assert_eq!(loc.range, zero_start_range());
}

#[tokio::test]
async fn definition_data_file_and_leftover_csv() {
    let (uri, text) = companion_open("data_file.mod");
    let byte = text.find("'data_file.csv'").expect("datafile span") + 1;
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let loc = match service
        .inner()
        .goto_definition(definition_params(uri, &text, byte))
        .await
        .expect("definition rpc")
        .expect("definition")
    {
        GotoDefinitionResponse::Scalar(loc) => loc,
        other => panic!("expected Scalar datafile definition, got {other:?}"),
    };
    assert!(
        loc.uri.as_str().contains("data_file.csv"),
        "expected data_file.csv, got {}",
        loc.uri
    );
    assert_eq!(loc.range, zero_start_range());

    let (uri, text) = companion_open("leftover_csv.mod");
    let byte = text.find("'leftover.csv'").expect("leftover span") + 1;
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let loc = match service
        .inner()
        .goto_definition(definition_params(uri, &text, byte))
        .await
        .expect("definition rpc")
        .expect("definition")
    {
        GotoDefinitionResponse::Scalar(loc) => loc,
        other => panic!("expected Scalar leftover definition, got {other:?}"),
    };
    assert!(
        loc.uri.as_str().contains("leftover.csv"),
        "expected leftover.csv, got {}",
        loc.uri
    );
    assert_eq!(loc.range, zero_start_range());
}

#[tokio::test]
async fn definition_named_missing_is_none() {
    let (uri, text) = companion_open("named_missing.mod");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    for needle in ["missing_mode", "missing_ext", "'missing_data.csv'"] {
        let byte = text
            .find(needle)
            .unwrap_or_else(|| panic!("missing {needle}"));
        let resp = service
            .inner()
            .goto_definition(definition_params(uri.clone(), &text, byte))
            .await
            .expect("definition rpc");
        assert!(
            resp.is_none(),
            "unresolved {needle} must not jump: {resp:?}"
        );
    }
}

#[tokio::test]
async fn watched_delete_drops_companion_link() {
    let dir = std::env::temp_dir().join(format!(
        "dygnosis-lsp-companion-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let src_mod = fixture_mod("companions/ss_present/ss_present.mod");
    let src_ss = fixture_mod("companions/ss_present/ss_present_steadystate.m");
    let dst_mod = dir.join("ss_present.mod");
    let dst_ss = dir.join("ss_present_steadystate.m");
    fs::copy(&src_mod, &dst_mod).unwrap();
    fs::copy(&src_ss, &dst_ss).unwrap();
    let dst_mod = dst_mod.canonicalize().unwrap();
    let dst_ss = dst_ss.canonicalize().unwrap();
    let text = fs::read_to_string(&dst_mod).unwrap().replace("\r\n", "\n");
    let uri = file_url(&dst_mod);
    let ss_uri = file_url(&dst_ss);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text, 1))
        .await;
    let before = service
        .inner()
        .document_link(link_params(uri.clone()))
        .await
        .expect("links before")
        .expect("companion link before delete");
    assert!(
        before
            .iter()
            .any(|l| link_target_contains(l, "ss_present_steadystate.m")),
        "expected steadystate link before delete: {before:?}"
    );

    fs::remove_file(&dst_ss).unwrap();
    service
        .inner()
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent::new(ss_uri, FileChangeType::DELETED)],
        })
        .await;
    let after = service
        .inner()
        .document_link(link_params(uri))
        .await
        .expect("links after")
        .unwrap_or_default();
    assert!(
        after
            .iter()
            .all(|l| !link_target_contains(l, "ss_present_steadystate.m")),
        "DELETE of companion must drop the document link: {after:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn rename_skips_open_m_buffer() {
    let (mod_uri, mod_text) = companion_open("ident_helper/ident_helper.mod");
    let m_path = fixture_mod("companions/ident_helper/my_ss_helper.m");
    let m_text = fs::read_to_string(&m_path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", m_path.display()))
        .replace("\r\n", "\n");
    let m_uri = file_url(&m_path);
    let byte = byte_on_trimmed_line(&mod_text, "var y;");
    let y_byte = mod_text[byte..]
        .find('y')
        .map(|i| byte + i)
        .expect("y in var y");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(mod_uri.clone(), mod_text.clone(), 1))
        .await;
    service
        .inner()
        .did_open(open_params(m_uri.clone(), m_text, 1))
        .await;
    let edit = service
        .inner()
        .rename(RenameParams {
            text_document_position: tdp(mod_uri.clone(), &mod_text, y_byte),
            new_name: "y_renamed".into(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("rename rpc")
        .expect("rename edit");
    let changes = edit.changes.expect("changes");
    assert!(
        changes.get(&mod_uri).is_some_and(|e| !e.is_empty()),
        "expected edits in the .mod: {changes:?}"
    );
    assert!(
        !changes.contains_key(&m_uri),
        "rename must not put a .m URI in WorkspaceEdit: {changes:?}"
    );
    for uri in changes.keys() {
        let path = uri
            .to_file_path()
            .unwrap_or_else(|_| PathBuf::from(uri.path()));
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        assert!(
            ext == "mod" || ext == "inc",
            "rename URI must be .mod or .inc, got {uri}"
        );
        assert!(
            !matches!(ext.as_str(), "m" | "csv" | "mat" | "xls" | "xlsx"),
            "forbidden companion extension in rename: {uri}"
        );
    }
}

#[tokio::test]
async fn semantic_tokens_full_classifies_betta_or_y() {
    let text = read_mod("trend_rbc_gov_inv");
    let uri = archive_url("trend_rbc_gov_inv");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let tokens = service
        .inner()
        .semantic_tokens_full(SemanticTokensParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("semantic rpc")
        .expect("tokens");
    let data = match tokens {
        SemanticTokensResult::Tokens(t) => t.data,
        SemanticTokensResult::Partial(p) => p.data,
    };
    assert!(!data.is_empty(), "expected nonempty semantic tokens");
    let index = dygnosis::span::LineIndex::new(&text);
    let mut line = 0u32;
    let mut col = 0u32;
    let mut found = false;
    for tok in &data {
        if tok.delta_line == 0 {
            col += tok.delta_start;
        } else {
            line += tok.delta_line;
            col = tok.delta_start;
        }
        let start = index.offset(
            &text,
            dygnosis::span::Position {
                line,
                character: col,
            },
        );
        let end = index.offset(
            &text,
            dygnosis::span::Position {
                line,
                character: col + tok.length,
            },
        );
        let word = &text[start as usize..end as usize];
        if word == "betta" || word == "y" {
            found = true;
            break;
        }
    }
    assert!(found, "expected a betta or y token");
}

#[tokio::test]
async fn code_lens_structure_summary_no_out() {
    let text = read_mod("trend_rbc_gov_inv");
    let uri = archive_url("trend_rbc_gov_inv");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text, 1))
        .await;
    let lenses = service
        .inner()
        .code_lens(CodeLensParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("lens rpc")
        .expect("lenses");
    let titles: Vec<String> = lenses
        .iter()
        .filter_map(|l| l.command.as_ref().map(|c| c.title.clone()))
        .collect();
    let joined = titles.join(" | ");
    assert!(
        joined.contains("endogenous") && joined.contains("varexo"),
        "lenses: {joined}"
    );
    assert!(
        !lenses.iter().any(|l| {
            l.command
                .as_ref()
                .is_some_and(|c| c.command == "dynare/runPreprocessor")
        }),
        "runPreprocessor must not be a code lens; titles: {joined}"
    );
    for bad in [
        "Compute Steady State",
        "Run Dynare",
        "MATLAB",
        "computeSteadyState",
    ] {
        assert!(!joined.contains(bad), "lens contains {bad:?}: {joined}");
    }
    assert!(
        !lenses.iter().any(|l| {
            l.command
                .as_ref()
                .is_some_and(|c| c.command == "dynare/showEffectiveModel")
        }),
        "showEffectiveModel must not be a code lens"
    );
}

#[tokio::test]
async fn formatting_tight_ops_matches_format_text() {
    let text = "y = exp(z) * k(-1);\n";
    let uri = archive_url("trend_rbc_gov_inv");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.to_string(), 1))
        .await;
    let edits = service
        .inner()
        .formatting(DocumentFormattingParams {
            text_document: TextDocumentIdentifier { uri },
            options: fmt_opts(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("format rpc");
    let expected = dygnosis::format_text(text, "\t");
    match (edits, expected) {
        (None, None) => {}
        (Some(edits), None) => assert!(edits.is_empty(), "expected no edits, got {edits:?}"),
        (None, Some(formatted)) => panic!("LSP declined; library formatted to {formatted:?}"),
        (Some(edits), Some(formatted)) => {
            assert_eq!(edits.len(), 1);
            assert_eq!(edits[0].new_text, formatted);
        }
    }
}

#[tokio::test]
async fn range_formatting_matches_format_range() {
    let text = read_mod("trend_rbc_gov_inv");
    let uri = archive_url("trend_rbc_gov_inv");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let edits = service
        .inner()
        .range_formatting(DocumentRangeFormattingParams {
            text_document: TextDocumentIdentifier { uri },
            range: Range::new(Position::new(0, 0), Position::new(3, 0)),
            options: fmt_opts(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("range format rpc");
    let expected = dygnosis::format_range(&text, 0, 2, "\t");
    match (edits, expected) {
        (None, None) => {}
        (Some(edits), None) => assert!(edits.is_empty(), "expected no edits, got {edits:?}"),
        (None, Some((_, _, formatted))) => {
            panic!("LSP declined; library formatted to {formatted:?}")
        }
        (Some(edits), Some((_, _, formatted))) => {
            assert_eq!(edits.len(), 1);
            assert_eq!(edits[0].new_text, formatted);
        }
    }
}

#[tokio::test]
async fn explain_diagnostic_w013_and_unknown_e040() {
    let (service, _socket) = new_service();
    let w013 = service
        .inner()
        .execute_command(ExecuteCommandParams {
            command: "dynare/explainDiagnostic".into(),
            arguments: vec![serde_json::json!({"code": "W013"})],
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("explain rpc")
        .expect("w013 value");
    let expected = dygnosis::explain::render_markdown("W013").expect("library W013");
    assert_eq!(w013, serde_json::Value::String(expected));

    let e040 = service
        .inner()
        .execute_command(ExecuteCommandParams {
            command: "dynare/explainDiagnostic".into(),
            arguments: vec![serde_json::json!({"code": "E040"})],
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("explain e040 rpc")
        .expect("e040 value");
    let s = e040.as_str().expect("unknown string");
    assert!(
        s.to_ascii_lowercase().contains("not")
            && (s.to_ascii_lowercase().contains("documented")
                || s.to_ascii_lowercase().contains("known")),
        "E040 body looks documented: {s}"
    );
    assert!(!s.contains("Equation count does not match"));
}

#[tokio::test]
async fn compare_models_trend_vs_sims_no_ss_fields() {
    let trend = read_mod("trend_rbc_gov_inv");
    let sims = read_mod("sims_wu_2019");
    let uri_a = archive_url("trend_rbc_gov_inv");
    let uri_b = archive_url("sims_wu_2019");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri_a.clone(), trend, 1))
        .await;
    service
        .inner()
        .did_open(open_params(uri_b.clone(), sims, 1))
        .await;
    let value = service
        .inner()
        .execute_command(ExecuteCommandParams {
            command: "dynare/compareModels".into(),
            arguments: vec![serde_json::json!({
                "uri_a": uri_a.as_str(),
                "uri_b": uri_b.as_str(),
            })],
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("compare rpc")
        .expect("diff");
    let obj = value.as_object().expect("json object");
    assert!(obj.contains_key("added_endogenous") || obj.contains_key("removed_endogenous"));
    assert!(obj.contains_key("added_equations") || obj.contains_key("removed_equations"));
    for key in obj.keys() {
        let lower = key.to_ascii_lowercase();
        assert!(
            !lower.contains("steady_state")
                && !lower.contains("steadystate")
                && !lower.contains("computed"),
            "SS field in compareModels: {key}"
        );
    }
}

#[tokio::test]
async fn format_indent_config_uses_four_spaces() {
    let text = "var y;\nmodel;\ny=c;\nend;\n";
    let uri = archive_url("trend_rbc_gov_inv");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.to_string(), 1))
        .await;
    service
        .inner()
        .did_change_configuration(DidChangeConfigurationParams {
            settings: serde_json::json!({ "dynare": { "formatIndent": 4 } }),
        })
        .await;
    let edits = service
        .inner()
        .formatting(DocumentFormattingParams {
            text_document: TextDocumentIdentifier { uri },
            options: fmt_opts(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("format rpc");
    let expected = dygnosis::format_text(text, "    ");
    match (edits, expected) {
        (None, None) => {}
        (Some(edits), None) => assert!(edits.is_empty()),
        (None, Some(formatted)) => {
            panic!("LSP declined after formatIndent 4; library produced {formatted:?}")
        }
        (Some(edits), Some(formatted)) => {
            assert_eq!(edits.len(), 1);
            assert_eq!(edits[0].new_text, formatted);
            assert!(
                formatted.contains("    ") || formatted.contains("y"),
                "expected 4-space indent in {formatted:?}"
            );
        }
    }
}

#[tokio::test]
async fn call_hierarchy_prepare_on_endogenous() {
    let text = read_mod("trend_rbc_gov_inv");
    let uri = archive_url("trend_rbc_gov_inv");
    let byte = text.find("k(-1)^alppha").expect("k lag");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let items = service
        .inner()
        .prepare_call_hierarchy(CallHierarchyPrepareParams {
            text_document_position_params: tdp(uri, &text, byte),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("prepare rpc")
        .expect("items");
    assert!(
        items
            .iter()
            .any(|i| i.name == "k" || i.detail.as_deref() == Some("equation")),
        "items: {items:?}"
    );
    let variable = items
        .iter()
        .find(|i| i.name == "k")
        .cloned()
        .expect("variable item");
    let incoming = service
        .inner()
        .incoming_calls(CallHierarchyIncomingCallsParams {
            item: variable,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("incoming rpc")
        .expect("incoming");
    assert!(
        !incoming.is_empty(),
        "expected equations that use k via ident_refs"
    );
    let equation = items
        .iter()
        .find(|i| i.detail.as_deref() == Some("equation"))
        .cloned()
        .expect("equation item");
    let outgoing = service
        .inner()
        .outgoing_calls(CallHierarchyOutgoingCallsParams {
            item: equation,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("outgoing rpc")
        .expect("outgoing");
    assert!(
        outgoing.iter().any(|c| c.to.name == "k"),
        "outgoing missing k: {outgoing:?}"
    );
}

#[tokio::test]
async fn did_save_clean_trend_has_no_e001() {
    let text = read_mod("trend_rbc_gov_inv");
    let uri = archive_url("trend_rbc_gov_inv");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    service
        .inner()
        .did_save(save_params(uri.clone(), text))
        .await;
    let items = pull_items(
        service
            .inner()
            .diagnostic(pull_params(uri))
            .await
            .expect("pull after clean save"),
    );
    assert!(
        !items.iter().any(|d| diag_code(d) == "E001"),
        "clean save must not keep dropped E001; got {:?}",
        items.iter().map(diag_code).collect::<Vec<_>>()
    );
    assert!(
        !items
            .iter()
            .any(|d| d.severity == Some(DiagnosticSeverity::ERROR)),
        "clean save must not keep own Error; got {:?}",
        items.iter().map(diag_code).collect::<Vec<_>>()
    );
}

async fn show_effective_payload(
    service: &tower_lsp::LspService<dygnosis::server::Backend>,
    uri: &Url,
) -> serde_json::Value {
    service
        .inner()
        .execute_command(ExecuteCommandParams {
            command: "dynare/showEffectiveModel".into(),
            arguments: vec![serde_json::json!({"uri": uri.as_str()})],
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("showEffectiveModel rpc")
        .expect("payload")
}

#[tokio::test]
async fn show_effective_model_whole_eq_for() {
    let (uri, text) = expand_open("whole_eq_for.mod");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let payload = show_effective_payload(&service, &uri).await;
    let payload_uri = payload.get("uri").and_then(|v| v.as_str()).expect("uri");
    assert_eq!(payload_uri, uri.as_str());
    assert!(
        !payload_uri.contains("dygnosis-effective") && !payload_uri.starts_with("untitled:"),
        "virtual scheme uri: {payload_uri}"
    );
    let effective = payload
        .get("effective_text")
        .and_then(|v| v.as_str())
        .expect("effective_text");
    assert!(effective.contains("y = 1"), "effective_text: {effective}");
    assert!(effective.contains("y = 2"), "effective_text: {effective}");
    assert!(effective.contains("y = 3"), "effective_text: {effective}");
    let origins = payload
        .get("origins")
        .and_then(|v| v.as_array())
        .expect("origins");
    assert_eq!(origins.len(), 3, "origins: {origins:?}");
    let indexes: Vec<u64> = origins
        .iter()
        .map(|o| o.get("index").and_then(|i| i.as_u64()).expect("index"))
        .collect();
    assert_eq!(indexes, vec![0, 1, 2]);
    for origin in origins {
        let slice = slice_range(&text, json_range(origin));
        assert!(slice.contains("y = @{i}"), "origin slice {slice:?}");
        assert!(!slice.contains("@#define"), "origin slice {slice:?}");
        assert!(
            origin.get("origin_frames").is_none(),
            "origin_frames present: {origin}"
        );
        let origin_uri = origin
            .get("origin_uri")
            .and_then(|v| v.as_str())
            .expect("origin_uri");
        assert!(
            origin_uri.contains("whole_eq_for.mod"),
            "origin_uri {origin_uri}"
        );
    }
}

#[tokio::test]
async fn show_effective_model_include_eq() {
    let (mod_uri, mod_text) = expand_open("include_eq.mod");
    let (inc_uri, inc_text) = expand_open("include_eq_body.inc");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(mod_uri.clone(), mod_text, 1))
        .await;
    service
        .inner()
        .did_open(open_params(inc_uri.clone(), inc_text.clone(), 1))
        .await;
    let payload = show_effective_payload(&service, &mod_uri).await;
    let effective = payload
        .get("effective_text")
        .and_then(|v| v.as_str())
        .expect("effective_text");
    assert!(effective.contains("z = 0"), "effective_text: {effective}");
    let origins = payload
        .get("origins")
        .and_then(|v| v.as_array())
        .expect("origins");
    assert!(origins.len() >= 2, "origins: {origins:?}");
    let row0_uri = origins[0]
        .get("origin_uri")
        .and_then(|v| v.as_str())
        .expect("origins[0].origin_uri");
    assert!(
        row0_uri.contains("include_eq.mod"),
        "origins[0].origin_uri {row0_uri}"
    );
    let row1_uri = origins[1]
        .get("origin_uri")
        .and_then(|v| v.as_str())
        .expect("origins[1].origin_uri");
    assert!(
        row1_uri.contains("include_eq_body.inc"),
        "origins[1].origin_uri {row1_uri}"
    );
    let slice = slice_range(&inc_text, json_range(&origins[1]));
    assert!(slice.contains("z = 0"), "include origin slice {slice:?}");
}

#[tokio::test]
async fn show_effective_model_nested_for() {
    let (uri, text) = expand_open("nested_for.mod");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let payload = show_effective_payload(&service, &uri).await;
    let origins = payload
        .get("origins")
        .and_then(|v| v.as_array())
        .expect("origins");
    assert_eq!(origins.len(), 4, "origins: {origins:?}");
    for origin in origins {
        let frames = origin
            .get("origin_frames")
            .and_then(|v| v.as_array())
            .expect("origin_frames");
        assert_eq!(frames.len(), 2, "origin_frames: {frames:?}");
        assert!(
            frames
                .iter()
                .all(|f| f.get("kind").and_then(|k| k.as_str()) == Some("for")),
            "origin_frames: {frames:?}"
        );
        let inner = frames.last().expect("innermost frame");
        let slice = slice_range(&text, json_range(inner));
        assert!(slice.contains("x = @{i}"), "inner frame slice {slice:?}");
        assert!(!slice.contains("@#for"), "inner frame slice {slice:?}");
    }
}

#[tokio::test]
async fn show_effective_model_us_re09() {
    let path = copilot_example("US_RE09_rep");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        .replace("\r\n", "\n");
    let uri = file_url(&path);
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text.clone(), 1))
        .await;
    let payload = show_effective_payload(&service, &uri).await;
    let effective = payload
        .get("effective_text")
        .and_then(|v| v.as_str())
        .expect("effective_text");
    assert!(
        effective.contains("EXPECTATION(-1)"),
        "effective_text missing EXPECTATION(-1)"
    );
    assert!(
        effective.contains("EXPECTATION(-16)"),
        "effective_text missing EXPECTATION(-16)"
    );
    let origins = payload
        .get("origins")
        .and_then(|v| v.as_array())
        .expect("origins");
    assert_eq!(origins.len(), 19, "origins: {origins:?}");
    let row = &origins[2];
    assert!(
        row.get("origin_frames").is_none(),
        "phillips origin_frames present: {row}"
    );
    let slice = slice_range(&text, json_range(row));
    assert!(slice.contains("p = lambda"), "phillips origin {slice:?}");
    assert!(
        slice.contains("@#for lag in lags"),
        "phillips origin {slice:?}"
    );
    assert!(slice.contains("@#endfor"), "phillips origin {slice:?}");
}
