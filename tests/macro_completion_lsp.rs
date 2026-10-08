//! Directive completion uses the macro scanner without requiring expansion.

use dygnosis::server::{initialize_result, new_service};
use dygnosis::span::LineIndex;
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

const DIRECTIVES: &[(&str, &str)] = &[
    ("define", "Define a macro variable or function"),
    ("include", "Insert another file"),
    ("includepath", "Add a folder to the include search"),
    ("if", "Keep the following text when the condition is true"),
    ("ifdef", "Keep the following text when the name is defined"),
    (
        "ifndef",
        "Keep the following text when the name is not defined",
    ),
    ("elseif", "Try another condition"),
    ("else", "Take the remaining branch"),
    ("endif", "End an if"),
    ("for", "Repeat the following text for each value"),
    ("endfor", "End a for"),
    ("echo", "Show a macro message"),
    ("error", "Stop expansion and show a message"),
    ("echomacrovars", "Show macro variable values"),
    (
        "line",
        "Mark a source line. Later checks keep the written file.",
    ),
];

async fn complete(marked: &str, extension: &str, trigger: Option<&str>) -> Vec<CompletionItem> {
    let byte = marked.find('|').expect("cursor marker");
    let text = marked.replacen('|', "", 1);
    let cursor = LineIndex::new(&text).position_utf16(&text, byte as u32);
    let uri = Url::parse(&format!("file:///tmp/macro-completion.{extension}")).unwrap();
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version: 1,
                text,
            },
        })
        .await;
    let response = service
        .inner()
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position::new(cursor.line, cursor.character),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: trigger.map(|character| CompletionContext {
                trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER,
                trigger_character: Some(character.into()),
            }),
        })
        .await
        .expect("completion RPC");
    match response {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    }
}

#[tokio::test]
async fn macro_completion_has_the_pinned_names_and_plain_name_edits() {
    let items = complete("var declared_name;\n@#def|", "mod", Some("#")).await;
    assert_eq!(items.len(), DIRECTIVES.len());
    for (order, (item, (name, documentation))) in items.iter().zip(DIRECTIVES).enumerate() {
        assert_eq!(item.label, *name);
        assert_eq!(item.kind, Some(CompletionItemKind::KEYWORD));
        assert_eq!(item.detail.as_deref(), Some("macro directive"));
        assert_eq!(item.filter_text.as_deref(), Some(*name));
        assert_eq!(item.insert_text.as_deref(), Some(*name));
        assert_eq!(item.insert_text_format, Some(InsertTextFormat::PLAIN_TEXT));
        assert_eq!(
            item.sort_text.as_deref(),
            Some(format!("{order:02}").as_str())
        );
        assert_eq!(
            item.documentation,
            Some(Documentation::String((*documentation).into()))
        );
        assert_eq!(
            item.text_edit,
            Some(CompletionTextEdit::Edit(TextEdit {
                range: Range::new(Position::new(1, 2), Position::new(1, 5)),
                new_text: (*name).into(),
            }))
        );
    }
}

#[tokio::test]
async fn macro_completion_reads_scanner_sites_even_when_expansion_fails() {
    for (marked, extension) in [
        ("@#|", "mod"),
        ("  \t@#|", "mod"),
        ("@# |define x=1", "mod"),
        ("@#\t |", "mod"),
        ("@#IF|", "dyn"),
        ("@#d|efine x=1", "mod"),
        ("/*\n@#|\n*/", "mod"),
        ("@#error \"earlier failure\"\n@#|", "mod"),
        ("@#|", "inc"),
        ("// non-ASCII 😀\r\n@#def|", "mod"),
    ] {
        let expected: Vec<_> = DIRECTIVES.iter().map(|(name, _)| *name).collect();
        for trigger in [None, Some("#")] {
            let items = complete(marked, extension, trigger).await;
            let labels: Vec<_> = items.iter().map(|item| item.label.as_str()).collect();
            assert_eq!(labels, expected, "{marked:?}, trigger {trigger:?}");
        }
    }
}

#[tokio::test]
async fn macro_completion_replaces_the_name_from_leading_space_without_losing_space() {
    let items = complete("@# |define x=1", "mod", None).await;
    let include = items.iter().find(|item| item.label == "include").unwrap();
    assert_eq!(include.insert_text.as_deref(), Some("include"));
    assert_eq!(
        include.text_edit,
        Some(CompletionTextEdit::Edit(TextEdit {
            range: Range::new(Position::new(0, 3), Position::new(0, 9)),
            new_text: "include".into(),
        }))
    );
    let items = complete("@#|  define x=1", "mod", None).await;
    let include = items.iter().find(|item| item.label == "include").unwrap();
    assert_eq!(
        include.text_edit,
        Some(CompletionTextEdit::Edit(TextEdit {
            range: Range::new(Position::new(0, 2), Position::new(0, 10)),
            new_text: "  include".into(),
        }))
    );
}

#[tokio::test]
async fn macro_completion_leaves_other_sites_manual_and_hash_trigger_quiet() {
    for marked in [
        "var y;\nmodel;\n#| tmp=y;\nend;",
        "var y;\nbeta = @#|",
        "var y;\n// @#|",
        "var y;\n@#define |x=1",
        "var y;\n@#if |1\n@#endif",
        "var y;\n@{|}",
        "var y;\n@#define x=1+\\\\\n |2",
    ] {
        let manual = complete(marked, "mod", None).await;
        assert!(
            !manual.is_empty(),
            "manual completion missing at {marked:?}"
        );
        assert!(
            manual
                .iter()
                .all(|item| item.detail.as_deref() != Some("macro directive")),
            "directive list at {marked:?}"
        );
        assert!(
            complete(marked, "mod", Some("#")).await.is_empty(),
            "# trigger at {marked:?}"
        );
    }
    let name_items = complete("var y;\nparameters p;\n|", "mod", None).await;
    assert!(name_items.iter().any(|item| item.label == "y"));
    assert!(name_items.iter().any(|item| item.label == "p"));
    for (marked, trigger) in [("stoch_simul(|);", "("), ("stoch_simul(order=1,|);", ",")] {
        let options = complete(marked, "mod", Some(trigger)).await;
        assert!(options.iter().any(|item| item.label == "irf"));
    }
}

#[test]
fn macro_completion_advertises_hash_without_changing_signature_triggers() {
    let capabilities = initialize_result().capabilities;
    assert_eq!(
        capabilities.completion_provider.unwrap().trigger_characters,
        Some(vec!["(".into(), ",".into(), "#".into()])
    );
    assert_eq!(
        capabilities
            .signature_help_provider
            .unwrap()
            .trigger_characters,
        Some(vec!["(".into(), ",".into(), "=".into()])
    );
}
