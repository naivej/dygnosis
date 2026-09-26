//! D-open family locks: thin parse of named openers with no later owner.

use dygnosis::explain::known_codes;
use dygnosis::model::DerivSpec;
use dygnosis::{analyze, check_file, parse, Diagnostic};

fn fixture(rel: &str) -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn fixture_path(rel: &str) -> String {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
        .to_str()
        .expect("utf-8 fixture path")
        .to_string()
}

fn codes(diags: &[Diagnostic]) -> Vec<&str> {
    diags.iter().map(|d| d.code.as_str()).collect()
}

fn find<'a>(diags: &'a [Diagnostic], code: &str) -> &'a Diagnostic {
    diags
        .iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("expected {code}, got {:?}", codes(diags)))
}

fn quiet(diags: &[Diagnostic], code: &str) {
    assert!(
        diags.iter().all(|d| d.code != code),
        "expected no {code}, got {:?}",
        codes(diags)
    );
}

fn diags(rel: &str) -> Vec<Diagnostic> {
    analyze(&parse(&fixture(rel)))
}

fn file_diags(rel: &str) -> Vec<Diagnostic> {
    check_file(&fixture(rel), &fixture_path(rel))
}

fn ar1() -> &'static str {
    "var y; varexo e; parameters rho; rho = 0.9; model; y = rho*y(-1)+e; end;"
}

/// One locked fire: fixture, code, and the exact 7.1 message.
struct Fire {
    code: &'static str,
    fixture: &'static str,
    message: &'static str,
}

const FIRES: &[Fire] = &[
    Fire {
        code: "E286",
        fixture: "d_open/e286_with_epilogue_without_block.mod",
        message: "the 'with_epilogue' option cannot be specified when there is no 'epilogue' block",
    },
    Fire {
        code: "E287",
        fixture: "d_open/e287_epilogue_dup.mod",
        message: "in the 'epilogue' block, variable 'foo' is declared twice",
    },
    Fire {
        code: "E288",
        fixture: "d_open/e288_epilogue_unknown.mod",
        message: "Variable bar used in the epilogue block but was not declared.",
    },
    Fire {
        code: "E289",
        fixture: "d_open/e289_epilogue_exo.mod",
        message:
            "Symbol 'e' cannot be used inside the epilogue block, because it is an exogenous variable.",
    },
    Fire {
        code: "E290",
        fixture: "d_open/e290_epilogue_exo_det.mod",
        message:
            "Symbol 'ed' cannot be used inside the epilogue block, because it is an exogenous deterministic variable.",
    },
    Fire {
        code: "E291",
        fixture: "d_open/e291_epilogue_expectation.mod",
        message: "The 'expectation' operator is forbidden in 'epilogue'.",
    },
    Fire {
        code: "E292",
        fixture: "d_open/e292_epilogue_steady_state.mod",
        message: "The STEADY_STATE() operator is forbidden in epilogue block",
    },
    Fire {
        code: "E293",
        fixture: "d_open/e293_epilogue_sum.mod",
        message: "The SUM() operator is forbidden in epilogue block",
    },
    Fire {
        code: "E294",
        fixture: "d_open/e294_epilogue_outside.mod",
        message: "Symbol 'foo' cannot be used outside the epilogue block.",
    },
    Fire {
        code: "E295",
        fixture: "d_open/e295_change_type_unknown.mod",
        message: "Unknown variable zzz",
    },
    Fire {
        code: "E296",
        fixture: "d_open/e296_change_type_used.mod",
        message: "You cannot modify the type of symbol y after having used it in an expression",
    },
    Fire {
        code: "E297",
        fixture: "d_open/e297_ramsey_model_twice.mod",
        message: "Several 'ramsey_model' statements cannot appear in a given .mod file.",
    },
    Fire {
        code: "E298",
        fixture: "d_open/e298_ramsey_model_after_policy.mod",
        message: "A 'ramsey_model' statement cannot follow a 'ramsey_policy' statement.",
    },
    Fire {
        code: "E299",
        fixture: "d_open/e299_ramsey_policy_after_model.mod",
        message: "A 'ramsey_policy' statement cannot follow a 'ramsey_model' statement.",
    },
    Fire {
        code: "E300",
        fixture: "d_open/e300_ramsey_policy_twice.mod",
        message: "Several 'ramsey_policy' statements cannot appear in a given .mod file.",
    },
    Fire {
        code: "E301",
        fixture: "d_open/e301_planner_discount_model.mod",
        message: "ramsey_model: the 'planner_discount' option cannot be used when the 'optimal_policy_discount_factor' parameter is explicitly declared.",
    },
    Fire {
        code: "E302",
        fixture: "d_open/e302_planner_discount_policy.mod",
        message: "ramsey_policy: the 'planner_discount' option cannot be used when the 'optimal_policy_discount_factor' parameter is explicitly declared.",
    },
    Fire {
        code: "E303",
        fixture: "d_open/e303_dsge_prior_weight_parameter.mod",
        message: "dsge_prior_weight cannot be declared as a parameter. Use the dsge_var option in the estimation statement instead.",
    },
    Fire {
        code: "E305",
        fixture: "d_open/e305_includepath_not_string.mod",
        message: "File name does not evaluate to a string",
    },
    Fire {
        code: "E307",
        fixture: "d_open/e307_trend_twice.mod",
        message: "Trend variable A was declared twice.",
    },
    Fire {
        code: "E308",
        fixture: "d_open/e308_trend_listed_twice.mod",
        message: "Variable y was listed more than once as following a trend.",
    },
    Fire {
        code: "E309",
        fixture: "d_open/e309_deflator_nonstationary.mod",
        message: "The deflator contains a non-stationary endogenous variable. This is not allowed. Please use only stationary endogenous and/or {log_}trend_vars.",
    },
    Fire {
        code: "E310",
        fixture: "d_open/e310_trend_outside_model.mod",
        message: "Variable A not allowed outside model declaration, because it is a trend variable.",
    },
    Fire {
        code: "E311",
        fixture: "d_open/e311_fis_not_endo.mod",
        message: "filter_initial_state: rho should be an endogenous or exogenous variable",
    },
    Fire {
        code: "E312",
        fixture: "d_open/e312_fis_exo_no_lag.mod",
        message: "filter_initial_state: exogenous variable e must be provided with a lag",
    },
    Fire {
        code: "E313",
        fixture: "d_open/e313_fis_dup.mod",
        message: "filter_initial_state: (y, 0) declared twice",
    },
    Fire {
        code: "E314",
        fixture: "d_open/e314_fis_lag_mismatch.mod",
        message: "filter_initial_state: variable y does not appear in the model with the lag -3 (see the reference manual for the timing convention in 'filter_initial_state')",
    },
    Fire {
        code: "E315",
        fixture: "d_open/e315_optim_weights_dup.mod",
        message: "optim_weights: y declared twice",
    },
    Fire {
        code: "E316",
        fixture: "d_open/e316_optim_weights_pair_dup.mod",
        message: "optim_weights: pair of variables (y, z) declared twice",
    },
    Fire {
        code: "E317",
        fixture: "d_open/e317_optim_weights_not_endo.mod",
        message: "e is not endogenous.",
    },
    Fire {
        code: "E318",
        fixture: "d_open/e318_ramsey_constraints_two.mod",
        message: "The ramsey_constraints block contains two constraints for variable y",
    },
    Fire {
        code: "E319",
        fixture: "d_open/e319_ramsey_constraints_not_inequality.mod",
        message: "Ramsey constraint has an incorrect form: This expression is not an inequality",
    },
    Fire {
        code: "E320",
        fixture: "d_open/e320_ramsey_constraints_bad_bound.mod",
        message: "Ramsey constraint has an incorrect form: Bounds must not contain any endogenous or exogenous variable",
    },
    Fire {
        code: "E321",
        fixture: "d_open/e321_ramsey_constraints_chain.mod",
        message: "Ramsey constraint has an incorrect form:",
    },
    Fire {
        code: "E322",
        fixture: "d_open/e322_extfun_no_name.mod",
        message: "The 'name' option must be passed to external_function().",
    },
    Fire {
        code: "E323",
        fixture: "d_open/e323_extfun_empty_name.mod",
        message: "An argument must be passed to the 'name' option of the external_function() statement.",
    },
    Fire {
        code: "E324",
        fixture: "d_open/e324_extfun_second_named.mod",
        message: "If the second derivative is provided to the external_function command, the first derivative must also be provided.",
    },
    Fire {
        code: "E325",
        fixture: "d_open/e325_extfun_second_bare.mod",
        message: "If the second derivative is provided in the top-level function, the first derivative must also be provided in that function.",
    },
    Fire {
        code: "E326",
        fixture: "d_open/e326_extfun_nargs_mismatch.mod",
        message: "The number of arguments passed to the external_function() statement do not match the number of arguments passed to a previous call or declaration of the top-level function.",
    },
    Fire {
        code: "E327",
        fixture: "d_open/e327_extfun_first_deriv_mismatch.mod",
        message: "The first derivative function passed to the external_function() statement does not match the first derivative function passed to a previous call or declaration of the top-level function.",
    },
    Fire {
        code: "E328",
        fixture: "d_open/e328_extfun_first_top_second_named.mod",
        message: "If the first derivative is provided by the top-level function, the second derivative cannot be provided by any other external function.",
    },
    Fire {
        code: "E329",
        fixture: "d_open/e329_init2shocks_dup.mod",
        message:
            "Init2shocks(default): enogenous variable 'y' appears more than once in the init2shocks statement",
    },
    Fire {
        code: "E330",
        fixture: "d_open/e330_init2shocks_first_not_endo.mod",
        message: "init2shocks: rho should be an endogenous variable",
    },
    Fire {
        code: "E331",
        fixture: "d_open/e331_init2shocks_second_not_exo.mod",
        message: "init2shocks: rho should be an exogenous variable",
    },
    Fire {
        code: "E332",
        fixture: "d_open/e332_homotopy_not_param.mod",
        message: "homotopy_val: y should be a parameter or exogenous variable",
    },
    Fire {
        code: "E333",
        fixture: "d_open/e333_shock_groups_not_exo.mod",
        message: "shock_groups: rho should be an exogenous variable",
    },
    Fire {
        code: "E058",
        fixture: "d_open/e058_fis_undeclared.mod",
        message: "Variable 'zzz' in filter_initial_state is not declared.",
    },
    Fire {
        code: "E058",
        fixture: "d_open/e058_init2shocks_undeclared.mod",
        message: "Variable 'zzz' in init2shocks is not declared.",
    },
    Fire {
        code: "E058",
        fixture: "d_open/e058_homotopy_undeclared.mod",
        message: "Variable 'zzz' in homotopy_setup is not declared.",
    },
    Fire {
        code: "E058",
        fixture: "d_open/e058_shock_groups_undeclared.mod",
        message: "Variable 'zzz' in shock_groups is not declared.",
    },
];

/// Workspace-only fires: the file next to the `.mod` decides.
const WORKSPACE_FIRES: &[Fire] = &[
    Fire {
        code: "E304",
        fixture: "d_open/e304_includepath_missing_dir.mod",
        message: "missing_dir does not evaluate to a valid directory",
    },
    Fire {
        code: "E306",
        fixture: "d_open/e306_load_params_missing.mod",
        message: "Can't open nope.txt",
    },
    Fire {
        code: "W204",
        fixture: "d_open/w204_load_params_unknown.mod",
        message: "Unknown symbol zzz in w204_params.txt",
    },
];

#[test]
fn d_open_fires_in_analyze() {
    let mut failures = Vec::new();
    for fire in FIRES {
        let diags = diags(fire.fixture);
        match diags.iter().find(|d| d.code == fire.code) {
            None => failures.push(format!(
                "{}: no {} in {:?}",
                fire.fixture,
                fire.code,
                codes(&diags)
            )),
            Some(d) => {
                if d.message != fire.message {
                    failures.push(format!(
                        "{}: {} message {:?} != {:?}",
                        fire.fixture, fire.code, d.message, fire.message
                    ));
                }
                if d.severity != dygnosis::Severity::Error {
                    failures.push(format!("{}: {} is not an Error", fire.fixture, fire.code));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn d_open_workspace_fires_in_check_file() {
    let mut failures = Vec::new();
    for fire in WORKSPACE_FIRES {
        let diags = file_diags(fire.fixture);
        match diags.iter().find(|d| d.code == fire.code) {
            None => failures.push(format!(
                "{}: no {} in {:?}",
                fire.fixture,
                fire.code,
                codes(&diags)
            )),
            Some(d) => {
                if d.message != fire.message {
                    failures.push(format!(
                        "{}: {} message {:?} != {:?}",
                        fire.fixture, fire.code, d.message, fire.message
                    ));
                }
                let expected = if fire.code == "W204" {
                    dygnosis::Severity::Warning
                } else {
                    dygnosis::Severity::Error
                };
                if d.severity != expected {
                    failures.push(format!(
                        "{}: {} severity {:?} != {expected:?}",
                        fire.fixture, fire.code, d.severity
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));

    // The directory and the load file are file-relative: not library checks.
    for code in ["E304", "E306", "W204"] {
        for fire in WORKSPACE_FIRES.iter().filter(|f| f.code == code) {
            quiet(&diags(fire.fixture), code);
        }
    }
}

#[test]
fn quiet_files_stay_quiet() {
    let quiets: &[(&str, &[&str])] = &[
        (
            "d_open/quiet_epilogue.mod",
            &["E286", "E287", "E294", "E001"],
        ),
        ("d_open/quiet_change_type.mod", &["E295", "E296", "E001"]),
        (
            "d_open/quiet_trend.mod",
            &["E307", "E308", "E309", "E310", "E001"],
        ),
        (
            "d_open/quiet_fis.mod",
            &["E311", "E312", "E313", "E314", "E001"],
        ),
        (
            "d_open/quiet_optim_weights.mod",
            &["E315", "E316", "E317", "E001"],
        ),
        (
            "d_open/quiet_ramsey_constraints.mod",
            &["E318", "E319", "E320", "E321", "E001"],
        ),
        (
            "d_open/quiet_extfun.mod",
            &[
                "E322", "E323", "E324", "E325", "E326", "E327", "E328", "E001",
            ],
        ),
        (
            "d_open/quiet_init2shocks.mod",
            &["E329", "E330", "E331", "E001"],
        ),
        ("d_open/quiet_homotopy.mod", &["E332", "E001"]),
        ("d_open/quiet_shock_groups.mod", &["E333", "E001"]),
        ("d_open/quiet_bvar.mod", &["E001"]),
        (
            "d_open/quiet_heterogeneity.mod",
            &["E001", "E020", "E030", "W031"],
        ),
    ];
    let mut failures = Vec::new();
    for (fixture, expected_quiet) in quiets {
        let got = diags(fixture);
        for code in *expected_quiet {
            if got.iter().any(|d| d.code == *code) {
                failures.push(format!("{fixture}: unexpected {code} in {:?}", codes(&got)));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn varexo_det_pair_rules() {
    // Homotopy and `filter_initial_state` take `varexo_det`; `init2shocks`
    // second slot and `shock_groups` members do not.
    let init2 = analyze(&parse(
        "var y; varexo e; varexo_det ed; parameters rho; rho = 0.9; model; y = rho*y(-1)+e+ed; end; init2shocks; y ed; end;",
    ));
    let d = find(&init2, "E331");
    assert_eq!(d.message, "init2shocks: ed should be an exogenous variable");

    let groups = analyze(&parse(
        "var y; varexo e; varexo_det ed; parameters rho; rho = 0.9; model; y = rho*y(-1)+e+ed; end; shock_groups; g = ed; end;",
    ));
    let d = find(&groups, "E333");
    assert_eq!(
        d.message,
        "shock_groups: ed should be an exogenous variable"
    );

    let homotopy = analyze(&parse(
        "var y; varexo e; varexo_det ed; parameters rho; rho = 0.9; model; y = rho*y(-1)+e+ed; end; homotopy_setup; ed, 0, 1; end;",
    ));
    quiet(&homotopy, "E332");
}

#[test]
fn some_shipped_codes_still_fire() {
    // 02 keeps these: E203 (constraints without Ramsey), E103 / E204 (weights),
    // E279 / E280 (external function names as variables).
    let no_ramsey = analyze(&parse(
        "var y; varexo e; model; y = e; end; ramsey_constraints; y > 0; end;",
    ));
    find(&no_ramsey, "E203");

    let weights = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; y = rho*y(-1)+e; end; osr_params rho; osr;",
    ));
    find(&weights, "E103");

    let both = analyze(&parse(&fixture("w100/e204_osr_both.mod")));
    find(&both, "E204");

    let extfun = analyze(&parse(&fixture("d_block/e279_external_fn_outside.mod")));
    find(&extfun, "E279");

    let extfun_in = analyze(&parse(&fixture("d_block/e280_external_fn_inside.mod")));
    find(&extfun_in, "E280");

    // Declaring the name as a parameter is E303, never E219.
    let dpw = diags("d_open/e303_dsge_prior_weight_parameter.mod");
    find(&dpw, "E303");
    quiet(&dpw, "E219");
}

#[test]
fn registry_known_codes_include_shock_diagnostics() {
    assert_eq!(known_codes().len(), 381);
}

#[test]
fn epilogue_fills_and_does_not_leak() {
    let model = parse(&format!("{0} epilogue; foo = y; end;", ar1()));
    assert!(model.epilogue_block.is_some());
    assert_eq!(model.epilogue.len(), 1);
    assert_eq!(model.name(model.epilogue[0].name), "foo");
    assert!(model.epilogue[0].expr.is_some());
    assert!(model.helper_assignments.is_empty());
    assert!(model.endogenous.iter().all(|d| model.name(d.name) != "foo"));
    assert!(model.model_block.is_some());
}

#[test]
fn epilogue_operator_forms_are_parsed() {
    let model = parse(&format!(
        "{0} epilogue; foo = EXPECTATION(1)(y); bar = STEADY_STATE(y); baz = SUM(y); end;",
        ar1()
    ));
    assert_eq!(model.epilogue.len(), 3);
    assert!(model.epilogue.iter().all(|a| a.expr.is_some()));
}

#[test]
fn with_epilogue_option_recorded_on_decomposition() {
    for command in [
        "shock_decomposition(with_epilogue)",
        "realtime_shock_decomposition(with_epilogue)",
        "initial_condition_decomposition(with_epilogue)",
    ] {
        let model = parse(&format!("{0}\n{command};", ar1()));
        assert!(
            model.with_epilogue_span.is_some(),
            "{command} should record with_epilogue"
        );
        assert!(model.epilogue_block.is_none());
    }
    let plain = parse(&format!("{0}\nshock_decomposition;", ar1()));
    assert!(plain.with_epilogue_span.is_none());
}

#[test]
fn change_type_statement_is_stored() {
    let model = parse("var y; parameters rho; rho = 0.9; change_type(varexo) y;");
    assert_eq!(model.change_type_statements.len(), 1);
    let stmt = &model.change_type_statements[0];
    assert_eq!(stmt.names.len(), 1);
    assert_eq!(model.name(stmt.names[0].0), "y");
    assert!(stmt.span.end > stmt.span.start);
}

#[test]
fn trend_var_is_not_endogenous() {
    let model = parse(&format!("{0} trend_var(growth_factor=1.02) A, B;", ar1()));
    assert_eq!(model.trend_vars.len(), 2);
    assert_eq!(model.name(model.trend_vars[0].name), "A");
    assert!(model.trend_vars[0].growth.is_some());
    assert!(!model.trend_vars[0].log_trend);
    assert!(model.endogenous.iter().all(|d| model.name(d.name) != "A"));
}

#[test]
fn log_trend_var_sets_flag() {
    let model = parse(&format!(
        "{0} log_trend_var(log_growth_factor=0.02) A;",
        ar1()
    ));
    assert_eq!(model.trend_vars.len(), 1);
    assert!(model.trend_vars[0].log_trend);
}

#[test]
fn deflator_declaration_declares_endo_and_keeps_expr() {
    let text = fixture("d_open/quiet_trend.mod");
    let model = parse(&text);
    assert!(
        model.endogenous.iter().any(|d| model.name(d.name) == "y"
            && model.trend_vars.iter().any(|t| model.name(t.name) == "A")),
        "y should be endogenous"
    );
    assert_eq!(model.nonstationary_vars.len(), 1);
    assert_eq!(model.name(model.nonstationary_vars[0].name), "y");
    assert!(model.nonstationary_vars[0].deflator.is_some());
    assert!(!model.nonstationary_vars[0].log_deflator);
}

#[test]
fn var_heterogeneity_is_not_e001() {
    let got = diags("d_open/quiet_heterogeneity.mod");
    quiet(&got, "E001");
    quiet(&got, "E020");
    quiet(&got, "E030");
    quiet(&got, "W031");
    let model = parse(&fixture("d_open/quiet_heterogeneity.mod"));
    let yh = model
        .endogenous
        .iter()
        .find(|d| model.name(d.name) == "yh")
        .expect("yh should be endogenous");
    let (dim, _) = yh.heterogeneity.expect("yh should carry its dimension");
    assert_eq!(model.name(dim), "d");
    assert_eq!(model.heterogeneity_dimensions.len(), 1);
    assert_eq!(model.name(model.heterogeneity_dimensions[0].name), "d");
}

#[test]
fn filter_initial_state_has_own_vec() {
    let model = parse(&format!("{0} filter_initial_state; y(0) = 0; end;", ar1()));
    assert!(model.filter_initial_state_block.is_some());
    assert_eq!(model.filter_initial_state.len(), 1);
    assert_eq!(model.name(model.filter_initial_state[0].name), "y");
    assert_eq!(model.filter_initial_state[0].lag, 0);
    assert!(model.histval.is_empty());
    assert!(model.initval.is_empty());
}

#[test]
fn optim_weights_body_fills_and_keeps_flag() {
    let model = parse(&format!(
        "{0} osr_params rho; optim_weights; y 1; y, y 2; end;",
        ar1()
    ));
    assert!(model.has_optim_weights);
    assert_eq!(model.optim_weights.len(), 2);
    assert_eq!(model.name(model.optim_weights[0].first), "y");
    assert!(model.optim_weights[0].second.is_none());
    assert!(model.optim_weights[0].expr.is_some());
    assert_eq!(model.name(model.optim_weights[1].first), "y");
    assert_eq!(
        model.name(model.optim_weights[1].second.expect("pair")),
        "y"
    );
}

#[test]
fn ramsey_constraints_body_fills_and_keeps_span() {
    let model = parse(&format!(
        "{0} ramsey_model; ramsey_constraints; y > 0; 0 < y < 1; end;",
        ar1()
    ));
    assert!(model.ramsey_constraints_span.is_some());
    assert_eq!(model.ramsey_constraints.len(), 2);
    assert!(model.ramsey_constraints.iter().all(|c| c.expr.is_some()));
}

#[test]
fn external_function_options_are_parsed() {
    let model = parse(&format!(
        "{0} external_function(name=foo, nargs=2, first_deriv_provided='fd', second_deriv_provided);",
        ar1()
    ));
    assert_eq!(model.external_functions.len(), 1);
    let stmt = &model.external_functions[0];
    assert_eq!(model.name(stmt.name.expect("name").0), "foo");
    assert_eq!(stmt.nargs, Some(2));
    match stmt.first_deriv {
        Some(DerivSpec::Named(name, _)) => assert_eq!(model.name(name), "fd"),
        other => panic!("expected named first derivative, got {other:?}"),
    }
    assert!(matches!(stmt.second_deriv, Some(DerivSpec::Bare(_))));
    assert!(model
        .external_function_names
        .iter()
        .any(|n| model.name(*n) == "foo"));
}

#[test]
fn init2shocks_rows_and_group() {
    let model = parse(&format!("{0} init2shocks(name=g1); y e; y, e; end;", ar1()));
    assert_eq!(model.init2shocks_blocks.len(), 1);
    let block = &model.init2shocks_blocks[0];
    assert_eq!(block.group, "g1");
    assert_eq!(block.rows.len(), 2);
    assert_eq!(model.name(block.rows[0].endo), "y");
    assert_eq!(model.name(block.rows[0].exo), "e");
    assert_eq!(model.name(block.rows[1].exo), "e");

    let defaulted = parse(&format!("{0} init2shocks; y e; end;", ar1()));
    assert_eq!(defaulted.init2shocks_blocks[0].group, "default");
}

#[test]
fn homotopy_rows_and_last_is_optional() {
    let model = parse(&format!(
        "{0} homotopy_setup; rho, 0, 1; rho, 0.5; end;",
        ar1()
    ));
    assert_eq!(model.homotopy_rows.len(), 2);
    assert_eq!(model.name(model.homotopy_rows[0].name), "rho");
    assert_eq!(model.name(model.homotopy_rows[1].name), "rho");
}

#[test]
fn shock_groups_members() {
    let model = parse(&format!(
        "{0} shock_groups; g = e; 'grp' = e, e; end;",
        ar1()
    ));
    assert_eq!(model.shock_groups.len(), 2);
    assert_eq!(model.shock_groups[0].members.len(), 1);
    assert_eq!(model.name(model.shock_groups[0].members[0].0), "e");
    assert_eq!(model.shock_groups[1].members.len(), 2);
}

#[test]
fn sims_bvar_statements_are_not_e001() {
    let text = fixture("d_open/quiet_bvar.mod");
    let got = analyze(&parse(&text));
    quiet(&got, "E001");
    assert!(parse(&text).bvar_present);
    assert!(!parse(ar1()).bvar_present);
}

#[test]
fn load_params_records_filename() {
    let model = parse(&format!(
        "{0} load_params_and_steady_state('params.txt');",
        ar1()
    ));
    let (file, span) = model.load_params_file.expect("load_params_file");
    assert_eq!(file, "params.txt");
    assert!(span.end > span.start);
}
