//! Guarded editor-session analysis choices. Source discovery stays available
//! when the selected choices are stale; it never substitutes Source semantics.

use super::*;
use crate::engine::analysis_configuration::{
    AnalysisConfiguration, AnalysisOverride, AnalysisSelectionState, DirectiveKind,
};
use crate::engine::language::macro_expr::MacroBudget;
use crate::engine::workspace::include_resolver::{normalize_uri, uri_to_path};
use std::path::PathBuf;

#[derive(Default)]
pub(super) struct AnalysisState {
    pub enabled: bool,
    roots: HashMap<String, RootSelection>,
}

struct RootSelection {
    root: Url,
    generation: u64,
    paths: Option<Vec<PathBuf>>,
}

impl AnalysisState {
    pub(super) fn paths_for(&self, root: &Url) -> Option<&[PathBuf]> {
        self.roots
            .get(&normalize_uri(root.as_str()))
            .and_then(|state| state.paths.as_deref())
    }
    pub(super) fn generation(&self, root: &Url) -> u64 {
        self.roots
            .get(&normalize_uri(root.as_str()))
            .map_or(0, |state| state.generation)
    }
    pub(super) fn reset_removed(&mut self, folders: &[Url]) -> Vec<Url> {
        let mut removed = Vec::new();
        self.roots.retain(|_, state| {
            let remove = folders.iter().any(|folder| {
                state.root.scheme() == folder.scheme()
                    && state.root.host_str() == folder.host_str()
                    && state
                        .root
                        .path()
                        .starts_with(&format!("{}/", folder.path().trim_end_matches('/')))
            });
            if remove {
                removed.push(state.root.clone());
            }
            !remove
        });
        removed
    }
    fn accept(&mut self, root: &Url, paths: Option<Vec<PathBuf>>) {
        let generation = self
            .generation(root)
            .checked_add(1)
            .expect("analysis selection generation");
        self.roots.insert(
            normalize_uri(root.as_str()),
            RootSelection {
                root: root.clone(),
                generation,
                paths,
            },
        );
    }
}

fn refusal(code: &str, message: impl Into<String>) -> Value {
    json!({"accepted":false,"code":code,"error":message.into()})
}

pub(super) fn context_revision(inner: &mut Inner, root: &Url) -> Option<String> {
    if root.scheme() == "untitled" {
        inner.workspace.set_unnamed_root(root.as_str());
    }
    inner.remember_request_root(root);
    inner.prepare_root(root);
    let source = inner
        .workspace
        .source_context_revision(root.as_str())
        .ok()?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hash);
    Some(format!("{:016x}", hash.finish()))
}

pub(super) fn metadata(inner: &mut Inner, root: &Url) -> Value {
    let state = inner.workspace.analysis_selection(root.as_str());
    let configuration = inner.workspace.root_analysis_configuration(root.as_str());
    let paths = inner.analysis_configuration.paths_for(root).map(|paths| {
        paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    });
    let (selection, mut is_stale, mut cause) = match state {
        AnalysisSelectionState::Selected(selection) => (selection, false, None),
        AnalysisSelectionState::Stale { selection, cause } => (selection, true, Some(cause)),
    };
    let facts = if cause.is_none() {
        match inner.workspace.analysis_metadata(root.as_str()) {
            Ok(facts) => Some(facts),
            Err(error) => {
                is_stale = true;
                cause = Some(error);
                None
            }
        }
    } else {
        None
    };
    if cause.is_none()
        && configuration.equation_selection.is_some()
        && facts.as_ref().is_some_and(|facts| !facts.model_complete)
    {
        let failure = inner
            .workspace
            .get_effective_model(root.as_str())
            .and_then(|model| model.invocation_analysis.as_ref())
            .and_then(|analysis| analysis.failure.clone());
        if let Some(message) = failure {
            cause = Some(
                crate::engine::analysis_configuration::ConfigurationError::new(
                    "ANALYSIS_EQUATION_SELECTION_UNAVAILABLE",
                    message,
                ),
            );
        }
    }
    let cause =
        cause.map(|cause| json!({"code":cause.code,"message":cause.message,"target":cause.target}));
    json!({"state":if is_stale {"stale"} else {"selected"},"selection":selection,
        "configuration":configuration.as_ref(),"selection_generation":inner.analysis_configuration.generation(root),
        "include_paths":paths,"macro_complete":facts.as_ref().map(|facts|facts.macro_complete),
        "model_complete":facts.as_ref().map(|facts|facts.model_complete),
        "overrides":facts.as_ref().map(|facts|facts.overrides.clone()).unwrap_or_default(),"cause":cause})
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PresetBinding {
    operation_index: usize,
    file: Option<String>,
    kind: String,
    name: Option<String>,
    expected_text: String,
    target_text: String,
    branch_index: Option<usize>,
    branch_text: Option<String>,
}

fn root_argument(inner: &Inner, arguments: &[Value]) -> std::result::Result<(Url, Url), Value> {
    if !inner.analysis_configuration.enabled {
        return Err(refusal(
            "UNSUPPORTED_CAPABILITY",
            "This client did not enable analysis configurations. Update the client and restart the server.",
        ));
    }
    let argument = arguments
        .first()
        .and_then(Value::as_object)
        .ok_or_else(|| {
            refusal(
                "INVALID_ARGUMENTS",
                "Analysis configuration requires root_uri.",
            )
        })?;
    let root = argument
        .get("root_uri")
        .and_then(Value::as_str)
        .and_then(|value| Url::parse(value).ok())
        .filter(|root| is_model_root(root) && matches!(root.scheme(), "file" | "untitled"))
        .ok_or_else(|| {
            refusal(
                "ROOT_REQUIRED",
                "Choose a file or untitled .mod or .dyn model root.",
            )
        })?;
    let document = argument
        .get("document_uri")
        .and_then(Value::as_str)
        .map(Url::parse)
        .transpose()
        .map_err(|_| refusal("INVALID_ARGUMENTS", "document_uri is invalid."))?
        .unwrap_or_else(|| root.clone());
    if let Some(version) = argument.get("document_version") {
        let current = inner
            .document(&document)
            .map(|document| i64::from(document.version));
        if version.as_i64() != current && !(version.is_null() && current.is_none()) {
            return Err(refusal(
                "STALE_DOCUMENT",
                "The displayed source changed. Request the analysis action again.",
            ));
        }
    }
    Ok((root, document))
}

fn site_rows(
    inner: &Inner,
    root: &Url,
    document: &Url,
    budget: &mut MacroBudget,
) -> std::result::Result<(Vec<Value>, Value), Value> {
    let key = normalize_uri(document.as_str());
    let Some(source) = inner.workspace.source_document(root.as_str(), &key) else {
        return Ok((
            Vec::new(),
            json!({"complete":false,"cause":"The displayed source has not been acquired."}),
        ));
    };
    let Some(macros) = source.macro_source() else {
        return Ok((
            Vec::new(),
            json!({"complete":false,"cause":"Guarded source directives are unavailable because source discovery stopped."}),
        ));
    };
    budget
        .spend_work(source.source().len().max(1))
        .map_err(|_| {
            refusal(
                "ANALYSIS_CONTEXT_LIMIT",
                "Source directive presentation reached the work allowance.",
            )
        })?;
    let index = LineIndex::new(source.source());
    let file = if root.scheme() == "untitled" && normalize_uri(root.as_str()) == key {
        None
    } else {
        Some(key)
    };
    let mut rows = Vec::new();
    for site in macros.sites.iter() {
        budget
            .spend_work(
                site.expected_text
                    .len()
                    .saturating_add(
                        site.branches
                            .iter()
                            .map(|branch| {
                                branch
                                    .expected_text
                                    .len()
                                    .saturating_add(branch.branch_id.len())
                            })
                            .sum::<usize>(),
                    )
                    .saturating_add(256),
            )
            .map_err(|_| {
                refusal(
                    "ANALYSIS_CONTEXT_LIMIT",
                    "Source directive presentation reached the work allowance.",
                )
            })?;
        let mut target = site.target.clone();
        target.file = file.clone();
        rows.push(json!({"target":target,"kind":site.kind,"name":site.name,"expected_text":site.expected_text,
            "location":{"uri":document,"range":span_range(&index,source.source(),site.directive_span)},
            "branches":site.branches.iter().map(|branch|json!({"branch_id":branch.branch_id,"expected_text":branch.expected_text,
                "location":{"uri":document,"range":span_range(&index,source.source(),branch.directive_span)}})).collect::<Vec<_>>()}));
    }
    Ok((
        rows,
        json!({"complete":macros.syntax_refusal.is_none(),"cause":macros.syntax_refusal.as_ref().map(|refusal|refusal.message.as_str())}),
    ))
}

fn saved_bindings(
    inner: &mut Inner,
    root: &Url,
    choices: &AnalysisConfiguration,
    budget: &mut MacroBudget,
) -> std::result::Result<Vec<PresetBinding>, Value> {
    let mut bindings = Vec::new();
    for (operation_index, operation) in choices.overrides.iter().enumerate() {
        let target = operation.target();
        let file = target.file.as_deref().unwrap_or(root.as_str());
        let source = inner
            .workspace
            .inspect_macro_source(root.as_str(), file, budget)
            .map_err(|cause| refusal(cause.code, cause.message))?
            .ok_or_else(|| {
                refusal(
                    "ANALYSIS_TARGET_MISSING",
                    "The selected override source is unavailable; no preset was prepared.",
                )
            })?;
        let site = source
            .sites
            .iter()
            .find(|site| {
                site.target.site_id == target.site_id
                    && source.source_revision == target.source_revision
            })
            .ok_or_else(|| {
                refusal(
                    "ANALYSIS_TARGET_STALE",
                    "The selected override changed; no preset was prepared.",
                )
            })?;
        let text = &source.original[site.span.start as usize..site.span.end as usize];
        budget.spend_work(text.len().saturating_add(site.expected_text.len()).saturating_add(128)).map_err(|_|refusal("ANALYSIS_PRESET_LIMIT","Saving this configuration reached the source-copy allowance; no preset was prepared."))?;
        let branch_index = match operation {
            AnalysisOverride::Branch {
                branch_id: Some(branch),
                ..
            } => Some(
                site.branches
                    .iter()
                    .position(|candidate| candidate.branch_id == *branch)
                    .ok_or_else(|| {
                        refusal(
                            "ANALYSIS_BRANCH_MISSING",
                            "The selected branch changed; no preset was prepared.",
                        )
                    })?,
            ),
            _ => None,
        };
        bindings.push(PresetBinding {
            operation_index,
            file: target.file.clone(),
            kind: match site.kind {
                DirectiveKind::Conditional => "conditional",
                DirectiveKind::Definition => "definition",
                _ => "function_definition",
            }
            .into(),
            name: site.name.clone(),
            expected_text: site.expected_text.clone(),
            target_text: text.into(),
            branch_index,
            branch_text: branch_index.map(|index| site.branches[index].expected_text.clone()),
        });
    }
    Ok(bindings)
}

fn context(inner: &mut Inner, arguments: &[Value]) -> std::result::Result<Value, Value> {
    let (root, document) = root_argument(inner, arguments)?;
    let revision = context_revision(inner,&root).ok_or_else(||refusal("ANALYSIS_CONTEXT_UNAVAILABLE","The source context could not be guarded. Open the root and resolve source acquisition limits, then try again."))?;
    let mut budget = MacroBudget::with_limit(inner.settings.resolve(&root).macro_work_limit, None);
    let argument = &arguments[0];
    if argument
        .get("expected_revision")
        .and_then(Value::as_str)
        .is_some_and(|expected| expected != revision)
        || argument
            .get("expected_selection_generation")
            .and_then(Value::as_u64)
            .is_some_and(|expected| expected != inner.analysis_configuration.generation(&root))
    {
        return Err(refusal(
            "ANALYSIS_CONTEXT_STALE",
            "The source or live analysis changed before comparison preparation. Request the comparison again.",
        ));
    }
    let prepared = if let Some(value) = argument.get("prepare_analysis") {
        let mut choices: AnalysisConfiguration =
            serde_json::from_value(value.clone()).map_err(|error| {
                refusal(
                    "INVALID_ARGUMENTS",
                    format!("Invalid comparison analysis choices: {error}"),
                )
            })?;
        if let Some(bindings) = argument.get("preset_bindings") {
            rebind(
                inner,
                &root,
                &mut choices,
                serde_json::from_value(bindings.clone()).map_err(|_| {
                    refusal(
                        "ANALYSIS_PRESET_STALE",
                        "Saved comparison override evidence is invalid.",
                    )
                })?,
                &mut budget,
            )?;
        }
        let paths = requested_paths(argument)?
            .unwrap_or_else(|| inner.settings.resolve(&root).search_paths);
        Some((choices, paths))
    } else {
        None
    };
    let (sites, coverage) = site_rows(inner,&root,&document,&mut budget).unwrap_or_else(|cause| (Vec::new(),json!({"complete":false,"cause":cause.get("error").and_then(Value::as_str).unwrap_or("Source directive presentation is unavailable.")})));
    let analysis = metadata(inner, &root);
    let mut result = json!({"schema_version":1,"root_uri":root,"document_uri":document,
        "document_version":inner.document(&document).map(|document|document.version),"revision":revision,
        "analysis":analysis,"sites":sites,"source_coverage":coverage});
    if arguments
        .first()
        .and_then(|value| value.get("prepare_preset"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        if root.scheme() != "file" {
            return Err(refusal(
                "ANALYSIS_PRESET_UNTITLED",
                "Untitled models support transient choices. Save the model before saving a path-based configuration.",
            ));
        }
        let choices = inner.workspace.root_analysis_configuration(root.as_str());
        result["preset_bindings"] = json!(saved_bindings(inner, &root, &choices, &mut budget)?);
    }
    if let Some((choices, paths)) = prepared {
        result["prepared_analysis"] = json!(choices);
        result["prepared_include_paths"] = json!(
            paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        );
    }
    Ok(result)
}

fn rebind(
    inner: &mut Inner,
    root: &Url,
    choices: &mut AnalysisConfiguration,
    bindings: Vec<PresetBinding>,
    budget: &mut MacroBudget,
) -> std::result::Result<(), Value> {
    if bindings.len() != choices.overrides.len() {
        return Err(refusal(
            "ANALYSIS_PRESET_STALE",
            "The saved configuration lacks exact evidence for each override. Save it again from current source.",
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for binding in bindings {
        if !seen.insert(binding.operation_index) {
            return Err(refusal(
                "ANALYSIS_PRESET_STALE",
                "The saved override evidence is ambiguous.",
            ));
        }
        let operation = choices
            .overrides
            .get_mut(binding.operation_index)
            .ok_or_else(|| {
                refusal(
                    "ANALYSIS_PRESET_STALE",
                    "The saved override index is invalid.",
                )
            })?;
        if binding.file.as_deref().map(normalize_uri)
            != operation.target().file.as_deref().map(normalize_uri)
        {
            return Err(refusal(
                "ANALYSIS_PRESET_STALE",
                "Saved override evidence identifies a different source file.",
            ));
        }
        let file = binding.file.as_deref().unwrap_or(root.as_str());
        let source = inner
            .workspace
            .inspect_macro_source(root.as_str(), file, budget)
            .map_err(|cause| refusal(cause.code, cause.message))?
            .ok_or_else(|| {
                refusal(
                    "ANALYSIS_PRESET_STALE",
                    "A saved override source is unavailable.",
                )
            })?;
        let mut matching = None;
        for site in source.sites.iter() {
            budget.spend_work(1).map_err(|_| {
                refusal(
                    "ANALYSIS_PRESET_LIMIT",
                    "Saved configuration matching reached the work allowance.",
                )
            })?;
            let kind = match site.kind {
                DirectiveKind::Definition => "definition",
                DirectiveKind::Conditional => "conditional",
                _ => "function_definition",
            };
            if kind != binding.kind
                || site.name != binding.name
                || site.expected_text != binding.expected_text
            {
                continue;
            }
            let text = &source.original[site.span.start as usize..site.span.end as usize];
            budget.spend_work(text.len().max(1)).map_err(|_| {
                refusal(
                    "ANALYSIS_PRESET_LIMIT",
                    "Saved configuration matching reached the work allowance.",
                )
            })?;
            if text != binding.target_text {
                continue;
            }
            if matching.is_some() {
                return Err(refusal(
                    "ANALYSIS_PRESET_AMBIGUOUS",
                    "More than one unchanged written directive matches this saved override. Select the directive again.",
                ));
            }
            matching = Some(site);
        }
        let site = matching.ok_or_else(||refusal("ANALYSIS_PRESET_STALE","A saved directive or conditional chain changed. Select the directive again; this preset was not applied."))?;
        let mut target = site.target.clone();
        target.file = binding.file.clone();
        match operation {
            AnalysisOverride::Definition { target: old, .. }
            | AnalysisOverride::SkipDefinition { target: old }
                if site.kind == DirectiveKind::Definition =>
            {
                *old = target
            }
            AnalysisOverride::Branch {
                target: old,
                branch_id,
            } if site.kind == DirectiveKind::Conditional => {
                *branch_id = match binding.branch_index {
                    Some(index) => {
                        let branch = site
                            .branches
                            .get(index)
                            .filter(|branch| {
                                Some(&branch.expected_text) == binding.branch_text.as_ref()
                            })
                            .ok_or_else(|| {
                                refusal(
                                    "ANALYSIS_PRESET_STALE",
                                    "The saved conditional arm changed.",
                                )
                            })?;
                        Some(branch.branch_id.clone())
                    }
                    None if branch_id.is_none() && binding.branch_text.is_none() => None,
                    _ => {
                        return Err(refusal(
                            "ANALYSIS_PRESET_STALE",
                            "The saved conditional arm evidence is invalid.",
                        ));
                    }
                };
                *old = target;
            }
            _ => {
                return Err(refusal(
                    "ANALYSIS_PRESET_STALE",
                    "The saved override kind does not match its unchanged directive.",
                ));
            }
        }
    }
    Ok(())
}

impl Backend {
    pub(super) fn analysis_context_command(&self, arguments: &[Value]) -> Value {
        context(&mut self.lock_inner(), arguments).unwrap_or_else(|refusal| refusal)
    }
    pub(super) async fn set_analysis_configuration(&self, arguments: &[Value]) -> Value {
        let output = self.output_gate.lock().await;
        let (result, publications) = {
            let mut inner = self.lock_inner();
            match set(&mut inner, arguments) {
                Ok(result) => {
                    inner.document_analysis.cancel();
                    (result, inner.refresh_diagnostics(None))
                }
                Err(refusal) => (refusal, Vec::new()),
            }
        };
        ordering::committed();
        for (uri, version, diagnostics) in publications {
            self.client
                .publish_diagnostics(uri, diagnostics, version)
                .await;
        }
        drop(output);
        self.refresh_presentation(true).await;
        self.kick_workspace();
        result
    }
}

fn requested_paths(argument: &Value) -> std::result::Result<Option<Vec<PathBuf>>, Value> {
    match argument.get("include_paths") {
        Some(Value::Null)=>Ok(None),
        Some(value)=>Ok(Some(serde_json::from_value::<Vec<String>>(value.clone()).map_err(|_|refusal("INVALID_ARGUMENTS","include_paths must be an array of absolute folders, or null to inherit settings."))?.into_iter().map(|path| { let path=uri_to_path(&path); if !path.is_absolute() { Err(refusal("INVALID_ARGUMENTS","Analysis include folders must be resolved against their owning workspace folder.")) }else{Ok(path)} }).collect::<std::result::Result<Vec<_>,_>>()?)),
        None=>Err(refusal("INVALID_ARGUMENTS","include_paths is required; use null to inherit the root settings.")),
    }
}

fn set(inner: &mut Inner, arguments: &[Value]) -> std::result::Result<Value, Value> {
    let (root, document) = root_argument(inner, arguments)?;
    let argument = &arguments[0];
    let current = context_revision(inner, &root).ok_or_else(|| {
        refusal(
            "ANALYSIS_CONTEXT_UNAVAILABLE",
            "The current source inputs could not be guarded. Refresh the analysis selector.",
        )
    })?;
    if argument.get("expected_revision").and_then(Value::as_str) != Some(current.as_str())
        || argument
            .get("expected_selection_generation")
            .and_then(Value::as_u64)
            != Some(inner.analysis_configuration.generation(&root))
    {
        return Err(refusal(
            "ANALYSIS_CONTEXT_STALE",
            "The source or analysis selection changed. Open the selector again; the previous accepted configuration was kept.",
        ));
    }
    let mut choices: AnalysisConfiguration = serde_json::from_value(
        argument
            .get("analysis")
            .cloned()
            .ok_or_else(|| refusal("INVALID_ARGUMENTS", "analysis choices are required."))?,
    )
    .map_err(|error| {
        refusal(
            "INVALID_ARGUMENTS",
            format!("Invalid analysis choices: {error}"),
        )
    })?;
    let paths = requested_paths(argument)?;
    let mut budget = MacroBudget::with_limit(inner.settings.resolve(&root).macro_work_limit, None);
    if let Some(bindings) = argument.get("preset_bindings") {
        let bindings = serde_json::from_value(bindings.clone()).map_err(|_| {
            refusal(
                "ANALYSIS_PRESET_STALE",
                "Saved override evidence is invalid.",
            )
        })?;
        rebind(inner, &root, &mut choices, bindings, &mut budget)?;
    }
    let inherited = inner.settings.resolve(&root).search_paths;
    let browsing = inner.analysis_browsing_contexts(&root);
    inner
        .workspace
        .set_root_analysis_configuration_with_paths(
            root.as_str(),
            choices,
            Some(paths.clone().unwrap_or(inherited)),
        )
        .map_err(|cause| refusal(cause.code, cause.message))?;
    inner.analysis_configuration.accept(&root, paths);
    for document in browsing {
        inner
            .document_contexts
            .entry(document)
            .or_insert(root.clone());
    }
    inner.analysis_configuration_committed(&root);
    let acknowledgement = [
        json!({"root_uri":root,"document_uri":document,"document_version":inner.document(&document).map(|document|document.version)}),
    ];
    let mut result=context(inner,&acknowledgement).unwrap_or_else(|cause|json!({"schema_version":1,"root_uri":root,"document_uri":document,
        "document_version":inner.document(&document).map(|document|document.version),"revision":"",
        "analysis":metadata(inner,&root),"sites":[],"source_coverage":{"complete":false,"cause":cause.get("error")},
        "partial":true,"message":"The analysis configuration was accepted, but its current source context could not be guarded. Refresh the selector before another action."}));
    result["accepted"] = json!(true);
    if let Some(cause) = result["analysis"]["cause"].as_object().cloned() {
        let message = cause.get("message").and_then(Value::as_str).unwrap_or("The analysis configuration was accepted, but equation selection could not be applied. Selected model facts are unavailable.");
        result["partial"] = json!(true);
        result["message"] = json!(message);
        result["cause"] = json!(cause);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(text: &str) -> (Inner, Url) {
        let root = Url::parse("file:///analysis-config/main.mod").unwrap();
        let mut inner = Inner::default();
        inner.analysis_configuration.enabled = true;
        edit(&mut inner, &root, text, 1);
        (inner, root)
    }
    fn edit(inner: &mut Inner, root: &Url, text: &str, version: i32) {
        inner.workspace.update_document(root.as_str(), text);
        inner.docs.insert(
            root.clone(),
            OpenDoc {
                text: text.into(),
                version,
                diagnostics: Vec::new(),
                library: Vec::new(),
            },
        );
    }
    fn info(inner: &mut Inner, root: &Url) -> Value {
        let version = inner.document(root).unwrap().version;
        context(
            inner,
            &[json!({"root_uri":root,"document_uri":root,"document_version":version})],
        )
        .unwrap()
    }
    fn request(root: &Url, proof: &Value, analysis: Value) -> Value {
        json!({"root_uri":root,"document_uri":root,"document_version":proof["document_version"],"expected_revision":proof["revision"],
            "expected_selection_generation":proof["analysis"]["selection_generation"],"analysis":analysis,"include_paths":null})
    }

    #[test]
    fn untitled_sites_accept_transient_definition_and_branch_overrides() {
        // Null file targets identify this unnamed root, while each operation
        // still requires the exact source revision and directive identity.
        let mut inner = Inner::default();
        inner.analysis_configuration.enabled = true;
        let root = Url::parse("untitled:Untitled-1.mod").unwrap();
        edit(
            &mut inner,
            &root,
            "@#define FLAG=0\n@#if FLAG\nvar a;\n@#else\nvar b;\n@#endif\n",
            1,
        );
        let first = info(&mut inner, &root);
        let sites = first["sites"].as_array().unwrap();
        let definition = sites
            .iter()
            .find(|site| site["kind"] == "definition")
            .unwrap();
        let conditional = sites
            .iter()
            .find(|site| site["kind"] == "conditional")
            .unwrap();
        assert!(definition["target"]["file"].is_null());
        let choices = json!({"overrides":[
            {"kind":"definition","target":definition["target"],"expression":"1"},
            {"kind":"branch","target":conditional["target"],"branch_id":conditional["branches"][1]["branch_id"]}
        ]});
        let accepted = set(&mut inner, &[request(&root, &first, choices)]).unwrap();
        assert_eq!(accepted["analysis"]["state"], "selected");
        assert_eq!(accepted["analysis"]["selection_generation"], 1);
        assert_eq!(
            accepted["analysis"]["overrides"][0]["application"],
            "applied"
        );
        assert_eq!(
            accepted["analysis"]["overrides"][1]["application"],
            "applied"
        );
        // The unnamed identity must still supply source presentation after
        // selection and reset, including availability and override receipts.
        let projected = super::super::source_intelligence::file(&mut inner, &root).unwrap();
        assert!(projected.file.is_none());
        let inactive = projected
            .inventory
            .items
            .iter()
            .find(|item| item.name.as_deref() == Some("a"))
            .unwrap();
        let inactive_byte = inactive.selection.start;
        assert_eq!(
            projected
                .availability
                .iter()
                .find(|row| row.source_id == inactive.source_id)
                .unwrap()
                .availability,
            crate::engine::analysis_configuration::SourceAvailability::Inactive
        );
        let definition_hover =
            super::super::source_intelligence::hover(&mut inner, &root, 4).unwrap();
        assert!(
            serde_json::to_string(&definition_hover)
                .unwrap()
                .contains("Application: applied")
        );
        set(&mut inner, &[request(&root, &accepted, json!({}))]).unwrap();
        let inactive_hover =
            super::super::source_intelligence::hover(&mut inner, &root, inactive_byte).unwrap();
        assert!(
            serde_json::to_string(&inactive_hover)
                .unwrap()
                .contains("inactive")
        );
    }

    #[test]
    fn selection_generation_changes_only_after_guarded_acceptance() {
        // A refused expression or delayed old selector cannot replace the
        // accepted choices or consume its server selection generation.
        let (mut inner, root) = state("var y; model; y=0; end;\n");
        let first = info(&mut inner, &root);
        let invalid = request(
            &root,
            &first,
            json!({"defines":[{"name":"N","expression":"MISSING"}]}),
        );
        assert!(set(&mut inner, &[invalid]).is_err());
        assert_eq!(inner.analysis_configuration.generation(&root), 0);
        let valid = request(
            &root,
            &first,
            json!({"name":"Alternative","defines":[{"name":"N","expression":"2"}]}),
        );
        let accepted = set(&mut inner, &[valid]).unwrap();
        assert_eq!(accepted["analysis"]["selection_generation"], 1);
        assert_eq!(accepted["analysis"]["selection"]["name"], "Alternative");
        let delayed = request(&root, &first, json!({}));
        assert_eq!(
            set(&mut inner, &[delayed]).unwrap_err()["code"],
            "ANALYSIS_CONTEXT_STALE"
        );
        assert_eq!(inner.analysis_configuration.generation(&root), 1);
        assert_eq!(
            inner
                .workspace
                .root_analysis_configuration(root.as_str())
                .name
                .as_deref(),
            Some("Alternative")
        );
    }

    #[test]
    fn guarded_selection_refreshes_changed_semantics_and_reuses_unrelated_root_view() {
        // A root-local selection must refresh its accepted names without
        // discarding the valid immutable binding view of another root.
        use crate::engine::analysis::symbols::Selector;
        let (mut inner, root) =
            state("@#ifdef ALT\ndatabase alternative;\n@#else\ndatabase base;\n@#endif\n");
        let other = Url::parse("file:///analysis-config/other.mod").unwrap();
        edit(&mut inner, &other, "database unrelated;\n", 1);
        let original = super::super::typed_names::root_view(&mut inner, &root).unwrap();
        let unrelated = super::super::typed_names::root_view(&mut inner, &other).unwrap();
        assert!(original.select(Selector::Name("base")).unwrap().is_some());
        assert!(
            original
                .select(Selector::Name("alternative"))
                .unwrap()
                .is_none()
        );
        assert!(
            unrelated
                .select(Selector::Name("unrelated"))
                .unwrap()
                .is_some()
        );
        let proof = info(&mut inner, &root);
        let accepted = set(
            &mut inner,
            &[request(
                &root,
                &proof,
                json!({"defines":[{"name":"ALT","expression":"false"}]}),
            )],
        )
        .unwrap();
        assert_eq!(accepted["accepted"], true);
        let selected = super::super::typed_names::root_view(&mut inner, &root).unwrap();
        assert!(!Arc::ptr_eq(&original, &selected));
        assert!(
            selected
                .select(Selector::Name("alternative"))
                .unwrap()
                .is_some()
        );
        assert!(selected.select(Selector::Name("base")).unwrap().is_none());
        let retained = super::super::typed_names::root_view(&mut inner, &other).unwrap();
        assert!(Arc::ptr_eq(&unrelated, &retained));
        assert!(
            retained
                .select(Selector::Name("unrelated"))
                .unwrap()
                .is_some()
        );
        assert!(
            retained
                .select(Selector::Name("alternative"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn presets_rebind_only_explicit_unique_unchanged_directives() {
        // Ordinary stale targets stay refused. An explicit saved directive can
        // rebind after an unrelated edit, but ambiguity cannot choose a site.
        let source = "@#define FLAG=0\n@#if FLAG\nvar a;\n@#else\nvar b;\n@#endif\n";
        let (mut inner, root) = state(source);
        let first = info(&mut inner, &root);
        let target = first["sites"]
            .as_array()
            .unwrap()
            .iter()
            .find(|site| site["kind"] == "definition")
            .unwrap()["target"]
            .clone();
        let choices = json!({"name":"Saved","overrides":[{"kind":"definition","target":target,"expression":"1"}]});
        set(&mut inner, &[request(&root, &first, choices.clone())]).unwrap();
        let prepared=context(&mut inner,&[json!({"root_uri":root,"document_uri":root,"document_version":1,"prepare_preset":true})]).unwrap();
        let bindings = prepared["preset_bindings"].clone();
        edit(&mut inner, &root, &format!("// unrelated\n{source}"), 2);
        let stale = info(&mut inner, &root);
        assert_eq!(stale["analysis"]["state"], "stale");
        assert_eq!(
            set(&mut inner, &[request(&root, &stale, choices.clone())]).unwrap_err()["code"],
            "ANALYSIS_TARGET_STALE"
        );
        let mut saved = request(&root, &stale, choices.clone());
        saved["preset_bindings"] = bindings.clone();
        let accepted = set(&mut inner, &[saved]).unwrap();
        assert_eq!(accepted["analysis"]["selection_generation"], 2);
        edit(&mut inner, &root, &format!("{source}@#define FLAG=0\n"), 3);
        let ambiguous = info(&mut inner, &root);
        let mut saved = request(&root, &ambiguous, choices);
        saved["preset_bindings"] = bindings;
        assert_eq!(
            set(&mut inner, &[saved]).unwrap_err()["code"],
            "ANALYSIS_PRESET_AMBIGUOUS"
        );
        assert_eq!(inner.analysis_configuration.generation(&root), 2);
    }

    #[test]
    fn comparison_preparation_never_changes_the_live_selection() {
        // Preparing captured sides cannot apply choices to the live editor,
        // consume a generation, or inherit its selected path override.
        let (mut inner, root) = state("var y; model; y=0; end;\n");
        let before = info(&mut inner, &root);
        let prepared=context(&mut inner,&[json!({"root_uri":root,"document_uri":root,"document_version":1,"expected_revision":before["revision"],
            "expected_selection_generation":0,"prepare_analysis":{"name":"Side A","defines":[{"name":"N","expression":"2"}]},"include_paths":null})]).unwrap();
        assert_eq!(prepared["prepared_analysis"]["name"], "Side A");
        assert_eq!(prepared["analysis"]["selection_generation"], 0);
        assert!(
            inner
                .workspace
                .root_analysis_configuration(root.as_str())
                .defines
                .is_empty()
        );
    }
}
