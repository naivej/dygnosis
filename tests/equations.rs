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
        class,
        timing_class,
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
    assert_eq!(rows[0].lhs, "y");
    assert_eq!(rows[0].rhs, "rho*y(-1)+c(+1)+e");
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
    assert_eq!(rows[1].lhs, "c");
    assert_eq!(rows[1].rhs, "tau+foo");
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
    assert_eq!(rows[0].lhs, "");
    assert_eq!(rows[0].rhs, "");
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
                "lhs": row.lhs,
                "rhs": row.rhs,
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
    assert_eq!(got["equations"][0]["lhs"], "y");
    assert_eq!(got["equations"][0]["idents"][0]["name"], "y");
    assert_eq!(got["equations"][0]["idents"][1]["name"], "z");
    assert_eq!(got["equations"][0]["idents"][2]["name"], "kg");
    assert_eq!(got["equations"][0]["idents"][2]["timing"], -1);
    assert_eq!(got, expected);
}
