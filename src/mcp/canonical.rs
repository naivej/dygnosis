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
use crate::engine::analysis_configuration::AnalysisConfiguration;

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
    ConfiguredSources {
        jobs: Vec<ConfiguredSource>,
    },
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
#[serde(deny_unknown_fields)]
struct ConfiguredSource {
    source: SourceInput,
    analysis: AnalysisConfiguration,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum View {
    #[default]
    Effective,
    Source,
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

/// Typed projection inventory shared by discovery and decoding.
#[derive(Debug, Deserialize, JsonSchema)]
#[allow(dead_code)] // Most fields pass unchanged to the common worker after typed decoding.
struct Params {
    /// Explicit request-local macro choices; saved editor presets are not inputs.
    analysis: Option<AnalysisConfiguration>,
    /// Independent choices for the Before side of comparison.
    analysis_a: Option<AnalysisConfiguration>,
    /// Independent choices for the After side of comparison.
    analysis_b: Option<AnalysisConfiguration>,
    /// Effective model facts (default) or labelled written source candidates/templates.
    view: Option<View>,
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
    /// Owning option for fixed-value lookup; requires parent.
    option: Option<String>,
    /// Written position for a fixed choice. Use with parent and without option. Omit kind or use syntax_value. For boolean_literal, use context=macro_expression and omit parent.
    slot: Option<String>,
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
    /// Compact by default. Full includes optional explanation.
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
        fields.extend([
            "source",
            "analysis",
            "expected_revision",
            "macro_work_limit",
        ]);
    }
    fields.extend_from_slice(match tool {
        "dynare_syntax" => &[
            "name", "search", "kind", "context", "parent", "option", "slot",
        ],
        "dynare_diagnostic_info" => &["code", "kind", "severity", "search"],
        "dynare_diagnose" => &[
            "batch",
            "expected_revisions",
            "code",
            "severity",
            "file",
            "root",
        ],
        "dynare_model_info" => &["file"],
        "dynare_symbol_info" => &["symbol", "view", "include_declaration", "occurrences"],
        "dynare_equations" => &[
            "symbol",
            "equation",
            "name",
            "tags",
            "scope",
            "dimension",
            "view",
            "file",
        ],
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
            "analysis_a",
            "analysis_b",
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
    if tool == "dynare_equations" {
        schema.insert("allOf".into(),json!([{"if":{"required":["view"],"properties":{"view":{"const":"source"}}},"then":{"not":{"anyOf":[{"required":["symbol"]},{"required":["equation"]},{"required":["scope"]},{"required":["dimension"]}]}}}]));
    }
    if tool == "dynare_symbol_info" {
        schema.insert("allOf".into(),json!([{"if":{"required":["view"],"properties":{"view":{"const":"source"}}},"then":{"properties":{"sections":{"maxItems":0}},"not":{"anyOf":[{"required":["occurrences"]},{"required":["include_declaration"]},{"properties":{"symbol":{"required":["binding_id"]}},"required":["symbol"]}]}}}]));
    }
    if tool == "dynare_syntax" {
        schema.insert(
            "allOf".into(),
            json!([{
                "if": {"required": ["slot"], "properties": {"slot": {"type": "string"}}},
                "then": {
                    "properties": {
                        "option": {"type": "null"},
                        "kind": {"enum": [null, "syntax_value"]}
                    },
                    "if": {"properties": {"slot": {"const": "boolean_literal"}}},
                    "then": {
                        "required": ["context"],
                        "properties": {
                            "context": {"const": "macro_expression"},
                            "parent": {"type": "null"}
                        }
                    },
                    "else": {
                        "required": ["parent"],
                        "properties": {"parent": {"type": "string"}}
                    }
                }
            }, {
                "if": {"required": ["option"], "properties": {"option": {"type": "string"}}},
                "then": {
                    "required": ["parent"],
                    "properties": {
                        "parent": {"type": "string"},
                        "kind": {"enum": [null, "option_value"]}
                    }
                }
            }]),
        );
    }
    super::request::prune_definitions(&mut schema);
    Arc::new(schema)
}

pub(super) fn advertised_tools(structured: bool) -> Vec<Tool> {
    TOOL_NAMES.iter().map(|&name|{
        let description=match name {
            "dynare_syntax"=>"Look up Dynare syntax and fixed choices. Use name or search to find forms. Use kind, context and parent to narrow results. For a command option, set parent to the command and option to the option name. For another written position, use parent and slot, such as change_type and target_type. Search with kind=nested_setting to find nested paths. Use a case-sensitive path as slot, such as optim.NumgradAlgorithm, to get its value rules and selector conditions. Use either option or slot. With slot, omit kind or set it to syntax_value. For macro booleans, use context=macro_expression and slot=boolean_literal and omit parent. Omit name to list choices. Write numeric names as strings. No model source is needed.",
            "dynare_diagnostic_info"=>"Look up diagnostic meaning and correction guidance. An exact code returns its explanation; kind, severity and search return bounded catalog summaries. This catalog does not report problems in a model. No model source is needed.",
            "dynare_diagnose"=>"Check one source with optional analysis choices, or an explicit batch. configured_sources.jobs contains source and analysis for each job and permits repeated roots. job:1, job:2 and later identities key expected_revisions; each job spends the finite root-job allowance. Other batches cannot share analysis. Results distinguish jobs from distinct roots, and retain each partial, failed or unfinished cause. An incomplete zero count is not a clean-model claim.",
            "dynare_model_info"=>"Give a configured model overview or inspect written source. Default summary has effective counts and model kind before transformations. Select analysis for explicit choices and override receipts; source_structure returns written items, guarded directive targets and branch IDs, availability and separate source coverage. Optional file selects one source under the same input policy, without making it an executed include. Other sections are structure, declarations, assignments, model_locals, statements, blocks, sources, includes and scope. Written source-item counts are not effective equation counts.",
            "dynare_symbol_info"=>"Inspect a resolved binding with view=effective (default), using a unique name, written target or returned binding identity. Default sections are definition, metadata and references; assignments and expanded occurrences are optional. References-only works directly. view=source accepts a literal name or written target and returns written candidates with availability and coverage. An inactive name use can select a written declaration or model-local definition through literal includes, with available labels or expression. Source results have no binding identity, accepted references, parameter values or model timing.",
            "dynare_equations"=>"Inspect view=effective equations (default), tags, timing and verified locations under the requested analysis. Name, tag, scope/dimension and resolved-symbol filters combine with AND; scoped indices are zero-based. Incomplete analysis withholds authoritative counts. view=source returns written equation templates filtered by name, tags or file, with source_id, conditions, availability and separate coverage. Source-item counts are not equation counts; source IDs cannot select expansion or extraction.",
            "dynare_expand"=>"Show include/macro-expanded text with verified written origins. Focus with inclusive line_range, exclusive-end expanded_range or an equation identity. A range cutting a row omits its origin and states why. Partial expansion retains available text and its causes.",
            "dynare_actions"=>"Discover source action summaries or request a complete verified proposal. Select action_id with its expected_revision for one alternative. Set fixes=true to propose compatible proven diagnostic fixes, optionally by code, target or diagnostic_ids, without a discovery call. Alternatives are never combined automatically. Skipped and remaining work is explained. No file is written.",
            "dynare_rename"=>"Propose a complete rename of one resolved binding. A unique name works directly; a target or binding identity can disambiguate. Incomplete sources, collisions, generated names or incompatible checked owners withhold the complete proposal with a cause. No file is written.",
            "dynare_format"=>"Propose whitespace formatting for the root or selected source file. Optional whole-line line_range is one-based and inclusive. formatIndent is tab or one to eight spaces. Native text and source outside the selected lines retain their layout. No file is written.",
            "dynare_extract"=>"Produce a selected equation fragment with required declarations, local definitions and model context. Names combine with OR; tags combine with AND. Select a returned identity or scoped zero-based index for unnamed equations. Omitted setup is explicit; no numerical or runnable-model claim is inferred.",
            "dynare_compare_models"=>"Compare two model snapshots with independent analysis_a and analysis_b choices. Same source text can produce different model facts; both choices and revisions remain explicit. Sources use text/map/disk inputs; current disk inputs share validated positive and negative file facts. Repository input selects local Git or saved Working versions on the server host; no fetch or editor-buffer read occurs. Historical targets use that side's source revisions. Default sections include semantic changes and their direct locations. Request references explicitly to include usages. Semantic changes, structural entries, references and changed source files share page_size; detail does not select additional sections. Filters select the same changes across sections. Request source for whole-file change context, or source_text for captured file contents. Reference owners are snapshot_pointer keys scoped to the two input revisions, including changes on other pages. Navigation needs semantic, structural or reference facts in the selected sections. No numerical equivalence is inferred.",
            _=>unreachable!(),
        };
        let configuration = if matches!(name,"dynare_syntax"|"dynare_diagnostic_info") {String::new()} else {
            let argument=if name=="dynare_compare_models"{"analysis_a and analysis_b"}else{"analysis"};
            format!(" Optional {argument} selects macro inputs, guarded overrides and equation tags by explicit values. Equation selection uses a tag name (default name) and one or more tag values. Omission selects Source; names do not load editor presets. Discover override targets with dynare_model_info source_structure. Overrides and equation selection create previews that withhold Rename and diagnostic-fix proposals. Equation selection follows full-source Parse and Check; failed selection withholds model facts. Read application receipts and coverage separately. Invocation settings and hooks are not imported.")};
        let source_contract = if matches!(name,"dynare_syntax"|"dynare_diagnostic_info") { "" } else {
            " Source maps read only supplied files. Positions are one-based Unicode scalars with exclusive ends. Reused identities require their original expected revision guards."
        };
        let edit_contract = if matches!(name,"dynare_actions"|"dynare_rename"|"dynare_format") {
            " A proposal is atomic: verify every input guard and expected_text before applying the whole edit set with external file tools."
        } else { "" };
        let mut tool=Tool::new(name,format!("{description}{configuration} Read outcome and coverage beside data. Default 25 rows share one response allowance; selection reports counts and continuation. Changed inputs or filters require a new first page.{source_contract}{edit_contract} No tool writes files or executes MATLAB/Octave."),input_schema(name))
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
            "dynare_symbol_info" if params.view != Some(View::Source) => {
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
    if tool == "dynare_diagnose" && params.batch.is_some() && params.analysis.is_some() {
        return Err(invalid(
            "A batch cannot share analysis choices. Use batch.kind=configured_sources with explicit source and analysis jobs.",
        ));
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
                BatchInput::ConfiguredSources { jobs } => {
                    json!({"configured_sources":jobs.iter().map(|job|json!({"source":job.source,"analysis":job.analysis})).collect::<Vec<_>>()})
                }
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
        if let Some(analysis) = arguments.as_object_mut().unwrap().remove("analysis_a") {
            arguments["analysis_before"] = analysis;
        }
        if let Some(analysis) = arguments.as_object_mut().unwrap().remove("analysis_b") {
            arguments["analysis_after"] = analysis;
        }
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
        if params.view == Some(View::Source)
            && (params.occurrences.is_some()
                || params.include_declaration.is_some()
                || !params.sections.is_empty())
        {
            return Err(invalid(
                "Source symbol view returns candidates. Effective sections, occurrences and include_declaration are unavailable in this view.",
            ));
        }
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
        if tool == "dynare_syntax" {
            syntax_revision()
        } else {
            env!("CARGO_PKG_VERSION")
        },
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
    let complete = !result
        .outcome
        .reasons
        .iter()
        .any(|reason| reason.code == "VALUE_REFERENCE_INCOMPLETE");
    page_collections("", &mut result.data, &mut pages, complete);
    result.selection = pages.finish(&options);
    Ok(result.wire())
}

fn syntax_revision() -> &'static str {
    concat!(
        "syntax-sources-v1:",
        env!("CARGO_PKG_VERSION"),
        ":",
        env!("DYGNOSIS_SYNTAX_REVISION")
    )
}

fn syntax(params: &Params, arguments: &Value) -> Result<CallToolResult, rmcp::ErrorData> {
    use crate::engine::catalog::syntax::{self, SyntaxQuery};
    if let Some(slot) = params.slot.as_deref() {
        if params.option.is_some() {
            return Err(invalid(
                "The slot and option filters select different positions. Use one filter to look up its choices.",
            ));
        }
        if params
            .kind
            .as_deref()
            .is_some_and(|kind| kind != "syntax_value")
        {
            return Err(invalid(
                "The slot filter selects written choices. Omit kind or set it to syntax_value.",
            ));
        }
        if slot == "boolean_literal" {
            if params.parent.is_some() || params.context.as_deref() != Some("macro_expression") {
                return Err(invalid(
                    "Macro booleans require context=macro_expression and no parent.",
                ));
            }
        } else if params.parent.is_none() {
            return Err(invalid(
                "The slot filter requires its owning construct in parent. Set parent and try again.",
            ));
        }
    }
    if params.option.is_some() && params.parent.is_none() {
        return Err(invalid(
            "The option filter requires a command in parent. No values were returned. Set parent to the command and try again.",
        ));
    }
    if params.option.is_some()
        && params
            .kind
            .as_deref()
            .is_some_and(|kind| kind != "option_value")
    {
        return Err(invalid(
            "The option filter selects option values, but kind selects another syntax form. No values were returned. Omit kind or set it to option_value.",
        ));
    }
    let owner = params.option.as_ref().and_then(|option| {
        syntax::option_record(params.parent.as_deref().unwrap_or_default(), option)
    });
    let written_slot = params
        .slot
        .as_deref()
        .and_then(|slot| syntax::written_slot(params.parent.as_deref(), slot));
    let slot_settings: Vec<_> = written_slot
        .into_iter()
        .flat_map(|slot| slot.settings.iter().copied())
        .filter(|row| {
            params.context.as_ref().is_none_or(|context| {
                row.contexts
                    .iter()
                    .any(|known| known.eq_ignore_ascii_case(context))
            })
        })
        .collect();
    let query = SyntaxQuery {
        name: params.name.clone(),
        search: params.search.clone(),
        kind: params.kind.clone(),
        context: params.context.clone(),
        parent: params.parent.clone(),
        option: params.option.clone(),
        slot: params.slot.clone(),
    };
    let found = syntax::query(&query);
    let exact_option =
        params.name.is_some() && found.records.len() == 1 && found.records[0].kind == "option";
    let mut missing = Vec::new();
    let mut unresolved = Vec::new();
    for row in &found.records {
        if let Some(value) = &row.value
            && value.description.is_empty()
        {
            missing.push(format!(
                "{}.{}={}",
                row.parent.as_deref().unwrap_or_default(),
                row.option.as_deref().unwrap_or_default(),
                row.name
            ));
        }
        if exact_option && let Some(domain) = &row.value_domain {
            if !domain.complete {
                unresolved.push(format!(
                    "{}.{}: {}",
                    row.parent.as_deref().unwrap_or_default(),
                    row.name,
                    domain.gaps.join("; ")
                ));
            }
            for value in &domain.choices {
                if value.description.is_empty() {
                    missing.push(format!(
                        "{}.{}={}",
                        row.parent.as_deref().unwrap_or_default(),
                        row.name,
                        value.value
                    ));
                }
            }
        }
    }
    if params.name.is_none()
        && let Some(row) = owner
        && let Some(domain) = &row.value_domain
        && !domain.complete
    {
        unresolved.push(format!(
            "{}.{}: {}",
            row.parent.as_deref().unwrap_or_default(),
            row.name,
            domain.gaps.join("; ")
        ));
    }
    if params.name.is_none() || found.records.is_empty() {
        for row in &slot_settings {
            if let Some(domain) = &row.value_domain
                && !domain.complete
            {
                unresolved.push(format!(
                    "{}.{}: {}",
                    row.parent.as_deref().unwrap_or_default(),
                    row.slot.as_deref().unwrap_or_default(),
                    domain.gaps.join("; ")
                ));
            }
        }
        unresolved.sort_unstable();
        unresolved.dedup();
    }
    let mut result = ToolResult::new(
        json!({"forms":found.records,"available_contexts":found.available_contexts}),
    );
    if let Some(row) = owner {
        result.data["option_domain"] = domain_summary(row);
    }
    if !slot_settings.is_empty() {
        result.data["slot_domains"] = json!(slot_settings);
    }
    let empty = result.data["forms"].as_array().is_none_or(Vec::is_empty);
    if params.option.is_some() && owner.is_none() {
        let command = params.parent.as_deref().unwrap_or_default();
        let option = params.option.as_deref().unwrap_or_default();
        let known_command = syntax::records().iter().any(|row| {
            row.name.eq_ignore_ascii_case(command)
                && matches!(row.kind.as_str(), "command" | "block" | "declaration")
        });
        let known_option = syntax::records()
            .iter()
            .any(|row| row.kind == "option" && row.name.eq_ignore_ascii_case(option));
        result.cause("refused", if known_command && known_option {"WRONG_CONTEXT"} else {"UNKNOWN_SYNTAX"},
            format!("No reference entry was found for option '{option}' under '{command}'. No values were returned. Check the command and option, or omit option to inspect the command."));
    } else if empty
        && let Some(slot) = params.slot.as_deref()
        && written_slot.is_none()
    {
        let known_slot = syntax::records()
            .iter()
            .any(|row| row.slot.as_deref() == Some(slot));
        let known_parent = params.parent.as_deref().is_none_or(|parent| {
            syntax::records().iter().any(|row| {
                row.name.eq_ignore_ascii_case(parent)
                    && matches!(row.kind.as_str(), "command" | "block" | "declaration")
            })
        });
        result.cause(
            "refused",
            if known_slot && known_parent { "WRONG_CONTEXT" } else { "UNKNOWN_SYNTAX" },
            format!("No written choice was found for slot '{slot}' in the selected construct. Check parent and slot, or omit slot to inspect the construct."),
        );
    } else if empty && !slot_settings.is_empty() && !unresolved.is_empty() {
        result.cause(
            "partial",
            "VALUE_REFERENCE_INCOMPLETE",
            format!(
                "Returned the known value rules. Fixed-choice coverage is unresolved for {}.",
                unresolved.join(", ")
            ),
        );
    } else if empty && params.name.is_some() {
        let name = params.name.as_deref().unwrap_or_default();
        let details = if slot_settings.is_empty() {
            "No value facts were returned."
        } else {
            "Returned the written slot rules. No matching fixed value was returned."
        };
        result.cause("refused", if found.known_name {"WRONG_CONTEXT"} else {"UNKNOWN_SYNTAX"}, format!(
            "No reference form of '{name}' matches kind={}, context={}, parent={}, option={}, slot={}. {details} Check these selectors, or omit name to list available forms. A missing reference entry does not establish that Dynare rejects the source.",
            params.kind.as_deref().unwrap_or("any"),params.context.as_deref().unwrap_or("any"),params.parent.as_deref().unwrap_or("any"),params.option.as_deref().unwrap_or("any"),params.slot.as_deref().unwrap_or("any")));
    } else if empty && !slot_settings.is_empty() {
        result.data["note"] =
            json!("Returned the written slot rules. No fixed choices match this query.");
    } else if empty && let Some(slot) = params.slot.as_deref() {
        if let Some(context) = params.context.as_deref()
            && !syntax::slot_values(params.parent.as_deref(), slot).any(|row| {
                row.contexts
                    .iter()
                    .any(|known| known.eq_ignore_ascii_case(context))
            })
        {
            result.cause("refused", "WRONG_CONTEXT", format!(
                "Slot '{slot}' has no choices in context '{context}'. Omit context or select the context of the written slot."
            ));
        } else {
            result.data["note"] = json!(
                "No choices match the selected search. Omit search to list choices for this slot."
            );
        }
    } else if empty
        && owner.is_some()
        && params
            .context
            .as_deref()
            .is_some_and(|context| context != "option_value")
    {
        result.cause("refused", "WRONG_CONTEXT", "The option filter selects value forms in context=option_value. No values were returned. Omit context or select option_value.");
    } else {
        if !missing.is_empty() {
            result.cause("partial", "VALUE_MEANING_UNAVAILABLE", format!("Returned the available syntax facts. Meanings are unavailable in the bundled reference for: {}.", missing.join(", ")));
        }
        if !unresolved.is_empty() {
            result.cause("partial", "VALUE_REFERENCE_INCOMPLETE", format!("Returned the known value rules and choices. Fixed-choice coverage is unresolved for: {}.", unresolved.join(", ")));
        }
        if empty && let Some(row) = owner {
            result.data["note"] = json!(if row
                .value_rules
                .iter()
                .all(|rule| rule.starts_with("Flag;"))
            {
                "This option is a flag; no value follows the key."
            } else if row
                .value_domain
                .as_ref()
                .is_some_and(|domain| !domain.choices.is_empty())
            {
                "No fixed choices match the selected filters. Omit search or context to list the available choices."
            } else if row
                .value_domain
                .as_ref()
                .is_some_and(|domain| !domain.complete)
            {
                "Returned the known open value rules. Fixed-choice coverage is unresolved."
            } else {
                "Returned the open value rules. The reference lists no fixed choices for this option."
            });
        }
    }
    if result.data["forms"]
        .as_array()
        .is_some_and(|forms| forms.len() > 1)
    {
        for form in result.data["forms"].as_array_mut().unwrap() {
            if let Some(options) = form.as_object_mut().unwrap().remove("options") {
                form["option_count"] = json!(options.as_array().map_or(0, Vec::len));
            }
            if let Some(domain) = form.get_mut("value_domain").and_then(Value::as_object_mut)
                && let Some(choices) = domain.remove("choices")
            {
                domain.insert(
                    "choice_count".into(),
                    json!(choices.as_array().map_or(0, Vec::len)),
                );
            }
        }
    }
    reference_result("dynare_syntax", params, arguments, result)
}

fn domain_summary(row: &crate::engine::catalog::syntax::SyntaxRecord) -> Value {
    let mut domain = serde_json::to_value(&row.value_domain).expect("value domain");
    if let Some(object) = domain.as_object_mut()
        && let Some(choices) = object.remove("choices")
    {
        object.insert(
            "choice_count".into(),
            json!(choices.as_array().map_or(0, Vec::len)),
        );
    }
    domain
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
                || (path == "forms"
                    && rows.len() == 1
                    && !matches!(
                        rows[0]["kind"].as_str(),
                        Some("option_value" | "syntax_value")
                    ));
            if container {
                for (index, row) in rows.iter_mut().enumerate() {
                    if path == "heterogeneous_equations" {
                        let dimension = row["dimension"].as_str().unwrap_or_default();
                        let block = row["block_index"].as_u64().unwrap_or(index as u64) as usize;
                        let collection = super::result::heterogeneous_collection(dimension, block);
                        if let Some(equations) = row.get_mut("equations") {
                            page_collections(&collection, equations, pages, complete);
                        }
                        continue;
                    }
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
            let complete = object
                .get("source_coverage")
                .or_else(|| {
                    object
                        .contains_key("source_item_count")
                        .then(|| object.get("coverage"))
                        .flatten()
                })
                .and_then(|coverage| coverage.get("complete"))
                .and_then(Value::as_bool)
                .unwrap_or(complete);
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
                    "manual_sources",
                    "runtime_sources",
                    "applicability",
                    "open_rules",
                    "gaps",
                    "value_rules",
                    "available_contexts",
                    "segments",
                    "origin_frames",
                    "tags",
                    "declaration",
                    "definition",
                    "scope",
                    "analysis",
                    "source_coverage",
                ]
                .contains(&key.as_str())
                    || key == "forms"
                        && (path.ends_with("value_domain") || path.ends_with("option_domain"))
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
    use std::collections::BTreeSet;
    #[test]
    fn option_value_queries_keep_the_option_owner_and_meaning() {
        // Guards dropped values and sharing a same-spelling value across options.
        let dl = call(
            "dynare_syntax",
            json!({"name":"dl","parent":"pac_model","option":"kind"}),
        );
        assert_eq!(dl["outcome"]["status"], "ok", "{dl}");
        assert_eq!(dl["data"]["forms"][0]["option"], "kind");
        assert!(
            dl["data"]["forms"][0]["summary"]
                .as_str()
                .unwrap()
                .contains("levels")
        );
        for option in ["dr", "lyapunov"] {
            let found = call(
                "dynare_syntax",
                json!({"name":"default","parent":"estimation","option":option}),
            );
            assert_eq!(found["data"]["forms"][0]["option"], option, "{found}");
        }
        let osr = call(
            "dynare_syntax",
            json!({"name":"13","parent":"osr","option":"opt_algo"}),
        );
        assert_eq!(osr["outcome"]["status"], "ok", "{osr}");
        assert_eq!(osr["data"]["forms"][0]["value"]["status"], "unavailable");
        assert!(
            osr["data"]["forms"][0]["value"]["applicability"]
                .as_str()
                .is_some_and(|text| text.contains("Unavailable in OSR"))
        );
    }

    #[test]
    fn written_choice_pages_keep_the_slot_and_shared_budget() {
        // Guards positional rows bypassing the page allowance or losing their owning role.
        let mut args = json!({"parent":"change_type","slot":"target_type","page_size":1});
        let mut values = BTreeSet::new();
        loop {
            let page = call("dynare_syntax", args.clone());
            assert_eq!(page["outcome"]["status"], "ok", "{page}");
            let rows = page["data"]["forms"].as_array().unwrap();
            assert_eq!(rows.len(), 1);
            for row in rows {
                assert_eq!(row["slot"], "target_type");
                assert_eq!(row["kind"], "syntax_value");
                assert!(values.insert(row["name"].as_str().unwrap().to_owned()));
            }
            let Some(next) = page["selection"]["continuation"].as_str() else {
                break;
            };
            if args.get("continuation").is_none() {
                let changed = call(
                    "dynare_syntax",
                    json!({
                        "parent":"change_type","slot":"interval","page_size":1,"continuation":next
                    }),
                );
                assert_eq!(changed["outcome"]["reasons"][0]["code"], "INPUT_CHANGED");
            }
            args["continuation"] = json!(next);
        }
        assert_eq!(
            values,
            BTreeSet::from([
                "var".into(),
                "varexo".into(),
                "varexo_det".into(),
                "parameters".into()
            ])
        );
    }

    #[test]
    fn scoped_choices_preserve_legacy_senses_and_validate_selectors() {
        // Guards a scoped sign replacing its generic operator or a slot leaking another owner.
        let legacy = call(
            "dynare_syntax",
            json!({"parent":"moment_calibration","kind":"operator","name":"+"}),
        );
        assert_eq!(legacy["data"]["forms"][0]["kind"], "operator");
        let exact = call(
            "dynare_syntax",
            json!({"parent":"moment_calibration","slot":"interval","name":"+"}),
        );
        assert!(
            exact["data"]["forms"][0]["summary"]
                .as_str()
                .unwrap()
                .contains("[0, Inf]")
        );
        let nested = call(
            "dynare_syntax",
            json!({"parent":"estimation","slot":"optim.NumgradAlgorithm","name":"2"}),
        );
        assert_eq!(
            nested["data"]["forms"][0]["selector"],
            json!({"option":"mode_compute","value":"4"})
        );
        let macro_value = call(
            "dynare_syntax",
            json!({"context":"macro_expression","slot":"boolean_literal","name":"true"}),
        );
        assert_eq!(
            macro_value["data"]["forms"][0]["value"]["insert_text"],
            "true"
        );
        for args in [
            json!({"parent":"estimation","slot":"target_type","option":"order"}),
            json!({"parent":"change_type","slot":"target_type","kind":"command"}),
            json!({"slot":"target_type"}),
            json!({"slot":"boolean_literal"}),
            json!({"parent":"model","context":"macro_expression","slot":"boolean_literal"}),
        ] {
            assert!(dispatch("dynare_syntax", args, RequestControl::default()).is_err());
        }
        let wrong = call(
            "dynare_syntax",
            json!({"parent":"model","slot":"target_type"}),
        );
        assert_eq!(wrong["outcome"]["reasons"][0]["code"], "WRONG_CONTEXT");
        let unknown = call(
            "dynare_syntax",
            json!({"parent":"model","slot":"unknown_slot"}),
        );
        assert_eq!(unknown["outcome"]["reasons"][0]["code"], "UNKNOWN_SYNTAX");
    }

    #[test]
    fn nested_paths_expose_scoped_conditions_and_open_rules() {
        let path = "posterior_sampler_options.optim.NumgradAlgorithm";
        let tarb = call(
            "dynare_syntax",
            json!({"parent":"estimation","slot":path,"name":"3"}),
        );
        assert_eq!(tarb["outcome"]["status"], "ok");
        let conditions = &tarb["data"]["forms"][0]["applicability"][0];
        assert_eq!(conditions[0]["option"], "posterior_sampling_method");
        assert_eq!(
            conditions[1],
            json!({"option":"mode_compute","value":"4","scope":"posterior_sampler_options"})
        );
        let proposal = call(
            "dynare_syntax",
            json!({"parent":"estimation","slot":"posterior_sampler_options.proposal_distribution","name":"rand_multivariate_student"}),
        );
        assert_eq!(
            proposal["data"]["forms"][0]["applicability"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        for parent in ["steady", "check"] {
            let open = call(
                "dynare_syntax",
                json!({"parent":parent,"slot":"fsolve_options"}),
            );
            assert_eq!(open["outcome"]["status"], "ok", "{open}");
            assert_eq!(open["data"]["forms"], json!([]));
            assert_eq!(
                open["data"]["slot_domains"][0]["value_domain"]["kind"],
                "open"
            );
        }
        let partial = call(
            "dynare_syntax",
            json!({"parent":"estimation","slot":"particle_filter_options.posterior_sampler"}),
        );
        let unknown = call(
            "dynare_syntax",
            json!({"parent":"steady","slot":"fsolve_options","name":"unlisted_value"}),
        );
        assert_eq!(unknown["outcome"]["status"], "refused");
        assert!(
            unknown["outcome"]["reasons"][0]["message"]
                .as_str()
                .unwrap()
                .contains("Returned the written slot rules.")
        );
        assert_eq!(
            unknown["data"]["slot_domains"][0]["value_domain"]["kind"],
            "open"
        );
        assert_eq!(
            partial["outcome"]["reasons"][0]["code"],
            "VALUE_REFERENCE_INCOMPLETE"
        );
        let wrong = call("dynare_syntax", json!({"parent":"osr","slot":path}));
        assert_eq!(wrong["outcome"]["reasons"][0]["code"], "WRONG_CONTEXT");
    }

    #[test]
    fn option_value_empty_queries_explain_filters_and_open_domains() {
        // Guards reporting an empty filtered page as an option with no fixed choices.
        let filtered = call(
            "dynare_syntax",
            json!({"parent":"pac_model","option":"kind","search":"no_such_choice"}),
        );
        assert_eq!(filtered["outcome"]["status"], "ok");
        assert!(
            filtered["data"]["note"]
                .as_str()
                .unwrap()
                .contains("filters")
        );
        assert_eq!(filtered["data"]["option_domain"]["kind"], "closed");
        let open = call(
            "dynare_syntax",
            json!({"parent":"stoch_simul","option":"dr_cycle_reduction_maxiter"}),
        );
        assert_eq!(open["outcome"]["status"], "ok", "{open}");
        assert_eq!(open["data"]["option_domain"]["kind"], "open");
        assert!(open["data"]["forms"].as_array().unwrap().is_empty());
        let unknown = call(
            "dynare_syntax",
            json!({"parent":"no_such_command","option":"kind"}),
        );
        assert_eq!(unknown["outcome"]["reasons"][0]["code"], "UNKNOWN_SYNTAX");
        assert!(
            dispatch(
                "dynare_syntax",
                json!({"option":"kind"}),
                RequestControl::default()
            )
            .is_err()
        );
    }

    #[test]
    fn standalone_boolean_meanings_do_not_leak_from_nested_settings() {
        // Guards unexplained boolean choices and nested key/value payloads exposed as outer values.
        for value in ["true", "false"] {
            let result = call(
                "dynare_syntax",
                json!({"parent":"estimation","option":"use_pct","name":value}),
            );
            assert_eq!(result["outcome"]["status"], "ok", "{result}");
            assert!(
                result["data"]["forms"][0]["summary"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
            );
        }
        for option in [
            "optim",
            "posterior_sampler_options",
            "particle_filter_options",
        ] {
            let result = call(
                "dynare_syntax",
                json!({"parent":"estimation","option":option}),
            );
            assert_eq!(result["outcome"]["status"], "ok", "{result}");
            assert!(
                result["data"]["forms"].as_array().unwrap().is_empty(),
                "{result}"
            );
        }
    }

    #[test]
    fn occbin_limits_are_open_and_continuations_keep_their_selector() {
        // Guards publishing the runtime default as a written choice or a reference gap.
        for (parent, option) in [
            ("occbin_setup", "simul_max_check_ahead_periods"),
            ("occbin_solver", "simul_max_check_ahead_periods"),
            ("occbin_setup", "smoother_max_check_ahead_periods"),
            ("occbin_setup", "likelihood_max_check_ahead_periods"),
        ] {
            let result = call("dynare_syntax", json!({"parent":parent,"option":option}));
            assert_eq!(result["outcome"]["status"], "ok", "{result}");
            assert_eq!(result["data"]["option_domain"]["kind"], "open");
            assert!(
                result["data"]["forms"].as_array().unwrap().is_empty(),
                "{result}"
            );
            let description = call(
                "dynare_syntax",
                json!({"parent":parent,"name":option,"kind":"option"}),
            );
            let summary = description["data"]["forms"][0]["summary"].as_str().unwrap();
            assert!(
                summary.contains("Inf") && summary.contains("integer"),
                "{description}"
            );
        }
        // A continuation cannot change its owner.
        let first = call(
            "dynare_syntax",
            json!({"parent":"pac_model","option":"kind","page_size":1}),
        );
        let changed = call(
            "dynare_syntax",
            json!({"parent":"estimation","option":"dr","page_size":1,"continuation":first["selection"]["continuation"]}),
        );
        assert_eq!(changed["outcome"]["reasons"][0]["code"], "INPUT_CHANGED");
    }

    #[test]
    fn exact_option_choices_use_the_shared_page_budget() {
        // Guards exact-option containers returning all choices outside the row allowance.
        let mut args = json!({"name":"kind","kind":"option","parent":"pac_model","page_size":1});
        let mut values = BTreeSet::new();
        loop {
            let page = call("dynare_syntax", args.clone());
            let rows = page["data"]["forms"][0]["value_domain"]["choices"]
                .as_array()
                .unwrap();
            assert!(rows.len() <= 1);
            for row in rows {
                assert!(values.insert(row["value"].as_str().unwrap().to_owned()));
            }
            let Some(next) = page["selection"]["continuation"].as_str() else {
                break;
            };
            args["continuation"] = json!(next);
        }
        assert!(values.contains("dl") && values.contains("dd") && values.contains("ll"));
    }

    #[test]
    fn unreached_override_reasons_keep_their_captured_target_and_branch_label() {
        // Guards associating two identical application causes with the wrong choice.
        let text = "@#if 0\n@#if A\n@#else\n@#endif\n@#if B\n@#else\n@#endif\n@#endif\nvar y; model; y=0; end;";
        let source = json!({"kind":"map","root":"main.mod","files":{"main.mod":text}});
        let discovery = call(
            "dynare_model_info",
            json!({"source":source,"sections":["source_structure"],"page_size":100}),
        );
        let directives = discovery["data"]["source_structure"]["directives"]
            .as_array()
            .unwrap();
        let choices: Vec<_> = directives.iter().filter(|site| matches!(site["expected_text"].as_str(),Some("@#if A"|"@#if B"))).map(|site|json!({"kind":"branch","target":site["target"],"branch_id":site["branches"][1]["branch_id"]})).collect();
        assert_eq!(choices.len(), 2);
        let result = call(
            "dynare_model_info",
            json!({"source":source,"analysis":{"overrides":choices},"sections":["summary"]}),
        );
        let reasons: Vec<_> = result["outcome"]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|reason| reason["code"] == "ANALYSIS_OVERRIDE_NOT_REACHED")
            .collect();
        assert_eq!(reasons.len(), 2);
        for (index, reason) in reasons.iter().enumerate() {
            assert_eq!(reason["target"], choices[index]["target"]);
            assert!(reason["message"].as_str().unwrap().contains(if index == 0 {
                "@#if A → @#else"
            } else {
                "@#if B → @#else"
            }));
        }
    }
    const MODEL: &str = "var y; varexo e; parameters a; a=.5; model; [name='eq'] y=a*y(-1)+e; end;";
    #[test]
    fn equation_pages_keep_repeated_dimension_blocks_and_empty_filtered_blocks() {
        // Guards lost later blocks, duplicate rows, and false complete counts.
        let text = "heterogeneity_dimension d; var y; var(heterogeneity=d) a b; model; [name='aggregate'] y=0; end; model(heterogeneity=d); [name='h0'] a=0; end; model(heterogeneity=d); [name='h1'] b=0; end;";
        for size in [1, 2, 25] {
            for selected in [None, Some("h1")] {
                let mut args = json!({"source":source(text),"page_size":size});
                if let Some(name) = selected {
                    args["name"] = json!(name);
                }
                let mut names = Vec::new();
                let mut ids = BTreeSet::new();
                for page in 0..20 {
                    let result = call("dynare_equations", args.clone());
                    assert_eq!(result["outcome"]["status"], "ok");
                    let data = &result["data"];
                    let mut rows = data["equations"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .collect::<Vec<_>>();
                    assert_eq!(
                        result["selection"]["collections"]["equations"]["returned_count"],
                        rows.len()
                    );
                    for block in data["heterogeneous_equations"]
                        .as_array()
                        .into_iter()
                        .flatten()
                    {
                        let collection = super::super::result::heterogeneous_collection(
                            block["dimension"].as_str().unwrap(),
                            block["block_index"].as_u64().unwrap() as usize,
                        );
                        let equations = block["equations"].as_array().unwrap();
                        assert_eq!(
                            result["selection"]["collections"][&collection]["returned_count"],
                            equations.len()
                        );
                        rows.extend(equations);
                    }
                    assert!(rows.len() <= size);
                    for row in rows {
                        names.push(row["name"].as_str().unwrap().to_owned());
                        assert!(ids.insert(row["equation_id"].as_str().unwrap().to_owned()));
                    }
                    if result["selection"]["continuation"].is_null() {
                        break;
                    }
                    assert!(page < 19, "continuation must finish");
                    args["continuation"] = result["selection"]["continuation"].clone();
                    args["expected_revision"] = result["inputs"][0]["revision"].clone();
                }
                assert_eq!(
                    names,
                    if selected.is_some() {
                        vec!["h1"]
                    } else {
                        vec!["aggregate", "h0", "h1"]
                    }
                );
            }
        }
    }
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

    #[test]
    fn equation_selection_accepts_direct_values_and_rejects_obsolete_file_input() {
        // Guards unknown file fields being rejected instead of silently
        // activating selector-file behavior, while direct tag values remain valid.
        for analysis in [
            json!({"equation_selection":{"mode":"include","file":"x","values":["a"]}}),
            json!({"equation_selection":{"mode":"include","file":"x"}}),
        ] {
            assert!(
                dispatch(
                    "dynare_model_info",
                    json!({"source":source(MODEL),"analysis":analysis}),
                    RequestControl::default(),
                )
                .is_err()
            );
        }
        let accepted = call(
            "dynare_model_info",
            json!({"source":source(MODEL),"analysis":{"equation_selection":{"mode":"include","values":["a"]}}}),
        );
        assert_ne!(accepted["outcome"]["status"], "refused");
        for field in ["analysis_a", "analysis_b"] {
            let mut arguments = json!({
                "comparison":{"before":{"source":source(MODEL)},"after":{"source":source(MODEL)}}
            });
            arguments[field] = json!({"equation_selection":{"mode":"include","file":"x"}});
            assert!(
                dispatch(
                    "dynare_compare_models",
                    arguments,
                    RequestControl::default(),
                )
                .is_err()
            );
        }
        let schema = input_schema("dynare_model_info");
        let selection = &schema["definitions"]["EquationSelection"];
        assert!(selection["properties"].get("file").is_none());
        assert!(selection["properties"].get("values").is_some());
        assert_eq!(selection["properties"]["values"]["minItems"], 1);
        assert_eq!(selection["properties"]["values"]["items"]["minLength"], 1);
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
