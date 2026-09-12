//! Dynare preprocessor discovery, run, parse, and reconcile.
//!
//! Transports call this after `analyze` / `check_file`. Those stay preprocessor-free.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{json, Value};

use crate::diagnostic::{Diagnostic, Severity};
use crate::include_resolver::{normalize_uri, uri_to_path};
use crate::parser::parse;
use crate::span::{LineIndex, Position, Span};
use crate::workspace::{split_includepath_argument, Workspace};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

pub const MISSING_BINARY_MESSAGE: &str = "Dynare preprocessor binary not found. Install Dynare, or set DYNARE_PREPROCESSOR to the dynare-preprocessor executable.";

const SAME_GROUND_ERRORS: &[&str] = &[
    "E001", "E020", "E021", "E023", "E024", "E025", "E030", "E058", "E059", "E060", "E061", "E062",
    "E063", "E064", "E065", "E090", "E093", "E095", "E100", "E101", "E103", "E111", "E130",
];

const SAME_GROUND_WARNINGS: &[&str] = &["W022", "W042", "W121", "W131", "W150"];

/// Result of one preprocessor run.
#[derive(Clone, Debug)]
pub struct PreprocessorResult {
    pub success: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub raw_stdout: String,
    pub raw_stderr: String,
    pub path: PathBuf,
    pub exit_code: Option<i32>,
}

/// Search order: configured file → `DYNARE_PREPROCESSOR` → common installs.
pub fn find_preprocessor(configured_path: Option<&Path>) -> Option<PathBuf> {
    let env = std::env::var_os("DYNARE_PREPROCESSOR").map(PathBuf::from);
    find_preprocessor_from(
        configured_path,
        env.as_deref(),
        &common_install_candidates(),
    )
}

/// Injectable discovery for tests (does not walk a real Dynare install).
pub fn find_preprocessor_from(
    configured_path: Option<&Path>,
    env_path: Option<&Path>,
    common_candidates: &[PathBuf],
) -> Option<PathBuf> {
    if let Some(path) = configured_path {
        if is_executable_file(path) {
            return Some(path.to_path_buf());
        }
    }
    if let Some(path) = env_path {
        if is_executable_file(path) {
            return Some(path.to_path_buf());
        }
    }
    for c in common_candidates {
        if is_executable_file(c) {
            return Some(c.clone());
        }
    }
    None
}

/// Windows candidate paths from `(root, listdir entries)` with reverse-sorted names.
pub fn windows_common_candidates(listings: &[(PathBuf, Vec<String>)]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for (root, entries) in listings {
        let mut sorted = entries.clone();
        sorted.sort();
        sorted.reverse();
        for entry in sorted {
            out.push(
                root.join(entry)
                    .join("preprocessor")
                    .join("dynare-preprocessor.exe"),
            );
        }
    }
    out
}

pub fn run_preprocessor(
    text: &str,
    preprocessor_path: &Path,
    source_dir: Option<&Path>,
    timeout: Duration,
) -> PreprocessorResult {
    let mut text = text.to_string();
    if let Some(stripped) = text.strip_prefix('\u{feff}') {
        text = stripped.to_string();
    }

    let tmp_dir = match make_tmp_dir("dynare_lsp_") {
        Ok(dir) => dir,
        Err(err) => {
            return spawn_fail_result(preprocessor_path, &text, &err.to_string());
        }
    };
    let source_dir_abs = source_dir.filter(|p| p.is_dir()).map(abs_path);
    let use_source_dir = source_dir_abs
        .as_ref()
        .is_some_and(|dir| requires_source_dir_file(&text) && dir.is_dir());

    let (tmp_file, run_cwd, generated_root) = if use_source_dir {
        let dir = source_dir_abs.as_ref().expect("checked");
        let tmp_file = unique_mod_in(dir);
        let stem = tmp_file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "model".into());
        let generated_root = dir.join(stem);
        (tmp_file, dir.clone(), Some(generated_root))
    } else {
        (tmp_dir.join("model.mod"), tmp_dir.clone(), None)
    };

    let write_err = std::fs::File::create(&tmp_file).and_then(|mut f| f.write_all(text.as_bytes()));
    if let Err(err) = write_err {
        cleanup_run(
            &tmp_dir,
            Some(&tmp_file),
            generated_root.as_deref(),
            source_dir_abs.as_deref(),
        );
        return spawn_fail_result(preprocessor_path, &text, &err.to_string());
    }

    let include_dirs = include_search_directories(&text, source_dir_abs.as_deref());
    let mut cmd = Command::new(preprocessor_path);
    cmd.arg(&tmp_file)
        .arg("json=check")
        .arg("onlyjson")
        .arg("nopreprocessoroutput");
    for dir in &include_dirs {
        cmd.arg(format!("-I{}", dir.display()));
    }
    cmd.current_dir(&run_cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let result = match cmd.spawn() {
        Ok(child) => wait_with_timeout(child, timeout),
        Err(err) => {
            cleanup_run(
                &tmp_dir,
                Some(&tmp_file),
                generated_root.as_deref(),
                source_dir_abs.as_deref(),
            );
            return spawn_fail_result(preprocessor_path, &text, &err.to_string());
        }
    };

    let outcome = match result {
        WaitOutcome::Timeout => PreprocessorResult {
            success: false,
            diagnostics: vec![p000_diagnostic(
                &text,
                format!("Dynare preprocessor timed out after {}s", timeout.as_secs()),
            )],
            raw_stdout: String::new(),
            raw_stderr: String::new(),
            path: preprocessor_path.to_path_buf(),
            exit_code: None,
        },
        WaitOutcome::Io(err) => spawn_fail_result(preprocessor_path, &text, &err.to_string()),
        WaitOutcome::Done {
            status,
            stdout,
            stderr,
        } => {
            let raw_stdout = stdout;
            let raw_stderr = stderr;
            let combined = format!("{raw_stderr}{raw_stdout}");
            let mut diagnostics = parse_preprocessor_output(&combined, Some(&tmp_file), &text);
            let code = status.code();
            let success = status.success();
            if !success && diagnostics.is_empty() {
                let detail = if combined.trim().is_empty() {
                    "without producing output"
                } else {
                    "without reporting a parseable diagnostic"
                };
                let exit = code.unwrap_or(-1);
                diagnostics.push(p000_diagnostic(
                    &text,
                    format!("Dynare preprocessor exited with code {exit} {detail}"),
                ));
            }
            PreprocessorResult {
                success,
                diagnostics,
                raw_stdout,
                raw_stderr,
                path: preprocessor_path.to_path_buf(),
                exit_code: code,
            }
        }
    };

    cleanup_run(
        &tmp_dir,
        Some(&tmp_file),
        generated_root.as_deref(),
        source_dir_abs.as_deref(),
    );
    outcome
}

/// Combine own diagnostics with a preprocessor verdict.
pub fn reconcile_diagnostics(
    own: &[Diagnostic],
    preproc: Option<&PreprocessorResult>,
) -> Vec<Diagnostic> {
    let Some(preproc) = preproc else {
        return own.to_vec();
    };
    let they_warned = preproc
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Warning && d.code != "P000");
    let they_refused = !preproc.success && preproc.diagnostics.iter().any(|d| d.code != "P000");
    let kept: Vec<Diagnostic> = if preproc.success {
        own.iter()
            .filter(|d| {
                if d.severity == Severity::Error {
                    return false;
                }
                if they_warned && SAME_GROUND_WARNINGS.contains(&d.code.as_str()) {
                    return false;
                }
                true
            })
            .cloned()
            .collect()
    } else if they_refused {
        own.iter()
            .filter(|d| !SAME_GROUND_ERRORS.contains(&d.code.as_str()))
            .cloned()
            .collect()
    } else {
        own.to_vec()
    };
    let mut out = kept;
    out.extend(preproc.diagnostics.iter().cloned());
    out
}

/// Find the binary (if any), run it, and reconcile. Missing binary leaves `own` unchanged.
pub fn maybe_run_and_reconcile(
    own: Vec<Diagnostic>,
    text: &str,
    source_dir: Option<&Path>,
    configured_path: Option<&Path>,
) -> Vec<Diagnostic> {
    match find_preprocessor(configured_path) {
        None => own,
        Some(path) => {
            let pre = run_preprocessor(text, &path, source_dir, DEFAULT_TIMEOUT);
            reconcile_diagnostics(&own, Some(&pre))
        }
    }
}

pub fn run_preprocessor_structured(
    text: &str,
    source_dir: Option<&Path>,
    timeout: Duration,
    configured_path: Option<&Path>,
) -> Value {
    run_preprocessor_structured_with_finder(text, source_dir, timeout, || {
        find_preprocessor(configured_path)
    })
}

pub fn run_preprocessor_structured_with_finder(
    text: &str,
    source_dir: Option<&Path>,
    timeout: Duration,
    find: impl FnOnce() -> Option<PathBuf>,
) -> Value {
    let Some(path) = find() else {
        return missing_binary_json();
    };
    result_to_structured(&run_preprocessor(text, &path, source_dir, timeout), text)
}

pub fn missing_binary_json() -> Value {
    json!({
        "success": false,
        "message": MISSING_BINARY_MESSAGE,
        "diagnostics": [],
    })
}

/// Point absolute `@#include` paths at their temp-mirror copies.
pub fn rewrite_supplied_absolute_includes(
    content: &str,
    target_by_norm: &HashMap<String, String>,
) -> String {
    let mut dir_by_norm: HashMap<String, String> = HashMap::new();
    for (norm_key, target) in target_by_norm {
        if let Some(parent) = Path::new(norm_key).parent() {
            let parent_s = parent.to_string_lossy();
            if !parent_s.is_empty() {
                dir_by_norm
                    .entry(normcase_str(&parent_s))
                    .or_insert_with(|| {
                        Path::new(target)
                            .parent()
                            .map(|p| p.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    });
            }
        }
    }

    let include_re = include_directive_re();
    let rewritten = include_re.replace_all(content, |caps: &regex::Captures| {
        let inner = caps
            .get(2)
            .or_else(|| caps.get(3))
            .map(|m| m.as_str())
            .unwrap_or("");
        match map_absolute(inner, target_by_norm, &dir_by_norm) {
            Some(mapped) => {
                let quote = if caps.get(2).is_some() { '"' } else { '\'' };
                format!("{}{quote}{mapped}{quote}", &caps[1])
            }
            None => caps[0].to_string(),
        }
    });

    let dir_by_norm = dir_by_norm;
    let target_by_norm = target_by_norm.clone();
    macro_directive_line_re()
        .replace_all(&rewritten, |caps: &regex::Captures| {
            let line = caps.get(0).map(|m| m.as_str()).unwrap_or("");
            quoted_literal_re()
                .replace_all(line, |qm: &regex::Captures| {
                    let (quote, raw) = if let Some(v) = qm.get(1) {
                        ("\"", v.as_str())
                    } else {
                        ("'", qm.get(2).map(|m| m.as_str()).unwrap_or(""))
                    };
                    match map_macro_literal(raw, &target_by_norm, &dir_by_norm) {
                        Some(mapped) => format!("{quote}{mapped}{quote}"),
                        None => qm[0].to_string(),
                    }
                })
                .into_owned()
        })
        .into_owned()
}

/// Materialize a `files` map in a temp tree, rewrite absolute includes, run.
pub fn run_workspace_preprocessor(
    entry_file: &str,
    files: &HashMap<String, String>,
    preprocessor_path: &Path,
    timeout: Duration,
) -> PreprocessorResult {
    let files = expand_workspace_files(entry_file, files);
    let tmp_root = match make_tmp_dir("dynare_lsp_mcp_") {
        Ok(dir) => dir,
        Err(err) => return spawn_fail_result(preprocessor_path, "", &err.to_string()),
    };
    let outcome = materialize_and_run(entry_file, &files, preprocessor_path, timeout, &tmp_root);
    remove_with_retries(&tmp_root);
    outcome
}

pub fn parse_preprocessor_output(
    output: &str,
    synthetic_path: Option<&Path>,
    model_text: &str,
) -> Vec<Diagnostic> {
    let synthetic_abs = synthetic_path.map(|p| normcase_path(&abs_path(p)));
    let mut diagnostics = Vec::new();
    let mut code_counter: u32 = 1;
    let lines: Vec<&str> = output
        .split('\n')
        .map(|l| l.trim_end_matches('\r'))
        .collect();
    let mut idx = 0;
    while idx < lines.len() {
        let stripped = lines[idx].trim();
        idx += 1;
        if let Some(caps) = preproc_line_re().captures(stripped) {
            let level = caps.get(1).map(|m| m.as_str()).unwrap_or("ERROR");
            let filename = caps.get(2).map(|m| m.as_str().trim()).unwrap_or("");
            let line_no = caps
                .get(3)
                .and_then(|m| m.as_str().parse::<i64>().ok())
                .unwrap_or(1)
                .saturating_sub(1)
                .max(0) as u32;
            let (col, end_line, end_col) = if let (Some(c0), Some(c1)) = (caps.get(7), caps.get(8))
            {
                let col = c0
                    .as_str()
                    .parse::<i64>()
                    .unwrap_or(1)
                    .saturating_sub(1)
                    .max(0) as u32;
                let end_col = c1.as_str().parse::<u32>().unwrap_or(col + 1);
                (col, line_no, end_col)
            } else {
                let col = caps
                    .get(4)
                    .and_then(|m| m.as_str().parse::<i64>().ok())
                    .map(|c| c.saturating_sub(1).max(0) as u32)
                    .unwrap_or(0);
                let end_line = caps
                    .get(5)
                    .and_then(|m| m.as_str().parse::<i64>().ok())
                    .map(|l| l.saturating_sub(1).max(0) as u32)
                    .unwrap_or(line_no);
                let end_col = caps
                    .get(6)
                    .and_then(|m| m.as_str().parse::<u32>().ok())
                    .unwrap_or(col + 1);
                (col, end_line, end_col)
            };
            let end_col = if end_line == line_no {
                end_col.max(col + 1)
            } else {
                end_col
            };
            let mut message = caps
                .get(9)
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_default();
            let is_synthetic =
                is_synthetic_filename(filename, synthetic_path, synthetic_abs.as_deref());
            if !filename.is_empty() && !is_synthetic {
                let label = diagnostic_file_label(filename, synthetic_path);
                message = format!("[{label}:{}:{}] {message}", line_no + 1, col + 1);
            }
            let severity = if level == "ERROR" {
                Severity::Error
            } else {
                Severity::Warning
            };
            let code = format!("P{code_counter:03}");
            code_counter += 1;
            let span = if !is_synthetic {
                range_span(model_text, 0, 0, 0, 1)
            } else {
                range_span(model_text, line_no, col, end_line, end_col)
            };
            diagnostics.push(Diagnostic::new(span, severity, code, message));
            continue;
        }

        let Some(macro_caps) = preproc_macro_re().captures(stripped) else {
            continue;
        };
        let level = macro_caps
            .get(1)
            .map(|m| m.as_str().to_ascii_uppercase())
            .unwrap_or_else(|| "ERROR".into());
        let mut message = (macro_caps.get(2).or_else(|| macro_caps.get(3)))
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();
        let mut rng = (0u32, 0u32, 0u32, 1u32);
        if message.to_ascii_lowercase().starts_with("backtrace") {
            let mut causes: Vec<String> = Vec::new();
            let mut located: Option<regex::Captures> = None;
            let mut inline_rng: Option<(u32, u32, u32, u32)> = None;
            while idx < lines.len() {
                let bullet = lines[idx].trim();
                if !bullet.starts_with('-') {
                    break;
                }
                idx += 1;
                let bullet_text = bullet.trim_start_matches('-').trim();
                if let Some(loc) = preproc_backtrace_re().captures(bullet_text) {
                    if located.is_none() {
                        located = Some(loc);
                    }
                    continue;
                }
                if let Some(inline) = preproc_macro_inline_re().captures(bullet_text) {
                    let (cause, bullet_rng) =
                        macro_inline_to_range(&inline, synthetic_path, synthetic_abs.as_deref());
                    if inline_rng.is_none() {
                        inline_rng = Some(bullet_rng);
                    }
                    if !cause.is_empty() {
                        causes.push(cause);
                    }
                    continue;
                }
                if !bullet_text.is_empty() {
                    causes.push(bullet_text.to_string());
                }
            }
            if !causes.is_empty() {
                message = causes.join("; ");
            }
            if let Some(loc) = located.as_ref() {
                let (msg, r) = macro_location_to_range(
                    loc,
                    &message,
                    synthetic_path,
                    synthetic_abs.as_deref(),
                );
                message = msg;
                rng = r;
            } else if let Some(r) = inline_rng {
                rng = r;
            }
        } else if let Some(inline) = preproc_macro_inline_re().captures(&message) {
            let (msg, r) = macro_inline_to_range(&inline, synthetic_path, synthetic_abs.as_deref());
            message = msg;
            rng = r;
        }
        let severity = if level == "ERROR" {
            Severity::Error
        } else {
            Severity::Warning
        };
        let code = format!("P{code_counter:03}");
        code_counter += 1;
        let span = range_span(model_text, rng.0, rng.1, rng.2, rng.3);
        diagnostics.push(Diagnostic::new(span, severity, code, message));
    }
    diagnostics
}

pub fn result_to_structured(result: &PreprocessorResult, model_text: &str) -> Value {
    json!({
        "success": result.success,
        "exit_code": result.exit_code,
        "diagnostics": result.diagnostics.iter().map(|d| diagnostic_to_struct(d, model_text)).collect::<Vec<_>>(),
        "raw_stdout": result.raw_stdout,
        "raw_stderr": result.raw_stderr,
    })
}

pub fn diagnostic_to_struct(d: &Diagnostic, model_text: &str) -> Value {
    let index = LineIndex::new(model_text);
    let start = index.position(model_text, d.span.start);
    json!({
        "line": start.line + 1,
        "column": start.character + 1,
        "severity": severity_name(d.severity),
        "message": d.message,
        "code": d.code,
    })
}

fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "ERROR",
        Severity::Warning => "WARNING",
        Severity::Information => "INFORMATION",
        Severity::Hint => "HINT",
    }
}

fn common_install_candidates() -> Vec<PathBuf> {
    platform_common_candidates()
}

#[cfg(windows)]
fn platform_common_candidates() -> Vec<PathBuf> {
    let mut listings = Vec::new();
    for root in [r"C:\Program Files\dynare", r"C:\dynare"] {
        let root = PathBuf::from(root);
        if !root.is_dir() {
            continue;
        }
        let entries = match std::fs::read_dir(&root) {
            Ok(rd) => rd
                .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
                .collect(),
            Err(_) => continue,
        };
        listings.push((root, entries));
    }
    windows_common_candidates(&listings)
}

#[cfg(target_os = "linux")]
fn platform_common_candidates() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/usr/lib/dynare/preprocessor/dynare-preprocessor"),
        PathBuf::from("/usr/local/lib/dynare/preprocessor/dynare-preprocessor"),
    ]
}

#[cfg(target_os = "macos")]
fn platform_common_candidates() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/Applications/Dynare/preprocessor/dynare-preprocessor"),
        PathBuf::from("/usr/local/lib/dynare/preprocessor/dynare-preprocessor"),
    ]
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn platform_common_candidates() -> Vec<PathBuf> {
    Vec::new()
}

fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(windows)]
    {
        let ext = path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default()
            .to_ascii_lowercase();
        let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        pathext
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| e.to_ascii_lowercase())
            .any(|e| e == ext)
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
}

fn p000_diagnostic(text: &str, message: String) -> Diagnostic {
    Diagnostic::new(
        range_span(text, 0, 0, 0, 1),
        Severity::Warning,
        "P000",
        message,
    )
}

fn spawn_fail_result(path: &Path, text: &str, err: &str) -> PreprocessorResult {
    let message = format!(
        "Could not run Dynare preprocessor '{}': {err}",
        path.display()
    );
    PreprocessorResult {
        success: false,
        diagnostics: vec![p000_diagnostic(text, message)],
        raw_stdout: String::new(),
        raw_stderr: err.to_string(),
        path: path.to_path_buf(),
        exit_code: None,
    }
}

fn range_span(text: &str, start_line: u32, start_char: u32, end_line: u32, end_char: u32) -> Span {
    let index = LineIndex::new(text);
    let start = index.offset(
        text,
        Position {
            line: start_line,
            character: start_char,
        },
    );
    let end = index.offset(
        text,
        Position {
            line: end_line,
            character: end_char,
        },
    );
    Span { start, end }
}

fn strip_quotes(raw: &str) -> &str {
    let path = raw.trim();
    if path.len() >= 2 {
        let b = path.as_bytes();
        if (b[0] == b'\'' || b[0] == b'"') && b[0] == b[b.len() - 1] {
            return path[1..path.len() - 1].trim();
        }
    }
    path
}

fn is_absolute_macro_path(raw: &str) -> bool {
    let path = strip_quotes(raw);
    if path.is_empty() || path.contains("@{") {
        return false;
    }
    let p = Path::new(path);
    if p.is_absolute() {
        return true;
    }
    if path.starts_with('/') {
        return true;
    }
    let b = path.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'/' || b[2] == b'\\')
}

fn requires_source_dir_file(mod_text: &str) -> bool {
    let model = parse(mod_text);
    if model
        .includes
        .iter()
        .any(|inc| !is_absolute_macro_path(&inc.filename))
    {
        return true;
    }
    for dir in &model.includepaths {
        let parts = split_includepath_argument(&dir.argument);
        if parts.is_empty() || parts.iter().any(|p| !is_absolute_macro_path(p)) {
            return true;
        }
    }
    false
}

fn include_search_directories(mod_text: &str, source_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    let mut seen_directories: HashSet<String> = HashSet::new();
    let mut seen_files: HashSet<String> = HashSet::new();

    fn add_directory(path: &Path, directories: &mut Vec<PathBuf>, seen: &mut HashSet<String>) {
        let absolute = abs_path(path);
        let key = normcase_path(&absolute);
        if seen.insert(key) {
            directories.push(absolute);
        }
    }

    fn walk(
        content: &str,
        base_dir: Option<&Path>,
        directories: &mut Vec<PathBuf>,
        seen_directories: &mut HashSet<String>,
        seen_files: &mut HashSet<String>,
    ) {
        let model = parse(content);
        for inc in &model.includes {
            let include_path = strip_quotes(&inc.filename).to_string();
            if include_path.is_empty() || include_path.contains("@{") {
                continue;
            }
            let candidate = if is_absolute_macro_path(&include_path) {
                if !Path::new(&include_path).is_absolute() {
                    continue;
                }
                abs_path(Path::new(&include_path))
            } else if let Some(base) = base_dir {
                abs_path(&base.join(&include_path))
            } else {
                continue;
            };
            if !candidate.is_file() {
                continue;
            }
            if let Some(parent) = candidate.parent() {
                add_directory(parent, directories, seen_directories);
            }
            let file_key = normcase_path(&candidate);
            if !seen_files.insert(file_key) {
                continue;
            }
            let Ok(bytes) = std::fs::read(&candidate) else {
                continue;
            };
            let nested = String::from_utf8_lossy(&bytes);
            let nested = nested.strip_prefix('\u{feff}').unwrap_or(&nested);
            let parent = candidate.parent().map(Path::to_path_buf);
            walk(
                nested,
                parent.as_deref(),
                directories,
                seen_directories,
                seen_files,
            );
        }
    }

    walk(
        mod_text,
        source_dir,
        &mut directories,
        &mut seen_directories,
        &mut seen_files,
    );
    directories
}

fn expand_workspace_files(
    entry_file: &str,
    files: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut ws = Workspace::new();
    for (name, content) in files {
        ws.update_document(name, content);
    }
    let supplied: HashSet<String> = files.keys().map(|f| normalize_uri(f)).collect();
    let mut expanded = files.clone();
    for (path_key, _model) in ws.resolve_all_includes(entry_file) {
        if supplied.contains(&path_key) {
            continue;
        }
        if let Some(src) = ws.get_source(&path_key) {
            expanded.insert(path_key, src.to_string());
        }
    }
    expanded
}

fn materialize_and_run(
    entry_file: &str,
    files: &HashMap<String, String>,
    preprocessor_path: &Path,
    timeout: Duration,
    tmp_root: &Path,
) -> PreprocessorResult {
    let normalized: HashMap<String, PathBuf> = files
        .keys()
        .map(|fname| (fname.clone(), uri_to_path(&normalize_uri(fname))))
        .collect();
    let parents: Vec<PathBuf> = normalized
        .values()
        .filter_map(|p| p.parent().map(Path::to_path_buf))
        .collect();
    let common_parent = commonpath(&parents).unwrap_or_else(|| {
        normalized
            .get(entry_file)
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| tmp_root.to_path_buf())
    });

    let relative_path = |fname: &str| -> PathBuf {
        let path = &normalized[fname];
        match path.strip_prefix(&common_parent) {
            Ok(rel)
                if !rel.is_absolute()
                    && rel
                        .components()
                        .all(|c| c.as_os_str() != std::ffi::OsStr::new("..")) =>
            {
                rel.to_path_buf()
            }
            _ => external_rel(path),
        }
    };

    let entry_parent = tmp_root.join(
        relative_path(entry_file)
            .parent()
            .unwrap_or_else(|| Path::new("")),
    );
    if let Err(err) = std::fs::create_dir_all(&entry_parent) {
        return spawn_fail_result(preprocessor_path, "", &err.to_string());
    }

    let mut planned: Vec<(String, PathBuf)> = Vec::new();
    let mut target_by_norm: HashMap<String, String> = HashMap::new();
    let mut target_owners: HashMap<String, String> = HashMap::new();
    for fname in files.keys() {
        let target = tmp_root.join(relative_path(fname));
        let target_key = normcase_path(&target);
        if let Some(prior) = target_owners.get(&target_key) {
            if normalized[prior] != normalized[fname] {
                return spawn_fail_result(
                    preprocessor_path,
                    files.get(entry_file).map(String::as_str).unwrap_or(""),
                    &format!(
                        "workspace files map to the same staged path: {prior:?} and {fname:?}"
                    ),
                );
            }
        }
        target_owners.insert(target_key, fname.clone());
        planned.push((fname.clone(), target.clone()));
        let norm_key = normcase_path(&abs_path(&normalized[fname]));
        target_by_norm.insert(norm_key, target.to_string_lossy().into_owned());
    }

    let rewritten: HashMap<String, String> = planned
        .iter()
        .map(|(fname, _)| {
            (
                fname.clone(),
                rewrite_supplied_absolute_includes(&files[fname], &target_by_norm),
            )
        })
        .collect();

    let mut basename_counts: HashMap<String, usize> = HashMap::new();
    let mut materialized: Vec<(String, String, PathBuf)> = Vec::new();
    for (fname, target) in &planned {
        let content = &rewritten[fname];
        if let Some(parent) = target.parent() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                return spawn_fail_result(preprocessor_path, content, &err.to_string());
            }
        }
        if let Err(err) = std::fs::write(target, content.as_bytes()) {
            return spawn_fail_result(preprocessor_path, content, &err.to_string());
        }
        let name = target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        *basename_counts.entry(name).or_insert(0) += 1;
        materialized.push((fname.clone(), content.clone(), target.clone()));
    }

    for (fname, content, target) in &materialized {
        if !is_bare_virtual_key(fname) {
            continue;
        }
        let name = target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if basename_counts.get(&name) != Some(&1) {
            continue;
        }
        let alias = entry_parent.join(&name);
        if alias.exists() {
            continue;
        }
        let _ = std::fs::write(&alias, content.as_bytes());
    }

    let original_entry = files.get(entry_file).cloned().unwrap_or_default();
    let entry_text = rewritten.get(entry_file).cloned().unwrap_or_default();
    let mut result = run_preprocessor(&entry_text, preprocessor_path, Some(&entry_parent), timeout);
    if entry_text != original_entry {
        remap_spans_to_text(&mut result.diagnostics, &entry_text, &original_entry);
    }
    result
}

fn remap_spans_to_text(diags: &mut [Diagnostic], from_text: &str, to_text: &str) {
    let from = LineIndex::new(from_text);
    let to = LineIndex::new(to_text);
    for d in diags {
        let start = from.position(from_text, d.span.start);
        let end = from.position(from_text, d.span.end);
        d.span.start = to.offset(to_text, start);
        d.span.end = to.offset(to_text, end);
    }
}

fn is_bare_virtual_key(fname: &str) -> bool {
    if fname.starts_with("file:") {
        return false;
    }
    if is_absolute_macro_path(fname) {
        return false;
    }
    !fname.replace('\\', "/").contains('/')
}

fn external_rel(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let parent_key = normcase_str(&parent.to_string_lossy());
    let parent_id = fnv1a_hex16(&parent_key);
    PathBuf::from("__external__")
        .join(parent_id)
        .join(path.file_name().unwrap_or_default())
}

fn fnv1a_hex16(s: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn commonpath(paths: &[PathBuf]) -> Option<PathBuf> {
    if paths.is_empty() {
        return None;
    }
    let mut prefix = paths[0].clone();
    for p in &paths[1..] {
        prefix = common_prefix(&prefix, p)?;
    }
    if prefix.as_os_str().is_empty() {
        None
    } else {
        Some(prefix)
    }
}

fn common_prefix(a: &Path, b: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    let mut matched = false;
    for (ca, cb) in a.components().zip(b.components()) {
        let same = if cfg!(windows) {
            ca.as_os_str().eq_ignore_ascii_case(cb.as_os_str())
        } else {
            ca == cb
        };
        if !same {
            break;
        }
        out.push(ca);
        matched = true;
    }
    if matched {
        Some(out)
    } else {
        None
    }
}

fn map_absolute(
    raw: &str,
    target_by_norm: &HashMap<String, String>,
    dir_by_norm: &HashMap<String, String>,
) -> Option<String> {
    let raw = raw.trim();
    if !is_absolute_macro_path(raw) {
        return None;
    }
    let key = normcase_path(&abs_path(Path::new(raw)));
    if let Some(target) = target_by_norm.get(&key) {
        return Some(target.replace('\\', "/"));
    }
    if let Some(mirror_dir) = dir_by_norm.get(&key) {
        return Some(mirror_dir.replace('\\', "/"));
    }
    None
}

fn with_trailing_separator(raw: &str, mapped: String) -> String {
    let trimmed = raw.trim_end();
    if (trimmed.ends_with('/') || trimmed.ends_with('\\')) && !mapped.ends_with('/') {
        return mapped + "/";
    }
    mapped
}

fn map_macro_literal(
    raw: &str,
    target_by_norm: &HashMap<String, String>,
    dir_by_norm: &HashMap<String, String>,
) -> Option<String> {
    if let Some(mapped) = map_absolute(raw, target_by_norm, dir_by_norm) {
        return Some(with_trailing_separator(raw, mapped));
    }
    let parts = split_includepath_argument(raw);
    if parts.len() < 2 {
        return None;
    }
    let mut rewritten = Vec::new();
    let mut changed = false;
    for part in &parts {
        if let Some(mapped) = map_absolute(part, target_by_norm, dir_by_norm) {
            rewritten.push(with_trailing_separator(part, mapped));
            changed = true;
        } else {
            rewritten.push(part.clone());
        }
    }
    if changed {
        Some(rewritten.join(":"))
    } else {
        None
    }
}

fn is_synthetic_filename(
    filename: &str,
    synthetic_path: Option<&Path>,
    synthetic_abs: Option<&str>,
) -> bool {
    if filename.is_empty() {
        return true;
    }
    let basename = Path::new(filename)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| filename.to_string());
    if let Some(synthetic_abs) = synthetic_abs {
        let synthetic_name = synthetic_path
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if filename == basename {
            return basename.eq_ignore_ascii_case(&synthetic_name);
        }
        return normcase_path(&abs_path(Path::new(filename))) == synthetic_abs;
    }
    basename == "model.mod"
}

fn diagnostic_file_label(filename: &str, synthetic_path: Option<&Path>) -> String {
    if filename.is_empty() {
        return String::new();
    }
    let basename = Path::new(filename)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| filename.to_string());
    if synthetic_path.is_none() || filename == basename {
        return slash_path(&basename);
    }
    let filename_is_abs = is_absolute_macro_path(filename) && Path::new(filename).is_absolute();
    if filename_is_abs {
        let filename_abs = abs_path(Path::new(filename));
        if let Some(synthetic_path) = synthetic_path {
            if let Some(synthetic_dir) = abs_path(synthetic_path).parent() {
                if let Ok(rel) = filename_abs.strip_prefix(synthetic_dir) {
                    return slash_path(&rel.to_string_lossy());
                }
            }
        }
        return slash_path(&filename_abs.to_string_lossy());
    }
    slash_path(filename)
}

fn slash_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn macro_location_to_range(
    loc: &regex::Captures,
    message: &str,
    synthetic_path: Option<&Path>,
    synthetic_abs: Option<&str>,
) -> (String, (u32, u32, u32, u32)) {
    let line0 = loc
        .get(2)
        .and_then(|m| m.as_str().parse::<i64>().ok())
        .unwrap_or(1)
        .saturating_sub(1)
        .max(0) as u32;
    let col0 = loc
        .get(3)
        .and_then(|m| m.as_str().parse::<i64>().ok())
        .unwrap_or(1)
        .saturating_sub(1)
        .max(0) as u32;
    let (end_line0, end_col0) = if let (Some(el), Some(ec)) = (loc.get(5), loc.get(6)) {
        (
            el.as_str()
                .parse::<i64>()
                .unwrap_or(1)
                .saturating_sub(1)
                .max(0) as u32,
            ec.as_str().parse::<u32>().unwrap_or(0),
        )
    } else if let Some(ec) = loc.get(4) {
        (
            line0,
            ec.as_str().parse::<u32>().unwrap_or(col0 + 1).max(col0 + 1),
        )
    } else {
        (line0, col0 + 1)
    };
    let path = loc.get(1).map(|m| m.as_str()).unwrap_or("");
    if macro_path_is_synthetic(path, synthetic_path, synthetic_abs) {
        (message.to_string(), (line0, col0, end_line0, end_col0))
    } else {
        let label = diagnostic_file_label(path, synthetic_path);
        (
            format!("[{label}:{}:{}] {message}", line0 + 1, col0 + 1),
            (0, 0, 0, 1),
        )
    }
}

fn macro_inline_to_range(
    inline: &regex::Captures,
    synthetic_path: Option<&Path>,
    synthetic_abs: Option<&str>,
) -> (String, (u32, u32, u32, u32)) {
    let directive = inline.get(1).map(|m| m.as_str());
    let path = inline.get(2).map(|m| m.as_str().trim()).unwrap_or("");
    let line0 = inline
        .get(3)
        .and_then(|m| m.as_str().parse::<i64>().ok())
        .unwrap_or(1)
        .saturating_sub(1)
        .max(0) as u32;
    let col0 = inline
        .get(4)
        .and_then(|m| m.as_str().parse::<i64>().ok())
        .unwrap_or(1)
        .saturating_sub(1)
        .max(0) as u32;
    let end_col0 = inline
        .get(5)
        .and_then(|m| m.as_str().parse::<u32>().ok())
        .unwrap_or(col0 + 1);
    let mut message = inline
        .get(6)
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_default();
    message = searched_dirs_re().replace(&message, "").into_owned();
    if let Some(directive) = directive {
        message = format!("{directive}: {message}");
    }
    if macro_path_is_synthetic(path, synthetic_path, synthetic_abs) {
        (message, (line0, col0, line0, end_col0.max(col0 + 1)))
    } else {
        let label = diagnostic_file_label(path, synthetic_path);
        (
            format!("[{label}:{}:{}] {message}", line0 + 1, col0 + 1),
            (0, 0, 0, 1),
        )
    }
}

fn macro_path_is_synthetic(
    path: &str,
    synthetic_path: Option<&Path>,
    synthetic_abs: Option<&str>,
) -> bool {
    let base = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    if path == base {
        if synthetic_abs.is_none() {
            return base == "model.mod";
        }
        let syn_name = synthetic_path
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        return base.eq_ignore_ascii_case(&syn_name);
    }
    let Some(synthetic_abs) = synthetic_abs else {
        return base == "model.mod";
    };
    normcase_path(&abs_path(Path::new(path))) == synthetic_abs
}

enum WaitOutcome {
    Done {
        status: std::process::ExitStatus,
        stdout: String,
        stderr: String,
    },
    Timeout,
    Io(std::io::Error),
}

fn wait_with_timeout(mut child: std::process::Child, timeout: Duration) -> WaitOutcome {
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut s) = stdout.take() {
            let _ = s.read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    });
    let err_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut s) = stderr.take() {
            let _ = s.read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = out_handle.join().unwrap_or_default();
                let stderr = err_handle.join().unwrap_or_default();
                return WaitOutcome::Done {
                    status,
                    stdout,
                    stderr,
                };
            }
            Ok(None) if start.elapsed() >= timeout => {
                terminate_tree(&child);
                let _ = child.wait();
                let _ = out_handle.join();
                let _ = err_handle.join();
                return WaitOutcome::Timeout;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(err) => return WaitOutcome::Io(err),
        }
    }
}

fn terminate_tree(child: &std::process::Child) {
    let pid = child.id();
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-9", &format!("-{pid}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn cleanup_run(
    tmp_dir: &Path,
    tmp_file: Option<&Path>,
    generated_root: Option<&Path>,
    source_dir_abs: Option<&Path>,
) {
    if let Some(root) = generated_root {
        remove_with_retries(root);
    }
    if let (Some(tmp_file), Some(source_dir)) = (tmp_file, source_dir_abs) {
        if let Some(parent) = tmp_file.parent() {
            if normcase_path(&abs_path(parent)) == normcase_path(source_dir) {
                remove_with_retries(tmp_file);
            }
        }
    }
    remove_with_retries(tmp_dir);
}

fn remove_with_retries(path: &Path) {
    for attempt in 0..4 {
        let result = if path.is_dir() {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        };
        match result {
            Ok(()) => return,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                if attempt + 1 == 4 {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20 * (1 << attempt)));
            }
            Err(_) => return,
        }
    }
}

fn make_tmp_dir(prefix: &str) -> std::io::Result<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("{prefix}{}_{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn unique_mod_in(dir: &Path) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    dir.join(format!(".dynare_lsp_{}_{nanos}.mod", std::process::id()))
}

fn abs_path(path: &Path) -> PathBuf {
    if let Ok(c) = std::fs::canonicalize(path) {
        return strip_verbatim(c);
    }
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

fn strip_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        p
    }
}

fn normcase_path(path: &Path) -> String {
    normcase_str(&path.to_string_lossy())
}

fn normcase_str(s: &str) -> String {
    if cfg!(windows) {
        s.replace('/', "\\").to_lowercase()
    } else {
        s.to_string()
    }
}

fn preproc_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(ERROR|WARNING):\s*(.+?):\s*line\s+(\d+)(?:,\s*(?:col\s+(\d+)(?:\s*-\s*line\s+(\d+),\s*col\s+(\d+))?|cols\s+(\d+)\s*-\s*(\d+)))?:\s*(.+)",
        )
        .expect("preproc line")
    })
}

fn preproc_macro_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:(ERROR|WARNING)\s+in\s+macro-processor:\s*(.+)|Macro-processing error:\s*(.+))",
        )
        .expect("macro line")
    })
}

fn preproc_backtrace_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#""([^"]+)"\s+line\s+(\d+),\s+col\s+(\d+)(?:\s*-\s*(\d+)|\s+to\s+line\s+(\d+),\s+col\s+(\d+))?"#,
        )
        .expect("backtrace")
    })
}

fn preproc_macro_inline_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?:(@#\w+):\s*)?(.*?):(\d+)\.(\d+)(?:-(\d+))?(?::|\s)\s*(.+)$")
            .expect("macro inline")
    })
}

fn include_directive_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)(@#\s*include\s*)(?:"([^"\n]+)"|'([^'\n]+)')"#).expect("include dir")
    })
}

fn macro_directive_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?im)^[ \t]*@#.*$").expect("macro dir line"))
}

fn quoted_literal_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#""([^"\n]*)"|'([^'\n]*)'"#).expect("quoted"))
}

fn searched_dirs_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\s*The following directories were searched:\s*$").expect("searched dirs")
    })
}
