use dygnosis::{analyze, find_preprocessor, parse, run_preprocessor, JsonStage};
use std::time::Duration;

fn source(operation: &str, definition: &str) -> String {
    format!("var y; parameters z a;\n@#for i in 1:2\n@#if i == 2\n{operation}\n@#endif\n@#if i == 1\n{definition}\n@#endif\n@#endfor\nmodel; y=z+a; end;")
}

#[test]
fn later_macro_iterations_use_previously_executed_subsample_definitions() {
    for operation in [
        "a.subsamples=z.subsamples;",
        "z.s.prior(shape=normal,mean=0,stdev=1);",
        "a.subsamples=z.subsamples; a.s.prior(shape=normal,mean=0,stdev=1);",
    ] {
        let source = source(operation, "z.subsamples(s=2000Q1:2000Q4);");
        let diagnostics = analyze(&parse(&source));
        assert!(
            !diagnostics
                .iter()
                .any(|row| matches!(row.code.as_str(), "E428" | "E429" | "E430")),
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
fn genuinely_missing_sources_and_ranges_keep_their_refusals() {
    for (operation, definition, code) in [
        ("a.subsamples=z.subsamples;", "", "E428"),
        ("z.s.prior(shape=normal,mean=0,stdev=1);", "", "E429"),
        (
            "z.missing.prior(shape=normal,mean=0,stdev=1);",
            "z.subsamples(s=2000Q1:2000Q4);",
            "E430",
        ),
    ] {
        let source = source(operation, definition);
        let diagnostics = analyze(&parse(&source));
        let refusal = diagnostics
            .iter()
            .find(|row| row.code == code)
            .unwrap_or_else(|| panic!("{code}: {source}: {diagnostics:?}"));
        if let Some(pp) = find_preprocessor(None) {
            let official = run_preprocessor(
                &source,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            assert!(
                !official.success && official.raw_stdout.contains(&refusal.message),
                "{source}: {official:?}"
            );
        }
        let mcp = dygnosis::dynare_diagnose(&source, None, None);
        let lsp = dygnosis::server::diagnostics_for("file:///subsample_order.mod", &source);
        let a = mcp.iter().find(|row| row.code == code).unwrap();
        let b = lsp
            .iter()
            .find(|row| row.code == Some(tower_lsp::lsp_types::NumberOrString::String(code.into())))
            .unwrap();
        assert_eq!(a.message, b.message);
        assert_eq!(
            (a.line, a.column),
            (b.range.start.line + 1, b.range.start.character + 1)
        );
    }
}
