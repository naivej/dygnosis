use dygnosis::{analyze, find_preprocessor, parse, run_preprocessor, JsonStage};
use std::time::Duration;

#[test]
fn expression_local_clashes_with_later_declarations_in_parser_order() {
    for later in ["var", "varexo", "varexo_det", "parameters"] {
        let source = format!("parameters a; a=mloc; {later} mloc; var y; model; y=0; end;");
        let diagnostics = analyze(&parse(&source));
        let error = diagnostics
            .iter()
            .find(|row| row.code == "E030")
            .unwrap_or_else(|| panic!("{source}: {diagnostics:?}"));
        assert_eq!(
            error.message,
            "Symbol mloc declared twice with different types!"
        );
        assert_eq!(
            &source[error.span.start as usize..error.span.end as usize],
            "mloc"
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

#[test]
fn ordinary_declarations_repeated_expressions_and_native_text_stay_quiet() {
    for body in [
        "parameters mloc; a=mloc; a=mloc;",
        "a=mloc; a=mloc;",
        "xx=mloc;\nvar mloc;",
    ] {
        let source = format!("parameters a; {body} var y; model; y=0; end;");
        let diagnostics = analyze(&parse(&source));
        assert!(
            !diagnostics
                .iter()
                .any(|row| matches!(row.code.as_str(), "E030" | "W031")),
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
            assert!(official.success, "{official:?}");
            assert!(
                !official.raw_stdout.contains("declared twice"),
                "{official:?}"
            );
        }
    }
}

#[test]
fn repeated_macro_spans_preserve_native_local_declaration_order() {
    let source =
        "parameters a;\n@#for j in 1:2\na=mloc;\nvar mloc;\n@#endfor\nvar y; model; y=0; end;";
    let diagnostics = analyze(&parse(source));
    assert!(
        diagnostics.iter().any(|row| row.code == "E030"
            && row.message == "Symbol mloc declared twice with different types!"),
        "{diagnostics:?}"
    );
    if let Some(pp) = find_preprocessor(None) {
        let official =
            run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(!official.success, "{official:?}");
        assert!(
            official
                .raw_stdout
                .contains("Symbol mloc declared twice with different types!"),
            "{official:?}"
        );
    }
}
