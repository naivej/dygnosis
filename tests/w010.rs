use std::path::PathBuf;

use dygnosis::span::LineIndex;
use dygnosis::{check_w010_family, parse};

const ARCHIVES: &[&str] = &[
    "trend_rbc_gov_inv",
    "sims_wu_2019",
    "govt_rbc_irf_matching",
    "lk2024",
];

const FAMILY: &[&str] = &["W010", "W011", "W012", "W020", "E021", "W022"];

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

fn rust_family(text: &str) -> Vec<Diag> {
    let model = parse(text);
    let index = LineIndex::new(&model.source);
    let rows: Vec<Diag> = check_w010_family(&model)
        .into_iter()
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
        .collect();
    assert!(
        rows.iter().all(|d| FAMILY.contains(&d.code.as_str())),
        "Rust emitted a non-family code: {rows:?}"
    );
    rows
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

fn assert_last_ident(text: &str, d: &Diag, context: &str, ident: &str) {
    assert_eq!(
        (d.start_line, d.start_char, d.end_line, d.end_char),
        last_ident_in(text, context, ident),
        "span should be last {ident:?} in {context:?}, message={}",
        d.message
    );
}

fn by_code<'a>(got: &'a [Diag], code: &str) -> &'a Diag {
    let hits: Vec<_> = got.iter().filter(|d| d.code == code).collect();
    assert_eq!(hits.len(), 1, "expected one {code}, got {got:?}");
    hits[0]
}

#[test]
fn w010_clean_archives_empty() {
    for name in ARCHIVES {
        let got = rust_family(&read_mod(name));
        assert!(
            got.is_empty(),
            "{name}: expected no W010 family, got {got:?}"
        );
    }
}

#[test]
fn w010_unassigned_referenced_param() {
    let text = check_mod("w010/w010_ref.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "W010");
    assert_eq!(got[0].severity, 2);
    assert!(got[0]
        .message
        .contains("Parameter 'orphan_p' is declared but never assigned"));
    assert_last_ident(&text, &got[0], "parameters rho betta orphan_p;", "orphan_p");
}

#[test]
fn w010_and_w022_unassigned_unref_param() {
    let text = check_mod("w010/w010_unref.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 2);
    let w010 = by_code(&got, "W010");
    assert_eq!(w010.severity, 2);
    assert!(w010
        .message
        .contains("Parameter 'orphan_p' is declared but never assigned"));
    assert_last_ident(&text, w010, "parameters rho betta orphan_p;", "orphan_p");
    let w022 = by_code(&got, "W022");
    assert_eq!(w022.severity, 2);
    assert!(w022
        .message
        .contains("Parameter 'orphan_p' is declared but never referenced"));
    assert_last_ident(&text, w022, "parameters rho betta orphan_p;", "orphan_p");
}

#[test]
fn w011_unknown_ident() {
    let text = check_mod("w010/w011_unknown.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "W011");
    assert_eq!(got[0].severity, 2);
    assert!(got[0].message.contains("rho = unknown_zzz"));
    assert_span(&text, &got[0], "rho = unknown_zzz;");
}

#[test]
fn w011_missing_in_expr() {
    let text = check_mod("w010/w011_expr.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "W011");
    assert_eq!(got[0].severity, 2);
    assert!(got[0].message.contains("rho = 1/(1-missing)"));
    assert_span(&text, &got[0], "rho = 1/(1-missing);");
}

#[test]
fn w011_latest_wins_before_steady() {
    let src = check_mod("w010/w011_later.mod");
    let got = rust_family(&src);
    assert!(
        got.is_empty(),
        "W011 latest-wins should be empty, got {got:?}"
    );
}

#[test]
fn w012_helper_before_first_block() {
    let text = check_mod("w010/w012_helper.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "W012");
    assert_eq!(got[0].severity, 2);
    assert!(got[0].message.contains("helper_foo"));
    assert_span(&text, &got[0], "helper_foo = 1.5;");
}

#[test]
fn w012_helper_after_model_end_skipped() {
    let src = check_mod("w010/w012_late.mod");
    let rust = rust_family(&src);
    assert!(
        rust.iter().all(|d| d.code != "W012"),
        "helper after model end should not W012, got {rust:?}"
    );
    assert!(
        rust.is_empty(),
        "W012 helper_late should have no W010 family, got {rust:?}"
    );
}

#[test]
fn w020_unused_endo_count_preserving() {
    let text = check_mod("w010/w020_zzz.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "W020");
    assert_eq!(got[0].severity, 2);
    assert!(got[0].message.contains("Endogenous variable 'z'"));
    assert_last_ident(&text, &got[0], "var y c z;", "z");
}

#[test]
fn w021_unused_varexo() {
    let text = check_mod("w010/w021_exo.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "E021");
    assert_eq!(got[0].severity, 1);
    assert!(got[0].message.contains("unused_exo"));
    assert_span_in(&text, &got[0], "varexo e unused_exo;", "unused_exo");
}

#[test]
fn w022_unused_assigned_param() {
    let text = check_mod("w010/w022_unused.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "W022");
    assert_eq!(got[0].severity, 2);
    assert!(got[0].message.contains("unused_p"));
    assert_last_ident(&text, &got[0], "parameters rho betta unused_p;", "unused_p");
}

#[test]
fn w022_param_used_only_in_stderr() {
    let got = rust_family(&check_mod("w010/w022_stderr.mod"));
    assert!(
        got.is_empty(),
        "W022 stderr sigma_z should be empty, got {got:?}"
    );
}

#[test]
fn w022_param_used_only_in_ss() {
    let got = rust_family(&check_mod("w010/w022_ss.mod"));
    assert!(
        got.is_empty(),
        "W022 SS dummy_p should be empty, got {got:?}"
    );
}

#[test]
fn w022_param_used_only_in_initval() {
    let got = rust_family(&check_mod("w010/w022_initval.mod"));
    assert!(
        got.is_empty(),
        "W022 initval kss should be empty, got {got:?}"
    );
}
