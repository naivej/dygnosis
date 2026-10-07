use dygnosis::{analyze, parse};

fn selected(source: &str, code: &str) -> Vec<String> {
    let model = parse(source);
    analyze(&model)
        .iter()
        .filter(|diagnostic| diagnostic.code == code)
        .map(|diagnostic| {
            model.source[diagnostic.span.start as usize..diagnostic.span.end as usize].to_string()
        })
        .collect()
}

#[test]
fn representative_summaries_keep_their_count_and_full_parser_spans() {
    let source = "/* 😀中 */var x y; model; x = y(-1); end; steady_state_model; x = 0; end;";
    assert_eq!(selected(source, "I050"), Vec::<String>::new());
    assert_eq!(
        selected("var x y; model; #p = 1; x = y(-1); end;", "W013"),
        ["model"]
    );
    assert_eq!(selected(source, "W042"), ["steady_state_model"]);
    let model = parse(source);
    let full = model
        .statements
        .iter()
        .find(|statement| statement.name == "model")
        .unwrap();
    assert_eq!(
        &model.source[full.span.start as usize..full.span.end as usize],
        "model; x = y(-1); end;"
    );
    assert_eq!(selected("var x; model; x = x(-1); end;", "I050"), ["model"]);
}

fn assert_fixture(
    file: &str,
    code: &str,
    keyword: &str,
    occurrence: usize,
    expected: &[(u8, &str)],
) {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(file),
    )
    .unwrap();
    let model = parse(&format!("/* 😀中 */\n{source}"));
    let owner = model
        .statements
        .iter()
        .filter(|statement| statement.name == keyword)
        .nth(occurrence)
        .unwrap();
    let diagnostics = analyze(&model);
    let hits: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == code)
        .collect();
    assert_eq!(hits.len(), expected.len(), "{file} {code}");
    for (diagnostic, &(severity, message)) in hits.iter().zip(expected) {
        assert_eq!(diagnostic.span, owner.keyword_span, "{file} {code}");
        assert_eq!(diagnostic.severity as u8, severity, "{file} {code}");
        assert_eq!(diagnostic.message, message, "{file} {code}");
        assert_eq!(
            &model.source[diagnostic.span.start as usize..diagnostic.span.end as usize],
            keyword,
            "{file} {code}"
        );
    }
}

#[test]
fn whole_construct_matrix_keeps_baseline_messages_severity_and_multiplicity() {
    assert_fixture("shape/i050_none.mod", "I050", "model", 0, &[(3, "No initval or steady_state_model block. Add an initval block with initial guesses, or a steady_state_model block with closed-form assignments.")]);
    assert_fixture(
        "shape/w042_missing.mod",
        "W042",
        "steady_state_model",
        0,
        &[(2, "variable 'c' is not assigned a value")],
    );
    assert_fixture(
        "shape/w052_partial.mod",
        "W052",
        "initval",
        0,
        &[(
            2,
            "1 endogenous variable(s) missing from initval (will default to 0): c",
        )],
    );
    assert_fixture("d_ms/e058_cfp_var_undeclared.mod", "W092", "varobs", 0, &[(2, "3 observed variable(s) but only 1 shock source(s) found (structural shocks plus measurement errors). Review the observed variables and shock sources before estimation.")]);
    assert_fixture(
        "compare/groups_after.mod",
        "E188",
        "model",
        0,
        &[(1, "There are 4 equations but 2 endogenous variables!")],
    );
    assert_fixture("clash/e027_ramsey_varexo_det.mod", "W013", "model", 0, &[(2, "Equation count mismatch: 1 equation(s) but 1 endogenous variable(s). ramsey_model with 1 instrument(s) expects delta = -1.")]);
    assert_fixture("compare/het_rewrite_before.mod", "E192", "model", 1, &[(1, "There are 1 equations but 2 endogenous variables in the model for heterogeneity dimension 'h'!")]);
    assert_fixture("compare/het_repeat_after.mod", "W208", "model", 1, &[(2, "Equation count mismatch: 2 equation(s) but 1 endogenous variable(s) in heterogeneity dimension 'h'.")]);
    assert_fixture("w100/w100_planner.mod", "E100", "planner_objective", 0, &[(1, "A planner_objective statement must be used with a ramsey_model, a ramsey_policy, osr, or a discretionary_policy statement and vice versa.")]);
    assert_fixture(
        "clash/e104_two_planner.mod",
        "E104",
        "planner_objective",
        1,
        &[(1, "there can only be one planner_objective statement")],
    );
    assert_fixture(
        "occbin/e170_two_blocks.mod",
        "E170",
        "occbin_constraints",
        1,
        &[(1, "Multiple 'occbin_constraints' blocks are not allowed")],
    );
    assert_fixture("w100/e203_ramsey_constraints.mod", "E203", "ramsey_constraints", 0, &[(1, "A ramsey_constraints block requires the presence of a ramsey_model or ramsey_policy statement")]);
    assert_fixture("d_check/e212_estimated_shock.mod", "E212", "estimated_params", 0, &[(1, "some estimated parameters (rho) also appear in the expressions defining the variance/covariance matrix of shocks; this is not allowed.")]);
    assert_fixture(
        "d_check/e217_initval_after_endval.mod",
        "E217",
        "initval",
        0,
        &[(
            1,
            "an 'initval' block cannot appear after an 'endval' block",
        )],
    );
    assert_fixture(
        "d_check/e218_all_values.mod",
        "E218",
        "initval",
        0,
        &[(
            1,
            "You have not set the following exogenous variables in initval: e",
        )],
    );
    assert_fixture(
        "d_block/e241_histval_missing_exo.mod",
        "E241",
        "histval",
        0,
        &[(
            1,
            "You have not set the following exogenous variables in endval: e",
        )],
    );
    assert_fixture(
        "d_block/e254_osr_bounds_before.mod",
        "E254",
        "osr_params_bounds",
        0,
        &[(
            1,
            "you must have an osr_params statement before the osr_params_bounds block.",
        )],
    );
    assert_fixture(
        "d_block/e258_several_varobs.mod",
        "E258",
        "varobs",
        1,
        &[(
            1,
            "varobs: you cannot have several 'varobs' statements in the same MOD file",
        )],
    );
    assert_fixture(
        "d_block/e259_several_varexobs.mod",
        "E259",
        "varexobs",
        1,
        &[(
            1,
            "varexobs: you cannot have several 'varexobs' statements in the same MOD file",
        )],
    );
    assert_fixture(
        "d_block/w203_osr_params_twice.mod",
        "W203",
        "osr_params",
        1,
        &[(
            2,
            "You have more than one osr_params statement in the .mod file.",
        )],
    );
    assert_fixture(
        "d_open/e297_ramsey_model_twice.mod",
        "E297",
        "ramsey_model",
        1,
        &[(
            1,
            "Several 'ramsey_model' statements cannot appear in a given .mod file.",
        )],
    );
    assert_fixture(
        "d_open/e298_ramsey_model_after_policy.mod",
        "E298",
        "ramsey_model",
        0,
        &[(
            1,
            "A 'ramsey_model' statement cannot follow a 'ramsey_policy' statement.",
        )],
    );
    assert_fixture(
        "d_open/e299_ramsey_policy_after_model.mod",
        "E299",
        "ramsey_policy",
        0,
        &[(
            1,
            "A 'ramsey_policy' statement cannot follow a 'ramsey_model' statement.",
        )],
    );
    assert_fixture(
        "d_open/e300_ramsey_policy_twice.mod",
        "E300",
        "ramsey_policy",
        1,
        &[(
            1,
            "Several 'ramsey_policy' statements cannot appear in a given .mod file.",
        )],
    );
    assert_fixture(
        "d_ms/e356_svar_identification_twice.mod",
        "E356",
        "svar_identification",
        1,
        &[(
            1,
            "You may only have one svar_identification block in your .mod file.",
        )],
    );
    assert_fixture("d_ms/e357_svar_identification_two_cholesky.mod", "E357", "svar_identification", 0, &[(1, "Within the svar_identification statement, you may only have one of upper_cholesky and lower_cholesky.")]);
    assert_fixture(
        "d_pac/e438_pac_missing_target.mod",
        "E438",
        "pac_target_info",
        0,
        &[(
            1,
            "the block 'pac_target_info(q)' is missing the 'target' statement",
        )],
    );
    assert_fixture("d_pac/e438_pac_missing_nonstat_aux.mod", "E438", "pac_target_info", 0, &[(1, "the block 'pac_target_info(q)' is missing the 'auxname_target_nonstationary' statement")]);
    assert_fixture("d_pac/e438_pac_no_nonstat.mod", "E438", "pac_target_info", 0, &[(1, "the block 'pac_target_info(q)' must contain at least one nonstationary component (i.e. of 'kind' equal to either 'dd' or 'dl').")]);
    assert_fixture(
        "d_hank/e474_optim.mod",
        "E474",
        "optim_weights",
        0,
        &[(
            1,
            "The 'optim_weights' block is not supported for heterogeneous models",
        )],
    );
    assert_fixture(
        "d_extfun/quiet_e334_no_name_option.mod",
        "E322",
        "external_function",
        0,
        &[(
            1,
            "The 'name' option must be passed to external_function().",
        )],
    );
    assert_fixture(
        "d_ms/e338_data_no_file.mod",
        "E338",
        "data",
        0,
        &[(
            1,
            "The file or series option must be passed to the data statement.",
        )],
    );
    assert_fixture("d_ms/e341_ms_estimation_missing.mod", "E341", "ms_estimation", 0, &[(1, "If you do not pass no_create_init to ms_estimation, you must pass the datafile and initial_year options.")]);
    assert_fixture(
        "d_ms/e342_conditional_forecast_no_parameter_set.mod",
        "E342",
        "conditional_forecast",
        0,
        &[(
            1,
            "You must pass the `parameter_set` option to conditional_forecast",
        )],
    );
    assert_fixture(
        "d_ms/e345_markov_switching_option_missing.mod",
        "E345",
        "markov_switching",
        0,
        &[(
            1,
            "A 'chain' option must be passed to the 'markov_switching' statement.",
        )],
    );
    assert_fixture(
        "d_ms/e363_svar_none_of_three.mod",
        "E363",
        "svar",
        0,
        &[(
            1,
            "You must pass one of 'coefficients', 'variances', or 'constants'.",
        )],
    );
    assert_fixture(
        "d_ms/e365_svar_chain_missing.mod",
        "E365",
        "svar",
        0,
        &[(
            1,
            "A 'chain' option must be passed to the 'svar' statement.",
        )],
    );
    assert_fixture("mom/mom_no_method.mod", "E382", "method_of_moments", 0, &[(1, "The 'method_of_moments' statement requires a method to be supplied via the 'mom_method' option. Possible values are 'GMM', 'SMM', or 'IRF_MATCHING'.")]);
    assert_fixture("mom/mom_gmm_no_datafile.mod", "E383", "method_of_moments", 0, &[(1, "The 'method_of_moments' statement requires a data file to be supplied via the 'datafile' option.")]);
    assert_fixture(
        "d_pac/e439_var_required.mod",
        "E439",
        "var_model",
        0,
        &[(
            1,
            "You must pass the 'eqtags' option to the 'var_model' statement.",
        )],
    );
    assert_fixture(
        "d_pac/e439_tcm_required.mod",
        "E439",
        "trend_component_model",
        0,
        &[(
            1,
            "You must pass the 'targets' option to the 'trend_component_model' statement.",
        )],
    );
    assert_fixture(
        "d_pac/e439_vem_required.mod",
        "E439",
        "var_expectation_model",
        0,
        &[(
            1,
            "You must pass the 'horizon' option to the 'var_expectation_model' statement.",
        )],
    );
    assert_fixture(
        "d_pac/e439_pac_required.mod",
        "E439",
        "pac_model",
        0,
        &[(
            1,
            "You must pass the 'discount' option to the 'pac_model' statement.",
        )],
    );
    assert_fixture("d_pac/e441_vem_neither.mod", "E441", "var_expectation_model", 0, &[(1, "You must pass either the 'variable' or the 'expression' option to the var_expectation_model statement.")]);
}

#[test]
fn empty_moment_blocks_move_only_the_empty_body_branch() {
    for keyword in [
        "matched_moments",
        "matched_irfs",
        "matched_irfs_weights",
        "moment_calibration",
        "irf_calibration",
    ] {
        let source = format!("/* 😀中 */{keyword};\nend;\n");
        let model = parse(&source);
        let diagnostics = analyze(&model);
        assert_eq!(diagnostics.len(), 1, "{keyword}: {diagnostics:?}");
        assert_eq!(diagnostics[0].code, "E001");
        assert_eq!(diagnostics[0].severity, dygnosis::Severity::Error);
        assert_eq!(
            diagnostics[0].message,
            format!("Unexpected token in '{keyword}'. The grammar takes at least one row here.")
        );
        assert_eq!(selected(&source, "E001"), [keyword]);
    }
    let source = include_str!("fixtures/mom/mom_empty_list.mod");
    assert_eq!(selected(source, "E001"), ["method_of_moments();"]);
}

#[test]
fn retained_mixed_branches_keep_their_present_field_or_component_range() {
    let both = include_str!("fixtures/d_pac/e441_vem_both.mod");
    let range = selected(both, "E441");
    assert_eq!(range.len(), 1);
    assert!(range[0].contains("variable="));
    assert!(range[0].contains("expression="));
    let growth = include_str!("fixtures/d_pac/e438_pac_stationary_growth.mod");
    assert!(selected(growth, "E438")[0].contains("growth y"));
    let option = include_str!("fixtures/d_hank/e474_block.mod");
    assert_eq!(selected(option, "E474"), ["block"]);
    let command = include_str!("fixtures/d_open/e297_ramsey_model_twice.mod");
    assert_eq!(selected(command, "E100"), ["ramsey_model"]);
}

#[test]
fn prior_refusal_still_suppresses_later_shock_paths_after_range_change() {
    let source = "var y; varexo e u; parameters p; p=1; model; y=e+u+p; end; estimated_params; p, 0.8, 0, 1, beta_pdf, 0.8, 0.1; end; shocks; var e; stderr p; end; shock_paths; var u; periods 1; values self.u; end;";
    let diagnostics = analyze(&parse(source));
    assert_eq!(selected(source, "E212"), ["estimated_params"]);
    assert!(!diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "E420"));
    let quiet_earlier = source.replace(
        "estimated_params; p, 0.8, 0, 1, beta_pdf, 0.8, 0.1; end;",
        "",
    );
    assert_eq!(selected(&quiet_earlier, "E420"), ["self.u"]);
}

#[test]
fn unmapped_macro_keyword_keeps_its_previous_range_and_count_fallback() {
    let source = "@#define k = \"model\"\nvar x;\n@{k}; x = x(-1); x = 0; end;\n";
    assert_eq!(selected(source, "E188"), ["@{k}; x = x(-1); x = 0; end;"]);
    let source = "heterogeneity_dimension h; var(heterogeneity=h) a;";
    assert_eq!(selected(source, "W208"), ["a"]);
}

#[test]
fn aggregate_and_dimension_counts_use_the_first_written_model_of_that_scope() {
    let source = "var x; model; x = x(-1); end; model; x = 0; end;";
    let model = parse(source);
    let count = analyze(&model)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "E188")
        .unwrap();
    assert_eq!(count.span, model.statements[1].keyword_span);
    let source = "heterogeneity_dimension h; var x; var(heterogeneity=h) a; model; x = 0; end; model(heterogeneity=h); a = a(-1); end; model(heterogeneity=h); a = 0; end;";
    let model = parse(source);
    let count = analyze(&model)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "E192")
        .unwrap();
    assert_eq!(
        count.span,
        model
            .statements
            .iter()
            .find(|statement| statement.name == "model"
                && statement.dimension.as_deref() == Some("h"))
            .unwrap()
            .keyword_span
    );
}

#[test]
fn repeated_initialization_blocks_keep_existing_owner_and_summary_scope() {
    let source = "var x y z; model; x = y(-1); y = x; z = y; end; steady_state_model; x = 0; end; steady_state_model; y = 0; end; initval; x = 0; end; initval; y = 0; end;";
    let model = parse(source);
    let diagnostics = analyze(&model);
    for (code, keyword, count, message) in [
        (
            "W042",
            "steady_state_model",
            1,
            "variable 'z' is not assigned a value",
        ),
        (
            "W052",
            "initval",
            1,
            "1 endogenous variable(s) missing from initval (will default to 0): z",
        ),
    ] {
        let owner = model
            .statements
            .iter()
            .rfind(|statement| statement.name == keyword)
            .unwrap();
        let hits: Vec<_> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == code)
            .collect();
        assert_eq!(hits.len(), count);
        assert_eq!(hits[0].span, owner.keyword_span);
        assert_eq!(hits[0].message, message);
    }
    let source = "var x; model; x=x(-1); end; endval(all_values_required); end;";
    assert_eq!(selected(source, "E218"), ["endval"]);
}

#[test]
fn shared_include_summary_multiplicity_remains_separate_for_each_root() {
    let files = std::collections::HashMap::from([
        (
            "a.mod".to_string(),
            "var x y;\n@#include \"shared.inc\"\n".to_string(),
        ),
        (
            "b.mod".to_string(),
            "var x y z;\n@#include \"shared.inc\"\n".to_string(),
        ),
        (
            "shared.inc".to_string(),
            "/*😀中*/steady_state_model; x = 0; end;\n@#for i in 1:2\nvarobs x;\n@#endfor\n"
                .to_string(),
        ),
    ]);
    let report = dygnosis::dynare_workspace_diagnose(
        Some(&files),
        Some(&["a.mod".to_string(), "b.mod".to_string()]),
        None,
    )
    .unwrap();
    let roots = report["roots"].as_array().unwrap();
    for (index, root) in roots.iter().enumerate() {
        let diagnostics = root["diagnostics"].as_array().unwrap();
        let missing: Vec<_> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic["code"] == "W042")
            .collect();
        assert_eq!(missing.len(), index + 1);
        for diagnostic in missing {
            assert_eq!(diagnostic["file"], "shared.inc");
            assert_eq!(diagnostic["line"], 1);
            assert_eq!(diagnostic["column"], 7);
            assert_eq!(diagnostic["end_column"], 25);
        }
        let repeat: Vec<_> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic["code"] == "E258")
            .collect();
        assert_eq!(repeat.len(), 1);
        assert_eq!(repeat[0]["file"], "shared.inc");
        assert_eq!(
            (
                repeat[0]["line"].as_u64().unwrap(),
                repeat[0]["column"].as_u64().unwrap(),
                repeat[0]["end_column"].as_u64().unwrap()
            ),
            (3, 1, 7)
        );
    }
}

#[test]
fn macro_copies_keep_the_checked_dimension_for_error_and_warning_anchors() {
    let source = "heterogeneity_dimension a; heterogeneity_dimension b; var(heterogeneity=a) xa ya; var(heterogeneity=b) xb yb;\nmodel(heterogeneity=a); ya=1; end;\n@#for d in [\"a\",\"b\"]\nmodel(heterogeneity=@{d}); x@{d}=1; end;\n@#endfor\n";
    for (code, source) in [
        ("E192", source.to_string()),
        ("W208", source.replace("x@{d}=1;", "#p=1; x@{d}=p;")),
    ] {
        let model = parse(&source);
        let b_owner = model
            .statements
            .iter()
            .find(|statement| {
                statement.name == "model" && statement.dimension.as_deref() == Some("b")
            })
            .unwrap();
        let a_owner = model
            .statements
            .iter()
            .find(|statement| {
                statement.name == "model" && statement.dimension.as_deref() == Some("a")
            })
            .unwrap();
        let diagnostic = analyze(&model)
            .into_iter()
            .find(|diagnostic| diagnostic.code == code)
            .unwrap();
        assert_eq!(diagnostic.model_dimension.as_deref(), Some("b"));
        assert_eq!(diagnostic.span, b_owner.keyword_span, "{code}");
        assert_ne!(
            diagnostic.span, a_owner.keyword_span,
            "{code} must not point at the earlier a-only model"
        );
        assert_eq!(
            diagnostic.span.start as usize,
            source.find("model(heterogeneity=@{d})").unwrap()
        );
    }
}

#[test]
fn shared_macro_count_owner_uses_the_correct_file_for_each_root() {
    let header = "heterogeneity_dimension a; heterogeneity_dimension b; var(heterogeneity=a) xa ya; var(heterogeneity=b) xb yb;\nmodel(heterogeneity=a); ya=1; end;\n@#include \"counts.inc\"\n";
    let body =
        "/*😀中*/\n@#for d in [\"a\",\"b\"]\nmodel(heterogeneity=@{d}); x@{d}=1; end;\n@#endfor\n";
    for (code, body) in [
        ("E192", body.to_string()),
        ("W208", body.replace("x@{d}=1;", "#p=1; x@{d}=p;")),
    ] {
        let files = std::collections::HashMap::from([
            ("a.mod".to_string(), header.to_string()),
            ("b.mod".to_string(), header.replace("xb yb;", "xb yb zb;")),
            ("counts.inc".to_string(), body),
        ]);
        let report = dygnosis::dynare_workspace_diagnose(
            Some(&files),
            Some(&["a.mod".to_string(), "b.mod".to_string()]),
            None,
        )
        .unwrap();
        for root in report["roots"].as_array().unwrap() {
            let diagnostic = root["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .find(|diagnostic| diagnostic["code"] == code)
                .unwrap();
            assert_eq!(diagnostic["file"], "counts.inc", "{root}");
            assert_eq!(diagnostic["line"], 3);
            assert_eq!(diagnostic["column"], 1);
            assert_eq!(diagnostic["end_line"], 3);
            assert_eq!(diagnostic["end_column"], 6);
        }
    }
}

#[test]
fn first_child_matrix_keeps_baseline_messages_severity_and_multiplicity() {
    assert_fixture("clash/e026_varexo_det_simul.mod", "E026", "varexo_det", 0, &[(1, "A .mod file cannot contain both one of {perfect_foresight_solver, simul, perfect_foresight_with_expectation_errors_solver} and varexo_det declaration (all exogenous variables are deterministic in this case)")]);
    assert_fixture("clash/e027_ramsey_varexo_det.mod", "E027", "varexo_det", 0, &[(1, "ramsey_model and ramsey_policy are incompatible with deterministic exogenous variables")]);
    assert_fixture(
        "occbin/e171_three.mod",
        "E171",
        "occbin_constraints",
        0,
        &[(
            1,
            "only up to two constraints are supported in 'occbin_constraints' block",
        )],
    );
    assert_fixture("w120/w121_dynamic.mod", "E208", "model", 0, &[(1, "the number of equations marked [static] must be equal to the number of equations marked [dynamic]")]);
}

#[test]
fn declaration_clashes_keep_first_statement_ownership_and_macro_fallback() {
    let source = "var y;\n@#for i in 1:2\nvarexo_det tau@{i};\n@#endfor\nvarexo_det later;\nmodel; y=tau1+tau2+later; end; simul;";
    let model = parse(source);
    let diagnostic = analyze(&model)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "E026")
        .unwrap();
    let first = model
        .statements
        .iter()
        .find(|statement| statement.name == "varexo_det")
        .unwrap();
    assert_eq!(diagnostic.span, first.keyword_span);
    assert_eq!(
        diagnostic.span.start as usize,
        source.find("varexo_det tau@").unwrap()
    );
    let source = "@#define kind=\"varexo_det\"\nvar y;\n@{kind} tau;\nmodel; y=tau; end; simul;";
    assert_eq!(selected(source, "E026"), ["tau"]);
}

#[test]
fn tag_counts_use_the_first_safe_aggregate_opener_and_keep_other_row_ranges() {
    let source =
        "var y; model; y=y(-1); end;\n@#for i in 1:2\nmodel; [dynamic] y=y(-1); end;\n@#endfor\n";
    let model = parse(source);
    let diagnostics = analyze(&model);
    let count = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "E208")
        .unwrap();
    assert_eq!(count.span.start as usize, source.find("model;").unwrap());
    let duplicate = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "W054")
        .unwrap();
    assert!(
        model.source[duplicate.span.start as usize..duplicate.span.end as usize]
            .contains("[dynamic]")
    );
    let source = "@#define kind=\"model\"\nvar y;\n@{kind}; [dynamic] y=y(-1); end;";
    assert_eq!(selected(source, "E208"), ["[dynamic] y=y(-1)"]);
    let source =
        "@#define kind=\"model\"\nvar y;\n@{kind}; y=y(-1); end;\nmodel; [dynamic] y=y(-1); end;";
    let model = parse(source);
    let count = analyze(&model)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "E208")
        .unwrap();
    assert_eq!(
        count.span.start as usize,
        source.find("model; [dynamic]").unwrap()
    );
}

#[test]
fn earlier_tag_count_still_suppresses_later_shock_path_refusal() {
    let source = "var y; varexo e; model; [dynamic] y=e; end; shock_paths; var e; periods 1; values self.e; end;";
    let diagnostics = analyze(&parse(source));
    assert_eq!(selected(source, "E208"), ["model"]);
    assert!(!diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "E420"));
    assert_eq!(
        selected(&source.replace("[dynamic]", ""), "E420"),
        ["self.e"]
    );
}

#[test]
fn nested_and_dotted_matrix_keeps_baseline_messages_severity_and_multiplicity() {
    for (source, code, keyword, message) in [
        (
            include_str!("fixtures/d_pac/e438_pac_missing_auxname.mod"),
            "E438",
            "component",
            "the block 'pac_target_info(q)' is missing the 'auxname' statement in some 'component'",
        ),
        (
            include_str!("fixtures/d_pac/e438_pac_missing_kind.mod"),
            "E438",
            "component",
            "the block 'pac_target_info(q)' is missing the 'kind' statement in some 'component'",
        ),
        (
            include_str!("fixtures/d_ms/e372_prior_no_shape.mod"),
            "E372",
            "prior",
            "You must pass the shape option to the prior statement.",
        ),
        (
            include_str!("fixtures/d_ms/e373_prior_no_mean_or_mode.mod"),
            "E373",
            "prior",
            "You must pass at least one of mean and mode to the prior statement.",
        ),
        (
            "parameters alpha; /*😀中*/alpha.prior(shape=normal,mean=.5);",
            "E374",
            "prior",
            "You must pass exactly one of stdev and variance to the prior statement.",
        ),
        (
            include_str!("fixtures/d_ms/e377_joint_prior_one_name.mod"),
            "E377",
            "prior",
            "you must pass at least two parameters to the joint prior statement",
        ),
    ] {
        let model = parse(source);
        let diagnostics = analyze(&model);
        let hits: Vec<_> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == code)
            .collect();
        assert_eq!(hits.len(), 1, "{code}: {diagnostics:?}");
        assert_eq!(hits[0].severity, dygnosis::Severity::Error);
        assert_eq!(hits[0].message, message);
        assert_eq!(selected(source, code), [keyword]);
    }
}

#[test]
fn nested_analysis_spans_and_present_field_branches_stay_full() {
    let source = include_str!("fixtures/d_pac/e438_pac_missing_auxname.mod");
    let model = parse(source);
    let component = model.pac_target_info[0]
        .rows
        .iter()
        .find_map(|row| match row {
            dygnosis::model::PacTargetInfoRow::Component(component) => Some(component),
            _ => None,
        })
        .unwrap();
    let analysis = dygnosis::check_d_pac::check_check(&model);
    assert_eq!(analysis[0].span, component.span);
    assert_eq!(
        analysis[0].display_keyword,
        Some((component.keyword_span, "component"))
    );
    assert!(
        model.source[component.span.start as usize..component.span.end as usize]
            .contains("kind dd")
    );
    assert_eq!(selected(source, "E438"), ["component"]);
    for (source, code) in [
        (
            include_str!("fixtures/d_pac/e438_pac_stationary_growth.mod"),
            "E438",
        ),
        (
            include_str!("fixtures/d_ms/e374_prior_stdev_and_variance.mod"),
            "E374",
        ),
    ] {
        let model = parse(source);
        let analysis = if code == "E438" {
            dygnosis::check_d_pac::check_check(&model)
        } else {
            dygnosis::check_d_ms::check_d_ms(&model)
        };
        let original = analysis
            .iter()
            .find(|diagnostic| diagnostic.code == code)
            .unwrap();
        let display = analyze(&model)
            .into_iter()
            .find(|diagnostic| diagnostic.code == code)
            .unwrap();
        assert_eq!(original.display_keyword, None);
        assert_eq!(display.span, original.span);
    }
    let source = "parameters alpha; alpha.prior(shape=normal,mean=.5);";
    let model = parse(source);
    let statement = &model.dotted_statements[0];
    let analysis = dygnosis::check_d_ms::check_d_ms(&model);
    assert_eq!(analysis[0].span, statement.span);
    assert_eq!(
        analysis[0].display_keyword,
        Some((statement.keyword_span, "prior"))
    );
    assert_eq!(
        &model.source[statement.span.start as usize..statement.span.end as usize],
        "alpha.prior(shape=normal,mean=.5);"
    );
}

#[test]
fn nested_macro_occurrences_and_synthetic_keywords_keep_their_owner_or_fallback() {
    let pac = "var y z; varexo e e2; parameters b; b=.8; model; [name='Y'] y=b*y(-1)+e; [name='Z'] z=z(-1)+e2; end; pac_model(model_name=q,discount=b); pac_target_info(q); target y; auxname_target_nonstationary yns;\ncomponent y; auxname yaux; kind dd;\n";
    let source = format!("{pac}@#for i in 1:2\n/*😀中*/component z; kind dd;\n@#endfor\nend;");
    let model = parse(&source);
    let hit = analyze(&model)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "E438")
        .unwrap();
    assert_eq!(
        hit.span.start as usize,
        source.find("component z;").unwrap()
    );
    assert_eq!(selected(&source, "E438"), ["component"]);
    let source = format!("@#define k=\"component\"\n{pac}@{{k}} z; kind dd; end;");
    assert_eq!(selected(&source, "E438"), ["@{k} z; kind dd;"]);
    let source = "parameters alpha; alpha.prior(shape=normal,mean=.5,stdev=.1);\n@#for i in 1:2\n/*😀中*/alpha.prior(mean=.5,stdev=.1);\n@#endfor\n";
    let model = parse(source);
    let hit = analyze(&model)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "E372")
        .unwrap();
    assert_eq!(hit.span, model.dotted_statements[1].keyword_span);
    assert_eq!(selected(source, "E372"), ["prior"]);
    let source = "@#define k=\"prior\"\nparameters alpha; alpha.@{k}(mean=.5,stdev=.1);";
    assert_eq!(selected(source, "E372"), ["alpha.@{k}(mean=.5,stdev=.1);"]);
}

#[test]
fn earlier_dotted_prior_still_suppresses_later_shock_path_refusal() {
    let source = "var y; varexo e; parameters alpha; alpha=1; model; y=alpha*y(-1)+e; end; alpha.prior(mean=.5,stdev=.1); shock_paths; var e; periods 1; values self.e; end;";
    assert_eq!(selected(source, "E372"), ["prior"]);
    assert!(selected(source, "E420").is_empty());
    assert_eq!(
        selected(
            &source.replace("alpha.prior(mean=.5,stdev=.1);", ""),
            "E420"
        ),
        ["self.e"]
    );
}
