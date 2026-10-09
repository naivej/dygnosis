use dygnosis::server::{initialize_result, new_service, Backend};
use serde_json::{json, Value};
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

async fn command(server: &Backend, name: &str, input: Value) -> Value {
    server
        .execute_command(ExecuteCommandParams {
            command: name.to_owned(),
            arguments: vec![input],
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap()
}

async fn open(server: &Backend, uri: &Url, text: &str) {
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".to_owned(),
                version: 1,
                text: text.to_owned(),
            },
        })
        .await;
}

async fn change(server: &Backend, uri: &Url, text: &str) {
    server
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: text.to_owned(),
            }],
        })
        .await;
}

fn historical(id: &str, value: u8, load_include: bool) -> Value {
    let mut sources = json!({"root.mod":{"kind":"text","text":"@#include \"part\"\n"}});
    if load_include {
        sources["part"] =
            json!({"kind":"text","text":format!("var y; model; [name='r'] y={value}; end;")});
    }
    json!({"kind":"git","input_id":id,"root_file":"root.mod","repository_uri":"file:///C:/snapshot-lsp",
        "commit":if id == "before" { "a".repeat(40) } else { "b".repeat(40) },
        "manifest":{"root.mod":{"mode":"100644","object_id":"c".repeat(40)},"part":{"mode":"100644","object_id":"d".repeat(40)}},
        "sources":sources})
}

#[test]
fn snapshot_capability_is_additive_and_advertises_bounded_source_negotiation() {
    let capabilities = initialize_result().capabilities;
    assert!(capabilities
        .execute_command_provider
        .unwrap()
        .commands
        .contains(&"dynare/compareModelSnapshots".to_owned()));
    let extensions = capabilities.experimental.unwrap();
    for command in ["compareModels", "compareModelSnapshots"] {
        for field in [
            "semantic_schema_version",
            "source_changes_schema_version",
            "coverage_schema_version",
        ] {
            assert_eq!(
                extensions["dygnosis"][command][field], 1,
                "{command}.{field}"
            );
        }
    }
    assert_eq!(
        extensions["dygnosis"]["compareModels"]["navigation_schema_version"],
        1
    );
    assert_eq!(
        extensions["dygnosis"]["compareModelSnapshots"]["schema_version"],
        1
    );
    assert_eq!(
        extensions["dygnosis"]["compareModelSnapshots"]["navigation_schema_version"],
        2
    );
    assert_eq!(
        extensions["dygnosis"]["compareModelSnapshots"]["needs_sources"],
        true
    );
}

#[tokio::test]
async fn working_revision_remains_pinned_across_source_rounds_and_uses_unsaved_includes() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let root = Url::parse("file:///C:/snapshot-lsp/root.mod").unwrap();
    let part = Url::parse("file:///C:/snapshot-lsp/part").unwrap();
    let unrelated = Url::parse("file:///C:/snapshot-lsp/unrelated.mod").unwrap();
    open(server, &part, "var y; model; [name='r'] y=2; end;").await;
    open(server, &root, "@#include \"part\"\n").await;
    let info = command(server, "dynare/modelInfo", json!({"root_uri":root})).await;
    let mut request = json!({"schema_version":1,"before":historical("before",1,false),
        "after":{"kind":"working","input_id":"after","root_uri":root,"expected_revision":info["revision"]}});
    let pending = command(server, "dynare/compareModelSnapshots", request.clone()).await;
    assert_eq!(pending["state"], "needs_sources");
    assert_eq!(
        pending["requests"],
        json!([{"side":"before","input_id":"before","file_keys":["part"]}])
    );
    assert!(pending.get("diff").is_none());
    open(server, &unrelated, "var z; model; z=7; end;").await;
    request["before"] = historical("before", 1, true);
    let result = command(server, "dynare/compareModelSnapshots", request.clone()).await;
    assert_eq!(result["state"], "result", "{result}");
    assert_eq!(
        result["diff"]["changed_equations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        result["inputs"]["after"]["source_policy"],
        "editor_buffers_and_disk"
    );
    assert_eq!(
        result["inputs"]["after"]["expected_revision"],
        info["revision"]
    );
    let preserved = command(server, "dynare/modelInfo", json!({"root_uri":root})).await;
    assert_eq!(
        preserved["revision"], info["revision"],
        "historical capture must not alter the live workspace"
    );
    change(server, &part, "var y; model; [name='r'] y=3; end;").await;
    let rejected = command(server, "dynare/compareModelSnapshots", request).await;
    assert_eq!(rejected["state"], "failure");
    assert_eq!(rejected["side"], "after");
    assert_eq!(rejected["code"], "INPUT_CHANGED");
    assert!(rejected.get("diff").is_none());
}

#[tokio::test]
async fn two_fixed_inputs_survive_live_edits_and_keep_legacy_file_comparison() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let live = Url::parse("file:///C:/snapshot-fixed/live.mod").unwrap();
    open(server, &live, "var y; model; [name='r'] y=9; end;").await;
    let request = json!({"schema_version":1,"before":historical("before",1,true),"after":historical("after",2,true)});
    let result = command(server, "dynare/compareModelSnapshots", request.clone()).await;
    assert_eq!(result["state"], "result");
    change(server, &live, "var y; model; [name='r'] y=10; end;").await;
    assert_eq!(
        command(server, "dynare/compareModelSnapshots", request).await,
        result
    );
    let legacy = command(
        server,
        "dynare/compareModels",
        json!({"uri_a":live,"uri_b":live}),
    )
    .await;
    assert_eq!(legacy["navigation"]["schema_version"], 1);
    assert_eq!(legacy["changed_equations"], json!([]));
    let invalid = command(
        server,
        "dynare/compareModelSnapshots",
        json!({"schema_version":2}),
    )
    .await;
    assert_eq!(invalid["code"], "INVALID_ARGUMENTS");
}
