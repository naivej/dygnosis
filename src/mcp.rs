//! MCP tool handlers (waves a–c) and stdio transport.
//!
//! Tests call these functions directly. Stdio is a thin wrap around the same
//! handlers.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{tool, tool_handler, tool_router, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auto_fix::auto_fix;
use crate::catalog::list_options;
use crate::diagnostic::{analyze, Diagnostic, Severity};
use crate::equations::{
    count_gap, equations, explain_equation, heterogeneous_equations, CountGap, EquationRow,
};
use crate::expand::{expand_report, EquationOrigin, ExpandReport, OriginFrame};
use crate::explain;
use crate::extract::{
    extract, EquationDomain, EquationRole, ExtractError, ExtractRequest, ExtractResult,
    ExtractStatus,
};
use crate::format::{format_outcome, parse_format_indent, FormatOutcome};
use crate::include_resolver::{normalize_uri, path_key};
use crate::model::Model;
use crate::model_diff::{compare_models_with_sources, CompareSource};
use crate::model_info::{heterogeneous_dimension_names, model_info_json, related_files_json};
use crate::parser::{normalize_newlines, parse};
use crate::refs::{is_legal_ident, occurrences, rename_in_text};
use crate::repository_compare::{compare_repository, RepositoryComparison, RepositorySelector};
use crate::span::{LineIndex, Span};
use crate::workspace::Workspace;
use crate::workspace_diagnose::{
    diagnose_map, diagnose_paths, RootStatus, WorkspaceDiagnoseError, WorkspaceDiagnoseReport,
    WorkspaceDiagnostic, WorkspaceRootReport,
};

const OUT_CODES: &[&str] = &[
    "E040", "W040", "W041", "I041", "W071", "I070", "I071", "W080", "W081", "DYNR",
];

const TOOL_NAMES: &[&str] = &[
    "dynare_diagnose",
    "dynare_model_info",
    "dynare_compare_models",
    "dynare_find_references",
    "dynare_rename",
    "dynare_auto_fix",
    "dynare_explain",
    "dynare_list_diagnostic_codes",
    "dynare_list_options",
    "dynare_equations",
    "dynare_related_files",
    "dynare_expand",
    "dynare_format",
    "dynare_extract",
    "dynare_workspace_diagnose",
];

/// One diagnostic as MCP JSON: 1-based line/column, severity `ERROR`/`WARNING`/`INFORMATION`/`HINT`.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct McpDiagnostic {
    /// Present for a writing summary anchored in an included file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub severity: String,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<Value>,
}

/// One row from `dynare_list_diagnostic_codes`.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct DiagnosticCodeItem {
    pub code: String,
    pub title: String,
    /// `shared`, `skipped`, or `added` relative to Dynare.
    pub kind: String,
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

const MACRO_INCOMPLETE_MESSAGE: &str = "Macro expansion is incomplete";

/// Shared by MCP and the LSP commands so both transports report the same status.
pub(crate) fn macro_incomplete_status() -> Value {
    json!({"status": "incomplete", "message": MACRO_INCOMPLETE_MESSAGE})
}

pub(crate) fn incomplete_model_status(model: &Model, includes_complete: bool) -> Option<Value> {
    if !includes_complete {
        Some(crate::model_info::model_incomplete_status())
    } else if model.macro_incomplete() {
        Some(macro_incomplete_status())
    } else if !crate::model_map::parser_complete(model) {
        Some(crate::model_info::model_incomplete_status())
    } else {
        None
    }
}

/// Registered tool names in registration order.
pub fn registered_tool_names() -> Vec<&'static str> {
    TOOL_NAMES.to_vec()
}

/// JSON for tools/list assertions (names + descriptions). No live client.
pub fn tools_list_json() -> Value {
    let tools = DygnosisMcp::tool_router().list_all();
    json!({"tools": TOOL_NAMES.iter().map(|name| {
        let tool = tools.iter().find(|tool| tool.name == *name).expect("registered tool");
        json!({"name": tool.name, "description": tool.description})
    }).collect::<Vec<_>>()})
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

/// `analyze` / `check_in_workspace` only. Product MCP does not spawn Dynare.
///
/// No map: `file_content` is the source. With a nonempty map: `active_file` must
/// be a key in `files` or the result is `[]`; `file_content` overwrites that key.
pub fn dynare_diagnose(
    file_content: &str,
    active_file: Option<&str>,
    files: Option<&HashMap<String, String>>,
) -> Vec<McpDiagnostic> {
    let Some(files) = nonempty_map(files) else {
        let model = parse(file_content);
        let mut diags = analyze(&model);
        crate::diagnostic_links::map_free_text(file_content, &mut diags);
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
    let own = crate::diagnostic::check_in_workspace_with_origins(&mut ws, active_file);
    diagnostics_to_json_with_origins(text, &own, files)
}

/// Aggregate timing lists and counts, per-dimension heterogeneous summaries,
/// and ParseSummary flags.
/// No `blocks` key.
pub fn dynare_model_info(
    file_content: &str,
    active_file: Option<&str>,
    files: Option<&HashMap<String, String>>,
) -> Value {
    let (model, includes_complete) =
        if let (Some(files), Some(active)) = (nonempty_map(files), active_file) {
            let mut workspace = Workspace::new();
            for (name, content) in files {
                workspace.update_document(name, content);
            }
            workspace.update_document(active, file_content);
            let complete = workspace.includes_complete(active);
            let model = workspace
                .get_effective_model(active)
                .cloned()
                .unwrap_or_else(|| parse(file_content));
            (model, complete)
        } else {
            (
                parse(file_content),
                crate::macro_expand::required_includes_complete(file_content),
            )
        };
    if let Some(status) = incomplete_model_status(&model, includes_complete) {
        return status;
    }
    model_info_json(&model)
}

/// Counted aggregate and heterogeneous equations, with the aggregate count
/// gap. `name` searches both kinds; `index` selects an aggregate row. Both
/// filters attach per-row explain markdown. Include map: same as
/// `dynare_model_info` (`synthesize_missing_active = false`).
pub fn dynare_equations(
    file_content: &str,
    active_file: Option<&str>,
    files: Option<&HashMap<String, String>>,
    name: Option<&str>,
    index: Option<usize>,
) -> Value {
    let unit = mcp_unit(file_content, active_file, files);
    if let Some(mut status) = incomplete_model_status(&unit.model, unit.includes_complete) {
        status["equations"] = json!([]);
        status["count_gap"] = Value::Null;
        return status;
    }
    let rows = equations(&unit.model);
    let heterogeneous = heterogeneous_equations(&unit.model);
    let gap = count_gap(&unit.model);
    let mut out = json!({
        "equations": [],
        "count_gap": count_gap_json(&gap),
    });
    match (name, index) {
        (Some(_), Some(_)) => {
            out["message"] = json!("name and index must not both be set");
        }
        (None, None) => {
            out["equations"] = Value::Array(
                rows.iter()
                    .map(|row| equation_row_json(row, origin_for(&unit, row.index), &unit, false))
                    .collect(),
            );
        }
        (Some(want), None) => {
            let hits: Vec<&EquationRow> = rows.iter().filter(|row| row.name == want).collect();
            let heterogeneous_hit = heterogeneous
                .iter()
                .any(|block| block.equations.iter().any(|row| row.name == want));
            if hits.is_empty() && !heterogeneous_hit {
                out["message"] = json!(format!("no equation named '{want}'"));
            } else {
                out["equations"] = Value::Array(
                    hits.into_iter()
                        .map(|row| {
                            equation_row_json(row, origin_for(&unit, row.index), &unit, true)
                        })
                        .collect(),
                );
            }
        }
        (None, Some(i)) => {
            if i >= rows.len() {
                out["message"] = json!(format!("index {i} is out of range (0..{})", rows.len()));
            } else {
                out["equations"] = json!([equation_row_json(
                    &rows[i],
                    origin_for(&unit, rows[i].index),
                    &unit,
                    true
                )]);
            }
        }
    }
    if !heterogeneous.is_empty() && index.is_none() {
        out["heterogeneous_equations"] = Value::Array(
            heterogeneous
                .iter()
                .filter_map(|block| {
                    let selected: Vec<Value> = block
                        .equations
                        .iter()
                        .enumerate()
                        .filter(|(_, row)| match (name, index) {
                            (None, None) => true,
                            (Some(want), None) => row.name == want,
                            _ => false,
                        })
                        .map(|(local_index, row)| {
                            let origin = unit
                                .report
                                .heterogeneous_origins
                                .get(block.block_index)
                                .and_then(|origins| origins.get(local_index));
                            equation_row_json(row, origin, &unit, name.is_some())
                        })
                        .collect();
                    if name.is_some() && selected.is_empty() {
                        return None;
                    }
                    Some(json!({
                        "block_index": block.block_index,
                        "dimension": block.dimension,
                        "equations": selected,
                    }))
                })
                .collect(),
        );
    }
    out
}

/// Compilation unit after include splice and macro expand, plus origin jumps.
///
/// Include map: same as [`dynare_model_info`] (`synthesize_missing_active = false`).
/// No preprocessor. If a mapped `Workspace::expand_report` is `None`, returns
/// empty `effective_text` / zero equations / empty `origins`.
pub fn dynare_expand(
    file_content: &str,
    active_file: Option<&str>,
    files: Option<&HashMap<String, String>>,
) -> Value {
    let unit = mcp_unit(file_content, active_file, files);
    let has_heterogeneous = !unit.report.heterogeneous_origins.is_empty();
    let origins: Vec<Value> = unit
        .report
        .origins
        .iter()
        .map(|origin| {
            let mut v = json!({ "index": origin.index });
            if has_heterogeneous {
                v["scope_index"] = json!(origin.scope_index);
                if let Some(dimension) = &origin.dimension {
                    v["scope"] = json!("heterogeneous");
                    v["dimension"] = json!(dimension);
                    v["block_index"] = json!(origin.block_index);
                } else {
                    v["scope"] = json!("aggregate");
                }
            }
            attach_origin(&mut v, origin, &unit);
            v
        })
        .collect();
    let mut result = json!({
        "effective_text": unit.report.effective_text,
        "n_equations": unit.report.n_equations,
        "origins": origins,
        "navigation_schema_version": crate::preview_navigation::NAVIGATION_SCHEMA_VERSION,
        "root_file": unit.root_file,
        "revision": unit.revision,
        "complete": unit.complete,
        "macro_messages": macro_messages_json(&unit.report.macro_messages, &unit),
    });
    result["navigation"] = if unit.complete {
        crate::preview_navigation::navigation_json(
            &unit.report,
            |span| range_json(span, &unit.report.effective_text),
            |segment| {
                let text = unit.source_for(segment.file.as_deref())?;
                if segment.span.is_empty() {
                    return None;
                }
                text.get(segment.span.start as usize..segment.span.end as usize)?;
                let normalized = normalize_newlines(text);
                let mut location = json!({"range":range_json(segment.span, &normalized)});
                if let Some(file) = unit.map_uri(segment.file.as_deref()) {
                    location["file"] = json!(file);
                } else if segment.file.is_some() {
                    return None;
                }
                Some(location)
            },
        )
    } else {
        json!([])
    };
    if !unit.complete {
        result["status"] = json!("incomplete");
    }
    // A missing include is not spliced. The root equations that follow it are
    // not a finished expansion, so the count stays zero.
    if !unit.includes_complete {
        result["n_equations"] = json!(0);
    }
    if has_heterogeneous {
        result["n_aggregate_equations"] = json!(unit.report.aggregate_origins.len());
        result["n_heterogeneous_equations"] =
            json!(unit.report.n_equations - unit.report.aggregate_origins.len());
        result["heterogeneity_dimensions"] = Value::Array(
            heterogeneous_dimension_names(&unit.model)
                .into_iter()
                .map(|dimension| {
                    let name = unit.model.name(dimension);
                    let count = unit
                        .report
                        .origins
                        .iter()
                        .filter(|origin| origin.dimension.as_deref() == Some(name))
                        .count();
                    json!({ "dimension": name, "n_equations": count })
                })
                .collect(),
        );
    }
    result
}

fn count_gap_json(gap: &CountGap) -> Value {
    json!({
        "n_endogenous": gap.n_endogenous,
        "n_equations": gap.n_equations,
        "delta": gap.delta,
        "unreferenced_endogenous": gap.unreferenced_endogenous,
        "expected_delta": gap.expected_delta,
    })
}

fn equation_row_json(
    row: &EquationRow,
    origin: Option<&EquationOrigin>,
    unit: &McpUnit,
    with_explain: bool,
) -> Value {
    let idents: Vec<Value> = row
        .idents
        .iter()
        .map(|id| {
            let mut v = json!({
                "name": id.name,
                "timing": id.timing,
                "dynare_timing": id.dynare_timing,
                "class": id.class.as_str(),
            });
            if let Some(tc) = id.timing_class {
                v["timing_class"] = json!(tc.label());
            }
            v
        })
        .collect();
    let mut v = json!({
        "index": row.index,
        "name": row.name,
        "text": row.text,
        "static_tag": row.static_tag,
        "dynamic_tag": row.dynamic_tag,
        "idents": idents,
    });
    v["tags"] = json!(row.tags);
    if let Some(comp) = &row.complementarity {
        v["complementarity"] = json!({
            "text": comp.text,
            "matched": match &comp.matched {
                Some(m) => json!({
                    "variable": m.variable,
                    "lower_bound": m.lower_bound,
                    "upper_bound": m.upper_bound,
                }),
                None => json!(null),
            },
        });
    }
    if let Some(origin) = origin {
        attach_origin(&mut v, origin, unit);
    }
    if with_explain {
        v["explain"] = json!(explain_equation(row));
    }
    v
}

struct McpUnit {
    model: Model,
    report: ExpandReport,
    raw: String,
    files: Option<HashMap<String, String>>,
    sources: HashMap<String, String>,
    root_file: Option<String>,
    revision: String,
    complete: bool,
    includes_complete: bool,
}

fn mcp_unit(
    file_content: &str,
    active_file: Option<&str>,
    files: Option<&HashMap<String, String>>,
) -> McpUnit {
    let Some(files) = nonempty_map(files) else {
        return McpUnit::free(file_content);
    };
    let Some(active) = active_file.filter(|a| files.contains_key(*a)) else {
        return McpUnit::free(file_content);
    };
    McpUnit::mapped(file_content, active, files)
}

impl McpUnit {
    fn free(file_content: &str) -> Self {
        use std::hash::{Hash, Hasher};
        let report = expand_report(file_content);
        let includes_complete = crate::macro_expand::required_includes_complete(file_content);
        let complete = report.navigation_complete && includes_complete;
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        file_content.hash(&mut hash);
        Self {
            model: parse(file_content),
            report,
            raw: normalize_newlines(file_content),
            files: None,
            sources: HashMap::new(),
            root_file: None,
            revision: format!("{:016x}", hash.finish()),
            complete,
            includes_complete,
        }
    }

    fn mapped(file_content: &str, active: &str, files: &HashMap<String, String>) -> Self {
        let workspace_files = overlay_files(file_content, active, files);
        let mut ws = Workspace::new();
        for (name, content) in &workspace_files {
            ws.update_document(name, content);
        }
        let revision = ws.input_revision(active).unwrap_or_default();
        let model = ws
            .get_effective_model(active)
            .cloned()
            .unwrap_or_else(|| parse(file_content));
        let report = ws
            .expand_report(active)
            .cloned()
            .unwrap_or_else(|| ExpandReport {
                model_map: Default::default(),
                complete: false,
                navigation_complete: false,
                effective_text: String::new(),
                navigation: Vec::new(),
                n_equations: 0,
                origins: Vec::new(),
                aggregate_origins: Vec::new(),
                heterogeneous_origins: Vec::new(),
                aggregate_row_origins: Vec::new(),
                heterogeneous_row_origins: Vec::new(),
                macro_messages: Vec::new(),
            });
        let mut sources = HashMap::new();
        let includes_complete = ws.includes_complete(active);
        let complete =
            report.navigation_complete && includes_complete && ws.input_snapshot_is_current(active);
        for uri in ws.document_uris() {
            if let Some(src) = ws.get_source(&uri) {
                sources.insert(uri, src.to_string());
            }
        }
        Self {
            model,
            report,
            raw: normalize_newlines(file_content),
            files: Some(workspace_files),
            sources,
            root_file: Some(active.to_string()),
            revision,
            complete,
            includes_complete,
        }
    }

    fn source_for(&self, origin_uri: Option<&str>) -> Option<&str> {
        match origin_uri {
            None => Some(self.raw.as_str()),
            Some(uri) => {
                if let Some(src) = self.sources.get(uri) {
                    return Some(src.as_str());
                }
                let key = path_key(Path::new(uri));
                if let Some(src) = self.sources.get(&key) {
                    return Some(src.as_str());
                }
                let files = self.files.as_ref()?;
                files
                    .iter()
                    .find(|(name, _)| normalize_uri(name) == key)
                    .map(|(_, content)| content.as_str())
            }
        }
    }

    fn map_uri(&self, origin_uri: Option<&str>) -> Option<String> {
        let uri = origin_uri?;
        let files = self.files.as_ref()?;
        Some(related_file_path(Path::new(uri), files))
    }
}

fn origin_for(unit: &McpUnit, index: usize) -> Option<&EquationOrigin> {
    unit.report
        .aggregate_origins
        .get(index)
        .filter(|origin| origin.scope_index == index)
}

fn attach_origin(target: &mut Value, origin: &EquationOrigin, unit: &McpUnit) {
    let Some(src) = unit.source_for(origin.origin_uri.as_deref()) else {
        return;
    };
    target["origin"] = range_json(origin.origin_span, src);
    if let Some(uri) = unit.map_uri(origin.origin_uri.as_deref()) {
        target["origin_uri"] = json!(uri);
    }
    if !publish_origin_frames(&origin.origin_frames) {
        return;
    }
    let mut frames = Vec::new();
    for frame in &origin.origin_frames {
        if let Some(v) = origin_frame_json(frame, unit) {
            frames.push(v);
        }
    }
    if publish_origin_frames_json(&frames) {
        target["origin_frames"] = Value::Array(frames);
    }
}

fn publish_origin_frames(frames: &[crate::expand::OriginFrame]) -> bool {
    frames.len() > 1 || frames.iter().any(|frame| frame.kind == "for")
}

fn publish_origin_frames_json(frames: &[Value]) -> bool {
    frames.len() > 1
        || frames
            .iter()
            .any(|frame| frame.get("kind").and_then(Value::as_str) == Some("for"))
}

fn origin_frame_json(frame: &OriginFrame, unit: &McpUnit) -> Option<Value> {
    let src = unit.source_for(frame.origin_uri.as_deref())?;
    let mut v = range_json(frame.origin_span, src);
    v["kind"] = json!(frame.kind);
    if let Some(variable) = &frame.variable {
        v["variable"] = json!(variable);
    }
    if let Some(value) = &frame.value {
        v["value"] = json!(value);
    }
    if let Some(uri) = unit.map_uri(frame.origin_uri.as_deref()) {
        v["origin_uri"] = json!(uri);
    }
    Some(v)
}

fn macro_messages_json(messages: &[crate::macro_expand::MacroMessage], unit: &McpUnit) -> Value {
    json!(messages
        .iter()
        .map(|message| {
            let text = message
                .file
                .as_deref()
                .and_then(|file| unit.sources.get(file).map(String::as_str))
                .unwrap_or(unit.raw.as_str());
            let mut row = json!({
                "kind": message.kind,
                "message": message.message,
                "location": range_json(message.span, text),
            });
            if let Some(file) = &message.file {
                row["file"] = json!(file);
            }
            row
        })
        .collect::<Vec<_>>())
}

fn range_json(span: Span, text: &str) -> Value {
    let index = LineIndex::new(text);
    let start = index.position(text, span.start);
    let end = index.position(text, span.end);
    json!({
        "line": start.line + 1,
        "column": start.character + 1,
        "end_line": end.line + 1,
        "end_column": end.character + 1,
    })
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
    let (model_a, mut workspace_a, root_a, revision_a) = mcp_compare_input(
        file_content_a,
        active_file_a,
        first_nonempty_files(files_a, files),
    );
    let (model_b, mut workspace_b, root_b, revision_b) = mcp_compare_input(
        file_content_b,
        active_file_b,
        first_nonempty_files(files_b, files),
    );
    if let Some(status) = incomplete_model_status(&model_a, workspace_a.includes_complete(&root_a))
        .or_else(|| incomplete_model_status(&model_b, workspace_b.includes_complete(&root_b)))
    {
        return status;
    }
    let diff = compare_models_with_sources(
        &model_a,
        &model_b,
        Some(CompareSource {
            text: file_content_a,
            origin_uri: active_file_a,
        }),
        Some(CompareSource {
            text: file_content_b,
            origin_uri: active_file_b,
        }),
    );
    let before = crate::compare_navigation::ComparisonInput::capture(
        &mut workspace_a,
        &root_a,
        active_file_a,
        revision_a,
        &model_a,
        diff.shock_setup_changes
            .iter()
            .map(|change| change.before.as_ref()),
    )
    .with_file_names(
        first_nonempty_files(files_a, files)
            .into_iter()
            .flat_map(|map| map.keys()),
    );
    let after = crate::compare_navigation::ComparisonInput::capture(
        &mut workspace_b,
        &root_b,
        active_file_b,
        revision_b,
        &model_b,
        diff.shock_setup_changes
            .iter()
            .map(|change| change.after.as_ref()),
    )
    .with_file_names(
        first_nonempty_files(files_b, files)
            .into_iter()
            .flat_map(|map| map.keys()),
    );
    if !workspace_a.input_snapshot_is_current(&root_a)
        || !workspace_b.input_snapshot_is_current(&root_b)
    {
        return json!({"error": "Comparison inputs changed while reading them; refresh the comparison", "code": "INPUT_CHANGED"});
    }
    let mut result = diff.to_json();
    result["navigation"] = crate::compare_navigation::navigation_json(
        &diff,
        &before,
        &after,
        crate::compare_navigation::Coordinates::Mcp,
    );
    result
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

fn mcp_compare_input(
    file_content: &str,
    active_file: Option<&str>,
    files: Option<&HashMap<String, String>>,
) -> (Model, Workspace, String, Option<String>) {
    let Some(files) = files.filter(|m| !m.is_empty()) else {
        // Preserve the historical no-map parse, with no host include lookup.
        let root = "mcp-compare:root".to_string();
        let mut ws = Workspace::new();
        ws.set_root_search_paths(&root, Vec::new());
        ws.update_document(&root, file_content);
        let revision = ws.input_revision(&root);
        return (parse(file_content), ws, root, revision);
    };
    let (active, owned) = match active_file {
        Some(active) => (active.to_string(), None),
        None => {
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
    };
    let files = owned.as_ref().unwrap_or(files);
    let mut ws = Workspace::new();
    for (name, content) in files {
        ws.update_document(name, content);
    }
    ws.update_document(&active, file_content);
    let revision = ws.input_revision(&active);
    let model = ws
        .get_effective_model(&active)
        .cloned()
        .unwrap_or_else(|| parse(file_content));
    (model, ws, active, revision)
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

/// Sorted `{code, title, kind}` for `known_codes()` keys.
pub fn dynare_list_diagnostic_codes() -> Vec<DiagnosticCodeItem> {
    explain::known_codes()
        .into_iter()
        .map(|code| {
            let entry = explain::explain(code);
            DiagnosticCodeItem {
                code: code.to_string(),
                title: entry.map(|e| e.title.to_string()).unwrap_or_default(),
                kind: entry
                    .map(|e| e.kind.as_str().to_string())
                    .unwrap_or_default(),
            }
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

/// Include targets plus companions for the active `.mod`.
///
/// Overlay `files` map same as [`dynare_diagnose`]. Empty or missing
/// `active_file` (or a nonempty map without that key) → `[]`.
///
/// Each row is `{kind, filename, resolved, path?}`. `path` is the caller
/// `files` key when `path_key(resolved)` equals `normalize_uri` of that key;
/// otherwise the library absolute path. Omit `path` when unresolved.
pub fn dynare_related_files(
    file_content: &str,
    active_file: Option<&str>,
    files: Option<&HashMap<String, String>>,
) -> Value {
    let Some(files) = nonempty_map(files) else {
        return json!([]);
    };
    let Some(active) = active_file.filter(|a| !a.is_empty() && files.contains_key(*a)) else {
        return json!([]);
    };
    let workspace_files = overlay_files(file_content, active, files);
    related_files_in_workspace(active, &workspace_files)
}

fn related_files_in_workspace(active_file: &str, files: &HashMap<String, String>) -> Value {
    let mut ws = Workspace::new();
    for (name, content) in files {
        ws.update_document(name, content);
    }
    let includes = ws.include_records(active_file).cloned().unwrap_or_default();
    let companions = ws
        .companion_records(active_file)
        .map(|c| c.to_vec())
        .unwrap_or_default();

    related_files_json(&includes, &companions, |path| {
        related_file_path(path, files)
    })
}

fn related_file_path(resolved: &Path, files: &HashMap<String, String>) -> String {
    let key = path_key(resolved);
    files
        .keys()
        .find(|orig| normalize_uri(orig) == key)
        .cloned()
        .unwrap_or_else(|| resolved.to_string_lossy().into_owned())
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
                file: None,
                line: start.line + 1,
                column: start.character + 1,
                end_line: end.line + 1,
                end_column: end.character + 1,
                severity: mcp_severity(d.severity).to_string(),
                code: d.code.clone(),
                message: d.message.clone(),
                related: related_json(d, text, |_| None),
            }
        })
        .collect()
}

fn diagnostics_to_json_with_origins(
    root_text: &str,
    set: &crate::diagnostic::DiagnosticSet,
    files: &HashMap<String, String>,
) -> Vec<McpDiagnostic> {
    set.diagnostics
        .iter()
        .enumerate()
        .filter(|(_, diag)| !is_dropped_code(&diag.code))
        .map(|(i, diag)| {
            let owner = set.origins.get(i).and_then(Option::as_ref);
            let (text, file, span) = match owner {
                Some(owner) => {
                    let key = files
                        .keys()
                        .find(|key| crate::include_resolver::normalize_uri(key) == owner.file);
                    (
                        owner.text.as_ref(),
                        (owner.file != set.root)
                            .then(|| key.cloned().unwrap_or_else(|| owner.file.clone())),
                        owner.span,
                    )
                }
                None => (root_text, None, diag.span),
            };
            let index = LineIndex::new(text);
            let start = index.position(text, span.start);
            let end = index.position(text, span.end);
            McpDiagnostic {
                file,
                line: start.line + 1,
                column: start.character + 1,
                end_line: end.line + 1,
                end_column: end.character + 1,
                severity: mcp_severity(diag.severity).to_string(),
                code: diag.code.clone(),
                message: diag.message.clone(),
                related: related_json(diag, root_text, |file| {
                    Some(
                        files
                            .keys()
                            .find(|key| normalize_uri(key) == file)
                            .cloned()
                            .unwrap_or_else(|| file.to_string()),
                    )
                }),
            }
        })
        .collect()
}

pub(crate) fn related_json(
    diag: &Diagnostic,
    text: &str,
    alias: impl Fn(&str) -> Option<String>,
) -> Vec<Value> {
    let location = |site: &crate::diagnostic::DiagnosticOrigin| {
        let normalized = normalize_newlines(&site.text);
        let index = LineIndex::new(&normalized);
        let start = index.position(&site.text, site.span.start);
        let end = index.position(&site.text, site.span.end);
        let mut row = json!({"line":start.line+1,"column":start.character+1,"end_line":end.line+1,"end_column":end.character+1});
        if let Some(file) = alias(&site.file) {
            row["file"] = json!(file);
        }
        row
    };
    diag.related
        .iter()
        .flat_map(|related| {
            let sites = if related.mapped {
                related.locations.clone()
            } else if related.file.is_none() {
                vec![crate::diagnostic::DiagnosticOrigin {
                    file: String::new(),
                    text: std::sync::Arc::from(text),
                    span: related.span,
                }]
            } else {
                Vec::new()
            };
            sites
                .iter()
                .map(|site| {
                    let mut row = location(site);
                    row["message"] = json!(related.message);
                    if !related.origin_frames.is_empty() {
                        row["origin_frames"] =
                            json!(related.origin_frames.iter().map(|frame| json!({
                    "kind": frame.kind, "variable": frame.variable, "value": frame.value,
                    "locations": frame.locations.iter().map(&location).collect::<Vec<_>>()
                })).collect::<Vec<_>>());
                    }
                    row
                })
                .collect::<Vec<_>>()
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

const FORMAT_INDENT_ERROR: &str = "formatIndent must be \"tab\" or a whole number from 1 to 8";

/// Format `file_content` with the editor's rules.
///
/// `format_indent` omitted or JSON null uses a tab. Invalid settings are an error.
/// `formatted_text` is the full file only when `status` is `changed`.
pub fn dynare_format(
    file_content: &str,
    format_indent: Option<&Value>,
) -> Result<Value, &'static str> {
    let unit = match format_indent {
        None | Some(Value::Null) => "\t".to_string(),
        Some(value) => parse_format_indent(value).ok_or(FORMAT_INDENT_ERROR)?,
    };
    let (status, formatted_text, reason) = match format_outcome(file_content, &unit) {
        FormatOutcome::Changed(text) => ("changed", Some(text), None),
        FormatOutcome::Unchanged => ("unchanged", None, None),
        FormatOutcome::Unsupported(reason) => ("unsupported", None, Some(reason)),
    };
    Ok(json!({
        "status": status,
        "formatted_text": formatted_text,
        "reason": reason,
    }))
}

const EMPTY_SELECTOR: &str = "names or tags must select at least one equation";

/// Extract a named equation group and the context it needs.
///
/// `names` are OR. `tags` are AND. When both are set, a row must satisfy both.
/// `dimension` searches only that heterogeneity dimension. Omit it to search
/// aggregate and heterogeneous scopes. An empty selector is an error. No match
/// is `empty` with `fragment` `""`. Unresolved expansion and a PAC or VAR
/// expectation in the closure are `unsupported_context` with `fragment` null.
pub fn dynare_extract(
    file_content: &str,
    active_file: Option<&str>,
    files: Option<&HashMap<String, String>>,
    names: &[String],
    tags: &HashMap<String, String>,
    dimension: Option<&str>,
) -> Result<Value, &'static str> {
    let request = ExtractRequest {
        file_content: file_content.to_string(),
        active_file: active_file.map(str::to_string),
        files: files
            .map(|map| {
                map.iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        names: names.to_vec(),
        tags: tags
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
        dimension: dimension.map(str::to_string),
    };
    match extract(&request) {
        Err(ExtractError::EmptySelector) => Err(EMPTY_SELECTOR),
        Ok(result) => {
            let overlaid = active_file
                .zip(nonempty_map(files))
                .map(|(active, map)| overlay_files(file_content, active, map));
            let files = overlaid.as_ref().or(files);
            Ok(extract_json(&result, file_content, files))
        }
    }
}

pub(crate) const WORKSPACE_DIAGNOSE_BOTH: &str =
    "pass either a nonempty files map and roots, or a nonempty paths list, not both";
pub(crate) const WORKSPACE_DIAGNOSE_NEITHER: &str =
    "pass a nonempty files map and roots, or a nonempty paths list";
pub(crate) const WORKSPACE_DIAGNOSE_NO_FILES: &str = "no .mod files found";

/// Batch diagnostics.
///
/// Map mode is a nonempty `files` map and a nonempty `roots` list. Path mode is
/// a nonempty `paths` list and no files or roots. Any other mix is an input error.
pub fn dynare_workspace_diagnose(
    files: Option<&HashMap<String, String>>,
    roots: Option<&[String]>,
    paths: Option<&[String]>,
) -> Result<Value, &'static str> {
    let files_on = files.is_some_and(|map| !map.is_empty());
    let roots_on = roots.is_some_and(|list| !list.is_empty());
    let paths_on = paths.is_some_and(|list| !list.is_empty());
    if paths_on && (files_on || roots_on) {
        return Err(WORKSPACE_DIAGNOSE_BOTH);
    }
    if let Some(paths) = paths.filter(|list| !list.is_empty()) {
        return diagnose_paths(paths)
            .map(|report| workspace_report_json(&report))
            .map_err(workspace_diagnose_message);
    }
    if files_on && roots_on {
        let files = files
            .unwrap()
            .iter()
            .map(|(key, text)| (key.clone(), text.clone()))
            .collect();
        return diagnose_map(&files, roots.unwrap())
            .map(|report| workspace_report_json(&report))
            .map_err(workspace_diagnose_message);
    }
    Err(WORKSPACE_DIAGNOSE_NEITHER)
}

fn workspace_diagnose_message(err: WorkspaceDiagnoseError) -> &'static str {
    match err {
        WorkspaceDiagnoseError::NoFiles => WORKSPACE_DIAGNOSE_NO_FILES,
        WorkspaceDiagnoseError::EmptyMap | WorkspaceDiagnoseError::EmptyRoots => {
            WORKSPACE_DIAGNOSE_NEITHER
        }
    }
}

fn extract_json(
    result: &ExtractResult,
    file_content: &str,
    files: Option<&HashMap<String, String>>,
) -> Value {
    let selected_equations: Vec<Value> = result
        .selected_equations
        .iter()
        .map(|row| {
            json!({
                "domain": domain_name(row.domain),
                "dimension": row.dimension,
                "index": row.index,
                "name": row.name,
                "role": role_name(row.role),
                "tags": row.tags,
            })
        })
        .collect();
    let origins: Vec<Value> = result
        .origins
        .iter()
        .map(|origin| extract_origin_json(origin, file_content, files))
        .collect();
    let omitted_context: Vec<Value> = result
        .omitted_context
        .iter()
        .map(|item| json!({ "kind": item.kind, "detail": item.detail }))
        .collect();
    json!({
        "status": extract_status(result.status),
        "fragment": result.fragment,
        "selected_equations": selected_equations,
        "origins": origins,
        "omitted_context": omitted_context,
        "explanation": result.explanation,
    })
}

fn extract_origin_json(
    origin: &crate::extract::ExtractOrigin,
    file_content: &str,
    files: Option<&HashMap<String, String>>,
) -> Value {
    let text = extract_source(origin.file.as_deref(), file_content, files);
    let mut value = match text {
        Some(text) if span_fits(text, origin.span) => range_json(origin.span, text),
        _ => json!({
            "line": null,
            "column": null,
            "end_line": null,
            "end_column": null,
        }),
    };
    value["domain"] = json!(domain_name(origin.domain));
    value["dimension"] = json!(origin.dimension);
    value["index"] = json!(origin.index);
    value["role"] = json!(role_name(origin.role));
    value["file"] = json!(extract_caller_file(origin.file.as_deref(), files));
    if !origin.frames.is_empty() {
        value["frames"] = json!(origin
            .frames
            .iter()
            .map(|frame| {
                let frame_text = extract_source(frame.file.as_deref(), file_content, files);
                let mut row = match frame_text {
                    Some(text) if span_fits(text, frame.span) => range_json(frame.span, text),
                    _ => json!({
                        "line": null,
                        "column": null,
                        "end_line": null,
                        "end_column": null,
                    }),
                };
                row["kind"] = json!(frame.kind);
                if let Some(variable) = &frame.variable {
                    row["variable"] = json!(variable);
                }
                if let Some(value) = &frame.value {
                    row["value"] = json!(value);
                }
                row["file"] = json!(extract_caller_file(frame.file.as_deref(), files));
                row
            })
            .collect::<Vec<_>>());
    }
    value
}

fn span_fits(text: &str, span: Span) -> bool {
    let end = span.end as usize;
    end <= text.len() && text.is_char_boundary(span.start as usize) && text.is_char_boundary(end)
}

fn extract_source<'a>(
    file: Option<&str>,
    file_content: &'a str,
    files: Option<&'a HashMap<String, String>>,
) -> Option<&'a str> {
    let Some(file) = file else {
        return Some(file_content);
    };
    let Some(files) = files else {
        return Some(file_content);
    };
    if let Some(text) = files.get(file) {
        return Some(text);
    }
    let wanted = normalize_uri(file);
    let mut hits = files.iter().filter(|(key, _)| normalize_uri(key) == wanted);
    let first = hits.next()?;
    if hits.next().is_some() {
        return None;
    }
    Some(first.1)
}

fn extract_caller_file(
    file: Option<&str>,
    files: Option<&HashMap<String, String>>,
) -> Option<String> {
    let file = file?;
    let Some(files) = files else {
        return Some(file.to_string());
    };
    if files.contains_key(file) {
        return Some(file.to_string());
    }
    let wanted = normalize_uri(file);
    let mut hits = files.keys().filter(|key| normalize_uri(key) == wanted);
    let first = hits.next()?;
    if hits.next().is_some() {
        return Some(file.to_string());
    }
    Some(first.clone())
}

fn extract_status(status: ExtractStatus) -> &'static str {
    match status {
        ExtractStatus::Ok => "ok",
        ExtractStatus::Empty => "empty",
        ExtractStatus::UnsupportedContext => "unsupported_context",
    }
}

fn domain_name(domain: EquationDomain) -> &'static str {
    match domain {
        EquationDomain::Aggregate => "aggregate",
        EquationDomain::Heterogeneous => "heterogeneous",
    }
}

fn role_name(role: EquationRole) -> &'static str {
    match role {
        EquationRole::Requested => "requested",
        EquationRole::Companion => "companion",
    }
}

fn workspace_report_json(report: &WorkspaceDiagnoseReport) -> Value {
    json!({
        "summary": {
            "checked": report.summary.checked,
            "failed": report.summary.failed,
            "errors": report.summary.errors,
            "warnings": report.summary.warnings,
            "information": report.summary.information,
        },
        "roots": report.roots.iter().map(workspace_root_json).collect::<Vec<_>>(),
    })
}

fn workspace_root_json(root: &WorkspaceRootReport) -> Value {
    json!({
        "root": root.root,
        "status": match root.status {
            RootStatus::Ok => "ok",
            RootStatus::Failed => "failed",
        },
        "diagnostics": root.diagnostics.iter().map(workspace_diag_json).collect::<Vec<_>>(),
        "failure": root.failure,
    })
}

fn workspace_diag_json(diag: &WorkspaceDiagnostic) -> Value {
    let mut row = json!({
        "file": diag.file,
        "line": diag.diagnostic.line,
        "column": diag.diagnostic.column,
        "end_line": diag.diagnostic.end_line,
        "end_column": diag.diagnostic.end_column,
        "severity": diag.diagnostic.severity,
        "code": diag.diagnostic.code,
        "message": diag.diagnostic.message,
    });
    if !diag.diagnostic.related.is_empty() {
        row["related"] = json!(diag.diagnostic.related);
    }
    row
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
    /// Full root text. Supply this when files is absent or empty. With a nonempty map, it replaces the active_file entry for this request.
    #[serde(default)]
    file_content: Option<String>,
    /// Required with a nonempty files map; must exactly match a key. A missing or unmatched key returns JSON-RPC invalid parameters (-32602). A map supplies text, not permission to read disk.
    #[serde(default)]
    active_file: Option<String>,
    /// Map of file keys to complete text, including executed includes. Use active_file to choose the root. An absent or empty map uses file_content only.
    #[serde(default)]
    files: Option<HashMap<String, String>>,
}

#[derive(Clone)]
struct DygnosisMcp;

#[derive(Debug, Deserialize, JsonSchema)]
struct FileContentParams {
    /// Complete .mod text to process. The tool returns text; it does not write a file.
    file_content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct FormatParams {
    /// Complete .mod text. Formatting does not write to disk.
    file_content: String,
    /// Indentation: "tab" or an integer from 1 to 8 spaces. Omitted values use a tab.
    #[serde(default, rename = "formatIndent")]
    format_indent: Option<Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ExtractParams {
    /// Complete root .mod text. With a nonempty files map, it overlays the active_file entry. Without a map, only this supplied text is used.
    file_content: String,
    /// Root key in the supplied files map. For nonempty maps it must match a key exactly; it is not a disk path to load.
    #[serde(default)]
    active_file: Option<String>,
    /// File-key to complete-text map, including executed includes. No disk files are read in map mode. A nonempty map needs an active_file root key.
    #[serde(default)]
    files: Option<HashMap<String, String>>,
    /// Equation names to select. Supply at least one name or tag filter.
    #[serde(default)]
    names: Vec<String>,
    /// Equation tag names and required values to select. Supply at least one name or tag filter.
    #[serde(default)]
    tags: HashMap<String, String>,
    /// Optional heterogeneity dimension name for equation selection.
    #[serde(default)]
    dimension: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct WorkspaceDiagnoseParams {
    /// Nonempty supplied file-key to text map. Combine with nonempty roots; do not combine with paths.
    #[serde(default)]
    files: Option<HashMap<String, String>>,
    /// Nonempty list of root .mod keys in files. Required for workspace map mode; omit when using paths.
    #[serde(default)]
    roots: Option<Vec<String>>,
    /// Nonempty file or directory paths on this server host. Path mode reads disk, walks directories and skips generated + directories. Do not combine with files or roots.
    #[serde(default)]
    paths: Option<Vec<String>>,
}

#[derive(Debug, JsonSchema)]
struct CompareModelsParams {
    /// Supplied-text mode: complete Before model text, overlaid on active_file_a when a map is supplied. Required with file_content_b; omit every repository-mode argument.
    file_content_a: Option<String>,
    /// Supplied-text mode: complete After model text, overlaid on active_file_b when a map is supplied. Required with file_content_a; omit every repository-mode argument.
    file_content_b: Option<String>,
    /// Before root key in files_a or the shared files map.
    active_file_a: Option<String>,
    /// After root key in files_b or the shared files map.
    active_file_b: Option<String>,
    /// Before include text map. A nonempty map overrides the shared files map for this side.
    files_a: Option<HashMap<String, String>>,
    /// After include text map. A nonempty map overrides the shared files map for this side.
    files_b: Option<HashMap<String, String>>,
    /// Shared supplied include map for either side whose files_a or files_b is absent or empty.
    files: Option<HashMap<String, String>>,
    /// Repository mode: absolute repository root directory on the MCP server host. Required with before and after; omit all supplied-text and include-map arguments.
    repository_path: Option<String>,
    /// Explicit Before selector: Git reads this local commit tree; Working reads saved server-host files. Each root_file is repository-relative .mod or .dyn.
    before: Option<RepositorySelector>,
    /// Explicit After selector. There is no implicit HEAD, current editor, or unsaved-buffer input.
    after: Option<RepositorySelector>,
    /// Repository mode: extra include folders on this server host, used by both sides. Relative paths resolve against repository_path. Omit for no additional folders. Written @#includepath still applies. No editor settings are inferred; historical dependencies must remain inside the repository tree.
    search_paths: Option<Vec<String>>,
}

const COMPARE_TEXT_KEYS: &[&str] = &[
    "file_content_a",
    "file_content_b",
    "active_file_a",
    "active_file_b",
    "files_a",
    "files_b",
    "files",
];
const COMPARE_REPOSITORY_KEYS: &[&str] = &["repository_path", "before", "after", "search_paths"];

impl<'de> Deserialize<'de> for CompareModelsParams {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| serde::de::Error::custom("Comparison arguments must be an object"))?;
        // Presence, including null or empty values, determines the family. Thus
        // an invalid mixed call cannot silently become supplied-text mode.
        if COMPARE_TEXT_KEYS
            .iter()
            .any(|key| object.contains_key(*key))
            && COMPARE_REPOSITORY_KEYS
                .iter()
                .any(|key| object.contains_key(*key))
        {
            return Err(serde::de::Error::custom(
                "Choose supplied-text mode or repository mode; do not combine their arguments",
            ));
        }
        fn field<T: serde::de::DeserializeOwned, E: serde::de::Error>(
            value: &Value,
            key: &str,
        ) -> Result<Option<T>, E> {
            serde_json::from_value(value.get(key).cloned().unwrap_or(Value::Null))
                .map_err(E::custom)
        }
        let params = Self {
            file_content_a: field(&value, "file_content_a")?,
            file_content_b: field(&value, "file_content_b")?,
            active_file_a: field(&value, "active_file_a")?,
            active_file_b: field(&value, "active_file_b")?,
            files_a: field(&value, "files_a")?,
            files_b: field(&value, "files_b")?,
            files: field(&value, "files")?,
            repository_path: field(&value, "repository_path")?,
            before: field(&value, "before")?,
            after: field(&value, "after")?,
            search_paths: field(&value, "search_paths")?,
        };
        params.validate().map_err(serde::de::Error::custom)?;
        Ok(params)
    }
}

impl CompareModelsParams {
    fn validate(&self) -> Result<(), String> {
        let repository_mode = self.repository_path.is_some()
            || self.before.is_some()
            || self.after.is_some()
            || self.search_paths.is_some();
        let supplied_mode = self.file_content_a.is_some()
            || self.file_content_b.is_some()
            || self.active_file_a.is_some()
            || self.active_file_b.is_some()
            || self.files_a.is_some()
            || self.files_b.is_some()
            || self.files.is_some();
        if repository_mode && supplied_mode {
            return Err(
                "Choose supplied-text mode or repository mode; do not combine their arguments"
                    .into(),
            );
        }
        if repository_mode {
            let path = self
                .repository_path
                .as_deref()
                .filter(|path| !path.is_empty() && !path.contains('\0'))
                .ok_or("Repository mode requires a nonempty repository_path on this server host")?;
            if !Path::new(path).is_absolute() {
                return Err("repository_path must be absolute on this server host".into());
            }
            self.before
                .as_ref()
                .ok_or("Repository mode requires both before and after selectors")?
                .validate()?;
            self.after
                .as_ref()
                .ok_or("Repository mode requires both before and after selectors")?
                .validate()?;
            if self
                .search_paths
                .iter()
                .flatten()
                .any(|path| path.contains('\0'))
            {
                return Err("search_paths must not contain NUL characters".into());
            }
        } else if self.file_content_a.is_none() || self.file_content_b.is_none() {
            return Err(
                "Supplied-text mode requires both file_content_a and file_content_b".into(),
            );
        }
        Ok(())
    }

    fn compare(self, cancelled: &(dyn Fn() -> bool + Sync)) -> Value {
        if let Some(repository_path) = self.repository_path {
            compare_repository(
                RepositoryComparison {
                    repository_path,
                    before: self.before.expect("validated Before selector"),
                    after: self.after.expect("validated After selector"),
                    search_paths: self.search_paths.unwrap_or_default(),
                },
                cancelled,
            )
        } else {
            dynare_compare_models(
                self.file_content_a
                    .as_deref()
                    .expect("validated Before text"),
                self.file_content_b
                    .as_deref()
                    .expect("validated After text"),
                self.active_file_a.as_deref(),
                self.active_file_b.as_deref(),
                self.files_a.as_ref(),
                self.files_b.as_ref(),
                self.files.as_ref(),
            )
        }
    }
}

fn compare_models_input_schema() -> std::sync::Arc<serde_json::Map<String, Value>> {
    let mut schema = mcp_input_schema::<CompareModelsParams>().as_ref().clone();
    let absent = |keys: &[&str]| json!({"not": {"anyOf": keys.iter().map(|key| json!({"required": [key]})).collect::<Vec<_>>()}});
    let mut text = absent(COMPARE_REPOSITORY_KEYS);
    text["required"] = json!(["file_content_a", "file_content_b"]);
    text["properties"] =
        json!({"file_content_a": {"type": "string"}, "file_content_b": {"type": "string"}});
    let mut repository = absent(COMPARE_TEXT_KEYS);
    repository["required"] = json!(["repository_path", "before", "after"]);
    repository["properties"] = json!({"repository_path": {"type": "string", "minLength": 1}, "before": {"type": "object"}, "after": {"type": "object"}});
    schema.insert("oneOf".into(), json!([text, repository]));
    std::sync::Arc::new(schema)
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ExplainParams {
    /// Diagnostic code, such as E020 or W013. Use dynare_list_diagnostic_codes to find documented codes and their classification.
    code: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListOptionsParams {
    /// Dynare command name. Omit it to list known commands; an unknown name returns known=false rather than inventing options.
    #[serde(default)]
    command: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct EquationsParams {
    /// Complete root .mod text. With a nonempty files map, it overlays the active_file entry. Without a map, only this supplied text is used.
    #[serde(default)]
    file_content: Option<String>,
    /// Required with a nonempty files map; must exactly match a key. A missing or unmatched key returns JSON-RPC invalid parameters (-32602). It is not a disk path to load.
    #[serde(default)]
    active_file: Option<String>,
    /// File-key to complete-text map, including executed includes. No disk files are read in map mode. A nonempty map needs an active_file root key.
    #[serde(default)]
    files: Option<HashMap<String, String>>,
    /// Optional equation-name filter. Searches aggregate and heterogeneous equations.
    #[serde(default)]
    name: Option<String>,
    /// Optional aggregate equation index, starting at zero. The index filter applies to aggregate equations.
    #[serde(default)]
    index: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct FindReferencesParams {
    /// Complete root .mod text. With a nonempty files map, it overlays the active_file entry. Without a map, only this supplied text is used.
    #[serde(default)]
    file_content: Option<String>,
    /// Exact name to find. Results skip comments and use one-based Unicode-scalar source coordinates.
    symbol: String,
    /// Required with a nonempty files map; must exactly match a key. A missing or unmatched key returns JSON-RPC invalid parameters (-32602). It is not a disk path to load.
    #[serde(default)]
    active_file: Option<String>,
    /// File-key to complete-text map, including executed includes. No disk files are read in map mode. A nonempty map needs an active_file root key.
    #[serde(default)]
    files: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RenameParams {
    /// Complete root .mod text. With a nonempty files map, it overlays the active_file entry. Without a map, only this supplied text is used.
    #[serde(default)]
    file_content: Option<String>,
    /// Exact identifier to rename. Comments are excluded; the tool returns text and does not write files.
    old_name: String,
    /// Replacement identifier. An illegal identifier leaves text unchanged.
    new_name: String,
    /// Required with a nonempty files map; must exactly match a key. A missing or unmatched key returns JSON-RPC invalid parameters (-32602). It is not a disk path to load.
    #[serde(default)]
    active_file: Option<String>,
    /// File-key to complete-text map, including executed includes. No disk files are read in map mode. A nonempty map needs an active_file root key.
    #[serde(default)]
    files: Option<HashMap<String, String>>,
}

/// Source text plus map args after overlay.
struct MappedSource<'a> {
    content: &'a str,
    active: Option<&'a str>,
    files: Option<&'a HashMap<String, String>>,
}

/// `None` means missing `file_content` on the single-file path. A nonempty
/// map requires an exact root key and never falls back to single-file input.
fn resolve_mapped<'a>(
    file_content: Option<&'a str>,
    active_file: Option<&'a str>,
    files: Option<&'a HashMap<String, String>>,
) -> Result<Option<MappedSource<'a>>, rmcp::ErrorData> {
    match nonempty_map(files) {
        None => Ok(file_content.map(|content| MappedSource {
            content,
            active: None,
            files: None,
        })),
        Some(files) => {
            let active = active_file.ok_or_else(|| {
                rmcp::ErrorData::invalid_params(
                    "active_file is required with a nonempty files map",
                    None,
                )
            })?;
            if !files.contains_key(active) {
                return Err(rmcp::ErrorData::invalid_params(
                    format!("\"{active}\" is not in the file map"),
                    None,
                ));
            }
            let content = file_content.unwrap_or_else(|| files[active].as_str());
            Ok(Some(MappedSource {
                content,
                active: Some(active),
                files: Some(files),
            }))
        }
    }
}

/// VS Code's minimum host has draft-07 metadata built in. Generate that
/// dialect rather than making tool discovery load the SDK's 2020-12 metadata,
/// whose dynamic references that host cannot validate.
fn mcp_input_schema<T: JsonSchema>() -> std::sync::Arc<rmcp::model::JsonObject> {
    let schema = schemars::generate::SchemaSettings::draft07()
        .into_generator()
        .into_root_schema_for::<T>();
    let mut object = schema
        .as_object()
        .expect("MCP input schema must be an object")
        .clone();
    assert_eq!(object.get("type"), Some(&json!("object")));
    // Retain the SDK's input-schema convention: parameter type names and docs
    // are not tool-level descriptions.
    object.remove("title");
    object.remove("description");
    std::sync::Arc::new(object)
}

#[tool_router]
impl DygnosisMcp {
    #[tool(
        name = "dynare_diagnose",
        input_schema = mcp_input_schema::<IncludeMapParams>(),
        description = "Check supplied model text and return code, severity, message and written source range. Lines and Unicode-scalar columns are one-based. A files map supplies includes; this mode does not read disk."
    )]
    fn diagnose_tool(
        &self,
        Parameters(params): Parameters<IncludeMapParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let diags = match resolve_mapped(
            params.file_content.as_deref(),
            params.active_file.as_deref(),
            params.files.as_ref(),
        )? {
            Some(src) => dynare_diagnose(src.content, src.active, src.files),
            None => Vec::new(),
        };
        Ok(tool_json(
            serde_json::to_value(diags).expect("diagnose json"),
        ))
    }

    #[tool(
        name = "dynare_model_info",
        input_schema = mcp_input_schema::<IncludeMapParams>(),
        description = "Return aggregate and per-dimension names, counts, timing classes and block flags. Classes and timing counts use offsets after the predetermined-variable convention conversion, without other equation transformations or numerical results. Incomplete expansion withholds authoritative counts."
    )]
    fn model_info_tool(
        &self,
        Parameters(params): Parameters<IncludeMapParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let info = match resolve_mapped(
            params.file_content.as_deref(),
            params.active_file.as_deref(),
            params.files.as_ref(),
        )? {
            Some(src) => dynare_model_info(src.content, src.active, src.files),
            None => dynare_model_info("", None, None),
        };
        Ok(tool_json(info))
    }

    #[tool(
        name = "dynare_compare_models",
        input_schema = compare_models_input_schema(),
        description = "Compare Before to After by symbol kinds and metadata, proven parameter values, aggregate and per-dimension equations, and written shock setup. Choose exactly one mode: supplied text uses both file_content_a/file_content_b and each side's own include maps, including explicitly supplied unsaved text; repository mode uses absolute repository_path on this server host, explicit before/after Git or Working selectors, and optional search_paths. Git refs resolve once to full local commits before source reads; no fetch or checkout occurs. Working means saved files on this server host, including saved active includes and untracked files, never editor buffers. Omitted search_paths means no additional configured include folders; both sides still use written @#includepath. Repository results identify resolved commits, snapshot revisions, source policies, include folders, and exact historical source targets. Incomplete or failed input returns no authoritative change arrays; changed saved input returns INPUT_CHANGED. Source lines and Unicode-scalar columns are one-based. These are structural changes, not numerical equivalence."
    )]
    async fn compare_models_tool(
        &self,
        Parameters(arguments): Parameters<Value>,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        // The SDK reports extractor deserialization errors as tool-result
        // errors. Decode here so invalid mode/selector combinations retain
        // this tool's JSON-RPC invalid-parameters contract.
        let params: CompareModelsParams = serde_json::from_value(arguments)
            .map_err(|error| rmcp::ErrorData::invalid_params(error.to_string(), None))?;
        params
            .validate()
            .map_err(|message| rmcp::ErrorData::invalid_params(message, None))?;
        let cancellation = context.ct;
        let value =
            tokio::task::spawn_blocking(move || params.compare(&|| cancellation.is_cancelled()))
                .await
                .map_err(|error| {
                    rmcp::ErrorData::internal_error(
                        format!("Comparison worker failed: {error}"),
                        None,
                    )
                })?;
        Ok(tool_json(value))
    }

    #[tool(
        name = "dynare_find_references",
        input_schema = mcp_input_schema::<FindReferencesParams>(),
        description = "Find whole-word occurrences of a name, including declarations, and skip comments. Source lines and Unicode-scalar columns are one-based. A files map returns file keys; without a map, positions refer to file_content."
    )]
    fn find_references_tool(
        &self,
        Parameters(params): Parameters<FindReferencesParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let value = match resolve_mapped(
            params.file_content.as_deref(),
            params.active_file.as_deref(),
            params.files.as_ref(),
        )? {
            Some(src) => dynare_find_references(src.content, &params.symbol, src.active, src.files),
            None => json!([]),
        };
        Ok(tool_json(value))
    }

    #[tool(
        name = "dynare_rename",
        input_schema = mcp_input_schema::<RenameParams>(),
        description = "Rename a name. Skips comments. Without a files map, returns the rewritten text (or the original if the new name is not a legal identifier). With a map, returns only files that changed."
    )]
    fn rename_tool(
        &self,
        Parameters(params): Parameters<RenameParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        match resolve_mapped(
            params.file_content.as_deref(),
            params.active_file.as_deref(),
            params.files.as_ref(),
        )? {
            Some(src) => {
                let result = dynare_rename(
                    src.content,
                    &params.old_name,
                    &params.new_name,
                    src.active,
                    src.files,
                );
                match result {
                    Value::String(text) => Ok(tool_text(text)),
                    other => Ok(tool_json(other)),
                }
            }
            None => Ok(tool_text(String::new())),
        }
    }

    #[tool(
        name = "dynare_auto_fix",
        input_schema = mcp_input_schema::<FileContentParams>(),
        description = "Apply stored diagnostic fixes to a .mod file. Leaves the text unchanged when macros would make the rewrite unsafe."
    )]
    fn auto_fix_tool(&self, Parameters(params): Parameters<FileContentParams>) -> String {
        dynare_auto_fix(&params.file_content)
    }

    #[tool(
        name = "dynare_explain",
        input_schema = mcp_input_schema::<ExplainParams>(),
        description = "Return the Markdown explanation for a diagnostic code. An unknown code returns the known code list."
    )]
    fn explain_tool(&self, Parameters(params): Parameters<ExplainParams>) -> String {
        dynare_explain(&params.code)
    }

    #[tool(
        name = "dynare_list_diagnostic_codes",
        description = "List all diagnostic codes, classified as shared, skipped, or added relative to Dynare."
    )]
    fn list_diagnostic_codes_tool(&self) -> CallToolResult {
        tool_json(serde_json::to_value(dynare_list_diagnostic_codes()).expect("list codes json"))
    }

    #[tool(
        name = "dynare_list_options",
        input_schema = mcp_input_schema::<ListOptionsParams>(),
        description = "List valid options for a Dynare command, or list known commands when omitted."
    )]
    fn list_options_tool(
        &self,
        Parameters(params): Parameters<ListOptionsParams>,
    ) -> CallToolResult {
        tool_json(dynare_list_options(params.command.as_deref()))
    }

    #[tool(
        name = "dynare_equations",
        input_schema = mcp_input_schema::<EquationsParams>(),
        description = "List aggregate and per-dimension written equations, identifiers and verified source locations. Identifier timing is the written offset; dynare_timing is the offset after the predetermined-variable convention conversion, not full Transform output. Use timing_class and model-info counts for classification. Older engines omit dynare_timing; do not infer it. Lines and Unicode-scalar columns are one-based. Count gap and index filter apply to aggregate equations; name searches both kinds. Equation numbers are before transformation. Incomplete required includes, parsing, or macro expansion return incomplete with no equations and a null count gap."
    )]
    fn equations_tool(
        &self,
        Parameters(params): Parameters<EquationsParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let index = params.index.map(|i| i as usize);
        let name = params.name.as_deref();
        let payload = match resolve_mapped(
            params.file_content.as_deref(),
            params.active_file.as_deref(),
            params.files.as_ref(),
        )? {
            Some(src) => dynare_equations(src.content, src.active, src.files, name, index),
            None => dynare_equations("", None, None, name, index),
        };
        Ok(tool_json(payload))
    }

    #[tool(
        name = "dynare_related_files",
        input_schema = mcp_input_schema::<IncludeMapParams>(),
        description = "List include targets and companion files for the active .mod (kind, filename, resolved, path)."
    )]
    fn related_files_tool(
        &self,
        Parameters(params): Parameters<IncludeMapParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let value = match resolve_mapped(
            params.file_content.as_deref(),
            params.active_file.as_deref(),
            params.files.as_ref(),
        )? {
            Some(src) => dynare_related_files(src.content, src.active, src.files),
            None => json!([]),
        };
        Ok(tool_json(value))
    }

    #[tool(
        name = "dynare_expand",
        input_schema = mcp_input_schema::<IncludeMapParams>(),
        description = "Return model text after include and macro expansion, before equation transformation, with verified written source and macro locations. macro_messages lists executed @#echo and @#echomacrovars output in order; @#echomacrovars(save) is generated assignment text in effective_text, not a message. Lines and Unicode-scalar columns are one-based. complete=false means expansion is incomplete and authoritative equation counts or source jumps are withheld."
    )]
    fn expand_tool(
        &self,
        Parameters(params): Parameters<IncludeMapParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let value = match resolve_mapped(
            params.file_content.as_deref(),
            params.active_file.as_deref(),
            params.files.as_ref(),
        )? {
            Some(src) => dynare_expand(src.content, src.active, src.files),
            None => dynare_expand("", None, None),
        };
        Ok(tool_json(value))
    }

    #[tool(
        name = "dynare_format",
        input_schema = mcp_input_schema::<FormatParams>(),
        description = "Format a .mod file with the editor's rules. Returns the full text only when formatting changes it. Empty or whitespace-only input is unchanged."
    )]
    fn format_tool(
        &self,
        Parameters(params): Parameters<FormatParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        match dynare_format(&params.file_content, params.format_indent.as_ref()) {
            Ok(value) => Ok(tool_json(value)),
            Err(message) => Err(rmcp::ErrorData::invalid_params(message, None)),
        }
    }

    #[tool(
        name = "dynare_extract",
        input_schema = mcp_input_schema::<ExtractParams>(),
        description = "Extract equations by names and tags, with required declarations, model locals and heterogeneity dimensions. Names match any supplied name; all supplied tags must match, and names and tags combine. The result is a model fragment that needs completion before running Dynare."
    )]
    fn extract_tool(
        &self,
        Parameters(params): Parameters<ExtractParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        match dynare_extract(
            &params.file_content,
            params.active_file.as_deref(),
            params.files.as_ref(),
            &params.names,
            &params.tags,
            params.dimension.as_deref(),
        ) {
            Ok(value) => Ok(tool_json(value)),
            Err(message) => Err(rmcp::ErrorData::invalid_params(message, None)),
        }
    }

    #[tool(
        name = "dynare_workspace_diagnose",
        input_schema = mcp_input_schema::<WorkspaceDiagnoseParams>(),
        description = "Diagnose root .mod files from a files map and roots, or from file and directory paths. Each root is reported on its own, with a summary; one failed root does not drop the others."
    )]
    fn workspace_diagnose_tool(
        &self,
        Parameters(params): Parameters<WorkspaceDiagnoseParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        match dynare_workspace_diagnose(
            params.files.as_ref(),
            params.roots.as_deref(),
            params.paths.as_deref(),
        ) {
            Ok(value) => Ok(tool_json(value)),
            Err(message) => Err(rmcp::ErrorData::invalid_params(message, None)),
        }
    }
}

#[tool_handler(name = "dygnosis")]
impl ServerHandler for DygnosisMcp {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_modes_are_exclusive_and_require_explicit_inputs() {
        let repository_path = std::env::temp_dir()
            .join("models")
            .to_string_lossy()
            .into_owned();
        let repository = json!({"repository_path":repository_path, "before":{"kind":"git", "root_file":"main.mod", "ref":"HEAD"}, "after":{"kind":"working", "root_file":"main.mod"}});
        assert!(serde_json::from_value::<CompareModelsParams>(repository.clone()).is_ok());
        let mut sibling_folder = repository.clone();
        sibling_folder["search_paths"] = json!(["../common"]);
        assert!(serde_json::from_value::<CompareModelsParams>(sibling_folder).is_ok());
        for field in COMPARE_TEXT_KEYS {
            let mut mixed = repository.clone();
            mixed[*field] = Value::Null;
            assert!(serde_json::from_value::<CompareModelsParams>(mixed)
                .unwrap_err()
                .to_string()
                .contains("do not combine"));
        }
        for invalid in [
            json!({}),
            json!({"file_content_a":"var y;"}),
            json!({"repository_path":repository_path, "before":{"kind":"working", "root_file":"main.mod"}}),
            json!({"repository_path":"relative", "before":{"kind":"working", "root_file":"main.mod"}, "after":{"kind":"working", "root_file":"main.mod"}}),
            json!({"repository_path":repository_path, "before":{"kind":"working", "root_file":"../main.mod"}, "after":{"kind":"working", "root_file":"main.mod"}}),
            json!({"repository_path":repository_path, "before":{"kind":"git", "root_file":"main.mod"}, "after":{"kind":"working", "root_file":"main.mod"}}),
            json!({"repository_path":repository_path, "before":{"kind":"working", "root_file":"main.mod", "ref":"HEAD"}, "after":{"kind":"working", "root_file":"main.mod"}}),
            json!({"repository_path":repository_path, "before":{"kind":"working", "root_file":"main.mod"}, "after":{"kind":"working", "root_file":"main.mod"}, "search_paths":["common\u{0}folder"]}),
        ] {
            assert!(
                serde_json::from_value::<CompareModelsParams>(invalid.clone()).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn supplied_text_compare_preserves_established_results() {
        let old = "var y;\nmodel;\n[name='eq'] y=1;\nend;\n";
        let new = "var y;\nmodel;\n[name='eq'] y=2;\nend;\n";
        let params = serde_json::from_value::<CompareModelsParams>(
            json!({"file_content_a":old, "file_content_b":new, "files_a":null, "files_b":{}}),
        )
        .unwrap();
        assert_eq!(
            params.compare(&|| false),
            dynare_compare_models(old, new, None, None, None, None, None)
        );
        let schema = compare_models_input_schema();
        assert_eq!(
            schema["oneOf"][0]["required"],
            json!(["file_content_a", "file_content_b"])
        );
        assert_eq!(
            schema["oneOf"][1]["required"],
            json!(["repository_path", "before", "after"])
        );
    }

    #[test]
    fn equation_index_schema_starts_at_zero() {
        let schema = mcp_input_schema::<EquationsParams>();
        assert!(schema["properties"]["index"]["description"]
            .as_str()
            .unwrap()
            .contains("starting at zero"));
    }

    #[test]
    fn input_schemas_use_the_offline_compatible_draft() {
        for tool in DygnosisMcp::tool_router().list_all() {
            assert_eq!(tool.input_schema["type"], "object", "{}", tool.name);
            if tool.name == "dynare_list_diagnostic_codes" {
                assert_eq!(tool.input_schema["properties"], json!({}));
            } else {
                assert_eq!(
                    tool.input_schema.get("$schema"),
                    Some(&json!("http://json-schema.org/draft-07/schema#")),
                    "{} must use the schema built into the minimum VS Code host",
                    tool.name
                );
            }
        }
    }

    #[test]
    fn input_schema_compatibility_preserves_every_tool_argument() {
        fn assert_same<T: JsonSchema + 'static>(names: &[&str]) {
            let mut previous = rmcp::handler::server::common::schema_for_input::<T>()
                .expect("SDK input schema")
                .as_ref()
                .clone();
            previous.remove("$schema");
            let tools = DygnosisMcp::tool_router().list_all();
            for name in names {
                let mut current = tools
                    .iter()
                    .find(|tool| tool.name == *name)
                    .expect("registered tool")
                    .input_schema
                    .as_ref()
                    .clone();
                current.remove("$schema");
                // Compare the whole shape, including required fields, nullable
                // types, maps, arrays, numeric limits, and serde defaults.
                assert_eq!(current, previous, "{name}");
            }
        }
        assert_same::<IncludeMapParams>(&[
            "dynare_diagnose",
            "dynare_model_info",
            "dynare_related_files",
            "dynare_expand",
        ]);
        assert_eq!(
            DygnosisMcp::tool_router()
                .list_all()
                .iter()
                .find(|tool| tool.name == "dynare_compare_models")
                .unwrap()
                .input_schema,
            compare_models_input_schema()
        );
        assert_same::<FindReferencesParams>(&["dynare_find_references"]);
        assert_same::<RenameParams>(&["dynare_rename"]);
        assert_same::<FileContentParams>(&["dynare_auto_fix"]);
        assert_same::<ExplainParams>(&["dynare_explain"]);
        assert_same::<ListOptionsParams>(&["dynare_list_options"]);
        assert_same::<EquationsParams>(&["dynare_equations"]);
        assert_same::<FormatParams>(&["dynare_format"]);
        assert_same::<ExtractParams>(&["dynare_extract"]);
        assert_same::<WorkspaceDiagnoseParams>(&["dynare_workspace_diagnose"]);
    }

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

        let format_tool = listed
            .iter()
            .find(|tool| tool.name == "dynare_format")
            .unwrap();
        let schema = serde_json::to_string(&format_tool.input_schema).expect("format schema");
        assert!(schema.contains("file_content"), "{schema}");
        assert!(schema.contains("formatIndent"), "{schema}");
        let extract_tool = listed
            .iter()
            .find(|tool| tool.name == "dynare_extract")
            .unwrap();
        let extract_schema =
            serde_json::to_string(&extract_tool.input_schema).expect("extract schema");
        for field in [
            "file_content",
            "active_file",
            "files",
            "names",
            "tags",
            "dimension",
        ] {
            assert!(
                extract_schema.contains(field),
                "missing {field}: {extract_schema}"
            );
        }
        let batch = listed
            .iter()
            .find(|tool| tool.name == "dynare_workspace_diagnose")
            .unwrap();
        let batch_schema = serde_json::to_string(&batch.input_schema).expect("batch schema");
        for field in ["files", "roots", "paths"] {
            assert!(
                batch_schema.contains(field),
                "workspace diagnose schema missing {field}: {batch_schema}"
            );
        }
    }

    #[test]
    fn extract_tool_rejects_an_empty_selector() {
        let server = DygnosisMcp;
        let err = server
            .extract_tool(Parameters(ExtractParams {
                file_content: "var y;\nmodel;\n[name='eq']\ny = 0;\nend;\n".into(),
                active_file: None,
                files: None,
                names: Vec::new(),
                tags: HashMap::new(),
                dimension: None,
            }))
            .expect_err("empty selector");
        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("names or tags"));
    }

    #[test]
    fn extract_origin_uses_the_replacement_for_the_active_file() {
        let mut files = HashMap::new();
        files.insert(
            "root.mod".into(),
            "var y;\nmodel;\n[name='old']\ny = 0;\nend;\n".into(),
        );
        let fresh = "var y;\nmodel;\n[name='eq']\ny = 1;\nend;\n";
        let value = dynare_extract(
            fresh,
            Some("root.mod"),
            Some(&files),
            &["eq".into()],
            &HashMap::new(),
            None,
        )
        .expect("extract");
        assert_eq!(value["status"], "ok");
        assert!(value["fragment"].as_str().unwrap().contains("name='eq'"));
        let origin = &value["origins"][0];
        assert_eq!(origin["file"], "root.mod");
        assert!(origin["line"].as_u64().is_some(), "{origin}");
        let index = LineIndex::new(fresh);
        let start = index.offset(
            fresh,
            crate::span::Position {
                line: origin["line"].as_u64().unwrap() as u32 - 1,
                character: origin["column"].as_u64().unwrap() as u32 - 1,
            },
        ) as usize;
        assert!(fresh[start..].starts_with('['), "{origin} in {fresh}");
    }

    #[test]
    fn format_tool_changed_and_bad_indent() {
        let server = DygnosisMcp;
        let changed = server
            .format_tool(Parameters(FormatParams {
                file_content: "var y;\nmodel;\ny=1;\nend;\n".into(),
                format_indent: None,
            }))
            .expect("format");
        let body = changed.structured_content.expect("structured");
        assert_eq!(body["status"], "changed");
        assert!(body["formatted_text"]
            .as_str()
            .unwrap()
            .contains("\ty = 1;"));
        assert!(body["reason"].is_null());
        for key in ["line", "column", "range", "cursor"] {
            assert!(body.get(key).is_none(), "{key}");
        }

        let err = server
            .format_tool(Parameters(FormatParams {
                file_content: "var y;\n".into(),
                format_indent: Some(json!(0)),
            }))
            .expect_err("bad indent");
        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("formatIndent"));
    }

    #[test]
    fn workspace_diagnose_tool_rejects_empty() {
        let server = DygnosisMcp;
        let err = server
            .workspace_diagnose_tool(Parameters(WorkspaceDiagnoseParams {
                files: None,
                roots: None,
                paths: None,
            }))
            .expect_err("empty");
        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert_eq!(err.message, WORKSPACE_DIAGNOSE_NEITHER);
    }
}
