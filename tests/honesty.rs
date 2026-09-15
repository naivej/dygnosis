use std::path::{Path, PathBuf};
use std::time::Duration;

use dygnosis::preprocessor::find_preprocessor;
use dygnosis::{
    analyze, check_file, parse, reconcile_diagnostics, run_preprocessor, Diagnostic, Severity,
};

const ACCEPT_ARCHIVES: &[&str] = &[
    "trend_rbc_gov_inv",
    "sims_wu_2019",
    "lk2024",
    "govt_rbc_irf_matching",
];

const NAMED_HOLES: &[&str] = &[];

const SAME_GROUND_WARNINGS: &[&str] = &["W022", "W031", "W042", "W121", "W131", "W150", "W170"];

fn copilot_mod(archive_dir: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/model-archive")
        .join(archive_dir)
        .join(format!("{archive_dir}.mod"))
}

fn read_mod(archive_dir: &str) -> String {
    std::fs::read_to_string(copilot_mod(archive_dir))
        .unwrap_or_else(|e| panic!("fixture missing: {e}"))
        .replace("\r\n", "\n")
}

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
}

fn read_fixture(rel: &str) -> String {
    std::fs::read_to_string(fixture(rel))
        .unwrap_or_else(|e| panic!("fixture missing: {e}"))
        .replace("\r\n", "\n")
}

fn assert_no_error(own: &[Diagnostic], label: &str) {
    let errors: Vec<_> = own
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.code.as_str())
        .collect();
    assert!(
        errors.is_empty(),
        "{label} must not emit Error, got {errors:?}"
    );
}

#[test]
fn accepted_archives_emit_no_error() {
    let Some(pp) = find_preprocessor(None) else {
        eprintln!("skipping honesty: dynare-preprocessor not found");
        return;
    };
    for name in ACCEPT_ARCHIVES {
        assert!(
            !NAMED_HOLES.contains(name),
            "named hole {name} must not be in the no-Error loop"
        );
        let text = read_mod(name);
        let path = copilot_mod(name);
        let path_str = path.to_str().expect("utf-8 path");
        let source_dir = path.parent().map(Path::to_path_buf);
        let result = run_preprocessor(&text, &pp, source_dir.as_deref(), Duration::from_secs(30));
        assert!(
            result.success,
            "{name} should be accepted: {:?}",
            result.diagnostics
        );
        assert_no_error(&analyze(&parse(&text)), &format!("{name} analyze()"));
        assert_no_error(&check_file(&text, path_str), &format!("{name} check_file"));
    }
}

#[test]
fn named_holes_are_absent_from_no_error_loop() {
    assert!(
        NAMED_HOLES.is_empty(),
        "0.2.0 closed the 0.1.1 named holes; got {NAMED_HOLES:?}"
    );
    for hole in NAMED_HOLES {
        assert!(
            !ACCEPT_ARCHIVES.contains(hole),
            "{hole} must stay off the accepted-archive no-Error loop"
        );
    }
}

#[test]
fn equation_count_is_warning_they_accept() {
    let Some(pp) = find_preprocessor(None) else {
        eprintln!("skipping honesty: dynare-preprocessor not found");
        return;
    };
    let path = fixture("e010/e010_extra.mod");
    let path_str = path.to_str().expect("utf-8 path");
    let text = read_fixture("e010/e010_extra.mod");
    let source_dir = path.parent().map(Path::to_path_buf);
    let result = run_preprocessor(&text, &pp, source_dir.as_deref(), Duration::from_secs(30));
    assert!(
        result.success,
        "e010_extra.mod should be accepted: {:?}",
        result.diagnostics
    );
    let own = check_file(&text, path_str);
    let w013 = own
        .iter()
        .find(|d| d.code == "W013")
        .expect("own W013 on e010_extra.mod");
    assert_eq!(w013.severity, Severity::Warning);
    let rec = reconcile_diagnostics(&own, Some(&result));
    assert!(
        rec.iter()
            .filter(|d| !d.code.starts_with('P'))
            .all(|d| d.severity != Severity::Error),
        "after reconcile, no own Error should remain: {:?}",
        rec.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
    assert!(
        rec.iter().any(|d| d.code == "W013"),
        "extra Warning W013 should stay after reconcile"
    );
}

#[test]
fn same_ground_warning_absent_on_quiet_archive() {
    let Some(pp) = find_preprocessor(None) else {
        eprintln!("skipping honesty: dynare-preprocessor not found");
        return;
    };
    let text = read_mod("trend_rbc_gov_inv");
    let path = copilot_mod("trend_rbc_gov_inv");
    let path_str = path.to_str().expect("utf-8 path");
    let source_dir = path.parent().map(Path::to_path_buf);
    let result = run_preprocessor(&text, &pp, source_dir.as_deref(), Duration::from_secs(30));
    assert!(
        result.success,
        "trend_rbc_gov_inv should be accepted: {:?}",
        result.diagnostics
    );
    let own = check_file(&text, path_str);
    for code in SAME_GROUND_WARNINGS {
        assert!(
            own.iter().all(|d| d.code != *code),
            "quiet archive must not emit same-ground Warning {code}, got {:?}",
            own.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
        );
    }
}

#[test]
fn same_kind_named_holes_accepted_reconcile_hides_w031() {
    let Some(pp) = find_preprocessor(None) else {
        eprintln!("skipping honesty: dynare-preprocessor not found");
        return;
    };
    let cases = [
        ("e030/same_kind_var.mod", "Symbol y declared twice."),
        ("e030/same_kind_param.mod", "Symbol betta declared twice."),
    ];
    for (rel, wording) in cases {
        let path = fixture(rel);
        let path_str = path.to_str().expect("utf-8 path");
        let text = read_fixture(rel);
        let source_dir = path.parent().map(Path::to_path_buf);
        let result = run_preprocessor(&text, &pp, source_dir.as_deref(), Duration::from_secs(30));
        assert!(
            result.success,
            "{rel} should be accepted: {:?}",
            result.diagnostics
        );
        let own = check_file(&text, path_str);
        assert_no_error(&analyze(&parse(&text)), &format!("{rel} analyze()"));
        assert_no_error(&own, &format!("{rel} check_file"));
        assert!(
            own.iter().any(|d| d.code == "W031"),
            "{rel} own must emit W031, got {:?}",
            own.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
        );
        let rec = reconcile_diagnostics(&own, Some(&result));
        assert!(
            rec.iter().all(|d| d.code != "W031"),
            "{rel} W031 should be hidden after reconcile: {:?}",
            rec.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
        );
        assert!(
            rec.iter().any(|d| d.message.contains(wording)),
            "{rel} should keep their wording {wording:?}, got {:?}",
            rec.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
        );
    }
}

const OCCBIN_FIRE: &[&str] = &[
    "occbin/e170_two_blocks.mod",
    "occbin/e171_three.mod",
    "occbin/e172_missing_regime.mod",
    "occbin/e173_bind_no_name.mod",
    "occbin/e174_bind_missing.mod",
    "occbin/e175_no_equation.mod",
    "occbin/e176_bind_and_relax.mod",
    "occbin/e177_regime_dup.mod",
    "occbin/e180_mcp_perp.mod",
    "occbin/e181_bind_eq.mod",
    "occbin/e182_lead.mod",
    "occbin/e183_perp_form.mod",
    "occbin/e184_dup_clause.mod",
    "occbin/e185_bad_name.mod",
];

const OCCBIN_ERRORS: &[&str] = &[
    "E170", "E171", "E172", "E173", "E174", "E175", "E176", "E177", "E180", "E181",
    "E182", "E183", "E184", "E185",
];

#[test]
fn occbin_honesty_when_preprocessor_present() {
    let Some(pp) = find_preprocessor(None) else {
        eprintln!("skipping honesty: dynare-preprocessor not found");
        return;
    };
    for rel in OCCBIN_FIRE {
        let path = fixture(rel);
        let text = read_fixture(rel);
        let source_dir = path.parent().map(Path::to_path_buf);
        let result = run_preprocessor(&text, &pp, source_dir.as_deref(), Duration::from_secs(30));
        assert!(
            !result.success,
            "{rel} should be refused: {:?}",
            result.diagnostics
        );
    }

    let path = fixture("occbin/square.mod");
    let path_str = path.to_str().expect("utf-8 path");
    let text = read_fixture("occbin/square.mod");
    let source_dir = path.parent().map(Path::to_path_buf);
    let result = run_preprocessor(&text, &pp, source_dir.as_deref(), Duration::from_secs(30));
    assert!(
        result.success,
        "occbin/square.mod should be accepted: {:?}",
        result.diagnostics
    );
    let own_analyze = analyze(&parse(&text));
    let own_file = check_file(&text, path_str);
    for own in [&own_analyze, &own_file] {
        let errors: Vec<_> = own
            .iter()
            .filter(|d| d.severity == Severity::Error && OCCBIN_ERRORS.contains(&d.code.as_str()))
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            errors.is_empty(),
            "square.mod must not emit OccBin Error, got {errors:?}"
        );
    }

    let path = fixture("occbin/w170_mcp.mod");
    let path_str = path.to_str().expect("utf-8 path");
    let text = read_fixture("occbin/w170_mcp.mod");
    let source_dir = path.parent().map(Path::to_path_buf);
    let result = run_preprocessor(&text, &pp, source_dir.as_deref(), Duration::from_secs(30));
    assert!(
        result.success,
        "w170_mcp.mod should be accepted: {:?}",
        result.diagnostics
    );
    let own = check_file(&text, path_str);
    assert!(
        own.iter().any(|d| d.code == "W170"),
        "w170_mcp.mod own must emit W170, got {:?}",
        own.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
    let rec = reconcile_diagnostics(&own, Some(&result));
    assert!(
        rec.iter().all(|d| d.code != "W170"),
        "W170 should be hidden after reconcile: {:?}",
        rec.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
    assert!(
        rec.iter().any(|d| d.message.contains("obsolete")),
        "should keep their wording containing obsolete, got {:?}",
        rec.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );

    let path = fixture("occbin/e185_bad_name.mod");
    let path_str = path.to_str().expect("utf-8 path");
    let text = read_fixture("occbin/e185_bad_name.mod");
    let source_dir = path.parent().map(Path::to_path_buf);
    let result = run_preprocessor(&text, &pp, source_dir.as_deref(), Duration::from_secs(30));
    assert!(
        !result.success,
        "e185_bad_name.mod should be refused: {:?}",
        result.diagnostics
    );
    let own = check_file(&text, path_str);
    assert!(
        own.iter().any(|d| d.code == "E185"),
        "e185_bad_name.mod own must emit E185, got {:?}",
        own.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
    let rec = reconcile_diagnostics(&own, Some(&result));
    assert!(
        rec.iter().all(|d| d.code != "E185"),
        "E185 should be hidden after reconcile: {:?}",
        rec.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
    assert!(
        rec.iter()
            .any(|d| d.message.contains("unauthorized characters")),
        "should keep their wording containing unauthorized characters, got {:?}",
        rec.iter().map(|d| d.message.as_str()).collect::<Vec<_>>()
    );
}
