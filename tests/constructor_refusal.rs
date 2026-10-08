//! Pinned constructor refusal order, written evidence, and shared Check phase.

use std::{collections::HashMap, path::PathBuf, time::Duration};

use dygnosis::server::{new_service, Backend};
use dygnosis::{
    analyze, dynare_diagnose, parse, run_preprocessor, Diagnostic, JsonStage, Severity,
};
use tower_lsp::{lsp_types::*, LanguageServer};

fn source(term: &str) -> String {
    format!("var y; varexo e; parameters p; p=0; model; y=e+({term}); end;")
}

fn sentence(numerator: &str) -> String {
    format!("Division by zero when forming ({numerator})/(0); denominator simplified to 0 (possibly after substituting a variable set to 0).")
}

fn official(source: &str, accepted: bool, message: Option<&str>) {
    let binary = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    if !binary.is_file() {
        eprintln!("skipping constructor honesty: pinned Dynare 7.2 is absent");
        return;
    }
    let result = run_preprocessor(
        source,
        &binary,
        None,
        Duration::from_secs(30),
        JsonStage::Check,
    );
    assert_eq!(result.success, accepted, "{source}: {result:?}");
    if let Some(message) = message {
        let output = format!("{}{}", result.raw_stdout, result.raw_stderr);
        assert!(output.contains(message), "{source}: {output}");
    }
}

fn withheld(rows: &[Diagnostic]) {
    assert!(
        rows.iter()
            .all(|row| !matches!(row.code.as_str(), "E021" | "W022" | "W042")),
        "{rows:?}"
    );
}

#[test]
fn canonical_denominator_refusals_match_the_pinned_sentence_and_written_fraction() {
    for (term, fraction, numerator) in [
        ("1/0", "1/0", "1"),
        ("0/0", "0/0", "0"),
        ("0/(p-p)", "0/(p-p)", "0"),
        ("0/(p^0-1)", "0/(p^0-1)", "0"),
        ("0/(p/p-1)", "0/(p/p-1)", "0"),
        ("0*(1/(p^0-1))", "1/(p^0-1)", "1"),
        ("(p/p)/(p^0-1)", "(p/p)/(p^0-1)", "1"),
        ("1/(p/p-1.0)", "1/(p/p-1.0)", "1"),
        ("1/(1.0-1)", "1/(1.0-1)", "1"),
        ("1/(0^0-1)", "1/(0^0-1)", "1"),
        ("1/(p/(1/p)-p*p)", "1/(p/(1/p)-p*p)", "1"),
        ("(p^0+2)/(p-p)", "(p^0+2)/(p-p)", "3"),
        ("(p+2)/(p-p)", "(p+2)/(p-p)", "2+p"),
        ("(-(p+2))/(p-p)", "(-(p+2))/(p-p)", "(-(2+p))"),
        ("log(p)/(p-p)", "log(p)/(p-p)", "log(p)"),
    ] {
        let source = source(term);
        let expected = sentence(numerator);
        official(&source, false, Some(&expected));
        let model = parse(&source);
        let rows = analyze(&model);
        let errors: Vec<_> = rows.iter().filter(|row| row.code == "E278").collect();
        assert_eq!(errors.len(), 1, "{term}: {rows:?}");
        assert_eq!(errors[0].severity, Severity::Error);
        assert_eq!(errors[0].message, expected, "{term}");
        assert_eq!(
            &source[errors[0].span.start as usize..errors[0].span.end as usize],
            fraction,
            "{term}"
        );
        assert_eq!(model.equations[0].rhs, format!("e+({term})"));
        withheld(&rows);
    }
}

#[test]
fn decimal_literals_timing_names_and_assigned_parameters_are_not_canonical_zero() {
    for term in [
        "1/0.0",
        "1/0e0",
        "1/(p^0.0-1)",
        "1/(p-p+1)",
        "1/(p-p(-1))",
        "1/(p-P)",
        "1/(EXPECTATION(0)(p)-EXPECTATION(-1)(p))",
        "1/p",
    ] {
        let source = source(term).replace("parameters p;", "parameters p P; P=1;");
        official(&source, true, None);
        assert!(
            analyze(&parse(&source))
                .iter()
                .all(|row| row.code != "E278"),
            "{term}"
        );
    }
}

#[test]
fn refused_children_do_not_create_an_enclosing_constructor_refusal() {
    for term in ["0*(1/(p^0-1))", "1/(1/(p-p)-1/(p-p))", "1/(1/(p-p))"] {
        let source = source(term);
        let rows = analyze(&parse(&source));
        let expected = if term == "1/(1/(p-p)-1/(p-p))" { 2 } else { 1 };
        assert_eq!(
            rows.iter().filter(|row| row.code == "E278").count(),
            expected,
            "{term}: {rows:?}"
        );
        withheld(&rows);
    }
    for (term, code, message) in [
        ("0*log(0)", "E276", "log(0) not defined!"),
        ("0*log10(0)", "E277", "log10(0) not defined!"),
    ] {
        let source = source(term);
        official(&source, false, Some(message));
        let rows = analyze(&parse(&source));
        assert_eq!(
            rows.iter().filter(|row| row.code == code).count(),
            1,
            "{rows:?}"
        );
        withheld(&rows);
    }
}

#[test]
fn shared_expression_readers_use_the_same_literal_and_constructor_rules() {
    for template in [
        "var y; varexo e; parameters p; p=1; model; y=e+TERM; end;",
        "var y; varexo e; parameters p; p=1; model; y=e; end; steady_state_model; y=TERM; end;",
        "var y; varexo e; parameters p q; p=1; q=TERM; model; y=e; end;",
        "var y; varexo e; parameters p; p=1; model; y=e; end; shocks; var e=TERM; end;",
    ] {
        for (term, fires) in [("0/(p^0-1)", true), ("1/0.0", false)] {
            let source = template.replace("TERM", term);
            official(&source, !fires, fires.then_some(sentence("0")).as_deref());
            let rows = analyze(&parse(&source));
            assert_eq!(
                rows.iter().filter(|row| row.code == "E278").count(),
                usize::from(fires),
                "{source}: {rows:?}"
            );
            if fires {
                withheld(&rows);
            }
        }
    }
}

#[test]
fn completed_local_values_stay_in_their_parse_time_data_tree() {
    let fire = "var y; varexo e; model; #q=1; y=e+1/(q-1); end;";
    official(fire, false, Some(&sentence("1")));
    assert!(analyze(&parse(fire)).iter().any(|row| row.code == "E278"));
    let quiet = "heterogeneity_dimension d; var y; varexo e; var(heterogeneity=d) yh; varexo(heterogeneity=d) eh; model; #q=1; y=e; end; model(heterogeneity=d); #q=2; yh=eh+1/(q-1); end;";
    official(quiet, true, None);
    assert!(analyze(&parse(quiet)).iter().all(|row| row.code != "E278"));
    // A later definition or type cannot supply an earlier Parse value.
    for source in [
        "model_local_variable q; var y; model; y=1/(q-1); #q=1; end;",
        "var y; parameters q; q=1; model; y=1/(q-1); end; change_type(model_local_variable) q;",
    ] {
        assert!(parse(source).const_fold_errors.is_empty(), "{source}");
    }
}

#[test]
fn pound_binding_targets_do_not_change_constructor_order_or_written_checks() {
    for numerator in ["q+p", "p+q"] {
        let source = format!("var y; parameters p; model; #q=p; y=({numerator})/0; end;");
        let expected = sentence("p+q");
        official(&source, false, Some(&expected));
        let model = parse(&source);
        let rows = analyze(&model);
        let error = rows.iter().find(|row| row.code == "E278").unwrap();
        assert_eq!(error.message, expected);
        assert_eq!(
            &source[error.span.start as usize..error.span.end as usize],
            format!("({numerator})/0")
        );
        let target = model.equations[0].lhs_expr.unwrap();
        assert!(model
            .exprs
            .walk_idents(target)
            .any(|reference| model.name(reference.name) == "q"));
        withheld(&rows);
    }
    for (source, code, message) in [
        (
            "var y; model; #q=1; #q=2; y=q; end;",
            "E030",
            "Local model variable q declared twice.",
        ),
        (
            "var y; parameters p; model; #p=1; y=1/(p-1); end;",
            "E025",
            "p has wrong type or was already used on the right-hand side.",
        ),
        (
            "var y; model; #q=missing; y=q; end;",
            "E020",
            "Unknown symbol: missing",
        ),
    ] {
        official(source, false, Some(message));
        let model = parse(source);
        let rows = analyze(&model);
        assert!(
            rows.iter().any(|row| row.code == code),
            "{source}: {rows:?}"
        );
        assert!(model.equations[0].lhs_expr.is_some());
        assert!(
            model.const_fold_errors.is_empty(),
            "a refused binding is not a local value: {source}"
        );
    }
}

#[test]
fn refused_pound_rhs_withholds_only_its_unreached_binding_callback() {
    for source in [
        "external_function(name=fun,nargs=1);var y;model;#fun=1/0;y=0;end;",
        "var y;parameters p;model;#p=1/0;y=p;end;",
        "var y;model;#q=1;#q=1/0;y=q;end;",
        "var y;model;#fresh=1/0;y=0;end;",
    ] {
        let expected = sentence("1");
        official(source, false, Some(&expected));
        let model = parse(source);
        let rows = analyze(&model);
        let errors: Vec<_> = rows
            .iter()
            .filter(|row| row.severity == Severity::Error)
            .collect();
        assert_eq!(errors.len(), 1, "{source}: {rows:?}");
        assert_eq!(errors[0].code, "E278");
        assert_eq!(errors[0].message, expected);
        assert!(model
            .equations
            .iter()
            .filter(|row| row.is_local)
            .all(|row| row.lhs_expr.is_some()));
        if source.contains("#q=1;") {
            let q = model.intern.lookup("q").unwrap();
            assert_eq!(model.final_symbol_kind(q), Some("model_local_variable"));
            assert_eq!(
                model
                    .write_targets
                    .iter()
                    .filter(|write| write.name == q)
                    .count(),
                1
            );
        } else if source.contains("#fresh") {
            let fresh = model.intern.lookup("fresh").unwrap();
            assert_eq!(model.final_symbol_kind(fresh), None);
            assert!(model.write_targets.iter().all(|write| write.name != fresh));
        }
        withheld(&rows);
    }
    // A real RHS name read retains its own role refusal; the target creates no
    // role use, declaration, or write after that refusal.
    let source = "external_function(name=fun,nargs=1);var y;model;#keep=1;#fresh=fun;y=keep;end;";
    official(
        source,
        false,
        Some("Symbol fun is a function name external to Dynare."),
    );
    let model = parse(source);
    let rows = analyze(&model);
    let errors: Vec<_> = rows
        .iter()
        .filter(|row| row.severity == Severity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{rows:?}");
    assert_eq!(errors[0].code, "E280");
    let keep = model.intern.lookup("keep").unwrap();
    let fresh = model.intern.lookup("fresh").unwrap();
    assert_eq!(model.final_symbol_kind(keep), Some("model_local_variable"));
    assert_eq!(model.final_symbol_kind(fresh), None);
    assert_eq!(
        model
            .write_targets
            .iter()
            .filter(|write| write.name == keep)
            .count(),
        1
    );
    assert!(model.write_targets.iter().all(|write| write.name != fresh));
    let source = "external_function(name=fun,nargs=1);var y;model;#fun=1;y=0;end;";
    official(
        source,
        false,
        Some("fun has wrong type or was already used on the right-hand side."),
    );
    let rows = analyze(&parse(source));
    let errors: Vec<_> = rows
        .iter()
        .filter(|row| row.severity == Severity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{rows:?}");
    assert_eq!(errors[0].code, "E025");
}

#[test]
fn steady_constructor_order_uses_its_own_tree_across_repeated_blocks() {
    for (source, numerator) in [
        ("var y; parameters p q; p=p; model; y=0; end; steady_state_model; y=(q+p)/0; end;", "q+p"),
        ("var y; parameters p q; p=p; model; y=0; end; steady_state_model; y=p; end; steady_state_model; y=(q+p)/0; end;", "p+q"),
    ] {
        let expected = sentence(numerator);
        official(source, false, Some(&expected));
        let rows = analyze(&parse(source));
        let errors: Vec<_> = rows.iter().filter(|row| row.code == "E278").collect();
        assert_eq!(errors.len(), 1, "{source}: {rows:?}");
        assert_eq!(errors[0].message, expected);
        assert_eq!(&source[errors[0].span.start as usize..errors[0].span.end as usize], "(q+p)/0");
        withheld(&rows);
    }
}

#[test]
fn planner_constructor_scope_and_eager_local_refusal_match_the_pin() {
    let source = "var y; parameters p q; model; y=p; end; planner_objective (q+p)/0; ramsey_model;";
    let expected = sentence("q+p");
    official(source, false, Some(&expected));
    let rows = analyze(&parse(source));
    assert_eq!(
        rows.iter()
            .find(|row| row.code == "E278")
            .unwrap_or_else(|| panic!("{source}: {rows:?}"))
            .message,
        expected
    );
    for (prefix, term) in [
        ("model; #q=1; y=0; end;", "1/(q-1)"),
        ("model; #q=1; y=0; end;", "1/(q-q)"),
        ("model_local_variable q; model; y=0; end;", "q/0"),
    ] {
        let source = format!("var y; {prefix} planner_objective {term}; ramsey_model;");
        official(
            &source,
            false,
            Some("Model local variable q cannot be used in 'planner_objective'."),
        );
        let rows = analyze(&parse(&source));
        let errors: Vec<_> = rows
            .iter()
            .filter(|row| row.severity == Severity::Error)
            .collect();
        assert_eq!(errors.len(), 1, "{source}: {rows:?}");
        assert_eq!(errors[0].code, "E253");
        withheld(&rows);
    }
}

#[test]
fn each_separate_official_tree_keeps_its_own_constructor_order() {
    for source in [
        "var y; parameters p q; model; y=0; end; planner_objective p; planner_objective (q+p)/0;",
        "var y; parameters p q; model; y=p+q; end; epilogue; x=(q+p)/0; end;",
        "var y; parameters p q; model; [name='Y',bind='C'] y=p+q; end; occbin_constraints; name 'C'; bind (q+p)/0<y; end;",
    ] {
        let expected = sentence("q+p");
        official(source, false, Some(&expected));
        let rows = analyze(&parse(source));
        assert_eq!(rows.iter().find(|row| row.code == "E278").unwrap_or_else(|| panic!("{source}: {rows:?}")).message, expected, "{source}: {rows:?}");
    }
}

#[test]
fn repeated_heterogeneous_blocks_share_the_dimension_constructor_and_local_scope() {
    let prefix = "heterogeneity_dimension d; var y; var(heterogeneity=d) yh; parameters p q; p=1; q=2; model; y=0; end;";
    for (first, second, numerator) in [
        ("#loc=1; yh=0;", "yh=1/(loc-1);", "1"),
        ("yh=p;", "yh=(q+p)/0;", "p+q"),
    ] {
        let source = format!(
            "{prefix}model(heterogeneity=d);{first}end;model(heterogeneity=d);{second}end;"
        );
        let expected = sentence(numerator);
        official(&source, false, Some(&expected));
        let rows = analyze(&parse(&source));
        let errors: Vec<_> = rows.iter().filter(|row| row.code == "E278").collect();
        assert_eq!(errors.len(), 1, "{source}: {rows:?}");
        assert_eq!(errors[0].message, expected);
    }
    let source = format!(
        "{prefix}model(heterogeneity=d);#loc=1;yh=0;end;model(heterogeneity=d);#loc=2;yh=loc;end;"
    );
    official(
        &source,
        false,
        Some("Local model variable loc declared twice."),
    );
    assert!(analyze(&parse(&source))
        .iter()
        .any(|row| row.code == "E030"));
    let source = "heterogeneity_dimension d; var y; var(heterogeneity=d) yh; parameters p; p=1; model; y=0; end; model(heterogeneity=d); #loc=p; yh=0; end; model(heterogeneity=d); yh=loc; end;";
    official(source, true, None);
    assert!(
        analyze(&parse(source)).iter().all(|row| row.code != "W022"),
        "{source}"
    );
}

#[test]
fn synthetic_steady_targets_do_not_enter_constructor_or_rhs_role_evidence() {
    let source = "external_function(name=fun,nargs=1); var y; model; y=0; end; steady_state_model; fun=1; end;";
    official(source, false, Some("fun has incorrect type"));
    let model = parse(source);
    let rows = analyze(&model);
    let errors: Vec<_> = rows
        .iter()
        .filter(|row| row.severity == Severity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{rows:?}");
    assert_eq!(errors[0].code, "E481");
    assert!(model
        .exprs
        .walk_idents(model.steady_state_equations[0].lhs_expr.unwrap())
        .next()
        .is_some());
    let source = "var y; parameters p q; model; y=0; end; steady_state_model; y=0; end; q=(p+y)/0;";
    let expected = sentence("p+y");
    official(source, false, Some(&expected));
    assert_eq!(
        analyze(&parse(source))
            .iter()
            .find(|row| row.code == "E278")
            .unwrap()
            .message,
        expected
    );
}

#[test]
fn known_ordinary_roles_stop_construction_while_unknown_model_names_are_deferred() {
    for source in [
        "var y; parameters p; model; y=0; end; epilogue; epi=y; end; p=1/(epi-epi);",
        "heterogeneity_dimension d; var y; parameters(heterogeneity=d) hp; parameters p; model; y=0; end; p=1/(hp-hp);",
    ] {
        official(source, false, None);
        let model = parse(source);
        assert!(model.const_fold_errors.is_empty(), "{source}: {:?}", model.const_fold_errors);
        let rows = analyze(&model);
        assert!(rows.iter().any(|row| matches!(row.code.as_str(), "E294" | "E463")), "{rows:?}");
    }
    let source = "var y; model; y=unknown/0; end;";
    let expected = sentence("unknown");
    official(source, false, Some(&expected));
    let rows = analyze(&parse(source));
    assert_eq!(
        rows.iter().find(|row| row.code == "E278").unwrap().message,
        expected
    );
    assert!(rows.iter().all(|row| row.code != "E020"), "{rows:?}");
    let completed = "var y; model; y=0*missing; end;";
    official(completed, false, Some("Unknown symbol: missing"));
    assert!(analyze(&parse(completed))
        .iter()
        .any(|row| row.code == "E020"));
    let distinct = "var y; model; y=0*missing; end; model; y=1/0; end;";
    let rows = analyze(&parse(distinct));
    assert!(rows.iter().any(|row| row.code == "E020"), "{rows:?}");
    assert!(rows.iter().any(|row| row.code == "E278"), "{rows:?}");
}

#[test]
fn nonmodel_reader_name_type_namespace_and_arity_refusals_keep_their_reach() {
    for template in [
        "var y; varexo e; parameters p a; p=1; model; y=e; end; a=TERM;",
        "var y; varexo e; parameters p; p=1; model; y=e; end; steady_state_model; y=TERM; end;",
        "var y; varexo e; parameters p; p=1; model; y=e; end; shocks; var e=TERM; end;",
    ] {
        for (prefix, term, code) in [
            ("", "pp.p", "E275"),
            ("external_function(name=fun,nargs=1);", "fun", "E279"),
            ("model_local_variable loc;", "loc", "E282"),
            ("trend_var(growth_factor=1.02) t;", "t", "E310"),
            ("", "sin(1,2)", "E001"),
        ] {
            let source = format!(
                "{prefix}{}",
                template.replace("TERM", &format!("0*({term})"))
            );
            official(&source, false, None);
            let rows = analyze(&parse(&source));
            assert!(
                rows.iter().any(|row| row.code == code),
                "{code}: {source}: {rows:?}"
            );
            withheld(&rows);
        }
    }
}

#[test]
fn builtin_arity_uses_the_pinned_separator_and_keeps_earlier_child_refusals() {
    for term in ["sin(1,2)", "max(1,2,3)", "normcdf(1,0,1,2)"] {
        let source = source(term);
        let message = "syntax error, unexpected COMMA";
        official(&source, false, Some(message));
        let rows = analyze(&parse(&source));
        let errors: Vec<_> = rows.iter().filter(|row| row.code == "E001").collect();
        assert_eq!(errors.len(), 1, "{term}: {rows:?}");
        assert_eq!(errors[0].message, message);
        assert_eq!(
            &source[errors[0].span.start as usize..errors[0].span.end as usize],
            ","
        );
    }
    for term in [
        "sin(1)",
        "max(1,2)",
        "normcdf(1)",
        "normcdf(1,0,1)",
        "normpdf(1)",
        "normpdf(1,0,1)",
    ] {
        let source = source(term);
        official(&source, true, None);
        assert!(
            analyze(&parse(&source))
                .iter()
                .all(|row| row.code != "E001"),
            "{term}"
        );
    }
    for (prefix, term, code) in [
        ("", "sin(1/0,2)", "E278"),
        ("", "sin(missing,2)", "E001"),
        ("external_function(name=fun,nargs=1);", "sin(fun,2)", "E280"),
        ("", "sin(1,1/0)", "E001"),
    ] {
        let source = format!("{prefix}{}", source(term));
        official(&source, false, None);
        let errors: Vec<_> = analyze(&parse(&source))
            .into_iter()
            .filter(|row| row.severity == Severity::Error)
            .collect();
        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
        assert_eq!(errors[0].code, code, "{source}: {errors:?}");
    }
}

#[test]
fn generic_refusals_in_discarded_terms_withhold_checks_across_the_unit() {
    for (prefix, term, code) in [
        ("", "missing", "E020"),
        ("external_function(name=fun,nargs=1);", "fun", "E280"),
        ("parameters q; q=local;", "local", "E281"),
        ("var gone; var_remove gone;", "gone", "E426"),
        ("varexo_det ed;", "ed(-1)", "E024"),
        ("", "p(1,2)", "E001"),
        ("", "fun(y)", "E001"),
        ("external_function(name=fun,nargs=2);", "fun(y)", "E001"),
    ] {
        let source = format!("{prefix}var y z; varexo e; parameters p; p=1; model; y=0*e; z=y; end; steady_state_model; y=1; end; model; y=0*({term}); end;");
        official(&source, false, None);
        let rows = analyze(&parse(&source));
        assert!(
            rows.iter().any(|row| row.code == code),
            "{source}: {rows:?}"
        );
        withheld(&rows);
    }
    let source = "var y z; varexo e; parameters p; p=1; model; #q=0; #q=1; y=0*e; z=y; end; steady_state_model; y=1; end;";
    official(source, false, None);
    let rows = analyze(&parse(source));
    assert!(rows.iter().any(|row| row.code == "E030"), "{rows:?}");
    withheld(&rows);
    let valid = "var y z; varexo e; parameters p; p=1; model; y=0*e; z=y; end; steady_state_model; y=1; end;";
    let rows = analyze(&parse(valid));
    for code in ["E021", "W022", "W042"] {
        assert!(rows.iter().any(|row| row.code == code), "{code}: {rows:?}");
    }
}

#[test]
fn constructor_parse_phase_blocks_prior_checks_and_keeps_later_stage_warning_controls() {
    let declarations = "var y z; varexo e; parameters p; p=1;";
    let body = "model; y=0*e; z=y; end; steady_state_model; y=1; end;";
    for tail in [format!("p=0/(p^0-1);{body}"), format!("{body}p=0/(p^0-1);")] {
        let source = format!("{declarations}{tail}");
        official(&source, false, Some(&sentence("0")));
        let rows = analyze(&parse(&source));
        assert_eq!(
            rows.iter().filter(|row| row.code == "E278").count(),
            1,
            "{rows:?}"
        );
        withheld(&rows);
    }
    for (extra_declarations, tail, code) in [
        (
            "",
            "shocks(surprise); var e; periods 1; values 1; end;",
            "E178",
        ),
        ("varexo_det d;", "d.subsamples(s=2000Q1:2000Q2);", "E431"),
    ] {
        let source = format!("{declarations}{extra_declarations}model; y=e+1/0.0; z=y; end; steady_state_model; y=1; end;{tail}");
        official(&source, true, None);
        let rows = analyze(&parse(&source));
        for code in [code, "W022", "W042"] {
            assert!(
                rows.iter().any(|row| row.code == code),
                "{source}: {rows:?}"
            );
        }
    }
}

#[test]
fn macro_and_include_refusals_keep_written_multiplicity_and_literal_spelling() {
    let root = "C:/constructor/main.mod";
    let child = "C:/constructor/body.inc";
    let root_text = "var y; varexo e; parameters p; p=1; model;\n@#include \"body.inc\"\nend;";
    let body = "@#for n in 1:2\ny=e+(p/p)/(p^0-1);\n@#endfor\n";
    let files = HashMap::from([(root.into(), root_text.into()), (child.into(), body.into())]);
    let rows = dynare_diagnose(root_text, Some(root), Some(&files));
    let errors: Vec<_> = rows.iter().filter(|row| row.code == "E278").collect();
    assert_eq!(errors.len(), 2, "{rows:?}");
    for error in errors {
        assert_eq!(error.file.as_deref(), Some(child));
        assert_eq!(error.line, 2);
        assert_eq!(error.column, 5);
        assert_eq!(error.end_column, 18);
        assert_eq!(error.message, sentence("1"));
    }
    for (literal, fires) in [("0", true), ("0.0", false)] {
        let source = format!("@#define denominator = \"{literal}\"\nvar y; varexo e; model; y=e+1/@{{denominator}}; end;\n");
        official(&source, !fires, fires.then_some(sentence("1")).as_deref());
        let rows = analyze(&parse(&source));
        assert_eq!(
            rows.iter().filter(|row| row.code == "E278").count(),
            usize::from(fires),
            "{source}: {rows:?}"
        );
    }
    let root_text = "external_function(name=fun,nargs=1); var y; varexo e; model; y=e; end;\n@#include \"body.inc\"\n";
    let body = "@#for n in 1:2\nshocks; var e=0*fun; end;\n@#endfor\n";
    let files = HashMap::from([(root.into(), root_text.into()), (child.into(), body.into())]);
    let rows = dynare_diagnose(root_text, Some(root), Some(&files));
    let errors: Vec<_> = rows.iter().filter(|row| row.code == "E279").collect();
    assert_eq!(errors.len(), 2, "{rows:?}");
    for error in errors {
        assert_eq!(error.file.as_deref(), Some(child));
        assert_eq!(error.line, 2);
        assert_eq!(error.column, 17);
        assert_eq!(error.end_column, 20);
    }
}

async fn pull(server: &Backend, uri: &Url) -> Vec<tower_lsp::lsp_types::Diagnostic> {
    match server
        .diagnostic(DocumentDiagnosticParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            identifier: None,
            previous_result_id: None,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
    {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
            full.full_document_diagnostic_report.items
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn lsp_unsaved_include_edits_fire_then_clear_the_constructor_refusal() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let root = Url::parse("file:///C:/constructor-live/main.mod").unwrap();
    let child = Url::parse("file:///C:/constructor-live/body.inc").unwrap();
    let root_text = "var y; varexo e; parameters p; p=1; model;\n@#include \"body.inc\"\nend;";
    for (uri, text) in [(&child, "y=e+0*(1/(p^0-1));"), (&root, root_text)] {
        server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "dynare".into(),
                    version: 1,
                    text: text.into(),
                },
            })
            .await;
    }
    let rows = pull(server, &child).await;
    let errors: Vec<_> = rows
        .iter()
        .filter(|row| row.code == Some(NumberOrString::String("E278".into())))
        .collect();
    assert_eq!(errors.len(), 1, "{rows:?}");
    assert_eq!(errors[0].message, sentence("1"));
    assert_eq!(errors[0].severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(
        errors[0].range,
        Range::new(Position::new(0, 7), Position::new(0, 16))
    );
    server
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: child.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "y=e+1/0.0;".into(),
            }],
        })
        .await;
    let rows = pull(server, &child).await;
    assert!(
        rows.iter()
            .all(|row| row.code != Some(NumberOrString::String("E278".into()))),
        "{rows:?}"
    );
}
