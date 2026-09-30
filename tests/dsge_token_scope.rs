use dygnosis::{analyze, find_preprocessor, parse, run_preprocessor, JsonStage};
use std::time::Duration;

#[test]
fn dsge_weight_is_an_ordinary_name_in_statement_expressions() {
    for source in [
        "parameters p; p=dsge_prior_weight; var y; model; y=0; end;",
        "var y pol; model; y=0; end; planner_objective y^2; discretionary_policy(instruments=(pol),planner_discount=dsge_prior_weight);",
        "@#define name = \"dsge_prior_weight\"\nparameters p; p=@{name}; var y; model; y=0; end;",
    ] {
        let diagnostics = analyze(&parse(source));
        assert!(!diagnostics.iter().any(|row| row.code == "E001"), "{source}: {diagnostics:?}");
        if let Some(pp) = find_preprocessor(None) {
            let official = run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
            assert!(official.success, "{source}: {official:?}");
        }
    }
}

#[test]
fn dsge_weight_remains_reserved_in_block_expressions_and_name_slots() {
    for source in [
        "var y dsge_prior_weight; model; y=dsge_prior_weight; end;",
        "var y; model; #dsge_prior_weight=1; y=0; end;",
        "var y; model; y=0; end; initval; y=dsge_prior_weight; end;",
        "var y; model; y=0; end; steady_state_model; y=dsge_prior_weight; end;",
        "var y; varexo e; model; y=e; end; shocks; var e; stderr dsge_prior_weight; end;",
        "var y; varexo e; model; y=e; end; mshocks; var e; periods 1; values dsge_prior_weight; end;",
        "var y; varexo e; model; y=e; end; mshocks; var e; periods 1; values (dsge_prior_weight+1); end;",
        "var y dsge_prior_weight; model; y=0; end; initval; dsge_prior_weight=1; end;",
        "@#define name = \"dsge_prior_weight\"\nvar y; model; y=@{name}; end;",
    ] {
        let diagnostics = analyze(&parse(source));
        assert!(
            diagnostics.iter().any(|row| row.code == "E001" && row.message.contains("dsge_prior_weight")),
            "{source}: {diagnostics:?}"
        );
        if let Some(pp) = find_preprocessor(None) {
            let official =
                run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
            assert!(!official.success, "{source}: {official:?}");
            assert!(
                official.raw_stdout.contains("DSGE_PRIOR_WEIGHT"),
                "{source}: {official:?}"
            );
        }
    }
}

#[test]
fn block_token_errors_agree_through_lsp_and_mcp() {
    let source = "var y;\nmodel; y=dsge_prior_weight; end;";
    let mcp = dygnosis::dynare_diagnose(source, None, None);
    let mcp = mcp.iter().find(|row| row.code == "E001").unwrap();
    let lsp = dygnosis::server::diagnostics_for("file:///dsge_token_scope.mod", source);
    let lsp = lsp
        .iter()
        .find(|row| row.code == Some(tower_lsp::lsp_types::NumberOrString::String("E001".into())))
        .unwrap();
    assert_eq!(mcp.message, lsp.message);
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
