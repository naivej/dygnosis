//! Refused and partial LSP requests must carry a cause, without an edit.

use dygnosis::server::new_service;
use serde_json::json;
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

// Guards malformed Explain requests silently returning null.
#[tokio::test]
async fn explain_requires_a_code_and_keeps_the_argument_cause() {
    let (service, _socket) = new_service();
    let result = service
        .inner()
        .execute_command(ExecuteCommandParams {
            command: "dynare/explainDiagnostic".into(),
            arguments: vec![],
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result["code"], "INVALID_ARGUMENTS");
    assert!(result["error"]
        .as_str()
        .unwrap()
        .contains("diagnostic code"));
}

async fn open(server: &dygnosis::server::Backend, uri: &Url, text: &str) {
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

// Guards collision, missing include, and generated-target refusals losing their cause.
#[tokio::test]
async fn rename_errors_and_capture_refusals_explain_the_same_decision() {
    for (file, text, expected) in [
        ("collision", "var y z; model;y=z;end;", "already declared"),
        (
            "include",
            "var y;\n@#include \"missing.inc\"\nmodel;y=0;end;",
            "include",
        ),
        (
            "generated",
            "var y;model;y=@{\"y\"};end;",
            "literal written target",
        ),
    ] {
        let (service, _socket) = new_service();
        let server = service.inner();
        let uri = Url::parse(&format!("file:///tmp/explain-{file}.mod")).unwrap();
        open(server, &uri, text).await;
        let params = RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position::new(0, 4),
            },
            new_name: "z".into(),
            work_done_progress_params: Default::default(),
        };
        let error = server.rename(params.clone()).await.unwrap_err();
        assert!(error.message.contains(expected), "{file}: {error}");
        let captured = server
            .execute_command(ExecuteCommandParams {
                command: "dynare/captureRename".into(),
                arguments: vec![serde_json::to_value(params).unwrap()],
                work_done_progress_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(captured["code"], "RENAME_REFUSED");
        assert_eq!(captured["error"], error.message.as_ref());
    }
}

// Guards an unavailable document and invalid selection looking like unchanged formatting.
#[tokio::test]
async fn formatting_distinguishes_refusal_from_unchanged_source() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let uri = Url::parse("file:///tmp/explain-format.mod").unwrap();
    let params = DocumentFormattingParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        options: serde_json::from_value(json!({"tabSize":4,"insertSpaces":true})).unwrap(),
        work_done_progress_params: Default::default(),
    };
    assert!(server
        .formatting(params.clone())
        .await
        .unwrap_err()
        .message
        .contains("unavailable"));
    open(server, &uri, "var y;\n").await;
    assert!(server.formatting(params).await.unwrap().is_none());
    let error = server
        .range_formatting(DocumentRangeFormattingParams {
            text_document: TextDocumentIdentifier { uri },
            range: Range::new(Position::new(9, 0), Position::new(10, 1)),
            options: serde_json::from_value(json!({"tabSize":4,"insertSpaces":true})).unwrap(),
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap_err();
    assert!(error.message.contains("outside the document"));
}

// Guards a partial preview and withheld counts having only a status flag.
#[tokio::test]
async fn partial_model_information_and_preview_name_the_missing_include() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let uri = Url::parse("file:///tmp/explain-preview.mod").unwrap();
    open(
        server,
        &uri,
        "var y;\n@#include \"missing.inc\"\nmodel;y=possibly_in_missing_include;end;\n",
    )
    .await;
    for command in ["dynare/modelInfo", "dynare/showEffectiveModel"] {
        let result = server
            .execute_command(ExecuteCommandParams {
                command: command.into(),
                arguments: vec![json!({"root_uri":uri})],
                work_done_progress_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result["status"], "incomplete");
        assert!(
            result["incomplete_reasons"]
                .to_string()
                .contains("missing.inc"),
            "{command}: {result}"
        );
        assert!(!result["message"].as_str().unwrap().is_empty());
        assert!(
            !result["incomplete_reasons"]
                .to_string()
                .contains("PARSE_REFUSED"),
            "a missing declaration include must not produce a downstream Parse claim: {result}"
        );
    }
}
