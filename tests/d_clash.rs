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

fn codes(diags: &[Diagnostic]) -> Vec<&str> {
    diags.iter().map(|d| d.code.as_str()).collect()
}

fn find<'a>(diags: &'a [Diagnostic], code: &str) -> &'a Diagnostic {
    diags
        .iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("expected {code}, got {:?}", codes(diags)))
}

#[test]
fn flags_planner_shock_paths_and_commands() {
    let two = parse(&fixture("clash/e104_two_planner.mod"));
    assert_eq!(two.planner_objective_spans.len(), 2);
    assert!(two.planner_objective_span.is_some());

    let paths = parse(&fixture("clash/e113_shock_paths_shocks.mod"));
    assert!(paths.shock_paths_span.is_some());
    assert!(paths.shocks_block.is_some());

    let ident = parse(&fixture("clash/e028_identification_varexo_det.mod"));
    assert!(ident.identification_span.is_some());
    assert!(!ident.deterministic_exogenous.is_empty());

    let simul = parse(&fixture("clash/e026_varexo_det_simul.mod"));
    assert!(!simul.simul_spans.is_empty());

    let pfc = parse("var y; varexo e; model; y = e; end; perfect_foresight_controlled_paths; end;");
    assert!(pfc.perfect_foresight_controlled_paths_span.is_some());

    let cmds = parse(
        "var y; varexo e; model; y = e; end; identification; perfect_foresight_solver; perfect_foresight_with_expectation_errors_solver; extended_path; method_of_moments; sensitivity;",
    );
    assert!(cmds.identification_span.is_some());
    assert!(cmds.perfect_foresight_solver_span.is_some());
    assert!(cmds.pfee_solver_span.is_some());
    assert!(cmds.extended_path_span.is_some());
    assert!(cmds.method_of_moments_span.is_some());
    assert!(cmds.sensitivity_span.is_some());
}

#[test]
fn e104_two_planner_with_ramsey() {
    let diags = analyze(&parse(&fixture("clash/e104_two_planner.mod")));
    let d = find(&diags, "E104");
    assert!(d
        .message
        .contains("there can only be one planner_objective statement"));
}

#[test]
fn e104_two_planner_with_ramsey_policy() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.9; model; y = rho * y(-1) + e; end; planner_objective y; planner_objective y; ramsey_policy;",
    ));
    find(&diags, "E104");
}

#[test]
fn e104_two_planner_with_osr_is_quiet() {
    let diags = analyze(&parse(&fixture("clash/e104_two_planner_osr_quiet.mod")));
    assert!(
        diags.iter().all(|d| d.code != "E104"),
        "osr-only two planners must not emit E104, got {:?}",
        codes(&diags)
    );
}

#[test]
fn e026_varexo_det_simul() {
    let diags = analyze(&parse(&fixture("clash/e026_varexo_det_simul.mod")));
    let d = find(&diags, "E026");
    assert!(d.message.contains("varexo_det declaration"));
}

#[test]
fn e026_varexo_det_without_solver_is_quiet() {
    let diags = analyze(&parse(&fixture("clash/e026_varexo_det_alone_quiet.mod")));
    assert!(
        diags.iter().all(|d| d.code != "E026"),
        "got {:?}",
        codes(&diags)
    );
}

#[test]
fn e027_ramsey_varexo_det() {
    let diags = analyze(&parse(&fixture("clash/e027_ramsey_varexo_det.mod")));
    let d = find(&diags, "E027");
    assert!(d
        .message
        .contains("incompatible with deterministic exogenous variables"));
}

#[test]
fn e027_ramsey_without_varexo_det_is_quiet() {
    let diags = analyze(&parse(&fixture("w100/w100_ok.mod")));
    assert!(
        diags.iter().all(|d| d.code != "E027"),
        "got {:?}",
        codes(&diags)
    );
}

#[test]
fn e028_identification_varexo_det() {
    let diags = analyze(&parse(&fixture("clash/e028_identification_varexo_det.mod")));
    let d = find(&diags, "E028");
    assert!(d
        .message
        .contains("identification is incompatible with deterministic exogenous variables"));
}

#[test]
fn e028_identification_without_varexo_det_is_quiet() {
    let diags = analyze(&parse(&fixture(
        "clash/e028_identification_alone_quiet.mod",
    )));
    assert!(
        diags.iter().all(|d| d.code != "E028"),
        "got {:?}",
        codes(&diags)
    );
}

#[test]
fn e028_sensitivity_varexo_det() {
    let diags = analyze(&parse(&fixture("clash/e028_sensitivity_varexo_det.mod")));
    let d = find(&diags, "E028");
    assert!(d
        .message
        .contains("identification is incompatible with deterministic exogenous variables"));
}

#[test]
fn e028_sensitivity_without_identification_eq_1_is_quiet() {
    let diags = analyze(&parse(
        "var y; varexo_det tau; parameters rho; rho = 0.5; model; y = rho * y(-1) + tau; end; sensitivity;",
    ));
    assert!(
        diags.iter().all(|d| d.code != "E028"),
        "got {:?}",
        codes(&diags)
    );
}

#[test]
fn e028_sensitivity_identification_eq_1_without_varexo_det_is_quiet() {
    let diags = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.5; model; y = rho * y(-1) + e; end; sensitivity(identification=1);",
    ));
    assert!(
        diags.iter().all(|d| d.code != "E028"),
        "got {:?}",
        codes(&diags)
    );
}

#[test]
fn e178_surprise_without_occbin() {
    let diags = analyze(&parse(&fixture("occbin/e178_surprise.mod")));
    let d = find(&diags, "E178");
    assert!(d.message.contains("shocks(surprise)"));
    let library = analyze(&parse(&fixture("occbin/surprise.mod")));
    find(&library, "E178");
}

#[test]
fn e179_occbin_identification() {
    let diags = analyze(&parse(&fixture("clash/e179_identification.mod")));
    let d = find(&diags, "E179");
    assert!(d.message.contains("occbin_constraints"));
}

#[test]
fn square_has_no_clash() {
    let diags = analyze(&parse(&fixture("occbin/square.mod")));
    for code in ["E178", "E179", "E026", "E027", "E028", "E104", "E113"] {
        assert!(
            diags.iter().all(|d| d.code != code),
            "square.mod must not emit {code}, got {:?}",
            codes(&diags)
        );
    }
}

#[test]
fn e113_shock_paths_with_shocks() {
    let diags = analyze(&parse(&fixture("clash/e113_shock_paths_shocks.mod")));
    let d = find(&diags, "E113");
    assert!(d.message.contains("shock_paths"));
}

#[test]
fn e113_shock_paths_alone_is_quiet() {
    let diags = analyze(&parse(&fixture("clash/e113_shock_paths_alone_quiet.mod")));
    assert!(
        diags.iter().all(|d| d.code != "E113"),
        "got {:?}",
        codes(&diags)
    );
}

#[test]
fn e113_shock_paths_with_mshocks_or_endval() {
    let mshocks = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.5; model; y = rho * y(-1) + e; end; shock_paths; var e; periods 1; values 0; end; mshocks; var e; stderr 0.01; end;",
    ));
    find(&mshocks, "E113");

    let endval = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.5; model; y = rho * y(-1) + e; end; shock_paths; var e; periods 1; values 0; end; endval; y = 0; end;",
    ));
    find(&endval, "E113");

    let pfc = analyze(&parse(
        "var y; varexo e; parameters rho; rho = 0.5; model; y = rho * y(-1) + e; end; shock_paths; var e; periods 1; values 0; end; perfect_foresight_controlled_paths; end;",
    ));
    find(&pfc, "E113");
}
