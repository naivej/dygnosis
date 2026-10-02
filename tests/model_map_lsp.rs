use std::collections::HashMap;

use dygnosis::server::new_service;
use serde_json::{json, Value};
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

fn url(name: &str) -> Url {
    Url::parse(&format!("file:///C:/dygnosis-map-lsp/{name}")).unwrap()
}
async fn open(server: &dygnosis::server::Backend, uri: &Url, text: &str) {
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;
}
async fn info(server: &dygnosis::server::Backend, root: &Url, document: Option<&Url>) -> Value {
    let mut argument = json!({"root_uri":root});
    if let Some(document) = document {
        argument["document_uri"] = json!(document);
    }
    server
        .execute_command(ExecuteCommandParams {
            command: "dynare/modelInfo".to_string(),
            arguments: vec![argument],
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap()
}
async fn symbols(server: &dygnosis::server::Backend, uri: &Url) -> Vec<DocumentSymbol> {
    match server
        .document_symbol(DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap()
    {
        DocumentSymbolResponse::Nested(symbols) => symbols,
        _ => panic!("expected nested symbols"),
    }
}
async fn folds(server: &dygnosis::server::Backend, uri: &Url) -> Vec<FoldingRange> {
    server
        .folding_range(FoldingRangeParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap_or_default()
}
fn assert_local_tree(rows: &[DocumentSymbol], text: &str, parent: Option<Range>) {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<_> = normalized.split('\n').collect();
    for row in rows {
        for position in [
            row.range.start,
            row.range.end,
            row.selection_range.start,
            row.selection_range.end,
        ] {
            assert!((position.line as usize) < lines.len(), "{row:?}");
            assert!(
                position.character as usize <= lines[position.line as usize].encode_utf16().count(),
                "{row:?}"
            );
        }
        assert!(
            row.range.start <= row.selection_range.start
                && row.selection_range.end <= row.range.end,
            "{row:?}"
        );
        if let Some(parent) = parent {
            assert!(
                parent.start <= row.range.start && row.range.end <= parent.end,
                "{row:?}"
            );
        }
        if let Some(children) = &row.children {
            assert_local_tree(children, text, Some(row.range));
        }
    }
}
fn all<'a>(rows: &'a [DocumentSymbol], result: &mut Vec<&'a DocumentSymbol>) {
    for row in rows {
        result.push(row);
        if let Some(children) = &row.children {
            all(children, result);
        }
    }
}

fn original_token(text: &str, range: &Value) -> String {
    let target = range["start"]["line"].as_u64().unwrap() as usize;
    assert_eq!(range["end"]["line"].as_u64().unwrap() as usize, target);
    let bytes = text.as_bytes();
    let mut line = 0;
    let mut start = 0;
    let mut end = text.len();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' || bytes[i] == b'\n' {
            if line == target {
                end = i;
                break;
            }
            if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
                i += 1;
            }
            line += 1;
            start = i + 1;
        }
        i += 1;
    }
    assert_eq!(line, target);
    let original = &text[start..end];
    let offset = |units: usize| {
        let mut count = 0;
        for (byte, character) in original.char_indices() {
            if count == units {
                return byte;
            }
            count += character.len_utf16();
        }
        assert_eq!(count, units);
        original.len()
    };
    original[offset(range["start"]["character"].as_u64().unwrap() as usize)
        ..offset(range["end"]["character"].as_u64().unwrap() as usize)]
        .to_string()
}

#[tokio::test]
async fn model_info_shares_counts_keeps_metadata_and_supplies_exact_lens_anchors() {
    let root = url("root.mod");
    let child = url("params.inc");
    let root_text = "var y $Y$ (long_name='Output');\n@#include \"params.inc\"\nmodel; [name='first'] y=p; end;\nmodel; [name='second'] y=0; end;\n";
    let child_text = "parameters p ${p}$ (long_name='Calibration'); p=1;\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &child, child_text).await;
    open(server, &root, root_text).await;
    let result = info(server, &root, Some(&child)).await;
    assert_eq!(result["schema_version"], 1);
    assert_eq!(result["root_uri"], root.as_str());
    assert_eq!(result["document_uri"], child.as_str());
    assert!(result["revision"].as_str().is_some());
    assert_eq!(result["complete"], true);
    let files = HashMap::from([
        (root.as_str().to_string(), root_text.to_string()),
        (child.as_str().to_string(), child_text.to_string()),
    ]);
    let mcp = dygnosis::dynare_model_info(root_text, Some(root.as_str()), Some(&files));
    for (field, expected) in mcp.as_object().unwrap() {
        assert_eq!(&result[field], expected, "{field}");
    }
    let declarations = result["declarations"].as_array().unwrap();
    assert_eq!(declarations[1]["location"]["uri"], child.as_str());
    assert_eq!(declarations[1]["long_name"], "Calibration");
    assert_eq!(declarations[1]["tex_name"], "{p}");
    let models: Vec<_> = result["statements"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["name"] == "model")
        .collect();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0]["lens_anchor"]["range"]["start"]["line"], 2);
    assert_eq!(models[1]["lens_anchor"]["range"]["start"]["line"], 3);
    assert_eq!(models[0]["equation_count"], 1);
    assert_eq!(models[1]["equation_count"], 1);
    assert_eq!(result["equations"][0]["number"], 1);
    assert_eq!(result["equations"][1]["number"], 2);
    assert_ne!(
        result["equations"][0]["block_id"],
        result["equations"][1]["block_id"]
    );
    assert_eq!(result["block_categories"].as_array().unwrap().len(), 45);
    assert_eq!(result["related_files"][0]["filename"], "params.inc");
    server.did_change_configuration(DidChangeConfigurationParams { settings: json!({"nameDetails":{"longName":false,"tex":false},"outline":{"sections":[],"equationNumbers":false}}) }).await;
    assert!(symbols(server, &root).await.is_empty());
    let hidden = info(server, &root, None).await;
    assert_eq!(hidden["declarations"], result["declarations"]);
    assert_eq!(hidden["n_equations"], 2);
    assert_ne!(hidden["revision"], result["revision"]);
}

#[tokio::test]
async fn outline_and_folding_are_local_for_split_blocks_and_every_supported_kind() {
    let root = url("split.mod");
    let body = url("body.inc");
    let root_text = "var y;\nmodel;\n@#include \"body.inc\"\nend;\n";
    let body_text = "[name='body'] y=0;\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &body, body_text).await;
    open(server, &root, root_text).await;
    for (uri, text) in [(&root, root_text), (&body, body_text)] {
        assert_local_tree(&symbols(server, uri).await, text, None);
    }
    let local = symbols(server, &root).await;
    let mut flat = Vec::new();
    all(&local, &mut flat);
    assert!(!flat.iter().any(|row| row.name.ends_with("body")));
    let child_rows = symbols(server, &body).await;
    let mut flat = Vec::new();
    all(&child_rows, &mut flat);
    assert!(flat.iter().any(|row| row.name == "1 · body"));
    let mapped = info(server, &root, None).await;
    let model = mapped["statements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "model")
        .unwrap();
    assert_eq!(model["segments"].as_array().unwrap().len(), 3);
    assert_eq!(mapped["equations"][0]["location"]["uri"], body.as_str());
    assert_eq!(
        folds(server, &root)
            .await
            .iter()
            .filter(|range| range.start_line == 1 && range.end_line == 3)
            .count(),
        1
    );
    for &kind in dygnosis::model_map::SUPPORTED_BLOCKS {
        let opener = if kind == "pac_target_info" {
            "pac_target_info(p)"
        } else {
            kind
        };
        let text = format!("{opener};\nend;\n");
        let uri = url(&format!("fold-{kind}.mod"));
        open(server, &uri, &text).await;
        assert!(
            folds(server, &uri)
                .await
                .iter()
                .any(|range| range.start_line == 0 && range.end_line == 1),
            "{kind}"
        );
        assert_local_tree(&symbols(server, &uri).await, &text, None);
    }
    let partial = url("partial.mod");
    open(server, &partial, "var y;\nmodel;\ny=0;\n").await;
    assert!(folds(server, &partial).await.is_empty());
    let incomplete = info(server, &partial, None).await;
    assert_eq!(incomplete["complete"], false);
    assert!(incomplete.get("n_equations").is_none());
    assert!(incomplete["equations"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn numbers_keep_macro_static_local_removed_and_dimension_scopes() {
    let root = url("numbers.mod");
    let text = "heterogeneity_dimension hh; var y z; var(heterogeneity=hh) c; model; #a=1; [name='old'] y=a; [static] z=0; [dynamic,name='keep'] z=z(-1); end; model_replace('old'); [name='new'] y=2; end; model(heterogeneity=hh); [name='hh'] c=0; end;\n@#for k in 1:2\nmodel; [name='copy'] y=@{k}; end;\n@#endfor\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &root, text).await;
    let result = info(server, &root, None).await;
    let eqs = result["equations"].as_array().unwrap();
    assert_eq!(
        eqs.iter()
            .map(|row| (
                row["name"].as_str().unwrap(),
                row["number"].as_u64().unwrap()
            ))
            .collect::<Vec<_>>(),
        [("keep", 1), ("new", 2), ("hh", 1), ("copy", 3), ("copy", 4)]
    );
    assert_ne!(eqs[3]["id"], eqs[4]["id"]);
    assert_eq!(eqs[3]["location"], eqs[4]["location"]);
    let rows = symbols(server, &root).await;
    assert_local_tree(&rows, text, None);
    let mut flat = Vec::new();
    all(&rows, &mut flat);
    assert!(flat.iter().any(|row| row.name == "3–4 · copy"));
    assert!(flat
        .iter()
        .any(|row| row.name == "old" && row.detail.as_deref() == Some("removed equation")));
    assert!(flat
        .iter()
        .any(|row| row.detail.as_deref() == Some("static-only equation")));
    assert!(flat
        .iter()
        .any(|row| row.detail.as_deref() == Some("model-local definition")));
    server
        .did_change_configuration(DidChangeConfigurationParams {
            settings: json!({"outline":{"sections":["equations"],"equationNumbers":false}}),
        })
        .await;
    let equations = symbols(server, &root).await;
    assert_local_tree(&equations, text, None);
    assert!(equations
        .iter()
        .all(|row| row.kind == SymbolKind::FUNCTION && !row.name.contains('·')));
}

#[tokio::test]
async fn ownerless_and_shared_includes_never_choose_an_arbitrary_model() {
    let child = url("shared.inc");
    let a = url("a.mod");
    let b = url("b.mod");
    let child_text = "[name='shared'] y=0;\n";
    let text = "var y; model;\n@#include \"shared.inc\"\nend;\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &child, child_text).await;
    assert_eq!(info(server, &child, None).await["code"], "ROOT_REQUIRED");
    open(server, &a, text).await;
    open(server, &b, text).await;
    assert_eq!(server.known_model_roots(&child).len(), 2);
    let rows = symbols(server, &child).await;
    assert_local_tree(&rows, child_text, None);
    assert!(rows.iter().any(|row| row.name == "shared"));
    assert!(rows.iter().all(|row| !row.name.contains('·')));
    assert_eq!(info(server, &a, Some(&child)).await["root_uri"], a.as_str());
    assert_eq!(info(server, &b, Some(&child)).await["root_uri"], b.as_str());
    assert_eq!(
        info(server, &a, Some(&url("unrelated.inc"))).await["code"],
        "DOCUMENT_NOT_OWNED"
    );
}

#[tokio::test]
async fn required_include_activity_controls_both_transport_count_authority() {
    let root = url("activity.mod");
    let (service, _socket) = new_service();
    let server = service.inner();
    for prefix in [
        "@#include \"absent.inc\"\n",
        "@#include filename\n",
        "@#if unavailable\n@#include \"absent.inc\"\n@#endif\n",
    ] {
        let text = format!("{prefix}var y; model; y=0; end;\n");
        open(server, &root, &text).await;
        let result = info(server, &root, None).await;
        assert_eq!(result["complete"], false);
        assert!(result.get("n_equations").is_none());
        assert!(result["equations"].as_array().unwrap().is_empty());
        assert!(result["statements"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["equation_count"].is_null()));
        let inactive = format!("@#if 0\n{prefix}@#endif\nvar y; model; y=0; end;\n");
        if !prefix.contains("unavailable") {
            open(server, &root, &inactive).await;
            let result = info(server, &root, None).await;
            assert_eq!(result["complete"], true);
            assert_eq!(result["n_equations"], 1);
        }
    }
    let cycle = url("activity-cycle.inc");
    open(server, &cycle, "@#include \"activity.mod\"\n").await;
    for (condition, complete) in [(1, false), (0, true)] {
        let text = format!("@#if {condition}\n@#include \"activity-cycle.inc\"\n@#endif\nvar y; model; y=0; end;\n");
        open(server, &root, &text).await;
        let result = info(server, &root, None).await;
        let files = HashMap::from([
            (root.as_str().to_string(), text.clone()),
            (
                cycle.as_str().to_string(),
                "@#include \"activity.mod\"\n".to_string(),
            ),
        ]);
        let mcp = dygnosis::dynare_model_info(&text, Some(root.as_str()), Some(&files));
        assert_eq!(result["complete"], complete);
        assert_eq!(result.get("n_equations"), mcp.get("n_equations"));
    }
    let flag = url("activity-flag.inc");
    let controlled = "@#include \"activity-flag.inc\"\n@#if ENABLED\n@#include \"absent.inc\"\n@#endif\nvar y; model; y=0; end;\n";
    open(server, &flag, "@#define ENABLED=0\n").await;
    open(server, &root, controlled).await;
    let accepted = info(server, &root, None).await;
    assert_eq!(accepted["complete"], true);
    assert_eq!(accepted["n_equations"], 1);
    open(server, &flag, "@#define ENABLED=1\n").await;
    let required = info(server, &root, None).await;
    assert_eq!(required["complete"], false);
    assert_ne!(accepted["revision"], required["revision"]);
}

#[tokio::test]
async fn parser_partial_counts_agree_and_native_optional_semicolons_stay_complete() {
    let root = url("parser-partial.mod");
    let (service, _socket) = new_service();
    let server = service.inner();
    for (text, complete) in [
        ("var y; model; y=0;", false),
        ("parameters p\nvar y; model; y=p; end;", false),
        ("parameters p; p=1\nmodel; y=p; end;", false),
        ("var y; model; y=0; end;", true),
        ("parameters p; var y; model; y=p; end;", true),
        ("var y; model; y=0; end;\nhelper=2\n", true),
    ] {
        open(server, &root, text).await;
        let lsp = info(server, &root, None).await;
        let mcp = dygnosis::dynare_model_info(text, None, None);
        assert_eq!(lsp["complete"], complete, "{text}: {lsp}");
        assert_eq!(mcp.get("n_equations").is_some(), complete, "{text}: {mcp}");
        assert_eq!(lsp.get("n_equations"), mcp.get("n_equations"), "{text}");
        if !complete {
            assert_eq!(mcp["status"], "incomplete");
        }
    }
}

#[tokio::test]
async fn crlf_mixed_newlines_unicode_and_macro_copies_keep_exact_written_ranges() {
    let root = url("mixed.mod");
    let child = url("mixed-params.inc");
    let text = "/* 🚀 */ var output_y $Y$ (long_name='Output');\r\n@#include \"mixed-params.inc\"\n/* α */ model;\r@#for k in 1:2\r\n[name='row'] output_y=calib_p+@{k};\r\n@#endfor\nend;\r\n";
    let included = "/* 🧮 */ parameters calib_p ${p}$ (long_name='Calibration');\r\ncalib_p=2;\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &child, included).await;
    open(server, &root, text).await;
    let result = info(server, &root, None).await;
    assert_eq!(
        result["declarations"][0]["location"]["range"]["start"],
        json!({"line":0,"character":13})
    );
    assert_eq!(
        result["declarations"][0]["location"]["range"]["end"],
        json!({"line":0,"character":21})
    );
    assert_eq!(
        original_token(text, &result["declarations"][0]["location"]["range"]),
        "output_y"
    );
    assert_eq!(result["declarations"][1]["location"]["uri"], child.as_str());
    assert_eq!(
        result["declarations"][1]["location"]["range"]["start"],
        json!({"line":0,"character":20})
    );
    assert_eq!(
        original_token(included, &result["declarations"][1]["location"]["range"]),
        "calib_p"
    );
    let model = result["statements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "model")
        .unwrap();
    assert_eq!(
        model["anchor"]["range"]["start"],
        json!({"line":2,"character":8})
    );
    assert_eq!(
        model["anchor"]["range"]["end"],
        json!({"line":2,"character":14})
    );
    assert_eq!(original_token(text, &model["anchor"]["range"]), "model;");
    assert_eq!(
        model["segments"][0]["range"]["end"],
        json!({"line":6,"character":4})
    );
    assert_eq!(
        result["equations"][0]["location"]["range"]["start"],
        json!({"line":4,"character":0})
    );
    assert_eq!(
        result["equations"][0]["location"],
        result["equations"][1]["location"]
    );
    assert_local_tree(&symbols(server, &root).await, text, None);
    assert_local_tree(&symbols(server, &child).await, included, None);
    let folded = folds(server, &root).await;
    assert!(folded
        .iter()
        .any(|row| row.start_line == 2 && row.end_line == 6));
    assert!(folded
        .iter()
        .any(|row| row.start_line == 3 && row.end_line == 5));
}

#[tokio::test]
async fn model_info_supplies_resolved_and_missing_native_dependency_candidates() {
    let root = url("watch-root.mod");
    let child = url("arbitrary-extension.data");
    let missing = url("not-created.settings");
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &child, "parameters calib_p; calib_p=1;").await;
    open(
        server,
        &root,
        "@#include \"arbitrary-extension.data\"\n@#include \"not-created.settings\"\nvar output_y; model; output_y=calib_p; end;",
    )
    .await;
    let result = info(server, &root, None).await;
    assert_eq!(result["complete"], false);
    let candidates = result["dependency_candidates"].as_array().unwrap();
    for expected in [&root, &child, &missing] {
        assert!(
            candidates.iter().any(|candidate| {
                let uri = Url::parse(candidate.as_str().unwrap()).unwrap();
                crate_path_key(&uri) == crate_path_key(expected)
            }),
            "missing candidate {expected}: {candidates:?}"
        );
    }
    let untitled = Url::parse("untitled:watch-untitled.mod").unwrap();
    open(server, &untitled, "var output_y; model; output_y=0; end;").await;
    assert!(info(server, &untitled, None).await["dependency_candidates"]
        .as_array()
        .unwrap()
        .is_empty());
}

fn crate_path_key(uri: &Url) -> String {
    dygnosis::include_resolver::normalize_uri(uri.as_str())
}
