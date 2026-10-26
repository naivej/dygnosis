//! Common MCP source acquisition. Each request owns its workspace and checked
//! input guards; supplied maps cannot consult server-host files.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::engine::analysis_configuration::{
    AnalysisConfiguration, SourceStructure, SourceStructureQuery,
};
use crate::engine::language::span::{LineIndex, Span};
use crate::engine::language::{expand::ExpandReport, model::Model};
use crate::engine::macro_work::MacroWorkLimit;
use crate::engine::workspace::include_resolver::{normalize_uri, supplied_path_key};
use crate::engine::workspace::{InputGuard, InputStamps, Workspace};

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CaptureBoundary {
    SourceStructure,
    AnalysisMetadata,
}

#[cfg(test)]
type CaptureHook = (CaptureBoundary, Box<dyn FnOnce()>);

#[cfg(test)]
thread_local! {
    static CAPTURE_HOOK: std::cell::RefCell<Option<CaptureHook>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn with_capture_hook<T>(
    boundary: CaptureBoundary,
    hook: impl FnOnce() + 'static,
    action: impl FnOnce() -> T,
) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            CAPTURE_HOOK.with(|slot| {
                slot.borrow_mut().take();
            });
        }
    }
    CAPTURE_HOOK.with(|slot| {
        assert!(
            slot.borrow_mut()
                .replace((boundary, Box::new(hook)))
                .is_none()
        );
    });
    let _reset = Reset;
    action()
}

#[cfg(test)]
fn capture_boundary(boundary: CaptureBoundary) {
    let hook = CAPTURE_HOOK.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot
            .as_ref()
            .is_some_and(|(selected, _)| *selected == boundary)
        {
            slot.take().map(|(_, hook)| hook)
        } else {
            None
        }
    });
    if let Some(hook) = hook {
        hook();
    }
}

fn captured_owners_current(
    workspace: &Workspace,
    proofs: &BTreeMap<String, (String, InputStamps)>,
) -> Result<(), SourceError> {
    #[cfg(feature = "experimental-mcp-reuse")]
    let _measurement = super::retention::measure(super::retention::Phase::Proof);
    workspace
        .check_cancelled()
        .map_err(|_| SourceError::failure("CANCELLED", "The request was cancelled"))?;
    if proofs
        .values()
        .any(|(_, proof)| !workspace.inputs_match(proof))
    {
        return Err(SourceError::failure(
            "INPUT_CHANGED",
            "An analyzed source or lookup changed during capture. No model facts were returned under refreshed input guards. Repeat the request.",
        ));
    }
    Ok(())
}

const TEXT_ROOT: &str = "dygnosis-text:/root.mod";

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceInput {
    /// Unnamed supplied source. It cannot read includes or companions from disk.
    Text { text: String },
    /// Supplied files only. The root and owners must be exact existing keys.
    Map {
        root: String,
        files: BTreeMap<String, String>,
        #[serde(default)]
        root_text: Option<String>,
        #[serde(default)]
        include_dirs: Vec<String>,
        #[serde(default)]
        owner_roots: Vec<String>,
    },
    /// Saved server-host files. All root, owner and search-folder paths are absolute.
    Disk {
        root: String,
        #[serde(default)]
        include_dirs: Vec<String>,
        #[serde(default)]
        owner_roots: Vec<String>,
    },
}

/// Shared single-model parameters, flattened by tool parameter structs.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct SourceArgs {
    /// Request-local macro inputs and guarded analysis overrides. Omission selects Source.
    pub analysis: Option<AnalysisConfiguration>,
    /// Finite macro work units per checked root, from 1 to 100000000. Omission uses the default. Source, output, value-size and depth bounds remain separate.
    #[serde(default)]
    pub macro_work_limit: MacroWorkLimit,
    /// Explicit text, supplied-map or saved server-host disk input. Exclusive with file_content, active_file and files.
    pub source: Option<SourceInput>,
    /// Legacy unnamed text, or an optional overlay of the exact existing map root. An empty string is an overlay.
    pub file_content: Option<String>,
    /// Legacy exact root key in a nonempty files map. This path never grants disk access.
    pub active_file: Option<String>,
    /// Legacy supplied files only; lookup resolves slash, dot and dot-dot aliases without disk reads or case folding. A nonempty map requires an exact existing active_file key; an empty map uses file_content alone.
    pub files: Option<HashMap<String, String>>,
    /// Returned source revision. Required when reusing snapshot-bound selections, proposals or continuation.
    pub expected_revision: Option<String>,
}

impl SourceArgs {
    pub fn input(&self) -> Result<SourceInput, SourceError> {
        if let Some(source) = &self.source {
            if self.file_content.is_some() || self.active_file.is_some() || self.files.is_some() {
                return Err(SourceError::invalid(
                    "source cannot be combined with file_content, active_file or files",
                ));
            }
            return Ok(source.clone());
        }
        resolve_legacy(
            self.file_content.as_deref(),
            self.active_file.as_deref(),
            self.files.as_ref(),
        )
    }
}

pub fn resolve_legacy(
    text: Option<&str>,
    active: Option<&str>,
    files: Option<&HashMap<String, String>>,
) -> Result<SourceInput, SourceError> {
    if let Some(files) = files.filter(|files| !files.is_empty()) {
        let root = active.ok_or_else(|| {
            SourceError::invalid("active_file is required with a nonempty files map")
        })?;
        if !files.contains_key(root) {
            return Err(SourceError::invalid(format!(
                "{root:?} is not in the file map"
            )));
        }
        Ok(SourceInput::Map {
            root: root.to_owned(),
            files: files
                .iter()
                .map(|(key, text)| (key.clone(), text.clone()))
                .collect(),
            root_text: text.map(str::to_owned),
            include_dirs: Vec::new(),
            owner_roots: Vec::new(),
        })
    } else {
        Ok(SourceInput::Text {
            text: text
                .ok_or_else(|| SourceError::invalid("Supply source or file_content"))?
                .to_owned(),
        })
    }
}

/// Acquisition ceilings. Macro expansion retains its own work/output bounds.
#[derive(Clone, Debug, Serialize)]
pub struct SourceLimits {
    pub max_file_bytes: usize,
    pub max_source_bytes: usize,
    pub max_files: usize,
    pub max_roots: usize,
}

impl Default for SourceLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 1_000_000,
            max_source_bytes: 8 * 1024 * 1024,
            max_files: 256,
            max_roots: 64,
        }
    }
}

#[derive(Clone, Default)]
pub struct RequestControl {
    #[cfg(feature = "experimental-mcp-reuse")]
    pub(super) retention: Option<Arc<super::retention::Session>>,
    pub cancelled: Arc<AtomicBool>,
    pub limits: SourceLimits,
    pub progress: Arc<ProgressState>,
    pub macro_work_limit: MacroWorkLimit,
}

pub struct ProgressState {
    pub completed: AtomicUsize,
    pub phase: Mutex<&'static str>,
}

impl Default for ProgressState {
    fn default() -> Self {
        Self {
            completed: AtomicUsize::new(0),
            phase: Mutex::new("Read sources"),
        }
    }
}

impl RequestControl {
    pub fn complete_phase(&self, phase: &'static str) {
        if self.cancelled.load(Ordering::Relaxed) {
            return;
        }
        let mut current = self
            .progress
            .phase
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *current = phase;
        self.progress.completed.fetch_add(1, Ordering::SeqCst);
    }
    pub fn check(&self) -> Result<(), SourceError> {
        if self.cancelled.load(Ordering::Relaxed) {
            Err(SourceError::failure(
                "CANCELLED",
                "The request was cancelled",
            ))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug)]
pub struct SourceError {
    pub code: &'static str,
    pub message: String,
    pub invalid_arguments: bool,
}

impl SourceError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: "INVALID_ARGUMENTS",
            message: message.into(),
            invalid_arguments: true,
        }
    }
    pub fn failure(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            invalid_arguments: false,
        }
    }
}

pub struct SourceAnalysis {
    pub model: Arc<Model>,
    pub report: Arc<ExpandReport>,
    pub coverage: Value,
}

pub struct SourceSnapshot {
    pub source_structure: Option<SourceStructure>,
    pub workspace: Workspace,
    /// Internal selected root. Text mode uses an explicitly synthetic root.
    pub root: String,
    pub root_file: Option<String>,
    pub owner_roots: Vec<String>,
    pub revision: String,
    pub model: Arc<Model>,
    pub report: Arc<ExpandReport>,
    pub raw: String,
    pub sources: BTreeMap<String, String>,
    pub line_indexes: BTreeMap<String, LineIndex>,
    pub analyses: BTreeMap<String, SourceAnalysis>,
    pub inputs: Value,
    pub guards: Vec<InputGuard>,
    pub coverage: Value,
}

#[derive(Clone, Copy)]
struct ReuseView<'a> {
    workspace: &'a Workspace,
    root: &'a str,
    owner_roots: &'a [String],
    exact_input: bool,
}

impl<'a> ReuseView<'a> {
    fn snapshot(snapshot: &'a SourceSnapshot, exact_input: bool) -> Self {
        Self {
            workspace: &snapshot.workspace,
            root: &snapshot.root,
            owner_roots: &snapshot.owner_roots,
            exact_input,
        }
    }

    fn copy_for(
        &self,
        root: &str,
        owners: &[String],
        control: &RequestControl,
    ) -> Option<Workspace> {
        (self.exact_input
            && root == self.root
            && owners == self.owner_roots
            && control.macro_work_limit == self.workspace.root_macro_work_limit(self.root))
        .then(|| self.workspace.snapshot_with_root_settings(root))
    }
}

#[cfg(feature = "experimental-mcp-reuse")]
pub(super) struct ReusableSource {
    workspace: Workspace,
    root: String,
    owner_roots: Vec<String>,
    #[cfg(test)]
    model: std::sync::Weak<Model>,
}

#[cfg(feature = "experimental-mcp-reuse")]
impl ReusableSource {
    #[cfg(test)]
    pub(super) fn model_weak(&self) -> std::sync::Weak<Model> {
        self.model.clone()
    }
    fn view(&self) -> ReuseView<'_> {
        ReuseView {
            workspace: &self.workspace,
            root: &self.root,
            owner_roots: &self.owner_roots,
            exact_input: true,
        }
    }
}

impl SourceSnapshot {
    #[cfg(feature = "experimental-mcp-reuse")]
    pub(super) fn reusable(&self) -> ReusableSource {
        let mut workspace = self.workspace.snapshot_with_root_settings(&self.root);
        workspace.clear_request_cancellation();
        ReusableSource {
            workspace,
            root: self.root.clone(),
            owner_roots: self.owner_roots.clone(),
            #[cfg(test)]
            model: Arc::downgrade(&self.model),
        }
    }
    pub fn range(&self, file: &str, span: Span) -> Option<Value> {
        let text = self.text(Some(file))?;
        text.get(span.start as usize..span.end as usize)?;
        let index = self.line_indexes.get(file)?;
        let start = index.position(text, span.start);
        let end = index.position(text, span.end);
        Some(
            json!({"line":start.line+1,"column":start.character+1,"end_line":end.line+1,"end_column":end.character+1}),
        )
    }
    pub fn text(&self, file: Option<&str>) -> Option<&str> {
        self.sources
            .get(file.unwrap_or(&self.root))
            .map(String::as_str)
    }

    pub fn written_file(&self, file: Option<&str>) -> Option<String> {
        if self.root_file.is_none() {
            None
        } else {
            file.map(str::to_owned).or_else(|| self.root_file.clone())
        }
    }
}

pub fn resolve_source(
    input: SourceInput,
    expected_revision: Option<&str>,
    control: &RequestControl,
) -> Result<SourceSnapshot, SourceError> {
    resolve_source_configured(input, expected_revision, control, None, None)
}

/// Acquire choices and explicitly inspected files under the same source policy
/// before computing identity. Source browsing never adds an execution owner.
pub fn resolve_source_configured(
    input: SourceInput,
    expected_revision: Option<&str>,
    control: &RequestControl,
    analysis: Option<&AnalysisConfiguration>,
    inspection: Option<&SourceStructureQuery>,
) -> Result<SourceSnapshot, SourceError> {
    resolve_source_configured_reusing(
        input,
        expected_revision,
        control,
        analysis,
        inspection,
        None,
    )
}

/// A caller can share immutable source records from another explicit job.
/// Only exactly equal caller inputs can also share root analysis results.
pub(super) fn resolve_source_configured_reusing(
    input: SourceInput,
    expected_revision: Option<&str>,
    control: &RequestControl,
    analysis: Option<&AnalysisConfiguration>,
    inspection: Option<&SourceStructureQuery>,
    previous: Option<(&SourceSnapshot, bool)>,
) -> Result<SourceSnapshot, SourceError> {
    #[cfg(test)]
    let probe = super::reuse_probe::Checkout::new(
        &input,
        control,
        analysis,
        inspection.is_some(),
        previous.is_none(),
    );
    #[cfg(test)]
    let previous = previous.or_else(|| probe.previous());
    let previous = previous.map(|(snapshot, exact)| ReuseView::snapshot(snapshot, exact));
    #[cfg(feature = "experimental-mcp-reuse")]
    let lease = control
        .retention
        .as_ref()
        .and_then(|session| session.checkout(&input, control, analysis, previous.is_none()));
    #[cfg(feature = "experimental-mcp-reuse")]
    let previous = previous.or_else(|| {
        lease
            .as_ref()
            .and_then(|lease| lease.source().map(ReusableSource::view))
    });
    let result = resolve_source_configured_inner(
        input,
        expected_revision,
        control,
        analysis,
        inspection,
        previous,
    );
    #[cfg(test)]
    if let Ok(snapshot) = &result {
        probe.retain(snapshot, control);
    }
    #[cfg(feature = "experimental-mcp-reuse")]
    if let (Some(lease), Ok(snapshot)) = (lease, &result) {
        lease.complete(snapshot, control);
    }
    result
}

fn resolve_source_configured_inner(
    input: SourceInput,
    expected_revision: Option<&str>,
    control: &RequestControl,
    analysis: Option<&AnalysisConfiguration>,
    inspection: Option<&SourceStructureQuery>,
    previous: Option<ReuseView<'_>>,
) -> Result<SourceSnapshot, SourceError> {
    #[cfg(feature = "experimental-mcp-reuse")]
    let _measurement = super::retention::measure(super::retention::Phase::Source);
    control.check()?;
    let (mut workspace, root, owner_roots, kind, include_dirs, root_file, reused) = match input {
        SourceInput::Text { text } => {
            check_files(
                &BTreeMap::from([(TEXT_ROOT.into(), text.clone())]),
                &control.limits,
            )?;
            let cached = previous
                .and_then(|previous| previous.copy_for(TEXT_ROOT, &[TEXT_ROOT.into()], control));
            let reused = cached.is_some();
            let mut workspace = cached.unwrap_or_else(|| {
                Workspace::overlay_documents_with_work_limit(
                    &BTreeMap::from([(TEXT_ROOT.into(), text)]),
                    Some(Arc::clone(&control.cancelled)),
                    control.macro_work_limit,
                )
            });
            workspace.set_root_search_paths(TEXT_ROOT, Vec::new());
            workspace.set_unnamed_root(TEXT_ROOT);
            (
                workspace,
                TEXT_ROOT.into(),
                vec![TEXT_ROOT.into()],
                "text",
                Vec::new(),
                None,
                reused,
            )
        }
        SourceInput::Map {
            root,
            mut files,
            root_text,
            include_dirs,
            owner_roots,
        } => {
            if files.is_empty() || !files.contains_key(&root) {
                return Err(SourceError::invalid(
                    "Map source requires a nonempty files map and an exact existing root key",
                ));
            }
            validate_aliases(&files)?;
            let owners = checked_owners(&root, owner_roots);
            if owners.len() > control.limits.max_roots {
                return Err(SourceError::failure(
                    "ROOT_LIMIT",
                    "The checked owner count exceeds the request root limit",
                ));
            }
            if owners.iter().any(|owner| !files.contains_key(owner)) {
                return Err(SourceError::invalid(
                    "Every owner_roots entry must be an exact existing key in files",
                ));
            }
            if let Some(text) = root_text {
                files.insert(root.clone(), text);
            }
            check_files(&files, &control.limits)?;
            validate_paths(&include_dirs)?;
            let cached = previous.and_then(|previous| previous.copy_for(&root, &owners, control));
            let reused = cached.is_some();
            let mut workspace = cached.unwrap_or_else(|| {
                Workspace::overlay_documents_with_work_limit(
                    &files,
                    Some(Arc::clone(&control.cancelled)),
                    control.macro_work_limit,
                )
            });
            let paths: Vec<_> = include_dirs
                .iter()
                .map(|dir| PathBuf::from(supplied_path_key(dir)))
                .collect();
            for owner in &owners {
                workspace.set_root_search_paths(owner, paths.clone());
            }
            let root_file = Some(root.clone());
            (
                workspace,
                root,
                owners,
                "map",
                include_dirs,
                root_file,
                reused,
            )
        }
        SourceInput::Disk {
            root,
            include_dirs,
            owner_roots,
        } => {
            let supplied = checked_owners(&root, owner_roots);
            if supplied.len() > control.limits.max_roots {
                return Err(SourceError::failure(
                    "ROOT_LIMIT",
                    "The checked owner count exceeds the request root limit",
                ));
            }
            if supplied
                .iter()
                .chain(&include_dirs)
                .any(|path| !Path::new(path).is_absolute() || path.contains('\0'))
            {
                return Err(SourceError::invalid(
                    "Disk root, owner_roots and include_dirs must be absolute server-host paths",
                ));
            }
            let owners = checked_owners(
                &normalize_uri(&root),
                supplied
                    .into_iter()
                    .map(|owner| normalize_uri(&owner))
                    .collect(),
            );
            let root = normalize_uri(&root);
            let cached = previous.and_then(|previous| previous.copy_for(&root, &owners, control));
            let reused = cached.is_some();
            let mut workspace = cached.unwrap_or_else(|| {
                Workspace::with_search_paths(include_dirs.iter().map(PathBuf::from).collect())
            });
            workspace.set_default_macro_work_limit(control.macro_work_limit);
            for owner in &owners {
                workspace
                    .set_root_search_paths(owner, include_dirs.iter().map(PathBuf::from).collect());
            }
            (
                workspace,
                root.clone(),
                owners,
                "disk",
                include_dirs,
                Some(root),
                reused,
            )
        }
    };
    if owner_roots.len() > control.limits.max_roots {
        return Err(SourceError::failure(
            "ROOT_LIMIT",
            "The checked owner count exceeds the request root limit",
        ));
    }
    workspace.set_cancellation(Arc::clone(&control.cancelled));
    workspace.set_source_limits(
        control.limits.max_file_bytes,
        control.limits.max_source_bytes,
        control.limits.max_files,
    );
    if !reused && let Some(previous) = previous {
        workspace.reuse_comparison_sources(previous.workspace, previous.root);
    }
    if reused || analysis.is_some() {
        workspace
            .set_root_analysis_configuration(&root, analysis.cloned().unwrap_or_default())
            .map_err(|error| SourceError::failure(error.code, error.message))?;
    }
    let mut analyses = BTreeMap::new();
    let mut owner_proofs = BTreeMap::new();
    let mut guards = BTreeMap::new();
    let mut sources = BTreeMap::new();
    for owner in &owner_roots {
        control.check()?;
        let Some(owner_revision) = workspace.input_revision(owner) else {
            control.check()?;
            return Err(acquisition_error(&workspace, owner));
        };
        owner_proofs.insert(
            owner.clone(),
            (owner_revision, workspace.input_stamps(owner)),
        );
        let model = workspace.effective_model_snapshot(owner).ok_or_else(|| {
            control
                .check()
                .err()
                .unwrap_or_else(|| acquisition_error(&workspace, owner))
        })?;
        control.check()?;
        let report = workspace.expand_report_snapshot(owner).ok_or_else(|| {
            control
                .check()
                .err()
                .unwrap_or_else(|| acquisition_error(&workspace, owner))
        })?;
        control.check()?;
        let snapshot_current = {
            #[cfg(feature = "experimental-mcp-reuse")]
            let _measurement = super::retention::measure(super::retention::Phase::Proof);
            workspace.input_snapshot_is_current(owner)
        };
        if !snapshot_current {
            return Err(SourceError::failure(
                "INPUT_CHANGED",
                "Source inputs changed while reading them; repeat the request",
            ));
        }
        captured_owners_current(&workspace, &owner_proofs)?;
        let includes_complete = workspace.includes_complete(owner);
        let causes = crate::engine::control::run(Some(Arc::clone(&control.cancelled)), || {
            crate::engine::analysis::model_info::incomplete_causes(&mut workspace, owner, &model)
        })
        .map_err(|_| SourceError::failure("CANCELLED", "The request was cancelled"))?;
        let reasons: Vec<_> = causes.into_iter().map(|cause| {
            let mut reason = cause.outcome_json();
            let location = |site: &crate::engine::workspace::IncludeSite| {
                let raw = workspace.source_for_normalized_key(&site.file)?;
                let normalized = crate::engine::language::parser::normalize_newlines(raw);
                normalized.get(site.span.start as usize..site.span.end as usize)?;
                let index = LineIndex::new(&normalized);
                let start = index.position(&normalized, site.span.start);
                let end = index.position(&normalized, site.span.end);
                Some(json!({"file":if kind=="text" {Value::Null}else {json!(site.file)},
                    "range":{"line":start.line+1,"column":start.character+1,"end_line":end.line+1,"end_column":end.character+1}}))
            };
            if let Some(source) = cause.origin.as_ref().and_then(location) {
                reason["source"] = source;
            }
            let chain: Vec<_> = cause.include_chain.iter().filter_map(location).collect();
            if !chain.is_empty() { reason["include_chain"] = json!(chain); }
            reason
        }).collect();
        control.check()?;
        let complete =
            crate::engine::analysis::model_info::incomplete_model_status(&model, includes_complete)
                .is_none()
                && report.navigation_complete;
        let work_limits = crate::engine::analysis::model_info::macro_work_limits(
            &model,
            (kind != "text").then_some(owner.as_str()),
        );
        let coverage = json!({"root":if kind=="text" { Value::Null } else { json!(owner) },"complete":complete,"includes_complete":includes_complete,"reasons":reasons,"work_limits":work_limits});
        analyses.insert(
            owner.clone(),
            SourceAnalysis {
                model,
                report,
                coverage,
            },
        );
        for guard in workspace.input_guards(owner) {
            // Check the captured fact during the existing collection pass;
            // a reused document must obey this caller's per-file ceiling too.
            if guard.exists
                && !guard.directory
                && guard
                    .length
                    .is_some_and(|length| length > control.limits.max_file_bytes as u64)
            {
                return Err(SourceError::failure(
                    "SOURCE_LIMIT",
                    "A checked file exceeds the request file byte limit",
                ));
            }
            guards.insert(guard.file.clone(), guard);
        }
        if guards.len() > control.limits.max_files
            || guards
                .values()
                .filter(|guard| guard.exists && !guard.directory)
                .try_fold(0usize, |used, guard| {
                    used.checked_add(guard.length.unwrap_or(0) as usize)
                })
                .is_none_or(|bytes| bytes > control.limits.max_source_bytes)
        {
            return Err(SourceError::failure(
                "SOURCE_LIMIT",
                "Checked source bytes or lookup candidate count exceeds the request source limit",
            ));
        }
        for key in workspace.snapshot_sources(owner).keys() {
            if let Some(text) = workspace.source_for_normalized_key(key) {
                sources.insert(key.clone(), text.to_owned());
            }
        }
        control.complete_phase("Analyze source");
    }
    #[cfg(test)]
    if inspection.is_some() {
        capture_boundary(CaptureBoundary::SourceStructure);
    }
    captured_owners_current(&workspace, &owner_proofs)?;
    let source_structure = inspection
        .map(|query| workspace.source_structure(&root, query))
        .transpose()
        .map_err(|error| SourceError::failure(error.code, error.message))?;
    captured_owners_current(&workspace, &owner_proofs)?;
    if source_structure
        .as_ref()
        .and_then(|structure| structure.analysis_revision.as_deref())
        .is_some_and(|revision| revision != owner_proofs[&root].0)
    {
        return Err(SourceError::failure(
            "INPUT_CHANGED",
            "Source inspection refreshed the analyzed input identity. No earlier model facts were returned. Repeat the request.",
        ));
    }
    if let Some(structure) = &source_structure {
        for guard in &structure.input_guards {
            guards.insert(guard.file.clone(), guard.clone());
        }
        for file in &structure.files {
            sources.insert(
                file.file.clone().unwrap_or_else(|| root.clone()),
                file.text.as_str().to_owned(),
            );
        }
    }
    if let Some(message) = workspace.acquisition_limit() {
        return Err(SourceError::failure("SOURCE_LIMIT", message));
    }
    if guards.len() > control.limits.max_files
        || guards
            .values()
            .filter(|guard| guard.exists && !guard.directory)
            .try_fold(0usize, |bytes, guard| {
                bytes.checked_add(guard.length.unwrap_or(0) as usize)
            })
            .is_none_or(|bytes| bytes > control.limits.max_source_bytes)
    {
        return Err(SourceError::failure(
            "SOURCE_LIMIT",
            "Checked source and lookup candidate count exceeds the request file limit",
        ));
    }
    for owner in &owner_roots {
        control.check()?;
        let snapshot_current = {
            #[cfg(feature = "experimental-mcp-reuse")]
            let _measurement = super::retention::measure(super::retention::Phase::Proof);
            workspace.input_snapshot_is_current(owner)
        };
        if !snapshot_current {
            return Err(SourceError::failure(
                "INPUT_CHANGED",
                "Source inputs changed while reading the checked owners; repeat the request",
            ));
        }
    }
    let guards: Vec<_> = guards.into_values().collect();
    let structure_current = source_structure.is_none() || {
        #[cfg(feature = "experimental-mcp-reuse")]
        let _measurement = super::retention::measure(super::retention::Phase::Proof);
        workspace.source_guards_are_current(&guards)
    };
    if !structure_current {
        return Err(SourceError::failure(
            "INPUT_CHANGED",
            "Source or lookup facts changed while capturing this request; repeat the request.",
        ));
    }
    #[cfg(test)]
    capture_boundary(CaptureBoundary::AnalysisMetadata);
    captured_owners_current(&workspace, &owner_proofs)?;
    let metadata = workspace
        .analysis_metadata(&root)
        .map_err(|error| SourceError::failure(error.code, error.message))?;
    captured_owners_current(&workspace, &owner_proofs)?;
    if metadata.revision != owner_proofs[&root].0 {
        return Err(SourceError::failure(
            "INPUT_CHANGED",
            "Analysis metadata refreshed the analyzed input identity. No earlier model facts were returned. Repeat the request.",
        ));
    }
    let identity = json!({"source_policy":kind,"root":root_file,"owner_roots":owner_roots,"include_dirs":include_dirs,"macro_work_limit":control.macro_work_limit,"guards":guards,"configuration_id":metadata.selection.configuration_id,"source_structure_revision":source_structure.as_ref().map(|source| &source.revision)});
    let revision = sha256(serde_json::to_vec(&identity).expect("source identity is serializable"));
    if expected_revision.is_some_and(|expected| expected != revision) {
        return Err(SourceError::failure(
            "INPUT_CHANGED",
            "Source inputs no longer match expected_revision; start a new request",
        ));
    }
    control.check()?;
    let selected = &analyses[&root];
    let model = selected.model.clone();
    let report = selected.report.clone();
    let coverage = selected.coverage.clone();
    let raw = sources
        .get(&root)
        .cloned()
        .ok_or_else(|| acquisition_error(&workspace, &root))?;
    let mut inputs = json!({"source_policy":kind,"root":root_file,"synthetic_root":kind=="text","checked_owners":if kind=="text" { Vec::new() } else { owner_roots.clone() },"include_dirs":include_dirs,"macro_work_limit":control.macro_work_limit,"revision":revision,"guards":guards});
    inputs["analysis"] = serde_json::to_value(metadata.as_ref()).expect("analysis metadata");
    if kind == "text"
        && let Some(rows) = inputs["guards"].as_array_mut()
    {
        for row in rows {
            row["file"] = Value::Null;
        }
    }
    let line_indexes = sources
        .iter()
        .map(|(file, text)| (file.clone(), LineIndex::new(text)))
        .collect();
    control.check()?;
    Ok(SourceSnapshot {
        source_structure,
        workspace,
        root,
        root_file,
        owner_roots,
        revision,
        model,
        report,
        raw,
        sources,
        line_indexes,
        analyses,
        inputs,
        guards,
        coverage,
    })
}

pub fn sha256(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

fn checked_owners(root: &str, owners: Vec<String>) -> Vec<String> {
    let mut out = vec![root.to_owned()];
    for owner in owners {
        if !out.contains(&owner) {
            out.push(owner);
        }
    }
    // The root stays first; additional owner order is not semantic.
    out[1..].sort();
    out
}

fn validate_paths(paths: &[String]) -> Result<(), SourceError> {
    if paths
        .iter()
        .any(|path| path.contains('\0') || path.is_empty())
    {
        Err(SourceError::invalid(
            "Source paths must be nonempty and contain no NUL",
        ))
    } else {
        Ok(())
    }
}

fn validate_aliases(files: &BTreeMap<String, String>) -> Result<(), SourceError> {
    validate_paths(&files.keys().cloned().collect::<Vec<_>>())?;
    let mut identities = BTreeSet::new();
    for key in files.keys() {
        let lookup = supplied_path_key(key);
        if lookup.is_empty() || !identities.insert(lookup) {
            return Err(SourceError::invalid(
                "Two supplied file keys resolve to the same lexical lookup key",
            ));
        }
    }
    Ok(())
}

fn check_files(files: &BTreeMap<String, String>, limits: &SourceLimits) -> Result<(), SourceError> {
    let mut bytes = 0usize;
    if files.len() > limits.max_files {
        return Err(SourceError::failure(
            "SOURCE_LIMIT",
            "Supplied file count exceeds the request file limit",
        ));
    }
    for text in files.values() {
        if text.len() > limits.max_file_bytes
            || text.len() > limits.max_source_bytes.saturating_sub(bytes)
        {
            return Err(SourceError::failure(
                "SOURCE_LIMIT",
                "Supplied source bytes exceed the request source limit",
            ));
        }
        bytes += text.len();
    }
    Ok(())
}

fn acquisition_error(workspace: &Workspace, root: &str) -> SourceError {
    if let Some(message) = workspace.acquisition_limit() {
        SourceError::failure("SOURCE_LIMIT", message)
    } else {
        SourceError::failure(
            "SOURCE_READ_FAILED",
            format!("The selected model root could not be read: {root}"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    const MODEL: &str = "var x; model; x=0; end;";
    fn declared(model: &Model, name: &str) -> bool {
        model
            .intern
            .lookup(name)
            .and_then(|name| model.final_symbol_kind(name))
            .is_some()
    }
    fn mapped(root: &str, files: &[(&str, &str)]) -> SourceInput {
        SourceInput::Map {
            root: root.into(),
            files: files
                .iter()
                .map(|(key, text)| ((*key).into(), (*text).into()))
                .collect(),
            root_text: None,
            include_dirs: Vec::new(),
            owner_roots: Vec::new(),
        }
    }
    fn error(input: SourceInput, control: &RequestControl) -> SourceError {
        match resolve_source(input, None, control) {
            Ok(_) => panic!("source unexpectedly accepted"),
            Err(error) => error,
        }
    }

    #[test]
    fn unnamed_text_input_guards_expose_no_invented_file_identity() {
        // Guards the internal parser root escaping through public input guards;
        // content identity and expected-revision checks still use private facts.
        let snapshot = resolve_source(
            SourceInput::Text { text: MODEL.into() },
            None,
            &RequestControl::default(),
        )
        .unwrap();
        assert_eq!(snapshot.inputs["root"], Value::Null);
        assert_eq!(snapshot.inputs["checked_owners"], json!([]));
        assert!(
            snapshot.inputs["guards"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["file"].is_null())
        );
        assert!(snapshot.guards.iter().any(|guard| guard.file == TEXT_ROOT));
        let repeated = resolve_source(
            SourceInput::Text { text: MODEL.into() },
            Some(&snapshot.revision),
            &RequestControl::default(),
        )
        .unwrap();
        assert_eq!(snapshot.revision, repeated.revision);
    }

    #[test]
    fn incomplete_accepted_commands_and_blocks_have_exact_causes() {
        // Guards incomplete accepted statement records producing an empty
        // structured reason list, and locations pointing at the include owner.
        for (tail, code, name) in [
            ("stoch_simul", "INCOMPLETE_STATEMENT", "stoch_simul"),
            ("priors;", "INCOMPLETE_BLOCK", "priors"),
        ] {
            let text = format!("var y;model;y=1;end;\n{tail}");
            let text_snapshot =
                resolve_source(SourceInput::Text { text }, None, &RequestControl::default())
                    .unwrap();
            assert_eq!(text_snapshot.coverage["complete"], false);
            let reason = text_snapshot.coverage["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .find(|reason| reason["code"] == code)
                .expect("specific incomplete cause");
            assert!(reason["message"].as_str().unwrap().contains(name));
            assert_eq!(reason["source"]["file"], Value::Null);
            assert_eq!(reason["source"]["range"]["line"], 2);
            assert_eq!(
                crate::engine::diagnostics::analyze(&text_snapshot.model)
                    .iter()
                    .any(|row| row.code == "E001"),
                tail == "priors;",
                "The priors alias uses the estimated-parameter reader and reports its incomplete body."
            );
            let map_snapshot = resolve_source(
                mapped(
                    "pkg/root.mod",
                    &[
                        (
                            "pkg/root.mod",
                            "var y;model;y=1;end;\r\n@#include \"child.inc\"\r\n",
                        ),
                        ("pkg\\.\\child.inc", &format!("// source\r\n{tail}")),
                    ],
                ),
                None,
                &RequestControl::default(),
            )
            .unwrap();
            let reason = map_snapshot.coverage["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .find(|reason| reason["code"] == code)
                .unwrap();
            assert_eq!(reason["source"]["file"], "pkg\\.\\child.inc");
            assert_eq!(reason["source"]["range"]["line"], 2);
        }
        let broken = resolve_source(
            SourceInput::Text {
                text: "var y;model;\ny=1\nend;".into(),
            },
            None,
            &RequestControl::default(),
        )
        .unwrap();
        assert!(
            broken.coverage["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reason| reason["code"] == "E001")
        );
    }

    #[test]
    fn cancellation_during_ordinary_parsing_returns_no_snapshot_or_progress() {
        // Guards the source resolver returning partially parsed facts after a
        // cancellation that occurred inside ordinary equation parsing.
        let control = RequestControl::default();
        crate::engine::control::cancel_after(&control.cancelled, 1000);
        let result = error(
            SourceInput::Text {
                text: format!("var y;model;\n{}end;", "y=1;\n".repeat(2000)),
            },
            &control,
        );
        assert_eq!(result.code, "CANCELLED");
        assert_eq!(control.progress.completed.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn reused_analysis_uses_the_next_requests_cancellation() {
        // A completed worker cancels its flag on disposal. That flag must not
        // cancel the next caller, or disconnect it from its own cancellation.
        let input = SourceInput::Text { text: MODEL.into() };
        let old = RequestControl::default();
        let before = resolve_source(input.clone(), None, &old).unwrap();
        old.cancelled.store(true, Ordering::Relaxed);
        let current = RequestControl::default();
        let after = resolve_source_configured_reusing(
            input.clone(),
            None,
            &current,
            None,
            None,
            Some((&before, true)),
        )
        .unwrap();
        assert!(Arc::ptr_eq(&before.model, &after.model));
        assert!(Arc::ptr_eq(&before.report, &after.report));
        assert!(Arc::ptr_eq(
            &after.workspace.cancellation().unwrap(),
            &current.cancelled
        ));
        current.cancelled.store(true, Ordering::Relaxed);
        let Err(error) = resolve_source_configured_reusing(
            input,
            None,
            &current,
            None,
            None,
            Some((&after, true)),
        ) else {
            panic!("a cancelled request must not acquire a snapshot")
        };
        assert_eq!(error.code, "CANCELLED");
    }

    #[test]
    fn reused_disk_analysis_obeys_the_next_requests_source_limits() {
        // A copied Workspace must not restore weaker acquisition ceilings.
        let fixture = DiskFixture::new();
        fixture.write("root.mod", MODEL);
        let input = SourceInput::Disk {
            root: fixture.key("root.mod"),
            include_dirs: vec![],
            owner_roots: vec![],
        };
        let before = resolve_source(input.clone(), None, &RequestControl::default()).unwrap();
        let mut current = RequestControl::default();
        current.limits.max_file_bytes = 1;
        let Err(error) = resolve_source_configured_reusing(
            input,
            None,
            &current,
            None,
            None,
            Some((&before, true)),
        ) else {
            panic!("the next request must enforce its file byte limit")
        };
        assert_eq!(error.code, "SOURCE_LIMIT");
    }

    #[test]
    fn reused_disk_analysis_rejects_changed_include_then_reads_new_facts() {
        // Guards old model facts being returned after a resolved include changes.
        let fixture = DiskFixture::new();
        fixture.write(
            "root.mod",
            "@#include \"child.inc\"\nvar y; model; y=beta; end;",
        );
        fixture.write("child.inc", "var alpha;");
        let input = SourceInput::Disk {
            root: fixture.key("root.mod"),
            include_dirs: vec![],
            owner_roots: vec![],
        };
        let before = resolve_source(input.clone(), None, &RequestControl::default()).unwrap();
        assert!(!declared(&before.model, "beta"));

        fixture.write("child.inc", "parameters beta;");
        let stale = match resolve_source_configured_reusing(
            input.clone(),
            Some(&before.revision),
            &RequestControl::default(),
            None,
            None,
            Some((&before, true)),
        ) {
            Err(error) => error,
            Ok(_) => panic!("changed include facts must not satisfy the old revision"),
        };
        assert_eq!(stale.code, "INPUT_CHANGED");

        let after = resolve_source_configured_reusing(
            input,
            None,
            &RequestControl::default(),
            None,
            None,
            Some((&before, true)),
        )
        .unwrap();
        assert_ne!(before.revision, after.revision);
        assert!(!std::sync::Arc::ptr_eq(&before.model, &after.model));
        assert!(declared(&after.model, "beta"));
    }

    #[test]
    fn reused_disk_analysis_detects_a_newly_present_missing_include() {
        // Guards a recorded missing candidate becoming present without reusing incomplete facts.
        let fixture = DiskFixture::new();
        fixture.write(
            "root.mod",
            "@#include \"child.inc\"\nvar y; model; y=beta; end;",
        );
        let input = SourceInput::Disk {
            root: fixture.key("root.mod"),
            include_dirs: vec![],
            owner_roots: vec![],
        };
        let before = resolve_source(input.clone(), None, &RequestControl::default()).unwrap();
        assert_eq!(before.coverage["complete"], false);
        assert!(before.guards.iter().any(|guard| {
            guard.file == normalize_uri(&fixture.key("child.inc")) && !guard.exists
        }));

        fixture.write("child.inc", "parameters beta;");
        let stale = match resolve_source_configured_reusing(
            input.clone(),
            Some(&before.revision),
            &RequestControl::default(),
            None,
            None,
            Some((&before, true)),
        ) {
            Err(error) => error,
            Ok(_) => panic!("a new include must not satisfy the old revision"),
        };
        assert_eq!(stale.code, "INPUT_CHANGED");

        let after = resolve_source_configured_reusing(
            input,
            None,
            &RequestControl::default(),
            None,
            None,
            Some((&before, true)),
        )
        .unwrap();
        assert_ne!(before.revision, after.revision);
        assert_eq!(after.coverage["complete"], true);
        assert!(declared(&after.model, "beta"));
    }

    #[test]
    fn concurrent_reused_snapshots_keep_cancellation_request_local() {
        // Guards one caller's canceled flag from poisoning another caller sharing model/report Arcs.
        let input = SourceInput::Text { text: MODEL.into() };
        let original = resolve_source(input.clone(), None, &RequestControl::default()).unwrap();
        let left_seed = resolve_source_configured_reusing(
            input.clone(),
            None,
            &RequestControl::default(),
            None,
            None,
            Some((&original, true)),
        )
        .unwrap();
        let right_seed = resolve_source_configured_reusing(
            input.clone(),
            None,
            &RequestControl::default(),
            None,
            None,
            Some((&original, true)),
        )
        .unwrap();
        assert!(std::sync::Arc::ptr_eq(&left_seed.model, &right_seed.model));
        assert!(std::sync::Arc::ptr_eq(
            &left_seed.report,
            &right_seed.report
        ));
        left_seed
            .workspace
            .cancellation()
            .unwrap()
            .store(true, Ordering::Relaxed);

        let live_input = input.clone();
        let cancelled_input = input;
        let (live, cancelled) = std::thread::scope(|scope| {
            let live = scope.spawn(move || {
                resolve_source_configured_reusing(
                    live_input,
                    None,
                    &RequestControl::default(),
                    None,
                    None,
                    Some((&left_seed, true)),
                )
            });
            let cancelled = scope.spawn(move || {
                let control = RequestControl::default();
                control.cancelled.store(true, Ordering::Relaxed);
                resolve_source_configured_reusing(
                    cancelled_input,
                    None,
                    &control,
                    None,
                    None,
                    Some((&right_seed, true)),
                )
            });
            (live.join().unwrap(), cancelled.join().unwrap())
        });

        let live = live.expect("a sibling request must survive another request's cancellation");
        assert!(std::sync::Arc::ptr_eq(&original.model, &live.model));
        assert!(std::sync::Arc::ptr_eq(&original.report, &live.report));
        let Err(cancelled) = cancelled else {
            panic!("the canceled request must not acquire a snapshot")
        };
        assert_eq!(cancelled.code, "CANCELLED");
    }

    #[test]
    fn reused_disk_analysis_respects_changed_search_paths_and_work_limit() {
        // Guards same-root requests from sharing facts across include paths or macro allowances.
        let fixture = DiskFixture::new();
        std::fs::create_dir(fixture.0.join("first")).unwrap();
        std::fs::create_dir(fixture.0.join("second")).unwrap();
        fixture.write(
            "root.mod",
            "@#include \"child.inc\"\nvar y; model; y=1; end;",
        );
        fixture.write("first/child.inc", "parameters beta;");
        fixture.write("second/child.inc", "parameters gamma;");
        let root = fixture.key("root.mod");
        let first_path = fixture.key("first");
        let first_input = SourceInput::Disk {
            root: root.clone(),
            include_dirs: vec![first_path],
            owner_roots: vec![],
        };
        let before = resolve_source(first_input, None, &RequestControl::default()).unwrap();
        assert!(declared(&before.model, "beta"));

        let second_input = SourceInput::Disk {
            root,
            include_dirs: vec![fixture.key("second")],
            owner_roots: vec![],
        };
        let after_path_change = resolve_source_configured_reusing(
            second_input.clone(),
            None,
            &RequestControl::default(),
            None,
            None,
            Some((&before, false)),
        )
        .unwrap();
        assert_ne!(before.revision, after_path_change.revision);
        assert!(!std::sync::Arc::ptr_eq(
            &before.model,
            &after_path_change.model
        ));
        assert!(!declared(&after_path_change.model, "beta"));
        assert!(declared(&after_path_change.model, "gamma"));

        let different_limit = RequestControl {
            macro_work_limit: crate::engine::macro_work::MacroWorkLimit::new(
                (crate::engine::macro_work::MacroWorkLimit::DEFAULT + 1) as u64,
            )
            .unwrap(),
            ..Default::default()
        };
        let after_limit_change = resolve_source_configured_reusing(
            second_input,
            None,
            &different_limit,
            None,
            None,
            Some((&after_path_change, true)),
        )
        .unwrap();
        assert_ne!(after_path_change.revision, after_limit_change.revision);
        assert!(!std::sync::Arc::ptr_eq(
            &after_path_change.model,
            &after_limit_change.model
        ));
    }

    struct DiskFixture(PathBuf);
    impl DiskFixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(1);
            let path = std::env::temp_dir().join(format!(
                "dygnosis-source-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn key(&self, name: &str) -> String {
            self.0.join(name).to_string_lossy().into_owned()
        }
        fn write(&self, name: &str, text: &str) {
            std::fs::write(self.0.join(name), text).unwrap();
        }
    }
    impl Drop for DiskFixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn map_includes_and_companions_never_consult_existing_disk_files() {
        // Guards omitted disk include content and companion existence entering a map result.
        let fixture = DiskFixture::new();
        let root = fixture.key("root.mod");
        let text = "@#include \"child.inc\"\nvar x; model; x=0; end;";
        let before = resolve_source(
            mapped(&root, &[(&root, text)]),
            None,
            &RequestControl::default(),
        )
        .unwrap();
        fixture.write("child.inc", "var disk_only;");
        fixture.write("root_steadystate.m", "function disk_only; end");
        let mut after = resolve_source(
            mapped(&root, &[(&root, text)]),
            Some(&before.revision),
            &RequestControl::default(),
        )
        .unwrap();
        assert_eq!(after.revision, before.revision);
        assert!(!declared(&after.model, "disk_only"));
        assert_eq!(after.sources.len(), 1);
        assert!(
            after
                .workspace
                .companion_records(&root)
                .unwrap()
                .iter()
                .all(|record| record.path.is_none())
        );
        assert!(!after.coverage["complete"].as_bool().unwrap());
    }

    #[test]
    fn lexical_lookup_preserves_exact_written_keys_and_search_order() {
        // Guards host case folding, suffix fallback, ignored configured folders and rewritten source keys.
        let root = "Models\\./Root.mod";
        let child = "Models\\parts\\../parts/child.inc";
        let mut input = mapped(
            root,
            &[
                (
                    root,
                    "@#include \"parts/../parts/child.inc\"\n@#include \"extra.inc\"\nmodel; x=0; end;",
                ),
                (child, "var x;"),
                ("Search\\./extra.inc", "parameters beta;"),
                ("Elsewhere/extra.inc", "var wrong;"),
            ],
        );
        if let SourceInput::Map { include_dirs, .. } = &mut input {
            include_dirs.push("Search/".into());
        }
        let snapshot = resolve_source(input, None, &RequestControl::default()).unwrap();
        assert_eq!(snapshot.root, root);
        assert!(snapshot.sources.contains_key(child));
        assert!(snapshot.sources.contains_key("Search\\./extra.inc"));
        assert_eq!(
            snapshot.raw,
            "@#include \"parts/../parts/child.inc\"\n@#include \"extra.inc\"\nmodel; x=0; end;"
        );
        assert!(declared(&snapshot.model, "beta"));
        assert!(!declared(&snapshot.model, "wrong"));
        assert!(
            snapshot
                .report
                .model_map
                .declarations
                .iter()
                .flat_map(|source| &source.segments)
                .any(|segment| segment.file.as_deref() == Some(child))
        );
        let suffix = resolve_source(
            mapped(
                "Models/Root.mod",
                &[
                    ("Models/Root.mod", "@#include \"child.inc\"\n"),
                    ("Other/child.inc", "var x;"),
                ],
            ),
            None,
            &RequestControl::default(),
        )
        .unwrap();
        assert!(!suffix.coverage["includes_complete"].as_bool().unwrap());
        let case = resolve_source(
            mapped(
                "Models/Root.mod",
                &[
                    ("Models/Root.mod", "@#include \"child.inc\"\n"),
                    ("Models/Child.inc", "var x;"),
                ],
            ),
            None,
            &RequestControl::default(),
        )
        .unwrap();
        assert!(!case.coverage["includes_complete"].as_bool().unwrap());
    }

    #[test]
    fn absolute_supplied_root_keeps_its_root_directory_during_include_lookup() {
        // Guards `/root.mod` looking up `child.inc` as a relative supplied key,
        // or constructing a UNC-style `//child.inc` when its parent is `/`.
        let snapshot = resolve_source(
            mapped(
                "/root.mod",
                &[
                    ("/root.mod", "@#include \"child.inc\"\nvar y;model;y=0;end;"),
                    ("/child.inc", "parameters from_root;"),
                    ("child.inc", "parameters relative_only;"),
                ],
            ),
            None,
            &RequestControl::default(),
        )
        .unwrap();
        assert_eq!(snapshot.coverage["complete"], true);
        assert!(declared(&snapshot.model, "from_root"));
        assert!(!declared(&snapshot.model, "relative_only"));
        assert!(snapshot.sources.contains_key("/child.inc"));
    }

    #[test]
    fn companion_merge_preserves_supplied_case_and_exact_source_keys() {
        // Guards host case folding or canonicalization collapsing distinct
        // supplied companions or distinct unresolved names in one root.
        let text = format!(
            "{MODEL}\ndata(file='A.csv');\ndata(file='a.csv');\ndata(file='Missing.csv');\ndata(file='missing.csv');"
        );
        let mut snapshot = resolve_source(
            mapped(
                "models/root.mod",
                &[
                    ("models/root.mod", &text),
                    ("models\\A.csv", "y\n1\n"),
                    ("models/./a.csv", "y\n2\n"),
                ],
            ),
            None,
            &RequestControl::default(),
        )
        .unwrap();
        let records = snapshot
            .workspace
            .companion_records(&snapshot.root)
            .unwrap();
        for (name, key) in [("A.csv", "models\\A.csv"), ("a.csv", "models/./a.csv")] {
            let row = records
                .iter()
                .find(|row| row.name == name)
                .expect("distinct supplied companion");
            assert_eq!(row.path.as_ref().unwrap().to_str().unwrap(), key);
        }
        for name in ["Missing.csv", "missing.csv"] {
            assert!(
                records
                    .iter()
                    .any(|row| row.name == name && row.path.is_none())
            );
        }
    }

    #[test]
    fn actual_macro_work_stop_has_a_typed_limit_but_unsupported_i211_does_not() {
        // Guards a real nested-loop ceiling being reported only as generic
        // incomplete analysis, and classifying every I211 as a budget stop.
        let text = "var y;\nmodel;\n@#for i in 1:1000\n@#for j in 1:1000\ny=0;\n@#endfor\n@#endfor\nend;\n";
        let root = "Models\\.\\root.mod";
        let stopped = resolve_source(
            mapped(root, &[(root, text)]),
            None,
            &RequestControl::default(),
        )
        .unwrap();
        assert_eq!(stopped.coverage["complete"], false);
        let limit = stopped.coverage["work_limits"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["limit"] == "iteration work")
            .expect("actual work receipt");
        assert_eq!(limit["code"], "MACRO_LIMIT");
        assert_eq!(
            limit["ceiling"],
            crate::engine::language::macro_expr::MACRO_WORK_CAP
        );
        assert_eq!(limit["root"], root);
        assert!(!limit["omitted"].as_str().unwrap().is_empty());
        assert!(
            stopped
                .model
                .incomplete_reasons
                .iter()
                .any(|reason| reason.limit_name == Some("iteration work"))
        );
        let unsupported = resolve_source(
            SourceInput::Text {
                text: format!("@#include \"unavailable.inc\"\n{MODEL}"),
            },
            None,
            &RequestControl::default(),
        )
        .unwrap();
        assert!(
            crate::engine::diagnostics::analyze(&unsupported.model)
                .iter()
                .any(|reason| reason.code == "I211")
        );
        assert!(
            unsupported
                .model
                .incomplete_reasons
                .iter()
                .all(|reason| reason.limit_name.is_none())
        );
        assert_eq!(unsupported.coverage["work_limits"], json!([]));
    }

    #[test]
    fn macro_precision_stop_does_not_invent_an_element_ceiling() {
        // Guards a non-advancing floating-point range being mislabeled as the
        // 10,000-element capacity stop, although it stops at its first value.
        let snapshot = resolve_source(
            SourceInput::Text {
                text: format!("@#define values=10000000000000000:1:10000000000000002\n{MODEL}"),
            },
            None,
            &RequestControl::default(),
        )
        .unwrap();
        let limit = snapshot.coverage["work_limits"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["limit"] == "range size")
            .expect("actual precision stop");
        assert!(limit["ceiling"].is_null());
        assert!(limit["unit"].is_null());
        assert_eq!(snapshot.coverage["complete"], false);
    }

    #[test]
    fn collisions_missing_exact_roots_and_conflicting_source_arguments_are_input_errors() {
        // Guards silent overwrite by separator/dot/dotdot aliases and synthetic missing extraction roots.
        for alias in ["dir\\root.mod", "dir/./root.mod", "dir/child/../root.mod"] {
            assert!(
                error(
                    mapped("dir/root.mod", &[("dir/root.mod", MODEL), (alias, MODEL)]),
                    &RequestControl::default()
                )
                .invalid_arguments
            );
        }
        assert!(
            error(
                mapped("root.mod", &[("./root.mod", MODEL)]),
                &RequestControl::default()
            )
            .invalid_arguments
        );
        let args = SourceArgs {
            source: Some(SourceInput::Text { text: MODEL.into() }),
            file_content: Some(MODEL.into()),
            ..Default::default()
        };
        assert!(args.input().unwrap_err().invalid_arguments);
        let files = HashMap::from([("root.mod".into(), MODEL.into())]);
        assert!(matches!(
            resolve_legacy(None, Some("root.mod"), Some(&files)).unwrap(),
            SourceInput::Map {
                root_text: None,
                ..
            }
        ));
        assert!(matches!(
            resolve_legacy(Some(MODEL), None, Some(&HashMap::new())).unwrap(),
            SourceInput::Text { .. }
        ));
    }

    #[test]
    fn revision_checks_includes_settings_owners_missing_candidates_and_loader_sources() {
        // Guards a stale proposal surviving changes in unchanged dependencies or lookup policy.
        let root = "root.mod";
        let base = mapped(
            root,
            &[
                (root, "@#include \"child.inc\"\nmodel; x=0; end;"),
                ("child.inc", "var x;"),
            ],
        );
        let snapshot = resolve_source(base.clone(), None, &RequestControl::default()).unwrap();
        assert_eq!(snapshot.revision.len(), 64);
        let mut variants = Vec::new();
        let mut changed = base.clone();
        if let SourceInput::Map { files, .. } = &mut changed {
            files.insert("child.inc".into(), "var x; parameters beta;".into());
        }
        variants.push(changed);
        let mut settings = base.clone();
        if let SourceInput::Map { include_dirs, .. } = &mut settings {
            include_dirs.push("extra".into());
        }
        variants.push(settings);
        let mut owners = base;
        if let SourceInput::Map {
            files, owner_roots, ..
        } = &mut owners
        {
            files.insert("other.mod".into(), MODEL.into());
            owner_roots.push("other.mod".into());
        }
        variants.push(owners);
        for variant in variants {
            assert_eq!(
                resolve_source(
                    variant,
                    Some(&snapshot.revision),
                    &RequestControl::default()
                )
                .err()
                .unwrap()
                .code,
                "INPUT_CHANGED"
            );
        }
        let missing = mapped(root, &[(root, "@#include \"missing.inc\"\n")]);
        let snapshot = resolve_source(missing.clone(), None, &RequestControl::default()).unwrap();
        let mut appeared = missing;
        if let SourceInput::Map { files, .. } = &mut appeared {
            files.insert("missing.inc".into(), MODEL.into());
        }
        assert_eq!(
            resolve_source(
                appeared,
                Some(&snapshot.revision),
                &RequestControl::default()
            )
            .err()
            .unwrap()
            .code,
            "INPUT_CHANGED"
        );
        let mut loader = mapped(
            root,
            &[
                (
                    root,
                    "parameters beta; load_params_and_steady_state('values.inc'); var x; model; x=beta; end;",
                ),
                ("values.inc", "beta = 1;"),
            ],
        );
        let snapshot = resolve_source(loader.clone(), None, &RequestControl::default()).unwrap();
        assert!(
            snapshot
                .guards
                .iter()
                .any(|guard| guard.file == "values.inc")
        );
        if let SourceInput::Map { files, .. } = &mut loader {
            files.insert("values.inc".into(), "beta = 2;".into());
        }
        assert_eq!(
            resolve_source(loader, Some(&snapshot.revision), &RequestControl::default())
                .err()
                .unwrap()
                .code,
            "INPUT_CHANGED"
        );
    }

    #[test]
    fn only_explicit_roots_are_checked_and_empty_root_overlay_is_kept() {
        // Guards treating every .mod supplied key as a Rename owner and discarding an empty overlay.
        let mut input = mapped(
            "root.mod",
            &[
                ("root.mod", MODEL),
                ("unrelated.mod", "@#include \"absent.inc\"\n"),
            ],
        );
        let snapshot = resolve_source(input.clone(), None, &RequestControl::default()).unwrap();
        assert_eq!(snapshot.owner_roots, vec!["root.mod"]);
        assert_eq!(snapshot.analyses.len(), 1);
        assert!(snapshot.coverage["complete"].as_bool().unwrap());
        if let SourceInput::Map { root_text, .. } = &mut input {
            *root_text = Some(String::new());
        }
        assert_eq!(
            resolve_source(input, None, &RequestControl::default())
                .unwrap()
                .raw,
            ""
        );
    }

    #[test]
    fn cached_ranges_use_scalar_columns_and_captured_original_source() {
        // Guards supplementary Unicode/CRLF mapping and a range invented for uncaptured or split UTF-8 source.
        let text = "// 😀\r\nvar x;\r\nmodel; x=0; end;";
        let snapshot = resolve_source(
            SourceInput::Text { text: text.into() },
            None,
            &RequestControl::default(),
        )
        .unwrap();
        let x = text.find("x;").unwrap();
        assert_eq!(
            snapshot.range(&snapshot.root, Span::new(x, x + 1)).unwrap(),
            json!({"line":2,"column":5,"end_line":2,"end_column":6})
        );
        let emoji = text.find('😀').unwrap();
        assert!(
            snapshot
                .range(&snapshot.root, Span::new(emoji + 1, emoji + 2))
                .is_none()
        );
        assert!(snapshot.range("uncaptured.inc", Span::new(0, 0)).is_none());
        assert_eq!(
            snapshot.guards[0].text_sha256.as_deref(),
            Some(sha256(text).as_str())
        );
    }

    #[test]
    fn saved_inputs_guard_unedited_sources_and_new_missing_candidates() {
        // Guards disk dependency edits or newly created candidates leaving a returned revision valid.
        let fixture = DiskFixture::new();
        fixture.write("root.mod", "@#include \"child.inc\"\nmodel; x=0; end;");
        fixture.write("child.inc", "var x;");
        let input = SourceInput::Disk {
            root: fixture.key("root.mod"),
            include_dirs: Vec::new(),
            owner_roots: Vec::new(),
        };
        let snapshot = resolve_source(input.clone(), None, &RequestControl::default()).unwrap();
        fixture.write("child.inc", "var x; parameters beta;");
        assert_eq!(
            resolve_source(
                input.clone(),
                Some(&snapshot.revision),
                &RequestControl::default()
            )
            .err()
            .unwrap()
            .code,
            "INPUT_CHANGED"
        );
        let snapshot = resolve_source(input.clone(), None, &RequestControl::default()).unwrap();
        fixture.write("root_steadystate.m", "function out=steadystate; out=0; end");
        assert_eq!(
            resolve_source(input, Some(&snapshot.revision), &RequestControl::default())
                .err()
                .unwrap()
                .code,
            "INPUT_CHANGED"
        );
    }

    #[test]
    fn ceilings_and_cancellation_stop_acquisition_before_analysis() {
        // Guards unbounded supplied allocations, candidate capture, owner work and ignored cancellation.
        let control = RequestControl {
            limits: SourceLimits {
                max_file_bytes: MODEL.len(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(resolve_source(SourceInput::Text { text: MODEL.into() }, None, &control).is_ok());
        assert_eq!(
            error(
                SourceInput::Text {
                    text: format!("{MODEL} ")
                },
                &control
            )
            .code,
            "SOURCE_LIMIT"
        );
        let control = RequestControl {
            limits: SourceLimits {
                max_source_bytes: MODEL.len(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            error(
                mapped("root.mod", &[("root.mod", MODEL), ("extra.inc", " ")]),
                &control
            )
            .code,
            "SOURCE_LIMIT"
        );
        let control = RequestControl {
            limits: SourceLimits {
                max_files: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(resolve_source(SourceInput::Text { text: MODEL.into() }, None, &control).is_ok());
        assert_eq!(
            error(
                mapped("root.mod", &[("root.mod", MODEL), ("extra.inc", "")]),
                &control
            )
            .code,
            "SOURCE_LIMIT"
        );
        let control = RequestControl {
            limits: SourceLimits {
                max_roots: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut input = mapped("root.mod", &[("root.mod", MODEL), ("owner.mod", MODEL)]);
        if let SourceInput::Map { owner_roots, .. } = &mut input {
            owner_roots.push("owner.mod".into());
        }
        assert_eq!(error(input, &control).code, "ROOT_LIMIT");
        control.cancelled.store(true, Ordering::Relaxed);
        assert_eq!(
            error(SourceInput::Text { text: MODEL.into() }, &control).code,
            "CANCELLED"
        );
    }

    #[test]
    fn recorded_default_ceilings_accept_the_boundary_and_refuse_the_next_input() {
        // Guards each advertised acquisition ceiling at its real value. Macro
        // work limits can still make an accepted byte-bound input incomplete.
        let control = RequestControl::default();
        let maximum = " ".repeat(control.limits.max_file_bytes);
        assert!(
            resolve_source(
                SourceInput::Text {
                    text: maximum.clone()
                },
                None,
                &control
            )
            .is_ok()
        );
        assert_eq!(
            error(
                SourceInput::Text {
                    text: format!("{maximum} ")
                },
                &control
            )
            .code,
            "SOURCE_LIMIT"
        );
        let mut files = BTreeMap::new();
        let mut remaining = control.limits.max_source_bytes;
        let mut index = 0;
        while remaining > 0 {
            let size = remaining.min(control.limits.max_file_bytes);
            files.insert(format!("source{index}.mod"), " ".repeat(size));
            remaining -= size;
            index += 1;
        }
        let input = SourceInput::Map {
            root: "source0.mod".into(),
            files: files.clone(),
            root_text: None,
            include_dirs: Vec::new(),
            owner_roots: Vec::new(),
        };
        assert!(resolve_source(input, None, &control).is_ok());
        files
            .get_mut(&format!("source{}.mod", index - 1))
            .unwrap()
            .push(' ');
        assert_eq!(
            error(
                SourceInput::Map {
                    root: "source0.mod".into(),
                    files,
                    root_text: None,
                    include_dirs: Vec::new(),
                    owner_roots: Vec::new()
                },
                &control
            )
            .code,
            "SOURCE_LIMIT"
        );
        let mut files: BTreeMap<_, _> = (0..control.limits.max_files)
            .map(|index| (format!("file{index}.mod"), String::new()))
            .collect();
        assert!(
            resolve_source(
                SourceInput::Map {
                    root: "file0.mod".into(),
                    files: files.clone(),
                    root_text: None,
                    include_dirs: Vec::new(),
                    owner_roots: Vec::new()
                },
                None,
                &control
            )
            .is_ok()
        );
        files.insert("one_extra.mod".into(), String::new());
        assert_eq!(
            error(
                SourceInput::Map {
                    root: "file0.mod".into(),
                    files,
                    root_text: None,
                    include_dirs: Vec::new(),
                    owner_roots: Vec::new()
                },
                &control
            )
            .code,
            "SOURCE_LIMIT"
        );
        let files: BTreeMap<_, _> = (0..control.limits.max_roots + 1)
            .map(|index| (format!("root{index}.mod"), String::new()))
            .collect();
        let owners: Vec<_> = (1..control.limits.max_roots)
            .map(|index| format!("root{index}.mod"))
            .collect();
        assert!(
            resolve_source(
                SourceInput::Map {
                    root: "root0.mod".into(),
                    files: files.clone(),
                    root_text: None,
                    include_dirs: Vec::new(),
                    owner_roots: owners.clone()
                },
                None,
                &control
            )
            .is_ok()
        );
        let mut owners = owners;
        owners.push(format!("root{}.mod", control.limits.max_roots));
        assert_eq!(
            error(
                SourceInput::Map {
                    root: "root0.mod".into(),
                    files,
                    root_text: None,
                    include_dirs: Vec::new(),
                    owner_roots: owners
                },
                &control
            )
            .code,
            "ROOT_LIMIT"
        );
    }
}
