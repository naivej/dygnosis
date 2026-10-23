use std::collections::HashMap;

use dygnosis::server::new_service;
use serde_json::{Value, json};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

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

#[tokio::test]
async fn outline_icons_distinguish_exogenous_names_and_equations() {
    // Guards Outline and preview rows using icons that differ from the status bar.
    fn check(rows: &[DocumentSymbol]) {
        let mut flat = Vec::new();
        all(rows, &mut flat);
        for (name, kind) in [
            ("y", SymbolKind::VARIABLE),
            ("e", SymbolKind::FIELD),
            ("d", SymbolKind::FIELD),
            ("p", SymbolKind::NUMBER),
            ("1 · law", SymbolKind::OPERATOR),
        ] {
            assert_eq!(
                flat.iter().find(|row| row.name == name).unwrap().kind,
                kind,
                "{name}"
            );
        }
    }
    let (service, _socket) = new_service();
    let server = service.inner();
    let document = url("icons.mod");
    open(
        server,
        &document,
        "var y; varexo e; varexo_det d; parameters p;\nmodel; [name='law'] y=p*y(-1)+e+d; end;\n",
    )
    .await;
    check(&symbols(server, &document).await);
    for layout in ["source", "readable"] {
        let payload = server
            .execute_command(ExecuteCommandParams {
                command: "dynare/showEffectiveModel".into(),
                arguments: vec![json!({"uri":document,"layout":layout})],
                work_done_progress_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();
        check(
            &serde_json::from_value::<Vec<DocumentSymbol>>(payload["document_symbols"].clone())
                .unwrap(),
        );
    }
}

#[tokio::test]
async fn outline_compacts_named_commands_and_lists_pac_target_and_components() {
    // Guards redundant name children, missing PAC targets/components, and wrong source targets.
    let source = "var x y z; parameters b; b=.9;\n\
var_model(model_name=vm,eqtags=['x']);\n\
trend_component_model(model_name=tcm,eqtags=['x'],targets=['x']);\n\
var_expectation_model(model_name=ve,auxiliary_model_name=vm,horizon=1,variable=x);\n\
pac_model(model_name=pm,discount=b,auxiliary_model_name=tcm);\n\
pac_target_info(pm);\n\
target z; auxname_target_nonstationary zns;\n\
component x; kind ll; auxname ax;\n\
component y; kind dd; auxname ay; growth 1;\n\
end;\n\
model; [name='x'] x=x(-1); [name='y'] y=y(-1); [name='z'] z=x+y; end;\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    let document = url("named-commands.mod");
    open(server, &document, source).await;
    let rows = symbols(server, &document).await;
    assert_local_tree(&rows, source, None);
    for (command, name) in [
        ("var_model", "vm"),
        ("trend_component_model", "tcm"),
        ("var_expectation_model", "ve"),
        ("pac_model", "pm"),
    ] {
        let label = format!("{command}({name})");
        let row = rows.iter().find(|row| row.name == label).unwrap();
        assert!(row.children.is_none(), "{row:?}");
        assert_eq!(original_token(source, &json!(row.selection_range)), name);
        assert!(
            source
                .lines()
                .nth(row.range.start.line as usize)
                .unwrap()
                .starts_with(command)
        );
    }
    let pac = rows
        .iter()
        .find(|row| row.name == "pac_target_info(pm)")
        .unwrap();
    let children = pac.children.as_ref().unwrap();
    assert_eq!(
        children
            .iter()
            .map(|row| row.name.as_str())
            .collect::<Vec<_>>(),
        ["target z", "component x", "component y"]
    );
    for (child, name) in children.iter().zip(["z", "x", "y"]) {
        assert_eq!(child.kind, SymbolKind::OBJECT);
        assert_eq!(original_token(source, &json!(child.selection_range)), name);
        assert_eq!(child.range.start.line, child.range.end.line);
        assert!(child.range.start.line < pac.range.end.line);
    }
    assert!(
        !rows
            .iter()
            .find(|row| row.name == "model")
            .unwrap()
            .children
            .as_ref()
            .unwrap()
            .is_empty()
    );

    server
        .did_change_configuration(DidChangeConfigurationParams {
            settings: json!({"outline":{"sections":["commands"]}}),
        })
        .await;
    let filtered = symbols(server, &document).await;
    assert!(filtered.iter().any(|row| row.name == "pac_model(pm)"));
    assert!(
        filtered
            .iter()
            .all(|row| !row.name.starts_with("pac_target_info")
                && !row.name.starts_with("component ")
                && !row.name.starts_with("target "))
    );
}

#[tokio::test]
async fn outline_projects_named_objects_and_component_macro_copies_in_includes() {
    // Guards using the first macro name for all copies or selecting root-file bytes for include rows.
    let root = url("named-macros.mod");
    let include = url("named-macros.inc");
    let root_text = "var x y z; parameters b; b=.9;\n@#define target_name = \"z\"\n@#include \"named-macros.inc\"\n";
    let include_text = "@#for name in [\"a\",\"c\"]\n\
var_model(model_name=@{name},eqtags=['x']);\n\
@#endfor\n\
pac_model(model_name=pm,discount=b);\n\
pac_target_info(pm);\n\
target @{target_name}; auxname_target_nonstationary zns;\n\
@#for name in [\"x\",\"y\"]\n\
component @{name}; kind dd; auxname aux_@{name};\n\
@#endfor\n\
end;\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &include, include_text).await;
    open(server, &root, root_text).await;
    let rows = symbols(server, &include).await;
    assert_local_tree(&rows, include_text, None);
    let mut written = Vec::new();
    all(&rows, &mut written);
    let command = written
        .iter()
        .find(|row| row.name == "var_model (multiple expansions)")
        .unwrap();
    assert!(command.children.is_none());
    assert!(rows.iter().any(|row| row.name.starts_with("@#for name")));
    let pac = written
        .iter()
        .find(|row| row.name == "pac_target_info(pm)")
        .unwrap();
    let mut descendants = Vec::new();
    all(pac.children.as_ref().unwrap(), &mut descendants);
    let children: Vec<_> = descendants
        .into_iter()
        .filter(|row| row.kind == SymbolKind::OBJECT)
        .collect();
    assert_eq!(
        children
            .iter()
            .map(|row| row.name.as_str())
            .collect::<Vec<_>>(),
        ["target z", "component x", "component y"]
    );
    assert_eq!(
        original_token(include_text, &json!(children[0].selection_range)),
        "@{target_name}"
    );
    assert!(children[1..].iter().all(|row| row.range.start.line == 7));
    assert!(
        !symbols(server, &root)
            .await
            .iter()
            .any(|row| row.name.starts_with("pac_"))
    );

    for layout in ["source", "readable"] {
        let payload = server
            .execute_command(ExecuteCommandParams {
                command: "dynare/showEffectiveModel".into(),
                arguments: vec![json!({"uri":root,"layout":layout})],
                work_done_progress_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();
        let preview = payload["effective_text"].as_str().unwrap();
        let rows: Vec<DocumentSymbol> =
            serde_json::from_value(payload["document_symbols"].clone()).unwrap();
        assert_local_tree(&rows, preview, None);
        for name in ["a", "c"] {
            let command = rows
                .iter()
                .find(|row| row.name == format!("var_model({name})"))
                .unwrap();
            assert!(command.children.is_none());
            assert_eq!(
                original_token(preview, &json!(command.selection_range)),
                name
            );
        }
        let pac = rows
            .iter()
            .find(|row| row.name == "pac_target_info(pm)")
            .unwrap();
        let children = pac.children.as_ref().unwrap();
        assert_eq!(
            children
                .iter()
                .map(|row| row.name.as_str())
                .collect::<Vec<_>>(),
            ["target z", "component x", "component y"]
        );
        assert!(
            children
                .windows(2)
                .all(|pair| pair[0].range.end <= pair[1].range.start)
        );
        for (child, name) in children.iter().zip(["z", "x", "y"]) {
            assert_eq!(original_token(preview, &json!(child.selection_range)), name);
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
    assert_eq!(hidden["revision"], result["revision"]);
}

#[tokio::test]
async fn outline_equation_with_includes_has_one_owner_and_no_fragment_rows() {
    // Guards one equation becoming a row for every include-separated segment,
    // duplicate model containers, and fragment containers in included files.
    for newline in ["\n", "\r\n"] {
        let root = url("equation-pieces.mod");
        let expression = url("expression.inc");
        let correction = url("correction.inc");
        let text = "var y;\nmodel;\n[name='joined'] y=1+(\n@#include \"expression.inc\"\n)+(\n@#include \"correction.inc\"\n);\nend;\n".replace('\n', newline);
        let (service, _socket) = new_service();
        let server = service.inner();
        open(server, &expression, "y(-1)\n").await;
        open(server, &correction, "0\n").await;
        open(server, &root, &text).await;
        assert_eq!(info(server, &root, None).await["n_equations"], 1);
        let rows = symbols(server, &root).await;
        assert_local_tree(&rows, &text, None);
        let mut entries = Vec::new();
        all(&rows, &mut entries);
        assert_eq!(
            entries.iter().filter(|row| row.name == "model").count(),
            1,
            "{rows:?}"
        );
        let equations: Vec<_> = entries
            .iter()
            .filter(|row| row.kind == SymbolKind::OPERATOR)
            .collect();
        assert_eq!(equations.len(), 1, "{rows:?}");
        assert_eq!(equations[0].name, "1 · joined");
        assert_eq!(equations[0].selection_range.start, Position::new(2, 0));
        assert!(entries.iter().all(|row| !row.name.contains("fragment")));
        for include in [&expression, &correction] {
            assert!(
                symbols(server, include).await.is_empty(),
                "an expression suffix is not a separate equation"
            );
        }
        server
            .did_change_configuration(DidChangeConfigurationParams {
                settings: json!({"outline":{"activity":"inactive"}}),
            })
            .await;
        assert!(
            symbols(server, &root).await.is_empty(),
            "split active equations must not leak into the Inactive filter"
        );
    }
}

#[tokio::test]
async fn outline_inactive_declarations_keep_metadata_and_icons_without_timing_or_candidate_labels()
{
    // Guards inactive names losing their literal metadata/type icons and
    // active endogenous names still displaying timing in Outline.
    let root = url("outline-metadata.mod");
    let text = "@#if 0\nvar hidden $Y$ (long_name='Output');\nvarexo hidden_e $E$ (long_name='Shock');\nparameters hidden_p $P$ (long_name='Value');\n@#else\nvar live $Y$ (long_name='Output');\nvarexo live_e $E$ (long_name='Shock');\nparameters live_p $P$ (long_name='Value');\n@#endif\nmodel; live=live(-1)+live_e; end;\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &root, text).await;
    let rows = symbols(server, &root).await;
    let mut entries = Vec::new();
    all(&rows, &mut entries);
    for (inactive, active, icon, detail) in [
        ("hidden", "live", SymbolKind::VARIABLE, "Output · $Y$"),
        ("hidden_e", "live_e", SymbolKind::FIELD, "Shock · $E$"),
        ("hidden_p", "live_p", SymbolKind::NUMBER, "Value · $P$"),
    ] {
        for name in [inactive, active] {
            let row = entries.iter().find(|row| row.name == name).unwrap();
            assert_eq!(row.detail.as_deref(), Some(detail), "{row:?}");
            assert_eq!(row.kind, icon, "{row:?}");
        }
    }
    server
        .did_change_configuration(DidChangeConfigurationParams {
            settings: json!({"nameDetails":{"longName":false,"tex":false}}),
        })
        .await;
    let rows = symbols(server, &root).await;
    let mut entries = Vec::new();
    all(&rows, &mut entries);
    for name in ["hidden", "live", "hidden_e", "live_e", "hidden_p", "live_p"] {
        assert!(
            entries
                .iter()
                .find(|row| row.name == name)
                .unwrap()
                .detail
                .is_none()
        );
    }
}

#[tokio::test]
async fn outline_keeps_literal_metadata_but_omits_unexpanded_metadata() {
    // Guards an inert interpolation placeholder being shown as a real label.
    let root = url("outline-literal-metadata.mod");
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &root, "@#if 0\nvar literal $L$ (long_name='Literal') templated $@{TEX}$ (long_name='@{LABEL}');\n@#endif\nvar y; model; y=0; end;\n").await;
    let rows = symbols(server, &root).await;
    let mut entries = Vec::new();
    all(&rows, &mut entries);
    assert_eq!(
        entries
            .iter()
            .find(|row| row.name == "literal")
            .unwrap()
            .detail
            .as_deref(),
        Some("Literal · $L$")
    );
    assert!(
        entries
            .iter()
            .find(|row| row.name == "templated")
            .unwrap()
            .detail
            .is_none(),
        "unexpanded metadata must not show inert placeholders"
    );
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
    assert!(child_rows.iter().all(|row| !row.name.contains("fragment")));
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
async fn conditional_outline_arms_are_siblings_with_their_own_contents() {
    // Guards the opening arm hiding its siblings, branch headings losing their
    // contents, and nested loops or conditionals escaping their written arm.
    for keyword in ["if", "ifdef", "ifndef"] {
        for newline in ["\n", "\r\n"] {
            let text = format!(
                "@#define FLAG=0\n@#{keyword} FLAG\nparameters first;\n@#for k in [1]\n@#if k\nparameters inner;\n@#else\nparameters alternate;\n@#endif\n@#endfor\n@#elseif 1\nparameters second;\n@#else\nparameters last;\n@#endif\n"
            )
            .replace('\n', newline);
            let root = url("conditional-outline.mod");
            let (service, _socket) = new_service();
            let server = service.inner();
            open(server, &root, &text).await;
            let rows = symbols(server, &root).await;
            assert_local_tree(&rows, &text, None);
            let arms: Vec<_> = rows
                .iter()
                .filter(|row| row.name.starts_with("@#"))
                .collect();
            assert_eq!(
                arms.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
                [
                    format!("@#{keyword} FLAG"),
                    "@#elseif 1".into(),
                    "@#else".into()
                ]
            );
            // Activity belongs to the selected arm, not to evaluation of its
            // header or the preceding arm. Contents must not repeat it.
            let first_active = keyword == "ifdef";
            assert_eq!(
                arms[0].detail.as_deref(),
                Some(if first_active { "active" } else { "inactive" })
            );
            assert_eq!(
                arms[1].detail.as_deref(),
                Some(if first_active { "inactive" } else { "active" })
            );
            assert_eq!(arms[2].detail.as_deref(), Some("inactive"));
            let mut entries = Vec::new();
            all(&rows, &mut entries);
            for row in entries.iter().filter(|row| !row.name.starts_with("@#")) {
                assert!(
                    !row.detail
                        .as_deref()
                        .unwrap_or_default()
                        .split_whitespace()
                        .any(|word| matches!(word, "active" | "inactive" | "mixed" | "unknown")),
                    "{row:?}"
                );
            }
            for (arm, start, end, declaration) in [
                (arms[0], 1, 10, "first"),
                (arms[1], 10, 12, "second"),
                (arms[2], 12, 14, "last"),
            ] {
                assert_eq!(arm.range.start.line, start);
                assert_eq!(arm.range.end, Position::new(end, 0));
                let mut contents = Vec::new();
                all(arm.children.as_deref().unwrap(), &mut contents);
                assert!(contents.iter().any(|row| row.name == declaration));
            }
            let loop_row = arms[0]
                .children
                .as_ref()
                .unwrap()
                .iter()
                .find(|row| row.name == "@#for k in [1]")
                .unwrap();
            let inner = loop_row.children.as_ref().unwrap();
            assert_eq!(
                inner
                    .iter()
                    .map(|row| row.name.as_str())
                    .collect::<Vec<_>>(),
                ["@#if k", "@#else"]
            );
            for (arm, start, end, declaration) in
                [(&inner[0], 4, 6, "inner"), (&inner[1], 6, 8, "alternate")]
            {
                assert_eq!(arm.range.start.line, start);
                assert_eq!(arm.range.end, Position::new(end, 0));
                let mut contents = Vec::new();
                all(arm.children.as_deref().unwrap(), &mut contents);
                assert!(contents.iter().any(|row| row.name == declaration));
            }
        }
    }
}

#[tokio::test]
async fn outline_control_activity_handles_empty_nested_and_repeated_arms() {
    // Guards body-token guesses mislabeling empty arms, nested exclusions or
    // conditions that select different arms in separate loop/include visits.
    // Incomplete or ownerless execution must not invent a selected arm.
    let root = url("control-activity.mod");
    let text = "@#if 0\n@#elseif 1\n@#else\n@#endif\n@#if 1\n@#if 0\n@#endif\n@#else\n@#if 1\n@#endif\n@#endif\n@#for k in [0,1]\n@#if k\n@#else\n@#endif\n@#endfor\n@#for empty in []\n@#if 1\n@#endif\n@#endfor\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &root, text).await;
    let rows = symbols(server, &root).await;
    let mut entries = Vec::new();
    all(&rows, &mut entries);
    for (line, expected) in [
        (0, "inactive"),
        (1, "active"),
        (2, "inactive"),
        (4, "active"),
        (5, "inactive"),
        (7, "inactive"),
        (8, "inactive"),
        (11, "active"),
        (12, "mixed"),
        (13, "mixed"),
        (16, "inactive"),
        (17, "inactive"),
    ] {
        let row = entries
            .iter()
            .find(|row| row.selection_range.start.line == line)
            .unwrap();
        assert_eq!(row.detail.as_deref(), Some(expected), "{row:?}");
    }
    server
        .did_change_configuration(DidChangeConfigurationParams {
            settings: json!({"outline":{"activity":"active"}}),
        })
        .await;
    let rows = symbols(server, &root).await;
    let mut visible = Vec::new();
    all(&rows, &mut visible);
    assert!(
        visible
            .iter()
            .all(|row| row.detail.as_deref() != Some("inactive"))
    );
    assert!(
        visible
            .iter()
            .any(|row| row.selection_range.start.line == 1)
    );
    assert!(
        visible
            .iter()
            .any(|row| row.detail.as_deref() == Some("mixed"))
    );
    let child = url("control-activity.inc");
    open(server, &child, "@#if k\n@#else\n@#endif\n").await;
    open(
        server,
        &root,
        "@#for k in [0,1]\n@#include \"control-activity.inc\"\n@#endfor\n",
    )
    .await;
    assert!(
        symbols(server, &child)
            .await
            .iter()
            .all(|row| row.detail.as_deref() == Some("mixed"))
    );
    open(server, &root, "@#if MISSING\n@#else\n@#endif\n").await;
    let unknown = symbols(server, &root).await;
    assert_eq!(unknown.len(), 2);
    assert!(
        unknown
            .iter()
            .all(|row| row.detail.as_deref() == Some("unknown"))
    );
    let ownerless = url("ownerless-control.inc");
    open(server, &ownerless, "@#if 1\n@#else\n@#endif\n").await;
    let unknown = symbols(server, &ownerless).await;
    assert_eq!(unknown.len(), 2);
    assert!(
        unknown
            .iter()
            .all(|row| row.detail.as_deref() == Some("unknown"))
    );
}

#[tokio::test]
async fn outline_activity_filter_prunes_semantic_and_source_rows_and_keeps_context() {
    // Guards filtering only source candidates while leaving active semantic
    // rows in Inactive, and losing containers needed for matching children.
    let root = url("outline-filter.mod");
    let text = "var y;\nmodel;\n@#if 0\n[name='dormant'] y=2;\n@#else\n[name='running'] y=1;\n@#endif\nend;\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &root, text).await;
    for (filter, present, absent) in [
        (
            "active",
            vec!["var", "y", "model", "@#else", "1 · running"],
            vec!["@#if 0", "dormant"],
        ),
        (
            "inactive",
            vec!["model", "@#if 0", "dormant"],
            vec!["var", "y", "@#else", "1 · running"],
        ),
        (
            "both",
            vec![
                "var",
                "y",
                "model",
                "@#if 0",
                "dormant",
                "@#else",
                "1 · running",
            ],
            vec![],
        ),
    ] {
        server
            .did_change_configuration(DidChangeConfigurationParams {
                settings: json!({"outline":{"activity":filter}}),
            })
            .await;
        let rows = symbols(server, &root).await;
        assert_local_tree(&rows, text, None);
        let mut entries = Vec::new();
        all(&rows, &mut entries);
        for name in present {
            assert!(
                entries.iter().any(|row| row.name == name),
                "{filter} missing {name}: {rows:?}"
            );
        }
        for name in absent {
            assert!(
                entries.iter().all(|row| row.name != name),
                "{filter} retained {name}: {rows:?}"
            );
        }
        assert_eq!(
            info(server, &root, None).await["n_equations"],
            1,
            "presentation must not change model facts"
        );
    }
    server
        .did_change_configuration(DidChangeConfigurationParams {
            settings: json!({"outline":{"activity":"inactive"}}),
        })
        .await;
    open(server, &root, "var y; model; y=0; end;\n").await;
    assert!(symbols(server, &root).await.is_empty());
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
    assert!(
        flat.iter()
            .any(|row| row.name == "old" && row.detail.as_deref() == Some("removed equation"))
    );
    assert!(
        flat.iter()
            .any(|row| row.detail.as_deref() == Some("static-only equation"))
    );
    assert!(
        flat.iter()
            .any(|row| row.detail.as_deref() == Some("model-local definition"))
    );
    server
        .did_change_configuration(DidChangeConfigurationParams {
            settings: json!({"outline":{"sections":["equations"],"equationNumbers":false}}),
        })
        .await;
    let equations = symbols(server, &root).await;
    assert_local_tree(&equations, text, None);
    assert!(
        equations
            .iter()
            .all(|row| row.kind == SymbolKind::OPERATOR && !row.name.contains('·'))
    );
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
    let rows = symbols(server, &child).await;
    assert_eq!(server.known_model_roots(&child).len(), 2);
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
        assert!(
            result["statements"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["equation_count"].is_null())
        );
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
        let text = format!(
            "@#if {condition}\n@#include \"activity-cycle.inc\"\n@#endif\nvar y; model; y=0; end;\n"
        );
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
    assert!(
        folded
            .iter()
            .any(|row| row.start_line == 2 && row.end_line == 6)
    );
    assert!(
        folded
            .iter()
            .any(|row| row.start_line == 3 && row.end_line == 5)
    );
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
    assert!(
        info(server, &untitled, None).await["dependency_candidates"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

fn crate_path_key(uri: &Url) -> String {
    dygnosis::include_resolver::normalize_uri(uri.as_str())
}

#[tokio::test]
async fn outline_omits_assignments_and_keeps_declared_names_and_model_structure() {
    // Guards assignment groups and lone assignment rows returning, or their
    // removal also hiding parameter declarations, commands or model equations.
    let root = url("assignments.mod");
    let text = "parameters rho;\nrho=0.9;\nhelper=2;\nvar y;\nsteady;\nmodel;\n[name='equation'] y=rho;\nend;\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &root, text).await;
    let rows = symbols(server, &root).await;
    assert_local_tree(&rows, text, None);
    assert_eq!(
        rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
        ["parameters", "var", "steady", "model"]
    );
    let parameter = &rows[0].children.as_ref().unwrap()[0];
    assert_eq!(parameter.name, "rho");
    assert_eq!(parameter.selection_range.start, Position::new(0, 11));
    assert_eq!(rows[3].children.as_ref().unwrap()[0].name, "1 · equation");
    server
        .did_change_configuration(DidChangeConfigurationParams {
            settings: json!({"outline":{"sections":["commands"]}}),
        })
        .await;
    let commands = symbols(server, &root).await;
    assert_eq!(
        commands
            .iter()
            .map(|row| row.name.as_str())
            .collect::<Vec<_>>(),
        ["steady"]
    );

    server
        .did_change_configuration(DidChangeConfigurationParams {
            settings: json!({"outline":{"sections":["declarations","blocks","commands","dimensions","equations"]}}),
        })
        .await;
    for text in ["helper=1;\n", "first=1;\nsecond=2;\n"] {
        open(server, &root, text).await;
        assert!(symbols(server, &root).await.is_empty());
    }
    open(server, &root, "parameters rho;\nrho=0.9;\n").await;
    let rows = symbols(server, &root).await;
    assert_eq!(
        rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
        ["parameters"]
    );
}

#[tokio::test]
async fn outline_omits_macro_and_included_assignments_in_recovered_and_shared_views() {
    // Guards fallback and shared-owner views restoring hidden assignment rows.
    // Declared names must still point at the include's declaration in both views.
    let a = url("assignments-owner-a.mod");
    let b = url("assignments-owner-b.mod");
    let child = url("shared-assignments.inc");
    let child_text = "parameters shared;\nshared=1;\nhelper=2;\n";
    let text = "parameters rho;\n@#for k in 1:2\nrho=@{k};\nhelper=@{k};\n@#endfor\n@#include \"shared-assignments.inc\"\nvar y;\nmodel;\ny=rho;\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &child, child_text).await;
    open(server, &a, text).await;
    assert_eq!(info(server, &a, None).await["complete"], false);
    let rows = symbols(server, &a).await;
    assert_local_tree(&rows, text, None);
    assert_eq!(
        rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
        ["parameters", "@#for k in 1:2", "var", "model"]
    );
    // The written loop is visible, but its native/parameter assignments still
    // have no Outline children.
    assert!(rows[1].children.is_none());
    let owned = symbols(server, &child).await;
    assert_local_tree(&owned, child_text, None);
    assert_eq!(
        owned
            .iter()
            .map(|row| row.name.as_str())
            .collect::<Vec<_>>(),
        ["parameters"]
    );
    let parameter = owned[0].children.as_ref().unwrap()[0].clone();
    assert_eq!(parameter.name, "shared");
    assert_eq!(parameter.selection_range.start, Position::new(0, 11));
    open(server, &b, text).await;
    let shared = symbols(server, &child).await;
    assert_local_tree(&shared, child_text, None);
    let mut leaves = Vec::new();
    all(&shared, &mut leaves);
    let locations = |name| {
        leaves
            .iter()
            .filter(|row| row.name == name)
            .map(|row| row.selection_range)
            .collect::<Vec<_>>()
    };
    assert_eq!(locations("shared"), [parameter.selection_range]);
    assert!(locations("helper").is_empty());
    let workspace = server
        .symbol(WorkspaceSymbolParams {
            query: "shared".to_string(),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    assert!(
        workspace
            .iter()
            .all(|row| row.location.range == parameter.range)
    );
    assert!(workspace.iter().any(|row| row.location.uri == child));
    let helpers = server
        .symbol(WorkspaceSymbolParams {
            query: "helper".to_string(),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    assert!(helpers.is_empty());
}

#[tokio::test]
async fn model_outline_detail_marks_recovery_without_removing_equation_numbers() {
    // Guards the retired complete-model detail returning, or its removal
    // accidentally removing the recovered marker, equation prefixes or wire fact.
    let root = url("model-detail.mod");
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &root, "var y;\nmodel; [name='euler'] y=0; end;\n").await;
    let rows = symbols(server, &root).await;
    let model = rows.iter().find(|row| row.name == "model").unwrap();
    assert!(model.detail.is_none());
    assert_eq!(model.children.as_ref().unwrap()[0].name, "1 · euler");
    assert_eq!(
        info(server, &root, None).await["equation_numbering"],
        "Dygnosis numbers before transformation"
    );
    open(server, &root, "var y;\nmodel; [name='euler'] y=0;\n").await;
    let rows = symbols(server, &root).await;
    let model = rows.iter().find(|row| row.name == "model").unwrap();
    assert_eq!(model.detail.as_deref(), Some("recovered written structure"));
    assert_eq!(model.children.as_ref().unwrap()[0].name, "euler");
}

#[tokio::test]
async fn outline_names_use_written_heterogeneity_and_preserve_declaration_children() {
    // Guards old spaced model titles, missing declaration dimensions, unrelated
    // model options in titles, and moving change_type names to another parent.
    let root = url("heterogeneous-names.mod");
    let text = "heterogeneity_dimension households;\nvar y;\nvarexo e;\nvar(heterogeneity=households) c $C$ (long_name='Consumption');\nvar(heterogeneity=households) assets;\nvarexo(heterogeneity=households) idio;\nparameters(heterogeneity=households) betta;\nvarexo_det deterministic_e;\nmodel(linear); [name='aggregate'] y=e; end;\nmodel(heterogeneity=households); [name='individual'] c=c(-1)+idio; assets=assets(-1); end;\nchange_type(parameters) c;\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &root, text).await;
    let rows = symbols(server, &root).await;
    assert_local_tree(&rows, text, None);
    assert!(rows.iter().any(|row| row.name == "model"));
    let model = rows
        .iter()
        .find(|row| row.name == "model(heterogeneity=households)")
        .unwrap();
    assert!(model.detail.is_none());
    assert!(
        model
            .children
            .as_ref()
            .unwrap()
            .iter()
            .any(|row| row.name == "1 · individual")
    );
    let declarations: Vec<_> = rows
        .iter()
        .filter(|row| row.name == "var(heterogeneity=households)")
        .collect();
    assert_eq!(declarations.len(), 2);
    let c = &declarations[0].children.as_ref().unwrap()[0];
    assert_eq!(c.name, "c");
    assert_eq!(c.kind, SymbolKind::NUMBER);
    assert_eq!(c.detail.as_deref(), Some("parameters · Consumption · $C$"));
    assert_eq!(declarations[1].children.as_ref().unwrap()[0].name, "assets");
    let exogenous = rows
        .iter()
        .find(|row| row.name == "varexo(heterogeneity=households)")
        .unwrap();
    assert_eq!(exogenous.children.as_ref().unwrap()[0].name, "idio");
    for unchanged in ["var", "varexo", "parameters", "varexo_det"] {
        assert!(rows.iter().any(|row| row.name == unchanged));
    }

    // An ownerless include takes the fallback path with final types unavailable.
    let ownerless = url("heterogeneous-names.inc");
    open(server, &ownerless, text).await;
    let recovered = symbols(server, &ownerless).await;
    let model = recovered
        .iter()
        .find(|row| row.name == "model(heterogeneity=households)")
        .unwrap();
    assert_eq!(model.detail.as_deref(), Some("recovered written structure"));
    assert!(
        recovered
            .iter()
            .any(|row| row.name == "var(heterogeneity=households)")
    );
    assert!(
        recovered
            .iter()
            .any(|row| row.name == "varexo(heterogeneity=households)")
    );
    let c = recovered
        .iter()
        .find(|row| row.name == "var(heterogeneity=households)")
        .unwrap()
        .children
        .as_ref()
        .unwrap()
        .iter()
        .find(|row| row.name == "c")
        .unwrap();
    assert_eq!(c.kind, SymbolKind::VARIABLE);
    assert_eq!(c.detail.as_deref(), Some("Consumption · $C$"));
}

#[tokio::test]
async fn outline_names_keep_multiple_dimension_macro_expansion_fallbacks() {
    // Guards selecting one dimension as the title of an opener or declaration
    // that expands into different dimensions at the same written location.
    let root = url("multiple-dimension-names.mod");
    let text = "heterogeneity_dimension hh workers;\n@#for dimension in [\"hh\",\"workers\"]\nvar(heterogeneity=@{dimension}) c_@{dimension};\nvarexo(heterogeneity=@{dimension}) e_@{dimension};\nmodel(heterogeneity=@{dimension});\n[name='individual'] c_@{dimension}=e_@{dimension};\nend;\n@#endfor\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &root, text).await;
    let rows = symbols(server, &root).await;
    assert_local_tree(&rows, text, None);
    let mut written = Vec::new();
    all(&rows, &mut written);
    assert!(
        rows.iter()
            .any(|row| row.name.starts_with("@#for dimension"))
    );
    for title in [
        "model (multiple expansions)",
        "var (multiple expansions)",
        "varexo (multiple expansions)",
    ] {
        let row = written
            .iter()
            .find(|row| row.name == title)
            .unwrap_or_else(|| panic!("{title}: {rows:?}"));
        assert!(row.detail.is_none());
    }
    let model = written
        .iter()
        .find(|row| row.name == "model (multiple expansions)")
        .unwrap();
    assert_eq!(
        model.children.as_ref().unwrap()[0].detail.as_deref(),
        Some("multiple expansions")
    );
}
