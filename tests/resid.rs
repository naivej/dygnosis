//! resid grammar and written-source recovery at the Dynare 7.2 Parse stage.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use dygnosis::server::new_service;
use dygnosis::{
    analyze, apply_fix, auto_fix, dynare_diagnose, parse, run_preprocessor, JsonStage, Severity,
};
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

const MODEL: &str = "var y; varexo e; model; y=e; end;\n";

// The refusing token and sentence come from pinned DynareBison.yy:1080–1086.
const REFUSALS: &[(&str, &str, &str)] = &[
    ("resid(1);", "1", "INT_NUMBER, expecting NON_ZERO"),
    ("resid(1.5);", "1.5", "FLOAT_NUMBER, expecting NON_ZERO"),
    ("resid();", ")", "')', expecting NON_ZERO"),
    ("resid(foo);", "foo", "IDENTIFIER, expecting NON_ZERO"),
    ("resid(noprint);", "noprint", "NOPRINT, expecting NON_ZERO"),
    ("resid(steady);", "steady", "IDENTIFIER, expecting NON_ZERO"),
    ("resid(end);", "end", "IDENTIFIER, expecting NON_ZERO"),
    (
        "resid('foo');",
        "'foo'",
        "QUOTED_STRING, expecting NON_ZERO",
    ),
    ("resid(-1);", "-", "MINUS, expecting NON_ZERO"),
    ("resid(non_zero=1);", "=", "EQUAL, expecting ')'"),
    ("resid(non_zero,non_zero);", ",", "COMMA, expecting ')'"),
    (
        "resid(non_zero non_zero);",
        "non_zero",
        "NON_ZERO, expecting ')'",
    ),
    ("resid(non_zero;", ";", "';', expecting ')'"),
    ("resid(non_zero", "", "end of file, expecting ')'"),
    ("resid(", "", "end of file, expecting NON_ZERO"),
    ("resid(non_zero)", "", "end of file, expecting ';'"),
    ("resid", "", "end of file, expecting ';' or '('"),
    ("resid foo;", "foo", "IDENTIFIER, expecting ';' or '('"),
    ("resid = 1;", "=", "EQUAL, expecting ';' or '('"),
    (
        "resid(non_zero) steady;",
        "steady",
        "IDENTIFIER, expecting ';'",
    ),
    ("resid(non_zero)\nvar z;", "var", "VAR, expecting ';'"),
];

#[test]
fn resid_accepts_only_the_two_pinned_productions() {
    for command in [
        "resid;",
        "resid(non_zero);",
        "resid(/* comment */ non_zero);",
        "resid; resid(non_zero); steady; check;",
        "RESID(NON_ZERO);",
    ] {
        let text = format!("{MODEL}{command}");
        let model = parse(&text);
        assert!(
            analyze(&model)
                .iter()
                .all(|row| row.severity != Severity::Error),
            "{command}: {:?}",
            analyze(&model)
        );
        assert!(model.statements.iter().any(|row| row.name == "resid"));
        assert_eq!(model.equations.len(), 1);
    }
}

#[test]
fn resid_refusals_keep_exact_text_token_ranges_and_earlier_actions() {
    for &(command, token, sentence) in REFUSALS {
        let text = format!("{MODEL}{command}");
        let model = parse(&text);
        let diagnostics = analyze(&model);
        assert_eq!(diagnostics.len(), 1, "{command}: {diagnostics:?}");
        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic.code, "E001", "{command}: {diagnostics:?}");
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(
            diagnostic.message,
            format!("syntax error, unexpected {sentence}")
        );
        assert_eq!(
            &text[diagnostic.span.start as usize..diagnostic.span.end as usize],
            token,
            "{command}: {diagnostic:?}"
        );
        assert!(!model.statements.iter().any(|row| row.name == "resid"));
        assert!(
            model.option_twice.is_empty(),
            "{command}: {:?}",
            model.option_twice
        );
        assert_eq!(model.equations.len(), 1);
        assert_eq!(
            model.endogenous.len(),
            1 + usize::from(command.contains("var z"))
        );
        if token.is_empty() {
            assert_eq!(diagnostic.span.start as usize, text.len());
        }
    }
}

#[test]
fn resid_recovers_a_following_statement_without_generic_option_uses() {
    for command in [
        "resid(1); var z;",
        "resid(non_zero)\nvar z;",
        "resid(non_zero\nvar z;",
    ] {
        let text = format!("{MODEL}{command}");
        let model = parse(&text);
        assert!(model
            .endogenous
            .iter()
            .any(|decl| model.name(decl.name) == "z"));
        assert!(analyze(&model).iter().all(|row| row.code == "E001"));
        assert!(!model.statements.iter().any(|row| row.name == "resid"));
    }
}

#[test]
fn resid_earlier_grammar_refusal_precedes_later_lexer_junk() {
    for (command, expected) in [
        (
            "resid(1 {);",
            "syntax error, unexpected INT_NUMBER, expecting NON_ZERO",
        ),
        ("resid({1);", "character unrecognized by lexer"),
        ("resid(non_zero {);", "character unrecognized by lexer"),
    ] {
        let text = format!("{MODEL}{command}");
        let diagnostics = analyze(&parse(&text));
        assert_eq!(diagnostics.len(), 1, "{command}: {diagnostics:?}");
        assert_eq!(diagnostics[0].message, expected);
        assert!(!parse(&text)
            .statements
            .iter()
            .any(|row| row.name == "resid"));
    }
}

#[test]
fn resid_grammar_uses_active_macro_tokens_and_keeps_native_lines() {
    let quiet = [
        "@#if 0\nresid(1);\n@#else\nresid(non_zero);\n@#endif\n",
        "@#define command = \"resid(non_zero);\"\n@{command}\n",
        "@#for i in 1:2\nresid(non_zero);\n@#endfor\n",
        "disp(1); resid(1);\n",
        "native = resid(1);\n",
        "verbatim;\nresid(1);\nend;\n",
    ];
    for command in quiet {
        let text = format!("{MODEL}{command}");
        let diagnostics = analyze(&parse(&text));
        assert!(
            diagnostics.iter().all(|row| row.code != "E001"),
            "{command}: {diagnostics:?}"
        );
    }
    for command in [
        "@#if 1\nresid(1);\n@#else\nresid(non_zero);\n@#endif\n",
        "@#define argument = 1\nresid(@{argument});\n",
        "@#define command = \"resid(1);\"\n@{command}\n",
        "@#for i in 1:2\nresid(1);\n@#endfor\n",
    ] {
        let text = format!("{MODEL}{command}");
        let diagnostics = analyze(&parse(&text));
        assert!(!diagnostics.is_empty(), "{command}");
        assert!(
            diagnostics.iter().all(|row| {
                row.code == "E001"
                    && row.message == "syntax error, unexpected INT_NUMBER, expecting NON_ZERO"
                    && row.fix.is_none()
            }),
            "{command}: {diagnostics:?}"
        );
    }
}

#[test]
fn resid_semicolon_fixes_need_a_complete_command_and_written_edge() {
    for command in [
        "resid",
        "resid(non_zero)",
        "resid(non_zero)\nsteady;",
        "@#if 1\nresid(non_zero)\n@#endif\n",
        "@#for i in 1:2\nresid(non_zero)\n@#endfor\n",
    ] {
        let text = format!("{MODEL}{command}");
        let diagnostics = analyze(&parse(&text));
        assert!(
            diagnostics.iter().any(|row| row.fix.is_some()),
            "{command}: {diagnostics:?}"
        );
        let fixed = auto_fix(&text);
        assert_ne!(fixed, text, "{command}");
        assert!(
            analyze(&parse(&fixed)).iter().all(|row| row.code != "E001"),
            "{fixed}"
        );
    }
    let text = format!("{MODEL}@#define command = \"resid(non_zero)\"\n@{{command}}\n");
    let diagnostic = analyze(&parse(&text))
        .into_iter()
        .find(|row| row.code == "E001")
        .unwrap();
    let fix = diagnostic
        .fix
        .expect("whole interpolation owns its final edge");
    let fixed = apply_fix(&text, &[fix]);
    assert!(fixed.contains("@{command};\n"));
    assert!(analyze(&parse(&fixed)).iter().all(|row| row.code != "E001"));
    // The existing bulk-auto-fix macro refusal is a separate transport contract.
    assert_eq!(auto_fix(&text), text);
    for command in [
        "resid(1);",
        "resid(non_zero",
        "@#define commands = \"resid(non_zero) steady;\"\n@{commands}\n",
    ] {
        let text = format!("{MODEL}{command}");
        assert!(
            analyze(&parse(&text)).iter().all(|row| row.fix.is_none()),
            "{command}"
        );
        assert_eq!(auto_fix(&text), text);
    }
}

#[test]
fn resid_mcp_uses_unsaved_includes_and_retains_refusing_token_ownership() {
    let root = "C:/slice21-resid/root.mod";
    let child = "C:/slice21-resid/command.inc";
    let text = format!("{MODEL}@#include \"command.inc\"\n");
    let files = HashMap::from([
        (root.to_string(), text.clone()),
        (child.to_string(), "resid(1);\n".to_string()),
    ]);
    let diagnostics = dynare_diagnose(&text, Some(root), Some(&files));
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    let diagnostic = &diagnostics[0];
    assert_eq!(diagnostic.code, "E001");
    assert_eq!(
        diagnostic.message,
        "syntax error, unexpected INT_NUMBER, expecting NON_ZERO"
    );
    assert!(diagnostic
        .file
        .as_deref()
        .is_some_and(|file| file.ends_with("command.inc")));
    assert_eq!(diagnostic.line, 1);
    assert_eq!(diagnostic.column, 7);
    let mut files = files;
    files.insert(child.to_string(), "resid(non_zero);\n".to_string());
    assert!(dynare_diagnose(&text, Some(root), Some(&files))
        .iter()
        .all(|row| row.severity != "error"));
}

#[tokio::test]
async fn resid_lsp_reads_unsaved_include_changes_with_utf16_token_ranges() {
    let root = Url::parse("file:///C:/slice21-resid/root.mod").unwrap();
    let child = Url::parse("file:///C:/slice21-resid/command.inc").unwrap();
    let text = format!("{MODEL}@#include \"command.inc\"\n");
    let (service, _socket) = new_service();
    for (uri, source) in [
        (child.clone(), "/*😀*/resid(1);\n".to_string()),
        (root.clone(), text),
    ] {
        service
            .inner()
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri,
                    language_id: "dynare".into(),
                    version: 1,
                    text: source,
                },
            })
            .await;
    }
    let pull = |uri| DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier { uri },
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let report = service
        .inner()
        .diagnostic(pull(child.clone()))
        .await
        .unwrap();
    let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) = report
    else {
        panic!("expected full report");
    };
    let diagnostics = full.full_document_diagnostic_report.items;
    let refusals: Vec<_> = diagnostics
        .iter()
        .filter(|row| row.code == Some(NumberOrString::String("E001".into())))
        .collect();
    assert_eq!(refusals.len(), 1, "{diagnostics:?}");
    assert_eq!(
        refusals[0].range,
        Range::new(Position::new(0, 12), Position::new(0, 13))
    );
    assert_eq!(
        refusals[0].message,
        "syntax error, unexpected INT_NUMBER, expecting NON_ZERO"
    );
    service
        .inner()
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: child.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "resid(non_zero);\n".into(),
            }],
        })
        .await;
    for uri in [root, child] {
        let report = service.inner().diagnostic(pull(uri)).await.unwrap();
        let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) = report
        else {
            panic!("expected full report after change");
        };
        assert!(full
            .full_document_diagnostic_report
            .items
            .iter()
            .all(|row| row.severity != Some(DiagnosticSeverity::ERROR)));
    }
}

#[tokio::test]
async fn resid_lsp_semicolon_fixes_keep_the_written_include_owner() {
    let root = Url::parse("file:///C:/slice21-resid/root.mod").unwrap();
    let child = Url::parse("file:///C:/slice21-resid/command.inc").unwrap();
    for same_owner in [true, false] {
        let root_text = format!(
            "{MODEL}@#include \"command.inc\"\n{}",
            if same_owner { "" } else { "steady;\n" }
        );
        let child_text = if same_owner {
            "resid(non_zero)\nsteady;\n"
        } else {
            "resid(non_zero)\n"
        };
        let (service, _socket) = new_service();
        for (uri, source) in [
            (child.clone(), child_text.to_string()),
            (root.clone(), root_text),
        ] {
            service
                .inner()
                .did_open(DidOpenTextDocumentParams {
                    text_document: TextDocumentItem {
                        uri,
                        language_id: "dynare".into(),
                        version: 1,
                        text: source,
                    },
                })
                .await;
        }
        let owner = if same_owner {
            child.clone()
        } else {
            root.clone()
        };
        let actions = service
            .inner()
            .code_action(CodeActionParams {
                text_document: TextDocumentIdentifier { uri: owner.clone() },
                range: Range::new(Position::new(0, 0), Position::new(3, 50)),
                context: CodeActionContext {
                    diagnostics: Vec::new(),
                    only: Some(vec![CodeActionKind::QUICKFIX]),
                    trigger_kind: None,
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .unwrap()
            .unwrap_or_default();
        let mut semicolon_edits = Vec::new();
        for action in actions {
            let CodeActionOrCommand::CodeAction(action) = action else {
                continue;
            };
            if let Some(DocumentChanges::Edits(edits)) =
                action.edit.and_then(|edit| edit.document_changes)
            {
                for edit in edits {
                    for change in edit.edits {
                        let OneOf::Left(change) = change else {
                            continue;
                        };
                        if change.new_text == ";" {
                            semicolon_edits.push((edit.text_document.uri.clone(), change));
                        }
                    }
                }
            }
        }
        if same_owner {
            assert!(!semicolon_edits.is_empty(), "no semicolon action");
            for (uri, edit) in semicolon_edits {
                assert_eq!(uri, child);
                assert_eq!(
                    edit.range,
                    Range::new(Position::new(0, 15), Position::new(0, 15))
                );
            }
        } else {
            assert!(
                semicolon_edits.is_empty(),
                "cross-file refusing token cannot own the edit: {semicolon_edits:?}"
            );
        }
    }
}

#[test]
fn resid_honesty_at_check_locks_refusals_and_legal_forms() {
    let binary = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    if !binary.is_file() {
        eprintln!("skipping honesty: Dynare 7.2 is absent");
        return;
    }
    for &(command, _, sentence) in REFUSALS {
        let text = format!("{MODEL}{command}");
        let result = run_preprocessor(
            &text,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        assert!(!result.success, "Dynare accepted {command}");
        let expected = format!("syntax error, unexpected {sentence}");
        assert!(
            result.diagnostics.iter().any(|row| row.message == expected),
            "{command}: {result:?}"
        );
        assert!(analyze(&parse(&text))
            .iter()
            .any(|row| row.message == expected));
    }
    for (command, expected) in [
        (
            "resid(1 {);",
            "syntax error, unexpected INT_NUMBER, expecting NON_ZERO",
        ),
        ("resid({1);", "character unrecognized by lexer"),
        ("resid(non_zero {);", "character unrecognized by lexer"),
    ] {
        let text = format!("{MODEL}{command}");
        let result = run_preprocessor(
            &text,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        assert!(!result.success, "Dynare accepted {command}");
        assert!(
            result.diagnostics.iter().any(|row| row.message == expected),
            "{command}: {result:?}"
        );
    }
    for command in [
        "resid;",
        "resid(non_zero);",
        "resid; resid(non_zero); steady; check;",
    ] {
        let text = format!("{MODEL}{command}");
        let result = run_preprocessor(
            &text,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        assert!(result.success, "{command}: {result:?}");
        assert!(analyze(&parse(&text))
            .iter()
            .all(|row| row.severity != Severity::Error));
    }
}
