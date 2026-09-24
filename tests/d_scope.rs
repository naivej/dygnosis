//! D-scope family locks: the two trigger edges the reach audit left.
//!
//! `check_w021` reads `Model::exogenous`, which the parser folds
//! `deterministic_exogenous` into — 7.1's unused check covers plain `varexo`
//! only, so an unused `varexo_det` is quiet here. And 7.1 declares every
//! function name an `external_function` statement carries, so the derivative
//! values collide with a declaration or a repeat like the `name=` value does.

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

fn diags(rel: &str) -> Vec<Diagnostic> {
    analyze(&parse(&fixture(rel)))
}

/// One locked fire: fixture, code, and the exact 7.1 message.
struct Fire {
    code: &'static str,
    fixture: &'static str,
    message: &'static str,
}

const FIRES: &[Fire] = &[
    Fire {
        code: "E030",
        fixture: "d_scope/e030_extfun_deriv_and_var.mod",
        message: "Symbol bar declared twice with different types!",
    },
    Fire {
        code: "W031",
        fixture: "d_scope/w031_extfun_deriv_same_name.mod",
        message: "Symbol foo declared twice.",
    },
];

#[test]
fn d_scope_fires_in_analyze() {
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
            Some(d) if d.message != fire.message => failures.push(format!(
                "{}: {} message {:?}, expected {:?}",
                fire.fixture, fire.code, d.message, fire.message
            )),
            Some(_) => {}
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn d_scope_fires_in_check_file() {
    for fire in FIRES {
        let text = fixture(fire.fixture);
        let diags = check_file(&text, &fixture_path(fire.fixture));
        assert_eq!(
            find(&diags, fire.code).message,
            fire.message,
            "{}",
            fire.fixture
        );
    }
}

#[test]
fn unused_varexo_det_is_quiet_for_e021() {
    // 7.1 accepts an unused `varexo_det`; the in-repo `e030` fixture keeps the
    // same lock on a file that also has a steady-state block.
    for rel in [
        "d_scope/quiet_unused_varexo_det.mod",
        "e030/varexo_det_only.mod",
    ] {
        let diags = diags(rel);
        assert!(
            diags.iter().all(|d| d.code != "E021"),
            "{rel}: E021 must not fire, got {:?}",
            codes(&diags)
        );
        let workspace = check_file(&fixture(rel), &fixture_path(rel));
        assert!(
            workspace.iter().all(|d| d.code != "E021"),
            "{rel}: E021 must not fire on check_file, got {:?}",
            codes(&workspace)
        );
    }
}

#[test]
fn unused_varexo_still_fires_e021() {
    let diags = diags("w010/w021_exo.mod");
    let e021 = find(&diags, "E021");
    assert!(
        e021.message.contains("not used in model block"),
        "{}",
        e021.message
    );
}

#[test]
fn different_derivative_names_are_quiet() {
    let diags = diags("d_scope/quiet_extfun_deriv_names.mod");
    for code in ["E030", "W031"] {
        assert!(
            diags.iter().all(|d| d.code != code),
            "no {code} expected, got {:?}",
            codes(&diags)
        );
    }
}

#[test]
fn registry_known_codes_include_shock_diagnostics() {
    assert_eq!(known_codes().len(), 359);
}
