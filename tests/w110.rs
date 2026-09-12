use std::path::PathBuf;

use dygnosis::span::LineIndex;
use dygnosis::{check_w110, parse, ShockKind};

const CLEAN: &[&str] = &[
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

fn rust_family(text: &str) -> Vec<Diag> {
    let model = parse(text);
    let index = LineIndex::new(&model.source);
    check_w110(&model)
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

fn offset_at(text: &str, line: u32, character: u32) -> usize {
    let mut off = 0usize;
    for (i, row) in text.split('\n').enumerate() {
        if i as u32 == line {
            for (n, (byte, _)) in row.char_indices().enumerate() {
                if n as u32 == character {
                    return off + byte;
                }
            }
            return off + row.len();
        }
        off += row.len() + 1;
    }
    off
}

fn underlined(text: &str, d: &Diag) -> String {
    let start = offset_at(text, d.start_line, d.start_char);
    let end = offset_at(text, d.end_line, d.end_char);
    text[start..end].to_string()
}

fn assert_stmt_span(text: &str, d: &Diag, label: &str) {
    let s = underlined(text, d);
    assert!(
        s.starts_with("var") || s.starts_with("corr"),
        "{label} {}: range should start at var/corr, got {s:?}",
        d.code
    );
    assert!(
        s.ends_with(';'),
        "{label} {}: range should run through ';', got {s:?}",
        d.code
    );
    assert_eq!(
        s.matches(';').count(),
        1,
        "{label} {}: statement span should contain one ';', got {s:?}",
        d.code
    );
    assert!(
        !s.to_ascii_lowercase().contains("shocks"),
        "{label} {}: statement span must not be the whole shocks block, got {s:?}",
        d.code
    );
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
fn w110_clean_four_empty() {
    for archive in CLEAN {
        let text = read_mod(archive);
        let rust = rust_family(&text);
        assert!(
            rust.is_empty(),
            "clean {archive} should have empty FAMILY, got {rust:?}"
        );
    }
}

#[test]
fn w110_parser_clean_var_stmts() {
    let model = parse(&trend());
    assert_eq!(model.shock_stmts.len(), 2);
    let names: Vec<&str> = model
        .shock_stmts
        .iter()
        .map(|s| match &s.kind {
            ShockKind::Var(n) => model.name(*n),
            other => panic!("expected Var, got {other:?}"),
        })
        .collect();
    assert_eq!(names, ["eps_z", "eps_ig"]);
    assert!(model.shock_stmts.iter().all(|s| s.rhs.is_none()));
}

#[test]
fn w110_parser_corr_1p5() {
    let src = check_mod("w110/w110_1p5.mod");
    let model = parse(&src);
    let corrs: Vec<_> = model
        .shock_stmts
        .iter()
        .filter(|s| matches!(s.kind, ShockKind::Corr { .. }))
        .collect();
    assert_eq!(corrs.len(), 1);
    match &corrs[0].kind {
        ShockKind::Corr { a, b } => {
            assert_eq!(model.name(*a), "e");
            assert_eq!(model.name(*b), "u");
        }
        other => panic!("expected Corr, got {other:?}"),
    }
    assert_eq!(corrs[0].rhs, Some(1.5));
}

#[test]
fn w110_w060_drop() {
    let src = check_mod("w110/w060_none.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "W060");
    assert!(rust[0].message.contains("e, u"));
    let underlined = underlined(&src, &rust[0]);
    assert_eq!(underlined, "e");
}

#[test]
fn w110_1p5() {
    let src = check_mod("w110/w110_1p5.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "W110");
    assert!(rust[0].message.contains("1.5"));
    assert_stmt_span(&src, &rust[0], "w110_1p5");
    assert_span(&src, &rust[0], "corr e, u = 1.5;");
}

#[test]
fn w110_m1p2() {
    let src = check_mod("w110/w110_m1p2.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "W110");
    assert!(rust[0].message.contains("-1.2"));
    assert_stmt_span(&src, &rust[0], "w110_m1p2");
    assert_span(&src, &rust[0], "corr e, u = -1.2;");
}

#[test]
fn w110_ok() {
    let src = check_mod("w110/w110_ok.mod");
    let rust = rust_family(&src);
    assert!(rust.is_empty(), "corr 0.5 should be empty, got {rust:?}");
}

#[test]
fn w110_corr_abs_1_ok() {
    let src = check_mod("w110/w110_corr1.mod");
    let rust = rust_family(&src);
    assert!(
        rust.is_empty(),
        "|corr| == 1 is in bounds; expected no W110, got {rust:?}"
    );
}

#[test]
fn w110_w111_dup_var() {
    let src = check_mod("w110/w111_dup_var.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "E111");
    assert_eq!(rust[0].severity, 1);
    assert!(rust[0]
        .message
        .contains("variance/standard error specified more than once"));
    assert_stmt_span(&src, &rust[0], "w111_dup_var");
}

#[test]
fn w110_w111_dup_corr() {
    let src = check_mod("w110/w111_dup_corr.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "E111");
    assert_eq!(rust[0].severity, 1);
    assert!(
        rust[0].message.contains("'e'") && rust[0].message.contains("'u'"),
        "dup corr message should name e and u, got {}",
        rust[0].message
    );
    assert!(
        rust[0]
            .message
            .starts_with("Correlation between 'u' and 'e'"),
        "second pair order, got {}",
        rust[0].message
    );
    assert_stmt_span(&src, &rust[0], "w111_dup_corr");
    assert_span(&src, &rust[0], "corr u, e = 0.2;");
}

#[test]
fn w110_w112_neg() {
    let src = check_mod("w110/w112_neg.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "W112");
    assert!(rust[0].message.contains("-0.01"));
    assert_stmt_span(&src, &rust[0], "w112_neg");
    assert_span(&src, &rust[0], "var e = -0.01;");
}

#[test]
fn w110_w112_stderr_neg() {
    let src = check_mod("w110/w112_stderr.mod");
    let rust = rust_family(&src);
    assert!(
        rust.is_empty(),
        "negative stderr should be zero, got {rust:?}"
    );
}

#[test]
fn w110_w112_cov_neg() {
    let src = check_mod("w110/w112_cov.mod");
    let rust = rust_family(&src);
    assert!(
        rust.is_empty(),
        "negative covariance should be zero W112, got {rust:?}"
    );
}
