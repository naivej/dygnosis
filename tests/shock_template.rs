//! Explicit shocks-template code action.

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use dygnosis::server::new_service;
use dygnosis::span::{LineIndex, Position as SpanPos};
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

fn e001_count(text: &str) -> usize {
    dygnosis::check_file(text, "file:///tmp/shock_template.mod")
        .into_iter()
        .filter(|diag| diag.code == "E001")
        .count()
}

fn apply_edit(text: &str, edit: &TextEdit) -> String {
    let index = LineIndex::new(text);
    let start = index.offset_utf16(
        text,
        SpanPos {
            line: edit.range.start.line,
            character: edit.range.start.character,
        },
    );
    let end = index.offset_utf16(
        text,
        SpanPos {
            line: edit.range.end.line,
            character: edit.range.end.character,
        },
    );
    let mut out = text.to_string();
    out.replace_range(start as usize..end as usize, &edit.new_text);
    out
}

async fn template_for(text: &str, only: Option<Vec<CodeActionKind>>) -> Option<CodeAction> {
    let uri = Url::parse("file:///tmp/shock_template.mod").unwrap();
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;
    let actions = service
        .inner()
        .code_action(CodeActionParams {
            text_document: TextDocumentIdentifier { uri },
            range: Range::new(Position::new(0, 0), Position::new(0, 0)),
            context: CodeActionContext {
                diagnostics: vec![],
                only,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("code action");
    actions.and_then(|items| {
        items.into_iter().find_map(|item| match item {
            CodeActionOrCommand::CodeAction(action) if action.title == "Insert shocks template" => {
                Some(action)
            }
            _ => None,
        })
    })
}

fn body_lines(text: &str) -> Vec<&str> {
    text.lines().filter(|line| !line.is_empty()).collect()
}

#[tokio::test]
async fn offers_commented_rows_before_the_command_without_applying_them() {
    let text = "var y;\nvarexo u, e;\nparameters a;\na = 0.9;\nmodel;\ny = a * y(-1) + e + u;\nend;\nstoch_simul;\n";
    let action = template_for(text, None).await.expect("template");
    assert_eq!(action.kind.as_ref(), Some(&CodeActionKind::REFACTOR));
    assert_ne!(action.is_preferred, Some(true));
    assert!(action.command.is_none());
    let edit = action
        .edit
        .as_ref()
        .and_then(|ws| ws.changes.as_ref())
        .and_then(|changes| changes.values().next())
        .and_then(|edits| edits.first())
        .expect("one insert");
    assert_eq!(edit.range.start, edit.range.end);
    assert_eq!(
        body_lines(&edit.new_text),
        vec![
            "// shocks;",
            "// var u;",
            "// stderr ;",
            "// var e;",
            "// stderr ;",
            "// end;",
        ]
    );
    assert!(!edit.new_text.chars().any(|ch| ch.is_ascii_digit()));
    let edited = apply_edit(text, edit);
    let template = edited.find("// shocks;").unwrap();
    let command = edited.find("stoch_simul").unwrap();
    let model_end = edited.find("end;").unwrap();
    assert!(model_end < template && template < command);
    assert!(e001_count(&edited) <= e001_count(text));
}

#[tokio::test]
async fn no_varexo_and_existing_ordinary_shocks_offer_nothing() {
    assert!(template_for("var y;\nparameters a;\n", None)
        .await
        .is_none());
    assert!(
        template_for("varexo e;\nshocks;\nvar e; stderr 0.01;\nend;\n", None)
            .await
            .is_none()
    );
}

#[tokio::test]
async fn heterogeneous_shocks_still_offer_and_varexo_det_is_ignored() {
    let het = "heterogeneity_dimension d;\nvarexo e;\nvarexo(heterogeneity=d) h;\nshocks(heterogeneity=d);\nvar h = 0.1;\nend;\n";
    let action = template_for(het, None).await.expect("still offered");
    let edit = action
        .edit
        .unwrap()
        .changes
        .unwrap()
        .into_values()
        .next()
        .unwrap()
        .remove(0);
    assert!(edit.new_text.contains("// var e;"));
    assert!(!edit.new_text.contains("// var h;"));
    assert!(!edit.new_text.chars().any(|ch| ch.is_ascii_digit()));
    let edited = apply_edit(het, &edit);
    assert!(e001_count(&edited) <= e001_count(het));

    assert!(template_for("varexo_det u;\n", None).await.is_none());
    let mixed = template_for("varexo e;\nvarexo_det u;\n", None)
        .await
        .expect("aggregate name");
    let mixed_edit = mixed
        .edit
        .unwrap()
        .changes
        .unwrap()
        .into_values()
        .next()
        .unwrap()
        .remove(0);
    assert!(mixed_edit.new_text.contains("// var e;"));
    assert!(!mixed_edit.new_text.contains("// var u;"));
}

#[tokio::test]
async fn quickfix_filter_does_not_return_the_template() {
    let text = "varexo e;\n";
    assert!(template_for(text, Some(vec![CodeActionKind::QUICKFIX]))
        .await
        .is_none());
    let action = template_for(text, None)
        .await
        .expect("offered without a filter");
    assert_ne!(action.is_preferred, Some(true));
}

#[tokio::test]
async fn utf16_emoji_inserts_after_the_character() {
    let text = "varexo e; \u{1F600}";
    let action = template_for(text, None).await.expect("template");
    let edit = action
        .edit
        .unwrap()
        .changes
        .unwrap()
        .into_values()
        .next()
        .unwrap()
        .remove(0);
    assert_eq!(edit.range.start.line, 0);
    assert_eq!(
        edit.range.start.character,
        text.encode_utf16().count() as u32
    );
    let edited = apply_edit(text, &edit);
    assert!(edited.starts_with("varexo e; \u{1F600}\n// shocks;"));
    assert!(e001_count(&edited) <= e001_count(text));
}

#[tokio::test]
async fn crlf_inserts_before_the_command() {
    let text = "varexo e;\r\nstoch_simul;\r\n";
    let action = template_for(text, None).await.expect("template");
    let edit = action
        .edit
        .unwrap()
        .changes
        .unwrap()
        .into_values()
        .next()
        .unwrap()
        .remove(0);
    assert_eq!(edit.range.start, Position::new(1, 0));
    let edited = apply_edit(text, &edit);
    assert!(edited.contains("// shocks;\r\n// var e;\r\n// stderr ;\r\n// end;\r\nstoch_simul;"));
    assert!(e001_count(&edited) <= e001_count(text));
}

#[tokio::test]
async fn include_supplies_names_and_an_ordinary_block_hides_the_action() {
    let dir = std::env::temp_dir().join(format!(
        "dygnosis-shock-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let helper = dir.join("helper.inc");
    let parent = dir.join("parent.mod");
    fs::write(&helper, "varexo e;\n").unwrap();
    let parent_src = "@#include \"helper.inc\"\nmodel;\ny = e;\nend;\nstoch_simul;\n";
    fs::write(&parent, parent_src).unwrap();
    let uri = Url::from_file_path(&parent).unwrap();
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version: 1,
                text: parent_src.to_string(),
            },
        })
        .await;
    let offered = service
        .inner()
        .code_action(CodeActionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range: Range::new(Position::new(0, 0), Position::new(0, 0)),
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("code action")
        .expect("actions");
    let action = offered
        .into_iter()
        .find_map(|item| match item {
            CodeActionOrCommand::CodeAction(action) if action.title == "Insert shocks template" => {
                Some(action)
            }
            _ => None,
        })
        .expect("include varexo is eligible");
    let edit = action
        .edit
        .unwrap()
        .changes
        .unwrap()
        .remove(&uri)
        .unwrap()
        .remove(0);
    assert!(edit.new_text.contains("// var e;"));
    assert_ne!(action.is_preferred, Some(true));
    let edited = apply_edit(parent_src, &edit);
    assert!(edited.find("// shocks;").unwrap() < edited.find("stoch_simul").unwrap());

    fs::write(&helper, "varexo e;\nshocks;\nvar e; stderr 0.01;\nend;\n").unwrap();
    let (service, _socket) = new_service();
    let hidden_src = "@#include \"helper.inc\"\nstoch_simul;\n";
    fs::write(&parent, hidden_src).unwrap();
    service
        .inner()
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version: 1,
                text: hidden_src.to_string(),
            },
        })
        .await;
    let hidden = service
        .inner()
        .code_action(CodeActionParams {
            text_document: TextDocumentIdentifier { uri },
            range: Range::new(Position::new(0, 0), Position::new(0, 0)),
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("code action");
    let still = hidden.and_then(|items| {
        items.into_iter().find_map(|item| match item {
            CodeActionOrCommand::CodeAction(action) if action.title == "Insert shocks template" => {
                Some(action)
            }
            _ => None,
        })
    });
    assert!(
        still.is_none(),
        "ordinary shocks in the include hide the action"
    );
    let _ = fs::remove_dir_all(&dir);
}
