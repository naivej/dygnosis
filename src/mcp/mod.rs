//! MCP contract 3, stdio transport and direct Rust query functions.
//!
//! The transport and Rust functions share source acquisition and analysis.

mod canonical;
mod edits;
mod multi_request;
pub(crate) mod repository_compare;
mod request;
mod result;
pub(crate) mod source;
pub(crate) mod workspace_diagnose;

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;

use rmcp::{ServerHandler, ServiceExt, tool_handler};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::engine::analysis::equations::{
    CountGap, EquationRow, count_gap, equations, explain_equation, heterogeneous_equations,
};
use crate::engine::analysis::model_info::incomplete_model_status;
use crate::engine::analysis::model_info::{
    heterogeneous_dimension_names, model_info_json, related_files_json,
};
use crate::engine::catalog::list_options;
use crate::engine::comparison::{CompareSource, compare_models_with_sources};
use crate::engine::diagnostics::explain;
use crate::engine::diagnostics::{Diagnostic, Severity, analyze};
use crate::engine::editing::auto_fix::auto_fix;
use crate::engine::editing::extract::{
    EquationDomain, EquationRole, ExtractError, ExtractRequest, ExtractResult, ExtractStatus,
    extract,
};
use crate::engine::editing::format::{FormatOutcome, format_outcome, parse_format_indent};
use crate::engine::editing::refs::is_legal_ident;
use crate::engine::language::expand::{EquationOrigin, ExpandReport, OriginFrame, expand_report};
use crate::engine::language::model::Model;
use crate::engine::language::parser::{normalize_newlines, parse};
use crate::engine::language::span::{LineIndex, Span};
use crate::engine::workspace::Workspace;
use crate::engine::workspace::include_resolver::normalize_uri;
use crate::mcp::repository_compare::RepositorySelector;
#[cfg(test)]
use crate::mcp::repository_compare::{RepositoryComparison, compare_repository};
use crate::mcp::workspace_diagnose::{
    RootStatus, WorkspaceDiagnoseError, WorkspaceDiagnoseReport, WorkspaceDiagnostic,
    WorkspaceRootReport, diagnose_map, diagnose_paths,
};

const OUT_CODES: &[&str] = &[
    "E040", "W040", "W041", "I041", "W071", "I070", "I071", "W080", "W081", "DYNR",
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

/// Registered tool names in registration order.
pub fn registered_tool_names() -> Vec<&'static str> {
    canonical::TOOL_NAMES.to_vec()
}

/// JSON for tools/list assertions (names + descriptions). No live client.
pub fn tools_list_json() -> Value {
    let tools = canonical::advertised_tools(true);
    json!({"tools": canonical::TOOL_NAMES.iter().map(|name| {
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

fn supplied_workspace(files: &HashMap<String, String>) -> Workspace {
    Workspace::overlay_documents(
        &files
            .iter()
            .map(|(key, text)| (key.clone(), text.clone()))
            .collect(),
    )
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
        crate::engine::diagnostics::diagnostic_links::map_free_text(file_content, &mut diags);
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
    let mut ws = supplied_workspace(files);
    let own = crate::engine::diagnostics::check_in_workspace_with_origins(&mut ws, active_file);
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
    // Model info has always treated a supplied active path as an overlay, even
    // when that path is not already present in `files`.
    let unit = match (nonempty_map(files), active_file) {
        (Some(files), Some(active)) => McpUnit::mapped(file_content, active, files),
        _ => McpUnit::free(file_content),
    };
    if let Some(mut status) = incomplete_model_status(&unit.model, unit.includes_complete) {
        status["incomplete_reasons"] = unit.incomplete_reasons();
        return status;
    }
    model_info_with_unit(&unit)
}

fn model_info_with_unit(unit: &McpUnit) -> Value {
    let mut info = model_info_json(&unit.model);
    attach_model_local_origins(&mut info, unit);
    info
}

fn attach_model_local_origins(info: &mut Value, unit: &McpUnit) {
    let facts = crate::engine::analysis::model_locals::ModelLocals::collect(&unit.model);
    let Some(locals) = info.get_mut("model_locals") else {
        return;
    };
    if let Some(rows) = locals.get_mut("declarations").and_then(Value::as_array_mut) {
        for (row, declaration) in rows.iter_mut().zip(&facts.declarations) {
            if let Some(source) = unit
                .report
                .model_map
                .declarations
                .get(declaration.declaration_index)
            {
                attach_local_source(row, source, unit);
            }
        }
    }
    if let Some(rows) = locals.get_mut("definitions").and_then(Value::as_array_mut) {
        for (row, definition) in rows.iter_mut().zip(&facts.definitions) {
            if let Some(source) = unit
                .report
                .model_map
                .equations
                .get(definition.equation_index)
                .map(|occurrence| &occurrence.source)
            {
                attach_local_source(row, source, unit);
            }
        }
    }
}

fn attach_local_source(
    row: &mut Value,
    source: &crate::engine::workspace::model_map::SourceOccurrence,
    unit: &McpUnit,
) {
    let [segment] = source.segments.as_slice() else {
        return;
    };
    let Some(text) = unit.source_for(segment.file.as_deref()) else {
        return;
    };
    if segment.span.is_empty() || !span_fits(text, segment.span) {
        return;
    }
    row["origin"] = range_json(segment.span, text);
    if let Some(file_key) = unit.map_uri(segment.file.as_deref()) {
        row["origin_uri"] = json!(file_key);
    }
    if source.origin_frames.is_empty() {
        return;
    }
    let mut frames = Vec::with_capacity(source.origin_frames.len());
    for frame in &source.origin_frames {
        let [segment] = frame.segments.as_slice() else {
            return;
        };
        let Some(text) = unit.source_for(segment.file.as_deref()) else {
            return;
        };
        if segment.span.is_empty() || !span_fits(text, segment.span) {
            return;
        }
        let mut value = range_json(segment.span, text);
        value["kind"] = json!(frame.kind);
        if let Some(variable) = &frame.variable {
            value["variable"] = json!(variable);
        }
        if let Some(expansion) = &frame.value {
            value["value"] = json!(expansion);
        }
        if let Some(file_key) = unit.map_uri(segment.file.as_deref()) {
            value["origin_uri"] = json!(file_key);
        }
        frames.push(value);
    }
    if publish_origin_frames_json(&frames) {
        row["origin_frames"] = Value::Array(frames);
    }
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
    equations_with_unit(&unit, name, index)
}

fn equations_with_unit(unit: &McpUnit, name: Option<&str>, index: Option<usize>) -> Value {
    if let Some(mut status) = incomplete_model_status(&unit.model, unit.includes_complete) {
        status["incomplete_reasons"] = unit.incomplete_reasons();
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
                    .map(|row| equation_row_json(row, origin_for(unit, row.index), unit, false))
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
                        .map(|row| equation_row_json(row, origin_for(unit, row.index), unit, true))
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
                    origin_for(unit, rows[i].index),
                    unit,
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
                            equation_row_json(row, origin, unit, name.is_some())
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
    expand_with_unit(&unit)
}

fn expand_with_unit(unit: &McpUnit) -> Value {
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
            attach_origin(&mut v, origin, unit);
            v
        })
        .collect();
    let mut result = json!({
        "effective_text": unit.report.effective_text,
        "n_equations": unit.report.n_equations,
        "origins": origins,
        "navigation_schema_version": crate::engine::preview::preview_navigation::NAVIGATION_SCHEMA_VERSION,
        "root_file": unit.root_file,
        "revision": unit.revision,
        "complete": unit.complete,
        "macro_messages": macro_messages_json(&unit.report.macro_messages, unit),
    });
    result["navigation"] = if unit.complete {
        crate::engine::preview::preview_navigation::navigation_json(
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
        result["message"] =
            json!("Expansion is partial; equation counts and source navigation are unavailable.");
        result["incomplete_reasons"] = unit.incomplete_reasons();
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

struct McpUnit<'a> {
    model: Cow<'a, Model>,
    report: Cow<'a, ExpandReport>,
    raw: String,
    files: Option<HashMap<String, String>>,
    sources: HashMap<String, String>,
    root_file: Option<String>,
    revision: String,
    complete: bool,
    includes_complete: bool,
    reasons: Option<Value>,
}

fn mcp_unit(
    file_content: &str,
    active_file: Option<&str>,
    files: Option<&HashMap<String, String>>,
) -> McpUnit<'static> {
    let Some(files) = nonempty_map(files) else {
        return McpUnit::free(file_content);
    };
    let Some(active) = active_file.filter(|a| files.contains_key(*a)) else {
        return McpUnit::free(file_content);
    };
    McpUnit::mapped(file_content, active, files)
}

impl McpUnit<'_> {
    fn incomplete_reasons(&self) -> Value {
        if let Some(reasons) = &self.reasons {
            return reasons.clone();
        }
        let reasons =
            crate::engine::analysis::model_info::incomplete_reason_records(&self.model, None);
        let mut reasons: Vec<Value> = reasons
            .into_iter()
            .map(|(span, code, message)| {
                let mut row = json!({"code":code, "message":message});
                let origin = if code == "E061" {
                    Some((self.root_file.as_deref(), span))
                } else {
                    self.files.is_none().then_some((None, span))
                };
                if let Some((file, span)) = origin {
                    if let Some(text) = self.source_for(file) {
                        row["range"] = range_json(span, text);
                    }
                    if let Some(file) = self.map_uri(file) {
                        row["file"] = json!(file);
                    }
                }
                row
            })
            .collect();
        if !self.includes_complete {
            reasons.push(json!({"message":"Required include text is unavailable. Supply a files map and its active_file root key."}));
        }
        if reasons.is_empty() {
            json!([{ "message": "The complete expanded source or its written locations could not be verified." }])
        } else {
            json!(reasons)
        }
    }

    fn free(file_content: &str) -> Self {
        use std::hash::{Hash, Hasher};
        let report = expand_report(file_content);
        let includes_complete =
            crate::engine::language::macro_expand::required_includes_complete(file_content);
        let complete = report.navigation_complete && includes_complete;
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        file_content.hash(&mut hash);
        Self {
            model: Cow::Owned(parse(file_content)),
            report: Cow::Owned(report),
            raw: normalize_newlines(file_content),
            files: None,
            sources: HashMap::new(),
            root_file: None,
            revision: format!("{:016x}", hash.finish()),
            complete,
            includes_complete,
            reasons: None,
        }
    }

    fn mapped(file_content: &str, active: &str, files: &HashMap<String, String>) -> Self {
        let workspace_files = overlay_files(file_content, active, files);
        let mut ws = supplied_workspace(&workspace_files);
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
        let reasons = (!complete).then(|| {
            comparison_incomplete_reasons(&mut ws, active, &model, Some(&workspace_files))
        });
        for uri in ws.document_uris() {
            if let Some(src) = ws.get_source(&uri) {
                sources.insert(uri, src.to_string());
            }
        }
        Self {
            model: Cow::Owned(model),
            report: Cow::Owned(report),
            raw: normalize_newlines(file_content),
            files: Some(workspace_files),
            sources,
            root_file: Some(active.to_string()),
            revision,
            complete,
            includes_complete,
            reasons,
        }
    }

    fn source_for(&self, origin_uri: Option<&str>) -> Option<&str> {
        match origin_uri {
            None => Some(self.raw.as_str()),
            Some(uri) => {
                if let Some(src) = self.sources.get(uri) {
                    return Some(src.as_str());
                }
                let files = self.files.as_ref()?;
                files
                    .iter()
                    .find(|(name, _)| {
                        crate::engine::workspace::include_resolver::supplied_path_key(name)
                            == crate::engine::workspace::include_resolver::supplied_path_key(uri)
                    })
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

fn origin_for<'a>(unit: &'a McpUnit<'_>, index: usize) -> Option<&'a EquationOrigin> {
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

fn publish_origin_frames(frames: &[crate::engine::language::expand::OriginFrame]) -> bool {
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

fn macro_messages_json(
    messages: &[crate::engine::language::macro_expand::MacroMessage],
    unit: &McpUnit,
) -> Value {
    json!(
        messages
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
            .collect::<Vec<_>>()
    )
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

/// Shared structural and semantic comparison JSON, without solver results.
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
    let mut incomplete = Vec::new();
    for (side, model, workspace, root, files) in [
        (
            "before",
            &model_a,
            &mut workspace_a,
            &root_a,
            first_nonempty_files(files_a, files),
        ),
        (
            "after",
            &model_b,
            &mut workspace_b,
            &root_b,
            first_nonempty_files(files_b, files),
        ),
    ] {
        if incomplete_model_status(model, workspace.includes_complete(root)).is_some() {
            incomplete.push(json!({"side":side, "incomplete_reasons":comparison_incomplete_reasons(workspace, root, model, files)}));
        }
    }
    if !incomplete.is_empty() {
        return json!({"status":"incomplete", "message":"Comparison inputs are incomplete; change arrays and source navigation were withheld.", "inputs":incomplete});
    }
    let mut diff = compare_models_with_sources(
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
    let before = crate::engine::comparison::compare_navigation::ComparisonInput::capture(
        &mut workspace_a,
        &root_a,
        active_file_a,
        revision_a,
        &model_a,
        diff.shock_setup_changes
            .iter()
            .map(|change| change.before.as_ref()),
    )
    .with_written_facts(
        &mut workspace_a,
        &root_a,
        &model_a,
        &diff,
        crate::engine::comparison::semantic_diff::Side::Before,
    )
    .with_file_names(
        first_nonempty_files(files_a, files)
            .into_iter()
            .flat_map(|map| map.keys()),
    );
    let after = crate::engine::comparison::compare_navigation::ComparisonInput::capture(
        &mut workspace_b,
        &root_b,
        active_file_b,
        revision_b,
        &model_b,
        diff.shock_setup_changes
            .iter()
            .map(|change| change.after.as_ref()),
    )
    .with_written_facts(
        &mut workspace_b,
        &root_b,
        &model_b,
        &diff,
        crate::engine::comparison::semantic_diff::Side::After,
    )
    .with_file_names(
        first_nonempty_files(files_b, files)
            .into_iter()
            .flat_map(|map| map.keys()),
    );
    let include_pairs = before.same_file_pairs(&after);
    crate::engine::comparison::compare_navigation::populate_written_statements(
        &mut diff, &before, &after,
    );
    let boundary =
        crate::engine::comparison::semantic_diff::CaptureBoundary::SuppliedRootAndExecutedIncludes;
    // Invalid retained maps leave explicit unavailable Source coverage.
    let _ = crate::engine::comparison::semantic_diff::populate_captured_sources(
        &mut diff,
        before.source_input(boundary),
        after.source_input(boundary),
        &include_pairs,
    );
    crate::engine::comparison::semantic_diff::enforce_output_budget(&mut diff);
    if !workspace_a.input_snapshot_is_current(&root_a)
        || !workspace_b.input_snapshot_is_current(&root_b)
    {
        return json!({"error": "Comparison inputs changed while reading them; refresh the comparison", "code": "INPUT_CHANGED"});
    }
    let mut result = diff.to_json();
    result["navigation"] = crate::engine::comparison::compare_navigation::navigation_json(
        &diff,
        &before,
        &after,
        crate::engine::comparison::compare_navigation::Coordinates::Mcp,
    );
    result["sources"] = json!({"before":before.sources(),"after":after.sources()});
    result
}

fn comparison_incomplete_reasons(
    workspace: &mut Workspace,
    root: &str,
    model: &Model,
    files: Option<&HashMap<String, String>>,
) -> Value {
    let causes = crate::engine::analysis::model_info::incomplete_causes(workspace, root, model);
    json!(
        causes
            .into_iter()
            .map(|cause| {
                let mut reason = cause.outcome_json();
                let mut location = |site: &crate::engine::workspace::IncludeSite| {
                    let text = workspace.source_for_normalized_key(&site.file)?;
                    let mut result = json!({"range":range_json(site.span, text)});
                    if let Some(files) = files {
                        result["file"] = json!(related_file_path(Path::new(&site.file), files));
                    }
                    Some(result)
                };
                if let Some(Value::Object(source)) = cause.origin.as_ref().and_then(&mut location) {
                    reason.as_object_mut().unwrap().extend(source);
                }
                let chain: Vec<_> = cause.include_chain.iter().filter_map(location).collect();
                if !chain.is_empty() {
                    reason["include_chain"] = json!(chain);
                }
                reason
            })
            .collect::<Vec<_>>()
    )
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
    let mut captured: std::collections::BTreeMap<_, _> = files
        .iter()
        .map(|(key, text)| (key.clone(), text.clone()))
        .collect();
    captured.insert(active.clone(), file_content.to_owned());
    let mut ws = Workspace::overlay_documents(&captured);
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

/// Accepted uses of one unique symbol binding, in the legacy coordinate shape.
///
/// No map: `[{ line, column, end_column }]`. With a nonempty map: objects also
/// have `file` (caller key). Missing `active_file` or empty symbol → `[]`.
pub fn dynare_find_references(
    file_content: &str,
    symbol: &str,
    active_file: Option<&str>,
    files: Option<&HashMap<String, String>>,
) -> Value {
    if symbol.is_empty()
        || nonempty_map(files)
            .is_some_and(|files| active_file.is_none_or(|active| !files.contains_key(active)))
    {
        return json!([]);
    }
    let arguments = json!({"file_content":file_content,"symbol":symbol,"active_file":active_file,"files":files});
    library_query("dynare_find_references", arguments, json!([]))
}

/// Rename one unique binding with the shared safety decision; never spelling replacement.
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
    let unchanged = if nonempty_map(files).is_some() {
        json!({})
    } else {
        json!(file_content)
    };
    if !is_legal_ident(new_name)
        || old_name.is_empty()
        || nonempty_map(files)
            .is_some_and(|files| active_file.is_none_or(|active| !files.contains_key(active)))
    {
        return unchanged;
    }
    let arguments = json!({"file_content":file_content,"old_name":old_name,"new_name":new_name,"active_file":active_file,"files":files});
    library_query("dynare_rename", arguments, unchanged)
}

fn library_query(tool: &str, arguments: Value, fallback: Value) -> Value {
    match request::dispatch(tool, arguments.clone(), source::RequestControl::default()) {
        Ok(result) => {
            if result.is_error == Some(true) {
                return result.structured_content.expect("canonical refusal");
            }
            let value = result.structured_content.expect("library query result");
            let data = &value["data"];
            match tool {
                "dynare_rename" => {
                    if arguments
                        .get("files")
                        .and_then(Value::as_object)
                        .is_some_and(|files| !files.is_empty())
                    {
                        data["files"].clone()
                    } else {
                        data["text"].clone()
                    }
                }
                "dynare_find_references" => {
                    let named = arguments
                        .get("files")
                        .and_then(Value::as_object)
                        .is_some_and(|files| !files.is_empty());
                    json!(data["references"].as_array().into_iter().flatten().map(|row| {
                        let range = &row["range"];
                        let mut location = json!({"line":range["line"],"column":range["column"],"end_column":range["end_column"]});
                        if named { location["file"] = row["file"].clone(); }
                        location
                    }).collect::<Vec<_>>())
                }
                _ => fallback,
            }
        }
        Err(error) => json!({"status":"refused","message":error.message}),
    }
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
    let mut ws = supplied_workspace(files);
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
    let key =
        crate::engine::workspace::include_resolver::supplied_path_key(&resolved.to_string_lossy());
    files
        .keys()
        .find(|orig| crate::engine::workspace::include_resolver::supplied_path_key(orig) == key)
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
    set: &crate::engine::diagnostics::DiagnosticSet,
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
                    let key = files.keys().find(|key| {
                        crate::engine::workspace::include_resolver::normalize_uri(key) == owner.file
                    });
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
    let location = |site: &crate::engine::diagnostics::DiagnosticOrigin| {
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
                vec![crate::engine::diagnostics::DiagnosticOrigin {
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
pub(crate) const WORKSPACE_DIAGNOSE_NO_FILES: &str = "no .mod or .dyn files found";

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
    origin: &crate::engine::editing::extract::ExtractOrigin,
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
        value["frames"] = json!(
            origin
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
                .collect::<Vec<_>>()
        );
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

#[derive(Clone)]
struct DygnosisMcp;

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

    #[cfg(test)]
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

#[tool_handler(name = "dygnosis")]
impl ServerHandler for DygnosisMcp {
    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, rmcp::ErrorData> {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        struct StopWorker(Arc<AtomicBool>);
        impl Drop for StopWorker {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Relaxed);
            }
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let _stop = StopWorker(Arc::clone(&cancelled));
        let structured = context
            .protocol_version()
            .is_some_and(|version| version >= rmcp::model::ProtocolVersion::V_2025_06_18);
        let name = request.name.to_string();
        if !canonical::TOOL_NAMES.contains(&name.as_str()) {
            return Err(rmcp::ErrorData::invalid_params(
                format!(
                    "Tool {name} is not available. Read tools/list for the supported tool names and arguments."
                ),
                None,
            ));
        }
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        let control = source::RequestControl {
            cancelled: Arc::clone(&cancelled),
            ..Default::default()
        };
        let progress = Arc::clone(&control.progress);
        let progress_token = context.meta.get_progress_token();
        let mut worker = tokio::task::spawn_blocking(move || {
            crate::engine::control::run(Some(Arc::clone(&control.cancelled)), || {
                canonical::dispatch(&name, arguments, control)
            })
            .map_err(|_| rmcp::ErrorData::internal_error("Request cancelled", None))?
        });
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(500));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut sent = None;
        let result=loop {
            tokio::select! {
                result=&mut worker=>break result,
                _=context.ct.cancelled()=>{
                    cancelled.store(true,Ordering::Relaxed);
                    let _=worker.await;
                    return Err(rmcp::ErrorData::internal_error("Request cancelled",None));
                }
                _=tick.tick(),if progress_token.is_some()=>{
                    let (count,phase)={let phase=progress.phase.lock().unwrap_or_else(|error|error.into_inner());
                        (progress.completed.load(Ordering::SeqCst),*phase)};
                    if sent.is_none_or(|sent|count>sent) && !context.ct.is_cancelled()
                        && let Some(token)=&progress_token {
                            let _=context.peer.notify_progress(rmcp::model::ProgressNotificationParam::new(token.clone(),count as f64).with_message(phase)).await;
                            sent=Some(count);
                        }
                }
            }
        }.map_err(|error|rmcp::ErrorData::internal_error(format!("Model request worker failed: {error}"),None))?;
        let mut result = result?;
        if !structured {
            result.structured_content = None;
        }
        Ok(result.into())
    }

    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::ListToolsResult, rmcp::ErrorData> {
        let structured = context
            .protocol_version()
            .is_some_and(|version| version >= rmcp::model::ProtocolVersion::V_2025_06_18);
        Ok(rmcp::model::ListToolsResult::with_all_items(
            canonical::advertised_tools(structured),
        ))
    }

    fn get_tool(&self, name: &str) -> Option<rmcp::model::Tool> {
        canonical::advertised_tools(true)
            .into_iter()
            .find(|tool| tool.name == name)
    }
}

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
            assert!(
                serde_json::from_value::<CompareModelsParams>(mixed)
                    .unwrap_err()
                    .to_string()
                    .contains("do not combine")
            );
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
    }

    // Guards source validation and unknown coverage becoming successful empty facts.
    #[test]
    fn model_requests_require_source_and_explain_incomplete_inputs() {
        let control = source::RequestControl::default;
        assert!(canonical::dispatch("dynare_model_info", json!({}), control()).is_err());
        let empty = canonical::dispatch(
            "dynare_diagnose",
            json!({"source":{"kind":"text","text":""}}),
            control(),
        )
        .unwrap()
        .structured_content
        .unwrap();
        assert!(empty["data"]["roots"][0]["diagnostics"].is_array());
        let incomplete = canonical::dispatch("dynare_model_info", json!({"source":{"kind":"map","root":"root.mod","files":{"root.mod":"var y;\n@#include \"missing.inc\"\nmodel;y=0;end;"}}}), control()).unwrap().structured_content.unwrap();
        assert_eq!(incomplete["outcome"]["status"], "partial");
        assert!(incomplete["coverage"].to_string().contains("missing.inc"));
    }

    // Guards malformed macro causes and built-in uses disappearing from library queries.
    #[test]
    fn library_queries_keep_macro_causes_and_builtin_references() {
        let info = dynare_model_info("@#define x = \"s\" + 1\nvar y;model;y=0;end;", None, None);
        assert!(
            info["incomplete_reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reason| reason["code"] == "E285")
        );
        let references = dynare_find_references("var y;model;y=sin(y);end;", "sin", None, None);
        assert_eq!(references.as_array().unwrap().len(), 1);
    }

    #[test]
    fn input_schemas_use_the_offline_compatible_draft() {
        for tool in canonical::advertised_tools(true) {
            assert_eq!(
                tool.input_schema.get("$schema"),
                Some(&json!("http://json-schema.org/draft-07/schema#"))
            );
        }
    }
}
