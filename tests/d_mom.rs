//! D-mom locks (0.6.0 02).
//!
//! The refuses 7.1 prints for the statements P-mom stores: the four
//! `method_of_moments` check sentences, the matched-moment walk, the name
//! sentences on the five blocks' rows, and the `matched_irfs` row sentences.
//!
//! Every fire fixture carries one problem, and its sentence is their text
//! character for character (see `honesty.rs` for the run against the installed
//! preprocessor). The walk's own table is locked here, shape by shape.

use dygnosis::explain::known_codes;
use dygnosis::{analyze, parse, Diagnostic};

fn fixture(rel: &str) -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn diags(rel: &str) -> Vec<Diagnostic> {
    analyze(&parse(&fixture(rel)))
}

fn codes(got: &[Diagnostic]) -> Vec<&str> {
    got.iter().map(|d| d.code.as_str()).collect()
}

fn find<'a>(got: &'a [Diagnostic], code: &str) -> &'a Diagnostic {
    got.iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("expected {code}, got {:?}", codes(got)))
}

fn quiet(got: &[Diagnostic], code: &str) {
    assert!(
        got.iter().all(|d| d.code != code),
        "expected no {code}, got {:?}",
        codes(got)
    );
}

/// The header every fire fixture is built on: a square model with the four
/// declaration kinds the rows below name.
fn head() -> &'static str {
    "var y c;\nvarexo e;\nvarexo_det ed;\nparameters a;\na = 0.5;\n\nmodel;\ny = a*y(-1) + e;\nc = y;\nend;\n\n"
}

/// The four `method_of_moments` sentences, with their text.
#[test]
fn the_four_statement_sentences_are_exact() {
    const TABLE: &[(&str, &str, &str)] = &[
        (
            "mom/mom_no_method.mod",
            "E382",
            "The 'method_of_moments' statement requires a method to be supplied via the 'mom_method' option. Possible values are 'GMM', 'SMM', or 'IRF_MATCHING'.",
        ),
        (
            "mom/mom_gmm_no_datafile.mod",
            "E383",
            "The 'method_of_moments' statement requires a data file to be supplied via the 'datafile' option.",
        ),
        (
            "mom/mom_analytic.mod",
            "E384",
            "The analytic_standard_errors statement requires the GMM option.",
        ),
        (
            "mom/mom_two_filters.mod",
            "E385",
            "method_of_moments: can only use one of HP, one-sided HP, and bandpass filters",
        ),
    ];
    for (rel, code, message) in TABLE {
        let got = diags(rel);
        assert_eq!(find(&got, code).message, *message, "{rel}");
    }
}

/// `analytic_jacobian` carries the same sentence with its own option name.
#[test]
fn analytic_jacobian_names_itself() {
    let source = format!(
        "{}method_of_moments(mom_method=IRF_MATCHING, analytic_jacobian);\n",
        head()
    );
    let got = analyze(&parse(&source));
    assert_eq!(
        find(&got, "E384").message,
        "The analytic_jacobian statement requires the GMM option."
    );
}

/// A `datafile` anywhere on the statement silences **E383**, even when the file
/// does not exist: the file itself is **W160**'s business, not this sentence's.
#[test]
fn a_named_datafile_silences_e383() {
    for method in ["GMM", "SMM", "gmm", "smm"] {
        let source = format!(
            "{}method_of_moments(mom_method={method}, datafile='nosuch_data.csv');\n",
            head()
        );
        let got = analyze(&parse(&source));
        quiet(&got, "E383");
    }
}

/// `IRF_MATCHING` needs no data file; the option is not required for it.
#[test]
fn irf_matching_needs_no_datafile() {
    for method in ["IRF_MATCHING", "irf_matching", "Irf_Matching"] {
        let source = format!("{}method_of_moments(mom_method={method});\n", head());
        let got = analyze(&parse(&source));
        quiet(&got, "E382");
        quiet(&got, "E383");
    }
}

/// `GMM` with an analytic flag is the shape their check pass accepts, so a
/// `datafile` beside it keeps the file clean.
#[test]
fn gmm_with_datafile_is_quiet() {
    let source = format!(
        "{}method_of_moments(mom_method=GMM, datafile='d.csv', analytic_standard_errors, analytic_jacobian);\n",
        head()
    );
    let got = analyze(&parse(&source));
    for code in ["E382", "E383", "E384", "E385"] {
        quiet(&got, code);
    }
}

/// The analytic flag is sticky across statements: an earlier `GMM` statement
/// satisfies a later `analytic_*` one, exactly as their two `mod_file_struct`
/// flags do.
#[test]
fn the_gmm_flag_carries_across_statements() {
    let source = format!(
        "{}method_of_moments(mom_method=GMM, datafile='d.csv');\nmethod_of_moments(mom_method=IRF_MATCHING, analytic_standard_errors);\n",
        head()
    );
    quiet(&analyze(&parse(&source)), "E384");

    let reverse = format!(
        "{}method_of_moments(mom_method=IRF_MATCHING, analytic_standard_errors);\nmethod_of_moments(mom_method=GMM, datafile='d.csv');\n",
        head()
    );
    let got = analyze(&parse(&reverse));
    assert_eq!(
        find(&got, "E384").message,
        "The analytic_standard_errors statement requires the GMM option."
    );
}

/// A missing method is **E382** alone: their run stops before the data-file test.
#[test]
fn a_missing_method_stops_before_the_datafile_test() {
    let got = diags("mom/mom_no_method.mod");
    quiet(&got, "E383");
    quiet(&got, "E385");
}

/// Both filters spellings count, and the two-filter sentence beats the
/// data-file test only when a method and a datafile are already there.
#[test]
fn the_filter_sentence_comes_last() {
    let no_datafile = format!(
        "{}method_of_moments(mom_method=GMM, hp_filter=1600, bandpass_filter);\n",
        head()
    );
    let got = analyze(&parse(&no_datafile));
    assert_eq!(find(&got, "E383").code, "E383");
    quiet(&got, "E385");

    let with_datafile = format!(
        "{}method_of_moments(mom_method=GMM, datafile='d.csv', one_sided_hp_filter=1600, hp_filter=100);\n",
        head()
    );
    let got = analyze(&parse(&with_datafile));
    assert_eq!(find(&got, "E385").code, "E385");
}

/// Every row of the matched-moment walk, with the reason their matcher gives.
#[test]
fn the_matched_moment_walk_reasons() {
    const TABLE: &[(&str, &str)] = &[
        // Accepted shapes: a variable, a product, a power, a lead or a lag.
        ("y", ""),
        ("c*y", ""),
        ("y^2", ""),
        ("y*y(-1)", ""),
        ("y(1)", ""),
        ("c*y^2", ""),
        ("y^2*c", ""),
        // A name that is not endogenous.
        ("e", "Variable e is not an endogenous"),
        ("a", "Variable a is not an endogenous"),
        ("ed", "Variable ed is not an endogenous"),
        ("e^2", "Variable e is not an endogenous"),
        ("y*e", "Variable e is not an endogenous"),
        // Operators their matcher refuses.
        ("y+c", "Unsupported binary operator"),
        ("y/c", "Unsupported binary operator"),
        ("y-2", "Unsupported binary operator"),
        // Powers: the base must be a variable, the exponent a positive integer.
        (
            "(y*c)^2",
            "First argument of power expression must be a variable",
        ),
        (
            "y^(-1)",
            "Second argument of power expression must be a positive integer",
        ),
        (
            "y^c",
            "Second argument of power expression must be a positive integer",
        ),
        (
            "y^2.5",
            "Second argument of power expression must be a positive integer",
        ),
        // Everything else.
        ("-y", "Unsupported expression"),
        ("log(y)", "Unsupported expression"),
        ("1", "Unsupported expression"),
        ("0", "Unsupported expression"),
        ("STEADY_STATE(y)", "Unsupported expression"),
        ("EXPECTATION(0)(y)", "Unsupported expression"),
        ("y-y", "Unsupported expression"),
        ("y*0", "Unsupported expression"),
    ];
    for (expr, reason) in TABLE {
        let source = format!("{}matched_moments;\n{expr};\nend;\n", head());
        let got = analyze(&parse(&source));
        if reason.is_empty() {
            quiet(&got, "E386");
        } else {
            assert_eq!(
                find(&got, "E386").message,
                format!("Matched moment expression has incorrect format: {reason}"),
                "{expr}"
            );
        }
    }
}

/// The walk reports a name the file never declares anywhere but which the row
/// itself writes; the scope sentence is for names already registered elsewhere.
#[test]
fn an_undeclared_moment_name_is_born_by_its_row() {
    let source = format!("{}matched_moments;\nzzz;\nend;\n", head());
    let got = analyze(&parse(&source));
    assert_eq!(
        find(&got, "E386").message,
        "Matched moment expression has incorrect format: Variable zzz is not an endogenous"
    );
    quiet(&got, "E281");
}

/// The walk runs on the tree their parser builds, which simplifies as it reads.
/// These are the simplifications that change an outcome: `y*1` folds to `y`,
/// `y-y` to their zero constant, `y^(2-1)` to `y^1`, while `y*1.0` and `y^0`
/// leave a constant operand their matcher then refuses. The identity of a
/// constant is the text it was interned under, so `1` folds and `1.0` does not.
#[test]
fn the_walk_sees_the_simplified_tree() {
    let accepted = [
        "y*1", "1*y", "y^1", "y^1.0", "y+0", "0+y", "y-0", "y/1", "y^+1", "y^(2-1)", "-(-y)",
        "y+y-y", "y*y/y", "y^2*1", "(y/c)*c", "y/(1/c)", "y*1*1", "y^0*y",
    ];
    for expr in accepted {
        let source = format!("{}matched_moments;\n{expr};\nend;\n", head());
        let got = analyze(&parse(&source));
        quiet(&got, "E386");
    }

    let refused = [
        "y*0",
        "0*y",
        "y*1.0",
        "1.0*y",
        "y^0",
        "y*(2-1)*1.0",
        "2*3",
        "y*(1.0+0)",
        "y*0.0",
    ];
    for expr in refused {
        let source = format!("{}matched_moments;\n{expr};\nend;\n", head());
        let got = analyze(&parse(&source));
        find(&got, "E386");
    }
}

/// A product walks the operand their node list holds first. Their constants are
/// interned before every variable, so a constant is walked before the variable
/// beside it — which decides whether the reason names the constant or the name.
#[test]
fn a_product_walks_the_constant_side_first() {
    // `e*2` reports the constant, because the product folds at once.
    let folded = format!("{}matched_moments;\ne*2;\nend;\n", head());
    assert_eq!(
        find(&analyze(&parse(&folded)), "E386").message,
        "Matched moment expression has incorrect format: Unsupported expression"
    );

    // `e*y` reports the name: both are variables, so the walk reaches one.
    let names = format!("{}matched_moments;\ne*y;\nend;\n", head());
    assert_eq!(
        find(&analyze(&parse(&names)), "E386").message,
        "Matched moment expression has incorrect format: Variable e is not an endogenous"
    );
}

/// The walk stops at the first failing row of a block, and reports at the
/// block's own end, as their `end_matched_moments` does.
#[test]
fn the_walk_stops_at_the_first_failing_row() {
    let source = format!("{}matched_moments;\ny;\ne;\na;\nend;\n", head());
    let got = analyze(&parse(&source));
    let diags: Vec<&Diagnostic> = got.iter().filter(|d| d.code == "E386").collect();
    assert_eq!(diags.len(), 1, "one row, one refuse: {:?}", codes(&got));
    assert_eq!(
        diags[0].message,
        "Matched moment expression has incorrect format: Variable e is not an endogenous"
    );
}

/// The name slots of the five blocks, sentence by sentence.
#[test]
fn the_row_name_sentences_are_exact() {
    const TABLE: &[(&str, &str, &str)] = &[
        (
            "mom/mc_undeclared.mod",
            "E058",
            "Variable 'zzz' in moment_calibration is not declared.",
        ),
        ("mom/mc_not_endo.mod", "E317", "e is not endogenous."),
        (
            "mom/mirf_det.mod",
            "E317",
            "ed is an exogenous deterministic.",
        ),
        ("mom/mirf_not_exo.mod", "E387", "y is not exogenous."),
        (
            "mom/ic_bad_shock.mod",
            "E387",
            "Variable y is not an exogenous.",
        ),
    ];
    for (rel, code, message) in TABLE {
        let got = diags(rel);
        assert_eq!(find(&got, code).message, *message, "{rel}");
    }
}

/// The `irf_calibration` shock sentence names the **endogenous** for every kind
/// of wrong shock, including a `varexo_det` and a parameter.
#[test]
fn the_irf_calibration_shock_sentence_names_the_endogenous() {
    for shock in ["y", "ed", "a"] {
        let source = format!("{}irf_calibration;\ny, {shock}, [0, 1];\nend;\n", head());
        let got = analyze(&parse(&source));
        assert_eq!(
            find(&got, "E387").message,
            "Variable y is not an exogenous.",
            "shock {shock}"
        );
    }
}

/// Every name slot of `matched_irfs_weights`: both endogenouses and both shocks.
#[test]
fn the_weights_row_checks_four_names() {
    const TABLE: &[(&str, &str, &str)] = &[
        ("e(1), e, c(2), e, 0.5", "E317", "e is not endogenous."),
        ("y(1), y, c(2), e, 0.5", "E387", "y is not exogenous."),
        ("y(1), e, e(2), e, 0.5", "E317", "e is not endogenous."),
        (
            "y(1), e, c(2), ed, 0.5",
            "E317",
            "ed is an exogenous deterministic.",
        ),
        ("y(1), a, c(2), e, 0.5", "E387", "a is not exogenous."),
    ];
    for (row, code, message) in TABLE {
        let source = format!("{}matched_irfs_weights;\n{row};\nend;\n", head());
        let got = analyze(&parse(&source));
        assert_eq!(find(&got, code).message, *message, "{row}");
    }
}

/// A `matched_irfs` row checks its endogenous before its shock, whatever order
/// they were written in.
#[test]
fn the_irfs_row_checks_the_endogenous_first() {
    for (row, message) in [
        (
            "var e; varexo y; periods 1; values 1",
            "e is not endogenous.",
        ),
        (
            "varexo y; var e; periods 1; values 1",
            "e is not endogenous.",
        ),
    ] {
        let source = format!("{}matched_irfs;\n{row};\nend;\n", head());
        let got = analyze(&parse(&source));
        assert_eq!(find(&got, "E317").message, message, "{row}");
    }
}

/// The `matched_irfs` row sentences, with their text.
#[test]
fn the_irfs_row_sentences_are_exact() {
    const TABLE: &[(&str, &str, &str)] = &[
        (
            "mom/mirf_dup.mod",
            "E388",
            "matched_irfs: the pair endogenous y with exogenous e appears two times",
        ),
        (
            "mom/mirfw_dup.mod",
            "E389",
            "matched_irfs: the tuple (y(1),e,c(2),e) appears two times",
        ),
        (
            "mom/mirf_count.mod",
            "E390",
            "matched_irfs: the 'periods' and 'values' keywords are not followed by the same number of elements",
        ),
        (
            "mom/mirf_weights_count.mod",
            "E391",
            "matched_irfs: the 'periods' and 'weights' keywords are not followed by the same number of elements",
        ),
        (
            "mom/mirf_date.mod",
            "E392",
            "matched_irfs: dates are not allowed in the 'periods' keyword",
        ),
    ];
    for (rel, code, message) in TABLE {
        let got = diags(rel);
        assert_eq!(find(&got, code).message, *message, "{rel}");
    }
}

/// The counts: a range is one entry, a parenthesised value is one entry, and a
/// single weight is legal because 7.1 copies it.
#[test]
fn the_counts_count_entries_not_tokens() {
    let quiet_rows = [
        "var y; varexo e; periods 1:2; values (1)",
        "var y; varexo e; periods 1 2; values 1 2; weights 3",
        "var y; varexo e; periods 1 2; values 1 2; weights 3 4",
        "var y; varexo e; periods 1 2; values 1 2; weights 7",
        "var y; varexo e; periods 1 2 3; values 1 2 3",
    ];
    for row in quiet_rows {
        let source = format!("{}matched_irfs;\n{row};\nend;\n", head());
        let got = analyze(&parse(&source));
        quiet(&got, "E390");
        quiet(&got, "E391");
    }

    let fired = [
        // `1:2` is one period, so two values do not match it.
        ("var y; varexo e; periods 1:2; values 1 2", "E390"),
        ("var y; varexo e; periods 1:2; values (1) (2)", "E390"),
        ("var y; varexo e; periods 1; values 1 2", "E390"),
        // `periods 1` is one entry, so two weights do not match it.
        ("var y; varexo e; periods 1; values 1; weights 3 4", "E391"),
    ];
    for (row, code) in fired {
        let source = format!("{}matched_irfs;\n{row};\nend;\n", head());
        let got = analyze(&parse(&source));
        find(&got, code);
    }
}

/// The weights count is checked only when more than one weight is written; a
/// single weight is copied across the periods.
#[test]
fn one_weight_is_never_a_count_error() {
    let source = format!(
        "{}matched_irfs;\nvar y; varexo e; periods 1 2 3; values 1 2 3; weights 7;\nend;\n",
        head()
    );
    quiet(&analyze(&parse(&source)), "E391");
}

/// A date is anything their lexer reads as `DATE`: a number with a unit suffix.
/// A bare integer, a range of integers, and a two-part shape are all periods.
#[test]
fn the_date_sentence_reads_their_date_token() {
    for entry in ["2000Q1", "1959M4", "2000y", "2000Y", "2000q4", "2000m12"] {
        let source = format!(
            "{}matched_irfs;\nvar y; varexo e; periods {entry}; values 1;\nend;\n",
            head()
        );
        let got = analyze(&parse(&source));
        assert_eq!(
            find(&got, "E392").message,
            "matched_irfs: dates are not allowed in the 'periods' keyword",
            "{entry}"
        );
    }
    let range = format!(
        "{}matched_irfs;\nvar y; varexo e; periods 2000Q1:2000Q4; values 1;\nend;\n",
        head()
    );
    find(&analyze(&parse(&range)), "E392");

    for entry in ["1", "1:2", "0"] {
        let source = format!(
            "{}matched_irfs;\nvar y; varexo e; periods {entry}; values 1;\nend;\n",
            head()
        );
        quiet(&analyze(&parse(&source)), "E392");
    }
}

/// The duplicate keys ignore the periods a `matched_irfs` row carries, and the
/// weight an `matched_irfs_weights` row carries; two separate blocks may repeat
/// either silently, because each block builds its own map.
#[test]
fn duplicates_are_keyed_within_one_block() {
    let irfs_same_pair = format!(
        "{}matched_irfs;\nvar y; varexo e; periods 1; values 1;\nvar y; varexo e; periods 2; values 2;\nend;\n",
        head()
    );
    find(&analyze(&parse(&irfs_same_pair)), "E388");

    let irfs_two_blocks = format!(
        "{}matched_irfs;\nvar y; varexo e; periods 1; values 1;\nend;\nmatched_irfs;\nvar y; varexo e; periods 1; values 1;\nend;\n",
        head()
    );
    quiet(&analyze(&parse(&irfs_two_blocks)), "E388");

    let weights_same_tuple = format!(
        "{}matched_irfs_weights;\ny(1), e, c(2), e, 0.5;\ny(1), e, c(2), e, 0.9;\nend;\n",
        head()
    );
    find(&analyze(&parse(&weights_same_tuple)), "E389");

    let weights_two_blocks = format!(
        "{}matched_irfs_weights;\ny(1), e, c(2), e, 0.5;\nend;\nmatched_irfs_weights;\ny(1), e, c(2), e, 0.5;\nend;\n",
        head()
    );
    quiet(&analyze(&parse(&weights_two_blocks)), "E389");

    // The tuple is written with both periods as the source spells them.
    let range_tuple = format!(
        "{}matched_irfs_weights;\ny(1:2), e, c(1), e, 0.5;\ny(1:2), e, c(1), e, 0.9;\nend;\n",
        head()
    );
    assert_eq!(
        find(&analyze(&parse(&range_tuple)), "E389").message,
        "matched_irfs: the tuple (y(1:2),e,c(1),e) appears two times"
    );
}

/// One `matched_moments` row that is a product of a name their matcher rejects
/// and one it accepts still reports the rejected name.
#[test]
fn a_product_reports_its_first_bad_name() {
    let source = format!("{}matched_moments;\ny*e;\nend;\n", head());
    assert_eq!(
        find(&analyze(&parse(&source)), "E386").message,
        "Matched moment expression has incorrect format: Variable e is not an endogenous"
    );
}

/// A name the file never declares anywhere and first writes inside a moment row
/// is registered by that row, so the walk reports its type rather than the scope
/// sentence.
#[test]
fn a_name_born_in_a_moment_row_goes_to_the_walk() {
    let source = format!("{}matched_moments;\nzzz;\nend;\n", head());
    let got = analyze(&parse(&source));
    assert_eq!(
        find(&got, "E386").message,
        "Matched moment expression has incorrect format: Variable zzz is not an endogenous"
    );
    quiet(&got, "E281");
}

/// A name born anywhere else is already a mod-file local when a moment row
/// mentions it, so the row takes the scope sentence and the walk stays quiet.
#[test]
fn a_name_born_elsewhere_takes_the_scope_sentence() {
    // Born in another block's expression slot.
    let source = format!(
        "{}matched_irfs;\nvar y; varexo e; periods 1; values (zzz);\nend;\nmatched_moments;\nzzz;\nend;\n",
        head()
    );
    let got = analyze(&parse(&source));
    assert_eq!(
        find(&got, "E281").message,
        "Variable zzz not allowed inside model declaration. Its scope is only outside model."
    );
    quiet(&got, "E386");

    // Born in a calibration range, before the row that mentions it.
    let born_in_range = format!(
        "{}moment_calibration;\ny, c, [zzz, 1];\nend;\nmatched_moments;\nzzz;\nend;\n",
        head()
    );
    let got = analyze(&parse(&born_in_range));
    find(&got, "E281");
    quiet(&got, "E386");
}

/// The birth site is what counts, not the distance: a name born in a moment row
/// is still theirs when a later block's row reuses it.
#[test]
fn a_name_born_in_a_moment_row_is_theirs_later_too() {
    for later in [
        "matched_moments;\nzzz;\nend;\n",
        "matched_irfs;\nvar y; varexo e; periods 1; values (zzz);\nend;\n",
        "moment_calibration;\ny, c, [zzz, 1];\nend;\n",
    ] {
        let source = format!("{}matched_moments;\nzzz;\nend;\n{later}", head());
        let got = analyze(&parse(&source));
        assert_eq!(
            find(&got, "E386").message,
            "Matched moment expression has incorrect format: Variable zzz is not an endogenous",
            "later: {later}"
        );
    }
}

/// A name of another symbol type inside a `#` definition is legal in the model
/// tree; the walk refuses the row that uses it on its type.
#[test]
fn a_model_local_goes_to_the_walk() {
    let source = "var y c;\nvarexo e;\nparameters a;\na = 0.5;\nmodel;\n#foo = y;\ny = a*y(-1) + e;\nc = y + foo;\nend;\nmatched_moments;\nfoo;\nend;\n";
    let got = analyze(&parse(source));
    assert_eq!(
        find(&got, "E386").message,
        "Matched moment expression has incorrect format: Variable foo is not an endogenous"
    );
}

/// An external function name in a row takes their external-function sentence.
#[test]
fn an_external_function_row_takes_e280() {
    let source = "external_function(name=ext);\nvar y c;\nvarexo e;\nparameters a;\na = 0.5;\nmodel;\ny = a*y(-1) + e;\nc = y;\nend;\nmatched_moments;\next;\nend;\n";
    let got = analyze(&parse(source));
    assert_eq!(
        find(&got, "E280").message,
        "Symbol ext is a function name external to Dynare. It cannot be used like a variable without input argument inside model."
    );
    quiet(&got, "E386");
}

/// An empty block is still the "at least one row" sentence. `y = 3` is their
/// syntax error on `=`, which stops the parse before that sentence.
#[test]
fn a_moment_block_with_no_stored_row_fires_e001() {
    let empty = format!("{}matched_moments;\nend;\n", head());
    let got = analyze(&parse(&empty));
    let hit = find(&got, "E001");
    assert!(
        hit.message.contains("'matched_moments'"),
        "empty block names the block, got {:?}",
        hit.message
    );
    let bad = format!("{}matched_moments;\ny = 3;\nend;\n", head());
    assert_eq!(
        find(&analyze(&parse(&bad)), "E001").message,
        "syntax error, unexpected EQUAL"
    );
}

/// The `(…)` shapes P-mom handed over, and the `mom_method` values the grammar
/// does not spell, are our **E001** with a hint naming what the grammar takes.
#[test]
fn the_handed_over_shapes_are_e001_with_a_hint() {
    const TABLE: &[(&str, &str)] = &[
        ("method_of_moments();", "'method_of_moments'"),
        ("method_of_moments(mom_method=FOO);", "'method_of_moments'"),
        (
            "method_of_moments(mom_method='GMM');",
            "'method_of_moments'",
        ),
        ("method_of_moments(mom_method=1);", "'method_of_moments'"),
        ("matched_moments;\nend;", "'matched_moments'"),
        ("matched_irfs;\nend;", "'matched_irfs'"),
        ("matched_irfs_weights;\nend;", "'matched_irfs_weights'"),
        ("moment_calibration;\nend;", "'moment_calibration'"),
        ("irf_calibration;\nend;", "'irf_calibration'"),
    ];
    for (body, subject) in TABLE {
        let source = format!("{}{body}\n", head());
        let got = analyze(&parse(&source));
        let hit = find(&got, "E001");
        assert!(
            hit.message.contains(subject),
            "{body}: message names {subject:?}, got {:?}",
            hit.message
        );
        assert!(
            hit.message.contains("The grammar takes"),
            "{body}: message carries the hint, got {:?}",
            hit.message
        );
    }
}

/// The empty-`()` shape does not also take the missing-method sentence: 7.1
/// refuses it while parsing, before any check pass runs.
#[test]
fn the_empty_list_does_not_also_take_e382() {
    let source = format!("{}method_of_moments();\n", head());
    let got = analyze(&parse(&source));
    find(&got, "E001");
    quiet(&got, "E382");
}

/// A bare `method_of_moments;` stores no option list and takes **E382**; the
/// `has_option_list` flag is what tells the two apart.
#[test]
fn a_bare_statement_takes_e382() {
    let model = parse(&format!("{}method_of_moments;\n", head()));
    assert!(!model.mom_statements[0].has_option_list);
    assert!(model.mom_statements[0].options.is_empty());

    let empty = parse(&format!("{}method_of_moments();\n", head()));
    assert!(empty.mom_statements[0].has_option_list);
    assert!(empty.mom_statements[0].options.is_empty());
}

/// The option list is still recorded, so **E271** and **W160** keep firing on
/// this statement.
#[test]
fn the_option_side_effects_still_run() {
    let source = format!(
        "{}method_of_moments(mom_method=GMM, datafile='nosuch_data.csv');\nmethod_of_moments(mom_method=GMM, mom_method=SMM, datafile='nosuch_data.csv');\n",
        head()
    );
    let got = analyze(&parse(&source));
    find(&got, "E271");
}

/// One refusal per file: 7.1 stops at its first. The block's parse refuse wins
/// over the check sentence, even though the statement is written first.
#[test]
fn one_refusal_per_file() {
    for (body, expected) in [
        (
            "method_of_moments(mom_method=GMM);\nmoment_calibration;\nzzz, y, [0, 1];\nend;\n",
            "E058",
        ),
        // The walk at the block's `end;` is a parse refuse too, and the block is
        // written after the statement, so it still wins.
        ("method_of_moments;\nmatched_moments;\ne;\nend;\n", "E386"),
    ] {
        let source = format!("{}{body}", head());
        let got = analyze(&parse(&source));
        let ours: Vec<&str> = got
            .iter()
            .filter(|d| is_mom_code(&d.code) || d.code == "E058")
            .map(|d| d.code.as_str())
            .collect();
        assert_eq!(ours, vec![expected], "{body}: {:?}", codes(&got));
    }
}

/// The codes this slice's checker owns.
fn is_mom_code(code: &str) -> bool {
    matches!(
        code,
        "E382"
            | "E383"
            | "E384"
            | "E385"
            | "E386"
            | "E387"
            | "E388"
            | "E389"
            | "E390"
            | "E391"
            | "E392"
    )
}

/// A `method_of_moments` sentence is a check-pass refuse, so a block's parse
/// refuse written later in the file still loses to nothing: the statements come
/// first in their check pass, but a parse refuse anywhere in the file pre-empts
/// them all.
#[test]
fn a_parse_refuse_anywhere_pre_empts_the_check_pass() {
    let source = format!(
        "{}method_of_moments;\nmoment_calibration;\nzzz, y, [0, 1];\nend;\n",
        head()
    );
    let got = analyze(&parse(&source));
    assert_eq!(find(&got, "E058").code, "E058");
    quiet(&got, "E382");
}

/// The five legal P-mom fixtures stay quiet, as do the two new quiet files.
#[test]
fn the_legal_fixtures_stay_quiet() {
    for name in [
        "matched_moments",
        "matched_irfs",
        "matched_irfs_overwrite",
        "matched_irfs_weights",
        "moment_calibration",
        "irf_calibration",
        "mom_irf",
        "mom_smm_datafile",
        "mirf_one_weight",
    ] {
        let got = diags(&format!("mom/{name}.mod"));
        let errors: Vec<&str> = got
            .iter()
            .filter(|d| d.severity == dygnosis::Severity::Error)
            .map(|d| d.code.as_str())
            .collect();
        assert!(
            errors.is_empty(),
            "{name}: expected no Error, got {errors:?}"
        );
    }
}

/// The syntax S061 held. One file per shape. The sentence is the one 7.1 prints,
/// on the token it stops on. A lead in a value is not E024: that sentence is the
/// moment-row `varexo_det` lead. The value lead has no code of its own, so it is
/// E001 with their text.
#[test]
fn the_syntax_s061_held_is_their_sentence() {
    const TABLE: &[(&str, &str, &str, &str)] = &[
        (
            "fmom_mirf_zoom",
            "E001",
            "syntax error, unexpected IDENTIFIER, expecting OVERWRITE",
            "zoom",
        ),
        (
            "fmom_mm_extra",
            "E001",
            "syntax error, unexpected '(', expecting ';'",
            "(",
        ),
        (
            "fmom_irf_hp",
            "E001",
            "syntax error, unexpected IDENTIFIER, expecting RELATIVE_IRF",
            "hp_filter",
        ),
        (
            "fmom_mirfw_zoom",
            "E001",
            "syntax error, unexpected IDENTIFIER, expecting OVERWRITE",
            "zoom",
        ),
        (
            "fmom_mcal_extra",
            "E001",
            "syntax error, unexpected '(', expecting ';'",
            "(",
        ),
        (
            "fmom_per_minus",
            "E001",
            "syntax error, unexpected MINUS, expecting DATE or INT_NUMBER",
            "-",
        ),
        (
            "fmom_per_foo",
            "E001",
            "syntax error, unexpected IDENTIFIER, expecting DATE or INT_NUMBER",
            "foo",
        ),
        (
            "fmom_per_float",
            "E001",
            "syntax error, unexpected FLOAT_NUMBER, expecting DATE or INT_NUMBER",
            "1.5",
        ),
        (
            "fmom_per_exp",
            "E001",
            "syntax error, unexpected FLOAT_NUMBER, expecting DATE or INT_NUMBER",
            "1e3",
        ),
        (
            "fmom_per_call",
            "E001",
            "syntax error, unexpected IDENTIFIER, expecting DATE or INT_NUMBER",
            "y",
        ),
        (
            "fmom_per_three",
            "E001",
            "syntax error, unexpected ':', expecting COMMA or DATE or INT_NUMBER or ';'",
            ":",
        ),
        (
            "fmom_per_empty",
            "E001",
            "syntax error, unexpected ';', expecting DATE or INT_NUMBER",
            ";",
        ),
        (
            "fmom_per_beside",
            "E392",
            "matched_irfs: dates are not allowed in the 'periods' keyword",
            "2000Q1",
        ),
        (
            "fmom_lag_foo",
            "E001",
            "syntax error, unexpected IDENTIFIER, expecting INT_NUMBER or PLUS or MINUS",
            "foo",
        ),
        (
            "fmom_lag_float",
            "E001",
            "syntax error, unexpected FLOAT_NUMBER, expecting INT_NUMBER or PLUS or MINUS",
            "1.5",
        ),
        (
            "fmom_lag_empty",
            "E001",
            "syntax error, unexpected ')', expecting INT_NUMBER or PLUS or MINUS",
            ")",
        ),
        (
            "fmom_lag_three",
            "E001",
            "syntax error, unexpected ':', expecting ')'",
            ":",
        ),
        (
            "fmom_lag_mix",
            "E001",
            "syntax error, unexpected DATE, expecting INT_NUMBER or PLUS or MINUS",
            "2000Q1",
        ),
        (
            "fmom_irf_minus",
            "E001",
            "syntax error, unexpected MINUS, expecting INT_NUMBER",
            "-",
        ),
        (
            "fmom_irf_date",
            "E001",
            "syntax error, unexpected DATE, expecting INT_NUMBER",
            "2000Q1",
        ),
        (
            "fmom_wper_minus",
            "E001",
            "syntax error, unexpected MINUS, expecting INT_NUMBER",
            "-",
        ),
        (
            "fmom_kw_values_first",
            "E001",
            "syntax error, unexpected VALUES, expecting PERIODS",
            "values",
        ),
        (
            "fmom_kw_miss_values",
            "E001",
            "syntax error, unexpected END, expecting VALUES or WEIGHTS",
            "end",
        ),
        (
            "fmom_kw_miss_periods",
            "E001",
            "syntax error, unexpected VALUES, expecting PERIODS",
            "values",
        ),
        (
            "fmom_kw_repeat",
            "E001",
            "syntax error, unexpected VALUES, expecting END or VAR or VAREXO",
            "values",
        ),
        (
            "fmom_kw_weight_name",
            "E001",
            "syntax error, unexpected IDENTIFIER",
            "y",
        ),
        (
            "fmom_bad_mm",
            "E001",
            "syntax error, unexpected EQUAL",
            "=",
        ),
        (
            "fmom_bad_mirf",
            "E001",
            "syntax error, unexpected IDENTIFIER, expecting END or VAR or VAREXO",
            "y",
        ),
        (
            "fmom_bad_mirfw",
            "E001",
            "syntax error, unexpected ';', expecting '('",
            ";",
        ),
        (
            "fmom_bad_mcal",
            "E001",
            "syntax error, unexpected ';', expecting COMMA",
            ";",
        ),
        (
            "fmom_bad_irf",
            "E001",
            "syntax error, unexpected ';', expecting COMMA or '('",
            ";",
        ),
        (
            "fmom_ex_helper",
            "E279",
            "Symbol 'helper' is the name of a MATLAB/Octave function, and cannot be used as a variable.",
            "helper",
        ),
        (
            "fmom_ex_loc",
            "E282",
            "Variable loc not allowed outside model declaration. Its scope is only inside model.",
            "loc",
        ),
        (
            "fmom_ex_trend",
            "E310",
            "Variable A not allowed outside model declaration, because it is a trend variable.",
            "A",
        ),
        (
            "fmom_ex_lead",
            "E001",
            "Using variable y with a lead or a lag is not allowed in this context",
            "y(1)",
        ),
        (
            "fmom_wt_helper",
            "E279",
            "Symbol 'helper' is the name of a MATLAB/Octave function, and cannot be used as a variable.",
            "helper",
        ),
        (
            "fmom_name_lead",
            "E001",
            "syntax error, unexpected '(', expecting ';'",
            "(",
        ),
        (
            "fmom_ns_dot",
            "E001",
            "syntax error, unexpected ';', expecting '(' or '.'",
            ";",
        ),
        (
            "fmom_ns_call",
            "E001",
            "To use an external function (foo.bar) within the model block, you must first declare it via the external_function() statement.",
            "foo.bar(y)",
        ),
        (
            "fmom_mm_det",
            "E024",
            "Exogenous deterministic variable tau cannot be given a lead or a lag",
            "tau(1)",
        ),
    ];
    for (name, code, message, token) in TABLE {
        let source = fixture(&format!("mom/{name}.mod"));
        let got = analyze(&parse(&source));
        let hit = find(&got, code);
        assert_eq!(hit.message, *message, "{name}");
        let text = &source[hit.span.start as usize..hit.span.end as usize];
        assert_eq!(text, *token, "{name} span");
        // A `#` local or a trend name in the slot is already a symbol. Registering
        // it again as a mod-file local makes E281 fire on the model, which 7.1
        // does not print for these files.
        if matches!(*name, "fmom_ex_loc" | "fmom_ex_trend" | "fmom_ex_helper" | "fmom_wt_helper") {
            assert!(
                got.iter().all(|d| d.code != "E281"),
                "{name} must not emit E281: {:?}",
                codes(&got)
            );
        }
    }
}

/// A legal neighbour of each group stays quiet: the one flag, a signed lag, a
/// parameter in a value, one bare weight, a well-formed period range.
#[test]
fn legal_syntax_neighbours_stay_quiet() {
    for body in [
        "matched_irfs(overwrite);\nvar y; varexo e; periods 1:2; values 1;\nend;\n",
        "irf_calibration(relative_irf);\ny(1:2), e, [0, 1];\nend;\n",
        "moment_calibration;\ny, c(-1), [0, 1];\ny, c(-(1:2)), +;\nend;\n",
        "matched_irfs;\nvar y; varexo e; periods 1 2; values 1 2; weights 3;\nend;\n",
        "matched_irfs;\nvar y; varexo e; periods 1; values (a);\nend;\n",
        "matched_irfs;\nvar y; varexo e; periods 1; values 1; weights (y);\nend;\n",
    ] {
        let got = analyze(&parse(&format!("{}{body}", head())));
        let errors: Vec<&str> = got
            .iter()
            .filter(|d| d.severity == dygnosis::Severity::Error)
            .map(|d| d.code.as_str())
            .collect();
        assert!(errors.is_empty(), "{body}: {errors:?}");
    }
}

/// The registry grew by the eleven new codes. S061 is dropped.
#[test]
fn registry_known_codes_include_shock_diagnostics() {
    assert_eq!(known_codes().len(), 380);
    for code in [
        "E382", "E383", "E384", "E385", "E386", "E387", "E388", "E389", "E390", "E391", "E392",
    ] {
        assert!(known_codes().contains(&code), "{code} missing");
    }
    assert!(!known_codes().contains(&"S032"), "S032 was dropped");
    assert!(!known_codes().contains(&"S061"), "S061 was dropped");
}
