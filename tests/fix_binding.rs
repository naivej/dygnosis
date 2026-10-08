//! Automatic fixes stay bound to the requested diagnostic.

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use dygnosis::server::new_service;
use dygnosis::span::LineIndex;
use dygnosis::{auto_fix, check_parse, dynare_auto_fix, parse, ExprId, ExprKind, Model};
use serde_json::json;
use tower_lsp::lsp_types::*;
use tower_lsp::{LanguageServer, LspService};

fn temp_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "dygnosis-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn file_url(path: &std::path::Path) -> Url {
    Url::from_file_path(path).unwrap()
}

async fn open(service: &LspService<dygnosis::server::Backend>, uri: Url, text: &str) {
    service
        .inner()
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id: "dynare".into(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;
}

async fn items(service: &LspService<dygnosis::server::Backend>, uri: &Url) -> Vec<Diagnostic> {
    match service
        .inner()
        .diagnostic(DocumentDiagnosticParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            identifier: None,
            previous_result_id: None,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("pull")
    {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
            full.full_document_diagnostic_report.items
        }
        other => panic!("expected a full report, got {other:?}"),
    }
}

fn code_of(diag: &Diagnostic) -> String {
    match &diag.code {
        Some(NumberOrString::String(code)) => code.clone(),
        Some(NumberOrString::Number(code)) => code.to_string(),
        None => String::new(),
    }
}

fn find_code<'a>(items: &'a [Diagnostic], code: &str) -> &'a Diagnostic {
    items
        .iter()
        .find(|diag| code_of(diag) == code)
        .unwrap_or_else(|| panic!("missing {code}: {items:?}"))
}

async fn actions(
    service: &LspService<dygnosis::server::Backend>,
    uri: &Url,
    range: Range,
    diagnostics: Vec<Diagnostic>,
    only: Option<Vec<CodeActionKind>>,
) -> Vec<CodeAction> {
    service
        .inner()
        .code_action(CodeActionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range,
            context: CodeActionContext {
                diagnostics,
                only,
                ..CodeActionContext::default()
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("code action")
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| match item {
            CodeActionOrCommand::CodeAction(action) => Some(action),
            CodeActionOrCommand::Command(_) => None,
        })
        .collect()
}

fn tags(actions: Vec<CodeAction>) -> Vec<CodeAction> {
    actions
        .into_iter()
        .filter(|action| action.title.starts_with("Add equation tags"))
        .collect()
}

fn long_names(actions: Vec<CodeAction>) -> Vec<CodeAction> {
    actions
        .into_iter()
        .filter(|action| action.title.starts_with("Add long names"))
        .collect()
}

fn semicolons(actions: Vec<CodeAction>) -> Vec<CodeAction> {
    actions
        .into_iter()
        .filter(|action| {
            action_edits(action)
                .iter()
                .any(|(_, _, edit)| edit.new_text.contains(';'))
        })
        .collect()
}

fn action_edits(action: &CodeAction) -> Vec<(Url, Option<i32>, TextEdit)> {
    let Some(DocumentChanges::Edits(rows)) = action
        .edit
        .as_ref()
        .and_then(|edit| edit.document_changes.as_ref())
    else {
        return Vec::new();
    };
    rows.iter()
        .flat_map(|row| {
            row.edits.iter().filter_map(|edit| {
                let OneOf::Left(edit) = edit else {
                    return None;
                };
                Some((
                    row.text_document.uri.clone(),
                    row.text_document.version,
                    edit.clone(),
                ))
            })
        })
        .collect()
}

fn apply_edit(text: &str, edit: &TextEdit) -> String {
    let index = LineIndex::new(text);
    let start = index.offset_utf16(
        text,
        dygnosis::span::Position {
            line: edit.range.start.line,
            character: edit.range.start.character,
        },
    ) as usize;
    let end = index.offset_utf16(
        text,
        dygnosis::span::Position {
            line: edit.range.end.line,
            character: edit.range.end.character,
        },
    ) as usize;
    let mut out = text.to_string();
    out.replace_range(start..end, &edit.new_text);
    out
}

fn root_of(diag: &Diagnostic) -> String {
    diag.data
        .as_ref()
        .and_then(|data| data.get("root"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string()
}

fn stale(diag: &Diagnostic) -> Diagnostic {
    let mut copy = diag.clone();
    copy.data.as_mut().unwrap()["input_revision"] = json!("stale-revision");
    copy
}

fn stripped(diag: &Diagnostic) -> Diagnostic {
    let mut copy = diag.clone();
    copy.data = None;
    copy
}

fn whole(text: &str) -> Range {
    let lines: Vec<&str> = text.split('\n').collect();
    let last = lines.last().copied().unwrap_or("");
    Range::new(
        Position::new(0, 0),
        Position::new(
            lines.len().saturating_sub(1) as u32,
            last.encode_utf16().count() as u32,
        ),
    )
}

#[tokio::test]
async fn selected_i050_does_not_receive_equation_tags() {
    let text = "var y;\nmodel;\ny = 0;\nend;\n";
    let uri = Url::parse("file:///tmp/fix-binding-i050.mod").unwrap();
    let (service, _socket) = new_service();
    open(&service, uri.clone(), text).await;
    let published = items(&service, &uri).await;
    let steady = find_code(&published, "I050").clone();
    let unnamed = find_code(&published, "I208").clone();
    assert!(
        tags(actions(&service, &uri, steady.range, vec![steady.clone()], None).await).is_empty()
    );
    let owned = tags(actions(&service, &uri, unnamed.range, vec![unnamed.clone()], None).await);
    assert_eq!(owned.len(), 1);
    assert_eq!(code_of(&owned[0].diagnostics.as_ref().unwrap()[0]), "I208");
    assert_eq!(owned[0].is_preferred, Some(true));
    let both = tags(
        actions(
            &service,
            &uri,
            unnamed.range,
            vec![steady.clone(), unnamed.clone()],
            None,
        )
        .await,
    );
    assert_eq!(both.len(), 1);
    assert_eq!(both[0].diagnostics.as_ref().unwrap().len(), 1);
    assert_eq!(code_of(&both[0].diagnostics.as_ref().unwrap()[0]), "I208");
    let discovered = tags(actions(&service, &uri, unnamed.range, vec![], None).await);
    assert_eq!(discovered.len(), 1);
    assert_eq!(
        code_of(&discovered[0].diagnostics.as_ref().unwrap()[0]),
        "I208"
    );
    assert!(tags(
        actions(
            &service,
            &uri,
            unnamed.range,
            vec![stripped(&unnamed)],
            None
        )
        .await
    )
    .is_empty());
    assert!(
        tags(actions(&service, &uri, unnamed.range, vec![stale(&unnamed)], None).await).is_empty()
    );
    assert!(tags(
        actions(
            &service,
            &uri,
            unnamed.range,
            vec![unnamed.clone()],
            Some(vec![CodeActionKind::REFACTOR]),
        )
        .await
    )
    .is_empty());
    assert_eq!(
        tags(
            actions(
                &service,
                &uri,
                unnamed.range,
                vec![unnamed],
                Some(vec![CodeActionKind::QUICKFIX]),
            )
            .await
        )
        .len(),
        1
    );
}

#[tokio::test]
async fn i208_edit_applies_only_to_its_block() {
    let text = "var x y;\nmodel;\nx = x(-1);\nend;\nmodel;\ny = x;\nend;\n";
    let uri = Url::parse("file:///tmp/fix-binding-blocks.mod").unwrap();
    let (service, _socket) = new_service();
    open(&service, uri.clone(), text).await;
    let published = items(&service, &uri).await;
    let notes: Vec<_> = published
        .iter()
        .filter(|diag| code_of(diag) == "I208")
        .cloned()
        .collect();
    assert_eq!(notes.len(), 2, "{published:?}");
    for note in &notes {
        let steady = published
            .iter()
            .find(|diag| code_of(diag) == "I050" && diag.range == note.range);
        if let Some(steady) = steady {
            assert!(
                tags(actions(&service, &uri, steady.range, vec![steady.clone()], None).await)
                    .is_empty()
            );
        }
        let owned = tags(actions(&service, &uri, note.range, vec![note.clone()], None).await);
        assert_eq!(owned.len(), 1, "{owned:?}");
        assert_eq!(owned[0].diagnostics.as_ref().unwrap().len(), 1);
        let edits = action_edits(&owned[0]);
        assert_eq!(edits.len(), 1);
        let edited = apply_edit(text, &edits[0].2);
        let mut blocks = edited.split("end;");
        let first = blocks.next().unwrap();
        let second = blocks.next().unwrap();
        assert_ne!(
            first.contains("[name="),
            second.contains("[name="),
            "exactly one block is tagged:\n{edited}"
        );
        assert_eq!(edits[0].1, Some(1));
    }
}

#[tokio::test]
async fn i209_stale_and_stripped_context_returns_nothing() {
    let text = "var y;\nmodel;\ny = 0;\nend;\n";
    let uri = Url::parse("file:///tmp/fix-binding-i209.mod").unwrap();
    let (service, _socket) = new_service();
    open(&service, uri.clone(), text).await;
    let published = items(&service, &uri).await;
    let note = find_code(&published, "I209").clone();
    let owned = long_names(actions(&service, &uri, note.range, vec![note.clone()], None).await);
    assert_eq!(owned.len(), 1);
    assert_eq!(owned[0].is_preferred, Some(true));
    let edit = &action_edits(&owned[0])[0].2;
    let edited = apply_edit(text, edit);
    assert!(edited.contains("long_name='y'"), "{edited}");
    assert!(!edited.contains("long_name='long_name'"));
    assert!(
        long_names(actions(&service, &uri, note.range, vec![stale(&note)], None).await).is_empty()
    );
    assert!(
        long_names(actions(&service, &uri, note.range, vec![stripped(&note)], None).await)
            .is_empty()
    );
    let mut other = note.clone();
    other.code = Some(NumberOrString::String("I050".into()));
    assert!(long_names(actions(&service, &uri, note.range, vec![other], None).await).is_empty());
}

#[tokio::test]
async fn two_roots_do_not_share_one_tag_action() {
    let dir = temp_dir("shared-tags");
    let a = dir.join("a.mod");
    let b = dir.join("b.mod");
    let child = dir.join("shared.inc");
    let a_text = "var y, a;\n@#include \"shared.inc\"\na = 0;\nend;\n";
    let b_text = "var y, b;\n@#include \"shared.inc\"\nb = 0;\nend;\n";
    let child_text = "model;\ny = 0;\n";
    fs::write(&a, a_text).unwrap();
    fs::write(&b, b_text).unwrap();
    fs::write(&child, child_text).unwrap();
    let a_uri = file_url(&a);
    let b_uri = file_url(&b);
    let child_uri = file_url(&child);
    let (service, _socket) = new_service();
    open(&service, child_uri.clone(), child_text).await;
    open(&service, a_uri.clone(), a_text).await;
    open(&service, b_uri.clone(), b_text).await;
    let published = items(&service, &child_uri).await;
    let notes: Vec<_> = published
        .iter()
        .filter(|diag| code_of(diag) == "I208")
        .cloned()
        .collect();
    assert_eq!(notes.len(), 2, "{published:?}");
    let steady = published.iter().find(|diag| code_of(diag) == "I050");
    if let Some(steady) = steady {
        assert!(tags(
            actions(
                &service,
                &child_uri,
                steady.range,
                vec![steady.clone()],
                None
            )
            .await
        )
        .is_empty());
    }
    for note in &notes {
        let owned = tags(actions(&service, &child_uri, note.range, vec![note.clone()], None).await);
        assert_eq!(owned.len(), 1);
        let attached = owned[0].diagnostics.as_ref().unwrap();
        assert_eq!(attached.len(), 1);
        assert_eq!(root_of(&attached[0]), root_of(note));
        let other = if root_of(note) == a_uri.as_str() {
            &b_uri
        } else {
            &a_uri
        };
        assert!(action_edits(&owned[0])
            .iter()
            .all(|(uri, _, _)| uri != other));
    }
    let discovered = tags(actions(&service, &child_uri, notes[0].range, vec![], None).await);
    assert_eq!(discovered.len(), 2);
    assert!(discovered
        .iter()
        .all(|action| action.diagnostics.as_ref().unwrap().len() == 1));
    let roots: Vec<_> = discovered
        .iter()
        .map(|action| root_of(&action.diagnostics.as_ref().unwrap()[0]))
        .collect();
    assert_ne!(roots[0], roots[1]);
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn semicolon_fix_follows_its_owner_and_applies() {
    let text = "parameters beta\n";
    let uri = Url::parse("file:///tmp/fix-binding-semi.mod").unwrap();
    let (service, _socket) = new_service();
    open(&service, uri.clone(), text).await;
    let published = items(&service, &uri).await;
    let error = find_code(&published, "E001").clone();
    let owned = semicolons(actions(&service, &uri, error.range, vec![error.clone()], None).await);
    assert_eq!(owned.len(), 1, "{owned:?}");
    assert_eq!(owned[0].is_preferred, Some(true));
    assert_eq!(owned[0].diagnostics.as_ref().unwrap().len(), 1);
    assert_eq!(code_of(&owned[0].diagnostics.as_ref().unwrap()[0]), "E001");
    let edit = &action_edits(&owned[0])[0];
    assert_eq!(edit.1, Some(1));
    let edited = apply_edit(text, &edit.2);
    assert!(edited.contains("parameters beta;"), "{edited}");
    assert_eq!(auto_fix(text), dynare_auto_fix(text));
    assert!(
        auto_fix(text).contains("parameters beta;"),
        "{}",
        auto_fix(text)
    );
    assert!(
        semicolons(actions(&service, &uri, error.range, vec![stale(&error)], None).await)
            .is_empty()
    );
    assert!(
        semicolons(actions(&service, &uri, error.range, vec![stripped(&error)], None).await)
            .is_empty()
    );
    let mut other = error.clone();
    other.code = Some(NumberOrString::String("I209".into()));
    assert!(semicolons(actions(&service, &uri, error.range, vec![other], None).await).is_empty());
    assert!(semicolons(
        actions(
            &service,
            &uri,
            error.range,
            vec![error.clone()],
            Some(vec![CodeActionKind::REFACTOR]),
        )
        .await
    )
    .is_empty());
    let false_semi = "var y;\nmodel;\n[name='law']\n@#if 0\ny=0;\n@#else\ny=0;\n@#endif\nend;\n";
    assert_eq!(auto_fix(false_semi), false_semi);
    let false_uri = Url::parse("file:///tmp/fix-binding-false-semi.mod").unwrap();
    open(&service, false_uri.clone(), false_semi).await;
    let false_items = items(&service, &false_uri).await;
    assert!(false_items.iter().all(|diag| code_of(diag) != "E001"));
    assert!(
        semicolons(actions(&service, &false_uri, whole(false_semi), vec![], None).await).is_empty()
    );
}

#[tokio::test]
async fn two_roots_do_not_merge_equal_semicolon_edits() {
    let dir = temp_dir("shared-semi");
    let a = dir.join("a.mod");
    let b = dir.join("b.mod");
    let child = dir.join("broken.inc");
    let child_text = "parameters beta\n";
    fs::write(&a, "@#include \"broken.inc\"\n").unwrap();
    fs::write(&b, "@#include \"broken.inc\"\n").unwrap();
    fs::write(&child, child_text).unwrap();
    let a_uri = file_url(&a);
    let b_uri = file_url(&b);
    let child_uri = file_url(&child);
    let (service, _socket) = new_service();
    open(&service, child_uri.clone(), child_text).await;
    open(&service, a_uri.clone(), "@#include \"broken.inc\"\n").await;
    open(&service, b_uri, "@#include \"broken.inc\"\n").await;
    let published = items(&service, &child_uri).await;
    let errors: Vec<_> = published
        .iter()
        .filter(|diag| code_of(diag) == "E001")
        .cloned()
        .collect();
    assert!(!errors.is_empty(), "{published:?}");
    let discovered =
        semicolons(actions(&service, &child_uri, whole(child_text), vec![], None).await);
    assert!(
        discovered.len() >= 2,
        "each root keeps its own edit: {discovered:?}"
    );
    assert!(discovered
        .iter()
        .all(|action| action.diagnostics.as_ref().unwrap().len() == 1));
    let roots: std::collections::BTreeSet<_> = discovered
        .iter()
        .map(|action| root_of(&action.diagnostics.as_ref().unwrap()[0]))
        .collect();
    assert!(roots.len() >= 2, "{roots:?}");
    for action in &discovered {
        let owner = action.diagnostics.as_ref().unwrap()[0].clone();
        let selected =
            semicolons(actions(&service, &child_uri, owner.range, vec![owner.clone()], None).await);
        assert_eq!(selected.len(), 1);
        assert_eq!(
            root_of(&selected[0].diagnostics.as_ref().unwrap()[0]),
            root_of(&owner)
        );
        let edited = apply_edit(child_text, &action_edits(&selected[0])[0].2);
        assert_eq!(
            edited.matches(';').count(),
            child_text.matches(';').count() + 1,
            "{edited}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn refactor_template_carries_the_open_version_and_stays_separate() {
    let text = "varexo e;\n";
    let uri = Url::parse("file:///tmp/fix-binding-template.mod").unwrap();
    let (service, _socket) = new_service();
    open(&service, uri.clone(), text).await;
    let refactor = actions(
        &service,
        &uri,
        whole(text),
        vec![],
        Some(vec![CodeActionKind::REFACTOR]),
    )
    .await;
    let template = refactor
        .iter()
        .find(|action| action.title == "Insert stochastic shocks template")
        .expect("template");
    assert_eq!(template.kind.as_ref(), Some(&CodeActionKind::REFACTOR));
    assert_ne!(template.is_preferred, Some(true));
    let edits = action_edits(template);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].1, Some(1));
    assert!(refactor
        .iter()
        .all(|action| action.kind.as_ref() == Some(&CodeActionKind::REFACTOR)));
    let quick = actions(
        &service,
        &uri,
        whole(text),
        vec![],
        Some(vec![CodeActionKind::QUICKFIX]),
    )
    .await;
    assert!(quick
        .iter()
        .all(|action| action.title != "Insert stochastic shocks template"));
}

#[tokio::test]
async fn same_range_e001_rows_keep_their_own_fixes() {
    let text = "var y z;\nmodel;\ny=0\nz=1+(2;\nend;\n";
    let uri = Url::parse("file:///tmp/fix-binding-two-e001.mod").unwrap();
    let (service, _socket) = new_service();
    open(&service, uri.clone(), text).await;
    let published = items(&service, &uri).await;
    let rows: Vec<_> = published
        .iter()
        .filter(|diag| code_of(diag) == "E001")
        .cloned()
        .collect();
    let merged = rows
        .iter()
        .find(|diag| diag.message.contains("merged due to a missing semicolon"))
        .unwrap_or_else(|| panic!("{rows:?}"))
        .clone();
    let paren = rows
        .iter()
        .find(|diag| diag.message.contains("Unbalanced parentheses"))
        .unwrap_or_else(|| panic!("{rows:?}"))
        .clone();
    assert_eq!(merged.range, paren.range, "{rows:?}");
    assert_eq!(merged.source, paren.source);
    assert_eq!(merged.severity, paren.severity);
    assert_eq!(
        merged.data.as_ref().unwrap()["root"],
        paren.data.as_ref().unwrap()["root"]
    );
    assert_eq!(
        merged.data.as_ref().unwrap()["input_revision"],
        paren.data.as_ref().unwrap()["input_revision"]
    );
    assert!(merged.message.contains("Fix:"), "{}", merged.message);

    let owned = semicolons(actions(&service, &uri, merged.range, vec![merged.clone()], None).await);
    assert_eq!(owned.len(), 1, "{owned:?}");
    assert_eq!(owned[0].is_preferred, Some(true));
    assert_eq!(
        owned[0].diagnostics.as_ref().unwrap()[0].message,
        merged.message
    );
    let edited = apply_edit(text, &action_edits(&owned[0])[0].2);
    assert_eq!(edited, "var y z;\nmodel;\ny=0;\nz=1+(2;\nend;\n");
    let after = check_parse(&parse(&edited));
    assert!(
        after
            .iter()
            .all(|diag| !diag.message.contains("merged due to a missing semicolon")),
        "{after:?}"
    );
    assert!(
        after
            .iter()
            .any(|diag| diag.message.contains("Unbalanced parentheses")),
        "the second equation keeps its unmatched '(': {after:?}"
    );
    let repaired = parse(&edited);
    let first = repaired
        .equations
        .iter()
        .find(|equation| equation.lhs == "y")
        .expect("repaired first equation");
    assert!(!expr_has_error(&repaired, first.lhs_expr.expect("lhs")));
    assert!(!expr_has_error(&repaired, first.rhs_expr.expect("rhs")));
    assert_eq!(first.rhs, "0");

    assert!(
        semicolons(actions(&service, &uri, paren.range, vec![paren.clone()], None).await)
            .is_empty(),
        "unmatched '(' borrowed the merged-equation edit"
    );
    let mut wrong_message = merged.clone();
    wrong_message.message = paren.message.clone();
    assert!(
        semicolons(actions(&service, &uri, merged.range, vec![wrong_message], None).await)
            .is_empty()
    );
    let mut other_source = merged.clone();
    other_source.source = Some("other".into());
    assert!(
        semicolons(actions(&service, &uri, merged.range, vec![other_source], None).await)
            .is_empty()
    );
    let mut other_severity = merged.clone();
    other_severity.severity = Some(DiagnosticSeverity::WARNING);
    assert!(
        semicolons(actions(&service, &uri, merged.range, vec![other_severity], None).await)
            .is_empty()
    );
    assert!(
        semicolons(actions(&service, &uri, merged.range, vec![stale(&merged)], None).await)
            .is_empty()
    );
    assert!(
        semicolons(actions(&service, &uri, merged.range, vec![stripped(&merged)], None).await)
            .is_empty()
    );
    let discovered = semicolons(actions(&service, &uri, merged.range, vec![], None).await);
    assert_eq!(discovered.len(), 1, "{discovered:?}");
    assert_eq!(
        discovered[0].diagnostics.as_ref().unwrap()[0].message,
        merged.message
    );

    let open_delim = "var y z;\nmodel;\ny=(0\nz=0;\nend;\n";
    let open_uri = Url::parse("file:///tmp/fix-binding-open-delim.mod").unwrap();
    open(&service, open_uri.clone(), open_delim).await;
    let open_published = items(&service, &open_uri).await;
    let open_merged = open_published
        .iter()
        .find(|diag| {
            code_of(diag) == "E001" && diag.message.contains("merged due to a missing semicolon")
        })
        .unwrap_or_else(|| panic!("{open_published:?}"))
        .clone();
    assert!(
        !open_merged.message.contains("Fix:"),
        "{}",
        open_merged.message
    );
    assert!(semicolons(
        actions(
            &service,
            &open_uri,
            open_merged.range,
            vec![open_merged.clone()],
            None,
        )
        .await
    )
    .is_empty());
    assert!(
        semicolons(actions(&service, &open_uri, open_merged.range, vec![], None).await).is_empty()
    );
    assert_eq!(auto_fix(open_delim), open_delim);
}

fn expr_has_error(model: &Model, id: ExprId) -> bool {
    match &model.exprs.get(id).kind {
        ExprKind::Error => true,
        ExprKind::Unary { arg, .. } => expr_has_error(model, *arg),
        ExprKind::Binary { lhs, rhs, .. } => {
            expr_has_error(model, *lhs) || expr_has_error(model, *rhs)
        }
        ExprKind::Call { args, .. } => args.iter().any(|arg| expr_has_error(model, *arg)),
        ExprKind::SteadyState { arg } | ExprKind::Expectation { arg, .. } => {
            expr_has_error(model, *arg)
        }
        ExprKind::Ident { .. }
        | ExprKind::Number
        | ExprKind::String
        | ExprKind::PathNamespace { .. } => false,
    }
}

fn equations_are_complete(text: &str) {
    let model = parse(text);
    assert!(!model.equations.is_empty(), "{text}");
    for equation in &model.equations {
        assert!(!expr_has_error(&model, equation.lhs_expr.expect("lhs")));
        assert!(!expr_has_error(&model, equation.rhs_expr.expect("rhs")));
    }
}

#[tokio::test]
async fn multi_token_interpolation_has_no_unsafe_separator_action() {
    let text = "@#define gen = \"0 z\"\nvar y z;\nmodel;\ny = @{gen} = 0;\nend;\n";
    let uri = Url::parse("file:///tmp/fix-binding-gen-span.mod").unwrap();
    let (service, _socket) = new_service();
    open(&service, uri.clone(), text).await;
    let published = items(&service, &uri).await;
    let merged = published
        .iter()
        .find(|diag| {
            code_of(diag) == "E001" && diag.message.contains("merged due to a missing semicolon")
        })
        .unwrap_or_else(|| panic!("{published:?}"))
        .clone();
    let owned = actions(&service, &uri, merged.range, vec![merged.clone()], None).await;
    assert!(
        owned.iter().all(|action| {
            action_edits(action)
                .iter()
                .all(|(_, _, edit)| !edit.new_text.contains(';'))
        }),
        "published E001 borrowed a separator inside @{{gen}}: {owned:?}"
    );
    assert!(
        semicolons(actions(&service, &uri, merged.range, vec![], None).await).is_empty(),
        "empty context discovered the unsafe separator"
    );
    assert_eq!(auto_fix(text), text);

    let single = "@#define n = 1\nvar y z;\nmodel;\ny=@{n}\nz=0;\nend;\n";
    let single_uri = Url::parse("file:///tmp/fix-binding-gen-one.mod").unwrap();
    open(&service, single_uri.clone(), single).await;
    let single_published = items(&service, &single_uri).await;
    let single_merged = single_published
        .iter()
        .find(|diag| {
            code_of(diag) == "E001" && diag.message.contains("merged due to a missing semicolon")
        })
        .unwrap_or_else(|| panic!("{single_published:?}"))
        .clone();
    let single_owned = semicolons(
        actions(
            &service,
            &single_uri,
            single_merged.range,
            vec![single_merged.clone()],
            None,
        )
        .await,
    );
    assert_eq!(single_owned.len(), 1, "{single_owned:?}");
    assert_eq!(single_owned[0].is_preferred, Some(true));
    assert_eq!(
        single_owned[0].diagnostics.as_ref().unwrap()[0].message,
        single_merged.message
    );
    let edited = apply_edit(single, &action_edits(&single_owned[0])[0].2);
    assert_eq!(
        edited,
        "@#define n = 1\nvar y z;\nmodel;\ny=@{n};\nz=0;\nend;\n"
    );
    equations_are_complete(&edited);
    assert_eq!(auto_fix(single), single);

    let invalid = [
        "@#define gen = \"z = 0\"\nvar y z;\nmodel;\ny = 1 + @{gen};\nend;\n",
        "@#define gen = \"1 +\"\nvar y z;\nmodel;\ny = @{gen}\nz = 0;\nend;\n",
        "@#define gen = \"alpha = 2\"\nparameters beta alpha;\nbeta = 1 + @{gen};\n",
    ];
    for (index, text) in invalid.iter().enumerate() {
        let uri = Url::parse(&format!("file:///tmp/fix-binding-invalid-{index}.mod")).unwrap();
        open(&service, uri.clone(), text).await;
        let published = items(&service, &uri).await;
        let owned_row = published
            .iter()
            .find(|diag| {
                code_of(diag) == "E001"
                    && (diag.message.contains("merged due to a missing semicolon")
                        || diag.message.contains("missing its terminating semicolon"))
            })
            .unwrap_or_else(|| panic!("{published:?}"))
            .clone();
        let owned = actions(&service, &uri, owned_row.range, vec![owned_row], None).await;
        assert!(
            owned.iter().all(|action| {
                action_edits(action)
                    .iter()
                    .all(|(_, _, edit)| !edit.new_text.contains(';'))
            }),
            "unsafe separator offered for {text}: {owned:?}"
        );
        assert!(
            semicolons(actions(&service, &uri, whole(text), vec![], None).await).is_empty(),
            "empty context offered a separator for {text}"
        );
        assert_eq!(auto_fix(text), *text);
    }

    let value_then_name = "@#define gen = \"z = 0\"\nvar y z;\nmodel;\ny=1 @{gen};\nend;\n";
    let value_uri = Url::parse("file:///tmp/fix-binding-gen-value.mod").unwrap();
    open(&service, value_uri.clone(), value_then_name).await;
    let value_published = items(&service, &value_uri).await;
    let value_row = value_published
        .iter()
        .find(|diag| {
            code_of(diag) == "E001" && diag.message.contains("merged due to a missing semicolon")
        })
        .unwrap_or_else(|| panic!("{value_published:?}"))
        .clone();
    let value_actions = semicolons(
        actions(
            &service,
            &value_uri,
            value_row.range,
            vec![value_row.clone()],
            None,
        )
        .await,
    );
    assert_eq!(value_actions.len(), 1, "{value_actions:?}");
    assert_eq!(value_actions[0].is_preferred, Some(true));
    assert_eq!(
        value_actions[0].diagnostics.as_ref().unwrap()[0].message,
        value_row.message
    );
    let value_edited = apply_edit(value_then_name, &action_edits(&value_actions[0])[0].2);
    assert_eq!(
        value_edited,
        "@#define gen = \"z = 0\"\nvar y z;\nmodel;\ny=1 ;\n@{gen};\nend;\n"
    );
    equations_are_complete(&value_edited);

    let closed_interp = "@#define gen = \"1 + 0\"\nvar y z;\nmodel;\ny=@{gen}\nz=0;\nend;\n";
    let closed_uri = Url::parse("file:///tmp/fix-binding-gen-closed.mod").unwrap();
    open(&service, closed_uri.clone(), closed_interp).await;
    let closed_published = items(&service, &closed_uri).await;
    let closed_row = closed_published
        .iter()
        .find(|diag| {
            code_of(diag) == "E001" && diag.message.contains("merged due to a missing semicolon")
        })
        .unwrap_or_else(|| panic!("{closed_published:?}"))
        .clone();
    let closed_actions = semicolons(
        actions(
            &service,
            &closed_uri,
            closed_row.range,
            vec![closed_row.clone()],
            None,
        )
        .await,
    );
    assert_eq!(closed_actions.len(), 1, "{closed_actions:?}");
    let closed_edited = apply_edit(closed_interp, &action_edits(&closed_actions[0])[0].2);
    assert_eq!(
        closed_edited,
        "@#define gen = \"1 + 0\"\nvar y z;\nmodel;\ny=@{gen};\nz=0;\nend;\n"
    );
    equations_are_complete(&closed_edited);
}

#[tokio::test]
async fn grouping_parentheses_do_not_offer_a_call_separator() {
    let (service, _socket) = new_service();
    let invalid = [
        "var y z;\nmodel;\ny=() z=0;\nend;\n",
        "var y z;\nmodel;\ny=(1,2) z=0;\nend;\n",
        "@#define gen = \"()\"\nvar y z;\nmodel;\ny=@{gen} z=0;\nend;\n",
        "@#define gen = \"(1,2)\"\nvar y z;\nmodel;\ny=@{gen} z=0;\nend;\n",
    ];
    for (index, text) in invalid.iter().enumerate() {
        let uri = Url::parse(&format!("file:///tmp/fix-binding-group-{index}.mod")).unwrap();
        open(&service, uri.clone(), text).await;
        let published = items(&service, &uri).await;
        let owned_row = published
            .iter()
            .find(|diag| {
                code_of(diag) == "E001"
                    && diag.message.contains("merged due to a missing semicolon")
            })
            .unwrap_or_else(|| panic!("{text}\n{published:?}"))
            .clone();
        assert!(
            !owned_row.message.contains("Fix:"),
            "{text}\n{}",
            owned_row.message
        );
        let owned = actions(&service, &uri, owned_row.range, vec![owned_row], None).await;
        assert!(
            owned.iter().all(|action| {
                action_edits(action)
                    .iter()
                    .all(|(_, _, edit)| !edit.new_text.contains(';'))
            }),
            "unsafe grouping separator for {text}: {owned:?}"
        );
        assert!(
            semicolons(actions(&service, &uri, whole(text), vec![], None).await).is_empty(),
            "empty context offered a grouping separator for {text}"
        );
        assert_eq!(auto_fix(text), *text);
    }

    let scalar = "var y z;\nmodel;\ny=(1) z=0;\nend;\n";
    let scalar_uri = Url::parse("file:///tmp/fix-binding-group-scalar.mod").unwrap();
    open(&service, scalar_uri.clone(), scalar).await;
    let scalar_row = items(&service, &scalar_uri)
        .await
        .into_iter()
        .find(|diag| {
            code_of(diag) == "E001" && diag.message.contains("merged due to a missing semicolon")
        })
        .expect("scalar group");
    let scalar_actions = semicolons(
        actions(
            &service,
            &scalar_uri,
            scalar_row.range,
            vec![scalar_row.clone()],
            None,
        )
        .await,
    );
    assert_eq!(scalar_actions.len(), 1, "{scalar_actions:?}");
    assert_eq!(scalar_actions[0].is_preferred, Some(true));
    assert_eq!(
        scalar_actions[0].diagnostics.as_ref().unwrap()[0].message,
        scalar_row.message
    );
    let scalar_edited = apply_edit(scalar, &action_edits(&scalar_actions[0])[0].2);
    assert_eq!(scalar_edited, "var y z;\nmodel;\ny=(1) ;\nz=0;\nend;\n");
    equations_are_complete(&scalar_edited);

    let call = "var y z;\nmodel;\ny=max(1,2) z=0;\nend;\n";
    let call_uri = Url::parse("file:///tmp/fix-binding-group-max.mod").unwrap();
    open(&service, call_uri.clone(), call).await;
    let call_row = items(&service, &call_uri)
        .await
        .into_iter()
        .find(|diag| {
            code_of(diag) == "E001" && diag.message.contains("merged due to a missing semicolon")
        })
        .expect("max call");
    let call_actions =
        semicolons(actions(&service, &call_uri, call_row.range, vec![call_row], None).await);
    assert_eq!(call_actions.len(), 1, "{call_actions:?}");
    let call_edited = apply_edit(call, &action_edits(&call_actions[0])[0].2);
    assert_eq!(call_edited, "var y z;\nmodel;\ny=max(1,2) ;\nz=0;\nend;\n");
    equations_are_complete(&call_edited);
}

#[tokio::test]
async fn retired_i210_has_no_diagnostic_or_action() {
    let text = "var y;\nparameters a;\nmodel;\n[name='power'] y = -1 + 0 + a*y(-1)^2;\nend;\n";
    let uri = Url::parse("file:///tmp/fix-binding-i210.mod").unwrap();
    let (service, _socket) = new_service();
    open(&service, uri.clone(), text).await;
    let published = items(&service, &uri).await;
    assert!(published.iter().all(|diag| code_of(diag) != "I210"));
    let offered = actions(&service, &uri, whole(text), vec![], None).await;
    assert!(offered.iter().all(|action| {
        action
            .diagnostics
            .as_ref()
            .is_none_or(|diags| diags.iter().all(|diag| code_of(diag) != "I210"))
    }));
}
