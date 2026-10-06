use std::collections::HashSet;
use std::path::{Path, PathBuf};

use dygnosis::span::Span;
use dygnosis::{
    check_file, check_walk, dynare_workspace_diagnose, format_check_lines, Diagnostic, Severity,
};

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

fn check_dir_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("check_dir")
}

fn check_dir_path(rel: &str) -> PathBuf {
    let mut path = check_dir_root();
    for part in rel.split('/') {
        path.push(part);
    }
    path
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
        assert!(
            diags
                .iter()
                .any(|d| d.code == "I050" && d.message == I050_MESSAGE),
            "{name} library check should emit I050, got {:?}",
            diags
                .iter()
                .map(|d| (&d.code, &d.message))
                .collect::<Vec<_>>()
        );
        assert!(
            diags
                .iter()
                .all(|d| matches!(d.code.as_str(), "I050" | "I208" | "I209")),
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
    let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
    assert!(
        !codes.contains(&"E001"),
        "govt_rbc_irf_matching must not emit option-list E001, got {codes:?}"
    );
    for extra in ["E062", "E063", "E064", "E065"] {
        assert!(
            !codes.contains(&extra),
            "cascade must not emit {extra}; got {codes:?}"
        );
    }
    assert_no_out_or_preproc(&format_check_lines(path_str, &diags, &text));
}

#[test]
fn swff_check_has_no_out_codes() {
    let path = copilot_mod("swff");
    let path_str = path.to_str().expect("utf-8 path");
    let text = read_mod("swff");
    let stdout = format_check_lines(path_str, &check_file(&text, path_str), &text);
    assert_no_out(&stdout);
}

#[test]
fn delete_model_end_emits_e001() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/e001/delete_model_end.mod");
    let path_str = path.to_str().expect("utf-8 path");
    let text = read_mod_file(&path);
    let diags = check_file(&text, path_str);
    assert!(
        diags
            .iter()
            .any(|d| d.code == "E001" && d.severity == Severity::Error),
        "{diags:?}"
    );
}

#[test]
fn saved_model_walk_skips_plus_directories_and_non_mod_files() {
    let files = check_walk::collect_mod_files(&check_dir_root()).expect("walk");
    let joined = files.join("\n").replace('\\', "/");
    assert!(!joined.contains("bad.mod"), "{joined}");
    assert!(!joined.contains("fragment.inc"), "{joined}");
    assert!(joined.contains("nested/ok.mod"), "{joined}");
    for name in ["err.mod", "clean.mod", "warn.mod", "info.mod"] {
        assert!(joined.contains(name), "{name} missing:\n{joined}");
    }
    assert_eq!(files.len(), 5, "{joined}");
}

#[test]
fn explicit_include_path_is_diagnosed() {
    let arg = check_dir_path("fragment.inc")
        .to_string_lossy()
        .into_owned();
    let body = dynare_workspace_diagnose(None, None, Some(&[arg])).expect("path");
    let diags = body["roots"][0]["diagnostics"]
        .as_array()
        .expect("diagnostics");
    assert!(
        diags.iter().any(|diag| diag["severity"] == "ERROR"),
        "{body}"
    );
}

#[test]
fn path_batch_keeps_warnings_and_information_separate() {
    let warn = check_dir_path("warn.mod").to_string_lossy().into_owned();
    let warned = dynare_workspace_diagnose(None, None, Some(&[warn])).expect("warn");
    assert_eq!(warned["summary"]["errors"], 0, "{warned}");
    assert!(
        warned["summary"]["warnings"].as_u64().unwrap() > 0,
        "{warned}"
    );

    let info = check_dir_path("info.mod").to_string_lossy().into_owned();
    let informed = dynare_workspace_diagnose(None, None, Some(&[info])).expect("info");
    assert_eq!(informed["summary"]["errors"], 0, "{informed}");
    assert_eq!(informed["summary"]["warnings"], 0, "{informed}");
    assert!(
        informed["summary"]["information"].as_u64().unwrap() > 0,
        "{informed}"
    );
}
