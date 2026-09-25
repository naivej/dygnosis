use std::path::{Path, PathBuf};
use std::time::Duration;

use dygnosis::{
    analyze, dynare_diagnose, find_preprocessor, parse, run_preprocessor, JsonStage, Severity,
};
use tower_lsp::lsp_types::NumberOrString;

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/d_pac")
        .join(name);
    let source = std::fs::read_to_string(&path).unwrap();
    assert!(source.starts_with("// inventory: "), "{}", path.display());
    source
}

fn pinned_binary() -> Option<PathBuf> {
    let pinned = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    if pinned.is_file() {
        return Some(pinned);
    }
    find_preprocessor(None).filter(|path| {
        path.components()
            .any(|part| part.as_os_str().to_string_lossy() == "7.2")
    })
}

struct Fire {
    file: &'static str,
    code: &'static str,
    message: &'static str,
    stage: JsonStage,
    span: &'static str,
}

const FIRES: &[Fire] = &[
    Fire { file: "e432_log_var.mod", code: "E432", message: "the following variables are declared with var(log) and therefore cannot appear in a VAR/TCM/PAC equation: y ", stage: JsonStage::Transform, span: "y" },
    Fire { file: "e432_nested_local_second_selected.mod", code: "E432", message: "the following variables are declared with var(log) and therefore cannot appear in a VAR/TCM/PAC equation: w ", stage: JsonStage::Transform, span: "second" },
    Fire { file: "e433_missing_tag.mod", code: "E433", message: "looking for equation tag Missing failed.", stage: JsonStage::Transform, span: "'Missing'" },
    Fire { file: "e433_missing_target.mod", code: "E433", message: "no equation is named 'Missing'", stage: JsonStage::Transform, span: "'Missing'" },
    Fire { file: "e433_after_remove.mod", code: "E433", message: "looking for equation tag Y failed.", stage: JsonStage::Transform, span: "'Y'" },
    Fire { file: "e434_var_lead.mod", code: "E434", message: "in Equation Y. A VAR model may not have leaded endogenous variables on the RHS. ", stage: JsonStage::Transform, span: "y" },
    Fire { file: "e434_nested_local_lead.mod", code: "E434", message: "in Equation Y. A VAR model may not have leaded endogenous variables on the RHS. ", stage: JsonStage::Transform, span: "ahead2" },
    Fire { file: "e434_tcm_current.mod", code: "E434", message: "in Equation Y. A trend component model may not have leaded or contemporaneous endogenous variables on the RHS. ", stage: JsonStage::Transform, span: "z" },
    Fire { file: "e434_var_lhs_type.mod", code: "E434", message: "in Equation Y. A VAR may only have one endogenous variable on the LHS. ", stage: JsonStage::Transform, span: "[name='Y']" },
    Fire { file: "e434_var_lhs_two.mod", code: "E434", message: "in Equation Y. A VAR may only have one endogenous variable on the LHS. ", stage: JsonStage::Transform, span: "y+z" },
    Fire { file: "e434_var_lhs_timing.mod", code: "E434", message: "in Equation Y. The variable on the LHS of a VAR may not appear with a lead or a lag. ", stage: JsonStage::Transform, span: "y(+1)" },
    Fire { file: "e434_var_rhs_current.mod", code: "E434", message: "in Equation Y. A non-structural VAR model may not have contemporaneous endogenous variables on the RHS. ", stage: JsonStage::Transform, span: "z" },
    Fire { file: "e434_var_rhs_exo_lag.mod", code: "E434", message: "in Equation Y. A VAR model may not have lagged or leaded exogenous variables on the RHS. ", stage: JsonStage::Transform, span: "e" },
    Fire { file: "e434_var_binary_lhs.mod", code: "E434", message: "you can only have variables or unary ops on LHS of VAR", stage: JsonStage::Transform, span: "2*y" },
    Fire { file: "e434_var_duplicate_lhs.mod", code: "E434", message: "The LHS variables of the VAR model are not unique", stage: JsonStage::Transform, span: "[name='Z']" },
    Fire { file: "e434_unary_duplicate_lhs.mod", code: "E434", message: "The LHS variables of the VAR model are not unique", stage: JsonStage::Transform, span: "[name='Z']" },
    Fire { file: "e434_tcm_lhs_type.mod", code: "E434", message: "in Equation Y. A trend component model may only have one endogenous variable on the LHS. ", stage: JsonStage::Transform, span: "[name='Y']" },
    Fire { file: "e434_tcm_lhs_timing.mod", code: "E434", message: "in Equation Y. The variable on the LHS of a trend component model may not appear with a lead or a lag. ", stage: JsonStage::Transform, span: "y(+1)" },
    Fire { file: "e434_tcm_exo_lag.mod", code: "E434", message: "in Equation Y. A trend component model may not have lagged or leaded exogenous variables on the RHS. ", stage: JsonStage::Transform, span: "e" },
    Fire { file: "e434_tcm_binary_lhs.mod", code: "E434", message: "you can only have variables or unary ops on LHS of VAR", stage: JsonStage::Transform, span: "2*y" },
    Fire { file: "e434_tcm_duplicate_lhs.mod", code: "E434", message: "The LHS variables of the trend component model are not unique", stage: JsonStage::Transform, span: "[name='Z']" },
    Fire { file: "e435_vem_aux.mod", code: "E435", message: "var_expectation_model a refers to nonexistent auxiliary model missing", stage: JsonStage::Transform, span: "missing" },
    Fire { file: "e436_vem_nonlinear.mod", code: "E436", message: "expression in var_expectation_model a is not of the expected form: More than one variable in this expression", stage: JsonStage::Transform, span: "y*y" },
    Fire { file: "e436_deterministic_exo_variable.mod", code: "E436", message: "expression in var_expectation_model a is not of the expected form: Symbol d not allowed here", stage: JsonStage::Transform, span: "d" },
    Fire { file: "e436_deterministic_exo_expression.mod", code: "E436", message: "expression in var_expectation_model a is not of the expected form: Symbol d not allowed here", stage: JsonStage::Transform, span: "d" },
    Fire { file: "e437_pac_growth_info.mod", code: "E437", message: "for PAC model 'q', it is not possible to declare a 'growth' option in the 'pac_model' command when there is also a 'pac_target_info' block", stage: JsonStage::Check, span: "growth" },
    Fire { file: "e437_pac_auxname_info.mod", code: "E437", message: "for PAC model 'q', it is not possible to declare an 'auxname' option in the 'pac_model' command when there is also a 'pac_target_info' block", stage: JsonStage::Check, span: "auxname" },
    Fire { file: "e437_pac_kind_info.mod", code: "E437", message: "for PAC model 'q', it is not possible to declare a 'kind' option in the 'pac_model' command when there is also a 'pac_target_info' block", stage: JsonStage::Check, span: "kind" },
    Fire { file: "e437_pac_kind_mce.mod", code: "E437", message: "for PAC model 'q', it is not possible to declare a 'kind' option in the 'pac_model' command since this is a MCE model", stage: JsonStage::Check, span: "kind" },
    Fire { file: "e438_pac_missing_kind.mod", code: "E438", message: "the block 'pac_target_info(q)' is missing the 'kind' statement in some 'component'", stage: JsonStage::Check, span: "component" },
    Fire { file: "e438_pac_missing_target.mod", code: "E438", message: "the block 'pac_target_info(q)' is missing the 'target' statement", stage: JsonStage::Check, span: "pac_target_info" },
    Fire { file: "e438_pac_missing_nonstat_aux.mod", code: "E438", message: "the block 'pac_target_info(q)' is missing the 'auxname_target_nonstationary' statement", stage: JsonStage::Check, span: "pac_target_info" },
    Fire { file: "e438_pac_missing_auxname.mod", code: "E438", message: "the block 'pac_target_info(q)' is missing the 'auxname' statement in some 'component'", stage: JsonStage::Check, span: "component" },
    Fire { file: "e438_pac_stationary_growth.mod", code: "E438", message: "in the block 'pac_target_info(q)', a component of 'kind ll' (i.e. stationary) has a 'growth' option. This is not permitted.", stage: JsonStage::Check, span: "component" },
    Fire { file: "e438_pac_no_nonstat.mod", code: "E438", message: "the block 'pac_target_info(q)' must contain at least one nonstationary component (i.e. of 'kind' equal to either 'dd' or 'dl').", stage: JsonStage::Check, span: "pac_target_info" },
    Fire { file: "e439_var_required.mod", code: "E439", message: "You must pass the 'eqtags' option to the 'var_model' statement.", stage: JsonStage::Check, span: "var_model" },
    Fire { file: "e439_tcm_required.mod", code: "E439", message: "You must pass the 'targets' option to the 'trend_component_model' statement.", stage: JsonStage::Check, span: "trend_component_model" },
    Fire { file: "e439_pac_required.mod", code: "E439", message: "You must pass the 'discount' option to the 'pac_model' statement.", stage: JsonStage::Check, span: "pac_model" },
    Fire { file: "e439_vem_required.mod", code: "E439", message: "You must pass the 'horizon' option to the 'var_expectation_model' statement.", stage: JsonStage::Check, span: "var_expectation_model" },
    Fire { file: "e440_vem_duplicate.mod", code: "E440", message: "a var_expectation_model already exists with the name a", stage: JsonStage::Check, span: "a" },
    Fire { file: "e440_var_duplicate.mod", code: "E440", message: "a VAR model already exists with the name v", stage: JsonStage::Check, span: "v" },
    Fire { file: "e440_tcm_duplicate.mod", code: "E440", message: "a trend component model already exists with the name t", stage: JsonStage::Check, span: "t" },
    Fire { file: "e440_pac_duplicate.mod", code: "E440", message: "a PAC model already exists with the name q", stage: JsonStage::Check, span: "q" },
    Fire { file: "e441_vem_both.mod", code: "E441", message: "You can't pass both the 'variable' or the 'expression' options to the var_expectation_model statement.", stage: JsonStage::Check, span: "var_expectation_model" },
    Fire { file: "e441_vem_neither.mod", code: "E441", message: "You must pass either the 'variable' or the 'expression' option to the var_expectation_model statement.", stage: JsonStage::Check, span: "var_expectation_model" },
    Fire { file: "e442_vem_discount.mod", code: "E442", message: "The discount factor must be a constant expression or a parameter", stage: JsonStage::Check, span: "y" },
    Fire { file: "e443_vem_shift.mod", code: "E443", message: "The 'time_shift' option must be a non-positive integer", stage: JsonStage::Check, span: "1" },
    Fire { file: "e444_pac_discount.mod", code: "E444", message: "y is not a parameter", stage: JsonStage::Check, span: "y" },
    Fire { file: "e445_epilogue_pac.mod", code: "E445", message: "The 'pac_expectation' operator is forbidden in 'epilogue'.", stage: JsonStage::Check, span: "pac_expectation" },
    Fire { file: "e445_epilogue_var.mod", code: "E445", message: "The 'var_expectation' operator is forbidden in 'epilogue'.", stage: JsonStage::Check, span: "var_expectation" },
    Fire { file: "e445_epilogue_target.mod", code: "E445", message: "The 'pac_target_nonstationary' operator is forbidden in 'epilogue'.", stage: JsonStage::Check, span: "pac_target_nonstationary" },
    Fire { file: "e446_pac_aux.mod", code: "E446", message: "aux_model_name not recognized as VAR model or Trend Component model", stage: JsonStage::Transform, span: "missing" },
    Fire { file: "e447_var_operator.mod", code: "E447", message: "unknown model 'nope' used in var_expectation expression", stage: JsonStage::Transform, span: "nope" },
    Fire { file: "e447_used_local_operator.mod", code: "E447", message: "unknown model 'nope' used in var_expectation expression", stage: JsonStage::Transform, span: "forecast" },
    Fire { file: "e448_growth_product.mod", code: "E448", message: "PAC growth must be a linear combination of variables", stage: JsonStage::Transform, span: "x*x" },
    Fire { file: "e449_no_pac_use.mod", code: "E449", message: "the model does not contain the 'pac_expectation(p)' operator.", stage: JsonStage::Transform, span: "p" },
    Fire { file: "e450_multiple_pac.mod", code: "E450", message: "It is not possible to use 'pac_expectation(p)' in several equations.", stage: JsonStage::Transform, span: "p" },
    Fire { file: "e451_unknown_pac.mod", code: "E451", message: "in equation Y, the pac_expectation operator references an unknown pac_model", stage: JsonStage::Transform, span: "nope" },
    Fire { file: "e451_untagged_lhs.mod", code: "E451", message: "in equation y, the pac_expectation operator references an unknown pac_model", stage: JsonStage::Transform, span: "nope" },
    Fire { file: "e451_used_local.mod", code: "E451", message: "in equation Y, the pac_expectation operator references an unknown pac_model", stage: JsonStage::Transform, span: "pterm" },
    Fire { file: "e452_no_target.mod", code: "E452", message: "in equation Y, the pac_target_nonstationary operator does not match a corresponding 'pac_target_info' block", stage: JsonStage::Transform, span: "nope" },
    Fire { file: "e452_target_without_pac_model.mod", code: "E452", message: "in equation X, the pac_target_nonstationary operator does not match a corresponding 'pac_target_info' block", stage: JsonStage::Transform, span: "p" },
    Fire { file: "e452_untagged_lhs.mod", code: "E452", message: "in equation y, the pac_target_nonstationary operator does not match a corresponding 'pac_target_info' block", stage: JsonStage::Transform, span: "nope" },
    Fire { file: "e453_growth_name_clash.mod", code: "E453", message: "The variable/parameter 'p_pac_growth_neutrality_correction' conflicts with the auxiliary parameter that will be generated for the growth neutrality correction of the 'p' PAC model. Please rename that parameter.", stage: JsonStage::Transform, span: "p_pac_growth_neutrality_correction" },
    Fire { file: "e454_mce_alpha_clash.mod", code: "E454", message: "The variable/parameter 'mce_alpha_p_1' conflicts with a parameter that will be generated for the 'p' PAC model. Please rename it.", stage: JsonStage::Transform, span: "mce_alpha_p_1" },
    Fire { file: "e455_backward_constant_clash.mod", code: "E455", message: "the variable/parameter 'h_p_constant' conflicts with some auxiliary parameter that will be generated for the 'p' PAC model. Please rename that parameter.", stage: JsonStage::Transform, span: "h_p_constant" },
    Fire { file: "e455_component_growth_clash.mod", code: "E455", message: "the variable/parameter 'p_component2_pac_growth_neutrality_correction' conflicts with some auxiliary parameter that will be generated for the 'p' PAC model. Please rename that parameter.", stage: JsonStage::Transform, span: "p_component2_pac_growth_neutrality_correction" },
    Fire { file: "e456_component_aux_clash.mod", code: "E456", message: "the variable/parameter 'y_part' conflicts with a variable that will be generated for a 'pac_expectation' expression. Please rename it.", stage: JsonStage::Transform, span: "y_part" },
    Fire { file: "e456_reused_component.mod", code: "E456", message: "the variable/parameter 'y_part' conflicts with a variable that will be generated for a 'pac_expectation' expression. Please rename it.", stage: JsonStage::Transform, span: "y_part" },
    Fire { file: "e456_target_component.mod", code: "E456", message: "the variable/parameter 'v_ns' conflicts with a variable that will be generated for a 'pac_expectation' expression. Please rename it.", stage: JsonStage::Transform, span: "v_ns" },
    Fire { file: "e456_cross_model.mod", code: "E456", message: "the variable/parameter 'y_part' conflicts with a variable that will be generated for a 'pac_expectation' expression. Please rename it.", stage: JsonStage::Transform, span: "y_part" },
    Fire { file: "e457_target_aux_clash.mod", code: "E457", message: "the variable/parameter 'v_ns' conflicts with a variable that will be generated for a 'pac_target_nonstationary' expression. Please rename it.", stage: JsonStage::Transform, span: "v_ns" },
    Fire { file: "e458_target_product.mod", code: "E458", message: "there is no equation whose LHS is equal to the 'target' of 'pac_target_info(p)'", stage: JsonStage::Transform, span: "v*v" },
    Fire { file: "w206_deterministic_param.mod", code: "W206", message: "Warning: Non-variable symbol used in deterministic_trends: p", stage: JsonStage::Check, span: "p" },
    Fire { file: "e021_vem_expression.mod", code: "E021", message: "ghost not used in model block. To bypass this error, use the `nostrict` option. This may lead to crashes or unexpected behavior.", stage: JsonStage::Check, span: "ghost" },
    Fire { file: "e021_pac_target.mod", code: "E021", message: "ghost not used in model block. To bypass this error, use the `nostrict` option. This may lead to crashes or unexpected behavior.", stage: JsonStage::Check, span: "ghost" },
    Fire { file: "e021_pac_component.mod", code: "E021", message: "ghost not used in model block. To bypass this error, use the `nostrict` option. This may lead to crashes or unexpected behavior.", stage: JsonStage::Check, span: "ghost" },
    Fire { file: "e021_target_two_names.mod", code: "E021", message: "ghost1 ghost2 not used in model block. To bypass this error, use the `nostrict` option. This may lead to crashes or unexpected behavior.", stage: JsonStage::Check, span: "ghost1" },
    Fire { file: "e058_pac_discount.mod", code: "E058", message: "Unknown symbol: ghost.", stage: JsonStage::Check, span: "ghost" },
    Fire { file: "e058_deterministic_name.mod", code: "E058", message: "Unknown symbol: ghost.", stage: JsonStage::Check, span: "ghost" },
    Fire { file: "e271_var_option.mod", code: "E271", message: "option var.model_name declared twice", stage: JsonStage::Check, span: "model_name" },
];

#[test]
fn official_fire_table() {
    let binary = pinned_binary();
    for fire in FIRES {
        let source = fixture(fire.file);
        let diagnostics = analyze(&parse(&source));
        let ours = diagnostics
            .iter()
            .find(|diag| diag.code == fire.code)
            .unwrap_or_else(|| panic!("{} missing {}: {diagnostics:?}", fire.file, fire.code));
        assert_eq!(ours.message, fire.message, "{}", fire.file);
        if fire.code.starts_with('E') {
            assert_eq!(
                diagnostics
                    .iter()
                    .filter(|diag| diag.severity == Severity::Error)
                    .count(),
                1,
                "{} must lock one refusal: {diagnostics:?}",
                fire.file
            );
        }
        assert_eq!(
            ours.severity,
            if fire.code.starts_with('W') {
                Severity::Warning
            } else {
                Severity::Error
            },
            "{}",
            fire.file
        );
        assert!(
            source[ours.span.start as usize..ours.span.end as usize].contains(fire.span),
            "{} span: {:?}",
            fire.file,
            &source[ours.span.start as usize..ours.span.end as usize]
        );
        if let Some(binary) = binary.as_deref() {
            if fire.file == "e436_deterministic_exo_expression.mod" {
                let check = run_preprocessor(
                    &source,
                    binary,
                    None,
                    Duration::from_secs(30),
                    JsonStage::Check,
                );
                assert!(
                    check.success,
                    "7.2 refused direct expression at check: {} {}",
                    check.raw_stdout, check.raw_stderr
                );
            }
            let result =
                run_preprocessor(&source, binary, None, Duration::from_secs(30), fire.stage);
            let report = format!("{} {}", result.raw_stdout, result.raw_stderr);
            if fire.code.starts_with('W') {
                assert!(result.success, "{} official refused: {report}", fire.file);
            } else {
                assert!(!result.success, "{} official accepted", fire.file);
            }
            assert!(report.contains(fire.message), "{}: {report}", fire.file);
        }
    }
}

#[test]
fn accepted_neighbours() {
    let binary = pinned_binary();
    for (file, stage, codes) in [
        (
            "quiet_var.mod",
            JsonStage::Transform,
            &["E432", "E433", "E434"][..],
        ),
        ("quiet_tcm.mod", JsonStage::Transform, &["E433", "E434"]),
        ("quiet_vem.mod", JsonStage::Transform, &["E435", "E436"]),
        (
            "quiet_vem_expression_endogenous.mod",
            JsonStage::Transform,
            &["E436"],
        ),
        ("quiet_pac_check.mod", JsonStage::Check, &["E437", "E438"]),
        ("quiet_deterministic.mod", JsonStage::Check, &["W206"]),
        ("quiet_var_structural.mod", JsonStage::Transform, &["E434"]),
        ("quiet_after_replace.mod", JsonStage::Transform, &["E433"]),
        (
            "quiet_nested_local_lag.mod",
            JsonStage::Transform,
            &["E434"],
        ),
        (
            "quiet_unused_local_operator.mod",
            JsonStage::Transform,
            &["E447"],
        ),
        (
            "quiet_pac_target_model_local.mod",
            JsonStage::Check,
            &["E021"],
        ),
        (
            "quiet_pac_target_component_growth.mod",
            JsonStage::Check,
            &["E021"],
        ),
        (
            "quiet_pac_growth_unknown.mod",
            JsonStage::Check,
            &["E020", "E021"],
        ),
        (
            "quiet_pac_component_growth_unknown.mod",
            JsonStage::Check,
            &["E020", "E021"],
        ),
        ("quiet_growth.mod", JsonStage::Transform, &["E448", "E453"]),
        ("quiet_no_pac_model.mod", JsonStage::Transform, &["E449"]),
        (
            "quiet_single_pac.mod",
            JsonStage::Transform,
            &["E449", "E450", "E451", "E452", "E455", "E457", "E458"],
        ),
        (
            "quiet_same_equation_twice.mod",
            JsonStage::Transform,
            &["E450"],
        ),
        (
            "quiet_no_unknown_pac.mod",
            JsonStage::Transform,
            &["E451", "E452"],
        ),
        (
            "quiet_untagged_lhs.mod",
            JsonStage::Transform,
            &["E451", "E452"],
        ),
        (
            "quiet_unused_local_pac.mod",
            JsonStage::Transform,
            &["E451"],
        ),
        ("quiet_mce.mod", JsonStage::Transform, &["E454", "E456"]),
        (
            "quiet_cross_model_unique.mod",
            JsonStage::Transform,
            &["E456"],
        ),
        (
            "quiet_backward_no_growth.mod",
            JsonStage::Transform,
            &["E455"],
        ),
    ] {
        let source = fixture(file);
        let diagnostics = analyze(&parse(&source));
        for code in codes {
            assert!(
                !diagnostics.iter().any(|diag| &diag.code == code),
                "{file}: {diagnostics:?}"
            );
        }
        if let Some(binary) = binary.as_deref() {
            let result = run_preprocessor(&source, binary, None, Duration::from_secs(30), stage);
            assert!(
                result.success,
                "{}: {} {}",
                file, result.raw_stdout, result.raw_stderr
            );
        }
    }
    // Dynare throws an exception without an ERROR line on this shape. The
    // parser owns it, but neither stage may claim a refusal.
    let source = fixture("quiet_vem_variable_missing.mod");
    assert!(!analyze(&parse(&source))
        .iter()
        .any(|diag| diag.severity == Severity::Error));
}

#[test]
fn parse_refusal_preempts_pac_check() {
    let source = fixture("e020_preempts_pac_check.mod");
    let diagnostics = analyze(&parse(&source));
    assert!(diagnostics.iter().any(|diag| diag.code == "E020"));
    assert!(!diagnostics.iter().any(|diag| diag.code == "E438"));
    if let Some(binary) = pinned_binary() {
        let result = run_preprocessor(
            &source,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        let report = format!("{} {}", result.raw_stdout, result.raw_stderr);
        assert!(!result.success);
        assert!(report.contains("Unknown symbol: ghost"), "{report}");
        assert!(
            !report.contains("missing the 'target' statement"),
            "{report}"
        );
    }
}

#[test]
fn rewrite_rows_stay_silent_without_mapping() {
    let binary = pinned_binary();
    let file = "rewrite_s012_tcm_nondiff.mod";
    let text = "does not have the diff operator applied to it yet you are trying to undiff it.";
    {
        let source = fixture(file);
        let diagnostics = analyze(&parse(&source));
        assert!(
            !diagnostics
                .iter()
                .any(|diag| diag.severity == Severity::Error),
            "{file}: {diagnostics:?}"
        );
        if let Some(binary) = binary.as_deref() {
            let check = run_preprocessor(
                &source,
                binary,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            assert!(
                check.success,
                "{file}: {} {}",
                check.raw_stdout, check.raw_stderr
            );
            let transform = run_preprocessor(
                &source,
                binary,
                None,
                Duration::from_secs(30),
                JsonStage::Transform,
            );
            let report = format!("{} {}", transform.raw_stdout, transform.raw_stderr);
            assert!(!transform.success, "{file} accepted transform");
            assert!(report.contains(text), "{file}: {report}");
        }
    }
}

#[test]
fn new_written_pac_errors_reach_both_transports() {
    for (file, code) in [
        ("e448_growth_product.mod", "E448"),
        ("e451_unknown_pac.mod", "E451"),
    ] {
        let source = fixture(file);
        let expected = analyze(&parse(&source))
            .into_iter()
            .find(|diag| diag.code == code)
            .expect("core PAC error");
        let lsp = dygnosis::server::diagnostics_for("file:///pac.mod", &source);
        let lsp_error = lsp
            .iter()
            .find(|diag| matches!(&diag.code, Some(NumberOrString::String(value)) if value == code))
            .expect("LSP PAC error");
        let mcp = dynare_diagnose(&source, None, None);
        let mcp_error = mcp
            .iter()
            .find(|diag| diag.code == code)
            .expect("MCP PAC error");
        assert_eq!(lsp_error.message, expected.message, "{file}");
        assert_eq!(mcp_error.message, expected.message, "{file}");
        assert_eq!(mcp_error.severity, "ERROR", "{file}");
    }
}

#[test]
fn generated_pac_auxiliary_reuse_points_to_second_written_name() {
    for (file, name) in [
        ("e456_reused_component.mod", "y_part"),
        ("e456_target_component.mod", "v_ns"),
        ("e456_cross_model.mod", "y_part"),
    ] {
        let source = fixture(file);
        let error = analyze(&parse(&source))
            .into_iter()
            .find(|diag| diag.code == "E456")
            .expect("generated PAC auxiliary clash");
        let expected_start = source
            .rfind(&format!("auxname {name};"))
            .expect("second auxname")
            + "auxname ".len();
        assert_eq!(error.span.start as usize, expected_start, "{file}");
        assert_eq!(
            error.span.end as usize,
            expected_start + name.len(),
            "{file}"
        );
        if let Some(binary) = pinned_binary() {
            let check = run_preprocessor(
                &source,
                &binary,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            assert!(
                check.success,
                "{file}: {} {}",
                check.raw_stdout, check.raw_stderr
            );
        }
    }
}

#[test]
fn pac_models_run_transform_in_sorted_name_order() {
    let source = fixture("precedence_target_before_growth.mod");
    let diagnostics = analyze(&parse(&source));
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diag| diag.severity == Severity::Error)
            .count(),
        1
    );
    assert!(diagnostics.iter().any(|diag| diag.code == "E458"));
    assert!(!diagnostics.iter().any(|diag| diag.code == "E448"));
    if let Some(binary) = pinned_binary() {
        let check = run_preprocessor(
            &source,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        assert!(check.success, "{} {}", check.raw_stdout, check.raw_stderr);
        let transform = run_preprocessor(
            &source,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Transform,
        );
        let report = format!("{} {}", transform.raw_stdout, transform.raw_stderr);
        assert!(!transform.success);
        assert!(
            report.contains(
                "there is no equation whose LHS is equal to the 'target' of 'pac_target_info(p)'"
            ),
            "{report}"
        );
        assert!(
            !report.contains("PAC growth must be a linear combination"),
            "{report}"
        );
    }
}
