use std::path::PathBuf;

use dygnosis::expr::ExprKind;
use dygnosis::span::LineIndex;
use dygnosis::{check_w070, parse};

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

fn trend() -> String {
    read_mod("trend_rbc_gov_inv")
}

fn rust_w070(text: &str) -> Vec<Diag> {
    let model = parse(text);
    let index = LineIndex::new(&model.source);
    check_w070(&model)
        .into_iter()
        .filter(|d| d.code == "W070")
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

fn range_of(text: &str, needle: &str) -> (u32, u32, u32, u32) {
    let start = text
        .find(needle)
        .unwrap_or_else(|| panic!("missing {needle:?}"));
    assert_eq!(
        text.matches(needle).count(),
        1,
        "needle {needle:?} must occur once"
    );
    let end = (start + needle.len()) as u32;
    let index = LineIndex::new(text);
    let s = index.position(text, start as u32);
    let e = index.position(text, end);
    (s.line, s.character, e.line, e.character)
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

fn assert_span(text: &str, d: &Diag, needle: &str) {
    assert_eq!(
        (d.start_line, d.start_char, d.end_line, d.end_char),
        range_of(text, needle),
        "span should be {needle:?}, message={}",
        d.message
    );
}

fn assert_span_in(text: &str, d: &Diag, context: &str, inner: &str) {
    assert_eq!(
        (d.start_line, d.start_char, d.end_line, d.end_char),
        range_in(text, context, inner),
        "span should be {inner:?} in {context:?}, message={}",
        d.message
    );
}

fn assert_fire(rel: &str, msg_sub: &str, needle: &str) {
    let text = check_mod(rel);
    let got = rust_w070(&text);
    assert_eq!(got.len(), 1, "{rel}: {got:?}");
    assert_eq!(got[0].code, "W070");
    assert_eq!(got[0].severity, 2);
    assert!(
        got[0].message.contains(msg_sub),
        "{rel}: missing {msg_sub:?} in {}",
        got[0].message
    );
    assert_span(&text, &got[0], needle);
}

#[test]
fn w070_betta_rhs_is_number() {
    let model = parse(&trend());
    let a = model
        .param_assignments
        .iter()
        .find(|a| model.name(a.name) == "betta")
        .expect("betta assignment");
    let id = a.expr.expect("betta RHS tree");
    assert!(
        matches!(model.exprs.get(id).kind, ExprKind::Number),
        "betta RHS should be Number, got {:?}",
        model.exprs.get(id).kind
    );
}

#[test]
fn w070_clean_archives_empty() {
    for name in ARCHIVES {
        let got = rust_w070(&read_mod(name));
        assert!(got.is_empty(), "{name}: expected no W070, got {got:?}");
    }
}

#[test]
fn w070_betta_99() {
    assert_fire("w070/w070_betta.mod", "'betta' = 99", "betta = 99;");
}

#[test]
fn w070_betta_0() {
    assert_fire("w070/w070_betta_0.mod", "'betta' = 0", "betta = 0;");
}

#[test]
fn w070_betta_1() {
    assert_fire("w070/w070_betta_1.mod", "'betta' = 1", "betta = 1;");
}

#[test]
fn w070_delta_1p5() {
    assert_fire("w070/w070_delta.mod", "'delta' = 1.5", "delta = 1.5;");
}

#[test]
fn w070_delta_1() {
    assert_fire("w070/w070_delta_1.mod", "'delta' = 1", "delta = 1;");
}

#[test]
fn w070_delta_0() {
    let text = check_mod("w070/w070_delta_0.mod");
    let got = rust_w070(&text);
    assert!(
        got.is_empty(),
        "delta = 0 is in [0,1); expected no W070, got {got:?}"
    );
}

#[test]
fn w070_rhoz_1p5() {
    assert_fire("w070/w070_rho.mod", "'rho' = 1.5", "rho = 1.5;");
}

#[test]
fn w070_rhoz_m1() {
    assert_fire("w070/w070_rho_m1.mod", "'rho' = -1", "rho = -1;");
}

#[test]
fn w070_sigma_e() {
    assert_fire(
        "w070/w070_sigma.mod",
        "'sigma_e' = -0.1",
        "sigma_e = -0.1;",
    );
}

#[test]
fn w070_sigma_e_helper() {
    let text = check_mod("w070/w070_sigma_helper.mod");
    let got = rust_w070(&text);
    assert!(
        got.is_empty(),
        "undeclared sigma_e is a helper; expected no W070, got {got:?}"
    );
}

#[test]
fn w070_alphag_1p5() {
    assert_fire("w070/w070_alpha.mod", "'alphag' = 1.5", "alphag = 1.5;");
}

#[test]
fn w070_alppha_1p5() {
    let text = check_mod("w070/w070_alppha.mod");
    let got = rust_w070(&text);
    assert!(
        got.is_empty(),
        "alppha does not contain substring alpha; expected no W070, got {got:?}"
    );
}

#[test]
fn w070_expr_sum() {
    assert_fire(
        "w070/w070_expr.mod",
        "'betta' = 1.1",
        "betta = 0.9 + 0.2;",
    );
}

#[test]
fn w070_later_wins() {
    let text = check_mod("w070/w070_later.mod");
    let got = rust_w070(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "W070");
    assert_eq!(got[0].severity, 2);
    assert!(
        got[0].message.contains("'betta' = 99"),
        "later wins, got {}",
        got[0].message
    );
    assert_span_in(
        &text,
        &got[0],
        "betta = 0.99;\nbetta = 99;",
        "betta = 99;",
    );
}
