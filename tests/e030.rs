use std::path::PathBuf;

use dygnosis::expr::ExprKind;
use dygnosis::span::LineIndex;
use dygnosis::{check_e030, parse};

const ARCHIVES: &[&str] = &[
    "trend_rbc_gov_inv",
    "sims_wu_2019",
    "govt_rbc_irf_matching",
    "lk2024",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct Diag {
    code: String,
    severity: i32,
    message: String,
    start_line: u32,
    start_char: u32,
    end_line: u32,
    end_char: u32,
}

fn copilot_mod(archive_dir: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/model-archive")
        .join(archive_dir)
        .join(format!("{archive_dir}.mod"))
}

fn read_mod(archive_dir: &str) -> String {
    let path = copilot_mod(archive_dir);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn check_mod(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn rust_e030(text: &str) -> Vec<Diag> {
    let model = parse(text);
    let index = LineIndex::new(&model.source);
    check_e030(&model)
        .into_iter()
        .filter(|d| d.code == "E030")
        .map(|d| {
            let start = index.position(&model.source, d.span.start);
            let end = index.position(&model.source, d.span.end);
            Diag {
                code: d.code,
                severity: d.severity as i32,
                message: d.message,
                start_line: start.line,
                start_char: start.character,
                end_line: end.line,
                end_char: end.character,
            }
        })
        .collect()
}

fn range_in(text: &str, context: &str, inner: &str) -> (u32, u32, u32, u32) {
    let at = text
        .find(context)
        .unwrap_or_else(|| panic!("missing context {context:?}"));
    let rel = text[at..at + context.len()]
        .find(inner)
        .unwrap_or_else(|| panic!("missing {inner:?} in {context:?}"));
    let start = (at + rel) as u32;
    let end = start + inner.len() as u32;
    let index = LineIndex::new(text);
    let s = index.position(text, start);
    let e = index.position(text, end);
    (s.line, s.character, e.line, e.character)
}

fn last_ident_in(text: &str, context: &str, ident: &str) -> (u32, u32, u32, u32) {
    let at = text
        .find(context)
        .unwrap_or_else(|| panic!("missing context {context:?}"));
    let rel = context
        .rfind(ident)
        .unwrap_or_else(|| panic!("missing {ident:?} in {context:?}"));
    let start = (at + rel) as u32;
    let end = start + ident.len() as u32;
    let index = LineIndex::new(text);
    let s = index.position(text, start);
    let e = index.position(text, end);
    (s.line, s.character, e.line, e.character)
}

fn assert_span_in(text: &str, d: &Diag, context: &str, inner: &str) {
    assert_eq!(
        (d.start_line, d.start_char, d.end_line, d.end_char),
        range_in(text, context, inner),
        "span should be {inner:?} in {context:?}, message={}",
        d.message
    );
}

fn assert_last_ident(text: &str, d: &Diag, context: &str, ident: &str) {
    assert_eq!(
        (d.start_line, d.start_char, d.end_line, d.end_char),
        last_ident_in(text, context, ident),
        "span should be last {ident:?} in {context:?}, message={}",
        d.message
    );
}

#[test]
fn e030_unmodified_archives() {
    for name in ARCHIVES {
        let got = rust_e030(&read_mod(name));
        assert!(got.is_empty(), "{name}: expected no E030, got {got:?}");
    }
}

#[test]
fn shocks_vars_lists_eps_z_and_eps_ig() {
    let model = parse(&read_mod("trend_rbc_gov_inv"));
    let names: Vec<&str> = model.shocks_vars.iter().map(|n| model.name(*n)).collect();
    assert_eq!(names, ["eps_z", "eps_ig"]);
}

#[test]
fn model_local_hash_sets_flag_and_lhs_ident() {
    let model = parse(&check_mod("e030/model_local_dup.mod"));
    let eq = model
        .equations
        .iter()
        .find(|e| e.model_local)
        .expect("model_local equation");
    assert!(eq.is_local);
    assert!(eq.model_local);
    let id = eq.lhs_expr.expect("lhs_expr");
    match &model.exprs.get(id).kind {
        ExprKind::Ident { name, .. } => assert_eq!(model.name(*name), "foo"),
        other => panic!("expected Ident lhs, got {other:?}"),
    }
}

#[test]
fn varexo_det_cloned_into_exogenous() {
    let model = parse(&check_mod("e030/varexo_det_only.mod"));
    let det: Vec<&str> = model
        .deterministic_exogenous
        .iter()
        .map(|d| model.name(d.name))
        .collect();
    assert!(
        det.contains(&"u"),
        "u missing from deterministic_exogenous: {det:?}"
    );
    let exo: Vec<&str> = model.exogenous.iter().map(|d| model.name(d.name)).collect();
    assert!(
        exo.contains(&"u"),
        "u missing from exogenous clone: {exo:?}"
    );
}

#[test]
fn e030_same_kind_var() {
    let text = check_mod("e030/same_kind_var.mod");
    let got = rust_e030(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "E030");
    assert_eq!(got[0].severity, 2);
    assert!(got[0].message.contains("'y' is declared more than once in 'var'"));
    assert_span_in(&text, &got[0], "var y c;", "y");
}

#[test]
fn e030_same_kind_var_third_compares_to_first() {
    let text = check_mod("e030/same_kind_var_third.mod");
    let got = rust_e030(&text);
    assert_eq!(got.len(), 2);
    assert!(got.iter().all(|d| d.code == "E030" && d.severity == 2));
    assert!(got
        .iter()
        .all(|d| d.message.contains("'y' is declared more than once in 'var'")));
    assert_last_ident(&text, &got[0], "var y;\nvar y;", "y");
    assert_span_in(&text, &got[1], "var y c;", "y");
}

#[test]
fn e030_same_kind_param() {
    let text = check_mod("e030/same_kind_param.mod");
    let got = rust_e030(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].severity, 2);
    assert!(got[0]
        .message
        .contains("'betta' is declared more than once in 'parameters'"));
    assert_span_in(&text, &got[0], "parameters rho betta;", "betta");
}

#[test]
fn e030_var_varexo_timed() {
    let text = check_mod("e030/var_varexo_timed.mod");
    let got = rust_e030(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].severity, 1);
    assert!(got[0]
        .message
        .contains("'c' is declared in both 'var' and 'varexo'"));
    assert_span_in(&text, &got[0], "varexo e c;", "c");
}

#[test]
fn e030_var_varexo_lhs() {
    let text = check_mod("e030/var_varexo_lhs.mod");
    let got = rust_e030(&text);
    assert_eq!(got.len(), 1);
    assert!(got[0]
        .message
        .contains("'y' is declared in both 'var' and 'varexo'"));
    assert_span_in(&text, &got[0], "varexo e y;", "y");
}

#[test]
fn e030_var_varexo_shock() {
    let text = check_mod("e030/var_varexo_shock.mod");
    let got = rust_e030(&text);
    assert_eq!(got.len(), 1);
    assert!(got[0]
        .message
        .contains("'e' is declared in both 'var' and 'varexo'"));
    assert!(got[0].message.contains("referenced in the shocks block"));
    assert_last_ident(&text, &got[0], "varexo e;", "e");
}

#[test]
fn e030_varexo_param_assigned() {
    let text = check_mod("e030/varexo_param.mod");
    let got = rust_e030(&text);
    assert_eq!(got.len(), 1);
    assert!(got[0]
        .message
        .contains("'betta' is declared in both 'varexo' and 'parameters'"));
    assert_span_in(&text, &got[0], "parameters rho betta;", "betta");
}

#[test]
fn e030_varexo_det() {
    let text = check_mod("e030/varexo_det.mod");
    let got = rust_e030(&text);
    assert_eq!(got.len(), 1);
    assert!(got[0]
        .message
        .contains("'e' is declared in both 'varexo_det' and 'varexo'"));
    assert_last_ident(&text, &got[0], "varexo e;", "e");
}

#[test]
fn e030_model_local_dup() {
    let text = check_mod("e030/model_local_dup.mod");
    let got = rust_e030(&text);
    assert_eq!(got.len(), 1);
    assert!(got[0]
        .message
        .contains("Model-local variable 'foo' is declared twice"));
    assert_span_in(&text, &got[0], "# foo = 2;", "foo");
}

#[test]
fn e030_model_local_shadow_emits_none() {
    let text = check_mod("e030/model_local_shadow.mod");
    assert!(
        rust_e030(&text).is_empty(),
        "model_local_shadow should be empty"
    );
}

#[test]
fn e030_varexo_det_only() {
    let text = check_mod("e030/varexo_det_only.mod");
    assert!(
        rust_e030(&text).is_empty(),
        "varexo_det_only should be empty"
    );
}
