use std::path::PathBuf;

use dygnosis::span::{LineIndex, Position};
use dygnosis::{analyze, parse};

const FAMILY: &[&str] = &[
    "E050", "E051", "E052", "E053", "W042", "W050", "W051", "W052", "W053", "I050",
];
const OUT: &[&str] = &["E040", "W040", "W041", "I041"];
const I050_MESSAGE: &str = "No initval or steady_state_model block. Add an initval block with initial guesses, or a steady_state_model block with closed-form assignments.";

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
    let all = analyze(&model);
    assert!(
        all.iter().all(|d| !OUT.contains(&d.code.as_str())),
        "Rust emitted Out SS codes: {:?}",
        all.iter()
            .filter(|d| OUT.contains(&d.code.as_str()))
            .map(|d| &d.code)
            .collect::<Vec<_>>()
    );
    all.into_iter()
        .filter(|d| FAMILY.contains(&d.code.as_str()))
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

fn underlined(text: &str, d: &Diag) -> String {
    let index = LineIndex::new(text);
    let start = index.offset(
        text,
        Position {
            line: d.start_line,
            character: d.start_char,
        },
    );
    let end = index.offset(
        text,
        Position {
            line: d.end_line,
            character: d.end_char,
        },
    );
    text[start as usize..end as usize].to_string()
}

fn find_code<'a>(got: &'a [Diag], code: &str) -> &'a Diag {
    got.iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("missing {code}, got {got:?}"))
}

fn assert_i050_forbidden(message: &str) {
    for forbidden in [
        "Compute Steady State",
        "code action",
        "Gauss-Seidel",
        "trust-region",
        "homotopy",
        "random restarts",
    ] {
        assert!(
            !message.contains(forbidden),
            "I050 message contains forbidden {forbidden:?}"
        );
    }
}

fn assert_i050_model_block(text: &str, d: &Diag) {
    assert_eq!(d.code, "I050");
    assert_eq!(d.severity, 3);
    assert_eq!(d.message, I050_MESSAGE);
    let slice = underlined(text, d);
    assert!(
        slice.starts_with("model"),
        "I050 underline should start with model, got {slice:?}"
    );
    assert!(
        slice.ends_with("end;"),
        "I050 underline should end with end;, got {slice:?}"
    );
}

#[test]
fn shape_clean_trend_rbc_gov_inv() {
    let got = rust_family(&read_mod("trend_rbc_gov_inv"));
    assert!(got.is_empty(), "clean FAMILY should be empty, got {got:?}");
}

#[test]
fn shape_clean_sims_wu_2019() {
    let text = read_mod("sims_wu_2019");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1, "sims_wu_2019: {got:?}");
    assert_i050_model_block(&text, &got[0]);
}

#[test]
fn shape_clean_govt_rbc_irf_matching() {
    let got = rust_family(&read_mod("govt_rbc_irf_matching"));
    assert!(
        got.is_empty(),
        "govt_rbc_irf_matching FAMILY empty (E001 cascade), got {got:?}"
    );
}

#[test]
fn shape_clean_lk2024() {
    let text = read_mod("lk2024");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1, "lk2024: {got:?}");
    assert_i050_model_block(&text, &got[0]);
}

#[test]
fn shape_e050_duplicate_equation() {
    let text = check_mod("shape/e050_dup.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].code, "E050");
    assert_eq!(got[0].severity, 1);
    assert!(got[0].message.contains("Duplicate equation (same as line 9)."));
    assert_span_in(
        &text,
        &got[0],
        "c = betta * c(+1);\ny = rho * y(-1) + e;",
        "y = rho * y(-1) + e",
    );
}

#[test]
fn shape_e051_contradictory() {
    let text = check_mod("shape/e051_false.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].code, "E051");
    assert_eq!(got[0].severity, 1);
    assert!(got[0].message.contains("Contradictory equation '0 = 1'"));
    assert_span(&text, &got[0], "0 = 1");
}

#[test]
fn shape_e051_trivially_true() {
    let text = check_mod("shape/e051_true.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].code, "E051");
    assert_eq!(got[0].severity, 1);
    assert!(got[0].message.contains("Trivially true equation 'y = y'"));
    assert_span(&text, &got[0], "y = y");
}

#[test]
fn shape_e052_duplicate_param() {
    let text = check_mod("shape/e052_dup_param.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].code, "E052");
    assert_eq!(got[0].severity, 2);
    assert!(got[0]
        .message
        .contains("Duplicate parameter assignment 'betta = 0.99'"));
    assert_span_in(
        &text,
        &got[0],
        "betta = 0.99;\n\nmodel;",
        "betta = 0.99;",
    );
}

#[test]
fn shape_e053_stray_top_level() {
    let text = check_mod("shape/e053_stray.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].code, "E053");
    assert_eq!(got[0].severity, 1);
    assert!(got[0].message.contains("Stray equation '0 = 1' outside model block"));
    assert_span(&text, &got[0], "0 = 1;");
}

#[test]
fn shape_e053_comment_is_not_stray() {
    let text = check_mod("shape/e053_stray.mod").replacen("0 = 1;", "// 0 = 1;", 1);
    let got = rust_family(&text);
    assert!(
        got.iter().all(|d| d.code != "E053"),
        "commented number-eq must not be E053: {got:?}"
    );
}

#[test]
fn shape_w042_missing_ss_coverage() {
    let text = check_mod("shape/w042_missing.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].code, "W042");
    assert_eq!(got[0].severity, 2);
    assert!(got[0]
        .message
        .contains("1 endogenous variable(s) missing from steady_state_model: c"));
    assert_span(&text, &got[0], "steady_state_model;\ny = 0;\nend;");
}

#[test]
fn shape_w050_undeclared_initval() {
    let text = check_mod("shape/w050_initval.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 2, "{got:?}");
    let w050 = find_code(&got, "W050");
    assert_eq!(w050.severity, 2);
    assert!(w050
        .message
        .contains("Variable 'undeclared_zzz' in initval is not declared."));
    assert_span(&text, w050, "undeclared_zzz = 1;");
    let w052 = find_code(&got, "W052");
    assert_eq!(w052.severity, 3);
    assert!(w052
        .message
        .contains("2 endogenous variable(s) missing from initval"));
    assert!(w052.message.contains("c, y"));
    assert_span(&text, w052, "initval; undeclared_zzz = 1; end;");
}

#[test]
fn shape_w051_varexo_in_initval() {
    let text = check_mod("shape/w051_varexo.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 2, "{got:?}");
    let w051 = find_code(&got, "W051");
    assert_eq!(w051.severity, 3);
    assert!(w051
        .message
        .contains("Exogenous variable 'e' is set in initval."));
    assert_span(&text, w051, "e = 0.1;");
    let w052 = find_code(&got, "W052");
    assert_eq!(w052.severity, 3);
    assert!(w052
        .message
        .contains("2 endogenous variable(s) missing from initval"));
    assert!(w052.message.contains("c, y"));
    assert_span(&text, w052, "initval; e = 0.1; end;");
}

#[test]
fn shape_w052_missing_initval() {
    let text = check_mod("shape/w052_partial.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].code, "W052");
    assert_eq!(got[0].severity, 3);
    assert!(got[0]
        .message
        .contains("1 endogenous variable(s) missing from initval"));
    assert!(got[0].message.contains(": c"));
    assert_span(&text, &got[0], "initval; y = 1; end;");
}

#[test]
fn shape_w053_param_in_initval() {
    let text = check_mod("shape/w053_initval.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 2, "{got:?}");
    let w053 = find_code(&got, "W053");
    assert_eq!(w053.severity, 2);
    assert!(w053
        .message
        .contains("Parameter 'betta' assigned in initval"));
    assert_span(&text, w053, "betta = 0.5;");
    let w052 = find_code(&got, "W052");
    assert_eq!(w052.severity, 3);
    assert!(w052
        .message
        .contains("2 endogenous variable(s) missing from initval"));
    assert!(w052.message.contains("c, y"));
    assert_span(&text, w052, "initval; betta = 0.5; end;");
}

#[test]
fn shape_i050_missing_ss_recorded_message() {
    let text = check_mod("shape/i050_none.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].code, "I050");
    assert_eq!(got[0].severity, 3);
    assert_eq!(got[0].message, I050_MESSAGE);
    assert_i050_forbidden(&got[0].message);
    assert_span(&text, &got[0], "model;\ny = rho * y(-1) + e;\nend;");
}

#[test]
fn shape_w050_undeclared_endval() {
    let text = check_mod("shape/w050_endval.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].code, "W050");
    assert_eq!(got[0].severity, 2);
    assert!(got[0]
        .message
        .contains("Variable 'undeclared_zzz' in endval is not declared."));
    assert_span(&text, &got[0], "undeclared_zzz = 1;");
}

#[test]
fn shape_w053_param_in_endval() {
    let text = check_mod("shape/w053_endval.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].code, "W053");
    assert_eq!(got[0].severity, 2);
    assert!(got[0]
        .message
        .contains("Parameter 'betta' assigned in endval"));
    assert_span(&text, &got[0], "betta = 0.5;");
}

#[test]
fn shape_static_vs_dynamic_same_body_not_e050() {
    let got = rust_family(&check_mod("shape/e050_static_dynamic.mod"));
    assert!(
        got.iter().all(|d| d.code != "E050"),
        "static vs dynamic same body must not be E050, got {got:?}"
    );
    assert!(got.is_empty(), "expected no shape-family codes, got {got:?}");
}
