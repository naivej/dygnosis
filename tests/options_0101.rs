use std::time::Duration;

use dygnosis::preprocessor::find_preprocessor;
use dygnosis::{check_file, run_preprocessor, JsonStage, Severity};

const BASE: &str = "var y; varexo e f; parameters beta; beta=.9; model; y=beta+e+f; end;";

fn errors(source: &str) -> Vec<dygnosis::Diagnostic> {
    check_file(source, "options_0101.mod")
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

fn assert_option(source: &str, token: &str, expected: bool) {
    let ours = errors(source);
    assert_eq!(
        ours.iter()
            .any(|d| d.code == "E001" && d.message.contains(token)),
        expected,
        "{source}: {ours:?}"
    );
    if let Some(pp) = find_preprocessor(None) {
        let official =
            run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert_eq!(official.success, !expected, "{source}: {official:?}");
    }
}

#[test]
fn command_qualified_option_membership() {
    for (command, bad, good) in [
        ("model", "not_a_dynare_option", "linear"),
        ("stoch_simul", "not_a_dynare_option=1", "order=1"),
        (
            "method_of_moments",
            "use_pct",
            "mom_method=GMM, datafile='data.csv', add_tiny_number_to_cholesky=0.1",
        ),
        (
            "ms_compute_mdd",
            "mdd_proposal_draws=10",
            "proposal_draws=10",
        ),
        ("ms_compute_mdd", "mdd_use_mean_center", "use_mean_center"),
        (
            "pac_model",
            "model_name=y, discount=beta, auxiliary_model=y",
            "model_name=y, discount=beta, auxiliary_model_name=y",
        ),
        (
            "var_expectation_model",
            "variable=y, horizon=1, model_name=y, auxiliary_model=y",
            "variable=y, horizon=1, model_name=y, auxiliary_model_name=y",
        ),
        ("plot_shock_decomposition", "with_epilogue", "nodisplay"),
        ("realtime_shock_decomposition", "kalman_algo=1", "nograph"),
        (
            "realtime_shock_decomposition",
            "kalman_tol=0.000001",
            "nograph",
        ),
    ] {
        let bad_source = if command == "model" {
            format!("var y; model({bad}); y=1; end;")
        } else {
            format!("{BASE} {command}({bad});")
        };
        let good_source = if command == "model" {
            format!("var y; model({good}); y=1; end;")
        } else {
            format!("{BASE} {command}({good});")
        };
        assert_option(&bad_source, "", true);
        assert_option(&good_source, "syntax error", false);
    }
}

#[test]
fn primitive_option_value_forms() {
    for (source, accepted) in [
        ("var y; model(linear); y=1; end;", true),
        ("var y; model(linear=1); y=1; end;", false),
        ("var y; model(mfs=2); y=1; end;", true),
        ("var y; model(mfs=2.5); y=1; end;", false),
        ("var y; model(cutoff=0.1); y=1; end;", true),
        ("var y; model(cutoff=-1); y=1; end;", false),
        (
            "var y; model(differentiate_forward_vars=(y)); y=1; end;",
            true,
        ),
        ("var y; model; y=1; end; stoch_simul(order=2);", true),
        ("var y; model; y=1; end; stoch_simul(order=2.5);", false),
        ("var y; model; y=1; end; stoch_simul(nograph);", true),
        ("var y; model; y=1; end; stoch_simul(nograph=1);", false),
        (
            "var y; model; y=1; end; stoch_simul(hp_filter=1600.5);",
            true,
        ),
    ] {
        assert_eq!(
            errors(source).iter().any(|d| d.code == "E001"),
            !accepted,
            "{source}"
        );
        if let Some(pp) = find_preprocessor(None) {
            let official =
                run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
            assert_eq!(official.success, accepted, "{source}: {official:?}");
        }
    }
}

#[test]
fn manual_discrepancies_are_absent_from_command_help() {
    for (command, rejected) in [
        ("method_of_moments", "use_pct"),
        ("ms_compute_mdd", "mdd_proposal_draws"),
        ("ms_compute_mdd", "mdd_use_mean_center"),
        ("pac_model", "auxiliary_model"),
        ("var_expectation_model", "auxiliary_model"),
        ("plot_shock_decomposition", "with_epilogue"),
        ("realtime_shock_decomposition", "kalman_algo"),
        ("realtime_shock_decomposition", "kalman_tol"),
    ] {
        assert!(
            dygnosis::command_options(command)
                .iter()
                .all(|(name, _)| *name != rejected),
            "{command}.{rejected} must not be offered"
        );
    }
    let realtime: Vec<_> = dygnosis::command_options("realtime_shock_decomposition")
        .iter()
        .map(|(name, _)| *name)
        .collect();
    assert!(realtime.contains(&"nograph"));
    assert!(realtime.contains(&"presample"));
    assert!(!realtime.contains(&"shock_decomposition_nograph"));
    assert!(!realtime.contains(&"shock_decomposition_presample"));
}

#[test]
fn estimated_params_remove_reaches_names_and_types() {
    let accepted =
        format!("{BASE} estimated_params_remove; beta; stderr e; corr e,f; skew e; end;");
    assert!(errors(&accepted).is_empty(), "{:?}", errors(&accepted));
    let unknown = format!("{BASE} estimated_params_remove; not_declared; end;");
    let ours = errors(&unknown);
    assert!(
        ours.iter()
            .any(|d| d.code == "E058" && d.message == "Unknown symbol: not_declared."),
        "{ours:?}"
    );
    if let Some(pp) = find_preprocessor(None) {
        for (source, accepted) in [(&accepted, true), (&unknown, false)] {
            let official =
                run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
            assert_eq!(official.success, accepted, "{source}: {official:?}");
        }
    }
}

#[test]
fn estimated_params_remove_generic_reach_pairs() {
    // fire/quiet pairs for the trigger-generic names, wrong types, and duplicates.
    for (row, code, official_accepts) in [
        ("ghost;", "E058", false),
        ("stderr ghost;", "E058", false),
        ("corr e,ghost;", "E058", false),
        ("beta;", "", true),
        ("stderr e;", "", true),
        ("corr e,f;", "", true),
        ("skew e;", "", true),
        ("y;", "E059", false),
        ("stderr beta;", "E317", false),
        ("corr e,beta;", "E317", false),
        ("beta; beta;", "", true),
        ("stderr e; stderr e;", "", true),
        ("corr e,f; corr e,f;", "", true),
        ("skew e; skew e;", "", true),
        ("skew beta;", "", true),
        ("beta,1;", "E001", false),
    ] {
        let source = format!("{BASE} estimated_params_remove; {row} end;");
        let own = errors(&source);
        if code.is_empty() {
            assert!(own.is_empty(), "{row}: {own:?}");
        } else {
            assert!(own.iter().any(|d| d.code == code), "{row}: {own:?}");
        }
        if let Some(pp) = find_preprocessor(None) {
            let official = run_preprocessor(
                &source,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            assert_eq!(official.success, official_accepts, "{row}: {official:?}");
        }
    }
}

#[test]
fn removal_uses_parse_order_roles_and_exact_opener() {
    for (source, code, accepted) in [
        ("var y; model; y=1; end; estimated_params_remove; beta; end; parameters beta;", "E058", false),
        ("parameters beta; var y; model; y=1; end; estimated_params_remove; beta; end;", "", true),
        ("var beta y; change_type(parameters) beta; model; y=1; end; estimated_params_remove; beta; end;", "", true),
        ("var beta y; model; y=1; end; estimated_params_remove; beta; end; change_type(parameters) beta;", "E059", false),
        ("parameters beta; var y; model; y=1; end; var_remove beta; estimated_params_remove; beta; end;", "E059", false),
        ("parameters model; var y; model; y=1; end; estimated_params_remove; model; end;", "", true),
        ("parameters beta; var y; model; y=1; end; estimated_params_remove(foo); beta; end;", "E001", false),
        ("parameters beta; var y; model; y=1; end; estimated_params_remove(overwrite); beta; end;", "E001", false),
        ("parameters beta; var y; model; y=1; end; estimated_params_remove; ; end;", "E001", false),
        ("parameters beta; var y; model; y=1; end; estimated_params_remove; beta end;", "E001", false),
    ] {
        let ours = errors(source);
        if code.is_empty() {
            assert!(ours.is_empty(), "{source}: {ours:?}");
        } else {
            assert!(ours.iter().any(|d| d.code == code), "{source}: {ours:?}");
        }
        if let Some(pp) = find_preprocessor(None) {
            let result = run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
            assert_eq!(result.success, accepted, "{source}: {result:?}");
        }
    }
}

#[test]
fn command_values_and_separators_use_entire_production() {
    for (source, accepted) in [
        ("var y; model(mfs=1+1); y=1; end;", false),
        ("var y; model(cutoff=1/2); y=1; end;", false),
        ("var y; model(linear block); y=1; end;", false),
        ("var y; model(mfs=1 linear); y=1; end;", false),
        ("var y; model(linear,block); y=1; end;", true),
        ("var y; model(mfs=1,linear); y=1; end;", true),
        ("var y; model; y=1; end; stoch_simul(order=1+1);", false),
        ("var y; model; y=1; end; stoch_simul(order='1');", false),
        ("var y; model; y=1; end; stoch_simul(hp_filter=1/2);", false),
        ("var y; model; y=1; end; stoch_simul(hp_filter=-0);", false),
        ("var y; model; y=1; end; stoch_simul(hp_filter=+0);", false),
        (
            "var y; model; y=1; end; stoch_simul(nograph nomoments);",
            false,
        ),
        (
            "var y; model; y=1; end; stoch_simul(nograph,nomoments);",
            true,
        ),
        ("var y; model; y=1; end; stoch_simul(hp_filter=1e-3);", true),
        (
            "var y; model; y=1; end; stoch_simul(hp_filter=1e400);",
            true,
        ),
        (
            "var y; model; y=1; end; stoch_simul(qz_zero_threshold=1e400);",
            true,
        ),
        ("var y; model; y=1; end; stoch_simul(hp_filter=1d3);", true),
        ("var y; model; y=1; end; stoch_simul(hp_filter=1.);", true),
        (
            "var y; varexo e; model; y=e; end; stoch_simul(irf_shocks=(e),order=1);",
            true,
        ),
    ] {
        assert_eq!(
            errors(source).iter().any(|d| d.code == "E001"),
            !accepted,
            "{source}: {:?}",
            errors(source)
        );
        if let Some(pp) = find_preprocessor(None) {
            let result =
                run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
            assert_eq!(result.success, accepted, "{source}: {result:?}");
        }
    }
    // Dynare recognizes this numeric spelling even if later conversion fails.
    assert!(errors("var y; model(cutoff=1e400); y=1; end;")
        .iter()
        .all(|d| d.code != "E001"));
}

#[test]
fn missing_comma_after_a_value_blames_the_next_option() {
    let source = "var y; model; y=1; end; stoch_simul(order=1 irf=10);";
    let ours = errors(source);
    assert_eq!(ours.len(), 1, "{ours:?}");
    assert_eq!(
        ours[0].message,
        "syntax error, expected ',' before option 'irf' in 'stoch_simul'"
    );
    assert_eq!(
        &source[ours[0].span.start as usize..ours[0].span.end as usize],
        "irf",
        "{ours:?}"
    );
    if let Some(pp) = find_preprocessor(None) {
        let result = run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(!result.success, "{result:?}");
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.span == ours[0].span
                    && d.message.contains("unexpected IRF, expecting COMMA")),
            "{result:?}"
        );
    }
}

#[test]
fn policy_membership_names_the_written_option() {
    let base = "var y; parameters beta; beta=.9; model; y=beta; end; planner_objective y;";
    assert_option(
        &format!("{base} ramsey_model(not_a_dynare_option);"),
        "unexpected option 'not_a_dynare_option' in 'ramsey_model'",
        true,
    );
    assert_option(
        &format!("{base} ramsey_model(planner_discount=beta);"),
        "syntax error",
        false,
    );
    assert_option(
        "var y; model; y=1; end; stoch_simul(linear);",
        "unexpected option 'linear' in 'stoch_simul'",
        true,
    );
    assert_option(
        "var y; model(order=1); y=1; end;",
        "unexpected option 'order' in 'model'",
        true,
    );
    if let Some(pp) = find_preprocessor(None) {
        for (source, needle) in [
            (
                "var y; model; y=1; end; stoch_simul(linear);",
                "syntax error, unexpected LINEAR",
            ),
            (
                "var y; model(order=1); y=1; end;",
                "syntax error, unexpected IDENTIFIER",
            ),
        ] {
            let result =
                run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
            assert!(!result.success);
            assert!(
                format!("{}{}", result.raw_stdout, result.raw_stderr).contains(needle),
                "{source}: {result:?}"
            );
        }
    }
}

#[test]
fn macro_copies_use_effective_role_order_despite_reused_spans() {
    let source = "parameters beta;\nvar y; model; y=1; end;\n@#define nums = 1:2\n@#for i in nums\nestimated_params_remove;\nbeta;\nend;\nvar_remove beta;\n@#endfor\n";
    let ours = errors(source);
    assert_eq!(
        ours.iter().filter(|d| d.code == "E059").count(),
        1,
        "{ours:?}"
    );
    assert!(ours.iter().any(|d| d.message == "beta is not a parameter"));
    if let Some(pp) = find_preprocessor(None) {
        let result = run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(!result.success);
        assert!(format!("{}{}", result.raw_stdout, result.raw_stderr)
            .contains("beta is not a parameter"));
    }
}

#[test]
fn heterogeneous_names_keep_their_distinct_removal_role() {
    let base = "heterogeneity_dimension d; var y; var(heterogeneity=d) yh; varexo(heterogeneity=d) eh; parameters(heterogeneity=d) ph; parameters beta; model; y=beta; end; model(heterogeneity=d); yh=eh+ph; end;";
    for (row, code, accepted) in [
        ("beta;", "", true),
        ("ph;", "E059", false),
        ("stderr yh;", "E317", false),
        ("stderr eh;", "E317", false),
        ("corr yh,yh;", "E317", false),
    ] {
        let source = format!("{base} estimated_params_remove; {row} end;");
        let ours = errors(&source);
        if code.is_empty() {
            assert!(ours.is_empty(), "{row}: {ours:?}");
        } else {
            assert!(ours.iter().any(|d| d.code == code), "{row}: {ours:?}");
        }
        if let Some(pp) = find_preprocessor(None) {
            let result = run_preprocessor(
                &source,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            assert_eq!(result.success, accepted, "{row}: {result:?}");
        }
    }
}
