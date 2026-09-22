//! P-mom structure locks (0.6.0 01).
//!
//! The five moment/calibration blocks are parsed rather than skipped as blobs, and
//! `method_of_moments` is a `;` statement whose option list is recorded. This slice
//! ships no diagnostic: a legal file parses with no Error, and a row the grammar has
//! no production for is skipped to its `;` rather than refused.
//!
//! Missing `end;` is the existing block **E001** (see `e001.rs`).

use dygnosis::model::{CalibrationRange, FamilyValueKind};
use dygnosis::{analyze, check_parse, parse, Diagnostic};

fn fixture(rel: &str) -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn codes(diags: &[Diagnostic]) -> Vec<&str> {
    diags.iter().map(|d| d.code.as_str()).collect()
}

/// Every legal `mom/` fixture is parse-clean and free of every Error.
#[test]
fn legal_mom_fixtures_are_parse_clean() {
    for name in [
        "matched_moments",
        "matched_irfs",
        "matched_irfs_overwrite",
        "matched_irfs_weights",
        "moment_calibration",
        "irf_calibration",
        "mom_irf",
    ] {
        let text = fixture(&format!("mom/{name}.mod"));
        let model = parse(&text);
        assert!(
            check_parse(&model).is_empty(),
            "{name}: expected no E001, got {:?}",
            check_parse(&model)
                .iter()
                .map(|d| (d.code.clone(), d.message.clone()))
                .collect::<Vec<_>>()
        );
        assert!(
            !model.shape_refuses.iter().any(|r| r.subject.is_empty()),
            "{name}: unexpected shape refuse"
        );
        assert!(
            analyze(&model)
                .iter()
                .all(|d| !matches!(d.severity, dygnosis::Severity::Error)),
            "{name}: expected no Error, got {:?}",
            codes(&analyze(&model))
        );
    }
}

#[test]
fn matched_moments_rows_keep_their_expressions() {
    let model = parse(&fixture("mom/matched_moments.mod"));
    assert_eq!(model.matched_moments_blocks.len(), 1);
    let rows: Vec<&str> = model
        .matched_moments
        .iter()
        .map(|r| r.text.as_str())
        .collect();
    assert_eq!(rows, vec!["y", "c*y", "y*y(-1)"]);
    for row in &model.matched_moments {
        assert!(row.expr.is_some(), "{}: no expression tree", row.text);
        assert!(
            row.span.start < row.span.end,
            "{}: empty row span",
            row.text
        );
        let text = &model.source[row.span.start as usize..row.span.end as usize];
        assert!(
            text.contains(&row.text),
            "{:?} should cover its expression, span covers {text:?}",
            row.text
        );
    }
}

#[test]
fn matched_irfs_row_keeps_periods_values_and_no_weights() {
    let model = parse(&fixture("mom/matched_irfs.mod"));
    assert_eq!(model.matched_irfs.len(), 1);
    let block = &model.matched_irfs[0];
    assert!(!block.overwrite);
    assert_eq!(block.rows.len(), 1);
    let row = &block.rows[0];
    assert_eq!(model.name(row.endogenous), "y");
    assert_eq!(model.name(row.exogenous), "e");
    // `periods 1 2` is two entries; so is `values 1 2`.
    assert_eq!(row.periods.len(), 2);
    assert_eq!(row.values.len(), 2);
    assert!(row.weights.is_empty());
}

#[test]
fn matched_irfs_overwrite_flag_and_single_entry_lists() {
    let model = parse(&fixture("mom/matched_irfs_overwrite.mod"));
    let block = &model.matched_irfs[0];
    assert!(block.overwrite);
    let row = &block.rows[0];
    // `1:2` is one period entry, `(1)` is one value, `3` is one weight.
    assert_eq!(row.periods.len(), 1);
    assert_eq!(row.values.len(), 1);
    assert_eq!(row.weights.len(), 1);
    assert_eq!(
        &model.source[row.periods[0].start as usize..row.periods[0].end as usize],
        "1:2"
    );
    assert_eq!(
        &model.source[row.values[0].start as usize..row.values[0].end as usize],
        "(1)"
    );
}

#[test]
fn matched_irfs_weights_row_keeps_both_periods() {
    let model = parse(&fixture("mom/matched_irfs_weights.mod"));
    assert_eq!(model.matched_irfs_weights.len(), 1);
    assert!(!model.matched_irfs_weights[0].overwrite);
    assert_eq!(model.matched_irfs_weight_rows.len(), 1);
    let row = &model.matched_irfs_weight_rows[0];
    assert_eq!(model.name(row.left_endo), "y");
    assert_eq!(row.left_periods, "1");
    assert_eq!(model.name(row.left_exo), "e");
    assert_eq!(model.name(row.right_endo), "c");
    assert_eq!(row.right_periods, "2");
    assert_eq!(model.name(row.right_exo), "e");
    assert_eq!(row.weight_text, "0.5");
}

#[test]
fn moment_calibration_rows_keep_lags_and_range_kinds() {
    let model = parse(&fixture("mom/moment_calibration.mod"));
    assert_eq!(model.moment_calibration.len(), 1);
    let rows = &model.moment_calibration[0].rows;
    assert_eq!(rows.len(), 3);

    assert_eq!(model.name(rows[0].first), "y");
    assert_eq!(model.name(rows[0].second), "c");
    assert_eq!(rows[0].lags, None, "no `(…)` means 7.1's default of 0");
    match &rows[0].range {
        CalibrationRange::Bracket { lower, upper, .. } => {
            assert_eq!(lower, "0.5");
            assert_eq!(upper, "1.2");
        }
        other => panic!("expected a bracket range, got {other:?}"),
    }

    assert_eq!(rows[1].lags.as_deref(), Some("1"));
    assert!(matches!(rows[1].range, CalibrationRange::Plus { .. }));

    assert_eq!(rows[2].lags.as_deref(), Some("-2:2"));
    assert!(matches!(rows[2].range, CalibrationRange::Minus { .. }));
}

#[test]
fn irf_calibration_relative_irf_and_period_range() {
    let model = parse(&fixture("mom/irf_calibration.mod"));
    assert_eq!(model.irf_calibration.len(), 1);
    let block = &model.irf_calibration[0];
    assert!(block.relative_irf);
    assert_eq!(block.rows.len(), 1);
    let row = &block.rows[0];
    assert_eq!(model.name(row.endogenous), "y");
    assert_eq!(row.periods.as_deref(), Some("1:4"));
    assert_eq!(model.name(row.exogenous), "e");
    assert!(matches!(row.range, CalibrationRange::Plus { .. }));
}

#[test]
fn mom_statement_records_options_and_the_clash_span() {
    let text = fixture("mom/mom_irf.mod");
    let model = parse(&text);
    assert_eq!(model.mom_statements.len(), 1);
    let stmt = &model.mom_statements[0];
    assert_eq!(stmt.options.len(), 1);
    assert_eq!(stmt.options[0].name, "mom_method");
    assert_eq!(stmt.options[0].value_text, "IRF_MATCHING");
    assert_eq!(stmt.options[0].value_kind, FamilyValueKind::Scalar);

    let span = model.method_of_moments_span.expect("clash span");
    assert_eq!(
        &text[span.start as usize..span.end as usize],
        "method_of_moments",
        "the clash check reads the keyword's own span"
    );
    assert!(span.start >= stmt.span.start && span.end <= stmt.span.end);
}

#[test]
fn bare_mom_statement_has_no_option_rows() {
    let model = parse("var y; varexo e; model; y = e; end; method_of_moments;");
    assert_eq!(model.mom_statements.len(), 1);
    assert!(model.mom_statements[0].options.is_empty());
    assert!(model.method_of_moments_span.is_some());
    let diags = analyze(&model);
    assert!(
        diags.iter().all(|d| !d.message.contains("mom_method")),
        "this slice emits no check sentence: {:?}",
        codes(&diags)
    );
}

/// The `(…)` on `matched_moments` / `moment_calibration` is outside the grammar.
/// This slice consumes it and stays quiet (close call 1): 02 owns the sentence.
#[test]
fn out_of_grammar_rows_are_skipped_without_an_error() {
    let head = "var y c; varexo e; parameters a; a = 0.5;\n\
                model; y = a*y(-1) + e; c = y; end;\n";
    for body in [
        "matched_moments(extra);\ny;\nend;\n",
        "matched_moments;\ny = 3;\nend;\n",
        "matched_moments; end;\n",
        "matched_irfs; end;\n",
        "matched_irfs(zoom);\nvar y; varexo e; periods 1; values 1; end;\n",
        "matched_irfs;\nvar y; varexo e; periods 2000Q1; values 1; end;\n",
        "matched_irfs;\nvar y; varexo e; periods 1:2; values (1) (2); end;\n",
        "matched_irfs_weights;\ny(1), e, c(2), e, 0.5;\ny(1), e, c(2), e, 0.7;\nend;\n",
        "moment_calibration();\ny, y, [0, 1];\nend;\n",
        "moment_calibration;\ne, y, [0, 1];\nend;\n",
        "irf_calibration(hp_filter=1600);\ny, e, +;\nend;\n",
        "irf_calibration;\ny, y, [0, 1];\nend;\n",
        "method_of_moments(mom_method=FOO);\n",
        "method_of_moments();\n",
    ] {
        let model = parse(&format!("{head}{body}"));
        assert!(
            check_parse(&model).is_empty(),
            "{body:?}: expected no E001, got {:?}",
            check_parse(&model)
                .iter()
                .map(|d| (d.code.clone(), d.message.clone()))
                .collect::<Vec<_>>()
        );
    }
}

/// A legal row after an out-of-grammar one is still read: the skip stops at the
/// row's own `;`.
#[test]
fn a_skipped_row_does_not_swallow_the_next_one() {
    let head = "var y c; varexo e; parameters a; a = 0.5;\n\
                model; y = a*y(-1) + e; c = y; end;\n";
    let model = parse(&format!("{head}matched_moments;\ny = 3;\ny;\nc*y;\nend;\n"));
    let rows: Vec<&str> = model
        .matched_moments
        .iter()
        .map(|r| r.text.as_str())
        .collect();
    assert_eq!(
        rows,
        vec!["y", "c*y"],
        "the out-of-grammar row is not stored; legal rows after it still are"
    );
}

/// Each `var`/`varexo` pair is its own row. The last keyword of a row already ate
/// that row's `;`; finishing the row must not walk into the next pair.
#[test]
fn matched_irfs_reads_every_row() {
    let source = "var y c; varexo e; parameters a; a = 0.5;\n\
                  model; y = a*y(-1) + e; c = y; end;\n\
                  matched_irfs;\n\
                  var y; varexo e; periods 1; values 1;\n\
                  var c; varexo e; periods 2; values 2;\n\
                  end;\n";
    let model = parse(source);
    assert_eq!(model.matched_irfs.len(), 1);
    let rows = &model.matched_irfs[0].rows;
    assert_eq!(rows.len(), 2, "both pairs must be stored, got {rows:?}");
    assert_eq!(model.name(rows[0].endogenous), "y");
    assert_eq!(model.name(rows[0].exogenous), "e");
    assert_eq!(rows[0].periods.len(), 1);
    assert_eq!(model.name(rows[1].endogenous), "c");
    assert_eq!(model.name(rows[1].exogenous), "e");
    assert_eq!(rows[1].periods.len(), 1);
}

/// The option side-effects the skip path used to run must still run: a repeated
/// option name is **E271** and a named `datafile` is **W160**.
#[test]
fn mom_option_side_effects_still_run() {
    let source = "var y c; varexo e; parameters a; a = 0.5;\n\
                  model; y = a*y(-1) + e; c = y; end;\n\
                  method_of_moments(mom_method=GMM, mom_method=SMM, datafile='missing_data.csv');\n";
    let model = parse(source);
    assert!(
        model
            .option_twice
            .iter()
            .any(|(name, _)| name == "mom_method"),
        "the repeated option must reach E271: {:?}",
        model.option_twice
    );
    let twice = model
        .option_twice
        .iter()
        .find(|(name, _)| name == "mom_method")
        .expect("mom_method recorded twice");
    assert_eq!(
        &source[twice.1.start as usize..twice.1.end as usize],
        "mom_method",
        "E271 points at the second option name"
    );
    // The `(…)` on the statement must not be read as a block opener's flag list.
    assert_eq!(model.mom_statements.len(), 1);
    assert!(model.matched_irfs.is_empty());
}

/// A name first seen inside a block row is a mod-file local at 7.1, so a later
/// model block using it refuses with **E281** (its scope is outside the model),
/// not with **E020**. Every name-bearing slot of the five blocks must register.
#[test]
fn block_row_names_are_mod_file_locals() {
    let head = "var y c; varexo e; parameters a; a = 0.5;\n\
                model; y = a*y(-1) + e; c = y; end;\n";
    for body in [
        "matched_moments;\nzzz;\nend;\n",
        "matched_irfs;\nvar y; varexo e; periods 1; values (zzz); end;\n",
        "matched_irfs;\nvar y; varexo e; periods 1; values 1; weights (zzz); end;\n",
        "matched_irfs_weights;\ny(1), e, c(2), e, zzz;\nend;\n",
        "moment_calibration;\ny, y, [zzz, 1];\nend;\n",
        "irf_calibration;\ny, e, [zzz, 1];\nend;\n",
    ] {
        let source = format!("{head}{body}model;\nc = y + zzz;\nend;\n");
        let diags = analyze(&parse(&source));
        let codes: Vec<&str> = codes(&diags);
        assert!(
            codes.contains(&"E281"),
            "{body:?}: expected E281 for the later model use, got {codes:?}"
        );
        assert!(
            !codes.contains(&"E020"),
            "{body:?}: the name is a mod-file local, so not E020: {codes:?}"
        );
    }
}

/// `priors` and the other still-skipped openers keep their old path.
#[test]
fn priors_is_still_skipped() {
    let head = "var y c; varexo e; parameters alpha; alpha = 0.5;\n\
                model; y = alpha*y(-1) + e; c = y; end;\n";
    let model = parse(&format!("{head}priors;\nalpha;\nend;\n"));
    assert!(model.matched_moments.is_empty());
    assert!(model.moment_calibration.is_empty());
    assert!(check_parse(&model).is_empty());
}
