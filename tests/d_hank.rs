use std::path::{Path, PathBuf};
use std::time::Duration;

use dygnosis::{analyze, find_preprocessor, parse, run_preprocessor, JsonStage, Severity};

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/d_hank")
        .join(name);
    let source = std::fs::read_to_string(&path).unwrap();
    assert!(source.starts_with("// inventory: "), "{}", path.display());
    source
}

fn pinned_binary() -> Option<PathBuf> {
    let pinned = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    if pinned.is_file() {
        return Some(pinned);
    }
    find_preprocessor(None).filter(|path| {
        path.components()
            .any(|part| part.as_os_str().to_string_lossy() == "7.2")
    })
}

struct Fire {
    file: &'static str,
    code: &'static str,
    message: &'static str,
    span: &'static str,
}

const FIRES: &[Fire] = &[
    Fire { file: "e459_var.mod", code: "E459", message: "Unknown heterogeneity dimension: ghost", span: "ghost" },
    Fire { file: "e459_varexo.mod", code: "E459", message: "Unknown heterogeneity dimension: ghost", span: "ghost" },
    Fire { file: "e459_parameters.mod", code: "E459", message: "Unknown heterogeneity dimension: ghost", span: "ghost" },
    Fire { file: "e459_model.mod", code: "E459", message: "Unknown heterogeneity dimension: ghost", span: "ghost" },
    Fire { file: "e459_shocks.mod", code: "E459", message: "Unknown heterogeneity dimension: ghost", span: "ghost" },
    Fire { file: "e460_repeat.mod", code: "E460", message: "Heterogeneity dimension 'd' already declared", span: "d" },
    Fire { file: "e461_planner.mod", code: "E461", message: "Symbol 'a' cannot be used in 'planner_objective', because it is heterogeneous.", span: "a" },
    Fire { file: "e462_occbin.mod", code: "E462", message: "Symbol 'a' cannot be used in 'occbin_constraints', because it is heterogeneous.", span: "a" },
    Fire { file: "e463_outside.mod", code: "E463", message: "Symbol 'a' cannot be used outside model declaration, because it is heterogeneous.", span: "a" },
    Fire { file: "e464_epilogue.mod", code: "E464", message: "Symbol 'a' cannot be used in epilogue block, because it is heterogeneous.", span: "a" },
    Fire { file: "e465_variance.mod", code: "E465", message: "shocks: setting a variance on 'y' is not allowed, because it is not a heterogeneous exogenous variable", span: "y" },
    Fire { file: "e466_stderr.mod", code: "E466", message: "shocks: setting a standard error on 'y' is not allowed, because it is not a heterogeneous exogenous variable", span: "y" },
    Fire { file: "e467_cov.mod", code: "E467", message: "shocks: setting a covariance between 'y' and 'e'is not allowed; covariances can only be specified for heterogeneous exogenous variables", span: "y" },
    Fire { file: "e468_corr.mod", code: "E468", message: "shocks: setting a correlation between 'y' and 'e'is not allowed; covariances can only be specified for heterogeneous exogenous variables", span: "y" },
    Fire { file: "e469_exo_lag.mod", code: "E469", message: "In model(heterogeneity=d), equation 1: lagged heterogeneous exogenous variable 'eh' is not supported.", span: "eh(-1)" },
    Fire { file: "e470_exo_lead.mod", code: "E470", message: "In model(heterogeneity=d), equation 1: lead on heterogeneous exogenous variable 'eh(+1)' is not supported.", span: "eh(+1)" },
    Fire { file: "e471_endo_lag.mod", code: "E471", message: "In model(heterogeneity=d), equation 1: heterogeneous endogenous variable 'a' with lag -2 is not supported (maximum lag is -1).", span: "a(-2)" },
    Fire { file: "e472_endo_lead.mod", code: "E472", message: "In model(heterogeneity=d), equation 1: heterogeneous endogenous variable 'a' with lead 2 is not supported (maximum lead is +1).", span: "a(+2)" },
    Fire { file: "e473_nonsep.mod", code: "E473", message: "In model(heterogeneity=d), equation 1 (line 5):\n  Non-separable expression 'log(a(-1)+a(1))'  combines forward-looking variables with lagged states and is not supported.", span: "log(a(-1)+a(+1))" },
    Fire { file: "e474_block.mod", code: "E474", message: "the 'block' option of the 'model' block is not supported for heterogeneous models", span: "block" },
    Fire { file: "e474_check.mod", code: "E474", message: "The 'check' command is not supported for heterogeneous models", span: "check" },
    Fire { file: "e474_steady.mod", code: "E474", message: "The 'steady' command is not supported for heterogeneous models", span: "steady" },
    Fire { file: "e474_pfs.mod", code: "E474", message: "The 'perfect_foresight_solver' command is not supported for heterogeneous models", span: "perfect_foresight_solver" },
    Fire { file: "e474_pfee.mod", code: "E474", message: "The 'perfect_foresight_with_expectation_errors_solver' command is not supported for heterogeneous models", span: "perfect_foresight_with_expectation_errors_solver" },
    Fire { file: "e474_stoch.mod", code: "E474", message: "The 'stoch_simul' command is not supported for heterogeneous models", span: "stoch_simul" },
    Fire { file: "e474_estimation.mod", code: "E474", message: "The 'estimation' command is not supported for heterogeneous models", span: "estimation" },
    Fire { file: "e474_osr.mod", code: "E474", message: "The 'osr' command is not supported for heterogeneous models", span: "osr" },
    Fire { file: "e474_osr_params.mod", code: "E474", message: "The 'osr_params' command is not supported for heterogeneous models", span: "osr_params" },
    Fire { file: "e474_optim.mod", code: "E474", message: "The 'optim_weights' block is not supported for heterogeneous models", span: "optim_weights" },
    Fire { file: "e474_ramsey.mod", code: "E474", message: "The 'ramsey_model' command is not supported for heterogeneous models", span: "ramsey_model" },
    Fire { file: "e474_disc.mod", code: "E474", message: "The 'discretionary_policy' command is not supported for heterogeneous models", span: "discretionary_policy" },
    Fire { file: "e474_extended.mod", code: "E474", message: "The 'extended_path' command is not supported for heterogeneous models", span: "extended_path" },
    Fire { file: "e474_ident.mod", code: "E474", message: "The 'identification' command is not supported for heterogeneous models", span: "identification" },
    Fire { file: "e474_sens.mod", code: "E474", message: "The 'sensitivity' command is not supported for heterogeneous models", span: "sensitivity" },
    Fire { file: "e474_mom.mod", code: "E474", message: "The 'methods_of_moments' command is not supported for heterogeneous models", span: "method_of_moments" },
    Fire { file: "e474_occbin.mod", code: "E474", message: "The 'occbin_constraints' block is not supported for heterogeneous models", span: "occbin_constraints" },
    Fire { file: "e475_sum_het.mod", code: "E475", message: "The SUM() operator cannot be used inside a model(heterogeneity=...) block", span: "SUM" },
    Fire { file: "e476_sum_expr.mod", code: "E476", message: "The argument to the SUM() operator must be a single variable", span: "SUM" },
    Fire { file: "e477_sum_lag.mod", code: "E477", message: "The argument to the SUM() operator must not have a lead or lag", span: "SUM" },
    Fire { file: "e478_sum_type.mod", code: "E478", message: "The argument to the SUM() operator must be a heterogeneous endogenous variable", span: "SUM" },
    Fire { file: "e479_mcp.mod", code: "E479", message: "'mcp' tags are not allowed in heterogeneous model blocks", span: "mcp" },
    Fire { file: "e001_tolf.mod", code: "E001", message: "syntax error, unexpected MINUS, expecting FLOAT_NUMBER or INT_NUMBER", span: "-" },
    Fire { file: "e001_solve_empty.mod", code: "E001", message: "syntax error, unexpected ')', expecting TRUNCATION_HORIZON", span: ")" },
    Fire { file: "e001_simulate_empty.mod", code: "E001", message: "syntax error, unexpected ')'", span: ")" },
    Fire { file: "e001_unclosed.mod", code: "E001", message: "syntax error, unexpected ';', expecting COMMA or ')'", span: ";" },
    Fire { file: "e001_dim_number.mod", code: "E001", message: "syntax error, unexpected INT_NUMBER", span: "1" },
    Fire { file: "e001_var_no_value.mod", code: "E001", message: "syntax error, unexpected ')', expecting EQUAL", span: ")" },
    Fire { file: "e001_model_no_value.mod", code: "E001", message: "syntax error, unexpected ')', expecting EQUAL", span: ")" },
    Fire { file: "e001_print.mod", code: "E001", message: "syntax error, unexpected PRINT, expecting FILENAME or TOLF or VARIABLE", span: "print" },
    Fire { file: "e001_log_het.mod", code: "E001", message: "syntax error, unexpected HETEROGENEITY, expecting DEFLATOR", span: "heterogeneity" },
    Fire { file: "e058_shock_unknown.mod", code: "E058", message: "Unknown symbol: ezz.", span: "ezz" },
    Fire { file: "e058_shock_unknown_dim.mod", code: "E058", message: "Unknown symbol: ezz.", span: "ezz" },
    Fire { file: "e262_mcp_unknown.mod", code: "E262", message: "Left-hand side of expression in 'mcp' tag is not a variable", span: "mcp" },
    Fire { file: "e180_mcp_perp.mod", code: "E180", message: "Can't have both an 'mcp' tag and a complementarity condition after the perpendicular symbol", span: "mcp" },
];

#[test]
fn official_fire_table() {
    let binary = pinned_binary();
    let mut failures = Vec::new();
    for fire in FIRES {
        let source = fixture(fire.file);
        let diagnostics = analyze(&parse(&source));
        let Some(ours) = diagnostics.iter().find(|diag| diag.code == fire.code) else {
            failures.push(format!(
                "{} missing {}: {diagnostics:?}",
                fire.file, fire.code
            ));
            continue;
        };
        if ours.message != fire.message {
            failures.push(format!(
                "{} message\n  ours: {}\n  want: {}",
                fire.file, ours.message, fire.message
            ));
        }
        let errors = diagnostics
            .iter()
            .filter(|diag| diag.severity == Severity::Error)
            .count();
        if errors != 1 {
            failures.push(format!(
                "{} error count {errors}: {diagnostics:?}",
                fire.file
            ));
        }
        let covered = &source[ours.span.start as usize..ours.span.end as usize];
        if !covered.contains(fire.span) {
            failures.push(format!(
                "{} span {covered:?} missing {}",
                fire.file, fire.span
            ));
        }
        if let Some(binary) = binary.as_deref() {
            let result = run_preprocessor(
                &source,
                binary,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            let report =
                format!("{} {}", result.raw_stdout, result.raw_stderr).replace("\r\n", "\n");
            if result.success {
                failures.push(format!("{} official accepted: {report}", fire.file));
            } else if !report.contains(fire.message) {
                failures.push(format!("{} official text missing:\n{report}", fire.file));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn w207_is_ours_and_official_accepts_two_dimensions() {
    let source = fixture("w207_two.mod");
    let diagnostics = analyze(&parse(&source));
    let warnings: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == "W207")
        .collect();
    assert_eq!(warnings.len(), 1, "{diagnostics:?}");
    assert_eq!(
        warnings[0].message,
        "Dynare 7.2 cannot load or compute a heterogeneous steady state with more than one heterogeneity dimension."
    );
    assert!(source[warnings[0].span.start as usize..warnings[0].span.end as usize].contains("e"));
    assert!(diagnostics
        .iter()
        .all(|diag| diag.severity != Severity::Error));
    let one = fixture("quiet_one.mod");
    assert!(analyze(&parse(&one)).iter().all(|diag| diag.code != "W207"));
    if let Some(binary) = pinned_binary() {
        let result = run_preprocessor(
            &source,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        let report = format!("{} {}", result.raw_stdout, result.raw_stderr);
        assert!(result.success, "7.2 refused two dimensions: {report}");
        assert!(!report.contains("more than one heterogeneity dimension"));
    }
}

#[test]
fn w207_stays_beside_a_check_error() {
    let source = fixture("w207_with_lag.mod");
    let diagnostics = analyze(&parse(&source));
    assert!(diagnostics.iter().any(|diag| diag.code == "W207"));
    assert!(diagnostics.iter().any(|diag| diag.code == "E469"));
}

#[test]
fn cross_dimension_symbol_stays_quiet() {
    let source = fixture("quiet_cross_dimension.mod");
    let diagnostics = analyze(&parse(&source));
    assert!(diagnostics.iter().all(|diag| diag.code != "E480"));
    assert!(diagnostics
        .iter()
        .all(|diag| diag.severity != Severity::Error));
    if let Some(binary) = pinned_binary() {
        let result = run_preprocessor(
            &source,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        let report = format!("{} {}", result.raw_stdout, result.raw_stderr);
        assert!(result.success, "7.2 refused cross-dimension use: {report}");
    }
}

#[test]
fn per_dimension_count_stays_silent() {
    let source = fixture("quiet_e192_count.mod");
    let diagnostics = analyze(&parse(&source));
    assert!(diagnostics.iter().all(|diag| diag.code != "E192"));
    assert!(diagnostics
        .iter()
        .all(|diag| diag.severity != Severity::Error));
}

#[test]
fn mcp_order_does_not_double_fire() {
    let declared = analyze(&parse(&fixture("e479_mcp.mod")));
    assert!(declared.iter().any(|diag| diag.code == "W170"));
    assert!(declared.iter().any(|diag| diag.code == "E479"));
    assert!(declared.iter().all(|diag| diag.code != "E263"));
    let unknown = analyze(&parse(&fixture("e262_mcp_unknown.mod")));
    assert!(unknown.iter().any(|diag| diag.code == "E262"));
    assert!(unknown.iter().all(|diag| diag.code != "E479"));
    let both = analyze(&parse(&fixture("e180_mcp_perp.mod")));
    assert!(both.iter().any(|diag| diag.code == "E180"));
    assert!(both
        .iter()
        .all(|diag| diag.code != "E479" && diag.code != "W170"));
}

#[test]
fn accepted_neighbours() {
    let binary = pinned_binary();
    for name in [
        "quiet_one.mod",
        "quiet_sum.mod",
        "quiet_timing.mod",
        "quiet_shock.mod",
        "quiet_separable.mod",
    ] {
        let source = fixture(name);
        let diagnostics = analyze(&parse(&source));
        assert!(
            diagnostics
                .iter()
                .all(|diag| diag.severity != Severity::Error),
            "{name}: {diagnostics:?}"
        );
        if let Some(binary) = binary.as_deref() {
            let result = run_preprocessor(
                &source,
                binary,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            let report =
                format!("{} {}", result.raw_stdout, result.raw_stderr).replace("\r\n", "\n");
            assert!(result.success, "7.2 refused {name}: {report}");
        }
    }
}

#[test]
fn older_sum_refusals_still_fire() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/p_hank");
    let epi = std::fs::read_to_string(root.join("fire_epilogue_sum.mod")).unwrap();
    let occ = std::fs::read_to_string(root.join("fire_occbin_sum.mod")).unwrap();
    assert!(analyze(&parse(&epi)).iter().any(|diag| diag.code == "E293"));
    assert!(analyze(&parse(&occ)).iter().any(|diag| diag.code == "E182"));
}
