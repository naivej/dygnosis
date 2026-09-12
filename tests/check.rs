use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use dygnosis::span::Span;
use dygnosis::{
    check_file, find_preprocessor, format_check_lines, reconcile_diagnostics, run_preprocessor,
    Diagnostic, Severity,
};
use std::time::Duration;

const ARCHIVES: &[&str] = &[
    "trend_rbc_gov_inv",
    "sims_wu_2019",
    "govt_rbc_irf_matching",
    "lk2024",
];

const OUT: &[&str] = &[
    "E040", "W040", "W041", "I041", "W071", "I070", "I071", "W080", "W081", "DYNR",
];

const I050_MESSAGE: &str = "No initval or steady_state_model block. Add an initval block with initial guesses, or a steady_state_model block with closed-form assignments.";

const SEVERITIES: &[&str] = &["ERROR", "WARNING", "INFO", "HINT", "UNKNOWN"];

fn copilot_mod(archive_dir: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/model-archive")
        .join(archive_dir)
        .join(format!("{archive_dir}.mod"))
}

fn read_mod_file(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn read_mod(archive_dir: &str) -> String {
    read_mod_file(&copilot_mod(archive_dir))
}

fn dygnosis() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_dygnosis"));
    cmd.env("RUST_LOG", "off");
    cmd
}

fn run_check(path: &str) -> Output {
    dygnosis()
        .args(["check", path])
        .output()
        .expect("dygnosis check")
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8(output.stdout.clone())
        .unwrap()
        .replace("\r\n", "\n")
}

fn stderr_text(output: &Output) -> String {
    String::from_utf8(output.stderr.clone())
        .unwrap()
        .replace("\r\n", "\n")
}

fn exit_code(output: &Output) -> i32 {
    output.status.code().expect("exit code")
}

fn is_p_digits(code: &str) -> bool {
    let rest = match code.strip_prefix('P') {
        Some(r) => r,
        None => return false,
    };
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

fn split_path_line_col(prefix: &str) -> Option<(&str, u32, u32)> {
    let col_at = prefix.rfind(':')?;
    let col: u32 = prefix[col_at + 1..].parse().ok()?;
    let rest = &prefix[..col_at];
    let line_at = rest.rfind(':')?;
    let line: u32 = rest[line_at + 1..].parse().ok()?;
    Some((&rest[..line_at], line, col))
}

/// `{filepath}:{line}:{col}: {SEVERITY} [{code}] {message}`
fn parse_diag_line(line: &str) -> Option<(String, u32, u32, String, String, String)> {
    let (sev, idx, needle_len) = SEVERITIES.iter().find_map(|sev| {
        let needle = format!(": {sev} [");
        line.find(&needle).map(|i| (*sev, i, needle.len()))
    })?;
    let (path, line_n, col) = split_path_line_col(&line[..idx])?;
    let after = &line[idx + needle_len..];
    let close = after.find(']')?;
    let code = after[..close].to_string();
    let message = after.get(close + 2..)?.to_string();
    Some((
        path.to_string(),
        line_n,
        col,
        sev.to_string(),
        code,
        message,
    ))
}

fn parse_summary(line: &str) -> Option<(usize, usize, usize)> {
    // "{n} issue(s): {e} error(s), {w} warning(s)"
    let (left, w_part) = line.rsplit_once(", ")?;
    let w: usize = w_part.strip_suffix(" warning(s)")?.parse().ok()?;
    let (n_part, e_part) = left.split_once(": ")?;
    let n: usize = n_part.strip_suffix(" issue(s)")?.parse().ok()?;
    let e: usize = e_part.strip_suffix(" error(s)")?.parse().ok()?;
    Some((n, e, w))
}

struct ParsedCheck {
    empty: bool,
    diags: Vec<(String, u32, u32, String, String, String)>,
    summary: Option<(usize, usize, usize)>,
}

fn parse_check_stdout(path: &str, stdout: &str) -> ParsedCheck {
    let text = stdout.strip_suffix('\n').unwrap_or(stdout);
    if text == format!("No issues found in {path}") {
        return ParsedCheck {
            empty: true,
            diags: Vec::new(),
            summary: None,
        };
    }
    let lines: Vec<&str> = text.split('\n').collect();
    assert!(
        lines.len() >= 3,
        "expected diag lines, blank, summary; got {stdout:?}"
    );
    assert_eq!(lines[lines.len() - 2], "", "blank line before summary");
    let summary = parse_summary(lines[lines.len() - 1])
        .unwrap_or_else(|| panic!("bad summary: {:?}", lines[lines.len() - 1]));
    let mut diags = Vec::new();
    for line in &lines[..lines.len() - 2] {
        let parsed =
            parse_diag_line(line).unwrap_or_else(|| panic!("bad diagnostic line: {line:?}"));
        assert_eq!(parsed.0, path, "filepath prefix must be the argument");
        diags.push(parsed);
    }
    ParsedCheck {
        empty: false,
        diags,
        summary: Some(summary),
    }
}

fn assert_line_format(path: &str, stdout: &str) {
    let parsed = parse_check_stdout(path, stdout);
    if parsed.empty {
        assert_eq!(stdout, format!("No issues found in {path}\n"));
        return;
    }
    let (n, e, w) = parsed.summary.unwrap();
    assert_eq!(n, parsed.diags.len(), "n must equal diagnostic count");
    let errors = parsed.diags.iter().filter(|d| d.3 == "ERROR").count();
    let warnings = parsed.diags.iter().filter(|d| d.3 == "WARNING").count();
    assert_eq!(e, errors);
    assert_eq!(w, warnings);
}

fn printed_codes(stdout: &str) -> HashSet<String> {
    stdout
        .lines()
        .filter_map(parse_diag_line)
        .map(|d| d.4)
        .collect()
}

fn assert_no_out(stdout: &str) {
    let codes = printed_codes(stdout);
    for code in OUT {
        assert!(
            !codes.contains(*code),
            "Out code {code} must not print; got {codes:?}"
        );
    }
}

fn assert_no_out_or_preproc(stdout: &str) {
    assert_no_out(stdout);
    let codes = printed_codes(stdout);
    for code in &codes {
        assert!(
            !is_p_digits(code),
            "preprocessor code {code} must not print"
        );
    }
}

fn assert_exit_for_errors(output: &Output, stdout: &str) {
    let has_error = stdout.contains(": ERROR [");
    let code = exit_code(output);
    if has_error {
        assert_eq!(code, 1, "ERROR must exit 1; stdout:\n{stdout}");
    } else {
        assert_eq!(code, 0, "no ERROR must exit 0; stdout:\n{stdout}");
    }
    assert_ne!(code, 2);
}

#[test]
fn format_check_lines_empty() {
    let out = format_check_lines("model.mod", &[], "var y;\n");
    assert_eq!(out, "No issues found in model.mod\n");
}

#[test]
fn format_check_lines_one_error() {
    let src = "var y;\n";
    let diags = vec![Diagnostic::new(
        Span::new(0, 3),
        Severity::Error,
        "E001",
        "missing semicolon",
    )];
    let out = format_check_lines("x.mod", &diags, src);
    assert_eq!(
        out,
        "x.mod:1:1: ERROR [E001] missing semicolon\n\n1 issue(s): 1 error(s), 0 warning(s)\n"
    );
}

#[test]
fn format_check_lines_mixed_info_counts_in_n_only() {
    let src = "var y;\nvar z;\n";
    let y = src.find('y').unwrap();
    let z = src.find('z').unwrap();
    let diags = vec![
        Diagnostic::new(Span::new(0, 3), Severity::Error, "E001", "err"),
        Diagnostic::new(Span::new(y, y + 1), Severity::Warning, "W010", "warn"),
        Diagnostic::new(Span::new(z, z + 1), Severity::Information, "I050", "info"),
    ];
    let out = format_check_lines("mix.mod", &diags, src);
    assert_eq!(
        out,
        "mix.mod:1:1: ERROR [E001] err\n\
         mix.mod:1:5: WARNING [W010] warn\n\
         mix.mod:2:5: INFO [I050] info\n\
         \n\
         3 issue(s): 1 error(s), 1 warning(s)\n"
    );
}

#[test]
fn p_core_check_file_no_preproc() {
    let path = copilot_mod("trend_rbc_gov_inv");
    let path_str = path.to_str().expect("utf-8 path");
    let text = read_mod("trend_rbc_gov_inv");
    let stdout = format_check_lines(path_str, &check_file(&text, path_str), &text);
    assert_eq!(stdout, format!("No issues found in {path_str}\n"));
    assert_no_out_or_preproc(&stdout);

    for name in ["sims_wu_2019", "lk2024"] {
        let path = copilot_mod(name);
        let path_str = path.to_str().expect("utf-8 path");
        let text = read_mod(name);
        let diags = check_file(&text, path_str);
        assert!(!diags.is_empty(), "{name} library check should emit I050");
        assert!(
            diags.iter().all(|d| d.code == "I050"),
            "{name} library check codes: {:?}",
            diags.iter().map(|d| &d.code).collect::<Vec<_>>()
        );
        let stdout = format_check_lines(path_str, &diags, &text);
        assert_no_out_or_preproc(&stdout);
    }

    let path = copilot_mod("govt_rbc_irf_matching");
    let path_str = path.to_str().expect("utf-8 path");
    let text = read_mod("govt_rbc_irf_matching");
    let diags = check_file(&text, path_str);
    assert!(
        diags.iter().any(|d| d.code == "E001"),
        "govt_rbc_irf_matching should emit E001, got {:?}",
        diags.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
    assert_no_out_or_preproc(&format_check_lines(path_str, &diags, &text));
}

#[test]
fn p_core_cli_line_format_and_exit() {
    for name in ARCHIVES {
        let path = copilot_mod(name);
        let path_str = path.to_str().expect("utf-8 path");
        let output = run_check(path_str);
        let stdout = stdout_text(&output);
        assert_line_format(path_str, &stdout);
        assert_no_out(&stdout);
        assert_exit_for_errors(&output, &stdout);
        if stdout.starts_with("No issues found in ") {
            assert_eq!(stdout, format!("No issues found in {path_str}\n"));
        }
    }
}

#[test]
fn i050_uses_recorded_rust_message() {
    for name in ["sims_wu_2019", "lk2024"] {
        let path = copilot_mod(name);
        let path_str = path.to_str().expect("utf-8 path");
        let output = run_check(path_str);
        let stdout = stdout_text(&output);
        assert!(
            stdout.contains(&format!("INFO [I050] {I050_MESSAGE}")),
            "{name} missing Rust I050 message:\n{stdout}"
        );
        assert!(
            !stdout.contains("Compute Steady State"),
            "{name} must not print Compute Steady State"
        );
        assert_eq!(exit_code(&output), 0);
    }
}

#[test]
fn govt_rbc_cascade_is_e001_only() {
    let path = copilot_mod("govt_rbc_irf_matching");
    let path_str = path.to_str().expect("utf-8 path");
    let text = read_mod("govt_rbc_irf_matching");
    let own = check_file(&text, path_str);
    let own_codes: Vec<&str> = own.iter().map(|d| d.code.as_str()).collect();
    assert!(
        own_codes.contains(&"E001"),
        "analyze/check_file cascade expected E001, got {own_codes:?}"
    );
    for extra in ["E062", "E063", "E064", "E065"] {
        assert!(
            !own_codes.contains(&extra),
            "cascade must not emit {extra}; got {own_codes:?}"
        );
    }
    let output = run_check(path_str);
    let stdout = stdout_text(&output);
    let codes = printed_codes(&stdout);
    assert_no_out(&stdout);
    if find_preprocessor(None).is_some() {
        assert!(
            !codes.contains("E001"),
            "CLI with preprocessor must drop E001 when Dynare accepts govt_rbc_irf_matching; got {codes:?}\n{stdout}"
        );
    } else {
        assert!(codes.contains("E001"), "expected E001, got {codes:?}");
        assert_eq!(exit_code(&output), 1);
    }
    for extra in ["E062", "E063", "E064", "E065"] {
        assert!(
            !codes.contains(extra),
            "cascade must not print {extra}; got {codes:?}"
        );
    }
    assert_exit_for_errors(&output, &stdout);
}

#[test]
fn cli_trend_rbc_gov_inv_does_not_print_dropped_e001() {
    let path = copilot_mod("trend_rbc_gov_inv");
    let path_str = path.to_str().expect("utf-8 path");
    let output = run_check(path_str);
    let stdout = stdout_text(&output);
    let codes = printed_codes(&stdout);
    assert!(
        !codes.contains("E001"),
        "accepted file must not print dropped E001; got {codes:?}\n{stdout}"
    );
    assert!(
        !stdout.contains(": ERROR ["),
        "accepted file must not print own Error; got {codes:?}\n{stdout}"
    );
    assert_no_out(&stdout);
}

#[test]
fn swff_workspace_check_no_out() {
    let path = copilot_mod("swff");
    let path_str = path.to_str().expect("utf-8 path");
    let output = run_check(path_str);
    let stdout = stdout_text(&output);
    let code = exit_code(&output);
    assert!(
        code == 0 || code == 1,
        "swff exit must be 0 or 1, got {code}"
    );
    assert_ne!(code, 2);
    assert_line_format(path_str, &stdout);
    assert_no_out(&stdout);
}

#[test]
fn error_check_fixture_exits_1() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/e001/delete_model_end.mod");
    let path_str = path.to_str().expect("utf-8 path");
    let text = read_mod_file(&path);
    let diags = check_file(&text, path_str);
    assert!(
        diags.iter().any(|d| d.code == "E001"),
        "check_file on delete_model_end should emit E001, got {:?}",
        diags.iter().map(|d| &d.code).collect::<Vec<_>>()
    );
    let output = run_check(path_str);
    let stdout = stdout_text(&output);
    assert!(
        stdout.contains(": ERROR ["),
        "expected an ERROR line (library E001; CLI may print P001 when Dynare is installed):\n{stdout}"
    );
    assert_eq!(exit_code(&output), 1);
    assert_no_out(&stdout);
}

#[test]
fn missing_file_exits_1() {
    let path = "no_such_file_dygnosis_check.mod";
    let output = run_check(path);
    let stderr = stderr_text(&output);
    assert!(
        stderr.contains("Error: File not found:"),
        "stderr: {stderr:?}"
    );
    assert!(stderr.contains(path), "stderr should echo the argument");
    assert_eq!(exit_code(&output), 1);
}

#[test]
fn cli_stdout_matches_format_check_lines() {
    for name in ["trend_rbc_gov_inv", "sims_wu_2019"] {
        let text = read_mod(name);
        let tmp = std::env::temp_dir().join(format!("dygnosis-check-wire-{name}.mod"));
        std::fs::write(&tmp, text.as_bytes()).expect("write temp");
        let path_str = tmp.to_str().expect("utf-8 path");
        let diags = check_file(&text, path_str);
        let parent = tmp.parent();
        let pre = find_preprocessor(None)
            .map(|pp| run_preprocessor(&text, &pp, parent, Duration::from_secs(30)));
        let reconciled = reconcile_diagnostics(&diags, pre.as_ref());
        let expected = format_check_lines(path_str, &reconciled, &text);
        let output = run_check(path_str);
        let stdout = stdout_text(&output);
        assert_eq!(stdout, expected, "CLI wire mismatch for {name}");
        assert_exit_for_errors(&output, &stdout);
        let _ = std::fs::remove_file(&tmp);
    }
}
