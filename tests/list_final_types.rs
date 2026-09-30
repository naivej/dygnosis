use dygnosis::{analyze, find_preprocessor, parse, run_preprocessor, JsonStage};
use std::time::Duration;

fn check_list(source: &str, refuses: bool, needle: &str) {
    let diagnostics = analyze(&parse(source));
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|row| matches!(row.code.as_str(), "E239" | "E240"))
        .collect();
    assert_eq!(
        errors.len(),
        usize::from(refuses),
        "{source}: {diagnostics:?}"
    );
    if refuses {
        assert_eq!(errors[0].code, "E240");
        assert_eq!(errors[0].message, needle);
    }
    if let Some(pp) = find_preprocessor(None) {
        let official =
            run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        if refuses {
            assert!(!official.success, "{source}: {official:?}");
            assert!(
                official.raw_stdout.contains(needle),
                "{source}: {official:?}"
            );
        } else {
            assert!(
                official.success
                    || (source.starts_with("heterogeneity_dimension")
                        && official
                            .raw_stdout
                            .contains("not supported for heterogeneous models")),
                "{source}: {official:?}"
            );
            assert!(
                !official.raw_stdout.contains("is not one of"),
                "{source}: {official:?}"
            );
            assert!(
                !official.raw_stdout.contains("was not declared"),
                "{source}: {official:?}"
            );
        }
    }
}

#[test]
fn command_lists_use_final_types_in_both_statement_orders() {
    for (written, final_type, command, allowed, label, prefix) in [
        (
            "var",
            "parameters",
            "osr_params",
            true,
            "{parameter}",
            "osr",
        ),
        (
            "parameters",
            "var",
            "forecast",
            true,
            "{endogenous}",
            "forecast",
        ),
        (
            "var",
            "parameters",
            "forecast",
            false,
            "{endogenous}",
            "forecast",
        ),
        (
            "parameters",
            "var",
            "osr_params",
            false,
            "{parameter}",
            "osr",
        ),
        (
            "parameters",
            "varexo",
            "rplot",
            true,
            "{endogenous, exogenous}",
            "rplot",
        ),
        (
            "varexo",
            "varexo_det",
            "rplot",
            false,
            "{endogenous, exogenous}",
            "rplot",
        ),
        (
            "parameters",
            "var",
            "plot_shock_decomposition",
            true,
            "{endogenous, epilogue}",
            "plot_shock_decomposition",
        ),
        (
            "var",
            "varexo",
            "plot_shock_decomposition",
            false,
            "{endogenous, epilogue}",
            "plot_shock_decomposition",
        ),
    ] {
        for before in [true, false] {
            let list = format!("{command} p;");
            let change = format!("change_type({final_type}) p;");
            let source = format!(
                "var y; {written} p; {} {} model; y=p; end;",
                if before { &list } else { &change },
                if before { &change } else { &list }
            );
            check_list(
                &source,
                !allowed,
                &format!("{prefix}: Variable p is not one of {label}"),
            );
        }
    }
}

#[test]
fn command_lists_distinguish_excluded_and_restored_names() {
    for suffix in ["", "change_type(var) p;"] {
        let source = format!("var y p; model; y=0; end; var_remove p; {suffix} forecast p;");
        check_list(
            &source,
            suffix.is_empty(),
            "forecast: Variable p is not one of {endogenous}",
        );
    }
}

#[test]
fn heterogeneous_names_are_wrong_types_until_successfully_made_ordinary() {
    for (written, command, label, prefix) in [
        ("var", "rplot", "{endogenous, exogenous}", "rplot"),
        ("parameters", "osr_params", "{parameter}", "osr"),
    ] {
        for ordinary in [false, true] {
            let change = if ordinary {
                format!("change_type({written}) p;")
            } else {
                String::new()
            };
            let source = format!("heterogeneity_dimension h; var y; {written}(heterogeneity=h) p; {change} model; y=0; end; {command} p;");
            check_list(
                &source,
                !ordinary,
                &format!("{prefix}: Variable p is not one of {label}"),
            );
        }
    }
}
