//! MCP tool handlers (waves a–c) and stdio transport.
//!
//! Tests call these functions directly. Stdio is a thin wrap around the same
//! handlers.

use std::collections::{HashMap, HashSet};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_handler, tool_router, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{json, Value};

use crate::auto_fix::auto_fix;
use crate::catalog::list_options;
use crate::diagnostic::{analyze, check_in_workspace, Diagnostic, Severity};
use crate::explain;
use crate::include_resolver::normalize_uri;
use crate::model::Model;
use crate::model_diff::compare_models;
use crate::model_info::{classify_variable_timing, TimingClass};
use crate::parser::parse;
use crate::preprocessor::{
    find_preprocessor, maybe_run_and_reconcile, missing_binary_json, reconcile_diagnostics,
    result_to_structured, run_preprocessor_structured_with_finder, run_workspace_preprocessor,
    DEFAULT_TIMEOUT,
};
use crate::refs::{is_legal_ident, occurrences, rename_in_text};
use crate::span::LineIndex;
use crate::workspace::Workspace;
use crate::ParseSummary;

const OUT_CODES: &[&str] = &[
    "E040", "W040", "W041", "I041", "W071", "I070", "I071", "W080", "W081", "DYNR",
];

const TOOLS: &[(&str, &str)] = &[
    (
        "dynare_diagnose",
        "Run diagnostics on a .mod file and return code, range, severity, and message.",
    ),
    (
        "dynare_diagnose_workspace",
        "Run diagnostics with in-memory @#include files. active_file must be a key in files.",
    ),
    (
        "dynare_parse_summary",
        "Parse a .mod file and return a structured outline (names and counts).",
    ),
    (
        "dynare_explain",
        "Return markdown documentation for a diagnostic code.",
    ),
    (
        "dynare_list_diagnostic_codes",
        "List documented diagnostic codes and their titles.",
    ),
    (
        "dynare_list_options",
        "List valid options for a Dynare command, or list known commands when omitted.",
    ),
    (
        "dynare_find_references",
        "Find every whole-word use of a name in a .mod file. Skips comments.",
    ),
    (
        "dynare_find_references_workspace",
        "Find every use of a name across an @#include graph. Unrelated files in the map are skipped.",
    ),
    (
        "dynare_rename",
        "Rename a name throughout a .mod file. Skips comments. Returns the original text if the new name is not a legal identifier.",
    ),
    (
        "dynare_rename_workspace",
        "Rename a name across an @#include graph. Returns only files that changed, or an empty map if nothing applies.",
    ),
    (
        "dynare_auto_fix",
        "Apply stored diagnostic fixes to a .mod file. Leaves the text unchanged when macros would make the rewrite unsafe.",
    ),
    (
        "dynare_run_preprocessor",
        "Run the local Dynare preprocessor in check mode and return its verdict, parsed diagnostics, and raw output.",
    ),
    (
        "dynare_model_info",
        "Summarise a .mod file: names, counts, and equation timing (static, predetermined, forward-looking, mixed).",
    ),
    (
        "dynare_compare_models",
        "Compare two .mod files by names, calibrations, and equations.",
    ),
];

/// One diagnostic as MCP JSON: 1-based line/column, severity `ERROR`/`WARNING`/`INFORMATION`/`HINT`.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct McpDiagnostic {
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub severity: String,
    pub code: String,
    pub message: String,
}

/// One row from `dynare_list_diagnostic_codes`.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct DiagnosticCodeItem {
    pub code: String,
    pub title: String,
}

/// One hit from `dynare_find_references` (1-based; no end_line, no file).
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct McpReference {
    pub line: u32,
    pub column: u32,
    pub end_column: u32,
}

/// One hit from `dynare_find_references_workspace` (caller-supplied `file` key).
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct McpWorkspaceReference {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub end_column: u32,
}

/// Registered tool names in registration order.
pub fn registered_tool_names() -> Vec<&'static str> {
    TOOLS.iter().map(|(name, _)| *name).collect()
}

/// JSON for tools/list assertions (names + descriptions). No live client.
pub fn tools_list_json() -> Value {
    json!({
        "tools": TOOLS.iter().map(|(name, description)| {
            json!({ "name": name, "description": description })
        }).collect::<Vec<_>>()
    })
}

/// `analyze(&parse(text))`, then reconcile with the preprocessor when found.
pub fn dynare_diagnose(file_content: &str) -> Vec<McpDiagnostic> {
    let own = analyze(&parse(file_content));
    let diags = maybe_run_and_reconcile(own, file_content, None, None);
    diagnostics_to_json(file_content, &diags)
}

/// Overlay `files` on a workspace, then `check_in_workspace` for `active_file`.
///
/// Returns `[]` if `active_file` is not a key in `files`.
pub fn dynare_diagnose_workspace(
    active_file: &str,
    files: &HashMap<String, String>,
) -> Vec<McpDiagnostic> {
    let Some(text) = files.get(active_file) else {
        return Vec::new();
    };
    let mut ws = Workspace::new();
    for (name, content) in files {
        ws.update_document(name, content);
    }
    let own = check_in_workspace(&mut ws, active_file);
    let diags = match find_preprocessor(None) {
        Some(pp) => {
            let pre = run_workspace_preprocessor(active_file, files, &pp, DEFAULT_TIMEOUT);
            reconcile_diagnostics(&own, Some(&pre))
        }
        None => own,
    };
    diagnostics_to_json(text, &diags)
}

/// Run the preprocessor. With a `files` map, materialize overlays first.
pub fn dynare_run_preprocessor(
    file_content: &str,
    active_file: Option<&str>,
    files: Option<&HashMap<String, String>>,
) -> Value {
    dynare_run_preprocessor_with_finder(file_content, active_file, files, || {
        find_preprocessor(None)
    })
}

pub fn dynare_run_preprocessor_with_finder(
    file_content: &str,
    active_file: Option<&str>,
    files: Option<&HashMap<String, String>>,
    find: impl FnOnce() -> Option<std::path::PathBuf>,
) -> Value {
    let found = find();
    if let (Some(active_file), Some(files)) = (active_file, files) {
        if !files.is_empty() {
            let Some(pp) = found else {
                return missing_binary_json();
            };
            let mut workspace_files = files.clone();
            workspace_files.insert(active_file.to_string(), file_content.to_string());
            let result =
                run_workspace_preprocessor(active_file, &workspace_files, &pp, DEFAULT_TIMEOUT);
            return result_to_structured(&result, file_content);
        }
    }
    run_preprocessor_structured_with_finder(file_content, None, DEFAULT_TIMEOUT, || found)
}

/// Timing lists and counts from the equation AST. No `blocks` key.
pub fn dynare_model_info(
    file_content: &str,
    active_file: Option<&str>,
    files: Option<&HashMap<String, String>>,
) -> Value {
    model_info_json(&mcp_parse_model(file_content, active_file, files, false))
}

/// Structural `compare_models` JSON. No solver / steady-state keys.
pub fn dynare_compare_models(
    file_content_a: &str,
    file_content_b: &str,
    active_file_a: Option<&str>,
    active_file_b: Option<&str>,
    files_a: Option<&HashMap<String, String>>,
    files_b: Option<&HashMap<String, String>>,
    files: Option<&HashMap<String, String>>,
) -> Value {
    let model_a = mcp_parse_model(
        file_content_a,
        active_file_a,
        first_nonempty_files(files_a, files),
        true,
    );
    let model_b = mcp_parse_model(
        file_content_b,
        active_file_b,
        first_nonempty_files(files_b, files),
        true,
    );
    compare_models(&model_a, &model_b).to_json()
}

/// Python `files_a or files`: an empty map is missing and the fallback is used.
fn first_nonempty_files<'a>(
    preferred: Option<&'a HashMap<String, String>>,
    fallback: Option<&'a HashMap<String, String>>,
) -> Option<&'a HashMap<String, String>> {
    [preferred, fallback]
        .into_iter()
        .flatten()
        .find(|m| !m.is_empty())
}

fn mcp_parse_model(
    file_content: &str,
    active_file: Option<&str>,
    files: Option<&HashMap<String, String>>,
    synthesize_missing_active: bool,
) -> Model {
    let Some(files) = files.filter(|m| !m.is_empty()) else {
        return parse(file_content);
    };
    let (active, owned) = match active_file {
        Some(active) => (active.to_string(), None),
        None if synthesize_missing_active => {
            let mut map = files.clone();
            let mut key = "__mcp_compare__.mod".to_string();
            let mut n = 1u32;
            while map.contains_key(&key) {
                n += 1;
                key = format!("__mcp_compare_{n}__.mod");
            }
            map.insert(key.clone(), file_content.to_string());
            (key, Some(map))
        }
        None => return parse(file_content),
    };
    let files = owned.as_ref().unwrap_or(files);
    let mut ws = Workspace::new();
    for (name, content) in files {
        ws.update_document(name, content);
    }
    ws.update_document(&active, file_content);
    ws.get_effective_model(&active)
        .cloned()
        .unwrap_or_else(|| parse(file_content))
}

fn model_info_json(model: &Model) -> Value {
    let endogenous: Vec<String> = model
        .endogenous
        .iter()
        .map(|d| model.name(d.name).to_string())
        .collect();
    let exogenous: Vec<String> = model
        .exogenous
        .iter()
        .map(|d| model.name(d.name).to_string())
        .collect();
    let parameters: Vec<String> = model
        .parameters
        .iter()
        .map(|d| model.name(d.name).to_string())
        .collect();
    let timing = classify_variable_timing(model);
    let mut static_vars = Vec::new();
    let mut predetermined = Vec::new();
    let mut forward_looking = Vec::new();
    let mut mixed = Vec::new();
    for name in &endogenous {
        match timing.get(name).map(|t| t.class) {
            Some(TimingClass::Mixed) => mixed.push(name.clone()),
            Some(TimingClass::ForwardLooking) => forward_looking.push(name.clone()),
            Some(TimingClass::Predetermined) => predetermined.push(name.clone()),
            _ => static_vars.push(name.clone()),
        }
    }
    json!({
        "n_endogenous": endogenous.len(),
        "endogenous": endogenous,
        "n_exogenous": exogenous.len(),
        "exogenous": exogenous,
        "n_parameters": parameters.len(),
        "parameters": parameters,
        "n_equations": model
            .equations
            .iter()
            .filter(|eq| !eq.is_local && !eq.static_tag)
            .count(),
        "static": static_vars,
        "predetermined": predetermined,
        "forward_looking": forward_looking,
        "mixed": mixed,
        "n_static": static_vars.len(),
        "n_predetermined": predetermined.len(),
        "n_forward_looking": forward_looking.len(),
        "n_mixed": mixed.len(),
        "n_state_variables": predetermined.len() + mixed.len(),
        "n_jumpers": forward_looking.len() + mixed.len(),
    })
}

/// `parse(text).summary()`.
pub fn dynare_parse_summary(file_content: &str) -> ParseSummary {
    parse(file_content).summary()
}

/// `explain::render_markdown`, or the unknown-code string using Rust `known_codes()`.
pub fn dynare_explain(code: &str) -> String {
    match explain::render_markdown(code) {
        Some(rendered) => rendered,
        None => format!(
            "No documentation found for code '{code}'. Known codes: {}",
            explain::known_codes().join(", ")
        ),
    }
}

/// Sorted `{code, title}` for the 54 `known_codes()` keys.
pub fn dynare_list_diagnostic_codes() -> Vec<DiagnosticCodeItem> {
    explain::known_codes()
        .into_iter()
        .map(|code| DiagnosticCodeItem {
            code: code.to_string(),
            title: explain::explain(code)
                .map(|e| e.title.to_string())
                .unwrap_or_default(),
        })
        .collect()
}

/// `catalog::list_options` as JSON.
pub fn dynare_list_options(command: Option<&str>) -> Value {
    serde_json::to_value(list_options(command)).expect("list_options is serializable")
}

/// Whole-word Ident occurrences → 1-based `{line, column, end_column}`.
pub fn dynare_find_references(file_content: &str, symbol: &str) -> Vec<McpReference> {
    if symbol.is_empty() {
        return Vec::new();
    }
    let index = LineIndex::new(file_content);
    occurrences(file_content, symbol)
        .into_iter()
        .map(|span| {
            let start = index.position(file_content, span.start);
            let end = index.position(file_content, span.end);
            McpReference {
                line: start.line + 1,
                column: start.character + 1,
                end_column: end.character + 1,
            }
        })
        .collect()
}

/// Workspace find-references: active + include family; raw overlay text; caller keys.
pub fn dynare_find_references_workspace(
    active_file: &str,
    symbol: &str,
    files: &HashMap<String, String>,
) -> Vec<McpWorkspaceReference> {
    if symbol.is_empty() || !files.contains_key(active_file) {
        return Vec::new();
    }
    let mut ws = Workspace::new();
    for (name, content) in files {
        ws.update_document(name, content);
    }
    let scope = workspace_scope_files(active_file, files, &mut ws);
    let mut results = Vec::new();
    for (fname, content) in &scope {
        let text = ws.get_source(fname).unwrap_or(content.as_str());
        let index = LineIndex::new(text);
        for span in occurrences(text, symbol) {
            let start = index.position(text, span.start);
            let end = index.position(text, span.end);
            results.push(McpWorkspaceReference {
                file: fname.clone(),
                line: start.line + 1,
                column: start.character + 1,
                end_column: end.character + 1,
            });
        }
    }
    results
}

/// Rename Ident occurrences; illegal/reserved old or new → original (Python parity).
pub fn dynare_rename(file_content: &str, old_name: &str, new_name: &str) -> String {
    if !is_legal_ident(old_name) || !is_legal_ident(new_name) {
        return file_content.to_string();
    }
    rename_in_text(file_content, old_name, new_name)
}

/// Workspace rename: only changed scoped files, or `{}`.
pub fn dynare_rename_workspace(
    active_file: &str,
    old_name: &str,
    new_name: &str,
    files: &HashMap<String, String>,
) -> HashMap<String, String> {
    if !is_legal_ident(old_name) || !is_legal_ident(new_name) || !files.contains_key(active_file) {
        return HashMap::new();
    }
    let mut ws = Workspace::new();
    for (name, content) in files {
        ws.update_document(name, content);
    }
    let scope = workspace_scope_files(active_file, files, &mut ws);
    let mut out = HashMap::new();
    for (fname, content) in &scope {
        let text = ws.get_source(fname).unwrap_or(content.as_str());
        let rewritten = rename_in_text(text, old_name, new_name);
        if rewritten != text {
            out.insert(fname.clone(), rewritten);
        }
    }
    out
}

/// Thin wrap of library `auto_fix`.
pub fn dynare_auto_fix(file_content: &str) -> String {
    auto_fix(file_content)
}

/// MCP stdio entry. Wired from `dygnosis mcp`.
pub async fn run_stdio() {
    use rmcp::transport::stdio;
    let service = match DygnosisMcp.serve(stdio()).await {
        Ok(s) => s,
        Err(err) => {
            tracing::error!("MCP stdio failed: {err}");
            std::process::exit(1);
        }
    };
    if let Err(err) = service.waiting().await {
        tracing::error!("MCP stdio stopped: {err}");
        std::process::exit(1);
    }
}

/// Active + transitive includes + supplied parents that reach active.
fn workspace_scope_files(
    active_file: &str,
    files: &HashMap<String, String>,
    ws: &mut Workspace,
) -> HashMap<String, String> {
    let active_key = normalize_uri(active_file);
    let mut supplied_by_key: HashMap<String, String> = HashMap::new();
    for fname in files.keys() {
        supplied_by_key.insert(normalize_uri(fname), fname.clone());
    }

    let mut scope: HashMap<String, String> = HashMap::new();
    let mut seen: HashSet<String> = HashSet::new();

    let add_named = |fname: &str,
                     content: &str,
                     seen: &mut HashSet<String>,
                     scope: &mut HashMap<String, String>| {
        let key = normalize_uri(fname);
        if !seen.insert(key) {
            return;
        }
        scope.insert(fname.to_string(), content.to_string());
    };

    if let Some(content) = files.get(active_file) {
        add_named(active_file, content, &mut seen, &mut scope);
    }

    let active_include_keys: Vec<String> = ws
        .resolve_all_includes(active_file)
        .keys()
        .cloned()
        .collect();
    for path_key in active_include_keys {
        add_path_key(
            &path_key,
            files,
            &supplied_by_key,
            ws,
            &mut seen,
            &mut scope,
        );
    }

    let other_files: Vec<(String, String)> = files
        .iter()
        .filter(|(fname, _)| normalize_uri(fname) != active_key)
        .map(|(fname, content)| (fname.clone(), content.clone()))
        .collect();
    for (fname, content) in other_files {
        let included = ws.resolve_all_includes(&fname);
        if !included.contains_key(&active_key) {
            continue;
        }
        add_named(&fname, &content, &mut seen, &mut scope);
        let include_keys: Vec<String> = included.keys().cloned().collect();
        for path_key in include_keys {
            add_path_key(
                &path_key,
                files,
                &supplied_by_key,
                ws,
                &mut seen,
                &mut scope,
            );
        }
    }

    scope
}

fn add_path_key(
    path_key: &str,
    files: &HashMap<String, String>,
    supplied_by_key: &HashMap<String, String>,
    ws: &mut Workspace,
    seen: &mut HashSet<String>,
    scope: &mut HashMap<String, String>,
) {
    if let Some(orig) = supplied_by_key.get(path_key) {
        if let Some(content) = files.get(orig) {
            let key = normalize_uri(orig);
            if seen.insert(key) {
                scope.insert(orig.clone(), content.clone());
            }
            return;
        }
    }
    if let Some(src) = ws.get_source(path_key) {
        let key = normalize_uri(path_key);
        if seen.insert(key) {
            scope.insert(path_key.to_string(), src.to_string());
        }
    }
}

fn diagnostics_to_json(text: &str, diags: &[Diagnostic]) -> Vec<McpDiagnostic> {
    let index = LineIndex::new(text);
    diags
        .iter()
        .filter(|d| !is_dropped_code(&d.code))
        .map(|d| {
            let start = index.position(text, d.span.start);
            let end = index.position(text, d.span.end);
            McpDiagnostic {
                line: start.line + 1,
                column: start.character + 1,
                end_line: end.line + 1,
                end_column: end.character + 1,
                severity: mcp_severity(d.severity).to_string(),
                code: d.code.clone(),
                message: d.message.clone(),
            }
        })
        .collect()
}

fn mcp_severity(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "ERROR",
        Severity::Warning => "WARNING",
        Severity::Information => "INFORMATION",
        Severity::Hint => "HINT",
    }
}

fn is_dropped_code(code: &str) -> bool {
    OUT_CODES.contains(&code)
}

fn tool_json(value: Value) -> CallToolResult {
    CallToolResult::structured(value)
}

#[derive(Clone)]
struct DygnosisMcp;

#[derive(Debug, serde::Deserialize, JsonSchema)]
struct FileContentParams {
    file_content: String,
}

#[derive(Debug, serde::Deserialize, JsonSchema)]
struct RunPreprocessorParams {
    file_content: String,
    #[serde(default)]
    active_file: Option<String>,
    #[serde(default)]
    files: Option<HashMap<String, String>>,
}

#[derive(Debug, serde::Deserialize, JsonSchema)]
struct ModelInfoParams {
    file_content: String,
    #[serde(default)]
    active_file: Option<String>,
    #[serde(default)]
    files: Option<HashMap<String, String>>,
}

#[derive(Debug, serde::Deserialize, JsonSchema)]
struct CompareModelsParams {
    file_content_a: String,
    file_content_b: String,
    #[serde(default)]
    active_file_a: Option<String>,
    #[serde(default)]
    active_file_b: Option<String>,
    #[serde(default)]
    files_a: Option<HashMap<String, String>>,
    #[serde(default)]
    files_b: Option<HashMap<String, String>>,
    #[serde(default)]
    files: Option<HashMap<String, String>>,
}

#[derive(Debug, serde::Deserialize, JsonSchema)]
struct DiagnoseWorkspaceParams {
    active_file: String,
    files: HashMap<String, String>,
}

#[derive(Debug, serde::Deserialize, JsonSchema)]
struct ExplainParams {
    code: String,
}

#[derive(Debug, serde::Deserialize, JsonSchema)]
struct ListOptionsParams {
    #[serde(default)]
    command: Option<String>,
}

#[derive(Debug, serde::Deserialize, JsonSchema)]
struct FindReferencesParams {
    file_content: String,
    symbol: String,
}

#[derive(Debug, serde::Deserialize, JsonSchema)]
struct FindReferencesWorkspaceParams {
    active_file: String,
    symbol: String,
    files: HashMap<String, String>,
}

#[derive(Debug, serde::Deserialize, JsonSchema)]
struct RenameParams {
    file_content: String,
    old_name: String,
    new_name: String,
}

#[derive(Debug, serde::Deserialize, JsonSchema)]
struct RenameWorkspaceParams {
    active_file: String,
    old_name: String,
    new_name: String,
    files: HashMap<String, String>,
}

#[tool_router]
impl DygnosisMcp {
    #[tool(
        name = "dynare_diagnose",
        description = "Run diagnostics on a .mod file and return code, range, severity, and message."
    )]
    fn diagnose_tool(&self, Parameters(params): Parameters<FileContentParams>) -> CallToolResult {
        tool_json(
            serde_json::to_value(dynare_diagnose(&params.file_content)).expect("diagnose json"),
        )
    }

    #[tool(
        name = "dynare_diagnose_workspace",
        description = "Run diagnostics with in-memory @#include files. active_file must be a key in files."
    )]
    fn diagnose_workspace_tool(
        &self,
        Parameters(params): Parameters<DiagnoseWorkspaceParams>,
    ) -> CallToolResult {
        tool_json(
            serde_json::to_value(dynare_diagnose_workspace(
                &params.active_file,
                &params.files,
            ))
            .expect("diagnose_workspace json"),
        )
    }

    #[tool(
        name = "dynare_parse_summary",
        description = "Parse a .mod file and return a structured outline (names and counts)."
    )]
    fn parse_summary_tool(
        &self,
        Parameters(params): Parameters<FileContentParams>,
    ) -> CallToolResult {
        tool_json(
            serde_json::to_value(dynare_parse_summary(&params.file_content)).expect("summary json"),
        )
    }

    #[tool(
        name = "dynare_explain",
        description = "Return markdown documentation for a diagnostic code."
    )]
    fn explain_tool(&self, Parameters(params): Parameters<ExplainParams>) -> String {
        dynare_explain(&params.code)
    }

    #[tool(
        name = "dynare_list_diagnostic_codes",
        description = "List documented diagnostic codes and their titles."
    )]
    fn list_diagnostic_codes_tool(&self) -> CallToolResult {
        tool_json(serde_json::to_value(dynare_list_diagnostic_codes()).expect("list codes json"))
    }

    #[tool(
        name = "dynare_list_options",
        description = "List valid options for a Dynare command, or list known commands when omitted."
    )]
    fn list_options_tool(
        &self,
        Parameters(params): Parameters<ListOptionsParams>,
    ) -> CallToolResult {
        tool_json(dynare_list_options(params.command.as_deref()))
    }

    #[tool(
        name = "dynare_find_references",
        description = "Find every whole-word use of a name in a .mod file. Skips comments."
    )]
    fn find_references_tool(
        &self,
        Parameters(params): Parameters<FindReferencesParams>,
    ) -> CallToolResult {
        tool_json(
            serde_json::to_value(dynare_find_references(&params.file_content, &params.symbol))
                .expect("find_references json"),
        )
    }

    #[tool(
        name = "dynare_find_references_workspace",
        description = "Find every use of a name across an @#include graph. Unrelated files in the map are skipped."
    )]
    fn find_references_workspace_tool(
        &self,
        Parameters(params): Parameters<FindReferencesWorkspaceParams>,
    ) -> CallToolResult {
        tool_json(
            serde_json::to_value(dynare_find_references_workspace(
                &params.active_file,
                &params.symbol,
                &params.files,
            ))
            .expect("find_references_workspace json"),
        )
    }

    #[tool(
        name = "dynare_rename",
        description = "Rename a name throughout a .mod file. Skips comments. Returns the original text if the new name is not a legal identifier."
    )]
    fn rename_tool(&self, Parameters(params): Parameters<RenameParams>) -> String {
        dynare_rename(&params.file_content, &params.old_name, &params.new_name)
    }

    #[tool(
        name = "dynare_rename_workspace",
        description = "Rename a name across an @#include graph. Returns only files that changed, or an empty map if nothing applies."
    )]
    fn rename_workspace_tool(
        &self,
        Parameters(params): Parameters<RenameWorkspaceParams>,
    ) -> CallToolResult {
        tool_json(
            serde_json::to_value(dynare_rename_workspace(
                &params.active_file,
                &params.old_name,
                &params.new_name,
                &params.files,
            ))
            .expect("rename_workspace json"),
        )
    }

    #[tool(
        name = "dynare_auto_fix",
        description = "Apply stored diagnostic fixes to a .mod file. Leaves the text unchanged when macros would make the rewrite unsafe."
    )]
    fn auto_fix_tool(&self, Parameters(params): Parameters<FileContentParams>) -> String {
        dynare_auto_fix(&params.file_content)
    }

    #[tool(
        name = "dynare_run_preprocessor",
        description = "Run the local Dynare preprocessor in check mode and return its verdict, parsed diagnostics, and raw output."
    )]
    fn run_preprocessor_tool(
        &self,
        Parameters(params): Parameters<RunPreprocessorParams>,
    ) -> CallToolResult {
        tool_json(dynare_run_preprocessor(
            &params.file_content,
            params.active_file.as_deref(),
            params.files.as_ref(),
        ))
    }

    #[tool(
        name = "dynare_model_info",
        description = "Summarise a .mod file: names, counts, and equation timing (static, predetermined, forward-looking, mixed)."
    )]
    fn model_info_tool(&self, Parameters(params): Parameters<ModelInfoParams>) -> CallToolResult {
        tool_json(dynare_model_info(
            &params.file_content,
            params.active_file.as_deref(),
            params.files.as_ref(),
        ))
    }

    #[tool(
        name = "dynare_compare_models",
        description = "Compare two .mod files by names, calibrations, and equations."
    )]
    fn compare_models_tool(
        &self,
        Parameters(params): Parameters<CompareModelsParams>,
    ) -> CallToolResult {
        tool_json(dynare_compare_models(
            &params.file_content_a,
            &params.file_content_b,
            params.active_file_a.as_deref(),
            params.active_file_b.as_deref(),
            params.files_a.as_ref(),
            params.files_b.as_ref(),
            params.files.as_ref(),
        ))
    }
}

#[tool_handler(name = "dygnosis")]
impl ServerHandler for DygnosisMcp {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rmcp_stdio_registers_rust_tools() {
        let listed = DygnosisMcp::tool_router().list_all();
        let mut got: Vec<String> = listed.iter().map(|t| t.name.to_string()).collect();
        got.sort_unstable();
        let mut want: Vec<String> = registered_tool_names()
            .into_iter()
            .map(str::to_string)
            .collect();
        want.sort_unstable();
        assert_eq!(got, want);

        let blob = serde_json::to_string(&listed).expect("tools json");
        for phrase in [
            "Compute Steady State",
            "Gauss-Seidel",
            "trust-region",
            "homotopy",
            "python_dynare_lsp",
            "dynare_compute_steady_state",
            "dynare_run_dynare",
        ] {
            assert!(
                !blob.contains(phrase),
                "stdio tools/list must not contain {phrase}: {blob}"
            );
        }

        let info = DygnosisMcp.get_info();
        assert_eq!(info.server_info.name, "dygnosis");
    }
}
