use std::time::Duration;

use dygnosis::model::{Model, ShockKind, SvarIdentificationElement};
use dygnosis::server::diagnostics_for;
use dygnosis::span::LineIndex;
use dygnosis::{
    analyze, dynare_diagnose, find_preprocessor, parse, run_preprocessor, JsonStage, Severity,
};
use tower_lsp::lsp_types::{DiagnosticSeverity, NumberOrString};

const BASE: &str = "var y;\nvarexo e u;\nparameters p;\np=.5;\nmodel;\ny=p*y(-1)+e+u;\nend;\n";

fn file(body: &str) -> String {
    format!("{BASE}{body}")
}

fn assert_refusal(body: &str, message: &str, token: &str) {
    let source = file(body);
    let model = parse(&source);
    let diags = analyze(&model);
    let errors: Vec<_> = diags.iter().filter(|d| d.code == "E001").collect();
    assert!(
        errors.iter().any(|d| d.message == message),
        "{body}: {diags:?}"
    );
    assert_eq!(errors.len(), 1, "{body}: {diags:?}");
    let refusal = errors.iter().find(|d| d.message == message).unwrap();
    assert_eq!(refusal.severity, Severity::Error);
    assert_eq!(
        &source[refusal.span.start as usize..refusal.span.end as usize],
        token
    );
    assert!(model.shock_stmts.is_empty(), "{body}");
    assert!(model.shocks_vars.is_empty(), "{body}");
    assert_eq!(model.shock_blocks.len(), 1, "{body}");
    assert!(model.shock_blocks[0].stochastic.is_empty(), "{body}");
    assert!(model.shock_blocks[0].scheduled.is_empty(), "{body}");
    assert!(model.namespace_qualified.is_empty(), "{body}");
}

#[test]
fn incomplete_regular_shock_at_end_uses_the_end_sentence_and_stores_no_row() {
    assert_refusal(
        "shocks; var e; end;",
        "syntax error, unexpected END, expecting PERIODS",
        "end",
    );
}

#[test]
fn incomplete_regular_shock_before_var_uses_the_var_sentence() {
    let source = file("shocks; var e; var u = 1; end;");
    let model = parse(&source);
    let diags = analyze(&model);
    let refusal = diags
        .iter()
        .find(|d| d.code == "E001")
        .unwrap_or_else(|| panic!("{diags:?}"));
    assert_eq!(
        refusal.message,
        "syntax error, unexpected VAR, expecting PERIODS or STDERR"
    );
    assert_eq!(
        &source[refusal.span.start as usize..refusal.span.end as usize],
        "var"
    );
    // The next complete row is a proved row boundary. The rejected e row adds
    // neither a variance record nor a measurement-error name.
    assert_eq!(model.shock_stmts.len(), 1);
    assert!(matches!(model.shock_stmts[0].kind, ShockKind::Var(n) if model.name(n) == "u"));
    assert_eq!(model.shocks_vars.len(), 1);
    assert_eq!(model.name(model.shocks_vars[0]), "u");
}

#[test]
fn incomplete_regular_shock_at_eof_uses_the_eof_sentence() {
    assert_refusal(
        "shocks; var e;",
        "syntax error, unexpected end of file, expecting PERIODS or STDERR",
        "",
    );
}

#[test]
fn incomplete_row_does_not_reach_name_type_or_duplicate_actions() {
    for name in ["bad", "p"] {
        let source = file(&format!("shocks; var {name}; end;"));
        let model = parse(&source);
        let diags = analyze(&model);
        assert!(diags
            .iter()
            .any(|d| d.code == "E001"
                && d.message == "syntax error, unexpected END, expecting PERIODS"));
        assert!(
            !diags
                .iter()
                .any(|d| matches!(d.code.as_str(), "E058" | "E266" | "E267")),
            "{name}: {diags:?}"
        );
        assert!(model.shock_stmts.is_empty());
        assert!(model.shocks_vars.is_empty());
    }
    let source = file("shocks; var e = .1; var e; end;");
    let model = parse(&source);
    let diags = analyze(&model);
    assert_eq!(model.shock_stmts.len(), 1);
    assert_eq!(model.shock_stmts[0].rhs, Some(0.1));
    assert!(!diags.iter().any(|d| d.code == "E111"), "{diags:?}");

    for (body, code) in [
        ("shocks; var bad = 1; end;", "E058"),
        ("shocks; var p = 1; end;", "E266"),
        ("shocks; var p; stderr 1; end;", "E267"),
        ("shocks; var e = 1; var e; stderr 1; end;", "E111"),
    ] {
        let diags = analyze(&parse(&file(body)));
        assert!(diags.iter().any(|d| d.code == code), "{body}: {diags:?}");
    }
}

#[test]
fn completed_unknown_shock_refuses_before_a_later_incomplete_row() {
    let source = file("shocks; var bad = 1; var e; end;");
    if let Some(binary) = find_preprocessor(None) {
        let result = run_preprocessor(
            &source,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        let output = format!("{} {}", result.raw_stdout, result.raw_stderr);
        assert!(!result.success, "Dynare 7.2 accepted {source}");
        assert!(output.contains("Unknown symbol: bad"), "{output}");
        assert!(!output.contains("unexpected END"), "{output}");
    }
    let diags = analyze(&parse(&source));
    assert!(
        diags
            .iter()
            .any(|d| d.code == "E058" && d.message == "Unknown symbol: bad."),
        "{diags:?}"
    );
    assert!(!diags.iter().any(|d| d.code == "E001"), "{diags:?}");
}

#[test]
fn completed_duplicate_shock_refuses_before_a_later_incomplete_row() {
    let source = file("shocks; var e = 1; var e = 2; var u; end;");
    if let Some(binary) = find_preprocessor(None) {
        let result = run_preprocessor(
            &source,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        let output = format!("{} {}", result.raw_stdout, result.raw_stderr);
        assert!(!result.success, "Dynare 7.2 accepted {source}");
        assert!(
            output.contains("variance or stderr of shock on e declared twice"),
            "{output}"
        );
        assert!(!output.contains("unexpected END"), "{output}");
    }
    let model = parse(&source);
    let diags = analyze(&model);
    assert!(diags.iter().any(|d| d.code == "E111"), "{diags:?}");
    assert!(!diags.iter().any(|d| d.code == "E001"), "{diags:?}");
    assert_eq!(model.shock_stmts.len(), 2);
}

#[test]
fn later_incomplete_row_preempts_completed_shock_check_stage_type_errors() {
    for body in [
        "shocks; var p = 1; var e; end;",
        "shocks; var p; stderr 1; var e; end;",
    ] {
        let source = file(body);
        if let Some(binary) = find_preprocessor(None) {
            let result = run_preprocessor(
                &source,
                &binary,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            let output = format!("{} {}", result.raw_stdout, result.raw_stderr);
            assert!(!result.success, "Dynare 7.2 accepted {source}");
            assert!(
                output.contains("syntax error, unexpected END, expecting PERIODS"),
                "{output}"
            );
            assert!(!output.contains("setting a variance"), "{output}");
            assert!(!output.contains("setting a standard error"), "{output}");
        }
        let diags = analyze(&parse(&source));
        assert!(
            diags.iter().any(|d| d.code == "E001"
                && d.message == "syntax error, unexpected END, expecting PERIODS"),
            "{diags:?}"
        );
        assert!(
            !diags
                .iter()
                .any(|d| matches!(d.code.as_str(), "E266" | "E267")),
            "{diags:?}"
        );
    }
}

#[test]
fn complete_regular_productions_keep_their_rows_and_values() {
    for (body, rows, scheduled) in [
        ("shocks; var e; stderr .1; end;", 1, 0),
        ("shocks; var e; periods 1; values .1; end;", 0, 1),
        ("shocks; var e = .1; end;", 1, 0),
        ("shocks; var e, u = .01; end;", 1, 0),
        ("shocks; corr e, u = .1; end;", 1, 0),
    ] {
        let model = parse(&file(body));
        let diags = analyze(&model);
        assert!(
            !diags.iter().any(|d| d.severity == Severity::Error),
            "{body}: {diags:?}"
        );
        assert_eq!(model.shock_stmts.len(), rows, "{body}");
        assert_eq!(model.shock_blocks[0].stochastic.len(), rows, "{body}");
        assert_eq!(model.shock_blocks[0].scheduled.len(), scheduled, "{body}");
    }
}

#[test]
fn regular_row_repair_preserves_heteroskedastic_stderr_sentence() {
    let source = file("heteroskedastic_shocks; var e; stderr .1; end;");
    let diags = analyze(&parse(&source));
    assert!(diags
        .iter()
        .any(|d| d.code == "E001"
            && d.message == "syntax error, unexpected STDERR, expecting PERIODS"));
}

#[test]
fn path_end_closes_the_body_before_reading_its_required_separator() {
    let control = file("shock_paths; var e; periods 1:end; values 1; end;");
    let model = parse(&control);
    assert!(!analyze(&model).iter().any(|d| d.code == "E001"));
    assert_eq!(model.shock_paths[0].stanzas.len(), 1);
    assert!(model.ms_unparsed_spans.is_empty());
    let source = file("shock_paths; var e; periods 1:end; values 1; end\nshocks; var u = 1; end;");
    let model = parse(&source);
    let diags = analyze(&model);
    assert!(
        diags
            .iter()
            .any(|d| d.code == "E001"
                && d.message == "syntax error, unexpected SHOCKS, expecting ';'"),
        "{diags:?}"
    );
    assert_eq!(model.shock_paths.len(), 1);
    assert_eq!(model.shock_paths[0].stanzas.len(), 1);
    assert!(model.shock_blocks.is_empty());
    assert!(model.shock_stmts.is_empty());
    let accepted = file("shock_paths; var e; periods 1:end; values 1; end\nplot(1);\n;");
    let model = parse(&accepted);
    assert!(!analyze(&model).iter().any(|d| d.code == "E001"));
    assert_eq!(model.shock_paths[0].stanzas.len(), 1);
}

#[test]
fn shock_row_refusal_and_complete_control_match_library_lsp_and_mcp() {
    let source = file("shocks; var e; end;");
    let library = analyze(&parse(&source))
        .into_iter()
        .find(|d| d.code == "E001")
        .unwrap();
    let mcp = dynare_diagnose(&source, None, None)
        .into_iter()
        .find(|d| d.code == "E001")
        .unwrap();
    let lsp = diagnostics_for("file:///C:/tmp/parse_gap_shocks.mod", &source)
        .into_iter()
        .find(|d| matches!(&d.code, Some(NumberOrString::String(c)) if c == "E001"))
        .unwrap();
    assert_eq!(library.message, mcp.message);
    assert_eq!(library.message, lsp.message);
    assert_eq!(mcp.severity, "ERROR");
    assert_eq!(lsp.severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(mcp.line, lsp.range.start.line + 1);
    assert_eq!(mcp.column, lsp.range.start.character + 1);
    assert_eq!(mcp.end_line, lsp.range.end.line + 1);
    assert_eq!(mcp.end_column, lsp.range.end.character + 1);
    assert_eq!(mcp.end_column - mcp.column, 3);
    let control = file("shocks; var e; stderr .1; end;");
    assert!(!analyze(&parse(&control)).iter().any(|d| d.code == "E001"));
    assert!(!dynare_diagnose(&control, None, None)
        .iter()
        .any(|d| d.code == "E001"));
    assert!(
        !diagnostics_for("file:///C:/tmp/parse_gap_shocks_quiet.mod", &control)
            .iter()
            .any(|d| matches!(&d.code, Some(NumberOrString::String(c)) if c == "E001"))
    );
}

#[test]
fn optional_dynare_72_locks_each_incomplete_row_and_complete_production() {
    let Some(binary) = find_preprocessor(None) else {
        eprintln!("Dynare 7.2 is not installed; skipping incomplete-shock honesty runs");
        return;
    };
    for (body, message) in [
        (
            "shocks; var e; end;",
            "syntax error, unexpected END, expecting PERIODS",
        ),
        (
            "shocks; var e; var u = 1; end;",
            "syntax error, unexpected VAR, expecting PERIODS or STDERR",
        ),
        (
            "shocks; var e;",
            "syntax error, unexpected end of file, expecting PERIODS or STDERR",
        ),
        (
            "shocks; var bad; end;",
            "syntax error, unexpected END, expecting PERIODS",
        ),
        (
            "shocks; var p; end;",
            "syntax error, unexpected END, expecting PERIODS",
        ),
        (
            "shocks; var e = .1; var e; end;",
            "syntax error, unexpected END, expecting PERIODS",
        ),
        (
            "heteroskedastic_shocks; var e; stderr .1; end;",
            "syntax error, unexpected STDERR, expecting PERIODS",
        ),
    ] {
        let result = run_preprocessor(
            &file(body),
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        let output = format!("{} {}", result.raw_stdout, result.raw_stderr);
        assert!(!result.success, "Dynare 7.2 accepted {body}");
        assert!(output.contains(message), "{body}: {output}");
        assert!(!output.contains("Unknown symbol"), "{body}: {output}");
        assert!(!output.contains("declared twice"), "{body}: {output}");
    }
    for body in [
        "shocks; var e; stderr .1; end;",
        "shocks; var e; periods 1; values .1; end;",
        "shocks; var e = .1; end;",
        "shocks; var e, u = .01; end;",
        "shocks; corr e, u = .1; end;",
    ] {
        let result = run_preprocessor(
            &file(body),
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        assert!(
            result.success,
            "{body}: {} {}",
            result.raw_stdout, result.raw_stderr
        );
    }
}

const FAMILY_BASE: &str = "var y; varexo e; model; y=e; end;\n";
const LEXER_MESSAGE: &str = "character unrecognized by lexer";
const PATH_DUPLICATE_MESSAGE: &str = "shocks/conditional_forecast_paths: variable y declared twice";
const SVAR_DUPLICATE_MESSAGE: &str = "y restriction added twice.";

fn family_file(body: &str) -> String {
    format!("{FAMILY_BASE}{body}")
}

/// Lock the first Parse refusal and its mapped presentation in both adapters.
fn family_refusal(source: &str, code: &str, message: &str) -> Model {
    let model = parse(source);
    let diags = analyze(&model);
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{source}: {diags:?}");
    assert_eq!(errors[0].code, code, "{source}: {diags:?}");
    assert_eq!(errors[0].message, message, "{source}: {diags:?}");
    let mcp = dynare_diagnose(source, None, None);
    let errors_mcp: Vec<_> = mcp.iter().filter(|d| d.severity == "ERROR").collect();
    assert_eq!(errors_mcp.len(), 1, "{source}: {mcp:?}");
    assert_eq!(errors_mcp[0].code, code, "{source}: {mcp:?}");
    assert_eq!(errors_mcp[0].message, message, "{source}: {mcp:?}");
    let index = LineIndex::new(source);
    let start = index.position(source, errors[0].span.start);
    let end = index.position(source, errors[0].span.end);
    assert_eq!(errors_mcp[0].line, start.line + 1);
    assert_eq!(errors_mcp[0].column, start.character + 1);
    assert_eq!(errors_mcp[0].end_line, end.line + 1);
    assert_eq!(errors_mcp[0].end_column, end.character + 1);
    model
}

fn family_honesty(source: &str, message: &str, absent: &str) {
    let Some(binary) = find_preprocessor(None) else {
        eprintln!("Dynare 7.2 is not installed; skipping family-row honesty run");
        return;
    };
    let result = run_preprocessor(
        source,
        &binary,
        None,
        Duration::from_secs(30),
        JsonStage::Check,
    );
    let output = format!("{} {}", result.raw_stdout, result.raw_stderr);
    assert!(!result.success, "Dynare 7.2 accepted {source}");
    assert!(output.contains(message), "{source}: {output}");
    assert!(!output.contains(absent), "{source}: {output}");
}

fn assert_complete_path_rows(model: &Model, count: usize) {
    assert_eq!(model.conditional_forecast_paths.len(), 1);
    let block = &model.conditional_forecast_paths[0];
    assert_eq!(block.rows.len(), count);
    assert!(block.shape_refuses.is_empty(), "{:?}", block.shape_refuses);
    for row in &block.rows {
        assert_eq!(model.name(row.name), "y");
        assert!(row.has_periods && row.has_values);
        assert_eq!(row.periods.len(), 1);
        assert_eq!(row.values.len(), 1);
    }
}

fn assert_completed_svar_exclusion(model: &Model) {
    assert_eq!(model.svar_identifications.len(), 1);
    let block = &model.svar_identifications[0];
    assert_eq!(block.elements.len(), 1, "{:?}", block.elements);
    assert!(block.shape_refuses.is_empty(), "{:?}", block.shape_refuses);
    let SvarIdentificationElement::ExclusionLag { lag, equations, .. } = &block.elements[0] else {
        panic!("Expected the completed exclusion, got {:?}", block.elements);
    };
    assert_eq!(*lag, Some(1));
    assert_eq!(equations.len(), 1);
    assert_eq!(equations[0].number, Some(1));
    assert_eq!(equations[0].names.len(), 2);
    assert!(equations[0]
        .names
        .iter()
        .all(|(name, _)| model.name(*name) == "y"));
}

#[test]
fn conditional_path_refused_values_create_no_partial_row_or_missing_values_refusal() {
    let source = family_file("conditional_forecast_paths; var y; periods 1; values {1}; end;");
    family_honesty(&source, LEXER_MESSAGE, "expecting VALUES");
    let model = family_refusal(&source, "E001", LEXER_MESSAGE);
    assert_complete_path_rows(&model, 0);
    assert!(model.namespace_qualified.is_empty());
}

#[test]
fn completed_conditional_path_duplicate_precedes_later_refused_values() {
    let source = family_file(
        "conditional_forecast_paths; \
         var y; periods 1; values 1; \
         var y; periods 2; values 2; \
         var y; periods 3; values {3}; end;",
    );
    family_honesty(&source, PATH_DUPLICATE_MESSAGE, LEXER_MESSAGE);
    let model = family_refusal(&source, "E344", PATH_DUPLICATE_MESSAGE);
    assert_complete_path_rows(&model, 2);
}

#[test]
fn conditional_path_lexer_refusal_precedes_later_completed_duplicates() {
    let source = family_file(
        "conditional_forecast_paths; \
         var y; periods 1; values {1}; \
         var y; periods 2; values 2; \
         var y; periods 3; values 3; end;",
    );
    family_honesty(&source, LEXER_MESSAGE, PATH_DUPLICATE_MESSAGE);
    let model = family_refusal(&source, "E001", LEXER_MESSAGE);
    assert_complete_path_rows(&model, 2);
}

#[test]
fn completed_svar_duplicate_precedes_later_refused_restriction() {
    let source = family_file(
        "svar_identification; exclusion lag 1; equation 1,y,y; \
         restriction equation 1,{y}=0; end;",
    );
    family_honesty(&source, SVAR_DUPLICATE_MESSAGE, LEXER_MESSAGE);
    let model = family_refusal(&source, "E361", SVAR_DUPLICATE_MESSAGE);
    assert_completed_svar_exclusion(&model);
}

#[test]
fn svar_lexer_refusal_precedes_later_completed_duplicate() {
    let source = family_file(
        "svar_identification; restriction equation 1,{y}=0; \
         exclusion lag 1; equation 1,y,y; end;",
    );
    family_honesty(&source, LEXER_MESSAGE, SVAR_DUPLICATE_MESSAGE);
    let model = family_refusal(&source, "E001", LEXER_MESSAGE);
    assert_completed_svar_exclusion(&model);
}

#[test]
fn complete_family_rows_remain_quiet_for_lexer_and_duplicate_refusals() {
    for body in [
        "conditional_forecast_paths; var y; periods 1; values 1; end;",
        "svar_identification; exclusion lag 1; equation 1,y; end;",
    ] {
        let source = family_file(body);
        let model = parse(&source);
        let diags = analyze(&model);
        assert!(
            !diags.iter().any(|d| d.severity == Severity::Error),
            "{source}: {diags:?}"
        );
        let mcp = dynare_diagnose(&source, None, None);
        assert!(
            !mcp.iter().any(|d| d.severity == "ERROR"),
            "{source}: {mcp:?}"
        );
        if body.starts_with("conditional_forecast_paths") {
            assert_complete_path_rows(&model, 1);
        } else {
            assert_eq!(model.svar_identifications.len(), 1);
            let elements = &model.svar_identifications[0].elements;
            assert!(matches!(elements.as_slice(),
                [SvarIdentificationElement::ExclusionLag { lag: Some(1), equations, .. }]
                if equations.len() == 1 && equations[0].names.len() == 1
                    && model.name(equations[0].names[0].0) == "y"));
        }
        if let Some(binary) = find_preprocessor(None) {
            let result = run_preprocessor(
                &source,
                &binary,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            assert!(
                result.success,
                "{source}: {} {}",
                result.raw_stdout, result.raw_stderr
            );
        }
    }
}

#[test]
fn family_row_priority_and_retention_use_macro_execution_order_at_shared_written_spans() {
    for (body, code, message, absent, paths) in [
        (
            "conditional_forecast_paths; var y; periods 1; values 1; var y; periods 2; values 2; var y; periods 3; values {3}; end;",
            "E344", PATH_DUPLICATE_MESSAGE, LEXER_MESSAGE, true,
        ),
        (
            "conditional_forecast_paths; var y; periods 1; values {1}; var y; periods 2; values 2; var y; periods 3; values 3; end;",
            "E001", LEXER_MESSAGE, PATH_DUPLICATE_MESSAGE, true,
        ),
        (
            "svar_identification; exclusion lag 1; equation 1,y,y; restriction equation 1,{y}=0; end;",
            "E361", SVAR_DUPLICATE_MESSAGE, LEXER_MESSAGE, false,
        ),
        (
            "svar_identification; restriction equation 1,{y}=0; exclusion lag 1; equation 1,y,y; end;",
            "E001", LEXER_MESSAGE, SVAR_DUPLICATE_MESSAGE, false,
        ),
    ] {
        let source = family_file(&format!(
            "@#define family_body = \"{body}\"\n@{{family_body}}\n"
        ));
        family_honesty(&source, message, absent);
        let model = family_refusal(&source, code, message);
        if paths {
            assert_complete_path_rows(&model, 2);
        } else {
            assert_completed_svar_exclusion(&model);
        }
    }
}
