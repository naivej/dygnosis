use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use dygnosis::preprocessor::{
    find_preprocessor, find_preprocessor_from, parse_preprocessor_output, reconcile_diagnostics,
    rewrite_supplied_absolute_includes, run_preprocessor, run_preprocessor_structured_with_finder,
    windows_common_candidates, PreprocessorResult, MISSING_BINARY_MESSAGE,
};
use dygnosis::span::LineIndex;
use dygnosis::{check_file, Diagnostic, Severity};

fn copilot_mod(archive_dir: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/model-archive")
        .join(archive_dir)
        .join(format!("{archive_dir}.mod"))
}

fn read_mod(archive_dir: &str) -> String {
    std::fs::read_to_string(copilot_mod(archive_dir))
        .unwrap_or_else(|e| panic!("fixture missing: {e}"))
        .replace("\r\n", "\n")
}

fn dummy_exe(dir: &Path, stem: &str) -> PathBuf {
    #[cfg(windows)]
    {
        let path = dir.join(format!("{stem}.bat"));
        std::fs::write(&path, b"@echo off\n").unwrap();
        path
    }
    #[cfg(not(windows))]
    {
        let path = dir.join(stem);
        std::fs::write(&path, b"#!/bin/sh\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("dygnosis-pp-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn diag(code: &str, severity: Severity) -> Diagnostic {
    Diagnostic::new(
        dygnosis::span::Span::new(0, 1),
        severity,
        code,
        format!("{code} message"),
    )
}

fn preproc(success: bool, codes: &[&str]) -> PreprocessorResult {
    PreprocessorResult {
        success,
        diagnostics: codes
            .iter()
            .map(|c| {
                let sev = if *c == "P000" {
                    Severity::Warning
                } else {
                    Severity::Error
                };
                diag(c, sev)
            })
            .collect(),
        raw_stdout: String::new(),
        raw_stderr: String::new(),
        path: PathBuf::from("dynare-preprocessor"),
        exit_code: Some(if success { 0 } else { 1 }),
    }
}

fn pad_lines(n: usize, width: usize) -> String {
    (0..n)
        .map(|_| "x".repeat(width))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn discovery_configured_wins() {
    let dir = temp_dir("cfg");
    let configured = dummy_exe(&dir, "configured");
    let env = dummy_exe(&dir, "env");
    let common = dummy_exe(&dir, "common");
    let found = find_preprocessor_from(Some(&configured), Some(&env), &[common]);
    assert_eq!(found.as_deref(), Some(configured.as_path()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn discovery_non_file_configured_skipped() {
    let dir = temp_dir("skip");
    let env = dummy_exe(&dir, "env");
    let found = find_preprocessor_from(Some(&dir), Some(&env), &[]);
    assert_eq!(found.as_deref(), Some(env.as_path()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn discovery_env_before_common() {
    let dir = temp_dir("env");
    let env = dummy_exe(&dir, "env");
    let common = dummy_exe(&dir, "common");
    let found = find_preprocessor_from(None, Some(&env), &[common]);
    assert_eq!(found.as_deref(), Some(env.as_path()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn discovery_empty_common_is_none() {
    assert_eq!(find_preprocessor_from(None, None, &[]), None);
}

#[test]
fn windows_candidate_shape_includes_dynare_version() {
    let cands = windows_common_candidates(&[(
        PathBuf::from(r"C:\dynare"),
        vec!["7.0".into(), "7.1".into()],
    )]);
    let want = PathBuf::from(r"C:\dynare\7.1\preprocessor\dynare-preprocessor.exe");
    assert!(
        cands.contains(&want),
        "candidates {cands:?} missing {want:?}"
    );
    assert_eq!(
        cands[0],
        PathBuf::from(r"C:\dynare\7.1\preprocessor\dynare-preprocessor.exe"),
        "newest listing first"
    );
}

#[test]
fn parse_error_col() {
    let text = pad_lines(15, 20);
    let diags = parse_preprocessor_output(
        "ERROR: model.mod: line 10, col 5: some message",
        None,
        &text,
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, "P001");
    assert_eq!(diags[0].severity, Severity::Error);
    assert_eq!(diags[0].message, "some message");
    let index = LineIndex::new(&text);
    let start = index.position(&text, diags[0].span.start);
    let end = index.position(&text, diags[0].span.end);
    assert_eq!(start.line, 9);
    assert_eq!(start.character, 4);
    assert_eq!(end.character, 5);
}

#[test]
fn parse_warning_line_only() {
    let text = pad_lines(15, 20);
    let diags = parse_preprocessor_output(
        "WARNING: model.mod: line 10: unused variable",
        None,
        &text,
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, "P001");
    assert_eq!(diags[0].severity, Severity::Warning);
    let index = LineIndex::new(&text);
    let start = index.position(&text, diags[0].span.start);
    let end = index.position(&text, diags[0].span.end);
    assert_eq!(start.line, 9);
    assert_eq!(start.character, 0);
    assert_eq!(end.character, 1);
}

#[test]
fn parse_cols_end_char_is_b_not_b_minus_one() {
    let text = pad_lines(15, 20);
    let diags = parse_preprocessor_output(
        "ERROR: model.mod: line 10, cols 5-12: span",
        None,
        &text,
    );
    assert_eq!(diags.len(), 1);
    let index = LineIndex::new(&text);
    let start = index.position(&text, diags[0].span.start);
    let end = index.position(&text, diags[0].span.end);
    assert_eq!(start.character, 4);
    assert_eq!(end.character, 12);
}

#[test]
fn parse_cross_file_prefix() {
    let text = pad_lines(5, 8);
    let diags = parse_preprocessor_output(
        "ERROR: helper.inc: line 3, col 2: included error",
        None,
        &text,
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].message, "[helper.inc:3:2] included error");
    let index = LineIndex::new(&text);
    let start = index.position(&text, diags[0].span.start);
    let end = index.position(&text, diags[0].span.end);
    assert_eq!((start.line, start.character), (0, 0));
    assert_eq!((end.line, end.character), (0, 1));
}

#[test]
fn parse_macro_processor() {
    let text = pad_lines(3, 8);
    let diags = parse_preprocessor_output(
        "ERROR in macro-processor: unexpected token",
        None,
        &text,
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, "P001");
    assert_eq!(diags[0].message, "unexpected token");
    assert_eq!(diags[0].severity, Severity::Error);
}

#[test]
fn parse_p000_timeout_message() {
    let dir = temp_dir("timeout");
    #[cfg(windows)]
    let sleeper = {
        let path = dir.join("sleep.bat");
        std::fs::write(&path, b"@echo off\nping -n 20 127.0.0.1 >nul\n").unwrap();
        path
    };
    #[cfg(not(windows))]
    let sleeper = {
        let path = dir.join("sleep.sh");
        std::fs::write(&path, b"#!/bin/sh\nsleep 20\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    };
    let result = run_preprocessor("var y;\n", &sleeper, None, Duration::from_secs(1));
    assert!(!result.success);
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code, "P000");
    assert_eq!(
        result.diagnostics[0].message,
        "Dynare preprocessor timed out after 1s"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn structured_missing_binary_via_empty_finder() {
    let value = run_preprocessor_structured_with_finder(
        "var y;\n",
        None,
        Duration::from_secs(30),
        || None,
    );
    assert_eq!(value["success"], false);
    assert_eq!(value["message"], MISSING_BINARY_MESSAGE);
    assert_eq!(value["diagnostics"], serde_json::json!([]));
}

#[test]
fn reconcile_none_unchanged() {
    let own = vec![diag("E001", Severity::Error), diag("W010", Severity::Warning)];
    let out = reconcile_diagnostics(&own, None);
    assert_eq!(
        out.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["E001", "W010"]
    );
}

#[test]
fn reconcile_success_drops_error_keeps_warning() {
    let own = vec![diag("E001", Severity::Error), diag("W010", Severity::Warning)];
    let pre = preproc(true, &[]);
    let out = reconcile_diagnostics(&own, Some(&pre));
    assert_eq!(
        out.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["W010"]
    );
}

#[test]
fn reconcile_success_keeps_e010() {
    let own = vec![diag("E010", Severity::Error), diag("E001", Severity::Error)];
    let pre = preproc(true, &[]);
    let out = reconcile_diagnostics(&own, Some(&pre));
    assert_eq!(
        out.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["E010"]
    );
}

#[test]
fn reconcile_reject_drops_e001() {
    let own = vec![diag("E001", Severity::Error), diag("W070", Severity::Warning)];
    let pre = preproc(false, &["P001"]);
    let out = reconcile_diagnostics(&own, Some(&pre));
    assert_eq!(
        out.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["W070", "P001"]
    );
}

#[test]
fn reconcile_only_p000_keeps_own() {
    let own = vec![diag("E001", Severity::Error)];
    let pre = preproc(false, &["P000"]);
    let out = reconcile_diagnostics(&own, Some(&pre));
    assert_eq!(
        out.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["E001", "P000"]
    );
}

#[test]
fn happy_path_trend_rbc_gov_inv_success() {
    let Some(pp) = find_preprocessor(None) else {
        eprintln!("skipping happy-path run: dynare-preprocessor not found");
        return;
    };
    let text = read_mod("trend_rbc_gov_inv");
    let source_dir = copilot_mod("trend_rbc_gov_inv").parent().map(Path::to_path_buf);
    let result = run_preprocessor(&text, &pp, source_dir.as_deref(), Duration::from_secs(30));
    assert!(
        result.success,
        "trend_rbc_gov_inv should be accepted: {:?}",
        result.diagnostics
    );
}

#[test]
fn happy_path_delete_model_end_rejects_with_non_p000() {
    let Some(pp) = find_preprocessor(None) else {
        eprintln!("skipping happy-path run: dynare-preprocessor not found");
        return;
    };
    let src = read_mod("trend_rbc_gov_inv");
    let mutated = src.replacen("log_n = log(n);\n\nend;", "log_n = log(n);\n", 1);
    assert_ne!(src, mutated);
    let result = run_preprocessor(&mutated, &pp, None, Duration::from_secs(30));
    assert!(!result.success);
    assert!(
        result.diagnostics.iter().any(|d| d.code != "P000"),
        "expected a real preprocessor error, got {:?}",
        result.diagnostics
    );
}

#[test]
fn happy_path_swff_source_dir_does_not_false_fail() {
    let Some(pp) = find_preprocessor(None) else {
        eprintln!("skipping happy-path run: dynare-preprocessor not found");
        return;
    };
    let text = read_mod("swff");
    let source_dir = copilot_mod("swff").parent().map(Path::to_path_buf);
    let result = run_preprocessor(&text, &pp, source_dir.as_deref(), Duration::from_secs(30));
    let joined: String = result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !joined.to_ascii_lowercase().contains("swff_params.inc")
            || result.success
            || !joined.to_ascii_lowercase().contains("could not open"),
        "swff should not false-fail on the sibling include: {joined}"
    );
}

#[test]
fn overlay_rewrite_absolute_include_remapped() {
    let src_dir = temp_dir("overlay-src");
    let mirror_dir = temp_dir("overlay-mirror");
    let original = src_dir.join("helper.inc");
    std::fs::write(&original, b"disk stale\n").unwrap();
    let abs = original.canonicalize().unwrap();
    let abs_s = abs.to_string_lossy();
    let abs_s = abs_s
        .strip_prefix(r"\\?\")
        .unwrap_or(&abs_s)
        .to_string();
    let include_path = abs_s.replace('\\', "/");
    let content = format!("@#include \"{include_path}\"\n");
    let target = mirror_dir.join("helper.inc");
    let mut map = HashMap::new();
    let key = if cfg!(windows) {
        abs_s.replace('/', "\\").to_lowercase()
    } else {
        abs_s.clone()
    };
    map.insert(key, target.to_string_lossy().into_owned());
    let rewritten = rewrite_supplied_absolute_includes(&content, &map);
    let want = target.to_string_lossy().replace('\\', "/");
    assert!(
        rewritten.contains(&want),
        "rewritten include should point at mirror {want}: {rewritten}"
    );
    assert!(
        !rewritten.contains(&include_path),
        "original absolute path should be remapped: {rewritten}"
    );
    let _ = std::fs::remove_dir_all(&src_dir);
    let _ = std::fs::remove_dir_all(&mirror_dir);
}

#[test]
fn library_check_file_has_no_preprocessor_codes() {
    let text = read_mod("trend_rbc_gov_inv");
    let path = copilot_mod("trend_rbc_gov_inv");
    let diags = check_file(&text, path.to_str().unwrap());
    for d in &diags {
        let rest = d.code.strip_prefix('P').unwrap_or("");
        assert!(
            rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit()),
            "check_file must stay preprocessor-free, got {}",
            d.code
        );
    }
}
