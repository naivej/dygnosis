use std::time::Duration;

use dygnosis::{analyze, find_preprocessor, parse, run_preprocessor, JsonStage};

/// Each pair changes only the type of z, before or after the checked statement.
/// Check-stage honesty is optional, like the rest of the installed-7.2 suite.
#[test]
fn statement_targets_use_the_type_when_read() {
    let mut failures = Vec::new();
    let pp = find_preprocessor(None);
    for (declaration, target_type, statement, code, sentence) in [
        (
            "parameters",
            "var",
            "initval; z=0; end;",
            "E059",
            "z is neither endogenous or exogenous.",
        ),
        (
            "parameters",
            "var",
            "endval; z=0; end;",
            "E059",
            "z is neither endogenous or exogenous.",
        ),
        (
            "parameters",
            "var",
            "histval; z(-1)=0; end;",
            "E059",
            "z is neither endogenous or exogenous.",
        ),
        (
            "parameters",
            "var",
            "filter_initial_state; z(0)=0; end;",
            "E311",
            "filter_initial_state: z should be an endogenous or exogenous variable",
        ),
        (
            "parameters",
            "var",
            "init2shocks; z e; end;",
            "E330",
            "init2shocks: z should be an endogenous variable",
        ),
        (
            "var",
            "parameters",
            "homotopy_setup; z,0,1; end;",
            "E332",
            "homotopy_val: z should be a parameter or exogenous variable",
        ),
        (
            "parameters",
            "varexo",
            "init2shocks; y z; end;",
            "E331",
            "init2shocks: z should be an exogenous variable",
        ),
        (
            "parameters",
            "varexo",
            "shock_groups; g=z; end;",
            "E333",
            "shock_groups: z should be an exogenous variable",
        ),
        (
            "parameters",
            "varexo",
            "shocks; var z; periods 1; values 1; end;",
            "E387",
            "z is not exogenous.",
        ),
        (
            "parameters",
            "var",
            "varobs z;",
            "E090",
            "z is not endogenous.",
        ),
        (
            "var",
            "parameters",
            "estimated_params; z, .5; end;",
            "E093",
            "z is not a parameter",
        ),
    ] {
        for before in [false, true] {
            let change = format!("change_type({target_type}) z;");
            // Give an endogenous z a lag so filter_initial_state reaches its type check.
            let equation = if target_type == "var" {
                "z=.5*z(-1);"
            } else {
                ""
            };
            let source = format!(
                "var y; varexo e; {declaration} z; {} model; y=z+e; {equation} end; {statement} {}",
                if before { &change } else { "" },
                if before { "" } else { &change }
            );
            let diagnostics = analyze(&parse(&source));
            if diagnostics.iter().any(|row| row.code == code) == before {
                failures.push(format!(
                    "{code}, before={before}: {source}\n{diagnostics:?}"
                ));
            }
            if let Some(pp) = &pp {
                let official =
                    run_preprocessor(&source, pp, None, Duration::from_secs(30), JsonStage::Check);
                assert_eq!(official.success, before, "{source}: {official:?}");
                if !before {
                    assert!(
                        official.raw_stdout.contains(sentence),
                        "{source}: {official:?}"
                    );
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn names_retyped_away_from_required_roles_are_refused() {
    for (declaration, target, statement, code, sentence) in [
        (
            "var",
            "parameters",
            "initval; z=0; end;",
            "E059",
            "z is neither endogenous or exogenous.",
        ),
        (
            "var",
            "parameters",
            "endval; z=0; end;",
            "E059",
            "z is neither endogenous or exogenous.",
        ),
        (
            "var",
            "parameters",
            "histval; z(-1)=0; end;",
            "E059",
            "z is neither endogenous or exogenous.",
        ),
        (
            "var",
            "parameters",
            "filter_initial_state; z(0)=0; end;",
            "E311",
            "filter_initial_state: z should be an endogenous or exogenous variable",
        ),
        (
            "var",
            "parameters",
            "init2shocks; z e; end;",
            "E330",
            "init2shocks: z should be an endogenous variable",
        ),
        (
            "parameters",
            "var",
            "homotopy_setup; z,0,1; end;",
            "E332",
            "homotopy_val: z should be a parameter or exogenous variable",
        ),
        (
            "varexo",
            "parameters",
            "init2shocks; y z; end;",
            "E331",
            "init2shocks: z should be an exogenous variable",
        ),
        (
            "varexo",
            "parameters",
            "shock_groups; g=z; end;",
            "E333",
            "shock_groups: z should be an exogenous variable",
        ),
        (
            "varexo",
            "parameters",
            "shocks; var z; periods 1; values 1; end;",
            "E387",
            "z is not exogenous.",
        ),
        (
            "var",
            "parameters",
            "varobs z;",
            "E090",
            "z is not endogenous.",
        ),
        (
            "parameters",
            "var",
            "estimated_params; z,.5; end;",
            "E093",
            "z is not a parameter",
        ),
    ] {
        let source = format!("var y; varexo e; {declaration} z; change_type({target}) z; {statement} model; y=z+e; {} end;", if target == "var" { "z=.5*z(-1);" } else { "" });
        let diagnostics = analyze(&parse(&source));
        assert!(
            diagnostics.iter().any(|row| row.code == code),
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
                official.raw_stdout.contains(sentence),
                "{source}: {official:?}"
            );
        }
    }
}

#[test]
fn skewness_uses_final_exogenous_types() {
    for (declaration, target, expected) in [("var", "varexo", false), ("varexo", "var", true)] {
        let source = format!("var y; {declaration} z; change_type({target}) z; model; y=z; {} end; estimated_params; skew z,0; end;", if target == "var" { "z=0;" } else { "" });
        let diagnostics = analyze(&parse(&source));
        assert_eq!(
            diagnostics.iter().any(|row| row.code == "E249"),
            expected,
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
            assert_eq!(official.success, !expected, "{source}: {official:?}");
            if expected {
                let refusal = diagnostics.iter().find(|row| row.code == "E249").unwrap();
                assert!(
                    official.raw_stdout.contains(&refusal.message),
                    "{official:?}"
                );
            }
        }
    }
}

#[test]
fn retyped_target_diagnostics_match_lsp_and_mcp() {
    let source = "var y z;\nchange_type(parameters) z;\ninitval; z=0; end;\nmodel; y=z; end;";
    let mcp = dygnosis::dynare_diagnose(source, None, None);
    let lsp = dygnosis::server::diagnostics_for("file:///parameter_consumers.mod", source);
    let a = mcp.iter().find(|row| row.code == "E059").unwrap();
    let b = lsp
        .iter()
        .find(|row| row.code == Some(tower_lsp::lsp_types::NumberOrString::String("E059".into())))
        .unwrap();
    assert_eq!(a.message, b.message);
    assert_eq!(a.line as u32 - 1, b.range.start.line);
    assert_eq!(a.column as u32 - 1, b.range.start.character);
}

#[test]
fn markov_parameter_option_uses_the_type_when_read() {
    for (declaration, target, before, expected) in [
        ("var", "parameters", true, false),
        ("parameters", "var", true, true),
        ("var", "parameters", false, true),
        ("parameters", "var", false, false),
    ] {
        let change = format!("change_type({target}) z;");
        let source = format!("var y; {declaration} z; {} markov_switching(chain=1,number_of_regimes=2,duration=3,parameters=[z]); {} model; y=z; {} end;", if before { &change } else { "" }, if before { "" } else { &change }, if target == "var" { "z=0;" } else { "" });
        let diagnostics = analyze(&parse(&source));
        assert_eq!(
            diagnostics.iter().any(|row| row.code == "E349"),
            expected,
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
            assert_eq!(official.success, !expected, "{source}: {official:?}");
            if expected {
                assert!(official.raw_stdout.contains("Variables passed to the parameters option of the markov_switching statement must be parameters."), "{official:?}");
            }
        }
    }
}

#[test]
fn check_and_transform_readers_use_final_types() {
    let mut failures = Vec::new();
    for (fixture, stage, code, expected) in [
        ("detvar2", JsonStage::Transform, "E436", false),
        ("detvarexpression2", JsonStage::Transform, "E436", false),
        ("paramdet", JsonStage::Transform, "E436", true),
        ("filterlater", JsonStage::Check, "E058", true),
        ("filternative2", JsonStage::Check, "E311", true),
        ("stderrdet", JsonStage::Check, "E093", true),
        ("corrdet", JsonStage::Check, "E093", true),
        ("corrmixed", JsonStage::Check, "E093", true),
        ("copy_source_accept", JsonStage::Check, "E059", false),
        ("macro_head_refuse", JsonStage::Check, "E059", true),
        ("macro_pac_decl_accept", JsonStage::Check, "E058", false),
        ("sumordinaryparam", JsonStage::Check, "E478", true),
        ("sumordinaryvar", JsonStage::Check, "E478", true),
        (
            "macro_subsample_decl_accept",
            JsonStage::Check,
            "E058",
            false,
        ),
        ("ssvar", JsonStage::Check, "E130", true),
        ("ssparam", JsonStage::Check, "E130", false),
        ("estimated", JsonStage::Check, "E212", true),
        ("vemparam", JsonStage::Transform, "E434", false),
        ("vemvar", JsonStage::Transform, "E434", false),
        ("vemparam2", JsonStage::Transform, "E436", false),
        ("vemvar2", JsonStage::Transform, "E436", false),
        ("targetcoef3", JsonStage::Transform, "E193", false),
        ("growthvar", JsonStage::Transform, "E448", true),
        ("macrodiscount", JsonStage::Check, "E442", false),
    ] {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("tests/fixtures/parameter_consumers/{fixture}.mod")),
        )
        .unwrap();
        let diagnostics = analyze(&parse(&source));
        let refusal = diagnostics.iter().find(|row| row.code == code);
        if refusal.is_some() != expected
            || (fixture == "estimated" && diagnostics.iter().any(|row| row.code == "E093"))
        {
            failures.push(format!("{fixture}: {diagnostics:?}"));
        }
        if fixture.starts_with("sumordinary") {
            assert!(
                !diagnostics.iter().any(|row| row.code == "W208"),
                "{fixture}: {diagnostics:?}"
            );
        }
        if let Some(pp) = find_preprocessor(None) {
            let official = run_preprocessor(&source, &pp, None, Duration::from_secs(30), stage);
            assert_eq!(official.success, !expected, "{fixture}: {official:?}");
            if let Some(refusal) =
                refusal.filter(|_| expected && code != "E058" && fixture != "stderrdet")
            {
                assert!(
                    official.raw_stdout.contains(&refusal.message),
                    "{fixture}: {official:?}"
                );
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn deterministic_trend_warnings_follow_final_types() {
    for (fixture, expected) in [("trends_accept", false), ("trends_warn", true)] {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("tests/fixtures/parameter_consumers/{fixture}.mod")),
        )
        .unwrap();
        let diagnostics = analyze(&parse(&source));
        assert_eq!(
            diagnostics.iter().any(|row| row.code == "W206"),
            expected,
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
            assert!(official.success, "{official:?}");
            assert_eq!(
                official
                    .raw_stdout
                    .contains("Warning: Non-variable symbol used in deterministic_trends: z"),
                expected,
                "{official:?}"
            );
        }
    }
}

#[test]
fn heterogeneous_parameters_made_ordinary_can_be_used_in_aggregate_equations() {
    for (declaration, target) in [
        ("var", "parameters"),
        ("parameters", "parameters"),
        ("parameters", "var"),
    ] {
        let source = format!("heterogeneity_dimension h; var y; {declaration}(heterogeneity=h) z; change_type({target}) z; model; y=z; {} end;", if target=="var" { "z=0;" } else { "" });
        let diagnostics = analyze(&parse(&source));
        assert!(
            !diagnostics
                .iter()
                .any(|row| row.severity == dygnosis::Severity::Error || row.code == "W208"),
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
fn undeclared_name_hints_do_not_call_a_retyped_parameter_exogenous() {
    let source =
        "var y; varexo e_foo_; change_type(parameters) e_foo_; model; y=e_foo_+e_missing_; end;";
    let diagnostics = analyze(&parse(source));
    let refusal = diagnostics.iter().find(|row| row.code == "E020").unwrap();
    assert!(
        !refusal
            .message
            .contains("follows the naming pattern of exogenous"),
        "{refusal:?}"
    );
    if let Some(pp) = find_preprocessor(None) {
        let official =
            run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(
            !official.success && official.raw_stdout.contains("Unknown symbol: e_missing_"),
            "{official:?}"
        );
    }
}

#[test]
fn sum_role_is_not_changed_by_a_later_removal() {
    let source="heterogeneity_dimension h; var y; var(heterogeneity=h) x; model; y=SUM(x); end; model(heterogeneity=h); x=0; end; var_remove x;";
    let diagnostics = analyze(&parse(source));
    assert!(
        !diagnostics.iter().any(|row| row.code == "E478"),
        "{diagnostics:?}"
    );
    if let Some(pp) = find_preprocessor(None) {
        let official =
            run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(
            !official.raw_stdout.contains(
                "The argument to the SUM() operator must be a heterogeneous endogenous variable"
            ),
            "{official:?}"
        );
    }
}

#[test]
fn generated_policy_parameters_are_parameters_in_shock_values() {
    let source="var y; varexo e; model; y=e; end; planner_objective y^2; ramsey_model; estimated_params; optimal_policy_discount_factor,.9; end; shocks; var e; stderr optimal_policy_discount_factor; end;";
    let diagnostics = analyze(&parse(source));
    let refusal = diagnostics
        .iter()
        .find(|row| row.code == "E212")
        .expect("implicit parameter also has the parameter type");
    if let Some(pp) = find_preprocessor(None) {
        let official =
            run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(
            !official.success && official.raw_stdout.contains(&refusal.message),
            "{official:?}"
        );
    }
}
