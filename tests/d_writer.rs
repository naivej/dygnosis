//! Writer-stage locks: refusals the official preprocessor prints only when it
//! writes the MATLAB files, after check and transform accept (0.5.5).
//!
//! Stage rule: `dev_logs/0.5/0.5.5/probe-writer.md`. The write run itself is the
//! honesty suite's (`tests/honesty.rs`); these locks are the product-side verdicts.

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

/// One locked fire: fixture, code, and the exact 7.1 message.
struct Fire {
    code: &'static str,
    fixture: &'static str,
    message: &'static str,
}

/// Library fires: `analyze` sees them, no file beside the `.mod` involved.
const FIRES: &[Fire] = &[
    Fire {
        code: "E381",
        fixture: "d_writer/e381_steady_state_extfun.mod",
        message: "The expression inside a steady_state operator cannot contain external functions",
    },
    Fire {
        code: "W205",
        fixture: "d_writer/w205_shock_groups_label_reused.mod",
        message: "shock group label 'g1' has been reused. Only using the last definition.",
    },
];

/// Workspace-only fires: the load file next to the `.mod` decides.
const WORKSPACE_FIRES: &[Fire] = &[
    Fire {
        code: "E380",
        fixture: "d_writer/e380_load_params_epilogue.mod",
        message: "Unsupported variable type for A in load_params_and_steady_state",
    },
    Fire {
        code: "E380",
        fixture: "d_writer/e380_load_params_used_trend.mod",
        message: "Unsupported variable type for A in load_params_and_steady_state",
    },
];

#[test]
fn d_writer_fires_in_analyze() {
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
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn d_writer_workspace_fires_in_check_file() {
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
                if d.severity != dygnosis::Severity::Error {
                    failures.push(format!("{}: {} is not an Error", fire.fixture, fire.code));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));

    // The load file is file-relative: `analyze` never sees E380.
    for fire in WORKSPACE_FIRES {
        quiet(&diags(fire.fixture), fire.code);
    }
}

/// E380 keeps only the genuinely unknown name; the three unsupported kinds
/// (epilogue helper, `external_function` name, unused trend) moved off **W204**.
#[test]
fn e380_leaves_w204_only_the_unknown_name() {
    // A genuinely unknown name still warns W204; it must not error E380.
    let w204 = file_diags("d_open/w204_load_params_unknown.mod");
    assert!(
        w204.iter()
            .any(|d| d.code == "W204" && d.severity == dygnosis::Severity::Warning),
        "w204 fixture must still warn W204, got {:?}",
        codes(&w204)
    );
    quiet(&w204, "E380");

    // The epilogue helper errors E380 instead.
    let e380 = file_diags("d_writer/e380_load_params_epilogue.mod");
    assert!(
        e380.iter()
            .any(|d| d.code == "E380" && d.severity == dygnosis::Severity::Error),
        "e380 fixture must error E380, got {:?}",
        codes(&e380)
    );
    quiet(&e380, "W204");
}

/// The positional rule: the loader's symbol table is filled as parsing reaches
/// the statement, so a name declared only *after* it is still unknown — 7.1
/// warns `Unknown symbol`, and so does **W204**. No **E380**.
#[test]
fn e380_is_positional_and_leaves_w204_after_the_statement() {
    let diags = file_diags("d_writer/e380_load_params_after_unknown.mod");
    quiet(&diags, "E380");
    for name in ["A", "ef"] {
        let needle = format!("Unknown symbol {name} in e380_after_params.txt");
        assert!(
            diags
                .iter()
                .any(|d| d.code == "W204" && d.message == needle),
            "expected W204 {needle:?}, got {:?}",
            diags.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
        );
    }
}

#[test]
fn quiet_files_stay_quiet() {
    let quiets: &[(&str, &[&str])] = &[
        ("d_writer/quiet_shock_groups.mod", &["W205"]),
        ("d_writer/quiet_shock_groups_two_blocks.mod", &["W205"]),
        ("d_writer/quiet_load_params.mod", &["E380", "W204"]),
    ];
    for (rel, expected_quiet) in quiets {
        let diags = file_diags(rel);
        for code in *expected_quiet {
            quiet(&diags, code);
        }
    }
}

#[test]
fn e381_finds_the_call_anywhere_in_the_operand() {
    // The operand walk descends, so a call nested under an operator is found
    // as well as a bare one (probed on 7.1).
    for source in [
        "var c; external_function(name='ef', nargs=1); model; c = steady_state(ef(c) + 1); end;",
        "var c; external_function(name='ef', nargs=1); model; c = steady_state(2*ef(c)); end;",
        "var c; external_function(name='ef', nargs=1); model; c = steady_state(ef(c)); end;",
    ] {
        let diags = analyze(&parse(source));
        assert!(
            diags.iter().any(|d| d.code == "E381"),
            "E381 must fire on {source:?}, got {:?}",
            codes(&diags)
        );
    }
}

/// A `steady_state(…)` operand that names no external function is quiet.
#[test]
fn e381_is_quiet_for_a_plain_operand() {
    let source = "var c; parameters p; p = 0.9; model; c = steady_state(p*c(-1)); end;";
    let diags = analyze(&parse(source));
    quiet(&diags, "E381");
}

#[test]
fn registry_known_codes_include_shock_diagnostics() {
    assert_eq!(known_codes().len(), 359);
}
