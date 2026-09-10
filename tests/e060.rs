use std::path::{Path, PathBuf};

use dygnosis::span::LineIndex;
use dygnosis::{check_e060_family, check_e060_family_on_model, parse, Workspace};

const FAMILY: &[&str] = &["E060", "E061", "E062", "E063", "E064", "E065", "W061"];

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

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn fixture_path(rel: &str) -> PathBuf {
    fixtures_root().join(rel)
}

fn check_mod(rel: &str) -> String {
    let path = fixture_path(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn copilot_mod(archive_dir: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/model-archive")
        .join(archive_dir)
        .join(format!("{archive_dir}.mod"))
}

fn copilot_example(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/examples")
        .join(format!("{name}.mod"))
}

fn swff_path() -> PathBuf {
    copilot_mod("swff")
}

fn read_mod_file(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn to_rows(source: &str, diags: Vec<dygnosis::diagnostic::Diagnostic>) -> Vec<Diag> {
    let index = LineIndex::new(source);
    diags
        .into_iter()
        .filter(|d| FAMILY.contains(&d.code.as_str()))
        .map(|d| {
            let start = index.position(source, d.span.start);
            let end = index.position(source, d.span.end);
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

fn rust_family(text: &str) -> Vec<Diag> {
    let model = parse(text);
    to_rows(&model.source, check_e060_family_on_model(&model))
}

fn rust_workspace(active: &str, files: &[(&str, &str)]) -> Vec<Diag> {
    let mut ws = Workspace::new();
    for (uri, src) in files {
        ws.update_document(uri, *src);
    }
    let diags = check_e060_family(&mut ws, active);
    let source = ws.get_source(active).unwrap_or("").to_string();
    to_rows(&source, diags)
}

fn load_fixture_dir(dir_rel: &str) -> Vec<(String, String)> {
    let dir = fixture_path(dir_rel);
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| {
        panic!("fixture directory missing at {}: {e}", dir.display())
    }) {
        let entry = entry.unwrap();
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let uri = path
            .to_str()
            .unwrap_or_else(|| panic!("fixture path is not UTF-8: {}", path.display()))
            .to_string();
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
            .replace("\r\n", "\n");
        files.push((uri, src));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

fn rust_fixture_dir(dir_rel: &str, active_name: &str) -> Vec<Diag> {
    let active = fixture_path(dir_rel)
        .join(active_name)
        .to_str()
        .unwrap()
        .to_string();
    let owned = load_fixture_dir(dir_rel);
    let files: Vec<(&str, &str)> = owned
        .iter()
        .map(|(uri, src)| (uri.as_str(), src.as_str()))
        .collect();
    rust_workspace(&active, &files)
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

fn assert_quiet(rel: &str) {
    let got = rust_family(&check_mod(rel));
    assert!(
        got.is_empty(),
        "{rel}: expected no E060 family, got {got:?}"
    );
}

fn assert_fire(rel: &str, code: &str, msg_sub: &str, needle: &str) {
    let text = check_mod(rel);
    let got = rust_family(&text);
    assert_eq!(got.len(), 1, "{rel}: {got:?}");
    assert_eq!(got[0].code, code);
    assert!(
        got[0].message.contains(msg_sub),
        "{rel}: missing {msg_sub:?} in {}",
        got[0].message
    );
    assert_span(&text, &got[0], needle);
}

fn assert_workspace_fire(
    dir_rel: &str,
    active_name: &str,
    code: &str,
    msg_sub: &str,
    needle: &str,
) {
    let text = check_mod(&format!("{dir_rel}/{active_name}"));
    let got = rust_fixture_dir(dir_rel, active_name);
    assert_eq!(got.len(), 1, "{dir_rel}: {got:?}");
    assert_eq!(got[0].code, code);
    assert!(
        got[0].message.contains(msg_sub),
        "{dir_rel}: missing {msg_sub:?} in {}",
        got[0].message
    );
    assert_span(&text, &got[0], needle);
}

#[test]
fn e060_clean_fixtures_empty() {
    for (label, path) in [
        ("swff", swff_path()),
        ("zlb_qe", copilot_mod("zlb_qe")),
        ("US_RE09_rep", copilot_example("US_RE09_rep")),
    ] {
        let text = read_mod_file(&path);
        let rust = rust_family(&text);
        assert!(
            rust.is_empty(),
            "clean {label} should have no E060 family, got {rust:?}"
        );
        if label == "swff" {
            let model = parse(&text);
            assert_eq!(model.includes.len(), 1);
            assert!(
                model
                    .macro_directives
                    .iter()
                    .all(|d| d.kind != "if" && d.kind != "error"),
                "clean swff should have no if/error directives, got {:?}",
                model
                    .macro_directives
                    .iter()
                    .map(|d| &d.kind)
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn e060_cycle() {
    assert_workspace_fire(
        "e060/cycle",
        "a.mod",
        "E060",
        "a.mod -> b.inc -> a.mod",
        "@#include \"b.inc\"",
    );
}

#[test]
fn e061_missing_sibling() {
    assert_workspace_fire(
        "e060/missing",
        "main.mod",
        "E061",
        "no_such_file.inc",
        "@#include \"no_such_file.inc\"",
    );
}

#[test]
fn e061_nested_missing() {
    assert_workspace_fire(
        "e060/nested",
        "nested_main.mod",
        "E061",
        "missing_nested.inc (included from nested_helper.inc)",
        "@#include \"nested_helper.inc\"",
    );
}

#[test]
fn e062_unterminated_if() {
    assert_fire(
        "e060/e062_if.mod",
        "E062",
        "Unterminated @#if",
        "@#if FOO",
    );
}

#[test]
fn e062_unterminated_for() {
    assert_fire(
        "e060/e062_for.mod",
        "E062",
        "Unterminated @#for",
        "@#for i in 1:2",
    );
}

#[test]
fn e062_stray_endif() {
    assert_fire(
        "e060/e062_stray_endif.mod",
        "E062",
        "Stray @#endif",
        "@#endif",
    );
}

#[test]
fn e062_mismatch_if_endfor() {
    assert_fire(
        "e060/e062_mismatch.mod",
        "E062",
        "Mismatched @#endfor",
        "@#endfor",
    );
}

#[test]
fn e062_duplicate_else() {
    let text = check_mod("e060/e062_dup_else.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].code, "E062");
    assert!(
        got[0].message.contains("Duplicate @#else"),
        "duplicate else, got {}",
        got[0].message
    );
    assert_span_in(&text, &got[0], "@#else\n@#endif", "@#else");
}

#[test]
fn e062_elseif_after_else() {
    assert_fire(
        "e060/e062_elseif.mod",
        "E062",
        "@#elseif after @#else",
        "@#elseif BAR",
    );
}

#[test]
fn e062_commented_endif_is_not_e062() {
    assert_quiet("e060/e062_comment.mod");
}

#[test]
fn e063_undef_simple() {
    assert_fire("e060/e063_undef.mod", "E063", "@{UNDEF}", "@{UNDEF}");
}

#[test]
fn e063_expression_skipped_when_include_present() {
    assert_quiet("e060/e063_expr_include.mod");
}

#[test]
fn e063_expression_without_include() {
    assert_fire(
        "e060/e063_expr.mod",
        "E063",
        "@{UNDEF+1}",
        "@{UNDEF+1}",
    );
}

#[test]
fn e063_defined_before_use() {
    assert_quiet("e060/e063_defined.mod");
}

#[test]
fn e063_for_var_not_known_after_endfor() {
    assert_fire("e060/e063_for.mod", "E063", "@{i}", "@{i}");
}

#[test]
fn e064_error_quoted() {
    let text = check_mod("e060/e064_quoted.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].code, "E064");
    assert!(
        got[0].message.contains(": boom."),
        "expected boom message, got {got:?}"
    );
    assert_span(&text, &got[0], "@#error \"boom\"");
}

#[test]
fn e064_error_no_argument() {
    let text = check_mod("e060/e064_noarg.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].code, "E064");
    assert_eq!(got[0].message, "Macro @#error triggered.");
    assert_span(&text, &got[0], "@#error");
}

#[test]
fn e064_spaced_error_directive() {
    assert_fire(
        "e060/e064_spaced.mod",
        "E064",
        ": spaced.",
        "@# error \"spaced\"",
    );
}

#[test]
fn e064_inactive_in_if_zero() {
    assert_quiet("e060/e064_inactive.mod");
}

#[test]
fn e065_steady_state_varexo() {
    assert_fire(
        "e060/e065_varexo.mod",
        "E065",
        "e",
        "y = rho * y(-1) + steady_state(e)",
    );
}

#[test]
fn e065_steady_state_two_varexo() {
    let text = check_mod("e060/e065_two.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].code, "E065");
    assert!(
        got[0].message.contains("e, u"),
        "expected sorted varexo names, got {got:?}"
    );
    assert_span(
        &text,
        &got[0],
        "y = rho * y(-1) + steady_state(e + u)",
    );
}

#[test]
fn e065_steady_state_endogenous_ok() {
    assert_quiet("e060/e065_endo.mod");
}

#[test]
fn e065_steady_state_uppercase() {
    let text = check_mod("e060/e065_upper.mod");
    let got = rust_family(&text);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].code, "E065");
    assert_span(
        &text,
        &got[0],
        "y = rho * y(-1) + STEADY_STATE(e)",
    );
}

#[test]
fn w061_two_sibling_parents() {
    let rust = rust_fixture_dir("e060/w061_siblings", "shared.inc");
    assert_eq!(rust.len(), 1);
    assert_eq!(rust[0].code, "W061");
    assert_eq!(rust[0].start_line, 0);
    assert_eq!(rust[0].start_char, 0);
    assert_eq!(rust[0].end_line, 0);
    assert_eq!(rust[0].end_char, 1);
    assert!(
        rust[0].message.contains("parent_a.mod, parent_b.mod"),
        "LSP basenames missing, got {}",
        rust[0].message
    );
    assert!(
        rust[0]
            .message
            .contains("open or run the intended parent model"),
        "LSP wording missing, got {}",
        rust[0].message
    );
    assert!(
        !rust[0]
            .message
            .contains("rerun with only the intended parent"),
        "must not use MCP wording: {}",
        rust[0].message
    );
}

#[test]
fn w061_chain_not_ambiguous() {
    let rust = rust_fixture_dir("e060/w061_chain", "chain_shared.inc");
    assert!(rust.is_empty(), "A→B→shared should not W061, got {rust:?}");
}
