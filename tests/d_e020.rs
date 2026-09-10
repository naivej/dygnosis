use std::path::{Path, PathBuf};

use dygnosis::span::LineIndex;
use dygnosis::{check_e020, parse};

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

fn rust_e020(text: &str) -> Vec<Diag> {
    let model = parse(text);
    let index = LineIndex::new(&model.source);
    check_e020(&model)
        .into_iter()
        .filter(|d| matches!(d.code.as_str(), "E020" | "E023" | "E024" | "E025"))
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

#[test]
fn e020_unmodified_archives() {
    for name in ARCHIVES {
        let got = rust_e020(&read_mod(name));
        assert!(got.is_empty(), "{name}: expected no E020 family, got {got:?}");
    }
}

#[test]
fn shocks_vars_lists_eps_z_and_eps_ig() {
    let model = parse(&read_mod("trend_rbc_gov_inv"));
    let names: Vec<&str> = model.shocks_vars.iter().map(|n| model.name(*n)).collect();
    assert_eq!(names, ["eps_z", "eps_ig"]);
}

#[test]
fn e020_alpph_typo_replace() {
    let text = check_mod("e020/e020_typo.mod");
    let got = rust_e020(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "E020");
    assert_eq!(got[0].severity, 1);
    assert!(
        got[0].message.contains("Undeclared identifier 'alpph'"),
        "alpph message, got {}",
        got[0].message
    );
    assert_span_in(&text, &got[0], "y = rho * y(-1) + alpph + e;", "alpph");
}

#[test]
fn e023_predetermined_eps_z() {
    let text = check_mod("e020/e023_predetermined.mod");
    let got = rust_e020(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "E023");
    assert_eq!(got[0].severity, 1);
    assert!(
        got[0]
            .message
            .contains("Predetermined variable 'e' is not declared as an endogenous variable."),
        "e023 message, got {}",
        got[0].message
    );
    assert_last_ident(&text, &got[0], "predetermined_variables e;", "e");
}

#[test]
fn e024_timed_det_exo_tau() {
    let text = check_mod("e020/e024_timed_det.mod");
    let got = rust_e020(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "E024");
    assert_eq!(got[0].severity, 1);
    assert!(
        got[0].message.contains(
            "Deterministic exogenous variable 'tau' cannot be used with a lead or lag."
        ),
        "e024 message, got {}",
        got[0].message
    );
    assert_span(&text, &got[0], "tau(-1)");
}

#[test]
fn e025_use_before_def() {
    let text = check_mod("e020/e025_use_before.mod");
    let got = rust_e020(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "E025");
    assert_eq!(got[0].severity, 1);
    assert!(
        got[0]
            .message
            .contains("Model-local variable 'foo' is used before its # definition."),
        "e025 use-before message, got {}",
        got[0].message
    );
    assert_span_in(&text, &got[0], "c = c + foo;", "foo");
}

#[test]
fn e025_shadow_declared_y() {
    let text = check_mod("e020/e025_shadow.mod");
    let got = rust_e020(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "E025");
    assert_eq!(got[0].severity, 1);
    assert!(
        got[0]
            .message
            .contains("Model-local variable 'y' shadows a declared Dynare symbol."),
        "e025 shadow message, got {}",
        got[0].message
    );
    assert_span_in(&text, &got[0], "# y = 1;", "y");
}

#[test]
fn mystery_call_is_not_e020() {
    let diags = rust_e020(&check_mod("e020/mystery_call.mod"));
    assert!(
        diags
            .iter()
            .all(|d| !(d.code == "E020" && d.message.contains("'mystery'"))),
        "Call callee mystery must not be E020: {diags:?}"
    );
    assert!(diags.is_empty(), "mystery_call: expected empty family, got {diags:?}");
}

#[test]
fn comment_and_string_are_not_e020() {
    let text = check_mod("e020/comment_string.mod");
    let diags = rust_e020(&text);
    assert!(
        diags.iter().all(|d| !d.message.contains("'sneaky_ident'")),
        "sneaky_ident leaked into E020 family: {diags:?}"
    );
    assert!(diags.is_empty(), "comment_string: expected empty family, got {diags:?}");
}

#[test]
fn src_has_no_ident_harvest_regex() {
    let src_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut hits = Vec::new();
    visit_rs(&src_dir, &mut hits);
    assert!(
        hits.is_empty(),
        "identifier-harvest regex on equation text in src/: {hits:?}"
    );
}

fn visit_rs(dir: &Path, hits: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            visit_rs(&path, hits);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        if path.file_name().and_then(|n| n.to_str()) == Some("preprocessor.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let rel = path.display().to_string();
        for needle in [
            r"\b([A-Za-z_]",
            r"\bident\b",
            "Regex::new",
            "regex::Regex",
            "finditer",
        ] {
            if text.contains(needle) {
                hits.push(format!("{rel}: {needle}"));
            }
        }
    }
}
