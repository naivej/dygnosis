use dygnosis::{analyze, find_preprocessor, parse, run_preprocessor, JsonStage};
use std::time::Duration;

fn instrument_errors(source: &str) -> Vec<dygnosis::Diagnostic> {
    analyze(&parse(source))
        .into_iter()
        .filter(|row| matches!(row.code.as_str(), "E101" | "E317"))
        .collect()
}

#[test]
fn model_locals_and_the_policy_created_parameter_are_declared_wrong_types() {
    let pp = find_preprocessor(None);
    for command in ["ramsey_model", "ramsey_policy", "discretionary_policy"] {
        for (decl, local, name, expected) in [
            ("", "#pol=1;", "pol", Some("E317")),
            ("", "", "optimal_policy_discount_factor", Some("E317")),
            (
                "var optimal_policy_discount_factor;",
                "",
                "optimal_policy_discount_factor",
                None,
            ),
        ] {
            let source = format!("var y; {decl} model(linear); {local} y=0; end; planner_objective y^2; {command}(instruments=({name}));");
            let errors = instrument_errors(&source);
            if let Some(code) = expected {
                assert_eq!(errors.len(), 1, "{source}: {errors:?}");
                assert_eq!(errors[0].code, code);
                assert_eq!(errors[0].message, format!("{name} is not endogenous."));
            } else {
                assert!(errors.is_empty(), "{source}: {errors:?}");
            }
            if let Some(pp) = &pp {
                let official =
                    run_preprocessor(&source, pp, None, Duration::from_secs(30), JsonStage::Check);
                if command == "discretionary_policy" && expected.is_none() {
                    // This command initializes the discount parameter before instrument checks.
                    assert!(
                        official
                            .raw_stdout
                            .contains("optimal_policy_discount_factor is not a parameter"),
                        "{official:?}"
                    );
                } else {
                    assert_eq!(
                        official.success,
                        expected.is_none(),
                        "{source}: {official:?}"
                    );
                }
                if expected.is_some() {
                    assert!(
                        official
                            .raw_stdout
                            .contains(&format!("{name} is not endogenous.")),
                        "{official:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn policy_instruments_use_the_type_when_each_command_is_parsed() {
    let pp = find_preprocessor(None);
    for command in ["ramsey_model", "ramsey_policy", "discretionary_policy"] {
        for (decl, before, after, expected) in [
            ("parameters pol;", "", "", Some("E317")),
            ("varexo pol;", "", "", Some("E317")),
            ("varexo_det pol;", "", "", Some("E317")),
            ("var pol;", "", "", None),
            (
                "heterogeneity_dimension h; var(heterogeneity=h) pol;",
                "",
                "",
                Some("E317"),
            ),
            ("", "", "", Some("E101")),
            ("", "", "var pol;", Some("E101")),
            ("parameters pol;", "change_type(var) pol;", "", None),
            ("parameters pol;", "", "change_type(var) pol;", Some("E317")),
            ("var pol;", "change_type(parameters) pol;", "", Some("E317")),
            ("var pol;", "", "change_type(parameters) pol;", None),
        ] {
            let source = format!("var y; {decl} model(linear); y=0; end; planner_objective y^2; {before} {command}(instruments=(pol)); {after}");
            let errors = instrument_errors(&source);
            if let Some(code) = expected {
                assert_eq!(errors.len(), 1, "{source}: {errors:?}");
                assert_eq!(errors[0].code, code, "{source}");
                if code == "E317" {
                    assert_eq!(errors[0].message, "pol is not endogenous.");
                }
            } else {
                assert!(errors.is_empty(), "{source}: {errors:?}");
            }
            if let Some(pp) = &pp {
                let official =
                    run_preprocessor(&source, pp, None, Duration::from_secs(30), JsonStage::Check);
                assert_eq!(
                    official.success,
                    expected.is_none(),
                    "{source}: {official:?}"
                );
                if let Some(code) = expected {
                    let needle = if code == "E101" {
                        "Unknown symbol: pol"
                    } else {
                        "pol is not endogenous."
                    };
                    assert!(
                        official.raw_stdout.contains(needle),
                        "{source}: {official:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn heterogeneous_model_locals_are_known_only_after_their_block() {
    let pp = find_preprocessor(None);
    let block = "model(heterogeneity=h); #pol=1; x=pol; end;";
    for command in ["ramsey_model", "ramsey_policy", "discretionary_policy"] {
        for earlier in [true, false] {
            let source = format!("heterogeneity_dimension h; var y; var(heterogeneity=h) x; model(linear); y=0; end; {} planner_objective y^2; {command}(instruments=(pol)); {}", if earlier { block } else { "" }, if earlier { "" } else { block });
            let errors = instrument_errors(&source);
            let code = if earlier { "E317" } else { "E101" };
            assert_eq!(errors.len(), 1, "{errors:?}");
            assert_eq!(errors[0].code, code);
            if let Some(pp) = &pp {
                let official =
                    run_preprocessor(&source, pp, None, Duration::from_secs(30), JsonStage::Check);
                assert!(!official.success, "{official:?}");
                assert!(
                    official.raw_stdout.contains(if earlier {
                        "pol is not endogenous."
                    } else {
                        "Unknown symbol: pol"
                    }),
                    "{official:?}"
                );
            }
        }
    }
}

#[test]
fn retyped_heterogeneous_instrument_passes_the_type_gate_before_the_policy_limit() {
    let source = "heterogeneity_dimension h; var y; var(heterogeneity=h) pol; change_type(var) pol; model(linear); y=0; end; planner_objective y^2; ramsey_model(instruments=(pol));";
    assert!(instrument_errors(source).is_empty());
    if let Some(pp) = find_preprocessor(None) {
        let official =
            run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(!official.success, "{official:?}");
        assert!(
            official
                .raw_stdout
                .contains("not supported for heterogeneous models"),
            "{official:?}"
        );
        assert!(
            !official.raw_stdout.contains("pol is not endogenous."),
            "{official:?}"
        );
    }
}

#[test]
fn instrument_diagnostics_match_lsp_and_mcp_messages_and_ranges() {
    for (decl, code) in [("parameters pol;", "E317"), ("", "E101")] {
        let source = format!("var y; {decl}\nmodel; y=0; end;\nplanner_objective y^2;\nramsey_model(instruments=(pol));");
        let mcp = dygnosis::dynare_diagnose(&source, None, None);
        let mcp = mcp.iter().find(|row| row.code == code).unwrap();
        let lsp = dygnosis::server::diagnostics_for("file:///instrument_types.mod", &source);
        let lsp = lsp
            .iter()
            .find(|row| row.code == Some(tower_lsp::lsp_types::NumberOrString::String(code.into())))
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
}

#[test]
fn macro_iterations_with_the_same_span_keep_distinct_instrument_types() {
    let source = "var y pol; model(linear); y=0; end; planner_objective y^2;\n@#for j in 1:2\ndiscretionary_policy(instruments=(pol));\nchange_type(parameters) pol;\n@#endfor\n";
    let errors = instrument_errors(source);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code, "E317");
    assert_eq!(errors[0].message, "pol is not endogenous.");
    if let Some(pp) = find_preprocessor(None) {
        let official =
            run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(!official.success, "{official:?}");
        assert!(
            official.raw_stdout.contains("pol is not endogenous."),
            "{official:?}"
        );
    }
}
