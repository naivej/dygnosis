use dygnosis::{analyze, find_preprocessor, parse, run_preprocessor, JsonStage};
use std::time::Duration;

#[test]
fn unused_trend_names_can_become_each_ordinary_type() {
    for (declaration, log_trend) in [
        ("trend_var(growth_factor=1.01)", false),
        ("log_trend_var(log_growth_factor=0.01)", true),
    ] {
        for kind in ["var", "varexo", "varexo_det", "parameters"] {
            let equation = if kind == "var" { "A=0;" } else { "" };
            let source = format!(
                "var y; {declaration} A; change_type({kind}) A; model; y=A; {equation} end;"
            );
            let model = parse(&source);
            let diagnostics = analyze(&model);
            assert!(
                !diagnostics
                    .iter()
                    .any(|row| row.severity == dygnosis::Severity::Error),
                "{source}: {diagnostics:?}"
            );
            let name = model.trend_vars[0].name;
            assert_eq!(model.final_symbol_kind(name), Some(kind));
            assert_eq!(model.trend_vars[0].log_trend, log_trend);
            assert_eq!(model.endogenous.len(), 1);
            assert!(model.parameters.is_empty());
            assert_eq!(
                model.final_endogenous().len(),
                if kind == "var" { 2 } else { 1 }
            );
            assert_eq!(
                model.final_parameters().len(),
                usize::from(kind == "parameters")
            );
            let info = dygnosis::dynare_model_info(&source, None, None);
            assert_eq!(info["n_endogenous"], if kind == "var" { 2 } else { 1 });
            assert_eq!(info["n_parameters"], usize::from(kind == "parameters"));
            assert_eq!(info["n_exogenous"], usize::from(kind.starts_with("varexo")));
            if let Some(pp) = find_preprocessor(None) {
                for stage in [JsonStage::Check, JsonStage::Transform] {
                    let official =
                        run_preprocessor(&source, &pp, None, Duration::from_secs(30), stage);
                    assert!(official.success, "{source}: {official:?}");
                }
            }
        }
    }
}

#[test]
fn retyped_trends_reach_lists_and_unused_exogenous_checks() {
    for (kind, command) in [
        ("var", "rplot"),
        ("parameters", "osr_params"),
        ("varexo", "shocks"),
    ] {
        let source = if command == "shocks" {
            format!("var y; trend_var(growth_factor=1.01) A; change_type({kind}) A; model; y=A; end; shocks; var A; stderr 1; end;")
        } else {
            format!("var y; trend_var(growth_factor=1.01) A; change_type({kind}) A; model; y=A; {} end; {command} A;", if kind == "var" { "A=0;" } else { "" })
        };
        assert!(
            !analyze(&parse(&source))
                .iter()
                .any(|row| row.severity == dygnosis::Severity::Error),
            "{source}: {:?}",
            analyze(&parse(&source))
        );
    }
    let source = "var y; trend_var(growth_factor=1.01) A; change_type(varexo) A; model; y=0; end;";
    let diagnostics = analyze(&parse(source));
    let refusal = diagnostics
        .iter()
        .find(|row| row.code == "E021")
        .expect("unused final exogenous refuses");
    if let Some(pp) = find_preprocessor(None) {
        let official =
            run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(!official.success, "{official:?}");
        assert!(
            official.raw_stdout.contains(&refusal.message),
            "{official:?}"
        );
    }
}

#[test]
fn ordinary_trend_roles_reach_existing_statement_readers() {
    for declaration in [
        "trend_var(growth_factor=1.01)",
        "log_trend_var(log_growth_factor=0.01)",
    ] {
        for suffix in [
            "change_type(parameters) A; A=.5; p=A; var y; model; y=p; end; A.prior(shape=normal,mean=.5,stdev=.1);",
            "change_type(varexo) A; var y; model; y=A; end; std(A).prior(shape=normal,mean=0,stdev=1);",
            "change_type(var) A; var y; predetermined_variables A; model; y=A; A=0; end; initval; A=0; y=0; end;",
            "change_type(var) A; var y; model; y=A; A=.5*A(-1); end; filter_initial_state; A(0)=0; end;",
            "change_type(parameters) A; A=.9; var y; model; [name='Y'] y=0; end; var_model(model_name=v,eqtags=['Y']); var_expectation_model(model_name=a,variable=y,auxiliary_model_name=v,horizon=1,discount=A);",
        ] {
            let source = format!("parameters p; {declaration} A; {suffix}");
            let model = parse(&source);
            let diagnostics = analyze(&model);
            assert!(!diagnostics.iter().any(|row| row.severity == dygnosis::Severity::Error), "{source}: {diagnostics:?}");
            if suffix.starts_with("change_type(parameters)") {
                assert!(model.param_assignments.iter().any(|row| model.name(row.name) == "A" && !row.native));
                assert!(!diagnostics.iter().any(|row| row.code == "W010" && row.message.contains("'A'")), "{diagnostics:?}");
            }
            if let Some(pp) = find_preprocessor(None) {
                let official = run_preprocessor(&source, &pp, None, Duration::from_secs(30), JsonStage::Check);
                assert!(official.success, "{source}: {official:?}");
            }
        }
    }
}

#[tokio::test]
async fn retyped_trend_hover_and_definition_keep_the_written_location() {
    use dygnosis::server::new_service;
    use tower_lsp::{lsp_types::*, LanguageServer};
    let source = "trend_var(growth_factor=1.01) A;\nchange_type(var) A;\nmodel; A=0; end;";
    let uri = Url::parse("file:///trend_retype.mod").unwrap();
    let (service, _) = new_service();
    service
        .inner()
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version: 1,
                text: source.into(),
            },
        })
        .await;
    let position = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri },
        position: Position::new(2, 7),
    };
    let hover = service
        .inner()
        .hover(HoverParams {
            text_document_position_params: position.clone(),
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    let HoverContents::Markup(markdown) = hover.contents else {
        panic!("Markdown hover");
    };
    assert!(
        markdown.value.contains("**Endogenous variable**"),
        "{}",
        markdown.value
    );
    let definition = service
        .inner()
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: position,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    let GotoDefinitionResponse::Scalar(location) = definition else {
        panic!("declaration location");
    };
    assert_eq!(location.range.start, Position::new(0, 30));
}

#[test]
fn trend_targets_keep_their_type_at_each_entry() {
    for block in [
        "initval; A=0; end;",
        "endval; A=0; end;",
        "histval; A(-1)=0; end;",
    ] {
        for (directives, refuses) in [
            (format!("{block} change_type(var) A;"), true),
            (
                format!("change_type(var) A; {block} change_type(parameters) A;"),
                false,
            ),
        ] {
            let source =
                format!("var y; trend_var(growth_factor=1.01) A; {directives} model; y=0; end;");
            let diagnostics = analyze(&parse(&source));
            assert_eq!(
                diagnostics.iter().any(|row| row.code == "E059"),
                refuses,
                "{source}: {diagnostics:?}"
            );
            if let Some(pp) = find_preprocessor(None) {
                let official = run_preprocessor(
                    &source,
                    &pp,
                    None,
                    Duration::from_secs(30),
                    JsonStage::Check,
                );
                assert_eq!(official.success, !refuses, "{source}: {official:?}");
                if refuses {
                    assert!(
                        official
                            .raw_stdout
                            .contains("A is neither endogenous or exogenous."),
                        "{official:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn future_trend_declarations_cannot_validate_earlier_targets() {
    for block in [
        "initval; A=0; end;",
        "endval; A=0; end;",
        "histval; A(-1)=0; end;",
    ] {
        for retype in ["", "change_type(parameters) A;"] {
            let source = format!(
                "var y; model; y=0; end; {block} trend_var(growth_factor=1.01) A; {retype}"
            );
            let diagnostics = analyze(&parse(&source));
            assert!(
                diagnostics.iter().any(|row| row.code == "E058"),
                "{source}: {diagnostics:?}"
            );
            assert!(
                !diagnostics.iter().any(|row| row.code == "E059"),
                "{source}: {diagnostics:?}"
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
                    official.raw_stdout.contains("Unknown symbol: A."),
                    "{official:?}"
                );
            }
        }
    }
}

#[test]
fn backward_macro_order_uses_the_captured_trend_type() {
    for (kind, target, body) in [
        ("varexo", "shocks; var A; periods 1; values 1; end;", "y=A;"),
        ("varexo", "shocks; var A; stderr 1; end;", "y=A;"),
        ("var", "initval; A=0; end;", "y=A; A=0;"),
        ("var", "predetermined_variables A;", "y=A; A=0;"),
        ("parameters", "A=.9;", "y=A;"),
    ] {
        let source = format!("trend_var(growth_factor=1.01) A;\n@#for j in 1:2\n@#if j==2\n{target}\n@#endif\n@#if j==1\nchange_type({kind}) A;\n@#endif\n@#endfor\nvar y; model; {body} end;");
        let diagnostics = analyze(&parse(&source));
        assert!(
            !diagnostics
                .iter()
                .any(|row| row.severity == dygnosis::Severity::Error),
            "{source}: {diagnostics:?}"
        );
        if let Some(pp) = find_preprocessor(None) {
            let official = run_preprocessor(
                &source,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            assert!(official.success, "{source}: {official:?}");
        }
    }
}

#[test]
fn already_used_trend_names_keep_the_official_refusal() {
    for declaration in [
        "trend_var(growth_factor=1.01)",
        "log_trend_var(log_growth_factor=0.01)",
    ] {
        let source =
            format!("{declaration} A; var(deflator=A) y; change_type(var) A; model; y=0; end;");
        let model = parse(&source);
        let diagnostics = analyze(&model);
        assert!(
            !diagnostics.iter().any(|row| row.code == "E295"),
            "{diagnostics:?}"
        );
        let refusal = diagnostics
            .iter()
            .find(|row| row.code == "E296")
            .expect("used trend refuses");
        if let Some(pp) = find_preprocessor(None) {
            let official = run_preprocessor(
                &source,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            assert!(!official.success, "{official:?}");
            assert!(
                official.raw_stdout.contains(&refusal.message),
                "{official:?}"
            );
        }
    }
}
