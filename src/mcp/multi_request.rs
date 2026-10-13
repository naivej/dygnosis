//! Bounded batch diagnosis and comparison over independent input snapshots.

use rmcp::model::{CallToolResult, ContentBlock};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::result::{Pages, ResultOptions, ToolResult};
use super::source::{self, RequestControl, SourceInput, SourceSnapshot};

#[derive(Debug, Deserialize, JsonSchema)]
struct BatchParams {
    /// Nonempty exact supplied file-key to UTF-8 text map, exclusive with paths.
    files: Option<BTreeMap<String, String>>,
    /// Nonempty exact root keys in files. Checked roots keep their input spelling.
    roots: Option<Vec<String>>,
    /// Absolute server-host model files or directories. Directory discovery skips + folders and applies exclusions before reading model text.
    paths: Option<Vec<String>>,
    #[serde(default)]
    include_dirs: Vec<String>,
    /// Exact keys or directory prefixes to omit from root discovery.
    #[serde(default)]
    exclude_paths: Vec<String>,
    /// Diagnostic code filter; totals still describe unfiltered diagnostics.
    code: Option<String>,
    severity: Option<String>,
    /// Exact source-file diagnostic filter.
    file: Option<String>,
    /// Exact root filter, applied before model analysis.
    root: Option<String>,
    /// Expected input revisions keyed by exact returned root identity.
    #[serde(default)]
    expected_revisions: BTreeMap<String, String>,
    #[serde(flatten)]
    output: ResultOptions,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CompareParams {
    #[serde(flatten)]
    base: super::CompareModelsParams,
    /// Expected Before source revision returned by the preceding comparison.
    expected_revision_a: Option<String>,
    /// Expected After source revision returned by the preceding comparison.
    expected_revision_b: Option<String>,
    /// Semantic family filters. Source and coverage retain separate selection.
    #[serde(default)]
    families: Vec<String>,
    /// Semantic change-kind filters.
    #[serde(default)]
    kinds: Vec<String>,
    #[serde(flatten)]
    output: ResultOptions,
}

pub(crate) fn input_schema(tool: &str) -> std::sync::Arc<serde_json::Map<String, Value>> {
    let generator = schemars::generate::SchemaSettings::draft07().into_generator();
    let schema = if tool == "dynare_workspace_diagnose" {
        generator.into_root_schema_for::<BatchParams>()
    } else {
        generator.into_root_schema_for::<CompareParams>()
    };
    let mut schema = schema.as_object().expect("input schema object").clone();
    if tool == "dynare_compare_models" {
        let absent = |keys: &[&str]| json!({"not":{"anyOf":keys.iter().map(|key| json!({"required":[key]})).collect::<Vec<_>>()}});
        schema.insert("oneOf".into(), json!([
            {"required":["file_content_a","file_content_b"],"allOf":[absent(super::COMPARE_REPOSITORY_KEYS)]},
            {"required":["repository_path","before","after"],"allOf":[absent(super::COMPARE_TEXT_KEYS)]}
        ]));
    } else {
        schema.insert("oneOf".into(), json!([
            {"required":["files","roots"],"not":{"required":["paths"]}},
            {"required":["paths"],"not":{"anyOf":[{"required":["files"]},{"required":["roots"]}]}}
        ]));
    }
    schema.insert("additionalProperties".into(), json!(false));
    std::sync::Arc::new(schema)
}

pub(crate) fn dispatch(
    tool: &str,
    arguments: Value,
    control: RequestControl,
) -> Result<CallToolResult, rmcp::ErrorData> {
    control.check().map_err(source_error)?;
    let schema = input_schema(tool);
    let properties = schema["properties"].as_object().expect("input properties");
    let object = arguments
        .as_object()
        .ok_or_else(|| invalid("Tool arguments must be an object"))?;
    if let Some(key) = object.keys().find(|key| !properties.contains_key(*key)) {
        return Err(invalid(format!("Unsupported argument: {key}")));
    }
    match tool {
        "dynare_workspace_diagnose" => batch(arguments, control),
        "dynare_compare_models" => comparison(arguments, control),
        _ => Err(invalid("Unknown multi-model tool")),
    }
}

fn invalid(message: impl Into<String>) -> rmcp::ErrorData {
    rmcp::ErrorData::invalid_params(message.into(), None)
}
fn source_error(error: source::SourceError) -> rmcp::ErrorData {
    if error.invalid_arguments {
        invalid(error.message)
    } else {
        rmcp::ErrorData::internal_error(format!("{}: {}", error.code, error.message), None)
    }
}

fn batch(arguments: Value, control: RequestControl) -> Result<CallToolResult, rmcp::ErrorData> {
    let params: BatchParams =
        serde_json::from_value(arguments.clone()).map_err(|error| invalid(error.to_string()))?;
    if params.output.continuation.is_some() && params.expected_revisions.is_empty() {
        return Err(invalid(
            "A batch continuation requires expected_revisions for its checked roots",
        ));
    }
    validate_severity(params.severity.as_deref())?;
    let map_mode = arguments.get("files").is_some() || arguments.get("roots").is_some();
    if map_mode && arguments.get("paths").is_some() {
        return Err(invalid(
            "Choose files with roots or paths; do not combine them",
        ));
    }
    if !map_mode && params.paths.as_ref().is_none_or(Vec::is_empty) {
        return Err(invalid(
            "Supply nonempty files with roots, or nonempty paths",
        ));
    }
    let exclusions: Vec<_> = params
        .exclude_paths
        .iter()
        .map(|path| {
            if map_mode {
                crate::engine::workspace::include_resolver::supplied_path_key(path)
            } else {
                crate::engine::workspace::include_resolver::normalize_uri(path)
            }
        })
        .collect();
    let (mut roots, discovery_complete, discovery_limits) = if map_mode {
        let files = params
            .files
            .as_ref()
            .filter(|files| !files.is_empty())
            .ok_or_else(|| invalid("files must be nonempty in map mode"))?;
        let roots = params
            .roots
            .as_ref()
            .filter(|roots| !roots.is_empty())
            .ok_or_else(|| invalid("roots must be nonempty in map mode"))?;
        let mut aliases = BTreeSet::new();
        for key in files.keys() {
            if !aliases.insert(crate::engine::workspace::include_resolver::supplied_path_key(key)) {
                return Err(invalid(
                    "Two supplied file keys resolve to the same lookup identity",
                ));
            }
        }
        (
            roots
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>(),
            true,
            Vec::new(),
        )
    } else {
        discover_paths(params.paths.as_ref().unwrap(), &exclusions, &control)?
    };
    roots.retain(|root| {
        params.root.as_ref().is_none_or(|filter| filter == root) && !excluded(root, &exclusions)
    });
    if roots.is_empty()
        && discovery_complete
        && params.root.is_none()
        && params.exclude_paths.is_empty()
    {
        return Err(invalid("No root files match the selected paths"));
    }
    let mut result = ToolResult::new(Value::Null);
    result.limits = discovery_limits;
    let mut reports = Vec::new();
    let mut coverages = Vec::new();
    let mut snapshots: Vec<(usize, SourceSnapshot)> = Vec::new();
    let mut revisions = BTreeMap::new();
    let mut captured_sources = BTreeMap::new();
    let mut checked_keys = BTreeSet::new();
    let mut bytes = 0usize;
    let mut stop = false;
    for (index, root) in roots.iter().enumerate() {
        control.check().map_err(source_error)?;
        if stop || index >= control.limits.max_roots {
            reports.push(json!({"root":root,"status":"unfinished","diagnostics":[],"failure":"Batch root or source limit reached before this root was checked","code":"WORK_LIMIT"}));
            coverages.push(json!({"root":root,"complete":false,"reasons":[{"code":"WORK_LIMIT","message":"This root was not checked"}]}));
            continue;
        }
        let input = if let Some(files) = &params.files {
            if !files.contains_key(root) {
                reports.push(json!({"root":root,"status":"failed","diagnostics":[],"failure":"The exact root key is absent from the supplied file map","code":"ROOT_NOT_FOUND"}));
                coverages.push(json!({"root":root,"complete":false,"reasons":[{"code":"ROOT_NOT_FOUND","message":"The exact root key is absent from the supplied file map"}]}));
                continue;
            }
            SourceInput::Map {
                root: root.clone(),
                files: files.clone(),
                root_text: None,
                include_dirs: params.include_dirs.clone(),
                owner_roots: Vec::new(),
            }
        } else {
            SourceInput::Disk {
                root: root.clone(),
                include_dirs: params.include_dirs.clone(),
                owner_roots: Vec::new(),
            }
        };
        let expected = params.expected_revisions.get(root).map(String::as_str);
        match source::resolve_source(input, expected, &control) {
            Ok(mut snapshot) => {
                if params.output.continuation.is_some()
                    && !params.expected_revisions.contains_key(root)
                {
                    return Err(invalid("A batch continuation requires expected_revisions for every available checked root"));
                }
                let new_bytes: usize = snapshot
                    .sources
                    .iter()
                    .filter(|(file, text)| captured_sources.get(*file) != Some(*text))
                    .map(|(_, text)| text.len())
                    .sum();
                let next_checked_keys: BTreeSet<_> = checked_keys
                    .iter()
                    .cloned()
                    .chain(snapshot.guards.iter().map(|guard| guard.file.clone()))
                    .chain(snapshot.sources.keys().cloned())
                    .collect();
                if bytes.saturating_add(new_bytes) > control.limits.max_source_bytes
                    || next_checked_keys.len() > control.limits.max_files
                {
                    stop = true;
                    reports.push(json!({"root":root,"status":"unfinished","diagnostics":[],"failure":"The request source-byte or checked-file and lookup-candidate limit was reached. Source capture finished for this root; its diagnostic checks were withheld.","code":"SOURCE_LIMIT","completed":["source capture"],"omitted":["diagnostic checks"]}));
                    coverages.push(json!({"root":root,"complete":false,"reasons":[{"code":"SOURCE_LIMIT","message":"This root was not checked"}]}));
                    continue;
                }
                bytes += new_bytes;
                checked_keys = next_checked_keys;
                captured_sources.extend(
                    snapshot
                        .sources
                        .iter()
                        .map(|(file, text)| (file.clone(), text.clone())),
                );
                revisions.insert(root.clone(), snapshot.revision.clone());
                result.inputs.push(snapshot.inputs.clone());
                coverages.push(snapshot.coverage.clone());
                let include_failure=snapshot.workspace.include_records(&snapshot.root).and_then(|records| {
                    if !records.unresolved.is_empty() {Some(("MISSING_INCLUDE","A required include is unresolved; this root has no diagnostic result"))}
                    else if !records.cycles.is_empty() {Some(("INCLUDE_CYCLE","A required include forms a cycle; diagnostic checks were withheld"))}
                    else {None}
                });
                if let Some((code, failure)) = include_failure {
                    reports.push(json!({"root":root,"status":"failed","diagnostics":[],"failure":failure,"code":code}));
                } else {
                    let set =
                        crate::engine::diagnostics::check_in_workspace_with_origins_controlled(
                            &mut snapshot.workspace,
                            &snapshot.root,
                            &control.cancelled,
                        )
                        .map_err(|_| {
                            rmcp::ErrorData::internal_error(
                                "CANCELLED: Diagnostic checks were cancelled",
                                None,
                            )
                        })?;
                    control.check().map_err(source_error)?;
                    let diagnostics = batch_diagnostics(&snapshot, &set);
                    if snapshot.coverage["complete"] == true {
                        reports.push(json!({"root":root,"status":"ok","diagnostics":diagnostics,"failure":null}));
                    } else {
                        let mut reasons = snapshot.coverage["reasons"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default();
                        if reasons.is_empty() {
                            reasons.push(json!({"code":"INCOMPLETE_INPUT","message":"Model parsing, macro expansion or source mapping is incomplete"}));
                        }
                        reports.push(json!({"root":root,"status":"partial","diagnostics":diagnostics,"failure":"Available diagnostics were checked. Checks that require complete parsing, macro expansion or source mapping were withheld.","code":"INCOMPLETE_INPUT","reasons":reasons,"completed":["source capture","available diagnostic checks"],"omitted":["checks that require complete model analysis"]}));
                    }
                }
                snapshots.push((reports.len() - 1, snapshot));
                control.complete_phase("Diagnose root");
            }
            Err(error) if error.invalid_arguments => return Err(invalid(error.message)),
            Err(error) if error.code == "CANCELLED" => return Err(source_error(error)),
            Err(error) => {
                stop |= error.code == "SOURCE_LIMIT" || error.code == "ROOT_LIMIT";
                if error.code == "INPUT_CHANGED" {
                    result.cause("refused", "INPUT_CHANGED", error.message.clone());
                }
                reports.push(json!({"root":root,"status":if stop {"unfinished"} else {"failed"},"diagnostics":[],"failure":error.message,"code":error.code}));
                coverages.push(json!({"root":root,"complete":false,"reasons":[{"code":error.code,"message":error.message}]}));
            }
        }
    }
    for (index, snapshot) in &snapshots {
        control.check().map_err(source_error)?;
        if !snapshot.workspace.input_snapshot_is_current(&snapshot.root) {
            reports[*index] = json!({"root":reports[*index]["root"],"status":"failed","diagnostics":[],"failure":"Inputs changed while checking this batch root; repeat the request","code":"INPUT_CHANGED"});
            coverages[*index] = json!({"root":reports[*index]["root"],"complete":false,"reasons":[{"code":"INPUT_CHANGED","message":"Inputs changed while checking this batch root"}]});
        }
    }
    let mut summary = json!({"checked":0,"partial":0,"failed":0,"unfinished":0,"errors":0,"warnings":0,"information":0,"discovery_complete":discovery_complete,"diagnostic_totals_complete":coverages.iter().all(|coverage|coverage["complete"]==true)});
    for root in &reports {
        let field = match root["status"].as_str() {
            Some("ok") => "checked",
            Some("partial") => "partial",
            Some("failed") => "failed",
            _ => "unfinished",
        };
        summary[field] = json!(summary[field].as_u64().unwrap() + 1);
        for diagnostic in root["diagnostics"].as_array().into_iter().flatten() {
            let field = match diagnostic["severity"].as_str() {
                Some("ERROR") => "errors",
                Some("WARNING") => "warnings",
                _ => "information",
            };
            summary[field] = json!(summary[field].as_u64().unwrap() + 1);
        }
    }
    if reports.iter().any(|root| root["status"] != "ok")
        || coverages
            .iter()
            .any(|coverage| coverage["complete"] != true)
        || !discovery_complete
    {
        let available = reports
            .iter()
            .any(|root| root["status"] == "ok" || root["status"] == "partial");
        result.cause(if available {"partial"} else {"refused"}, "BATCH_INCOMPLETE", "The result retains completed and partially checked roots. Each partial, failed or unfinished root states its cause and unavailable work.");
    }
    if roots.len() > control.limits.max_roots || stop {
        result.limits.push(json!({"code":"WORK_LIMIT","root_ceiling":control.limits.max_roots,"source_byte_ceiling":control.limits.max_source_bytes,"checked_file_and_candidate_ceiling":control.limits.max_files,"omitted":"unfinished root analysis"}));
    }
    for root in &mut reports {
        if let Some(rows) = root["diagnostics"].as_array_mut() {
            rows.retain(|diagnostic| {
                params
                    .code
                    .as_ref()
                    .is_none_or(|code| diagnostic["code"] == *code)
                    && params.severity.as_ref().is_none_or(|severity| {
                        diagnostic["severity"]
                            .as_str()
                            .is_some_and(|value| value.eq_ignore_ascii_case(severity))
                    })
                    && params
                        .file
                        .as_ref()
                        .is_none_or(|file| diagnostic["file"] == *file)
            });
        }
    }
    let revision = source::sha256(serde_json::to_vec(&json!({"inputs":result.inputs,"roots":roots,"discovery_complete":discovery_complete,"revisions":revisions,"limits":control.limits})).expect("batch identity"));
    let mut pages = match Pages::new(
        "dynare_workspace_diagnose",
        &revision,
        &arguments,
        &params.output,
    ) {
        Ok(pages) => pages,
        Err(message) if message.starts_with("INPUT_CHANGED") => {
            return Ok(ToolResult::refused("INPUT_CHANGED", message)
                .wire(Vec::new(), params.output.detail))
        }
        Err(message) => return Err(invalid(message)),
    };
    for root in &mut reports {
        let collection = format!("diagnostics:{}", root["root"].as_str().unwrap_or(""));
        let complete = root["status"] == "ok";
        if let Some(rows) = root["diagnostics"].as_array_mut() {
            pages.collection_with_coverage(&collection, rows, complete);
        }
    }
    result.selection = pages.finish(&params.output);
    result.coverage = Some(json!({"roots":coverages,"discovery_complete":discovery_complete}));
    result.data =
        json!({"summary":summary,"roots":reports,"revision":revision,"root_revisions":revisions});
    let legacy = vec![ContentBlock::text(
        json!({"summary":summary,"roots":result.data["roots"]}).to_string(),
    )];
    control.check().map_err(source_error)?;
    Ok(result.wire(legacy, params.output.detail))
}

fn batch_diagnostics(
    snapshot: &SourceSnapshot,
    set: &crate::engine::diagnostics::DiagnosticSet,
) -> Vec<Value> {
    set.diagnostics.iter().enumerate().filter(|(_, diagnostic)| !super::is_dropped_code(&diagnostic.code)).map(|(index, diagnostic)| {
        let origin = set.origins.get(index).and_then(Option::as_ref);
        let (file, text, span) = origin.map(|origin| (origin.file.as_str(), origin.text.as_ref(), origin.span)).unwrap_or((&snapshot.root, &snapshot.raw, diagnostic.span));
        let line_index = crate::engine::language::span::LineIndex::new(text);
        let start = line_index.position(text, span.start);
        let end = line_index.position(text, span.end);
        json!({"file":file,"line":start.line+1,"column":start.character+1,"end_line":end.line+1,"end_column":end.character+1,"severity":super::mcp_severity(diagnostic.severity),"code":diagnostic.code,"message":diagnostic.message,"related":super::related_json(diagnostic, &snapshot.raw, |file| Some(file.into()))})
    }).collect()
}

fn validate_severity(value: Option<&str>) -> Result<(), rmcp::ErrorData> {
    if value.is_some_and(|value| {
        !["ERROR", "WARNING", "INFORMATION", "HINT"]
            .iter()
            .any(|expected| value.eq_ignore_ascii_case(expected))
    }) {
        Err(invalid(
            "severity must be ERROR, WARNING, INFORMATION or HINT",
        ))
    } else {
        Ok(())
    }
}

fn excluded(key: &str, exclusions: &[String]) -> bool {
    let key = key.replace('\\', "/");
    exclusions.iter().any(|exclude| {
        let exclude = exclude.replace('\\', "/");
        let exclude = exclude.trim_end_matches('/');
        key == exclude
            || key
                .strip_prefix(exclude)
                .is_some_and(|tail| tail.starts_with('/'))
    })
}

fn discover_paths(
    paths: &[String],
    exclusions: &[String],
    control: &RequestControl,
) -> Result<(Vec<String>, bool, Vec<Value>), rmcp::ErrorData> {
    let mut pending = BTreeSet::new();
    for path in paths {
        let path = Path::new(path);
        if !path.is_absolute() {
            return Err(invalid("Every path must be absolute on the server host"));
        }
        pending.insert(path.to_path_buf());
    }
    let mut seen = BTreeSet::new();
    let mut roots = BTreeSet::new();
    let mut visited = 0usize;
    while let Some(path) = pending.pop_first() {
        control.check().map_err(source_error)?;
        let key =
            crate::engine::workspace::include_resolver::normalize_uri(&path.to_string_lossy());
        if !seen.insert(key.clone()) || excluded(&key, exclusions) {
            continue;
        }
        visited += 1;
        if visited > control.limits.max_files || roots.len() >= control.limits.max_roots {
            return Ok((
                roots.into_iter().collect(),
                false,
                vec![
                    json!({"code":"DISCOVERY_LIMIT","omitted":"remaining path discovery; exact root count is unknown","directory_or_file":key}),
                ],
            ));
        }
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => {
                if path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with('+'))
                {
                    continue;
                }
                match std::fs::read_dir(&path) {
                    Ok(entries) => {
                        for entry in entries {
                            control.check().map_err(source_error)?;
                            let Ok(entry) = entry else {
                                roots.insert(key.clone());
                                break;
                            };
                            if pending.len().saturating_add(visited) > control.limits.max_files {
                                return Ok((
                                    roots.into_iter().collect(),
                                    false,
                                    vec![
                                        json!({"code":"DISCOVERY_LIMIT","omitted":"remaining directory entries; exact root count is unknown","directory_or_file":key}),
                                    ],
                                ));
                            }
                            pending.insert(entry.path());
                        }
                    }
                    Err(_) => {
                        roots.insert(key);
                    }
                }
            }
            Ok(_)
                if matches!(
                    path.extension().and_then(|value| value.to_str()),
                    Some("mod" | "dyn")
                ) =>
            {
                roots.insert(key);
            }
            Err(_) => {
                roots.insert(key);
            }
            _ => {}
        }
    }
    Ok((roots.into_iter().collect(), true, Vec::new()))
}

fn comparison(
    arguments: Value,
    control: RequestControl,
) -> Result<CallToolResult, rmcp::ErrorData> {
    let params: CompareParams =
        serde_json::from_value(arguments.clone()).map_err(|error| invalid(error.to_string()))?;
    params.base.validate().map_err(invalid)?;
    if params.output.continuation.is_some()
        && (params.expected_revision_a.is_none() || params.expected_revision_b.is_none())
    {
        return Err(invalid(
            "Comparison continuation requires expected_revision_a and expected_revision_b",
        ));
    }
    for kind in &params.kinds {
        if !matches!(kind.as_str(), "added" | "removed" | "changed") {
            return Err(invalid("kinds contains an unsupported change kind"));
        }
    }
    let (mut data, inputs, coverage) = if let Some(repository_path) = &params.base.repository_path {
        let data = super::repository_compare::compare_repository_bounded(
            super::repository_compare::RepositoryComparison {
                repository_path: repository_path.clone(),
                before: params.base.before.as_ref().unwrap().clone(),
                after: params.base.after.as_ref().unwrap().clone(),
                search_paths: params.base.search_paths.clone().unwrap_or_default(),
            },
            &control,
        );
        control.check().map_err(source_error)?;
        let inputs = vec![
            data["inputs"]["before"].clone(),
            data["inputs"]["after"].clone(),
        ];
        for (input, expected) in inputs.iter().zip([
            params.expected_revision_a.as_deref(),
            params.expected_revision_b.as_deref(),
        ]) {
            if expected.is_some_and(|expected| input["revision"].as_str() != Some(expected)) {
                return Ok(ToolResult::refused("INPUT_CHANGED", "Comparison inputs no longer match their expected revisions; start a new comparison").wire(Vec::new(), params.output.detail));
            }
        }
        let coverage = data.get("coverage").cloned().unwrap_or_else(|| json!({"before":{"complete":inputs[0]["complete"]},"after":{"complete":inputs[1]["complete"]}}));
        (data, inputs, coverage)
    } else {
        let mut before = match comparison_source(
            &params.base,
            true,
            params.expected_revision_a.as_deref(),
            &control,
        ) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return failed_comparison_source(
                    error,
                    "before",
                    params.output.detail,
                    &control.limits,
                )
            }
        };
        let mut after = match comparison_source(
            &params.base,
            false,
            params.expected_revision_b.as_deref(),
            &control,
        ) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return failed_comparison_source(
                    error,
                    "after",
                    params.output.detail,
                    &control.limits,
                )
            }
        };
        if before
            .sources
            .values()
            .chain(after.sources.values())
            .map(String::len)
            .sum::<usize>()
            > control.limits.max_source_bytes
            || before
                .guards
                .iter()
                .map(|guard| guard.file.as_str())
                .chain(before.sources.keys().map(String::as_str))
                .collect::<BTreeSet<_>>()
                .len()
                + after
                    .guards
                    .iter()
                    .map(|guard| guard.file.as_str())
                    .chain(after.sources.keys().map(String::as_str))
                    .collect::<BTreeSet<_>>()
                    .len()
                > control.limits.max_files
        {
            let mut result=ToolResult::refused(
                "SOURCE_LIMIT",
                "The two comparison inputs exceed the request source-byte or checked-file and lookup-candidate limit. No comparison facts were produced.",
            );
            result.limits.push(json!({"code":"SOURCE_LIMIT","ceilings":control.limits,"affected":"comparison input capture","omitted":"comparison model facts and navigation"}));
            return Ok(result.wire(Vec::new(), params.output.detail));
        }
        let inputs = vec![before.inputs.clone(), after.inputs.clone()];
        let coverage = json!({"before":before.coverage,"after":after.coverage});
        let data = if before.coverage["complete"] != true || after.coverage["complete"] != true {
            json!({"status":"incomplete","message":"Comparison inputs are incomplete; change arrays and source navigation were withheld.","inputs":coverage})
        } else {
            compare_supplied(&mut before, &mut after, &control)?
        };
        (data, inputs, coverage)
    };
    control.check().map_err(source_error)?;
    let mut result = ToolResult::new(Value::Null);
    control.complete_phase("Compare models");
    result.inputs = inputs;
    result.coverage = Some(json!({"inputs":coverage,"comparison":data.get("coverage")}));
    if data["state"] == "failure" || data["error"].is_string() || data["status"] == "incomplete" {
        let code = data["code"]
            .as_str()
            .unwrap_or("INCOMPLETE_INPUT")
            .to_owned();
        let message = data["message"]
            .as_str()
            .or_else(|| data["error"].as_str())
            .unwrap_or("Comparison inputs are incomplete; change arrays were withheld")
            .to_owned();
        result.cause("refused", &code, message);
        if code == "CAPTURE_LIMIT" {
            result.limits.push(json!({"code":code,"source_ceilings":control.limits,"affected":"repository input capture","omitted":"comparison model facts and navigation"}));
        }
        result.data = data.clone();
        return Ok(result.wire(
            vec![ContentBlock::text(data.to_string())],
            params.output.detail,
        ));
    }
    result.limits = data["semantic"]["limits"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    result.limits.extend(
        data["source_changes"]["limits"]
            .as_array()
            .into_iter()
            .flatten()
            .cloned(),
    );
    if !result.limits.is_empty() {
        result.cause("partial", "COMPARISON_LIMIT", "Some comparison detail is unavailable. Coverage and limits identify the retained facts and omitted detail.");
    }
    let revision =
        source::sha256(serde_json::to_vec(&result.inputs).expect("comparison identities"));
    if let Err(message) = project_comparison(&mut data, &params, &arguments, &revision, &mut result)
    {
        if message.starts_with("INPUT_CHANGED") {
            return Ok(ToolResult::refused("INPUT_CHANGED", message)
                .wire(Vec::new(), params.output.detail));
        }
        return Err(invalid(message));
    }
    let legacy = vec![ContentBlock::text(data.to_string())];
    result.data = data;
    control.check().map_err(source_error)?;
    Ok(result.wire(legacy, params.output.detail))
}

fn comparison_source(
    params: &super::CompareModelsParams,
    before: bool,
    expected: Option<&str>,
    control: &RequestControl,
) -> Result<SourceSnapshot, source::SourceError> {
    let (text, active, preferred, side) = if before {
        (
            params.file_content_a.as_deref().unwrap(),
            params.active_file_a.as_deref(),
            params.files_a.as_ref(),
            "before",
        )
    } else {
        (
            params.file_content_b.as_deref().unwrap(),
            params.active_file_b.as_deref(),
            params.files_b.as_ref(),
            "after",
        )
    };
    let files = super::first_nonempty_files(preferred, params.files.as_ref());
    let input = if let Some(files) = files {
        let mut files: BTreeMap<_, _> = files
            .iter()
            .map(|(file, text)| (file.clone(), text.clone()))
            .collect();
        let root = if let Some(active) = active {
            active.to_owned()
        } else {
            let mut serial = 0usize;
            loop {
                let root = format!("__dygnosis_compare_{side}_{serial}.mod");
                if !files.keys().any(|key| {
                    crate::engine::workspace::include_resolver::supplied_path_key(key) == root
                }) {
                    break root;
                }
                serial += 1;
            }
        };
        files.insert(root.clone(), text.into());
        SourceInput::Map {
            root,
            files,
            root_text: None,
            include_dirs: Vec::new(),
            owner_roots: Vec::new(),
        }
    } else {
        SourceInput::Text { text: text.into() }
    };
    let mut snapshot = source::resolve_source(input, expected, control)?;
    snapshot.inputs["side"] = json!(side);
    if active.is_none() {
        snapshot.root_file = None;
        snapshot.inputs["root"] = Value::Null;
        snapshot.inputs["synthetic_root"] = json!(true);
        snapshot.inputs["checked_owners"] = json!([]);
    }
    Ok(snapshot)
}

fn failed_comparison_source(
    error: source::SourceError,
    side: &str,
    detail: super::result::Detail,
    ceilings: &source::SourceLimits,
) -> Result<CallToolResult, rmcp::ErrorData> {
    if error.invalid_arguments || error.code == "CANCELLED" {
        return Err(source_error(error));
    }
    let mut result = ToolResult::refused(error.code, error.message);
    if matches!(error.code, "SOURCE_LIMIT" | "ROOT_LIMIT") {
        result.limits.push(json!({"code":error.code,"ceilings":ceilings,"side":side,"affected":"comparison input capture","omitted":"comparison model facts and navigation"}));
    }
    result.coverage = Some(json!({"side":side,"complete":false}));
    Ok(result.wire(Vec::new(), detail))
}

fn compare_supplied(
    before: &mut SourceSnapshot,
    after: &mut SourceSnapshot,
    control: &RequestControl,
) -> Result<Value, rmcp::ErrorData> {
    use crate::engine::comparison::compare_navigation::{self, ComparisonInput, Coordinates};
    use crate::engine::comparison::semantic_diff::{self, CaptureBoundary, Side};
    use crate::engine::comparison::{compare_models_with_sources, CompareSource};
    control.check().map_err(source_error)?;
    let mut diff = compare_models_with_sources(
        &before.model,
        &after.model,
        Some(CompareSource {
            text: &before.raw,
            origin_uri: before.root_file.as_deref(),
        }),
        Some(CompareSource {
            text: &after.raw,
            origin_uri: after.root_file.as_deref(),
        }),
    );
    control.check().map_err(source_error)?;
    let old = ComparisonInput::capture(
        &mut before.workspace,
        &before.root,
        before.root_file.as_deref(),
        Some(before.revision.clone()),
        &before.model,
        diff.shock_setup_changes
            .iter()
            .map(|change| change.before.as_ref()),
    )
    .with_written_facts(
        &mut before.workspace,
        &before.root,
        &before.model,
        &diff,
        Side::Before,
    )
    .with_file_names(before.sources.keys());
    let new = ComparisonInput::capture(
        &mut after.workspace,
        &after.root,
        after.root_file.as_deref(),
        Some(after.revision.clone()),
        &after.model,
        diff.shock_setup_changes
            .iter()
            .map(|change| change.after.as_ref()),
    )
    .with_written_facts(
        &mut after.workspace,
        &after.root,
        &after.model,
        &diff,
        Side::After,
    )
    .with_file_names(after.sources.keys());
    let pairs = old.same_file_pairs(&new);
    compare_navigation::populate_written_statements(&mut diff, &old, &new);
    let boundary = CaptureBoundary::SuppliedRootAndExecutedIncludes;
    let _ = semantic_diff::populate_captured_sources(
        &mut diff,
        old.source_input(boundary),
        new.source_input(boundary),
        &pairs,
    );
    semantic_diff::enforce_output_budget(&mut diff);
    control.check().map_err(source_error)?;
    if !before.workspace.input_snapshot_is_current(&before.root)
        || !after.workspace.input_snapshot_is_current(&after.root)
    {
        return Ok(
            json!({"error":"Comparison inputs changed while reading them; repeat the request","code":"INPUT_CHANGED"}),
        );
    }
    let mut data = diff.to_json();
    data["navigation"] = compare_navigation::navigation_json(&diff, &old, &new, Coordinates::Mcp);
    data["sources"] = json!({"before":old.sources(),"after":new.sources()});
    Ok(data)
}

fn project_comparison(
    data: &mut Value,
    params: &CompareParams,
    arguments: &Value,
    revision: &str,
    result: &mut ToolResult,
) -> Result<(), String> {
    let allowed = [
        "summary",
        "structural",
        "semantic",
        "references",
        "source",
        "source_text",
        "navigation",
        "markdown",
    ];
    if params
        .output
        .sections
        .iter()
        .any(|section| !allowed.contains(&section.as_str()))
    {
        return Err("Unsupported comparison section".into());
    }
    let want = |section: &str| {
        if params.output.sections.is_empty() {
            params.output.detail == super::result::Detail::Full
                || matches!(section, "summary" | "semantic" | "navigation")
        } else {
            params
                .output
                .sections
                .iter()
                .any(|selected| selected == section)
        }
    };
    let mut pages = Pages::new("dynare_compare_models", revision, arguments, &params.output)?;
    let semantic_complete = data["semantic"]["availability"] == "complete";
    let source_complete = data["source_changes"]["availability"] == "complete";
    let mut pointer_map = BTreeMap::new();
    let mut reference_ids = BTreeSet::new();
    let projected = params.output.page_size.is_some()
        || !params.families.is_empty()
        || !params.kinds.is_empty()
        || !params.output.sections.is_empty()
        || params.output.detail == super::result::Detail::Compact;
    let summary = json!({"semantic_rows":semantic_complete.then(||data["semantic"]["rows"].as_array().map(Vec::len)).flatten(),"available_semantic_rows":data["semantic"]["rows"].as_array().map(Vec::len),"source_files":source_complete.then(||data["source_changes"]["files"].as_array().map(Vec::len)).flatten(),"available_source_files":data["source_changes"]["files"].as_array().map(Vec::len),"semantic_availability":data["semantic"]["availability"],"source_availability":data["source_changes"]["availability"]});
    if let Some(rows) = data["semantic"]["rows"].as_array_mut() {
        rows.retain(|row| {
            params.families.is_empty()
                || params
                    .families
                    .iter()
                    .any(|family| row["family"] == *family)
        });
        rows.retain(|row| {
            params.kinds.is_empty() || params.kinds.iter().any(|kind| row["change"] == *kind)
        });
        if want("semantic") {
            pages.collection_with_coverage("semantic.rows", rows, semantic_complete);
            for (index, row) in rows.iter_mut().enumerate() {
                reference_ids.extend(
                    row["references"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_owned),
                );
                if projected && let Some(pointer) = row["pointer"].as_str().map(str::to_owned) {
                    let new = format!("/semantic/rows/{index}");
                    pointer_map.insert(pointer.clone(), new.clone());
                    row["snapshot_pointer"] = json!(pointer);
                    row["pointer"] = json!(new);
                }
                if params.output.detail == super::result::Detail::Compact {
                    strip_optional_detail(row);
                }
            }
        } else {
            rows.clear();
        }
    }
    if let Some(references) = data["semantic"]["references"].as_array_mut() {
        if want("references") && !want("semantic") {
            pages.collection_with_coverage("semantic.references", references, semantic_complete);
        } else if want("semantic") && projected {
            references.retain(|reference| {
                reference["pointer"]
                    .as_str()
                    .is_some_and(|pointer| reference_ids.contains(pointer))
            });
        } else if !want("references") && !want("semantic") {
            references.clear();
        }
        if projected {
            for (index, reference) in references.iter_mut().enumerate() {
                if let Some(pointer) = reference["pointer"].as_str().map(str::to_owned) {
                    let new = format!("/semantic/references/{index}");
                    pointer_map.insert(pointer.clone(), new.clone());
                    reference["snapshot_pointer"] = json!(pointer);
                    reference["pointer"] = json!(new);
                }
                if let Some(pointer) = reference["equation_pointer"].as_str().map(str::to_owned) {
                    reference["snapshot_equation_pointer"] = json!(pointer);
                    reference["equation_pointer"] = pointer_map
                        .get(&pointer)
                        .map(|value| json!(value))
                        .unwrap_or(Value::Null);
                }
            }
        }
    }
    if projected {
        if let Some(rows) = data["semantic"]["rows"].as_array_mut() {
            for row in rows {
                if let Some(references) = row["references"].as_array_mut() {
                    *references = references
                        .iter()
                        .filter_map(Value::as_str)
                        .filter_map(|pointer| pointer_map.get(pointer).map(|value| json!(value)))
                        .collect();
                }
            }
        }
        if let Some(rows) = data["navigation"]["rows"].as_array_mut() {
            rows.retain_mut(|row| {
                let Some(id) = row["id"].as_str().map(str::to_owned) else {
                    return false;
                };
                if let Some(pointer) = pointer_map.get(&id) {
                    row["id"] = json!(pointer);
                    row["snapshot_pointer"] = json!(id);
                    true
                } else {
                    want("structural") && !id.starts_with("/semantic/")
                }
            });
        }
    }
    if want("source") {
        if let Some(files) = data["source_changes"]["files"].as_array_mut() {
            pages.collection_with_coverage("source_changes.files", files, source_complete);
        }
    } else if let Some(object) = data.as_object_mut() {
        object.remove("source_changes");
    }
    if !want("source_text") {
        data.as_object_mut().unwrap().remove("sources");
    }
    if !want("navigation") {
        data.as_object_mut().unwrap().remove("navigation");
    }
    if !want("markdown") {
        data.as_object_mut().unwrap().remove("markdown");
    }
    if !want("semantic") && !want("references") {
        data.as_object_mut().unwrap().remove("semantic");
    }
    if !want("structural") {
        let keep = [
            "semantic",
            "source_changes",
            "sources",
            "navigation",
            "coverage",
            "comparison_versions",
        ];
        data.as_object_mut()
            .unwrap()
            .retain(|key, _| keep.contains(&key.as_str()));
    }
    if want("summary") {
        data["summary"] = summary;
    }
    result.selection = pages.finish(&params.output);
    Ok(())
}

fn strip_optional_detail(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("runs");
            object.remove("statement_text");
            for child in object.values_mut() {
                strip_optional_detail(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                strip_optional_detail(child);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(tool: &str, arguments: Value, control: RequestControl) -> Value {
        dispatch(tool, arguments, control)
            .unwrap()
            .structured_content
            .unwrap()
    }

    // Failure of one exact root does not discard complete roots or their causes.
    #[test]
    fn batch_preserves_exact_keys_filters_and_per_root_failures() {
        let arguments = json!({"files":{"Models\\Root.mod":"var y;model;y=1;end;", "Models\\Other.mod":"var y;model;y=unknown;end;"},"roots":["Models\\Root.mod","Models\\Other.mod","missing.mod"],"code":"E020","detail":"compact"});
        let result = run(
            "dynare_workspace_diagnose",
            arguments,
            RequestControl::default(),
        );
        assert_eq!(result["outcome"]["status"], "partial");
        let roots = result["data"]["roots"].as_array().unwrap();
        assert!(roots
            .iter()
            .any(|root| root["root"] == "Models\\Root.mod" && root["status"] == "ok"));
        assert!(roots.iter().any(|root| root["root"] == "missing.mod"
            && root["status"] == "failed"
            && root["code"] == "ROOT_NOT_FOUND"));
        assert_eq!(result["data"]["summary"]["checked"], 2);
        assert_eq!(result["data"]["summary"]["failed"], 1);
        assert!(
            roots
                .iter()
                .any(|root| !root["diagnostics"].as_array().unwrap().is_empty()),
            "the code filter must retain a real matching diagnostic"
        );
        assert_eq!(result["inputs"][0]["revision"].as_str().unwrap().len(), 64);
        for root in roots {
            assert!(root["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["code"] == "E020"));
        }
    }

    // A work limit retains complete root results and names every unstarted root.
    #[test]
    fn root_limit_retains_completed_and_unfinished_accounting() {
        let mut control = RequestControl::default();
        control.limits.max_roots = 1;
        let result = run(
            "dynare_workspace_diagnose",
            json!({"files":{"a.mod":"var y;model;y=1;end;","b.mod":"var y;model;y=2;end;"},"roots":["a.mod","b.mod"],"detail":"compact"}),
            control,
        );
        assert_eq!(result["outcome"]["status"], "partial");
        assert_eq!(result["data"]["summary"]["checked"], 1);
        assert_eq!(result["data"]["summary"]["unfinished"], 1);
        assert_eq!(result["data"]["roots"][1]["root"], "b.mod");
        assert_eq!(result["data"]["roots"][1]["status"], "unfinished");
        assert!(result["limits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|limit| limit["code"] == "WORK_LIMIT"));
    }

    // Per-root input guards reject a changed dependency, including unedited text.
    #[test]
    fn batch_reused_revision_rejects_changed_include() {
        let arguments = json!({"files":{"root.mod":"var y;model;\n@#include \"eq.inc\"\nend;","eq.inc":"y=1;"},"roots":["root.mod"],"detail":"compact"});
        let first = run(
            "dynare_workspace_diagnose",
            arguments.clone(),
            RequestControl::default(),
        );
        let mut next = arguments;
        next["files"]["eq.inc"] = json!("y=2;");
        next["expected_revisions"] = first["data"]["root_revisions"].clone();
        let second = run("dynare_workspace_diagnose", next, RequestControl::default());
        assert_eq!(second["outcome"]["status"], "refused");
        assert_eq!(second["data"]["roots"][0]["code"], "INPUT_CHANGED");
        assert!(second["data"]["roots"][0]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    // Compact comparison keeps selected row targets while dropping supporting
    // full source and highlighted-expression detail from its response.
    #[test]
    fn compact_comparison_preserves_counts_and_rebases_selected_navigation() {
        let arguments = json!({"file_content_a":"var y;parameters p;p=1;model;[name='eq'] y=p;end;", "file_content_b":"var y;parameters p;p=2;model;[name='eq'] y=p+1;end;"});
        let full = dispatch(
            "dynare_compare_models",
            arguments.clone(),
            RequestControl::default(),
        )
        .unwrap();
        let full_value = full.structured_content.as_ref().unwrap();
        let mut compact_arguments = arguments.clone();
        compact_arguments["detail"] = json!("compact");
        compact_arguments["families"] = json!(["parameters"]);
        compact_arguments["sections"] = json!(["summary", "semantic", "navigation"]);
        let compact = dispatch(
            "dynare_compare_models",
            compact_arguments,
            RequestControl::default(),
        )
        .unwrap();
        assert!(
            serde_json::to_vec(&compact).unwrap().len() < serde_json::to_vec(&full).unwrap().len()
        );
        let value = compact.structured_content.unwrap();
        assert_eq!(value["outcome"]["status"], full_value["outcome"]["status"]);
        assert_eq!(
            value["data"]["summary"]["available_semantic_rows"]
                .as_u64()
                .unwrap() as usize,
            full_value["data"]["semantic"]["rows"]
                .as_array()
                .unwrap()
                .len()
        );
        assert!(value["data"].get("sources").is_none());
        for row in value["data"]["semantic"]["rows"].as_array().unwrap() {
            assert_eq!(row["family"], "parameters");
            let pointer = row["pointer"].as_str().unwrap();
            assert!(
                value["data"].pointer(pointer).is_some(),
                "selected row pointer must resolve"
            );
            assert!(value["data"]["navigation"]["rows"]
                .as_array()
                .unwrap()
                .iter()
                .any(|target| target["id"] == pointer));
        }
    }

    // Before and After revisions are independent and a mismatch returns no rows.
    #[test]
    fn comparison_checks_both_expected_source_revisions() {
        let arguments = json!({"file_content_a":"var y;model;y=1;end;", "file_content_b":"var y;model;y=2;end;","detail":"compact"});
        let first = run(
            "dynare_compare_models",
            arguments.clone(),
            RequestControl::default(),
        );
        let mut next = arguments;
        next["expected_revision_a"] = first["inputs"][0]["revision"].clone();
        next["expected_revision_b"] = first["inputs"][1]["revision"].clone();
        next["file_content_b"] = json!("var y;model;y=3;end;");
        let second = run("dynare_compare_models", next, RequestControl::default());
        assert_eq!(second["outcome"]["status"], "refused");
        assert_eq!(second["outcome"]["reasons"][0]["code"], "INPUT_CHANGED");
        assert!(second["data"].is_null());
    }

    // An already-cancelled request starts no roots and records no late progress.
    #[test]
    fn cancellation_stops_before_batch_or_comparison_work() {
        let control = RequestControl::default();
        control
            .cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let error = dispatch(
            "dynare_workspace_diagnose",
            json!({"files":{"a.mod":"var y;"},"roots":["a.mod"]}),
            control.clone(),
        )
        .unwrap_err();
        assert!(error.message.contains("CANCELLED"));
        assert_eq!(
            control
                .progress
                .completed
                .load(std::sync::atomic::Ordering::Relaxed),
            0
        );
    }

    // Every root's diagnostics can continue without losing rows from roots
    // whose first page was returned beside another root's first page.
    #[test]
    fn batch_pages_preserve_each_root_collection() {
        let arguments = json!({"files":{"a.mod":"var y;model;y=one+two;end;","b.mod":"var y;model;y=three+four;end;"},"roots":["a.mod","b.mod"],"code":"E020","detail":"compact","page_size":1});
        let first = run(
            "dynare_workspace_diagnose",
            arguments.clone(),
            RequestControl::default(),
        );
        let mut next = arguments;
        let token = first["selection"]["continuation"].as_str().unwrap();
        next["continuation"] = json!(token);
        next["expected_revisions"] = first["data"]["root_revisions"].clone();
        let second = run("dynare_workspace_diagnose", next, RequestControl::default());
        for index in 0..2 {
            let initial = &first["data"]["roots"][index]["diagnostics"][0];
            let continued = &second["data"]["roots"][index]["diagnostics"][0];
            assert_ne!(initial["column"], continued["column"]);
            assert_eq!(continued["code"], "E020");
        }
        assert!(second["selection"]["continuation"].is_null());
    }

    // Comparison pagination remains bound to both source sides, after adding
    // expected revisions that were absent on the first request.
    #[test]
    fn comparison_pages_require_unchanged_sides_and_keep_valid_targets() {
        let arguments = json!({"file_content_a":"var y;parameters p;p=1;model;[name='eq'] y=p;end;", "file_content_b":"var y;parameters p;p=2;model;[name='eq'] y=p+1;end;","detail":"compact","sections":["semantic","navigation"],"page_size":1});
        let first = run(
            "dynare_compare_models",
            arguments.clone(),
            RequestControl::default(),
        );
        let mut next = arguments;
        next["continuation"] = first["selection"]["continuation"].clone();
        next["expected_revision_a"] = first["inputs"][0]["revision"].clone();
        next["expected_revision_b"] = first["inputs"][1]["revision"].clone();
        let second = run("dynare_compare_models", next, RequestControl::default());
        assert_ne!(
            first["data"]["semantic"]["rows"][0]["snapshot_pointer"],
            second["data"]["semantic"]["rows"][0]["snapshot_pointer"]
        );
        let pointer = second["data"]["semantic"]["rows"][0]["pointer"]
            .as_str()
            .unwrap();
        assert!(second["data"].pointer(pointer).is_some());
        assert!(second["data"]["navigation"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == pointer));
    }

    // A continued query cannot change its diagnostic filter and return stale
    // rows, even when both source snapshots still match their guards.
    #[test]
    fn changed_batch_filter_returns_refusal_without_stale_data() {
        let arguments = json!({"files":{"a.mod":"var y;model;y=one+two;end;"},"roots":["a.mod"],"code":"E020","detail":"compact","page_size":1});
        let first = run(
            "dynare_workspace_diagnose",
            arguments.clone(),
            RequestControl::default(),
        );
        let mut next = arguments;
        next["continuation"] = first["selection"]["continuation"].clone();
        next["expected_revisions"] = first["data"]["root_revisions"].clone();
        next["code"] = json!("E025");
        let refused = run("dynare_workspace_diagnose", next, RequestControl::default());
        assert_eq!(refused["outcome"]["status"], "refused");
        assert_eq!(refused["outcome"]["reasons"][0]["code"], "INPUT_CHANGED");
        assert!(refused["data"].is_null());
    }

    // Default text clients receive the selected page, not an unbounded copy
    // of every diagnostic before its filters and pagination were applied.
    #[test]
    fn legacy_batch_text_retains_only_the_selected_page() {
        let arguments = json!({"files":{"a.mod":"var y;model;y=one+two;end;"},"roots":["a.mod"],"code":"E020","page_size":1});
        let result = dispatch(
            "dynare_workspace_diagnose",
            arguments,
            RequestControl::default(),
        )
        .unwrap();
        let ContentBlock::Text(legacy) = &result.content[0] else {
            panic!("leading legacy JSON text");
        };
        let legacy: Value = serde_json::from_str(&legacy.text).unwrap();
        let canonical = result.structured_content.unwrap();
        assert_eq!(legacy["roots"], canonical["data"]["roots"]);
        assert_eq!(
            legacy["roots"][0]["diagnostics"].as_array().unwrap().len(),
            1
        );
    }

    // An unnamed supplied comparison root still resolves sibling supplied
    // includes and retains its synthetic identity without host-disk access.
    #[test]
    fn synthetic_comparison_roots_resolve_relative_supplied_includes() {
        let source = "var y;model;\n@#include \"eq.inc\"\nend;";
        let result = run(
            "dynare_compare_models",
            json!({"file_content_a":source,"file_content_b":source,"files_a":{"eq.inc":"y=1;"},"files_b":{"eq.inc":"y=2;"},"detail":"compact"}),
            RequestControl::default(),
        );
        assert_ne!(result["outcome"]["status"], "refused");
        assert!(result["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|input| input["synthetic_root"] == true
                && input["root"].is_null()
                && input["source_policy"] == "map"));
        assert!(!result["data"]["semantic"]["rows"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    // Two individually valid snapshots must still obey one request file cap.
    #[test]
    fn comparison_enforces_combined_checked_file_cap() {
        let mut control = RequestControl::default();
        control.limits.max_files = 1;
        let result = run(
            "dynare_compare_models",
            json!({"file_content_a":"var y;model;y=1;end;","file_content_b":"var y;model;y=2;end;","detail":"compact"}),
            control,
        );
        assert_eq!(result["outcome"]["status"], "refused");
        assert_eq!(result["outcome"]["reasons"][0]["code"], "SOURCE_LIMIT");
        assert_eq!(result["limits"][0]["code"], "SOURCE_LIMIT");
        assert_eq!(result["limits"][0]["ceilings"]["max_files"], 1);
        assert!(result["data"].is_null());
    }

    // Missing search candidates from several roots share one request budget.
    // A later root cannot make earlier complete roots disappear at that limit.
    #[test]
    fn batch_enforces_combined_lookup_candidate_cap() {
        let files = BTreeMap::from([
            (
                "a/root.mod".into(),
                "var y;model;\n@#include \"eq.inc\"\nend;".into(),
            ),
            (
                "b/root.mod".into(),
                "var y;model;\n@#include \"eq.inc\"\nend;".into(),
            ),
            ("shared/eq.inc".into(), "y=1;".into()),
        ]);
        let mut key_sets = Vec::new();
        for root in ["a/root.mod", "b/root.mod"] {
            let snapshot = source::resolve_source(
                SourceInput::Map {
                    root: root.into(),
                    files: files.clone(),
                    root_text: None,
                    include_dirs: vec!["shared".into()],
                    owner_roots: Vec::new(),
                },
                None,
                &RequestControl::default(),
            )
            .unwrap();
            key_sets.push(
                snapshot
                    .guards
                    .iter()
                    .map(|guard| guard.file.clone())
                    .chain(snapshot.sources.keys().cloned())
                    .collect::<BTreeSet<_>>(),
            );
        }
        let limit = key_sets
            .iter()
            .map(BTreeSet::len)
            .max()
            .unwrap()
            .max(files.len());
        assert!(
            key_sets[0].union(&key_sets[1]).count() > limit,
            "the fixture must exceed only the request-wide key limit"
        );
        let mut control = RequestControl::default();
        control.limits.max_files = limit;
        let result = run(
            "dynare_workspace_diagnose",
            json!({"files":files,"roots":["a/root.mod","b/root.mod"],"include_dirs":["shared"],"detail":"compact"}),
            control,
        );
        assert_eq!(result["data"]["summary"]["checked"], 1);
        assert_eq!(result["data"]["summary"]["unfinished"], 1);
        assert_eq!(result["data"]["roots"][1]["code"], "SOURCE_LIMIT");
    }

    // Incomplete Parse can retain diagnostics, but it cannot claim a complete
    // root or an exact total for the diagnostics that were not checked.
    #[test]
    fn incomplete_batch_roots_report_partial_work_and_unknown_totals() {
        let result = run(
            "dynare_workspace_diagnose",
            json!({"files":{"a.mod":"var y;\n@#if 1\nmodel;y=1;end;"},"roots":["a.mod"],"detail":"compact"}),
            RequestControl::default(),
        );
        assert_eq!(result["outcome"]["status"], "partial");
        assert_eq!(result["data"]["roots"][0]["status"], "partial");
        assert_eq!(result["data"]["summary"]["partial"], 1);
        assert!(!result["data"]["roots"][0]["reasons"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(result["selection"]["collections"]["diagnostics:a.mod"]["total_matches"].is_null());
    }

    // A small source can spend the real expansion budget through nested loops.
    // Batch and comparison outcomes must retain that typed stop receipt.
    #[test]
    fn actual_macro_work_stop_survives_batch_and_comparison_results() {
        let source =
            "var y;\nmodel;\n@#for i in 1:1000\n@#for j in 1:1000\ny=0;\n@#endfor\n@#endfor\nend;\n";
        let batch = run(
            "dynare_workspace_diagnose",
            json!({"files":{"root.mod":source},"roots":["root.mod"],"detail":"compact"}),
            RequestControl::default(),
        );
        let comparison = run(
            "dynare_compare_models",
            json!({"file_content_a":source,"file_content_b":"var y;model;y=2;end;","detail":"compact"}),
            RequestControl::default(),
        );
        assert_eq!(batch["data"]["roots"][0]["status"], "partial", "{batch}");
        assert_eq!(batch["data"]["roots"][0]["code"], "INCOMPLETE_INPUT");
        assert!(batch["data"]["roots"][0]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["code"] == "I211"));
        assert!(!batch["data"]["roots"][0]["failure"]
            .as_str()
            .unwrap()
            .contains("include is unresolved"));
        for result in [&batch, &comparison] {
            assert_ne!(result["outcome"]["status"], "ok", "{result}");
            let receipt = result["limits"]
                .as_array()
                .unwrap()
                .iter()
                .find(|limit| limit["code"] == "MACRO_LIMIT" && limit["limit"] == "iteration work")
                .unwrap_or_else(|| {
                    panic!("an applied macro work stop must be a canonical limit receipt: {result}")
                });
            assert_eq!(receipt["ceiling"], 1_000_000);
            assert_eq!(receipt["unit"], "work units");
            assert!(receipt["omitted"].is_string());
        }
        assert_eq!(comparison["outcome"]["status"], "refused");
        assert!(comparison["data"].get("semantic").is_none());
    }
}
