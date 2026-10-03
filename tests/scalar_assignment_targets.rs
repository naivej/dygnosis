use std::collections::HashMap;
use std::time::Duration;

use dygnosis::server::{new_service, Backend};
use dygnosis::span::Span;
use dygnosis::{analyze, dynare_diagnose, find_preprocessor, parse, run_preprocessor, JsonStage};
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

const PREFIX: &str = "var y; model; y=0; end; steady_state_model; ";
const TIMED_SENTENCE: &str = "syntax error, unexpected '(', expecting EQUAL";

#[test]
fn block_word_naming_and_symbol_admission_match_pinned_check_in_every_case() {
    let fixture = include_str!("fixtures/scalar_assignment_targets/block_words.tsv");
    let pp = find_preprocessor(None);
    let mut count = 0;
    for row in fixture
        .lines()
        .filter(|row| !row.starts_with('#') && !row.is_empty())
    {
        let columns: Vec<_> = row.split('\t').collect();
        let (word, token, admitted) = (columns[0], columns[1], columns[2] == "true");
        count += 1;
        let mixed: String = word
            .chars()
            .enumerate()
            .map(|(index, ch)| {
                if index % 2 == 0 {
                    ch.to_ascii_uppercase()
                } else {
                    ch
                }
            })
            .collect();
        for spelling in [word.to_string(), word.to_ascii_uppercase(), mixed] {
            let source = format!("{PREFIX}y {spelling}=1; y=2; end;");
            let sentence = format!("syntax error, unexpected {token}, expecting EQUAL");
            let diagnostics = analyze(&parse(&source));
            let syntax: Vec<_> = diagnostics
                .iter()
                .filter(|row| row.code == "E001")
                .collect();
            assert_eq!(syntax.len(), 1, "{source}: {diagnostics:?}");
            assert_eq!(syntax[0].message, sentence, "{source}");
            assert_eq!(
                syntax[0].span,
                Span::new(PREFIX.len() + 2, PREFIX.len() + 2 + spelling.len()),
                "{source}"
            );
            if let Some(pp) = &pp {
                let official =
                    run_preprocessor(&source, pp, None, Duration::from_secs(30), JsonStage::Check);
                assert!(!official.success, "{source}: {official:?}");
                assert!(
                    official.raw_stdout.contains(&sentence),
                    "{source}: {official:?}"
                );
            }
            // END belongs to the block closer, so its malformed-head recovery
            // remains in the existing block-end checks rather than this guard.
            if token == "END" {
                continue;
            }
            let source = format!("{PREFIX}{spelling}=1; y=2; end;");
            let diagnostics = analyze(&parse(&source));
            if admitted {
                assert!(
                    !diagnostics.iter().any(|row| row.code.starts_with('E')),
                    "{source}: {diagnostics:?}"
                );
            } else {
                let sentence = format!("syntax error, unexpected {token}");
                let syntax: Vec<_> = diagnostics
                    .iter()
                    .filter(|row| row.code == "E001")
                    .collect();
                assert_eq!(syntax.len(), 1, "{source}: {diagnostics:?}");
                assert_eq!(syntax[0].message, sentence, "{source}");
                assert_eq!(
                    syntax[0].span,
                    Span::new(PREFIX.len(), PREFIX.len() + spelling.len()),
                    "{source}"
                );
            }
            if let Some(pp) = &pp {
                let official =
                    run_preprocessor(&source, pp, None, Duration::from_secs(30), JsonStage::Check);
                assert_eq!(official.success, admitted, "{source}: {official:?}");
                if !admitted {
                    assert!(
                        official
                            .raw_stdout
                            .contains(&format!("syntax error, unexpected {token}")),
                        "{source}: {official:?}"
                    );
                }
            }
        }
    }
    assert_eq!(count, 105);
    // These names are ordinary identifiers in BLOCK even when another lexer
    // state or the editor's call catalog recognizes their spelling.
    for word in [
        "alpha",
        "beta",
        "gamma",
        "floor",
        "log2",
        "adl",
        "growth_factor",
    ] {
        let mixed: String = word
            .chars()
            .enumerate()
            .map(|(index, ch)| {
                if index % 2 == 0 {
                    ch.to_ascii_uppercase()
                } else {
                    ch
                }
            })
            .collect();
        for spelling in [word.to_string(), word.to_ascii_uppercase(), mixed] {
            for (body, accepted) in [
                (format!("{spelling}=1; y=2;"), true),
                (format!("y {spelling}=1; y=2;"), false),
            ] {
                let source = format!("{PREFIX}{body} end;");
                let diagnostics = analyze(&parse(&source));
                if accepted {
                    assert!(
                        !diagnostics.iter().any(|row| row.code.starts_with('E')),
                        "{source}: {diagnostics:?}"
                    );
                } else {
                    assert!(
                        diagnostics.iter().any(|row| row.code == "E001"
                            && row.message
                                == "syntax error, unexpected IDENTIFIER, expecting EQUAL"),
                        "{source}: {diagnostics:?}"
                    );
                }
                if let Some(pp) = &pp {
                    let official = run_preprocessor(
                        &source,
                        pp,
                        None,
                        Duration::from_secs(30),
                        JsonStage::Check,
                    );
                    assert_eq!(official.success, accepted, "{source}: {official:?}");
                    if !accepted {
                        assert!(
                            official
                                .raw_stdout
                                .contains("syntax error, unexpected IDENTIFIER, expecting EQUAL"),
                            "{source}: {official:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn invalid_scalar_targets_keep_pinned_wording_and_token_range() {
    for (row, token, sentence) in [
        ("y(0)=1;", "(", TIMED_SENTENCE),
        ("y(-1)=1;", "(", TIMED_SENTENCE),
        ("y(+1)=1;", "(", TIMED_SENTENCE),
        ("tmp(0)=1;", "(", TIMED_SENTENCE),
        ("p(0)=1;", "(", TIMED_SENTENCE),
        ("y(foo)=1;", "(", TIMED_SENTENCE),
        ("y(0=1;", "(", TIMED_SENTENCE),
        ("(y)=1;", "(", "syntax error, unexpected '('"),
        ("-y=1;", "-", "syntax error, unexpected MINUS"),
        ("+y=1;", "+", "syntax error, unexpected PLUS"),
        ("-1=1;", "-", "syntax error, unexpected MINUS"),
        ("+1=1;", "+", "syntax error, unexpected PLUS"),
        ("exp(0)=1;", "exp", "syntax error, unexpected EXP"),
        ("sum=1;", "sum", "syntax error, unexpected SUM"),
        (
            "steady_state=1;",
            "steady_state",
            "syntax error, unexpected STEADY_STATE",
        ),
        (
            "expectation=1;",
            "expectation",
            "syntax error, unexpected EXPECTATION",
        ),
        ("nan=1;", "nan", "syntax error, unexpected NAN_CONSTANT"),
        ("inf=1;", "inf", "syntax error, unexpected INF_CONSTANT"),
        (
            "constants=1;",
            "constants",
            "syntax error, unexpected CONSTANTS",
        ),
        (
            "y+1=1;",
            "+",
            "syntax error, unexpected PLUS, expecting EQUAL",
        ),
        (
            "y*1=1;",
            "*",
            "syntax error, unexpected TIMES, expecting EQUAL",
        ),
        (
            "y/1=1;",
            "/",
            "syntax error, unexpected DIVIDE, expecting EQUAL",
        ),
        (
            "y^1=1;",
            "^",
            "syntax error, unexpected POWER, expecting EQUAL",
        ),
        (
            "y+=1;",
            "+=",
            "syntax error, unexpected PLUS_EQUAL, expecting EQUAL",
        ),
        (
            "y*=1;",
            "*=",
            "syntax error, unexpected TIMES_EQUAL, expecting EQUAL",
        ),
        (
            "y.x=1;",
            ".",
            "syntax error, unexpected '.', expecting EQUAL",
        ),
        (
            "y log=1;",
            "log",
            "syntax error, unexpected LOG, expecting EQUAL",
        ),
        (
            "y floor=1;",
            "floor",
            "syntax error, unexpected IDENTIFIER, expecting EQUAL",
        ),
        (
            "y sum=1;",
            "sum",
            "syntax error, unexpected SUM, expecting EQUAL",
        ),
        (
            "y steady_state=1;",
            "steady_state",
            "syntax error, unexpected STEADY_STATE, expecting EQUAL",
        ),
        (
            "y expectation=1;",
            "expectation",
            "syntax error, unexpected EXPECTATION, expecting EQUAL",
        ),
        (
            "y nan=1;",
            "nan",
            "syntax error, unexpected NAN_CONSTANT, expecting EQUAL",
        ),
        (
            "y inf=1;",
            "inf",
            "syntax error, unexpected INF_CONSTANT, expecting EQUAL",
        ),
        (
            "y constants=1;",
            "constants",
            "syntax error, unexpected CONSTANTS, expecting EQUAL",
        ),
        ("y;", ";", "syntax error, unexpected ';', expecting EQUAL"),
        ("#tmp=1;", "#", "syntax error, unexpected '#'"),
        ("1=1;", "1", "syntax error, unexpected INT_NUMBER"),
    ] {
        let source = format!("parameters p; {PREFIX}{row} y=2; end;");
        let model = parse(&source);
        let diagnostics = analyze(&model);
        let syntax: Vec<_> = diagnostics
            .iter()
            .filter(|row| row.code == "E001")
            .collect();
        assert_eq!(syntax.len(), 1, "{source}: {diagnostics:?}");
        assert_eq!(syntax[0].message, sentence, "{source}");
        let start = "parameters p; ".len() + PREFIX.len() + row.find(token).unwrap();
        assert_eq!(syntax[0].span, Span::new(start, start + token.len()));
        assert_eq!(model.steady_state_equations.len(), 1, "{source}");
        assert_eq!(model.steady_state_equations[0].text, "y = 2", "{source}");
        assert!(
            model.mod_file_locals.is_empty(),
            "an invalid target must not declare a temporary: {source}"
        );
        if let Some(pp) = find_preprocessor(None) {
            let official = run_preprocessor(
                &source,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            assert!(!official.success, "{source}: {official:?}");
            assert!(
                official.raw_stdout.contains(sentence),
                "{source}: {official:?}"
            );
        }
    }
}

#[test]
fn refused_unmatched_target_keeps_its_block_end_and_later_statements() {
    for body in [
        "y(0=1; y=2; end; parameters q; q=3;",
        "y(0=1;\ny=2;\nend;\nparameters q; q=3;",
        "y(0=1 end;\nparameters q; q=3;",
    ] {
        let source = format!("{PREFIX}{body}");
        let model = parse(&source);
        let diagnostics = analyze(&model);
        assert!(
            diagnostics
                .iter()
                .any(|row| row.code == "E001" && row.message == TIMED_SENTENCE),
            "{source}: {diagnostics:?}"
        );
        assert!(
            !diagnostics
                .iter()
                .any(|row| row.message.contains("Missing 'end;'")),
            "{source}: {diagnostics:?}"
        );
        assert_eq!(
            model.ss_block.unwrap().end as usize,
            source.rfind("end;").unwrap() + "end;".len()
        );
        assert_eq!(model.param_assignments.len(), 1, "{source}");
        assert_eq!(model.name(model.param_assignments[0].name), "q");
        assert_eq!(
            model.steady_state_equations.len(),
            usize::from(body.contains("y=2;")),
            "{source}"
        );
        if let Some(pp) = find_preprocessor(None) {
            let official = run_preprocessor(
                &source,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            assert!(!official.success, "{source}: {official:?}");
            assert!(
                official.raw_stdout.contains(TIMED_SENTENCE),
                "{source}: {official:?}"
            );
        }
    }
}

#[test]
fn interpolated_compound_tokens_follow_expanded_text_and_keep_written_ranges() {
    let mut cases = Vec::new();
    for (operation, token) in [
        ("+=", "PLUS_EQUAL"),
        ("*=", "TIMES_EQUAL"),
        ("+ =", "PLUS"),
        ("* =", "TIMES"),
    ] {
        cases.push((
            format!("@#define op=\"{operation}\"\n{PREFIX}y@{{op}}1; y=2; end;"),
            token,
            Some("@{op}"),
        ));
        cases.push((
            format!("@#define row=\"y{operation}1;\"\n{PREFIX}@{{row}} y=2; end;"),
            token,
            Some("@{row}"),
        ));
    }
    for (operator, combined, separate) in
        [("+", "PLUS_EQUAL", "PLUS"), ("*", "TIMES_EQUAL", "TIMES")]
    {
        for (value, gap, token) in [
            (operator.to_string(), "", combined),
            (format!("{operator} "), "", separate),
            (operator.to_string(), " ", separate),
        ] {
            cases.push((
                format!("@#define op=\"{value}\"\n{PREFIX}y@{{op}}{gap}=1; y=2; end;"),
                token,
                None,
            ));
        }
        for (value, token) in [("=", combined), (" =", separate)] {
            cases.push((
                format!("@#define eq=\"{value}\"\n{PREFIX}y{operator}@{{eq}}1; y=2; end;"),
                token,
                None,
            ));
        }
        for (gap, token) in [("", combined), (" ", separate)] {
            cases.push((format!("@#define op=\"{operator}\"\n@#define eq=\"=\"\n{PREFIX}y@{{op}}{gap}@{{eq}}1; y=2; end;"), token, None));
        }
    }
    for (source, token, marker) in cases {
        let sentence = format!("syntax error, unexpected {token}, expecting EQUAL");
        let model = parse(&source);
        let diagnostics = analyze(&model);
        let syntax: Vec<_> = diagnostics
            .iter()
            .filter(|row| row.code == "E001")
            .collect();
        assert_eq!(syntax.len(), 1, "{source}: {diagnostics:?}");
        assert_eq!(syntax[0].message, sentence, "{source}");
        assert_eq!(model.steady_state_equations.len(), 1, "{source}");
        assert_eq!(model.steady_state_equations[0].text, "y = 2", "{source}");
        assert!(syntax[0].span.start as usize >= source.find("steady_state_model;").unwrap());
        if let Some(marker) = marker {
            let start = source.find(marker).unwrap();
            assert_eq!(
                syntax[0].span,
                Span::new(start, start + marker.len()),
                "{source}"
            );
            let rows = dynare_diagnose(&source, None, None);
            let row = rows.iter().find(|row| row.code == "E001").unwrap();
            assert_eq!(row.message, sentence);
            let column = PREFIX.len() as u32 + if marker == "@{op}" { 2 } else { 1 };
            assert_eq!(
                (row.line, row.column, row.end_line, row.end_column),
                (2, column, 2, column + marker.len() as u32)
            );
        }
        if let Some(pp) = find_preprocessor(None) {
            let official = run_preprocessor(
                &source,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            assert!(!official.success, "{source}: {official:?}");
            assert!(
                official.raw_stdout.contains(&sentence),
                "{source}: {official:?}"
            );
        }
    }
}

#[test]
fn valid_scalar_and_lag_controls_have_no_new_syntax_errors() {
    for source in [
        "var y; model; y=0; end; steady_state_model; y=1; end;",
        "var y; model; y=0; end; steady_state_model; tmp=1; y=tmp; end;",
        "var y; model; y=0; end; steady_state_model; floor=1; y=-floor; end;",
        "var y; model; y=0; end; steady_state_model; y=+1; end;",
        "var y; model; y=0; end; steady_state_model; y=-1; end;",
        "parameters p; var y; model; y=p; end; steady_state_model; p=1; y=p; end;",
        "varexo z; change_type(parameters) z; var y; model; y=0; end; steady_state_model; z=1; y=0; end;",
        "var y; model; y=0; end; steady_state_model; [y,tmp]=foo(1); end;",
        "var y; model; y=0; end; steady_state_model; [y tmp]=foo(1); end;",
        "var y; model; y=.5*y(-1); end; histval; y(-1)=1; end;",
        "var y; model; y=.5*y(-1); end; filter_initial_state; y(0)=1; end;",
        "var y; model; y=0; end; initval; y=1; end; endval; y=2; end;",
        "var y; model; y=0; end;\n@#for i in 1:2\nsteady_state_model; tmp@{i}=@{i}; y=tmp@{i}; end;\n@#endfor\n",
        "var y; model; y=0; end;\n@#if 0\nsteady_state_model; y(0)=1; end;\n@#endif\n",
    ] {
        let diagnostics = analyze(&parse(source));
        assert!(
            !diagnostics.iter().any(|row| row.code.starts_with('E')),
            "{source}: {diagnostics:?}"
        );
        if let Some(pp) = find_preprocessor(None) {
            let official =
                run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
            assert!(official.success, "{source}: {official:?}");
        }
    }
}

#[test]
fn existing_generic_diagnostics_still_reach_scalar_and_lag_rows() {
    for (source, code, sentence, success) in [
        (
            "var y; model; y=0; end; steady_state_model; tmp=1; y=tmp; end; var tmp;",
            "E030",
            "Symbol tmp declared twice with different types!",
            false,
        ),
        (
            "var y; model; y=0; end; steady_state_model; y=1; y=2; end;",
            "W131",
            "in the 'steady_state_model' block, variable 'y' is declared twice",
            true,
        ),
        (
            "var y z; model; y=0; z=0; end; steady_state_model; y=z; z=1; end;",
            "E130",
            "variable 'z' is undefined in the declaration of variable 'y'",
            false,
        ),
        (
            "var y; model; y=.5*y(-1); end; histval; missing(-1)=1; end;",
            "E058",
            "Unknown symbol: missing",
            false,
        ),
        (
            "parameters p; var y; model; y=0; end; histval; p(-1)=1; end;",
            "E059",
            "p is neither endogenous or exogenous.",
            false,
        ),
        (
            "var y; model; y=.5*y(-1); end; histval; y(-1)=1; y(-1)=2; end;",
            "E243",
            "histval: y(-1) declared twice",
            false,
        ),
        (
            "var y; model; y=.5*y(-1); end; filter_initial_state; missing(0)=1; end;",
            "E058",
            "Unknown symbol: missing",
            false,
        ),
        (
            "parameters p; var y; model; y=.5*y(-1); end; filter_initial_state; p(0)=1; end;",
            "E311",
            "filter_initial_state: p should be an endogenous or exogenous variable",
            false,
        ),
        (
            "var y; model; y=.5*y(-1); end; filter_initial_state; y(0)=1; y(0)=2; end;",
            "E313",
            "filter_initial_state: (y, 0) declared twice",
            false,
        ),
    ] {
        let diagnostics = analyze(&parse(source));
        let our_needle = if code == "E058" {
            "is not declared."
        } else {
            sentence
        };
        assert!(
            diagnostics
                .iter()
                .any(|row| row.code == code && row.message.contains(our_needle)),
            "{source}: {diagnostics:?}"
        );
        assert!(
            !diagnostics.iter().any(|row| row.code == "E001"),
            "{source}: {diagnostics:?}"
        );
        if let Some(pp) = find_preprocessor(None) {
            let official =
                run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
            assert_eq!(official.success, success, "{source}: {official:?}");
            assert!(
                official.raw_stdout.contains(sentence),
                "{source}: {official:?}"
            );
        }
    }
}

#[test]
fn mcp_keeps_written_ranges_for_macro_copies_and_included_targets() {
    let source = "var y; model; y=0; end;\r\n@#for i in 1:2\r\nsteady_state_model; /* 🚀 */ y(@{i})=1; end;\r\n@#endfor\r\n";
    let rows = dynare_diagnose(source, None, None);
    let syntax: Vec<_> = rows.iter().filter(|row| row.code == "E001").collect();
    assert_eq!(syntax.len(), 2, "{rows:?}");
    for row in syntax {
        assert_eq!(row.message, TIMED_SENTENCE);
        assert_eq!(
            (row.line, row.column, row.end_line, row.end_column),
            (3, 30, 3, 31)
        );
    }
    let root = "C:/scalar-targets/model.mod";
    let included = "C:/scalar-targets/ss.inc";
    let source = "var y; model; y=0; end;\n@#include \"ss.inc\"\n";
    let files = HashMap::from([
        (root.to_string(), source.to_string()),
        (
            included.to_string(),
            "steady_state_model; y(0)=1; end;".to_string(),
        ),
    ]);
    let rows = dynare_diagnose(source, Some(root), Some(&files));
    let row = rows.iter().find(|row| row.code == "E001").unwrap();
    assert_eq!(row.message, TIMED_SENTENCE);
    assert_eq!(row.file.as_deref(), Some(included));
    assert_eq!(
        (row.line, row.column, row.end_line, row.end_column),
        (1, 22, 1, 23)
    );
}

async fn pull(server: &Backend, uri: &Url) -> Vec<Diagnostic> {
    match server
        .diagnostic(DocumentDiagnosticParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            identifier: None,
            previous_result_id: None,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
    {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
            full.full_document_diagnostic_report.items
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn lsp_exposes_the_same_syntax_sentence_and_utf16_range_then_clears_it() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let uri = Url::parse("file:///C:/scalar-targets/live.mod").unwrap();
    let source = "var y; model; y=0; end;\r\nsteady_state_model; /* 🚀 */ y(0)=1; end;";
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
    let rows = pull(server, &uri).await;
    let syntax: Vec<_> = rows
        .iter()
        .filter(|row| row.code == Some(NumberOrString::String("E001".into())))
        .collect();
    assert_eq!(syntax.len(), 1, "{rows:?}");
    assert_eq!(syntax[0].message, TIMED_SENTENCE);
    assert_eq!(
        syntax[0].range,
        Range::new(Position::new(1, 30), Position::new(1, 31))
    );
    assert_eq!(syntax[0].severity, Some(DiagnosticSeverity::ERROR));
    server
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: source.replace("y(0)=1", "y=1"),
            }],
        })
        .await;
    let rows = pull(server, &uri).await;
    assert!(
        !rows
            .iter()
            .any(|row| row.code == Some(NumberOrString::String("E001".into()))),
        "{rows:?}"
    );
    let macro_source = "@#define op=\"+=\"\r\nvar y; model; y=0; end;\r\nsteady_state_model; /* 🚀 */ y@{op}1; end;";
    server
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: 3,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: macro_source.into(),
            }],
        })
        .await;
    let rows = pull(server, &uri).await;
    let row = rows
        .iter()
        .find(|row| row.code == Some(NumberOrString::String("E001".into())))
        .unwrap();
    assert_eq!(
        row.message,
        "syntax error, unexpected PLUS_EQUAL, expecting EQUAL"
    );
    assert_eq!(
        row.range,
        Range::new(Position::new(2, 30), Position::new(2, 35))
    );
    server
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: 4,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: macro_source.replace("\"+=\"", "\"=\""),
            }],
        })
        .await;
    let rows = pull(server, &uri).await;
    assert!(
        !rows
            .iter()
            .any(|row| row.code == Some(NumberOrString::String("E001".into()))),
        "{rows:?}"
    );
}
