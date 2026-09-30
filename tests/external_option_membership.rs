use dygnosis::{analyze, find_preprocessor, parse, run_preprocessor, JsonStage, Severity};
use std::time::Duration;

fn source(options: &str) -> String {
    format!("var y; model; y=0; end; external_function({options});")
}

#[test]
fn named_external_functions_reject_options_outside_the_pinned_table() {
    for options in ["name=foo,wrong=1", "wrong=1,name=foo", "name=foo,periods=1"] {
        let source = source(options);
        let diagnostics = analyze(&parse(&source));
        let refusal = diagnostics
            .iter()
            .find(|row| row.code == "E001")
            .expect("unknown option refuses");
        let option = if options.contains("wrong") {
            "wrong"
        } else {
            "periods"
        };
        assert_eq!(
            &source[refusal.span.start as usize..refusal.span.end as usize],
            option
        );
        assert!(
            !diagnostics.iter().any(|row| row.code == "E322"),
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
            assert!(!official.success, "{source}: {official:?}");
            assert!(
                official.raw_stdout.contains("syntax error, unexpected"),
                "{official:?}"
            );
        }
    }
}

#[test]
fn valid_and_repeated_external_options_keep_the_first_value() {
    for options in [
        "name=foo,nargs=1",
        "name=foo,nargs=1,first_deriv_provided",
        "name=foo,nargs=1,first_deriv_provided,second_deriv_provided",
        "name=foo,nargs=1,nargs=2",
        "name=foo,name=bar,nargs=1",
    ] {
        let source = source(options);
        let model = parse(&source);
        let diagnostics = analyze(&model);
        assert!(
            !diagnostics
                .iter()
                .any(|row| row.severity == Severity::Error),
            "{source}: {diagnostics:?}"
        );
        assert_eq!(
            model.name(model.external_functions[0].name.unwrap().0),
            "foo"
        );
        assert_eq!(model.external_functions[0].nargs, Some(1));
        if let Some(pp) = find_preprocessor(None) {
            let official = run_preprocessor(
                &source,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            assert!(official.success, "{source}: {official:?}");
        }
    }
}

#[test]
fn nameless_shapes_keep_the_reviewed_missing_name_substitution() {
    for options in ["", "name=", "wrong=foo", "nargs=1"] {
        let source = source(options);
        let diagnostics = analyze(&parse(&source));
        assert!(
            diagnostics.iter().any(|row| row.code == "E322"),
            "{diagnostics:?}"
        );
        assert!(
            !diagnostics.iter().any(|row| row.code == "E001"),
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
            assert!(!official.success, "{source}: {official:?}");
        }
    }
}

#[test]
fn every_nargs_value_must_match_the_integer_production() {
    for options in [
        "name=foo,nargs=1.5,nargs=2",
        "name=foo,nargs=2,nargs=1.5",
        "name=foo,nargs=-1,nargs=2",
        "name=foo,nargs=2,nargs=1+1",
        "name=foo,nargs",
    ] {
        let source = source(options);
        let diagnostics = analyze(&parse(&source));
        assert!(
            diagnostics.iter().any(|row| row.code == "E001"),
            "{source}: {diagnostics:?}"
        );
        assert!(
            !diagnostics.iter().any(|row| row.code == "E271"),
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
            assert!(!official.success, "{source}: {official:?}");
            assert!(
                official.raw_stdout.contains("syntax error, unexpected"),
                "{official:?}"
            );
        }
    }
}

#[test]
fn repeated_named_values_still_require_a_filename() {
    for options in [
        "name=foo,name=",
        "name=foo,first_deriv_provided,first_deriv_provided=",
        "name=foo,first_deriv_provided,second_deriv_provided,second_deriv_provided=",
        "name=foo,first_deriv_provided=1,first_deriv_provided",
    ] {
        let source = source(options);
        let diagnostics = analyze(&parse(&source));
        assert!(
            diagnostics.iter().any(|row| row.code == "E001"),
            "{source}: {diagnostics:?}"
        );
        assert!(
            !diagnostics.iter().any(|row| row.code == "E271"),
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
            assert!(!official.success, "{source}: {official:?}");
        }
    }
    for options in [
        "name=foo,name='bar'",
        "name=foo,first_deriv_provided='bar',first_deriv_provided='baz'",
        "name=foo.bar,nargs=1",
    ] {
        let source = source(options);
        let diagnostics = analyze(&parse(&source));
        assert!(
            !diagnostics
                .iter()
                .any(|row| row.severity == Severity::Error),
            "{source}: {diagnostics:?}"
        );
        if let Some(pp) = find_preprocessor(None) {
            let official = run_preprocessor(
                &source,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            assert!(official.success, "{source}: {official:?}");
        }
    }
}

#[test]
fn repeated_options_still_need_one_comma_between_entries() {
    for options in [
        "name=foo,nargs=1 nargs=2",
        "name=foo,first_deriv_provided first_deriv_provided",
        "name=foo,nargs=1,,nargs=2",
        ",name=foo,nargs=1,nargs=2",
        "name=foo,nargs=1,nargs=2,",
    ] {
        let source = source(options);
        let diagnostics = analyze(&parse(&source));
        assert!(
            diagnostics.iter().any(|row| row.code == "E001"),
            "{source}: {diagnostics:?}"
        );
        if let Some(pp) = find_preprocessor(None) {
            let official = run_preprocessor(
                &source,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            assert!(!official.success, "{source}: {official:?}");
            assert!(
                official.raw_stdout.contains("syntax error, unexpected"),
                "{official:?}"
            );
        }
    }
}

#[test]
fn external_option_errors_agree_in_lsp_and_mcp() {
    use tower_lsp::lsp_types::{DiagnosticSeverity, NumberOrString};
    for options in [
        "name=foo,wrong=1",
        "name=foo,nargs=2,nargs=1.5",
        "name=foo,nargs=1,nargs=2",
    ] {
        let source = source(options);
        let mcp = dygnosis::dynare_diagnose(&source, None, None);
        let lsp = dygnosis::server::diagnostics_for("file:///external_options.mod", &source);
        let mcp = mcp.iter().find(|row| row.code == "E001");
        let lsp = lsp
            .iter()
            .find(|row| row.code == Some(NumberOrString::String("E001".into())));
        assert_eq!(mcp.is_some(), lsp.is_some());
        if let (Some(mcp), Some(lsp)) = (mcp, lsp) {
            assert_eq!(mcp.message, lsp.message);
            assert_eq!(mcp.severity, "ERROR");
            assert_eq!(lsp.severity, Some(DiagnosticSeverity::ERROR));
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
    }
}
