//! MS-SBVAR family locks (0.5.4 01 P-parse).
//!
//! The family's `;` statements, its two `… end;` blocks, the `data` statement, and
//! the dotted `prior` / `options` / `subsamples` heads are parsed, so their option
//! lists no longer read as declarations or as parameter assignments. The slice ships
//! no new codes: 7.1's refusals are 02's, and a legal family file must be quiet.
//!
//! Two false **E001** shapes have a lock each: `keyword=[…]` option values on
//! `markov_switching` (invalid-identifier pass) and a multi-line option list on a
//! statement whose head is not a command name (missing-semicolon pass).
//!
//! Lines 7.1 reads as native MATLAB text — a head that is undeclared, a mod-file
//! local, an external-function name, or a non-keyword identifier before `(` — are
//! claimed but not interpreted, so no Error fires on them either.

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
fn registry_known_codes_stays_247() {
    assert_eq!(dygnosis::explain::known_codes().len(), 247);
}
