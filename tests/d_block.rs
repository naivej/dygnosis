//! D-block family locks: completeness / duplicates / parse-time on trees we have.

use dygnosis::explain::known_codes;
use dygnosis::expr::ExprKind;
use dygnosis::model::{EstimatedParamKind, ShockKind};
use dygnosis::{analyze, block_openers, parse, Diagnostic, Severity};

fn fixture(rel: &str) -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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

fn ar1() -> &'static str {
    "var y; varexo e; parameters rho; rho = 0.9; model; y = rho*y(-1)+e; end;"
}

const E241_EXO: &str = "You have not set the following exogenous variables in endval: e";
const E241_ENDO: &str = "You have not set the following endogenous variables in histval: y";
const E242_MSG: &str = "histval: the lag on y should be less than or equal to 0";
const E243_MSG: &str = "hist_val: (y, 0) declared twice";

#[test]
fn registry_known_codes_grew_to_307() {
    assert_eq!(known_codes().len(), 307);
}

#[test]
fn histval_fills_entries_not_initval() {
    let model = parse(
        "var y; varexo e; parameters rho; model; y = rho*y(-1)+e; end; histval(all_values_required); y(0)=0; end;",
    );
    assert!(model.histval_all_values_required);
    assert!(model.histval_block.is_some());
    assert_eq!(model.histval.len(), 1);
    assert_eq!(model.name(model.histval[0].name), "y");
    assert_eq!(model.histval[0].lag, 0);
    assert!(model.histval[0].expr.is_some());
    assert!(model.initval.is_empty());
    assert!(model
        .helper_assignments
        .iter()
        .all(|a| model.name(a.name) != "y"));
    assert!(!model.initval_all_values_required);
}

#[test]
fn estimated_params_init_fills_and_keeps_use_calibration() {
    let model = parse("parameters rho; estimated_params_init(use_calibration); rho, 0.5; end;");
    assert!(model.estimated_params_init_use_calibration.is_some());
    assert_eq!(model.estimated_params_init.len(), 1);
    assert_eq!(model.name(model.estimated_params_init[0].name), "rho");
    assert!(model.estimated_params.is_empty());
}

#[test]
fn osr_params_bounds_fills() {
    let model = parse("parameters rho; osr_params_bounds; rho, 0, 1; end;");
    assert!(model.osr_params_bounds_span.is_some());
    assert_eq!(model.osr_params_bounds.len(), 1);
    assert_eq!(model.name(model.osr_params_bounds[0].name), "rho");
}

#[test]
fn planner_objective_expr_has_lead() {
    let model = parse("var y; varexo e; model; y = e; end; planner_objective y(+1);");
    let id = model
        .planner_objective_expr
        .expect("planner_objective_expr");
    match &model.exprs.get(id).kind {
        ExprKind::Ident { timing, .. } => assert_eq!(*timing, 1),
        other => panic!("expected Ident lead, got {other:?}"),
    }
}

#[test]
fn varexobs_records() {
    let model = parse("varexo e; varexobs e;");
    assert_eq!(model.varexobs.len(), 1);
    assert_eq!(model.name(model.varexobs[0].name), "e");
    assert_eq!(model.varexobs_statement_count, 1);
}

#[test]
fn generate_irfs_records() {
    let model = parse("varexo e; generate_irfs; a, e=1; end;");
    assert_eq!(model.generate_irfs.len(), 1);
    assert_eq!(model.name(model.generate_irfs[0].name), "a");
    assert_eq!(model.generate_irfs[0].exos.len(), 1);
    assert_eq!(model.name(model.generate_irfs[0].exos[0].0), "e");
}

#[test]
fn shock_var_stderr_is_stderr_kind() {
    let model = parse("varexo e; shocks; var e; stderr 0.1; end;");
    assert_eq!(model.shock_stmts.len(), 1);
    match &model.shock_stmts[0].kind {
        ShockKind::Stderr(n) => assert_eq!(model.name(*n), "e"),
        other => panic!("expected Stderr, got {other:?}"),
    }
}

#[test]
fn estimated_params_skew_kind() {
    let model = parse("varexo e; estimated_params; skew e, 0; end;");
    assert_eq!(model.estimated_params.len(), 1);
    assert_eq!(model.estimated_params[0].kind, EstimatedParamKind::Skew);
    assert_eq!(model.name(model.estimated_params[0].name), "e");
}

#[test]
fn option_twice_same_statement_dsge_var() {
    let model = parse("estimation(dsge_var, dsge_var=0.5);");
    assert!(
        model
            .option_twice
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case("dsge_var")),
        "expected dsge_var twice, got {:?}",
        model.option_twice
    );
}

#[test]
fn namespace_qualified_dot_pair() {
    let model = parse("var y; varexo e; model; y = self.y; end;");
    assert!(
        model
            .namespace_qualified
            .iter()
            .any(|(s, _)| s == "self.y"),
        "expected self.y, got {:?}",
        model.namespace_qualified
    );
}

#[test]
fn log_zero_records_fold_error() {
    let model = parse("var y; varexo e; model; y = log(0); end;");
    assert!(
        model
            .const_fold_errors
            .iter()
            .any(|(_, code, msg)| *code == "E276" || msg.contains("log(0)")),
        "expected log(0) fold error, got {:?}",
        model.const_fold_errors
    );
}

#[test]
fn e241_missing_exo_uses_endval_typo() {
    let diags = analyze(&parse(&fixture("d_block/e241_histval_missing_exo.mod")));
    quiet(&diags, "E218");
    assert_eq!(find(&diags, "E241").message, E241_EXO);
}

#[test]
fn e241_missing_endo_says_histval() {
    let diags = analyze(&parse(&format!(
        "{src} histval(all_values_required); e(0)=0; end;",
        src = ar1()
    )));
    quiet(&diags, "E218");
    assert_eq!(find(&diags, "E241").message, E241_ENDO);
}

#[test]
fn e241_complete_histval_is_quiet() {
    let diags = analyze(&parse(&format!(
        "{src} histval(all_values_required); y(0)=0; e(0)=0; end;",
        src = ar1()
    )));
    quiet(&diags, "E241");
    quiet(&diags, "E218");
}

#[test]
fn e218_histval_quiet_fixture_stays_quiet_for_e218() {
    let diags = analyze(&parse(&fixture("d_check/e218_histval_quiet.mod")));
    quiet(&diags, "E218");
    assert_eq!(find(&diags, "E241").message, E241_EXO);
}

#[test]
fn e242_histval_positive_lag() {
    let diags = analyze(&parse(&fixture("d_block/e242_histval_lag.mod")));
    assert_eq!(find(&diags, "E242").message, E242_MSG);
}

#[test]
fn e243_histval_duplicate_pair() {
    let diags = analyze(&parse(&fixture("d_block/e243_histval_dup.mod")));
    assert_eq!(find(&diags, "E243").message, E243_MSG);
}

#[test]
fn e058_histval_unknown() {
    let diags = analyze(&parse(&format!("{src} histval; z(0)=0; end;", src = ar1())));
    let d = find(&diags, "E058");
    assert!(
        d.message.contains("z"),
        "expected z in E058, got {}",
        d.message
    );
}

#[test]
fn e059_histval_parameter() {
    let diags = analyze(&parse(&format!(
        "{src} histval; rho(0)=0; end;",
        src = ar1()
    )));
    assert_eq!(
        find(&diags, "E059").message,
        "rho is neither endogenous or exogenous."
    );
}

const E244_MSG: &str = "in `estimated_params' block, the symbol rho is declared twice.";
const E245_MSG: &str = "in `estimated_params' block, the stderr of e is declared twice.";
const E246_MSG: &str = "in `estimated_params' block, the correlation between e and e2 is declared twice.";
const E247_MSG: &str = "in `estimated_params' block, the skewness of e is declared twice.";
const E250_MSG: &str =
    "The prior density is not defined for the beta distribution when the mean = standard deviation = 0.5.";

fn ep_model(body: &str) -> String {
    format!(
        "var y; varexo e e2; parameters rho alpha; rho = 0.9; alpha = 0.5; model; y = rho*y(-1)+e+e2; end; estimated_params; {body} end;"
    )
}

#[test]
fn e244_duplicate_symbol() {
    let diags = analyze(&parse(&ep_model("rho; rho;")));
    assert_eq!(find(&diags, "E244").message, E244_MSG);
}

#[test]
fn e245_duplicate_stderr() {
    let diags = analyze(&parse(&ep_model("stderr e; stderr e;")));
    assert_eq!(find(&diags, "E245").message, E245_MSG);
}

#[test]
fn e246_duplicate_corr() {
    let diags = analyze(&parse(&ep_model("corr e, e2, 0.1; corr e2, e, 0.2;")));
    assert_eq!(find(&diags, "E246").message, E246_MSG);
}

#[test]
fn e247_duplicate_skew() {
    let diags = analyze(&parse(&ep_model("skew e, 0; skew e, 0.1;")));
    assert_eq!(find(&diags, "E247").message, E247_MSG);
}

#[test]
fn e248_value_used() {
    let diags = analyze(&parse(&ep_model("rho, 0.5; alpha, rho;")));
    let msg = &find(&diags, "E248").message;
    assert!(
        msg.contains("the value of estimated parameter rho is used"),
        "{msg}"
    );
}

#[test]
fn e249_skew_on_endo() {
    let diags = analyze(&parse(&ep_model("skew y, 0;")));
    assert!(
        find(&diags, "E249")
            .message
            .contains("skewness can only be specified for exogenous variables")
    );
    quiet(&diags, "E093");
}

#[test]
fn e249_skew_on_param() {
    let diags = analyze(&parse(&ep_model("skew rho, 0;")));
    assert!(
        find(&diags, "E249")
            .message
            .contains("skewness can only be specified for exogenous variables")
    );
    quiet(&diags, "E093");
}

#[test]
fn unknown_skew_is_e093_not_e249() {
    let diags = analyze(&parse(&ep_model("skew zzz, 0;")));
    assert!(
        find(&diags, "E093").message.contains("skew 'zzz'"),
        "{diags:?}"
    );
    quiet(&diags, "E249");
}

#[test]
fn e250_beta_half_half() {
    let diags = analyze(&parse(&ep_model("rho, 0.5, 0, 1, beta_pdf, 0.5, 0.5;")));
    assert_eq!(find(&diags, "E250").message, E250_MSG);
}

#[test]
fn estimated_params_quiet_legal() {
    let diags = analyze(&parse(&ep_model("rho, 0.5, 0, 1, beta_pdf, 0.4, 0.1;")));
    quiet(&diags, "E244");
    quiet(&diags, "E248");
    quiet(&diags, "E249");
    quiet(&diags, "E250");
}

#[test]
fn e244_init_block_name() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; y = rho*y(-1)+e; end; estimated_params_init; rho; rho; end;",
    ));
    assert_eq!(
        find(&diags, "E244").message,
        "in `estimated_params_init' block, the symbol rho is declared twice."
    );
}

#[test]
fn e249_e250_do_not_fire_on_init() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; y = rho*y(-1)+e; end; estimated_params_init; skew y, 0; rho, 0.5, 0, 1, beta_pdf, 0.5, 0.5; end;",
    ));
    quiet(&diags, "E249");
    quiet(&diags, "E250");
}

fn ramsey_ar1() -> String {
    format!("{src} ramsey_model;", src = ar1())
}

#[test]
fn e251_planner_exo() {
    let diags = analyze(&parse(&format!(
        "{src} planner_objective e;",
        src = ramsey_ar1()
    )));
    assert!(
        find(&diags, "E251")
            .message
            .contains("You cannot include exogenous variables")
    );
}

#[test]
fn e252_planner_lead() {
    let diags = analyze(&parse(&format!(
        "{src} planner_objective y(+1);",
        src = ramsey_ar1()
    )));
    assert_eq!(
        find(&diags, "E252").message,
        "Leads and lags on variables are forbidden in 'planner_objective'."
    );
}

#[test]
fn e253_planner_model_local() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; #z = y; y = rho*y(-1)+e; end; ramsey_model; planner_objective z;",
    ));
    assert_eq!(
        find(&diags, "E253").message,
        "Model local variable z cannot be used in 'planner_objective'."
    );
}

#[test]
fn planner_objective_y_quiet() {
    let diags = analyze(&parse(&format!(
        "{src} planner_objective y;",
        src = ramsey_ar1()
    )));
    quiet(&diags, "E251");
    quiet(&diags, "E252");
    quiet(&diags, "E253");
}

#[test]
fn w203_several_osr_params() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; y = rho*y(-1)+e; end; planner_objective y; osr_params rho; osr_params rho; osr;",
    ));
    assert_eq!(
        find(&diags, "W203").message,
        "You have more than one osr_params statement in the .mod file."
    );
}

#[test]
fn e254_bounds_before_params() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; y = rho*y(-1)+e; end; planner_objective y; osr; osr_params_bounds; rho, 0, 1; end; osr_params rho;",
    ));
    assert_eq!(
        find(&diags, "E254").message,
        "you must have an osr_params statement before the osr_params_bounds block."
    );
}

#[test]
fn e255_osr_bounds_not_param() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; y = rho*y(-1)+e; end; planner_objective y; osr; osr_params rho; osr_params_bounds; y, 0, 1; end;",
    ));
    assert_eq!(
        find(&diags, "E255").message,
        "y must be a parameter to be used in the osr_bounds block"
    );
}

#[test]
fn osr_bounds_after_params_quiet() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; y = rho*y(-1)+e; end; planner_objective y; osr; osr_params rho; osr_params_bounds; rho, 0, 1; end;",
    ));
    quiet(&diags, "E254");
    quiet(&diags, "E255");
    quiet(&diags, "W203");
}

#[test]
fn e256_tag_twice() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; [name='a', name='b'] y = rho*y(-1)+e; end;",
    ));
    assert_eq!(
        find(&diags, "E256").message,
        "Tag 'name' cannot be used twice for the same equation"
    );
}

#[test]
fn e257_default_eq_tag() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; [name='2'] y = rho*y(-1)+e; y + y = 0; end;",
    ));
    assert!(
        find(&diags, "E257")
            .message
            .contains("Error creating default equation tag")
    );
}

#[test]
fn e258_several_varobs() {
    let diags = analyze(&parse(&format!(
        "{src} varobs y; varobs y;",
        src = ar1()
    )));
    assert_eq!(
        find(&diags, "E258").message,
        "varobs: you cannot have several 'varobs' statements in the same MOD file"
    );
}

#[test]
fn e259_several_varexobs() {
    let diags = analyze(&parse(&format!(
        "{src} varexobs e; varexobs e;",
        src = ar1()
    )));
    assert_eq!(
        find(&diags, "E259").message,
        "varexobs: you cannot have several 'varexobs' statements in the same MOD file"
    );
}

#[test]
fn e260_varexobs_not_exo() {
    let diags = analyze(&parse(&format!("{src} varexobs y;", src = ar1())));
    assert_eq!(
        find(&diags, "E260").message,
        "varexobs: y is not an exogenous variable"
    );
}

#[test]
fn e261_trends_twice() {
    let diags = analyze(&parse(&format!(
        "{src} varobs y; observation_trends; y(1); y(1); end;",
        src = ar1()
    )));
    assert_eq!(
        find(&diags, "E261").message,
        "observation_trends: y declared twice"
    );
}

#[test]
fn varobs_varexobs_quiet() {
    let diags = analyze(&parse(&format!(
        "{src} varobs y; varexobs e;",
        src = ar1()
    )));
    quiet(&diags, "E258");
    quiet(&diags, "E259");
    quiet(&diags, "E260");
}

#[test]
fn e262_mcp_lhs_not_var() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; [mcp = '0 > 0'] y = rho*y(-1)+e; end;",
    ));
    assert_eq!(
        find(&diags, "E262").message,
        "Left-hand side of expression in 'mcp' tag is not a variable"
    );
}

#[test]
fn e263_mcp_lhs_not_endo() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; [mcp = 'e > 0'] y = rho*y(-1)+e; end;",
    ));
    assert_eq!(
        find(&diags, "E263").message,
        "Left-hand side of expression in 'mcp' tag is not an endogenous variable"
    );
}

#[test]
fn e264_mcp_rhs_not_constant() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; [mcp = 'y > rho'] y = rho*y(-1)+e; end;",
    ));
    assert_eq!(
        find(&diags, "E264").message,
        "Right-hand side of expression in 'mcp' tag should be a constant"
    );
}

#[test]
fn e265_mcp_no_inequality() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; [mcp = 'y'] y = rho*y(-1)+e; end;",
    ));
    assert_eq!(
        find(&diags, "E265").message,
        "'mcp' tag does not contain an inequality"
    );
}

#[test]
fn mcp_legal_is_w170_only() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; [mcp = 'y < 0'] y = rho*y(-1)+e; end;",
    ));
    find(&diags, "W170");
    quiet(&diags, "E262");
    quiet(&diags, "E263");
    quiet(&diags, "E264");
    quiet(&diags, "E265");
}

#[test]
fn e266_shock_variance_on_param() {
    let diags = analyze(&parse(&format!(
        "{src} shocks; var rho = 0.01; end;",
        src = ar1()
    )));
    assert!(
        find(&diags, "E266")
            .message
            .contains("setting a variance on 'rho'")
    );
    quiet(&diags, "E020");
}

#[test]
fn e267_shock_stderr_on_param() {
    let diags = analyze(&parse(&format!(
        "{src} shocks; var rho; stderr 0.01; end;",
        src = ar1()
    )));
    assert!(
        find(&diags, "E267")
            .message
            .contains("setting a standard error on 'rho'")
    );
}

#[test]
fn e268_shock_cov_mixed() {
    let diags = analyze(&parse(&format!(
        "{src} shocks; var y, e = 0.01; end;",
        src = ar1()
    )));
    assert!(
        find(&diags, "E268")
            .message
            .contains("setting a covariance between")
    );
}

#[test]
fn e269_shock_corr_mixed() {
    let diags = analyze(&parse(&format!(
        "{src} shocks; corr y, e = 0.1; end;",
        src = ar1()
    )));
    assert!(
        find(&diags, "E269")
            .message
            .contains("setting a correlation between")
    );
}

#[test]
fn e270_shock_skew_on_endo() {
    let diags = analyze(&parse(&format!(
        "{src} shocks; skew y = 0; end;",
        src = ar1()
    )));
    assert_eq!(
        find(&diags, "E270").message,
        "shocks: setting skewness for 'y', 'y', 'y' is not allowed; skewness can only be specified for exogenous variables"
    );
}

#[test]
fn jc6_varobs_variance_quiet() {
    let diags = analyze(&parse(&format!(
        "{src} varobs y; shocks; var y = 0.01; end;",
        src = ar1()
    )));
    quiet(&diags, "E020");
    quiet(&diags, "E266");
}

#[test]
fn e271_option_twice_not_e226() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; y = rho*y(-1)+e; end; varobs y; estimation(dsge_var, dsge_var=0.5, datafile='d.csv');",
    ));
    assert_eq!(
        find(&diags, "E271").message,
        "option dsge_var declared twice"
    );
    quiet(&diags, "E226");
}

#[test]
fn e272_static_with_lag() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; [static] y = y(-1); end;",
    ));
    assert!(
        find(&diags, "E272")
            .message
            .contains("An equation tagged [static] cannot contain")
    );
}

#[test]
fn e273_generate_irfs_name_twice() {
    let diags = analyze(&parse(&format!(
        "{src} generate_irfs; a, e = 1; a, e = 1; end;",
        src = ar1()
    )));
    assert!(
        find(&diags, "E273")
            .message
            .contains("generate_irfs block must be unique")
    );
}

#[test]
fn e274_generate_irfs_exo_twice() {
    let diags = analyze(&parse(&format!(
        "{src} generate_irfs; a, e = 1, e = 2; end;",
        src = ar1()
    )));
    assert!(
        find(&diags, "E274")
            .message
            .contains("You have set the exogenous variable")
    );
}

#[test]
fn e275_namespace() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = foo.bar; model; y = rho*y(-1)+e; end;",
    ));
    assert!(
        find(&diags, "E275")
            .message
            .contains("Namespace-qualified symbol")
    );
}

#[test]
fn e276_log_zero() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; y = log(0)+e; end;",
    ));
    assert_eq!(find(&diags, "E276").message, "log(0) not defined!");
}

#[test]
fn e277_log10_zero() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; y = log10(0)+e; end;",
    ));
    assert_eq!(find(&diags, "E277").message, "log10(0) not defined!");
}

#[test]
fn e278_div_zero() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; y = 1/(1-1)+e; end;",
    ));
    assert!(
        find(&diags, "E278")
            .message
            .contains("Division by zero when forming")
    );
}

#[test]
fn jc7_log_rho_quiet() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0; model; y = log(rho)+e; end;",
    ));
    quiet(&diags, "E276");
    quiet(&diags, "E277");
    quiet(&diags, "E278");
}

#[test]
fn e279_external_function_outside() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; external_function(name=myf); rho = myf; model; y = rho*y(-1)+e; end;",
    ));
    assert!(
        find(&diags, "E279")
            .message
            .contains("name of a MATLAB/Octave function")
    );
}

#[test]
fn e280_external_function_inside() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; external_function(name=myf); model; y = myf; end;",
    ));
    assert!(
        find(&diags, "E280")
            .message
            .contains("function name external to Dynare")
    );
}

#[test]
fn e281_mod_file_local_in_model() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = foo; model; y = foo; end;",
    ));
    assert!(
        find(&diags, "E281")
            .message
            .contains("not allowed inside model declaration")
    );
    quiet(&diags, "E020");
}

#[test]
fn e282_model_local_outside() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; #z = y; y = rho*y(-1)+e; end; rho = z;",
    ));
    assert!(
        find(&diags, "E282")
            .message
            .contains("not allowed outside model declaration. Its scope is only inside model")
    );
}

#[test]
fn e283_if_string_not_bool() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; @#if \"hello\"\n@#endif\nmodel; y = rho*y(-1)+e; end;",
    ));
    assert_eq!(
        find(&diags, "E283").message,
        "The condition must evaluate to a boolean or a double"
    );
}

#[test]
fn e284_for_tuple_arity() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; @#for (a, b) in [(1, 2, 3)]\n@#define z = a\n@#endfor\nmodel; y = rho*y(-1)+e; end;",
    ));
    assert!(
        find(&diags, "E284")
            .message
            .contains("Encountered tuple of size")
    );
}

#[test]
fn e285_plus_type_mismatch() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; @#define x = \"a\"\n@#define y = x + 1\nmodel; y = rho*y(-1)+e; end;",
    ));
    assert_eq!(
        find(&diags, "E285").message,
        "Type mismatch for operands of + operator"
    );
}

// --- 0.6.0 slice 06: every `BLOCK_OPENERS` name is a legal declaration name ---
//
// Driven from `dygnosis::block_openers()` so a later slice that adds a name to the
// table cannot leave it refused. The two statement-scoped words (`epilogue`,
// `init2shocks`) are 7.1 refusals and are checked separately.

/// The 29 names 7.1 accepts as declaration names, each declared and used.
#[test]
fn opener_names_declared_and_used_are_quiet() {
    let mut failures: Vec<String> = Vec::new();
    for name in block_openers().iter().copied() {
        if matches!(name, "epilogue" | "init2shocks") {
            continue;
        }
        let src = format!(
            "var y {name};\nvarexo e;\nparameters rho;\nrho = 0.95;\n\
             model;\ny = rho*y(-1) + e + {name};\n{name} = 0.1*y;\nend;\n\
             initval;\ny = 0;\n{name} = 0;\nend;\n\
             shocks;\nvar e; stderr 0.01;\nend;\nstoch_simul(order = 1, nograph);\n"
        );
        let diags = analyze(&parse(&src));
        let errors: Vec<&str> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.code.as_str())
            .collect();
        if !errors.is_empty() {
            failures.push(format!("{name}: {errors:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "a declared opener name must not draw an Error:\n{}",
        failures.join("\n")
    );
}

/// The two `DYNARE_STATEMENT`-scoped words still refuse on both sides.
#[test]
fn opener_names_statement_scoped_still_refuse() {
    for name in ["epilogue", "init2shocks"] {
        let src = format!(
            "var y {name};\nvarexo e;\nparameters rho;\nrho = 0.95;\n\
             model;\ny = rho*y(-1) + e;\nend;\n"
        );
        let diags = analyze(&parse(&src));
        assert!(
            diags.iter().any(|d| d.code == "E001"),
            "{name}: 7.1 refuses this declaration, expected E001, got {:?}",
            codes(&diags)
        );
    }
}

/// The reach-audit pairs the fix touches: with the declaration scan corrected, the
/// duplicate passes see an opener-named variable and 7.1's own sentences fire.
#[test]
fn opener_name_reaches_e030_w031_and_e111() {
    let e030 = analyze(&parse(
        "var y shocks;\nvarexo e shocks;\nparameters rho;\nrho = 0.95;\n\
         model;\ny = rho*y(-1) + e + shocks;\nend;\n\
         initval;\ny = 0;\nshocks = 0;\nend;\n",
    ));
    assert_eq!(
        find(&e030, "E030").message,
        "Symbol shocks declared twice with different types!"
    );

    let w031 = analyze(&parse(
        "var y shocks;\nvar shocks;\nvarexo e;\nparameters rho;\nrho = 0.95;\n\
         model;\ny = rho*y(-1) + e + shocks;\nshocks = 0.1*y;\nend;\n\
         initval;\ny = 0;\nshocks = 0;\nend;\n",
    ));
    assert_eq!(find(&w031, "W031").message, "Symbol shocks declared twice.");

    let e111 = analyze(&parse(
        "var y;\nvarexo e shocks;\nparameters rho;\nrho = 0.95;\n\
         model;\ny = rho*y(-1) + e + shocks;\nend;\n\
         initval;\ny = 0;\nshocks = 0;\nend;\n\
         shocks;\nvar shocks; stderr 0.1;\nvar shocks; stderr 0.2;\nend;\n",
    ));
    assert_eq!(
        find(&e111, "E111").message,
        "shocks: variance or stderr of shock on shocks declared twice"
    );
}
