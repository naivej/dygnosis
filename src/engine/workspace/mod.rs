//! Workspace index for `@#include` resolution, companion records, and the
//! effective model.
//!
//! Overlay (editor/MCP in-memory text) beats disk. Include and companion
//! records are stored here. This module does not emit diagnostic codes.

pub mod check_walk;
pub mod companion;
pub mod include_resolver;
pub mod model_map;

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

static INPUT_GENERATION: AtomicU64 = AtomicU64::new(1);
/// Input provenance keeps this fixed byte bound when macro work is increased.
const INPUT_FILE_BYTE_CAP: usize = 1_000_000;

use self::companion::{CompanionKind, CompanionRecord};
use crate::engine::language::expand::{
    expand_report_from_spliced_controlled, ExpandReport, NavigationSource, SpliceSegment,
};
use crate::engine::language::macro_expand::{IncompleteReason, MacroEvalError};
use crate::engine::language::macro_expr::MacroBudget;
use crate::engine::language::model::Model;
use crate::engine::language::span::Span;
use crate::engine::macro_work::MacroWorkLimit;
use crate::engine::workspace::include_resolver::{
    is_virtual_uri, normalize_separators, normalize_uri, path_key, resolve_companion_path,
    resolve_include_path, resolve_scoped_include_path, uri_to_path,
};

/// Resolved `@#include` in the file that contains the directive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedInclude {
    pub filename: String,
    pub span: Span,
    pub path: PathBuf,
}

/// Unresolved `@#include`. Nested misses use the original-file (root) span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnresolvedInclude {
    /// Literal nested target (not decorated).
    pub filename: String,
    pub span: Span,
    /// Basename of the nested file that named this target. `None` on a root miss.
    pub included_from: Option<String>,
    /// Directories that were searched for this include.
    pub searched: Vec<String>,
    /// Actual written site and executed caller chain, separate from the root anchor.
    pub site: IncludeSite,
    pub chain: Vec<IncludeSite>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncludeSite {
    pub file: String,
    pub span: Span,
}

/// Include graph results for one root document. No diagnostic codes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IncludeRecords {
    pub resolved: Vec<ResolvedInclude>,
    pub unresolved: Vec<UnresolvedInclude>,
}

struct Doc {
    source: Arc<String>,
    model: OnceLock<Arc<Model>>,
    work_limit: MacroWorkLimit,
    overlay: bool,
    input_generation: u64,
}

impl Doc {
    /// An acquired include needs only its source for execution in its parent.
    /// Parse it independently only when a standalone document query needs it.
    fn model(&self, cancellation: Option<Arc<AtomicBool>>) -> Option<&Arc<Model>> {
        let mut budget = MacroBudget::with_limit(self.work_limit, cancellation);
        let flag = budget.cancellation();
        if crate::engine::control::flag_cancelled(flag.as_deref()) {
            return None;
        }
        if let Some(model) = self.model.get() {
            return Some(model);
        }
        let model = crate::engine::language::parser::parse_with_budget(&self.source, &mut budget);
        // A canceled parser can retain unfinished facts without a macro reason.
        // Shared document caches must publish only an uninterrupted parse.
        if crate::engine::control::flag_cancelled(flag.as_deref()) {
            return None;
        }
        let _ = self.model.set(Arc::new(model));
        self.model.get()
    }

    fn with_work_limit(&self, work_limit: MacroWorkLimit) -> Self {
        Self {
            source: Arc::clone(&self.source),
            model: OnceLock::new(),
            work_limit,
            overlay: self.overlay,
            input_generation: self.input_generation,
        }
    }
}

/// One joined include source and its written-file map. The model, expand view,
/// and diagnostic locations all read this same snapshot until invalidation.
struct SplicedSource {
    text: String,
    segments: Vec<SpliceSegment>,
    /// Leftover include-directive indent / line endings for source layout.
    gaps: Vec<crate::engine::language::macro_expand::SourceLayoutGap>,
    includes_complete: bool,
    navigation: NavigationSource,
    include_search: Vec<PathBuf>,
    /// A bound reached before joined text could be allocated or scanned.
    limit: Option<IncompleteReason>,
    /// Written cause before include splicing, which may stop before this file is joined.
    limit_site: Option<IncludeSite>,
    stop_chain: Vec<IncludeSite>,
    /// First refusal in a separately read macro file, before joined parsing.
    refusal: Option<crate::engine::language::macro_expand::MacroFileRefusal>,
    /// Actual text and debug messages emitted before an incomplete execution.
    emitted_prefix: Option<String>,
    macro_messages: Vec<crate::engine::language::macro_expand::MacroMessage>,
}

type SplicedPieces = (
    String,
    Vec<SpliceSegment>,
    Vec<crate::engine::language::macro_expand::SourceLayoutGap>,
);

#[derive(Clone, Debug)]
struct IncludeHit {
    path: String,
    resolved: bool,
    iterations: Vec<(Span, usize)>,
    call: usize,
    parent_calls: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct IncludeIteration {
    frame: usize,
    input: String,
}

/// Executed resolutions of one written include directive.
#[derive(Clone, Debug, Default)]
struct SitePlan {
    hits: Vec<IncludeHit>,
    /// Missing or not certain. The directive stays in the spliced text.
    unresolved: bool,
}

#[derive(Default)]
struct IncludeTargets {
    sites: HashMap<(String, Span), SitePlan>,
    loop_occurrences: Vec<crate::engine::language::macro_expand::MacroLoopOccurrence>,
    /// First failed splice occurrence, retained while errors unwind to the root.
    splice_failure: RefCell<Option<(IncludeSite, Option<usize>)>>,
}

fn note_unresolved(targets: &mut IncludeTargets, site: (String, Span)) {
    targets.sites.entry(site).or_default().unresolved = true;
}

fn note_resolved(
    targets: &mut IncludeTargets,
    site: (String, Span),
    path: String,
    event: &crate::engine::language::macro_expand::MacroFileEvent<'_>,
    resolved: bool,
) {
    let plan = targets.sites.entry(site).or_default();
    plan.hits.push(IncludeHit {
        path,
        resolved,
        iterations: event.iterations.to_vec(),
        call: event.call,
        parent_calls: event.parent_calls.to_vec(),
    });
}

/// End of the earliest missing include. A taken miss stops the root,
/// including a miss inside `@#if` or `@#for`.
fn fatal_include_cut(records: &IncludeRecords) -> Option<u32> {
    records.unresolved.iter().map(|item| item.span.end).min()
}

struct IncludeWalk {
    records: IncludeRecords,
    proof: crate::engine::language::macro_expand::MacroFileProof,
    targets: IncludeTargets,
    search: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct InputFile {
    overlay: bool,
    bytes: Option<Vec<u8>>,
    exists: bool,
    directory: bool,
    identity: Option<String>,
    length: Option<u64>,
    modified: Option<std::time::SystemTime>,
    overlay_generation: Option<u64>,
}

/// Checked source and lookup fact. Missing candidates and directories have no
/// text digest; regular source files have a digest of their original UTF-8 text.
#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct InputGuard {
    pub file: String,
    pub exists: bool,
    pub directory: bool,
    pub identity: Option<String>,
    pub text_sha256: Option<String>,
    pub byte_sha256: Option<String>,
    pub length: Option<u64>,
}

/// Compact provenance retained by project reports after parsed caches expire.
#[derive(Clone, Default)]
pub(crate) struct InputStamps {
    files: BTreeMap<String, u64>,
    root: String,
    work_limit: MacroWorkLimit,
}

impl InputStamps {
    pub(crate) fn new() -> Self {
        Self::default()
    }
    pub(crate) fn keys(&self) -> impl Iterator<Item = &String> {
        self.files.keys()
    }
    pub(crate) fn into_keys(self) -> impl Iterator<Item = String> {
        self.files.into_keys()
    }
}

/// URI/path-keyed document index plus include graph walks.
#[derive(Default)]
pub struct Workspace {
    docs: HashMap<String, Arc<Doc>>,
    search_paths: Vec<PathBuf>,
    root_search_paths: HashMap<String, Vec<PathBuf>>,
    default_macro_work_limit: MacroWorkLimit,
    root_macro_work_limits: HashMap<String, MacroWorkLimit>,
    virtual_roots: HashSet<String>,
    unnamed_roots: HashSet<String>,
    // Retain input provenance when model/source caches are cleared.
    dependency_candidates: HashMap<String, HashSet<String>>,
    input_snapshots: HashMap<String, BTreeMap<String, InputFile>>,
    include_owners: HashMap<String, HashSet<String>>,
    effective: HashMap<String, Arc<Model>>,
    spliced: HashMap<String, Arc<SplicedSource>>,
    expand: HashMap<String, Arc<ExpandReport>>,
    records: HashMap<String, IncludeRecords>,
    companions: HashMap<String, Vec<CompanionRecord>>,
    /// When set, document text and `@#include` targets come only from overlays.
    overlay_only: bool,
    /// Request cancellation; absent for existing editor and comparison calls.
    cancellation: Option<Arc<AtomicBool>>,
    source_limits: Option<(usize, usize, usize)>,
    acquisition_limit: Option<&'static str>,
    pub(crate) snapshot_lookup:
        Option<crate::engine::comparison::compare_snapshots::SnapshotLookup>,
}

impl Workspace {
    /// Isolated editor snapshot that preserves this root's include settings.
    pub(crate) fn snapshot_with_root_settings(&self, uri: &str) -> Self {
        let key = if self.overlay_only {
            uri.to_owned()
        } else {
            normalize_uri(uri)
        };
        self.snapshot_for_root(uri, self.root_paths(&key).to_vec())
    }

    /// Owned editor snapshot. Parsed overlays and joined source are immutable
    /// and shared; disk IO and root analysis happen on the receiving worker.
    pub(crate) fn snapshot_for_root(&self, uri: &str, search_paths: Vec<PathBuf>) -> Self {
        self.snapshot_for_root_with_limit(uri, search_paths, self.root_macro_work_limit(uri))
    }

    pub(crate) fn snapshot_for_root_with_limit(
        &self,
        uri: &str,
        search_paths: Vec<PathBuf>,
        work_limit: MacroWorkLimit,
    ) -> Self {
        let key = if self.overlay_only {
            uri.to_owned()
        } else {
            normalize_uri(uri)
        };
        let reuse_joined = self.root_paths(&key) == search_paths.as_slice()
            && self.root_macro_work_limit(uri) == work_limit;
        let candidates = self.input_snapshots.get(&key);
        let mut snapshot = Self::with_search_paths(search_paths);
        snapshot.default_macro_work_limit = self.default_macro_work_limit;
        snapshot.root_macro_work_limits = self.root_macro_work_limits.clone();
        snapshot.overlay_only = self.overlay_only;
        snapshot.virtual_roots = self.virtual_roots.clone();
        snapshot.unnamed_roots = self.unnamed_roots.clone();
        snapshot.cancellation = self.cancellation.clone();
        snapshot.source_limits = self.source_limits;
        snapshot.docs = self
            .docs
            .iter()
            .filter(|(path, doc)| {
                doc.overlay
                    || path.as_str() == key
                    || candidates.is_some_and(|files| files.contains_key(*path))
            })
            .map(|(path, doc)| (path.clone(), Arc::clone(doc)))
            .collect();
        if let Some(inputs) = candidates {
            snapshot.input_snapshots.insert(key.clone(), inputs.clone());
        }
        if let Some(source) = self.spliced.get(&key).filter(|_| reuse_joined) {
            snapshot.spliced.insert(key.clone(), Arc::clone(source));
        }
        if let Some(records) = self.records.get(&key) {
            snapshot.records.insert(key.clone(), records.clone());
        }
        if let Some(candidates) = self.dependency_candidates.get(&key) {
            snapshot
                .dependency_candidates
                .insert(key.clone(), candidates.clone());
        }
        snapshot.set_root_search_paths(uri, snapshot.search_paths.clone());
        snapshot.set_root_macro_work_limit(uri, work_limit);
        // set_root_search_paths invalidates the cache when the scope is first
        // registered; restore the immutable joined source after that operation.
        if let Some(source) = self.spliced.get(&key).filter(|_| reuse_joined) {
            snapshot.spliced.insert(key.clone(), Arc::clone(source));
            if let Some(model) = self.effective.get(&key) {
                snapshot.effective.insert(key.clone(), model.clone());
            }
            if let Some(report) = self.expand.get(&key) {
                snapshot.expand.insert(key.clone(), report.clone());
            }
            if let Some(records) = self.records.get(&key) {
                snapshot.records.insert(key, records.clone());
            }
        }
        snapshot
    }

    /// Bound heavyweight editor caches without dropping input provenance.
    /// Open overlay documents are always retained.
    pub(crate) fn retain_analysis_roots(&mut self, roots: &HashSet<String>) {
        self.effective.retain(|root, _| roots.contains(root));
        self.spliced.retain(|root, _| roots.contains(root));
        self.expand.retain(|root, _| roots.contains(root));
        self.records.retain(|root, _| roots.contains(root));
        self.companions.retain(|root, _| roots.contains(root));
        let mut files = roots.clone();
        for root in roots {
            if let Some(inputs) = self.input_snapshots.get(root) {
                files.extend(inputs.keys().cloned());
            }
        }
        self.docs
            .retain(|key, doc| doc.overlay || files.contains(key));
        self.input_snapshots.retain(|root, _| roots.contains(root));
        // These lightweight maps preserve owner/dependency provenance after
        // model and full-byte snapshots are evicted.
        self.root_search_paths
            .retain(|root, _| roots.contains(root));
        self.root_macro_work_limits.retain(|root, _| {
            roots.contains(root) || self.docs.get(root).is_some_and(|doc| doc.overlay)
        });
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn set_cancellation(&mut self, cancellation: Arc<AtomicBool>) {
        self.cancellation = Some(cancellation);
    }

    pub(crate) fn cancellation(&self) -> Option<Arc<AtomicBool>> {
        self.cancellation.clone()
    }

    pub(crate) fn check_cancelled(&self) -> Result<(), crate::engine::control::Cancelled> {
        if self.cancelled() {
            Err(crate::engine::control::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Unnamed supplied text has no identity for file-location checks.
    pub(crate) fn set_unnamed_root(&mut self, root: &str) {
        self.unnamed_roots.insert(root.to_owned());
    }

    /// Bounds apply before root reads, include reads and candidate capture.
    pub(crate) fn set_source_limits(
        &mut self,
        file_bytes: usize,
        total_bytes: usize,
        files: usize,
    ) {
        self.source_limits = Some((file_bytes, total_bytes, files));
    }

    pub(crate) fn acquisition_limit(&self) -> Option<&'static str> {
        self.acquisition_limit
    }

    fn cancelled(&self) -> bool {
        self.cancellation
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
    }

    /// Overlay workspace. Include lookup does not read file bodies from disk.
    ///
    /// Keys are stored as given. This path does not canonicalize, lowercase,
    /// or change the supplied key. Lookup resolves slash, `.` and `..` aliases.
    pub(crate) fn overlay_documents(files: &BTreeMap<String, String>) -> Self {
        Self::overlay_documents_with_control(files, None)
    }

    pub(crate) fn overlay_documents_with_control(
        files: &BTreeMap<String, String>,
        cancellation: Option<Arc<AtomicBool>>,
    ) -> Self {
        Self::overlay_documents_with_work_limit(files, cancellation, MacroWorkLimit::default())
    }

    pub(crate) fn overlay_documents_with_work_limit(
        files: &BTreeMap<String, String>,
        cancellation: Option<Arc<AtomicBool>>,
        work_limit: MacroWorkLimit,
    ) -> Self {
        let mut ws = Self {
            overlay_only: true,
            cancellation,
            default_macro_work_limit: work_limit,
            ..Self::default()
        };
        for (name, content) in files {
            if ws.cancelled() {
                break;
            }
            ws.insert_overlay(name, content.clone());
        }
        ws
    }

    /// Fixed tree lookup. Neither include candidates nor text use disk fallback.
    pub(crate) fn snapshot_documents_with_limit(
        input: &crate::engine::comparison::compare_snapshots::GitSnapshotInput,
        work_limit: MacroWorkLimit,
    ) -> Self {
        let mut workspace = Self {
            overlay_only: true,
            default_macro_work_limit: work_limit,
            search_paths: input.search_paths.iter().map(PathBuf::from).collect(),
            snapshot_lookup: Some(
                crate::engine::comparison::compare_snapshots::SnapshotLookup {
                    repository_path: tower_lsp::lsp_types::Url::parse(&input.repository_uri)
                        .ok()
                        .map(|uri| {
                            if uri.scheme() == "file" {
                                uri_to_path(&input.repository_uri)
                                    .to_string_lossy()
                                    .replace('\\', "/")
                            } else {
                                uri_to_path(&format!("file://{}", uri.path()))
                                    .to_string_lossy()
                                    .replace('\\', "/")
                            }
                        })
                        .unwrap_or_else(|| input.repository_uri.replace('\\', "/")),
                    manifest: input.manifest.clone(),
                    facts: input.sources.clone(),
                    ..Default::default()
                },
            ),
            ..Default::default()
        };
        for (key, fact) in &input.sources {
            if let crate::engine::comparison::compare_snapshots::SourceFact::Text { text } = fact {
                workspace.insert_overlay(
                    key,
                    crate::engine::language::parser::normalize_newlines(text),
                );
            }
        }
        workspace
    }

    pub(crate) fn snapshot_sources(&self, root: &str) -> BTreeMap<String, String> {
        let key = if self.overlay_only {
            root.to_owned()
        } else {
            normalize_uri(root)
        };
        std::iter::once(key.clone())
            .chain(self.records.get(&key).into_iter().flat_map(|records| {
                records
                    .resolved
                    .iter()
                    .map(|record| self.include_key(&record.path))
            }))
            .filter_map(|key| {
                self.docs.get(&key).map(|doc| {
                    (
                        key,
                        crate::engine::language::parser::normalize_newlines(&doc.source),
                    )
                })
            })
            .collect()
    }

    pub(crate) fn snapshot_search_paths(&self, root: &str) -> Vec<String> {
        let key = if self.overlay_only {
            root.to_owned()
        } else {
            normalize_uri(root)
        };
        self.root_paths(&key)
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect()
    }

    fn overlay_generation(&self, key: &str, source: &str) -> u64 {
        self.docs
            .get(key)
            .filter(|doc| doc.source.as_str() == source)
            .map(|doc| doc.input_generation)
            .unwrap_or_else(|| INPUT_GENERATION.fetch_add(1, Ordering::Relaxed))
    }

    /// Store one overlay under `key` exactly. No disk and no path folding.
    fn insert_overlay(&mut self, key: &str, source: String) {
        let input_generation = self.overlay_generation(key, &source);
        let model = crate::engine::language::parser::parse_with_budget(
            &source,
            &mut self.macro_budget(key),
        );
        self.docs.insert(
            key.to_string(),
            Arc::new(Doc {
                source: Arc::new(source),
                model: OnceLock::from(Arc::new(model)),
                work_limit: self.root_macro_work_limit(key),
                overlay: true,
                input_generation,
            }),
        );
        self.effective.clear();
        self.spliced.clear();
        self.expand.clear();
        self.records.clear();
        self.companions.clear();
    }

    pub fn with_search_paths(search_paths: Vec<PathBuf>) -> Self {
        Self {
            search_paths,
            ..Self::default()
        }
    }

    /// In-memory overlay. Beats disk for this key.
    pub fn update_document(&mut self, uri: &str, source: impl Into<String>) {
        let key = if self.overlay_only {
            uri.to_owned()
        } else {
            normalize_uri(uri)
        };
        let source = source.into();
        let input_generation = self.overlay_generation(&key, &source);
        let model = crate::engine::language::parser::parse_with_budget(
            &source,
            &mut self.macro_budget(&key),
        );
        self.docs.insert(
            key.clone(),
            Arc::new(Doc {
                source: Arc::new(source),
                model: OnceLock::from(Arc::new(model)),
                work_limit: self.root_macro_work_limit(&key),
                overlay: true,
                input_generation,
            }),
        );
        // Other roots may have spliced this file.
        self.effective.clear();
        self.spliced.clear();
        self.expand.clear();
        self.records.clear();
        self.companions.clear();
    }

    /// Drop this document so a later load can read disk again.
    ///
    /// Clears every cached effective model, include-record map, and companion
    /// cache: other roots may have spliced this file.
    pub fn remove_document(&mut self, uri: &str) {
        let key = if self.overlay_only {
            uri.to_owned()
        } else {
            normalize_uri(uri)
        };
        self.docs.remove(&key);
        self.effective.clear();
        self.spliced.clear();
        self.expand.clear();
        self.records.clear();
        self.companions.clear();
    }

    /// Read `path` from disk (utf-8, then latin-1) unless an overlay exists.
    pub fn load_from_disk(&mut self, path: &Path) -> Option<&Model> {
        let key = path_key(path);
        if self.docs.get(&key).is_some_and(|d| d.overlay) {
            return self
                .docs
                .get(&key)
                .and_then(|d| d.model(self.cancellation.clone()).map(Arc::as_ref));
        }
        if self.overlay_only {
            return None;
        }
        let source = if let Some((file_limit, total_limit, file_count)) = self.source_limits {
            let metadata = std::fs::metadata(path).ok()?;
            let used: usize = self.docs.values().map(|doc| doc.source.len()).sum();
            if metadata.len() > file_limit as u64
                || metadata.len() > total_limit.saturating_sub(used) as u64
                || self.docs.len() >= file_count
            {
                self.acquisition_limit =
                    Some("Source acquisition limit reached before reading a model file");
                return None;
            }
            let file = std::fs::File::open(path).ok()?;
            let mut bytes = Vec::new();
            file.take(file_limit as u64 + 1)
                .read_to_end(&mut bytes)
                .ok()?;
            if bytes.len() > file_limit {
                self.acquisition_limit = Some("Source file byte limit reached");
                return None;
            }
            let source = String::from_utf8(bytes)
                .unwrap_or_else(|error| error.into_bytes().into_iter().map(char::from).collect());
            if source.len() > file_limit || source.len() > total_limit.saturating_sub(used) {
                self.acquisition_limit = Some("Decoded source byte limit reached");
                return None;
            }
            source
        } else {
            read_text(path)?
        };
        if self.cancelled() {
            return None;
        }
        let model = crate::engine::language::parser::parse_with_budget(
            &source,
            &mut self.macro_budget(&key),
        );
        self.docs.insert(
            key.clone(),
            Arc::new(Doc {
                source: Arc::new(source),
                model: OnceLock::from(Arc::new(model)),
                work_limit: self.root_macro_work_limit(&key),
                overlay: false,
                input_generation: 0,
            }),
        );
        // Other roots may have spliced this file.
        self.effective.clear();
        self.spliced.clear();
        self.expand.clear();
        self.records.clear();
        self.companions.clear();
        self.docs
            .get(&key)
            .and_then(|d| d.model(self.cancellation.clone()).map(Arc::as_ref))
    }

    pub fn add_search_path(&mut self, path: PathBuf) {
        if !self.search_paths.iter().any(|p| p == &path) {
            self.search_paths.push(path);
        }
        self.effective.clear();
        self.spliced.clear();
        self.expand.clear();
        self.records.clear();
        self.companions.clear();
    }

    pub fn set_search_paths(&mut self, paths: Vec<PathBuf>) {
        let mut deduped = Vec::new();
        for path in paths {
            if !deduped.iter().any(|p| p == &path) {
                deduped.push(path);
            }
        }
        self.search_paths = deduped;
        self.effective.clear();
        self.spliced.clear();
        self.expand.clear();
        self.records.clear();
        self.companions.clear();
    }

    /// Set one root's paths without changing any other root or the CLI/MCP defaults.
    pub fn set_root_search_paths(&mut self, uri: &str, paths: Vec<PathBuf>) {
        let key = if self.overlay_only {
            uri.to_owned()
        } else {
            normalize_uri(uri)
        };
        if is_virtual_uri(uri) {
            self.virtual_roots.insert(key.clone());
        }
        let paths = append_unique(&[], &paths);
        if self.root_search_paths.get(&key) == Some(&paths) {
            return;
        }
        self.root_search_paths.insert(key.clone(), paths);
        self.invalidate_root(&key);
    }

    fn invalidate_root(&mut self, key: &str) {
        self.effective.remove(key);
        self.spliced.remove(key);
        self.expand.remove(key);
        self.records.remove(key);
        self.companions.remove(key);
    }

    fn root_paths(&self, key: &str) -> &[PathBuf] {
        self.root_search_paths
            .get(key)
            .map(Vec::as_slice)
            .unwrap_or(&self.search_paths)
    }

    /// Configure a request workspace before any source is parsed.
    pub fn set_default_macro_work_limit(&mut self, work_limit: MacroWorkLimit) {
        if self.default_macro_work_limit == work_limit {
            return;
        }
        self.default_macro_work_limit = work_limit;
        let changed: Vec<_> = self
            .docs
            .iter()
            .filter_map(|(key, doc)| {
                let selected = self.root_macro_work_limit(key);
                (doc.work_limit != selected)
                    .then(|| (key.clone(), Arc::new(doc.with_work_limit(selected))))
            })
            .collect();
        self.docs.extend(changed);
        self.effective.clear();
        self.spliced.clear();
        self.expand.clear();
        self.records.clear();
        self.companions.clear();
    }

    pub fn root_macro_work_limit(&self, uri: &str) -> MacroWorkLimit {
        let key = if self.overlay_only {
            uri.to_owned()
        } else {
            normalize_uri(uri)
        };
        self.root_macro_work_limits
            .get(&key)
            .copied()
            .unwrap_or(self.default_macro_work_limit)
    }

    /// Changing one root's allowance cannot reuse its old standalone parse.
    pub fn set_root_macro_work_limit(&mut self, uri: &str, work_limit: MacroWorkLimit) {
        let key = if self.overlay_only {
            uri.to_owned()
        } else {
            normalize_uri(uri)
        };
        let previous = self.root_macro_work_limit(uri);
        self.root_macro_work_limits.insert(key.clone(), work_limit);
        if previous == work_limit {
            return;
        }
        if let Some(doc) = self.docs.get(&key) {
            self.docs
                .insert(key.clone(), Arc::new(doc.with_work_limit(work_limit)));
        }
        self.invalidate_root(&key);
    }

    fn macro_budget(&self, root: &str) -> MacroBudget {
        MacroBudget::with_limit(self.root_macro_work_limit(root), self.cancellation.clone())
    }

    /// Observe disk changes without replacing editor overlays. Missing search
    /// candidates are inputs too: creating one can change an include's owner.
    pub fn input_revision(&mut self, uri: &str) -> Option<String> {
        let key = if self.overlay_only {
            uri.to_owned()
        } else {
            normalize_uri(uri)
        };
        if let Some(previous) = self.input_snapshots.get(&key).cloned() {
            let changed: Vec<_> = previous
                .iter()
                .filter_map(|(path, state)| {
                    (self.input_file(path) != *state).then_some(path.clone())
                })
                .collect();
            if !changed.is_empty() {
                for path in changed {
                    if self.docs.get(&path).is_some_and(|doc| !doc.overlay) {
                        self.docs.remove(&path);
                    }
                }
                self.invalidate_root(&key);
            }
        }
        let Some(key) = self.ensure_loaded(uri) else {
            self.include_owners.remove(&key);
            return None;
        };
        self.include_records(uri)?;
        self.companion_records(uri)?;
        let resolved_inputs: Vec<_> =
            self.records
                .get(&key)
                .into_iter()
                .flat_map(|records| records.resolved.iter().map(|record| record.path.clone()))
                .chain(
                    self.companions.get(&key).into_iter().flat_map(|records| {
                        records.iter().filter_map(|record| record.path.clone())
                    }),
                )
                .collect();
        let resolved_keys: Vec<_> = resolved_inputs
            .iter()
            .map(|path| self.include_key(path))
            .collect();
        self.dependency_candidates
            .entry(key.clone())
            .or_default()
            .extend(resolved_keys);
        if self.overlay_only && !self.virtual_roots.contains(&key) {
            let loader_names: Vec<_> = self
                .get_effective_model(uri)
                .into_iter()
                .flat_map(|model| model.load_params_file.iter().map(|(name, _)| name.clone()))
                .collect();
            for name in loader_names {
                // Parameter loaders use only the root's sibling directory.
                self.record_candidates(&key, &key, &name, &[]);
            }
        }
        if !self.overlay_only && !self.virtual_roots.contains(&key) {
            // These checks read directory existence and a loader file outside
            // include/companion resolution. They are revision inputs too.
            let paths: Vec<_> = self
                .get_effective_model(uri)
                .into_iter()
                .flat_map(|model| {
                    model.load_params_file.iter().map(|(name, _)| {
                        let path = PathBuf::from(name);
                        if path.is_absolute() {
                            path
                        } else {
                            Path::new(&key).parent().unwrap_or(Path::new("")).join(path)
                        }
                    })
                })
                .collect();
            let directories = self
                .spliced
                .get(&key)
                .into_iter()
                .flat_map(|source| source.include_search.iter());
            self.dependency_candidates
                .entry(key.clone())
                .or_default()
                .extend(paths.iter().chain(directories).map(|path| path_key(path)));
        }
        let mut files = self
            .dependency_candidates
            .get(&key)
            .cloned()
            .unwrap_or_default();
        files.insert(key.clone());
        if let Some((file_limit, total_limit, file_count)) = self.source_limits {
            if files.len() > file_count {
                self.acquisition_limit = Some("Source lookup candidate count limit reached");
                return None;
            }
            let mut bytes = 0usize;
            for path in &files {
                if self.cancelled() {
                    return None;
                }
                let length = if let Some(doc) = self.docs.get(path) {
                    doc.source.len() as u64
                } else if self.overlay_only {
                    0
                } else {
                    std::fs::metadata(path)
                        .ok()
                        .filter(|metadata| metadata.is_file())
                        .map_or(0, |metadata| metadata.len())
                };
                if length > file_limit as u64 || length > total_limit.saturating_sub(bytes) as u64 {
                    self.acquisition_limit =
                        Some("Source byte limit reached before capturing input guards");
                    return None;
                }
                bytes += length as usize;
            }
        }
        let snapshot: BTreeMap<_, _> = files
            .into_iter()
            .map(|path| {
                let state = self.input_file(&path);
                (path, state)
            })
            .collect();
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut hash);
        self.root_paths(&key).hash(&mut hash);
        self.root_macro_work_limit(&key).hash(&mut hash);
        // Revisions describe analysis inputs, not whether identical text came
        // from disk or an editor overlay. Keep that mode in snapshot equality
        // so cache invalidation and source ownership still observe transitions.
        for (path, state) in &snapshot {
            path.hash(&mut hash);
            state.bytes.hash(&mut hash);
            state.exists.hash(&mut hash);
            state.directory.hash(&mut hash);
            state.identity.hash(&mut hash);
            state.length.hash(&mut hash);
            state.modified.hash(&mut hash);
            state.overlay_generation.hash(&mut hash);
        }
        self.input_snapshots.insert(key, snapshot);
        Some(format!("{:016x}", hash.finish()))
    }

    /// Guards use the exact candidates already selected by the engine. Hosts
    /// must not repeat the include lookup algorithm to build a revision.
    pub(crate) fn input_guards(&self, uri: &str) -> Vec<InputGuard> {
        use sha2::{Digest, Sha256};
        let key = if self.overlay_only {
            uri.to_owned()
        } else {
            normalize_uri(uri)
        };
        self.input_snapshots
            .get(&key)
            .into_iter()
            .flat_map(|files| files.iter())
            .map(|(file, state)| {
                let text = state.bytes.as_ref().map(|bytes| {
                    String::from_utf8(bytes.clone())
                        .unwrap_or_else(|_| bytes.iter().map(|byte| char::from(*byte)).collect())
                });
                InputGuard {
                    file: file.clone(),
                    exists: state.exists,
                    directory: state.directory,
                    identity: state.identity.clone(),
                    text_sha256: text.map(|text| format!("{:x}", Sha256::digest(text.as_bytes()))),
                    byte_sha256: state
                        .bytes
                        .as_ref()
                        .map(|bytes| format!("{:x}", Sha256::digest(bytes))),
                    length: state.length,
                }
            })
            .collect()
    }

    /// Exact native candidates in the last input revision, including misses.
    /// Clients watch these paths; they must not repeat include lookup rules.
    pub(crate) fn input_candidate_paths(&self, uri: &str) -> Vec<PathBuf> {
        self.input_snapshots
            .get(&normalize_uri(uri))
            .into_iter()
            .flat_map(|snapshot| snapshot.keys())
            .filter(|key| !is_virtual_uri(key))
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .collect()
    }

    pub(crate) fn input_stamps(&self, uri: &str) -> InputStamps {
        let root = if self.overlay_only {
            uri.to_owned()
        } else {
            normalize_uri(uri)
        };
        let files = self
            .input_snapshots
            .get(&root)
            .into_iter()
            .flat_map(|snapshot| snapshot.iter())
            .map(|(path, state)| {
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                state.hash(&mut hash);
                (path.clone(), hash.finish())
            })
            .collect();
        InputStamps {
            files,
            work_limit: self.root_macro_work_limit(&root),
            root,
        }
    }

    /// This validation reads disk and must run on the background worker.
    pub(crate) fn inputs_match(&self, stamps: &InputStamps) -> bool {
        !stamps.files.is_empty()
            && self.root_macro_work_limit(&stamps.root) == stamps.work_limit
            && stamps.files.iter().all(|(path, expected)| {
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                self.input_file(path).hash(&mut hash);
                hash.finish() == *expected
            })
    }

    fn input_file(&self, key: &str) -> InputFile {
        let identity = if self.overlay_only || is_virtual_uri(key) {
            None
        } else {
            std::fs::canonicalize(key).ok().map(|path| {
                let path = path.to_string_lossy();
                path_key(Path::new(path.strip_prefix(r"\\?\").unwrap_or(&path)))
            })
        };
        if let Some(doc) = self.docs.get(key).filter(|doc| doc.overlay) {
            InputFile {
                overlay: true,
                bytes: (doc.source.len() <= INPUT_FILE_BYTE_CAP)
                    .then(|| doc.source.as_bytes().to_vec()),
                exists: true,
                directory: false,
                identity,
                length: Some(doc.source.len() as u64),
                modified: None,
                overlay_generation: (doc.source.len() > INPUT_FILE_BYTE_CAP)
                    .then_some(doc.input_generation),
            }
        } else {
            if self.overlay_only {
                let prefix = format!("{}/", overlay_key(key).trim_end_matches('/'));
                let directory = self
                    .docs
                    .keys()
                    .any(|file| overlay_key(file).starts_with(&prefix));
                return InputFile {
                    overlay: false,
                    bytes: None,
                    exists: directory,
                    directory,
                    identity: None,
                    length: None,
                    modified: None,
                    overlay_generation: None,
                };
            }
            let metadata = (!self.overlay_only && !is_virtual_uri(key))
                .then(|| std::fs::metadata(key).ok())
                .flatten();
            InputFile {
                overlay: false,
                bytes: if self.overlay_only
                    || is_virtual_uri(key)
                    || metadata
                        .as_ref()
                        .is_some_and(|value| value.len() > INPUT_FILE_BYTE_CAP as u64)
                {
                    None
                } else {
                    read_input_bytes(Path::new(key))
                },
                exists: metadata.is_some(),
                directory: metadata.as_ref().is_some_and(|metadata| metadata.is_dir()),
                identity,
                length: metadata.as_ref().map(std::fs::Metadata::len),
                modified: metadata
                    .as_ref()
                    .filter(|value| value.len() > INPUT_FILE_BYTE_CAP as u64)
                    .and_then(|value| value.modified().ok()),
                overlay_generation: None,
            }
        }
    }

    /// Validate a result against the exact files observed by input_revision.
    /// Also catch a disk write between loading a parsed source and hashing it.
    pub(crate) fn input_snapshot_is_current(&self, uri: &str) -> bool {
        let key = if self.overlay_only {
            uri.to_owned()
        } else {
            normalize_uri(uri)
        };
        let Some(snapshot) = self.input_snapshots.get(&key) else {
            return false;
        };
        snapshot.iter().all(|(key, state)| {
            if self.input_file(key) != *state {
                return false;
            }
            let Some(document) = self.docs.get(key) else {
                return true;
            };
            let Some(bytes) = state.bytes.as_ref() else {
                return document.source.len() > INPUT_FILE_BYTE_CAP
                    && state.length == Some(document.source.len() as u64);
            };
            let decoded = String::from_utf8(bytes.clone())
                .unwrap_or_else(|_| bytes.iter().map(|&byte| byte as char).collect());
            document.source.as_str() == decoded
        })
    }

    /// Known compilation-unit owners from resolved include edges. Retained
    /// across cache invalidation so an editor edit does not lose root context.
    pub fn owner_roots(&self, uri: &str) -> Vec<String> {
        let key = if self.overlay_only {
            uri.to_owned()
        } else {
            normalize_uri(uri)
        };
        let mut roots: Vec<_> = self
            .include_owners
            .iter()
            .filter(|(root, included)| root.as_str() != key && included.contains(&key))
            .map(|(root, _)| root.clone())
            .collect();
        roots.sort();
        roots
    }

    fn record_candidates(
        &mut self,
        root: &str,
        including_key: &str,
        filename: &str,
        paths: &[PathBuf],
    ) {
        if self.virtual_roots.contains(root) {
            return;
        }
        if self.overlay_only {
            let candidates = self.overlay_candidates(including_key, filename, paths);
            let candidates: Vec<_> = candidates
                .into_iter()
                .map(|candidate| self.overlay_match_key(&candidate).unwrap_or(candidate))
                .collect();
            self.dependency_candidates
                .entry(root.to_owned())
                .or_default()
                .extend(candidates);
            return;
        }
        let name = PathBuf::from(normalize_separators(filename));
        let mut candidates = Vec::new();
        if name.is_absolute() {
            candidates.push(name);
        } else {
            if let Some(parent) = Path::new(including_key).parent() {
                candidates.push(parent.join(&name));
            }
            candidates.extend(paths.iter().map(|path| path.join(&name)));
        }
        self.dependency_candidates
            .entry(root.to_owned())
            .or_default()
            .extend(candidates.iter().map(|path| path_key(path)));
    }

    pub fn get_model(&self, uri: &str) -> Option<&Model> {
        let key = if self.overlay_only {
            uri.to_owned()
        } else {
            normalize_uri(uri)
        };
        self.docs
            .get(&key)
            .and_then(|d| d.model(self.cancellation.clone()).map(Arc::as_ref))
    }

    pub fn get_source(&self, uri: &str) -> Option<&str> {
        let key = if self.overlay_only {
            uri.to_string()
        } else {
            normalize_uri(uri)
        };
        self.docs.get(&key).map(|d| d.source.as_str())
    }

    /// Source-map keys are already normalized; no repeat path lookup is needed.
    pub(crate) fn source_for_normalized_key(&self, key: &str) -> Option<&str> {
        self.docs.get(key).map(|document| document.source.as_str())
    }

    /// Whether this workspace represents only caller-supplied map entries.
    pub(crate) fn is_overlay_only(&self) -> bool {
        self.overlay_only
    }

    pub(crate) fn is_virtual_root(&self, uri: &str) -> bool {
        self.virtual_roots.contains(&normalize_uri(uri))
    }

    /// A virtual directory exists when a supplied file has that directory as
    /// a key prefix. This never asks whether the host directory exists.
    pub(crate) fn overlay_directory_exists(&self, including_key: &str, raw: &str) -> bool {
        let directory = overlay_includepath_key(including_key, raw);
        if directory.is_empty() {
            return self.docs.contains_key(including_key);
        }
        let prefix = format!("{}/", directory.trim_end_matches('/'));
        self.docs
            .keys()
            .any(|key| overlay_key(key).starts_with(&prefix))
    }

    /// File content beside a map root, for commands that read a named file.
    pub(crate) fn overlay_beside_source(&self, root_key: &str, name: &str) -> Option<&str> {
        let name = overlay_key(name);
        let key = if overlay_absolute(&name) {
            name
        } else if let Some(parent) = overlay_parent(root_key) {
            overlay_join(&parent, &name)
        } else {
            name
        };
        let supplied = self.overlay_match_key(&key)?;
        self.docs.get(&supplied).map(|doc| doc.source.as_str())
    }

    /// Loaded document keys (normalized paths), for W061 parent lookup.
    pub fn document_uris(&self) -> Vec<String> {
        self.docs.keys().cloned().collect()
    }

    /// Parse of the joined source: executed include bodies replace directives;
    /// required unresolved edges stay empty. Dormant sites remain for
    /// the ordinary macro branch/loop expansion to skip.
    pub fn get_effective_model(&mut self, uri: &str) -> Option<&Model> {
        let key = self.ensure_loaded(uri)?;
        if !self.effective.contains_key(&key) {
            let spliced = self.spliced_source(&key);
            let fatal = self.records.get(&key).and_then(|records| {
                let span = records.unresolved.first().map(|item| item.span)?;
                Some(span)
            });
            let same_root_doc = self.docs.get(&key).filter(|_| {
                let [segment] = spliced.segments.as_slice() else {
                    return false;
                };
                spliced.gaps.is_empty()
                    && segment.file.as_deref() == Some(key.as_str())
                    && segment.spliced == Span::new(0, spliced.text.len())
                    && segment.origin == segment.spliced
                    && segment.line == 1
                    && segment.evaluation.is_none()
            });
            let same_root = if let Some(doc) = same_root_doc {
                let model = doc.model(self.cancellation.clone())?;
                (model.source == spliced.text).then_some(model)
            } else {
                None
            };
            if fatal.is_none()
                && spliced.limit.is_none()
                && spliced.refusal.is_none()
                && let Some(model) = same_root
            {
                self.effective.insert(key.clone(), Arc::clone(model));
                return self.effective.get(&key).map(Arc::as_ref);
            }
            // A one-segment unchanged root already has the same parser input,
            // original lines and macro evaluation context. Reuse its facts.
            let mut model = if let Some(model) = same_root {
                model.as_ref().clone()
            } else {
                let lines: Vec<_> = spliced
                    .segments
                    .iter()
                    .map(|segment| (segment.spliced, segment.line))
                    .collect();
                let evaluations =
                    crate::engine::language::expand::splice_evaluations(&spliced.segments);
                crate::engine::language::parser::parse_with_lines_and_evaluations_budget(
                    &spliced.text,
                    &lines,
                    &evaluations,
                    &mut self.macro_budget(&key),
                )
            };
            if let Some(span) = fatal {
                model.macro_incomplete_span.get_or_insert(span);
            }
            if let Some(reason) = &spliced.limit {
                // The retained prefix is not a complete macro program. Its
                // missing closers and undefined child bindings are not errors
                // in the written file; the resource reason owns this stop.
                model.macro_type_errors.clear();
                model.incomplete_reasons.clear();
                model.macro_incomplete_span.get_or_insert(reason.span);
                model.incomplete_reasons.push(reason.clone());
            }
            if let Some((file, written, code, message)) = &spliced.refusal {
                let span = spliced
                    .segments
                    .iter()
                    .find_map(|segment| {
                        (segment.file.as_deref() == Some(file.as_str())
                            && written.start >= segment.origin.start
                            && written.start <= segment.origin.end)
                            .then(|| {
                                Span::new(
                                    (segment.spliced.start + written.start - segment.origin.start)
                                        as usize,
                                    (segment.spliced.start + written.end.min(segment.origin.end)
                                        - segment.origin.start)
                                        as usize,
                                )
                            })
                    })
                    .unwrap_or_default();
                if !model
                    .macro_type_errors
                    .iter()
                    .any(|(_, existing, text)| existing == code && text == message)
                {
                    model.macro_type_errors = vec![(span, *code, message.clone())];
                }
                model.macro_incomplete_span.get_or_insert(span);
                model.incomplete_reasons.clear();
            }
            self.effective.insert(key.clone(), Arc::new(model));
        }
        self.effective.get(&key).map(Arc::as_ref)
    }

    pub(crate) fn effective_model_snapshot(&mut self, root: &str) -> Option<Arc<Model>> {
        self.get_effective_model(root)?;
        let key = if self.overlay_only {
            root.to_owned()
        } else {
            normalize_uri(root)
        };
        self.effective.get(&key).cloned()
    }

    /// Map one span in the include-spliced model back to the active root file.
    /// Included-file spans have no location in the root and return `None`.
    pub(crate) fn map_effective_span_to_root(&mut self, uri: &str, span: Span) -> Option<Span> {
        let key = self.ensure_loaded(uri)?;
        let spliced = self.spliced_source(&key);
        let segments = &spliced.segments;
        let segment = segments.iter().find(|segment| {
            segment.spliced.start <= span.start
                && span.end <= segment.spliced.end
                && segment.file.as_deref() == Some(key.as_str())
        })?;
        let delta = span.start - segment.spliced.start;
        Some(Span {
            start: segment.origin.start + delta,
            end: segment.origin.start + delta + (span.end - span.start),
        })
    }

    /// Map a span in the include-spliced model to the physical file that owns
    /// its start. `None` when no segment owns that start, or the segment has
    /// no file. Callers must not treat that as the root file.
    pub(crate) fn map_effective_origin(&mut self, uri: &str, span: Span) -> Option<(String, Span)> {
        let key = self.ensure_loaded(uri)?;
        let spliced = self.spliced_source(&key);
        let segments = &spliced.segments;
        let segment = segments
            .iter()
            .find(|s| span.start >= s.spliced.start && span.start < s.spliced.end)
            .or_else(|| {
                segments
                    .last()
                    .filter(|s| span.start == s.spliced.end && s.spliced.start < s.spliced.end)
            })?;
        let file = segment.file.clone()?;
        let delta = span.start.saturating_sub(segment.spliced.start);
        let len = span.end.saturating_sub(span.start);
        Some((
            file,
            Span {
                start: segment.origin.start + delta,
                end: segment.origin.start + delta + len,
            },
        ))
    }

    /// Literal joined-source positions for a written cursor. Repeated include
    /// executions remain separate; generated or resized segments supply no proof.
    pub(crate) fn effective_positions(&mut self, uri: &str, file: &str, byte: u32) -> Vec<u32> {
        let Some(key) = self.ensure_loaded(uri) else {
            return Vec::new();
        };
        let spliced = self.spliced_source(&key);
        let mut positions: Vec<_> = spliced
            .segments
            .iter()
            .filter(|segment| {
                segment.file.as_deref() == Some(file)
                    && segment.spliced.end - segment.spliced.start
                        == segment.origin.end - segment.origin.start
                    && segment.origin.start <= byte
                    && byte <= segment.origin.end
            })
            .map(|segment| segment.spliced.start + byte - segment.origin.start)
            .collect();
        positions.sort_unstable();
        positions.dedup();
        positions
    }

    pub fn expand_report(&mut self, uri: &str) -> Option<&ExpandReport> {
        let key = self.ensure_loaded(uri)?;
        if !self.expand.contains_key(&key) {
            let spliced = self.spliced_source(&key);
            let incomplete = self
                .records
                .get(&key)
                .map(|records| !records.unresolved.is_empty())
                .unwrap_or(false);
            let mut report = expand_report_from_spliced_controlled(
                &spliced.text,
                &spliced.segments,
                Some(&spliced.navigation),
                self.cancellation.clone(),
                self.root_macro_work_limit(&key),
            )
            .ok()?;
            if spliced.limit.is_some() || spliced.refusal.is_some() {
                report.complete = false;
                report.navigation_complete = false;
                report.n_equations = 0;
                report.navigation.clear();
                report.origins.clear();
                report.aggregate_origins.clear();
                report.heterogeneous_origins.clear();
            }
            if incomplete {
                report.complete = false;
                report.n_equations = 0;
            }
            if let Some(prefix) = &spliced.emitted_prefix {
                report.effective_text =
                    crate::engine::language::expand::compact_emitted_prefix(prefix);
                report.macro_messages = spliced.macro_messages.clone();
                report.navigation_complete = false;
                report.navigation.clear();
                report.origins.clear();
            }
            self.expand.insert(key.clone(), Arc::new(report));
        }
        self.expand.get(&key).map(Arc::as_ref)
    }

    pub(crate) fn expand_report_snapshot(&mut self, root: &str) -> Option<Arc<ExpandReport>> {
        self.expand_report(root)?;
        let key = if self.overlay_only {
            root.to_owned()
        } else {
            normalize_uri(root)
        };
        self.expand.get(&key).cloned()
    }

    /// Independently verified written portions of any include-spliced span.
    pub fn map_effective_segments(
        &mut self,
        uri: &str,
        span: Span,
    ) -> Vec<crate::engine::workspace::model_map::WrittenSegment> {
        let Some(key) = self.ensure_loaded(uri) else {
            return Vec::new();
        };
        let spliced = self.spliced_source(&key);
        crate::engine::language::expand::map_written_segments(&spliced.segments, span)
    }

    /// Transitively included files (root excluded).
    pub fn resolve_all_includes(&mut self, uri: &str) -> HashMap<String, Model> {
        let root_key = self.ensure_loaded(uri);
        let records = self.include_records(uri).cloned().unwrap_or_default();
        let mut out = HashMap::new();
        let mut seen = HashSet::new();
        for resolved in &records.resolved {
            let key = self.include_key(&resolved.path);
            if root_key.as_deref() == Some(key.as_str()) || !seen.insert(key.clone()) {
                continue;
            }
            if let Some(model) = self.model_for_key(&key).cloned() {
                out.insert(key, model);
            }
        }
        out
    }

    pub fn find_unresolved_includes(&mut self, uri: &str) -> Vec<UnresolvedInclude> {
        self.include_records(uri)
            .map(|r| r.unresolved.clone())
            .unwrap_or_default()
    }

    /// Executed include records (written spans, resolved targets and misses).
    pub fn include_records(&mut self, uri: &str) -> Option<&IncludeRecords> {
        let key = self.ensure_loaded(uri)?;
        if !self.records.contains_key(&key) {
            self.spliced_source(&key);
        }
        self.records.get(&key)
    }

    /// Whether expansion of executed includes is proven complete.
    pub fn includes_complete(&mut self, uri: &str) -> bool {
        let Some(key) = self.ensure_loaded(uri) else {
            return false;
        };
        self.spliced_source(&key).includes_complete
    }

    /// Companion records for the root `.mod` (convention + named mentions).
    pub fn companion_records(&mut self, uri: &str) -> Option<&[CompanionRecord]> {
        let key = self.ensure_loaded(uri)?;
        if self.unnamed_roots.contains(&key) {
            return Some(&[]);
        }
        if !self.companions.contains_key(&key) {
            let records = self.build_companions(&key);
            self.companions.insert(key.clone(), records);
        }
        self.companions.get(&key).map(|v| v.as_slice())
    }

    fn ensure_loaded(&mut self, uri: &str) -> Option<String> {
        if self.overlay_only {
            return self.docs.contains_key(uri).then(|| uri.to_string());
        }
        let key = normalize_uri(uri);
        if self.docs.contains_key(&key) {
            return Some(key);
        }
        if is_virtual_uri(uri) {
            return None;
        }
        let path = uri_to_path(uri);
        if path.exists() {
            self.load_from_disk(&path)?;
            return Some(path_key(&path));
        }
        None
    }

    fn model_for_key(&mut self, key: &str) -> Option<&Model> {
        if self.docs.contains_key(key) {
            return self
                .docs
                .get(key)
                .and_then(|d| d.model(self.cancellation.clone()).map(Arc::as_ref));
        }
        if self.overlay_only {
            return None;
        }
        let path = PathBuf::from(key);
        if path.exists() {
            self.load_from_disk(&path)?;
        }
        self.docs
            .get(key)
            .and_then(|d| d.model(self.cancellation.clone()).map(Arc::as_ref))
    }

    fn source_for_key(
        &mut self,
        key: &str,
        budget: &mut MacroBudget,
    ) -> Result<Option<Arc<String>>, MacroEvalError> {
        if self.cancelled() {
            return Err(MacroEvalError::Limit("request cancelled"));
        }
        if let Some((file_limit, total_limit, file_count)) = self.source_limits {
            let length = self
                .docs
                .get(key)
                .map(|doc| doc.source.len() as u64)
                .or_else(|| {
                    (!self.overlay_only)
                        .then(|| std::fs::metadata(key).ok().map(|metadata| metadata.len()))
                        .flatten()
                })
                .unwrap_or(0);
            let used: usize = self
                .docs
                .iter()
                .filter(|(file, _)| file.as_str() != key)
                .map(|(_, doc)| doc.source.len())
                .sum();
            if length > file_limit as u64
                || length > total_limit.saturating_sub(used) as u64
                || (!self.docs.contains_key(key) && self.docs.len() >= file_count)
            {
                self.acquisition_limit =
                    Some("Source acquisition limit reached before reading an include");
                return Err(MacroEvalError::Limit("source acquisition"));
            }
        }
        if let Some(doc) = self.docs.get(key) {
            budget.spend_work(1)?;
            return Ok(Some(Arc::clone(&doc.source)));
        }
        if self.overlay_only {
            if let Some(lookup) = &mut self.snapshot_lookup {
                lookup.request(key);
            }
            return Ok(None);
        }
        let path = PathBuf::from(key);
        let Some(source) = read_include_text(&path, budget)? else {
            return Ok(None);
        };
        budget.spend_work(1)?;
        let source = Arc::new(source);
        let returned = Arc::clone(&source);
        self.docs.insert(
            key.to_string(),
            Arc::new(Doc {
                source,
                model: OnceLock::new(),
                work_limit: self.root_macro_work_limit(key),
                overlay: false,
                input_generation: 0,
            }),
        );
        // Loading this written input can invalidate other cached roots.
        self.effective.clear();
        self.spliced.clear();
        self.expand.clear();
        self.records.clear();
        self.companions.clear();
        Ok(Some(returned))
    }

    fn known_keys(&self) -> HashSet<String> {
        self.docs.keys().cloned().collect()
    }

    fn configured_search(&self, root: &str, extra: &[PathBuf]) -> Vec<PathBuf> {
        append_unique(self.root_paths(root), extra)
    }

    fn resolve_filename(
        &mut self,
        root_key: &str,
        including_key: &str,
        filename: &str,
        active_search: &[PathBuf],
    ) -> Option<PathBuf> {
        if self.snapshot_lookup.is_some() {
            let paths = self.configured_search(root_key, active_search);
            return self
                .snapshot_include_key(including_key, filename, &paths)
                .map(PathBuf::from);
        }
        if self.virtual_roots.contains(root_key) {
            return None;
        }
        if self.overlay_only {
            let paths = self.configured_search(root_key, active_search);
            self.record_candidates(root_key, including_key, filename, &paths);
            return self
                .overlay_include_key(including_key, filename, &paths)
                .map(PathBuf::from);
        }
        let paths = self.configured_search(root_key, active_search);
        self.record_candidates(root_key, including_key, filename, &paths);
        let known = self.known_keys();
        if self.root_search_paths.contains_key(root_key) {
            resolve_scoped_include_path(filename, including_key, &paths, Some(&known))
        } else {
            resolve_include_path(filename, including_key, &paths, Some(&known))
        }
    }

    /// Workspace key for a resolved include. Overlay keys stay as stored.
    fn include_key(&self, path: &Path) -> String {
        if self.overlay_only {
            let candidate = overlay_path_key(path);
            self.overlay_match_key(&candidate).unwrap_or(candidate)
        } else {
            path_key(path)
        }
    }

    fn snapshot_include_key(
        &mut self,
        including_key: &str,
        filename: &str,
        paths: &[PathBuf],
    ) -> Option<String> {
        use crate::engine::comparison::compare_snapshots::tree_parent;
        if filename.is_empty() {
            return None;
        }
        let lookup = self.snapshot_lookup.as_mut().expect("snapshot lookup");
        let mut candidates = vec![lookup.path(tree_parent(including_key), filename)];
        candidates.extend(
            paths
                .iter()
                .map(|path| lookup.path(&path.to_string_lossy(), filename)),
        );
        for candidate in candidates {
            let Some(candidate) = candidate else {
                lookup.refuse(
                    filename,
                    "Historical include leaves the selected repository",
                );
                return None;
            };
            if lookup.contains(&candidate) {
                return Some(candidate);
            }
        }
        None
    }

    /// Resolve separator aliases to one supplied key. Ambiguous aliases do
    /// not establish a file identity for a map-only compilation unit.
    fn overlay_match_key(&self, candidate: &str) -> Option<String> {
        let mut matched = None;
        for key in self.docs.keys() {
            if overlay_key(key) == candidate {
                if matched.is_some() {
                    return None;
                }
                matched = Some(key.clone());
            }
        }
        matched
    }

    fn overlay_candidates(
        &self,
        including_key: &str,
        filename: &str,
        active_search: &[PathBuf],
    ) -> Vec<String> {
        let name = overlay_key(filename);
        if name.is_empty() {
            return Vec::new();
        }
        let mut candidates = Vec::new();
        if overlay_absolute(&name) {
            candidates.push(name);
        } else {
            match overlay_parent(including_key) {
                Some(parent) => candidates.push(overlay_join(&parent, &name)),
                None => candidates.push(name.clone()),
            }
            for dir in active_search {
                let candidate = overlay_join(&overlay_path_key(dir), &name);
                if !candidates.contains(&candidate) {
                    candidates.push(candidate);
                }
            }
        }
        candidates
    }

    /// Match the explicit sibling/search-folder candidates to exact supplied keys.
    fn overlay_include_key(
        &self,
        including_key: &str,
        filename: &str,
        search: &[PathBuf],
    ) -> Option<String> {
        self.overlay_candidates(including_key, filename, search)
            .into_iter()
            .find_map(|candidate| self.overlay_match_key(&candidate))
    }

    fn resolve_companion_from_root(
        &mut self,
        root_key: &str,
        name: &str,
        extra_suffixes: &[&str],
    ) -> Option<PathBuf> {
        if self.virtual_roots.contains(root_key) {
            return None;
        }
        let joined = self.spliced_source(root_key);
        if self.overlay_only {
            let paths = self.configured_search(root_key, &joined.include_search);
            self.record_candidates(root_key, root_key, name, &paths);
            if let Some(key) = self.overlay_include_key(root_key, name, &paths) {
                return Some(PathBuf::from(key));
            }
            if Path::new(name).extension().is_some() {
                return None;
            }
            for suffix in extra_suffixes {
                self.record_candidates(root_key, root_key, &format!("{name}{suffix}"), &paths);
                if let Some(key) =
                    self.overlay_include_key(root_key, &format!("{name}{suffix}"), &paths)
                {
                    return Some(PathBuf::from(key));
                }
            }
            return None;
        }
        let mut paths = self.root_paths(root_key).to_vec();
        let directories: Vec<_> = joined
            .include_search
            .iter()
            .filter(|path| path.is_dir())
            .cloned()
            .collect();
        paths = append_unique(&paths, &directories);
        let known = self.known_keys();
        self.record_candidates(root_key, root_key, name, &paths);
        if Path::new(name).extension().is_none() {
            for suffix in extra_suffixes {
                self.record_candidates(root_key, root_key, &format!("{name}{suffix}"), &paths);
            }
        }
        if self.root_search_paths.contains_key(root_key) {
            if let Some(path) = resolve_scoped_include_path(name, root_key, &paths, Some(&known)) {
                return Some(path);
            }
            if Path::new(name).extension().is_none() {
                for suffix in extra_suffixes {
                    if let Some(path) = resolve_scoped_include_path(
                        &format!("{name}{suffix}"),
                        root_key,
                        &paths,
                        Some(&known),
                    ) {
                        return Some(path);
                    }
                }
            }
            return None;
        }
        resolve_companion_path(name, root_key, &paths, Some(&known), extra_suffixes)
    }

    fn build_companions(&mut self, root_key: &str) -> Vec<CompanionRecord> {
        let _ = self.spliced_source(root_key);
        let Some(root) = self.docs.get(root_key).map(|doc| doc.source.clone()) else {
            return Vec::new();
        };
        let cut = self.records.get(root_key).and_then(fatal_include_cut);
        let (source, model) = if let Some(cut) = cut {
            let text = root[..(cut as usize).min(root.len())].to_string();
            let model = Arc::new(crate::engine::language::parser::parse_with_budget(
                &text,
                &mut self.macro_budget(root_key),
            ));
            (text, model)
        } else {
            let Some(doc) = self.docs.get(root_key) else {
                return Vec::new();
            };
            let Some(model) = doc.model(self.cancellation.clone()) else {
                return Vec::new();
            };
            (model.source.clone(), Arc::clone(model))
        };
        let stem = Path::new(root_key)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let convention = [
            (
                CompanionKind::SteadyStateFile,
                format!("{stem}_steadystate.m"),
            ),
            (
                CompanionKind::PriorRestrictions,
                format!("{stem}_prior_restrictions.m"),
            ),
            (CompanionKind::RunScript, format!("run_{stem}.m")),
        ];
        let identity = if self.overlay_only {
            companion::CompanionIdentity::Supplied
        } else {
            companion::CompanionIdentity::Host
        };
        companion::harvest(&source, &model, &convention, identity, |name, extra| {
            self.resolve_companion_from_root(root_key, name, extra)
        })
    }

    /// Resolve files only when the shared macro walker executes their site.
    fn walk_graph(&mut self, root_key: &str) -> IncludeWalk {
        self.dependency_candidates.remove(root_key);
        let mut records = IncludeRecords::default();
        let mut targets = IncludeTargets::default();
        let mut active_search = Vec::new();
        let mut seen_sites = HashSet::new();
        let source = self.docs.get(root_key).expect("loaded root").source.clone();
        let mut proof = crate::engine::language::macro_expand::walk_macro_files_with_budget(
            root_key,
            &source,
            self.macro_budget(root_key),
            |event| {
                if !event.certain {
                    if matches!(
                        event.directive,
                        crate::engine::language::macro_expand::MacroFileDirective::Include(_)
                    ) {
                        note_unresolved(&mut targets, (event.file.to_string(), event.span));
                    }
                    return None;
                }
                if let crate::engine::language::macro_expand::MacroFileDirective::IncludePath(
                    path,
                ) = event.directive
                {
                    if let Some(lookup) = &mut self.snapshot_lookup {
                        let Some(directory) = lookup.path(
                            crate::engine::comparison::compare_snapshots::tree_parent(root_key),
                            path,
                        ) else {
                            lookup.refuse(
                                path,
                                "Historical include folder leaves the selected repository",
                            );
                            return None;
                        };
                        if path.is_empty() || !lookup.directory_exists(&directory) {
                            return None;
                        }
                        active_search = append_unique(&active_search, &[PathBuf::from(directory)]);
                        return Some(crate::engine::language::macro_expand::MacroFileLoad::Path);
                    }
                    let added = if self.overlay_only {
                        PathBuf::from(overlay_includepath_key(root_key, path))
                    } else {
                        resolve_includepath(root_key, path)
                    };
                    if !path.is_empty() {
                        // The directory check reads existence/type even when it
                        // refuses. Creating that directory must invalidate E304.
                        self.dependency_candidates
                            .entry(root_key.to_string())
                            .or_default()
                            .insert(if self.overlay_only {
                                overlay_path_key(&added)
                            } else {
                                path_key(&added)
                            });
                    }
                    let valid = if self.overlay_only {
                        !path.is_empty() && self.overlay_directory_exists(root_key, path)
                    } else {
                        !path.is_empty() && added.is_dir()
                    };
                    if !valid {
                        return None;
                    }
                    let added = vec![added];
                    active_search = append_unique(&active_search, &added);
                    return Some(crate::engine::language::macro_expand::MacroFileLoad::Path);
                }
                let crate::engine::language::macro_expand::MacroFileDirective::Include(filename) =
                    event.directive
                else {
                    return None;
                };
                let filename = filename.to_string();
                let site = (event.file.to_string(), event.span);
                let root_span = event
                    .parents
                    .first()
                    .map(|(_, span)| *span)
                    .unwrap_or(event.span);
                let Some(path) =
                    self.resolve_filename(root_key, event.file, &filename, &active_search)
                else {
                    note_resolved(&mut targets, site.clone(), filename.clone(), &event, false);
                    note_unresolved(&mut targets, site);
                    if seen_sites.insert((event.file.to_string(), event.span)) {
                        let mut searched = Vec::new();
                        if let Some(parent) = Path::new(event.file).parent() {
                            searched.push(parent.display().to_string());
                        }
                        for path in self.configured_search(root_key, &active_search) {
                            let display = path.display().to_string();
                            if !searched.contains(&display) {
                                searched.push(display);
                            }
                        }
                        records.unresolved.push(UnresolvedInclude {
                            filename: filename.clone(),
                            span: root_span,
                            included_from: (!event.parents.is_empty())
                                .then(|| file_basename(event.file)),
                            searched,
                            site: IncludeSite {
                                file: event.file.to_string(),
                                span: event.span,
                            },
                            chain: event
                                .parents
                                .iter()
                                .map(|(file, span)| IncludeSite {
                                    file: file.clone(),
                                    span: *span,
                                })
                                .collect(),
                        });
                    }
                    return None;
                };
                let target = self.include_key(&path);
                note_resolved(&mut targets, site, target.clone(), &event, true);
                if !records.resolved.iter().any(|record| record.path == path) {
                    records.resolved.push(ResolvedInclude {
                        filename,
                        span: event.span,
                        path,
                    });
                }
                let source = match self.source_for_key(&target, event.budget) {
                    Ok(Some(source)) => source,
                    Ok(None) => return None,
                    Err(MacroEvalError::Limit(limit)) => {
                        return Some(crate::engine::language::macro_expand::MacroFileLoad::Limit(
                            limit,
                        ))
                    }
                    Err(_) => {
                        return Some(crate::engine::language::macro_expand::MacroFileLoad::Limit(
                            "iteration work",
                        ))
                    }
                };
                Some(
                    crate::engine::language::macro_expand::MacroFileLoad::Source {
                        file: target,
                        source,
                    },
                )
            },
        );
        targets.loop_occurrences = std::mem::take(&mut proof.loop_occurrences);
        let included = records
            .resolved
            .iter()
            .map(|record| self.include_key(&record.path))
            .filter(|file| file != root_key)
            .collect();
        self.include_owners.insert(root_key.to_string(), included);
        IncludeWalk {
            records,
            proof,
            targets,
            search: active_search,
        }
    }

    fn spliced_source(&mut self, key: &str) -> Arc<SplicedSource> {
        if let Some(source) = self.spliced.get(key) {
            return Arc::clone(source);
        }
        // Loading an executed include can invalidate other roots. Publish the
        // records and joined source only after the ordered walk has finished.
        let IncludeWalk {
            records,
            mut proof,
            targets,
            search: include_search,
        } = self.walk_graph(key);
        let result = self.splice_with_map(
            key,
            &proof.sites,
            &targets,
            &mut Vec::new(),
            None,
            &mut proof.budget,
        );
        let (pieces, splice_limit) = match result {
            Ok(pieces) => (pieces, None),
            Err(MacroEvalError::Limit(limit)) => {
                let span = proof
                    .sites
                    .iter()
                    .filter_map(|(file, span)| (file == key).then_some(*span))
                    .min_by_key(|span| span.start)
                    .unwrap_or_default();
                (self.splice_limit_prefix(key, span), Some((span, limit)))
            }
            Err(_) => (
                self.splice_limit_prefix(key, Span::default()),
                Some((Span::default(), "iteration work")),
            ),
        };
        let (text, segments, gaps) = pieces;
        // Execution errors precede the work needed to join their retained text.
        // A later splice stop must not replace an earlier refusal or miss.
        let actual_limit = proof
            .limit
            .as_ref()
            .map(|(file, span, limit)| {
                (
                    IncludeSite {
                        file: file.clone(),
                        span: *span,
                    },
                    *limit,
                    proof
                        .stop_chain
                        .iter()
                        .map(|(file, span)| IncludeSite {
                            file: file.clone(),
                            span: *span,
                        })
                        .collect(),
                )
            })
            .or_else(|| {
                if proof.refusal.is_some() || !records.unresolved.is_empty() {
                    return None;
                }
                let (span, limit) = splice_limit?;
                let failure = targets.splice_failure.borrow();
                let site = failure
                    .as_ref()
                    .map(|(site, _)| site.clone())
                    .unwrap_or(IncludeSite {
                        file: key.to_string(),
                        span,
                    });
                let call = failure.as_ref().and_then(|(_, call)| *call);
                let chain = Self::splice_call_chain(&targets, call);
                Some((site, limit, chain))
            });
        let limit_site = actual_limit.as_ref().map(|(site, _, _)| site.clone());
        let stop_chain = actual_limit
            .as_ref()
            .map(|(_, _, chain)| chain.clone())
            .unwrap_or_else(|| {
                proof
                    .stop_chain
                    .iter()
                    .map(|(file, span)| IncludeSite {
                        file: file.clone(),
                        span: *span,
                    })
                    .collect()
            });
        let limit = actual_limit.map(|(site, limit, _)| {
            let span = if site.file == key {
                site.span
            } else {
                proof
                    .sites
                    .iter()
                    .filter_map(|(file, span)| (file == key).then_some(*span))
                    .min_by_key(|span| span.start)
                    .unwrap_or_default()
            };
            IncompleteReason {
                span,
                code: "I211",
                message: crate::engine::language::macro_expand::i211_limit_message(limit),
                limit_name: Some(limit),
                limit_ceiling: proof.budget.limit_ceiling(limit),
            }
        });
        let targets_complete = targets
            .sites
            .values()
            .all(|plan| !plan.unresolved && !plan.hits.is_empty());
        let navigation = if proof.complete && targets_complete && limit.is_none() {
            NavigationSource::Mapped {
                text: text.clone(),
                segments: segments.clone(),
            }
        } else {
            NavigationSource::Unavailable
        };
        // Include availability is separate from macro syntax/evaluation. An
        // executed malformed file still needs its normal diagnostics. The
        // original file walk owns availability: the joined buffer can retain
        // dormant includes and synthetic headers whose values are metadata.
        // Model/preview expansion checks that buffer with the same metadata.
        let includes_complete = records.unresolved.is_empty()
            && targets_complete
            && limit.is_none()
            && (proof.complete
                || !crate::engine::language::macro_expand::has_include_directives(&text));
        let incomplete_execution = !proof.complete || !includes_complete || limit.is_some();
        let source = Arc::new(SplicedSource {
            text,
            segments,
            gaps,
            includes_complete,
            navigation,
            include_search,
            limit,
            limit_site,
            stop_chain,
            refusal: proof.refusal,
            emitted_prefix: incomplete_execution.then_some(proof.output),
            macro_messages: proof.messages,
        });
        self.records.insert(key.to_string(), records);
        self.spliced.insert(key.to_string(), Arc::clone(&source));
        source
    }

    /// Written ownership of the first macro refusal, independent of a splice.
    pub(crate) fn macro_file_refusal(
        &mut self,
        uri: &str,
    ) -> Option<crate::engine::language::macro_expand::MacroFileRefusal> {
        let key = self.ensure_loaded(uri)?;
        self.spliced_source(&key).refusal.clone()
    }

    /// Written cause and include chain for one incomplete result. Evaluator
    /// stops can be outside the retained splice, so their receipt owns location.
    pub(crate) fn incomplete_cause_origin(
        &mut self,
        root: &str,
        span: Span,
        code: &str,
    ) -> (Option<IncludeSite>, Vec<IncludeSite>) {
        let Some(key) = self.ensure_loaded(root) else {
            return (None, Vec::new());
        };
        let source = self.spliced_source(&key);
        if code == "I211"
            && let Some(site) = &source.limit_site
        {
            return (Some(site.clone()), source.stop_chain.clone());
        }
        if let Some((file, written, actual, _)) = &source.refusal
            && *actual == code
        {
            return (
                Some(IncludeSite {
                    file: file.clone(),
                    span: *written,
                }),
                source.stop_chain.clone(),
            );
        }
        if code == "E061"
            && let Some(miss) = self
                .records
                .get(&key)
                .and_then(|records| records.unresolved.iter().find(|miss| miss.span == span))
        {
            return (Some(miss.site.clone()), miss.chain.clone());
        }
        (self.mapped_incomplete_origin(root, span), Vec::new())
    }

    pub(crate) fn mapped_incomplete_origin(
        &mut self,
        root: &str,
        span: Span,
    ) -> Option<IncludeSite> {
        let key = self.ensure_loaded(root)?;
        let source = self.spliced_source(&key);
        self.map_effective_origin(root, span)
            .map(|(file, mut written)| {
                // A span that crosses splice pieces has no continuous written end,
                // even when its first byte maps to a valid root-file position.
                let continuous = source.segments.iter().any(|segment| {
                    segment.file.as_deref() == Some(file.as_str())
                        && segment.spliced.start <= span.start
                        && span.end <= segment.spliced.end
                        && written.end <= segment.origin.end
                });
                if !continuous {
                    let first_character = self
                        .docs
                        .get(&file)
                        .and_then(|document| {
                            let source = crate::engine::language::parser::normalize_newlines(
                                &document.source,
                            );
                            source
                                .get(written.start as usize..)?
                                .chars()
                                .next()
                                .map(char::len_utf8)
                        })
                        .unwrap_or(0);
                    written.end = written.start + first_character as u32;
                }
                IncludeSite {
                    file,
                    span: written,
                }
            })
    }

    pub(crate) fn macro_file_limit(
        &mut self,
        root: &str,
    ) -> Option<(IncludeSite, IncompleteReason, Vec<IncludeSite>)> {
        let key = self.ensure_loaded(root)?;
        let source = self.spliced_source(&key);
        Some((
            source.limit_site.clone()?,
            source.limit.clone()?,
            source.stop_chain.clone(),
        ))
    }

    /// Retain a small written prefix when an include cannot be safely joined.
    fn splice_limit_prefix(&self, key: &str, span: Span) -> SplicedPieces {
        let Some(source) = self.docs.get(key).map(|doc| doc.source.as_str()) else {
            return (String::new(), Vec::new(), Vec::new());
        };
        let end = (span.end as usize).min(source.len());
        let end = if end <= crate::engine::language::macro_expr::STRING_CAP {
            end
        } else {
            0
        };
        let text = source[..end].to_string();
        let segments = if end == 0 {
            Vec::new()
        } else {
            vec![SpliceSegment {
                spliced: Span::new(0, end),
                file: Some(key.to_string()),
                origin: Span::new(0, end),
                line: 1,
                evaluation: None,
            }]
        };
        (text, segments, Vec::new())
    }

    /// Spliced offsets where the next byte belongs to a different written file.
    pub(crate) fn source_file_cuts(&mut self, uri: &str) -> Vec<u32> {
        let Some(key) = self.ensure_loaded(uri) else {
            return Vec::new();
        };
        let spliced = self.spliced_source(&key);
        let mut cuts = Vec::new();
        let mut previous = None;
        for segment in &spliced.segments {
            if segment.spliced.is_empty() {
                continue;
            }
            if previous.is_some() && previous != Some(segment.file.as_deref()) {
                cuts.push(segment.spliced.start);
            }
            previous = Some(segment.file.as_deref());
        }
        cuts
    }

    /// Spliced spans owned by executed includes, not the root file.
    pub(crate) fn source_include_spans(
        &mut self,
        uri: &str,
    ) -> Vec<crate::engine::language::span::Span> {
        let Some(key) = self.ensure_loaded(uri) else {
            return Vec::new();
        };
        let spliced = self.spliced_source(&key);
        spliced
            .segments
            .iter()
            .filter(|segment| {
                !segment.spliced.is_empty()
                    && segment
                        .file
                        .as_deref()
                        .is_some_and(|file| file != key.as_str())
            })
            .map(|segment| segment.spliced)
            .collect()
    }

    /// Leftover include-directive ranges for the source-layout preview.
    pub(crate) fn source_layout_gaps(
        &mut self,
        uri: &str,
    ) -> Vec<crate::engine::language::macro_expand::SourceLayoutGap> {
        let Some(key) = self.ensure_loaded(uri) else {
            return Vec::new();
        };
        self.spliced_source(&key).gaps.clone()
    }

    /// Written line of each spliced piece, for save-text line numbers.
    pub(crate) fn source_line_segments(
        &mut self,
        uri: &str,
    ) -> Vec<(crate::engine::language::span::Span, u32)> {
        let Some(key) = self.ensure_loaded(uri) else {
            return Vec::new();
        };
        self.spliced_source(&key)
            .segments
            .iter()
            .filter(|segment| !segment.spliced.is_empty())
            .map(|segment| (segment.spliced, segment.line))
            .collect()
    }

    /// Evaluate original expressions at the points removed by include splicing.
    pub(crate) fn source_evaluations(
        &mut self,
        uri: &str,
    ) -> Vec<crate::engine::language::macro_expand::MacroReplay> {
        let Some(key) = self.ensure_loaded(uri) else {
            return Vec::new();
        };
        crate::engine::language::expand::splice_evaluations(&self.spliced_source(&key).segments)
    }

    fn splice_call_chain(targets: &IncludeTargets, call: Option<usize>) -> Vec<IncludeSite> {
        let Some(call) = call else {
            return Vec::new();
        };
        let Some(hit) = targets
            .sites
            .values()
            .flat_map(|plan| &plan.hits)
            .find(|hit| hit.call == call)
        else {
            return Vec::new();
        };
        hit.parent_calls
            .iter()
            .copied()
            .chain(std::iter::once(call))
            .filter_map(|call| {
                targets.sites.iter().find_map(|((file, span), plan)| {
                    plan.hits
                        .iter()
                        .any(|hit| hit.call == call)
                        .then(|| IncludeSite {
                            file: file.clone(),
                            span: *span,
                        })
                })
            })
            .collect()
    }

    /// Splice only executed sites, using the targets selected in execution order.
    /// Dormant directives stay in place so ordinary branch expansion skips them.
    fn splice_with_map(
        &self,
        key: &str,
        sites: &HashSet<(String, Span)>,
        targets: &IncludeTargets,
        stack: &mut Vec<String>,
        call: Option<usize>,
        budget: &mut MacroBudget,
    ) -> Result<SplicedPieces, MacroEvalError> {
        let result = self.splice_call_with_map(key, sites, targets, stack, call, budget);
        if result.is_err() {
            targets.splice_failure.borrow_mut().get_or_insert_with(|| {
                (
                    IncludeSite {
                        file: key.to_string(),
                        span: Span::new(
                            0,
                            self.docs.get(key).map(|doc| doc.source.len()).unwrap_or(0),
                        ),
                    },
                    call,
                )
            });
        }
        result
    }

    fn splice_call_with_map(
        &self,
        key: &str,
        sites: &HashSet<(String, Span)>,
        targets: &IncludeTargets,
        stack: &mut Vec<String>,
        call: Option<usize>,
        budget: &mut MacroBudget,
    ) -> Result<SplicedPieces, MacroEvalError> {
        let Some(source) = self.docs.get(key).map(|document| document.source.as_str()) else {
            return Ok((String::new(), Vec::new(), Vec::new()));
        };
        if stack.len() > crate::engine::language::macro_expr::EXEC_DEPTH_CAP {
            return Err(MacroEvalError::Limit("execution depth"));
        }
        budget.spend_work(source.len().max(1))?;
        stack.push(key.to_string());
        let mut replacements = Vec::new();
        let mut seen_spans = HashSet::new();
        let mut claimed_loops: Vec<Span> = Vec::new();
        let mut series = Vec::new();
        budget.spend_work(sites.len().max(1))?;
        let written_sites = written_include_sites(key, sites);
        for span in &written_sites {
            let Some(plan) = targets.sites.get(&(key.to_string(), *span)) else {
                continue;
            };
            let hits: Vec<_> = hits_for_call(plan, call).collect();
            if hits.is_empty() {
                continue;
            }
            let mut varies = false;
            for hit in hits.iter().skip(1) {
                if !Self::same_call_shape(hits[0], hit, targets, 0, budget)? {
                    varies = true;
                    break;
                }
            }
            if varies {
                series.push(*span);
            }
        }
        let mut grouped: Vec<Span> = Vec::new();
        for &span in &series {
            budget.spend_work(grouped.len() + claimed_loops.len() + 1)?;
            if grouped.contains(&span) {
                continue;
            }
            if claimed_loops
                .iter()
                .any(|claimed| claimed.start <= span.start && span.end <= claimed.end)
            {
                continue;
            }
            let loops =
                crate::engine::language::macro_expand::loops_containing(source, span, budget)?;
            let Some(outer) = loops.first() else {
                let site = (key.to_string(), span);
                let Some(plan) = targets.sites.get(&site) else {
                    continue;
                };
                let hits: Vec<_> = hits_for_call(plan, call).collect();
                let (body, mut segments, gaps) =
                    self.concat_include_hits(&hits, sites, targets, stack, budget)?;
                prepend_evaluation(&mut segments, key, source, span, budget)?;
                replacements.push((span, body, segments, gaps));
                seen_spans.insert(span);
                grouped.push(span);
                continue;
            };
            let for_span = Span::new(outer.header.start as usize, outer.closer.end as usize);
            budget.spend_work(series.len().max(1))?;
            let group: Vec<Span> = series
                .iter()
                .copied()
                .filter(|site| for_span.start <= site.start && site.end <= for_span.end)
                .collect();
            let (body, segments, gaps) = self.unroll_loop_includes(
                key,
                source,
                &loops,
                sites,
                targets,
                stack,
                call,
                &[],
                budget,
            )?;
            replacements.push((for_span, body, segments, gaps));
            claimed_loops.push(for_span);
            seen_spans.insert(for_span);
            for site in group {
                seen_spans.insert(site);
                grouped.push(site);
            }
        }
        budget.spend_work(sites.len().saturating_mul(claimed_loops.len() + 2).max(1))?;
        let stable_sites: Vec<Span> = written_sites
            .iter()
            .copied()
            .filter(|span| !seen_spans.contains(span))
            .filter(|span| {
                !claimed_loops
                    .iter()
                    .any(|claimed| claimed.start <= span.start && span.end <= claimed.end)
            })
            .collect();
        for span in stable_sites {
            seen_spans.insert(span);
            let site = (key.to_string(), span);
            let Some(plan) = targets.sites.get(&site) else {
                continue;
            };
            let Some(hit) = hits_for_call(plan, call).next() else {
                continue;
            };
            if !hit.resolved {
                continue;
            }
            let (body, mut segments, gaps) =
                self.splice_with_map(&hit.path, sites, targets, stack, Some(hit.call), budget)?;
            prepend_evaluation(&mut segments, key, source, span, budget)?;
            replacements.push((span, body, segments, gaps));
        }
        stack.pop();
        apply_replacements_mapped(source, replacements, Some(key.to_string()), budget)
    }

    fn concat_include_hits(
        &self,
        hits: &[&IncludeHit],
        sites: &HashSet<(String, Span)>,
        targets: &IncludeTargets,
        stack: &mut Vec<String>,
        budget: &mut MacroBudget,
    ) -> Result<SplicedPieces, MacroEvalError> {
        let mut text = String::new();
        let mut segments = Vec::new();
        let mut gaps = Vec::new();
        for hit in hits {
            let (body, nested, nested_gaps) =
                self.splice_with_map(&hit.path, sites, targets, stack, Some(hit.call), budget)?;
            budget.spend_work(body.len() + nested.len() + nested_gaps.len() + 1)?;
            let offset = text.len() as u32;
            for mut segment in nested {
                segment.spliced.start += offset;
                segment.spliced.end += offset;
                if segment.spliced.start < segment.spliced.end || segment.evaluation.is_some() {
                    segments.push(segment);
                }
            }
            for gap in nested_gaps {
                gaps.push(gap.shift(offset));
            }
            text.push_str(&body);
            if !text.ends_with('\n') {
                text.push('\n');
            }
        }
        Ok((text, segments, gaps))
    }

    #[allow(clippy::too_many_arguments)]
    fn unroll_loop_includes(
        &self,
        key: &str,
        source: &str,
        loops: &[crate::engine::language::macro_expand::ContainingLoop],
        sites: &HashSet<(String, Span)>,
        targets: &IncludeTargets,
        stack: &mut Vec<String>,
        call: Option<usize>,
        context: &[(Span, usize)],
        budget: &mut MacroBudget,
    ) -> Result<SplicedPieces, MacroEvalError> {
        let Some(outer) = loops.first() else {
            return Ok((String::new(), Vec::new(), Vec::new()));
        };
        if budget.exec_depth >= crate::engine::language::macro_expr::EXEC_DEPTH_CAP {
            return Err(MacroEvalError::Limit("execution depth"));
        }
        budget.exec_depth += 1;
        let keys = iteration_keys(key, outer.header, targets, call, context, budget)?;
        let mut text = String::new();
        let mut segments = Vec::new();
        let mut gaps = Vec::new();
        push_evaluation(&mut segments, 0, key, source, outer.header, budget)?;
        for iteration in keys {
            push_loop_input(
                &mut segments,
                text.len(),
                key,
                source,
                outer.header,
                &iteration.input,
                budget,
            )?;
            push_generated(
                &mut text,
                &mut segments,
                &one_shot_header_budget(&outer.variables, budget)?,
                key,
                outer.header,
                source,
                budget,
            )?;
            self.copy_loop_body(
                key,
                source,
                outer.body_start,
                outer.body_end,
                outer.header,
                iteration.frame,
                context,
                sites,
                targets,
                stack,
                call,
                &mut text,
                &mut segments,
                &mut gaps,
                budget,
            )?;
            push_generated(
                &mut text,
                &mut segments,
                "@#endfor\n",
                key,
                outer.closer,
                source,
                budget,
            )?;
        }
        budget.exec_depth -= 1;
        Ok((text, segments, gaps))
    }

    /// Compare executed occurrences, not filenames. A recursive invocation may
    /// reach a different target or stop before a site reached by its caller.
    fn same_call_shape(
        left: &IncludeHit,
        right: &IncludeHit,
        targets: &IncludeTargets,
        depth: usize,
        budget: &mut MacroBudget,
    ) -> Result<bool, MacroEvalError> {
        budget.spend_work(left.path.len() + right.path.len() + targets.sites.len() + 1)?;
        if left.path != right.path || left.resolved != right.resolved {
            return Ok(false);
        }
        if depth > crate::engine::language::macro_expr::EXEC_DEPTH_CAP {
            return Err(MacroEvalError::Limit("execution depth"));
        }
        // Reconstructed loops carry evaluated inputs in private replay points.
        // Identical target paths do not make those values interchangeable.
        budget.spend_work(targets.loop_occurrences.len().max(1))?;
        let mut left_loops = targets
            .loop_occurrences
            .iter()
            .filter(|item| item.parent_call == Some(left.call));
        let mut right_loops = targets
            .loop_occurrences
            .iter()
            .filter(|item| item.parent_call == Some(right.call));
        loop {
            match (left_loops.next(), right_loops.next()) {
                (None, None) => break,
                (Some(a), Some(b)) => {
                    budget.spend_work(
                        a.input.len() + b.input.len() + a.context.len() + b.context.len() + 1,
                    )?;
                    if a.header != b.header
                        || a.id != b.id
                        || a.context != b.context
                        || a.input != b.input
                    {
                        return Ok(false);
                    }
                }
                _ => return Ok(false),
            }
        }
        let mut written_plans: Vec<_> = targets
            .sites
            .iter()
            .filter_map(|((owner, span), plan)| (owner == &left.path).then_some((*span, plan)))
            .collect();
        written_plans.sort_by_key(|(span, _)| (span.start, span.end));
        for (_, plan) in written_plans {
            budget.spend_work(plan.hits.len().max(1))?;
            let mut left_hits = hits_for_call(plan, Some(left.call));
            let mut right_hits = hits_for_call(plan, Some(right.call));
            loop {
                match (left_hits.next(), right_hits.next()) {
                    (None, None) => break,
                    (Some(a), Some(b))
                        if Self::same_call_shape(a, b, targets, depth + 1, budget)? => {}
                    _ => return Ok(false),
                }
            }
        }
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    fn copy_loop_body(
        &self,
        key: &str,
        source: &str,
        from: u32,
        to: u32,
        header: Span,
        frame: usize,
        parent_context: &[(Span, usize)],
        sites: &HashSet<(String, Span)>,
        targets: &IncludeTargets,
        stack: &mut Vec<String>,
        call: Option<usize>,
        text: &mut String,
        segments: &mut Vec<SpliceSegment>,
        gaps: &mut Vec<crate::engine::language::macro_expand::SourceLayoutGap>,
        budget: &mut MacroBudget,
    ) -> Result<(), MacroEvalError> {
        budget.spend_work(parent_context.len() + sites.len() + 1)?;
        let mut context = parent_context.to_vec();
        context.push((header, frame));
        let mut cuts: Vec<(u32, u32, BodyCut)> = Vec::new();
        for span in written_include_sites(key, sites) {
            if span.start < from || span.end > to {
                continue;
            }
            let loops =
                crate::engine::language::macro_expand::loops_containing(source, span, budget)?;
            let nested = loops
                .iter()
                .position(|item| item.header == header)
                .and_then(|index| loops.get(index + 1));
            if let Some(nested) = nested {
                budget.spend_work(
                    cuts.len() + nested.variables.iter().map(String::len).sum::<usize>() + 1,
                )?;
                if !cuts
                    .iter()
                    .any(|(start, _, _)| *start == nested.header.start)
                {
                    cuts.push((
                        nested.header.start,
                        nested.closer.end,
                        BodyCut::Loop(nested.clone()),
                    ));
                }
                continue;
            }
            let Some(plan) = targets.sites.get(&(key.to_string(), span)) else {
                continue;
            };
            budget.spend_work(plan.hits.len().saturating_mul(context.len() + 1).max(1))?;
            let Some(hit) = hits_for_call(plan, call).find(|hit| in_iteration(hit, &context))
            else {
                continue;
            };
            if !hit.resolved {
                continue;
            }
            budget.spend_work(hit.path.len().max(1))?;
            cuts.push((
                span.start,
                span.end,
                BodyCut::Child(hit.path.clone(), hit.call),
            ));
        }
        cuts.sort_by_key(|(start, _, _)| *start);
        let mut cursor = from;
        for (start, end, cut) in cuts {
            if start < cursor || end > to {
                continue;
            }
            push_root_piece(
                text,
                segments,
                source,
                cursor as usize,
                start as usize,
                Some(key),
                budget,
            )?;
            let (body, nested, nested_gaps) = match cut {
                BodyCut::Loop(nested) => self.unroll_loop_includes(
                    key,
                    source,
                    &[nested],
                    sites,
                    targets,
                    stack,
                    call,
                    &context,
                    budget,
                )?,
                BodyCut::Child(path, child_call) => {
                    let (body, mut nested, gaps) = self.splice_with_map(
                        &path,
                        sites,
                        targets,
                        stack,
                        Some(child_call),
                        budget,
                    )?;
                    prepend_evaluation(&mut nested, key, source, Span { start, end }, budget)?;
                    (body, nested, gaps)
                }
            };
            budget.spend_work(body.len() + nested.len() + nested_gaps.len() + 1)?;
            let offset = text.len() as u32;
            for mut segment in nested {
                segment.spliced.start += offset;
                segment.spliced.end += offset;
                if segment.spliced.start < segment.spliced.end || segment.evaluation.is_some() {
                    segments.push(segment);
                }
            }
            for gap in nested_gaps {
                gaps.push(gap.shift(offset));
            }
            text.push_str(&body);
            if source.as_bytes().get(end as usize) == Some(&b'\n') {
                gaps.push(
                    crate::engine::language::macro_expand::SourceLayoutGap::Omit(Span::new(
                        text.len(),
                        text.len() + 1,
                    )),
                );
            } else if end > start
                && source.as_bytes().get(end as usize - 1) == Some(&b'\n')
                && !body.ends_with('\n')
            {
                let at = text.len();
                text.push('\n');
                gaps.push(
                    crate::engine::language::macro_expand::SourceLayoutGap::SyntheticNewline(
                        Span::new(at, at + 1),
                    ),
                );
            }
            cursor = end;
        }
        push_root_piece(
            text,
            segments,
            source,
            cursor as usize,
            to as usize,
            Some(key),
            budget,
        )?;
        Ok(())
    }
}

enum BodyCut {
    Loop(crate::engine::language::macro_expand::ContainingLoop),
    Child(String, usize),
}

/// A hash set proves membership; budgeted splice traversal follows written order.
/// Repeated executions stay in SitePlan.hits, in their recorded call order.
fn written_include_sites(key: &str, sites: &HashSet<(String, Span)>) -> Vec<Span> {
    let mut written: Vec<_> = sites
        .iter()
        .filter_map(|(file, span)| (file == key).then_some(*span))
        .collect();
    written.sort_by_key(|span| (span.start, span.end));
    written
}

/// Include calls in one execution of their written file.
fn hits_for_call(plan: &SitePlan, call: Option<usize>) -> impl Iterator<Item = &IncludeHit> {
    plan.hits
        .iter()
        .filter(move |hit| hit.parent_calls.last().copied() == call)
}

fn in_iteration(hit: &IncludeHit, context: &[(Span, usize)]) -> bool {
    context.iter().all(|entry| hit.iterations.contains(entry))
}

fn iteration_keys(
    file: &str,
    header: Span,
    targets: &IncludeTargets,
    call: Option<usize>,
    context: &[(Span, usize)],
    budget: &mut MacroBudget,
) -> Result<Vec<IncludeIteration>, MacroEvalError> {
    budget.spend_work(
        targets
            .loop_occurrences
            .len()
            .saturating_mul(context.len() + file.len() + 1)
            .max(1),
    )?;
    let mut keys = Vec::new();
    for occurrence in &targets.loop_occurrences {
        if occurrence.file != file || occurrence.header != header || occurrence.parent_call != call
        {
            continue;
        }
        budget.spend_work(context.len().saturating_mul(occurrence.context.len() + 1))?;
        if !context
            .iter()
            .all(|entry| occurrence.context.contains(entry))
        {
            continue;
        }
        budget.spend_work(occurrence.input.len() + 1)?;
        keys.push(IncludeIteration {
            frame: occurrence.id,
            input: occurrence.input.clone(),
        });
    }
    Ok(keys)
}

fn one_shot_header(variables: &[String]) -> String {
    if variables.len() == 1 {
        format!("@#for {} in [0]\n", variables[0])
    } else {
        let names = variables.join(", ");
        format!("@#for ({names}) in [0]\n")
    }
}

fn one_shot_header_budget(
    variables: &[String],
    budget: &mut MacroBudget,
) -> Result<String, MacroEvalError> {
    let bytes =
        variables.iter().map(String::len).sum::<usize>() + variables.len().saturating_mul(8) + 32;
    budget.spend_work(bytes)?;
    Ok(one_shot_header(variables))
}

fn push_loop_input(
    segments: &mut Vec<SpliceSegment>,
    at: usize,
    file: &str,
    source: &str,
    origin: Span,
    input: &str,
    budget: &mut MacroBudget,
) -> Result<(), MacroEvalError> {
    budget.spend_work(
        input.len() + crate::engine::language::macro_expand::FOR_INPUT_PREFIX.len() + 1,
    )?;
    let mut marker = String::with_capacity(
        crate::engine::language::macro_expand::FOR_INPUT_PREFIX.len() + input.len(),
    );
    marker.push_str(crate::engine::language::macro_expand::FOR_INPUT_PREFIX);
    marker.push_str(input);
    push_evaluation_text(segments, at, file, source, origin, &marker, budget)
}

fn push_evaluation(
    segments: &mut Vec<SpliceSegment>,
    at: usize,
    file: &str,
    source: &str,
    origin: Span,
    budget: &mut MacroBudget,
) -> Result<(), MacroEvalError> {
    let Some(directive) = source.get(origin.start as usize..origin.end as usize) else {
        return Ok(());
    };
    push_evaluation_text(segments, at, file, source, origin, directive, budget)
}

fn push_evaluation_text(
    segments: &mut Vec<SpliceSegment>,
    at: usize,
    file: &str,
    source: &str,
    origin: Span,
    directive: &str,
    budget: &mut MacroBudget,
) -> Result<(), MacroEvalError> {
    if source.get(..origin.start as usize).is_none() {
        return Err(MacroEvalError::Limit("source mapping"));
    }
    budget.spend_work(directive.len() + file.len() + origin.start as usize + 1)?;
    let line = 1 + source[..origin.start as usize]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count() as u32;
    segments.push(SpliceSegment {
        spliced: Span::new(at, at),
        file: Some(file.to_string()),
        origin,
        line,
        evaluation: Some(directive.to_string()),
    });
    Ok(())
}

fn prepend_evaluation(
    segments: &mut Vec<SpliceSegment>,
    file: &str,
    source: &str,
    origin: Span,
    budget: &mut MacroBudget,
) -> Result<(), MacroEvalError> {
    if source
        .get(origin.start as usize..origin.end as usize)
        .is_none()
    {
        return Ok(());
    }
    push_evaluation(segments, 0, file, source, origin, budget)?;
    budget.spend_work(segments.len().max(1))?;
    // A parent include evaluates before any nested header or include at this point.
    let marker = segments.pop().expect("evaluation marker");
    segments.insert(0, marker);
    Ok(())
}

fn push_generated(
    out: &mut String,
    segments: &mut Vec<SpliceSegment>,
    piece: &str,
    file: &str,
    origin: Span,
    source: &str,
    budget: &mut MacroBudget,
) -> Result<(), MacroEvalError> {
    if piece.is_empty() {
        return Ok(());
    }
    budget.spend_work(piece.len() + (origin.start as usize).min(source.len()) + 1)?;
    let line = if (origin.start as usize) <= source.len() {
        1 + source[..origin.start as usize]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count() as u32
    } else {
        1
    };
    let start = out.len() as u32;
    out.push_str(piece);
    segments.push(SpliceSegment {
        spliced: Span {
            start,
            end: out.len() as u32,
        },
        file: Some(file.to_string()),
        origin,
        line,
        evaluation: None,
    });
    Ok(())
}

fn apply_replacements_mapped(
    source: &str,
    replacements: Vec<(
        Span,
        String,
        Vec<SpliceSegment>,
        Vec<crate::engine::language::macro_expand::SourceLayoutGap>,
    )>,
    file: Option<String>,
    budget: &mut MacroBudget,
) -> Result<SplicedPieces, MacroEvalError> {
    budget.spend_work(source.len().max(1) + replacements.len())?;
    let mut out = String::with_capacity(source.len());
    let mut last = 0usize;
    let mut ordered = replacements;
    ordered.sort_by_key(|(span, _, _, _)| span.start);
    let mut segments = Vec::new();
    let mut gaps = Vec::new();
    // Directive trailing newlines land at these spliced offsets once copied.
    let mut newline_marks: Vec<(u32, usize, bool)> = Vec::new();
    for (span, body, nested, nested_gaps) in ordered {
        let start = span.start as usize;
        let end = span.end as usize;
        if start < last || end > source.len() || start > source.len() {
            continue;
        }
        let line = crate::engine::language::macro_expand::directive_line_extent(source, span);
        push_root_piece(
            &mut out,
            &mut segments,
            source,
            last,
            start,
            file.as_deref(),
            budget,
        )?;
        let indent_from = (line.start as usize).max(last);
        if indent_from < start {
            let indent_len = (start - indent_from) as u32;
            let indent_end = out.len() as u32;
            gaps.push(
                crate::engine::language::macro_expand::SourceLayoutGap::Omit(Span {
                    start: indent_end - indent_len,
                    end: indent_end,
                }),
            );
        }
        let offset = out.len() as u32;
        budget.spend_work(body.len() + nested.len() + nested_gaps.len() + 1)?;
        for mut seg in nested {
            seg.spliced.start += offset;
            seg.spliced.end += offset;
            if seg.spliced.start < seg.spliced.end || seg.evaluation.is_some() {
                segments.push(seg);
            }
        }
        for gap in nested_gaps {
            gaps.push(gap.shift(offset));
        }
        out.push_str(&body);
        let consumed_newline = end > start && source.as_bytes()[end - 1] == b'\n';
        let following_newline = if source[end..].starts_with("\r\n") {
            2
        } else if source.as_bytes().get(end) == Some(&b'\n') {
            1
        } else {
            0
        };
        if following_newline > 0 {
            let synthetic = !body.is_empty() && !body.ends_with('\n') && !body.ends_with('\r');
            newline_marks.push((out.len() as u32, following_newline, synthetic));
        } else if consumed_newline && !body.ends_with('\n') && !body.ends_with('\r') {
            // The directive span owns the line break. Put it back so the next
            // written line stays on its own line, including an empty include.
            let at = out.len() as u32;
            out.push('\n');
            newline_marks.push((at, 1, !body.is_empty()));
        }
        last = end;
    }
    push_root_piece(
        &mut out,
        &mut segments,
        source,
        last,
        source.len(),
        file.as_deref(),
        budget,
    )?;
    for (at, bytes, synthetic) in newline_marks {
        if (at as usize) < out.len() && matches!(out.as_bytes()[at as usize], b'\n' | b'\r') {
            let span = Span {
                start: at,
                end: at + bytes as u32,
            };
            gaps.push(if synthetic {
                crate::engine::language::macro_expand::SourceLayoutGap::SyntheticNewline(span)
            } else {
                crate::engine::language::macro_expand::SourceLayoutGap::Omit(span)
            });
        }
    }
    gaps.sort_by_key(|gap| gap.span().start);
    Ok((out, segments, gaps))
}

fn push_root_piece(
    out: &mut String,
    segments: &mut Vec<SpliceSegment>,
    source: &str,
    from: usize,
    to: usize,
    file: Option<&str>,
    budget: &mut MacroBudget,
) -> Result<(), MacroEvalError> {
    if from >= to {
        return Ok(());
    }
    budget.spend_work(to + 1)?;
    let offset = out.len() as u32;
    let len = (to - from) as u32;
    let line = 1 + source[..from].bytes().filter(|byte| *byte == b'\n').count() as u32;
    segments.push(SpliceSegment {
        spliced: Span {
            start: offset,
            end: offset + len,
        },
        file: file.map(str::to_string),
        origin: Span::new(from, to),
        line,
        evaluation: None,
    });
    out.push_str(&source[from..to]);
    Ok(())
}

/// Lexical lookup identity; the stored supplied key remains unchanged.
fn overlay_key(key: &str) -> String {
    include_resolver::supplied_path_key(key)
}

fn overlay_path_key(path: &Path) -> String {
    overlay_key(&path.to_string_lossy())
}

fn overlay_absolute(key: &str) -> bool {
    key.starts_with('/')
        || (key.len() >= 3
            && key.as_bytes()[0].is_ascii_alphabetic()
            && key.as_bytes()[1] == b':'
            && key.as_bytes()[2] == b'/')
}

fn overlay_parent(key: &str) -> Option<String> {
    let normalized = overlay_key(key);
    normalized
        .rfind('/')
        .map(|idx| normalized[..idx.max(1)].to_string())
}

fn overlay_join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        let separator = if parent.ends_with('/') { "" } else { "/" };
        overlay_key(&format!("{parent}{separator}{name}"))
    }
}

fn overlay_includepath_key(key: &str, raw: &str) -> String {
    let path = overlay_key(raw);
    let joined = if overlay_absolute(&path) {
        path
    } else if let Some(parent) = overlay_parent(key) {
        overlay_join(&parent, &path)
    } else {
        path
    };
    overlay_key(&joined)
}

pub fn resolve_includepath(key: &str, raw: &str) -> PathBuf {
    let path = PathBuf::from(normalize_separators(raw));
    let path = if path.is_absolute() {
        path
    } else if let Some(dir) = Path::new(key).parent() {
        dir.join(path)
    } else {
        path
    };
    std::fs::canonicalize(&path)
        .map(strip_verbatim)
        .unwrap_or(path)
}

/// The single path named by a literal `@#includepath` argument.
pub fn includepath_literal(argument: &str) -> Option<String> {
    let mut raw = argument.trim();
    if (raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2)
        || (raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2)
    {
        raw = &raw[1..raw.len() - 1];
    }
    if raw.is_empty() {
        return None;
    }
    Some(raw.to_string())
}

fn append_unique(base: &[PathBuf], extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = base.to_vec();
    for p in extra {
        if !out.iter().any(|e| e == p) {
            out.push(p.clone());
        }
    }
    out
}

/// Bound include IO and decoding before the shared parser sees any bytes.
fn read_include_text(
    path: &Path,
    budget: &mut MacroBudget,
) -> Result<Option<String>, MacroEvalError> {
    let Ok(file) = std::fs::File::open(path) else {
        return Ok(None);
    };
    let Ok(metadata) = file.metadata() else {
        return Ok(None);
    };
    let bytes = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    let allowed = budget.remaining_work() / 2;
    if bytes > allowed {
        return Err(MacroEvalError::Limit("iteration work"));
    }
    // Count IO and UTF-8 validation first, including one byte to detect growth.
    budget.spend_work(bytes.saturating_mul(2).saturating_add(1))?;
    let mut input = Vec::with_capacity(bytes);
    if file.take(bytes as u64 + 1).read_to_end(&mut input).is_err() {
        return Ok(None);
    }
    if input.len() > bytes {
        return Err(MacroEvalError::Limit("iteration work"));
    }
    match String::from_utf8(input) {
        Ok(source) => Ok(Some(source)),
        Err(error) => {
            let bytes = error.into_bytes();
            let length = bytes.len() + bytes.iter().filter(|byte| **byte >= 128).count();
            budget.spend_work(length.max(1))?;
            Ok(Some(bytes.into_iter().map(char::from).collect()))
        }
    }
}

/// Inputs above the fixed provenance byte bound keep metadata without a byte copy.
fn read_input_bytes(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(INPUT_FILE_BYTE_CAP as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= INPUT_FILE_BYTE_CAP).then_some(bytes)
}

fn read_text(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    match String::from_utf8(bytes.clone()) {
        Ok(s) => Some(s),
        Err(_) => Some(bytes.iter().map(|&b| b as char).collect()),
    }
}

#[cfg(test)]
mod include_load_budget_tests {
    use super::*;

    #[test]
    fn splice_work_stop_visits_siblings_in_written_order() {
        // Guards hash iteration changing the actual child whose source-copy
        // charge fails after an otherwise complete macro execution walk.
        let table = (0..4097)
            .map(|n| format!("\"{n}\""))
            .collect::<Vec<_>>()
            .join(",");
        let root = "/root.mod";
        let files = BTreeMap::from([
            (
                root.to_string(),
                "@#include \"table0.inc\"\n@#include \"table1.inc\"\nvar y; model; y=0; end;\n"
                    .to_string(),
            ),
            (
                "/table0.inc".to_string(),
                format!("@#define a0=[{table}]\n// @{{a0[1]}}\n"),
            ),
            (
                "/table1.inc".to_string(),
                format!("@#define a1=[{table}]\n// @{{a1[1]}}\n"),
            ),
        ]);
        for _ in 0..32 {
            let mut workspace = Workspace::overlay_documents_with_work_limit(
                &files,
                None,
                MacroWorkLimit::new(250_000).unwrap(),
            );
            let IncludeWalk {
                mut proof, targets, ..
            } = workspace.walk_graph(root);
            assert!(
                proof.complete && proof.limit.is_none(),
                "execution must complete before the splice stop"
            );
            let remaining = proof.budget.remaining_work();
            assert!(matches!(
                workspace.splice_with_map(
                    root,
                    &proof.sites,
                    &targets,
                    &mut Vec::new(),
                    None,
                    &mut proof.budget
                ),
                Err(MacroEvalError::Limit("iteration work"))
            ));
            let failure = targets.splice_failure.borrow();
            let (site, call) = failure.as_ref().expect("actual splice stop");
            assert_eq!(
                site.file, "/table0.inc",
                "remaining={remaining} call={call:?}"
            );
            let chain = Workspace::splice_call_chain(&targets, *call);
            assert_eq!(chain.len(), 1);
            assert_eq!(chain[0].span.start, 0);
        }
    }

    #[test]
    fn root_work_changes_replace_cached_facts_without_changing_another_root() {
        // Guards standalone parse and joined-cache reuse after a setting change,
        // worker snapshots using old allowances, and cross-root configuration leaks.
        let source = "@#for i in 1:100\nvar y@{i};\n@#endfor\nmodel;\n@#for i in 1:100\ny@{i}=0;\n@#endfor\nend;\n";
        let mut workspace = Workspace::new();
        let a = "/budget/a.mod";
        let b = "/budget/b.mod";
        let small = MacroWorkLimit::new(1000).unwrap();
        let large = MacroWorkLimit::new(100000).unwrap();
        for root in [a, b] {
            workspace.set_root_macro_work_limit(root, large);
            workspace.update_document(root, source);
            assert_eq!(
                workspace.get_effective_model(root).unwrap().equations.len(),
                100
            );
            assert!(workspace.expand_report(root).unwrap().complete);
        }
        let a_before = workspace.input_revision(a).unwrap();
        let b_before = workspace.input_revision(b).unwrap();
        let stamps = workspace.input_stamps(a);
        let mut stopped = workspace.snapshot_for_root_with_limit(a, Vec::new(), small);
        assert!(!stopped.inputs_match(&stamps));
        let model = stopped.get_effective_model(a).unwrap();
        assert!(model.macro_incomplete());
        assert!(
            crate::engine::analysis::model_info::macro_work_limits(model, Some(a))
                .iter()
                .any(|limit| limit["limit"] == "iteration work" && limit["ceiling"] == 1000)
        );
        assert!(!stopped.expand_report(a).unwrap().complete);
        assert_eq!(workspace.input_revision(a).unwrap(), a_before);
        workspace.set_root_macro_work_limit(a, small);
        assert_ne!(workspace.input_revision(a).unwrap(), a_before);
        assert!(workspace.get_model(a).unwrap().macro_incomplete());
        assert!(workspace.get_effective_model(a).unwrap().macro_incomplete());
        assert_eq!(workspace.input_revision(b).unwrap(), b_before);
        assert_eq!(
            workspace.get_effective_model(b).unwrap().equations.len(),
            100
        );
        workspace.set_root_macro_work_limit(a, large);
        assert_eq!(
            workspace.get_effective_model(a).unwrap().equations.len(),
            100
        );
        assert!(workspace.expand_report(a).unwrap().complete);
        assert_eq!(workspace.input_revision(a).unwrap(), a_before);
    }

    #[test]
    fn cached_child_reuses_source_without_a_byte_copy() {
        // Guards a cached include copying all its bytes before execution. The
        // caller receives the same immutable source with a small work balance.
        let mut workspace = Workspace::new();
        let child = "@#if 0\n".to_string() + &" ".repeat(1_024) + "\n@#endif\n";
        workspace.update_document("/budget/child.inc", child);
        let key = normalize_uri("/budget/child.inc");
        let mut budget = MacroBudget::new();
        budget.spend_work(budget.remaining_work() - 8).unwrap();
        let source = workspace
            .source_for_key(&key, &mut budget)
            .unwrap()
            .unwrap();
        assert!(Arc::ptr_eq(&source, &workspace.docs[&key].source));
        assert_eq!(budget.remaining_work(), 7);
    }

    #[test]
    fn disk_child_is_bounded_before_read() {
        let folder = std::env::temp_dir().join(format!(
            "dyg-include-budget-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("child.inc");
        std::fs::write(&file, " ".repeat(1_024)).unwrap();
        let mut workspace = Workspace::new();
        let mut budget = MacroBudget::new();
        budget.spend_work(budget.remaining_work() - 8).unwrap();
        let key = path_key(&file);
        assert!(matches!(
            workspace.source_for_key(&key, &mut budget),
            Err(MacroEvalError::Limit("iteration work"))
        ));
        assert!(!workspace.docs.contains_key(&key));
        assert_eq!(budget.remaining_work(), 8);
        let resolved = folder.canonicalize().unwrap();
        let temporary = std::env::temp_dir().canonicalize().unwrap();
        assert!(resolved.starts_with(&temporary) && resolved != temporary);
        std::fs::remove_dir_all(resolved).unwrap();
    }

    #[test]
    fn acquired_tables_use_parent_work_without_unused_standalone_parses() {
        // Guards acquisition spending the root allowance on unused standalone
        // child parses and full source copies before real parent execution.
        let folder = std::env::temp_dir().join(format!(
            "dyg-acquired-tables-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let table = (0..=4096)
            .map(|number| format!("\"{number}\""))
            .collect::<Vec<_>>()
            .join(",");
        let mut source = String::new();
        for number in 0..3 {
            let name = format!("table{number}.inc");
            std::fs::write(
                folder.join(&name),
                format!("@#define table{number} = [{table}]\n"),
            )
            .unwrap();
            source.push_str(&format!("@#include \"{name}\"\n"));
        }
        source.push_str("var y; model; y=@{(real)table0[2]+(real)table2[3]}; end;\n");
        let root = folder.join("root.mod");
        std::fs::write(&root, source).unwrap();
        let root = path_key(&root);
        let mut workspace = Workspace::new();
        workspace.set_root_macro_work_limit(&root, MacroWorkLimit::new(1_000_000).unwrap());
        let model = workspace.get_effective_model(&root).unwrap();
        assert!(!model.macro_incomplete(), "{:?}", model.incomplete_reasons);
        assert!(model.macro_type_errors.is_empty());
        assert_eq!(model.equations.len(), 1);
        let expanded = workspace.expand_report(&root).unwrap();
        assert!(expanded.complete);
        assert!(
            expanded.effective_text.contains("y = 3"),
            "{}",
            expanded.effective_text
        );
        let temporary = std::env::temp_dir().canonicalize().unwrap();
        let resolved = folder.canonicalize().unwrap();
        assert!(resolved.starts_with(&temporary) && resolved != temporary);
        std::fs::remove_dir_all(resolved).unwrap();
    }

    #[test]
    fn acquired_child_standalone_facts_do_not_replace_parent_context() {
        // Guards lazy standalone queries and shared snapshots contaminating a
        // child's real parent context or hiding an actual parent-context error.
        let folder = std::env::temp_dir().join(format!(
            "dyg-acquired-context-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let child = folder.join("child.inc");
        std::fs::write(&child, "@#if flag[1]\nvar y;\n@#endif\n").unwrap();
        let root = folder.join("root.mod");
        let body = "@#include \"child.inc\"\nmodel; y=0; end;\n";
        std::fs::write(&root, format!("@#define flag=[1]\n{body}")).unwrap();
        let root = path_key(&root);
        let child = path_key(&child);
        let mut workspace = Workspace::new();
        let model = workspace.get_effective_model(&root).unwrap();
        assert!(!model.macro_incomplete());
        assert!(model.macro_type_errors.is_empty());
        assert_eq!(model.endogenous.len(), 1);
        let standalone = workspace.get_model(&child).unwrap();
        assert!(standalone
            .macro_type_errors
            .iter()
            .any(|(_, code, message)| *code == "E063" && message.contains("flag")));
        for _ in 0..2 {
            let mut snapshot = workspace.snapshot_with_root_settings(&root);
            let model = snapshot.get_effective_model(&root).unwrap();
            assert!(!model.macro_incomplete());
            assert!(model.macro_type_errors.is_empty());
            assert_eq!(model.endogenous.len(), 1);
        }
        workspace.update_document(&root, format!("@#define flag=true\n{body}"));
        let model = workspace.get_effective_model(&root).unwrap();
        assert!(model
            .macro_type_errors
            .iter()
            .any(|(_, code, message)| *code == "E285" && message == "You cannot index a boolean"));
        let temporary = std::env::temp_dir().canonicalize().unwrap();
        let resolved = folder.canonicalize().unwrap();
        assert!(resolved.starts_with(&temporary) && resolved != temporary);
        std::fs::remove_dir_all(resolved).unwrap();
    }

    #[test]
    fn canceled_lazy_snapshot_does_not_publish_a_shared_standalone_model() {
        // Guards an interrupted lazy parse poisoning the immutable Doc shared
        // by the original workspace and later uncanceled worker snapshots.
        let folder = std::env::temp_dir().join(format!(
            "dyg-lazy-cancel-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let child = folder.join("child.mod");
        std::fs::write(&child, "var y; model; y=0; end;\n").unwrap();
        let key = path_key(&child);
        for mode in ["explicit before", "inherited before", "during parse"] {
            let mut workspace = Workspace::new();
            workspace
                .source_for_key(&key, &mut MacroBudget::new())
                .unwrap()
                .unwrap();
            let mut interrupted = workspace.snapshot_with_root_settings(&key);
            let flag = Arc::new(AtomicBool::new(false));
            if mode == "inherited before" {
                assert!(crate::engine::control::run(Some(Arc::clone(&flag)), || {
                    flag.store(true, Ordering::Relaxed);
                    assert!(interrupted.get_model(&key).is_none());
                })
                .is_err());
            } else {
                interrupted.cancellation = Some(Arc::clone(&flag));
                if mode == "during parse" {
                    // The first checkpoint enters Doc::model; the next is the
                    // expanded parser, after macro expansion has completed.
                    crate::engine::control::cancel_after(&flag, 1);
                } else {
                    flag.store(true, Ordering::Relaxed);
                }
                assert!(interrupted.get_model(&key).is_none(), "{mode}");
            }
            assert!(flag.load(Ordering::Relaxed), "{mode}");
            assert!(workspace.docs[&key].model.get().is_none(), "{mode}");
            let fresh = workspace.snapshot_with_root_settings(&key);
            let model = fresh.get_model(&key).unwrap();
            assert!(!model.macro_incomplete(), "{mode}");
            assert_eq!(model.endogenous.len(), 1, "{mode}");
            assert_eq!(model.equations.len(), 1, "{mode}");
            let original = workspace.get_model(&key).unwrap();
            assert_eq!(original.equations.len(), 1, "{mode}");
        }
        let temporary = std::env::temp_dir().canonicalize().unwrap();
        let resolved = folder.canonicalize().unwrap();
        assert!(resolved.starts_with(&temporary) && resolved != temporary);
        std::fs::remove_dir_all(resolved).unwrap();
    }

    #[test]
    fn oversized_overlay_edits_change_revision_without_copying_contents() {
        let mut workspace = Workspace::new();
        workspace
            .set_default_macro_work_limit(MacroWorkLimit::new(MacroWorkLimit::MAX as u64).unwrap());
        let source = " ".repeat(INPUT_FILE_BYTE_CAP + 1);
        workspace.update_document("/budget/root.mod", source.clone());
        let first = workspace.input_revision("/budget/root.mod").unwrap();
        workspace.update_document("/budget/root.mod", source.clone());
        assert_eq!(first, workspace.input_revision("/budget/root.mod").unwrap());
        let mut changed = source;
        changed.replace_range(0..1, "x");
        workspace.update_document("/budget/root.mod", changed);
        assert_ne!(first, workspace.input_revision("/budget/root.mod").unwrap());
        let key = normalize_uri("/budget/root.mod");
        assert!(workspace.input_file(&key).bytes.is_none());
    }
}

fn strip_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        p
    }
}

fn file_basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

#[cfg(test)]
mod compare_snapshot_tests {
    use super::Workspace;

    #[test]
    fn snapshot_rejects_disk_changes_before_hash_and_after_hash() {
        let path = std::env::temp_dir().join(format!(
            "dygnosis-compare-snapshot-{}-{}.mod",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, "var old_name;").unwrap();
        let uri = path.to_str().unwrap();
        let mut workspace = Workspace::new();
        workspace.load_from_disk(&path).unwrap();
        std::fs::write(&path, "var new_name;").unwrap();
        workspace.input_revision(uri).unwrap();
        assert!(
            !workspace.input_snapshot_is_current(uri),
            "parsed source predates hashed bytes"
        );
        workspace.load_from_disk(&path).unwrap();
        workspace.input_revision(uri).unwrap();
        assert!(workspace.input_snapshot_is_current(uri));
        std::fs::write(&path, "var newest_name;").unwrap();
        assert!(
            !workspace.input_snapshot_is_current(uri),
            "disk write after captured revision"
        );
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(test)]
mod project_snapshot_tests {
    use super::*;

    #[test]
    fn snapshots_share_immutable_overlay_documents() {
        let root = std::env::temp_dir().join("dygnosis-overlay-snapshot.mod");
        let uri = root.to_str().unwrap();
        let mut workspace = Workspace::new();
        workspace.update_document(uri, "var before;");
        let snapshot = workspace.snapshot_for_root(uri, Vec::new());
        let key = normalize_uri(uri);
        assert!(Arc::ptr_eq(&workspace.docs[&key], &snapshot.docs[&key]));
        workspace.update_document(uri, "var after;");
        assert_eq!(snapshot.get_source(uri), Some("var before;"));
        assert_eq!(workspace.get_source(uri), Some("var after;"));
    }

    #[test]
    fn heavyweight_cache_eviction_keeps_owner_and_dependency_provenance() {
        let folder =
            std::env::temp_dir().join(format!("dygnosis-project-cache-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let included = folder.join("body.data");
        std::fs::write(&included, "y=0;\n").unwrap();
        let mut workspace = Workspace::new();
        let mut roots = Vec::new();
        for number in 0..12 {
            let root = folder.join(format!("root-{number}.mod"));
            std::fs::write(&root, "var y; model;\n@#include \"body.data\"\nend;\n").unwrap();
            workspace.input_revision(root.to_str().unwrap()).unwrap();
            roots.push(path_key(&root));
        }
        let retained = roots.iter().rev().take(8).cloned().collect();
        workspace.retain_analysis_roots(&retained);
        assert!(workspace.effective.len() <= 8);
        assert!(workspace.spliced.len() <= 8);
        assert!(workspace.input_snapshots.len() <= 8);
        assert!(
            workspace.docs.len() <= 9,
            "eight roots and their shared include"
        );
        assert_eq!(workspace.owner_roots(included.to_str().unwrap()).len(), 12);
        assert_eq!(workspace.dependency_candidates.len(), 12);
        let absolute = folder.canonicalize().unwrap();
        assert!(absolute.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        std::fs::remove_dir_all(absolute).unwrap();
    }
}

#[cfg(test)]
mod supplied_compare_revision_tests {
    use super::*;

    #[test]
    fn supplied_revisions_preserve_exact_keys_and_detect_changed_include_text() {
        let root = "map/./root.mod";
        let child = "map/./body";
        let mut workspace = Workspace::overlay_documents(&BTreeMap::from([
            (root.into(), "@#include \"body\"\n".into()),
            (child.into(), "var y; model; y=1; end;".into()),
        ]));
        let before = workspace.input_revision(root).unwrap();
        assert!(workspace.input_snapshot_is_current(root));
        assert!(workspace.input_snapshots.contains_key(root));
        assert!(workspace.snapshot_sources(root).contains_key(child));
        workspace.insert_overlay(child, "var y; model; y=2; end;".into());
        assert!(!workspace.input_snapshot_is_current(root));
        assert_ne!(workspace.input_revision(root).unwrap(), before);
        assert!(workspace.input_snapshot_is_current(root));
    }

    #[test]
    fn missing_supplied_input_has_no_host_disk_state() {
        let path =
            std::env::temp_dir().join(format!("dygnosis-map-state-{}.inc", std::process::id()));
        std::fs::write(&path, "host source").unwrap();
        let workspace = Workspace::overlay_documents(&BTreeMap::new());
        let state = workspace.input_file(path.to_str().unwrap());
        std::fs::remove_file(path).unwrap();
        assert!(!state.exists && !state.directory);
        assert!(state.bytes.is_none() && state.identity.is_none() && state.length.is_none());
    }
}
