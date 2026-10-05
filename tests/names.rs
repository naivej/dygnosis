use dygnosis::server::{new_service, Backend};
use serde_json::json;
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

fn uri(name: &str) -> Url {
    Url::parse(&format!("file:///C:/dygnosis-names/{name}")).unwrap()
}
async fn initialize(
    server: &Backend,
    types: Option<&[&str]>,
    modifiers: &[&str],
    details: bool,
    snippets: bool,
) -> SemanticTokensLegend {
    let mut capability = json!({"textDocument":{"completion":{"completionItem":{"labelDetailsSupport":details,"snippetSupport":snippets}}}});
    if let Some(types) = types {
        capability["textDocument"]["semanticTokens"] = json!({"requests":{"full":true,"range":true},"tokenTypes":types,"tokenModifiers":modifiers,"formats":["relative"]});
    }
    let result = server
        .initialize(InitializeParams {
            capabilities: serde_json::from_value(capability).unwrap(),
            ..Default::default()
        })
        .await
        .unwrap();
    match result.capabilities.semantic_tokens_provider.unwrap() {
        SemanticTokensServerCapabilities::SemanticTokensOptions(options) => options.legend,
        _ => panic!("expected options"),
    }
}
async fn open(server: &Backend, document: &Url, text: &str) {
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: document.clone(),
                language_id: "dynare".into(),
                version: 1,
                text: text.into(),
            },
        })
        .await;
}
fn position(document: &Url, text: &str, needle: &str, offset: usize) -> TextDocumentPositionParams {
    let byte = text.find(needle).unwrap() + offset;
    let normalized = normalized(text);
    let index = dygnosis::span::LineIndex::new(&normalized);
    let position = index.position_utf16(text, byte as u32);
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier {
            uri: document.clone(),
        },
        position: Position::new(position.line, position.character),
    }
}

fn normalized(text: &str) -> String {
    text.char_indices()
        .map(|(index, character)| {
            if character == '\r' && text.as_bytes().get(index + 1) != Some(&b'\n') {
                '\n'
            } else {
                character
            }
        })
        .collect()
}

fn written_text(text: &str, range: Range) -> &str {
    let index = dygnosis::span::LineIndex::new(&normalized(text));
    let start = index.offset_utf16(
        text,
        dygnosis::span::Position {
            line: range.start.line,
            character: range.start.character,
        },
    );
    let end = index.offset_utf16(
        text,
        dygnosis::span::Position {
            line: range.end.line,
            character: range.end.character,
        },
    );
    &text[start as usize..end as usize]
}
async fn completion(server: &Backend, document: &Url) -> Vec<CompletionItem> {
    completion_at(server, document, Position::new(0, 0)).await
}
async fn completion_at(
    server: &Backend,
    document: &Url,
    position: Position,
) -> Vec<CompletionItem> {
    let result = server
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: document.clone(),
                },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .unwrap()
        .unwrap();
    match result {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    }
}
async fn hover(server: &Backend, pos: TextDocumentPositionParams) -> String {
    let result = server
        .hover(HoverParams {
            text_document_position_params: pos,
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    match result.contents {
        HoverContents::Markup(markdown) => markdown.value,
        _ => panic!("expected markdown"),
    }
}
async fn full(server: &Backend, document: &Url) -> Vec<SemanticToken> {
    match server
        .semantic_tokens_full(SemanticTokensParams {
            text_document: TextDocumentIdentifier {
                uri: document.clone(),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap()
    {
        SemanticTokensResult::Tokens(tokens) => tokens.data,
        _ => panic!("expected full"),
    }
}

#[tokio::test]
async fn predetermined_timing_agrees_in_hover_tokens_outline_and_model_map() {
    let document = uri("predetermined.mod");
    let text = "var k f s m u;\npredetermined_variables k f s m;\nmodel;\nk(1)=k;\nf(2)=0;\ns(1)=0;\nm(1)=m+m(2);\nu=u(-1);\nend;";
    let (service, _socket) = new_service();
    let server = service.inner();
    let legend = initialize(
        server,
        Some(&["variable"]),
        &["declaration", "predetermined", "forwardLooking"],
        false,
        false,
    )
    .await;
    open(server, &document, text).await;
    let expected = [
        ("k", "predetermined", vec![-1, 0]),
        ("f", "forward-looking", vec![1]),
        ("s", "static", vec![0]),
        ("m", "mixed", vec![-1, 0, 1]),
        ("u", "predetermined", vec![-1, 0]),
    ];
    for (name, class, offsets) in &expected {
        let markdown = hover(server, position(&document, text, &format!("{name}("), 0)).await;
        assert!(
            markdown.contains(&format!("Timing: **{class}**")),
            "{markdown}"
        );
        if *name == "u" {
            assert!(markdown.contains("written offsets: t-1, t"), "{markdown}");
            assert!(!markdown.contains("one period earlier"), "{markdown}");
        } else {
            assert!(
                markdown.contains("Dynare end-of-period offsets:"),
                "{markdown}"
            );
            assert!(
                markdown.contains("one period earlier than written offsets"),
                "{markdown}"
            );
        }
        assert!(!markdown.contains("appears at"), "{markdown}");
        for offset in offsets {
            let display = match offset {
                0 => "t".to_string(),
                offset if *offset > 0 => format!("t+{offset}"),
                offset => format!("t{offset}"),
            };
            assert!(markdown.contains(&display), "{markdown}");
        }
    }
    let tokens = decode(&full(server, &document).await, &legend);
    for (line, name, modifiers) in [
        (3, "k", vec!["predetermined"]),
        (4, "f", vec!["forwardLooking"]),
        (5, "s", vec![]),
        (6, "m", vec!["predetermined", "forwardLooking"]),
    ] {
        let occurrences: Vec<_> = tokens
            .iter()
            .filter(|token| {
                token.0 == line
                    && written_text(
                        text,
                        Range::new(
                            Position::new(token.0, token.1),
                            Position::new(token.0, token.1 + token.2),
                        ),
                    ) == name
            })
            .collect();
        assert!(!occurrences.is_empty());
        for token in occurrences {
            for modifier in ["predetermined", "forwardLooking"] {
                assert_eq!(
                    token.4.iter().any(|value| value == modifier),
                    modifiers.contains(&modifier),
                    "{token:?}"
                );
            }
        }
    }
    let response = server
        .document_symbol(DocumentSymbolParams {
            text_document: TextDocumentIdentifier {
                uri: document.clone(),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    let DocumentSymbolResponse::Nested(outline) = response else {
        panic!("nested outline");
    };
    let declarations = outline
        .iter()
        .find(|row| row.name == "var")
        .unwrap()
        .children
        .as_ref()
        .unwrap();
    for (name, class, _) in &expected {
        let row = declarations.iter().find(|row| row.name == *name).unwrap();
        assert!(row.detail.as_deref().unwrap().contains(class), "{row:?}");
    }
    let map = server
        .execute_command(ExecuteCommandParams {
            command: "dynare/modelInfo".into(),
            arguments: vec![json!({"root_uri": document, "document_uri": document})],
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(map["predetermined"], json!(["k", "u"]));
    assert_eq!(map["forward_looking"], json!(["f"]));
    assert_eq!(map["mixed"], json!(["m"]));
    assert_eq!(map["static"], json!(["s"]));
    let declarations = map["declarations"].as_array().unwrap();
    for (name, class, offsets) in &expected {
        let row = declarations
            .iter()
            .find(|row| row["name"] == *name && row["written_kind"] == "var")
            .unwrap();
        assert_eq!(row["timing"]["class"], *class);
        assert_eq!(row["timing"]["offsets"], json!(offsets));
    }
}
fn decode(
    tokens: &[SemanticToken],
    legend: &SemanticTokensLegend,
) -> Vec<(u32, u32, u32, String, Vec<String>)> {
    let mut line = 0;
    let mut column = 0;
    let mut decoded = Vec::new();
    for token in tokens {
        line += token.delta_line;
        column = if token.delta_line == 0 {
            column + token.delta_start
        } else {
            token.delta_start
        };
        assert!((token.token_type as usize) < legend.token_types.len());
        assert_eq!(
            token.token_modifiers_bitset >> legend.token_modifiers.len(),
            0
        );
        let modifiers = legend
            .token_modifiers
            .iter()
            .enumerate()
            .filter(|(index, _)| token.token_modifiers_bitset & (1 << index) != 0)
            .map(|(_, name)| name.as_str().to_string())
            .collect();
        decoded.push((
            line,
            column,
            token.length,
            legend.token_types[token.token_type as usize]
                .as_str()
                .to_string(),
            modifiers,
        ));
    }
    decoded
}

#[tokio::test]
async fn heterogeneous_uses_do_not_change_aggregate_timing_displays() {
    let document = uri("separate-timing-scopes.mod");
    let text = "var k;\npredetermined_variables k;\nheterogeneity_dimension d;\nvar(heterogeneity=d) h;\nmodel; k(1)=SUM(h); end;\nmodel(heterogeneity=d); h=k(-1); end;";
    let (service, _socket) = new_service();
    let server = service.inner();
    let legend = initialize(
        server,
        Some(&["variable"]),
        &["declaration", "predetermined", "forwardLooking"],
        false,
        false,
    )
    .await;
    open(server, &document, text).await;
    for needle in ["k;", "k(1)", "k(-1)"] {
        let markdown = hover(server, position(&document, text, needle, 0)).await;
        assert!(
            markdown.contains("Timing: **static** · Dynare end-of-period offsets: t"),
            "{markdown}"
        );
        assert!(
            markdown.contains("one period earlier than written offsets"),
            "{markdown}"
        );
        assert!(!markdown.contains("t-1"), "{markdown}");
    }
    let heterogeneous_hover = hover(server, position(&document, text, "h=k", 0)).await;
    assert!(
        heterogeneous_hover.contains("Timing: **static** · written offsets: t"),
        "{heterogeneous_hover}"
    );
    assert!(
        !heterogeneous_hover.contains("one period earlier"),
        "{heterogeneous_hover}"
    );
    let tokens = decode(&full(server, &document).await, &legend);
    let name_tokens: Vec<_> = tokens
        .iter()
        .filter(|token| {
            let written = written_text(
                text,
                Range::new(
                    Position::new(token.0, token.1),
                    Position::new(token.0, token.1 + token.2),
                ),
            );
            matches!(written, "k" | "h")
        })
        .collect();
    assert!(!name_tokens.is_empty());
    assert!(
        name_tokens.iter().all(|token| !token
            .4
            .iter()
            .any(|modifier| matches!(modifier.as_str(), "predetermined" | "forwardLooking"))),
        "{name_tokens:?}"
    );
    let response = server
        .document_symbol(DocumentSymbolParams {
            text_document: TextDocumentIdentifier {
                uri: document.clone(),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    let DocumentSymbolResponse::Nested(outline) = response else {
        panic!("nested outline");
    };
    fn declaration<'a>(rows: &'a [DocumentSymbol], name: &str) -> Option<&'a DocumentSymbol> {
        rows.iter().find_map(|row| {
            if row.name == name {
                Some(row)
            } else {
                row.children
                    .as_ref()
                    .and_then(|children| declaration(children, name))
            }
        })
    }
    for name in ["k", "h"] {
        assert!(
            declaration(&outline, name)
                .unwrap()
                .detail
                .as_deref()
                .unwrap()
                .contains("static"),
            "{outline:?}"
        );
    }
    let map = server
        .execute_command(ExecuteCommandParams {
            command: "dynare/modelInfo".into(),
            arguments: vec![json!({"root_uri":document})],
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    let mcp_info = dygnosis::dynare_model_info(text, None, None);
    for info in [&map, &mcp_info] {
        assert_eq!(info["static"], json!(["k"]));
        assert_eq!(info["n_static"], 1);
        assert_eq!(info["predetermined"], json!([]));
        assert_eq!(info["forward_looking"], json!([]));
    }
    for name in ["k", "h"] {
        let row = map["declarations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == name && row["written_kind"] == "var")
            .unwrap();
        assert_eq!(row["timing"]["class"], "static");
        assert_eq!(row["timing"]["offsets"], json!([0]));
    }
    let equations = dygnosis::dynare_equations(text, None, None, None, None);
    let aggregate = &equations["equations"][0]["idents"][0];
    assert_eq!(
        (
            aggregate["timing"].as_i64(),
            aggregate["dynare_timing"].as_i64()
        ),
        (Some(1), Some(0))
    );
    assert_eq!(aggregate["timing_class"], "static");
    let heterogeneous = &equations["heterogeneous_equations"][0]["equations"][0]["idents"][1];
    assert_eq!(heterogeneous["name"], "k");
    assert_eq!(
        (
            heterogeneous["timing"].as_i64(),
            heterogeneous["dynare_timing"].as_i64()
        ),
        (Some(-1), Some(-1))
    );
    assert_eq!(heterogeneous["timing_class"], "static");
    let summary = dygnosis::structure_summary(&dygnosis::parse(text));
    assert_eq!(
        (
            summary.static_vars,
            summary.predetermined,
            summary.forward_looking,
            summary.max_lead,
            summary.max_lag
        ),
        (1, 0, 0, 0, 0)
    );
}

#[tokio::test]
async fn written_metadata_preferences_and_final_role_icons_are_independent() {
    let document = uri("metadata.mod");
    let text="var y $y_t$ (long_name='Output_[level]');\nparameters p $p$ (long_name='Price');\nvarexo e $e$;\nvarexo_det d $d$;\nchange_type(parameters) y; change_type(var) p; change_type(varexo_det) e; change_type(varexo) d;\nmodel; p=p(-1)+y+e+d; end;\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    initialize(server, None, &[], true, true).await;
    open(server, &document, text).await;
    let items = completion(server, &document).await;
    for (name, kind) in [
        ("p", CompletionItemKind::VARIABLE),
        ("y", CompletionItemKind::CONSTANT),
        ("e", CompletionItemKind::EVENT),
        ("d", CompletionItemKind::EVENT),
    ] {
        assert_eq!(
            items.iter().find(|item| item.label == name).unwrap().kind,
            Some(kind)
        );
    }
    let item = items.iter().find(|item| item.label == "y").unwrap();
    assert_eq!(item.insert_text.as_deref(), Some("y"));
    assert_eq!(
        item.label_details.as_ref().unwrap().description.as_deref(),
        Some("Output_[level]")
    );
    assert_eq!(
        items
            .iter()
            .find(|item| item.label == "e")
            .unwrap()
            .detail
            .as_deref(),
        Some("deterministic exogenous variable")
    );
    let markdown = hover(server, position(&document, text, "p=p(-1)", 0)).await;
    assert!(
        markdown.contains("Endogenous variable")
            && markdown.contains("Price")
            && markdown.contains("`p`"),
        "{markdown}"
    );
    let before = dygnosis::dynare_model_info(text, None, None);
    server
        .did_change_configuration(DidChangeConfigurationParams {
            settings: json!({"nameDetails":{"longName":false,"tex":false}}),
        })
        .await;
    let items = completion(server, &document).await;
    assert!(items
        .iter()
        .find(|item| item.label == "y")
        .unwrap()
        .label_details
        .is_none());
    assert!(items
        .iter()
        .find(|item| item.label == "y")
        .unwrap()
        .documentation
        .is_none());
    let markdown = hover(server, position(&document, text, "p=p(-1)", 0)).await;
    assert!(!markdown.contains("Price") && !markdown.contains("TeX:"));
    assert_eq!(dygnosis::dynare_model_info(text, None, None), before);
    let model = dygnosis::parse(text);
    assert_eq!(
        model.written_declarations[0]
            .declaration
            .long_name
            .as_deref(),
        Some("Output_[level]")
    );
    let tilde = "var y (long_name='A ~~B~~'); model; y=0; end;";
    server
        .did_change_configuration(DidChangeConfigurationParams {
            settings: json!({"nameDetails":{"longName":true}}),
        })
        .await;
    open(server, &document, tilde).await;
    let markdown = hover(server, position(&document, tilde, "y=0", 0)).await;
    assert!(markdown.contains("A \\~\\~B\\~\\~"), "{markdown}");
    let items = completion(server, &document).await;
    let y = items.iter().find(|item| item.label == "y").unwrap();
    assert_eq!(
        y.label_details.as_ref().unwrap().description.as_deref(),
        Some("A ~~B~~")
    );
    assert_eq!(
        dygnosis::parse(tilde).written_declarations[0]
            .declaration
            .long_name
            .as_deref(),
        Some("A ~~B~~")
    );
}

#[tokio::test]
async fn completion_fallback_and_snippets_follow_real_capabilities() {
    for supported in [true, false] {
        let (service, _socket) = new_service();
        let server = service.inner();
        let document = uri("snippets.mod");
        initialize(server, None, &[], false, supported).await;
        open(
            server,
            &document,
            "parameters p $p_t$ (long_name='Discount'); var y; model; y=p; end;",
        )
        .await;
        let items = completion(server, &document).await;
        let parameter = items.iter().find(|item| item.label == "p").unwrap();
        assert_eq!(parameter.detail.as_deref(), Some("parameter · Discount"));
        assert!(parameter.label_details.is_none());
        for block in ["model", "steady_state_model", "initval", "endval", "shocks"] {
            let item = items
                .iter()
                .find(|item| item.label == block && item.kind == Some(CompletionItemKind::SNIPPET))
                .unwrap();
            assert_eq!(
                item.insert_text.as_deref(),
                Some(format!("{block};\n{}\nend;", if supported { "$0" } else { "" }).as_str())
            );
            assert_eq!(
                item.insert_text_format,
                Some(if supported {
                    InsertTextFormat::SNIPPET
                } else {
                    InsertTextFormat::PLAIN_TEXT
                })
            );
        }
    }
}

#[tokio::test]
async fn semantic_capability_matrix_uses_its_returned_legend_for_full_and_range() {
    let custom = [
        "dynareModelLocal",
        "dynareParameter",
        "dynareEndogenous",
        "dynareExogenous",
        "variable",
    ];
    let standard = ["variable", "type", "macro", "parameter"];
    let partial = ["dynareParameter"];
    let nothing = ["function"];
    for types in [
        None,
        Some(custom.as_slice()),
        Some(standard.as_slice()),
        Some(partial.as_slice()),
        Some(nothing.as_slice()),
    ] {
        for modifiers in [
            vec![],
            vec!["predetermined"],
            vec!["forwardLooking", "declaration", "predetermined"],
        ] {
            let (service, _socket) = new_service();
            let server = service.inner();
            let document = uri("legend.mod");
            let legend = initialize(server, types, &modifiers, false, false).await;
            let text =
                "var y z; varexo e; parameters p;\nmodel; #loc=p; y=y(-1)+e+loc; z=z(+1); end;\n";
            open(server, &document, text).await;
            let tokens = full(server, &document).await;
            let decoded = decode(&tokens, &legend);
            if types.is_none() || types == Some(nothing.as_slice()) {
                assert!(decoded.is_empty());
            }
            if types == Some(standard.as_slice()) {
                assert!(decoded.iter().all(|token| token.3 == "variable"));
            }
            if types == Some(partial.as_slice()) {
                assert!(decoded.iter().all(|token| token.3 == "dynareParameter"));
            }
            let range = Range::new(Position::new(1, 0), Position::new(2, 0));
            let response = server
                .semantic_tokens_range(SemanticTokensRangeParams {
                    text_document: TextDocumentIdentifier {
                        uri: document.clone(),
                    },
                    range,
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                })
                .await
                .unwrap()
                .unwrap();
            let SemanticTokensRangeResult::Tokens(tokens) = response else {
                panic!("expected range")
            };
            assert_eq!(
                decode(&tokens.data, &legend),
                decoded
                    .into_iter()
                    .filter(|token| token.0 == 1)
                    .collect::<Vec<_>>()
            );
            assert!(legend
                .token_modifiers
                .iter()
                .all(|modifier| modifiers.contains(&modifier.as_str())));
        }
    }
}

#[tokio::test]
async fn highlight_targets_come_from_parser_and_model_equalities_are_reads() {
    let document = uri("writes.mod");
    let text="var y; parameters p; p=1;\nmodel; #loc=p; y=y(-1)+p+loc; end;\nsteady_state_model; y=p; end;\ninitval; y=0; end;\nendval; y=1; end;\nhistval; y(-1)=0; end;\nfilter_initial_state; y(-1)=0; end;\nepilogue; out=y; end;\nhelper=p;\n// y = p;\n\"y = p\";\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    open(server, &document, text).await;
    let response = server
        .document_highlight(DocumentHighlightParams {
            text_document_position_params: position(&document, text, "y=y(-1)", 0),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    let writes: Vec<_> = response
        .iter()
        .filter(|row| row.kind == Some(DocumentHighlightKind::WRITE))
        .map(|row| row.range.start.line)
        .collect();
    assert_eq!(writes, [0, 2, 3, 4, 5, 6]);
    assert!(response
        .iter()
        .filter(|row| row.range.start.line == 1)
        .all(|row| row.kind == Some(DocumentHighlightKind::READ)));
    assert!(response.iter().all(|row| row.range.start.line < 9));
    let parameter = server
        .document_highlight(DocumentHighlightParams {
            text_document_position_params: position(&document, text, "p=1", 0),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        parameter
            .iter()
            .filter(|row| row.kind == Some(DocumentHighlightKind::WRITE))
            .count(),
        2
    );
    let local = server
        .document_highlight(DocumentHighlightParams {
            text_document_position_params: position(&document, text, "loc=p", 0),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        local
            .iter()
            .filter(|row| row.kind == Some(DocumentHighlightKind::WRITE))
            .count(),
        1
    );
}

#[tokio::test]
async fn displayed_include_preferences_are_independent_of_its_owner() {
    let (service, _socket) = new_service();
    let server = service.inner();
    initialize(server, None, &[], true, true).await;
    let a = uri("a");
    let b = uri("b");
    server
        .did_change_workspace_folders(DidChangeWorkspaceFoldersParams {
            event: WorkspaceFoldersChangeEvent {
                added: vec![
                    WorkspaceFolder {
                        uri: a.clone(),
                        name: "a".into(),
                    },
                    WorkspaceFolder {
                        uri: b.clone(),
                        name: "b".into(),
                    },
                ],
                removed: vec![],
            },
        })
        .await;
    server.did_change_configuration(DidChangeConfigurationParams {
        settings:json!({"dynare":{"configuration":{"schemaVersion":1,"loose":{},"folders":[
            {"uri":a,"settings":{"searchPaths":["C:/dygnosis-names/b"],"nameDetails":{"longName":false,"tex":true}}},
            {"uri":b,"settings":{"nameDetails":{"longName":true,"tex":false}}}
        ]}}}),
    }).await;
    let root = uri("a/root.mod");
    let child = uri("b/body.inc");
    let text = "parameters p $p_t$ (long_name='Discount_[written]');\nvarexo_det d $d_{t+1}$ (long_name='Deterministic');\nvar y;\nmodel;\n@#include \"body.inc\"\nend;\n";
    let body = "y=p+d;\n";
    open(server, &child, body).await;
    open(server, &root, text).await;
    let root_items = completion(server, &root).await;
    let p = root_items.iter().find(|item| item.label == "p").unwrap();
    assert!(p.label_details.is_none());
    assert!(
        matches!(&p.documentation, Some(Documentation::MarkupContent(md)) if md.value == "TeX: `p_t`")
    );
    let included = hover(server, position(&child, body, "p", 0)).await;
    assert!(
        included.contains("Discount\\_\\[written\\]") && !included.contains("TeX:"),
        "{included}"
    );
    let deterministic = hover(server, position(&root, text, "d $", 0)).await;
    assert!(
        deterministic.contains("Deterministic exogenous variable")
            && deterministic.contains("`d_{t+1}`"),
        "{deterministic}"
    );
    let comparison =
        dygnosis::compare_models(&dygnosis::parse(text), &dygnosis::parse(text)).to_json();
    server.did_change_configuration(DidChangeConfigurationParams {
        settings:json!({"dynare":{"configuration":{"schemaVersion":1,"loose":{},"folders":[
            {"uri":a,"settings":{"searchPaths":["C:/dygnosis-names/b"],"nameDetails":{"longName":false,"tex":true}}},
            {"uri":b,"settings":{"nameDetails":{"longName":false,"tex":true}}}
        ]}}}),
    }).await;
    let included = hover(server, position(&child, body, "p", 0)).await;
    assert!(
        !included.contains("Discount") && included.contains("`p_t`"),
        "{included}"
    );
    let metadata_items = completion(server, &child).await;
    assert_eq!(metadata_items.len(), 1);
    assert_eq!(metadata_items[0].label, "name");
    let child_items = completion_at(server, &child, position(&child, body, "p", 0).position).await;
    let p = child_items.iter().find(|item| item.label == "p").unwrap();
    assert_eq!(p.insert_text.as_deref(), Some("p"));
    assert!(p.label_details.is_none() && p.documentation.is_some());
    assert_eq!(
        dygnosis::compare_models(&dygnosis::parse(text), &dygnosis::parse(text)).to_json(),
        comparison
    );
}

#[tokio::test]
async fn semantic_roles_keep_local_scopes_final_types_and_removed_sites() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let document = uri("scopes.mod");
    let legend = initialize(
        server,
        Some(&[
            "dynareEndogenous",
            "dynareExogenous",
            "dynareParameter",
            "dynareModelLocal",
        ]),
        &["declaration", "predetermined", "forwardLooking"],
        false,
        false,
    )
    .await;
    let text = "heterogeneity_dimension h;\nvar y gone;\nvar(heterogeneity=h) hx x;\nparameters q;\nvarexo_det d;\nmodel_local_variable declared_local;\nvar_remove gone;\nchange_type(parameters) x;\nmodel; #declared_local=q; y=q+d+x+declared_local; end;\nmodel(heterogeneity=h); #loc=q; hx=hx(-1)+loc; end;\nhelper=loc;\n";
    open(server, &document, text).await;
    let tokens = decode(&full(server, &document).await, &legend);
    let named: Vec<_> = tokens
        .iter()
        .map(|token| {
            (
                written_text(
                    text,
                    Range::new(
                        Position::new(token.0, token.1),
                        Position::new(token.0, token.1 + token.2),
                    ),
                ),
                token,
            )
        })
        .collect();
    assert!(named
        .iter()
        .filter(|(name, _)| *name == "x")
        .all(|(_, token)| token.3 == "dynareParameter"
            && !token
                .4
                .iter()
                .any(|modifier| modifier == "predetermined" || modifier == "forwardLooking")));
    assert!(named.iter().all(|(name, _)| *name != "gone"));
    assert!(named
        .iter()
        .filter(|(name, _)| *name == "hx")
        .all(|(_, token)| token.3 == "dynareEndogenous"
            && token.4.iter().any(|modifier| modifier == "predetermined")));
    assert!(named
        .iter()
        .filter(|(name, _)| *name == "d")
        .all(|(_, token)| token.3 == "dynareExogenous"));
    assert!(named
        .iter()
        .filter(|(name, _)| *name == "declared_local")
        .all(|(_, token)| token.3 == "dynareModelLocal"));
    let locals: Vec<_> = named.iter().filter(|(name, _)| *name == "loc").collect();
    assert_eq!(locals.len(), 2);
    assert!(locals
        .iter()
        .all(|(_, token)| token.0 == 9 && token.3 == "dynareModelLocal"));
    let copies = "@#for k in 1:2\nvar x@{k};\n@#endfor\nvar_remove x1;\nmodel; x2=0; end;\n";
    open(server, &document, copies).await;
    let tokens = decode(&full(server, &document).await, &legend);
    assert!(tokens.iter().all(|token| token.0 != 1), "{tokens:?}");
    assert!(tokens
        .iter()
        .any(|token| token.0 == 4 && token.3 == "dynareEndogenous"));
}

#[tokio::test]
async fn mapped_name_ranges_use_original_utf16_and_mixed_line_endings() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let legend = initialize(
        server,
        Some(&["dynareEndogenous", "dynareParameter"]),
        &["declaration"],
        false,
        false,
    )
    .await;
    let root = uri("positions.mod");
    let child = uri("calibration.inc");
    let text = "/* β🚀 */ var y;\r\nparameters p $p_t$;\rmodel; y=p; end;\n@#include \"calibration.inc\"\n";
    let body = "/* 🚀β */ p=2;\r\nsteady_state_model;\r/* 🧮 */ y=p;\nend;\r\n";
    open(server, &child, body).await;
    open(server, &root, text).await;
    for (document, source, needle, expected) in [
        (&root, text, "y=p", "Endogenous variable"),
        (&child, body, "p=2", "Parameter"),
        (&child, body, "y=p", "Endogenous variable"),
    ] {
        let pos = position(document, source, needle, 0);
        let response = server
            .hover(HoverParams {
                text_document_position_params: pos.clone(),
                work_done_progress_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(written_text(source, response.range.unwrap()), &needle[..1]);
        assert!(
            matches!(response.contents,HoverContents::Markup(md) if md.value.contains(expected))
        );
        let highlights = server
            .document_highlight(DocumentHighlightParams {
                text_document_position_params: pos,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();
        assert!(highlights
            .iter()
            .all(|row| written_text(source, row.range) == &needle[..1]));
    }
    let tokens = decode(&full(server, &child).await, &legend);
    assert!(tokens.iter().all(|token| matches!(
        written_text(
            body,
            Range::new(
                Position::new(token.0, token.1),
                Position::new(token.0, token.1 + token.2)
            )
        ),
        "p" | "y"
    )));
    assert!(tokens
        .iter()
        .any(|token| token.0 == 2 && token.3 == "dynareEndogenous"));
    let writes = server
        .document_highlight(DocumentHighlightParams {
            text_document_position_params: position(&child, body, "y=p", 0),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(writes[0].kind, Some(DocumentHighlightKind::WRITE));
}

#[tokio::test]
async fn opaque_and_partial_targets_do_not_gain_fabricated_writes() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let document = uri("partial.mod");
    let text="var y; parameters p;\n[y,p]=native_call();\nobj.y=p;\nmodel; y=; end;\nsteady_state_model; y=; end;\nsteady_state_model; y(0)=1; end;\nmodel; #y(0)=1; y=0; end;\n";
    open(server, &document, text).await;
    let rows = server
        .document_highlight(DocumentHighlightParams {
            text_document_position_params: position(&document, text, "[y,p]", 1),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        rows.iter()
            .filter(|row| row.kind == Some(DocumentHighlightKind::WRITE))
            .count(),
        1
    );
    assert!(
        rows.iter()
            .filter(|row| row.range.start.line > 0)
            .all(|row| row.kind == Some(DocumentHighlightKind::READ)),
        "{rows:?}"
    );
}

#[tokio::test]
async fn several_owners_keep_common_facts_and_withhold_conflicting_roles_and_timing() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let legend = initialize(
        server,
        Some(&["dynareEndogenous", "dynareParameter"]),
        &["declaration", "predetermined", "forwardLooking"],
        true,
        false,
    )
    .await;
    let child = uri("shared.inc");
    let a = uri("owner_a.mod");
    let b = uri("owner_b.mod");
    let body = "var y $y$ (long_name='Output'); parameters p $p$; p=2;\n";
    let root = "@#include \"shared.inc\"\nmodel; y=y(-1)+p; end;\n";
    open(server, &child, body).await;
    open(server, &a, root).await;
    open(server, &b, root).await;
    assert!(hover(server, position(&child, body, "y $", 0))
        .await
        .contains("Output"));
    let tokens = decode(&full(server, &child).await, &legend);
    assert!(tokens.iter().any(|token| token.3 == "dynareEndogenous"
        && token.4.iter().any(|modifier| modifier == "predetermined")));
    open(
        server,
        &b,
        "@#include \"shared.inc\"\nmodel; y=y(+1)+p; end;\n",
    )
    .await;
    let tokens = decode(&full(server, &child).await, &legend);
    assert!(tokens
        .iter()
        .filter(|token| token.3 == "dynareEndogenous")
        .all(|token| !token
            .4
            .iter()
            .any(|modifier| modifier == "predetermined" || modifier == "forwardLooking")));
    open(
        server,
        &b,
        "@#include \"shared.inc\"\nchange_type(parameters) y; var z; model; z=y+p; end;\n",
    )
    .await;
    let tokens = decode(&full(server, &child).await, &legend);
    assert!(tokens.iter().all(|token| token.3 == "dynareParameter"));
    let items = completion(server, &child).await;
    assert!(!items.iter().any(|item| item.label == "y"));
    assert!(items
        .iter()
        .any(|item| item.label == "p" && item.kind == Some(CompletionItemKind::CONSTANT)));
    assert!(server
        .hover(HoverParams {
            text_document_position_params: position(&child, body, "y $", 0),
            work_done_progress_params: Default::default()
        })
        .await
        .unwrap()
        .is_none());
    let highlights = server
        .document_highlight(DocumentHighlightParams {
            text_document_position_params: position(&child, body, "y $", 0),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(highlights[0].kind, Some(DocumentHighlightKind::WRITE));
    let row = uri("equation.inc");
    let written = "y=p;\n";
    open(server, &row, written).await;
    open(
        server,
        &uri("read_owner.mod"),
        "var y; parameters p; model;\n@#include \"equation.inc\"\nend;\n",
    )
    .await;
    open(server,&uri("write_owner.mod"),"var y; parameters p; model; y=p; end; steady_state_model;\n@#include \"equation.inc\"\nend;\n").await;
    let highlights = server
        .document_highlight(DocumentHighlightParams {
            text_document_position_params: position(&row, written, "y", 0),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(highlights[0].kind, Some(DocumentHighlightKind::READ));
}

#[tokio::test]
async fn rejected_pound_shadows_preserve_final_roles_and_accepted_locals_stay_scoped() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let document = uri("shadow.mod");
    let legend = initialize(
        server,
        Some(&[
            "dynareEndogenous",
            "dynareExogenous",
            "dynareParameter",
            "dynareModelLocal",
        ]),
        &[],
        false,
        false,
    )
    .await;
    for (text, name, expected) in [
        (
            include_str!("fixtures/e020/e025_shadow.mod"),
            "y",
            Some("dynareEndogenous"),
        ),
        (
            "var y; parameters p; model; #p=1; y=p; end;",
            "p",
            Some("dynareParameter"),
        ),
        (
            "var y gone; var_remove gone; model; #gone=1; y=gone; end;",
            "gone",
            None,
        ),
        (
            "parameters p; p=borrowed; var y; model; #borrowed=1; y=borrowed; end;",
            "borrowed",
            None,
        ),
        (
            "var y; parameters p; model; #loc=p; y=loc; end;",
            "loc",
            Some("dynareModelLocal"),
        ),
    ] {
        open(server, &document, text).await;
        let tokens = decode(&full(server, &document).await, &legend);
        let matching: Vec<_> = tokens
            .iter()
            .filter(|token| {
                written_text(
                    text,
                    Range::new(
                        Position::new(token.0, token.1),
                        Position::new(token.0, token.1 + token.2),
                    ),
                ) == name
            })
            .collect();
        if let Some(expected) = expected {
            assert!(!matching.is_empty(), "{name}: {tokens:?}");
            assert!(
                matching.iter().all(|token| token.3 == expected),
                "{name}: {matching:?}"
            );
        } else {
            assert!(matching.is_empty(), "{name}: {matching:?}");
        }
    }
}
