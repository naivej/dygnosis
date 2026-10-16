//! Version-3 agent tools. Each entry owns one caller task; source proof remains
//! in the shared request workers and engine.

use std::collections::BTreeMap;
use std::sync::Arc;

use rmcp::model::{CallToolResult, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::request::{ExpandedRange, LineRange, Target};
use super::result::{Detail, Pages, ResultOptions, ToolResult};
use super::source::{RequestControl, SourceInput};

pub(super) const TOOL_NAMES: &[&str] = &[
    "dynare_syntax",
    "dynare_diagnostic_info",
    "dynare_diagnose",
    "dynare_model_info",
    "dynare_symbol_info",
    "dynare_equations",
    "dynare_expand",
    "dynare_actions",
    "dynare_rename",
    "dynare_format",
    "dynare_extract",
    "dynare_compare_models",
];

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum SymbolSelector {
    Name {
        name: String,
    },
    Binding {
        binding_id: String,
    },
    Target {
        #[serde(flatten)]
        target: Target,
    },
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum EquationSelector {
    Id {
        equation_id: String,
    },
    Index {
        scope: String,
        dimension: Option<String>,
        index: usize,
    },
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum BatchInput {
    Map {
        files: BTreeMap<String, String>,
        roots: Vec<String>,
        #[serde(default)]
        include_dirs: Vec<String>,
        #[serde(default)]
        exclude_paths: Vec<String>,
    },
    Paths {
        paths: Vec<String>,
        #[serde(default)]
        include_dirs: Vec<String>,
        #[serde(default)]
        exclude_paths: Vec<String>,
    },
    Sources {
        sources: Vec<SourceInput>,
    },
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ComparisonInput {
    Sources {
        before: SourceInput,
        after: SourceInput,
    },
    Repository {
        repository_path: String,
        before: super::repository_compare::RepositorySelector,
        after: super::repository_compare::RepositorySelector,
        #[serde(default)]
        search_paths: Vec<String>,
    },
}

/// Projection inventory shared by discovery and decoding. Selectors are typed,
/// and legacy source aliases are absent from this contract.
#[derive(Debug, Deserialize, JsonSchema)]
#[allow(dead_code)] // Most fields pass unchanged to the common worker after typed decoding.
struct Params {
    source: Option<SourceInput>,
    batch: Option<BatchInput>,
    comparison: Option<ComparisonInput>,
    /// Original revision, required for reused identities and continuations.
    expected_revision: Option<String>,
    #[serde(default)]
    expected_revisions: BTreeMap<String, String>,
    expected_revision_a: Option<String>,
    expected_revision_b: Option<String>,
    #[serde(default)]
    macro_work_limit: crate::engine::macro_work::MacroWorkLimit,
    symbol: Option<SymbolSelector>,
    equation: Option<EquationSelector>,
    target: Option<Target>,
    name: Option<String>,
    search: Option<String>,
    kind: Option<String>,
    /// Grammar context, for example model_expression or macro_expression.
    context: Option<String>,
    /// Owning construct for an option or block instruction.
    parent: Option<String>,
    code: Option<String>,
    severity: Option<String>,
    file: Option<String>,
    root: Option<String>,
    scope: Option<String>,
    dimension: Option<String>,
    #[serde(default)]
    tags: BTreeMap<String, String>,
    #[serde(default)]
    names: Vec<String>,
    new_name: Option<String>,
    include_declaration: Option<bool>,
    occurrences: Option<bool>,
    action_id: Option<String>,
    action_kind: Option<String>,
    /// Propose compatible proven diagnostic fixes. Omit to discover summaries.
    #[serde(default)]
    fixes: bool,
    /// Select returned diagnostic identities; requires expected_revision.
    #[serde(default)]
    diagnostic_ids: Vec<String>,
    line_range: Option<LineRange>,
    expanded_range: Option<ExpandedRange>,
    #[serde(rename = "formatIndent")]
    format_indent: Option<Value>,
    #[serde(default)]
    families: Vec<String>,
    #[serde(default)]
    kinds: Vec<String>,
    /// Return selected sections; omitted values use a bounded useful default.
    #[serde(default)]
    sections: Vec<String>,
    /// Shared maximum rows across collections, from 1 to 1000. Default 25.
    #[schemars(range(min = 1, max = 1000))]
    page_size: Option<usize>,
    continuation: Option<String>,
    /// Compact by default. Full includes optional explanation, never legacy mirrors.
    detail: Option<Detail>,
}

fn fields(tool: &str) -> Vec<&'static str> {
    let mut fields = vec!["page_size", "continuation", "detail"];
    if matches!(
        tool,
        "dynare_model_info" | "dynare_symbol_info" | "dynare_compare_models"
    ) {
        fields.push("sections");
    }
    if !matches!(
        tool,
        "dynare_syntax" | "dynare_diagnostic_info" | "dynare_compare_models"
    ) {
        fields.extend(["source", "expected_revision", "macro_work_limit"]);
    }
    fields.extend_from_slice(match tool {
        "dynare_syntax" => &["name", "search", "kind", "context", "parent"],
        "dynare_diagnostic_info" => &["code", "kind", "severity", "search"],
        "dynare_diagnose" => &[
            "batch",
            "expected_revisions",
            "code",
            "severity",
            "file",
            "root",
        ],
        "dynare_model_info" => &[],
        "dynare_symbol_info" => &["symbol", "include_declaration", "occurrences"],
        "dynare_equations" => &["symbol", "equation", "name", "tags", "scope", "dimension"],
        "dynare_expand" => &["line_range", "expanded_range", "equation"],
        "dynare_actions" => &[
            "target",
            "action_id",
            "action_kind",
            "diagnostic_ids",
            "code",
            "fixes",
        ],
        "dynare_rename" => &["symbol", "new_name"],
        "dynare_format" => &["file", "line_range", "formatIndent"],
        "dynare_extract" => &["names", "tags", "dimension", "equation"],
        "dynare_compare_models" => &[
            "comparison",
            "macro_work_limit",
            "expected_revision_a",
            "expected_revision_b",
            "families",
            "kinds",
        ],
        _ => &[],
    });
    fields
}

fn invalid(message: impl Into<String>) -> rmcp::ErrorData {
    rmcp::ErrorData::invalid_params(message.into(), None)
}

pub(super) fn input_schema(tool: &str) -> Arc<serde_json::Map<String, Value>> {
    let mut schema = super::mcp_input_schema::<Params>().as_ref().clone();
    let allowed = fields(tool);
    schema["properties"]
        .as_object_mut()
        .unwrap()
        .retain(|key, _| allowed.contains(&key.as_str()));
    schema.insert("additionalProperties".into(), json!(false));
    if tool == "dynare_diagnose" {
        schema.insert("oneOf".into(),json!([{"required":["source"],"properties":{"source":{"not":{"type":"null"}}},"not":{"required":["batch"]}},{"required":["batch"],"properties":{"batch":{"not":{"type":"null"}}},"not":{"required":["source"]}}]));
    } else if allowed.contains(&"source") {
        schema.insert("required".into(), json!(["source"]));
    }
    let required = match tool {
        "dynare_symbol_info" => vec!["source", "symbol"],
        "dynare_rename" => vec!["source", "symbol", "new_name"],
        "dynare_compare_models" => vec!["comparison"],
        _ => vec![],
    };
    if !required.is_empty() {
        schema.insert("required".into(), json!(required));
    }
    for key in schema
        .get("required")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
    {
        let property = &mut schema["properties"][key];
        *property = json!({"allOf":[property.clone(),{"not":{"type":"null"}}]});
    }
    super::request::prune_definitions(&mut schema);
    Arc::new(schema)
}

pub(super) fn advertised_tools(structured: bool) -> Vec<Tool> {
    TOOL_NAMES.iter().map(|&name|{
        let description=match name {
            "dynare_syntax"=>"Look up pinned Dynare syntax: commands, blocks, instructions, functions, operators, macros and options. Select name or search; kind, context and parent distinguish forms with the same spelling. Exact forms include signatures, value rules and bounded options. Omit filters for a bounded index. No model source is needed.",
            "dynare_diagnostic_info"=>"Look up diagnostic meaning and correction guidance. An exact code returns its explanation; kind, severity and search return bounded catalog summaries. This catalog does not report problems in a model. No model source is needed.",
            "dynare_diagnose"=>"Check one source or an explicit batch of model roots. Returns aggregate summary, per-root reports and revisions, and paged diagnostics in the same shape. Batch map roots are exact keys; paths read saved server-host files; sources can mix explicit text/map/disk inputs. Each partial, failed or unfinished root explains its cause; an incomplete zero count is not a clean-model claim.",
            "dynare_model_info"=>"Give a model overview and its source relationships. Default summary has scalar counts and model kind. Select structure for timing lists and block flags, or declarations, assignments, model_locals, statements, blocks, sources, includes or scope for detail. sources includes resolved/missing includes and companion lookup causes. Unnamed text cannot discover file relationships. Counts describe written source before transformations.",
            "dynare_symbol_info"=>"Inspect one resolved binding and its accepted uses in one call. Select a unique name, written target or returned binding identity. Default sections are definition, metadata and references; assignments and expanded occurrences are optional. References-only works directly without an earlier lookup. Ambiguous names return candidates; lookalike text is excluded.",
            "dynare_equations"=>"Find and inspect equations, tags, identifiers, timing and verified source locations. Name, tag, scope/dimension and resolved-symbol filters combine with AND. A symbol name works directly. Index selectors are zero-based within aggregate or dimension scope; identities select one expanded row. Incomplete analysis withholds authoritative counts.",
            "dynare_expand"=>"Show include/macro-expanded text with verified written origins. Focus with inclusive line_range, exclusive-end expanded_range or an equation identity. A range cutting a row omits its origin and states why. Partial expansion retains available text and its causes.",
            "dynare_actions"=>"Discover source action summaries or request a complete verified proposal. Select action_id with its expected_revision for one alternative. Set fixes=true to propose compatible proven diagnostic fixes, optionally by code, target or diagnostic_ids, without a discovery call. Alternatives are never combined automatically. Skipped and remaining work is explained. No file is written.",
            "dynare_rename"=>"Propose a complete rename of one resolved binding. A unique name works directly; a target or binding identity can disambiguate. Incomplete sources, collisions, generated names or incompatible checked owners withhold the complete proposal with a cause. No file is written.",
            "dynare_format"=>"Propose whitespace formatting for the root or selected source file. Optional whole-line line_range is one-based and inclusive. formatIndent is tab or one to eight spaces. Native text and source outside the selected lines retain their layout. No file is written.",
            "dynare_extract"=>"Produce a selected equation fragment with required declarations, local definitions and model context. Names combine with OR; tags combine with AND. Select a returned identity or scoped zero-based index for unnamed equations. Omitted setup is explicit; no numerical or runnable-model claim is inferred.",
            "dynare_compare_models"=>"Compare two model snapshots and captured source. Sources use explicit text/map/disk inputs. Repository input selects local Git or saved Working versions on this server host; no fetch or editor-buffer read occurs. Filters select semantic families and change kinds. Coverage and source navigation retain each side's identity; no algebraic or numerical equivalence is inferred.",
            _=>unreachable!(),
        };
        let mut tool=Tool::new(name,format!("{description} Tool contract 3: read data beside outcome, inputs, coverage, selection, proposal and limits. Default 25 rows share one response allowance; selection reports counts and continuation. Reuse identities or continuations with their original revision guards. Source maps read only supplied files. Positions are one-based Unicode scalars with exclusive ends. A proposal is atomic: verify every input guard and expected_text, then apply the whole edit set with external file tools. No tool writes files or executes MATLAB/Octave."),input_schema(name))
            .with_annotations(ToolAnnotations::new().read_only(true).destructive(false).idempotent(true).open_world(false));
        if structured {let mut schema=super::result::output_schema().as_ref().clone();schema["properties"]["schema_version"]=json!({"const":3});tool.output_schema=Some(Arc::new(schema));}
        tool
    }).collect()
}

fn defaults(arguments: &mut Value, params: &Params, tool: &str) {
    let object = arguments.as_object_mut().unwrap();
    object.insert(
        "detail".into(),
        json!(params.detail.unwrap_or(Detail::Compact)),
    );
    object.insert("page_size".into(), json!(params.page_size.unwrap_or(25)));
    if params.sections.is_empty() {
        match tool {
            "dynare_model_info" => {
                object.insert("sections".into(), json!(["summary"]));
            }
            "dynare_symbol_info" => {
                object.insert(
                    "sections".into(),
                    json!(["definition", "metadata", "references"]),
                );
            }
            _ => {}
        }
    }
}

pub(super) fn dispatch(
    tool: &str,
    mut arguments: Value,
    control: RequestControl,
) -> Result<CallToolResult, rmcp::ErrorData> {
    control.check().map_err(|e| invalid(e.message))?;
    let object = arguments
        .as_object()
        .ok_or_else(|| invalid("Tool arguments must be an object"))?;
    let allowed = fields(tool);
    if !TOOL_NAMES.contains(&tool) {
        return Err(invalid("Unknown canonical tool"));
    }
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(invalid(format!(
            "{tool} does not accept {key}; use the advertised version-3 schema"
        )));
    }
    let params: Params =
        serde_json::from_value(arguments.clone()).map_err(|e| invalid(e.to_string()))?;
    if params
        .page_size
        .is_some_and(|size| !(1..=1000).contains(&size))
    {
        return Err(invalid("page_size must be from 1 to 1000"));
    }
    if params.severity.as_deref().is_some_and(|severity| {
        !["ERROR", "WARNING", "INFORMATION", "HINT"]
            .iter()
            .any(|known| known.eq_ignore_ascii_case(severity))
    }) {
        return Err(invalid(
            "severity must be ERROR, WARNING, INFORMATION or HINT",
        ));
    }
    if tool == "dynare_diagnostic_info"
        && params
            .kind
            .as_deref()
            .is_some_and(|kind| !["shared", "added", "skipped"].contains(&kind))
    {
        return Err(invalid("Diagnostic kind must be shared, added or skipped"));
    }
    defaults(&mut arguments, &params, tool);
    if tool == "dynare_syntax" {
        return syntax(&params, &arguments);
    }
    if tool == "dynare_diagnostic_info" {
        return diagnostic_info(&params, &arguments);
    }
    if tool == "dynare_diagnose" && (params.source.is_some() == params.batch.is_some()) {
        return Err(invalid("Choose exactly one source or batch"));
    }
    if tool == "dynare_diagnose" {
        let object = arguments.as_object_mut().unwrap();
        object.remove("batch");
        object.remove("source");
        object.remove("expected_revision");
        let batch = if let Some(batch) = &params.batch {
            if params.expected_revision.is_some() {
                return Err(invalid(
                    "A batch uses expected_revisions keyed by root, not expected_revision",
                ));
            }
            match batch {
                BatchInput::Map {
                    files,
                    roots,
                    include_dirs,
                    exclude_paths,
                } => {
                    json!({"files":files,"roots":roots,"include_dirs":include_dirs,"exclude_paths":exclude_paths})
                }
                BatchInput::Paths {
                    paths,
                    include_dirs,
                    exclude_paths,
                } => {
                    json!({"paths":paths,"include_dirs":include_dirs,"exclude_paths":exclude_paths})
                }
                BatchInput::Sources { sources } => json!({"sources":sources}),
            }
        } else {
            if !params.expected_revisions.is_empty() {
                return Err(invalid(
                    "One source uses expected_revision, not expected_revisions",
                ));
            }
            let source = params.source.as_ref().unwrap();
            let root = match source {
                SourceInput::Text { .. } => "input:1",
                SourceInput::Map { root, .. } | SourceInput::Disk { root, .. } => root,
            };
            if let Some(expected) = &params.expected_revision {
                object.insert("expected_revisions".into(), json!({root:expected}));
            }
            json!({"sources":[source]})
        };
        arguments
            .as_object_mut()
            .unwrap()
            .extend(batch.as_object().unwrap().clone());
        return super::multi_request::dispatch_batch(arguments, control);
    }
    if tool == "dynare_compare_models" {
        arguments.as_object_mut().unwrap().remove("comparison");
        match params
            .comparison
            .ok_or_else(|| invalid("comparison is required"))?
        {
            ComparisonInput::Sources { before, after } => {
                return super::multi_request::comparison_sources(arguments, control, before, after);
            }
            ComparisonInput::Repository {
                repository_path,
                before,
                after,
                search_paths,
            } => {
                let original = arguments.as_object_mut().unwrap();
                original.insert("repository_path".into(), json!(repository_path));
                // Repository selectors were decoded by their shared type; keep their exact wire forms.
                let selector =
                    |selector: super::repository_compare::RepositorySelector| match selector {
                        super::repository_compare::RepositorySelector::Git {
                            root_file,
                            requested_ref,
                        } => json!({"kind":"git","root_file":root_file,"ref":requested_ref}),
                        super::repository_compare::RepositorySelector::Working { root_file } => {
                            json!({"kind":"working","root_file":root_file})
                        }
                    };
                original.insert("before".into(), selector(before));
                original.insert("after".into(), selector(after));
                original.insert("search_paths".into(), json!(search_paths));
                return super::multi_request::comparison_canonical(arguments, control);
            }
        }
    }
    if params.source.is_none() {
        return Err(invalid("source is required"));
    }
    let object = arguments.as_object_mut().unwrap();
    if let Some(selector) = params.symbol {
        object.remove("symbol");
        match selector {
            SymbolSelector::Name { name } => {
                object.insert(
                    if tool == "dynare_rename" {
                        "old_name"
                    } else {
                        "symbol"
                    }
                    .into(),
                    json!(name),
                );
            }
            SymbolSelector::Binding { binding_id } => {
                object.insert("binding_id".into(), json!(binding_id));
            }
            SymbolSelector::Target { target } => {
                object.insert(
                    "target".into(),
                    json!({"file":target.file,"line":target.line,"column":target.column}),
                );
            }
        }
    } else if matches!(tool, "dynare_symbol_info" | "dynare_rename") {
        return Err(invalid("symbol is required"));
    }
    if let Some(selector) = params.equation {
        object.remove("equation");
        match selector {
            EquationSelector::Id { equation_id } => {
                object.insert("equation_id".into(), json!(equation_id));
            }
            EquationSelector::Index {
                scope,
                dimension,
                index,
            } => {
                if tool == "dynare_expand" {
                    return Err(invalid(
                        "Expansion requires an equation identity or text range",
                    ));
                }
                object.insert(
                    "equation".into(),
                    json!({"scope":scope,"dimension":dimension,"index":index}),
                );
            }
        }
    }
    if tool == "dynare_symbol_info" {
        for section in &params.sections {
            if !["definition", "metadata", "assignments", "references"].contains(&section.as_str())
            {
                return Err(invalid(format!("Unknown symbol section {section}")));
            }
        }
    }
    let actual = if tool == "dynare_actions" {
        object.remove("fixes");
        if params.fixes {
            if params.action_id.is_some()
                || params
                    .action_kind
                    .as_deref()
                    .is_some_and(|kind| kind != "diagnostic_fix")
            {
                return Err(invalid(
                    "Choose one action_id or diagnostic fixes; action alternatives cannot be combined",
                ));
            }
            object.remove("action_kind");
            object.remove("action_id");
            "dynare_auto_fix"
        } else {
            if !params.diagnostic_ids.is_empty() {
                return Err(invalid(
                    "diagnostic_ids select compatible fixes; set fixes=true",
                ));
            }
            object.remove("diagnostic_ids");
            "dynare_code_actions"
        }
    } else {
        tool
    };
    super::request::dispatch_agent(actual, arguments, control)
}

fn output(params: &Params) -> ResultOptions {
    ResultOptions {
        detail: params.detail.unwrap_or(Detail::Compact),
        sections: params.sections.clone(),
        page_size: Some(params.page_size.unwrap_or(25)),
        continuation: params.continuation.clone(),
    }
}

fn reference_result(
    tool: &str,
    params: &Params,
    arguments: &Value,
    mut result: ToolResult,
) -> Result<CallToolResult, rmcp::ErrorData> {
    let options = output(params);
    let mut pages = match Pages::new(
        &format!("v3:{tool}"),
        env!("CARGO_PKG_VERSION"),
        arguments,
        &options,
    ) {
        Ok(pages) => pages,
        Err(message) if message.starts_with("INPUT_CHANGED") => {
            return Ok(ToolResult::refused("INPUT_CHANGED", message).wire());
        }
        Err(message) => return Err(invalid(message)),
    };
    pages.share_budget();
    page_collections("", &mut result.data, &mut pages, true);
    result.selection = pages.finish(&options);
    Ok(result.wire())
}

fn syntax(params: &Params, arguments: &Value) -> Result<CallToolResult, rmcp::ErrorData> {
    let query = crate::engine::catalog::syntax::SyntaxQuery {
        name: params.name.clone(),
        search: params.search.clone(),
        kind: params.kind.clone(),
        context: params.context.clone(),
        parent: params.parent.clone(),
    };
    let found = crate::engine::catalog::syntax::query(&query);
    let mut result = ToolResult::new(
        json!({"forms":found.records,"available_contexts":found.available_contexts}),
    );
    if result.data["forms"].as_array().is_none_or(Vec::is_empty) && params.name.is_some() {
        result.cause("refused",if found.known_name{"WRONG_CONTEXT"}else{"UNKNOWN_SYNTAX"},if found.known_name{"The spelling is known, but no form matches the selected kind, context and parent. Read available_contexts or remove a filter."}else{"The spelling is not in the pinned Dynare syntax records. Search or omit name to inspect the available index."});
    }
    // An index returns useful form summaries. Option detail has its own exact
    // parent lookup; an exact form keeps its identity while paging its options.
    if result.data["forms"]
        .as_array()
        .is_some_and(|forms| forms.len() > 1)
    {
        for form in result.data["forms"].as_array_mut().unwrap() {
            if let Some(options) = form.as_object_mut().unwrap().remove("options") {
                form["option_count"] = json!(options.as_array().map_or(0, Vec::len));
            }
        }
    }
    reference_result("dynare_syntax", params, arguments, result)
}

fn diagnostic_info(params: &Params, arguments: &Value) -> Result<CallToolResult, rmcp::ErrorData> {
    let mut rows = serde_json::to_value(super::dynare_list_diagnostic_codes()).unwrap();
    rows.as_array_mut().unwrap().retain(|row| {
        params.code.as_ref().is_none_or(|code| row["code"] == *code)
            && params.kind.as_ref().is_none_or(|kind| row["kind"] == *kind)
            && params.severity.as_ref().is_none_or(|severity| {
                row["code"].as_str().is_some_and(|code| {
                    code.starts_with(match severity.to_ascii_uppercase().as_str() {
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
    if let Some(code) = &params.code {
        if !crate::engine::diagnostics::explain::known_codes().contains(&code.as_str()) {
            return reference_result(
                "dynare_diagnostic_info",
                params,
                arguments,
                ToolResult::refused(
                    "UNKNOWN_CODE",
                    format!(
                        "Diagnostic code {code} is not in the current catalog. Search or omit code to inspect known codes."
                    ),
                ),
            );
        }
        if let Some(row) = rows.as_array_mut().unwrap().first_mut() {
            row["explanation"] = json!(super::dynare_explain(code));
        }
    }
    reference_result(
        "dynare_diagnostic_info",
        params,
        arguments,
        ToolResult::new(json!({"diagnostics":rows})),
    )
}

/// Page nested fact collections with one response allowance. Syntax forms are
/// selected before their option lists; edit proposals are never visited here.
pub(super) fn page_collections(path: &str, value: &mut Value, pages: &mut Pages, complete: bool) {
    match value {
        Value::Array(rows) => {
            let collection = if path.is_empty() { "rows" } else { path };
            let container = path == "roots"
                || path == "heterogeneous_equations"
                || (path == "forms" && rows.len() == 1);
            if container {
                for (index, row) in rows.iter_mut().enumerate() {
                    let key = row
                        .get("root")
                        .or_else(|| row.get("id"))
                        .or_else(|| row.get("dimension"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .unwrap_or_else(|| index.to_string());
                    page_collections(&format!("{collection}:{key}"), row, pages, complete);
                }
            } else {
                pages.collection_with_coverage(collection, rows, complete);
            }
        }
        Value::Object(object) => {
            for (key, value) in object.iter_mut() {
                if [
                    "proposal",
                    "inputs",
                    "edits",
                    "range",
                    "coverage",
                    "summary",
                    "root_revisions",
                    "signatures",
                    "aliases",
                    "contexts",
                    "grammar_sources",
                    "available_contexts",
                    "segments",
                    "origin_frames",
                    "tags",
                    "declaration",
                    "definition",
                    "scope",
                ]
                .contains(&key.as_str())
                {
                    continue;
                }
                let next = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                page_collections(&next, value, pages, complete);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const MODEL: &str = "var y; varexo e; parameters a; a=.5; model; [name='eq'] y=a*y(-1)+e; end;";
    fn source(text: &str) -> Value {
        json!({"kind":"text","text":text})
    }
    fn call(tool: &str, args: Value) -> Value {
        let wire = dispatch(tool, args, RequestControl::default()).unwrap();
        assert_eq!(
            wire.content.len(),
            1,
            "Canonical tools have one result object"
        );
        let value = wire.structured_content.unwrap();
        assert_eq!(value["schema_version"], 3);
        value
    }

    // Guards a compact default still computing and returning every name list.
    #[test]
    fn model_default_is_scalar_and_source_relationships_are_available_on_demand() {
        let result = call("dynare_model_info", json!({"source":source(MODEL)}));
        let data = result["data"].as_object().unwrap();
        assert_eq!(data.len(), 1);
        assert!(
            data["summary"]
                .as_object()
                .unwrap()
                .values()
                .all(|value| !value.is_array() && !value.is_object())
        );
        assert_eq!(data["summary"]["declaration_count"], 3);
        let relationships = call(
            "dynare_model_info",
            json!({"source":{"kind":"map","root":"root.mod","files":{"root.mod":"@#include \"body.inc\"\n","body.inc":MODEL}},"sections":["sources"]}),
        );
        assert!(
            relationships["data"]["sources"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["kind"] == "include" && row["path"] == "body.inc")
        );
        let unnamed = call(
            "dynare_model_info",
            json!({"source":source(MODEL),"sections":["sources"]}),
        );
        assert_eq!(unnamed["data"]["sources"]["available"], false);
        assert!(
            !unnamed["data"]["sources"]["cause"]
                .as_str()
                .unwrap()
                .is_empty()
        );
    }

    // Guards a references-only request requiring an earlier binding lookup,
    // and missing uses on later shared-budget pages.
    #[test]
    fn binding_and_uses_share_one_call_and_references_page_without_preflight() {
        let text = format!(
            "var y; parameters a; a=.5; model; y={}; end;",
            std::iter::repeat_n("a", 61).collect::<Vec<_>>().join("+")
        );
        let query = json!({"source":source(&text),"symbol":{"kind":"name","name":"a"},"sections":["references"]});
        let first = call("dynare_symbol_info", query.clone());
        assert_eq!(first["data"]["binding"]["name"], "a");
        assert_eq!(first["data"]["references"].as_array().unwrap().len(), 25);
        assert!(first["data"]["binding"]["assignment_values"].is_null());
        let expected = first["inputs"][0]["revision"].clone();
        let mut result = first;
        let mut count = 25;
        while let Some(token) = result["selection"]["continuation"].as_str() {
            let mut next = query.clone();
            next["continuation"] = json!(token);
            next["expected_revision"] = expected.clone();
            result = call("dynare_symbol_info", next);
            count += result["data"]["references"].as_array().unwrap().len();
        }
        assert_eq!(
            count, 63,
            "declaration, assignment and each model use remain reachable"
        );
        let filtered = call(
            "dynare_symbol_info",
            json!({"source":source(MODEL),"symbol":{"kind":"name","name":"a"},"include_declaration":false,"occurrences":true}),
        );
        assert_eq!(filtered["data"]["references"].as_array().unwrap().len(), 2);
        assert_eq!(filtered["data"]["occurrences"].as_array().unwrap().len(), 2);
    }

    // Guards the same checking job returning two incompatible data shapes,
    // dropped failed roots, and invented paths for unnamed text.
    #[test]
    fn single_and_batch_diagnosis_have_one_shape_and_keep_root_failures() {
        let one = call("dynare_diagnose", json!({"source":source(MODEL)}));
        let batch = call(
            "dynare_diagnose",
            json!({"batch":{"kind":"map","files":{"root.mod":MODEL},"roots":["root.mod","missing.mod"]}}),
        );
        assert_eq!(
            one["data"].as_object().unwrap().keys().collect::<Vec<_>>(),
            batch["data"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>()
        );
        assert_eq!(one["data"]["roots"][0]["root"], "input:1");
        assert!(one["inputs"][0]["root"].is_null());
        assert_eq!(batch["data"]["summary"]["failed"], 1);
        assert_eq!(batch["data"]["summary"]["checked"], 1);
        assert_eq!(batch["outcome"]["status"], "partial");
        let mixed = call(
            "dynare_diagnose",
            json!({"batch":{"kind":"sources","sources":[source(MODEL),{"kind":"map","root":"other.mod","files":{"other.mod":MODEL}}]}}),
        );
        assert_eq!(mixed["data"]["roots"].as_array().unwrap().len(), 2);
    }

    // Guards discovery returning every full-file action alternative, or a
    // selected proposal losing its complete guards and original text receipts.
    #[test]
    fn actions_discover_summaries_and_select_one_complete_alternative() {
        let listed = call("dynare_actions", json!({"source":source(MODEL)}));
        let action = listed["data"]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["kind"] == "equation_tags" || row["kind"] == "long_names")
            .unwrap();
        assert!(action.get("proposal").is_none());
        assert!(action.get("data").is_none());
        let selected = call(
            "dynare_actions",
            json!({"source":source(MODEL),"action_id":action["action_id"],"expected_revision":listed["inputs"][0]["revision"]}),
        );
        assert!(!selected["proposal"]["edits"].as_array().unwrap().is_empty());
        assert!(
            !selected["proposal"]["inputs"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(selected["data"].get("text").is_none());
        assert!(
            dispatch(
                "dynare_actions",
                json!({"source":source(MODEL),"fixes":true,"action_id":action["action_id"]}),
                RequestControl::default()
            )
            .is_err()
        );
    }

    // Guards Diagnose identities differing from the fix worker's identities.
    #[test]
    fn diagnose_id_selects_a_verified_fix_directly_and_rejects_stale_source() {
        let text = "var y; parameters a;\na=.5\nmodel; y=a; end;";
        let diagnosis = call("dynare_diagnose", json!({"source":source(text)}));
        let diagnostic = diagnosis["data"]["roots"][0]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["code"] == "E001")
            .unwrap();
        let expected = &diagnosis["data"]["root_revisions"]["input:1"];
        let fixed = call(
            "dynare_actions",
            json!({"source":source(text),"fixes":true,"diagnostic_ids":[diagnostic["diagnostic_id"]],"expected_revision":expected}),
        );
        assert!(fixed["proposal"].is_object(), "{fixed}");
        let stale = call(
            "dynare_actions",
            json!({"source":source(&format!("{text}\n")),"fixes":true,"diagnostic_ids":[diagnostic["diagnostic_id"]],"expected_revision":expected}),
        );
        assert_eq!(stale["outcome"]["reasons"][0]["code"], "INPUT_CHANGED");
        assert!(stale["proposal"].is_null());
    }

    // Guards canonical selectors requiring a binding preflight or accepting
    // old alias fields whose meaning conflicts with the advertised schema.
    #[test]
    fn typed_selectors_support_direct_equations_and_rename() {
        let equations = call(
            "dynare_equations",
            json!({"source":source(MODEL),"symbol":{"kind":"name","name":"a"}}),
        );
        assert_eq!(equations["data"]["equations"].as_array().unwrap().len(), 1);
        let renamed = call(
            "dynare_rename",
            json!({"source":source(MODEL),"symbol":{"kind":"name","name":"a"},"new_name":"rho"}),
        );
        assert_eq!(renamed["proposal"]["edits"].as_array().unwrap().len(), 3);
        assert!(
            dispatch(
                "dynare_rename",
                json!({"file_content":MODEL,"old_name":"a","new_name":"rho"}),
                RequestControl::default()
            )
            .is_err()
        );
        let target = call(
            "dynare_symbol_info",
            json!({"source":source(MODEL),"symbol":{"kind":"target","line":1,"column":5}}),
        );
        assert_eq!(target["data"]["binding"]["name"], "y");
    }

    // Guards unchanged/no-match edit outcomes returning the entire input as a
    // replacement mirror.
    #[test]
    fn no_change_edit_results_omit_source_mirrors() {
        let formatted = "var y;\nmodel;\n\ty = 0;\nend;\n";
        for named in [false, true] {
            for full in [false, true] {
                for (tool, text, current_selector, reason) in [
                    (
                        "dynare_rename",
                        MODEL,
                        json!({"symbol":{"kind":"name","name":"a"},"new_name":"a"}),
                        "SAME_NAME",
                    ),
                    (
                        "dynare_rename",
                        MODEL,
                        json!({"symbol":{"kind":"name","name":"absent_binding"},"new_name":"rho"}),
                        "NO_MATCH",
                    ),
                    ("dynare_format", formatted, json!({}), "NO_CHANGE"),
                    ("dynare_actions", MODEL, json!({"fixes":true}), "NO_CHANGE"),
                ] {
                    let input = if named {
                        json!({"kind":"map","root":"root.mod","files":{"root.mod":text}})
                    } else {
                        source(text)
                    };
                    let mut current = json!({"source":input});
                    current
                        .as_object_mut()
                        .unwrap()
                        .extend(current_selector.as_object().unwrap().clone());
                    if full {
                        current["detail"] = json!("full");
                    }
                    let result = call(tool, current);
                    assert_eq!(
                        result["outcome"]["status"], "unchanged",
                        "{tool}, named={named}, full={full}"
                    );
                    assert!(
                        result["outcome"]["reasons"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|row| row["code"] == reason)
                    );
                    assert!(result["proposal"].is_null());
                    assert!(!result["inputs"][0]["guards"].as_array().unwrap().is_empty());
                    for field in ["text", "files", "formatted_text"] {
                        assert!(
                            result["data"].get(field).is_none(),
                            "{tool}, named={named}, full={full}: {field}"
                        );
                    }
                }
            }
        }
        let fragment = call(
            "dynare_extract",
            json!({"source":source(MODEL),"equation":{"kind":"index","scope":"aggregate","index":0},"detail":"full"}),
        );
        assert!(fragment["data"]["fragment"].as_str().unwrap().contains("a"));
        let expanded = call(
            "dynare_expand",
            json!({"source":source(MODEL),"detail":"full"}),
        );
        assert!(
            expanded["data"]["effective_text"]
                .as_str()
                .unwrap()
                .contains("a")
        );
    }

    // Guards source-free language and diagnostic queries being split across
    // retired wrappers, and exact syntax options losing their value rules.
    #[test]
    fn syntax_and_diagnostic_reference_return_selected_details_in_one_call() {
        let syntax = call("dynare_syntax", json!({"name":"pac_model"}));
        assert!(!syntax["data"]["forms"].as_array().unwrap().is_empty());
        let option = call(
            "dynare_syntax",
            json!({"name":"irf","kind":"option","parent":"stoch_simul"}),
        );
        assert!(
            !option["data"]["forms"][0]["signatures"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let wrong = call(
            "dynare_syntax",
            json!({"name":"pac_model","context":"macro_expression"}),
        );
        assert_eq!(wrong["outcome"]["reasons"][0]["code"], "WRONG_CONTEXT");
        let info = call("dynare_diagnostic_info", json!({"code":"W010"}));
        assert!(
            !info["data"]["diagnostics"][0]["explanation"]
                .as_str()
                .unwrap()
                .is_empty()
        );
        assert!(info["inputs"].as_array().unwrap().is_empty());
    }

    // Guards collection budgets multiplying by root count or section count.
    #[test]
    fn batch_and_inspection_collections_share_one_row_allowance() {
        let diagnosed = call(
            "dynare_diagnose",
            json!({"batch":{"kind":"map","files":{"a.mod":MODEL,"b.mod":MODEL},"roots":["a.mod","b.mod"]},"page_size":1}),
        );
        let returned: usize = diagnosed["data"]["roots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|root| root["diagnostics"].as_array().unwrap().len())
            .sum();
        assert_eq!(returned, 1);
        assert!(diagnosed["selection"]["continuation"].is_string());
        let overview = call(
            "dynare_model_info",
            json!({"source":source(MODEL),"sections":["declarations","statements"],"page_size":2}),
        );
        let returned: u64 = overview["selection"]["collections"]
            .as_object()
            .unwrap()
            .values()
            .map(|collection| collection["returned_count"].as_u64().unwrap())
            .sum();
        assert_eq!(returned, 2);
    }

    // Guards a same-name tool accepting a continuation from another contract
    // after defaults and collection semantics changed.
    // Guards two typed comparison sources silently losing virtual include
    // folders, or returning full source text by default.
    #[test]
    fn comparison_uses_explicit_sources_and_preserves_include_policy() {
        let before = json!({"kind":"map","root":"root.mod","files":{"root.mod":"@#include \"body.inc\"\n","shared/body.inc":MODEL},"include_dirs":["shared"]});
        let after = json!({"kind":"map","root":"root.mod","files":{"root.mod":"@#include \"body.inc\"\n","shared/body.inc":MODEL.replace(".5",".7")},"include_dirs":["shared"]});
        let result = call(
            "dynare_compare_models",
            json!({"comparison":{"kind":"sources","before":before,"after":after}}),
        );
        assert_eq!(result["coverage"]["inputs"]["before"]["complete"], true);
        assert_eq!(result["coverage"]["inputs"]["after"]["complete"], true);
        assert!(
            result["data"]["semantic"]["rows"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["family"] == "parameters")
        );
        assert_eq!(result["inputs"][0]["include_dirs"], json!(["shared"]));
        assert!(result["data"]["sources"].is_null());
    }
}
