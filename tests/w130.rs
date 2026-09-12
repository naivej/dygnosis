use std::path::PathBuf;

use dygnosis::span::LineIndex;
use dygnosis::{check_w130, parse};

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
    tags: Vec<i32>,
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
    check_w130(&model)
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
                tags: d.tags,
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

fn last_ident_in(text: &str, context: &str, ident: &str) -> (u32, u32, u32, u32) {
    let at = text
        .find(context)
        .unwrap_or_else(|| panic!("missing context {context:?}"));
    assert_eq!(
        &text[at..at + context.len()],
        context,
        "context slice mismatch"
    );
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

fn assert_native_range(text: &str, d: &Diag, needle: &str) {
    assert_eq!(
        (d.start_line, d.start_char, d.end_line, d.end_char),
        range_of(text, needle),
        "native range should be {needle:?}, got {d:?}"
    );
}

fn assert_w140_ops(id: &str, needle: &str) {
    let src = check_mod("w130/w140_ops.mod");
    let rust = rust_family(&src);
    let want = range_of(&src, needle);
    let w140s: Vec<_> = rust.iter().filter(|d| d.code == "W140").collect();
    assert_eq!(w140s.len(), 18, "{id} W140 count, got {rust:?}");
    let matched: Vec<_> = w140s
        .iter()
        .filter(|d| (d.start_line, d.start_char, d.end_line, d.end_char) == want)
        .collect();
    assert_eq!(matched.len(), 1, "{id} W140 at {needle:?}, got {rust:?}");
}

fn assert_w140_quiet(id: &str, rel: &str) {
    let src = check_mod(rel);
    let rust = rust_family(&src);
    assert!(rust.is_empty(), "{id} should be empty FAMILY, got {rust:?}");
}

#[test]
fn w130_clean_four_empty() {
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
fn w130_logn_before_n() {
    let src = check_mod("w130/w130_order.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "E130");
    assert_eq!(rust[0].severity, 1);
    assert_eq!(
        (
            rust[0].start_line,
            rust[0].start_char,
            rust[0].end_line,
            rust[0].end_char
        ),
        last_ident_in(&src, "log_n = log(n);", "n")
    );
}

#[test]
fn w130_k_before_y() {
    let src = check_mod("w130/w130_k_before_y.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "E130");
    assert_eq!(rust[0].severity, 1);
    assert_eq!(
        (
            rust[0].start_line,
            rust[0].start_char,
            rust[0].end_line,
            rust[0].end_char
        ),
        last_ident_in(&src, "steady_state_model;\nk = y;", "y")
    );
}

#[test]
fn w130_timed_rhs_native() {
    let src = check_mod("w130/w130_timed.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1, "native E130 on y(-1), got {rust:?}");
    assert_eq!(rust[0].code, "E130");
    assert_eq!(rust[0].severity, 1);
    assert!(
        rust[0].message.starts_with("'y'"),
        "E130 on y, got {}",
        rust[0].message
    );
    assert_eq!(
        (
            rust[0].start_line,
            rust[0].start_char,
            rust[0].end_line,
            rust[0].end_char
        ),
        last_ident_in(&src, "k = y(-1);", "y")
    );
}

#[test]
fn w131_n_zero() {
    let src = check_mod("w130/w131_zero.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "W131");
    assert_native_range(&src, &rust[0], "n = 0");
}

#[test]
fn w131_n_const() {
    let src = check_mod("w130/w131_const.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(
        (
            rust[0].start_line,
            rust[0].start_char,
            rust[0].end_line,
            rust[0].end_char
        ),
        range_in(&src, "n = 1/3;", "n = 1/3")
    );
}

#[test]
fn w131_log_ok() {
    let src = check_mod("w130/w131_log.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w131_exp_ok() {
    let src = check_mod("w130/w131_exp.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w131_lag_ok() {
    let src = check_mod("w130/w131_lag.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w131_psi_param() {
    let src = check_mod("w130/w131_param.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w131_nss_helper() {
    let src = check_mod("w130/w131_helper.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w042_not_w130() {
    let src = check_mod("w130/w042_not_w130.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w150_simul() {
    let src = check_mod("w130/w150_simul.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].tags, vec![2]);
    assert_eq!(rust[0].end_char - rust[0].start_char, 5);
}

#[test]
fn w150_simul_paren() {
    let src = check_mod("w130/w150_simul_paren.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].end_char - rust[0].start_char, 5);
    assert_eq!(rust[0].tags, vec![2]);
}

#[test]
fn w150_two_simul() {
    let src = check_mod("w130/w150_two.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 2);
    assert!(rust.iter().all(|d| d.code == "W150" && d.tags == vec![2]));
}

#[test]
fn w150_ramsey() {
    let src = check_mod("w130/w150_ramsey.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].end_char - rust[0].start_char, 13);
    assert_eq!(rust[0].tags, vec![2]);
}

#[test]
fn w150_aim() {
    let src = check_mod("w130/w150_aim.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].tags, vec![2]);
    assert_native_range(&src, &rust[0], "aim_solver");
}

#[test]
fn w150_bytecode() {
    let src = check_mod("w130/w150_bytecode.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(
        (
            rust[0].start_line,
            rust[0].start_char,
            rust[0].end_line,
            rust[0].end_char
        ),
        range_in(&src, "stoch_simul(order=1, bytecode)", "bytecode")
    );
}

#[test]
fn w150_declared_bytecode() {
    let src = check_mod("w130/w150_declared.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w150_comment_simul() {
    let src = check_mod("w130/w150_comment.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w150_string_bytecode() {
    let src = check_mod("w130/w150_string.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w150_perfect_ok() {
    let src = check_mod("w130/w150_perfect.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w150_equation_bytecode_native() {
    let src = check_mod("w130/w150_eq.mod");
    let rust: Vec<_> = rust_family(&src)
        .into_iter()
        .filter(|d| d.code == "W150")
        .collect();
    assert!(
        rust.is_empty(),
        "equation-text bytecode must not be W150, got {rust:?}"
    );
}

#[test]
fn w140_log() {
    assert_w140_ops("w140_log", "log(y_log)");
}

#[test]
fn w140_exp() {
    assert_w140_ops("w140_exp", "exp(y_exp)");
}

#[test]
fn w140_abs() {
    assert_w140_ops("w140_abs", "abs(y_abs)");
}

#[test]
fn w140_abs_upper() {
    assert_w140_ops("w140_ABS", "ABS(y_ABS)");
}

#[test]
fn w140_max() {
    assert_w140_ops("w140_max", "max(y_max, 0)");
}

#[test]
fn w140_min() {
    assert_w140_ops("w140_min", "min(y_min, 0)");
}

#[test]
fn w140_sign() {
    assert_w140_ops("w140_sign", "sign(y_sign)");
}

#[test]
fn w140_sin() {
    assert_w140_ops("w140_sin", "sin(y_sin)");
}

#[test]
fn w140_foo() {
    assert_w140_ops("w140_foo", "foo(y_foo)");
}

#[test]
fn w140_pow2() {
    assert_w140_ops("w140_pow2", "y_pow(+1)^2");
}

#[test]
fn w140_pow1() {
    assert_w140_quiet("w140_pow1", "w130/w140_pow1.mod");
}

#[test]
fn w140_pow0() {
    assert_w140_quiet("w140_pow0", "w130/w140_pow0.mod");
}

#[test]
fn w140_pow1p0() {
    assert_w140_quiet("w140_pow1p0", "w130/w140_pow1p0.mod");
}

#[test]
fn w140_mul_vars() {
    assert_w140_ops("w140_mul_vars", "y_mul * y_log");
}

#[test]
fn w140_mul_param() {
    assert_w140_quiet("w140_mul_param", "w130/w140_mul_param.mod");
}

#[test]
fn w140_div_var() {
    assert_w140_ops("w140_div_var", "2 / y_div");
}

#[test]
fn w140_div_const() {
    assert_w140_quiet("w140_div_const", "w130/w140_div_const.mod");
}

#[test]
fn w140_div_two() {
    assert_w140_ops("w140_div_two", "y_div2 / y_log");
}

#[test]
fn w140_cmp() {
    assert_w140_ops("w140_cmp", "y_cmp > 0");
}

#[test]
fn w140_log_times() {
    assert_w140_ops("w140_log_times", "log(y_lt)*y_log");
}

#[test]
fn w140_log_pow() {
    assert_w140_ops("w140_log_pow", "log(y_lp)^2");
}

#[test]
fn w140_ss() {
    assert_w140_quiet("w140_ss", "w130/w140_ss.mod");
}

#[test]
fn w140_ss_log() {
    assert_w140_quiet("w140_ss_log", "w130/w140_ss_log.mod");
}

#[test]
fn w140_expect() {
    assert_w140_quiet("w140_expect", "w130/w140_expect.mod");
}

#[test]
fn w140_expect_log() {
    assert_w140_ops("w140_expect_log", "log(y_el)");
}

#[test]
fn w140_diff() {
    assert_w140_quiet("w140_diff", "w130/w140_diff.mod");
}

#[test]
fn w140_diff_log() {
    assert_w140_ops("w140_diff_log", "log(y_dl)");
}

#[test]
fn w140_local_abs() {
    let src = check_mod("w130/w140_local.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "W140");
    assert_native_range(&src, &rust[0], "abs(x)");
}

#[test]
fn w140_log_param() {
    assert_w140_quiet("w140_log_param", "w130/w140_log_param.mod");
}

#[test]
fn w140_not_linear() {
    let src = check_mod("w130/w140_not_linear.mod");
    assert!(rust_family(&src).is_empty());
}

#[test]
fn w140_two_eqs() {
    let src = check_mod("w130/w140_two.mod");
    let rust = rust_family(&src);
    let w140s: Vec<_> = rust.iter().filter(|d| d.code == "W140").collect();
    assert_eq!(w140s.len(), 2, "two W140, got {rust:?}");
    assert_native_range(&src, w140s[0], "log(x)");
    assert_native_range(&src, w140s[1], "abs(pi)");
}

#[test]
fn w150_model_bytecode() {
    let src = check_mod("w130/w150_model.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].tags, vec![2]);
    assert_eq!(
        (
            rust[0].start_line,
            rust[0].start_char,
            rust[0].end_line,
            rust[0].end_char
        ),
        range_in(&src, "model(linear, bytecode)", "bytecode")
    );
}
