//! Batch diagnostics over an in-memory file map.
//!
//! Each call builds a fresh overlay workspace. Includes resolve only from that
//! map. A missing include fails that root and publishes no diagnostics.
//! Slice 14 wraps this; there is no public tool here.
//!
//! Slice 14 is the first non-test caller, so a lib build does not use this yet.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::diagnostic::{check_in_workspace, Diagnostic, Severity};
use crate::mcp::McpDiagnostic;
use crate::span::LineIndex;
use crate::workspace::Workspace;

/// Input error. No report is produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceDiagnoseError {
    EmptyMap,
    EmptyRoots,
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
    let files = normalize_files(files);
    let roots = normalize_roots(roots);
    let mut ws = Workspace::overlay_documents(&files);
    let ws_to_map = workspace_keys(&files);
    let mut reports = Vec::with_capacity(roots.len());
    for root in &roots {
        reports.push(diagnose_root(&mut ws, root, &files, &ws_to_map));
    }
    Ok(WorkspaceDiagnoseReport {
        summary: summarize(&reports),
        roots: reports,
    })
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
) -> WorkspaceRootReport {
    if !files.contains_key(root) {
        return failed(root, format!("\"{root}\" is not in the file map"));
    }
    if !root.ends_with(".mod") {
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

/// Spans that `check_in_workspace` already stores in the root text.
/// Mapping them again would treat those offsets as spliced coordinates.
fn is_root_text_code(code: &str) -> bool {
    matches!(code, "W060" | "W061" | "W062" | "W160" | "E061")
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
    if is_root_text_code(&diag.code) {
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

    use crate::dynare_diagnose;
    use crate::mcp::McpDiagnostic;
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
}
