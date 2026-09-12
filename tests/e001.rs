use std::path::PathBuf;

use dygnosis::span::LineIndex;
use dygnosis::{analyze, check_parse, has_structural_error, parse, TextEdit};

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
    fix: Option<TextEdit>,
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

fn rust_e001(text: &str) -> Vec<Diag> {
    let model = parse(text);
    let index = LineIndex::new(&model.source);
    check_parse(&model)
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
                fix: d.fix,
            }
        })
        .collect()
}

fn missing_semi_only(rows: Vec<Diag>) -> Vec<Diag> {
    rows.into_iter()
        .filter(|d| !d.message.contains("merged due to a missing semicolon"))
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
    let got = rust_e001(&text);
    assert_eq!(got.len(), 1, "{rel}: {got:?}");
    assert_eq!(got[0].code, "E001");
    assert_eq!(got[0].severity, 1);
    assert!(
        got[0].message.contains(msg_sub),
        "{rel}: missing {msg_sub:?} in {}",
        got[0].message
    );
    assert_span(&text, &got[0], needle);
}

#[test]
fn e001_clean_archive_files_are_empty() {
    for name in ARCHIVES {
        let text = read_mod(name);
        let rust = rust_e001(&text);
        if *name != "govt_rbc_irf_matching" {
            assert!(rust.is_empty(), "{name}: expected no E001, got {rust:?}");
            let model = parse(&text);
            assert!(
                !has_structural_error(&model),
                "{name} should not have structural errors"
            );
        } else {
            assert!(
                rust.iter().any(|d| d.code == "E001"),
                "govt_rbc_irf_matching should emit E001, got {rust:?}"
            );
        }
    }
}

#[test]
fn e001_delete_model_end() {
    let text = check_mod("e001/delete_model_end.mod");
    let got = rust_e001(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "E001");
    assert!(got[0].message.contains("Missing 'end;' for 'model'"));
    assert_span(&text, &got[0], "model;");
    assert!(has_structural_error(&parse(&text)));
}

#[test]
fn e001_delete_ss_end() {
    assert_fire(
        "e001/delete_ss_end.mod",
        "Missing 'end;' for 'steady_state_model'",
        "steady_state_model;",
    );
}

#[test]
fn e001_delete_shocks_end() {
    assert_fire(
        "e001/delete_shocks_end.mod",
        "Missing 'end;' for 'shocks'",
        "shocks;",
    );
}

#[test]
fn e001_strip_var_semicolon() {
    assert_fire(
        "e001/strip_var_semi.mod",
        "Declaration 'var' appears to be missing its terminating semicolon",
        "var ",
    );
}

#[test]
fn e001_strip_betta_semicolon() {
    let text = check_mod("e001/strip_betta_semi.mod");
    let got = missing_semi_only(rust_e001(&text));
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "E001");
    assert!(got[0]
        .message
        .contains("Parameter assignment 'betta' is missing its terminating semicolon"));
    assert_span(&text, &got[0], "betta = 0.99");
}

#[test]
fn e001_strip_last_model_equation_semicolon() {
    assert_fire(
        "e001/strip_last_eq.mod",
        "Statement in 'model' block is missing its terminating semicolon",
        "y = rho * y(-1) + e",
    );
}

#[test]
fn e001_join_two_model_equations() {
    let text = check_mod("e001/join_two_eqs.mod");
    let got = rust_e001(&text);
    assert_eq!(got.len(), 1);
    assert!(got[0].message.contains("merged due to a missing semicolon"));
    assert_span(&text, &got[0], "y = rho * y(-1) + e\nc = betta * c(+1)");
}

#[test]
fn e001_join_two_param_assignments() {
    let text = check_mod("e001/join_two_params.mod");
    let got = missing_semi_only(rust_e001(&text));
    assert_eq!(got.len(), 1);
    assert!(got[0]
        .message
        .contains("Parameter assignment 'betta' is missing its terminating semicolon"));
    assert_span(&text, &got[0], "betta = 0.99");
}

#[test]
fn e001_shocks_var_missing_semicolon() {
    assert_fire(
        "e001/shocks_var_no_semi.mod",
        "Missing semicolon in shocks block before 'stderr'",
        "var e ",
    );
}

#[test]
fn e001_keyword_typo_mdoel() {
    let text = check_mod("e001/typo_mdoel.mod");
    let got = rust_e001(&text);
    assert_eq!(got.len(), 1);
    assert!(got[0]
        .message
        .contains("Possible misspelling of 'model' keyword: 'mdoel'"));
    assert_span_in(&text, &got[0], "mdoel;", "mdoel");
}

#[test]
fn e001_variable_inside_model_is_not_var_typo() {
    let text = check_mod("e001/variable_ident.mod");
    let rust = rust_e001(&text);
    assert!(
        rust.iter()
            .all(|d| !d.message.contains("Possible misspelling of 'var'")),
        "inside-block `variable` must not be a var typo: {rust:?}"
    );
}

#[test]
fn e001_unbalanced_paren() {
    assert_fire(
        "e001/unbalanced_paren.mod",
        "Unbalanced parentheses in equation: unmatched ')'",
        "y = rho * y(-1) + e))",
    );
}

#[test]
fn e001_invalid_ident_c_minus_x() {
    assert_fire(
        "e001/invalid_ident.mod",
        "Invalid Dynare identifier 'c-x'",
        "c-x",
    );
}

#[test]
fn e001_reserved_parameters_log() {
    let text = check_mod("e001/reserved_log.mod");
    let got = rust_e001(&text);
    assert_eq!(got.len(), 1);
    assert!(got[0].message.contains("Invalid Dynare identifier 'log'"));
    assert_span_in(&text, &got[0], "parameters log;", "log");
}

#[test]
fn e001_cascade_missing_model_end() {
    let text = check_mod("e001/delete_model_end.mod");
    let model = parse(&text);
    assert!(has_structural_error(&model));
    let rust = rust_e001(&text);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "E001");

    let all = analyze(&model);
    assert!(all.iter().any(|d| d.code == "E001"));
    assert!(
        all.iter().all(|d| d.code == "E001"),
        "analyze() skips later families when E001 is present, got {:?}",
        all.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
    for skip in ["W013", "E020", "E030", "W054", "W010"] {
        assert!(
            !all.iter().any(|d| d.code == skip),
            "analyze() cascade should suppress {skip}, got {:?}",
            all.iter().map(|d| &d.code).collect::<Vec<_>>()
        );
    }
}
