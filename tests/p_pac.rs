use std::path::{Path, PathBuf};
use std::time::Duration;

use dygnosis::model::{
    NamedModelOperatorKind, PacTargetComponentRow, PacTargetInfoRow, SemiStructuralKind,
    SemiStructuralValue,
};
use dygnosis::{analyze, find_preprocessor, parse, run_preprocessor, JsonStage};

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/p_pac")
        .join(name);
    let source = std::fs::read_to_string(&path).unwrap();
    assert!(source.starts_with("// inventory: "), "{}", path.display());
    source
}

fn pinned_binary() -> Option<PathBuf> {
    find_preprocessor(None).filter(|path| {
        path.components()
            .any(|part| part.as_os_str().to_string_lossy() == "7.2")
    })
}

fn official_check(source: &str, binary: &Path) -> (bool, String) {
    let result = run_preprocessor(
        source,
        binary,
        None,
        Duration::from_secs(30),
        JsonStage::Check,
    );
    (
        result.success,
        format!("{} {}", result.raw_stdout, result.raw_stderr),
    )
}

fn quiet_file(name: &str, binary: Option<&Path>) -> dygnosis::model::Model {
    let source = fixture(name);
    let model = parse(&source);
    let diagnostics = analyze(&model);
    assert!(
        !diagnostics.iter().any(|diag| diag.code == "E001"),
        "{name}: {diagnostics:?}"
    );
    if let Some(binary) = binary {
        let (accepted, report) = official_check(&source, binary);
        assert!(accepted, "7.2 refused {name}: {report}");
    }
    model
}

#[test]
fn accepted_family_keeps_written_structure() {
    let binary = pinned_binary();
    let model = quiet_file("accepted_family.mod", binary.as_deref());
    assert_eq!(model.semi_structural_commands.len(), 4);
    assert_eq!(
        model
            .semi_structural_commands
            .iter()
            .map(|command| command.kind)
            .collect::<Vec<_>>(),
        [
            SemiStructuralKind::VarModel,
            SemiStructuralKind::TrendComponentModel,
            SemiStructuralKind::VarExpectationModel,
            SemiStructuralKind::PacModel,
        ]
    );
    let var = &model.semi_structural_commands[0];
    assert_eq!(var.options[0].name, "MODEL_NAME");
    assert!(matches!(
        var.options[1].value,
        SemiStructuralValue::Tags(ref tags) if tags[0].0 == "eq:x" && tags[1].0 == "eq:y"
    ));
    assert!(matches!(var.options[2].value, SemiStructuralValue::Flag));
    let expectation = &model.semi_structural_commands[2];
    assert!(matches!(
        expectation.options[3].value,
        SemiStructuralValue::Horizon { ref first, ref last, .. }
            if first == "1" && last == "Inf"
    ));
    assert!(matches!(
        expectation.options[1].value,
        SemiStructuralValue::Expression(ref expr)
            if expr.text == "x+y(-1)" && expr.expr.is_some()
    ));
    assert_eq!(model.pac_target_info.len(), 1);
    let info = &model.pac_target_info[0];
    assert_eq!(model.name(info.name), "pac_x");
    assert_eq!(info.rows.len(), 4);
    assert!(matches!(info.rows[0], PacTargetInfoRow::Target(_)));
    let PacTargetInfoRow::Component(ref component) = info.rows[3] else {
        panic!("missing second component")
    };
    assert_eq!(component.component.text, "x");
    assert!(matches!(
        component.rows[0],
        PacTargetComponentRow::Growth(ref expr) if expr.text == "diff(x(-1))"
    ));
    assert_eq!(
        model
            .named_model_operators
            .iter()
            .map(|operator| operator.kind)
            .collect::<Vec<_>>(),
        [
            NamedModelOperatorKind::VarExpectation,
            NamedModelOperatorKind::PacExpectation,
            NamedModelOperatorKind::PacTargetNonstationary,
        ]
    );
    assert_eq!(model.equations.len(), 4);
    assert_eq!(model.summary().n_model_equations, 4);
}

#[test]
fn deterministic_trends_is_independent() {
    let binary = pinned_binary();
    let model = quiet_file("deterministic_trends.mod", binary.as_deref());
    assert!(model.semi_structural_commands.is_empty());
    assert_eq!(model.deterministic_trends.len(), 1);
    let row = &model.deterministic_trends[0].rows[0];
    assert_eq!(model.name(row.name), "y");
    assert_eq!(row.expression.text, "a+b");
    assert!(row.expression.expr.is_some());
}

#[test]
fn accepted_expression_neighbours() {
    let binary = pinned_binary();
    quiet_file("quiet_growth.mod", binary.as_deref());
    let repeated = quiet_file("quiet_repeated_options.mod", binary.as_deref());
    assert!(!analyze(&repeated).iter().any(|diag| diag.code == "E271"));
    assert_eq!(repeated.semi_structural_commands[1].options.len(), 7);
    assert_eq!(repeated.semi_structural_commands[2].options.len(), 6);
}

#[test]
fn bison_expression_and_row_fire_files() {
    let binary = pinned_binary();
    for (name, message, offending) in [
        (
            "e001_pac_growth_binary.mod",
            "syntax error, unexpected ')'",
            ")",
        ),
        (
            "e001_vem_expression_string.mod",
            "syntax error, unexpected QUOTED_STRING",
            "'x'",
        ),
        (
            "e001_vem_discount_string.mod",
            "syntax error, unexpected QUOTED_STRING",
            "'beta'",
        ),
        (
            "e001_target_string.mod",
            "syntax error, unexpected QUOTED_STRING",
            "'x'",
        ),
        (
            "e001_component_string.mod",
            "syntax error, unexpected QUOTED_STRING",
            "'x'",
        ),
        (
            "e001_component_growth_string.mod",
            "syntax error, unexpected QUOTED_STRING",
            "'x'",
        ),
        (
            "e001_trends_string.mod",
            "syntax error, unexpected QUOTED_STRING",
            "'beta'",
        ),
        (
            "e001_component_final_semi.mod",
            "syntax error, unexpected END, expecting ';'",
            "end",
        ),
        (
            "e001_trends_final_semi.mod",
            "syntax error, unexpected END, expecting ';'",
            "end",
        ),
        ("e001_empty_growth.mod", "syntax error, unexpected ')'", ")"),
        (
            "e001_eqtags_unquoted.mod",
            "syntax error, unexpected IDENTIFIER, expecting COMMA or QUOTED_STRING",
            "eqx",
        ),
        (
            "e001_horizon_float.mod",
            "syntax error, unexpected FLOAT_NUMBER, expecting INT_NUMBER",
            "1.5",
        ),
        (
            "e001_pac_kind_bad.mod",
            "syntax error, unexpected IDENTIFIER, expecting LL or DL or DD",
            "other",
        ),
        (
            "e001_named_operator_number.mod",
            "syntax error, unexpected INT_NUMBER",
            "1",
        ),
    ] {
        let source = fixture(name);
        let diagnostics = analyze(&parse(&source));
        let ours = diagnostics
            .iter()
            .find(|diag| diag.code == "E001")
            .unwrap_or_else(|| panic!("missing E001 in {name}: {diagnostics:?}"));
        assert_eq!(ours.message, message, "{name}");
        assert_eq!(
            &source[ours.span.start as usize..ours.span.end as usize],
            offending,
            "{name}"
        );
        if let Some(ref binary) = binary {
            let (accepted, report) = official_check(&source, binary);
            assert!(!accepted, "7.2 accepted {name}");
            assert!(report.contains(message), "{name}: {report}");
        }
    }
}

#[test]
fn deterministic_duplicate_is_scoped() {
    let binary = pinned_binary();
    let source = fixture("e261_deterministic_duplicate.mod");
    let diagnostics = analyze(&parse(&source));
    let duplicate = diagnostics
        .iter()
        .find(|diag| diag.code == "E261")
        .unwrap_or_else(|| panic!("missing E261: {diagnostics:?}"));
    assert_eq!(duplicate.message, "observation_trends: x declared twice");
    assert_eq!(
        &source[duplicate.span.start as usize..duplicate.span.end as usize],
        "x"
    );
    if let Some(ref binary) = binary {
        let (accepted, report) = official_check(&source, binary);
        assert!(!accepted);
        assert!(report.contains(&duplicate.message));
    }
    let quiet = quiet_file("quiet_deterministic_two_blocks.mod", binary.as_deref());
    assert!(!analyze(&quiet).iter().any(|diag| diag.code == "E261"));
}

#[test]
fn pinned_pac_example_keeps_written_equations() {
    let path = Path::new("C:/dynare/7.2/examples/semistructural/pac_model.mod");
    let Ok(source) = std::fs::read_to_string(path) else {
        return;
    };
    let model = parse(&source);
    assert!(!analyze(&model).iter().any(|diag| diag.code == "E001"));
    assert_eq!(model.equations.len(), 4);
    assert_eq!(model.summary().n_model_equations, 4);
    assert_eq!(model.semi_structural_commands.len(), 2);
    assert_eq!(model.pac_target_info[0].rows.len(), 4);
    assert_eq!(model.named_model_operators.len(), 2);
}
