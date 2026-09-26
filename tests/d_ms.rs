//! MS-SBVAR family locks (0.5.4 01 P-parse, extended by 0.5.4 04 F-check).
//!
//! The family's `;` statements, its two `… end;` blocks, the `data` statement, and
//! the dotted `prior` / `options` / `subsamples` heads are parsed, so their option
//! lists no longer read as declarations or as parameter assignments.
//!
//! Two false **E001** shapes have a lock each: `keyword=[…]` option values on
//! `markov_switching` (invalid-identifier pass) and a multi-line option list on a
//! statement whose head is not a command name (missing-semicolon pass). A third —
//! an undeclared `aaaa = 1` with no `;` — is 04's: 7.1's lexer sends that line to
//! native MATLAB, so the missing-semicolon pass now claims a line only when its
//! head was declared before it.
//!
//! The sweep of shapes the grammar cannot spell is 04's too. Each fires **E001**
//! on a file 7.1 refuses with generic bison text, and the accepted neighbours stay
//! quiet.

use dygnosis::model::{DottedKind, FamilyValueKind, SvarIdentificationElement};
use dygnosis::{analyze, check_file, parse, Diagnostic};

const QUIET: &[&str] = &[
    "d_ms/quiet_statements.mod",
    "d_ms/quiet_blocks.mod",
    "d_ms/quiet_blocks_lower.mod",
    "d_ms/quiet_prior.mod",
    "d_ms/multiline_switching.mod",
    "d_ms/multiline_sbvar_prior.mod",
    "d_ms/native_heads.mod",
    "d_ms/excluded_heads.mod",
    "d_ms/spreadsheet_range.mod",
    "d_ms/quiet_shape_neighbours.mod",
    "d_ms/quiet_handed_over_statement_shapes.mod",
    "d_ms/quiet_option_shapes.mod",
    "d_ms/e001_native_assign_pair.mod",
    "d_ms/native_assign_heads.mod",
];

fn fixture(rel: &str) -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn fixture_path(rel: &str) -> String {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
        .to_str()
        .expect("utf-8 fixture path")
        .to_string()
}

fn codes(diags: &[Diagnostic]) -> Vec<&str> {
    diags.iter().map(|d| d.code.as_str()).collect()
}

fn find<'a>(diags: &'a [Diagnostic], code: &str) -> &'a Diagnostic {
    diags
        .iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("expected {code}, got {:?}", codes(diags)))
}

fn quiet(diags: &[Diagnostic], code: &str) {
    assert!(
        diags.iter().all(|d| d.code != code),
        "expected no {code}, got {:?}",
        codes(diags)
    );
}

/// **E227**'s official sentence, for the order locks below.
const E227_MSG_TEXT: &str =
    "The estimation statement requires a data file to be supplied via the datafile option.";

#[test]
fn legal_family_files_have_no_error() {
    for rel in QUIET {
        let diags = analyze(&parse(&fixture(rel)));
        let errors: Vec<&Diagnostic> = diags
            .iter()
            .filter(|d| d.severity == dygnosis::Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "{rel}: expected no Error, got {:?}",
            codes(&diags)
        );
        let file_diags = check_file(&fixture(rel), &fixture_path(rel));
        assert!(
            file_diags
                .iter()
                .all(|d| d.severity != dygnosis::Severity::Error),
            "{rel} check_file: expected no Error, got {:?}",
            codes(&file_diags)
        );
    }
}

#[test]
fn every_family_statement_is_parsed() {
    let model = parse(&fixture("d_ms/quiet_statements.mod"));
    let parsed: Vec<&str> = model
        .ms_statements
        .iter()
        .map(|stmt| stmt.command.as_str())
        .collect();
    for command in [
        "ms_estimation",
        "ms_simulation",
        "ms_compute_mdd",
        "ms_compute_probabilities",
        "ms_irf",
        "ms_forecast",
        "ms_variance_decomposition",
        "markov_switching",
        "svar",
        "svar_global_identification_check",
        "sbvar",
        "conditional_forecast",
        "plot_conditional_forecast",
    ] {
        assert!(
            parsed.contains(&command),
            "{command} is not parsed: {parsed:?}"
        );
    }
    assert_eq!(model.data_statements.len(), 2);
    assert!(model.svar_identifications.is_empty());
}

#[test]
fn option_rows_keep_their_value_shape() {
    let model = parse(&fixture("d_ms/quiet_statements.mod"));
    let switching = model
        .ms_statements
        .iter()
        .find(|stmt| stmt.command == "markov_switching")
        .expect("markov_switching is parsed");
    let parameters = switching
        .options
        .iter()
        .find(|opt| opt.name == "parameters")
        .expect("the parameters option is read");
    assert_eq!(parameters.value_kind, FamilyValueKind::NameList);
    assert_eq!(
        parameters
            .names
            .iter()
            .map(|(name, _)| model.name(*name))
            .collect::<Vec<_>>(),
        vec!["alpha", "beta"]
    );

    let restrictions = model
        .ms_statements
        .iter()
        .filter(|stmt| stmt.command == "markov_switching")
        .flat_map(|stmt| &stmt.options)
        .find(|opt| opt.name == "restrictions")
        .expect("the restrictions option is read");
    assert_eq!(restrictions.value_kind, FamilyValueKind::Matrix);

    let svar = model
        .ms_statements
        .iter()
        .find(|stmt| stmt.command == "svar")
        .expect("svar is parsed");
    let equations = svar
        .options
        .iter()
        .find(|opt| opt.name == "equations")
        .expect("the equations option is read");
    assert_eq!(equations.value_kind, FamilyValueKind::Vector);
    assert!(svar
        .options
        .iter()
        .any(|opt| opt.name == "coefficients" && !opt.has_value));

    let data = model
        .data_statements
        .iter()
        .find(|stmt| stmt.has_file_or_series())
        .expect("the data statement with a file is read");
    let nobs = data
        .options
        .iter()
        .find(|opt| opt.name == "nobs")
        .expect("the nobs option is read");
    assert_eq!(nobs.value_kind, FamilyValueKind::Scalar);
    let first_obs = data
        .options
        .iter()
        .find(|opt| opt.name == "first_obs")
        .expect("the first_obs option is read");
    assert_eq!(first_obs.value_kind, FamilyValueKind::Date);
}

#[test]
fn svar_identification_body_keeps_structured_rows() {
    let model = parse(&fixture("d_ms/quiet_blocks.mod"));
    let block = &model.svar_identifications[0];
    assert!(block.elements.iter().any(|element| matches!(
        element,
        SvarIdentificationElement::ExclusionConstants { .. }
    )));
    assert!(block
        .elements
        .iter()
        .any(|element| matches!(element, SvarIdentificationElement::UpperCholesky { .. })));
    let lag0 = block
        .elements
        .iter()
        .find_map(|element| match element {
            SvarIdentificationElement::ExclusionLag {
                lag: Some(0),
                equations,
                ..
            } => Some(equations),
            _ => None,
        })
        .expect("`exclusion lag 0` with its equations");
    assert_eq!(lag0.len(), 2);
    assert_eq!(lag0[0].number, Some(1));
    assert_eq!(
        lag0[0]
            .names
            .iter()
            .map(|(name, _)| model.name(*name))
            .collect::<Vec<_>>(),
        vec!["y", "c"]
    );
    let restrictions = block
        .elements
        .iter()
        .filter(|element| matches!(element, SvarIdentificationElement::Restriction { .. }))
        .count();
    assert_eq!(restrictions, 2);
}

#[test]
fn conditional_forecast_paths_rows_keep_their_lists() {
    let model = parse(&fixture("d_ms/quiet_blocks.mod"));
    let block = &model.conditional_forecast_paths[0];
    assert_eq!(block.rows.len(), 2);
    assert_eq!(model.name(block.rows[0].name), "y");
    assert_eq!(block.rows[0].periods.len(), 4);
    assert_eq!(block.rows[0].values.len(), 4);
    // `1:4` is one period entry, matching the official parser.
    assert_eq!(model.name(block.rows[1].name), "c");
    assert_eq!(block.rows[1].periods.len(), 1);
    assert_eq!(block.rows[1].values.len(), 1);
}

#[test]
fn dotted_prior_heads_are_parsed() {
    let model = parse(&fixture("d_ms/quiet_prior.mod"));
    let priors: Vec<&dygnosis::model::DottedStatement> = model
        .dotted_statements
        .iter()
        .filter(|stmt| stmt.kind == DottedKind::Prior)
        .collect();
    assert_eq!(priors.len(), 6, "five prior bodies and one copy form");
    let joint = priors
        .iter()
        .find(|stmt| matches!(stmt.head, dygnosis::model::DottedHead::Vec { .. }))
        .expect("the `[alpha, beta].prior(…)` head");
    let mean = joint
        .options
        .iter()
        .find(|opt| opt.name == "mean")
        .expect("the joint prior's mean option");
    assert_eq!(mean.value_kind, FamilyValueKind::Vector);
    assert!(priors
        .iter()
        .any(|stmt| matches!(stmt.head, dygnosis::model::DottedHead::Std { .. })));
    assert!(priors
        .iter()
        .any(|stmt| matches!(stmt.head, dygnosis::model::DottedHead::Corr { .. })));
    assert!(model
        .dotted_statements
        .iter()
        .any(|stmt| stmt.kind == DottedKind::Options));
    assert!(model
        .dotted_statements
        .iter()
        .any(|stmt| stmt.kind == DottedKind::Subsamples));
}

#[test]
fn multiline_option_lists_are_not_missing_semicolons() {
    for rel in [
        "d_ms/multiline_switching.mod",
        "d_ms/multiline_sbvar_prior.mod",
    ] {
        let diags = analyze(&parse(&fixture(rel)));
        quiet(&diags, "E001");
    }
    // The invalid-identifier pass must not read `parameters=[…]` as a declaration.
    let diags = analyze(&parse(&fixture("d_ms/multiline_switching.mod")));
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("Invalid Dynare identifier")),
        "{:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn genuine_missing_semicolon_still_fires() {
    let diags = analyze(&parse(&fixture("d_ms/e001_assign_no_semi.mod")));
    let hit = find(&diags, "E001");
    assert!(
        hit.message
            .contains("Parameter assignment 'alpha' is missing its terminating semicolon"),
        "{:?}",
        hit.message
    );
}

#[test]
fn shipped_extras_still_reach_the_family() {
    let diags = analyze(&parse(&fixture("d_ms/kept_extras.mod")));
    let twice: Vec<&Diagnostic> = diags.iter().filter(|d| d.code == "E271").collect();
    assert_eq!(
        twice.len(),
        7,
        "one per statement that repeats an option: {:?}",
        codes(&diags)
    );
    assert!(twice
        .iter()
        .any(|d| d.message == "option freq declared twice"));
    assert!(twice
        .iter()
        .any(|d| d.message == "option mean declared twice"));
    assert!(diags.iter().any(|d| d.code == "W201"));

    // W160 reaches a family statement: `ms_estimation(datafile=…)` names a file 7.1
    // reads as a companion. `kept_extras.mod` names no such file on disk.
    let rel = "d_ms/kept_extras.mod";
    let file_diags = check_file(&fixture(rel), &fixture_path(rel));
    let w160: Vec<&Diagnostic> = file_diags.iter().filter(|d| d.code == "W160").collect();
    assert!(
        w160.iter().any(|d| d.message.contains("'a.csv'")),
        "expected W160 for a.csv, got {:?}",
        codes(&file_diags)
    );
}

#[test]
fn repeated_name_inside_a_bracketed_value_is_not_an_option_repeat() {
    // 7.1 accepts `parameters=[alpha, alpha]`; only a repeated option fires.
    let src = "var y; parameters alpha; alpha = 0.5;\nmarkov_switching(chain=1, number_of_regimes=2, duration=2.5, parameters=[alpha, alpha]);\n";
    quiet(&analyze(&parse(src)), "E271");
    let twice = parse("var y; parameters alpha; alpha = 0.5;\ndata(series=y, series=y);\n");
    assert_eq!(twice.option_twice.len(), 1);
}

#[test]
fn native_lines_are_claimed_but_not_interpreted() {
    // 7.1 makes no language claim on these lines, so neither may we.
    let model = parse(&fixture("d_ms/native_heads.mod"));
    assert_eq!(
        model.ms_unparsed_spans.len(),
        6,
        "one span per native line: {:?}",
        model.ms_unparsed_spans
    );
    let diags = analyze(&model);
    assert!(
        diags
            .iter()
            .all(|d| d.severity != dygnosis::Severity::Error),
        "native lines must not carry an Error: {:?}",
        codes(&diags)
    );
    // A native line is also not read as a declaration or an assignment.
    quiet(&diags, "E001");
    quiet(&diags, "E271");
}

#[test]
fn excluded_dotted_heads_go_native() {
    // The pin's rule excludes mod-file locals and external-function names.
    let model = parse(&fixture("d_ms/excluded_heads.mod"));
    assert!(
        model.dotted_statements.is_empty(),
        "an excluded head is not a dotted statement: {:?}",
        model.dotted_statements.len()
    );
    assert_eq!(model.ms_unparsed_spans.len(), 2);
    quiet(&analyze(&model), "E271");
    // The one Warning is W160 for the `external_function` companion, not a parse claim.
    let diags = analyze(&model);
    assert!(
        diags
            .iter()
            .all(|d| d.severity != dygnosis::Severity::Error),
        "{:?}",
        codes(&diags)
    );
}

#[test]
fn a_declared_head_stays_a_statement() {
    // A head that passes the pin's rule keeps the grammar's own refusals.
    let dup = "var y; parameters alpha;\ny.prior(shape=beta, mean=0.5, mean=0.6, stdev=0.1);\n";
    assert_eq!(
        find(&analyze(&parse(dup)), "E271").message,
        "option mean declared twice"
    );
    let options = "var y; parameters alpha;\nalpha.options(init=1, init=2);\n";
    assert_eq!(
        find(&analyze(&parse(options)), "E271").message,
        "option init declared twice"
    );
    // ...and an `options` body is still claimed, so its lines are not assignments.
    let multi = "var y; parameters alpha;\nalpha.options(init=1,\n              init=2);\n";
    quiet(&analyze(&parse(multi)), "E001");
}

#[test]
fn an_xls_range_is_one_value() {
    let model = parse(&fixture("d_ms/spreadsheet_range.mod"));
    let data = &model.data_statements[0];
    let range = data
        .options
        .iter()
        .find(|opt| opt.name == "xls_range")
        .expect("the xls_range option is read");
    assert_eq!(range.value_kind, FamilyValueKind::Range);
    let text = &model.source[range.value_span.start as usize..range.value_span.end as usize];
    assert_eq!(text, "A1:B10");
    assert_eq!(
        data.options.len(),
        3,
        "file, xls_sheet, xls_range; nothing reads `B10` as an option"
    );
    quiet(&analyze(&model), "E271");
}

#[test]
fn trailing_symbol_lists_reach_e239() {
    let diags = analyze(&parse(&fixture("d_ms/e239_undeclared_irf.mod")));
    let hit = find(&diags, "E239");
    assert_eq!(hit.message, "ms_irf: Variable zzz was not declared.");
    quiet(
        &analyze(&parse(&fixture("d_ms/quiet_statements.mod"))),
        "E239",
    );
    quiet(
        &analyze(&parse(&fixture("d_ms/quiet_statements.mod"))),
        "E240",
    );
}

#[test]
fn data_statement_carries_file_or_series_for_the_e227_gate() {
    let nobs_only = parse("data(nobs=10);");
    assert_eq!(nobs_only.data_statements.len(), 1);
    assert!(!nobs_only.data_statements[0].has_file_or_series());
    let with_series = parse("data(series=c);");
    assert!(with_series.data_statements[0].has_file_or_series());
}

#[test]
fn registry_known_codes_include_shock_diagnostics() {
    assert_eq!(dygnosis::explain::known_codes().len(), 380);
}

/// Every one of the family's 42 official sentences, on its fire fixture, with
/// their text character for character.
#[test]
fn the_family_refusals_fire_with_their_text() {
    const TABLE: &[(&str, &str, &str)] = &[
        ("d_ms/e338_data_no_file.mod", "E338",
         "The file or series option must be passed to the data statement."),
        ("d_ms/e339_data_file_and_series.mod", "E339",
         "The file and series options cannot be used simultaneously in the data statement."),
        ("d_ms/e340_data_nobs_zero.mod", "E340",
         "The nobs option of the data statement only accepts positive integers."),
        ("d_ms/e341_ms_estimation_missing.mod", "E341",
         "If you do not pass no_create_init to ms_estimation, you must pass the datafile and initial_year options."),
        ("d_ms/e342_conditional_forecast_no_parameter_set.mod", "E342",
         "You must pass the `parameter_set` option to conditional_forecast"),
        ("d_ms/e343_cfp_count_mismatch.mod", "E343",
         "shocks/conditional_forecast_paths: variable Pie: number of periods is different from number of shock values"),
        ("d_ms/e344_cfp_var_twice.mod", "E344",
         "shocks/conditional_forecast_paths: variable Pie declared twice"),
        ("d_ms/e345_markov_switching_option_missing.mod", "E345",
         "A 'chain' option must be passed to the 'markov_switching' statement."),
        ("d_ms/e346_markov_switching_chain_zero.mod", "E346",
         "The value passed to the chain option must be greater than zero."),
        ("d_ms/e347_markov_switching_regimes_zero.mod", "E347",
         "The value passed to the number_of_regimes option must be greater than zero."),
        ("d_ms/e348_markov_switching_chain_order.mod", "E348",
         "The markov_switching chain option takes consecutive integers beginning at 1."),
        ("d_ms/e349_markov_switching_parameters_type.mod", "E349",
         "Variables passed to the parameters option of the markov_switching statement must be parameters. Caused by: Pie"),
        ("d_ms/e350_markov_switching_restrictions_form.mod", "E350",
         "restrictions in the subsample statement must be specified in the form [current_period_regime, next_period_regime, transition_probability]"),
        ("d_ms/e351_markov_switching_regime_beyond.mod", "E351",
         "the regimes specified in the restrictions option must be <= the number of regimes specified in the number_of_regimes option"),
        ("d_ms/e352_markov_switching_restriction_twice.mod", "E352",
         "two restrictions were given for: 1, 2"),
        ("d_ms/e353_markov_switching_probability_gt_one.mod", "E353",
         "the transition probability, 1.5 must be less than 1"),
        ("d_ms/e354_markov_switching_sums.mod", "E354",
         "When all transitions probabilities are specified for a certain regime, they must sum to 1"),
        ("d_ms/e355_markov_switching_partial_sum.mod", "E355",
         "When transition probabilites are not specified for every regime, their sum must be < 1"),
        ("d_ms/e356_svar_identification_twice.mod", "E356",
         "You may only have one svar_identification block in your .mod file."),
        ("d_ms/e357_svar_identification_two_cholesky.mod", "E357",
         "Within the svar_identification statement, you may only have one of upper_cholesky and lower_cholesky."),
        ("d_ms/e358_svar_identification_lag_twice.mod", "E358",
         "lag 0 used more than once."),
        ("d_ms/e359_svar_identification_equation_twice.mod", "E359",
         "equation number 1 referenced more than once under a single lag."),
        ("d_ms/e360_svar_identification_equation_zero.mod", "E360",
         "equation numbers must be greater than or equal to 1."),
        ("d_ms/e361_svar_identification_name_twice.mod", "E361",
         "Pie restriction added twice."),
        ("d_ms/e362_svar_identification_qi_ri.mod", "E362",
         "SVAR_IDENTIFICATION: a single restrictions must affect either Qi or Ri, but not both"),
        ("d_ms/e363_svar_none_of_three.mod", "E363",
         "You must pass one of 'coefficients', 'variances', or 'constants'."),
        ("d_ms/e364_svar_two_of_three.mod", "E364",
         "You may only pass one of 'coefficients', 'variances', or 'constants'."),
        ("d_ms/e365_svar_chain_missing.mod", "E365",
         "A 'chain' option must be passed to the 'svar' statement."),
        ("d_ms/e366_svar_chain_zero.mod", "E366",
         "The value passed to the 'chain' option must be greater than zero."),
        ("d_ms/e367_svar_equations_zero.mod", "E367",
         "The value(s) passed to the 'equations' option must be greater than zero."),
        ("d_ms/e368_ms_compute_probabilities_two.mod", "E368",
         "You may only pass one of real_time_smoothed and filtered_probabilities to ms_compute_probabilities."),
        ("d_ms/e369_ms_irf_two.mod", "E369",
         "You may only pass one of regime, regimes and filtered_probabilities to ms_irf"),
        ("d_ms/e370_ms_forecast_two.mod", "E370",
         "You may only pass one of regime and regimes to ms_forecast"),
        ("d_ms/e371_ms_variance_decomposition_two.mod", "E371",
         "You may only pass one of regime, regimes and filtered_probabilities to ms_variance_decomposition"),
        ("d_ms/e372_prior_no_shape.mod", "E372",
         "You must pass the shape option to the prior statement."),
        ("d_ms/e373_prior_no_mean_or_mode.mod", "E373",
         "You must pass at least one of mean and mode to the prior statement."),
        ("d_ms/e374_prior_stdev_and_variance.mod", "E374",
         "You must pass exactly one of stdev and variance to the prior statement."),
        ("d_ms/e375_prior_domain_two_values.mod", "E375",
         "You must pass exactly two values to the domain option."),
        ("d_ms/e376_joint_prior_domain_four_values.mod", "E376",
         "You must pass exactly four values to the domain option."),
        ("d_ms/e377_joint_prior_one_name.mod", "E377",
         "you must pass at least two parameters to the joint prior statement"),
        ("d_ms/e378_prior_head_not_parameter.mod", "E378",
         "Pie is not a parameter"),
        ("d_ms/e379_prior_corr_mixed_types.mod", "E379",
         "In the corr(A,B).prior statement, A and B must be of the same type. In your case, Pie and eps are of different types."),
    ];
    for (rel, code, message) in TABLE {
        let diags = analyze(&parse(&fixture(rel)));
        let hit = find(&diags, code);
        assert_eq!(&hit.message, message, "{rel}");
    }
}

/// The two shapes 7.1 crashes on stay silent on purpose: there is no official
/// text to copy, so no code may fire.
#[test]
fn the_two_crash_shapes_stay_silent() {
    let raw = "var R Pie Y;\nvarexo eps;\nvarobs Y Pie R;\n";
    // `parameters=[nosuch]` is an uncaught `UnknownSymbolNameException`.
    let undeclared_params = analyze(&parse(&format!(
        "{raw}markov_switching(chain=1, number_of_regimes=2, duration=2.5, parameters=[nosuch]);\n"
    )));
    for code in ["E001", "E349"] {
        quiet(&undeclared_params, code);
    }
    // A `restriction` whose expression is not a `coeff(…)` term is an
    // `ExprNode::EvalException`.
    let eval_crash = analyze(&parse(&format!(
        "{raw}svar_identification;\nrestriction equation 1, Pie = 0;\nend;\n"
    )));
    for code in ["E001", "E362"] {
        quiet(&eval_crash, code);
    }
}

/// The shapes 7.1 refuses with generic bison junk, which no digit of this
/// slice may borrow. The **0.5.4 04 F-check** slice owns them.
#[test]
fn the_bison_junk_shapes_borrow_no_digit() {
    let raw = "var R Pie Y;\nvarexo eps;\nvarobs Y Pie R;\n";
    let owned = [
        "E338", "E339", "E340", "E341", "E342", "E343", "E344", "E345", "E346", "E347", "E348",
        "E349", "E350", "E351", "E352", "E353", "E354", "E355", "E356", "E357", "E358", "E359",
        "E360", "E361", "E362", "E363", "E364", "E365", "E366", "E367", "E368", "E369", "E370",
        "E371", "E372", "E373", "E374", "E375", "E376", "E377", "E378", "E379",
    ];
    for src in [
        format!("{raw}data();\n"),
        format!("{raw}data(series=[R]);\n"),
        format!("{raw}markov_switching();\n"),
        format!("{raw}svar;\n"),
        format!("{raw}svar(constants, chain=1);\n"),
        format!("{raw}conditional_forecast;\n"),
        format!("{raw}plot_conditional_forecast;\n"),
        format!("{raw}ms_irf();\n"),
        format!("{raw}svar(coefficients, chain=1, equations=[]);\n"),
        format!("{raw}svar_identification;\nend;\n"),
        format!("{raw}svar_identification;\nexclusion lag 0;\nend;\n"),
        format!("{raw}markov_switching(chain=-1, number_of_regimes=2, duration=2.5);\n"),
        format!("{raw}markov_switching(chain=1.5, number_of_regimes=2, duration=2.5);\n"),
        format!("{raw}svar(coefficients, chain=1.5);\n"),
        format!("{raw}svar(coefficients, chain=1, equations=[-1, 2]);\n"),
        format!("{raw}data(file='x.csv', nobs=-2);\n"),
        format!("{raw}conditional_forecast(parameter_set=1);\n"),
        format!("{raw}svar_identification;\nexclusion lag -1;\nequation 1, Pie;\nend;\n"),
        format!("{raw}conditional_forecast_paths;\nexogenize Pie;\nperiods 1:4;\nvalues 1 2 3 4;\nend;\n"),
    ] {
        let diags = analyze(&parse(&src));
        for code in owned {
            assert!(
                diags.iter().all(|d| d.code != code),
                "{src:?} must not emit {code}: {:?}",
                codes(&diags)
            );
        }
    }
}

/// The `data` statement's own reading is what silences **E227**, and only when
/// it is parsed before the `estimation` it would silence.
#[test]
fn the_e227_gate_reads_file_order() {
    let preamble = "var y; varexo e; parameters rho; rho = 0.5; model; y = rho * y(-1) + e; end; ";
    // Quiet: a `data` statement before the estimation, and its own `datafile`.
    quiet(
        &analyze(&parse(&format!(
            "{preamble}data(file='x.csv'); estimation;"
        ))),
        "E227",
    );
    quiet(
        &analyze(&parse(&format!("{preamble}data(series=y); estimation;"))),
        "E227",
    );
    quiet(
        &analyze(&parse(&format!("{preamble}estimation(datafile='d.csv');"))),
        "E227",
    );
    // Fire: a `data` statement written after the estimation, at the second
    // `estimation`, and on a `data` statement that carries neither option.
    for src in [
        format!("{preamble}estimation; data(file='x.csv');"),
        format!("{preamble}estimation(datafile='d.csv'); estimation;"),
        format!("{preamble}data(nobs=10); estimation;"),
        format!("{preamble}database foo; estimation;"),
        format!("{preamble}estimation(dataseries=foo);"),
    ] {
        let diags = analyze(&parse(&src));
        assert_eq!(find(&diags, "E227").message, E227_MSG_TEXT, "{src}");
    }
    // The `data(nobs=10);` shape also earns its own sentence, and 7.1 stops
    // before it ever reaches the estimation.
    let both = analyze(&parse(&format!("{preamble}data(nobs=10); estimation;")));
    assert_eq!(
        find(&both, "E338").message,
        "The file or series option must be passed to the data statement."
    );
    assert_eq!(find(&both, "E227").message, E227_MSG_TEXT);
}

/// The two `estimation` statements keep their own records, so the second one
/// reads a gate the first did not satisfy.
#[test]
fn estimation_statements_keep_their_own_datafile() {
    let model = parse(
        "var y; varexo e; parameters rho; rho = 0.5; model; y = rho*y(-1)+e; end;\n\
         estimation(datafile='a.csv');\n\
         estimation;\n",
    );
    assert_eq!(model.estimation_statements.len(), 2);
    assert!(model.estimation_statements[0].has_datafile);
    assert!(!model.estimation_statements[1].has_datafile);
}

/// An `exclusion lag` with no `equation` row is a syntax error in 7.1 — their
/// grammar needs a row before the lag is closed — so **E358** must not fire on
/// it, and no span may ever land at offset 0.
#[test]
fn a_lag_with_no_equation_row_borrows_no_digit() {
    let head = "var R Pie Y;\nvarexo eps;\nvarobs Y Pie R;\n";
    let owned = [
        "E338", "E339", "E340", "E341", "E342", "E343", "E344", "E345", "E346", "E347", "E348",
        "E349", "E350", "E351", "E352", "E353", "E354", "E355", "E356", "E357", "E358", "E359",
        "E360", "E361", "E362", "E363", "E364", "E365", "E366", "E367", "E368", "E369", "E370",
        "E371", "E372", "E373", "E374", "E375", "E376", "E377", "E378", "E379", "E058", "E317",
        "E059",
    ];
    for src in [
        format!("{head}svar_identification;\nexclusion lag 0;\nexclusion lag 0;\nend;\n"),
        format!("{head}svar_identification;\nexclusion lag 0;\nend;\n"),
        format!("{head}svar_identification;\nexclusion lag 0;\nequation 1, Pie;\nexclusion lag 1;\nend;\n"),
        format!("{head}svar_identification;\nequation 1, Pie;\nend;\n"),
    ] {
        let diags = analyze(&parse(&src));
        for code in owned {
            assert!(
                diags.iter().all(|d| d.code != code),
                "{src:?} must not emit {code}: {:?}",
                codes(&diags)
            );
        }
        for d in &diags {
            assert_ne!(
                d.span.start, 0,
                "{src:?} put {} at offset 0",
                d.code
            );
        }
    }
    // The shape 7.1 does refuse keeps its sentence and its range.
    let full = fixture("d_ms/e358_svar_identification_lag_twice.mod");
    let diags = analyze(&parse(&full));
    let hit = find(&diags, "E358");
    assert_eq!(hit.message, "lag 0 used more than once.");
    assert_ne!(hit.span.start, 0, "E358 must point at the repeated element");
}

/// A `varexo_det` name on a `std(…)` / `corr(…)` prior head earns their other
/// sentence, which **E317** carries beside its `is not endogenous.` one.
#[test]
fn a_deterministic_exogenous_prior_head_is_e317() {
    for (rel, message) in [
        (
            "d_ms/e317_prior_std_exo_det.mod",
            "dve is an exogenous deterministic.",
        ),
        (
            "d_ms/e317_prior_corr_exo_det.mod",
            "dve is an exogenous deterministic.",
        ),
        (
            "d_ms/e317_prior_std_exo_det_subsample.mod",
            "dve is an exogenous deterministic.",
        ),
    ] {
        let diags = analyze(&parse(&fixture(rel)));
        assert_eq!(find(&diags, "E317").message, message, "{rel}");
        // Their walk stops at that name, so no body sentence follows.
        for code in [
            "E372", "E373", "E374", "E375", "E376", "E379", "E058", "E059",
        ] {
            quiet(&diags, code);
        }
    }
    // The legal neighbours stay quiet on both sides.
    for rel in [
        "d_ms/quiet_prior_std_exo.mod",
        "d_ms/quiet_prior_std_endo.mod",
        "d_ms/quiet_prior_corr_endo.mod",
    ] {
        let diags = analyze(&parse(&fixture(rel)));
        for code in ["E058", "E059", "E317"] {
            quiet(&diags, code);
        }
    }
}

/// One Error per prior head, whatever the head's shape: 7.1's walk stops at the
/// first name it refuses, and at the first statement that refuses.
#[test]
fn one_prior_head_error_per_statement() {
    let head = "var R Pie Y;\nvarexo eps;\nparameters alpha beta;\nalpha = 0.36;\nbeta = 0.99;\n\
                varobs Y Pie R;\n";
    for (rel, code, count) in [
        ("d_ms/e058_prior_corr_two_undeclared.mod", "E058", 1),
        ("d_ms/e059_prior_corr_two_params.mod", "E059", 1),
    ] {
        let diags = analyze(&parse(&fixture(rel)));
        assert_eq!(
            diags.iter().filter(|d| d.code == code).count(),
            count,
            "{rel}: {:?}",
            codes(&diags)
        );
    }
    // Two separate statements still print one Error: 7.1 exits at the first
    // refusal in the file, and so does this module.
    let two = analyze(&parse(&format!(
        "{head}corr(nosuch1, nosuch2).prior(shape=beta, mean=0.5, stdev=0.1);\n\
         corr(nosuch3, nosuch4).prior(shape=beta, mean=0.5, stdev=0.1);\n"
    )));
    assert_eq!(two.iter().filter(|d| d.code == "E058").count(), 1);
}

/// The reach closures: the shipped codes on the surfaces 01 added.
#[test]
fn the_reused_codes_reach_the_new_surfaces() {
    let ms = "var R Pie Y;\nvarexo eps;\nparameters alpha beta;\nalpha = 0.36;\nbeta = 0.99;\n\
              model;\nR = beta*R(-1) + eps;\nPie = alpha*R(-1) + eps;\nY = beta*Pie(-1) + eps;\nend;\n\
              initval;\nR = 0;\nPie = 0;\nY = 0;\nend;\nshocks;\nvar eps; stderr 0.1;\nend;\nvarobs Y Pie R;\n";
    let si = "svar_identification;\nupper_cholesky;\nend;\n";

    // E058 on the identification body's two row kinds, on a `var` row, and on a
    // `std(…)` head.
    let eq = analyze(&parse(&format!(
        "{ms}svar_identification;\nexclusion lag 0;\nequation 1, nosuchvar;\nend;\n"
    )));
    assert_eq!(
        find(&eq, "E058").message,
        "Variable 'nosuchvar' in svar_identification is not declared."
    );
    let restr = analyze(&parse(&format!(
        "{ms}svar_identification;\nrestriction equation 1, coeff(nosuchvar,0) = 0;\nend;\n"
    )));
    assert_eq!(
        find(&restr, "E058").message,
        "Variable 'nosuchvar' in svar_identification is not declared."
    );
    let cfp = analyze(&parse(&format!(
        "{ms}{si}conditional_forecast_paths;\nvar nosuch;\nperiods 1 2 3 4;\nvalues 1 2 3 4;\nend;\n"
    )));
    assert_eq!(
        find(&cfp, "E058").message,
        "Variable 'nosuch' in conditional_forecast_paths is not declared."
    );
    let std_prior = analyze(&parse(&format!(
        "{ms}std(nosuchvar).prior(shape=beta, mean=0.5, stdev=0.1);\n"
    )));
    assert_eq!(
        find(&std_prior, "E058").message,
        "Variable 'nosuchvar' in prior is not declared."
    );

    // E317 on a `var` row whose name is exogenous and on a parameter head.
    let exo_row = analyze(&parse(&format!(
        "{ms}{si}conditional_forecast_paths;\nvar eps;\nperiods 1 2 3 4;\nvalues 1 2 3 4;\nend;\n"
    )));
    assert_eq!(find(&exo_row, "E317").message, "eps is not endogenous.");
    let param_head = analyze(&parse(&format!(
        "{ms}std(alpha).prior(shape=beta, mean=0.5, stdev=0.1);\n"
    )));
    assert_eq!(
        find(&param_head, "E059").message,
        "alpha is neither endogenous or exogenous."
    );

    // The legal neighbours stay quiet, and no name is reported twice.
    let legal = analyze(&parse(&format!(
        "{ms}{si}conditional_forecast_paths;\nvar Pie;\nperiods 1 2 3 4;\nvalues 1 2 3 4;\nend;\n"
    )));
    for code in ["E058", "E317", "E343", "E344"] {
        quiet(&legal, code);
    }
    let legal_std = analyze(&parse(&format!(
        "{ms}std(eps).prior(shape=inv_gamma, mean=0.5, stdev=0.1);\n"
    )));
    for code in ["E058", "E059"] {
        quiet(&legal_std, code);
    }
    let legal_corr = analyze(&parse(&format!(
        "{ms}corr(Pie,Y).prior(shape=beta, mean=0.5, stdev=0.1);\n"
    )));
    for code in ["E058", "E059", "E379"] {
        quiet(&legal_corr, code);
    }
    for src in [&eq, &restr, &cfp, &std_prior] {
        assert_eq!(
            src.iter().filter(|d| d.code == "E058").count(),
            1,
            "one row per shape: {:?}",
            codes(src)
        );
    }
}

/// The two trailing symbol lists fire **E239** / **E240** with their text and
/// stay quiet on a legal list.
#[test]
fn the_trailing_lists_fire_e239_and_e240() {
    let table = [
        (
            "d_ms/e239_undeclared_irf.mod",
            "E239",
            "ms_irf: Variable zzz was not declared.",
        ),
        (
            "d_ms/e240_irf_not_endogenous.mod",
            "E240",
            "ms_irf: Variable eps is not one of {endogenous}",
        ),
        (
            "d_ms/e239_pcf_undeclared.mod",
            "E239",
            "plot_conditional_forecast: Variable zzz was not declared.",
        ),
        (
            "d_ms/e240_pcf_not_endogenous.mod",
            "E240",
            "plot_conditional_forecast: Variable eps is not one of {endogenous}",
        ),
    ];
    for (rel, code, message) in table {
        let diags = analyze(&parse(&fixture(rel)));
        assert_eq!(find(&diags, code).message, message, "{rel}");
    }
    let legal = analyze(&parse(&fixture("d_ms/quiet_statements.mod")));
    for code in ["E239", "E240"] {
        quiet(&legal, code);
    }
}

/// Every shape the grammar cannot spell fires **E001** with our wording, naming
/// the command or the option. One row per swept fixture; each is a file 7.1
/// refuses with `syntax error, unexpected …`, probed at `json=check`.
#[test]
fn the_shape_sweep_fires_e001() {
    let rows = [
        ("d_ms/e001_data_no_list.mod", "data"),
        ("d_ms/e001_data_empty_list.mod", "data"),
        ("d_ms/e001_markov_switching_no_list.mod", "markov_switching"),
        (
            "d_ms/e001_markov_switching_empty_list.mod",
            "markov_switching",
        ),
        ("d_ms/e001_svar_no_list.mod", "svar"),
        ("d_ms/e001_svar_empty_list.mod", "svar"),
        (
            "d_ms/e001_conditional_forecast_no_list.mod",
            "conditional_forecast",
        ),
        (
            "d_ms/e001_conditional_forecast_empty_list.mod",
            "conditional_forecast",
        ),
        (
            "d_ms/e001_plot_cf_no_symbols.mod",
            "plot_conditional_forecast",
        ),
        ("d_ms/e001_ms_irf_empty_list.mod", "ms_irf"),
        ("d_ms/e001_ms_estimation_empty_list.mod", "ms_estimation"),
        (
            "d_ms/e001_sgic_option_list.mod",
            "svar_global_identification_check",
        ),
        ("d_ms/e001_data_unknown_option.mod", "data"),
        (
            "d_ms/e001_markov_switching_unknown_option.mod",
            "markov_switching",
        ),
        ("d_ms/e001_svar_unknown_option.mod", "svar"),
        (
            "d_ms/e001_conditional_forecast_unknown_option.mod",
            "conditional_forecast",
        ),
        ("d_ms/e001_sbvar_unknown_option.mod", "sbvar"),
        ("d_ms/e001_ms_forecast_unknown_option.mod", "ms_forecast"),
        ("d_ms/e001_ms_irf_unknown_option.mod", "ms_irf"),
        ("d_ms/e001_prior_unknown_option.mod", "prior"),
        (
            "d_ms/e001_conditional_forecast_datafile.mod",
            "conditional_forecast",
        ),
        (
            "d_ms/e001_conditional_forecast_parameter_set_int.mod",
            "conditional_forecast",
        ),
        (
            "d_ms/e001_conditional_forecast_controlled_varexo_int.mod",
            "conditional_forecast",
        ),
        ("d_ms/e001_data_series_vector.mod", "data"),
        (
            "d_ms/e001_data_first_obs_integer.mod",
            "syntax error, unexpected INT_NUMBER, expecting DATE",
        ),
        ("d_ms/e001_data_nobs_negative.mod", "data"),
        ("d_ms/e001_svar_constants_flag.mod", "svar"),
        ("d_ms/e001_svar_equations_empty.mod", "svar"),
        ("d_ms/e001_svar_equations_negative.mod", "svar"),
        (
            "d_ms/e001_markov_switching_chain_negative.mod",
            "markov_switching",
        ),
        ("d_ms/e001_svar_chain_fraction.mod", "svar"),
        (
            "d_ms/e001_markov_switching_chain_fraction.mod",
            "markov_switching",
        ),
        ("d_ms/e001_ms_irf_regime_fraction.mod", "ms_irf"),
        ("d_ms/e001_prior_domain_empty.mod", "prior"),
        ("d_ms/e001_joint_prior_domain_empty.mod", "[…].prior"),
        ("d_ms/e001_joint_prior_scalar_mean.mod", "[…].prior"),
        ("d_ms/e001_identification_empty.mod", "svar_identification"),
        (
            "d_ms/e001_identification_lag_no_equation.mod",
            "exclusion lag",
        ),
        (
            "d_ms/e001_identification_stray_equation.mod",
            "svar_identification",
        ),
        ("d_ms/e001_identification_lag_negative.mod", "exclusion lag"),
        ("d_ms/e001_cfp_empty.mod", "conditional_forecast_paths"),
        ("d_ms/e001_cfp_exogenize.mod", "conditional_forecast_paths"),
        ("d_ms/e001_cfp_endogenize.mod", "conditional_forecast_paths"),
        ("d_ms/e001_cfp_periods_empty.mod", "periods"),
        ("d_ms/e001_cfp_values_empty.mod", "values"),
        ("d_ms/e001_dsample_list.mod", "dsample"),
        ("d_ms/e001_rplot_option_list.mod", "rplot"),
        ("d_ms/e001_smoother2histval_periods.mod", "smoother2histval"),
        ("d_ms/e001_var_remove_option_list.mod", "var_remove"),
        ("d_ms/e001_timed_assignment.mod", "y"),
        ("d_ms/e001_dotted_tail_unknown.mod", "alpha"),
        ("d_ms/e001_dotted_two_level_tail.mod", "alpha"),
        ("d_ms/e001_dotted_tail_assign.mod", "alpha"),
        ("d_ms/e001_dotted_head_bare.mod", "alpha"),
        (
            "d_ms/e356_identification_lag_no_equation.mod",
            "exclusion lag",
        ),
    ];
    for (rel, subject) in rows {
        let text = fixture(rel);
        let diags = analyze(&parse(&text));
        let hit = find(&diags, "E001");
        assert!(
            hit.message.contains(subject),
            "{rel}: message names {subject:?}, got {:?}",
            hit.message
        );
        assert_eq!(
            diags.iter().filter(|d| d.code == "E001").count(),
            1,
            "{rel}: one shape refuse per file: {:?}",
            codes(&diags)
        );
    }
}

/// The two `y = 3;` shapes close through the existing **E378**: 7.1's
/// `init_param` runs the same `check_symbol_is_parameter` its prior head does.
#[test]
fn the_top_level_assignment_closes_through_e378() {
    for (rel, message) in [
        (
            "d_ms/e378_assignment_not_parameter.mod",
            "y is not a parameter",
        ),
        ("d_ms/e378_exogenous_assignment.mod", "e is not a parameter"),
    ] {
        let diags = analyze(&parse(&fixture(rel)));
        assert_eq!(find(&diags, "E378").message, message, "{rel}");
        for code in ["E001", "E020", "W010"] {
            quiet(&diags, code);
        }
    }
    // A parameter assignment, an undeclared head and a mod-file local are not
    // this code's: 7.1 accepts the last two as native MATLAB.
    for src in [
        "var y; parameters alpha; alpha = 0.36; model; y = alpha; end;\nalpha = 0.5;\n",
        "var y; parameters alpha; alpha = 0.36; model; y = alpha; end;\nzzz = 0.5;\n",
        "var y; parameters alpha; alpha = 0.36; model; y = alpha; end;\n#x = 1;\nx = 0.5;\n",
    ] {
        quiet(&analyze(&parse(src)), "E378");
    }
}

/// The over-refusal this slice fixes: an undeclared head with no `;` is native
/// MATLAB text to 7.1's lexer, so the missing-semicolon pass must not claim it.
/// A head declared **before** the line still fires.
#[test]
fn the_missing_semicolon_pass_reads_the_lexer_rule() {
    for src in [
        "var y; varexo e; parameters alpha; alpha = 0.5;\nmodel; y = alpha*y(-1) + e; end;\n\
         aaaa = 1\nbbbb = 2;\n",
        "var y; varexo e; parameters alpha; alpha = 0.5;\nmodel; y = alpha*y(-1) + e; end;\n\
         aaaa = 1\nalpha = 0.5;\n",
        // A name declared only *after* the line: the lexer decides as it reads.
        "var y; varexo e; parameters alpha; alpha = 0.5;\nmodel; y = alpha*y(-1) + e; end;\n\
         gg = 1\nhh = 2;\nparameters gg hh;\n",
        // A mod-file local and an external-function name.
        "var y; varexo e; parameters alpha; alpha = 0.5;\nmodel; y = alpha*y(-1) + e; end;\n\
         #x = 1;\nx = 2\nzz = 3;\n",
        "var y; varexo e; parameters alpha; alpha = 0.5;\nmodel; y = alpha*y(-1) + e; end;\n\
         external_function(name=M_helper);\nM_helper = 2\nM2_helper = 3;\n",
    ] {
        quiet(&analyze(&parse(src)), "E001");
    }
    // A declared head with no `;` is still refused, on both sides.
    let declared = analyze(&parse(
        "var y; varexo e; parameters alpha beta; alpha = 0.5; beta = 0.9;\n\
         model; y = alpha*y(-1) + e; end;\nalpha = 1\nbeta = 2;\n",
    ));
    let hit = find(&declared, "E001");
    assert!(
        hit.message
            .contains("Parameter assignment 'alpha' is missing its terminating semicolon"),
        "{:?}",
        hit.message
    );
}

/// The shape refuse is a parse-stage refuse: 7.1's parser stops on it before any
/// check pass, whatever line the check pass would have been on. The other
/// direction matters too — a sentence whose statement comes **first** keeps its
/// digit.
#[test]
fn a_shape_refuse_comes_before_the_check_stage_sentences() {
    // A shape refuse written after a `data` sentence still wins: the `data`
    // sentence is a check-pass action, the junk is a parse-stage one.
    let junk_late = analyze(&parse(
        "var R Pie Y;\nvarexo eps;\nparameters alpha beta;\nalpha = 0.36;\nbeta = 0.99;\n\
         model;\nR = beta*R(-1) + eps;\nPie = alpha*R(-1) + eps;\nY = beta*Pie(-1) + eps;\nend;\n\
         initval;\nR = 0;\nPie = 0;\nY = 0;\nend;\nshocks;\nvar eps; stderr 0.1;\nend;\n\
         varobs Y Pie R;\ndata(nobs=10);\nmarkov_switching();\n",
    ));
    assert!(find(&junk_late, "E001")
        .message
        .contains("markov_switching"));
    quiet(&junk_late, "E338");

    // Two parse-stage refusals keep file order.
    let chain_first = analyze(&parse(
        "var R Pie Y;\nvarexo eps;\nparameters alpha beta;\nalpha = 0.36;\nbeta = 0.99;\n\
         model;\nR = beta*R(-1) + eps;\nPie = alpha*R(-1) + eps;\nY = beta*Pie(-1) + eps;\nend;\n\
         initval;\nR = 0;\nPie = 0;\nY = 0;\nend;\nshocks;\nvar eps; stderr 0.1;\nend;\n\
         varobs Y Pie R;\nmarkov_switching(chain=0, number_of_regimes=2, duration=2);\n\
         markov_switching();\n",
    ));
    assert_eq!(
        find(&chain_first, "E346").message,
        "The value passed to the chain option must be greater than zero."
    );
    quiet(&chain_first, "E001");

    let junk_first = analyze(&parse(
        "var R Pie Y;\nvarexo eps;\nparameters alpha beta;\nalpha = 0.36;\nbeta = 0.99;\n\
         model;\nR = beta*R(-1) + eps;\nPie = alpha*R(-1) + eps;\nY = beta*Pie(-1) + eps;\nend;\n\
         initval;\nR = 0;\nPie = 0;\nY = 0;\nend;\nshocks;\nvar eps; stderr 0.1;\nend;\n\
         varobs Y Pie R;\nmarkov_switching();\n\
         markov_switching(chain=0, number_of_regimes=2, duration=2);\n",
    ));
    assert!(find(&junk_first, "E001")
        .message
        .contains("markov_switching"));
    quiet(&junk_first, "E346");

    // The `conditional_forecast_paths` count sentence is a parse-stage action too,
    // so it beats a `data` sentence written after it.
    let count_first = analyze(&parse(
        "var R Pie Y;\nvarexo eps;\nparameters alpha beta;\nalpha = 0.36;\nbeta = 0.99;\n\
         model;\nR = beta*R(-1) + eps;\nPie = alpha*R(-1) + eps;\nY = beta*Pie(-1) + eps;\nend;\n\
         initval;\nR = 0;\nPie = 0;\nY = 0;\nend;\nshocks;\nvar eps; stderr 0.1;\nend;\n\
         varobs Y Pie R;\nconditional_forecast_paths;\nvar Pie;\nperiods 1 2 3;\nvalues 1 2;\nend;\n\
         data(nobs=10);\n",
    ));
    assert_eq!(
        find(&count_first, "E343").message,
        "shocks/conditional_forecast_paths: variable Pie: number of periods is different from number of shock values"
    );
    quiet(&count_first, "E338");
}

/// The shapes 7.1 crashes on stay silent: no official text to agree with, so no
/// code of this sweep may reach them. The `restriction` body is spellable by the
/// grammar, which is why the sweep's shape test is deliberately not applied to it.
#[test]
fn the_crash_shapes_are_not_swept() {
    let undeclared_params = analyze(&parse(
        "var R Pie Y;\nvarexo eps;\nvarobs Y Pie R;\n\
         markov_switching(chain=1, number_of_regimes=2, duration=2, parameters=[nosuch]);\n",
    ));
    for code in ["E001", "E349"] {
        quiet(&undeclared_params, code);
    }
    for src in [
        "var R Pie Y;\nvarexo eps;\nvarobs Y Pie R;\n\
         svar_identification;\nrestriction equation 1, Pie = 0;\nend;\n",
        "var R Pie Y;\nvarexo eps;\nvarobs Y Pie R;\n\
         svar_identification;\nrestriction equation 1, R;\nend;\n",
    ] {
        let diags = analyze(&parse(src));
        for code in ["E001", "E362"] {
            quiet(&diags, code);
        }
    }
}

/// The option tables accept exactly what 7.1's own productions take. Every row
/// here is a file 7.1 accepts, so no refuse may fire on it.
///
/// `markov_switching(…, duration=[1 2]);` is **not** in this list: the grammar
/// spells it, but their reader throws `std::bad_variant_access` on it and aborts
/// (exit `0xC0000409`), so it is a third crash shape with no sentence to agree
/// with. `duration=0` is the accepted scalar and stays.
#[test]
fn every_legal_family_option_list_is_quiet() {
    for src in [
        "ms_estimation(freq=quarterly, no_create_init);",
        "ms_estimation(freq=4, no_create_init, specification=sims_zha, alpha=0.5);",
        "ms_simulation(mh_replic=1000, drop=100, save_draws);",
        "ms_compute_mdd(proposal_lower_bound=-1, use_mean_center);",
        "ms_compute_probabilities(real_time_smoothed);",
        "ms_irf(median) y;",
        "ms_irf(median=0.5) y;",
        "ms_forecast(data_obs_nbr=20);",
        "ms_variance_decomposition(no_error_bands);",
        "sbvar(datafile=msdata, freq=4, vlist=1, vlistlog=(y, c), restriction_fname=upper_cholesky);",
        "markov_switching(chain=1, number_of_regimes=2, duration=0);",
        "markov_switching(chain=1, number_of_regimes=2, duration=2, parameters=[alpha, alpha]);",
        "svar(coefficients, chain=1, equations=[1 2]);",
        "svar(coefficients, chain=1, equations=1);",
        "svar(coefficients, chain=1, equations=[1, 2]);",
        "svar(variances, chain=1);",
        "conditional_forecast(controlled_varexo=(e));",
        "conditional_forecast(controlled_varexo=(e, u));",
        "data(file='x.csv', xls_sheet=Sheet1, xls_range=A1:B10);",
        "data(series=c);",
        "smoother2histval(period=1, invars=(y), outvars=(y), outfile='x.csv');",
        "alpha.prior(shape=beta, mean=0.5, stdev=0.1, median);",
        "alpha.prior(shape=beta, mean=0.5, stdev=0.1, median=0.4);",
        "alpha.options(init=1, jscale=0.5, bounds=[0, 1]);",
        "dsample 10;",
        "rplot y;",
        "var_remove alpha;",
        "database myfile;",
        "smoother2histval;",
    ] {
        let diags = analyze(&parse(src));
        quiet(&diags, "E001");
    }
}

/// A family keyword reached mid-expression is not a statement start to 7.1's
/// lexer: the pin enters a statement only from `INITIAL`, and `INITIAL` comes
/// back only at a `;` that closed a Dynare statement or at the newline that ends
/// a `NATIVE` block. So `w = 1./xx.data(2,3);` and `zz = 1; data(nobs=1);` are
/// native MATLAB text however their contents read, and no sweep may claim them.
#[test]
fn a_family_keyword_mid_expression_is_not_a_statement() {
    for src in [
        "var w xx; varexo e; parameters alpha; alpha = 0.5;\n\
         model; w = alpha*w(-1) + e; end;\nw = 1./xx.data(2,3);\n",
        "var w xx; varexo e; parameters alpha; alpha = 0.5;\n\
         model; w = alpha*w(-1) + e; end;\nzz = 1./xx.ms_irf(2,3);\n",
        "var w xx; varexo e; parameters alpha; alpha = 0.5;\n\
         model; w = alpha*w(-1) + e; end;\nzz = 1./xx.sbvar(2,3);\n",
        "var w xx; varexo e; parameters alpha; alpha = 0.5;\n\
         model; w = alpha*w(-1) + e; end;\nzz = 1./xx.dsample(2);\n",
        "var w xx; varexo e; parameters alpha; alpha = 0.5;\n\
         model; w = alpha*w(-1) + e; end;\nzz = 1./xx.var_remove(2);\n",
        "var w xx; varexo e; parameters alpha; alpha = 0.5;\n\
         model; w = alpha*w(-1) + e; end;\nzz = 1; data(nobs=1);\n",
        "var w xx; varexo e; parameters alpha; alpha = 0.5;\n\
         model; w = alpha*w(-1) + e; end;\nzz = 1; zz2 = 2; data(nobs=1);\n",
    ] {
        quiet(&analyze(&parse(src)), "E001");
        quiet(&analyze(&parse(src)), "E338");
    }
    // A statement that really starts after a `;` keeps its refuse.
    let real = analyze(&parse(
        "var y; varexo e; parameters alpha; alpha = 0.5;\n\
         model; y = alpha*y(-1) + e; end;\n\
         data(file='f.csv'); data(nobs=1);\n",
    ));
    assert_eq!(
        find(&real, "E338").message,
        "The file or series option must be passed to the data statement."
    );
}

/// A block body is not a statement list. Every pin `DYNARE_BLOCK` opener whose
/// body this parser does not read is skipped whole, so a bare-expression row
/// (`matched_moments; ln_c; end;`) and a bare-name row (`priors; alpha; end;`)
/// never reach the top-level recognisers.
#[test]
fn a_skipped_block_body_is_not_a_statement_list() {
    let head = "var y c; varexo e; parameters alpha; alpha = 0.5;\n\
                model; y = alpha*y(-1) + e; c = y; end;\n";
    for body in [
        "matched_moments;\ny;\nc;\nc*y;\nend;\n",
        "priors;\nalpha;\nend;\n",
        "matched_irfs;\nvar y; varexo e; periods 2:80; values (1);\nend;\n",
    ] {
        let diags = analyze(&parse(&format!("{head}{body}")));
        quiet(&diags, "E001");
        quiet(&diags, "E378");
    }
}

/// A shape refuse anywhere inside a statement pre-empts every sentence that
/// statement's own actions or check pass would print, because 7.1's parser stops
/// at the first token its grammar cannot spell. These are the five pairs the
/// review found, with the sentence on each side.
#[test]
fn a_shape_refuse_inside_a_statement_pre_empts_its_own_sentence() {
    let head = "var y c k R Pie Y; varexo e eps; parameters alpha beta;\n\
                alpha = 0.36; beta = 0.99;\n\
                model;\n\
                c = alpha*y + beta*c(-1) + e;\n\
                y = beta*y(-1) + c;\n\
                k = y;\n\
                R = beta*R(-1) + eps;\n\
                Pie = alpha*R(-1) + eps;\n\
                Y = beta*Pie(-1) + eps;\n\
                end;\n";
    let pairs = [
        (
            "markov_switching(chain=0, number_of_regimes=2, duration=2, nonsense=1);",
            "E346",
        ),
        (
            "markov_switching(chain=1, nonsense=1);",
            "E345",
        ),
        (
            "ms_estimation(datafile=nosuch, nonsense=1);",
            "E341",
        ),
        (
            "conditional_forecast(periods=8, nonsense=1);",
            "E342",
        ),
        (
            "markov_switching(chain=1, number_of_regimes=2, duration=2, parameters=[], nonsense=1);",
            "E349",
        ),
    ];
    for (stmt, sentence) in pairs {
        let diags = analyze(&parse(&format!("{head}{stmt}\n")));
        let hit = find(&diags, "E001");
        assert!(
            hit.message
                .starts_with("Unexpected token in 'markov_switching'")
                || hit
                    .message
                    .starts_with("Unexpected token in 'ms_estimation'")
                || hit
                    .message
                    .starts_with("Unexpected token in 'conditional_forecast'"),
            "{stmt}: the shape refuse must be the one reported, got {:?}",
            hit.message
        );
        quiet(&diags, sentence);
    }
    // The same statements without the shape refuse keep their sentences.
    for (stmt, sentence) in [
        (
            "markov_switching(chain=0, number_of_regimes=2, duration=2);",
            "E346",
        ),
        ("markov_switching(chain=1, number_of_regimes=2);", "E345"),
        ("ms_estimation(datafile=nosuch);", "E341"),
        ("conditional_forecast(periods=8);", "E342"),
        (
            "markov_switching(chain=1, number_of_regimes=2, duration=2, parameters=[y]);",
            "E349",
        ),
    ] {
        let diags = analyze(&parse(&format!("{head}{stmt}\n")));
        find(&diags, sentence);
        quiet(&diags, "E001");
    }
}

/// The value-shape tests tell a name from a number and from a quoted string, so
/// the files 7.1 refuses for those shapes are refused here too.
#[test]
fn names_numbers_and_strings_are_told_apart() {
    for (src, subject) in [
        ("data(file=1);", "data"),
        ("data(series=1);", "data"),
        ("data(series='c');", "data"),
        ("sbvar(restriction_fname=1);", "sbvar"),
        ("sbvar(restriction_fname='x.txt');", "sbvar"),
        (
            "ms_estimation(datafile=1, no_create_init);",
            "ms_estimation",
        ),
        ("ms_irf(file_tag=1) y;", "ms_irf"),
        (
            "conditional_forecast(parameter_set=calibration, controlled_varexo=(1));",
            "conditional_forecast",
        ),
    ] {
        let diags = analyze(&parse(src));
        let hit = find(&diags, "E001");
        assert!(
            hit.message.contains(subject),
            "{src}: got {:?}",
            hit.message
        );
    }
    // `equations` takes a bare integer as well as a vector, and 0 reaches E367.
    assert_eq!(
        find(
            &analyze(&parse("svar(coefficients, chain=1, equations=0);")),
            "E367"
        )
        .message,
        "The value(s) passed to the 'equations' option must be greater than zero."
    );
    for src in [
        "data(file=x);",
        "data(series=c);",
        "sbvar(restriction_fname=upper_cholesky);",
        "ms_estimation(datafile=x, no_create_init);",
    ] {
        quiet(&analyze(&parse(src)), "E001");
    }
}

/// The two `duration` vector forms abort 7.1's reader rather than printing a
/// sentence, so they are a third crash shape and must stay silent.
#[test]
fn the_duration_vector_crash_shape_stays_silent() {
    for src in [
        "markov_switching(chain=1, number_of_regimes=2, duration=[1 2]);",
        "markov_switching(chain=1, number_of_regimes=2, duration=[1]);",
    ] {
        let diags = analyze(&parse(src));
        for code in ["E001", "E345", "E346", "E347"] {
            quiet(&diags, code);
        }
    }
    // The scalar forms are the accepted ones and stay sentence-clean.
    for src in [
        "markov_switching(chain=1, number_of_regimes=2, duration=2);",
        "markov_switching(chain=1, number_of_regimes=2, duration=0);",
    ] {
        let diags = analyze(&parse(src));
        quiet(&diags, "E001");
    }
}
