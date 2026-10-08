use dygnosis::server::{new_service, Backend};
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

fn at(uri: &Url, text: &str, marker: &str) -> TextDocumentPositionParams {
    let byte = text.find(marker).unwrap() + marker.len() - 1;
    let prefix = &text[..byte];
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        position: Position::new(
            prefix.bytes().filter(|byte| *byte == b'\n').count() as u32,
            prefix.rsplit('\n').next().unwrap().encode_utf16().count() as u32,
        ),
    }
}

async fn open(server: &Backend, uri: &Url, text: &str) {
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version: 7,
                text: text.into(),
            },
        })
        .await;
}

async fn complete(server: &Backend, pos: TextDocumentPositionParams) -> Vec<CompletionItem> {
    match server
        .completion(CompletionParams {
            text_document_position: pos,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .unwrap()
    {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    }
}

async fn hover(server: &Backend, pos: TextDocumentPositionParams) -> Option<String> {
    server
        .hover(HoverParams {
            text_document_position_params: pos,
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
        .map(|hover| match hover.contents {
            HoverContents::Markup(content) => content.value,
            other => panic!("{other:?}"),
        })
}

async fn refs(server: &Backend, pos: TextDocumentPositionParams, include: bool) -> Vec<Location> {
    server
        .references(ReferenceParams {
            text_document_position: pos,
            context: ReferenceContext {
                include_declaration: include,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap_or_default()
}

async fn target(
    server: &Backend,
    pos: TextDocumentPositionParams,
    declaration: bool,
) -> Option<Location> {
    let params = GotoDefinitionParams {
        text_document_position_params: pos,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let result = if declaration {
        server.goto_declaration(params).await
    } else {
        server.goto_definition(params).await
    }
    .unwrap();
    result.map(|result| match result {
        GotoDefinitionResponse::Scalar(location) => location,
        other => panic!("{other:?}"),
    })
}

#[tokio::test]
async fn local_completion_respects_order_role_and_value_sites() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let uri = Url::parse("file:///tmp/local-completion.mod").unwrap();
    let text = "var y; parameters p; p=.9;\nmodel;\ny=0;\n#z=p*y(-1);\n#w=z+1;\ny=z(+1);\n// z comment\nend;\np=0;";
    open(server, &uri, text).await;
    assert!(!complete(server, at(&uri, text, "y=0"))
        .await
        .iter()
        .any(|item| item.label == "z"));
    let items = complete(server, at(&uri, text, "#w=z")).await;
    let local = items.iter().find(|item| item.label == "z").unwrap();
    assert_eq!(local.kind, Some(CompletionItemKind::VARIABLE));
    assert_eq!(local.detail.as_deref(), Some("model-local variable"));
    assert_eq!(local.filter_text.as_deref(), Some("z"));
    assert_eq!(local.insert_text.as_deref(), Some("z"));
    assert!(items.iter().any(|item| item.label == "p"));
    assert!(!complete(server, at(&uri, text, "#w"))
        .await
        .iter()
        .any(|item| item.label == "z"));
    assert!(!complete(server, at(&uri, text, "// z"))
        .await
        .iter()
        .any(|item| item.label == "z"));
    assert!(!complete(server, at(&uri, text, "p=0"))
        .await
        .iter()
        .any(|item| item.label == "z"));
    let partial = "var y; parameters p; model; #z=p; y=z";
    open(server, &uri, partial).await;
    assert!(complete(server, at(&uri, partial, "y=z"))
        .await
        .iter()
        .any(|item| item.label == "z"));
}

#[tokio::test]
async fn explicit_local_forward_use_has_metadata_and_distinct_navigation() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let uri = Url::parse("file:///tmp/local-forward.mod").unwrap();
    let text =
        "var y; parameters p;\nmodel_local_variable z $Z_t$;\nmodel;\ny=z;\n#z=p*y(-1);\nend;";
    open(server, &uri, text).await;
    let pos = at(&uri, text, "y=z");
    assert!(complete(server, pos.clone())
        .await
        .iter()
        .any(|item| item.label == "z"));
    let help = hover(server, pos.clone()).await.unwrap();
    assert!(help.contains("Model-local variable"), "{help}");
    assert!(help.contains("Z_t"), "{help}");
    assert!(help.contains("p") && help.contains("y"), "{help}");
    assert_eq!(
        target(server, pos.clone(), false)
            .await
            .unwrap()
            .range
            .start
            .line,
        4
    );
    assert_eq!(
        target(server, pos.clone(), true)
            .await
            .unwrap()
            .range
            .start
            .line,
        1
    );
    assert_eq!(refs(server, pos.clone(), false).await.len(), 1);
    assert_eq!(refs(server, pos.clone(), true).await.len(), 3);
    let highlights = server
        .document_highlight(DocumentHighlightParams {
            text_document_position_params: pos,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        highlights
            .iter()
            .filter(|item| item.kind == Some(DocumentHighlightKind::WRITE))
            .count(),
        2
    );
    assert_eq!(
        highlights
            .iter()
            .filter(|item| item.kind == Some(DocumentHighlightKind::READ))
            .count(),
        1
    );
    server
        .did_change_configuration(DidChangeConfigurationParams {
            settings: serde_json::json!({"dynare":{"nameDetails":{"tex":false}}}),
        })
        .await;
    assert!(!hover(server, at(&uri, text, "y=z"))
        .await
        .unwrap()
        .contains("Z_t"));
}

#[tokio::test]
async fn unopened_include_navigation_and_versioned_rename_use_written_files() {
    let folder = std::env::temp_dir().join(format!(
        "dygnosis-local-lsp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&folder).unwrap();
    let helper = folder.join("helper.inc");
    std::fs::write(&helper, "#z=p*y(-1);\r\n").unwrap();
    let uri = Url::from_file_path(folder.join("root.mod")).unwrap();
    let helper_uri = Url::from_file_path(&helper).unwrap();
    let text =
        "// 😀\r\nvar y; parameters p;\r\nmodel;\r\n@#include \"helper.inc\"\r\ny=z(+1);\r\nend;";
    let (service, _socket) = new_service();
    open(service.inner(), &uri, text).await;
    let pos = at(&uri, text, "y=z");
    let definition = target(service.inner(), pos.clone(), false).await.unwrap();
    assert_eq!(definition.uri, helper_uri);
    assert_eq!(
        definition.range,
        Range::new(Position::new(0, 1), Position::new(0, 2))
    );
    assert_eq!(refs(service.inner(), pos.clone(), true).await.len(), 2);
    let edit = service
        .inner()
        .rename(RenameParams {
            text_document_position: pos,
            new_name: "helper".into(),
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    let Some(DocumentChanges::Edits(edits)) = edit.document_changes else {
        panic!("versioned edits required")
    };
    assert_eq!(edits.len(), 2);
    assert_eq!(
        edits
            .iter()
            .find(|edit| edit.text_document.uri == helper_uri)
            .unwrap()
            .text_document
            .version,
        None
    );
    assert_eq!(
        edits
            .iter()
            .find(|edit| edit.text_document.uri == uri)
            .unwrap()
            .text_document
            .version,
        Some(7)
    );
    std::fs::remove_dir_all(&folder).unwrap();
}

#[tokio::test]
async fn declaration_without_definition_has_no_invented_definition() {
    let (service, _socket) = new_service();
    let uri = Url::parse("file:///tmp/local-declaration.mod").unwrap();
    let text = "var y; model_local_variable z $Z$; model; y=z; end;";
    open(service.inner(), &uri, text).await;
    let pos = at(&uri, text, "y=z");
    let help = hover(service.inner(), pos.clone()).await.unwrap();
    assert!(!help.contains("Expression"));
    assert!(target(service.inner(), pos.clone(), false).await.is_none());
    assert!(target(service.inner(), pos, true).await.is_some());
}

#[tokio::test]
async fn local_model_info_uses_lsp_origins_and_withholds_incomplete_inventory() {
    let (service, _socket) = new_service();
    let uri = Url::parse("file:///tmp/local-info.mod").unwrap();
    let text = "var y; model_local_variable z $Z$; model; #z=1; y=z; end;";
    open(service.inner(), &uri, text).await;
    let params = || ExecuteCommandParams {
        command: "dynare/modelInfo".into(),
        arguments: vec![serde_json::json!({"root_uri":uri})],
        work_done_progress_params: Default::default(),
    };
    let info = service
        .inner()
        .execute_command(params())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        info["model_locals"]["declarations"][0]["tex_name"], "Z",
        "{info}"
    );
    assert_eq!(info["model_locals"]["definitions"][0]["expression"], "1");
    assert_eq!(
        info["model_locals"]["definitions"][0]["origin"]["uri"],
        uri.as_str()
    );
    assert!(info["model_locals"]["definitions"][0]["origin"]["range"].is_object());
    open(service.inner(), &uri, "var y; model; #z=1; #w=").await;
    let info = service
        .inner()
        .execute_command(params())
        .await
        .unwrap()
        .unwrap();
    assert!(info.get("model_locals").is_none());
}

#[tokio::test]
async fn rename_is_bound_versioned_and_collision_checked() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let uri = Url::parse("file:///tmp/local-rename.mod").unwrap();
    let other = Url::parse("file:///tmp/unrelated-local.mod").unwrap();
    let text = "var y; parameters p; model; #z=p; #w=z+1; y=z; end; // z\n";
    open(server, &uri, text).await;
    open(server, &other, "var y; model; #z=1; y=z; end;").await;
    let pos = at(&uri, text, "y=z");
    assert!(server.prepare_rename(pos.clone()).await.unwrap().is_some());
    let edit = server
        .rename(RenameParams {
            text_document_position: pos.clone(),
            new_name: "helper".into(),
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    let Some(DocumentChanges::Edits(edits)) = edit.document_changes else {
        panic!("versioned edits required")
    };
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].text_document.uri, uri);
    assert_eq!(edits[0].text_document.version, Some(7));
    assert_eq!(edits[0].edits.len(), 3);
    for name in ["p", "w", "var", "end", "bad name"] {
        assert!(
            server
                .rename(RenameParams {
                    text_document_position: pos.clone(),
                    new_name: name.into(),
                    work_done_progress_params: Default::default()
                })
                .await
                .unwrap()
                .is_none(),
            "{name}"
        );
    }
}

#[tokio::test]
async fn rename_rejects_duplicate_declarations_and_shared_macro_bindings() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let uri = Url::parse("file:///tmp/local-ambiguous-rename.mod").unwrap();
    let duplicate =
        "var y; model_local_variable z; model_local_variable z $Z$; model; #z=1; y=z; end;";
    open(server, &uri, duplicate).await;
    for marker in ["#z", "y=z", "; model_local_variable z"] {
        assert!(server
            .prepare_rename(at(&uri, duplicate, marker))
            .await
            .unwrap()
            .is_none());
    }
    let shared = "heterogeneity_dimension h k; var(heterogeneity=h) yh; var(heterogeneity=k) yk;\n@#for dim in [\"h\",\"k\"]\nmodel(heterogeneity=@{dim}); #z=1; end;\n@#endfor\nmodel(heterogeneity=h); yh=z; end; model(heterogeneity=k); yk=z; end;";
    open(server, &uri, shared).await;
    assert!(server
        .prepare_rename(at(&uri, shared, "yh=z"))
        .await
        .unwrap()
        .is_none());
    let mixed = "@#for i in 1:2\n@#if i==2\nvar y;\nmodel;\n#z=1;\n@#endif\ny=z;\n@#if i==2\nend;\n@#endif\n@#endfor\n";
    open(server, &uri, mixed).await;
    assert!(server
        .prepare_rename(at(&uri, mixed, "#z"))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn later_explicit_metadata_keeps_one_binding() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let uri = Url::parse("file:///tmp/local-later-metadata.mod").unwrap();
    let text = "var y; model; #z=1; y=z; end; model_local_variable z $Z$;";
    open(server, &uri, text).await;
    for marker in ["#z", "y=z"] {
        let pos = at(&uri, text, marker);
        assert_eq!(refs(server, pos.clone(), true).await.len(), 3);
        let edit = server
            .rename(RenameParams {
                text_document_position: pos,
                new_name: "helper".into(),
                work_done_progress_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();
        let Some(DocumentChanges::Edits(edits)) = edit.document_changes else {
            panic!("versioned edits required")
        };
        assert_eq!(edits.iter().map(|edit| edit.edits.len()).sum::<usize>(), 3);
    }
}

#[tokio::test]
async fn dimension_bindings_are_separate_and_shared_declaration_rename_is_unavailable() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let uri = Url::parse("file:///tmp/local-dimensions.mod").unwrap();
    let text = "heterogeneity_dimension h k;\nvar(heterogeneity=h) a;\nvar(heterogeneity=k) b;\nmodel(heterogeneity=h);\n#z=1; a=z;\nend;\nmodel(heterogeneity=k);\n#z=2; b=z;\nend;";
    open(server, &uri, text).await;
    let pos = at(&uri, text, "b=z");
    let help = hover(server, pos.clone()).await.unwrap();
    assert!(help.contains("`2`"), "{help}");
    assert_eq!(
        target(server, pos.clone(), false)
            .await
            .unwrap()
            .range
            .start
            .line,
        7
    );
    assert_eq!(refs(server, pos.clone(), true).await.len(), 2);
    assert!(server.prepare_rename(pos).await.unwrap().is_some());
    let text = format!("model_local_variable z;\n{text}");
    open(server, &uri, &text).await;
    assert!(server
        .prepare_rename(at(&uri, &text, "b=z"))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn literal_macro_reads_keep_navigation_and_synthesized_names_refuse_rename() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let uri = Url::parse("file:///tmp/local-macro.mod").unwrap();
    let text = "@#define value=2\nvar y; model; #z=@{value}; y=z; end;";
    open(server, &uri, text).await;
    let pos = at(&uri, text, "y=z");
    assert!(hover(server, pos.clone())
        .await
        .unwrap()
        .contains("Expanded expression"));
    assert!(server.prepare_rename(pos).await.unwrap().is_some());
    let text = "@#define name=\"z\"\nvar y; model; #@{name}=1; y=@{name}; end;";
    open(server, &uri, text).await;
    assert!(server
        .prepare_rename(at(&uri, text, "y=@{name"))
        .await
        .unwrap()
        .is_none());
}
