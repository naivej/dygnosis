//! D-walk family locks: options / flags / symbol lists on recorded commands.

use std::path::PathBuf;

use dygnosis::explain::known_codes;
use dygnosis::{analyze, parse, Diagnostic};

fn fixture(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n")
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

#[test]
fn registry_known_codes_include_shock_diagnostics() {
    assert_eq!(known_codes().len(), 338);
}

#[test]
fn estimation_dsge_var_datafile_bayesian_irf() {
    let model = parse("estimation(dsge_var, datafile='d.csv', bayesian_irf);");
    assert!(model.dsge_var_estimated.is_some());
    assert!(model.dsge_var_calibrated.is_none());
    assert!(model.estimation_datafile_span.is_some());
    assert!(model.bayesian_irf_span.is_some());
    assert_eq!(model.estimation_dsge_var_stmts.len(), 1);
    assert!(model.estimation_dsge_var_stmts[0].estimated.is_some());
    assert!(model.estimation_dsge_var_stmts[0].calibrated.is_none());
}

#[test]
fn dsge_var_bare_vs_calibrated() {
    let bare = parse("estimation(dsge_var);");
    assert!(bare.dsge_var_estimated.is_some());
    assert!(bare.dsge_var_calibrated.is_none());
    let cal = parse("estimation(dsge_var=0.5);");
    assert!(cal.dsge_var_estimated.is_none());
    assert!(cal.dsge_var_calibrated.is_some());
    let inf = parse("estimation(dsge_var=Inf);");
    assert!(inf.dsge_var_calibrated.is_some());
    assert!(inf.dsge_var_estimated.is_none());
}

#[test]
fn data_opener_presence_not_database() {
    let data = parse("data(file='x.csv');");
    assert_eq!(data.data_statements.len(), 1);
    assert!(data.data_statements[0].has_file_or_series());
    assert!(data.helper_assignments.is_empty());
    assert!(data.param_assignments.is_empty());
    let nobs_only = parse("data(nobs=10);");
    assert_eq!(nobs_only.data_statements.len(), 1);
    assert!(!nobs_only.data_statements[0].has_file_or_series());
    let db = parse("database foo;");
    assert!(db.data_statements.is_empty());
}

#[test]
fn prior_function_opener_without_function() {
    let missing = parse("prior_function(sampling_draws=10);");
    assert!(missing.prior_function_span.is_some());
    assert!(missing.prior_function_has_parens);
    assert!(!missing.prior_function_has_function);
    let present = parse("prior_function(function=foo);");
    assert!(present.prior_function_span.is_some());
    assert!(present.prior_function_has_function);
}

#[test]
fn sensitivity_identification_eq_1_does_not_set_identification_span() {
    let model = parse("sensitivity(identification=1);");
    assert!(model.sensitivity_span.is_some());
    assert!(model.sensitivity_identification_eq_1.is_some());
    assert!(model.identification_span.is_none());
    let other = parse("sensitivity(identification=2);");
    assert!(other.sensitivity_identification_eq_1.is_none());
    assert!(other.identification_span.is_none());
}

#[test]
fn identification_order_and_max_dim_cova_group() {
    let order = parse("identification(order=4);");
    assert_eq!(order.identification_order.map(|(n, _)| n), Some(4));
    let zero = parse("identification(max_dim_cova_group=0);");
    assert_eq!(zero.max_dim_cova_group.map(|(n, _)| n), Some(0));
}

#[test]
fn discretionary_order_keeps_instruments_flag() {
    let src = "var y; varexo e; model; y = e; end; planner_objective y; ";
    let model = parse(&format!(
        "{src}discretionary_policy(order=2, instruments=(y));"
    ));
    assert!(model.discretionary_has_instruments_option);
    assert_eq!(model.discretionary_order.map(|(n, _)| n), Some(2));
    assert_eq!(model.instruments.len(), 1);
}

#[test]
fn stoch_simul_filters_and_trailing_symbols() {
    let filters = parse("stoch_simul(hp_filter=1600, bandpass_filter);");
    assert!(filters.stoch_simul_hp_filter.is_some());
    assert!(filters.stoch_simul_bandpass_filter.is_some());
    assert!(filters.stoch_simul_one_sided_hp_filter.is_none());
    let trail = parse("var y z; stoch_simul y, z;");
    assert_eq!(trail.command_symbols.len(), 2);
    assert_eq!(trail.command_symbols[0].command, "stoch_simul");
    assert_eq!(trail.name(trail.command_symbols[0].name), "y");
    assert_eq!(trail.name(trail.command_symbols[1].name), "z");
}

#[test]
fn planner_discount_keeps_expr() {
    let model = parse(
        "var y; varexo e; parameters beta; model; y = e; end; planner_objective y; ramsey_model(planner_discount=beta);",
    );
    assert!(model.planner_discount_expr.is_some());
}

#[test]
fn estimated_params_init_use_calibration_skips_body() {
    let model = parse("estimated_params_init(use_calibration); alpha, 0.5; end;");
    assert!(model.estimated_params_init_use_calibration.is_some());
    assert!(model.estimated_params.is_empty());
}

#[test]
fn estimation_other_option_flags() {
    let model = parse(
        "estimation(dsge_varlag=4, mode_file='m.mat', mh_tune_jscale, mh_jscale=0.4, mh_tune_guess=0.2, filter_algorithm=gmf, proposal_approximation=montecarlo, distribution_approximation=montecarlo, dataseries=foo);",
    );
    assert!(model.dsge_varlag_span.is_some());
    assert!(model.estimation_mode_file_span.is_some());
    assert!(model.mh_tune_jscale_span.is_some());
    assert!(model.mh_jscale_span.is_some());
    assert!(model.mh_tune_guess_span.is_some());
    assert!(model.filter_algorithm_gmf_span.is_some());
    assert!(model.proposal_approximation_montecarlo_span.is_some());
    assert!(model.distribution_approximation_montecarlo_span.is_some());
    assert!(model.estimation_dataseries_span.is_some());
    assert!(model.estimation_datafile_span.is_none());
}

#[test]
fn restriction_fname_and_osr_params_symbols() {
    let sbvar = parse("sbvar(restriction_fname=foo);");
    assert!(sbvar.restriction_fname_span.is_some());
    let osr = parse("osr_params rho, z;");
    assert_eq!(osr.osr_params.len(), 2);
    assert_eq!(osr.command_symbols.len(), 2);
    assert_eq!(osr.command_symbols[0].command, "osr_params");
    assert_eq!(osr.name(osr.command_symbols[0].name), "rho");
    assert_eq!(osr.name(osr.command_symbols[1].name), "z");
}

#[test]
fn two_estimation_dsge_var_forms_are_per_statement() {
    let model = parse(
        "estimation(dsge_var, datafile='d.csv'); estimation(dsge_var=0.5, datafile='d.csv');",
    );
    assert!(model.dsge_var_estimated.is_some());
    assert!(model.dsge_var_calibrated.is_some());
    assert_eq!(model.estimation_dsge_var_stmts.len(), 2);
    assert!(model.estimation_dsge_var_stmts[0].estimated.is_some());
    assert!(model.estimation_dsge_var_stmts[1].calibrated.is_some());
}

const E219_MSG: &str = "dsge_prior_weight should not be declared as a model variable / parameter when the dsge_var option is passed to the estimation statement.";
const E220_MSG: &str = "When estimating a DSGE-Var and the bayesian_irf option is passed to the estimation statement, the number of shocks must equal the number of observed variables.";
const E221_MSG: &str = "When estimating a DSGE-Var, the number of shocks must be greater than or equal to the number of observed variables.";

#[test]
fn e219_declared_dsge_prior_weight_with_dsge_var() {
    let diags = analyze(&parse(&fixture(
        "d_walk/e219_dsge_prior_weight_declared.mod",
    )));
    assert_eq!(find(&diags, "E219").message, E219_MSG);
}

#[test]
fn e219_undeclared_dsge_prior_weight_is_quiet() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.5; model; y = rho * y(-1) + e; end; estimation(dsge_var=0.5, datafile='d.csv');",
    ));
    quiet(&diags, "E219");
}

#[test]
fn e220_bayesian_irf_unequal_counts() {
    let diags = analyze(&parse(&fixture("d_walk/e220_bayesian_irf_counts.mod")));
    assert_eq!(find(&diags, "E220").message, E220_MSG);
}

#[test]
fn e221_shocks_lt_varobs() {
    let diags = analyze(&parse(&fixture("d_walk/e221_shocks_lt_varobs.mod")));
    assert_eq!(find(&diags, "E221").message, E221_MSG);
}

const E222_MSG: &str = "When estimating a DSGE-VAR model and estimating the weight of the prior, dsge_prior_weight must be referenced in the estimated_params block.";
const E223_MSG: &str = "If dsge_prior_weight is in the estimated_params block, the prior weight cannot be calibrated via the dsge_var option in the estimation statement.";
const E224_MSG: &str = "If dsge_prior_weight is in the estimated_params block, the dsge_var option must be passed to the estimation statement.";
const E225_MSG: &str = "The estimation statement requires a dsge_var option to be passed if the dsge_varlag option is passed.";
const E226_MSG: &str = "An estimation statement cannot take more than one dsge_var option.";
const E227_MSG: &str = "The estimation statement requires a data file to be supplied via the datafile option.";
const E228_MSG: &str = "The mode_file option of the estimation statement is incompatible with the use_calibration option of the estimated_params_init block.";
const E229_MSG: &str = "The mh_tune_jscale and mh_jscale options of the estimation statement are incompatible.";
const E230_MSG: &str = "The option mh_tune_guess in estimation statement cannot be used without option mh_tune_jscale.";
const E231_MSG: &str = "The filter_algorithm=gmf option is incompatible with proposal_approximation=montecarlo in the estimation statement.";
const E232_MSG: &str = "The filter_algorithm=gmf option is incompatible with distribution_approximation=montecarlo in the estimation statement.";
const E234_MSG: &str = "both the 'prior_function' and 'posterior_function' commands require the 'function' option";

fn fire(rel: &str, code: &str, msg: &str) {
    let diags = analyze(&parse(&fixture(rel)));
    assert_eq!(find(&diags, code).message, msg);
}

#[test]
fn e222_through_e234_estimation_check() {
    fire(
        "d_walk/e222_dsge_var_missing_weight.mod",
        "E222",
        E222_MSG,
    );
    fire(
        "d_walk/e223_weight_and_calibrated.mod",
        "E223",
        E223_MSG,
    );
    fire(
        "d_walk/e224_weight_without_dsge_var.mod",
        "E224",
        E224_MSG,
    );
    fire(
        "d_walk/e225_dsge_varlag_without_dsge_var.mod",
        "E225",
        E225_MSG,
    );
    fire(
        "d_walk/e226_two_estimation_dsge_var.mod",
        "E226",
        E226_MSG,
    );
    fire("d_walk/e227_estimation_no_data.mod", "E227", E227_MSG);
    fire(
        "d_walk/e228_mode_file_use_calibration.mod",
        "E228",
        E228_MSG,
    );
    fire(
        "d_walk/e229_mh_tune_jscale_mh_jscale.mod",
        "E229",
        E229_MSG,
    );
    fire("d_walk/e230_mh_tune_guess_alone.mod", "E230", E230_MSG);
    fire(
        "d_walk/e231_gmf_proposal_montecarlo.mod",
        "E231",
        E231_MSG,
    );
    fire(
        "d_walk/e232_gmf_distribution_montecarlo.mod",
        "E232",
        E232_MSG,
    );
    let e233 = analyze(&parse(&fixture(
        "d_walk/e233_estimated_planner_discount.mod",
    )));
    assert_eq!(
        find(&e233, "E233").message,
        "It is not possible to estimate a parameter (beta) that appears in the discount factor of the planner (i.e. in the 'planner_discount' option)."
    );
    fire(
        "d_walk/e234_prior_function_no_function.mod",
        "E234",
        E234_MSG,
    );
}

#[test]
fn e227_datafile_or_data_opener_quiet_database_does_not_silence() {
    let preamble =
        "var y; varexo e; parameters rho; rho = 0.5; model; y = rho * y(-1) + e; end; ";
    quiet(
        &analyze(&parse(&format!("{preamble}estimation(datafile='d.csv');"))),
        "E227",
    );
    quiet(
        &analyze(&parse(&format!("{preamble}data(file='x.csv'); estimation;"))),
        "E227",
    );
    let db = analyze(&parse(&format!("{preamble}database foo; estimation;")));
    assert_eq!(find(&db, "E227").message, E227_MSG);
    let series = analyze(&parse(&format!(
        "{preamble}estimation(dataseries=foo);"
    )));
    assert_eq!(find(&series, "E227").message, E227_MSG);
}

#[test]
fn e226_same_statement_two_dsge_var_forms_is_quiet() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.5; model; y = rho * y(-1) + e; end; estimation(dsge_var, dsge_var=0.5, datafile='d.csv');",
    ));
    quiet(&diags, "E226");
}

const E235_MSG: &str = "discretionary_policy: order > 1 is not yet implemented";
const E236_MSG: &str = "the order option of identification command must be between 1 and 3";
const E237_MSG: &str = "The max_dim_cova_group option to identification only accepts integers > 0.";
const E238_MSG: &str = "stoch_simul: can only use one of HP, one-sided HP, and bandpass filters";
const W201_MSG: &str = "restriction_fname is now deprecated, and may be removed in a future version of Dynare. Use svar_identification instead.";

#[test]
fn e235_through_w201_policy_ident_filters() {
    fire("d_walk/e235_disc_order.mod", "E235", E235_MSG);
    fire("d_walk/e236_identification_order.mod", "E236", E236_MSG);
    fire("d_walk/e237_max_dim_cova_group.mod", "E237", E237_MSG);
    fire("d_walk/e238_stoch_simul_filters.mod", "E238", E238_MSG);
    let w201 = analyze(&parse(&fixture("d_walk/w201_restriction_fname.mod")));
    let d = find(&w201, "W201");
    assert_eq!(d.message, W201_MSG);
    assert_eq!(d.severity, dygnosis::Severity::Warning);
}

#[test]
fn e238_ramsey_policy_filters_are_not_e238() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.5; model; y = rho * y(-1) + e; end; planner_objective y; ramsey_policy(hp_filter=1600, bandpass_filter);",
    ));
    quiet(&diags, "E238");
}

#[test]
fn e239_e240_w202_symbol_lists() {
    fire(
        "d_walk/e239_stoch_simul_z.mod",
        "E239",
        "stoch_simul: Variable z was not declared.",
    );
    fire(
        "d_walk/e240_stoch_simul_rho.mod",
        "E240",
        "stoch_simul: Variable rho is not one of {endogenous}",
    );
    let w202 = analyze(&parse(&fixture("d_walk/w202_stoch_simul_dup.mod")));
    let d = find(&w202, "W202");
    assert_eq!(
        d.message,
        "In stoch_simul: y found more than once in symbol list. Removing all but first occurrence."
    );
    assert_eq!(d.severity, dygnosis::Severity::Warning);
    quiet(&w202, "E239");
    quiet(&w202, "E240");

    let preamble =
        "var y; varexo e; parameters rho; rho = 0.5; model; y = rho * y(-1) + e; end; ";
    // Dynare runs removeDuplicates on one statement's list, so the same name on
    // two stoch_simul statements is not a duplicate.
    let two_stmts = analyze(&parse(&format!("{preamble}stoch_simul y; stoch_simul y;")));
    quiet(&two_stmts, "W202");
    let est = analyze(&parse(&format!(
        "{preamble}estimation(datafile='d.csv') z;"
    )));
    assert_eq!(
        find(&est, "E239").message,
        "estimation: Variable z was not declared."
    );
    let osr = analyze(&parse(&format!("{preamble}osr_params rho, z;")));
    assert_eq!(
        find(&osr, "E239").message,
        "osr: Variable z was not declared."
    );
    quiet(&osr, "E240");

    let inst = analyze(&parse(&format!(
        "{preamble}planner_objective y; ramsey_model(instruments=(z));"
    )));
    assert!(
        inst.iter().any(|d| d.code == "E101"),
        "instruments stay E101, got {:?}",
        codes(&inst)
    );
    quiet(&inst, "E239");

    let aux = analyze(&parse(&format!("{preamble}stoch_simul AUX_ENDO_1;")));
    quiet(&aux, "E239");
    quiet(&aux, "W186");
}
