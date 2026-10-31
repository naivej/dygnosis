//! MCP request contracts and adapters over one explicitly acquired source snapshot.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::engine::language::span::LineIndex;
use rmcp::model::CallToolResult;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::result::{Detail, Pages, ResultOptions, ToolResult};
use super::source::{RequestControl, SourceArgs, SourceSnapshot};

#[cfg(test)]
thread_local! {
    static SYMBOL_REFERENCE_PROJECTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static SYMBOL_OCCURRENCE_PROJECTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub(crate) struct Target {
    /// Exact source file key. Null or omitted selects the root; text sources have no file identity.
    pub file: Option<String>,
    /// One-based written line.
    #[schemars(range(min = 1))]
    pub line: u32,
    /// One-based Unicode-scalar column. End positions are exclusive.
    #[schemars(range(min = 1))]
    pub column: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct LineRange {
    /// First selected line, one-based and inclusive.
    #[schemars(range(min = 1))]
    pub start_line: u32,
    /// Last selected line, one-based and inclusive.
    #[schemars(range(min = 1))]
    pub end_line: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ExpandedRange {
    #[schemars(range(min = 1))]
    pub line: u32,
    #[schemars(range(min = 1))]
    pub column: u32,
    #[schemars(range(min = 1))]
    pub end_line: u32,
    #[schemars(range(min = 1))]
    pub end_column: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ScopedEquation {
    /// Aggregate or dimension scope. Dimension scope requires dimension.
    pub scope: String,
    pub dimension: Option<String>,
    /// Zero-based counted row in the selected scope, before Dynare transformations.
    pub index: usize,
}

/// The schema projection and decoder use this single field inventory. Each
/// tool accepts only its own projection; validation checks cross-field rules.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub(crate) struct Params {
    pub view: Option<super::canonical::View>,
    #[serde(skip)]
    #[schemars(skip)]
    /// Agent projections use selected sections and shared page allowances.
    /// Direct Rust library queries retain their established data projections.
    pub agent_output: bool,
    #[serde(flatten)]
    pub source_args: SourceArgs,
    #[serde(flatten)]
    pub output: ResultOptions,
    /// Select one written symbol or source action location.
    pub target: Option<Target>,
    /// Symbol name. A unique binding is required; ambiguous names return scoped candidates.
    pub symbol: Option<String>,
    /// Snapshot-bound symbol identity returned by an earlier query.
    pub binding_id: Option<String>,
    /// Internal Rename name selector. Use exactly one of old_name, target or binding_id.
    pub old_name: Option<String>,
    /// Legal, non-reserved replacement identifier. Rename returns a proposal and never writes files.
    pub new_name: Option<String>,
    /// Include declarations in References. Omitted values include them.
    pub include_declaration: Option<bool>,
    /// Return separate expanded occurrences beside deduplicated written locations.
    pub occurrences: Option<bool>,
    /// Exact equation display-name filter, or code/catalog text filter where documented.
    pub name: Option<String>,
    /// Zero-based aggregate counted equation index, before transformations.
    pub index: Option<usize>,
    /// Aggregate or dimension equation scope.
    pub scope: Option<String>,
    /// Exact heterogeneity dimension name.
    pub dimension: Option<String>,
    /// Required equation tag values. Independent filters combine with AND.
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
    /// Snapshot-bound expanded equation identity.
    pub equation_id: Option<String>,
    /// Select an unnamed equation by explicit scope and zero-based index.
    pub equation: Option<ScopedEquation>,
    /// Extraction name selectors combine with OR; names and tag filters combine with AND.
    #[serde(default)]
    pub names: Vec<String>,
    /// Diagnostic or catalog code filter.
    pub code: Option<String>,
    /// Diagnostic severity filter: ERROR, WARNING, INFORMATION or HINT.
    pub severity: Option<String>,
    /// Exact written source-file filter, or selected source for Format.
    pub file: Option<String>,
    /// Diagnostic classification filter: shared, added or skipped.
    pub kind: Option<String>,
    /// Case-insensitive catalog text search.
    pub search: Option<String>,
    /// Dynare command name; omit to list known commands.
    pub command: Option<String>,
    /// Exact option name filter.
    pub option: Option<String>,
    /// Whole-line formatting or expanded-text selection, one-based and inclusive.
    pub line_range: Option<LineRange>,
    /// Half-open range in the full expanded text, with one-based Unicode-scalar coordinates.
    pub expanded_range: Option<ExpandedRange>,
    /// Formatting indentation: tab or a whole number from 1 to 8.
    #[serde(rename = "formatIndent")]
    pub format_indent: Option<Value>,
    /// Snapshot-bound selected diagnostic identity.
    pub diagnostic_id: Option<String>,
    #[serde(default)]
    pub diagnostic_ids: Vec<String>,
    /// Snapshot-bound selected action identity. Its complete proposal is returned atomically.
    pub action_id: Option<String>,
    /// diagnostic_fix, equation_tags, long_names or shock_template.
    pub action_kind: Option<String>,
}

impl std::ops::Deref for Params {
    type Target = SourceArgs;
    fn deref(&self) -> &SourceArgs {
        &self.source_args
    }
}

const SOURCE_FIELDS: &[&str] = &[
    "source",
    "analysis",
    "file_content",
    "active_file",
    "files",
    "expected_revision",
];
const OUTPUT_FIELDS: &[&str] = &["detail", "sections", "page_size", "continuation"];

fn fields(tool: &str) -> Vec<&'static str> {
    let mut fields = OUTPUT_FIELDS.to_vec();
    if !matches!(
        tool,
        "dynare_explain" | "dynare_list_diagnostic_codes" | "dynare_list_options"
    ) {
        fields.extend_from_slice(SOURCE_FIELDS);
        fields.push("macro_work_limit");
    }
    fields.extend_from_slice(match tool {
        "dynare_diagnose" => &["code", "severity", "file"],
        "dynare_model_info" => &["file"],
        "dynare_related_files" => &[],
        "dynare_find_references" => &[
            "symbol",
            "binding_id",
            "target",
            "include_declaration",
            "occurrences",
            "view",
        ],
        "dynare_symbol_info" => &["symbol", "binding_id", "target", "view"],
        "dynare_rename" => &["old_name", "binding_id", "target", "new_name"],
        "dynare_equations" => &[
            "name",
            "index",
            "scope",
            "dimension",
            "tags",
            "symbol",
            "binding_id",
            "target",
            "equation_id",
            "equation",
            "view",
            "file",
        ],
        "dynare_expand" => &["line_range", "expanded_range", "equation_id"],
        "dynare_format" => &["file", "line_range", "formatIndent"],
        "dynare_extract" => &["names", "tags", "dimension", "equation_id", "equation"],
        "dynare_auto_fix" => &["code", "diagnostic_id", "target"],
        "dynare_code_actions" => &[
            "target",
            "action_kind",
            "diagnostic_id",
            "code",
            "action_id",
        ],
        "dynare_explain" => &["code"],
        "dynare_list_diagnostic_codes" => &["code", "kind", "severity", "search"],
        "dynare_list_options" => &["command", "option", "search"],
        _ => &[],
    });
    fields
}

pub(crate) fn prune_definitions(schema: &mut serde_json::Map<String, Value>) {
    fn references(value: &Value, found: &mut std::collections::BTreeSet<String>) {
        match value {
            Value::Object(object) => {
                if let Some(reference) = object
                    .get("$ref")
                    .and_then(Value::as_str)
                    .and_then(|s| s.strip_prefix("#/definitions/"))
                {
                    found.insert(reference.to_owned());
                }
                for child in object.values() {
                    references(child, found);
                }
            }
            Value::Array(rows) => {
                for row in rows {
                    references(row, found);
                }
            }
            _ => {}
        }
    }
    let Some(Value::Object(definitions)) = schema.remove("definitions") else {
        return;
    };
    let mut found = std::collections::BTreeSet::new();
    references(&Value::Object(schema.clone()), &mut found);
    loop {
        let count = found.len();
        for name in found.clone() {
            if let Some(value) = definitions.get(&name) {
                references(value, &mut found);
            }
        }
        if found.len() == count {
            break;
        }
    }
    if !found.is_empty() {
        schema.insert(
            "definitions".into(),
            Value::Object(
                definitions
                    .into_iter()
                    .filter(|(name, _)| found.contains(name))
                    .collect(),
            ),
        );
    }
}

pub(crate) fn decode(tool: &str, value: &Value) -> Result<Params, rmcp::ErrorData> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid("Tool arguments must be an object"))?;
    let allowed = fields(tool);
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(invalid(format!(
            "{tool} does not accept the argument {key}"
        )));
    }
    if object.contains_key("source")
        && ["file_content", "active_file", "files"]
            .iter()
            .any(|key| object.contains_key(*key))
    {
        return Err(invalid(
            "source is mutually exclusive with file_content, active_file and files",
        ));
    }
    let params: Params =
        serde_json::from_value(value.clone()).map_err(|e| invalid(e.to_string()))?;
    if params
        .scope
        .as_ref()
        .is_some_and(|scope| !matches!(scope.as_str(), "aggregate" | "dimension"))
        || params.equation.as_ref().is_some_and(|equation| {
            !matches!(equation.scope.as_str(), "aggregate" | "dimension")
                || (equation.scope == "dimension" && equation.dimension.is_none())
        })
    {
        return Err(invalid(
            "Equation scope must be aggregate or dimension; dimension scope requires dimension",
        ));
    }
    if params
        .output
        .page_size
        .is_some_and(|size| !(1..=1000).contains(&size))
    {
        return Err(invalid("page_size must be a whole number from 1 to 1000"));
    }
    if [
        params.binding_id.is_some(),
        params.equation_id.is_some(),
        params.action_id.is_some(),
        params.diagnostic_id.is_some(),
        params.output.continuation.is_some() && allowed.contains(&"expected_revision"),
    ]
    .into_iter()
    .any(|v| v)
        && params.expected_revision.is_none()
    {
        return Err(invalid(
            "Reusing an identity or continuation requires expected_revision from the original input",
        ));
    }
    Ok(params)
}

fn invalid(message: impl Into<String>) -> rmcp::ErrorData {
    rmcp::ErrorData::invalid_params(message.into(), None)
}

pub(crate) fn byte_position(text: &str, line: u32, column: u32) -> Result<u32, rmcp::ErrorData> {
    if line == 0 || column == 0 {
        return Err(invalid("Source lines and columns must start at one"));
    }
    let mut start = 0usize;
    let mut lines = text.split('\n');
    for _ in 1..line {
        start += lines
            .next()
            .ok_or_else(|| invalid("Source line is outside the file"))?
            .len()
            + 1;
    }
    let text_line = lines
        .next()
        .ok_or_else(|| invalid("Source line is outside the file"))?
        .trim_end_matches('\r');
    let count = text_line.chars().count();
    let offset = column as usize - 1;
    if offset > count {
        return Err(invalid("Source column is outside the line"));
    }
    let bytes = text_line
        .char_indices()
        .nth(offset)
        .map_or(text_line.len(), |(at, _)| at);
    Ok((start + bytes) as u32)
}

// The request worker below uses the same source snapshot for its model query,
// coverage and proposal guards. It never constructs an editor server.

pub(crate) fn dispatch(
    tool: &str,
    arguments: Value,
    control: RequestControl,
) -> Result<CallToolResult, rmcp::ErrorData> {
    dispatch_query(tool, arguments, control, false)
}

pub(super) fn dispatch_agent(
    tool: &str,
    arguments: Value,
    control: RequestControl,
) -> Result<CallToolResult, rmcp::ErrorData> {
    dispatch_query(tool, arguments, control, true)
}

fn dispatch_query(
    tool: &str,
    arguments: Value,
    mut control: RequestControl,
    agent_output: bool,
) -> Result<CallToolResult, rmcp::ErrorData> {
    let mut decoded_arguments = arguments.clone();
    let diagnostic_ids = if agent_output {
        decoded_arguments
            .as_object_mut()
            .and_then(|object| object.remove("diagnostic_ids"))
    } else {
        None
    };
    let decoder = if agent_output && tool == "dynare_symbol_info" {
        "dynare_find_references"
    } else {
        tool
    };
    let mut params = decode(decoder, &decoded_arguments)?;
    if let Some(ids) = diagnostic_ids {
        params.diagnostic_ids =
            serde_json::from_value(ids).map_err(|error| invalid(error.to_string()))?;
    }
    params.agent_output = agent_output;
    if !params.diagnostic_ids.is_empty() && params.expected_revision.is_none() {
        return Err(invalid(
            "Reusing diagnostic identities requires expected_revision from the original input",
        ));
    }
    control.macro_work_limit = params.macro_work_limit;
    if matches!(
        tool,
        "dynare_explain" | "dynare_list_diagnostic_codes" | "dynare_list_options"
    ) {
        return catalog(tool, &arguments, &params);
    }
    let input = params.source_args.input().map_err(|e| invalid(e.message))?;
    let inspection = super::source_views::inspection(tool, &params)?;
    let acquired = if params.analysis.is_none() && inspection.is_none() {
        super::source::resolve_source(input, params.expected_revision.as_deref(), &control)
    } else {
        super::source::resolve_source_inspecting(
            input,
            params.expected_revision.as_deref(),
            &control,
            params.analysis.as_ref(),
            inspection.as_ref(),
        )
    };
    let mut snapshot = match acquired {
        Ok(snapshot) => snapshot,
        Err(error) if error.invalid_arguments => return Err(invalid(error.message)),
        Err(error) => {
            let mut result = ToolResult::refused(error.code, error.message);
            if matches!(error.code, "SOURCE_LIMIT" | "ROOT_LIMIT") {
                result.limits.push(json!({"code":error.code,"ceilings":control.limits,"affected_work":"source acquisition","omitted":"model analysis and edit proposals"}));
            }
            return Ok(result.wire());
        }
    };
    control
        .check()
        .map_err(|e| rmcp::ErrorData::internal_error(e.message, None))?;
    let mut projected_pages = None;
    let mut result = match tool {
        "dynare_symbol_info" if params.view == Some(super::canonical::View::Source) => {
            super::source_views::symbols(&snapshot, &params)?
        }
        "dynare_find_references" | "dynare_symbol_info" | "dynare_rename" => symbols(
            tool,
            &params,
            &mut snapshot,
            &arguments,
            &mut projected_pages,
        )?,
        "dynare_diagnose" => {
            let diagnostics =
                crate::engine::diagnostics::check_in_workspace_with_origins_controlled(
                    &mut snapshot.workspace,
                    &snapshot.root,
                    &control.cancelled,
                )
                .map_err(|_| rmcp::ErrorData::internal_error("The request was cancelled", None))?;
            let map: HashMap<_, _> = snapshot
                .sources
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let mut rows = serde_json::to_value(super::diagnostics_to_json_with_origins(
                &snapshot.raw,
                &diagnostics,
                &map,
            ))
            .expect("diagnostics JSON");
            if let Some(rows) = rows.as_array_mut() {
                rows.retain(|row| {
                    params.code.as_ref().is_none_or(|code| row["code"] == *code)
                        && params
                            .severity
                            .as_ref()
                            .is_none_or(|severity| row["severity"] == *severity)
                        && params.file.as_ref().is_none_or(|file| {
                            row["file"]
                                .as_str()
                                .unwrap_or(snapshot.root_file.as_deref().unwrap_or(""))
                                == file
                        })
                });
                for row in rows {
                    row["diagnostic_id"] = json!(format!(
                        "{}:diagnostic:{}",
                        snapshot.revision,
                        super::source::sha256(row.to_string())
                    ));
                }
            }
            ToolResult::new(rows)
        }
        "dynare_model_info" => ToolResult::new(model_info(&mut snapshot, &params)?),
        "dynare_equations" if params.view == Some(super::canonical::View::Source) => {
            super::source_views::equations(&snapshot, &params)?
        }
        "dynare_equations" => {
            equation_query(&mut snapshot, &params, &arguments, &mut projected_pages)?
        }
        "dynare_expand" => expand_query(&snapshot, &params, &arguments, &mut projected_pages)?,
        "dynare_related_files" => {
            if snapshot.root_file.is_none() {
                return Err(invalid(
                    "Related files requires map or disk source identity",
                ));
            }
            let includes = snapshot
                .workspace
                .include_records(&snapshot.root)
                .cloned()
                .unwrap_or_default();
            let companions = snapshot
                .workspace
                .companion_records(&snapshot.root)
                .map(|rows| rows.to_vec())
                .unwrap_or_default();
            ToolResult::new(super::related_files_json(&includes, &companions, |path| {
                snapshot
                    .written_file(Some(&path.to_string_lossy()))
                    .unwrap_or_default()
            }))
        }
        "dynare_format" | "dynare_auto_fix" | "dynare_extract" | "dynare_code_actions" => {
            super::edits::edit_request(tool, &params, &mut snapshot)?
        }
        _ => return Err(invalid(format!("Unknown tool: {tool}"))),
    };
    result.inputs = vec![snapshot.inputs.clone()];
    if params.agent_output
        && matches!(tool, "dynare_symbol_info" | "dynare_equations")
        && params.view != Some(super::canonical::View::Source)
        && let Some(data) = result.data.as_object_mut()
    {
        data.insert("view".into(), json!("effective"));
    }
    if matches!(tool, "dynare_expand" | "dynare_extract" | "dynare_format")
        && let Some(data) = result.data.as_object_mut()
    {
        data.insert("analysis".into(), snapshot.inputs["analysis"].clone());
    }
    super::source_views::outcomes(&snapshot, &mut result);
    result.coverage = Some(json!(
        snapshot
            .analyses
            .values()
            .map(|analysis| analysis.coverage.clone())
            .collect::<Vec<_>>()
    ));
    if let Some((root, analysis)) = snapshot
        .analyses
        .iter()
        .find(|(_, analysis)| analysis.coverage["complete"] != true)
        && !matches!(result.outcome.status.as_str(), "refused" | "unchanged")
    {
        let cause = analysis.coverage["reasons"]
            .as_array()
            .and_then(|rows| rows.first())
            .and_then(|reason| reason["message"].as_str())
            .unwrap_or("Parsing, includes or macro expansion did not complete.");
        result.cause("partial","INCOMPLETE_INPUT",format!("{tool} for {root}: {cause} Available source facts are returned; complete model coverage is unavailable."));
    }
    project_and_page(
        tool,
        &arguments,
        &params,
        &snapshot.revision,
        &mut result,
        projected_pages,
    )?;
    let changed = {
        #[cfg(feature = "mcp-reuse")]
        let _measurement = super::retention::measure(super::retention::Phase::Proof);
        snapshot.source_structure.is_some()
            && !snapshot
                .workspace
                .source_guards_are_current(&snapshot.guards)
            || snapshot
                .owner_roots
                .iter()
                .any(|root| !snapshot.workspace.input_snapshot_is_current(root))
    };
    if changed {
        return Ok(ToolResult::refused("INPUT_CHANGED","Source inputs changed before the result was returned. No stale facts or edits were returned.").wire());
    }
    if agent_output {
        result.schema_version = 3;
        if matches!(
            tool,
            "dynare_rename" | "dynare_format" | "dynare_auto_fix" | "dynare_code_actions"
        ) && let Some(object) = result.data.as_object_mut()
        {
            object.remove("text");
            object.remove("files");
            object.remove("formatted_text");
        }
    }
    control.complete_phase("Return model result");
    Ok(result.wire())
}

fn unit(snapshot: &SourceSnapshot) -> super::McpUnit<'_> {
    super::McpUnit {
        model: std::borrow::Cow::Borrowed(&snapshot.model),
        report: std::borrow::Cow::Borrowed(&snapshot.report),
        raw: snapshot.raw.clone(),
        files: snapshot.root_file.as_ref().map(|_| {
            snapshot
                .sources
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        }),
        sources: snapshot
            .sources
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        root_file: snapshot.root_file.clone(),
        revision: snapshot.revision.clone(),
        complete: snapshot.coverage["complete"] == true,
        includes_complete: snapshot.coverage["includes_complete"] == true,
        reasons: Some(snapshot.coverage["reasons"].clone()),
    }
}

pub(crate) fn location(
    snapshot: &SourceSnapshot,
    file: &str,
    span: crate::engine::language::span::Span,
) -> Option<Value> {
    let text = snapshot.text(Some(file))?;
    text.get(span.start as usize..span.end as usize)?;
    Some(json!({"file":snapshot.written_file(Some(file)),"range":snapshot.range(file,span)?}))
}

fn source_json(
    snapshot: &SourceSnapshot,
    source: &crate::engine::workspace::model_map::SourceOccurrence,
) -> Value {
    let segments: Vec<_> = source
        .segments
        .iter()
        .filter_map(|segment| {
            location(
                snapshot,
                segment.file.as_deref().unwrap_or(&snapshot.root),
                segment.span,
            )
        })
        .collect();
    json!({"location":if segments.len()==1 {segments.first().cloned()} else {None},"segments":segments,
        "origin_frames":source.origin_frames.iter().map(|frame|json!({"kind":frame.kind,"variable":frame.variable,"value":frame.value,
            "segments":frame.segments.iter().filter_map(|segment|location(snapshot,segment.file.as_deref().unwrap_or(&snapshot.root),segment.span)).collect::<Vec<_>>() })).collect::<Vec<_>>()})
}

fn model_info(snapshot: &mut SourceSnapshot, params: &Params) -> Result<Value, rmcp::ErrorData> {
    let valid = [
        "analysis",
        "source_structure",
        "summary",
        "declarations",
        "assignments",
        "model_locals",
        "statements",
        "blocks",
        "includes",
        "scope",
        "sources",
        "structure",
    ];
    validate_sections(&params.output.sections, &valid)?;
    let mut data = if params.agent_output {
        json!({})
    } else if snapshot.coverage["complete"] == true {
        super::model_info_with_unit(&unit(snapshot))
    } else {
        json!({"status":"incomplete","incomplete_reasons":snapshot.coverage["reasons"]})
    };
    let want = |name: &str| params.output.sections.iter().any(|section| section == name);
    if want("analysis") {
        data["analysis"] = snapshot.inputs["analysis"].clone();
    }
    if want("source_structure") {
        data["source_structure"] = super::source_views::structure_json(snapshot);
    }
    if params.agent_output && want("structure") && snapshot.coverage["complete"] == true {
        data["structure"] = super::model_info_with_unit(&unit(snapshot));
    }
    if params.agent_output && want("summary") {
        data["summary"] = json!({"complete":snapshot.coverage["complete"],
            "model_kind":if snapshot.model.heterogeneity_dimensions.is_empty(){"aggregate"}else{"heterogeneous"},
            "declaration_count":snapshot.model.written_declarations.len(),
            "statement_count":snapshot.model.statements.len(),
            "equation_count":if snapshot.coverage["complete"]==true{Some(snapshot.report.model_map.equations.iter().filter(|equation|equation.active && !equation.local && !equation.static_only).count())}else{None},
            "dimension_count":snapshot.model.heterogeneity_dimensions.len(),
            "assignment_count":snapshot.model.param_assignments.len()+snapshot.model.helper_assignments.len()});
    }
    if params.agent_output && want("model_locals") && snapshot.coverage["complete"] == true {
        data["model_locals"] = super::model_info_with_unit(&unit(snapshot))["model_locals"].clone();
    }
    if want("declarations") {
        data["declarations"] = json!(
            snapshot
                .model
                .written_declarations
                .iter()
                .zip(&snapshot.report.model_map.declarations)
                .map(|(written, source)| {
                    let declaration = &written.declaration;
                    let mut row = source_json(snapshot, source);
                    row["name"] = json!(snapshot.model.name(declaration.name));
                    row["written_kind"] = json!(written.written_kind);
                    row["final_kind"] = if snapshot.coverage["complete"] == true {
                        json!(snapshot.model.final_symbol_kind(declaration.name))
                    } else {
                        Value::Null
                    };
                    row["long_name"] = json!(declaration.long_name);
                    row["tex_name"] = json!(declaration.tex_name);
                    row["dimension"] = json!(
                        declaration
                            .heterogeneity
                            .map(|(name, _)| snapshot.model.name(name))
                    );
                    row
                })
                .collect::<Vec<_>>()
        );
    }
    if want("assignments") {
        let values = crate::engine::analysis::assignment_values::assignment_values(&snapshot.model);
        data["assignments"] = json!(values.iter().filter_map(|proven| {
            let statement = &snapshot.model.statements[proven.statement_id];
            let assignment = match statement.assignment? {
                crate::engine::language::model::AssignmentIndex::Parameter(index) => &snapshot.model.param_assignments[index],
                crate::engine::language::model::AssignmentIndex::Helper(index) => &snapshot.model.helper_assignments[index],
            };
            let source = snapshot.report.model_map.statements.get(proven.statement_id);
            Some(json!({"name":snapshot.model.name(assignment.name),"expression":assignment.expression,"value":proven.value,
                "statement_id":proven.statement_id,"provenance":"value at this assignment's execution position",
                "source":source.map(|source|source_json(snapshot,source))}))
        }).collect::<Vec<_>>());
    }
    if want("statements") || want("blocks") {
        for section in ["statements", "blocks"] {
            if want(section) {
                data[section] = json!(snapshot.model.statements.iter().zip(&snapshot.report.model_map.statements)
                .filter(|(statement,_)|section=="statements"||statement.kind==crate::engine::language::model::StatementKind::Block)
                .map(|(statement,source)|json!({"id":statement.id,"kind":statement.kind.as_str(),"name":statement.name,
                    "complete":statement.complete,"native":statement.native,"dimension":statement.dimension,"source":source_json(snapshot,source)})).collect::<Vec<_>>());
            }
        }
    }
    if want("includes") {
        let includes = snapshot
            .workspace
            .include_records(&snapshot.root)
            .cloned()
            .unwrap_or_default();
        data["includes"] = super::related_files_json(&includes, &[], |path| {
            snapshot
                .written_file(Some(&path.to_string_lossy()))
                .unwrap_or_default()
        });
    }
    if want("sources") {
        if snapshot.root_file.is_none() {
            data["sources"] = json!({"available":false,"cause":"Unnamed text has no file identity. Supply a map or saved disk root to discover includes and companions."});
        } else {
            let includes = snapshot
                .workspace
                .include_records(&snapshot.root)
                .cloned()
                .unwrap_or_default();
            let companions = snapshot
                .workspace
                .companion_records(&snapshot.root)
                .map(|rows| rows.to_vec())
                .unwrap_or_default();
            data["sources"] = super::related_files_json(&includes, &companions, |path| {
                snapshot
                    .written_file(Some(&path.to_string_lossy()))
                    .unwrap_or_default()
            });
            for row in data["sources"].as_array_mut().into_iter().flatten() {
                if row["resolved"] == false {
                    row["cause"] = json!(if snapshot.inputs["source_policy"] == "map" {
                        "No supplied file resolves this relationship under the selected source lookup rules."
                    } else {
                        "No available file resolves this relationship under the selected source lookup rules."
                    });
                }
            }
        }
    }
    if want("scope") {
        data["scope"] = json!({"root":snapshot.root_file,"checked_owners":snapshot.inputs["checked_owners"],"dimensions":snapshot.model.heterogeneity_dimensions.iter().map(|d|snapshot.model.name(d.name)).collect::<Vec<_>>()});
    }
    if !params.output.sections.is_empty() && !want("summary") {
        data.as_object_mut()
            .expect("model info object")
            .retain(|key, _| {
                params.output.sections.contains(key)
                    || matches!(key.as_str(), "status" | "incomplete_reasons")
            });
    }
    Ok(data)
}

fn validate_sections(sections: &[String], valid: &[&str]) -> Result<(), rmcp::ErrorData> {
    if let Some(section) = sections
        .iter()
        .find(|section| !valid.contains(&section.as_str()))
    {
        return Err(invalid(format!(
            "Unknown section {section}; available sections: {}",
            valid.join(", ")
        )));
    }
    Ok(())
}

fn line_bytes(text: &str, range: &LineRange) -> Result<(usize, usize), rmcp::ErrorData> {
    if range.start_line == 0
        || range.end_line < range.start_line
        || range.end_line as usize > text.split('\n').count()
    {
        return Err(invalid(
            "line_range must be ordered, one-based and inside the selected file",
        ));
    }
    let starts: Vec<_> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(at, _)| at + 1))
        .collect();
    let start = starts[range.start_line as usize - 1];
    let end = starts
        .get(range.end_line as usize)
        .copied()
        .unwrap_or(text.len());
    Ok((start, end))
}

fn expand_query(
    snapshot: &SourceSnapshot,
    params: &Params,
    arguments: &Value,
    projected: &mut Option<Pages>,
) -> Result<ToolResult, rmcp::ErrorData> {
    use crate::engine::language::span::{LineIndex, Span};
    use crate::engine::preview::preview_navigation::{
        NAVIGATION_SCHEMA_VERSION, navigation_row_json,
    };
    if [
        params.line_range.is_some(),
        params.expanded_range.is_some(),
        params.equation_id.is_some(),
    ]
    .into_iter()
    .filter(|set| *set)
    .count()
        > 1
    {
        return Err(invalid(
            "Choose one line_range, expanded_range or equation_id",
        ));
    }
    let full = &snapshot.report.effective_text;
    let mut selected_occurrence = None;
    let selection = if let Some(range) = &params.line_range {
        let (start, end) = line_bytes(full, range)?;
        Some(Span::new(start, end))
    } else if let Some(range) = &params.expanded_range {
        let start = byte_position(full, range.line, range.column)?;
        let end = byte_position(full, range.end_line, range.end_column)?;
        if end < start {
            return Err(invalid(
                "expanded_range must have an exclusive end at or after its start",
            ));
        }
        Some(Span { start, end })
    } else if let Some(id) = &params.equation_id {
        let selected = selected_equation(snapshot, id)?;
        let fact = &snapshot.report.model_map.equations[selected];
        let Some(row) = snapshot
            .report
            .navigation
            .iter()
            .find(|row| row.equation.id == fact.id)
        else {
            return Ok(ToolResult::refused(
                "UNVERIFIED_EXPANDED_RANGE",
                "The selected occurrence has no verified range in the available expanded text.",
            ));
        };
        selected_occurrence =
            Some(json!({"equation_id":id,"source":source_json(snapshot,&fact.source)}));
        Some(row.effective_span)
    } else {
        None
    };
    let span = selection.unwrap_or(Span::new(0, full.len()));
    let text = full
        .get(span.start as usize..span.end as usize)
        .ok_or_else(|| invalid("Expanded selection is outside the available text"))?;
    let Ok(mut pages) = prepare_pages("dynare_expand", arguments, params, &snapshot.revision)
    else {
        // The final paging step returns the established error envelope.
        return Ok(ToolResult::new(Value::Null));
    };
    let complete = snapshot.coverage["complete"] == true;
    let collection_complete = snapshot
        .analyses
        .values()
        .all(|analysis| analysis.coverage["complete"] == true);
    let has_heterogeneous = !snapshot.report.heterogeneous_origins.is_empty();
    let mut selected_rows = Vec::new();
    let mut origin_keys = HashSet::new();
    let mut omitted = 0usize;
    for row in &snapshot.report.navigation {
        if crate::engine::control::cancelled() {
            break;
        }
        let range = row.effective_span;
        if selection.is_some() && (range.end <= span.start || range.start >= span.end) {
            continue;
        }
        if selection.is_some() && (range.start < span.start || range.end > span.end) {
            omitted += 1;
            continue;
        }
        if selection.is_some()
            && let Some(number) = row.equation.number
        {
            origin_keys.insert((row.equation.dimension.as_deref(), number));
        }
        if complete {
            selected_rows.push(row);
        }
    }
    let mut origins = Vec::new();
    let mut dimension_counts = HashMap::new();
    for origin in &snapshot.report.origins {
        if crate::engine::control::cancelled() {
            break;
        }
        *dimension_counts
            .entry(origin.dimension.as_deref())
            .or_insert(0usize) += 1;
        if selection.is_none()
            || origin_keys.contains(&(origin.dimension.as_deref(), origin.scope_index + 1))
        {
            origins.push(origin);
        }
    }
    let mut data = json!({
        "effective_text":text,
        "n_equations":if snapshot.coverage["includes_complete"] == true { snapshot.report.n_equations } else { 0 },
        "navigation_schema_version":NAVIGATION_SCHEMA_VERSION,
        "root_file":snapshot.root_file,"revision":snapshot.revision,"complete":complete,
    });
    if has_heterogeneous {
        data["n_aggregate_equations"] = json!(snapshot.report.aggregate_origins.len());
        data["n_heterogeneous_equations"] =
            json!(snapshot.report.n_equations - snapshot.report.aggregate_origins.len());
        let dimensions =
            crate::engine::analysis::model_info::heterogeneous_dimension_names(&snapshot.model);
        let range = pages.projection_range(
            "heterogeneity_dimensions",
            dimensions.len(),
            collection_complete,
        );
        data["heterogeneity_dimensions"] = json!(dimensions[range].iter().map(|dimension| {
            let name = snapshot.model.name(*dimension);
            json!({"dimension":name,"n_equations":dimension_counts.get(&Some(name)).copied().unwrap_or(0)})
        }).collect::<Vec<_>>());
    }
    if !complete {
        data["status"] = json!("incomplete");
        data["message"] =
            json!("Expansion is partial; equation counts and source navigation are unavailable.");
        let reasons = snapshot.coverage["reasons"]
            .as_array()
            .expect("incomplete reasons");
        let range =
            pages.projection_range("incomplete_reasons", reasons.len(), collection_complete);
        data["incomplete_reasons"] = json!(&reasons[range]);
    }
    let range = pages.projection_range(
        "macro_messages",
        snapshot.report.macro_messages.len(),
        collection_complete,
    );
    data["macro_messages"] = json!(
        snapshot.report.macro_messages[range]
            .iter()
            .map(|message| {
                let file = message
                    .file
                    .as_deref()
                    .filter(|file| snapshot.sources.contains_key(*file));
                let mut value = json!({"kind":message.kind,"message":message.message,
            "location":snapshot_range(snapshot,file,message.span)});
                if let Some(file) = &message.file {
                    value["file"] = json!(file);
                }
                value
            })
            .collect::<Vec<_>>()
    );
    let index = LineIndex::new(text);
    let mut normalized = BTreeMap::<String, Option<(String, LineIndex)>>::new();
    let range = pages.projection_range("navigation", selected_rows.len(), collection_complete);
    data["navigation"] = json!(
        selected_rows[range]
            .iter()
            .map(|row| {
                let mut value = navigation_row_json(
                    row,
                    |range| {
                        indexed_range(
                            &index,
                            text,
                            Span {
                                start: range.start - span.start,
                                end: range.end - span.start,
                            },
                        )
                    },
                    |segment| {
                        let original = snapshot.text(segment.file.as_deref())?;
                        if segment.span.is_empty() {
                            return None;
                        }
                        original.get(segment.span.start as usize..segment.span.end as usize)?;
                        let key = segment.file.as_deref().unwrap_or(&snapshot.root);
                        let normalized = normalized.entry(key.to_owned()).or_insert_with(|| {
                            original.contains('\r').then(|| {
                                let text =
                                    crate::engine::language::parser::normalize_newlines(original);
                                let index = LineIndex::new(&text);
                                (text, index)
                            })
                        });
                        let range = if let Some((text, index)) = normalized {
                            indexed_range(index, text, segment.span)
                        } else {
                            snapshot_range(snapshot, segment.file.as_deref(), segment.span)
                        };
                        let mut location = json!({"range":range});
                        if let Some(file) = mapped_file(snapshot, segment.file.as_deref()) {
                            location["file"] = json!(file);
                        } else if segment.file.is_some() {
                            return None;
                        }
                        Some(location)
                    },
                );
                if selection.is_some() {
                    value["equation_id"] = json!(equation_id(snapshot, &row.equation));
                }
                value
            })
            .collect::<Vec<_>>()
    );
    let range = pages.projection_range("origins", origins.len(), collection_complete);
    data["origins"] = json!(
        origins[range]
            .iter()
            .map(|origin| {
                let mut value = json!({"index":origin.index});
                if has_heterogeneous {
                    value["scope_index"] = json!(origin.scope_index);
                    value["scope"] = json!(if origin.dimension.is_some() {
                        "heterogeneous"
                    } else {
                        "aggregate"
                    });
                    if let Some(dimension) = &origin.dimension {
                        value["dimension"] = json!(dimension);
                        value["block_index"] = json!(origin.block_index);
                    }
                }
                attach_snapshot_origin(snapshot, &mut value, origin);
                value
            })
            .collect::<Vec<_>>()
    );
    if let Some(selection) = selection {
        data["selected_expanded_range"] = super::range_json(selection, full);
    }
    if let Some(occurrence) = selected_occurrence {
        data["selected_occurrence"] = occurrence;
    }
    let mut result = ToolResult::new(data);
    if omitted > 0 {
        result.data["omitted_navigation_count"] = json!(omitted);
        result.cause("partial","PARTIAL_ROW_ORIGINS",format!("The selected text is returned. Written navigation for {omitted} partly selected rows was withheld because a complete row range is required. Select their equation identity to inspect their verified origins."));
    }
    *projected = Some(pages);
    Ok(result)
}

fn indexed_range(
    index: &crate::engine::language::span::LineIndex,
    text: &str,
    span: crate::engine::language::span::Span,
) -> Value {
    let start = index.position(text, span.start);
    let end = index.position(text, span.end);
    json!({"line":start.line+1,"column":start.character+1,"end_line":end.line+1,"end_column":end.character+1})
}

fn snapshot_range(
    snapshot: &SourceSnapshot,
    file: Option<&str>,
    span: crate::engine::language::span::Span,
) -> Value {
    let file = file.unwrap_or(&snapshot.root);
    let text = &snapshot.sources[file];
    indexed_range(&snapshot.line_indexes[file], text, span)
}

fn mapped_file(snapshot: &SourceSnapshot, file: Option<&str>) -> Option<String> {
    snapshot.root_file.as_ref()?;
    file.map(str::to_owned)
}

fn attach_snapshot_origin(
    snapshot: &SourceSnapshot,
    value: &mut Value,
    origin: &crate::engine::language::expand::EquationOrigin,
) {
    let file = origin.origin_uri.as_deref();
    if snapshot.text(file).is_none() {
        return;
    }
    value["origin"] = snapshot_range(snapshot, file, origin.origin_span);
    if let Some(file) = mapped_file(snapshot, file) {
        value["origin_uri"] = json!(file);
    }
    if !super::publish_origin_frames(&origin.origin_frames) {
        return;
    }
    let frames = origin
        .origin_frames
        .iter()
        .filter_map(|frame| {
            snapshot.text(frame.origin_uri.as_deref())?;
            let mut value =
                snapshot_range(snapshot, frame.origin_uri.as_deref(), frame.origin_span);
            value["kind"] = json!(frame.kind);
            if let Some(variable) = &frame.variable {
                value["variable"] = json!(variable);
            }
            if let Some(value_) = &frame.value {
                value["value"] = json!(value_);
            }
            if let Some(file) = mapped_file(snapshot, frame.origin_uri.as_deref()) {
                value["origin_uri"] = json!(file);
            }
            Some(value)
        })
        .collect::<Vec<_>>();
    if super::publish_origin_frames_json(&frames) {
        value["origin_frames"] = json!(frames);
    }
}

pub(crate) fn selected_equation(
    snapshot: &SourceSnapshot,
    id: &str,
) -> Result<usize, rmcp::ErrorData> {
    if !id.starts_with(&format!("{}:equation:", snapshot.revision)) {
        return Err(invalid(
            "equation_id must be an accepted effective equation identity from this revision. Written source_id values select no effective equation; analyze a configuration that emits the template, then query effective equations.",
        ));
    }
    snapshot
        .report
        .model_map
        .equations
        .iter()
        .enumerate()
        .find_map(|(index, row)| (equation_id(snapshot, row) == id).then_some(index))
        .ok_or_else(|| invalid("equation_id does not belong to this input revision"))
}

pub(crate) fn equation_id(
    snapshot: &SourceSnapshot,
    row: &crate::engine::workspace::model_map::EquationOccurrence,
) -> String {
    format!(
        "{}:equation:{}:{}:{}",
        snapshot.revision,
        row.dimension.as_deref().unwrap_or("aggregate"),
        if row.local {
            "local"
        } else if row.static_only {
            "static"
        } else {
            "counted"
        },
        row.id
    )
}

fn equation_query(
    snapshot: &mut SourceSnapshot,
    params: &Params,
    arguments: &Value,
    projected: &mut Option<Pages>,
) -> Result<ToolResult, rmcp::ErrorData> {
    if params.index.is_some() && params.equation.is_some() {
        return Err(invalid("Use index or equation, not both"));
    }
    if params
        .scope
        .as_deref()
        .is_some_and(|scope| !matches!(scope, "aggregate" | "dimension"))
    {
        return Err(invalid("scope must be aggregate or dimension"));
    }
    if params.scope.as_deref() == Some("dimension") && params.dimension.is_none() {
        return Err(invalid("Dimension scope requires dimension"));
    }
    let selected_id = params
        .equation_id
        .as_ref()
        .map(|id| selected_equation(snapshot, id))
        .transpose()?;
    let symbol = if params.symbol.is_some()
        || params.target.is_some()
        || params.binding_id.is_some()
    {
        let (model, selected) = semantic_selection(snapshot, params, false)?;
        match selected {
            Ok(Some(binding)) => Some((model, binding)),
            Ok(None) => {
                let mut data = super::equations_with_unit(&unit(snapshot), None, None);
                data["equations"] = json!([]);
                data["heterogeneous_equations"] = json!([]);
                return Ok(ToolResult::new(data));
            }
            Err(error) => {
                let mut result = ToolResult::refused(error.reason.code, error.reason.message);
                result.data = json!({"candidates":error.candidates.iter().map(|candidate| binding_json(snapshot,candidate)).collect::<Vec<_>>()});
                return Ok(result);
            }
        }
    } else {
        None
    };
    if snapshot.coverage["complete"] != true {
        return Ok(ToolResult::new(super::equations_with_unit(
            &unit(snapshot),
            None,
            None,
        )));
    }
    let Ok(mut pages) = prepare_pages("dynare_equations", arguments, params, &snapshot.revision)
    else {
        return Ok(ToolResult::new(Value::Null));
    };
    let complete = snapshot
        .analyses
        .values()
        .all(|analysis| analysis.coverage["complete"] == true);
    let (rows, heterogeneous) =
        crate::engine::analysis::equations::equation_collections(&snapshot.model);
    let gap = crate::engine::analysis::equations::count_gap_with_rows(&snapshot.model, &rows);
    let mut gap = super::count_gap_json(&gap);
    if params.agent_output {
        let names = gap["unreferenced_endogenous"]
            .as_array_mut()
            .expect("count gap names");
        let range =
            pages.projection_range("count_gap.unreferenced_endogenous", names.len(), complete);
        *names = names.drain(range).collect();
    }
    // The shared map supplies the accepted equation identity once, rather than
    // rescanning it for each projected row.
    let mut facts = HashMap::new();
    for (index, fact) in snapshot.report.model_map.equations.iter().enumerate() {
        if crate::engine::control::cancelled() {
            break;
        }
        if fact.active
            && !fact.local
            && !fact.static_only
            && let Some(counted_index) = fact.number.and_then(|number| number.checked_sub(1))
        {
            facts
                .entry((fact.dimension.as_deref(), counted_index))
                .or_insert((index, fact));
        }
    }
    let uses = symbol
        .as_ref()
        .map(|(model, binding)| model.references(binding, false))
        .unwrap_or_default();
    let matches = |row: &crate::engine::analysis::equations::EquationRow,
                   dimension: Option<&str>| {
        let Some((fact_index, _)) = facts.get(&(dimension, row.index)) else {
            return false;
        };
        !(params.name.as_ref().is_some_and(|name| row.name != *name)
            || params
                .index
                .is_some_and(|want| dimension.is_some() || want != row.index)
            || params
                .dimension
                .as_deref()
                .is_some_and(|want| dimension != Some(want))
            || params.scope.as_deref() == Some("aggregate") && dimension.is_some()
            || params.scope.as_deref() == Some("dimension") && dimension.is_none()
            || params.equation.as_ref().is_some_and(|want| {
                want.index != row.index
                    || want.dimension.as_deref() != dimension
                    || (want.scope == "aggregate") != dimension.is_none()
            })
            || selected_id.is_some_and(|want| want != *fact_index)
            || params
                .tags
                .iter()
                .any(|(tag, value)| row.tags.get(tag) != Some(value))
            || symbol.is_some()
                && !uses.iter().any(|use_| {
                    snapshot.model.written_equations[*fact_index]
                        .equation
                        .active_tokens
                        .contains(&use_.order)
                }))
    };
    let serialize =
        |row: &crate::engine::analysis::equations::EquationRow,
         dimension: Option<&str>,
         origin: Option<&crate::engine::language::expand::EquationOrigin>| {
            let (_, fact) = facts[&(dimension, row.index)];
            let mut value = super::equation_facts_json(row, false);
            if let Some(origin) = origin {
                attach_snapshot_origin(snapshot, &mut value, origin);
            }
            value["equation_id"] = json!(equation_id(snapshot, fact));
            value["scope"] = json!(if dimension.is_some() {
                "dimension"
            } else {
                "aggregate"
            });
            value["dimension"] = json!(dimension);
            value["index_base"] = json!(0);
            value
        };
    let selected = rows
        .iter()
        .take_while(|_| !crate::engine::control::cancelled())
        .filter(|row| matches(row, None))
        .collect::<Vec<_>>();
    let range = pages.projection_range("equations", selected.len(), complete);
    let aggregate = selected[range]
        .iter()
        .map(|row| {
            let origin = snapshot
                .report
                .aggregate_origins
                .get(row.index)
                .filter(|origin| origin.scope_index == row.index);
            serialize(row, None, origin)
        })
        .collect::<Vec<_>>();
    let mut data = json!({"count_gap":gap,"equations":aggregate});
    if !heterogeneous.is_empty() {
        let mut blocks = Vec::new();
        for block in &heterogeneous {
            if crate::engine::control::cancelled() {
                break;
            }
            let selected = block
                .equations
                .iter()
                .enumerate()
                .filter(|(_, row)| matches(row, Some(&block.dimension)))
                .collect::<Vec<_>>();
            if selected.is_empty() {
                continue;
            }
            let collection = if params.agent_output {
                format!("heterogeneous_equations:{}.equations", block.dimension)
            } else {
                format!("equations:{}", block.dimension)
            };
            let range = pages.projection_range(&collection, selected.len(), complete);
            let rows = selected[range]
                .iter()
                .map(|(local_index, row)| {
                    let origin = snapshot
                        .report
                        .heterogeneous_origins
                        .get(block.block_index)
                        .and_then(|origins| origins.get(*local_index));
                    serialize(row, Some(&block.dimension), origin)
                })
                .collect::<Vec<_>>();
            blocks.push(json!({"block_index":block.block_index,"dimension":block.dimension,"equations":rows}));
        }
        data["heterogeneous_equations"] = json!(blocks);
    }
    *projected = Some(pages);
    Ok(ToolResult::new(data))
}

type SemanticModel = crate::engine::analysis::symbols::SemanticModel;
type Binding = crate::engine::analysis::symbols::Binding;

fn semantic_selection(
    snapshot: &mut SourceSnapshot,
    params: &Params,
    rename: bool,
) -> Result<
    (
        SemanticModel,
        Result<Option<Binding>, crate::engine::analysis::symbols::SelectionError>,
    ),
    rmcp::ErrorData,
> {
    use crate::engine::analysis::symbols::Selector;
    let name = if rename {
        params.old_name.as_deref()
    } else {
        params.symbol.as_deref()
    };
    if [
        name.is_some(),
        params.target.is_some(),
        params.binding_id.is_some(),
    ]
    .into_iter()
    .filter(|yes| *yes)
    .count()
        != 1
    {
        return Err(invalid(
            "Select exactly one symbol name, target or binding_id",
        ));
    }
    let model = SemanticModel::capture(
        &mut snapshot.workspace,
        &snapshot.root,
        snapshot.revision.clone(),
    )
    .map_err(|reason| rmcp::ErrorData::internal_error(reason.message, None))?;
    let selected = if let Some(name) = name {
        model.select(Selector::Name(name))
    } else if let Some(id) = &params.binding_id {
        model.select(Selector::Id(id))
    } else {
        let target = params.target.as_ref().expect("validated target");
        let file = target.file.as_deref().unwrap_or(&snapshot.root);
        if !model.sources.contains_key(file) {
            return Err(invalid("target.file must belong to the selected model"));
        }
        let text = snapshot
            .text(Some(file))
            .ok_or_else(|| invalid("target.file is unavailable"))?;
        let byte = byte_position(text, target.line, target.column)?;
        model.select(Selector::Target { file, byte })
    };
    Ok((model, selected))
}

fn binding_json(snapshot: &SourceSnapshot, binding: &Binding) -> Value {
    json!({"binding_id":binding.id,"name":binding.name,"kind":binding.kind,
        "namespace":binding.namespace,
        "scope":if binding.dimension.is_some() {"dimension"} else {"aggregate"},"dimension":binding.dimension,
        "declaration":binding.declaration.as_ref().and_then(|(file,span)|location(snapshot,file,*span)),
        "definition":binding.definition.as_ref().and_then(|(file,span)|location(snapshot,file,*span))})
}

fn symbols(
    tool: &str,
    params: &Params,
    snapshot: &mut SourceSnapshot,
    arguments: &Value,
    projected: &mut Option<Pages>,
) -> Result<ToolResult, rmcp::ErrorData> {
    let rename = tool == "dynare_rename";
    if rename && snapshot.model.is_analysis_preview() {
        return Ok(ToolResult::refused(
            "ANALYSIS_PREVIEW_EDIT_UNSUPPORTED",
            snapshot
                .workspace
                .root_analysis_configuration(&snapshot.root)
                .preview_edit_cause("Rename"),
        ));
    }
    if rename
        && params
            .new_name
            .as_deref()
            .is_none_or(|name| !crate::engine::editing::refs::is_legal_ident(name))
    {
        return Err(invalid(
            "new_name must be a legal, non-reserved Dynare identifier",
        ));
    }
    let (model, selected) = semantic_selection(snapshot, params, rename)?;
    let binding = match selected {
        Ok(Some(binding)) => binding,
        Ok(None) => {
            let mut result = ToolResult::new(if rename {
                json!({"text":snapshot.raw,"files":{}})
            } else {
                json!({"references":[],"occurrences":[],"symbol":null})
            });
            if rename {
                result.cause(
                    "unchanged",
                    "NO_MATCH",
                    format!("No binding for {} in {} under the selected configuration. No Rename edits were proposed.", params.old_name.as_deref().or(params.symbol.as_deref()).or(params.binding_id.as_deref()).unwrap_or("the selected source position"), snapshot.root_file.as_deref().unwrap_or("unnamed model")),
                );
            } else if tool == "dynare_symbol_info" {
                result.cause(if snapshot.coverage["complete"]==true{"unchanged"}else{"partial"}, "BINDING_UNAVAILABLE", format!("No binding for {} in {} under the selected configuration.{} Use view=source to inspect source declarations; they do not establish references.",params.symbol.as_deref().or(params.binding_id.as_deref()).unwrap_or("the selected source position"), snapshot.root_file.as_deref().unwrap_or("unnamed model"), if snapshot.coverage["complete"]==true{""}else{" Model coverage is incomplete; a missing binding is not proof of absence."}));
                result.data["source_view"] = json!({"tool":"dynare_symbol_info","view":"source"});
            }
            return Ok(result);
        }
        Err(error) => {
            let mut result = ToolResult::refused(error.reason.code, error.reason.message);
            result.data = json!({"candidates":error.candidates.iter().map(|candidate|binding_json(snapshot,candidate)).collect::<Vec<_>>()});
            return Ok(result);
        }
    };
    let complete = snapshot
        .analyses
        .values()
        .all(|analysis| analysis.coverage["complete"] == true);
    let mut pages = if rename {
        None
    } else {
        let Ok(pages) = prepare_pages(tool, arguments, params, &snapshot.revision) else {
            return Ok(ToolResult::new(Value::Null));
        };
        Some(pages)
    };
    let want = |section: &str| {
        !params.agent_output
            || params
                .output
                .sections
                .iter()
                .any(|selected| selected == section)
    };
    let mut metadata = None;
    if tool == "dynare_symbol_info"
        && (!params.agent_output
            || !params
                .output
                .sections
                .iter()
                .all(|section| section == "references"))
    {
        let mut data = binding_json(snapshot, &binding);
        if want("metadata") {
            let declaration = model
                .model
                .written_declarations
                .iter()
                .zip(&snapshot.report.model_map.declarations)
                .find(|(decl, source)| {
                    (binding.is_ordinary()
                        || matches!(
                            binding.kind.as_str(),
                            "trend_var" | "log_trend_var" | "external_function"
                        ))
                        && model.model.name(decl.declaration.name) == binding.name
                        && binding.declaration.as_ref().is_some_and(|(file, span)| {
                            source.segments.iter().any(|segment| {
                                segment.file.as_deref().unwrap_or(&snapshot.root) == file
                                    && segment.span.start <= span.start
                                    && span.end <= segment.span.end
                            })
                        })
                })
                .map(|(decl, _)| decl);
            data["long_name"] =
                json!(declaration.and_then(|decl| decl.declaration.long_name.as_deref()));
            data["tex_name"] =
                json!(declaration.and_then(|decl| decl.declaration.tex_name.as_deref()));
            data["written_kind"] = json!(declaration.map(|decl| decl.written_kind.as_str()));
            data["timing"] = if !binding.is_ordinary() {
                Value::Null
            } else {
                let timing =
                    crate::engine::analysis::timing::classify_variable_timing(&model.model);
                timing
                    .get(&binding.name)
                    .map(|fact| json!({"class":fact.class.label(),"offsets":fact.offsets}))
                    .unwrap_or(Value::Null)
            };
            if binding.is_local() {
                data["expression"] = json!(model.local_expression(&binding));
            }
        }
        // Assignment traces keep their execution positions. A trace value is
        // not a claim that later native code retained that final value.
        if want("assignments") {
            let values = if binding.is_ordinary() {
                crate::engine::analysis::assignment_values::assignment_values(&model.model)
            } else {
                Vec::new()
            };
            let values: Vec<_> = values
                .iter()
                .filter_map(|value| {
                    let statement = &model.model.statements[value.statement_id];
                    let assignment = match statement.assignment? {
                        crate::engine::language::model::AssignmentIndex::Parameter(index) => {
                            &model.model.param_assignments[index]
                        }
                        crate::engine::language::model::AssignmentIndex::Helper(index) => {
                            &model.model.helper_assignments[index]
                        }
                    };
                    (model.model.name(assignment.name) == binding.name)
                        .then_some((value, assignment))
                })
                .collect();
            let range = if params.agent_output {
                pages.as_mut().unwrap().projection_range(
                    "binding.assignment_values",
                    values.len(),
                    complete,
                )
            } else {
                0..values.len()
            };
            data["assignment_values"] = json!(values[range].iter().map(|(value, assignment)|json!({"value":value.value,
            "statement_id":value.statement_id,"expression":assignment.expression,"provenance":"assignment execution position"})).collect::<Vec<_>>());
        }
        // Preserve the generic collection order: assignment values, timing
        // offsets, expanded occurrences, then written references.
        if params.agent_output
            && let Some(offsets) = data
                .pointer_mut("/timing/offsets")
                .and_then(Value::as_array_mut)
        {
            let range = pages.as_mut().unwrap().projection_range(
                "binding.timing.offsets",
                offsets.len(),
                complete,
            );
            *offsets = offsets.drain(range).collect();
        }
        if !params.agent_output {
            return Ok(ToolResult::new(data));
        }
        metadata = Some(data);
    }
    if rename {
        let replacement = params.new_name.as_deref().expect("validated replacement");
        if replacement == binding.name {
            let mut result = ToolResult::new(json!({"text":snapshot.raw,"files":{}}));
            result.cause(
                "unchanged",
                "SAME_NAME",
                "The requested name is already the current name.",
            );
            return Ok(result);
        }
        let mut owners = vec![model];
        for owner in snapshot
            .owner_roots
            .iter()
            .filter(|owner| *owner != &snapshot.root)
        {
            match SemanticModel::capture(&mut snapshot.workspace, owner, snapshot.revision.clone())
            {
                Ok(model) => owners.push(model),
                Err(reason) => return Ok(ToolResult::refused(reason.code, reason.message)),
            }
        }
        let proposal =
            match crate::engine::analysis::symbols::rename(&owners, 0, &binding, replacement) {
                Ok(proposal) => proposal,
                Err(reason) => return Ok(ToolResult::refused(reason.code, reason.message)),
            };
        let affected = proposal.roots.clone();
        let (data, proposal) = proposal_result(snapshot, &proposal.edits)?;
        let mut result = ToolResult::new(data);
        result.data["affected_owners"] = json!(affected);
        result.outcome.status = "changed".into();
        result.proposal = Some(proposal);
        return Ok(result);
    }
    let occurrences = model.references(&binding, params.include_declaration.unwrap_or(true));
    let references = want("references");
    let mut locations = BTreeMap::<(&str, u32, u32), usize>::new();
    let mut verified = Vec::with_capacity(occurrences.len());
    let mut omitted = 0;
    // Coverage still checks every accepted use, including definition-only
    // queries. Coordinate conversion and JSON are limited to selected rows.
    for (index, occurrence) in occurrences.iter().enumerate() {
        if crate::engine::control::cancelled() {
            return Err(rmcp::ErrorData::internal_error(
                "The request was cancelled",
                None,
            ));
        }
        let written = occurrence.written.as_ref().filter(|(file, span)| {
            snapshot
                .text(Some(file))
                .and_then(|text| text.get(span.start as usize..span.end as usize))
                .is_some()
                && snapshot.line_indexes.contains_key(file)
        });
        verified.push(written.is_some());
        if let Some((file, span)) = written {
            if references {
                locations
                    .entry((file.as_str(), span.start, span.end))
                    .or_insert(index);
            }
        } else {
            omitted += 1;
        }
    }
    let mut result = ToolResult::new(
        json!({"binding": metadata.unwrap_or_else(|| binding_json(snapshot, &binding))}),
    );
    let page_state = pages.as_mut().expect("read-only symbol pages");
    if params.occurrences.unwrap_or(false) {
        let range = page_state.projection_range("occurrences", occurrences.len(), complete);
        let expanded_index = (!range.is_empty()).then(|| LineIndex::new(&model.model.source));
        result.data["occurrences"] = json!(range.map(|index| {
            #[cfg(test)]
            SYMBOL_OCCURRENCE_PROJECTIONS.with(|count| count.set(count.get() + 1));
            let occurrence = &occurrences[index];
            let expanded_index = expanded_index.as_ref().unwrap();
            let start = expanded_index.position(&model.model.source, occurrence.span.start);
            let end = expanded_index.position(&model.model.source, occurrence.span.end);
            let written = verified[index].then(|| {
                let (file, span) = occurrence.written.as_ref().unwrap();
                location(snapshot, file, *span).unwrap()
            });
            json!({"occurrence_id":format!("{}:use:{}:{}",snapshot.revision,occurrence.order,occurrence.role),
                "expanded_order":occurrence.order,"expanded_range":{
                    "line":start.line+1,"column":start.character+1,"end_line":end.line+1,"end_column":end.character+1
                },"role":occurrence.role,"dimension":occurrence.dimension,"written":written})
        }).collect::<Vec<_>>());
    }
    if references {
        let range = page_state.projection_range("references", locations.len(), complete);
        result.data["references"] = json!(
            locations
                .values()
                .skip(range.start)
                .take(range.len())
                .map(|index| {
                    #[cfg(test)]
                    SYMBOL_REFERENCE_PROJECTIONS.with(|count| count.set(count.get() + 1));
                    let occurrence = &occurrences[*index];
                    let (file, span) = occurrence.written.as_ref().unwrap();
                    let mut row = location(snapshot, file, *span).unwrap();
                    row["role"] = json!(occurrence.role);
                    row["dimension"] = json!(occurrence.dimension);
                    row["declaration"] = json!(occurrence.declaration);
                    row
                })
                .collect::<Vec<_>>()
        );
    }
    if omitted > 0 {
        result.cause("partial","UNVERIFIED_TARGET",format!("{omitted} accepted occurrences have no verified written location. Their source targets were omitted."));
    }
    *projected = pages;
    Ok(result)
}

pub(crate) fn proposal_result(
    snapshot: &SourceSnapshot,
    edits: &[crate::engine::analysis::symbols::Edit],
) -> Result<(Value, Value), rmcp::ErrorData> {
    let mut grouped = BTreeMap::<String, Vec<&crate::engine::analysis::symbols::Edit>>::new();
    for edit in edits {
        grouped.entry(edit.file.clone()).or_default().push(edit);
    }
    let mut changed = BTreeMap::new();
    let mut wire = Vec::new();
    for (file, mut edits) in grouped {
        edits.sort_by_key(|edit| (edit.span.start, edit.span.end));
        if edits
            .windows(2)
            .any(|pair| pair[0].span.end > pair[1].span.start)
        {
            return Err(rmcp::ErrorData::internal_error(
                "The proposed source edits overlap",
                None,
            ));
        }
        let original = snapshot.text(Some(&file)).ok_or_else(|| {
            rmcp::ErrorData::internal_error("Proposed source is unavailable", None)
        })?;
        let mut text = original.to_owned();
        for edit in edits.into_iter().rev() {
            if original.get(edit.span.start as usize..edit.span.end as usize)
                != Some(edit.expected.as_str())
            {
                return Err(rmcp::ErrorData::internal_error(
                    "Proposed edit does not match captured source",
                    None,
                ));
            }
            text.replace_range(
                edit.span.start as usize..edit.span.end as usize,
                &edit.replacement,
            );
            let mut row = location(snapshot, &file, edit.span).expect("verified source edit");
            row["expected_text"] = json!(edit.expected);
            row["new_text"] = json!(edit.replacement);
            wire.push(row);
        }
        changed.insert(snapshot.written_file(Some(&file)).unwrap_or_default(), text);
    }
    wire.reverse();
    let data = if snapshot.root_file.is_none() {
        json!({"text":changed.get("").unwrap_or(&snapshot.raw)})
    } else {
        json!({"files":changed})
    };
    let mut guards = serde_json::to_value(&snapshot.guards).expect("input guards");
    if snapshot.root_file.is_none()
        && let Some(rows) = guards.as_array_mut()
    {
        for row in rows {
            row["file"] = Value::Null;
        }
    }
    Ok((
        data,
        json!({"revision":snapshot.revision,"source_policy":snapshot.inputs["source_policy"],"checked_owners":snapshot.inputs["checked_owners"],
        "include_dirs":snapshot.inputs["include_dirs"],"macro_work_limit":snapshot.inputs["macro_work_limit"],"analysis":snapshot.inputs["analysis"],"inputs":guards,"edits":wire}),
    ))
}

fn catalog(
    tool: &str,
    arguments: &Value,
    params: &Params,
) -> Result<CallToolResult, rmcp::ErrorData> {
    let mut result = match tool {
        "dynare_explain" => {
            let code = params
                .code
                .as_deref()
                .ok_or_else(|| invalid("code is required"))?;
            let mut result =
                ToolResult::new(json!({"code":code,"explanation":super::dynare_explain(code)}));
            if !crate::engine::diagnostics::explain::known_codes().contains(&code) {
                result.cause("refused","UNKNOWN_CODE",format!("Diagnostic code {code} is not in the current catalog. Use dynare_list_diagnostic_codes to select a code."));
            }
            result
        }
        "dynare_list_diagnostic_codes" => {
            let mut rows =
                serde_json::to_value(super::dynare_list_diagnostic_codes()).expect("catalog JSON");
            rows.as_array_mut().expect("code rows").retain(|row| {
                params.code.as_ref().is_none_or(|code| row["code"] == *code)
                    && params.kind.as_ref().is_none_or(|kind| row["kind"] == *kind)
                    && params.severity.as_ref().is_none_or(|severity| {
                        row["code"].as_str().is_some_and(|code| {
                            code.starts_with(match severity.as_str() {
                                "ERROR" => "E",
                                "WARNING" => "W",
                                "INFORMATION" => "I",
                                "HINT" => "H",
                                _ => "?",
                            })
                        })
                    })
                    && params.search.as_ref().is_none_or(|search| {
                        row.to_string()
                            .to_lowercase()
                            .contains(&search.to_lowercase())
                    })
            });
            ToolResult::new(rows)
        }
        _ => {
            let mut data = super::dynare_list_options(params.command.as_deref());
            if let Some(rows) = data.get_mut("options").and_then(Value::as_array_mut) {
                rows.retain(|row| {
                    params
                        .option
                        .as_ref()
                        .is_none_or(|name| row["name"] == *name)
                        && params.search.as_ref().is_none_or(|search| {
                            row.to_string()
                                .to_lowercase()
                                .contains(&search.to_lowercase())
                        })
                });
            }
            if let Some(rows) = data.get_mut("commands").and_then(Value::as_array_mut) {
                rows.retain(|row| {
                    params.search.as_ref().is_none_or(|search| {
                        row.as_str().is_some_and(|name| {
                            name.to_lowercase().contains(&search.to_lowercase())
                        })
                    })
                });
            }
            let mut result = ToolResult::new(data);
            if result.data["known"] == false {
                result.cause(
                    "refused",
                    "UNKNOWN_COMMAND",
                    result.data["message"]
                        .as_str()
                        .unwrap_or("The command is not in the bundled catalog")
                        .to_owned(),
                );
            }
            result
        }
    };
    project_and_page(
        tool,
        arguments,
        params,
        env!("CARGO_PKG_VERSION"),
        &mut result,
        None,
    )?;
    Ok(result.wire())
}

fn project_and_page(
    tool: &str,
    arguments: &Value,
    params: &Params,
    revision: &str,
    result: &mut ToolResult,
    prepared: Option<Pages>,
) -> Result<(), rmcp::ErrorData> {
    if result.proposal.is_some() {
        return Ok(());
    }
    let mut pages = match prepared
        .map(Ok)
        .unwrap_or_else(|| prepare_pages(tool, arguments, params, revision))
    {
        Ok(pages) => pages,
        Err(message) if message.starts_with("INPUT_CHANGED") => {
            *result = ToolResult::refused("INPUT_CHANGED", message);
            return Ok(());
        }
        Err(message) => return Err(invalid(message)),
    };
    let complete = result.coverage.as_ref().is_none_or(|coverage| {
        coverage
            .as_array()
            .is_none_or(|roots| roots.iter().all(|root| root["complete"] == true))
    });
    if params.agent_output {
        super::canonical::page_collections("", &mut result.data, &mut pages, complete);
    } else if let Some(rows) = result.data.as_array_mut() {
        pages.collection_with_coverage("rows", rows, complete);
    } else if let Some(object) = result.data.as_object_mut() {
        for (name, rows) in object
            .iter_mut()
            .filter(|(name, _)| name.as_str() != "edits")
        {
            if let Some(rows) = rows.as_array_mut() {
                if name == "heterogeneous_equations" {
                    for block in rows.iter_mut() {
                        let key = format!(
                            "equations:{}",
                            block["dimension"].as_str().unwrap_or_default()
                        );
                        if let Some(rows) = block["equations"].as_array_mut() {
                            pages.collection_with_coverage(&key, rows, complete);
                        }
                    }
                } else {
                    pages.collection_with_coverage(name, rows, complete);
                }
            }
        }
    }
    result.selection = pages.finish(&params.output);
    if params.output.detail == Detail::Compact {
        remove_prose(&mut result.data);
    }
    Ok(())
}

fn prepare_pages(
    tool: &str,
    arguments: &Value,
    params: &Params,
    revision: &str,
) -> Result<Pages, String> {
    let canonical_tool = format!("v3:{tool}");
    let mut pages = Pages::new(
        if params.agent_output {
            &canonical_tool
        } else {
            tool
        },
        revision,
        arguments,
        &params.output,
    )?;
    if params.agent_output {
        pages.share_budget();
    }
    Ok(pages)
}

fn remove_prose(data: &mut Value) {
    match data {
        Value::Object(object) => {
            object.remove("markdown");
            object.remove("explain_markdown");
            for value in object.values_mut() {
                remove_prose(value);
            }
        }
        Value::Array(rows) => {
            for row in rows {
                remove_prose(row);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn call(tool: &str, args: Value) -> Value {
        dispatch(tool, args, RequestControl::default())
            .unwrap()
            .structured_content
            .unwrap()
    }
    const MODEL: &str = "var y;\nmodel;\ny=1;\nend;\n";
    fn projection_source() -> Value {
        let rows = (0..32)
            .map(|i| format!("#l{i}=y(-1); [name='e{i}'] y=l{i};\n"))
            .collect::<String>();
        json!({"kind":"text","text":format!("var y; model;\n{rows}end;")})
    }

    #[test]
    fn selected_and_paged_equations_project_only_returned_rows() {
        // Guards serializing every equation before a named selection or page.
        for selector in [
            json!({"name":"e17"}),
            json!({"name":"absent"}),
            json!({"symbol":{"kind":"name","name":"l17"}}),
            json!({"page_size":2}),
        ] {
            super::super::EQUATION_ROW_PROJECTIONS.with(|count| count.set(0));
            let mut args = json!({"source":projection_source()});
            args.as_object_mut()
                .unwrap()
                .extend(selector.as_object().unwrap().clone());
            let result = super::super::canonical::dispatch(
                "dynare_equations",
                args,
                RequestControl::default(),
            )
            .unwrap()
            .structured_content
            .unwrap();
            assert_eq!(
                super::super::EQUATION_ROW_PROJECTIONS.with(std::cell::Cell::get),
                result["data"]["equations"].as_array().unwrap().len(),
                "{selector}"
            );
        }
    }

    #[test]
    fn symbol_sections_and_pages_project_only_returned_uses() {
        // Guards full reference/occurrence JSON construction for a small page
        // or a definition-only query, including the shared collection order.
        for extra in [
            json!({"sections":["definition"]}),
            json!({"sections":["references"],"page_size":2}),
            json!({"sections":["references"],"occurrences":true,"page_size":2}),
            json!({"sections":["metadata","references"],"page_size":1}),
        ] {
            SYMBOL_REFERENCE_PROJECTIONS.with(|count| count.set(0));
            SYMBOL_OCCURRENCE_PROJECTIONS.with(|count| count.set(0));
            let mut args =
                json!({"source":projection_source(),"symbol":{"kind":"name","name":"y"}});
            args.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let result = super::super::canonical::dispatch(
                "dynare_symbol_info",
                args,
                RequestControl::default(),
            )
            .unwrap()
            .structured_content
            .unwrap();
            let count = |name: &str| result["data"][name].as_array().map_or(0, Vec::len);
            assert_eq!(
                SYMBOL_REFERENCE_PROJECTIONS.with(std::cell::Cell::get),
                count("references"),
                "{extra}"
            );
            assert_eq!(
                SYMBOL_OCCURRENCE_PROJECTIONS.with(std::cell::Cell::get),
                count("occurrences"),
                "{extra}"
            );
            if extra["sections"] == json!(["definition"]) {
                assert!(result["data"].get("references").is_none());
                assert!(result["data"]["binding"].get("timing").is_none());
            }
            if extra["occurrences"] == true
                || extra["sections"] == json!(["metadata", "references"])
            {
                assert_eq!(
                    count("references"),
                    0,
                    "an earlier collection must consume the row allowance"
                );
            }
        }
    }

    #[test]
    fn equation_scopes_share_fact_collection() {
        // Guards redoing final-type, timing and local analysis for each scope.
        let args = json!({"source":projection_source(),"page_size":1});
        let mut snapshot = super::super::source::resolve_source(
            serde_json::from_value(projection_source()).unwrap(),
            None,
            &RequestControl::default(),
        )
        .unwrap();
        let params = Params {
            agent_output: true,
            output: ResultOptions {
                page_size: Some(1),
                ..Default::default()
            },
            ..Default::default()
        };
        crate::engine::analysis::equations::EQUATION_FACT_BUILDS.with(|count| count.set(0));
        let result = equation_query(&mut snapshot, &params, &args, &mut None).unwrap();
        assert_eq!(result.data["equations"].as_array().unwrap().len(), 1);
        assert_eq!(
            crate::engine::analysis::equations::EQUATION_FACT_BUILDS.with(std::cell::Cell::get),
            1
        );
    }

    #[test]
    fn selected_expansion_projects_only_returned_navigation() {
        // Guards building full navigation before selecting one verified equation.
        let input = projection_source();
        let equation = super::super::canonical::dispatch(
            "dynare_equations",
            json!({"source":input,"name":"e17"}),
            RequestControl::default(),
        )
        .unwrap()
        .structured_content
        .unwrap();
        crate::engine::preview::preview_navigation::NAVIGATION_ROW_PROJECTIONS
            .with(|count| count.set(0));
        let result = super::super::canonical::dispatch("dynare_expand",json!({"source":input,"equation":{"kind":"id","equation_id":equation["data"]["equations"][0]["equation_id"]},"expected_revision":equation["inputs"][0]["revision"]}),RequestControl::default()).unwrap().structured_content.unwrap();
        assert_eq!(
            crate::engine::preview::preview_navigation::NAVIGATION_ROW_PROJECTIONS
                .with(std::cell::Cell::get),
            result["data"]["navigation"].as_array().unwrap().len()
        );
        assert_eq!(result["data"]["navigation"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn projection_obeys_the_shared_allowance_before_serializing_rows() {
        // Guards navigation or equations being projected after an earlier
        // collection has consumed the response's shared row allowance.
        super::super::EQUATION_ROW_PROJECTIONS.with(|count| count.set(0));
        let equations = super::super::canonical::dispatch("dynare_equations",json!({"source":{"kind":"text","text":"var y unused; model; y=y(-1); end;"},"page_size":1}),RequestControl::default()).unwrap().structured_content.unwrap();
        assert_eq!(
            equations["selection"]["collections"]["equations"]["available_matches"],
            1
        );
        assert!(
            equations["data"]["equations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            super::super::EQUATION_ROW_PROJECTIONS.with(std::cell::Cell::get),
            0
        );
        crate::engine::preview::preview_navigation::NAVIGATION_ROW_PROJECTIONS
            .with(|count| count.set(0));
        let expanded = super::super::canonical::dispatch(
            "dynare_expand",
            json!({"source":projection_source(),"page_size":2}),
            RequestControl::default(),
        )
        .unwrap()
        .structured_content
        .unwrap();
        let returned = expanded["data"]["navigation"].as_array().unwrap().len();
        assert_eq!(returned, 2);
        assert_eq!(
            crate::engine::preview::preview_navigation::NAVIGATION_ROW_PROJECTIONS
                .with(std::cell::Cell::get),
            returned
        );
    }

    #[test]
    fn counted_equation_ids_keep_the_written_row_after_model_locals() {
        // Guards using a counted index in place of the source map's row index.
        let source = projection_source();
        let selected = super::super::canonical::dispatch(
            "dynare_equations",
            json!({"source":source,"name":"e17"}),
            RequestControl::default(),
        )
        .unwrap()
        .structured_content
        .unwrap();
        let by_id = super::super::canonical::dispatch("dynare_equations",json!({"source":source,"equation":{"kind":"id","equation_id":selected["data"]["equations"][0]["equation_id"]},"expected_revision":selected["inputs"][0]["revision"]}),RequestControl::default()).unwrap().structured_content.unwrap();
        assert_eq!(by_id["data"]["equations"], selected["data"]["equations"]);
    }

    #[test]
    fn focused_queries_keep_invalid_and_changed_continuation_outcomes() {
        // Guards an early projection error escaping as successful null data.
        let source = projection_source();
        for tool in ["dynare_equations", "dynare_expand"] {
            let first = super::super::canonical::dispatch(
                tool,
                json!({"source":source,"page_size":1}),
                RequestControl::default(),
            )
            .unwrap()
            .structured_content
            .unwrap();
            let args = json!({"source":source,"page_size":1,"continuation":"invalid JSON","expected_revision":first["inputs"][0]["revision"]});
            assert!(
                super::super::canonical::dispatch(tool, args, RequestControl::default()).is_err()
            );
            let mut args = json!({"source":source,"page_size":1,"continuation":first["selection"]["continuation"],"expected_revision":first["inputs"][0]["revision"]});
            if tool == "dynare_equations" {
                args["name"] = json!("e17");
            } else {
                args["line_range"] = json!({"start_line":1,"end_line":1});
            }
            let result = super::super::canonical::dispatch(tool, args, RequestControl::default())
                .unwrap()
                .structured_content
                .unwrap();
            assert_eq!(result["outcome"]["status"], "refused");
            assert_eq!(result["outcome"]["reasons"][0]["code"], "INPUT_CHANGED");
        }
    }
    #[test]
    fn formatting_distinguishes_changes_unchanged_and_partial_input() {
        // Guards proposed formatting on incomplete source being reported as complete.
        for (text, status, reason) in [
            (MODEL, "changed", None),
            ("   \n", "unchanged", Some("NO_CHANGE")),
            ("model;\ny=1;\n", "partial", Some("INCOMPLETE_INPUT")),
        ] {
            let result = call("dynare_format", json!({"file_content":text}));
            assert_eq!(result["outcome"]["status"], status);
            if let Some(reason) = reason {
                assert!(
                    result["outcome"]["reasons"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|row| row["code"] == reason)
                );
            }
            assert_eq!(result["proposal"].is_object(), status != "unchanged");
        }
    }
    #[test]
    fn scalar_targets_reject_out_of_line_columns_and_keep_crlf_offsets() {
        assert_eq!(byte_position("😀x\r\ny", 1, 2).unwrap(), 4);
        assert_eq!(byte_position("😀x\r\ny", 2, 1).unwrap(), 7);
        assert!(byte_position("😀x\r\ny", 1, 4).is_err());
        assert!(byte_position("x", 0, 1).is_err());
        assert!(byte_position("x", 2, 1).is_err());
    }
    #[test]
    fn selected_expansion_navigation_refers_to_the_returned_fragment() {
        let args = json!({"source":{"kind":"text","text":MODEL}});
        let equations = call("dynare_equations", args.clone());
        let id = &equations["data"]["equations"][0]["equation_id"];
        assert!(id.is_string(), "{equations}");
        let selected = call(
            "dynare_expand",
            json!({"source":args["source"],"equation_id":id,"expected_revision":equations["inputs"][0]["revision"]}),
        );
        assert_eq!(selected["outcome"]["status"], "ok");
        let text = selected["data"]["effective_text"].as_str().unwrap();
        for row in selected["data"]["navigation"].as_array().unwrap() {
            let range = &row["effective_range"];
            let start = byte_position(
                text,
                range["line"].as_u64().unwrap() as u32,
                range["column"].as_u64().unwrap() as u32,
            )
            .unwrap();
            let end = byte_position(
                text,
                range["end_line"].as_u64().unwrap() as u32,
                range["end_column"].as_u64().unwrap() as u32,
            )
            .unwrap();
            assert!(end > start);
            assert!(text.get(start as usize..end as usize).is_some());
        }
        assert!(
            !selected["data"]["navigation"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn partial_expanded_row_withholds_its_written_jump_and_names_the_omission() {
        let full = call(
            "dynare_expand",
            json!({"source":{"kind":"text","text":MODEL}}),
        );
        let range = &full["data"]["navigation"][0]["effective_range"];
        let result = call(
            "dynare_expand",
            json!({"source":{"kind":"text","text":MODEL},"expanded_range":{
            "line":range["line"],"column":range["column"].as_u64().unwrap()+1,
            "end_line":range["end_line"],"end_column":range["end_column"]}}),
        );
        assert_eq!(result["outcome"]["status"], "partial");
        assert_eq!(
            result["outcome"]["reasons"][0]["code"],
            "PARTIAL_ROW_ORIGINS"
        );
        assert!(result["data"]["navigation"].as_array().unwrap().is_empty());
        assert!(result["data"]["origins"].as_array().unwrap().is_empty());
    }
    #[test]
    fn source_limit_refusal_has_limit_metadata_and_no_proposal() {
        let control = RequestControl {
            limits: super::super::source::SourceLimits {
                max_file_bytes: MODEL.len() - 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let wire = dispatch(
            "dynare_rename",
            json!({"source":{"kind":"text","text":MODEL},"old_name":"y","new_name":"z"}),
            control,
        )
        .unwrap();
        let result = wire.structured_content.unwrap();
        assert_eq!(result["outcome"]["status"], "refused");
        assert_eq!(result["limits"][0]["code"], "SOURCE_LIMIT");
        assert!(result["proposal"].is_null());
    }
    #[test]
    fn unknown_equation_scope_and_a_target_outside_the_selected_model_are_input_errors() {
        assert!(dispatch(
            "dynare_extract",
            json!({"source":{"kind":"text","text":MODEL},"equation":{"scope":"guess","index":0}}),
            RequestControl::default()
        )
        .is_err());
        assert!(dispatch("dynare_symbol_info",json!({"source":{"kind":"map","root":"root.mod","files":{"root.mod":MODEL,"other.mod":MODEL}},"target":{"file":"other.mod","line":1,"column":5}}),RequestControl::default()).is_err());
    }
    #[test]
    fn unknown_command_refuses_with_its_catalog_cause_and_command_search_filters_rows() {
        let result = call(
            "dynare_list_options",
            json!({"command":"not_a_dynare_command"}),
        );
        assert_eq!(result["outcome"]["status"], "refused");
        assert_eq!(result["outcome"]["reasons"][0]["code"], "UNKNOWN_COMMAND");
        let result = call("dynare_list_options", json!({"search":"steady"}));
        assert!(!result["data"]["commands"].as_array().unwrap().is_empty());
        assert!(
            result["data"]["commands"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row.as_str().unwrap().contains("steady"))
        );
    }
}
