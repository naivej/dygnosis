use dygnosis::{compare_models, parse};
use std::collections::HashMap;
use tower_lsp::{lsp_types::*, LanguageServer};

#[test]
fn compare_includes_values_of_retyped_parameters() {
    let before = "var y z; change_type(parameters) z; z=.3; model; y=z; end;";
    let after = before.replace("z=.3", "z=.4");
    let diff = compare_models(&parse(before), &parse(&after)).to_json();
    assert_eq!(diff["changed_parameter_values"][0]["name"], "z", "{diff}");
}

#[test]
fn compare_uses_final_kinds_and_keeps_written_metadata() {
    let before = "var y z(long_name='level'); change_type(parameters) z; z=.3; model; y=z; end;";
    let after = "var y; parameters z(long_name='level'); z=.3; model; y=z; end;";
    let diff = compare_models(&parse(before), &parse(after)).to_json();
    assert_eq!(
        diff["common_parameters"],
        serde_json::json!(["z"]),
        "{diff}"
    );
    assert_eq!(
        diff["common_endogenous"],
        serde_json::json!(["y"]),
        "{diff}"
    );
    assert_eq!(diff["symbols_changed"], serde_json::json!([]), "{diff}");

    let before = "heterogeneity_dimension h; var y; var(heterogeneity=h) z(long_name='level'); change_type(parameters) z; model; y=z; end;";
    let after = "var y; parameters z(long_name='level'); model; y=z; end;";
    let diff = compare_models(&parse(before), &parse(after)).to_json();
    assert_eq!(diff["symbols_changed"], serde_json::json!([]), "{diff}");

    let before = "var y; varexo e; model; y=e; end;";
    let after = "var y; varexo e; change_type(varexo_det) e; model; y=e; end;";
    let diff = compare_models(&parse(before), &parse(after)).to_json();
    assert_eq!(
        diff["symbols_changed"][0]["after"]["kind"], "varexo_det",
        "{diff}"
    );
}

#[tokio::test]
async fn completion_and_semantic_tokens_agree_with_final_type_views() {
    let source = "var y z;\nparameters p;\nchange_type(parameters) z;\nchange_type(var) p;\nmodel;\ny=z+p;\np=0;\nend;";
    let uri = Url::parse("file:///parameter_views.mod").unwrap();
    let (service, _) = dygnosis::server::new_service();
    let server = service.inner();
    let initialized = server
        .initialize(InitializeParams {
            capabilities: serde_json::from_value(serde_json::json!({
                "textDocument": {"semanticTokens": {
                    "requests": {"full": true},
                    "tokenTypes": ["dynareParameter", "dynareEndogenous"],
                    "tokenModifiers": [],
                    "formats": ["relative"]
                }}
            }))
            .unwrap(),
            ..Default::default()
        })
        .await
        .unwrap();
    let SemanticTokensServerCapabilities::SemanticTokensOptions(options) =
        initialized.capabilities.semantic_tokens_provider.unwrap()
    else {
        panic!("expected semantic token legend");
    };
    let legend = options.legend;
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
    let response = server
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position::new(5, 2),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .unwrap()
        .unwrap();
    let items = match response {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    for (name, detail) in [("z", "parameter"), ("p", "endogenous variable")] {
        let matching: Vec<_> = items.iter().filter(|item| item.label == name).collect();
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].detail.as_deref(), Some(detail));
    }
    let response = server
        .semantic_tokens_full(SemanticTokensParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    let SemanticTokensResult::Tokens(tokens) = response else {
        panic!("full tokens");
    };
    let mut line = 0;
    let mut character = 0;
    let mut kinds = HashMap::new();
    for token in tokens.data {
        line += token.delta_line;
        character = if token.delta_line == 0 {
            character + token.delta_start
        } else {
            token.delta_start
        };
        if line == 5 {
            kinds.insert(character, token.token_type);
        }
    }
    assert_eq!(
        legend.token_types[kinds[&2] as usize].as_str(),
        "dynareParameter",
        "parameter z"
    );
    assert_eq!(
        legend.token_types[kinds[&4] as usize].as_str(),
        "dynareEndogenous",
        "endogenous p"
    );
}
