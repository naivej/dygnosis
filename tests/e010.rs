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
        .filter(|d| d.code == "E010")
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
        assert!(got.is_empty(), "{name}: expected no E010, got {got:?}");
    }
}

#[test]
fn e010_extra() {
    let text = check_mod("e010/e010_extra.mod");
    let got = rust_e010(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "E010");
    assert_eq!(got[0].severity, 1);
    assert!(got[0].message.contains("2 equation(s) but 1 endogenous"));
    assert_span(
        &text,
        &got[0],
        "model;\ny = rho * y(-1) + e;\ny = 0;\nend;",
    );
}

#[test]
fn e010_linked() {
    let text = check_mod("e010/e010_linked.mod");
    let got = rust_e010(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "E010");
    assert_eq!(got[0].severity, 1);
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
    assert_eq!(got[0].code, "E010");
    assert_eq!(got[0].severity, 1);
    assert!(
        got[0].message.contains("add 1 missing equation"),
        "generic missing, got {}",
        got[0].message
    );
    assert_span(&text, &got[0], "model;\ny = rho * y(-1) + z + e;\nend;");
}
