//! D-check family locks: context, tags, linear Errors, initval/shocks/order.

use std::path::PathBuf;

use dygnosis::{analyze, parse, Diagnostic};

fn fixture(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn diags(rel: &str) -> Vec<Diagnostic> {
    analyze(&parse(&fixture(rel)))
}

fn codes(got: &[Diagnostic]) -> Vec<&str> {
    got.iter().map(|d| d.code.as_str()).collect()
}

fn find<'a>(got: &'a [Diagnostic], code: &str) -> &'a Diagnostic {
    got.iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("expected {code}, got {:?}", codes(got)))
}

fn quiet(got: &[Diagnostic], code: &str) {
    assert!(
        got.iter().all(|d| d.code != code),
        "expected no {code}, got {:?}",
        codes(got)
    );
}

#[test]
fn e200_write_latex_without_ssm() {
    let got = diags("d_check/e200_write_latex.mod");
    assert!(find(&got, "E200")
        .message
        .contains("write_latex_steady_state_model statement without a steady_state_model block"));
}

#[test]
fn e201_zero_eq_with_check() {
    let got = diags("d_check/e201_zero_eq.mod");
    assert_eq!(
        find(&got, "E201").message,
        "At least one model equation must be declared!"
    );
    assert!(
        got.iter().any(|x| x.code == "W013"),
        "W013 may also warn, got {:?}",
        codes(&got)
    );
}

#[test]
fn e201_empty_model_without_run_is_quiet() {
    quiet(&diags("d_check/e201_empty_quiet.mod"), "E201");
}

#[test]
fn e205_pf_and_stoch() {
    let got = diags("d_check/e205_pf_stoch.mod");
    assert!(find(&got, "E205")
        .message
        .contains("cannot mix perfect foresight context with stochastic context"));
    quiet(&got, "E026");
    quiet(&got, "E179");
}

#[test]
fn e206_use_dll_bytecode() {
    let got = diags("d_check/e206_use_dll_bytecode.mod");
    assert!(find(&got, "E206")
        .message
        .contains("'use_dll' option is not compatible with 'bytecode'"));
}

#[test]
fn e207_no_static() {
    let got = diags("d_check/e207_no_static.mod");
    assert!(find(&got, "E207")
        .message
        .contains("no_static option is incompatible"));
}

#[test]
fn e208_static_dynamic_count() {
    let got = diags("d_check/e208_static_dynamic.mod");
    assert!(find(&got, "E208")
        .message
        .contains("equations marked [static]"));
}

#[test]
fn e209_tags_with_ramsey() {
    let got = diags("d_check/e209_tags_ramsey.mod");
    assert!(find(&got, "E209")
        .message
        .contains("marking equations as [static] or [dynamic]"));
}

#[test]
fn w200_stoch_abs() {
    let got = diags("d_check/w200_stoch_abs.mod");
    assert!(find(&got, "W200")
        .message
        .contains("unsuitable for a stochastic context"));
}

#[test]
fn w200_stoch_abs_in_local() {
    let got = diags("d_check/w200_stoch_local.mod");
    assert!(find(&got, "W200")
        .message
        .contains("unsuitable for a stochastic context"));
}

#[test]
fn e210_linear_abs_endo() {
    let got = diags("d_check/e210_linear_abs.mod");
    assert!(find(&got, "E210")
        .message
        .contains("on an endogenous variable"));
    quiet(&got, "W140");
}

#[test]
fn e211_linear_abs_exo() {
    let got = diags("d_check/e211_linear_exo.mod");
    assert!(find(&got, "E211")
        .message
        .contains("on an exogenous variable"));
    quiet(&got, "W140");
}

#[test]
fn w140_log_stays_warning() {
    let got = diags("d_check/w140_linear_log.mod");
    find(&got, "W140");
    quiet(&got, "E210");
    quiet(&got, "E211");
}

#[test]
fn w140_linear_abs_exo_with_pf() {
    let got = diags("d_check/w140_linear_exo_pf.mod");
    let w140 = find(&got, "W140");
    assert!(
        w140.message.contains("nonlinear operator 'abs'"),
        "PF + linear + abs(exo) is W140 extra with the real op, got {}",
        w140.message
    );
    quiet(&got, "E210");
    quiet(&got, "E211");
}

#[test]
fn e212_estimated_in_shock_expr() {
    let got = diags("d_check/e212_estimated_shock.mod");
    assert!(find(&got, "E212").message.contains(
        "also appear in the expressions defining the variance/covariance matrix of shocks"
    ));
}

#[test]
fn e213_solver_before_setup() {
    let got = diags("d_check/e213_pf_order.mod");
    assert!(find(&got, "E213")
        .message
        .contains("perfect_foresight_setup"));
}

#[test]
fn e214_pfee_solver_before_setup() {
    let got = diags("d_check/e214_pfee_order.mod");
    assert!(find(&got, "E214")
        .message
        .contains("perfect_foresight_with_expectation_errors_setup"));
}

#[test]
fn e216_extended_path_periods() {
    let got = diags("d_check/e216_extended_path.mod");
    assert!(find(&got, "E216")
        .message
        .contains("'periods' option of 'extended_path' is mandatory"));
}

#[test]
fn e217_initval_after_endval() {
    let got = diags("d_check/e217_initval_after_endval.mod");
    assert!(find(&got, "E217")
        .message
        .contains("initval' block cannot appear after an 'endval'"));
}

#[test]
fn e218_all_values_required() {
    let got = diags("d_check/e218_all_values.mod");
    assert!(find(&got, "E218")
        .message
        .contains("You have not set the following"));
}

#[test]
fn e218_histval_does_not_fire() {
    quiet(&diags("d_check/e218_histval_quiet.mod"), "E218");
}
