//! Empty steady-state blocks refuse before Check and keep written recovery facts.

use std::{collections::HashMap, path::PathBuf, time::Duration};

use dygnosis::server::new_service;
use dygnosis::{analyze, dynare_diagnose, parse, run_preprocessor, JsonStage, Severity};
use tower_lsp::{lsp_types::*, LanguageServer};

const MODEL: &str = "var y z; varexo e; model; y=z+e; z=y; end;\n";
const EMPTY: &str = "steady_state_model; end;";
const SENTENCE: &str = "syntax error, unexpected END";

fn official(source: &str, accepted: bool, sentence: Option<&str>) {
    let binary = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    if !binary.is_file() {
        return;
    }
    let result = run_preprocessor(
        source,
        &binary,
        None,
        Duration::from_secs(30),
        JsonStage::Check,
    );
    assert_eq!(result.success, accepted, "{source}: {result:?}");
    if let Some(sentence) = sentence {
        assert!(result.raw_stdout.contains(sentence), "{result:?}");
    }
}

#[test]
fn empty_and_comment_only_blocks_refuse_on_the_written_end() {
    for body in ["", "/* empty */", "% empty\n", "// empty\n"] {
        let source = format!("{MODEL}steady_state_model; {body} end; parameters later;");
        official(&source, false, Some(SENTENCE));
        let model = parse(&source);
        let rows = analyze(&model);
        let errors: Vec<_> = rows.iter().filter(|row| row.code == "E001").collect();
        assert_eq!(errors.len(), 1, "{source}: {rows:?}");
        assert_eq!(errors[0].message, SENTENCE);
        assert_eq!(errors[0].severity, Severity::Error);
        assert_eq!(
            &source[errors[0].span.start as usize..errors[0].span.end as usize],
            "end"
        );
        assert!(errors[0].fix.is_none());
        assert!(model.ss_block.is_none());
        assert!(model.steady_state_equations.is_empty());
        assert_eq!(model.equations.len(), 2);
        assert!(model
            .parameters
            .iter()
            .any(|decl| model.name(decl.name) == "later"));
        assert!(rows
            .iter()
            .all(|row| !matches!(row.code.as_str(), "E021" | "W022" | "W042")));
    }
}

#[test]
fn assignments_from_an_earlier_block_cannot_fill_a_later_empty_block() {
    let source = format!("{MODEL}steady_state_model; y=0; end; {EMPTY}");
    official(&source, false, Some(SENTENCE));
    let model = parse(&source);
    assert_eq!(model.steady_state_equations.len(), 1);
    let accepted = model.ss_block.expect("retain the earlier completed block");
    assert_eq!(
        accepted.end as usize,
        source.find("y=0; end;").unwrap() + "y=0; end;".len()
    );
    let rows = analyze(&model);
    let refusal = rows.iter().find(|row| row.code == "E001").unwrap();
    assert_eq!(refusal.message, SENTENCE);
    assert_eq!(refusal.span.start as usize, source.rfind("end;").unwrap());
    assert!(rows.iter().all(|row| row.code != "W042"));
}

#[test]
fn invalid_only_rows_keep_the_first_refusal_without_an_empty_body_error() {
    for (body, code, sentence) in [
        (
            "y(0)=1;",
            "E001",
            "syntax error, unexpected '(', expecting EQUAL",
        ),
        ("y=;", "E001", "syntax error, unexpected ';'"),
        (";", "E001", "syntax error, unexpected ';'"),
        ("e=1;", "E481", "e has incorrect type"),
        (
            "y=pp.value;",
            "E275",
            "Namespace-qualified expressions like",
        ),
    ] {
        let source = format!("{MODEL}steady_state_model; {body} end;");
        official(&source, false, (code != "E275").then_some(sentence));
        let model = parse(&source);
        let rows = analyze(&model);
        assert!(
            rows.iter().any(|row| row.code == code),
            "{source}: {rows:?}"
        );
        assert!(
            rows.iter().all(|row| row.message != SENTENCE),
            "{source}: {rows:?}"
        );
        assert!(model.ss_block.is_none(), "{source}");
        assert!(rows
            .iter()
            .all(|row| !matches!(row.code.as_str(), "E021" | "W022" | "W042")));
    }
}

#[test]
fn scalar_and_bracketed_rows_accept_and_unfinished_blocks_keep_their_closer_error() {
    for body in ["y=0; z=0;", "[y,z]=helper(1);"] {
        let source = format!("{MODEL}steady_state_model; {body} end;");
        official(&source, true, None);
        let model = parse(&source);
        assert!(model.ss_block.is_some());
        assert!(analyze(&model)
            .iter()
            .all(|row| row.severity != Severity::Error));
    }
    for body in ["", "y=0;"] {
        let source = format!("{MODEL}steady_state_model; {body}");
        let rows = analyze(&parse(&source));
        assert!(rows.iter().any(|row| row.code == "E001"), "{rows:?}");
        assert!(rows.iter().all(|row| row.message != SENTENCE));
    }
}

#[test]
fn an_invalid_rhs_role_cannot_make_a_block_successful() {
    let source = "var y; model; #local=1; y=local; end; steady_state_model; y=local; end;";
    official(source, false, Some("not allowed outside model declaration"));
    let model = parse(source);
    assert!(model.ss_block.is_none());
    let rows = analyze(&model);
    assert!(rows.iter().any(|row| row.code == "E282"));
    assert!(rows.iter().all(|row| row.message != SENTENCE));
}

#[test]
fn a_refused_rhs_does_not_register_a_steady_state_output() {
    for row in ["fresh=1/0;", "[fresh,y]=1/0;"] {
        let source = format!("{MODEL}steady_state_model; y=0; {row} end;");
        official(&source, false, Some("Division by zero"));
        let model = parse(&source);
        assert!(model
            .mod_file_locals
            .iter()
            .all(|name| model.name(*name) != "fresh"));
        assert_eq!(
            model.steady_state_equations.len(),
            2,
            "retain written recovery rows"
        );
        assert!(model.ss_block.is_none());
        let rows = analyze(&model);
        assert_eq!(
            rows.iter().filter(|row| row.code == "E278").count(),
            1,
            "{rows:?}"
        );
        assert!(rows.iter().all(|row| row.code != "E481"));
    }
    let source =
        format!("external_function(name=fun,nargs=1);{MODEL}steady_state_model; fun=1/0; end;");
    official(&source, false, Some("Division by zero"));
    let rows = analyze(&parse(&source));
    assert!(rows.iter().any(|row| row.code == "E278"));
    assert!(rows
        .iter()
        .all(|row| !matches!(row.code.as_str(), "E279" | "E481")));
}

#[test]
fn a_captured_epilogue_role_has_one_owner_per_executed_refusal() {
    for copies in [1, 2] {
        let root = "C:/slice22-role/root.mod";
        let child = "C:/slice22-role/body.inc";
        let source = format!("var y; parameters p; model;y=0;end; epilogue;x=y;end;\n@#for j in 1:{copies}\n@#include \"body.inc\"\n@#endfor\n");
        let files = HashMap::from([
            (root.to_string(), source.clone()),
            (child.to_string(), "p=x/0;".to_string()),
        ]);
        let rows = dynare_diagnose(&source, Some(root), Some(&files));
        assert_eq!(
            rows.iter().filter(|row| row.code == "E294").count(),
            copies,
            "{rows:?}"
        );
        assert!(rows.iter().all(|row| row.code != "E278"), "{rows:?}");
    }
}

#[test]
fn macro_selection_and_copies_use_the_current_blocks_completed_rows() {
    for tail in [
        "@#if 1\nsteady_state_model;\n@#if 0\ny=0;\n@#endif\nend;\n@#endif\n",
        "@#for n in 1:2\nsteady_state_model; end;\n@#endfor\n",
    ] {
        let source = format!("{MODEL}{tail}");
        official(&source, false, Some(SENTENCE));
        let model = parse(&source);
        assert!(model.ss_block.is_none());
        let rows = analyze(&model);
        assert!(rows
            .iter()
            .any(|row| row.code == "E001" && row.message == SENTENCE));
        assert!(rows.iter().all(|row| row.code != "W042"));
    }
    let source =
        format!("{MODEL}@#if 0\n{EMPTY}\n@#else\nsteady_state_model; y=0; z=0; end;\n@#endif\n");
    official(&source, true, None);
    assert!(analyze(&parse(&source))
        .iter()
        .all(|row| row.severity != Severity::Error));
}

#[test]
fn steady_state_check_order_and_duplicates_wait_for_parse_completion() {
    for (body, code) in [("y=unknown;", "E130"), ("y=0; y=0;", "W131")] {
        let accepted = format!("{MODEL}steady_state_model; {body} end;");
        let control = analyze(&parse(&accepted));
        assert!(control.iter().any(|row| row.code == code), "{control:?}");
        let refused = format!("{MODEL}steady_state_model; {body} fresh=1/0; end;");
        official(&refused, false, Some("Division by zero"));
        let rows = analyze(&parse(&refused));
        assert!(rows.iter().any(|row| row.code == "E278"), "{rows:?}");
        assert!(
            rows.iter()
                .all(|row| !matches!(row.code.as_str(), "E130" | "W131" | "W042")),
            "{rows:?}"
        );
    }
}

#[test]
fn mcp_unsaved_include_reports_the_empty_block_end_and_accepts_its_repair() {
    let root = "C:/slice22-ss/root.mod";
    let child = "C:/slice22-ss/ss.inc";
    let source = format!("{MODEL}@#include \"ss.inc\"\n");
    let mut files = HashMap::from([
        (root.to_string(), "old disk contents".to_string()),
        (
            child.to_string(),
            "/*😀*/steady_state_model; end;\n".to_string(),
        ),
    ]);
    let rows = dynare_diagnose(&source, Some(root), Some(&files));
    let refusal = rows.iter().find(|row| row.code == "E001").unwrap();
    assert_eq!(refusal.message, SENTENCE);
    assert_eq!(refusal.severity, "ERROR");
    assert_eq!(refusal.file.as_deref(), Some(child));
    assert_eq!(
        (refusal.line, refusal.column, refusal.end_column),
        (1, 26, 29)
    );
    assert!(rows.iter().all(|row| row.code != "W042"));
    files.insert(
        child.to_string(),
        "steady_state_model; y=0; z=0; end;\n".to_string(),
    );
    assert!(dynare_diagnose(&source, Some(root), Some(&files))
        .iter()
        .all(|row| row.severity != "ERROR"));
}

#[tokio::test]
async fn lsp_unsaved_include_updates_the_same_mapped_empty_block_error() {
    let root = Url::parse("file:///C:/slice22-ss/root.mod").unwrap();
    let child = Url::parse("file:///C:/slice22-ss/ss.inc").unwrap();
    let (service, _socket) = new_service();
    for (uri, text) in [
        (
            child.clone(),
            "/*😀*/steady_state_model; end;\n".to_string(),
        ),
        (root.clone(), format!("{MODEL}@#include \"ss.inc\"\n")),
    ] {
        service
            .inner()
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri,
                    language_id: "dynare".into(),
                    version: 1,
                    text,
                },
            })
            .await;
    }
    let params = |uri| DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier { uri },
        identifier: None,
        previous_result_id: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let report = service
        .inner()
        .diagnostic(params(child.clone()))
        .await
        .unwrap();
    let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) = report
    else {
        panic!("expected full report");
    };
    let rows = full.full_document_diagnostic_report.items;
    let refusal = rows
        .iter()
        .find(|row| row.code == Some(NumberOrString::String("E001".into())))
        .unwrap();
    assert_eq!(refusal.message, SENTENCE);
    assert_eq!(refusal.severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(
        refusal.range,
        Range::new(Position::new(0, 26), Position::new(0, 29))
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
                text: "steady_state_model; y=0; z=0; end;\n".into(),
            }],
        })
        .await;
    for uri in [root, child] {
        let report = service.inner().diagnostic(params(uri)).await.unwrap();
        let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) = report
        else {
            panic!("expected full report");
        };
        assert!(full
            .full_document_diagnostic_report
            .items
            .iter()
            .all(|row| row.severity != Some(DiagnosticSeverity::ERROR)));
    }
}
