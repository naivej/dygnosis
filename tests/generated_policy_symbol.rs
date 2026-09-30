use dygnosis::{analyze, find_preprocessor, parse, run_preprocessor, JsonStage};
use std::time::Duration;

#[test]
fn generated_discount_parameter_is_known_to_final_symbol_lists() {
    for earlier in [
        "ramsey_model(instruments=(pol));",
        "ramsey_policy(instruments=(pol));",
        "discretionary_policy(instruments=(pol));",
        "osr_params p; osr;",
    ] {
        for (list, refuses) in [
            ("forecast optimal_policy_discount_factor;", true),
            ("osr_params optimal_policy_discount_factor;", false),
        ] {
            let source = format!("var y pol; parameters p; p=1; model(linear); y=p; end; planner_objective y^2; {earlier} {list}");
            let diagnostics = analyze(&parse(&source));
            assert!(
                !diagnostics.iter().any(|row| row.code == "E239"),
                "{source}: {diagnostics:?}"
            );
            assert_eq!(
                diagnostics.iter().any(|row| row.code == "E240"),
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
                    assert!(official.raw_stdout.contains("forecast: Variable optimal_policy_discount_factor is not one of {endogenous}"), "{official:?}");
                }
            }
        }
    }
}

#[test]
fn generated_parameter_history_preserves_written_lists_and_assignment_types() {
    let base = "var y pol; model(linear); y=0; end; planner_objective y^2; discretionary_policy(instruments=(pol));";
    let source = format!("{base} optimal_policy_discount_factor=.9;");
    let model = parse(&source);
    let name = model
        .intern
        .lookup("optimal_policy_discount_factor")
        .unwrap();
    assert_eq!(model.final_symbol_kind(name), Some("parameters"));
    assert!(model.parameters.is_empty());
    assert!(model.param_assignments.iter().any(|row| row.name == name));
    let diagnostics = analyze(&model);
    assert!(
        !diagnostics.iter().any(|row| row.code == "E378"),
        "{diagnostics:?}"
    );
    for (declaration, code, needle) in [
        (
            "parameters optimal_policy_discount_factor;",
            "W031",
            "Symbol optimal_policy_discount_factor declared twice.",
        ),
        (
            "var optimal_policy_discount_factor;",
            "E030",
            "Symbol optimal_policy_discount_factor declared twice with different types!",
        ),
    ] {
        let source = format!("{base} {declaration}");
        let diagnostics = analyze(&parse(&source));
        assert!(
            diagnostics
                .iter()
                .any(|row| row.code == code && row.message == needle),
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
            assert_eq!(official.success, code == "W031", "{official:?}");
            assert!(official.raw_stdout.contains(needle), "{official:?}");
        }
    }
}

#[test]
fn generated_parameter_retypes_follow_effective_order() {
    let base = "var y pol; model(linear); y=0; end; planner_objective y^2; discretionary_policy(instruments=(pol)); change_type(var) optimal_policy_discount_factor;";
    for (list, refuses) in [
        ("forecast optimal_policy_discount_factor;", false),
        ("osr_params optimal_policy_discount_factor;", true),
    ] {
        let source = format!("{base} {list}");
        let diagnostics = analyze(&parse(&source));
        assert!(
            !diagnostics
                .iter()
                .any(|row| matches!(row.code.as_str(), "E295" | "E296" | "E239")),
            "{diagnostics:?}"
        );
        assert_eq!(
            diagnostics.iter().any(|row| row.code == "E240"),
            refuses,
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
            assert_eq!(official.success, !refuses, "{official:?}");
        }
    }
    let source = "var y pol; model(linear); y=0; end; planner_objective y^2;\n@#for j in 1:2\n@#if j==2\noptimal_policy_discount_factor=.9;\n@#endif\ndiscretionary_policy(instruments=(pol));\n@#endfor";
    let model = parse(source);
    assert!(model
        .param_assignments
        .iter()
        .any(|row| model.name(row.name) == "optimal_policy_discount_factor"));
    if let Some(pp) = find_preprocessor(None) {
        assert!(
            run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check).success
        );
    }
}

#[test]
fn earlier_generation_reaches_existing_planner_discount_clash() {
    let source = "var y; parameters p; p=1; model(linear); y=p; end; planner_objective y^2; osr_params p; osr; ramsey_model(planner_discount=.9);";
    let diagnostics = analyze(&parse(source));
    assert!(
        diagnostics.iter().any(|row| row.code == "E301"),
        "{diagnostics:?}"
    );
    if let Some(pp) = find_preprocessor(None) {
        let official =
            run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(!official.success, "{official:?}");
        assert!(
            official
                .raw_stdout
                .contains("the 'planner_discount' option cannot be used"),
            "{official:?}"
        );
    }
}
