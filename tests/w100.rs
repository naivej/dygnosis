use std::path::PathBuf;

use dygnosis::span::LineIndex;
use dygnosis::{check_w100, parse, PolicyCommand};

const CLEAN: &[&str] = &[
    "trend_rbc_gov_inv",
    "sims_wu_2019",
    "govt_rbc_irf_matching",
    "lk2024",
];

const POLICY_KEYS: &[&str] = &[
    "discretionary_policy",
    "ramsey_policy",
    "ramsey_model",
    "osr",
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

fn rust_family(text: &str) -> Vec<Diag> {
    let model = parse(text);
    let index = LineIndex::new(&model.source);
    check_w100(&model)
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

fn policy_ident_range(text: &str) -> (u32, u32, u32, u32) {
    let mut best: Option<(usize, usize)> = None;
    for key in POLICY_KEYS {
        for at in ident_offsets(text, key) {
            if best.map(|(start, _)| at < start).unwrap_or(true) {
                best = Some((at, key.len()));
            }
        }
    }
    let (start, len) = best.expect("policy command identifier");
    let index = LineIndex::new(text);
    let s = index.position(text, start as u32);
    let e = index.position(text, (start + len) as u32);
    (s.line, s.character, e.line, e.character)
}

fn assert_policy_span(text: &str, d: &Diag) {
    assert_eq!(
        (d.start_line, d.start_char, d.end_line, d.end_char),
        policy_ident_range(text),
        "span should be the first policy-command identifier, message={}",
        d.message
    );
}

#[test]
fn w100_clean_four_empty() {
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
fn w100_parser_ramsey_fields() {
    let text = check_mod("w100/w102_1p5.mod");
    let model = parse(&text);
    assert_eq!(model.policy_commands, [PolicyCommand::RamseyModel]);
    let instruments: Vec<_> = model.instruments.iter().map(|n| model.name(*n)).collect();
    assert_eq!(instruments, ["y"]);
    assert_eq!(model.planner_discount, Some(1.5));
}

#[test]
fn w100_parser_optim_weights() {
    let text = check_mod("w100/w103_params.mod");
    let model = parse(&text);
    assert!(model.has_optim_weights);
    assert_eq!(model.policy_commands, [PolicyCommand::Osr]);
}

#[test]
fn w100_ramsey() {
    let src = check_mod("w100/w100_ramsey.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "W100");
    assert!(rust[0].message.starts_with("ramsey_model requires"));
    assert_policy_span(&src, &rust[0]);
}

#[test]
fn w100_disc() {
    let src = check_mod("w100/w100_disc.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "W100");
    assert!(rust[0].message.starts_with("discretionary_policy requires"));
    assert_policy_span(&src, &rust[0]);
}

#[test]
fn w100_planner_objective_alone() {
    let src = check_mod("w100/w100_planner.mod");
    let rust = rust_family(&src);
    assert!(
        rust.is_empty(),
        "planner_objective without a policy command should be empty, got {rust:?}"
    );
}

#[test]
fn w100_disc_ok() {
    let src = check_mod("w100/w100_disc_ok.mod");
    let rust = rust_family(&src);
    assert!(
        rust.is_empty(),
        "discretionary_policy with planner_objective should be empty, got {rust:?}"
    );
}

#[test]
fn w100_w101() {
    let src = check_mod("w100/w101_inst.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "W101");
    assert!(rust[0].message.contains("'not_endo'"));
    assert_policy_span(&src, &rust[0]);
}

#[test]
fn w100_w102_1p5() {
    let src = check_mod("w100/w102_1p5.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "W102");
    assert!(rust[0].message.contains("= 1.5 "));
    assert_policy_span(&src, &rust[0]);
}

#[test]
fn w100_w102_0() {
    let src = check_mod("w100/w102_0.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "W102");
    assert!(rust[0].message.contains("= 0 "));
    assert_policy_span(&src, &rust[0]);
}

#[test]
fn w100_w102_m0p1() {
    let src = check_mod("w100/w102_m0p1.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "W102");
    assert!(rust[0].message.contains("= -0.1 "));
    assert_policy_span(&src, &rust[0]);
}

#[test]
fn w100_w102_1_ok() {
    let src = check_mod("w100/w102_1.mod");
    let rust = rust_family(&src);
    assert!(
        rust.is_empty(),
        "planner_discount=1 should be empty, got {rust:?}"
    );
}

#[test]
fn w100_ramsey_ok() {
    let src = check_mod("w100/w100_ok.mod");
    let rust = rust_family(&src);
    assert!(
        rust.is_empty(),
        "ramsey_model with objective, y, 0.99 should be empty, got {rust:?}"
    );
}

#[test]
fn w100_w103_both() {
    let src = check_mod("w100/w103_both.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 2);
    assert!(rust.iter().all(|d| d.code == "W103"));
    assert!(rust
        .iter()
        .any(|d| d.message.contains("osr_params statement")));
    assert!(rust
        .iter()
        .any(|d| d.message.contains("optim_weights block")));
    for d in &rust {
        assert_policy_span(&src, d);
    }
}

#[test]
fn w100_w103_weights() {
    let src = check_mod("w100/w103_weights.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "W103");
    assert!(rust[0].message.contains("optim_weights block"));
    assert_policy_span(&src, &rust[0]);
}

#[test]
fn w100_w103_params() {
    let src = check_mod("w100/w103_params.mod");
    let rust = rust_family(&src);
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "W103");
    assert!(rust[0].message.contains("osr_params statement"));
    assert_policy_span(&src, &rust[0]);
}

#[test]
fn w100_osr_ok() {
    let src = check_mod("w100/w103_ok.mod");
    let rust = rust_family(&src);
    assert!(
        rust.is_empty(),
        "osr with params and weights should be empty, got {rust:?}"
    );
}
