use std::{collections::HashMap, time::Duration};

use dygnosis::server::{new_service, Backend};
use dygnosis::{analyze, dynare_diagnose, find_preprocessor, parse, run_preprocessor, JsonStage};
use tower_lsp::{lsp_types::*, LanguageServer};

const PREFIX: &str = "var y; model; y=0; end; steady_state_model; ";

fn official(source: &str, accepted: bool, sentence: Option<&str>) {
    let Some(pp) = find_preprocessor(None) else {
        return;
    };
    let result = run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
    assert_eq!(result.success, accepted, "{source}: {result:?}");
    if let Some(sentence) = sentence {
        assert!(result.raw_stdout.contains(sentence), "{source}: {result:?}");
    }
}

#[test]
fn accepted_output_lists_assign_every_written_name_without_fake_tags() {
    for lhs in ["[y,tmp]", "[y tmp]", "[y]", "[y,y]"] {
        let source = format!("{PREFIX}{lhs}=foo(1); end;");
        official(&source, true, None);
        let model = parse(&source);
        let diagnostics = analyze(&model);
        assert!(
            !diagnostics
                .iter()
                .any(|row| row.code.starts_with('E') || row.code == "W042" || row.code == "W131"),
            "{source}: {diagnostics:?}"
        );
        assert_eq!(model.summary().n_steady_state_equations, 1);
        let row = &model.steady_state_equations[0];
        assert!(row.tags.is_empty() && row.tag_map.is_empty());
        assert_eq!(row.rhs, "foo(1)");
        assert!(row.lhs.starts_with('[') && row.lhs.ends_with(']'));
    }
}

#[test]
fn malformed_output_lists_match_the_exact_check_sentence_and_range() {
    for (lhs, token, spelling, expected) in [
        ("[y(0),tmp]", "'('", "(", None),
        ("[y,tmp(-1)]", "'('", "(", None),
        ("[]", "']'", "]", None),
        ("[,y]", "COMMA", ",", None),
        ("[y,]", "']'", "]", None),
        ("[y,,tmp]", "COMMA", ",,", None),
        ("[y+tmp]", "PLUS", "+", None),
        ("[y tmp", "EQUAL", "=", None),
        ("[y] foo", "IDENTIFIER", "foo", Some("EQUAL")),
        ("[y] +", "PLUS_EQUAL", "+=", Some("EQUAL")),
        ("[1,y]", "INT_NUMBER", "1", None),
        ("[y,log]", "LOG", "log", None),
        ("[y,end]", "END", "end", None),
        ("[y,var]", "VAR", "var", None),
    ] {
        let source = format!("{PREFIX}{lhs}=foo(1); y=2; end;");
        let sentence = match expected {
            Some(expected) => format!("syntax error, unexpected {token}, expecting {expected}"),
            None => format!("syntax error, unexpected {token}"),
        };
        official(&source, false, Some(&sentence));
        let model = parse(&source);
        let rows = analyze(&model);
        let syntax: Vec<_> = rows.iter().filter(|row| row.code == "E001").collect();
        assert_eq!(syntax.len(), 1, "{source}: {rows:?}");
        assert_eq!(syntax[0].message, sentence, "{source}");
        let marker = if spelling == ",," {
            source.find(",,").unwrap() + 1
        } else {
            source[PREFIX.len()..].find(spelling).unwrap() + PREFIX.len()
        };
        let length = if spelling == ",," { 1 } else { spelling.len() };
        assert_eq!(
            syntax[0].span,
            dygnosis::span::Span::new(marker, marker + length),
            "{source}"
        );
        assert_eq!(model.steady_state_equations.len(), 1, "{source}");
        assert_eq!(model.steady_state_equations[0].lhs, "y");
        assert!(!model
            .mod_file_locals
            .iter()
            .any(|name| model.name(*name) == "tmp"));
    }
}

#[test]
fn implicit_output_declarations_reach_e030_with_the_first_output_location() {
    let source = format!("{PREFIX}[y,tmp]=foo(1); end; var tmp;");
    let sentence = "Symbol tmp declared twice with different types!";
    official(&source, false, Some(sentence));
    let rows = analyze(&parse(&source));
    let duplicate = rows
        .iter()
        .find(|row| row.code == "E030")
        .expect("implicit output declaration participates in duplicate checking");
    assert_eq!(duplicate.message, sentence);
    assert_eq!(duplicate.related.len(), 1);
    assert_eq!(
        duplicate.related[0].span,
        dygnosis::span::Span::new(PREFIX.len() + 3, PREFIX.len() + 6)
    );
}

#[test]
fn prior_assignments_reach_w131_for_every_output_role() {
    for (source, names) in [
        (format!("{PREFIX}[y,tmp]=foo(1); y=2; end;"), vec!["y"]),
        (format!("{PREFIX}y=1; [y,tmp]=foo(1); end;"), vec!["y"]),
        (format!("{PREFIX}[y,tmp]=foo(1); tmp=2; end;"), vec!["tmp"]),
        (
            format!("{PREFIX}[y,tmp]=foo(1); [y,tmp]=foo(2); end;"),
            vec!["y", "tmp"],
        ),
        (
            format!("{PREFIX}[y,tmp]=foo(1); [y,tmp]=foo(y); end;"),
            vec!["y", "tmp"],
        ),
        (
            "parameters p; var y; model; y=p; end; steady_state_model; p=1; [y,p]=foo(1); end;"
                .into(),
            vec!["p"],
        ),
    ] {
        let rows = analyze(&parse(&source));
        let warnings: Vec<_> = rows.iter().filter(|row| row.code == "W131").collect();
        assert_eq!(warnings.len(), names.len(), "{source}: {rows:?}");
        for name in names {
            let sentence =
                format!("in the 'steady_state_model' block, variable '{name}' is declared twice");
            official(&source, true, Some(&sentence));
            assert!(warnings.iter().any(|row| row.message == sentence));
        }
    }
}

#[test]
fn parameter_outputs_are_assigned_without_inventing_a_scalar_value() {
    let source = "parameters p; var y; model; y=p; end; steady_state_model; [y,p]=foo(1); end;";
    official(source, true, None);
    let model = parse(source);
    let rows = analyze(&model);
    assert!(
        !rows
            .iter()
            .any(|row| row.code == "W010" || row.code == "W042"),
        "{rows:?}"
    );
    assert_eq!(dygnosis::assigned_number(&model, "p"), None);
}

#[test]
fn complete_block_word_admission_is_shared_with_scalar_targets() {
    for row in include_str!("fixtures/scalar_assignment_targets/block_words.tsv")
        .lines()
        .filter(|row| !row.starts_with('#') && !row.is_empty())
    {
        let columns: Vec<_> = row.split('\t').collect();
        let (word, token, admitted) = (columns[0], columns[1], columns[2] == "true");
        for body in [format!("[y,{word}]=foo(1);"), format!("[{word} y]=foo(1);")] {
            let source = format!("{PREFIX}{body} end;");
            let diagnostics = analyze(&parse(&source));
            if admitted {
                official(&source, true, None);
                assert!(
                    !diagnostics
                        .iter()
                        .any(|row| row.code.starts_with('E') || row.code == "W042"),
                    "{source}: {diagnostics:?}"
                );
            } else {
                let sentence = format!("syntax error, unexpected {token}");
                official(&source, false, Some(&sentence));
                assert!(
                    diagnostics
                        .iter()
                        .any(|row| row.code == "E001" && row.message == sentence),
                    "{source}: {diagnostics:?}"
                );
            }
        }
    }
}

#[test]
fn rhs_registration_precedes_targets_and_preserves_calls_and_uses() {
    let source = format!("{PREFIX}[y,left]=foo(right); end;");
    let model = parse(&source);
    let names: Vec<_> = model
        .symbol_type_events
        .iter()
        .map(|event| model.name(event.name))
        .collect();
    assert!(
        names.iter().position(|name| *name == "right").unwrap()
            < names.iter().position(|name| *name == "left").unwrap()
    );
    assert!(
        names.iter().position(|name| *name == "foo").unwrap()
            < names.iter().position(|name| *name == "left").unwrap()
    );
    let row = &model.steady_state_equations[0];
    assert_eq!(
        row.steady_state_targets
            .iter()
            .map(|target| model.name(target.name))
            .collect::<Vec<_>>(),
        ["y", "left"]
    );
    assert_eq!(
        model
            .ident_refs(row)
            .iter()
            .map(|reference| model.name(reference.name))
            .collect::<Vec<_>>(),
        ["y", "left", "right"]
    );
    assert!(row.lhs_expr.is_none());
    assert!(matches!(
        model.exprs.get(row.rhs_expr.unwrap()).kind,
        dygnosis::ExprKind::Call { .. }
    ));

    let source = format!("{PREFIX}[y,foo]=foo(1); end;");
    official(&source, false, Some("foo has incorrect type"));
    let model = parse(&source);
    let foo = model.steady_state_equations[0].steady_state_targets[1].name;
    assert_eq!(model.final_symbol_kind(foo), Some("external_function"));
    assert!(!model.mod_file_locals.contains(&foo));
    // Target type diagnostics are owned by the next slice; do not relabel foo.
}

#[test]
fn assigned_sets_and_forward_endogenous_checks_consume_all_outputs() {
    for (source, code, sentence, accepted) in [
        (
            "var y z; model; y=0; z=0; end; steady_state_model; [y,tmp]=foo(1); end;",
            "W042",
            "variable 'z' is not assigned a value",
            true,
        ),
        (
            "var y z; model; y=0; z=0; end; steady_state_model; [y,tmp]=foo(z); z=1; end;",
            "E130",
            "variable 'z' is undefined in the declaration of variable 'y'",
            false,
        ),
    ] {
        official(source, accepted, Some(sentence));
        let rows = analyze(&parse(source));
        assert!(
            rows.iter()
                .any(|row| row.code == code && row.message == sentence),
            "{rows:?}"
        );
        assert!(
            !rows
                .iter()
                .any(|row| row.code == "W042" && row.message.contains("'y'")),
            "{rows:?}"
        );
    }
    let source = "varexo z; change_type(parameters) z; var y; model; y=z; end; steady_state_model; [y,z]=foo(1); end;";
    official(source, true, None);
    let model = parse(source);
    assert!(analyze(&model)
        .iter()
        .all(|row| row.code != "W010" && row.code != "W042" && !row.code.starts_with('E')));
}

#[test]
fn written_ramsey_commands_exempt_rhs_order_but_preserve_duplicate_warnings() {
    let exact = "var y; model;y=0;end;planner_objective y^2;ramsey_model(instruments=(y));steady_state_model;[y,tmp]=foo(y);end;";
    official(exact, true, None);
    let rows = analyze(&parse(exact));
    assert!(
        !rows
            .iter()
            .any(|row| row.code == "E130" || row.code == "W131"),
        "{rows:?}"
    );

    for command in [
        "ramsey_model(instruments=(y));",
        "ramsey_policy(instruments=(y));",
    ] {
        for body in [
            "y=y; z=1;",
            "y=z; z=1;",
            "[y,tmp]=foo(y); z=1;",
            "[y,tmp]=foo(z); z=1;",
            "y=missing; z=1;",
            "[y,tmp]=foo(missing); z=1;",
        ] {
            for before in [true, false] {
                let prefix = "var y z; model; y=0; z=0; end; planner_objective y^2;";
                let source = if before {
                    format!("{prefix}{command}steady_state_model;{body}end;")
                } else {
                    format!("{prefix}steady_state_model;{body}end;{command}")
                };
                official(&source, true, None);
                let rows = analyze(&parse(&source));
                assert!(
                    !rows.iter().any(|row| row.code == "E130"),
                    "{source}: {rows:?}"
                );
            }
        }
        let source = format!("var y; model; y=0; end; planner_objective y^2; {command}steady_state_model; [y,tmp]=foo(y); [y,tmp]=foo(y); end;");
        let rows = analyze(&parse(&source));
        assert!(
            !rows.iter().any(|row| row.code == "E130"),
            "{source}: {rows:?}"
        );
        for name in ["y", "tmp"] {
            let sentence =
                format!("in the 'steady_state_model' block, variable '{name}' is declared twice");
            official(&source, true, Some(&sentence));
            assert!(
                rows.iter()
                    .any(|row| row.code == "W131" && row.message == sentence),
                "{rows:?}"
            );
        }
    }
    // A planner objective or discretionary policy is not the Ramsey flag.
    for command in [
        "",
        "planner_objective y^2;",
        "planner_objective y^2; discretionary_policy(instruments=(y),order=1);",
    ] {
        let source = format!(
            "var y z; model; y=0; z=0; end; {command}steady_state_model; [y,tmp]=foo(z); z=1; end;"
        );
        let sentence = "variable 'z' is undefined in the declaration of variable 'y'";
        official(&source, false, Some(sentence));
        let rows = analyze(&parse(&source));
        assert!(
            rows.iter()
                .any(|row| row.code == "E130" && row.message == sentence),
            "{source}: {rows:?}"
        );
    }
}

#[test]
fn mcp_retains_written_macro_and_include_ranges_for_syntax_and_declarations() {
    let source = "var y; model; y=0; end;\r\n@#for i in 1:2\r\nsteady_state_model; /* 🚀 */ [y,tmp(@{i})]=foo(1); end;\r\n@#endfor\r\n";
    let rows = dynare_diagnose(source, None, None);
    let syntax: Vec<_> = rows.iter().filter(|row| row.code == "E001").collect();
    assert_eq!(syntax.len(), 2, "{rows:?}");
    let column = source
        .lines()
        .nth(2)
        .unwrap()
        .chars()
        .position(|character| character == '(')
        .unwrap() as u32
        + 1;
    for row in syntax {
        assert_eq!(row.message, "syntax error, unexpected '('");
        assert_eq!(
            (row.line, row.column, row.end_line, row.end_column),
            (3, column, 3, column + 1)
        );
    }

    let root = "C:/ssm-multiple/model.mod";
    let include = "C:/ssm-multiple/ss.inc";
    let source = "var y; model; y=0; end;\n@#include \"ss.inc\"\nvar tmp;";
    let files = HashMap::from([
        (root.to_string(), source.to_string()),
        (
            include.to_string(),
            "steady_state_model; [y,tmp]=foo(1); end;".to_string(),
        ),
    ]);
    let rows = dynare_diagnose(source, Some(root), Some(&files));
    let duplicate = rows.iter().find(|row| row.code == "E030").unwrap();
    assert_eq!(
        duplicate.message,
        "Symbol tmp declared twice with different types!"
    );
    assert_eq!(duplicate.file.as_deref(), None);
    assert_eq!(duplicate.line, 3);
    assert_eq!(duplicate.related.len(), 1);
    assert_eq!(duplicate.related[0]["file"].as_str(), Some(include));
    assert!(!rows.iter().any(|row| row.code == "W042"));
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
async fn lsp_maps_utf16_syntax_and_all_output_writes_then_clears_after_edit() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let uri = Url::parse("file:///C:/ssm-multiple/live.mod").unwrap();
    let source = "var y; model; y=0; end;\r\nsteady_state_model; /* 🚀 */ [y,tmp(0)]=foo(1); end;";
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
    let row = rows
        .iter()
        .find(|row| row.code == Some(NumberOrString::String("E001".into())))
        .unwrap();
    assert_eq!(row.message, "syntax error, unexpected '('");
    let line = source.lines().nth(1).unwrap();
    let column = line[..line.find('(').unwrap()].encode_utf16().count() as u32;
    assert_eq!(
        row.range,
        Range::new(Position::new(1, column), Position::new(1, column + 1))
    );
    let valid = source.replace("tmp(0)", "tmp");
    server
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: valid.clone(),
            }],
        })
        .await;
    let rows = pull(server, &uri).await;
    assert!(
        !rows.iter().any(
            |row| row.code == Some(NumberOrString::String("E001".into()))
                || row.code == Some(NumberOrString::String("W042".into()))
        ),
        "{rows:?}"
    );
    for name in ["y", "tmp"] {
        let line = valid.lines().nth(1).unwrap();
        let byte = if name == "y" {
            line.find("[y").unwrap() + 1
        } else {
            line.find("tmp").unwrap()
        };
        let column = line[..byte].encode_utf16().count() as u32;
        let hits = server
            .document_highlight(DocumentHighlightParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(1, column),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();
        assert!(
            hits.iter().any(|hit| hit.range
                == Range::new(
                    Position::new(1, column),
                    Position::new(1, column + name.len() as u32)
                )
                && hit.kind == Some(DocumentHighlightKind::WRITE)),
            "{name}: {hits:?}"
        );
    }
}
