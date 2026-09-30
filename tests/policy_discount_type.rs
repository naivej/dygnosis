use dygnosis::{analyze, find_preprocessor, parse, run_preprocessor, JsonStage};
use std::time::Duration;

#[test]
fn discretionary_discount_parameter_uses_its_type_at_the_command() {
    let pp = find_preprocessor(None);
    for (decl, before, after, refuses) in [
        ("var optimal_policy_discount_factor;", "", "", true),
        ("varexo optimal_policy_discount_factor;", "", "", true),
        ("varexo_det optimal_policy_discount_factor;", "", "", true),
        ("parameters optimal_policy_discount_factor;", "", "", false),
        ("", "", "", false),
        (
            "var optimal_policy_discount_factor;",
            "change_type(parameters) optimal_policy_discount_factor;",
            "",
            false,
        ),
        (
            "var optimal_policy_discount_factor;",
            "",
            "change_type(parameters) optimal_policy_discount_factor;",
            true,
        ),
        (
            "parameters optimal_policy_discount_factor;",
            "change_type(var) optimal_policy_discount_factor;",
            "",
            true,
        ),
        (
            "parameters optimal_policy_discount_factor;",
            "",
            "change_type(var) optimal_policy_discount_factor;",
            false,
        ),
    ] {
        let source = format!("var y pol; {decl} model(linear); y=0; end; planner_objective y^2; {before} discretionary_policy(instruments=(pol)); {after}");
        let diagnostics = analyze(&parse(&source));
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|row| row.code == "E378")
            .collect();
        assert_eq!(
            errors.len(),
            usize::from(refuses),
            "{source}: {diagnostics:?}"
        );
        if refuses {
            assert_eq!(
                errors[0].message,
                "optimal_policy_discount_factor is not a parameter"
            );
        }
        if let Some(pp) = &pp {
            let official =
                run_preprocessor(&source, pp, None, Duration::from_secs(30), JsonStage::Check);
            assert_eq!(official.success, !refuses, "{source}: {official:?}");
            if refuses {
                assert!(
                    official
                        .raw_stdout
                        .contains("optimal_policy_discount_factor is not a parameter"),
                    "{official:?}"
                );
            }
        }
    }
}

#[test]
fn discount_initialization_refusal_precedes_invalid_instruments() {
    let source = "var y optimal_policy_discount_factor; model(linear); y=0; end; planner_objective y^2; discretionary_policy(instruments=(missing));";
    let diagnostics = analyze(&parse(source));
    assert_eq!(
        diagnostics.iter().filter(|row| row.code == "E378").count(),
        1,
        "{diagnostics:?}"
    );
    assert!(
        !diagnostics
            .iter()
            .any(|row| matches!(row.code.as_str(), "E101" | "E317")),
        "{diagnostics:?}"
    );
    if let Some(pp) = find_preprocessor(None) {
        let official =
            run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(!official.success, "{official:?}");
        assert!(
            official
                .raw_stdout
                .contains("optimal_policy_discount_factor is not a parameter"),
            "{official:?}"
        );
        assert!(
            !official.raw_stdout.contains("Unknown symbol: missing"),
            "{official:?}"
        );
    }
}

#[test]
fn repeated_policy_spans_keep_distinct_discount_types() {
    let source = "var y pol; parameters optimal_policy_discount_factor; model(linear); y=0; end; planner_objective y^2;\n@#for j in 1:2\ndiscretionary_policy(instruments=(pol));\nchange_type(var) optimal_policy_discount_factor;\n@#endfor";
    let diagnostics = analyze(&parse(source));
    assert_eq!(
        diagnostics.iter().filter(|row| row.code == "E378").count(),
        1,
        "{diagnostics:?}"
    );
    if let Some(pp) = find_preprocessor(None) {
        let official =
            run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(!official.success, "{official:?}");
        assert!(
            official
                .raw_stdout
                .contains("optimal_policy_discount_factor is not a parameter"),
            "{official:?}"
        );
    }
}

#[test]
fn policy_option_expressions_register_native_locals_before_instrument_validation() {
    let pp = find_preprocessor(None);
    for command in ["ramsey_model", "ramsey_policy", "discretionary_policy"] {
        for (prefix, options) in [
            ("parameters p; p=pol;", "instruments=(pol)"),
            ("", "instruments=(pol),planner_discount=pol"),
            ("", "planner_discount=pol,instruments=(pol)"),
        ] {
            let source = format!("var y; {prefix} model(linear); y=0; end; planner_objective y^2; {command}({options});");
            let diagnostics = analyze(&parse(&source));
            let errors: Vec<_> = diagnostics
                .iter()
                .filter(|row| matches!(row.code.as_str(), "E101" | "E317"))
                .collect();
            assert_eq!(errors.len(), 1, "{source}: {diagnostics:?}");
            assert_eq!(errors[0].code, "E317");
            assert_eq!(errors[0].message, "pol is not endogenous.");
            if let Some(pp) = &pp {
                let official =
                    run_preprocessor(&source, pp, None, Duration::from_secs(30), JsonStage::Check);
                assert!(!official.success, "{official:?}");
                assert!(
                    official.raw_stdout.contains("pol is not endogenous."),
                    "{official:?}"
                );
            }
        }
    }
}

#[test]
fn later_policy_expressions_know_the_generated_discount_parameter() {
    for earlier_command in ["ramsey_model", "ramsey_policy", "discretionary_policy"] {
        let source = format!("var y pol; model(linear); y=0; end; planner_objective y^2; {earlier_command}(instruments=(pol)); discretionary_policy(instruments=(pol),planner_discount=optimal_policy_discount_factor);");
        let diagnostics = analyze(&parse(&source));
        assert!(
            !diagnostics.iter().any(|row| row.code == "E378"),
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
            // Mixing Ramsey and discretionary commands refuses later at Check,
            // after the valid parameter initialization during parsing.
            assert_eq!(
                official.success,
                earlier_command == "discretionary_policy",
                "{official:?}"
            );
            assert!(
                !official.raw_stdout.contains("is not a parameter"),
                "{official:?}"
            );
        }
    }
    let source = "var y pol; parameters p; p=1; model(linear); y=p; end; planner_objective y^2; osr_params p; osr; discretionary_policy(instruments=(pol),planner_discount=optimal_policy_discount_factor);";
    let diagnostics = analyze(&parse(source));
    assert!(
        !diagnostics.iter().any(|row| row.code == "E378"),
        "{diagnostics:?}"
    );
    if let Some(pp) = find_preprocessor(None) {
        let official =
            run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(official.success, "{official:?}");
    }
}

#[test]
fn discount_parameter_diagnostics_agree_through_lsp_and_mcp() {
    let source = "var y pol optimal_policy_discount_factor;\nmodel(linear); y=0; end;\nplanner_objective y^2;\ndiscretionary_policy(instruments=(pol));";
    let mcp = dygnosis::dynare_diagnose(source, None, None);
    let mcp = mcp.iter().find(|row| row.code == "E378").unwrap();
    let lsp = dygnosis::server::diagnostics_for("file:///policy_discount_type.mod", source);
    let lsp = lsp
        .iter()
        .find(|row| row.code == Some(tower_lsp::lsp_types::NumberOrString::String("E378".into())))
        .unwrap();
    assert_eq!(mcp.message, lsp.message);
    assert_eq!(mcp.severity, "ERROR");
    assert_eq!(
        lsp.severity,
        Some(tower_lsp::lsp_types::DiagnosticSeverity::ERROR)
    );
    assert_eq!(
        (mcp.line, mcp.column, mcp.end_line, mcp.end_column),
        (
            lsp.range.start.line + 1,
            lsp.range.start.character + 1,
            lsp.range.end.line + 1,
            lsp.range.end.character + 1
        )
    );
}
