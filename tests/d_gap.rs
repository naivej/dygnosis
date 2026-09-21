//! D-gap family locks: shipped-code reach on the surfaces 03 added.
//!
//! `trend_var` / `log_trend_var` declarations and `epilogue` helpers join the
//! duplicate-declaration pass, so **E030** (different kinds) and **W031** (same
//! kind) fire there as they do for `var` / `varexo` / `parameters`. **E307** and
//! **E287** stay the paired Errors on their own surfaces.
//!
//! `dsge_prior_weight` is a reserved preprocessor symbol: a declaration list and
//! `estimated_params` may name it, an expression may not — that use is a parse
//! refuse (**E001**), the same family as `var sin;`.

use dygnosis::explain::known_codes;
use dygnosis::{analyze, check_file, parse, Diagnostic};

const GATE_MSG: &str = "Invalid use of 'dsge_prior_weight': reserved preprocessor symbol, allowed only in a declaration. Choose a different name.";

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

/// One locked fire: fixture, code, and the exact 7.1 message.
struct Fire {
    code: &'static str,
    fixture: &'static str,
    message: &'static str,
}

const FIRES: &[Fire] = &[
    Fire {
        code: "E030",
        fixture: "d_gap/e030_trend_mixed.mod",
        message: "Symbol A declared twice with different types!",
    },
    Fire {
        code: "E030",
        fixture: "d_gap/e030_trend_and_var.mod",
        message: "Symbol A declared twice with different types!",
    },
    Fire {
        code: "E030",
        fixture: "d_gap/e030_epilogue_and_var.mod",
        message: "Symbol foo declared twice with different types!",
    },
    Fire {
        code: "W031",
        fixture: "d_gap/w031_trend_same_kind.mod",
        message: "Symbol A declared twice.",
    },
    Fire {
        code: "W031",
        fixture: "d_gap/w031_epilogue_dup.mod",
        message: "Symbol foo declared twice.",
    },
    Fire {
        code: "E001",
        fixture: "d_gap/e001_dsge_prior_weight_use.mod",
        message: GATE_MSG,
    },
    Fire {
        code: "E001",
        fixture: "d_gap/e001_dsge_prior_weight_slot.mod",
        message: GATE_MSG,
    },
];

#[test]
fn d_gap_fires_in_analyze() {
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
fn d_gap_fires_in_check_file() {
    for fire in FIRES {
        let text = fixture(fire.fixture);
        let diags = check_file(&text, &fixture_path(fire.fixture));
        let d = find(&diags, fire.code);
        assert_eq!(d.message, fire.message, "{}", fire.fixture);
    }
}

#[test]
fn quiet_files_stay_quiet() {
    let legal = diags("d_gap/quiet_legal_trends.mod");
    for code in ["E030", "W031", "E307", "E287"] {
        quiet(&legal, code);
    }
    quiet(&diags("d_gap/quiet_dsge_prior_weight_decl.mod"), "E001");
}

#[test]
fn the_reserved_symbol_keeps_its_declaration_slots() {
    // A declaration list, `estimated_params`, and a model-local `#` definition
    // are legal for 7.1; only an expression use is refused.
    let declared = diags("d_gap/quiet_dsge_prior_weight_decl.mod");
    quiet(&declared, "E001");
    let model_local = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; #dsge_prior_weight = 1; \
         model; y = rho*y(-1) + e; end;",
    ));
    quiet(&model_local, "E001");
}

#[test]
fn parameter_declaration_is_e303_only() {
    // `parameters dsge_prior_weight;` keeps their check string (**E303**);
    // the token gate must not add an **E001** on top.
    let diags = diags("d_open/e303_dsge_prior_weight_parameter.mod");
    find(&diags, "E303");
    quiet(&diags, "E001");
}

#[test]
fn trend_same_kind_keeps_its_own_error() {
    // 7.1 prints the warning and `Trend variable A was declared twice.` together.
    let diags = diags("d_gap/w031_trend_same_kind.mod");
    let w031 = diags.iter().filter(|d| d.code == "W031").count();
    let e307 = diags.iter().filter(|d| d.code == "E307").count();
    assert_eq!(
        w031,
        1,
        "one warning per repeated name: {:?}",
        codes(&diags)
    );
    assert_eq!(e307, 1, "E307 stays paired: {:?}", codes(&diags));
    assert_eq!(
        find(&diags, "E307").message,
        "Trend variable A was declared twice."
    );
}

#[test]
fn epilogue_duplicate_keeps_its_own_error() {
    let diags = diags("d_gap/w031_epilogue_dup.mod");
    assert_eq!(find(&diags, "W031").message, "Symbol foo declared twice.");
    assert_eq!(
        find(&diags, "E287").message,
        "in the 'epilogue' block, variable 'foo' is declared twice"
    );
}

#[test]
fn registry_known_codes_grew_to_297() {
    assert_eq!(known_codes().len(), 297);
}
