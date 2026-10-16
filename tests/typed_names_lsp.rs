//! Typed names must reach real editor requests through accepted source sites.

use dygnosis::server::{Backend, new_service};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

fn uri(name: &str) -> Url {
    Url::from_file_path(
        std::path::Path::new(if cfg!(windows) {
            "C:/typed-names-lsp"
        } else {
            "/typed-names-lsp"
        })
        .join(name),
    )
    .unwrap()
}

async fn open(server: &Backend, uri: &Url, text: &str) {
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version: 1,
                text: text.into(),
            },
        })
        .await;
}

fn position(uri: &Url, text: &str, needle: &str, offset: usize) -> TextDocumentPositionParams {
    let index = dygnosis::span::LineIndex::new(text);
    let position = index.position_utf16(text, (text.find(needle).unwrap() + offset) as u32);
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        position: Position::new(position.line, position.character),
    }
}

async fn help(server: &Backend, pos: TextDocumentPositionParams) -> Option<Hover> {
    server
        .hover(HoverParams {
            text_document_position_params: pos,
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
}

fn markdown(hover: &Hover) -> &str {
    match &hover.contents {
        HoverContents::Markup(content) => &content.value,
        _ => panic!("Markdown hover"),
    }
}

async fn initialize(server: &Backend, custom: bool) -> SemanticTokensLegend {
    let types = if custom {
        vec![
            "dynareDimension",
            "dynareDatabase",
            "dynareModel",
            "dynareSubsample",
            "dynareTrend",
            "dynareExternalFunction",
        ]
    } else {
        vec!["namespace", "variable", "property", "function"]
    };
    let result = server.initialize(InitializeParams { capabilities: serde_json::from_value(serde_json::json!({"textDocument":{"semanticTokens":{"requests":{"full":true,"range":true},"tokenTypes":types,"tokenModifiers":["declaration"],"formats":["relative"]}}})).unwrap(), ..Default::default() }).await.unwrap();
    match result.capabilities.semantic_tokens_provider.unwrap() {
        SemanticTokensServerCapabilities::SemanticTokensOptions(options) => options.legend,
        _ => panic!("semantic token legend"),
    }
}

async fn tokens(
    server: &Backend,
    uri: &Url,
    legend: &SemanticTokensLegend,
) -> Vec<(Range, String, bool)> {
    let result = server
        .semantic_tokens_full(SemanticTokensParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    let SemanticTokensResult::Tokens(result) = result else {
        panic!("tokens");
    };
    let mut line = 0;
    let mut column = 0;
    result
        .data
        .into_iter()
        .map(|token| {
            line += token.delta_line;
            column = if token.delta_line == 0 {
                column + token.delta_start
            } else {
                token.delta_start
            };
            let declaration = legend
                .token_modifiers
                .iter()
                .enumerate()
                .any(|(index, modifier)| {
                    modifier.as_str() == "declaration"
                        && token.token_modifiers_bitset & (1 << index) != 0
                });
            (
                Range::new(
                    Position::new(line, column),
                    Position::new(line, column + token.length),
                ),
                legend.token_types[token.token_type as usize]
                    .as_str()
                    .to_string(),
                declaration,
            )
        })
        .collect()
}

const SOURCE: &str = "heterogeneity_dimension h;\nvar y;\nvar(heterogeneity=h) hx;\nvarexo e;\nparameters b;\ntrend_var(growth_factor=1.01) trend;\nlog_trend_var(log_growth_factor=.01) logtrend;\nexternal_function(name=ext,nargs=1);\ndatabase db;\nmodel;\n[name='aux'] y=b*y(-1)+e+ext(trend)+logtrend;\nend;\nmodel(heterogeneity=h); hx=y; end;\nvar_model(model_name=vm,eqtags=['aux']);\nvar_expectation_model(model_name=ve,auxiliary_model_name=vm,horizon=1,variable=y);\npac_model(model_name=pm,discount=b,auxiliary_model_name=vm);\nmodel;\n[name='wanted'] y=var_expectation(ve)+pac_expectation(pm);\nend;\nstd(e).subsamples(early=2000Q1:2000Q2);\nstd(e).early.options(jscale=.2);\nshock_paths; var e; periods 1; values db.x; end;\nverbatim;\nh db vm ve pm early trend logtrend ext;\nend;\n";

#[tokio::test]
async fn accepted_typed_names_have_exact_hover_and_negotiated_roles() {
    // Guards losing accepted identities in editor requests or coloring equal spellings in native text.
    for custom in [false, true] {
        let (service, _socket) = new_service();
        let server = service.inner();
        let legend = initialize(server, custom).await;
        let document = uri("roles.mod");
        open(server, &document, SOURCE).await;
        let tokens = tokens(server, &document, &legend).await;
        for (needle, offset, title, standard, custom_type, declaration) in [
            (
                "dimension h",
                10,
                "Heterogeneity dimension",
                "namespace",
                "dynareDimension",
                true,
            ),
            (
                "heterogeneity=h",
                14,
                "Heterogeneity dimension",
                "namespace",
                "dynareDimension",
                false,
            ),
            (
                "database db",
                9,
                "Database",
                "variable",
                "dynareDatabase",
                true,
            ),
            ("db.x", 0, "Database", "variable", "dynareDatabase", false),
            (
                "model_name=vm",
                11,
                "VAR auxiliary model",
                "namespace",
                "dynareModel",
                true,
            ),
            (
                "auxiliary_model_name=vm",
                21,
                "VAR auxiliary model",
                "namespace",
                "dynareModel",
                false,
            ),
            (
                "var_expectation(ve",
                16,
                "VAR expectation model",
                "namespace",
                "dynareModel",
                false,
            ),
            (
                "pac_expectation(pm",
                16,
                "PAC model",
                "namespace",
                "dynareModel",
                false,
            ),
            (
                "subsamples(early",
                11,
                "Named subsample",
                "property",
                "dynareSubsample",
                true,
            ),
            (
                "early.options",
                0,
                "Named subsample",
                "property",
                "dynareSubsample",
                false,
            ),
            (
                "trend;",
                0,
                "Trend variable",
                "variable",
                "dynareTrend",
                true,
            ),
            (
                "ext(trend",
                4,
                "Trend variable",
                "variable",
                "dynareTrend",
                false,
            ),
            (
                "logtrend;",
                0,
                "Log trend variable",
                "variable",
                "dynareTrend",
                true,
            ),
            (
                "name=ext",
                5,
                "External function",
                "function",
                "dynareExternalFunction",
                true,
            ),
            (
                "ext(trend",
                0,
                "External function",
                "function",
                "dynareExternalFunction",
                false,
            ),
        ] {
            let pos = position(&document, SOURCE, needle, offset);
            let hover = help(server, pos.clone())
                .await
                .unwrap_or_else(|| panic!("missing {needle} hover"));
            assert!(markdown(&hover).contains(title), "{needle}: {hover:?}");
            let range = hover.range.unwrap();
            assert_eq!(range.start, pos.position, "{needle}: exact selection");
            let expected = if custom { custom_type } else { standard };
            assert!(
                tokens
                    .iter()
                    .any(|(token_range, kind, decl)| token_range == &range
                        && kind == expected
                        && *decl == declaration),
                "{needle}: {tokens:?}"
            );
        }
        let native = position(&document, SOURCE, "h db vm", 0).position.line;
        assert!(
            tokens
                .iter()
                .all(|(range, _, _)| range.start.line != native),
            "{tokens:?}"
        );
        assert!(
            help(server, position(&document, SOURCE, "h db vm", 0))
                .await
                .is_none()
        );
    }
}

async fn completions(marked: &str) -> Vec<CompletionItem> {
    let (service, _socket) = new_service();
    let document = uri("completion.mod");
    let byte = marked.find('|').unwrap();
    let source = marked.replace('|', "");
    open(service.inner(), &document, &source).await;
    let index = dygnosis::span::LineIndex::new(&source);
    let pos = index.position_utf16(&source, byte as u32);
    let result = service
        .inner()
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: document },
                position: Position::new(pos.line, pos.character),
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

#[tokio::test]
async fn named_argument_completion_uses_its_identity_kind() {
    // Guards suggesting ordinary variables or another identity kind in named operator/option slots.
    let prefix = "heterogeneity_dimension h;\nvar y;\nparameters b;\nvar_model(model_name=vm,eqtags=['aux']);\ntrend_component_model(model_name=tm,eqtags=['aux'],targets=['aux']);\nvar_expectation_model(model_name=ve,auxiliary_model_name=vm,horizon=1,variable=y);\npac_model(model_name=pm,discount=b);\n";
    for (suffix, expected, excluded) in [
        (
            "model; y=var_expectation(|); end;",
            vec!["ve"],
            vec!["vm", "pm", "h", "y", "log"],
        ),
        (
            "model; y=pac_expectation(|); end;",
            vec!["pm"],
            vec!["vm", "ve", "h", "y", "log"],
        ),
        (
            "model; y=pac_target_nonstationary(|); end;",
            vec!["pm"],
            vec!["vm", "ve", "h", "y"],
        ),
        (
            "pac_target_info(|); target y; end;",
            vec!["pm"],
            vec!["vm", "ve", "h", "y"],
        ),
        (
            "var(heterogeneity=|) hx;",
            vec!["h"],
            vec!["pm", "vm", "ve", "y"],
        ),
        (
            "pac_model(model_name=new,discount=b,auxiliary_model_name=|);",
            vec!["vm", "tm"],
            vec!["pm", "ve", "h", "y"],
        ),
        (
            "pac_model(model_name=|,discount=b);",
            vec![],
            vec!["pm", "vm", "ve", "h", "y", "log"],
        ),
    ] {
        let items = completions(&format!("{prefix}{suffix}")).await;
        for name in expected {
            assert!(
                items.iter().any(|item| item.label == name),
                "{suffix}: {items:?}"
            );
        }
        for name in excluded {
            assert!(
                !items.iter().any(|item| item.label == name),
                "{suffix}: {items:?}"
            );
        }
    }
    assert!(
        completions(&format!("{prefix}verbatim;\npac_expectation(|)\nend;"))
            .await
            .is_empty()
    );
    let both =
        format!("{prefix}trend_component_model(model_name=vm,eqtags=['aux'],targets=['aux']);\n");
    for (owner, expected) in [
        ("pac_model", "Trend component auxiliary model"),
        ("var_expectation_model", "VAR auxiliary model"),
    ] {
        let items = completions(&format!(
            "{both}{owner}(model_name=new,auxiliary_model_name=|);"
        ))
        .await;
        let vm: Vec<_> = items.iter().filter(|item| item.label == "vm").collect();
        assert_eq!(vm.len(), 1, "{owner}: {vm:?}");
        assert_eq!(vm[0].detail.as_deref(), Some(expected));
    }
}

#[tokio::test]
async fn typed_sites_in_includes_keep_written_utf16_ranges_and_omit_generated_names() {
    // Guards using root offsets for a helper file or assigning a template prefix the generated name's role.
    let (service, _socket) = new_service();
    let server = service.inner();
    let legend = initialize(server, false).await;
    let child = uri("decl.inc");
    let root = uri("root.mod");
    let body = "/* 🚀 */ heterogeneity_dimension h;\nvar_model(model_name=vm,eqtags=['eq']);\n";
    let source = "@#include \"decl.inc\"\nvar y; parameters b; model; [name='eq'] y=0; end;\n@#for i in 1:2\npac_model(model_name=pm@{i},discount=b);\n@#endfor\n";
    open(server, &child, body).await;
    open(server, &root, source).await;
    let files = std::collections::BTreeMap::from([
        (root.to_string(), source.to_string()),
        (child.to_string(), body.to_string()),
    ]);
    let mut workspace = dygnosis::Workspace::new();
    for (file, text) in files {
        workspace.update_document(&file, text);
    }
    let expanded = workspace.get_effective_model(root.as_str()).unwrap();
    for name in ["pm1", "pm2"] {
        assert!(expanded.semi_structural_commands.iter().flat_map(|command| &command.options).any(|option| option.name == "model_name" && matches!(option.value,dygnosis::model::SemiStructuralValue::Symbol { name: id, .. } if expanded.name(id) == name)), "generated declaration {name} must be retained before testing its written projection");
    }
    let hover = help(server, position(&child, body, "dimension h", 10))
        .await
        .unwrap();
    let expected = position(&child, body, "dimension h", 10).position;
    assert_eq!(hover.range.unwrap().start, expected);
    let child_tokens = tokens(server, &child, &legend).await;
    assert!(
        child_tokens
            .iter()
            .any(|(range, kind, _)| range.start == expected && kind == "namespace")
    );
    let template = position(&root, source, "pm@{i}", 0).position;
    assert!(
        tokens(server, &root, &legend)
            .await
            .iter()
            .all(|(range, _, _)| range.start != template)
    );
}

#[tokio::test]
async fn quoted_external_declarations_are_exact_inner_name_sites() {
    // Guards dropping a valid quoted name declaration while its calls still have a semantic role.
    let (service, _socket) = new_service();
    let server = service.inner();
    let legend = initialize(server, false).await;
    let document = uri("quoted.mod");
    let source = "external_function(name='ext',nargs=1);\nvar y; model; y=ext(y(-1)); end;\n";
    open(server, &document, source).await;
    let pos = position(&document, source, "'ext'", 1);
    let hover = help(server, pos.clone()).await.unwrap();
    assert!(markdown(&hover).contains("External function"));
    let range = hover.range.unwrap();
    assert_eq!(
        range,
        Range::new(
            pos.position,
            Position::new(pos.position.line, pos.position.character + 3)
        )
    );
    assert!(
        tokens(server, &document, &legend)
            .await
            .iter()
            .any(|(site, kind, decl)| *site == range && kind == "function" && *decl)
    );
}

#[tokio::test]
async fn typed_roles_fall_back_to_an_advertised_variable_type() {
    // Guards suppressing accepted names when a client lacks namespace/property/function types.
    let (service, _socket) = new_service();
    let server = service.inner();
    let result = server.initialize(InitializeParams {
        capabilities: serde_json::from_value(serde_json::json!({"textDocument":{"semanticTokens":{"requests":{"full":true},"tokenTypes":["variable"],"tokenModifiers":["declaration"],"formats":["relative"]}}})).unwrap(),
        ..Default::default()
    }).await.unwrap();
    let SemanticTokensServerCapabilities::SemanticTokensOptions(options) =
        result.capabilities.semantic_tokens_provider.unwrap()
    else {
        panic!("legend");
    };
    let document = uri("fallback.mod");
    open(server, &document, SOURCE).await;
    let tokens = tokens(server, &document, &options.legend).await;
    for (needle, offset) in [
        ("dimension h", 10),
        ("model_name=vm", 11),
        ("early.options", 0),
        ("name=ext", 5),
    ] {
        let pos = position(&document, SOURCE, needle, offset).position;
        assert!(
            tokens
                .iter()
                .any(|(range, kind, _)| range.start == pos && kind == "variable"),
            "{needle}: {tokens:?}"
        );
    }
}

#[tokio::test]
async fn model_expression_completion_keeps_trends_and_external_functions() {
    // Guards dropping declared expression names while keeping only built-in functions.
    let source = "trend_var(growth_factor=1.01) trend;\nexternal_function(name=ext,nargs=1);\nvar y; model; y=|; end;\n";
    let items = completions(source).await;
    for (name, kind) in [
        ("trend", CompletionItemKind::VARIABLE),
        ("ext", CompletionItemKind::FUNCTION),
    ] {
        assert!(
            items
                .iter()
                .any(|item| item.label == name && item.kind == Some(kind)),
            "{items:?}"
        );
    }
    assert!(
        completions(&source.replace("model; y=|; end;", "verbatim;\n|\nend;"))
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn outline_selects_declared_identities_without_adding_each_operator_call() {
    // Guards missing identity children or selecting a later use instead of the written declaration.
    fn leaves<'a>(rows: &'a [DocumentSymbol], out: &mut Vec<&'a DocumentSymbol>) {
        for row in rows {
            out.push(row);
            if let Some(children) = &row.children {
                leaves(children, out);
            }
        }
    }
    let (service, _socket) = new_service();
    let server = service.inner();
    let document = uri("outline.mod");
    open(server, &document, SOURCE).await;
    let result = server
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
    let DocumentSymbolResponse::Nested(rows) = result else {
        panic!("nested Outline");
    };
    let mut all = Vec::new();
    leaves(&rows, &mut all);
    for (name, needle, offset) in [
        ("h", "dimension h", 10),
        ("db", "database db", 9),
        ("vm", "model_name=vm", 11),
        ("ve", "model_name=ve", 11),
        ("pm", "model_name=pm", 11),
        ("early", "subsamples(early", 11),
        ("ext", "name=ext", 5),
    ] {
        let expected = position(&document, SOURCE, needle, offset).position;
        let matching: Vec<_> = all.iter().filter(|row| row.name == name).collect();
        assert_eq!(matching.len(), 1, "{name}: {matching:?}");
        assert_eq!(
            matching[0].selection_range,
            Range::new(
                expected,
                Position::new(expected.line, expected.character + name.len() as u32)
            ),
            "{name}"
        );
    }
}
