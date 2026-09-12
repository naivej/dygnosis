use std::path::PathBuf;

use dygnosis::{analyze, apply_fix, auto_fix, has_structural_error, parse, TextEdit};

const OUT: &[&str] = &["E040", "W040", "W041", "I041"];

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

fn rust_thin_codes(text: &str) -> Vec<String> {
    let mut codes: Vec<String> = analyze(&parse(text))
        .into_iter()
        .map(|d| d.code)
        .filter(|c| !OUT.contains(&c.as_str()))
        .collect();
    codes.sort();
    codes.dedup();
    codes
}

fn te(start_line: u32, start_char: u32, end_line: u32, end_char: u32, new_text: &str) -> TextEdit {
    TextEdit {
        start_line,
        start_char,
        end_line,
        end_char,
        new_text: new_text.to_string(),
    }
}

fn delete_model_end() -> String {
    check_mod("e001/delete_model_end.mod")
}

fn delete_ss_end() -> String {
    check_mod("e001/delete_ss_end.mod")
}

fn delete_shocks_end() -> String {
    check_mod("e001/delete_shocks_end.mod")
}

fn strip_var_semi() -> String {
    check_mod("e001/strip_var_semi.mod")
}

fn strip_last_eq_semi() -> String {
    check_mod("e001/strip_last_eq.mod")
}

fn shocks_var_no_semi() -> String {
    check_mod("e001/shocks_var_no_semi.mod")
}

fn typo_mdoel() -> String {
    check_mod("e001/typo_mdoel.mod")
}

fn join_two_model_eqs() -> String {
    check_mod("e001/join_two_eqs.mod")
}

fn unbalanced_paren() -> String {
    check_mod("e001/unbalanced_paren.mod")
}

fn invalid_ident() -> String {
    check_mod("e001/invalid_ident.mod")
}

fn reserved_log() -> String {
    check_mod("e001/reserved_log.mod")
}

fn strip_betta_semi() -> String {
    check_mod("e001/strip_betta_semi.mod")
}

fn join_two_param_assigns() -> String {
    check_mod("e001/join_two_params.mod")
}

fn refuse_macro() -> String {
    check_mod("e001/refuse_macro.mod")
}

#[test]
fn apply_fix_table() {
    let cases = vec![
        ("insert", "ab", vec![te(0, 1, 0, 1, "X")], "aXb"),
        ("replace", "ab", vec![te(0, 0, 0, 1, "X")], "Xb"),
        (
            "999999 clamp",
            "ab",
            vec![te(0, 999_999, 0, 999_999, "X")],
            "abX",
        ),
        (
            "unicode scalar insert",
            "产出",
            vec![te(0, 1, 0, 1, "!")],
            "产!出",
        ),
        (
            "overlapping skip",
            "abcdef",
            vec![te(0, 0, 0, 3, "X"), te(0, 2, 0, 5, "Y")],
            "abYf",
        ),
        (
            "two non-overlapping inserts",
            "ab\ncd",
            vec![te(0, 0, 0, 0, "1"), te(1, 0, 1, 0, "2")],
            "1ab\n2cd",
        ),
    ];
    for (name, text, edits, want) in cases {
        assert_eq!(apply_fix(text, &edits), want, "{name}");
    }
}

#[test]
fn apply_identity_on_clean_archives() {
    for name in ["trend_rbc_gov_inv", "sims_wu_2019", "lk2024"] {
        let text = read_mod(name);
        assert_eq!(auto_fix(&text), text, "{name} auto_fix should be identity");
    }
}

#[test]
fn apply_required_e001_rows() {
    let rows: Vec<(&str, String, &str)> = vec![
        ("delete_model_end", delete_model_end(), "end;"),
        ("delete_ss_end", delete_ss_end(), "end;"),
        ("delete_shocks_end", delete_shocks_end(), "end;"),
        ("strip_var_semi", strip_var_semi(), "var y;"),
        (
            "strip_last_eq_semi",
            strip_last_eq_semi(),
            "y = rho * y(-1) + e;",
        ),
        ("shocks_var_no_semi", shocks_var_no_semi(), "var e;"),
        ("typo_mdoel", typo_mdoel(), "model;"),
        (
            "join_two_model_eqs",
            join_two_model_eqs(),
            "y = rho * y(-1) + e;",
        ),
    ];
    for (label, text, restored) in rows {
        let fixed = auto_fix(&text);
        assert_ne!(fixed, text, "{label} should change");
        assert!(
            fixed.contains(restored),
            "{label} should restore {restored:?}, got {fixed:?}"
        );
        assert!(
            !has_structural_error(&parse(&fixed)),
            "{label} should clear structural error"
        );
    }
    let govt = read_mod("govt_rbc_irf_matching");
    let govt_fixed = auto_fix(&govt);
    assert_ne!(
        govt_fixed, govt,
        "govt_rbc should insert `;` on MoM option lines"
    );
    assert!(govt_fixed.contains("mom_method                = irf_matching,;"));
}

#[test]
fn apply_strip_betta_semi_missing_semi_only() {
    let mutated = strip_betta_semi();
    let fixed = auto_fix(&mutated);
    assert_ne!(
        fixed, mutated,
        "missing-`;` should be applied, got {fixed:?}"
    );
    assert!(
        fixed.contains("betta = 0.99;"),
        "missing-`;` should restore the semicolon; got {fixed:?}"
    );
    assert!(
        !fixed.contains("; gam     = 0.005;"),
        "do not apply Python's extra `; ` prefix"
    );
    assert!(!has_structural_error(&parse(&fixed)));
}

#[test]
fn apply_join_two_param_assigns_missing_semi_only() {
    let mutated = join_two_param_assigns();
    let fixed = auto_fix(&mutated);
    assert_ne!(fixed, mutated, "missing-`;` should be applied");
    assert!(
        !fixed.contains("; gam     = 0.005;"),
        "do not apply Python's extra `; ` prefix"
    );
    assert!(!has_structural_error(&parse(&fixed)));
}

#[test]
fn cascade_clean_trend_is_empty() {
    let text = trend();
    assert!(rust_thin_codes(&text).is_empty());
}

#[test]
fn cascade_delete_model_end_is_e001_only() {
    let text = delete_model_end();
    let rust = rust_thin_codes(&text);
    assert_eq!(rust, vec!["E001".to_string()]);
    let all = analyze(&parse(&text));
    assert!(all.iter().all(|d| d.code == "E001"));
    for skip in ["W013", "E020", "E030", "W054", "W010"] {
        assert!(
            !all.iter().any(|d| d.code == skip),
            "cascade should suppress {skip}, got {:?}",
            all.iter().map(|d| &d.code).collect::<Vec<_>>()
        );
    }
}

#[test]
fn cascade_govt_rbc_is_e001_only() {
    let text = read_mod("govt_rbc_irf_matching");
    assert_eq!(rust_thin_codes(&text), vec!["E001".to_string()]);
}

#[test]
fn analyze_emits_w120_when_no_e001() {
    let text = check_mod("w120/w120_det.mod");
    let all = analyze(&parse(&text));
    assert!(
        !all.iter().any(|d| d.code == "E001"),
        "this mutation must stay parse-clean, got {:?}",
        all.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
    assert!(
        all.iter().any(|d| d.code == "W120"),
        "analyze() should include the W120 family, got {:?}",
        all.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
}

#[test]
fn auto_fix_unbalanced_paren_is_noop() {
    let text = unbalanced_paren();
    assert_eq!(auto_fix(&text), text);
    assert!(has_structural_error(&parse(&text)));
    assert!(rust_thin_codes(&text).contains(&"E001".to_string()));
}

#[test]
fn auto_fix_invalid_ident_is_noop() {
    let text = invalid_ident();
    assert_eq!(auto_fix(&text), text);
    assert!(has_structural_error(&parse(&text)));
    assert!(rust_thin_codes(&text).contains(&"E001".to_string()));
}

#[test]
fn auto_fix_reserved_log_is_noop() {
    let text = reserved_log();
    assert_eq!(auto_fix(&text), text);
    assert!(has_structural_error(&parse(&text)));
    assert!(rust_thin_codes(&text).contains(&"E001".to_string()));
}

#[test]
fn auto_fix_refuse_macro() {
    let text = refuse_macro();
    assert_eq!(auto_fix(&text), text);
    assert!(has_structural_error(&parse(&text)));
    assert!(rust_thin_codes(&text).contains(&"E001".to_string()));
}
