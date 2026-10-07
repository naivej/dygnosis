use std::{collections::HashMap, path::PathBuf, time::Duration};

use dygnosis::{
    analyze, check_w021, check_w022, dynare_diagnose, find_preprocessor, parse, run_preprocessor,
    JsonStage, Severity,
};

fn official(source: &str, accepted: bool, warning: Option<&str>) {
    let pinned = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    let binary = pinned.is_file().then_some(pinned).or_else(|| {
        find_preprocessor(None)
            .filter(|path| path.components().any(|part| part.as_os_str() == "7.2"))
    });
    let Some(binary) = binary else {
        return;
    };
    let result = run_preprocessor(
        source,
        &binary,
        None,
        Duration::from_secs(30),
        JsonStage::Check,
    );
    assert_eq!(result.success, accepted, "{source}: {result:?}");
    let output = format!("{}{}", result.raw_stdout, result.raw_stderr);
    if let Some(name) = warning {
        assert!(
            output.contains("WARNING: Parameter(s)") && output.contains(&format!("{name} ")),
            "{source}: {output}"
        );
    } else if accepted {
        assert!(
            !output.contains("WARNING: Parameter(s)"),
            "{source}: {output}"
        );
    }
}

fn parameter_source(term: &str) -> String {
    format!("var y; varexo e; parameters p; p=2; model; y=e+({term}); end;")
}

#[test]
fn usage_parse_constructor_cancellations_match_pinned_dynare() {
    for term in [
        "0*p",
        "p*0",
        "p-p",
        "p/p",
        "p^0",
        "1^p",
        "0/p",
        "(p+1)-p",
        "(1+p)-p",
        "(2*p)/p",
        "(p*2)/p",
        "(2/p)*p",
        "p*(2/p)",
        "(1-p)+p",
        "p+(1-p)",
        "p+(-p)",
        "(-p)+p",
        "p-(-(-p))",
        "0^p",
        "0^0+p-p",
        "(0+0)*p",
        "(0.1+0.2-0.3)*p",
        "sin(0)*p",
        "log(1)*p",
        "(exp(0)-1)*p",
        "min(0,1)*p",
        "erf(0)*p",
        "(erfc(0)-1)*p",
        "(normcdf(0)-0.5)*p",
        "(normcdf(0,0,1)-0.5)*p",
        "(normpdf(0,0,1)-normpdf(0))*p",
    ] {
        let source = parameter_source(term);
        official(&source, true, Some("p"));
        let model = parse(&source);
        let rows = check_w022(&model);
        assert_eq!(rows.len(), 1, "{term}: {rows:?}");
        assert_eq!(rows[0].message, "Parameter(s) p not used in the model");
        assert_eq!(rows[0].severity, Severity::Warning);
        assert_eq!(
            &source[rows[0].span.start as usize..rows[0].span.end as usize],
            "p"
        );
        // Written names stay available for navigation and W020 guidance.
        assert!(
            model
                .ident_refs(&model.equations[0])
                .iter()
                .any(|r| model.name(r.name) == "p"),
            "{term}"
        );
    }
}

#[test]
fn usage_keeps_written_identities_that_dynare_does_not_remove() {
    for term in [
        "(p-1)+1",
        "(p/2)*2",
        "p^1",
        "0.0*p",
        "p*0e0",
        "1.0^p",
        "p^0.0",
        "p-p(-1)",
        "p/p(-1)",
        "EXPECTATION(0)(p)-EXPECTATION(-1)(p)",
        "exp(p)-exp(p(-1))",
        "exp(p)-log(p)",
        "max(p,1)-max(1,p)",
    ] {
        let source = parameter_source(term);
        official(&source, true, None);
        assert!(check_w022(&parse(&source)).is_empty(), "{term}");
    }
    // Assigned parameters are not Parse constants.
    let source = "var y; varexo e; parameters p q; p=0; q=1; model; y=e+p*q; end;";
    official(source, true, None);
    assert!(check_w022(&parse(source)).is_empty());
    for term in ["LOG(1/e)+ln(e)", "NORMCDF(e)-normcdf(e,0,1)"] {
        let source = format!("var y; varexo e; model; y={term}; end;");
        official(&source, false, None);
        assert_eq!(check_w021(&parse(&source)).len(), 1, "{source}");
    }
}

#[test]
fn usage_builtin_tokens_and_parse_rewrites_match_dynare() {
    for term in [
        "log(1/p)+log(p)",
        "log10(1/p)+log10(p)",
        "ln(1/p)+LOG(p)",
        "log(p)-LOG(p)",
        "ln(p)-log(p)",
        "LN(p)-LOG(p)",
        "normcdf(p)-normcdf(p,0,1)",
        "normpdf(p)-normpdf(p,0,1)",
        "NORMCDF(p)-normcdf(p,0,1)",
        "NORMPDF(p)-normpdf(p,0,1)",
        "(EXP(0)-1)*p",
        "SIN(0)*p",
        "(ERFC(0)-1)*p",
        "(NORMCDF(0)-0.5)*p",
        "(NORMPDF(0)-normpdf(0,0,1))*p",
        "log(1/log(1/p))+log(-log(p))",
    ] {
        let source = parameter_source(term);
        official(&source, true, Some("p"));
        let model = parse(&source);
        assert_eq!(check_w022(&model).len(), 1, "{source}");
        assert!(
            model.equations[0].rhs.replace(' ', "").contains(term),
            "written RHS changed: {source}"
        );
        assert!(model
            .ident_refs(&model.equations[0])
            .iter()
            .any(|reference| model.name(reference.name) == "p"));
    }
    for term in [
        "log(1.0/p)+log(p)",
        "log10(2/p)+log10(p)",
        "normcdf(p)-normcdf(p,0,2)",
        "normpdf(p)-normpdf(p,1,1)",
        "normcdf(p)-normcdf(p,0.0,1)",
        "LOG(p)-LN(p(-1))",
    ] {
        let source = parameter_source(term);
        official(&source, true, None);
        assert!(check_w022(&parse(&source)).is_empty(), "{source}");
    }
    // Generic identifiers and external functions retain case-sensitive names.
    let source = "external_function(name=fun,nargs=1); external_function(name=FUN,nargs=1); var y; varexo e; parameters p; p=2; model; y=e+fun(p)-FUN(p); end;";
    official(source, true, None);
    assert!(check_w022(&parse(source)).is_empty());
    let source = "var y; varexo e; parameters p P; p=2; P=3; model; y=e+LOG(p)-log(P); end;";
    official(source, true, None);
    assert!(check_w022(&parse(source)).is_empty());
}

#[test]
fn usage_constant_spelling_survives_macro_copy_origins() {
    let source = "var y; varexo e; parameters p; p=2;\n@#define terms = [\"0\", \"0.0\"]\nmodel;\n@#for term in terms\ny=e+@{term}*p;\n@#endfor\nend;\n";
    official(source, true, None);
    assert!(check_w022(&parse(source)).is_empty());
}

#[test]
fn usage_follows_only_live_model_local_definitions() {
    for (definitions, term, used) in [
        ("#q=p;", "0", false),
        ("#q=p;", "0*q", false),
        ("#q=p;", "q", true),
        ("#q=p; #r=q;", "r", true),
        ("#q=p; #r=q;", "r-r", false),
        ("#q=0*p; #r=q;", "r", false),
        ("#q=p; #r=0*q;", "r", false),
        ("#q=1; #r=q-q;", "r*p", true),
        ("#q=0;", "q*p", true),
    ] {
        let source =
            format!("var y; varexo e; parameters p; p=2; model; {definitions} y=e+{term}; end;");
        official(&source, true, (!used).then_some("p"));
        assert_eq!(check_w022(&parse(&source)).is_empty(), used, "{source}");
    }
}

#[test]
fn usage_keeps_model_local_definitions_in_their_data_tree() {
    for (aggregate, heterogeneous, used) in [
        ("#loc=1; y=e+loc;", "#loc=p; yh=eh+0*loc;", false),
        ("#loc=p; y=e+0*loc;", "#loc=1; yh=eh+loc;", false),
        ("#loc=p; y=e+loc;", "#loc=1; yh=eh+loc;", true),
        ("#loc=1; y=e+loc;", "#loc=p; yh=eh+loc;", true),
    ] {
        let source = format!("heterogeneity_dimension d; var y; varexo e; var(heterogeneity=d) yh; varexo(heterogeneity=d) eh; parameters p; p=1; model; {aggregate} end; model(heterogeneity=d); {heterogeneous} end;");
        official(&source, true, (!used).then_some("p"));
        assert_eq!(check_w022(&parse(&source)).is_empty(), used, "{source}");
    }
}

#[test]
fn usage_w022_roots_match_official_consumers() {
    for source in [
        "var y; parameters p q; p=1; q=p; model; y=q; end;",
        "var y; parameters p; p=1; model; y=0; end; initval; y=p; end;",
        "var y; varexo e; parameters p; p=1; model; y=e; end; shocks; var e; stderr p; end;",
    ] {
        official(source, true, Some("p"));
        let rows = check_w022(&parse(source));
        assert_eq!(rows.len(), 1, "{source}: {rows:?}");
        assert_eq!(rows[0].message, "Parameter(s) p not used in the model");
    }
    for source in ["var y; parameters p; p=1; model; y=p; end;", "var y; parameters p; p=1; model; [static] y=p; [dynamic] y=0; end;", "var y; parameters p; p=1; model; y=0; end; steady_state_model; y=p; end;", "var y; parameters p; model; y=0; end; steady_state_model; p=1; y=0; end;", "var y; parameters p; model; y=0; end; steady_state_model; [y,p]=helper(1); end;", "heterogeneity_dimension d; var(heterogeneity=d) yh; parameters p; p=1; model(heterogeneity=d); yh=p; end;"] {
        official(source, true, None);
        assert!(check_w022(&parse(source)).is_empty(), "{source}");
    }
    let source = "parameters p; p=1;";
    official(source, true, Some("p"));
    assert_eq!(check_w022(&parse(source)).len(), 1);
}

#[test]
fn usage_exogenous_simplifications_and_exemptions_match_dynare() {
    for term in ["0*e", "e*0", "e-e", "e/e", "e^0", "0/e", "(e+1)-e"] {
        let source = format!("var y; varexo e; model; y={term}; end;");
        official(&source, false, None);
        let rows = check_w021(&parse(&source));
        assert_eq!(rows.len(), 1, "{term}: {rows:?}");
        assert_eq!(rows[0].code, "E021");
        assert_eq!(rows[0].severity, Severity::Error);
        assert_eq!(rows[0].message, "e not used in model block. To bypass this error, use the `nostrict` option. This may lead to crashes or unexpected behavior.");
        assert_eq!(
            &source[rows[0].span.start as usize..rows[0].span.end as usize],
            "e"
        );
    }
    for source in ["var y; varexo e; varexobs e; model; y=0*e; end;", "var y; varexo_det e; model; y=0*e; end;", "var y; varexo e; model; [static] y=e; [dynamic] y=0; end;", "var y; varexo e; var_remove e; model; y=0; end;", "var y e; change_type(varexo) e; varexobs e; model; y=0; end;", "var y z; varexo e e2; parameters b; b=0.8; model; [name='Y'] y=b*y(-1)+e; [name='Z'] z=z(-1)+e2; end; pac_model(model_name=q,discount=b,growth=ghost);"] {
        official(source, true, None);
        assert!(!analyze(&parse(source)).iter().any(|row| row.code == "E021"), "{source}");
    }
}

#[test]
fn usage_discarded_terms_keep_generic_parse_reach() {
    for (prefix, term, code, needle) in [
        ("", "missing", "E020", "missing"),
        (
            "external_function(name=fun,nargs=1);",
            "fun",
            "E280",
            "function name",
        ),
        (
            "parameters p; p=local;",
            "local",
            "E281",
            "scope is only outside",
        ),
        ("var gone; var_remove gone;", "gone", "E426", "excluded"),
        ("varexo_det ed;", "ed(-1)", "E024", "lead or a lag"),
        (
            "parameters p; p=1;",
            "p(1,2)",
            "E001",
            "given several arguments",
        ),
        ("", "fun(y)", "E001", "first declare"),
        (
            "external_function(name=fun,nargs=2);",
            "fun(y)",
            "E001",
            "number of arguments",
        ),
    ] {
        let source = format!("{prefix}var y; model; y=0*({term}); end;");
        official(&source, false, None);
        assert!(
            analyze(&parse(&source))
                .iter()
                .any(|row| row.code == code && row.message.contains(needle)),
            "{source}: {:?}",
            analyze(&parse(&source))
        );
    }
    let duplicate = "var y; model; #q=0; #q=1; y=0*q; end;";
    official(duplicate, false, None);
    assert!(analyze(&parse(duplicate))
        .iter()
        .any(|row| row.code == "E030"));
    let zero_denominator = "var y; parameters p; p=1; model; y=0/(p-p); end;";
    official(zero_denominator, false, None);
    assert!(analyze(&parse(zero_denominator))
        .iter()
        .any(|row| row.code == "E278"));
}

#[test]
fn usage_written_references_and_include_owners_stay_available() {
    let root = "C:/usage-check/main.mod";
    let child = "C:/usage-check/parameters.inc";
    let source = "var y; varexo e;\n@#include \"parameters.inc\"\nmodel; y=e+0*p; end;";
    let files = HashMap::from([
        (root.to_string(), source.to_string()),
        (child.to_string(), "parameters p; p=1;".to_string()),
    ]);
    let rows = dynare_diagnose(source, Some(root), Some(&files));
    let warning = rows
        .iter()
        .find(|row| row.code == "W022")
        .expect("included unused parameter");
    assert_eq!(warning.file.as_deref(), Some(child));
    assert_eq!(warning.column, 12);
    let source = parameter_source("0*p");
    let model = parse(&source);
    assert_eq!(model.equations[0].rhs, "e+(0*p)");
    assert!(model
        .ident_refs(&model.equations[0])
        .iter()
        .any(|reference| model.name(reference.name) == "p"));
}
