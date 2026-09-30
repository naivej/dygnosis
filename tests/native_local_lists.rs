use dygnosis::{analyze, find_preprocessor, parse, run_preprocessor, JsonStage};
use std::time::Duration;

const BASE: &str = "var y; varexo e; parameters a; a=.5; model; y=a*y(-1)+e; end;\n";

#[test]
fn expression_registered_locals_are_declared_wrong_types_on_every_list_set() {
    for (command, label, prefix) in [
        ("forecast", "{endogenous}", "forecast"),
        ("rplot", "{endogenous, exogenous}", "rplot"),
        (
            "plot_shock_decomposition",
            "{endogenous, epilogue}",
            "plot_shock_decomposition",
        ),
        ("osr_params", "{parameter}", "osr"),
    ] {
        for before in [true, false] {
            let source = format!(
                "{BASE}{} {command} mloc;\n{}",
                if before { "a=mloc;" } else { "" },
                if before { "" } else { "a=mloc;" }
            );
            let diagnostics = analyze(&parse(&source));
            assert!(
                !diagnostics.iter().any(|row| row.code == "E239"),
                "{source}: {diagnostics:?}"
            );
            let error = diagnostics
                .iter()
                .find(|row| row.code == "E240")
                .unwrap_or_else(|| panic!("{diagnostics:?}"));
            assert_eq!(
                error.message,
                format!("{prefix}: Variable mloc is not one of {label}")
            );
            if let Some(pp) = find_preprocessor(None) {
                let official = run_preprocessor(
                    &source,
                    &pp,
                    None,
                    Duration::from_secs(30),
                    JsonStage::Check,
                );
                assert!(!official.success, "{official:?}");
                assert!(official.raw_stdout.contains(&error.message), "{official:?}");
            }
        }
    }
}

#[test]
fn native_matlab_heads_stay_undeclared_and_known_aux_looking_locals_do_not_warn() {
    for prefix in ["xx=3;\n", "# xx=3;\n"] {
        let source = format!("{BASE}{prefix}forecast xx;");
        let diagnostics = analyze(&parse(&source));
        assert!(
            diagnostics.iter().any(|row| row.code == "E239"),
            "{diagnostics:?}"
        );
        assert!(
            !diagnostics.iter().any(|row| row.code == "E240"),
            "{diagnostics:?}"
        );
        if let Some(pp) = find_preprocessor(None) {
            let official = run_preprocessor(
                &source,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            assert!(
                official.raw_stdout.contains("was not declared"),
                "{official:?}"
            );
        }
    }
    for (name, code) in [("mloc", "E239"), ("AUX_ENDO_local", "W186")] {
        let source = format!("{BASE}xx={name};\nforecast {name};");
        let diagnostics = analyze(&parse(&source));
        assert!(
            diagnostics.iter().any(|row| row.code == code),
            "{diagnostics:?}"
        );
        assert!(
            !diagnostics.iter().any(|row| row.code == "E240"),
            "{diagnostics:?}"
        );
        if let Some(pp) = find_preprocessor(None) {
            let official = run_preprocessor(
                &source,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            assert_eq!(official.success, code == "W186", "{official:?}");
            assert!(
                official.raw_stdout.contains(if code == "W186" {
                    "possible auxiliary"
                } else {
                    "was not declared"
                }),
                "{official:?}"
            );
        }
    }
    let source = format!("{BASE}a=AUX_ENDO_local; forecast AUX_ENDO_local;");
    let diagnostics = analyze(&parse(&source));
    assert!(
        diagnostics.iter().any(|row| row.code == "E240"),
        "{diagnostics:?}"
    );
    assert!(
        !diagnostics.iter().any(|row| row.code == "W186"),
        "{diagnostics:?}"
    );
    if let Some(pp) = find_preprocessor(None) {
        let official = run_preprocessor(
            &source,
            &pp,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        assert!(
            official.raw_stdout.contains("is not one of {endogenous}"),
            "{official:?}"
        );
        assert!(
            !official.raw_stdout.contains("possible auxiliary"),
            "{official:?}"
        );
    }
}

#[test]
fn native_local_list_errors_agree_through_lsp_and_mcp() {
    let source = format!("{BASE}a=mloc; forecast mloc;");
    let mcp = dygnosis::dynare_diagnose(&source, None, None);
    let mcp = mcp.iter().find(|row| row.code == "E240").unwrap();
    let lsp = dygnosis::server::diagnostics_for("file:///native_local_lists.mod", &source);
    let lsp = lsp
        .iter()
        .find(|row| row.code == Some(tower_lsp::lsp_types::NumberOrString::String("E240".into())))
        .unwrap();
    assert_eq!(mcp.message, lsp.message);
    assert_eq!(mcp.severity, "ERROR");
    assert_eq!(
        lsp.severity,
        Some(tower_lsp::lsp_types::DiagnosticSeverity::ERROR)
    );
    assert_eq!(
        (mcp.line, mcp.column, mcp.end_line, mcp.end_column),
        (
            lsp.range.start.line + 1,
            lsp.range.start.character + 1,
            lsp.range.end.line + 1,
            lsp.range.end.character + 1
        )
    );
}

#[test]
fn generated_discount_parameter_heads_register_expression_locals() {
    let source = "var y pol; model(linear); y=0; end; planner_objective y^2; discretionary_policy(instruments=(pol));\noptimal_policy_discount_factor=mloc;\nforecast mloc;";
    let diagnostics = analyze(&parse(source));
    assert!(
        diagnostics.iter().any(|row| row.code == "E240"),
        "{diagnostics:?}"
    );
    assert!(
        !diagnostics.iter().any(|row| row.code == "E239"),
        "{diagnostics:?}"
    );
    if let Some(pp) = find_preprocessor(None) {
        let official =
            run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(
            official
                .raw_stdout
                .contains("forecast: Variable mloc is not one of {endogenous}"),
            "{official:?}"
        );
    }
}
