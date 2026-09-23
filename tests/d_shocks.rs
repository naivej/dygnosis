use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use dygnosis::server::diagnostics_for;
use dygnosis::{
    analyze, check_file, dynare_diagnose, find_preprocessor, parse, run_preprocessor, Diagnostic,
    JsonStage, Severity,
};
use tower_lsp::lsp_types::NumberOrString;

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/d_shocks")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    analyze(&parse(source))
}

fn has(diags: &[Diagnostic], code: &str) -> bool {
    diags.iter().any(|d| d.code == code)
}

fn base(extra: &str) -> String {
    format!("var y;\nvarexo e u;\nmodel;\ny = e + u;\nend;\n{extra}\n")
}

#[test]
fn e111_cross_form_and_skew_duplicates() {
    let cross = fixture("e111_cov_corr.mod");
    let d = diagnostics(&cross);
    let duplicate = d.iter().find(|d| d.code == "E111").unwrap();
    assert_eq!(
        duplicate.message,
        "shocks: covariance or correlation shock on variable pair (u, e) declared twice"
    );
    let reverse = base("shocks; corr e, u = 0.5; var u, e = 0.2; end;");
    assert!(has(&diagnostics(&reverse), "E111"));

    let single = diagnostics(&fixture("e393_skew_single.mod"));
    assert_eq!(
        single.iter().find(|d| d.code == "E393").unwrap().message,
        "shocks: skewness of e declared twice"
    );
    let triple = diagnostics(&fixture("e394_skew_triple.mod"));
    assert_eq!(
        triple.iter().find(|d| d.code == "E394").unwrap().message,
        "shocks: co-skewness of (v, e, u) declared twice"
    );
    let equivalent = base("shocks; skew e = 1; skew e, e, e = 2; end;");
    assert!(has(&diagnostics(&equivalent), "E394"));
    let quiet = diagnostics(&fixture("quiet_skew_blocks.mod"));
    assert!(!has(&quiet, "E393") && !has(&quiet, "E394"));
    let pair_quiet = base("shocks; var e, u = 0.2; end; shocks; corr u, e = 0.5; end;");
    assert!(!has(&diagnostics(&pair_quiet), "E111"));
}

#[test]
fn unknown_stochastic_shock_reaches_e058_before_type_checks() {
    let result = diagnostics(&fixture("e058_stochastic_unknown.mod"));
    assert!(result
        .iter()
        .any(|d| d.code == "E058" && d.message == "Unknown symbol: z."));
    assert!(!has(&result, "E266"));
    let valid = base("shocks; var e = 0.1; end;");
    assert!(!has(&diagnostics(&valid), "E058"));
}

#[test]
fn w060_requires_written_irf_request() {
    for extra in [
        "",
        "stoch_simul;",
        "stoch_simul(periods=100);",
        "stoch_simul(conditional_variance_decomposition=1);",
        "stoch_simul(irf=0);",
        "stoch_simul(irf=0, irf_shocks=(e));",
    ] {
        assert!(!has(&diagnostics(&base(extra)), "W060"), "{extra}");
    }
    let no_size = diagnostics(&base("stoch_simul(irf=20);"));
    let d = no_size.iter().find(|d| d.code == "W060").unwrap();
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(
        d.message,
        "stoch_simul requests IRFs, but no stochastic shock size is specified."
    );
    let selected = base("shocks; var u = 0.1; end; stoch_simul(irf_shocks=(e,u));");
    let d = diagnostics(&selected);
    let missing: Vec<_> = d.iter().filter(|d| d.code == "W060").collect();
    assert_eq!(missing.len(), 1);
    assert!(missing[0].message.contains("'e'"));
    assert_eq!(
        &selected[missing[0].span.start as usize..missing[0].span.end as usize],
        "e"
    );
}

#[test]
fn w060_explicit_sizes_and_partial_unselected_stay_quiet() {
    for block in [
        "shocks; var e = 0; end;",
        "shocks; var e; stderr 0; end;",
        "shocks; var e = 0.1; end;",
    ] {
        let selected = base(&format!("{block} stoch_simul(irf_shocks=(e));"));
        assert!(!has(&diagnostics(&selected), "W060"), "{block}");
        let unselected = base(&format!("{block} stoch_simul(irf=20);"));
        assert!(!has(&diagnostics(&unselected), "W060"), "{block}");
    }
    let removed = base(
        "shocks; var e = 0.1; end; shocks(overwrite); var u = 0.2; end; stoch_simul(irf_shocks=(e));",
    );
    assert!(has(&diagnostics(&removed), "W060"));
    let estimated = base("estimated_params; stderr e, 0.1; end; stoch_simul(irf_shocks=(e));");
    assert!(!has(&diagnostics(&estimated), "W060"));
    let estimated_later =
        base("stoch_simul(irf_shocks=(e)); estimated_params; stderr e, 0.1; end;");
    assert!(has(&diagnostics(&estimated_later), "W060"));
    let uncertain = base("@#if FLAG\nshocks; var e = 0.1; end;\n@#endif\nstoch_simul(irf=20);");
    assert!(!has(&diagnostics(&uncertain), "W060"));
    let unresolved = base("@#include \"missing_shocks.inc\"\nstoch_simul(irf=20);");
    assert!(!has(
        &check_file(&unresolved, "C:/tmp/d_shocks_w060_unresolved.mod"),
        "W060"
    ));
    let external = base("verbatim; M_.Sigma_e(1,1) = 0.01; end; stoch_simul(irf_shocks=(e));");
    assert!(!has(&diagnostics(&external), "W060"));
    let spaced_external =
        base("verbatim ; M_.Sigma_e(1,1) = 0.01; end; stoch_simul(irf_shocks=(e));");
    assert!(!has(&diagnostics(&spaced_external), "W060"));
    let size_after_request = base("stoch_simul(irf_shocks=(e)); shocks; var e = 0.1; end;");
    assert!(has(&diagnostics(&size_after_request), "W060"));
    let overwrite_after_request = base(
        "shocks; var e = 0.1; end; stoch_simul(irf_shocks=(e)); shocks(overwrite); var u = 0.2; end;",
    );
    assert!(!has(&diagnostics(&overwrite_after_request), "W060"));
}

#[test]
fn w060_resolved_include_maps_selected_span_to_active_file() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/d_shocks/include_w060/main.mod");
    let source = std::fs::read_to_string(&path).unwrap();
    let diags = check_file(&source, path.to_str().unwrap());
    let warning = diags.iter().find(|d| d.code == "W060").unwrap();
    assert_eq!(
        &source[warning.span.start as usize..warning.span.end as usize],
        "e"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_dygnosis"))
        .arg("check")
        .arg(&path)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("main.mod:2:25: WARNING [W060]"), "{stdout}");
    let included_path = path.with_file_name("included_request.mod");
    let included_source = std::fs::read_to_string(&included_path).unwrap();
    let included_diags = check_file(&included_source, included_path.to_str().unwrap());
    assert!(
        !has(&included_diags, "W060"),
        "an included-file request has no range in the active root: {included_diags:?}"
    );
}

#[test]
fn w060_message_and_selected_name_match_library_lsp_and_mcp() {
    let source = base("stoch_simul(irf_shocks=(e));");
    let library = diagnostics(&source)
        .into_iter()
        .find(|d| d.code == "W060")
        .unwrap();
    let mcp = dynare_diagnose(&source, None, None)
        .into_iter()
        .find(|d| d.code == "W060")
        .unwrap();
    let lsp = diagnostics_for("file:///C:/tmp/d_shocks_w060.mod", &source)
        .into_iter()
        .find(|d| matches!(&d.code, Some(NumberOrString::String(code)) if code == "W060"))
        .unwrap();
    assert_eq!(library.message, mcp.message);
    assert_eq!(library.message, lsp.message);
    assert_eq!(
        &source[library.span.start as usize..library.span.end as usize],
        "e"
    );
    assert_eq!(mcp.line, lsp.range.start.line + 1);
    assert_eq!(mcp.column, lsp.range.start.character + 1);
}

#[test]
fn irf_shocks_option_uses_parse_time_name_and_exogenous_role() {
    for command in ["stoch_simul", "estimation"] {
        let unknown = base(&format!("{command}(irf_shocks=(bad));"));
        let result = diagnostics(&unknown);
        assert!(has(&result, "E058"), "{command}: {result:?}");

        let wrong = base(&format!("{command}(irf_shocks=(y));"));
        let result = diagnostics(&wrong);
        assert!(result.iter().any(|d| d.code == "E240"
            && d.message == "Variables passed to irf_shocks must be exogenous. Caused by: y"));

        let accepted_command = if command == "estimation" {
            "estimation(datafile='data.mat', irf_shocks=(e e));".to_string()
        } else {
            "stoch_simul(irf_shocks=(e e));".to_string()
        };
        let accepted = base(&accepted_command);
        let result = diagnostics(&accepted);
        assert!(
            !has(&result, "E058") && !has(&result, "E240") && !has(&result, "E227"),
            "{command}: {result:?}"
        );
    }
    let later = "var y;\nmodel;\ny=e;\nend;\nstoch_simul(irf_shocks=(e));\nvarexo e;\n";
    assert!(has(&diagnostics(later), "E058"));
    for option in ["irf=-1", "irf=1.5", "irf_shocks=()"] {
        let result = diagnostics(&base(&format!("stoch_simul({option});")));
        assert!(has(&result, "E001"), "{option}: {result:?}");
    }
    for (list, message) in [
        ("(e,)", "syntax error, unexpected ')'"),
        ("(e,1)", "syntax error, unexpected INT_NUMBER"),
        ("(e 1)", "syntax error, unexpected INT_NUMBER"),
    ] {
        let result = diagnostics(&base(&format!("stoch_simul(irf_shocks={list});")));
        assert!(
            result
                .iter()
                .any(|d| d.code == "E001" && d.message == message),
            "{list}: {result:?}"
        );
    }
    let valid_list = diagnostics(&base("stoch_simul(irf_shocks=(e,u));"));
    assert!(!has(&valid_list, "E001"));
    let repeated_irf = diagnostics(&base("stoch_simul(irf=10,irf=20);"));
    assert!(repeated_irf
        .iter()
        .any(|d| d.code == "E271" && d.message == "option irf declared twice"));
    assert!(!has(&repeated_irf, "W060"));
}

#[test]
fn scheduled_shock_refusals_match_their_classes() {
    let cases = [
        (
            "shocks; var e; periods 1; values 1; var e; periods 2; values 2; end;",
            "E344",
        ),
        ("shocks; var e; periods 1, 2; values 1; end;", "E343"),
        ("shocks; var e; periods 3:1; values 1; end;", "E395"),
        ("shocks; var e; periods 1; add 1; end;", "E396"),
        ("shocks; var e; periods 1; multiply 1; end;", "E396"),
        ("mshocks; var e; periods 1; add 1; end;", "E397"),
        ("mshocks; var e; periods 1; multiply 1; end;", "E397"),
        ("shocks(surprise); var e; periods 1; add 1; end;", "E398"),
        (
            "shocks(surprise); var e; periods 2000Q1; values 1; end;",
            "E399",
        ),
        (
            "shocks(learnt_in=0); var e; periods 1; values 1; end;",
            "E400",
        ),
        (
            "mshocks(learnt_in=0); var e; periods 1; values 1; end;",
            "E400",
        ),
        (
            "shocks(learnt_in=3); var e; periods 2; values 1; end;",
            "E401",
        ),
        (
            "mshocks(learnt_in=3); var e; periods 2; values 1; end;",
            "E401",
        ),
        (
            "heteroskedastic_shocks; var e; periods 1; values 1; var e; periods 2; values 2; end;",
            "E402",
        ),
        (
            "heteroskedastic_shocks; var e; periods 1, 2; values 1; end;",
            "E403",
        ),
    ];
    for (body, code) in cases {
        let result = diagnostics(&base(body));
        assert!(has(&result, code), "{code} on {body}: {result:?}");
    }
    let valid = base(
        "heteroskedastic_shocks; var e; periods 1; values 1; var e; periods 2; scales 2; end;",
    );
    let result = diagnostics(&valid);
    assert!(!has(&result, "E402"));
    let deterministic_exo = "var y;\nvarexo e;\nvarexo_det d;\nmodel;\ny=e+d;\nend;\nmshocks; var d; periods 1; values 1; end;\n";
    let result = diagnostics(deterministic_exo);
    assert!(!has(&result, "E317"), "{result:?}");
}

#[test]
fn shock_path_and_endval_refusals_match_their_classes() {
    let cases = [
        ("shock_paths; var e; periods 1, 2; values 1; end;", "E404"),
        (
            "shock_paths; var e; periods 1; values self.e(-1); end;",
            "E405",
        ),
        (
            "shock_paths; exogenize y; periods 1, 2; values 1; endogenize e; end;",
            "E406",
        ),
        ("shock_paths; var e; periods 1; values u; end;", "E407"),
        (
            "shock_paths; var e; periods 1; values self.u(1); end;",
            "E408",
        ),
        (
            "shock_paths; var e; periods 1; values self.u(p); end;",
            "E409",
        ),
        (
            "shock_paths; var e; periods 1; values self.u(1,2); end;",
            "E410",
        ),
        ("shock_paths; var e; periods 1; values prev.u; end;", "E411"),
        (
            "shock_paths(learnt_in=2); var e; periods 1; values learnt_in(0).u; end;",
            "E412",
        ),
        (
            "shock_paths(learnt_in=2); var e; periods 1; values learnt_in(2).u; end;",
            "E413",
        ),
        ("database db db;", "E414"),
        ("shock_paths; var e; periods 1; values db.x; end;", "E415"),
        (
            "shock_paths; exogenize y; periods 1; values self.e; endogenize e; end;",
            "E416",
        ),
        ("endval; e += 1; end;", "E417"),
        ("endval(learnt_in=0); e = 1; end;", "E418"),
        ("endval(learnt_in=2); y = 1; end;", "E419"),
        ("shock_paths; var e; periods 1; values self.e; end;", "E420"),
        (
            "shock_paths(learnt_in=0); var e; periods 1; values 1; end;",
            "E421",
        ),
    ];
    for (body, code) in cases {
        let result = diagnostics(&base(body));
        assert!(has(&result, code), "{code} on {body}: {result:?}");
    }
    for body in [
        "shock_paths; var e; periods 1; values self.u; end;",
        "database db; shock_paths; var e; periods 1; values db.x; end;",
        "shock_paths; var e; periods 1; values missing.x(1); end;",
        "shock_paths; exogenize y; periods 1; values missing.x(1); endogenize e; end;",
        "shock_paths; var e; periods 1; values self.u(p-p); end;",
    ] {
        let result = diagnostics(&base(body));
        assert!(
            !has(&result, "E415")
                && !has(&result, "E409")
                && !has(&result, "E420")
                && !has(&result, "E416"),
            "{body}: {result:?}"
        );
    }
    let declared_control = diagnostics(&base(
        "database db; shock_paths; exogenize y; periods 1; values db.x(1); endogenize e; end;",
    ));
    assert!(has(&declared_control, "E416"));
}

#[test]
fn path_lag_folding_and_namespace_call_grammar() {
    for body in [
        "shock_paths; var e; periods 2; values self.e(0-0); end;",
        "shock_paths; var e; periods 2; values self.e(p-p); end;",
    ] {
        assert!(has(&diagnostics(&base(body)), "E420"), "{body}");
    }
    for (body, code) in [
        (
            "shock_paths; var e; periods 1; values self.u(1+0); end;",
            "E408",
        ),
        (
            "shock_paths; var e; periods 1; values self.u(0-1); end;",
            "E405",
        ),
        (
            "shock_paths; var e; periods 1; values self.u(p+1); end;",
            "E409",
        ),
        (
            "database db; shock_paths; var e; periods 1; values db.x(p+1); end;",
            "E409",
        ),
        (
            "shock_paths; var e; periods 2; values self.u(p*p); end;",
            "E409",
        ),
        (
            "shock_paths; var e; periods 2; values self.u(.5); end;",
            "E409",
        ),
    ] {
        assert!(has(&diagnostics(&base(body)), code), "{body}");
    }
    for body in [
        "shock_paths; var e; periods 1; values self.u(); end;",
        "shock_paths; var e; periods 1; values self.u(1,); end;",
        "shock_paths(learnt_in=2); var e; periods 1; values prev.u(); end;",
        "database db; shock_paths; var e; periods 1; values db.x(); end;",
        "shock_paths; var e; periods 1; values missing.x(); end;",
        "shock_paths(learnt_in=2); var e; periods 1; values learnt_in(1).u(); end;",
        "shock_paths(learnt_in=2); exogenize y; periods 1; values learnt_in(1).u(1,2); endogenize e; end;",
    ] {
        let result = diagnostics(&base(body));
        assert!(has(&result, "E001"), "{body}: {result:?}");
        assert!(!has(&result, "E416"), "syntax must take precedence: {body}");
    }
    for body in [
        "shock_paths; var e; periods 2; values self.u(0-1); end;",
        "shock_paths; var e; periods 2; values self.u(p-p); end;",
        "database db; shock_paths; var e; periods 1; values db.x(1-1); end;",
    ] {
        let result = diagnostics(&base(body));
        assert!(
            !has(&result, "E405") && !has(&result, "E408") && !has(&result, "E409"),
            "{body}: {result:?}"
        );
    }
}

#[test]
fn parse_refusals_preempt_path_self_cycle() {
    for (body, first_code) in [
        (
            "stoch_simul(irf_shocks=(y)); shock_paths; var e; periods 1; values self.e; end;",
            "E240",
        ),
        (
            "shock_paths; var e; periods 2, 2; values self.u(p), self.e; end;",
            "E409",
        ),
        (
            "database db db; shock_paths; var e; periods 1; values self.e; end;",
            "E414",
        ),
        (
            "initval; z = 1; end; shock_paths; var e; periods 1; values self.e; end;",
            "E058",
        ),
    ] {
        let result = diagnostics(&base(body));
        assert!(has(&result, first_code), "{body}: {result:?}");
        assert!(
            !has(&result, "E420"),
            "E420 leaked past {first_code}: {result:?}"
        );
    }
}

#[test]
fn reviewed_neighbours_accept_in_dynare_72() {
    let Some(pp) = find_preprocessor(None) else {
        return;
    };
    let sources = [
        fixture("quiet_control_unknown_call.mod"),
        "var y; varexo e u; parameters p; p=1; model; y=e+u+p; end; shock_paths; var e; periods 2; values self.u(p-p); end;".to_string(),
        base("shock_paths; var e; periods 2; values self.u(1-1); end;"),
        base("stoch_simul(irf_shocks=(e,u));"),
    ];
    for source in sources {
        let result = run_preprocessor(
            &source,
            &pp,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        assert!(result.success, "{}", result.raw_stdout);
        assert!(
            diagnostics(&source)
                .iter()
                .all(|d| d.severity != Severity::Error),
            "accepted neighbour had a Dygnosis Error: {source}"
        );
    }
}

#[test]
fn nondefault_learning_requires_expectation_errors_pair() {
    let cases = [
        ("shocks(learnt_in=2); var e; periods 2; values 1; end;", "E422"),
        ("mshocks(learnt_in=2); var e; periods 2; values 1; end;", "E422"),
        ("endval(learnt_in=2); e = 1; end;", "E423"),
        ("perfect_foresight_controlled_paths(learnt_in=2); exogenize y; periods 1; values 1; endogenize e; end;", "E424"),
        ("shock_paths(learnt_in=2); var e; periods 1; values 1; end;", "E425"),
    ];
    for (body, code) in cases {
        let result = diagnostics(&base(body));
        assert!(has(&result, code), "{code} on {body}: {result:?}");
        let complete = base(&format!(
            "{body} perfect_foresight_with_expectation_errors_setup(periods=3); perfect_foresight_with_expectation_errors_solver;"
        ));
        assert!(
            !has(&diagnostics(&complete), code),
            "{code} should be quiet with PFEE pair"
        );
    }
    for (body, code) in [
        ("shocks(learnt_in=1); var e; periods 1; values 1; end;", "E422"),
        ("endval(learnt_in=1); e = 1; end;", "E423"),
        ("perfect_foresight_controlled_paths(learnt_in=1); exogenize y; periods 1; values 1; endogenize e; end;", "E424"),
        ("shock_paths(learnt_in=1); var e; periods 1; values 1; end;", "E425"),
    ] {
        assert!(!has(&diagnostics(&base(body)), code), "{body}");
    }
}

#[test]
fn mshocks_and_shock_paths_option_duplicates_reach_e271() {
    for (body, name) in [
        (
            "mshocks(overwrite overwrite); var e; periods 1; values 1; end;",
            "overwrite",
        ),
        (
            "shock_paths(overwrite overwrite); var e; periods 1; values p; end;",
            "overwrite",
        ),
    ] {
        let result = diagnostics(&base(body));
        assert!(
            result.iter().any(|d| {
                d.code == "E271"
                    && d.message == format!("The '{name}' option is declared multiple times")
            }),
            "{body}: {result:?}"
        );
    }
    for body in [
        "mshocks(overwrite relative_to_initval); var e; periods 1; values 1; end;",
        "shock_paths(overwrite learnt_in=1); var e; periods 1; values p; end;",
    ] {
        assert!(!has(&diagnostics(&base(body)), "E271"), "{body}");
    }
    for body in [
        "mshocks(learnt_in=2 learnt_in=0); var e; periods 2; values 1; end;",
        "shock_paths(learnt_in=2 learnt_in=0); var e; periods 1; values p; end;",
    ] {
        let result = diagnostics(&base(body));
        assert!(has(&result, "E271") && !has(&result, "E400") && !has(&result, "E421"));
    }
}
