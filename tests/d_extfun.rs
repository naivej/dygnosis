//! D-extfun family locks: the Jacobian/Hessian same-function Error.
//!
//! 7.1 refuses a statement whose `first_deriv_provided` and
//! `second_deriv_provided` name the same external function when that function
//! is not the statement's top-level function. A derivative name equal to the
//! statement's own `name=` counts as the top-level function, one derivative
//! alone and two different derivative functions are legal, and the no-`name=`
//! shape is `E322`'s (probed 7.1 prints the missing-name Error there, not the
//! Jacobian sentence).

use dygnosis::explain::known_codes;
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

const E334_MSG: &str = "If the Jacobian and Hessian are provided by the same function, that function must be the top-level function.";

#[test]
fn same_function_pair_fires_in_analyze() {
    let diags = analyze(&parse(&fixture("d_extfun/e334_extfun_same_function.mod")));
    assert_eq!(find(&diags, "E334").message, E334_MSG);
    assert_eq!(find(&diags, "W031").message, "Symbol bar declared twice.");
}

#[test]
fn same_function_pair_fires_in_check_file() {
    let text = fixture("d_extfun/e334_extfun_same_function.mod");
    let path = fixture_path("d_extfun/e334_extfun_same_function.mod");
    assert_eq!(find(&check_file(&text, &path), "E334").message, E334_MSG);
}

#[test]
fn own_name_pair_warns_only() {
    // `name=`, `first_deriv_provided` and `second_deriv_provided` all name the
    // top-level function: 7.1 warns twice and refuses nothing.
    let diags = analyze(&parse(&fixture(
        "d_extfun/quiet_e334_own_name_function.mod",
    )));
    let w031 = diags.iter().filter(|d| d.code == "W031").count();
    assert_eq!(w031, 2, "got {:?}", codes(&diags));
    assert!(
        diags.iter().all(|d| d.code != "E334"),
        "got {:?}",
        codes(&diags)
    );
}

#[test]
fn single_derivative_and_distinct_functions_are_quiet() {
    for rel in [
        "d_extfun/quiet_e334_single_deriv_function.mod",
        "d_extfun/quiet_e334_distinct_functions.mod",
    ] {
        let diags = analyze(&parse(&fixture(rel)));
        assert!(
            diags.iter().all(|d| d.code != "E334"),
            "{rel}: {:?}",
            codes(&diags)
        );
    }
}

#[test]
fn no_name_option_is_e322_not_e334() {
    let diags = analyze(&parse(&fixture("d_extfun/quiet_e334_no_name_option.mod")));
    assert_eq!(
        find(&diags, "E322").message,
        "The 'name' option must be passed to external_function()."
    );
    assert!(
        diags.iter().all(|d| d.code != "E334"),
        "got {:?}",
        codes(&diags)
    );
}

#[test]
fn var_collision_fires_both_errors() {
    // 7.1 prints one of the two (its first-Error stop, order-dependent); we
    // report each decidable problem (slice log, call 3).
    let diags = analyze(&parse(&fixture("d_extfun/e334_extfun_var_collision.mod")));
    assert_eq!(
        find(&diags, "E030").message,
        "Symbol bar declared twice with different types!"
    );
    assert_eq!(find(&diags, "E334").message, E334_MSG);
}

#[test]
fn registry_known_codes_include_shock_diagnostics() {
    assert_eq!(known_codes().len(), 359);
}
