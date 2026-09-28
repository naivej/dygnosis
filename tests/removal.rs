use dygnosis::diagnostic::{Diagnostic, Severity};
use dygnosis::preprocessor::find_preprocessor;
use dygnosis::{analyze, parse};
use dygnosis::{run_preprocessor, JsonStage};
use std::time::Duration;

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    analyze(&parse(source))
}

fn hits<'a>(rows: &'a [Diagnostic], code: &str) -> Vec<&'a Diagnostic> {
    rows.iter().filter(|row| row.code == code).collect()
}

const BASE: &str = "var y x;\nmodel;\ny=.5*y(-1);\n[name='drop'] x=y;\nend;\n";

#[test]
fn auxiliary_name_warning_uses_official_text_and_stops_only_its_list() {
    let source = "var y; model; y=y(-1); end; stoch_simul AUX_EXPECT_1, z; forecast z;";
    let rows = diagnostics(source);
    let warning = hits(&rows, "W186");
    assert_eq!(warning.len(), 1, "{rows:?}");
    assert_eq!(
        warning[0].message,
        "symbol_list variable AUX_EXPECT_1 has not yet been declared. This is being ignored because the variable name corresponds to a possible auxiliary variable name."
    );
    assert_eq!(
        warning[0].span.start,
        source.find("AUX_EXPECT_1").unwrap() as u32
    );
    let unknown = hits(&rows, "E239");
    assert_eq!(unknown.len(), 1, "{rows:?}");
    assert!(unknown[0].message.starts_with("forecast:"), "{rows:?}");

    let duplicate_after_hit =
        diagnostics("var y; model; y=y(-1); end; stoch_simul AUX_EXPECT_1, AUX_EXPECT_1;");
    let duplicated = hits(&duplicate_after_hit, "W202");
    assert_eq!(duplicated.len(), 1, "{duplicate_after_hit:?}");
    assert!(
        duplicated[0]
            .message
            .contains("AUX_EXPECT_1 found more than once"),
        "{duplicate_after_hit:?}"
    );
    assert_eq!(
        hits(&duplicate_after_hit, "W186").len(),
        1,
        "{duplicate_after_hit:?}"
    );
    assert!(
        hits(&duplicate_after_hit, "E239").is_empty(),
        "{duplicate_after_hit:?}"
    );

    let duplicate_later =
        diagnostics("var y; model; y=y(-1); end; stoch_simul AUX_EXPECT_1, y, y;");
    let duplicated_y = hits(&duplicate_later, "W202");
    assert_eq!(duplicated_y.len(), 1, "{duplicate_later:?}");
    assert!(
        duplicated_y[0].message.contains("y found more than once"),
        "{duplicate_later:?}"
    );
    assert_eq!(
        hits(&duplicate_later, "W186").len(),
        1,
        "{duplicate_later:?}"
    );
    assert!(
        hits(&duplicate_later, "E239").is_empty(),
        "{duplicate_later:?}"
    );
    assert!(
        hits(&duplicate_later, "E240").is_empty(),
        "{duplicate_later:?}"
    );

    let duplicate_before =
        diagnostics("var y; model; y=y(-1); end; stoch_simul y, y, AUX_EXPECT_1;");
    assert_eq!(
        hits(&duplicate_before, "W202").len(),
        1,
        "{duplicate_before:?}"
    );
    assert!(
        hits(&duplicate_before, "W202")[0]
            .message
            .contains("y found more than once"),
        "{duplicate_before:?}"
    );
    assert_eq!(
        hits(&duplicate_before, "W186").len(),
        1,
        "{duplicate_before:?}"
    );
}

#[test]
fn auxiliary_prefix_allowed_type_controls_the_warning() {
    let model = "var y; parameters a; a=1; model; y=y(-1); end;";
    for prefix in ["AUX_EXPECT_1", "MULT_1"] {
        let rows = diagnostics(&format!("{model} osr_params {prefix};"));
        assert_eq!(hits(&rows, "W186").len(), 1, "{rows:?}");
        assert!(hits(&rows, "E239").is_empty(), "{rows:?}");
    }
    for prefix in ["AUX_ENDO_1", "LOG_1"] {
        let rows = diagnostics(&format!("{model} osr_params {prefix};"));
        assert!(hits(&rows, "W186").is_empty(), "{rows:?}");
        assert_eq!(hits(&rows, "E239").len(), 1, "{rows:?}");
    }
    let declared = diagnostics(&format!("{model} stoch_simul y;"));
    assert!(hits(&declared, "W186").is_empty(), "{declared:?}");
}

#[test]
fn assignment_before_genuine_model_exclusion_warns_at_assignment() {
    for block in ["initval", "endval", "initval(all_values_required)"] {
        let source = format!("{BASE}{block}; y=1; x=1; end;\nmodel_remove('drop');\n");
        let rows = diagnostics(&source);
        let warning = hits(&rows, "W212");
        assert_eq!(warning.len(), 1, "{block}: {rows:?}");
        assert_eq!(warning[0].span.start, source.find("x=1").unwrap() as u32);
        assert!(hits(&rows, "E058").is_empty(), "{rows:?}");
    }
}

#[test]
fn var_remove_excluded_assignment_warns_without_false_unused_exogenous_error() {
    for declaration in ["var x;", "varexo x;", "varexo_det x;"] {
        let source = format!(
            "{declaration} var y; model; y=.5*y(-1); end; initval; y=1; x=1; end; var_remove x;"
        );
        let rows = diagnostics(&source);
        assert_eq!(hits(&rows, "W212").len(), 1, "{declaration}: {rows:?}");
        assert!(hits(&rows, "E021").is_empty(), "{declaration}: {rows:?}");
    }
}

#[test]
fn exclusion_guidance_quiets_still_used_replacement_omission_and_post_removal() {
    let used = "var y x; model; y=.5*y(-1)+x; [name='drop'] x=y; end; initval; x=1; end; model_remove('drop');";
    let replaced = format!("{BASE}initval; x=1; end; model_replace('drop'); x=2*y; end;");
    let omitted = format!("{BASE}initval; y=1; end; model_remove('drop');");
    for source in [used, &replaced, &omitted] {
        let rows = diagnostics(source);
        assert!(hits(&rows, "W212").is_empty(), "{rows:?}");
    }
    let after = format!("{BASE}model_remove('drop'); initval; x=1; end;");
    let rows = diagnostics(&after);
    assert!(hits(&rows, "W212").is_empty(), "{rows:?}");
    assert_eq!(hits(&rows, "E058").len(), 1, "{rows:?}");
}

#[test]
fn var_remove_without_assignment_is_accepted_and_fresh_unused_name_still_errors() {
    let removed = "var y; varexo x; model; y=y(-1); end; var_remove x;";
    let rows = diagnostics(removed);
    assert!(hits(&rows, "E021").is_empty(), "{rows:?}");
    assert!(hits(&rows, "W212").is_empty(), "{rows:?}");

    let fresh = format!("{removed} varexo z;");
    let rows = diagnostics(&fresh);
    assert_eq!(hits(&rows, "E021").len(), 1, "{rows:?}");
    assert!(hits(&rows, "E021")[0].message.starts_with("z not used"));
}

#[test]
fn change_type_back_to_varexo_restores_the_unused_exogenous_error() {
    let source = "var y; varexo x; model; y=y(-1); end; var_remove x; change_type(varexo) x;";
    let rows = diagnostics(source);
    let unused = hits(&rows, "E021");
    assert_eq!(unused.len(), 1, "{rows:?}");
    assert!(unused[0].message.starts_with("x not used"), "{rows:?}");
    assert!(hits(&rows, "W212").is_empty(), "{rows:?}");
}

#[test]
fn change_type_after_removal_does_not_claim_the_name_is_still_excluded() {
    let source = "var y; varexo x; model; y=y(-1); end; initval; x=1; end; var_remove x; change_type(varexo_det) x;";
    let rows = diagnostics(source);
    assert!(hits(&rows, "W212").is_empty(), "{rows:?}");
    assert!(hits(&rows, "E021").is_empty(), "{rows:?}");
}

#[test]
fn change_type_to_endogenous_is_not_an_unused_exogenous_error() {
    let source = "var y; varexo x; model; y=y(-1); end; change_type(var) x;";
    let rows = diagnostics(source);
    assert!(hits(&rows, "E021").is_empty(), "{rows:?}");
    let errors: Vec<_> = rows
        .iter()
        .filter(|row| row.severity == Severity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{rows:?}");
    assert_eq!(errors[0].code, "E186", "{rows:?}");
    assert_eq!(errors[0].message, "x not used in the model block");
    if let Some(pp) = find_preprocessor(None) {
        let check = run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(check.success, "{check:?}");
        let transform = run_preprocessor(
            source,
            &pp,
            None,
            Duration::from_secs(30),
            JsonStage::Transform,
        );
        assert!(
            transform
                .raw_stdout
                .contains("x not used in the model block"),
            "{transform:?}"
        );
    }
}

#[test]
fn change_type_to_varexo_before_the_model_is_an_unused_exogenous_error() {
    let source = "var y x; change_type(varexo) x; model; y=y(-1); end;";
    let rows = diagnostics(source);
    let unused = hits(&rows, "E021");
    assert_eq!(unused.len(), 1, "{rows:?}");
    assert!(unused[0].message.starts_with("x not used"), "{rows:?}");
    assert!(
        rows.iter()
            .filter(|row| row.severity == Severity::Error)
            .all(|row| row.code == "E021"),
        "{rows:?}"
    );
    assert!(hits(&rows, "W013").is_empty(), "{rows:?}");
    assert!(
        hits(&rows, "W020")
            .iter()
            .all(|row| !row.message.contains("'x'")),
        "{rows:?}"
    );
}

#[test]
fn equation_counts_follow_the_final_type_after_change_type() {
    // (source, E186 message, W013 fires, Transform accepts)
    for (source, unused, count_warning, accepted) in [
        (
            "var y z; change_type(parameters) z; model; y=z; end;",
            None,
            false,
            true,
        ),
        (
            "var y x; change_type(varexo) x; model; y=x; end;",
            None,
            false,
            true,
        ),
        (
            "var y; varexo e; change_type(var) e; model; y=e(-1); e=0.5*e(-1); end;",
            None,
            false,
            true,
        ),
        (
            "var y; parameters p; change_type(var) p; model; y=p; p=0.5*p(-1); end;",
            None,
            false,
            true,
        ),
        (
            "var y; parameters p; change_type(var) p; model; y=0.5*y(-1); end;",
            Some("p not used in the model block"),
            false,
            false,
        ),
        // A lagged exogenous adds an auxiliary equation and variable, so the
        // written count warns like a plain `varexo` and the count Error stays quiet.
        (
            "var y x; change_type(varexo) x; model; y=y(-1); x=x(-1); end;",
            None,
            true,
            false,
        ),
    ] {
        let rows = diagnostics(source);
        let errors: Vec<_> = rows
            .iter()
            .filter(|row| row.severity == Severity::Error)
            .collect();
        match unused {
            Some(message) => {
                assert_eq!(errors.len(), 1, "{source}: {rows:?}");
                assert_eq!(errors[0].code, "E186", "{source}: {rows:?}");
                assert_eq!(errors[0].message, message, "{source}: {rows:?}");
            }
            None => assert!(errors.is_empty(), "{source}: {rows:?}"),
        }
        assert_eq!(
            !hits(&rows, "W013").is_empty(),
            count_warning,
            "{source}: {rows:?}"
        );
        if let Some(pp) = find_preprocessor(None) {
            let official = run_preprocessor(
                source,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Transform,
            );
            assert_eq!(official.success, accepted, "{source}: {official:?}");
            if let Some(message) = unused {
                assert!(official.raw_stdout.contains(message), "{official:?}");
            }
        }
    }
}

#[test]
fn parameter_checks_follow_change_type() {
    // (source, E378 message, W010 fires, W121 fires). The type written before
    // the statement decides E378; the final type decides W010 and W121.
    for (source, not_parameter, unassigned, timed) in [
        (
            "var y z; varexo e; parameters a; change_type(parameters) z; z = 1; a = 0.5; model; y = a*y(-1) + z + e; end;",
            None,
            false,
            false,
        ),
        (
            "var y z; varexo e; parameters a; change_type(parameters) z; a = 0.5; z = 1; z.prior(shape=beta, mean=0.5, stdev=0.1); model; y = a*y(-1) + z + e; end;",
            None,
            false,
            false,
        ),
        (
            "var y z; varexo e; parameters a; change_type(parameters) z; a = 0.5; model; y = a*y(-1) + z(+1) + e; end;",
            None,
            true,
            true,
        ),
        (
            "var y; varexo e; parameters a p; change_type(var) p; a = 0.5; model; y = a*y(-1) + p(+1) + e; p = 0.9*p(-1); end;",
            None,
            false,
            false,
        ),
        (
            "var y z; varexo e; parameters a; z = 1; change_type(parameters) z; a = 0.5; model; y = a*y(-1) + z + e; end;",
            Some("z is not a parameter"),
            false,
            false,
        ),
        (
            "var y; varexo e; parameters a p; change_type(var) p; p = 1; a = 0.5; model; y = a*y(-1) + p + e; p = 0.9*p(-1); end;",
            Some("p is not a parameter"),
            false,
            false,
        ),
        (
            "var y; varexo e; parameters a p; change_type(var) p; a = 0.5; p.prior(shape=beta, mean=0.5, stdev=0.1); model; y = a*y(-1) + p + e; p = 0.9*p(-1); end;",
            Some("p is not a parameter"),
            false,
            false,
        ),
    ] {
        let rows = diagnostics(source);
        let refusals = hits(&rows, "E378");
        match not_parameter {
            Some(message) => {
                assert_eq!(refusals.len(), 1, "{source}: {rows:?}");
                assert_eq!(refusals[0].message, message, "{source}: {rows:?}");
            }
            None => assert!(refusals.is_empty(), "{source}: {rows:?}"),
        }
        assert_eq!(
            !hits(&rows, "W010").is_empty(),
            unassigned,
            "{source}: {rows:?}"
        );
        assert_eq!(
            !hits(&rows, "W121").is_empty(),
            timed,
            "{source}: {rows:?}"
        );
        if let Some(pp) = find_preprocessor(None) {
            let official =
                run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
            assert_eq!(official.success, not_parameter.is_none(), "{official:?}");
            if let Some(message) = not_parameter {
                assert!(official.raw_stdout.contains(message), "{official:?}");
            }
            assert_eq!(
                official
                    .raw_stdout
                    .contains("The following parameter(s) are used with a lead or a"),
                timed,
                "{source}: {official:?}"
            );
        }
    }
}

#[test]
fn endval_learnt_in_assignment_before_exclusion_warns() {
    let source =
        "var y; varexo x; model; y=y(-1); end; endval(learnt_in=1); x=1; end; var_remove x;";
    let rows = diagnostics(source);
    let warning = hits(&rows, "W212");
    assert_eq!(warning.len(), 1, "{rows:?}");
    assert!(warning[0].message.contains("endval"), "{rows:?}");
    assert_eq!(warning[0].span.start, source.find("x=1").unwrap() as u32);
    assert!(hits(&rows, "E021").is_empty(), "{rows:?}");
}

#[test]
fn later_declaration_after_var_remove_is_a_type_clash_not_reintroduction() {
    let source = "varexo x; var y; model; y=y(-1); end; var_remove x; varexo x;";
    let rows = diagnostics(source);
    let clash = hits(&rows, "E030");
    assert_eq!(clash.len(), 1, "{rows:?}");
    assert_eq!(
        clash[0].message,
        "Symbol x declared twice with different types!"
    );
    assert!(hits(&rows, "E021").is_empty(), "{rows:?}");

    let before = "varexo x; varexo x; var y; model; y=y(-1); end; var_remove x;";
    let rows = diagnostics(before);
    assert_eq!(hits(&rows, "W031").len(), 1, "{rows:?}");
    assert!(hits(&rows, "E030").is_empty(), "{rows:?}");
    assert!(hits(&rows, "E021").is_empty(), "{rows:?}");
}

#[test]
fn repeated_macro_spans_keep_declaration_and_removal_execution_order() {
    let source = "@#define is = 1:2\n@#for i in is\nvarexo x;\nvar_remove x;\n@#endfor\nvar y; model; y=y(-1); end;";
    let rows = diagnostics(source);
    assert_eq!(hits(&rows, "E030").len(), 1, "{rows:?}");
    assert!(hits(&rows, "W031").is_empty(), "{rows:?}");
    assert_eq!(
        hits(&rows, "E030")[0].message,
        "Symbol x declared twice with different types!"
    );
    if let Some(pp) = find_preprocessor(None) {
        let official =
            run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(!official.success, "{official:?}");
        assert!(
            official
                .raw_stdout
                .contains("Symbol x declared twice with different types!"),
            "{official:?}"
        );
    }
}

#[test]
fn repeated_macro_spans_keep_distinct_prior_initialization_warnings() {
    let source = "var y; model; y=y(-1); end;\n@#define is = 1:2\n@#for i in is\nvarexo x@{i};\ninitval; x@{i}=1; end;\nvar_remove x@{i};\n@#endfor";
    let rows = diagnostics(source);
    let warnings = hits(&rows, "W212");
    assert_eq!(warnings.len(), 2, "{rows:?}");
    for name in ["x1", "x2"] {
        assert!(
            warnings.iter().any(|row| row.message.contains(name)),
            "{rows:?}"
        );
    }
    assert!(hits(&rows, "E021").is_empty(), "{rows:?}");
    if let Some(pp) = find_preprocessor(None) {
        let official =
            run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(official.success, "{official:?}");
    }
}
