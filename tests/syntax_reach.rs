//! Regression and official reach checks for repaired Dynare statement readers.
use dygnosis::{analyze, find_preprocessor, parse, run_preprocessor, JsonStage, Severity};
use std::time::Duration;

const BASE: &str = "var y;varexo e;parameters a;a=0.5;model;y=a*y(-1)+e;end;";

fn assert_code(source: &str, code: &str, needle: &str) {
    let diagnostics = analyze(&parse(source));
    assert!(
        diagnostics
            .iter()
            .any(|row| row.code == code && row.message.contains(needle)),
        "expected {code}: {needle}; got {diagnostics:?}"
    );
    if let Some(binary) = find_preprocessor(None) {
        let official = run_preprocessor(
            source,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        assert!(!official.success, "official accepted: {source}");
        assert!(
            format!("{}{}", official.raw_stdout, official.raw_stderr).contains(needle),
            "official message: {}{}",
            official.raw_stdout,
            official.raw_stderr
        );
    }
}

// The alias must execute the prior reader and reach its unknown/type checks.
#[test]
fn priors_alias_retains_rows_and_reaches_generic_prior_checks() {
    for head in ["priors", "estimated_params"] {
        let source = format!("{BASE}{head};a,0.5,,,beta_pdf,0.5,0.1;end;");
        let model = parse(&source);
        assert_eq!(model.estimated_params.len(), 1);
        assert_eq!(model.name(model.estimated_params[0].name), "a");
        assert!(
            !analyze(&model)
                .iter()
                .any(|row| row.severity == Severity::Error),
            "{:?}",
            analyze(&model)
        );
        assert_code(
            &format!("{BASE}{head};ghost,0.5,,,beta_pdf,0.5,0.1;end;"),
            "E093",
            "ghost",
        );
    }
}

// A refused instruction order or missing separator cannot claim completed Parse.
#[test]
fn controlled_paths_require_order_separators_and_complete_values() {
    for tail in [
        "exogenize y;endogenize e;periods 1;values 2;",
        "exogenize y;periods 1;values 2;endogenize e",
        "exogenize y;periods 1;values (a+);endogenize e;",
        "exogenize y;periods 1;values 2,;endogenize e;",
    ] {
        let source = format!("{BASE}perfect_foresight_controlled_paths;{tail}end;");
        let model = parse(&source);
        assert!(
            analyze(&model).iter().any(|row| row.code == "E001"),
            "{tail}"
        );
        if let Some(binary) = find_preprocessor(None) {
            assert!(
                !run_preprocessor(
                    &source,
                    &binary,
                    None,
                    Duration::from_secs(30),
                    JsonStage::Check
                )
                .success,
                "{tail}"
            );
        }
    }
    let source = format!("{BASE}perfect_foresight_controlled_paths;exogenize y;periods 1;values (a);endogenize e;end;");
    let model = parse(&source);
    assert!(!analyze(&model)
        .iter()
        .any(|row| row.severity == Severity::Error));
}

// A range must consume its complete production; rejected text creates no row.
#[test]
fn calibration_rows_reject_trailing_and_missing_tokens() {
    for (head, names) in [("moment_calibration", "y,y"), ("irf_calibration", "y,e")] {
        for range in ["-1,1", "[-1,1],2", "[1 2,3]", "[1,2", "[1,]"] {
            let source = format!("{BASE}{head};{names},{range};end;");
            assert_code(&source, "E001", "syntax error");
            let model = parse(&source);
            assert!(
                model
                    .moment_calibration
                    .iter()
                    .all(|block| block.rows.is_empty()),
                "{range}"
            );
            assert!(
                model
                    .irf_calibration
                    .iter()
                    .all(|block| block.rows.is_empty()),
                "{range}"
            );
        }
        let source = format!("{BASE}{head};{names},[-1,1];end;");
        assert!(!analyze(&parse(&source))
            .iter()
            .any(|row| row.severity == Severity::Error));
    }
}

// New alias/path reading must reach existing generic diagnostics, without
// inventing a new code or treating a later retype as the earlier type.
#[test]
fn controlled_path_callbacks_reach_name_type_and_count_checks() {
    for (row, code, needle) in [
        (
            "exogenize missing;periods 1;values 2;endogenize e;",
            "E058",
            "Unknown symbol: missing",
        ),
        (
            "exogenize a;periods 1;values 2;endogenize e;",
            "E317",
            "a is not endogenous",
        ),
        (
            "exogenize y;periods 1;values 2;endogenize a;",
            "E387",
            "a is not exogenous",
        ),
        (
            "exogenize y;periods 1 2;values 2;endogenize e;",
            "E406",
            "The number of periods is different from the number of values",
        ),
    ] {
        assert_code(
            &format!("{BASE}perfect_foresight_controlled_paths;{row}end;"),
            code,
            needle,
        );
    }
}

// Alias, calibration and trend readers must reach existing type, duplicate,
// declaration-order and expression gates on their newly retained rows.
#[test]
fn repaired_readers_reach_existing_generic_diagnostics() {
    assert_code(
        &format!("{BASE}priors;a,0.5,,,beta_pdf,0.5,0.1;a,0.5,,,beta_pdf,0.5,0.1;end;"),
        "E244",
        "declared twice",
    );
    assert_code(
        &format!("{BASE}priors;y,0.5,,,beta_pdf,0.5,0.1;end;"),
        "E093",
        "y",
    );
    assert_code(
        "var y;model;y=0;end;priors;a,0.5,,,beta_pdf,0.5,0.1;end;parameters a;",
        "E093",
        "a",
    );
    for (head, row, code, needle) in [
        ("moment_calibration", "missing,y,[0,1]", "E058", "missing"),
        (
            "moment_calibration",
            "a,y,[0,1]",
            "E317",
            "a is not endogenous",
        ),
        ("irf_calibration", "y,a,[0,1]", "E387", "not an exogenous"),
        ("observation_trends", "y(a+)", "E001", "syntax error"),
    ] {
        assert_code(&format!("{BASE}{head};{row};end;"), code, needle);
    }
}

fn path_source(controlled: bool, periods: &str, values: &str) -> String {
    if controlled {
        format!("{BASE}perfect_foresight_controlled_paths;exogenize y;periods {periods};values {values};endogenize e;end;")
    } else {
        format!("{BASE}conditional_forecast_paths;var y;periods {periods};values {values};end;")
    }
}

fn official_parse(source: &str, accepted: bool) {
    if let Some(binary) = find_preprocessor(None) {
        let result = run_preprocessor(
            source,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Parse,
        );
        assert_eq!(
            result.success, accepted,
            "{source}\n{}{}",
            result.raw_stdout, result.raw_stderr
        );
        if !accepted {
            assert!(format!("{}{}", result.raw_stdout, result.raw_stderr).contains("syntax error"));
        }
    }
}

// A bare signed number must not consume a Pratt power operand. Full
// expressions are legal only between the value-list parentheses.
#[test]
fn ordinary_path_values_keep_signed_number_and_expression_boundaries() {
    for controlled in [false, true] {
        for values in ["-2^2", "+2^2", "-2*2", "+2/2"] {
            let source = path_source(controlled, "1", values);
            let model = parse(&source);
            assert!(
                analyze(&model).iter().any(|row| row.code == "E001"),
                "{source}"
            );
            official_parse(&source, false);
        }
        for (periods, values) in [
            ("1", "(-2^2)"),
            ("1", "+2"),
            ("1 2", "-2 +2"),
            ("1,2", "-2,(a+2)"),
        ] {
            let source = path_source(controlled, periods, values);
            let model = parse(&source);
            assert!(
                !analyze(&model)
                    .iter()
                    .any(|row| row.severity == Severity::Error),
                "{source}: {:?}",
                analyze(&model)
            );
            official_parse(&source, true);
        }
    }
}

// Both callers must reject a partially consumed period list; no skipped
// comma, identifier or colon can produce a completed row callback.
#[test]
fn ordinary_path_periods_consume_exact_integer_or_date_ranges() {
    for controlled in [false, true] {
        for periods in [
            "1,",
            "1 garbage",
            ":1",
            "1:",
            "1::2",
            ",1",
            "1.5",
            "-1",
            "1:2000Q1",
            "2000Q1:1",
            "2000Q1-1",
            "2000Q1+1.5",
        ] {
            let source = path_source(controlled, periods, "2");
            let model = parse(&source);
            assert!(
                analyze(&model).iter().any(|row| row.code == "E001"),
                "{source}"
            );
            assert!(
                model
                    .controlled_paths
                    .iter()
                    .all(|block| block.stanzas.is_empty()),
                "{source}"
            );
            assert!(
                model
                    .conditional_forecast_paths
                    .iter()
                    .all(|block| block.rows.is_empty()),
                "{source}"
            );
            official_parse(&source, false);
        }
        for (periods, values) in [
            ("1:2", "2"),
            ("1 2,3", "1 2 3"),
            ("2000Q1:2000Q2", "2"),
            ("2000Q1+1:2000Q1+2", "(a)"),
            ("2000Q1,2000Q2", "2,3"),
        ] {
            if !controlled && periods.contains("2000Q") {
                continue;
            }
            let source = path_source(controlled, periods, values);
            let model = parse(&source);
            assert!(
                !analyze(&model)
                    .iter()
                    .any(|row| row.severity == Severity::Error),
                "{source}: {:?}",
                analyze(&model)
            );
            official_parse(&source, true);
        }
    }
}

// The block's DATE refusal runs after row callbacks; an earlier row type or
// count refusal must keep its own cause instead of being replaced by E482.
#[test]
fn conditional_dates_reach_the_pinned_block_callback_after_row_checks() {
    for periods in ["2000Q1", "2000Q1:2000Q2", "2000Q1+1"] {
        let source = path_source(false, periods, "2");
        assert_code(
            &source,
            "E482",
            "conditional_forecast_paths: dates are not allowed in the 'periods' keyword",
        );
        if let Some(binary) = find_preprocessor(None) {
            let result = run_preprocessor(
                &source,
                &binary,
                None,
                Duration::from_secs(30),
                JsonStage::Parse,
            );
            assert!(!result.success);
            assert!(format!("{}{}", result.raw_stdout, result.raw_stderr)
                .contains("conditional_forecast_paths: dates are not allowed"));
        }
    }
    let source = path_source(false, "2000Q1,2000Q2", "2");
    assert_code(&source, "E343", "number of periods is different");
    assert!(!analyze(&parse(&source))
        .iter()
        .any(|row| row.code == "E482"));
    let source = format!("{BASE}conditional_forecast_paths;var a;periods 2000Q1;values 2;end;");
    assert_code(&source, "E317", "a is not endogenous");
    assert!(!analyze(&parse(&source))
        .iter()
        .any(|row| row.code == "E482"));
}
