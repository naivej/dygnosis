use std::time::Duration;

use dygnosis::preprocessor::find_preprocessor;
use dygnosis::{analyze, check_file, parse, run_preprocessor, JsonStage};

struct Pair {
    code: &'static str,
    fire: &'static str,
    quiet: &'static str,
    official: &'static str,
    span_needle: &'static str,
}

const PAIRS: &[Pair] = &[
    Pair {
        code: "E186",
        fire: include_str!("fixtures/written/e186_fire.mod"),
        quiet: include_str!("fixtures/written/e186_quiet.mod"),
        official: "x not used in the model block",
        span_needle: "x",
    },
    Pair {
        code: "E188",
        fire: include_str!("fixtures/written/e188_fire.mod"),
        quiet: include_str!("fixtures/written/e188_quiet.mod"),
        official: "There are 2 equations but 1 endogenous variables!",
        span_needle: "model",
    },
    Pair {
        code: "E192",
        fire: include_str!("fixtures/written/e192_fire.mod"),
        quiet: include_str!("fixtures/written/e192_quiet.mod"),
        official: "There are 1 equations but 2 endogenous variables in the model for heterogeneity dimension 'h'!",
        span_needle: "model(heterogeneity=h)",
    },
    Pair {
        code: "E189",
        fire: include_str!("fixtures/written/e189_fire.mod"),
        quiet: include_str!("fixtures/written/e189_quiet.mod"),
        official: "Division by zero when substituting constants in equation 2",
        span_needle: "x-1",
    },
    Pair {
        code: "E190",
        fire: include_str!("fixtures/written/e190_fire.mod"),
        quiet: include_str!("fixtures/written/e190_quiet.mod"),
        official: "In Partial Information models, EXPECTATION(0)(X) can only be used when X is a single variable.",
        span_needle: "EXPECTATION(0)",
    },
];

fn own(text: &str) -> Vec<dygnosis::Diagnostic> {
    analyze(&parse(text))
}

#[test]
fn bounded_written_fires_and_quiet_neighbors() {
    for pair in PAIRS {
        let fire = own(pair.fire);
        let diagnostic = fire
            .iter()
            .find(|diag| diag.code == pair.code)
            .unwrap_or_else(|| panic!("{} fire: {fire:?}", pair.code));
        assert!(
            diagnostic.message.contains(pair.official),
            "{} text: {diagnostic:?}",
            pair.code
        );
        let source_span = pair
            .fire
            .get(diagnostic.span.start as usize..diagnostic.span.end as usize)
            .unwrap_or("");
        assert!(
            source_span.contains(pair.span_needle),
            "{} span {source_span:?}",
            pair.code
        );
        let quiet = own(pair.quiet);
        assert!(
            quiet.iter().all(|diag| diag.code != pair.code),
            "{} quiet: {quiet:?}",
            pair.code
        );
    }
    let unused = own(PAIRS[0].fire);
    assert!(
        unused
            .iter()
            .all(|diag| diag.code != "E188" && diag.code != "W013" && diag.code != "W020"),
        "{unused:?}"
    );
}

#[test]
fn bounded_written_pairs_agree_with_pinned_transform() {
    let Some(pp) = find_preprocessor(None) else {
        eprintln!("skipping honesty: pinned Dynare preprocessor unavailable");
        return;
    };
    for pair in PAIRS {
        let check = run_preprocessor(
            pair.fire,
            &pp,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        assert!(
            check.success,
            "{} check: {}{}",
            pair.code, check.raw_stdout, check.raw_stderr
        );
        let fire = run_preprocessor(
            pair.fire,
            &pp,
            None,
            Duration::from_secs(30),
            JsonStage::Transform,
        );
        assert!(!fire.success, "{} transform fire accepted", pair.code);
        assert!(
            format!("{}{}", fire.raw_stdout, fire.raw_stderr).contains(pair.official),
            "{}: {}{}",
            pair.code,
            fire.raw_stdout,
            fire.raw_stderr
        );
        let quiet = run_preprocessor(
            pair.quiet,
            &pp,
            None,
            Duration::from_secs(30),
            JsonStage::Transform,
        );
        assert!(
            quiet.success,
            "{} transform quiet: {}{}",
            pair.code, quiet.raw_stdout, quiet.raw_stderr
        );
    }
}

#[test]
fn rewrite_and_local_boundaries_remain_quiet() {
    let lead = own("var y; model; y=y(-1); y=y(-2); end;");
    assert!(lead.iter().all(|diag| diag.code != "E188"), "{lead:?}");

    let local = own("var y; varexo e; model; # a = y+e; y=EXPECTATION(0)(a); end; stoch_simul(partial_information) y;");
    assert!(local.iter().all(|diag| diag.code != "E190"), "{local:?}");
}

#[test]
fn repeated_same_kind_declaration_does_not_invent_a_count_gap() {
    let text = "var y; var y; model; y=0; end;";
    let model = parse(text);
    let own = analyze(&model);
    assert!(own.iter().any(|diag| diag.code == "W031"), "{own:?}");
    assert!(
        own.iter()
            .all(|diag| diag.code != "W013" && diag.code != "E188"),
        "{own:?}"
    );
    let gap = dygnosis::equations::count_gap(&model);
    assert_eq!((gap.n_equations, gap.n_endogenous, gap.delta), (1, 1, 0));
    let info = dygnosis::mcp::dynare_model_info(text, None, None);
    assert_eq!(info["n_endogenous"], 1);
    assert_eq!(info["endogenous"], serde_json::json!(["y"]));
    let rows = dygnosis::mcp::dynare_equations(text, None, None, None, None);
    assert_eq!(rows["count_gap"]["n_endogenous"], 1);

    if let Some(pp) = find_preprocessor(None) {
        let result = run_preprocessor(text, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(result.success, "{}{}", result.raw_stdout, result.raw_stderr);
        assert!(format!("{}{}", result.raw_stdout, result.raw_stderr).contains("WARNING:"));
    }

    let macro_text = "@#define i = 1\nvar y_@{i}; var y_1; model; y_1=0; end;";
    let macro_model = parse(macro_text);
    let macro_diags = analyze(&macro_model);
    assert!(
        macro_diags.iter().any(|d| d.code == "W031"),
        "{macro_diags:?}"
    );
    assert!(
        macro_diags
            .iter()
            .all(|d| d.code != "W013" && d.code != "E188"),
        "{macro_diags:?}"
    );
    assert_eq!(dygnosis::equations::count_gap(&macro_model).n_endogenous, 1);
    if let Some(pp) = find_preprocessor(None) {
        let result = run_preprocessor(
            macro_text,
            &pp,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        assert!(result.success, "{}{}", result.raw_stdout, result.raw_stderr);
        assert!(format!("{}{}", result.raw_stdout, result.raw_stderr).contains("WARNING:"));
    }
}

#[test]
fn simplified_partial_information_arguments_do_not_get_e190() {
    for arg in ["+y", "y+0", "y*1", "y^1"] {
        let text = format!(
            "var y; model; y=EXPECTATION(0)({arg}); end; stoch_simul(partial_information) y;"
        );
        if let Some(pp) = find_preprocessor(None) {
            let official = run_preprocessor(
                &text,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Transform,
            );
            assert!(
                official.success,
                "{arg}: {}{}",
                official.raw_stdout, official.raw_stderr
            );
        }
        let ours = own(&text);
        assert!(ours.iter().all(|d| d.code != "E190"), "{arg}: {ours:?}");
    }
}

#[test]
fn repeated_heterogeneous_blocks_are_counted_per_dimension() {
    let text = "heterogeneity_dimension h; var(heterogeneity=h) c n; model(heterogeneity=h); c=n; end; model(heterogeneity=h); n=c; end;";
    let ours = own(text);
    assert!(ours.iter().all(|d| d.code != "E192"), "{ours:?}");
    if let Some(pp) = find_preprocessor(None) {
        let official = run_preprocessor(
            text,
            &pp,
            None,
            Duration::from_secs(30),
            JsonStage::Transform,
        );
        assert!(
            official.success,
            "{}{}",
            official.raw_stdout, official.raw_stderr
        );
    }
}

#[test]
fn constant_zero_precedes_unused_name_and_count() {
    let text = "var x y z; model; x=1; y=1/(x-1); end;";
    let own = own(text);
    assert!(own.iter().any(|d| d.code == "E189"), "{own:?}");
    assert!(
        own.iter().all(|d| d.code != "E186" && d.code != "E188"),
        "{own:?}"
    );
    if let Some(pp) = find_preprocessor(None) {
        let result = run_preprocessor(
            text,
            &pp,
            None,
            Duration::from_secs(30),
            JsonStage::Transform,
        );
        assert!(!result.success);
        assert!(format!("{}{}", result.raw_stdout, result.raw_stderr)
            .contains("Division by zero when substituting constants in equation 2"));
    }
}

#[test]
fn aggregate_use_inside_heterogeneous_body_exempts_e186() {
    let text = "var x y; heterogeneity_dimension h; var(heterogeneity=h) c; model; y=0; y=y(-1); end; model(heterogeneity=h); c=x; end;";
    let ours = own(text);
    assert!(ours.iter().all(|d| d.code != "E186"), "{ours:?}");
    if let Some(pp) = find_preprocessor(None) {
        let result = run_preprocessor(
            text,
            &pp,
            None,
            Duration::from_secs(30),
            JsonStage::Transform,
        );
        assert!(result.success, "{}{}", result.raw_stdout, result.raw_stderr);
    }
}

#[test]
fn unresolved_include_withholds_written_transform_errors() {
    let text = "@#include \"dygnosis_written_missing.inc\"\nvar x y; model; y=y(-1); end;";
    let own = check_file(text, "C:/dygnosis-written-incomplete/root.mod");
    assert!(own.iter().any(|d| d.code == "E061"), "{own:?}");
    assert!(
        own.iter()
            .all(|d| !matches!(d.code.as_str(), "E186" | "E188" | "E189" | "E190" | "E192")),
        "{own:?}"
    );
}

#[test]
fn heterogeneous_local_keeps_count_warning_outside_e192_bound() {
    let text = include_str!("fixtures/written/w208_rewrite_boundary.mod");
    let ours = own(text);
    assert!(ours.iter().any(|d| d.code == "W208"), "{ours:?}");
    assert!(ours.iter().all(|d| d.code != "E192"), "{ours:?}");
    if let Some(pp) = find_preprocessor(None) {
        let check = run_preprocessor(text, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(check.success, "{}{}", check.raw_stdout, check.raw_stderr);
    }
}

#[test]
fn original_dsge_weight_var_stops_at_unused_endogenous_before_e219() {
    let text = include_str!("fixtures/written/e186_preempts_e219.mod");
    let ours = own(text);
    assert!(
        ours.iter()
            .any(|d| d.code == "E186" && d.message.contains("dsge_prior_weight")),
        "{ours:?}"
    );
    assert!(ours.iter().all(|d| d.code != "E219"), "{ours:?}");
    if let Some(pp) = find_preprocessor(None) {
        let check = run_preprocessor(text, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(check.success, "{}{}", check.raw_stdout, check.raw_stderr);
        let transform = run_preprocessor(
            text,
            &pp,
            None,
            Duration::from_secs(30),
            JsonStage::Transform,
        );
        assert!(!transform.success);
        let output = format!("{}{}", transform.raw_stdout, transform.raw_stderr);
        assert!(
            output.contains("dsge_prior_weight not used in the model block"),
            "{output}"
        );
        assert!(
            !output.contains("should not be declared as a model variable"),
            "{output}"
        );
    }
}

#[test]
fn direct_literal_constant_zero_covers_values_and_equation_order() {
    let fires = [
        ("var x y; model; x=0; y=2/(x-0); end;", 2),
        ("var x y; model; x=2; y=2/(x-2); end;", 2),
        ("var x y; model; x=0.5; y=2/(x-0.5); end;", 2),
        ("var x y; model; y=2/(x-2); x=2; end;", 1),
        ("var x y z; model; x=1; z=2; y=2/(x-1); end;", 3),
    ];
    for (text, equation) in fires {
        let expected =
            format!("Division by zero when substituting constants in equation {equation}");
        let ours = own(text);
        let error = ours
            .iter()
            .find(|d| d.code == "E189")
            .unwrap_or_else(|| panic!("{text}: {ours:?}"));
        assert_eq!(error.message, expected, "{text}");
        if let Some(pp) = find_preprocessor(None) {
            let result = run_preprocessor(
                text,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Transform,
            );
            assert!(!result.success, "{text}");
            assert!(format!("{}{}", result.raw_stdout, result.raw_stderr).contains(&expected));
        }
    }

    let quiet = [
        "var x y; model; x=-1; y=2/(x-(-1)); end;",
        "var x y; model; x=2; y=0/(x-2); end;",
        "var x y; model; x=2; y=2/(x-1); end;",
    ];
    for text in quiet {
        let ours = own(text);
        assert!(ours.iter().all(|d| d.code != "E189"), "{text}: {ours:?}");
        if let Some(pp) = find_preprocessor(None) {
            let result = run_preprocessor(
                text,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Transform,
            );
            assert!(
                result.success,
                "{text}: {}{}",
                result.raw_stdout, result.raw_stderr
            );
        }
    }
}

#[test]
fn heterogeneous_count_waits_for_direct_unused_aggregate_name() {
    let text = "var y z; heterogeneity_dimension h; var(heterogeneity=h) c n; model; y=y(-1); end; model(heterogeneity=h); c=c(-1); end;";
    let ours = own(text);
    assert!(
        ours.iter()
            .any(|d| d.code == "E186" && d.message.contains("z not used")),
        "{ours:?}"
    );
    assert!(ours.iter().all(|d| d.code != "E192"), "{ours:?}");
    if let Some(pp) = find_preprocessor(None) {
        let check = run_preprocessor(text, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(check.success, "{}{}", check.raw_stdout, check.raw_stderr);
        let transform = run_preprocessor(
            text,
            &pp,
            None,
            Duration::from_secs(30),
            JsonStage::Transform,
        );
        assert!(!transform.success);
        assert!(format!("{}{}", transform.raw_stdout, transform.raw_stderr)
            .contains("z not used in the model block"));
    }
}

#[test]
fn expectation_arguments_changed_by_other_equations_stay_outside_e190() {
    for (definition, accepted) in [("0+0", true), ("y-y", true), ("0*y", true), ("y^0", false)] {
        let text = format!("var y z; model; z={definition}; y=EXPECTATION(0)(y+z); end; stoch_simul(partial_information) y;");
        let ours = own(&text);
        assert!(
            ours.iter().all(|d| d.code != "E190"),
            "{definition}: {ours:?}"
        );
        if let Some(pp) = find_preprocessor(None) {
            let result = run_preprocessor(
                &text,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Transform,
            );
            assert_eq!(
                result.success, accepted,
                "{definition}: {}{}",
                result.raw_stdout, result.raw_stderr
            );
        }
    }
}

fn assert_e189_equation(text: &str, equation: usize) {
    let expected = format!("Division by zero when substituting constants in equation {equation}");
    let ours = own(text);
    let error = ours
        .iter()
        .find(|d| d.code == "E189")
        .unwrap_or_else(|| panic!("{text}: {ours:?}"));
    assert_eq!(error.message, expected, "{text}");
    let span = text
        .get(error.span.start as usize..error.span.end as usize)
        .unwrap_or("");
    assert!(span.contains("x-1"), "{text}: span {span:?}");
    let first = text.find("x-1").expect("denominator");
    assert!(
        (error.span.start as usize) <= first && (error.span.end as usize) > first,
        "{text}: span {span:?} is not the first denominator"
    );
    assert!(ours.iter().all(|d| d.code != "E186"), "{text}: {ours:?}");
    if let Some(pp) = find_preprocessor(None) {
        let check = run_preprocessor(text, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(
            check.success,
            "{text} check: {}{}",
            check.raw_stdout, check.raw_stderr
        );
        let result = run_preprocessor(
            text,
            &pp,
            None,
            Duration::from_secs(30),
            JsonStage::Transform,
        );
        assert!(!result.success, "{text}");
        assert!(
            format!("{}{}", result.raw_stdout, result.raw_stderr).contains(&expected),
            "{text}: {}{}",
            result.raw_stdout,
            result.raw_stderr
        );
    }
}

#[test]
fn left_hand_direct_zero_reports_the_first_denominator() {
    assert_e189_equation("var x y z; model; x=1; y/(x-1)=0; z=1/(x-1); end;", 2);
}

#[test]
fn left_hand_direct_zero_without_the_later_equation() {
    assert_e189_equation("var x y z; model; x=1; y/(x-1)=0; end;", 2);
}

#[test]
fn nonzero_left_hand_denominator_stays_quiet() {
    let quiet = "var x y; model; x=2; 2/(x-1)=y; end;";
    let ours = own(quiet);
    assert!(ours.iter().all(|d| d.code != "E189"), "{quiet}: {ours:?}");
    if let Some(pp) = find_preprocessor(None) {
        let check = run_preprocessor(quiet, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(
            check.success,
            "{quiet} check: {}{}",
            check.raw_stdout, check.raw_stderr
        );
        let result = run_preprocessor(
            quiet,
            &pp,
            None,
            Duration::from_secs(30),
            JsonStage::Transform,
        );
        assert!(
            result.success,
            "{quiet}: {}{}",
            result.raw_stdout, result.raw_stderr
        );
    }
}
