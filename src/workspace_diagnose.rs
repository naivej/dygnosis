//! Batch diagnostics for `dynare_workspace_diagnose`.
//!
//! Map mode reads an in-memory file map. Path mode walks files and directories
//! for that call only, then diagnoses the texts it read. A missing include
//! fails that root and publishes no diagnostics.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io;
use std::path::{Path, PathBuf};

use crate::check_walk::{absolute_path, collect_mod_files, starts_with_plus};
use crate::diagnostic::{check_in_workspace, Diagnostic, Severity};
use crate::include_resolver::{normalize_uri, path_key};
use crate::mcp::McpDiagnostic;
use crate::span::LineIndex;
use crate::workspace::Workspace;

/// Input error. No report is produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceDiagnoseError {
    EmptyMap,
    EmptyRoots,
    /// Path mode walked no files and hit no missing or unreadable path.
    NoFiles,
}

/// `ok` or `failed` for one root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RootStatus {
    Ok,
    Failed,
}

/// Counts for one batch. Severity totals cover succeeded roots only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceDiagnoseSummary {
    pub checked: usize,
    pub failed: usize,
    pub errors: usize,
    pub warnings: usize,
    pub information: usize,
}

/// One diagnostic plus the map key of the file that owns its span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceDiagnostic {
    pub file: String,
    pub diagnostic: McpDiagnostic,
}

/// One deduplicated root, in normalized-key order when collected on a report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceRootReport {
    pub root: String,
    pub status: RootStatus,
    pub diagnostics: Vec<WorkspaceDiagnostic>,
    pub failure: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceDiagnoseReport {
    pub summary: WorkspaceDiagnoseSummary,
    pub roots: Vec<WorkspaceRootReport>,
}

/// Diagnose `roots` against `files`.
///
/// Map keys and root names use [`normalize_key`]. An empty map or an empty
/// roots list is an input error. One failed root does not drop the others.
pub(crate) fn diagnose_map(
    files: &BTreeMap<String, String>,
    roots: &[String],
) -> Result<WorkspaceDiagnoseReport, WorkspaceDiagnoseError> {
    if files.is_empty() {
        return Err(WorkspaceDiagnoseError::EmptyMap);
    }
    if roots.is_empty() {
        return Err(WorkspaceDiagnoseError::EmptyRoots);
    }
    Ok(diagnose_loaded(files, roots, true))
}

/// Diagnose files and directories on disk.
///
/// Directories recurse into `*.mod` and skip directories whose names start
/// with `+`. The same absolute path is diagnosed once. A missing or unreadable
/// path is a failed root. An empty result is [`WorkspaceDiagnoseError::NoFiles`].
pub(crate) fn diagnose_paths(
    paths: &[String],
) -> Result<WorkspaceDiagnoseReport, WorkspaceDiagnoseError> {
    let hits = expand_paths(paths);
    let mut files = BTreeMap::new();
    let mut readable = Vec::new();
    let mut failed_roots = Vec::new();
    for hit in hits {
        match hit {
            PathHit::File { key, text } => {
                files.insert(key.clone(), text);
                readable.push(key);
            }
            PathHit::Failed { key, failure } => failed_roots.push(failed(&key, failure)),
        }
    }
    if readable.is_empty() && failed_roots.is_empty() {
        return Err(WorkspaceDiagnoseError::NoFiles);
    }
    let mut reports = Vec::new();
    for key in &readable {
        let text = files.get(key).expect("readable path");
        reports.push(diagnose_disk_root(key, text));
    }
    reports.extend(failed_roots);
    reports.sort_by(|left, right| left.root.cmp(&right.root));
    Ok(WorkspaceDiagnoseReport {
        summary: summarize(&reports),
        roots: reports,
    })
}

/// One disk root, resolved the same way as `dygnosis check`.
///
/// Includes use the real search, including a parent `@#includepath`. A shared
/// basename in another directory is not a match.
fn diagnose_disk_root(root: &str, text: &str) -> WorkspaceRootReport {
    let mut ws = Workspace::new();
    ws.update_document(root, text.to_string());
    if let Some(name) = missing_include(&mut ws, root) {
        return failed(root, format!("unresolved @#include \"{name}\""));
    }
    let mut loaded = BTreeMap::new();
    loaded.insert(root.to_string(), text.to_string());
    let paths: Vec<PathBuf> = ws
        .include_records(root)
        .map(|records| {
            records
                .resolved
                .iter()
                .map(|row| row.path.clone())
                .collect()
        })
        .unwrap_or_default();
    for path in paths {
        if let Ok(body) = read_disk(&path) {
            loaded.insert(path_result_key(&path), body);
        }
    }
    let ws_to_map = disk_alias_keys(&loaded);
    let owners = writing_owners(&mut ws, root, &ws_to_map);
    let raw = check_in_workspace(&mut ws, root);
    let mut diagnostics = Vec::new();
    for diag in raw {
        if is_dropped(&diag.code) {
            continue;
        }
        diagnostics.push(sourced(&mut ws, root, &diag, &loaded, &ws_to_map, &owners));
    }
    WorkspaceRootReport {
        root: root.to_string(),
        status: RootStatus::Ok,
        diagnostics,
        failure: None,
    }
}

fn disk_alias_keys(files: &BTreeMap<String, String>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for key in files.keys() {
        out.insert(key.clone(), key.clone());
        let canonical = path_key(Path::new(key));
        out.insert(canonical.clone(), key.clone());
        out.insert(normalize_key(&canonical), key.clone());
        out.insert(normalize_uri(key), key.clone());
    }
    out
}

fn diagnose_loaded(
    files: &BTreeMap<String, String>,
    roots: &[String],
    require_mod: bool,
) -> WorkspaceDiagnoseReport {
    let files = normalize_files(files);
    let roots = normalize_roots(roots);
    let mut ws = Workspace::overlay_documents(&files);
    let ws_to_map = workspace_keys(&files);
    let mut reports = Vec::with_capacity(roots.len());
    for root in &roots {
        reports.push(diagnose_root(
            &mut ws,
            root,
            &files,
            &ws_to_map,
            require_mod,
        ));
    }
    WorkspaceDiagnoseReport {
        summary: summarize(&reports),
        roots: reports,
    }
}

enum PathHit {
    File { key: String, text: String },
    Failed { key: String, failure: String },
}

fn expand_paths(paths: &[String]) -> Vec<PathHit> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for path in paths {
        expand_one(path, &mut seen, &mut out);
    }
    out
}

fn expand_one(path: &str, seen: &mut BTreeSet<String>, out: &mut Vec<PathHit>) {
    let key = path_result_key(&absolute_path(path));
    if !seen.insert(key.clone()) {
        return;
    }
    let abs = PathBuf::from(&key);
    if abs.is_dir() {
        if starts_with_plus(&abs) {
            return;
        }
        match collect_mod_files(&abs) {
            Ok(files) => {
                for file in files {
                    expand_one(&file, seen, out);
                }
            }
            Err(err) => {
                let failure = cannot_read(&key, &err);
                out.push(PathHit::Failed { key, failure });
            }
        }
        return;
    }
    match read_disk(&abs) {
        Ok(text) => out.push(PathHit::File { key, text }),
        Err(err) => {
            let failure = if err.kind() == io::ErrorKind::NotFound {
                format!("File not found: {key}")
            } else {
                cannot_read(&key, &err)
            };
            out.push(PathHit::Failed { key, failure });
        }
    }
}

fn cannot_read(key: &str, err: &io::Error) -> String {
    format!("Cannot read {key}: {err}")
}

fn path_result_key(path: &Path) -> String {
    // `components` removes redundant `.` without folding `..`, case, or links.
    let without_current_dir: PathBuf = path.components().collect();
    normalize_key(&without_current_dir.to_string_lossy())
}

fn read_disk(path: &Path) -> io::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// `\` becomes `/`, then every trailing `/` is removed. No disk and no `..` fold.
fn normalize_key(key: &str) -> String {
    key.replace('\\', "/").trim_end_matches('/').to_string()
}

fn normalize_files(files: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (key, text) in files {
        out.insert(normalize_key(key), text.clone());
    }
    out
}

fn normalize_roots(roots: &[String]) -> Vec<String> {
    let mut set = BTreeSet::new();
    for root in roots {
        set.insert(normalize_key(root));
    }
    set.into_iter().collect()
}

fn workspace_keys(files: &BTreeMap<String, String>) -> HashMap<String, String> {
    files.keys().map(|key| (key.clone(), key.clone())).collect()
}

fn diagnose_root(
    ws: &mut Workspace,
    root: &str,
    files: &BTreeMap<String, String>,
    ws_to_map: &HashMap<String, String>,
    require_mod: bool,
) -> WorkspaceRootReport {
    if !files.contains_key(root) {
        return failed(root, format!("\"{root}\" is not in the file map"));
    }
    if require_mod && !root.ends_with(".mod") {
        return failed(root, format!("\"{root}\" is not a .mod file"));
    }
    if let Some(name) = missing_include(ws, root) {
        return failed(root, format!("unresolved @#include \"{name}\""));
    }
    let owners = writing_owners(ws, root, ws_to_map);
    let raw = check_in_workspace(ws, root);
    let mut diagnostics = Vec::new();
    for diag in raw {
        if is_dropped(&diag.code) {
            continue;
        }
        diagnostics.push(sourced(ws, root, &diag, files, ws_to_map, &owners));
    }
    WorkspaceRootReport {
        root: root.to_string(),
        status: RootStatus::Ok,
        diagnostics,
        failure: None,
    }
}

fn missing_include(ws: &mut Workspace, root: &str) -> Option<String> {
    ws.find_unresolved_includes(root)
        .into_iter()
        .next()
        .map(|miss| miss.filename)
}

fn failed(root: &str, failure: String) -> WorkspaceRootReport {
    WorkspaceRootReport {
        root: root.to_string(),
        status: RootStatus::Failed,
        diagnostics: Vec::new(),
        failure: Some(failure),
    }
}

fn summarize(roots: &[WorkspaceRootReport]) -> WorkspaceDiagnoseSummary {
    let checked = roots
        .iter()
        .filter(|root| root.status == RootStatus::Ok)
        .count();
    let failed = roots
        .iter()
        .filter(|root| root.status == RootStatus::Failed)
        .count();
    let mut errors = 0;
    let mut warnings = 0;
    let mut information = 0;
    for root in roots.iter().filter(|root| root.status == RootStatus::Ok) {
        for diag in &root.diagnostics {
            match diag.diagnostic.severity.as_str() {
                "ERROR" => errors += 1,
                "WARNING" => warnings += 1,
                "INFORMATION" => information += 1,
                _ => {}
            }
        }
    }
    WorkspaceDiagnoseSummary {
        checked,
        failed,
        errors,
        warnings,
        information,
    }
}

/// Same codes `dynare_diagnose` omits before it builds MCP rows.
fn is_dropped(code: &str) -> bool {
    matches!(
        code,
        "E040" | "W040" | "W041" | "I041" | "W071" | "I070" | "I071" | "W080" | "W081" | "DYNR"
    )
}

fn writing_owners(
    ws: &mut Workspace,
    root: &str,
    ws_to_map: &HashMap<String, String>,
) -> HashMap<String, String> {
    let Some(model) = ws.get_effective_model(root).cloned() else {
        return HashMap::new();
    };
    let mut owners = HashMap::new();
    for diag in crate::diagnostic::analyze(&model) {
        if !crate::check_writing::is_writing_code(&diag.code) {
            continue;
        }
        let Some((ws_file, _)) = ws.map_effective_origin(root, diag.span) else {
            continue;
        };
        if let Some(map_key) = ws_to_map.get(&ws_file) {
            owners.insert(diag.code, map_key.clone());
        }
    }
    owners
}

fn sourced(
    ws: &mut Workspace,
    root: &str,
    diag: &Diagnostic,
    files: &BTreeMap<String, String>,
    ws_to_map: &HashMap<String, String>,
    writing_owners: &HashMap<String, String>,
) -> WorkspaceDiagnostic {
    let root_text = files.get(root).map(String::as_str).unwrap_or("");
    let as_root = WorkspaceDiagnostic {
        file: root.to_string(),
        diagnostic: to_mcp(root_text, diag),
    };
    if crate::diagnostic::is_root_text_code(&diag.code) {
        return as_root;
    }
    if crate::check_writing::is_writing_code(&diag.code) {
        let file = writing_owners
            .get(&diag.code)
            .cloned()
            .unwrap_or_else(|| root.to_string());
        return mcp_in_file(file, diag, files, as_root);
    }
    let Some((ws_file, origin)) = ws.map_effective_origin(root, diag.span) else {
        return as_root;
    };
    let Some(file) = ws_to_map.get(&ws_file).cloned() else {
        return as_root;
    };
    if file == root {
        return as_root;
    }
    let mut moved = diag.clone();
    moved.span = origin;
    mcp_in_file(file, &moved, files, as_root)
}

fn mcp_in_file(
    file: String,
    diag: &Diagnostic,
    files: &BTreeMap<String, String>,
    fallback: WorkspaceDiagnostic,
) -> WorkspaceDiagnostic {
    if file == fallback.file {
        return fallback;
    }
    let Some(text) = files.get(&file) else {
        return fallback;
    };
    WorkspaceDiagnostic {
        file,
        diagnostic: to_mcp(text, diag),
    }
}

fn to_mcp(text: &str, diag: &Diagnostic) -> McpDiagnostic {
    let index = LineIndex::new(text);
    let start = index.position(text, diag.span.start);
    let end = index.position(text, diag.span.end);
    McpDiagnostic {
        file: None,
        line: start.line + 1,
        column: start.character + 1,
        end_line: end.line + 1,
        end_column: end.character + 1,
        severity: severity_label(diag.severity).to_string(),
        code: diag.code.clone(),
        message: diag.message.clone(),
    }
}

fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "ERROR",
        Severity::Warning => "WARNING",
        Severity::Information => "INFORMATION",
        Severity::Hint => "HINT",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};
    use std::path::{Path, PathBuf};

    use crate::check_file;
    use crate::dynare_diagnose;
    use crate::dynare_workspace_diagnose;
    use crate::mcp::{
        McpDiagnostic, WORKSPACE_DIAGNOSE_BOTH, WORKSPACE_DIAGNOSE_NEITHER,
        WORKSPACE_DIAGNOSE_NO_FILES,
    };
    use crate::span::{LineIndex, Position};

    use super::*;

    const SHARED: &str = "batch13_shared.mod";
    const CAL: &str = "batch13_cal.txt";
    const ROOT_A: &str = "batch13_a.mod";
    const ROOT_B: &str = "batch13_b.mod";

    fn map_of(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, text)| ((*key).to_string(), (*text).to_string()))
            .collect()
    }

    fn hash_of(files: &BTreeMap<String, String>) -> HashMap<String, String> {
        files.clone().into_iter().collect()
    }

    fn root_with(term: &str) -> String {
        format!(
            "@#include \"{CAL}\"\nvar y;\nvarexo e;\nparameters a;\nmodel;\n@#include \"{SHARED}\"\ny = a * y(-1) + {term} + e;\nend;\n"
        )
    }

    fn shared_eq(name: &str) -> String {
        format!("y = {name};\n")
    }

    fn two_roots(shared_name: &str) -> BTreeMap<String, String> {
        map_of(&[
            (ROOT_A, &root_with("only_in_root_a")),
            (ROOT_B, &root_with("only_in_root_b")),
            (SHARED, &shared_eq(shared_name)),
            (CAL, "% calibration\n"),
        ])
    }

    fn by_root<'a>(report: &'a WorkspaceDiagnoseReport, root: &str) -> &'a WorkspaceRootReport {
        report
            .roots
            .iter()
            .find(|entry| entry.root == root)
            .unwrap_or_else(|| panic!("missing root {root}"))
    }

    fn mentions(entry: &WorkspaceRootReport, needle: &str) -> bool {
        entry
            .diagnostics
            .iter()
            .any(|diag| diag.diagnostic.message.contains(needle))
    }

    fn slice_diag<'a>(text: &'a str, diag: &McpDiagnostic) -> &'a str {
        let index = LineIndex::new(text);
        let start = index.offset(
            text,
            Position {
                line: diag.line.saturating_sub(1),
                character: diag.column.saturating_sub(1),
            },
        ) as usize;
        let end = index.offset(
            text,
            Position {
                line: diag.end_line.saturating_sub(1),
                character: diag.end_column.saturating_sub(1),
            },
        ) as usize;
        let start = start.min(text.len());
        let end = end.max(start).min(text.len());
        &text[start..end]
    }

    fn assert_matches_dynare(files: &BTreeMap<String, String>, entry: &WorkspaceRootReport) {
        let hash = hash_of(files);
        let text = files.get(&entry.root).expect("root text");
        let mcp = dynare_diagnose(text, Some(&entry.root), Some(&hash));
        assert_eq!(
            entry.diagnostics.len(),
            mcp.len(),
            "codes {:?}",
            entry
                .diagnostics
                .iter()
                .map(|diag| diag.diagnostic.code.as_str())
                .collect::<Vec<_>>()
        );
        for (got, want) in entry.diagnostics.iter().zip(&mcp) {
            assert_eq!(got.diagnostic.code, want.code);
            assert_eq!(got.diagnostic.message, want.message);
            assert_eq!(got.diagnostic.severity, want.severity);
            if got.file == entry.root {
                assert_eq!(got.diagnostic, *want, "{}", want.message);
            }
        }
    }

    #[test]
    fn shared_include_keeps_each_root_and_names_the_include() {
        let files = two_roots("only_in_shared");
        let report = diagnose_map(
            &files,
            &[ROOT_B.to_string(), ROOT_A.to_string(), ROOT_B.to_string()],
        )
        .expect("batch");

        assert_eq!(report.roots.len(), 2);
        assert_eq!(report.roots[0].root, ROOT_A);
        assert_eq!(report.roots[1].root, ROOT_B);
        assert!(report.roots.iter().all(|entry| entry.root != SHARED));
        assert!(report.roots.iter().all(|entry| entry.root != CAL));
        assert!(report
            .roots
            .iter()
            .all(|entry| entry.status == RootStatus::Ok));
        assert!(report.roots.iter().all(|entry| entry.failure.is_none()));

        let a = by_root(&report, ROOT_A);
        let b = by_root(&report, ROOT_B);
        assert!(mentions(a, "only_in_root_a"));
        assert!(!mentions(b, "only_in_root_a"));
        assert!(mentions(b, "only_in_root_b"));
        assert!(!mentions(a, "only_in_root_b"));

        assert_matches_dynare(&files, a);
        assert_matches_dynare(&files, b);

        for entry in [a, b] {
            let shared: Vec<_> = entry
                .diagnostics
                .iter()
                .filter(|diag| diag.diagnostic.message.contains("only_in_shared"))
                .collect();
            assert!(!shared.is_empty(), "{entry:?}");
            for diag in shared {
                assert_eq!(diag.file, SHARED);
                let slice = slice_diag(&files[SHARED], &diag.diagnostic);
                assert!(
                    slice.contains("only_in_shared"),
                    "slice {slice:?} diag {:?}",
                    diag.diagnostic
                );
            }
            let own = if entry.root == ROOT_A {
                "only_in_root_a"
            } else {
                "only_in_root_b"
            };
            let owned: Vec<_> = entry
                .diagnostics
                .iter()
                .filter(|diag| diag.diagnostic.message.contains(own))
                .collect();
            assert!(!owned.is_empty());
            for diag in owned {
                assert_eq!(diag.file, entry.root);
            }
        }
    }

    #[test]
    fn summary_counts_successful_roots_only() {
        let (files, roots) = partial_inputs();
        let report = diagnose_map(&files, &roots).expect("batch");
        assert_eq!(
            report.summary.checked + report.summary.failed,
            report.roots.len()
        );
        assert_eq!(report.summary.checked, 1);
        assert_eq!(report.summary.failed, 3);
        let good = by_root(&report, "batch13_good.mod");
        assert_eq!(report.summary.errors, severity_count(good, "ERROR"));
        assert_eq!(report.summary.warnings, severity_count(good, "WARNING"));
        assert_eq!(
            report.summary.information,
            severity_count(good, "INFORMATION")
        );
        assert!(report.summary.errors >= 1);
    }

    #[test]
    fn one_failed_root_keeps_the_other() {
        let (files, roots) = partial_inputs();
        let report = diagnose_map(&files, &roots).expect("batch");
        assert_eq!(
            report
                .roots
                .iter()
                .map(|entry| entry.root.as_str())
                .collect::<Vec<_>>(),
            vec![
                "batch13_absent.mod",
                "batch13_bad.mod",
                "batch13_good.mod",
                "batch13_notes.txt",
            ]
        );

        let absent = by_root(&report, "batch13_absent.mod");
        assert_eq!(absent.status, RootStatus::Failed);
        assert!(absent.diagnostics.is_empty());
        assert_eq!(
            absent.failure.as_deref(),
            Some("\"batch13_absent.mod\" is not in the file map")
        );

        let bad = by_root(&report, "batch13_bad.mod");
        assert_eq!(bad.status, RootStatus::Failed);
        assert!(bad.diagnostics.is_empty());
        assert_eq!(
            bad.failure.as_deref(),
            Some("unresolved @#include \"dygnosis_slice13_missing.inc\"")
        );
        assert!(report
            .roots
            .iter()
            .all(|entry| entry.root != "batch13_mid.mod"));

        let notes = by_root(&report, "batch13_notes.txt");
        assert_eq!(notes.status, RootStatus::Failed);
        assert!(notes.diagnostics.is_empty());
        assert_eq!(
            notes.failure.as_deref(),
            Some("\"batch13_notes.txt\" is not a .mod file")
        );

        let good = by_root(&report, "batch13_good.mod");
        assert_eq!(good.status, RootStatus::Ok);
        assert!(good.failure.is_none());
        assert!(mentions(good, "only_in_good"));
        assert_matches_dynare(&files, good);
    }

    fn severity_count(entry: &WorkspaceRootReport, severity: &str) -> usize {
        entry
            .diagnostics
            .iter()
            .filter(|diag| diag.diagnostic.severity == severity)
            .count()
    }

    fn partial_inputs() -> (BTreeMap<String, String>, Vec<String>) {
        let good = "\
// 产出
var y;
varexo e;
parameters a;
model;
y = a * y(-1) + only_in_good + e;
end;
";
        let bad = "\
@#include \"batch13_mid.mod\"
var y;
model;
y = 0;
end;
";
        let mid = "@#include \"dygnosis_slice13_missing.inc\"\n";
        let files = map_of(&[
            ("batch13_good.mod", good),
            ("batch13_bad.mod", bad),
            ("batch13_mid.mod", mid),
            ("batch13_notes.txt", "not a model\n"),
        ]);
        let roots = vec![
            "batch13_notes.txt".to_string(),
            "batch13_bad.mod".to_string(),
            "batch13_absent.mod".to_string(),
            "batch13_good.mod".to_string(),
        ];
        (files, roots)
    }

    #[test]
    fn empty_map_and_empty_roots_are_input_errors() {
        let mut files = BTreeMap::new();
        files.insert("batch13_good.mod".to_string(), "var y;\n".to_string());
        assert_eq!(
            diagnose_map(&BTreeMap::new(), &["batch13_good.mod".to_string()]),
            Err(WorkspaceDiagnoseError::EmptyMap)
        );
        assert_eq!(
            diagnose_map(&files, &[]),
            Err(WorkspaceDiagnoseError::EmptyRoots)
        );
        assert_eq!(
            diagnose_map(&BTreeMap::new(), &[]),
            Err(WorkspaceDiagnoseError::EmptyMap)
        );
    }

    #[test]
    fn changed_overlay_is_not_reused() {
        let mut files = two_roots("only_in_shared");
        let roots = vec![ROOT_A.to_string()];
        let first = diagnose_map(&files, &roots).expect("first");
        assert!(mentions(by_root(&first, ROOT_A), "only_in_shared"));
        assert!(!mentions(by_root(&first, ROOT_A), "replaced_in_second"));

        files.insert(SHARED.to_string(), shared_eq("replaced_in_second"));
        let second = diagnose_map(&files, &roots).expect("second");
        assert!(mentions(by_root(&second, ROOT_A), "replaced_in_second"));
        assert!(!mentions(by_root(&second, ROOT_A), "only_in_shared"));
        assert!(mentions(by_root(&first, ROOT_A), "only_in_shared"));
    }

    #[test]
    fn map_includepath_resolves_without_a_directory_and_is_inherited() {
        let dir = scratch("map-virtual-includepath");
        let root = slash_key(&dir.path.join("main.mod"));
        let part = slash_key(&dir.path.join("parts/first.inc"));
        let nested = slash_key(&dir.path.join("parts/second.inc"));
        let files = map_of(&[
            (
                root.as_str(),
                "@#includepath \"parts\"\n@#include \"first.inc\"\nvar y;\nmodel;\ny=0;\nend;\n",
            ),
            (part.as_str(), "@#include \"second.inc\"\n"),
            (nested.as_str(), "parameters only_nested;\n"),
        ]);
        assert!(!dir.path.join("parts").exists());
        let report = diagnose_map(&files, std::slice::from_ref(&root)).expect("map report");
        let entry = by_root(&report, &root);
        assert_eq!(entry.status, RootStatus::Ok, "{entry:?}");
        assert_eq!(entry.failure, None);
        assert!(!entry
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.code == "E304"));

        std::fs::create_dir_all(dir.path.join("parts")).expect("disk directory");
        let with_host_directory =
            diagnose_map(&files, std::slice::from_ref(&root)).expect("same map");
        assert_eq!(report, with_host_directory);
    }

    #[test]
    fn map_nested_includepath_is_relative_to_root_invocation() {
        let dir = scratch("map-nested-includepath");
        let root = slash_key(&dir.path.join("main.mod"));
        let first = slash_key(&dir.path.join("parts/first.inc"));
        let second = slash_key(&dir.path.join("more/second.inc"));
        let child_relative = slash_key(&dir.path.join("parts/more/second.inc"));
        let files = map_of(&[
            (
                root.as_str(),
                "@#include \"parts/first.inc\"\nvar y;\nmodel;\ny=0;\nend;\n",
            ),
            (
                first.as_str(),
                "@#includepath \"more\"\n@#include \"second.inc\"\n",
            ),
            (second.as_str(), "parameters root_hit;\n"),
            (child_relative.as_str(), "parameters child_hit;\n"),
        ]);
        assert!(!dir.path.join("parts").exists());
        let report = diagnose_map(&files, std::slice::from_ref(&root)).expect("nested map");
        let entry = by_root(&report, &root);
        assert_eq!(entry.status, RootStatus::Ok, "{entry:?}");
        assert!(!entry
            .diagnostics
            .iter()
            .any(|diag| diag.diagnostic.code == "E304"));
        assert!(mentions(entry, "root_hit"), "{entry:?}");
        assert!(!mentions(entry, "child_hit"), "{entry:?}");
    }

    #[test]
    fn map_nested_includepath_does_not_use_child_directory_for_e304() {
        let dir = scratch("map-nested-includepath-e304");
        let root = slash_key(&dir.path.join("main.mod"));
        let first = slash_key(&dir.path.join("parts/first.inc"));
        let child_relative = slash_key(&dir.path.join("parts/more/unused.inc"));
        let files = map_of(&[
            (
                root.as_str(),
                "@#include \"parts/first.inc\"\nvar y;\nmodel;\ny=0;\nend;\n",
            ),
            (first.as_str(), "@#includepath \"more\"\n"),
            (child_relative.as_str(), "parameters child_only;\n"),
        ]);
        let report = diagnose_map(&files, std::slice::from_ref(&root)).expect("nested map");
        let entry = by_root(&report, &root);
        assert_eq!(entry.status, RootStatus::Ok, "{entry:?}");
        assert!(
            entry
                .diagnostics
                .iter()
                .any(|diag| diag.diagnostic.code == "E304"
                    && diag.file == first
                    && diag.diagnostic.line == 1
                    && diag.diagnostic.column == 1),
            "{entry:?}"
        );
    }

    #[test]
    fn map_includepath_colon_is_one_directory() {
        let root = "batch13_colon/main.mod";
        let files = map_of(&[
            (root, "@#includepath \"a:b\"\nvar y;\nmodel;\ny=0;\nend;\n"),
            ("batch13_colon/a/one.inc", ""),
            ("batch13_colon/b/two.inc", ""),
        ]);
        let report = diagnose_map(&files, &[root.to_string()]).expect("colon map");
        let entry = by_root(&report, root);
        assert_eq!(entry.status, RootStatus::Ok, "{entry:?}");
        assert!(
            entry
                .diagnostics
                .iter()
                .any(|diag| diag.diagnostic.code == "E304"),
            "{entry:?}"
        );
    }

    #[test]
    fn map_includepath_colon_does_not_search_each_component() {
        let root = "batch13_colon_search/main.mod";
        let files = map_of(&[
            (
                root,
                "@#includepath \"a:b\"\n@#include \"one.inc\"\nvar y;\nmodel;\ny=0;\nend;\n",
            ),
            ("batch13_colon_search/a/one.inc", "parameters wrong_hit;\n"),
            ("batch13_colon_search/b/two.inc", ""),
        ]);
        let report = diagnose_map(&files, &[root.to_string()]).expect("colon map");
        let entry = by_root(&report, root);
        assert_eq!(entry.status, RootStatus::Failed, "{entry:?}");
        assert_eq!(
            entry.failure.as_deref(),
            Some("unresolved @#include \"one.inc\"")
        );
    }

    #[test]
    fn map_includepath_dot_components_find_virtual_directory() {
        let root = "batch13_dot_dir/main.mod";
        let files = map_of(&[
            (root, "@#includepath \".\"\n@#includepath \"parts/.\"\n@#include \"leaf.inc\"\nvar y;\nmodel;\ny=0;\nend;\n"),
            ("batch13_dot_dir/parts/leaf.inc", "parameters hit;\n"),
        ]);
        let report = diagnose_map(&files, &[root.to_string()]).expect("dot map");
        let entry = by_root(&report, root);
        assert_eq!(entry.status, RootStatus::Ok, "{entry:?}");
        assert!(
            !entry
                .diagnostics
                .iter()
                .any(|diag| diag.diagnostic.code == "E304"),
            "{entry:?}"
        );
        assert!(mentions(entry, "hit"), "{entry:?}");
    }

    #[test]
    fn map_includepath_dot_accepts_relative_root_directory() {
        let root = "main.mod";
        let files = map_of(&[
            (
                root,
                "@#includepath \".\"\n@#include \"leaf.inc\"\nvar y;\nmodel;\ny=0;\nend;\n",
            ),
            ("leaf.inc", "parameters hit;\n"),
        ]);
        let report = diagnose_map(&files, &[root.to_string()]).expect("relative dot map");
        let entry = by_root(&report, root);
        assert_eq!(entry.status, RootStatus::Ok, "{entry:?}");
        assert!(
            !entry
                .diagnostics
                .iter()
                .any(|diag| diag.diagnostic.code == "E304"),
            "{entry:?}"
        );
    }

    #[test]
    fn map_includepath_leading_dot_accepts_relative_root_directory() {
        let root = "main.mod";
        let files = map_of(&[
            (
                root,
                "@#includepath \"./parts\"\n@#include \"leaf.inc\"\nvar y;\nmodel;\ny=0;\nend;\n",
            ),
            ("parts/leaf.inc", "parameters hit;\n"),
        ]);
        let report = diagnose_map(&files, &[root.to_string()]).expect("leading dot map");
        let entry = by_root(&report, root);
        assert_eq!(entry.status, RootStatus::Ok, "{entry:?}");
        assert!(
            !entry
                .diagnostics
                .iter()
                .any(|diag| diag.diagnostic.code == "E304"),
            "{entry:?}"
        );
    }

    #[test]
    fn map_includepath_empty_string_is_not_a_directory() {
        let root = "batch13_empty_dir/main.mod";
        let files = map_of(&[
            (root, "@#includepath \"\"\nvar y;\nmodel;\ny=0;\nend;\n"),
            ("batch13_empty_dir/sibling.inc", ""),
        ]);
        let report = diagnose_map(&files, &[root.to_string()]).expect("empty path map");
        let entry = by_root(&report, root);
        assert_eq!(entry.status, RootStatus::Ok, "{entry:?}");
        assert!(
            entry
                .diagnostics
                .iter()
                .any(|diag| diag.diagnostic.code == "E304"),
            "{entry:?}"
        );
    }

    #[test]
    fn child_includepath_persists_for_later_root_include() {
        let root = "batch13_child_effect/main.mod";
        let files = map_of(&[
            (
                root,
                "@#include \"child.inc\"\n@#include \"late.inc\"\nvar y;\nmodel;\ny=0;\nend;\n",
            ),
            ("batch13_child_effect/child.inc", "@#includepath \"more\"\n"),
            (
                "batch13_child_effect/more/late.inc",
                "parameters later_hit;\n",
            ),
        ]);
        let report = diagnose_map(&files, &[root.to_string()]).expect("child side effect");
        let entry = by_root(&report, root);
        assert_eq!(entry.status, RootStatus::Ok, "{entry:?}");
        assert!(mentions(entry, "later_hit"), "{entry:?}");
    }

    #[test]
    fn map_include_cannot_resolve_from_disk() {
        let dir = scratch("map-include-disk");
        let root = slash_key(&dir.path.join("main.mod"));
        let source = "@#include \"part.inc\"\nvar y;\nmodel;\ny=0;\nend;\n";
        let files = map_of(&[(root.as_str(), source)]);
        let roots = [root.clone()];
        let missing = diagnose_map(&files, &roots).expect("missing include");
        assert_eq!(by_root(&missing, &root).status, RootStatus::Failed);

        std::fs::write(dir.path.join("part.inc"), "parameters on_disk;\n").expect("disk include");
        let still_missing = diagnose_map(&files, &roots).expect("same map");
        assert_eq!(missing, still_missing);
    }

    #[test]
    fn map_includepath_applies_after_its_directive() {
        let root = "batch13_order/main.mod";
        let files = map_of(&[
            (
                root,
                "@#include \"part.inc\"\n@#includepath \"parts\"\nvar y;\nmodel;\ny=0;\nend;\n",
            ),
            ("batch13_order/parts/part.inc", "parameters too_late;\n"),
        ]);
        let report = diagnose_map(&files, &[root.to_string()]).expect("map report");
        let entry = by_root(&report, root);
        assert_eq!(entry.status, RootStatus::Failed);
        assert_eq!(
            entry.failure.as_deref(),
            Some("unresolved @#include \"part.inc\"")
        );
    }

    #[test]
    fn map_companions_depend_only_on_supplied_keys() {
        let dir = scratch("map-companion");
        let root = slash_key(&dir.path.join("main.mod"));
        let companion = dir.path.join("foo.mat");
        let steady_file = dir.path.join("main_steadystate.m");
        let source = "var y;\nmodel;\ny=0;\nend;\nsteady;\nestimation(datafile='foo.mat');\n";
        let files = map_of(&[(root.as_str(), source)]);
        let roots = [root.clone()];
        let missing = diagnose_map(&files, &roots).expect("missing companion");
        assert!(by_root(&missing, &root)
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.code == "W160"));
        assert!(by_root(&missing, &root)
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.code == "I050"));

        std::fs::write(&companion, "disk only").expect("disk companion");
        std::fs::write(&steady_file, "disk only").expect("disk steady file");
        let still_missing = diagnose_map(&files, &roots).expect("same map");
        assert_eq!(missing, still_missing);

        let mut supplied = files;
        supplied.insert(slash_key(&companion), "supplied".to_string());
        supplied.insert(slash_key(&steady_file), "supplied".to_string());
        let resolved = diagnose_map(&supplied, &roots).expect("supplied companion");
        assert!(!by_root(&resolved, &root)
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.code == "W160"));
        assert!(!by_root(&resolved, &root)
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.code == "I050"));
    }

    #[test]
    fn map_load_params_reads_only_supplied_text() {
        let dir = scratch("map-load-params");
        let root = slash_key(&dir.path.join("main.mod"));
        let named = dir.path.join("params.txt");
        let source = "var y;\nmodel;\ny=0;\nend;\nload_params_and_steady_state('params.txt');\n";
        let files = map_of(&[(root.as_str(), source)]);
        let roots = [root.clone()];
        let missing = diagnose_map(&files, &roots).expect("missing file");
        assert!(by_root(&missing, &root)
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.code == "E306"));

        std::fs::write(&named, "host_name 1\n").expect("host file");
        assert_eq!(missing, diagnose_map(&files, &roots).expect("same map"));

        let mut supplied = files;
        supplied.insert(slash_key(&named), "supplied_name 1\n".to_string());
        let mapped = diagnose_map(&supplied, &roots).expect("mapped file");
        let entry = by_root(&mapped, &root);
        assert!(!entry
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.code == "E306"));
        assert!(entry.diagnostics.iter().any(|d| {
            d.diagnostic.code == "W204" && d.diagnostic.message.contains("supplied_name")
        }));
        assert!(!entry.diagnostics.iter().any(|d| {
            d.diagnostic.code == "W204" && d.diagnostic.message.contains("host_name")
        }));

        std::fs::write(&named, "changed_host_name 1\n").expect("change host file");
        assert_eq!(
            mapped,
            diagnose_map(&supplied, &roots).expect("same supplied text")
        );
    }

    #[test]
    fn root_without_includes_matches_dynare_diagnose() {
        let text = "\
// 产出
var y;
varexo e;
parameters a;
model;
y = a * y(-1) + solo_unknown + e;
end;
";
        let files = map_of(&[("batch13_solo.mod", text)]);
        let report = diagnose_map(&files, &["batch13_solo.mod".to_string()]).expect("batch");
        assert_eq!(report.roots.len(), 1);
        let entry = &report.roots[0];
        assert_eq!(entry.status, RootStatus::Ok);
        assert!(entry.failure.is_none());
        assert!(entry
            .diagnostics
            .iter()
            .all(|diag| diag.file == "batch13_solo.mod"));
        assert_matches_dynare(&files, entry);
        assert!(mentions(entry, "solo_unknown"));
    }

    #[test]
    fn case_and_dotdot_keys_stay_distinct() {
        let upper = "\
var y;
model;
y = only_upper;
end;
";
        let lower = "\
var y;
model;
y = only_lower;
end;
";
        let dotted = "\
var y;
model;
y = only_dotted;
end;
";
        let plain = "\
var y;
model;
y = only_plain;
end;
";
        let files = map_of(&[
            ("Batch13_Case.mod", upper),
            ("batch13_case.mod", lower),
            ("sub/../batch13_dot.mod", dotted),
            ("batch13_dot.mod", plain),
        ]);
        let report = diagnose_map(
            &files,
            &[
                "Batch13_Case.mod".to_string(),
                "batch13_case.mod".to_string(),
                "sub/../batch13_dot.mod".to_string(),
                "batch13_dot.mod".to_string(),
            ],
        )
        .expect("batch");
        assert_eq!(report.roots.len(), 4);
        assert!(mentions(by_root(&report, "Batch13_Case.mod"), "only_upper"));
        assert!(!mentions(
            by_root(&report, "Batch13_Case.mod"),
            "only_lower"
        ));
        assert!(mentions(by_root(&report, "batch13_case.mod"), "only_lower"));
        assert!(!mentions(
            by_root(&report, "batch13_case.mod"),
            "only_upper"
        ));
        assert!(mentions(
            by_root(&report, "sub/../batch13_dot.mod"),
            "only_dotted"
        ));
        assert!(!mentions(
            by_root(&report, "sub/../batch13_dot.mod"),
            "only_plain"
        ));
        assert!(mentions(by_root(&report, "batch13_dot.mod"), "only_plain"));
        assert!(!mentions(
            by_root(&report, "batch13_dot.mod"),
            "only_dotted"
        ));
    }

    #[test]
    fn normalized_roots_are_deduplicated_and_sorted() {
        let text = "\
var y;
model;
y = 1;
end;
";
        let mut files = BTreeMap::new();
        files.insert(r"dir\batch13_b.mod".to_string(), text.to_string());
        files.insert("dir/batch13_a.mod/".to_string(), text.to_string());
        files.insert("dir/../batch13_keep_dots.mod".to_string(), text.to_string());
        let report = diagnose_map(
            &files,
            &[
                r"dir\batch13_b.mod".to_string(),
                "dir/batch13_b.mod/".to_string(),
                "dir/batch13_a.mod".to_string(),
                "dir/../batch13_keep_dots.mod/".to_string(),
            ],
        )
        .expect("batch");
        assert_eq!(
            report
                .roots
                .iter()
                .map(|entry| entry.root.as_str())
                .collect::<Vec<_>>(),
            vec![
                "dir/../batch13_keep_dots.mod",
                "dir/batch13_a.mod",
                "dir/batch13_b.mod",
            ]
        );
        let norm = map_of(&[
            ("dir/batch13_a.mod", text),
            ("dir/batch13_b.mod", text),
            ("dir/../batch13_keep_dots.mod", text),
        ]);
        for entry in &report.roots {
            assert_eq!(entry.status, RootStatus::Ok);
            assert!(entry.diagnostics.iter().all(|diag| diag.file == entry.root));
            assert_matches_dynare(&norm, entry);
        }
        assert_eq!(report.summary.checked, 3);
        assert_eq!(report.summary.failed, 0);
    }

    fn scratch(label: &str) -> Scratch {
        let path =
            std::env::temp_dir().join(format!("dygnosis-b14-{}-{label}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch dir");
        Scratch { path }
    }

    struct Scratch {
        path: PathBuf,
    }

    impl Scratch {
        fn write(&self, name: &str, text: &str) -> PathBuf {
            let path = self.path.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("parent");
            }
            std::fs::write(&path, text).expect("write");
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    struct ReadBlock {
        _file: Option<std::fs::File>,
        path: PathBuf,
    }

    impl ReadBlock {
        fn deny(path: &Path) -> Self {
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                let file = std::fs::OpenOptions::new()
                    .read(true)
                    .share_mode(0)
                    .open(path)
                    .unwrap_or_else(|err| panic!("lock {}: {err}", path.display()));
                Self {
                    _file: Some(file),
                    path: path.to_path_buf(),
                }
            }
            #[cfg(not(windows))]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(path).expect("meta").permissions();
                perms.set_mode(0);
                std::fs::set_permissions(path, perms).expect("chmod");
                Self {
                    _file: None,
                    path: path.to_path_buf(),
                }
            }
        }
    }

    impl Drop for ReadBlock {
        fn drop(&mut self) {
            self._file.take();
            #[cfg(not(windows))]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = std::fs::metadata(&self.path) {
                    let mut perms = meta.permissions();
                    perms.set_mode(0o644);
                    let _ = std::fs::set_permissions(&self.path, perms);
                }
            }
            let _ = &self.path;
        }
    }

    fn tiny(name: &str) -> String {
        format!("var y;\nmodel;\ny = {name};\nend;\n")
    }

    fn disk_root(term: &str) -> String {
        format!(
            "var y;\nvarexo e;\nparameters a;\nmodel;\n@#include \"shared.mod\"\ny = a * y(-1) + {term} + e;\nend;\n"
        )
    }

    fn path_arg(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    fn slash_key(path: &Path) -> String {
        path_result_key(path)
    }

    fn root_named<'a>(body: &'a serde_json::Value, suffix: &str) -> &'a serde_json::Value {
        body["roots"]
            .as_array()
            .expect("roots")
            .iter()
            .find(|entry| {
                entry["root"]
                    .as_str()
                    .is_some_and(|root| root.ends_with(suffix))
            })
            .unwrap_or_else(|| panic!("missing root {suffix}: {body}"))
    }

    fn messages(entry: &serde_json::Value) -> String {
        entry["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .iter()
            .filter_map(|diag| diag["message"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn severity_total(body: &serde_json::Value, severity: &str) -> u64 {
        body["roots"]
            .as_array()
            .expect("roots")
            .iter()
            .filter(|root| root["status"].as_str() == Some("ok"))
            .map(|root| {
                root["diagnostics"]
                    .as_array()
                    .expect("diagnostics")
                    .iter()
                    .filter(|diag| diag["severity"].as_str() == Some(severity))
                    .count() as u64
            })
            .sum()
    }

    fn assert_summary(body: &serde_json::Value) {
        let roots = body["roots"].as_array().expect("roots");
        let checked = roots
            .iter()
            .filter(|root| root["status"].as_str() == Some("ok"))
            .count() as u64;
        let failed = roots
            .iter()
            .filter(|root| root["status"].as_str() == Some("failed"))
            .count() as u64;
        assert_eq!(body["summary"]["checked"].as_u64(), Some(checked));
        assert_eq!(body["summary"]["failed"].as_u64(), Some(failed));
        assert_eq!(checked + failed, roots.len() as u64);
        assert_eq!(
            body["summary"]["errors"].as_u64(),
            Some(severity_total(body, "ERROR"))
        );
        assert_eq!(
            body["summary"]["warnings"].as_u64(),
            Some(severity_total(body, "WARNING"))
        );
        assert_eq!(
            body["summary"]["information"].as_u64(),
            Some(severity_total(body, "INFORMATION"))
        );
    }

    fn assert_slash_abs(key: &str) {
        assert!(!key.contains('\\'), "{key}");
        assert!(
            key.starts_with('/') || (key.len() >= 3 && key.as_bytes()[1] == b':'),
            "{key}"
        );
    }

    fn cli_diags(text: &str, path: &str) -> Vec<McpDiagnostic> {
        check_file(text, path)
            .into_iter()
            .filter(|diag| !is_dropped(&diag.code))
            .map(|diag| to_mcp(text, &diag))
            .collect()
    }

    #[test]
    fn path_shared_include_workflow() {
        let dir = scratch("share");
        let shared = dir.write("shared.mod", "y = only_on_disk;\n");
        let a = dir.write("a.mod", &disk_root("only_in_root_a"));
        let b = dir.write("b.mod", &disk_root("only_in_root_b"));
        let paths = vec![path_arg(&b), path_arg(&a), path_arg(&a)];
        let body = dynare_workspace_diagnose(None, None, Some(&paths)).expect("path batch");
        assert_summary(&body);
        assert_eq!(body["summary"]["checked"].as_u64(), Some(2));
        assert_eq!(body["summary"]["failed"].as_u64(), Some(0));
        assert!(body["summary"]["errors"].as_u64().unwrap() >= 1);

        let keys: Vec<_> = body["roots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["root"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(keys.len(), 2);
        assert!(keys[0] < keys[1]);
        assert!(keys.iter().all(|key| !key.ends_with("/shared.mod")));
        for key in &keys {
            assert_slash_abs(key);
        }

        let left = root_named(&body, "/a.mod");
        let right = root_named(&body, "/b.mod");
        assert_eq!(left["status"], "ok");
        assert!(left["failure"].is_null());
        assert_eq!(right["status"], "ok");
        assert!(right["failure"].is_null());
        assert!(messages(left).contains("only_in_root_a"));
        assert!(!messages(left).contains("only_in_root_b"));
        assert!(messages(right).contains("only_in_root_b"));
        assert!(!messages(right).contains("only_in_root_a"));
        assert!(!messages(left).contains("only_in_map"));
        assert!(!messages(right).contains("only_in_map"));

        for entry in [left, right] {
            let owned: Vec<_> = entry["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|diag| messages_one(diag).contains("only_on_disk"))
                .collect();
            assert!(!owned.is_empty(), "{entry}");
            for diag in owned {
                let file = diag["file"].as_str().unwrap();
                assert_eq!(file, slash_key(&shared));
                let shared_text = std::fs::read_to_string(&shared).unwrap();
                let slice = json_slice(&shared_text, diag);
                assert!(slice.contains("only_on_disk"), "{slice:?}");
            }
        }

        let a_text = std::fs::read_to_string(&a).unwrap();
        let b_text = std::fs::read_to_string(&b).unwrap();
        let shared_text = std::fs::read_to_string(&shared).unwrap();
        let mut disk_files = HashMap::new();
        disk_files.insert(slash_key(&a), a_text);
        disk_files.insert(slash_key(&b), b_text);
        disk_files.insert(slash_key(&shared), shared_text);
        for entry in [left, right] {
            let root = entry["root"].as_str().unwrap();
            let text = disk_files.get(root).expect("root text");
            let want = dynare_diagnose(text, Some(root), Some(&disk_files));
            let got = entry["diagnostics"].as_array().unwrap();
            assert_eq!(got.len(), want.len(), "{root}");
            for (diag, mcp) in got.iter().zip(&want) {
                assert_eq!(diag["code"], mcp.code);
                assert_eq!(diag["message"], mcp.message);
                assert_eq!(diag["severity"], mcp.severity);
                if diag["file"].as_str() == Some(root) {
                    assert_eq!(diag["line"].as_u64(), Some(mcp.line as u64));
                    assert_eq!(diag["column"].as_u64(), Some(mcp.column as u64));
                    assert_eq!(diag["end_line"].as_u64(), Some(mcp.end_line as u64));
                    assert_eq!(diag["end_column"].as_u64(), Some(mcp.end_column as u64));
                }
            }
        }

        let mut map_files = HashMap::new();
        map_files.insert("a.mod".to_string(), disk_root("only_in_root_a"));
        map_files.insert("b.mod".to_string(), disk_root("only_in_root_b"));
        map_files.insert("shared.mod".to_string(), "y = only_in_map;\n".to_string());
        let roots = vec!["b.mod".to_string(), "a.mod".to_string()];
        let mapped =
            dynare_workspace_diagnose(Some(&map_files), Some(&roots), None).expect("map batch");
        assert_eq!(mapped["roots"][0]["root"], "a.mod");
        assert_eq!(mapped["roots"][1]["root"], "b.mod");
        assert!(messages(&mapped["roots"][0]).contains("only_in_map"));
        assert!(!messages(&mapped["roots"][0]).contains("only_on_disk"));
        assert!(messages(left).contains("only_on_disk"));

        std::fs::write(&shared, "y = replaced_on_disk;\n").unwrap();
        let again = dynare_workspace_diagnose(None, None, Some(&paths)).expect("second path");
        assert!(messages(root_named(&again, "/a.mod")).contains("replaced_on_disk"));
        assert!(!messages(root_named(&again, "/a.mod")).contains("only_on_disk"));
        assert!(messages(left).contains("only_on_disk"));
        assert!(!messages(&mapped["roots"][0]).contains("replaced_on_disk"));
    }

    fn messages_one(diag: &serde_json::Value) -> String {
        diag["message"].as_str().unwrap_or("").to_string()
    }

    fn json_slice<'a>(text: &'a str, diag: &serde_json::Value) -> &'a str {
        let line = diag["line"].as_u64().unwrap() as u32;
        let column = diag["column"].as_u64().unwrap() as u32;
        let end_line = diag["end_line"].as_u64().unwrap() as u32;
        let end_column = diag["end_column"].as_u64().unwrap() as u32;
        let index = LineIndex::new(text);
        let start = index.offset(
            text,
            Position {
                line: line.saturating_sub(1),
                character: column.saturating_sub(1),
            },
        ) as usize;
        let end = index.offset(
            text,
            Position {
                line: end_line.saturating_sub(1),
                character: end_column.saturating_sub(1),
            },
        ) as usize;
        let start = start.min(text.len());
        let end = end.max(start).min(text.len());
        &text[start..end]
    }

    #[test]
    fn path_dedup_recurse_and_skip_plus() {
        let dir = scratch("dedup");
        let a = dir.write("a.mod", &tiny("only_a"));
        let b = dir.write("nested/b.mod", &tiny("only_b"));
        let _hidden = dir.write("+skip/c.mod", &tiny("only_hidden"));
        let mut paths = vec![
            path_arg(&dir.path),
            path_arg(&a),
            format!("{}/./a.mod", slash_key(&dir.path)),
            slash_key(&a),
            path_arg(&b),
        ];
        if let Ok(cwd) = std::env::current_dir() {
            if let Ok(rel) = a.strip_prefix(&cwd) {
                paths.push(rel.to_string_lossy().into_owned());
            }
        }
        let body = dynare_workspace_diagnose(None, None, Some(&paths)).expect("dedup");
        assert_summary(&body);
        let keys: Vec<_> = body["roots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["root"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(keys, vec![slash_key(&a), slash_key(&b)]);
        assert!(messages(root_named(&body, "/a.mod")).contains("only_a"));
        assert!(messages(root_named(&body, "/nested/b.mod")).contains("only_b"));
        assert!(!body.to_string().contains("only_hidden"));
    }

    #[test]
    fn path_does_not_bind_another_files_basename() {
        let dir = scratch("basename");
        let main = dir.write(
            "a.mod",
            "@#include \"helper.mod\"\nvar y;\nmodel;\ny = 1;\nend;\n",
        );
        let _other = dir.write("sub/helper.mod", "var z;\nmodel;\nz = only_other;\nend;\n");
        let report = diagnose_paths(&[path_arg(&main), path_arg(&dir.path.join("sub/helper.mod"))])
            .expect("report");
        let entry = by_root(&report, &slash_key(&main));
        assert_eq!(entry.status, RootStatus::Failed);
        assert!(!mentions(entry, "only_other"));
        assert!(entry.diagnostics.is_empty());
    }

    #[test]
    fn path_missing_file_is_failed_root() {
        let dir = scratch("missing");
        let missing = dir.path.join("no-such.mod");
        let paths = vec![path_arg(&missing)];
        let body = dynare_workspace_diagnose(None, None, Some(&paths)).expect("report");
        assert_summary(&body);
        assert_eq!(body["summary"]["checked"].as_u64(), Some(0));
        assert_eq!(body["summary"]["failed"].as_u64(), Some(1));
        assert_eq!(body["summary"]["errors"].as_u64(), Some(0));
        let entry = &body["roots"][0];
        assert_eq!(entry["status"], "failed");
        assert!(entry["diagnostics"].as_array().unwrap().is_empty());
        let root = slash_key(&missing);
        let failure = format!("File not found: {root}");
        assert_eq!(entry["root"], root);
        assert_eq!(entry["failure"].as_str(), Some(failure.as_str()));
    }

    #[test]
    fn path_directory_mixes_good_and_unreadable() {
        let dir = scratch("mix");
        let good = dir.write("good.mod", &tiny("only_good"));
        let locked = dir.write("locked.mod", &tiny("only_locked"));
        let _hidden = dir.write("+skip/hidden.mod", &tiny("only_hidden"));
        let _block = ReadBlock::deny(&locked);
        let missing = dir.path.join("gone.mod");
        let paths = vec![path_arg(&dir.path), path_arg(&missing)];
        let body = dynare_workspace_diagnose(None, None, Some(&paths)).expect("report");
        assert_summary(&body);
        assert_eq!(body["summary"]["checked"].as_u64(), Some(1));
        assert_eq!(body["summary"]["failed"].as_u64(), Some(2));
        let good_entry = root_named(&body, "/good.mod");
        assert_eq!(good_entry["status"], "ok");
        assert!(good_entry["failure"].is_null());
        assert!(messages(good_entry).contains("only_good"));
        assert_eq!(good_entry["root"], slash_key(&good));

        let locked_entry = root_named(&body, "/locked.mod");
        assert_eq!(locked_entry["status"], "failed");
        assert!(locked_entry["diagnostics"].as_array().unwrap().is_empty());
        let failure = locked_entry["failure"].as_str().unwrap();
        assert!(
            failure.starts_with(&format!("Cannot read {}:", slash_key(&locked))),
            "{failure}"
        );
        assert!(!messages(good_entry).contains("only_locked"));
        assert!(!body.to_string().contains("only_hidden"));

        let gone = root_named(&body, "/gone.mod");
        assert_eq!(gone["status"], "failed");
        assert!(gone["diagnostics"].as_array().unwrap().is_empty());
        assert!(gone["failure"]
            .as_str()
            .unwrap()
            .starts_with("File not found:"));
        assert!(body["summary"]["errors"].as_u64().unwrap() >= 1);
    }

    #[test]
    fn path_unresolved_include_is_not_e061() {
        let dir = scratch("inc");
        let root = dir.write(
            "a.mod",
            "@#include \"missing_slice14.inc\"\nvar y;\nmodel;\ny = 0;\nend;\n",
        );
        let paths = vec![path_arg(&root)];
        let body = dynare_workspace_diagnose(None, None, Some(&paths)).expect("report");
        assert_summary(&body);
        let entry = &body["roots"][0];
        assert_eq!(entry["status"], "failed");
        assert!(entry["diagnostics"].as_array().unwrap().is_empty());
        assert_eq!(
            entry["failure"].as_str(),
            Some("unresolved @#include \"missing_slice14.inc\"")
        );
        assert!(!body.to_string().contains("E061"));
        assert_eq!(body["summary"]["errors"].as_u64(), Some(0));
    }

    #[test]
    fn path_without_includes_matches_diagnose_and_cli() {
        let dir = scratch("solo");
        let text = "\
// 产出
var y;
varexo e;
parameters a;
model;
y = a * y(-1) + solo_unknown + e;
end;
";
        let file = dir.write("solo.mod", text);
        let paths = vec![path_arg(&file)];
        let body = dynare_workspace_diagnose(None, None, Some(&paths)).expect("path");
        assert_summary(&body);
        let entry = &body["roots"][0];
        assert_eq!(entry["status"], "ok");
        assert!(entry["failure"].is_null());
        let root = entry["root"].as_str().unwrap();
        assert_eq!(root, slash_key(&file));
        let mut files = HashMap::new();
        files.insert(root.to_string(), text.to_string());
        let want = dynare_diagnose(text, Some(root), Some(&files));
        let cli = cli_diags(text, root);
        assert_eq!(want, cli);
        let got = entry["diagnostics"].as_array().unwrap();
        assert_eq!(got.len(), want.len());
        for (diag, mcp) in got.iter().zip(&want) {
            assert_eq!(diag["file"], root);
            assert_eq!(diag["code"], mcp.code);
            assert_eq!(diag["message"], mcp.message);
            assert_eq!(diag["severity"], mcp.severity);
            assert_eq!(diag["line"].as_u64(), Some(mcp.line as u64));
            assert_eq!(diag["column"].as_u64(), Some(mcp.column as u64));
            assert_eq!(diag["end_line"].as_u64(), Some(mcp.end_line as u64));
            assert_eq!(diag["end_column"].as_u64(), Some(mcp.end_column as u64));
        }
        assert!(messages(entry).contains("solo_unknown"));
    }

    #[test]
    fn path_explicit_non_mod_is_diagnosed() {
        let dir = scratch("incfile");
        let file = dir.write("piece.inc", &tiny("only_in_inc"));
        let paths = vec![path_arg(&file)];
        let body = dynare_workspace_diagnose(None, None, Some(&paths)).expect("path");
        let entry = &body["roots"][0];
        assert_eq!(entry["status"], "ok");
        assert!(entry["failure"].is_null());
        assert!(messages(entry).contains("only_in_inc"));

        let mut files = HashMap::new();
        files.insert("piece.inc".to_string(), tiny("only_in_inc"));
        let roots = vec!["piece.inc".to_string()];
        let mapped = dynare_workspace_diagnose(Some(&files), Some(&roots), None).expect("map");
        assert_eq!(mapped["roots"][0]["status"], "failed");
        assert_eq!(
            mapped["roots"][0]["failure"].as_str(),
            Some("\"piece.inc\" is not a .mod file")
        );
    }

    #[test]
    fn path_empty_inputs_and_empty_directory() {
        assert_eq!(
            dynare_workspace_diagnose(None, None, None).unwrap_err(),
            WORKSPACE_DIAGNOSE_NEITHER
        );
        let empty_files = HashMap::new();
        let empty_list = Vec::new();
        assert_eq!(
            dynare_workspace_diagnose(Some(&empty_files), Some(&empty_list), Some(&empty_list))
                .unwrap_err(),
            WORKSPACE_DIAGNOSE_NEITHER
        );
        let mut files = HashMap::new();
        files.insert("a.mod".to_string(), tiny("only_a"));
        let roots = vec!["a.mod".to_string()];
        assert_eq!(
            dynare_workspace_diagnose(Some(&files), None, None).unwrap_err(),
            WORKSPACE_DIAGNOSE_NEITHER
        );
        assert_eq!(
            dynare_workspace_diagnose(None, Some(&roots), None).unwrap_err(),
            WORKSPACE_DIAGNOSE_NEITHER
        );
        let paths = vec!["a.mod".to_string()];
        assert_eq!(
            dynare_workspace_diagnose(Some(&files), Some(&roots), Some(&paths)).unwrap_err(),
            WORKSPACE_DIAGNOSE_BOTH
        );
        assert_eq!(
            dynare_workspace_diagnose(Some(&files), None, Some(&paths)).unwrap_err(),
            WORKSPACE_DIAGNOSE_BOTH
        );
        assert_eq!(
            dynare_workspace_diagnose(None, Some(&roots), Some(&paths)).unwrap_err(),
            WORKSPACE_DIAGNOSE_BOTH
        );

        let dir = scratch("empty");
        let _notes = dir.write("notes.txt", "not a model\n");
        let _hidden = dir.write("+skip/hidden.mod", &tiny("only_hidden"));
        let only_dir = vec![path_arg(&dir.path)];
        assert_eq!(
            dynare_workspace_diagnose(None, None, Some(&only_dir)).unwrap_err(),
            WORKSPACE_DIAGNOSE_NO_FILES
        );
        assert_eq!(
            diagnose_paths(&[]).unwrap_err(),
            WorkspaceDiagnoseError::NoFiles
        );
    }
}
