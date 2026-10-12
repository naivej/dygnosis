use std::path::PathBuf;

use dygnosis::span::LineIndex;
use dygnosis::{check_e020, parse};

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
        .join(".agents/skills/use-dynare/references/model-archive")
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

fn rust_e020(text: &str) -> Vec<Diag> {
    let model = parse(text);
    let index = LineIndex::new(&model.source);
    check_e020(&model)
        .into_iter()
        .filter(|d| matches!(d.code.as_str(), "E020" | "E023" | "E024" | "E025"))
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

fn last_ident_in(text: &str, context: &str, ident: &str) -> (u32, u32, u32, u32) {
    let at = text
        .find(context)
        .unwrap_or_else(|| panic!("missing context {context:?}"));
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

fn assert_last_ident(text: &str, d: &Diag, context: &str, ident: &str) {
    assert_eq!(
        (d.start_line, d.start_char, d.end_line, d.end_char),
        last_ident_in(text, context, ident),
        "span should be last {ident:?} in {context:?}, message={}",
        d.message
    );
}

#[test]
fn e020_unmodified_archives() {
    for name in ARCHIVES {
        let got = rust_e020(&read_mod(name));
        assert!(
            got.is_empty(),
            "{name}: expected no E020 family, got {got:?}"
        );
    }
}

#[test]
fn shocks_vars_lists_eps_z_and_eps_ig() {
    let model = parse(&read_mod("trend_rbc_gov_inv"));
    let names: Vec<&str> = model.shocks_vars.iter().map(|n| model.name(*n)).collect();
    assert_eq!(names, ["eps_z", "eps_ig"]);
}

#[test]
fn e020_alpph_typo_replace() {
    let text = check_mod("e020/e020_typo.mod");
    let got = rust_e020(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "E020");
    assert_eq!(got[0].severity, 1);
    assert!(
        got[0].message.contains("Undeclared identifier 'alpph'"),
        "alpph message, got {}",
        got[0].message
    );
    assert_span_in(&text, &got[0], "y = rho * y(-1) + alpph + e;", "alpph");
}

#[test]
fn e023_predetermined_eps_z() {
    let text = check_mod("e020/e023_predetermined.mod");
    let got = rust_e020(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "E023");
    assert_eq!(got[0].severity, 1);
    assert!(
        got[0]
            .message
            .contains("Predetermined variable 'e' is not declared as an endogenous variable."),
        "e023 message, got {}",
        got[0].message
    );
    assert_last_ident(&text, &got[0], "predetermined_variables e;", "e");
}

#[test]
fn e023_requires_an_ordinary_endogenous_at_the_command() {
    // Guard against accepting heterogeneous names, later declarations, and
    // later type changes, or rejecting a name already changed to ordinary var.
    for (source, needle) in [
        ("heterogeneity_dimension hh; var(heterogeneity=hh) a; predetermined_variables a; model(heterogeneity=hh); a=a(-1); end;", Some("a is not endogenous.")),
        ("predetermined_variables a; var a; model; a=0; end;", Some("Unknown symbol: a")),
        ("parameters a; predetermined_variables a; change_type(var) a; model; a=0; end;", Some("a is not endogenous.")),
        ("var a; predetermined_variables a; change_type(parameters) a; predetermined_variables a;", Some("a is not endogenous.")),
        ("var a; predetermined_variables a; model; a=0; end;", None),
        ("parameters a; change_type(var) a; predetermined_variables a; model; a=0; end;", None),
        ("heterogeneity_dimension hh; var(heterogeneity=hh) a; change_type(var) a; predetermined_variables a; model; a=0; end;", None),
        ("trend_var(growth_factor=1) a; change_type(var) a; predetermined_variables a; model; a=0; end;", None),
    ] {
        let rows: Vec<_> = rust_e020(source).into_iter().filter(|d| d.code == "E023").collect();
        if needle.is_some() {
            assert_eq!(rows.len(), 1, "{source}: {rows:?}");
            let target = source.rfind("predetermined_variables a;").unwrap()
                + "predetermined_variables ".len();
            assert_eq!((rows[0].start_line, rows[0].start_char, rows[0].end_line, rows[0].end_char), (0, target as u32, 0, target as u32 + 1), "{source}");
        } else {
            assert!(rows.is_empty(), "{source}: {rows:?}");
        }
        let binary = std::path::Path::new("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
        if binary.is_file() {
            let result = dygnosis::run_preprocessor(source, binary, None, std::time::Duration::from_secs(30), dygnosis::JsonStage::Check);
            let report = format!("{} {}", result.raw_stdout, result.raw_stderr);
            assert_eq!(result.success, needle.is_none(), "{source}\n{report}");
            if let Some(needle) = needle {
                assert!(report.contains(needle), "{source}\n{report}");
            }
        }
    }
}

#[test]
fn e023_checks_a_repeated_macro_command_after_its_type_changes() {
    // Guard against letting an earlier valid execution hide the refusal at
    // the same written command in a later macro iteration.
    let source = "var a;\n@#for j in 1:2\n@#if j == 2\nchange_type(parameters) a;\n@#endif\npredetermined_variables a;\n@#endfor\n";
    let mut workspace = dygnosis::Workspace::new();
    workspace.update_document("model.mod", source.to_string());
    let model = workspace.get_effective_model("model.mod").unwrap();
    let rows: Vec<_> = check_e020(model)
        .into_iter()
        .filter(|d| d.code == "E023")
        .collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    let binary = std::path::Path::new("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    if binary.is_file() {
        let result = dygnosis::run_preprocessor(
            source,
            binary,
            None,
            std::time::Duration::from_secs(30),
            dygnosis::JsonStage::Check,
        );
        let report = format!("{} {}", result.raw_stdout, result.raw_stderr);
        assert!(!result.success, "{report}");
        assert!(report.contains("a is not endogenous."), "{report}");
    }
}

#[test]
fn e024_timed_det_exo_tau() {
    let text = check_mod("e020/e024_timed_det.mod");
    let got = rust_e020(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "E024");
    assert_eq!(got[0].severity, 1);
    assert!(
        got[0]
            .message
            .contains("Exogenous deterministic variable tau cannot be given a lead or a lag"),
        "e024 message, got {}",
        got[0].message
    );
    assert_span(&text, &got[0], "tau(-1)");
}

#[test]
fn e025_use_before_def() {
    let text = check_mod("e020/e025_use_before.mod");
    let got = rust_e020(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "E025");
    assert_eq!(got[0].severity, 1);
    assert!(
        got[0]
            .message
            .contains("foo has wrong type or was already used on the right-hand side"),
        "e025 use-before message, got {}",
        got[0].message
    );
    assert_span_in(&text, &got[0], "c = c + foo;", "foo");
}

#[test]
fn e025_shadow_declared_y() {
    let text = check_mod("e020/e025_shadow.mod");
    let got = rust_e020(&text);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].code, "E025");
    assert_eq!(got[0].severity, 1);
    assert!(
        got[0]
            .message
            .contains("y has wrong type or was already used on the right-hand side"),
        "e025 shadow message, got {}",
        got[0].message
    );
    assert_span_in(&text, &got[0], "# y = 1;", "y");
}

#[test]
fn mystery_call_is_not_e020() {
    let diags = rust_e020(&check_mod("e020/mystery_call.mod"));
    assert!(
        diags
            .iter()
            .all(|d| !(d.code == "E020" && d.message.contains("'mystery'"))),
        "Call callee mystery must not be E020: {diags:?}"
    );
    assert!(
        diags.is_empty(),
        "mystery_call: expected empty family, got {diags:?}"
    );
}

#[test]
fn comment_and_string_are_not_e020() {
    let text = check_mod("e020/comment_string.mod");
    let diags = rust_e020(&text);
    assert!(
        diags.iter().all(|d| !d.message.contains("'sneaky_ident'")),
        "sneaky_ident leaked into E020 family: {diags:?}"
    );
    assert!(
        diags.is_empty(),
        "comment_string: expected empty family, got {diags:?}"
    );
}
