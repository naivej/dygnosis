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
    if dygnosis::find_preprocessor(None).is_some() {
        assert!(
            items.iter().any(
                |d| is_p_digits(&diag_code(d)) && d.severity == Some(DiagnosticSeverity::ERROR)
            ),
            "didSave with preprocessor should publish a P-digit ERROR; got {:?}",
            items.iter().map(diag_code).collect::<Vec<_>>()
        );
    } else {
        assert!(
            items.iter().any(|d| diag_code(d) == "E001"),
            "didSave mutation should publish E001"
        );
    }
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
fn symbol_names(resp: DocumentSymbolResponse) -> Vec<String> {
    fn walk(syms: &[DocumentSymbol], out: &mut Vec<String>) {
        for s in syms {
            out.push(s.name.clone());
            if let Some(children) = &s.children {
                walk(children, out);
            }
        }
    }
    match resp {
        DocumentSymbolResponse::Nested(syms) => {
            let mut out = Vec::new();
            walk(&syms, &mut out);
            out
        }
        DocumentSymbolResponse::Flat(info) => info.into_iter().map(|s| s.name).collect(),
    }
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
async fn document_symbols_have_var_varexo_parameters() {
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
    let names = symbol_names(resp);
    let joined = names.join(" | ");
    assert!(
        names
            .iter()
            .any(|n| n.contains("var") && n.contains("endogenous"))
            || names.iter().any(|n| n == "y" || n == "c"),
        "outline missing var/endogenous: {joined}"
    );
    assert!(
        names
            .iter()
            .any(|n| n.contains("varexo") || n.contains("exogenous"))
            || names.iter().any(|n| n.starts_with("eps_")),
        "outline missing varexo/exogenous: {joined}"
    );
    assert!(
        names.iter().any(|n| n == "parameters" || n == "betta"),
        "outline missing parameters: {joined}"
    );
    assert!(
        names.iter().any(|n| n == "model"),
        "outline missing model equations: {joined}"
    );
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
    assert!(commands.contains(&"dynare/runPreprocessor".into()));
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
    let pre = lenses
        .iter()
        .find(|l| {
            l.command
                .as_ref()
                .is_some_and(|c| c.command == "dynare/runPreprocessor")
        })
        .expect("runPreprocessor lens");
    let pre_cmd = pre.command.as_ref().unwrap();
    assert_eq!(pre_cmd.title, "Run preprocessor");
    assert_eq!(
        pre_cmd.arguments.as_ref(),
        Some(&vec![serde_json::json!({"uri": uri.as_str()})])
    );
    for bad in [
        "Compute Steady State",
        "Run Dynare",
        "MATLAB",
        "computeSteadyState",
    ] {
        assert!(!joined.contains(bad), "lens contains {bad:?}: {joined}");
    }
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
async fn run_preprocessor_command_trend_success() {
    let Some(_) = dygnosis::find_preprocessor(None) else {
        return;
    };
    let text = read_mod("trend_rbc_gov_inv");
    let uri = archive_url("trend_rbc_gov_inv");
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(open_params(uri.clone(), text, 1))
        .await;
    let value = service
        .inner()
        .execute_command(ExecuteCommandParams {
            command: "dynare/runPreprocessor".into(),
            arguments: vec![serde_json::json!({"uri": uri.as_str()})],
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("runPreprocessor rpc")
        .expect("payload");
    assert_eq!(
        value.get("success").and_then(|v| v.as_bool()),
        Some(true),
        "payload: {value}"
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
