use std::path::PathBuf;

use dygnosis::span::LineIndex;
use dygnosis::{check_e010, parse};

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

fn rust_e010(text: &str) -> Vec<Diag> {
    let model = parse(text);
    let index = LineIndex::new(&model.source);
    check_e010(&model)
        .into_iter()
        .filter(|d| d.code == "W013")
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

fn assert_span(text: &str, d: &Diag, needle: &str) {
    assert_eq!(
        (d.start_line, d.start_char, d.end_line, d.end_char),
        range_of(text, needle),
        "span should be {needle:?}, message={}",
        d.message
    );
}

#[test]
fn e010_unmodified_archives() {
    for name in ARCHIVES {
        let got = rust_e010(&read_mod(name));
        assert!(got.is_empty(), "{name}: expected no W013, got {got:?}");
    }
}

#[test]
fn e010_extra() {
    let text = check_mod("e010/e010_extra.mod");
    let got = rust_e010(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "W013");
    assert_eq!(got[0].severity, 2);
    assert!(got[0].message.contains("2 equation(s) but 1 endogenous"));
    assert_span(&text, &got[0], "model;\ny = rho * y(-1) + e;\ny = 0;\nend;");
}

#[test]
fn e010_linked() {
    let text = check_mod("e010/e010_linked.mod");
    let got = rust_e010(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "W013");
    assert_eq!(got[0].severity, 2);
    assert!(
        got[0].message.contains("unreferenced variable(s) z"),
        "linked message, got {}",
        got[0].message
    );
    assert_span(&text, &got[0], "model;\ny = rho * y(-1) + e;\nend;");
}

#[test]
fn e010_missing() {
    let text = check_mod("e010/e010_missing.mod");
    let got = rust_e010(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "W013");
    assert_eq!(got[0].severity, 2);
    assert!(
        got[0].message.contains("add 1 missing equation"),
        "generic missing, got {}",
        got[0].message
    );
    assert_span(&text, &got[0], "model;\ny = rho * y(-1) + z + e;\nend;");
}

fn assert_minus_n(d: &Diag, cmd: &str) {
    assert_eq!(d.code, "W013");
    assert_eq!(d.severity, 2);
    assert!(
        d.message.contains("expects delta = -1"),
        "−N message, got {}",
        d.message
    );
    assert!(
        d.message.contains(cmd),
        "message should name {cmd}, got {}",
        d.message
    );
    assert!(
        !d.message.contains("missing equation"),
        "−N message must not use the generic missing template, got {}",
        d.message
    );
    assert!(
        !d.message.contains("duplicate/extra"),
        "−N message must not use the generic extra template, got {}",
        d.message
    );
}

#[test]
fn w100_ok_square_ramsey_warns() {
    let text = check_mod("w100/w100_ok.mod");
    let got = rust_e010(&text);
    assert_eq!(
        got.len(),
        1,
        "square Ramsey with 1 instrument must warn, got {got:?}"
    );
    assert_minus_n(&got[0], "ramsey_model");
    assert_eq!(
        got[0].message,
        "Equation count mismatch: 2 equation(s) but 2 endogenous variable(s). ramsey_model with 1 instrument(s) expects delta = -1."
    );
}

#[test]
fn e010_ramsey_gap_quiet() {
    let text = check_mod("e010/e010_ramsey_gap.mod");
    let got = rust_e010(&text);
    assert!(got.is_empty(), "Ramsey −1 should be quiet, got {got:?}");
}

#[test]
fn e010_ramsey_policy_gap_quiet() {
    let text = check_mod("e010/e010_ramsey_policy_gap.mod");
    let got = rust_e010(&text);
    assert!(
        got.is_empty(),
        "ramsey_policy −1 should be quiet, got {got:?}"
    );
}

#[test]
fn e010_disc_gap_quiet() {
    let text = check_mod("e010/e010_disc_gap.mod");
    let got = rust_e010(&text);
    assert!(
        got.is_empty(),
        "discretionary −1 should be quiet, got {got:?}"
    );
}

#[test]
fn e010_disc_square_warns() {
    let text = check_mod("e010/e010_disc_square.mod");
    let got = rust_e010(&text);
    assert_eq!(
        got.len(),
        1,
        "square discretionary with instruments must warn, got {got:?}"
    );
    assert_minus_n(&got[0], "discretionary_policy");
}

#[test]
fn e010_ramsey_wrong_n_warns() {
    let text = check_mod("e010/e010_ramsey_wrong_n.mod");
    let got = rust_e010(&text);
    assert_eq!(got.len(), 1, "wrong N must warn, got {got:?}");
    assert_minus_n(&got[0], "ramsey_model");
    assert!(
        got[0].message.contains("1 equation(s) but 3 endogenous"),
        "δ is −2, got {}",
        got[0].message
    );
}

#[test]
fn w100_disc_ok_and_ramsey_empty_instruments_stay_quiet() {
    for rel in ["w100/w100_disc_ok.mod", "w100/w100_ramsey.mod"] {
        let text = check_mod(rel);
        let got = rust_e010(&text);
        assert!(
            got.is_empty(),
            "{rel}: empty instruments, square → no W013, got {got:?}"
        );
    }
}

#[test]
fn e010_osr_square_quiet() {
    let text = check_mod("e010/e010_osr_square.mod");
    let got = rust_e010(&text);
    assert!(
        got.is_empty(),
        "osr square with instruments is not −N, got {got:?}"
    );
}

#[test]
fn e010_osr_gap_generic() {
    let text = check_mod("e010/e010_osr_gap.mod");
    let got = rust_e010(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "W013");
    assert_eq!(got[0].severity, 2);
    assert!(
        got[0].message.contains("add 1 missing equation"),
        "osr mismatch uses the generic message, got {}",
        got[0].message
    );
    assert!(
        !got[0].message.contains("expects delta"),
        "osr must not use the −N message, got {}",
        got[0].message
    );
}
