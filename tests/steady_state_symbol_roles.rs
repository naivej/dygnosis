use std::{collections::HashMap, time::Duration};

use dygnosis::server::{new_service, Backend};
use dygnosis::{analyze, dynare_diagnose, find_preprocessor, parse, run_preprocessor, JsonStage};
use tower_lsp::{lsp_types::*, LanguageServer};

const PREFIX: &str = "var y; model; y=0; end; ";

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

fn owned(source: &str) -> Vec<dygnosis::Diagnostic> {
    analyze(&parse(source))
        .into_iter()
        .filter(|row| matches!(row.code.as_str(), "E130" | "E481"))
        .collect()
}

#[test]
fn wrong_target_roles_have_exact_parse_sentence_and_name_range() {
    for (prefix, name) in [
        ("var y; varexo e; model; y=e; end;", "e"),
        ("var y; varexo_det d; model; y=d; end;", "d"),
        ("var y; model; #loc=1; y=loc; end;", "loc"),
        ("trend_var(growth_factor=1) t; var y; model; y=0; end;", "t"),
        (
            "log_trend_var(log_growth_factor=1) t; var y; model; y=0; end;",
            "t",
        ),
        (
            "external_function(name=foo,nargs=1); var y; model; y=0; end;",
            "foo",
        ),
        ("var y; model; y=0; end; epilogue; epi=y; end;", "epi"),
        ("var y gone; var_remove gone; model; y=0; end;", "gone"),
    ] {
        for bracketed in [false, true] {
            let lhs = if bracketed {
                format!("[y,{name}]")
            } else {
                name.into()
            };
            let rhs = if bracketed { "bar(1)" } else { "1" };
            let source = format!("{prefix} steady_state_model; {lhs}={rhs}; y=0; end;");
            let sentence = format!("{name} has incorrect type");
            official(&source, false, Some(&sentence));
            let rows = owned(&source);
            let types: Vec<_> = rows.iter().filter(|row| row.code == "E481").collect();
            assert_eq!(types.len(), 1, "{source}: {rows:?}");
            assert_eq!(types[0].message, sentence);
            let block = source.find("steady_state_model;").unwrap();
            let offset = block
                + source[block..].find(&format!("{lhs}=")).unwrap()
                + if bracketed { 3 } else { 0 };
            assert_eq!(
                types[0].span,
                dygnosis::span::Span::new(offset, offset + name.len())
            );
        }
    }
}

#[test]
fn ordinary_endogenous_parameter_and_new_temporary_targets_stay_allowed() {
    for body in [
        "y=1;",
        "tmp=1; y=tmp;",
        "[y,tmp]=foo(1);",
        "[y tmp]=foo(1);",
        "[y,y]=foo(1);",
        "[tmp,y]=foo(1);",
        "floor=1; y=floor;",
    ] {
        let source = format!("{PREFIX}steady_state_model; {body} end;");
        official(&source, true, None);
        assert!(owned(&source).is_empty(), "{source}: {:?}", owned(&source));
    }
    for body in ["p=1; y=p;", "[y,p]=foo(1);"] {
        let source = format!("parameters p; {PREFIX}steady_state_model; {body} end;");
        official(&source, true, None);
        assert!(owned(&source).is_empty(), "{source}: {:?}", owned(&source));
    }
}

#[test]
fn target_type_uses_rhs_first_parser_context_instead_of_final_type() {
    for body in ["z=1; y=0;", "[y,z]=foo(1);"] {
        for (prefix, tail, accepted) in [
            ("varexo z; change_type(parameters) z;", "", true),
            ("varexo z;", "change_type(parameters) z;", false),
            ("parameters z; change_type(varexo) z;", "", false),
        ] {
            let source = format!("{prefix}{PREFIX}steady_state_model;{body}end;{tail}");
            official(
                &source,
                accepted,
                (!accepted).then_some("z has incorrect type"),
            );
            assert_eq!(
                owned(&source).iter().any(|row| row.code == "E481"),
                !accepted,
                "{source}"
            );
        }
        let source = format!("parameters z; var y; steady_state_model;{body}end;change_type(varexo) z; model; y=z; end;");
        official(&source, true, None);
        assert!(owned(&source).is_empty(), "{source}: {:?}", owned(&source));
    }
}

#[test]
fn heterogeneous_targets_require_a_prior_change_to_an_ordinary_type() {
    for kind in ["var", "varexo", "parameters"] {
        for body in ["z=1; y=0;", "[y,z]=foo(1);"] {
            for before in [false, true] {
                let change = if before {
                    "change_type(parameters) z;"
                } else {
                    ""
                };
                let source = format!("heterogeneity_dimension h; {kind}(heterogeneity=h) z; {PREFIX}{change}steady_state_model;{body}end;");
                official(&source, before, (!before).then_some("z has incorrect type"));
                let model = parse(&source);
                assert_eq!(
                    owned(&source).iter().any(|row| row.code == "E481"),
                    !before,
                    "{source}: declarations={:?}, events={:?}, targets={:?}",
                    model.endogenous,
                    model.symbol_type_events,
                    model.steady_state_equations
                );
            }
            let source = format!("heterogeneity_dimension h; {kind}(heterogeneity=h) z; {PREFIX}steady_state_model;{body}end;change_type(parameters) z;");
            official(&source, false, Some("z has incorrect type"));
            assert!(
                owned(&source).iter().any(|row| row.code == "E481"),
                "{source}"
            );
        }
    }
}

#[test]
fn unknown_rhs_calls_register_before_scalar_and_multiple_output_types() {
    for name in [
        "foo", "floor", "ceil", "round", "log2", "norminv", "logncdf",
    ] {
        for lhs in [
            name.to_string(),
            format!("[y,{name}]"),
            format!("[{name},y]"),
        ] {
            let source = format!("{PREFIX}steady_state_model;{lhs}={name}(1);y=0;end;");
            let sentence = format!("{name} has incorrect type");
            official(&source, false, Some(&sentence));
            let rows = owned(&source);
            assert!(
                rows.iter()
                    .any(|row| row.code == "E481" && row.message == sentence),
                "{source}: {rows:?}"
            );
            assert!(
                !rows.iter().any(|row| row.code == "E130"),
                "{source}: {rows:?}"
            );
        }
        let source = format!("{PREFIX}steady_state_model;y={name}(1);end;");
        official(&source, true, None);
        assert!(owned(&source).is_empty(), "{source}: {:?}", owned(&source));
    }
}

#[test]
fn undefined_implicit_rhs_locals_are_checked_before_any_current_outputs() {
    for (body, missing, output) in [
        ("y=tmp;", "tmp", "y"),
        ("y=tmp; tmp=1;", "tmp", "y"),
        ("tmp=tmp; y=0;", "tmp", "tmp"),
        ("[y,tmp]=foo(tmp);", "tmp", "y"),
        ("[y,tmp]=foo(later); later=1;", "later", "y"),
    ] {
        let source = format!("{PREFIX}steady_state_model;{body}end;");
        let sentence =
            format!("variable '{missing}' is undefined in the declaration of variable '{output}'");
        official(&source, false, Some(&sentence));
        let rows = owned(&source);
        assert!(
            rows.iter()
                .any(|row| row.code == "E130" && row.message == sentence),
            "{source}: {rows:?}"
        );
        let row = rows.iter().find(|row| row.code == "E130").unwrap();
        assert_eq!(
            &source[row.span.start as usize..row.span.end as usize],
            missing
        );
    }
    for name in ["floor", "ceil", "round", "log2", "norminv", "logncdf"] {
        let source = format!("{PREFIX}steady_state_model;y={name};end;");
        let sentence = format!("variable '{name}' is undefined in the declaration of variable 'y'");
        official(&source, false, Some(&sentence));
        assert!(
            owned(&source)
                .iter()
                .any(|row| row.code == "E130" && row.message == sentence),
            "{source}"
        );
    }
}

#[test]
fn rhs_order_uses_final_types_and_does_not_require_a_later_output() {
    for body in ["y=z;", "[y,tmp]=foo(z);"] {
        let source = format!("var y z; model;y=0;z=0;end;steady_state_model;{body}end;");
        let sentence = "variable 'z' is undefined in the declaration of variable 'y'";
        official(&source, false, Some(sentence));
        assert!(
            owned(&source)
                .iter()
                .any(|row| row.code == "E130" && row.message == sentence),
            "{source}"
        );
        let source =
            format!("parameters z;{PREFIX}steady_state_model;{body}end;change_type(var) z;");
        official(&source, false, Some(sentence));
        assert!(
            owned(&source)
                .iter()
                .any(|row| row.code == "E130" && row.message == sentence),
            "{source}"
        );
    }
    for (decl, model) in [
        ("parameters z;", "y=0;"),
        ("varexo z;", "y=z;"),
        ("varexo_det z;", "y=z;"),
    ] {
        let source = format!("{decl}var y;model;{model}end;steady_state_model;y=z;end;");
        official(&source, true, None);
        assert!(owned(&source).is_empty(), "{source}: {:?}", owned(&source));
    }
    let source = format!("{PREFIX}\ntmp=1;\nsteady_state_model;y=tmp;end;");
    official(&source, false, Some("variable 'tmp' is undefined"));
    assert!(owned(&source).iter().any(|row| row.code == "E130"));
    let source = format!("{PREFIX}steady_state_model;tmp=1;end;steady_state_model;y=tmp;end;");
    official(&source, true, None);
    assert!(owned(&source).is_empty());
}

#[test]
fn ramsey_exempts_local_rhs_order_without_exempting_wrong_target_types() {
    for command in [
        "ramsey_model(instruments=(y));",
        "ramsey_policy(instruments=(y));",
    ] {
        for body in ["y=missing;", "[y,tmp]=foo(missing);"] {
            for before in [false, true] {
                let prefix = format!("{PREFIX}planner_objective y^2;");
                let source = if before {
                    format!("{prefix}{command}steady_state_model;{body}end;")
                } else {
                    format!("{prefix}steady_state_model;{body}end;{command}")
                };
                official(&source, true, None);
                assert!(owned(&source).is_empty(), "{source}: {:?}", owned(&source));
            }
        }
        let source = format!("varexo e;var y;model;y=e;end;planner_objective y^2;{command}steady_state_model;e=1;y=0;end;");
        official(&source, false, Some("e has incorrect type"));
        assert!(owned(&source).iter().any(|row| row.code == "E481"));
    }
    for command in [
        "",
        "planner_objective y^2;",
        "planner_objective y^2;discretionary_policy(instruments=(y),order=1);",
    ] {
        let source = format!("{PREFIX}{command}steady_state_model;[y,tmp]=foo(missing);end;");
        official(&source, false, Some("variable 'missing' is undefined"));
        assert!(owned(&source).iter().any(|row| row.code == "E130"));
    }
}

#[test]
fn captured_target_context_follows_macro_execution_not_shared_source_offsets() {
    let source = "var y; varexo z; model;y=0;end;\n@#for k in 1:2\n@#if k == 1\nchange_type(parameters) z;\n@#else\nchange_type(varexo) z;\n@#endif\nsteady_state_model; z=1; y=0; end;\n@#endfor\n";
    official(source, false, Some("z has incorrect type"));
    let rows = owned(source);
    let types: Vec<_> = rows.iter().filter(|row| row.code == "E481").collect();
    assert_eq!(types.len(), 1, "{rows:?}");
    assert_eq!(
        &source[types[0].span.start as usize..types[0].span.end as usize],
        "z"
    );
    let source =
        "var y;model;y=0;end;\n@#for k in 1:2\nsteady_state_model;y=missing@{k};end;\n@#endfor\n";
    official(source, false, Some("variable 'missing1' is undefined"));
    let rows = owned(source);
    assert_eq!(
        rows.iter().filter(|row| row.code == "E130").count(),
        2,
        "{rows:?}"
    );
}

#[test]
fn recorded_special_roles_can_be_retyped_before_steady_state_use() {
    for (declaration, name) in [
        ("model_local_variable loc;", "loc"),
        ("external_function(name=foo,nargs=1);", "foo"),
        ("steady_state_model;tmp=1;y=0;end;", "tmp"),
        ("epilogue;epi=y;end;", "epi"),
    ] {
        for body in [format!("{name}=1;y={name};"), format!("[y,{name}]=bar(1);")] {
            let source = format!(
                "{PREFIX}{declaration}change_type(parameters) {name};steady_state_model;{body}end;"
            );
            official(&source, true, None);
            let model = parse(&source);
            assert_eq!(
                model.final_symbol_kind(model.intern.lookup(name).unwrap()),
                Some("parameters")
            );
            assert!(owned(&source).is_empty(), "{source}: {:?}", owned(&source));
            assert!(
                !analyze(&model)
                    .iter()
                    .any(|row| matches!(row.code.as_str(), "E295" | "E296")),
                "{source}"
            );
        }
    }
    let source = "var y;model;y=0;end;\n@#for k in 1:2\n@#if k == 2\nchange_type(parameters) loc;\n@#endif\n@#if k == 1\nmodel_local_variable loc;\n@#endif\n@#endfor\nsteady_state_model;loc=1;y=loc;end;";
    official(source, true, None);
    let rows = analyze(&parse(source));
    assert!(
        !rows
            .iter()
            .any(|row| matches!(row.code.as_str(), "E130" | "E481" | "E295")),
        "{rows:?}"
    );
    for (declaration, name) in [
        ("model_local_variable loc;", "loc"),
        ("external_function(name=foo,nargs=1);", "foo"),
    ] {
        let source = format!("{PREFIX}{declaration}\nnative={name};\nchange_type(parameters) {name};steady_state_model;{name}=1;y={name};end;");
        official(&source, true, None);
        assert!(owned(&source).is_empty(), "{source}: {:?}", owned(&source));
        assert!(
            !analyze(&parse(&source))
                .iter()
                .any(|row| row.code == "E296"),
            "{source}"
        );
    }
}

#[test]
fn pound_history_does_not_register_rejected_shadows_or_earlier_reads() {
    for (source, name, expected) in [
        (
            "var y;parameters p;model;#loc=p;y=loc;end;",
            "loc",
            Some("model_local_variable"),
        ),
        (
            "var y;parameters p;model;#p=1;y=p;end;",
            "p",
            Some("parameters"),
        ),
        (
            "var y gone;var_remove gone;model;#gone=1;y=gone;end;",
            "gone",
            Some("excluded"),
        ),
        (
            "parameters p;p=borrowed;var y;model;#borrowed=1;y=borrowed;end;",
            "borrowed",
            Some("mod_file_local"),
        ),
        ("var y;model;y=loc;#loc=1;end;", "loc", None),
        ("var y;model;#loc=loc;y=0;end;", "loc", None),
    ] {
        let model = parse(source);
        assert_eq!(
            model.final_symbol_kind(model.intern.lookup(name).unwrap()),
            expected,
            "{source}"
        );
        official(source, name == "loc" && expected.is_some(), None);
    }
    let duplicate = "var y;model;#loc=1;#loc=2;y=loc;end;";
    official(
        duplicate,
        false,
        Some("Local model variable loc declared twice."),
    );
    let model = parse(duplicate);
    let loc = model.intern.lookup("loc").unwrap();
    assert_eq!(
        model
            .symbol_type_events
            .iter()
            .filter(|event| event.name == loc && !event.changed)
            .count(),
        1
    );
    assert!(analyze(&model).iter().any(|row| row.code == "E030"));
    let source = "var y;parameters p;model;#loc=p;y=loc;end;change_type(parameters) loc;";
    official(
        source,
        false,
        Some("You cannot modify the type of symbol loc after having used it in an expression"),
    );
    assert!(analyze(&parse(source)).iter().any(|row| row.code == "E296"));
    let source = "var y;model;y=0;end;change_type(parameters) missing;";
    official(source, false, Some("Unknown variable missing"));
    assert!(analyze(&parse(source)).iter().any(|row| row.code == "E295"));
}

#[test]
fn nonfinite_constants_keep_number_identity_without_implicit_declarations() {
    for constant in ["NaN", "nan", "Inf", "inf", "-Inf", "+Inf"] {
        let source = format!("{PREFIX}steady_state_model;y={constant};end;");
        official(&source, true, None);
        let model = parse(&source);
        assert!(model.mod_file_locals.is_empty(), "{source}");
        assert!(owned(&source).is_empty(), "{source}: {:?}", owned(&source));
    }
}

#[test]
fn ordinary_retypes_have_shared_check_counts_and_first_written_metadata() {
    for (declaration, name) in [
        ("model_local_variable z;", "z"),
        ("external_function(name=z,nargs=1);", "z"),
        ("steady_state_model;z=1;y=0;end;", "z"),
        ("epilogue;z=1;end;", "z"),
    ] {
        for kind in ["var", "parameters", "varexo", "varexo_det"] {
            let model_body = if kind == "var" {
                "y=z;z=0;"
            } else if kind == "parameters" {
                "y=0;"
            } else {
                "y=z;"
            };
            let ss_body = if matches!(kind, "var" | "parameters") {
                "z=1;y=0;"
            } else {
                "y=0;"
            };
            let source = format!("var y;{declaration}change_type({kind}) {name};model;{model_body}end;steady_state_model;{ss_body}end;");
            official(&source, true, None);
            let model = parse(&source);
            let info = dygnosis::model_info::model_info_json(&model);
            let field = if kind == "var" {
                "endogenous"
            } else if kind == "parameters" {
                "parameters"
            } else {
                "exogenous"
            };
            assert!(
                info[field]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|value| value == name),
                "{source}: {info}"
            );
            assert_eq!(
                info["n_endogenous"],
                if kind == "var" { 2 } else { 1 },
                "{source}: {info}"
            );
            assert_eq!(
                info["n_parameters"],
                if kind == "parameters" { 1 } else { 0 },
                "{source}: {info}"
            );
            assert_eq!(
                info["n_exogenous"],
                if matches!(kind, "varexo" | "varexo_det") {
                    1
                } else {
                    0
                },
                "{source}: {info}"
            );
            assert_eq!(model.retyped_trend_decls.len(), 1);
            let declaration_view = &model.retyped_trend_decls[0];
            let first = model
                .symbol_type_events
                .iter()
                .find(|event| model.name(event.name) == name && !event.changed)
                .unwrap();
            assert_eq!(declaration_view.span, first.span);
            assert!(declaration_view.parse_order < model.change_type_statements[0].parse_order);
            assert!(owned(&source).is_empty(), "{source}: {:?}", owned(&source));
        }
    }
}

#[test]
fn retyped_implicit_metadata_navigation_uses_include_and_macro_occurrences() {
    let root = "@#include \"part.inc\"\n";
    let before = "var y;\n@#for k in 1:2\nsteady_state_model;tmp@{k}=1;y=0;end;\n@#endfor\nchange_type(parameters) tmp1 tmp2;model;y=tmp1+tmp2;end;";
    let after = before
        .replace("change_type(parameters)", "change_type(var)")
        .replace(
            "model;y=tmp1+tmp2;end;",
            "model;y=tmp1+tmp2;tmp1=0;tmp2=0;end;",
        );
    official(before, true, None);
    official(&after, true, None);
    let files_a = HashMap::from([
        ("before/root.mod".into(), root.into()),
        ("before/part.inc".into(), before.into()),
    ]);
    let files_b = HashMap::from([
        ("after/root.mod".into(), root.into()),
        ("after/part.inc".into(), after),
    ]);
    let diff = dygnosis::dynare_compare_models(
        root,
        root,
        Some("before/root.mod"),
        Some("after/root.mod"),
        Some(&files_a),
        Some(&files_b),
        None,
    );
    let mut occurrence_ids = std::collections::HashSet::new();
    for name in ["tmp1", "tmp2"] {
        let index = diff["symbols_changed"]
            .as_array()
            .unwrap()
            .iter()
            .position(|row| row["name"] == name)
            .unwrap();
        let id = format!("/symbols_changed/{index}");
        let row = diff["navigation"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        assert!(
            occurrence_ids.insert(row["before"]["occurrence_id"].as_str().unwrap()),
            "{row}"
        );
        for (side, file) in [("before", "before/part.inc"), ("after", "after/part.inc")] {
            assert_eq!(row[side]["written_locations"][0]["file"], file, "{row}");
            assert_eq!(row[side]["written_locations"][0]["line"], 3, "{row}");
            assert_eq!(row[side]["written_locations"][0]["column"], 20, "{row}");
        }
    }
}

#[test]
fn mcp_retains_exact_output_and_rhs_names_in_included_source() {
    let root = "C:/ssm-roles/model.mod";
    let include = "C:/ssm-roles/ss.inc";
    for (declarations, body, code, name, sentence) in [
        (
            "varexo e;var y;model;y=e;end;",
            "[y,e]=foo(1);",
            "E481",
            "e",
            "e has incorrect type",
        ),
        (
            PREFIX,
            "[y,tmp]=foo(missing);",
            "E130",
            "missing",
            "variable 'missing' is undefined in the declaration of variable 'y'",
        ),
    ] {
        let source = format!("{declarations}\n@#include \"ss.inc\"\n");
        let included = format!("steady_state_model; /* 🚀 */ {body} end;");
        let files = HashMap::from([
            (root.into(), source.clone()),
            (include.into(), included.clone()),
        ]);
        let rows = dynare_diagnose(&source, Some(root), Some(&files));
        let row = rows.iter().find(|row| row.code == code).unwrap();
        assert_eq!(row.message, sentence);
        assert_eq!(row.file.as_deref(), Some(include));
        let byte = included.find(body).unwrap() + body.find(name).unwrap();
        let column = included[..byte].chars().count() as u32 + 1;
        assert_eq!(
            (row.line, row.column, row.end_line, row.end_column),
            (1, column, 1, column + name.len() as u32)
        );
    }
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
async fn lsp_uses_utf16_name_ranges_and_clears_each_family_after_edit() {
    for (declarations, body, replacement, code, name) in [
        (
            "varexo e;var y;model;y=e;end;",
            "[y,e]=foo(1);",
            "[y,tmp]=foo(1);",
            "E481",
            "e",
        ),
        (PREFIX, "y=missing;", "y=1;", "E130", "missing"),
    ] {
        let (service, _socket) = new_service();
        let server = service.inner();
        let uri = Url::parse("file:///C:/ssm-roles/live.mod").unwrap();
        let source = format!("{declarations}\r\nsteady_state_model; /* 🚀 */ {body} end;");
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "dynare".into(),
                    version: 1,
                    text: source.clone(),
                },
            })
            .await;
        let rows = pull(server, &uri).await;
        let row = rows
            .iter()
            .find(|row| row.code == Some(NumberOrString::String(code.into())))
            .unwrap();
        let line = source.lines().nth(1).unwrap();
        let byte = line.find(body).unwrap() + body.find(name).unwrap();
        let column = line[..byte].encode_utf16().count() as u32;
        assert_eq!(
            row.range,
            Range::new(
                Position::new(1, column),
                Position::new(1, column + name.len() as u32)
            )
        );
        server
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 2,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: source.replace(body, replacement),
                }],
            })
            .await;
        let rows = pull(server, &uri).await;
        assert!(
            !rows
                .iter()
                .any(|row| row.code == Some(NumberOrString::String(code.into()))),
            "{rows:?}"
        );
    }
}
