//! MCP tool handlers (waves a–c) and stdio transport.
//!
//! Tests call these functions directly. Stdio is a thin wrap around the same
//! handlers.

use std::collections::{HashMap, HashSet};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{tool, tool_handler, tool_router, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
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
    find_preprocessor, maybe_run_and_reconcile, reconcile_diagnostics, run_workspace_preprocessor,
    DEFAULT_TIMEOUT,
};
use crate::refs::{is_legal_ident, occurrences, rename_in_text};
use crate::span::LineIndex;
use crate::workspace::Workspace;

const OUT_CODES: &[&str] = &[
    "E040", "W040", "W041", "I041", "W071", "I070", "I071", "W080", "W081", "DYNR",
];

const TOOLS: &[(&str, &str)] = &[
    (
        "dynare_diagnose",
        "Run diagnostics on a .mod file and return code, range, severity, and message.",
    ),
    (
        "dynare_model_info",
        "Summarise a .mod file: names, counts, timing, and block flags.",
    ),
    (
        "dynare_compare_models",
        "Compare two .mod files by names, calibrations, and equations.",
    ),
    (
        "dynare_find_references",
        "Find every whole-word use of a name. Skips comments.",
    ),
    (
        "dynare_rename",
        "Rename a name. Skips comments. Without a files map, returns the rewritten text (or the original if the new name is not a legal identifier). With a map, returns only files that changed.",
    ),
    (
        "dynare_auto_fix",
        "Apply stored diagnostic fixes to a .mod file. Leaves the text unchanged when macros would make the rewrite unsafe.",
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

/// One hit from `dynare_find_references` without a files map (1-based; no end_line, no file).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpReference {
    pub line: u32,
    pub column: u32,
    pub end_column: u32,
}

/// One hit from `dynare_find_references` with a files map (caller-supplied `file` key).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
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

/// Empty `files` is missing: single-file path.
fn nonempty_map(files: Option<&HashMap<String, String>>) -> Option<&HashMap<String, String>> {
    files.filter(|m| !m.is_empty())
}

/// Clone `files` and set `files[active_file] = file_content` (overlay).
fn overlay_files(
    file_content: &str,
    active_file: &str,
    files: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut workspace_files = files.clone();
    workspace_files.insert(active_file.to_string(), file_content.to_string());
    workspace_files
}

/// `analyze` / `check_in_workspace`, then reconcile when the binary is found.
///
/// No map: `file_content` is the source. With a nonempty map: `active_file` must
/// be a key in `files` or the result is `[]`; `file_content` overwrites that key.
pub fn dynare_diagnose(
    file_content: &str,
    active_file: Option<&str>,
    files: Option<&HashMap<String, String>>,
) -> Vec<McpDiagnostic> {
    let Some(files) = nonempty_map(files) else {
        let own = analyze(&parse(file_content));
        let diags = maybe_run_and_reconcile(own, file_content, None, None);
        return diagnostics_to_json(file_content, &diags);
    };
    let Some(active) = active_file.filter(|a| files.contains_key(*a)) else {
        return Vec::new();
    };
    let workspace_files = overlay_files(file_content, active, files);
    diagnose_in_workspace(active, &workspace_files)
}

fn diagnose_in_workspace(active_file: &str, files: &HashMap<String, String>) -> Vec<McpDiagnostic> {
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

/// Timing lists and counts from the equation AST, plus ParseSummary flags.
/// No `blocks` key.
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
    let summary = model.summary();
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
        "n_model_equations": summary.n_model_equations,
        "n_steady_state_equations": summary.n_steady_state_equations,
        "n_initval_entries": summary.n_initval_entries,
        "is_linear": summary.is_linear,
        "has_model_block": summary.has_model_block,
        "has_steady_state_model_block": summary.has_steady_state_model_block,
        "has_initval_block": summary.has_initval_block,
        "has_shocks_block": summary.has_shocks_block,
    })
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

/// Sorted `{code, title}` for `known_codes()` keys.
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

/// Whole-word Ident occurrences.
///
/// No map: `[{ line, column, end_column }]`. With a nonempty map: objects also
/// have `file` (caller key). Missing `active_file` or empty symbol → `[]`.
pub fn dynare_find_references(
    file_content: &str,
    symbol: &str,
    active_file: Option<&str>,
    files: Option<&HashMap<String, String>>,
) -> Value {
    let Some(files) = nonempty_map(files) else {
        return serde_json::to_value(find_references_in_text(file_content, symbol))
            .expect("find_references json");
    };
    let Some(active) = active_file.filter(|a| files.contains_key(*a)) else {
        return json!([]);
    };
    let workspace_files = overlay_files(file_content, active, files);
    serde_json::to_value(find_references_in_workspace(
        active,
        symbol,
        &workspace_files,
    ))
    .expect("find_references json")
}

fn find_references_in_text(file_content: &str, symbol: &str) -> Vec<McpReference> {
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

fn find_references_in_workspace(
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

/// Rename Ident occurrences.
///
/// No map: JSON string (illegal / reserved / no hits → original). With a
/// nonempty map: object of changed files only, or `{}`.
pub fn dynare_rename(
    file_content: &str,
    old_name: &str,
    new_name: &str,
    active_file: Option<&str>,
    files: Option<&HashMap<String, String>>,
) -> Value {
    let Some(files) = nonempty_map(files) else {
        return Value::String(rename_in_single(file_content, old_name, new_name));
    };
    let Some(active) = active_file.filter(|a| files.contains_key(*a)) else {
        return json!({});
    };
    let workspace_files = overlay_files(file_content, active, files);
    serde_json::to_value(rename_in_workspace(
        active,
        old_name,
        new_name,
        &workspace_files,
    ))
    .expect("rename json")
}

fn rename_in_single(file_content: &str, old_name: &str, new_name: &str) -> String {
    if !is_legal_ident(old_name) || !is_legal_ident(new_name) {
        return file_content.to_string();
    }
    rename_in_text(file_content, old_name, new_name)
}

fn rename_in_workspace(
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

fn tool_text(text: String) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(text)])
}

/// Map args for diagnose / model_info: optional `file_content` overlay.
#[derive(Debug, Deserialize, JsonSchema)]
struct IncludeMapParams {
    #[serde(default)]
    file_content: Option<String>,
    #[serde(default)]
    active_file: Option<String>,
    #[serde(default)]
    files: Option<HashMap<String, String>>,
}

#[derive(Clone)]
struct DygnosisMcp;

#[derive(Debug, Deserialize, JsonSchema)]
struct FileContentParams {
    file_content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
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

#[derive(Debug, Deserialize, JsonSchema)]
struct ExplainParams {
    code: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListOptionsParams {
    #[serde(default)]
    command: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct FindReferencesParams {
    #[serde(default)]
    file_content: Option<String>,
    symbol: String,
    #[serde(default)]
    active_file: Option<String>,
    #[serde(default)]
    files: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RenameParams {
    #[serde(default)]
    file_content: Option<String>,
    old_name: String,
    new_name: String,
    #[serde(default)]
    active_file: Option<String>,
    #[serde(default)]
    files: Option<HashMap<String, String>>,
}

/// Source text plus map args after overlay.
struct MappedSource<'a> {
    content: &'a str,
    active: Option<&'a str>,
    files: Option<&'a HashMap<String, String>>,
}

/// `None` = missing `file_content` on the single-file path, or nonempty map
/// without a usable `active_file`.
fn resolve_mapped<'a>(
    file_content: Option<&'a str>,
    active_file: Option<&'a str>,
    files: Option<&'a HashMap<String, String>>,
) -> Option<MappedSource<'a>> {
    match nonempty_map(files) {
        None => Some(MappedSource {
            content: file_content?,
            active: None,
            files: None,
        }),
        Some(files) => {
            let active = active_file.filter(|a| files.contains_key(*a))?;
            let content = file_content.unwrap_or_else(|| files[active].as_str());
            Some(MappedSource {
                content,
                active: Some(active),
                files: Some(files),
            })
        }
    }
}

#[tool_router]
impl DygnosisMcp {
    #[tool(
        name = "dynare_diagnose",
        description = "Run diagnostics on a .mod file and return code, range, severity, and message."
    )]
    fn diagnose_tool(&self, Parameters(params): Parameters<IncludeMapParams>) -> CallToolResult {
        let diags = match resolve_mapped(
            params.file_content.as_deref(),
            params.active_file.as_deref(),
            params.files.as_ref(),
        ) {
            Some(src) => dynare_diagnose(src.content, src.active, src.files),
            None => Vec::new(),
        };
        tool_json(serde_json::to_value(diags).expect("diagnose json"))
    }

    #[tool(
        name = "dynare_model_info",
        description = "Summarise a .mod file: names, counts, timing, and block flags."
    )]
    fn model_info_tool(&self, Parameters(params): Parameters<IncludeMapParams>) -> CallToolResult {
        let info = match nonempty_map(params.files.as_ref()) {
            None => dynare_model_info(params.file_content.as_deref().unwrap_or(""), None, None),
            Some(files) => match params
                .active_file
                .as_deref()
                .filter(|a| files.contains_key(*a))
            {
                Some(active) => {
                    let content = params
                        .file_content
                        .as_deref()
                        .unwrap_or_else(|| files[active].as_str());
                    dynare_model_info(content, Some(active), Some(files))
                }
                None => dynare_model_info(
                    params.file_content.as_deref().unwrap_or(""),
                    None,
                    Some(files),
                ),
            },
        };
        tool_json(info)
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

    #[tool(
        name = "dynare_find_references",
        description = "Find every whole-word use of a name. Skips comments."
    )]
    fn find_references_tool(
        &self,
        Parameters(params): Parameters<FindReferencesParams>,
    ) -> CallToolResult {
        let value = match resolve_mapped(
            params.file_content.as_deref(),
            params.active_file.as_deref(),
            params.files.as_ref(),
        ) {
            Some(src) => dynare_find_references(src.content, &params.symbol, src.active, src.files),
            None => json!([]),
        };
        tool_json(value)
    }

    #[tool(
        name = "dynare_rename",
        description = "Rename a name. Skips comments. Without a files map, returns the rewritten text (or the original if the new name is not a legal identifier). With a map, returns only files that changed."
    )]
    fn rename_tool(&self, Parameters(params): Parameters<RenameParams>) -> CallToolResult {
        match resolve_mapped(
            params.file_content.as_deref(),
            params.active_file.as_deref(),
            params.files.as_ref(),
        ) {
            Some(src) => {
                let result = dynare_rename(
                    src.content,
                    &params.old_name,
                    &params.new_name,
                    src.active,
                    src.files,
                );
                match result {
                    Value::String(text) => tool_text(text),
                    other => tool_json(other),
                }
            }
            None if nonempty_map(params.files.as_ref()).is_some() => tool_json(json!({})),
            None => tool_text(String::new()),
        }
    }

    #[tool(
        name = "dynare_auto_fix",
        description = "Apply stored diagnostic fixes to a .mod file. Leaves the text unchanged when macros would make the rewrite unsafe."
    )]
    fn auto_fix_tool(&self, Parameters(params): Parameters<FileContentParams>) -> String {
        dynare_auto_fix(&params.file_content)
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
