use std::collections::HashMap;

use dygnosis::{analyze, check_e030, check_file_with_origins, dynare_diagnose, parse};

fn first_related(text: &str, code: &str) -> dygnosis::diagnostic::RelatedDiagnostic {
    let rows = analyze(&parse(text));
    rows.iter()
        .find(|row| row.code == code)
        .unwrap_or_else(|| panic!("{code}: {rows:?}"))
        .related
        .first()
        .unwrap_or_else(|| panic!("missing earlier {code}"))
        .clone()
}

#[test]
fn declaration_taxonomy_retains_the_first_occurrence_through_retyping() {
    let ordinary = ["var", "varexo", "varexo_det", "parameters"];
    for first in ordinary {
        for second in ordinary {
            let text = format!("{first} foo; {second} foo;");
            let rows = check_e030(&parse(&text));
            let code = if first == second { "W031" } else { "E030" };
            let row = rows.iter().find(|row| row.code == code).unwrap();
            assert_eq!(
                row.related[0].span.start as usize,
                text.find("foo").unwrap(),
                "{text}"
            );
        }
    }
    for (first, second, code) in [
        (
            "trend_var(growth_factor=1) foo;",
            "trend_var(growth_factor=1) foo;",
            "W031",
        ),
        (
            "log_trend_var(log_growth_factor=1) foo;",
            "log_trend_var(log_growth_factor=1) foo;",
            "W031",
        ),
        (
            "trend_var(growth_factor=1) foo;",
            "log_trend_var(log_growth_factor=1) foo;",
            "E030",
        ),
        ("trend_var(growth_factor=1) foo;", "var foo;", "E030"),
        ("epilogue; foo=1; end;", "epilogue; foo=2; end;", "W031"),
        ("var foo;", "epilogue; foo=1; end;", "E030"),
        (
            "model_local_variable foo;",
            "model_local_variable foo;",
            "W031",
        ),
        ("model_local_variable foo;", "var foo;", "E030"),
        ("external_function(name=foo,nargs=1);", "var foo;", "E030"),
        (
            "external_function(name=bar,nargs=1,first_deriv_provided=foo);",
            "var foo;",
            "E030",
        ),
        (
            "external_function(name=bar,nargs=1,first_deriv_provided,second_deriv_provided=foo);",
            "var foo;",
            "E030",
        ),
        (
            "external_function(name=foo,nargs=1);",
            "external_function(name=foo,nargs=1);",
            "W031",
        ),
        ("parameters p; p=foo;", "var foo;", "E030"),
        ("parameters p; p=foo(1);", "var foo;", "E030"),
        (
            "parameters p; p=foo(1);",
            "external_function(name=foo,nargs=1);",
            "W031",
        ),
        (
            "var foo; change_type(parameters) foo;",
            "parameters foo;",
            "W031",
        ),
        ("varexo foo; var_remove foo;", "varexo foo;", "E030"),
        (
            "heterogeneity_dimension hh ff; var(heterogeneity=hh) foo;",
            "var(heterogeneity=ff) foo;",
            "W031",
        ),
        (
            "heterogeneity_dimension hh; var(heterogeneity=hh) foo;",
            "varexo foo;",
            "E030",
        ),
    ] {
        let text = format!("{first} {second}");
        let rows = check_e030(&parse(&text));
        let row = rows
            .iter()
            .find(|row| row.code == code)
            .unwrap_or_else(|| panic!("{text}: {rows:?}"));
        assert_eq!(
            row.related[0].span.start as usize,
            first.find("foo").unwrap(),
            "{text}"
        );
    }
    let text = "var foo; var foo; var foo;";
    assert!(check_e030(&parse(text))
        .iter()
        .all(|row| row.related[0].span.start == 4));
    for option in ["first_deriv_provided", "second_deriv_provided"] {
        let text = format!("external_function(name=leftfn,nargs=1,{option}=foo); external_function(name=rightfn,nargs=1,{option}=foo);");
        let rows = check_e030(&parse(&text));
        let row = rows.iter().find(|row| row.code == "W031").unwrap();
        assert_eq!(
            row.related[0].span.start as usize,
            text.find("foo").unwrap()
        );
    }
    let text = "var y pol; model(linear); y=0; end; planner_objective y^2; discretionary_policy(instruments=(pol)); parameters optimal_policy_discount_factor;";
    let related = first_related(text, "W031");
    assert_eq!(
        related.span.start as usize,
        text.find("discretionary_policy").unwrap()
    );
}

#[test]
fn equation_local_and_regime_links_keep_the_actual_earlier_row() {
    for body in [
        "model; #a=1; #a=2; y=a; end;",
        "model(heterogeneity=hh); #a=1; #a=2; h=a; end;",
    ] {
        let text = format!("heterogeneity_dimension hh; var y; var(heterogeneity=hh) h; {body}");
        let related = first_related(&text, "E030");
        assert_eq!(related.span.start as usize, text.find("a=1").unwrap());
    }
    let text = "var y; model; y=0; y=0; end;";
    assert_eq!(
        first_related(text, "W054").span.start as usize,
        text.find("y=0").unwrap()
    );
    let text = include_str!("fixtures/occbin/e177_regime_dup.mod").replace("\r\n", "\n");
    let related = first_related(&text, "E177");
    assert_eq!(
        related.span.start as usize,
        text.find("[name='policy', bind='ELB']").unwrap()
    );
    assert_ne!(
        related.span.start as usize,
        text.find("[name='policy', relax='ELB']").unwrap()
    );
    let text="var y; model; [name='x',bind='C1'] y=0; [name='x',bind='C1,C2'] y=1; end; occbin_constraints; name 'C1'; bind y<0; relax y>0; name 'C2'; bind y<1; relax y>1; end;";
    assert_eq!(
        first_related(text, "E177").span.start as usize,
        text.find("[name='x',bind='C1']").unwrap()
    );
}

#[test]
fn every_estimated_block_and_entry_kind_links_first_and_keeps_scope() {
    let prefix = "var y; varexo e f; parameters rho; rho=.5; model; y=rho*y(-1)+e+f; end;";
    for block in [
        "estimated_params",
        "estimated_params_init",
        "estimated_params_bounds",
    ] {
        for (head, code) in [
            ("rho", "E244"),
            ("stderr e", "E245"),
            ("corr e,f", "E246"),
            ("skew e", "E247"),
        ] {
            let values = match block {
                "estimated_params_init" => ",.1",
                "estimated_params_bounds" => ",0,1",
                _ => "",
            };
            let row = format!("{head}{values};");
            let text = format!("{prefix} {block}; {row} {row} {row} end;");
            let first =
                prefix.len() + block.len() + 3 + head.find(' ').map_or(0, |index| index + 1);
            let rows = analyze(&parse(&text));
            let duplicates: Vec<_> = rows.iter().filter(|row| row.code == code).collect();
            assert_eq!(duplicates.len(), 2, "{text}: {rows:?}");
            assert!(
                duplicates
                    .iter()
                    .all(|row| row.related[0].span.start as usize == first),
                "{text}: {duplicates:?}"
            );
            let quiet = format!("{prefix} {block}; {row} end; {block}; {row} end;");
            assert!(!analyze(&parse(&quiet)).iter().any(|row| row.code == code));
        }
    }
}

#[test]
fn shocks_history_and_varobs_cover_all_duplicate_keys() {
    let prefix = "var y; varexo e f g; parameters p; p=1; model; y=e+f+g+p; end;";
    for (block, first, second, code) in [
        ("shocks", "var e=.1;", "var e=.2;", "E111"),
        ("shocks", "var e; stderr .1;", "var e; stderr .2;", "E111"),
        ("shocks", "var e=.1;", "var e; stderr .2;", "E111"),
        ("shocks", "var e; stderr .1;", "var e=.2;", "E111"),
        ("shocks", "var e,f=.1;", "var f,e=.2;", "E111"),
        ("shocks", "corr e,f=.1;", "corr f,e=.2;", "E111"),
        ("shocks", "var e,f=.1;", "corr f,e=.2;", "E111"),
        ("shocks", "corr e,f=.1;", "var f,e=.2;", "E111"),
        ("shocks", "skew e=.1;", "skew e=.2;", "E393"),
        ("shocks", "skew e,f,g=.1;", "skew g,e,f=.2;", "E394"),
        (
            "shocks",
            "var e; periods 1; values .1;",
            "var e; periods 2; values .2;",
            "E344",
        ),
        (
            "mshocks",
            "var e; periods 1; values .1;",
            "var e; periods 2; values .2;",
            "E344",
        ),
        (
            "shocks(surprise)",
            "var e; periods 1; values .1;",
            "var e; periods 2; values .2;",
            "E344",
        ),
        (
            "shocks(learnt_in=1)",
            "var e; periods 1; values .1;",
            "var e; periods 2; values .2;",
            "E344",
        ),
        (
            "heteroskedastic_shocks",
            "var e; periods 1; values .1;",
            "var e; periods 2; values .2;",
            "E402",
        ),
        (
            "heteroskedastic_shocks",
            "var e; periods 1; scales .1;",
            "var e; periods 2; scales .2;",
            "E402",
        ),
        ("histval", "y(0)=1;", "y(0)=2;", "E243"),
        (
            "conditional_forecast_paths",
            "var y; periods 1; values .1;",
            "var y; periods 2; values .2;",
            "E344",
        ),
    ] {
        let text = format!("{prefix} {block}; {first} {second} end;");
        assert_eq!(
            first_related(&text, code).span.start as usize,
            text.find(first).unwrap(),
            "{text}"
        );
        if block != "conditional_forecast_paths" {
            let quiet = format!("{prefix} {block}; {first} end; {block}; {second} end;");
            assert!(
                !analyze(&parse(&quiet)).iter().any(|row| row.code == code),
                "{quiet}"
            );
        }
    }
    let text = format!("{prefix} varobs y y;");
    assert_eq!(
        first_related(&text, "W091").span.start as usize,
        text.find("varobs y").unwrap() + 7
    );
    let text = format!("{prefix} varobs y; varobs y;");
    assert_eq!(
        first_related(&text, "E258").span.start as usize,
        text.find("varobs").unwrap()
    );
}

#[test]
fn macro_context_and_independent_include_targets_reach_mcp_and_batch() {
    let root = "C:/dygnosis-links/model.mod";
    let first = "C:/dygnosis-links/first.inc";
    let text = "@#include \"first.inc\"\n/* 🚀 */ var y; model; y=0; end;\r\n";
    let files = HashMap::from([
        (root.to_string(), text.to_string()),
        (first.to_string(), "/* 🧮 */ var y;\r\n".to_string()),
    ]);
    let rows = dynare_diagnose(text, Some(root), Some(&files));
    let row = rows.iter().find(|row| row.code == "W031").unwrap();
    assert_eq!(row.related[0]["file"], first);
    assert_eq!(row.related[0]["line"], 1);
    assert_eq!(row.related[0]["column"], 13); // scalar, not UTF-16
    let batch =
        dygnosis::dynare_workspace_diagnose(Some(&files), Some(&[root.to_string()]), None).unwrap();
    let related = &batch["roots"][0]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["code"] == "W031")
        .unwrap()["related"];
    assert_eq!(related, &serde_json::json!(row.related));
    for text in [
        "@#for k in 1:2\nvar y;\n@#endfor\nmodel; y=0; end;",
        "parameters p;\n@#for k in 1:2\np=foo(1); external_function(name=foo,nargs=1);\n@#endfor\nvar y; model; y=0; end;",
        "@#for k in 1:2\nepilogue; z=1; end;\n@#endfor\nvar y; model; y=0; end;",
    ] {
        let rows = dynare_diagnose(text,None,None);
        let row = rows.iter().find(|row| row.code == "W031").unwrap();
        assert_eq!(row.related[0]["origin_frames"][0]["value"],"1", "{rows:?}");
    }
    let set = check_file_with_origins("var y; parameters unused; model; y=y(-1); end;", root);
    assert_eq!(
        set.diagnostics
            .iter()
            .find(|row| row.code == "W022")
            .unwrap()
            .tags,
        vec![1]
    );
    let rows = dynare_diagnose(
        "// comment\r/* 🚀 */ var y;\rvar y;\rmodel; y=0; end;",
        None,
        None,
    );
    assert_eq!(
        rows.iter().find(|row| row.code == "W031").unwrap().related[0]["line"],
        2
    );
}
