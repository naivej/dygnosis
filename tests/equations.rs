use std::path::PathBuf;

use dygnosis::model_info::TimingClass;
use dygnosis::{
    count_gap, equations, explain_equation, parse, CountGap, EquationIdent, EquationRow, IdentClass,
};

fn fixture(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn expected(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/expected/equations")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("expected missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn copilot_mod(archive_dir: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/model-archive")
        .join(archive_dir)
        .join(format!("{archive_dir}.mod"))
}

fn ident(
    name: &str,
    timing: i32,
    class: IdentClass,
    timing_class: Option<TimingClass>,
) -> EquationIdent {
    EquationIdent {
        name: name.to_string(),
        timing,
        dynare_timing: timing,
        class,
        timing_class,
    }
}

const DEFAULT_CAPITAL: &str = "var y k i; parameters alppha delta; alppha=.3; delta=.1; model; [name='production'] y=k(-1)^alppha; [name='capital'] k=i+(1-delta)*k(-1); [name='investment'] i=.2*y; end;";
const BEGINNING_CAPITAL: &str = "var y k i; predetermined_variables k; parameters alppha delta; alppha=.3; delta=.1; model; [name='production'] y=k^alppha; [name='capital'] k(+1)=i+(1-delta)*k; [name='investment'] i=.2*y; end;";

#[test]
fn equivalent_capital_models_share_dynare_timing_and_keep_written_offsets() {
    use dygnosis::{
        classify_variable_timing, dynare_equations, dynare_model_info, structure_summary,
    };
    let default = parse(DEFAULT_CAPITAL);
    let beginning = parse(BEGINNING_CAPITAL);
    let default_timing = classify_variable_timing(&default);
    let beginning_timing = classify_variable_timing(&beginning);
    for (name, timing) in &default_timing {
        assert_eq!(timing.class, beginning_timing[name].class);
        assert_eq!(timing.offsets, beginning_timing[name].offsets);
    }
    assert!(!default_timing["k"].predetermined_conversion);
    assert!(beginning_timing["k"].predetermined_conversion);
    assert_eq!(structure_summary(&default), structure_summary(&beginning));
    let summary = structure_summary(&beginning);
    assert_eq!(
        (
            summary.predetermined,
            summary.forward_looking,
            summary.static_vars
        ),
        (1, 0, 2)
    );
    assert_eq!((summary.max_lead, summary.max_lag), (0, -1));
    let default_rows = equations(&default);
    let beginning_rows = equations(&beginning);
    for (a, b) in default_rows.iter().zip(&beginning_rows) {
        assert_eq!(
            a.idents
                .iter()
                .map(|id| (&id.name, id.dynare_timing, id.timing_class))
                .collect::<Vec<_>>(),
            b.idents
                .iter()
                .map(|id| (&id.name, id.dynare_timing, id.timing_class))
                .collect::<Vec<_>>()
        );
    }
    assert_eq!(default_rows[0].idents[1].timing, -1);
    assert_eq!(beginning_rows[0].idents[1].timing, 0);
    assert_eq!(beginning_rows[1].idents[0].timing, 1);
    assert!(beginning_rows[1].text.contains("k(+1)"));
    let transport = dynare_equations(BEGINNING_CAPITAL, None, None, None, None);
    assert_eq!(transport["equations"][1]["idents"][0]["timing"], 1);
    assert_eq!(transport["equations"][1]["idents"][0]["dynare_timing"], 0);
    assert!(explain_equation(&beginning_rows[1]).contains("written offset 1, Dynare offset 0"));
    let info = dynare_model_info(BEGINNING_CAPITAL, None, None);
    assert_eq!(info["predetermined"], serde_json::json!(["k"]));
    assert_eq!(info["mixed"], serde_json::json!([]));
    assert_eq!(info["n_predetermined"], 1);
    let expanded = dygnosis::dynare_expand(BEGINNING_CAPITAL, None, None);
    assert!(expanded["effective_text"]
        .as_str()
        .unwrap()
        .contains("k(+1)"));
    let extracted = dygnosis::dynare_extract(
        BEGINNING_CAPITAL,
        None,
        None,
        &["capital".into()],
        &std::collections::HashMap::new(),
        None,
    )
    .unwrap();
    assert!(extracted["fragment"].as_str().unwrap().contains("k(+1)"));
}

#[test]
fn predetermined_four_offsets_and_combined_classes() {
    use dygnosis::{classify_variable_timing, dynare_model_info, structure_summary};
    let source =
        "var k u; predetermined_variables k; model; k(+1)=k+k(-1)+k(+2)+u(+1); u=u(-1); end;";
    let model = parse(source);
    let row = &equations(&model)[0];
    let offsets: Vec<_> = row
        .idents
        .iter()
        .filter(|id| id.name == "k")
        .map(|id| (id.timing, id.dynare_timing))
        .collect();
    assert_eq!(offsets, [(1, 0), (0, -1), (-1, -2), (2, 1)]);
    assert!(row
        .idents
        .iter()
        .filter(|id| id.name == "u")
        .all(|id| id.timing == id.dynare_timing));
    let timing = classify_variable_timing(&model);
    assert_eq!(timing["k"].offsets, [-2, -1, 0, 1]);
    assert_eq!(timing["k"].class, TimingClass::Mixed);
    let summary = structure_summary(&model);
    assert_eq!((summary.predetermined, summary.forward_looking), (2, 2));
    assert_eq!((summary.max_lead, summary.max_lag), (1, -2));
    let info = dynare_model_info(source, None, None);
    assert_eq!(info["mixed"], serde_json::json!(["k", "u"]));
    assert_eq!(info["n_predetermined"], 0);
    for (written, expected, offsets) in [
        ("k(1)=0;", TimingClass::Static, vec![0]),
        ("k(+2)=0;", TimingClass::ForwardLooking, vec![1]),
        ("k=0;", TimingClass::Predetermined, vec![-1]),
    ] {
        let model = parse(&format!(
            "var k unused; predetermined_variables k unused; model; {written} end;"
        ));
        let timing = classify_variable_timing(&model);
        assert_eq!(timing["k"].class, expected);
        assert_eq!(timing["k"].offsets, offsets);
        assert_eq!(timing["unused"].class, TimingClass::Static);
        assert_eq!(timing["unused"].offsets, [0]);
    }
}

#[test]
fn predetermined_marks_follow_valid_roles_and_successful_type_changes() {
    use dygnosis::classify_variable_timing;
    for (prefix, shifted) in [
        ("predetermined_variables k; var k;", false),
        ("parameters k; predetermined_variables k; change_type(var) k;", false),
        ("varexo k; predetermined_variables k; change_type(var) k;", false),
        ("var k; predetermined_variables k; change_type(parameters) k; change_type(var) k;", false),
        ("var k; predetermined_variables k; change_type(parameters) k; change_type(var) k; predetermined_variables k;", true),
        ("parameters k; change_type(var) k; predetermined_variables k;", true),
        ("var k; predetermined_variables k; change_type(var) k;", true),
    ] {
        let model = parse(&format!("{prefix} model; k(1)=0; end;"));
        let id = &equations(&model)[0].idents[0];
        assert_eq!((id.timing, id.dynare_timing), (1, if shifted { 0 } else { 1 }), "{prefix}");
        assert_eq!(classify_variable_timing(&model)["k"].class, if shifted { TimingClass::Static } else { TimingClass::ForwardLooking }, "{prefix}");
    }
    let model =
        parse("var k; predetermined_variables k; model; k(1)=0; end; change_type(parameters) k;");
    // Their type change refuses after an expression use and therefore cannot clear the mark.
    assert_eq!(equations(&model)[0].idents[0].dynare_timing, 0);
    let model =
        parse("var k; predetermined_variables k; change_type(parameters) k; model; k=0; end;");
    assert_eq!(equations(&model)[0].idents[0].dynare_timing, 0);
    assert!(equations(&model)[0].idents[0].timing_class.is_none());
}

#[test]
fn predetermined_macro_execution_order_clears_and_restores_marks() {
    for (remark, expected) in [("", 1), ("predetermined_variables k;", 0)] {
        let source = format!("var k;\n@#for j in 1:2\n@#if j == 1\npredetermined_variables k;\n@#else\nchange_type(parameters) k; change_type(var) k; {remark}\n@#endif\n@#endfor\nmodel; k(1)=0; end;");
        let rows = dygnosis::dynare_equations(&source, None, None, None, None);
        assert_eq!(rows["equations"][0]["idents"][0]["timing"], 1);
        assert_eq!(
            rows["equations"][0]["idents"][0]["dynare_timing"], expected,
            "{rows}"
        );
    }
}

#[test]
fn predetermined_local_definitions_shift_once_and_static_rows_do_not_add_lags() {
    use dygnosis::classify_variable_timing;
    let local = parse("var k; predetermined_variables k; model; # loc=k; k(1)=loc; end;");
    let timing = classify_variable_timing(&local);
    assert_eq!(timing["k"].offsets, [-1, 0]);
    assert_eq!(timing["k"].class, TimingClass::Predetermined);
    assert_eq!(local.ident_refs(&local.equations[0])[1].timing, 0);
    assert!(equations(&local)[0]
        .idents
        .iter()
        .any(|id| id.name == "loc" && id.dynare_timing == 0));
    for (dynamic, expected) in [
        ("k(1)=0;", TimingClass::Static),
        ("k(2)=0;", TimingClass::ForwardLooking),
    ] {
        let source = format!(
            "var k; predetermined_variables k; model; [static] k=0; [dynamic] {dynamic} end;"
        );
        let model = parse(&source);
        assert_eq!(classify_variable_timing(&model)["k"].class, expected);
        assert!(!classify_variable_timing(&model)["k"]
            .offsets
            .iter()
            .any(|offset| *offset < 0));
    }
}

#[test]
fn heterogeneous_timing_never_applies_an_aggregate_mark() {
    use dygnosis::{classify_variable_timing, dynare_equations};
    let source = "var k; predetermined_variables k; heterogeneity_dimension d; var(heterogeneity=d) h; predetermined_variables h; model; k(1)=SUM(h); end; model(heterogeneity=d); h=k+h(1); end;";
    let model = parse(source);
    assert_eq!(
        classify_variable_timing(&model)["h"].class,
        TimingClass::ForwardLooking
    );
    let result = dynare_equations(source, None, None, None, None);
    let idents = result["heterogeneous_equations"][0]["equations"][0]["idents"]
        .as_array()
        .unwrap();
    assert!(
        idents.iter().all(|id| id["timing"] == id["dynare_timing"]),
        "{result}"
    );
}

#[test]
fn equivalent_capital_models_are_accepted_at_pinned_check_and_transform() {
    use dygnosis::{run_preprocessor, JsonStage};
    let binary = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    if !binary.is_file() {
        eprintln!("skip predetermined timing honesty: Dynare 7.2 is absent");
        return;
    }
    for source in [DEFAULT_CAPITAL, BEGINNING_CAPITAL] {
        for stage in [JsonStage::Check, JsonStage::Transform] {
            let result = run_preprocessor(
                source,
                &binary,
                None,
                std::time::Duration::from_secs(30),
                stage,
            );
            assert!(
                result.success,
                "{stage:?}: {} {}",
                result.raw_stdout, result.raw_stderr
            );
        }
    }
}

#[test]
fn reader_skips_local_and_static_indexes() {
    let model = parse(&fixture("equations/reader.mod"));
    let rows = equations(&model);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].index, 0);
    assert_eq!(rows[0].name, "euler");
    assert_eq!(rows[0].text, "y = rho*y(-1)+c(+1)+e");
    assert!(!rows[0].static_tag);
    assert!(!rows[0].dynamic_tag);
    assert_eq!(
        rows[0].idents,
        vec![
            ident(
                "y",
                0,
                IdentClass::Endogenous,
                Some(TimingClass::Predetermined)
            ),
            ident("rho", 0, IdentClass::Parameter, None),
            ident(
                "y",
                -1,
                IdentClass::Endogenous,
                Some(TimingClass::Predetermined)
            ),
            ident(
                "c",
                1,
                IdentClass::Endogenous,
                Some(TimingClass::ForwardLooking)
            ),
            ident("e", 0, IdentClass::Varexo, None),
        ]
    );

    assert_eq!(rows[1].index, 1);
    assert_eq!(rows[1].name, "");
    assert_eq!(rows[1].text, "c = tau+foo");
    assert!(!rows[1].static_tag);
    assert!(rows[1].dynamic_tag);
    assert_eq!(
        rows[1].idents,
        vec![
            ident(
                "c",
                0,
                IdentClass::Endogenous,
                Some(TimingClass::ForwardLooking)
            ),
            ident("tau", 0, IdentClass::VarexoDet, None),
            ident("foo", 0, IdentClass::Undeclared, None),
        ]
    );
}

#[test]
fn reader_count_gap_has_no_planner_exception() {
    let model = parse(&fixture("equations/reader.mod"));
    let gap = count_gap(&model);
    assert_eq!(
        gap,
        CountGap {
            n_endogenous: 3,
            n_equations: 2,
            delta: -1,
            unreferenced_endogenous: vec![],
            expected_delta: None,
        }
    );
}

#[test]
fn reader_explain_markdown() {
    let model = parse(&fixture("equations/reader.mod"));
    let rows = equations(&model);
    assert_eq!(
        explain_equation(&rows[0]),
        expected("reader_euler.md").trim_end()
    );
    assert_eq!(
        explain_equation(&rows[1]),
        expected("reader_unnamed.md").trim_end()
    );
}

#[test]
fn tags_mod_does_not_collapse_duplicate_name() {
    let model = parse(&fixture("equations/tags.mod"));
    let rows = equations(&model);
    assert_eq!(rows.len(), 3);
    assert_eq!(count_gap(&model).n_equations, 3);
}

#[test]
fn reader_tag_map() {
    let model = parse(&fixture("equations/reader.mod"));
    let rows = equations(&model);
    assert_eq!(rows[0].tags.get("name").map(String::as_str), Some("euler"));

    let static_eq = model
        .equations
        .iter()
        .find(|e| e.static_tag)
        .expect("static equation");
    assert_eq!(
        static_eq.tag_map.get("static").map(String::as_str),
        Some("")
    );
    assert_eq!(static_eq.tags, vec!["static".to_string()]);

    let dynamic_eq = model
        .equations
        .iter()
        .find(|e| e.dynamic_tag)
        .expect("dynamic equation");
    assert_eq!(
        dynamic_eq.tag_map.get("dynamic").map(String::as_str),
        Some("")
    );
    assert_eq!(dynamic_eq.tags, vec!["dynamic".to_string()]);
}

#[test]
fn residual_without_eq_keeps_empty_sides() {
    let model = parse("var y;\nmodel;\ny;\nend;\n");
    let rows = equations(&model);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].text, "y");
    let written = model
        .equations
        .iter()
        .find(|eq| eq.text == "y")
        .expect("residual equation");
    assert_eq!(written.lhs, "");
    assert_eq!(written.rhs, "");
}

#[test]
fn count_gap_planner_and_osr() {
    let ramsey_ok = parse(&fixture("w100/w100_ok.mod"));
    assert_eq!(
        count_gap(&ramsey_ok),
        CountGap {
            n_endogenous: 2,
            n_equations: 2,
            delta: 0,
            unreferenced_endogenous: vec![],
            expected_delta: Some(-1),
        }
    );

    let ramsey_gap = parse(&fixture("e010/e010_ramsey_gap.mod"));
    assert_eq!(
        count_gap(&ramsey_gap),
        CountGap {
            n_endogenous: 2,
            n_equations: 1,
            delta: -1,
            unreferenced_endogenous: vec!["c".to_string()],
            expected_delta: Some(-1),
        }
    );

    let osr = parse(&fixture("e010/e010_osr_square.mod"));
    assert_eq!(count_gap(&osr).expected_delta, None);
    assert_eq!(count_gap(&osr).delta, 0);
}

fn dump_lib(model: &dygnosis::Model) -> serde_json::Value {
    let gap = count_gap(model);
    let eqs: Vec<serde_json::Value> = equations(model)
        .into_iter()
        .map(|row: EquationRow| {
            serde_json::json!({
                "index": row.index,
                "text": row.text,
                "idents": row.idents.into_iter().map(|id| {
                    let mut v = serde_json::json!({
                        "name": id.name,
                        "timing": id.timing,
                        "class": id.class.as_str(),
                    });
                    if let Some(tc) = id.timing_class {
                        v["timing_class"] = serde_json::Value::String(tc.label().to_string());
                    }
                    v
                }).collect::<Vec<_>>(),
            })
        })
        .collect();
    serde_json::json!({
        "count_gap": {
            "n_endogenous": gap.n_endogenous,
            "n_equations": gap.n_equations,
            "delta": gap.delta,
            "unreferenced_endogenous": gap.unreferenced_endogenous,
            "expected_delta": gap.expected_delta,
        },
        "equations": eqs,
    })
}

#[test]
fn trend_rbc_gov_inv_library_dump() {
    let path = copilot_mod("trend_rbc_gov_inv");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n");
    let model = parse(&text);
    let got = dump_lib(&model);
    let expected: serde_json::Value =
        serde_json::from_str(&expected("trend_rbc_gov_inv.json")).unwrap();
    assert_eq!(got["count_gap"]["n_endogenous"], 16);
    assert_eq!(got["count_gap"]["n_equations"], 16);
    assert_eq!(got["count_gap"]["delta"], 0);
    assert_eq!(got["count_gap"]["expected_delta"], serde_json::Value::Null);
    assert_eq!(got["equations"][0]["index"], 0);
    assert!(got["equations"][0].get("lhs").is_none());
    assert!(got["equations"][0].get("rhs").is_none());
    assert_eq!(got["equations"][0]["idents"][0]["name"], "y");
    assert_eq!(got["equations"][0]["idents"][1]["name"], "z");
    assert_eq!(got["equations"][0]["idents"][2]["name"], "kg");
    assert_eq!(got["equations"][0]["idents"][2]["timing"], -1);
    assert_eq!(got, expected);
}
