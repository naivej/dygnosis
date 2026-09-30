use dygnosis::{analyze, find_preprocessor, parse, run_preprocessor, JsonStage};
use std::time::Duration;

#[test]
fn non_model_calls_are_functions_and_later_declarations_keep_their_kind() {
    for (suffix, expected, needle) in [
        (
            "external_function(name=foo,nargs=1);",
            "W031",
            "Symbol foo declared twice.",
        ),
        (
            "var foo;",
            "E030",
            "Symbol foo declared twice with different types!",
        ),
        (
            "forecast foo;",
            "E240",
            "forecast: Variable foo is not one of {endogenous}",
        ),
        ("", "", ""),
    ] {
        let source = format!("parameters p; p=foo(1); {suffix} var y; model; y=0; end;");
        let model = parse(&source);
        let diagnostics = analyze(&model);
        assert!(
            !diagnostics
                .iter()
                .any(|row| matches!(row.code.as_str(), "E279" | "E239")),
            "{source}: {diagnostics:?}"
        );
        assert!(matches!(
            model
                .exprs
                .get(model.param_assignments[0].expr.unwrap())
                .kind,
            dygnosis::expr::ExprKind::Call { .. }
        ));
        if !expected.is_empty() {
            assert!(
                diagnostics
                    .iter()
                    .any(|row| row.code == expected && row.message == needle),
                "{source}: {diagnostics:?}"
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
            assert_eq!(
                official.success,
                matches!(expected, "" | "W031"),
                "{source}: {official:?}"
            );
            if !needle.is_empty() {
                assert!(
                    official.raw_stdout.contains(needle),
                    "{source}: {official:?}"
                );
            }
        }
    }
}

#[test]
fn declared_calls_repeated_calls_and_native_text_keep_their_context() {
    for source in [
        "external_function(name=foo,nargs=1); parameters p; p=foo(1); var y; model; y=0; end;",
        "external_function(name=foo,nargs=1); var y; model; y=foo(1); end;",
        "parameters p; p=foo(1); p=foo(1,2); var y; model; y=0; end;",
        "xx=foo(1);\nvar y; model; y=0; end; forecast foo;",
        "xx=foo;\nexternal_function(name=foo,nargs=1); var y; model; y=0; end;",
    ] {
        let diagnostics = analyze(&parse(source));
        assert!(
            !diagnostics
                .iter()
                .any(|row| matches!(row.code.as_str(), "E279" | "E280" | "E030" | "W031")),
            "{source}: {diagnostics:?}"
        );
        let native = source.contains("forecast foo;");
        assert_eq!(
            diagnostics.iter().any(|row| row.code == "E239"),
            native,
            "{diagnostics:?}"
        );
        if let Some(pp) = find_preprocessor(None) {
            let official =
                run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
            assert_eq!(official.success, !native, "{official:?}");
            assert!(
                !official.raw_stdout.contains("declared twice"),
                "{official:?}"
            );
        }
    }
    let model = parse("var y; model; y=y(-1); end;");
    let rhs = model.exprs.get(model.equations[0].rhs_expr.unwrap());
    assert!(matches!(
        rhs.kind,
        dygnosis::expr::ExprKind::Ident { timing: -1, .. }
    ));
}

#[test]
fn implicit_functions_need_explicit_declarations_before_model_expressions() {
    for before in [false, true] {
        let declaration = "external_function(name=foo,nargs=1);";
        let source = format!(
            "parameters p; p=foo(1); var y; {} model; y=foo(1); end; {}",
            if before { declaration } else { "" },
            if before { "" } else { declaration }
        );
        let diagnostics = analyze(&parse(&source));
        assert_eq!(
            diagnostics
                .iter()
                .any(|row| row.code == "E001" && row.message.contains("Before using foo()")),
            !before,
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
            assert_eq!(official.success, before, "{official:?}");
            if !before {
                assert!(
                    official.raw_stdout.contains("Before using foo()"),
                    "{official:?}"
                );
            }
        }
    }
}

#[test]
fn epilogue_unknown_calls_refuse_without_implicit_registration() {
    for declared in [false, true] {
        for arg in ["1", "1.5"] {
            let source = format!(
                "{} var y; model; y=0; end; epilogue; z=foo({arg}); end;",
                if declared {
                    "external_function(name=foo,nargs=1);"
                } else {
                    ""
                }
            );
            let diagnostics = analyze(&parse(&source));
            assert_eq!(
                diagnostics.iter().any(|row| row.code == "E288"
                    && row.message
                        == "Variable foo used in the epilogue block but was not declared."),
                !declared,
                "{diagnostics:?}"
            );
            if let Some(pp) = find_preprocessor(None) {
                let official = run_preprocessor(
                    &source,
                    &pp,
                    None,
                    Duration::from_secs(30),
                    JsonStage::Check,
                );
                assert_eq!(official.success, declared, "{official:?}");
                if !declared {
                    assert!(
                        official.raw_stdout.contains(
                            "Variable foo used in the epilogue block but was not declared."
                        ),
                        "{official:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn expectation_discount_uses_non_model_function_context() {
    let source = "parameters p; p=foo(1); var y; model; [name='Y'] y=0; end; var_model(model_name=v,eqtags=['Y']); var_expectation_model(model_name=a,variable=y,auxiliary_model_name=v,horizon=1,discount=foo(1));";
    let model = parse(source);
    assert!(
        !model.shape_refuses.iter().any(|row| row
            .official_message
            .as_deref()
            .is_some_and(|message| message.contains("Before using"))),
        "{:?}",
        model.shape_refuses
    );
    let diagnostics = analyze(&model);
    assert!(
        !diagnostics
            .iter()
            .any(|row| row.code == "E001" && row.message.contains("Before using")),
        "{diagnostics:?}"
    );
    assert!(
        diagnostics.iter().any(|row| row.code == "E442"),
        "{diagnostics:?}"
    );
    if let Some(pp) = find_preprocessor(None) {
        let official =
            run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(
            official
                .raw_stdout
                .contains("The discount factor must be a constant expression or a parameter"),
            "{official:?}"
        );
        assert!(
            !official.raw_stdout.contains("Before using"),
            "{official:?}"
        );
    }
    let expression_source = source
        .replace("variable=y", "expression=foo(1)")
        .replace("discount=foo(1)", "discount=.9");
    let expression_model = parse(&expression_source);
    assert!(
        expression_model.shape_refuses.iter().any(|row| row
            .official_message
            .as_deref()
            .is_some_and(|message| message.contains("Before using"))),
        "{:?}",
        expression_model.shape_refuses
    );
    if let Some(pp) = find_preprocessor(None) {
        let official = run_preprocessor(
            &expression_source,
            &pp,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        assert!(
            official.raw_stdout.contains("Before using foo()"),
            "{official:?}"
        );
    }
}
