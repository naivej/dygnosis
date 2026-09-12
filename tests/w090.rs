use std::path::PathBuf;

use dygnosis::span::LineIndex;
use dygnosis::{check_w090, parse, EstimatedParamKind};

const CLEAN: &[&str] = &["trend_rbc_gov_inv", "sims_wu_2019", "lk2024"];

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
    check_w090(&model)
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

fn offending_name(d: &Diag) -> &str {
    let rest = d
        .message
        .split('\'')
        .nth(1)
        .expect("quoted name in message");
    rest
}

fn ident_offsets(hay: &str, name: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let bytes = hay.as_bytes();
    let mut i = 0;
    while let Some(rel) = hay[i..].find(name) {
        let at = i + rel;
        let before = at
            .checked_sub(1)
            .and_then(|p| bytes.get(p).copied())
            .unwrap_or(b' ');
        let after = bytes.get(at + name.len()).copied().unwrap_or(b' ');
        let word_before = before.is_ascii_alphanumeric() || before == b'_';
        let word_after = after.is_ascii_alphanumeric() || after == b'_';
        if !word_before && !word_after {
            out.push(at);
        }
        i = at + name.len();
    }
    out
}

fn varobs_stmt_range(src: &str) -> (usize, usize) {
    let start = src.find("varobs").expect("varobs statement");
    let end = start + src[start..].find(';').expect("varobs ;") + 1;
    (start, end)
}

fn assert_name_token(text: &str, d: &Diag, occurrence: usize) {
    let name = offending_name(d);
    let (stmt_start, stmt_end) = varobs_stmt_range(text);
    let occs = ident_offsets(&text[stmt_start..stmt_end], name);
    assert!(
        occurrence < occs.len(),
        "expected occurrence {occurrence} of {name:?} in varobs, found {}",
        occs.len()
    );
    let byte_start = (stmt_start + occs[occurrence]) as u32;
    let byte_end = byte_start + name.len() as u32;
    let index = LineIndex::new(text);
    let start = index.position(text, byte_start);
    let end = index.position(text, byte_end);
    assert_eq!(
        (d.start_line, d.start_char, d.end_line, d.end_char),
        (start.line, start.character, end.line, end.character),
        "W090/W091 range should be the {occurrence} ident token {name:?}"
    );
}

#[test]
fn w090_unmodified_govt_empty() {
    let text = read_mod("govt_rbc_irf_matching");
    let rust = rust_family(&text);
    assert!(
        rust.is_empty(),
        "unmodified govt FAMILY should be empty, got {rust:?}"
    );
}

#[test]
fn w090_clean_extra_empty() {
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
fn w090_base_stripped() {
    let src = check_mod("w090/w090_base.mod");
    let model = parse(&src);
    let varobs: Vec<_> = model.varobs.iter().map(|v| model.name(v.name)).collect();
    assert_eq!(varobs, ["log_y", "log_c"]);
    assert_eq!(model.estimated_params.len(), 2);
    let a = &model.estimated_params[0];
    assert_eq!(model.name(a.name), "alphag");
    assert_eq!(a.kind, EstimatedParamKind::Param);
    assert_eq!(a.init, Some(0.08));
    assert_eq!(a.lower, Some(0.01));
    assert_eq!(a.upper, Some(0.15));
    let r = &model.estimated_params[1];
    assert_eq!(model.name(r.name), "rho_ig");
    assert_eq!(r.kind, EstimatedParamKind::Param);
    assert_eq!(r.init, Some(0.88));
    assert_eq!(r.lower, Some(0.50));
    assert_eq!(r.upper, Some(0.995));
    let rust = rust_family(&src);
    assert!(
        rust.is_empty(),
        "stripped base should have empty FAMILY, got {rust:?}"
    );
}

#[test]
fn w090_w091_dup() {
    let src = check_mod("w090/w091_dup.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "W091");
    assert_name_token(&src, &rust[0], 1);
}

#[test]
fn w090_exo() {
    let src = check_mod("w090/w090_exo.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "E090");
    assert_eq!(rust[0].severity, 1);
    assert!(rust[0].message.contains("(it is an exogenous variable)"));
    assert_name_token(&src, &rust[0], 0);
}

#[test]
fn w090_param() {
    let src = check_mod("w090/w090_param.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "E090");
    assert_eq!(rust[0].severity, 1);
    assert!(rust[0].message.contains("(it is a parameter)"));
    assert_name_token(&src, &rust[0], 0);
}

#[test]
fn w090_undecl() {
    let src = check_mod("w090/w090_undecl.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "E090");
    assert_eq!(rust[0].severity, 1);
    assert!(!rust[0].message.contains("(it is an "));
    assert_name_token(&src, &rust[0], 0);
}

#[test]
fn w090_w092_3obs() {
    let src = check_mod("w090/w092_3obs.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "W092");
    assert!(rust[0].message.contains("3 observed"));
    assert!(rust[0].message.contains("only 2 shock"));
    assert_span(&src, &rust[0], "varobs log_y log_c y;");
}

#[test]
fn w090_w092_4obs() {
    let src = check_mod("w090/w092_4obs.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert!(rust[0].message.contains("4 observed"));
    assert!(rust[0].message.contains("only 2 shock"));
    assert_span(&src, &rust[0], "varobs log_y log_c y c;");
}

#[test]
fn w090_w092_4obs_me() {
    let src = check_mod("w090/w092_4obs_me.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert!(rust[0].message.contains("4 observed"));
    assert!(rust[0].message.contains("only 3 shock"));
    assert_span(&src, &rust[0], "varobs log_y log_c y c;");
}

#[test]
fn w090_w092_3obs_me() {
    let src = check_mod("w090/w092_3obs_me.mod");
    let rust = rust_family(&src);
    assert!(
        rust.is_empty(),
        "3 obs + measurement error should be zero, got {rust:?}"
    );
}

#[test]
fn w090_w093_param() {
    let src = check_mod("w090/w093_param.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "E093");
    assert_eq!(rust[0].severity, 1);
    assert!(rust[0].message.contains("not a declared parameter"));
    assert_span(&src, &rust[0], "\nnot_a_param, 0.08, 0.01, 0.15;");
}

#[test]
fn w090_w093_stderr() {
    let src = check_mod("w090/w093_stderr.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "E093");
    assert_eq!(rust[0].severity, 1);
    assert!(rust[0]
        .message
        .contains("stderr 'not_a_shock' is not a declared shock or observed variable"));
    assert_span(&src, &rust[0], "\nstderr not_a_shock, 0.1, 0, 1;");
}

#[test]
fn w090_w093_corr() {
    let src = check_mod("w090/w093_corr.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 2);
    assert!(rust.iter().all(|d| d.code == "E093"));
    assert!(rust.iter().all(|d| d.severity == 1));
    assert!(rust.iter().any(|d| d.message.contains("'foo'")));
    assert!(rust.iter().any(|d| d.message.contains("'bar'")));
    for d in &rust {
        assert_span(&src, d, "\ncorr foo, bar, 0.1, -1, 1;");
    }
}

#[test]
fn w090_w094_swap() {
    let src = check_mod("w090/w094_swap.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 2);
    assert!(rust.iter().all(|d| d.code == "W094"));
    for d in &rust {
        assert_span(&src, d, "\nalphag, 0.08, 0.15, 0.01;");
    }
}

#[test]
fn w090_w094_init() {
    let src = check_mod("w090/w094_init.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "W094");
    assert!(rust[0].message.contains("initial value 0.2 "));
    assert_span(&src, &rust[0], "\nalphag, 0.20, 0.01, 0.15;");
}

#[test]
fn w090_w095_ot() {
    let src = check_mod("w090/w095_ot.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "E095");
    assert_eq!(rust[0].severity, 1);
    let idx = src.find("y(1)").expect("y(1)");
    let index = LineIndex::new(&src);
    let start = index.position(&src, idx as u32);
    let end = index.position(&src, (idx + "y".len()) as u32);
    assert_eq!(
        (
            rust[0].start_line,
            rust[0].start_char,
            rust[0].end_line,
            rust[0].end_char
        ),
        (start.line, start.character, end.line, end.character),
        "W095 should underline the y name"
    );
}
