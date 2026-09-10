use std::path::PathBuf;

use dygnosis::span::LineIndex;
use dygnosis::{check_w120_family, parse};

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

impl PartialOrd for Diag {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Diag {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (
            self.code.as_str(),
            self.start_line,
            self.start_char,
            self.message.as_str(),
        )
            .cmp(&(
                other.code.as_str(),
                other.start_line,
                other.start_char,
                other.message.as_str(),
            ))
    }
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
    check_w120_family(&model)
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
        .collect()
}

fn range_of(text: &str, needle: &str) -> (u32, u32, u32, u32) {
    let start = text
        .find(needle)
        .unwrap_or_else(|| panic!("missing {needle:?}"));
    let end = (start + needle.len()) as u32;
    let index = LineIndex::new(text);
    let s = index.position(text, start as u32);
    let e = index.position(text, end);
    (s.line, s.character, e.line, e.character)
}

fn assert_w121_spans(text: &str, rust: &[Diag], needles: &[&str]) {
    let w121: Vec<&Diag> = rust.iter().filter(|d| d.code == "W121").collect();
    assert_eq!(
        w121.len(),
        needles.len(),
        "W121 count vs needles: rust={rust:?} needles={needles:?}"
    );
    for (d, needle) in w121.iter().zip(needles) {
        assert_eq!(
            (d.start_line, d.start_char, d.end_line, d.end_char),
            range_of(text, needle),
            "W121 native range should be {needle:?}"
        );
    }
}

#[test]
fn w120_clean_four_empty() {
    for archive in ARCHIVES {
        let text = read_mod(archive);
        let rust = rust_family(&text);
        assert!(
            rust.is_empty(),
            "clean {archive} should have empty FAMILY, got {rust:?}"
        );
    }
}

#[test]
fn w120_det() {
    let src = check_mod("w120/w120_det.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "W120");
    assert_eq!(
        (
            rust[0].start_line,
            rust[0].start_char,
            rust[0].end_line,
            rust[0].end_char
        ),
        range_of(&src, "stoch_simul")
    );
}

#[test]
fn w120_estimation() {
    let src = check_mod("w120/w120_est.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(
        rust[0].end_char - rust[0].start_char,
        10,
        "estimation token length"
    );
}

#[test]
fn w120_first() {
    let src = check_mod("w120/w120_first.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert!(
        rust[0].message.starts_with("'estimation'"),
        "first command should be estimation, got {}",
        rust[0].message
    );
}

#[test]
fn w120_case() {
    let src = check_mod("w120/w120_case.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert!(
        rust[0].message.starts_with("'Stoch_Simul'"),
        "source spelling, got {}",
        rust[0].message
    );
}

#[test]
fn w120_one_varexo() {
    let src = check_mod("w120/w120_one.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w120_simul() {
    let src = check_mod("w120/w120_simul.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w120_comment() {
    let src = check_mod("w120/w120_comment.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w121_betta() {
    let src = check_mod("w120/w121_lead.mod");
    let mut rust = rust_family(&src);
    rust.sort();
    assert_w121_spans(&src, &rust, &["betta(+1)"]);
}

#[test]
fn w121_rhoz() {
    let src = check_mod("w120/w121_lag.mod");
    let mut rust = rust_family(&src);
    rust.sort();
    assert_w121_spans(&src, &rust, &["rho(-1)"]);
}

#[test]
fn w121_noplus() {
    let src = check_mod("w120/w121_noplus.mod");
    let mut rust = rust_family(&src);
    rust.sort();
    assert_eq!(rust.len(), 1);
    assert!(
        rust[0].message.contains("'betta(1)'"),
        "no plus, got {}",
        rust[0].message
    );
    assert_w121_spans(&src, &rust, &["betta(1)"]);
}

#[test]
fn w121_spaces() {
    let src = check_mod("w120/w121_spaces.mod");
    let mut rust = rust_family(&src);
    rust.sort();
    assert_eq!(rust.len(), 1);
    assert!(
        rust[0].message.contains("'betta(+1)'"),
        "spaces still +1, got {}",
        rust[0].message
    );
    assert_w121_spans(&src, &rust, &["betta ( +1 )"]);
}

#[test]
fn w121_two_names() {
    let src = check_mod("w120/w121_two_names.mod");
    let mut rust = rust_family(&src);
    rust.sort();
    assert_w121_spans(&src, &rust, &["betta(+1)", "rho(+1)"]);
}

#[test]
fn w121_twice() {
    let src = check_mod("w120/w121_twice.mod");
    let mut rust = rust_family(&src);
    rust.sort();
    assert_eq!(rust.len(), 1);
    assert!(
        rust[0].message.contains("'betta(+1)'"),
        "first hit spelling, got {}",
        rust[0].message
    );
    assert_w121_spans(&src, &rust, &["betta(+1)"]);
}

#[test]
fn w121_two_eq() {
    let src = check_mod("w120/w121_two_eq.mod");
    let mut rust = rust_family(&src);
    rust.sort();
    assert_w121_spans(&src, &rust, &["betta(+1)", "rho(-1)"]);
}

#[test]
fn w121_dynamic() {
    let src = check_mod("w120/w121_dynamic.mod");
    let mut rust = rust_family(&src);
    rust.sort();
    assert_w121_spans(&src, &rust, &["betta(+1)"]);
}

#[test]
fn w121_neg_call() {
    let src = check_mod("w120/w121_neg_call.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w121_neg_mul() {
    let src = check_mod("w120/w121_neg_mul.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w121_neg_hash() {
    let src = check_mod("w120/w121_neg_hash.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w121_neg_static() {
    let src = check_mod("w120/w121_neg_static.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w122_inf() {
    let src = check_mod("w120/w122_inf.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert!(
        rust[0].message.contains("'steady'"),
        "command steady, got {}",
        rust[0].message
    );
}

#[test]
fn w122_capital_inf() {
    let src = check_mod("w120/w122_Inf.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "W122");
    assert!(
        rust[0].message.contains("assigned Inf"),
        "kind Inf, got {}",
        rust[0].message
    );
}

#[test]
fn w122_nan() {
    let src = check_mod("w120/w122_nan.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert!(
        rust[0].message.contains("assigned NaN"),
        "kind NaN, got {}",
        rust[0].message
    );
}

#[test]
fn w122_0inf() {
    let src = check_mod("w120/w122_0inf.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert!(
        rust[0].message.contains("assigned a non-finite value"),
        "kind, got {}",
        rust[0].message
    );
}

#[test]
fn w122_1e400() {
    let src = check_mod("w120/w122_1e400.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert!(
        rust[0].message.contains("assigned Inf"),
        "kind Inf, got {}",
        rust[0].message
    );
}

#[test]
fn w122_div0() {
    let src = check_mod("w120/w122_div0.mod");
    let mut rust = rust_family(&src);
    rust.sort();
    assert_eq!(rust.len(), 1, "native W122 on 1/0, got {rust:?}");
    assert_eq!(rust[0].code, "W122");
    assert_eq!(rust[0].severity, 2);
    assert_eq!(
        rust[0].message,
        "Parameter 'rho' is assigned Inf. Dynare requires every deep parameter used in the model to be finite before running 'steady'."
    );
    assert_eq!(
        (
            rust[0].start_line,
            rust[0].start_char,
            rust[0].end_line,
            rust[0].end_char
        ),
        range_of(&src, "rho = 1/0;")
    );
}

#[test]
fn w122_later_finite() {
    let src = check_mod("w120/w122_later_ok.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w122_later_inf() {
    let src = check_mod("w120/w122_later_inf.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(
        (
            rust[0].start_line,
            rust[0].start_char,
            rust[0].end_line,
            rust[0].end_char
        ),
        range_of(&src, "rho = Inf;")
    );
}

#[test]
fn w122_before_stoch() {
    let src = check_mod("w120/w122_before.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert!(
        rust[0].message.contains("'stoch_simul'"),
        "command stoch_simul, got {}",
        rust[0].message
    );
}

#[test]
fn w122_after_all() {
    let src = check_mod("w120/w122_after.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w122_no_run() {
    let src = check_mod("w120/w122_norun.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w122_unused() {
    let src = check_mod("w120/w122_unused.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w122_gy() {
    let src = check_mod("w120/w122_gy.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w122_ss() {
    let src = check_mod("w120/w122_ss.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w122_helper() {
    let src = check_mod("w120/w122_helper.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w122_auto() {
    let src = check_mod("w120/w122_auto.mod");
    assert!(rust_family(&src).is_empty());
}
