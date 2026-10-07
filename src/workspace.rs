//! Workspace index for `@#include` resolution, companion records, and the
//! effective model.
//!
//! Overlay (editor/MCP in-memory text) beats disk. Include and companion
//! records are stored here. This module does not emit diagnostic codes.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::companion::{self, CompanionKind, CompanionRecord};
use crate::expand::{expand_report_from_spliced, ExpandReport, NavigationSource, SpliceSegment};
use crate::include_resolver::{
    is_virtual_uri, normalize_separators, normalize_uri, path_key, resolve_companion_path,
    resolve_include_path, resolve_scoped_include_path, uri_to_path,
};
use crate::model::Model;
use crate::parser::parse;
use crate::span::Span;

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
}

/// One include cycle, canonicalized so rotations are not reported twice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CycleRecord {
    /// Absolute path keys around the cycle, including the closing repeat.
    pub chain: Vec<String>,
    /// Original-file directive span (root include that reaches the cycle).
    pub span: Span,
    /// Verified written include edges, independent of the root warning anchor.
    pub earlier: IncludeSite,
    pub closing: IncludeSite,
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
    pub cycles: Vec<CycleRecord>,
}

struct Doc {
    source: String,
    model: Model,
    overlay: bool,
}

/// One joined include source and its written-file map. The model, expand view,
/// and diagnostic locations all read this same snapshot until invalidation.
struct SplicedSource {
    text: String,
    segments: Vec<SpliceSegment>,
    /// Leftover include-directive indent / line endings for source layout.
    gaps: Vec<crate::macro_expand::SourceLayoutGap>,
    includes_complete: bool,
    navigation: NavigationSource,
    include_search: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
struct IncludeHit {
    path: String,
    bindings: Vec<(String, String)>,
}

/// Executed resolutions of one written include directive.
#[derive(Clone, Debug, Default)]
struct SitePlan {
    hits: Vec<IncludeHit>,
    /// Missing, cyclic, or not certain. The directive stays in the spliced text.
    unresolved: bool,
}

type IncludeTargets = HashMap<(String, Span), SitePlan>;

fn note_unresolved(targets: &mut IncludeTargets, site: (String, Span)) {
    targets.entry(site).or_default().unresolved = true;
}

fn note_resolved(
    targets: &mut IncludeTargets,
    site: (String, Span),
    path: String,
    bindings: &[(String, String)],
) {
    let plan = targets.entry(site).or_default();
    if plan.hits.last().is_some_and(|hit| hit.path == path) {
        return;
    }
    plan.hits.push(IncludeHit {
        path,
        bindings: bindings.to_vec(),
    });
}

/// Nesting of `@#if` / `@#for` before `byte`. A top-level directive has depth 0.
fn directive_depth(source: &str, byte: u32) -> i32 {
    let end = (byte as usize).min(source.len());
    let mut depth = 0i32;
    for line in source[..end].split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("@#if") || trimmed.starts_with("@#for") {
            depth += 1;
        } else if trimmed.starts_with("@#endif") || trimmed.starts_with("@#endfor") {
            depth = depth.saturating_sub(1);
        }
    }
    depth
}

/// End of the earliest missing or cyclic include that is not inside `@#if` / `@#for`.
/// An include inside a branch still reports E061, and the equations after that
/// branch stay in the root.
fn top_level_fatal_cut(source: &str, records: &IncludeRecords) -> Option<u32> {
    records
        .unresolved
        .iter()
        .map(|item| item.span)
        .chain(records.cycles.iter().map(|item| item.span))
        .filter(|span| directive_depth(source, span.start) == 0)
        .map(|span| span.end)
        .min()
}

struct IncludeWalk {
    records: IncludeRecords,
    proof: crate::macro_expand::MacroFileProof,
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
}

/// Compact provenance retained by project reports after parsed caches expire.
pub(crate) type InputStamps = BTreeMap<String, u64>;

/// URI/path-keyed document index plus include graph walks.
#[derive(Default)]
pub struct Workspace {
    docs: HashMap<String, Arc<Doc>>,
    search_paths: Vec<PathBuf>,
    root_search_paths: HashMap<String, Vec<PathBuf>>,
    virtual_roots: HashSet<String>,
    // Retain input provenance when model/source caches are cleared.
    dependency_candidates: HashMap<String, HashSet<String>>,
    input_snapshots: HashMap<String, BTreeMap<String, InputFile>>,
    include_owners: HashMap<String, HashSet<String>>,
    effective: HashMap<String, Model>,
    spliced: HashMap<String, Arc<SplicedSource>>,
    expand: HashMap<String, ExpandReport>,
    records: HashMap<String, IncludeRecords>,
    companions: HashMap<String, Vec<CompanionRecord>>,
    /// When set, document text and `@#include` targets come only from overlays.
    overlay_only: bool,
}

impl Workspace {
    /// Isolated editor snapshot that preserves this root's include settings.
    pub(crate) fn snapshot_with_root_settings(&self, uri: &str) -> Self {
        self.snapshot_for_root(uri, self.root_paths(&normalize_uri(uri)).to_vec())
    }

    /// Owned editor snapshot. Parsed overlays and joined source are immutable
    /// and shared; disk IO and root analysis happen on the receiving worker.
    pub(crate) fn snapshot_for_root(&self, uri: &str, search_paths: Vec<PathBuf>) -> Self {
        let key = normalize_uri(uri);
        let reuse_joined = self.root_paths(&key) == search_paths.as_slice();
        let candidates = self.input_snapshots.get(&key);
        let mut snapshot = Self::with_search_paths(search_paths);
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
        // set_root_search_paths invalidates the cache when the scope is first
        // registered; restore the immutable joined source after that operation.
        if let Some(source) = self.spliced.get(&key).filter(|_| reuse_joined) {
            snapshot.spliced.insert(key.clone(), Arc::clone(source));
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
    }

    pub fn new() -> Self {
        Self::default()
    }

    /// Overlay workspace. Include lookup does not read file bodies from disk.
    ///
    /// Keys are stored as given. This path does not canonicalize, lowercase,
    /// or collapse `..`.
    pub(crate) fn overlay_documents(files: &BTreeMap<String, String>) -> Self {
        let mut ws = Self {
            overlay_only: true,
            ..Self::default()
        };
        for (name, content) in files {
            ws.insert_overlay(name, content.clone());
        }
        ws
    }

    /// Store one overlay under `key` exactly. No disk and no path folding.
    fn insert_overlay(&mut self, key: &str, source: String) {
        let model = parse(&source);
        self.docs.insert(
            key.to_string(),
            Arc::new(Doc {
                source,
                model,
                overlay: true,
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
        let key = normalize_uri(uri);
        let source = source.into();
        let model = parse(&source);
        self.docs.insert(
            key.clone(),
            Arc::new(Doc {
                source,
                model,
                overlay: true,
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
        let key = normalize_uri(uri);
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
            return self.docs.get(&key).map(|d| &d.model);
        }
        if self.overlay_only {
            return None;
        }
        let source = read_text(path)?;
        let model = parse(&source);
        self.docs.insert(
            key.clone(),
            Arc::new(Doc {
                source,
                model,
                overlay: false,
            }),
        );
        // Other roots may have spliced this file.
        self.effective.clear();
        self.spliced.clear();
        self.expand.clear();
        self.records.clear();
        self.companions.clear();
        self.docs.get(&key).map(|d| &d.model)
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
        let key = normalize_uri(uri);
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

    /// Observe disk changes without replacing editor overlays. Missing search
    /// candidates are inputs too: creating one can change an include's owner.
    pub fn input_revision(&mut self, uri: &str) -> Option<String> {
        let key = normalize_uri(uri);
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
        self.dependency_candidates
            .entry(key.clone())
            .or_default()
            .extend(resolved_inputs.iter().map(|path| path_key(path)));
        if !self.overlay_only && !self.virtual_roots.contains(&key) {
            // These checks read directory existence and a loader file outside
            // include/companion resolution. They are revision inputs too.
            let paths: Vec<_> = self
                .get_effective_model(uri)
                .into_iter()
                .flat_map(|model| {
                    let loader = model.load_params_file.iter().map(|(name, _)| {
                        let path = PathBuf::from(name);
                        if path.is_absolute() {
                            path
                        } else {
                            Path::new(&key).parent().unwrap_or(Path::new("")).join(path)
                        }
                    });
                    loader
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
        // Revisions describe analysis inputs, not whether identical text came
        // from disk or an editor overlay. Keep that mode in snapshot equality
        // so cache invalidation and source ownership still observe transitions.
        for (path, state) in &snapshot {
            path.hash(&mut hash);
            state.bytes.hash(&mut hash);
            state.exists.hash(&mut hash);
            state.directory.hash(&mut hash);
            state.identity.hash(&mut hash);
        }
        self.input_snapshots.insert(key, snapshot);
        Some(format!("{:016x}", hash.finish()))
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
        self.input_snapshots
            .get(&normalize_uri(uri))
            .into_iter()
            .flat_map(|snapshot| snapshot.iter())
            .map(|(path, state)| {
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                state.hash(&mut hash);
                (path.clone(), hash.finish())
            })
            .collect()
    }

    /// This validation reads disk and must run on the background worker.
    pub(crate) fn inputs_match(&self, stamps: &InputStamps) -> bool {
        !stamps.is_empty()
            && stamps.iter().all(|(path, expected)| {
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
                bytes: Some(doc.source.as_bytes().to_vec()),
                exists: true,
                directory: false,
                identity,
            }
        } else {
            let metadata = (!is_virtual_uri(key))
                .then(|| std::fs::metadata(key).ok())
                .flatten();
            InputFile {
                overlay: false,
                bytes: if is_virtual_uri(key) {
                    None
                } else {
                    std::fs::read(key).ok()
                },
                exists: metadata.is_some(),
                directory: metadata.is_some_and(|metadata| metadata.is_dir()),
                identity,
            }
        }
    }

    /// Validate a result against the exact files observed by input_revision.
    /// Also catch a disk write between loading a parsed source and hashing it.
    pub(crate) fn input_snapshot_is_current(&self, uri: &str) -> bool {
        let Some(snapshot) = self.input_snapshots.get(&normalize_uri(uri)) else {
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
                return false;
            };
            let decoded = String::from_utf8(bytes.clone())
                .unwrap_or_else(|_| bytes.iter().map(|&byte| byte as char).collect());
            document.source == decoded
        })
    }

    /// Known compilation-unit owners from resolved include edges. Retained
    /// across cache invalidation so an editor edit does not lose root context.
    pub fn owner_roots(&self, uri: &str) -> Vec<String> {
        let key = normalize_uri(uri);
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
        self.docs.get(&normalize_uri(uri)).map(|d| &d.model)
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
    /// required unresolved/cyclic edges stay empty. Dormant sites remain for
    /// the ordinary macro branch/loop expansion to skip.
    pub fn get_effective_model(&mut self, uri: &str) -> Option<&Model> {
        let key = self.ensure_loaded(uri)?;
        if !self.effective.contains_key(&key) {
            let spliced = self.spliced_source(&key);
            let root = self
                .docs
                .get(&key)
                .map(|doc| doc.source.clone())
                .unwrap_or_default();
            let fatal = self.records.get(&key).and_then(|records| {
                let span = records
                    .unresolved
                    .first()
                    .map(|item| item.span)
                    .or_else(|| records.cycles.first().map(|item| item.span))?;
                Some((span, top_level_fatal_cut(&root, records)))
            });
            let mut model = if let Some((_, Some(cut))) = fatal {
                let cut = (cut as usize).min(root.len());
                parse(&root[..cut])
            } else {
                let lines: Vec<_> = spliced
                    .segments
                    .iter()
                    .map(|segment| (segment.spliced, segment.line))
                    .collect();
                crate::parser::parse_with_lines(&spliced.text, &lines)
            };
            if let Some((span, _)) = fatal {
                model.macro_incomplete_span.get_or_insert(span);
            }
            self.effective.insert(key.clone(), model);
        }
        self.effective.get(&key)
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

    pub fn expand_report(&mut self, uri: &str) -> Option<&ExpandReport> {
        let key = self.ensure_loaded(uri)?;
        if !self.expand.contains_key(&key) {
            let spliced = self.spliced_source(&key);
            let root = self
                .docs
                .get(&key)
                .map(|doc| doc.source.clone())
                .unwrap_or_default();
            let (incomplete, cut) = self
                .records
                .get(&key)
                .map(|records| {
                    (
                        !records.unresolved.is_empty() || !records.cycles.is_empty(),
                        top_level_fatal_cut(&root, records),
                    )
                })
                .unwrap_or((false, None));
            let mut report = expand_report_from_spliced(
                &spliced.text,
                &spliced.segments,
                Some(&spliced.navigation),
            );
            if incomplete {
                report.complete = false;
                report.n_equations = 0;
                if let Some(cut) = cut {
                    let cut = (cut as usize).min(spliced.text.len()).min(root.len());
                    let prefix = if spliced.text.as_bytes().get(..cut) == root.as_bytes().get(..cut)
                    {
                        &spliced.text[..cut]
                    } else {
                        &root[..cut]
                    };
                    let prefix_report = expand_report_from_spliced(prefix, &[], None);
                    report.effective_text = prefix_report.effective_text;
                    report.macro_messages = prefix_report.macro_messages;
                }
            }
            self.expand.insert(key.clone(), report);
        }
        self.expand.get(&key)
    }

    /// Independently verified written portions of any include-spliced span.
    pub fn map_effective_segments(
        &mut self,
        uri: &str,
        span: Span,
    ) -> Vec<crate::model_map::WrittenSegment> {
        let Some(key) = self.ensure_loaded(uri) else {
            return Vec::new();
        };
        let spliced = self.spliced_source(&key);
        crate::expand::map_written_segments(&spliced.segments, span)
    }

    /// Transitively included files (root excluded).
    pub fn resolve_all_includes(&mut self, uri: &str) -> HashMap<String, Model> {
        let records = self.include_records(uri).cloned().unwrap_or_default();
        let mut out = HashMap::new();
        let mut seen = HashSet::new();
        for resolved in &records.resolved {
            let key = self.include_key(&resolved.path);
            if !seen.insert(key.clone()) {
                continue;
            }
            if let Some(model) = self.model_for_key(&key).cloned() {
                out.insert(key, model);
            }
        }
        out
    }

    pub fn find_circular_includes(&mut self, uri: &str) -> Vec<CycleRecord> {
        self.include_records(uri)
            .map(|r| r.cycles.clone())
            .unwrap_or_default()
    }

    pub fn find_unresolved_includes(&mut self, uri: &str) -> Vec<UnresolvedInclude> {
        self.include_records(uri)
            .map(|r| r.unresolved.clone())
            .unwrap_or_default()
    }

    /// Executed include records (written spans, resolved targets, misses, cycles).
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
            return self.docs.get(key).map(|d| &d.model);
        }
        if self.overlay_only {
            return None;
        }
        let path = PathBuf::from(key);
        if path.exists() {
            self.load_from_disk(&path)?;
        }
        self.docs.get(key).map(|d| &d.model)
    }

    fn source_for_key(&mut self, key: &str) -> Option<String> {
        if let Some(doc) = self.docs.get(key) {
            return Some(doc.source.clone());
        }
        if self.overlay_only {
            return None;
        }
        let path = PathBuf::from(key);
        if path.exists() {
            self.load_from_disk(&path)?;
            return self.docs.get(key).map(|d| d.source.clone());
        }
        None
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
        if self.virtual_roots.contains(root_key) {
            return None;
        }
        if self.overlay_only {
            return self
                .overlay_include_key(including_key, filename, active_search)
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

    /// Match an `@#include` filename to an overlay key. No disk and no `..` fold.
    fn overlay_include_key(
        &self,
        including_key: &str,
        filename: &str,
        active_search: &[PathBuf],
    ) -> Option<String> {
        let name = overlay_key(filename);
        if name.is_empty() {
            return None;
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
            if !candidates.contains(&name) {
                candidates.push(name);
            }
        }
        candidates
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
            let paths = &joined.include_search;
            if let Some(key) = self.overlay_include_key(root_key, name, paths) {
                return Some(PathBuf::from(key));
            }
            if Path::new(name).extension().is_some() {
                return None;
            }
            for suffix in extra_suffixes {
                if let Some(key) =
                    self.overlay_include_key(root_key, &format!("{name}{suffix}"), paths)
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
        let cut = self
            .records
            .get(root_key)
            .and_then(|records| top_level_fatal_cut(&root, records));
        let (source, model) = if let Some(cut) = cut {
            let text = root[..(cut as usize).min(root.len())].to_string();
            let model = parse(&text);
            (text, model)
        } else {
            let Some(doc) = self.docs.get(root_key) else {
                return Vec::new();
            };
            (doc.model.source.clone(), doc.model.clone())
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
        companion::harvest(&source, &model, &convention, |name, extra| {
            self.resolve_companion_from_root(root_key, name, extra)
        })
    }

    /// Resolve files only when the shared macro walker executes their site.
    fn walk_graph(&mut self, root_key: &str) -> IncludeWalk {
        self.dependency_candidates.remove(root_key);
        let mut records = IncludeRecords::default();
        let mut targets = IncludeTargets::new();
        let mut active_search = Vec::new();
        let mut seen_sites = HashSet::new();
        let mut seen_cycles = HashSet::new();
        let source = self.docs.get(root_key).expect("loaded root").source.clone();
        let proof = crate::macro_expand::walk_macro_files(root_key, &source, |event| {
            if !event.certain {
                if matches!(
                    event.directive,
                    crate::macro_expand::MacroFileDirective::Include(_)
                ) {
                    note_unresolved(&mut targets, (event.file.to_string(), event.span));
                }
                return None;
            }
            if let crate::macro_expand::MacroFileDirective::IncludePath(path) = event.directive {
                let added = if self.overlay_only {
                    PathBuf::from(overlay_includepath_key(root_key, path))
                } else {
                    resolve_includepath(root_key, path)
                };
                if !path.is_empty() && !self.overlay_only {
                    // The directory check reads existence/type even when it
                    // refuses. Creating that directory must invalidate E304.
                    self.dependency_candidates
                        .entry(root_key.to_string())
                        .or_default()
                        .insert(path_key(&added));
                }
                let valid = if self.overlay_only {
                    self.overlay_directory_exists(root_key, path)
                } else {
                    !path.is_empty() && added.is_dir()
                };
                if !valid {
                    return None;
                }
                let added = vec![added];
                active_search = append_unique(&active_search, &added);
                return Some(crate::macro_expand::MacroFileLoad::Path);
            }
            let crate::macro_expand::MacroFileDirective::Include(filename) = event.directive else {
                return None;
            };
            let filename = filename.to_string();
            let site = (event.file.to_string(), event.span);
            let first_visit = seen_sites.insert(site.clone());
            let root_span = event
                .parents
                .first()
                .map(|(_, span)| *span)
                .unwrap_or(event.span);
            let Some(path) = self.resolve_filename(root_key, event.file, &filename, &active_search)
            else {
                note_unresolved(&mut targets, site);
                if first_visit {
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
                    });
                }
                return None;
            };
            let target = self.include_key(&path);
            let callers: Vec<_> = event
                .parents
                .iter()
                .map(|(file, _)| file.clone())
                .chain(std::iter::once(event.file.to_string()))
                .collect();
            if let Some(index) = callers.iter().position(|file| file == &target) {
                let mut chain = callers[index..].to_vec();
                let min_index = chain
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, file)| *file)
                    .map(|(index, _)| index)
                    .unwrap();
                let canonical: Vec<_> = chain[min_index..]
                    .iter()
                    .chain(&chain[..min_index])
                    .cloned()
                    .collect();
                chain.push(target);
                if seen_cycles.insert(canonical) {
                    let earlier = event
                        .parents
                        .get(index.saturating_sub(1))
                        .map(|(file, span)| IncludeSite {
                            file: file.clone(),
                            span: *span,
                        })
                        .unwrap_or_else(|| IncludeSite {
                            file: event.file.to_string(),
                            span: event.span,
                        });
                    records.cycles.push(CycleRecord {
                        chain,
                        span: root_span,
                        earlier,
                        closing: IncludeSite {
                            file: event.file.to_string(),
                            span: event.span,
                        },
                    });
                }
                note_unresolved(&mut targets, site);
                return None;
            }
            note_resolved(&mut targets, site, target.clone(), event.bindings);
            if !records.resolved.iter().any(|record| record.path == path) {
                records.resolved.push(ResolvedInclude {
                    filename,
                    span: event.span,
                    path,
                });
            }
            let source = self.source_for_key(&target)?;
            Some(crate::macro_expand::MacroFileLoad::Source {
                file: target,
                source,
            })
        });
        let included = records
            .resolved
            .iter()
            .map(|record| self.include_key(&record.path))
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
            proof,
            targets,
            search: include_search,
        } = self.walk_graph(key);
        let (text, segments, gaps) =
            self.splice_with_map(key, &proof.sites, &targets, &mut Vec::new());
        let targets_complete = targets
            .values()
            .all(|plan| !plan.unresolved && !plan.hits.is_empty());
        let navigation = if proof.complete && targets_complete {
            NavigationSource::Mapped {
                text: text.clone(),
                segments: segments.clone(),
            }
        } else {
            NavigationSource::Unavailable
        };
        // Include availability is separate from macro syntax/evaluation. An
        // executed malformed file still needs its normal diagnostics.
        let includes_complete = records.unresolved.is_empty()
            && records.cycles.is_empty()
            && targets_complete
            && (!crate::macro_expand::has_include_directives(&text)
                || crate::macro_expand::required_includes_complete(&text));
        let source = Arc::new(SplicedSource {
            text,
            segments,
            gaps,
            includes_complete,
            navigation,
            include_search,
        });
        self.records.insert(key.to_string(), records);
        self.spliced.insert(key.to_string(), Arc::clone(&source));
        source
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
            if previous.is_some() && previous != Some(segment.file.as_deref()) {
                cuts.push(segment.spliced.start);
            }
            previous = Some(segment.file.as_deref());
        }
        cuts
    }

    /// Spliced spans owned by executed includes, not the root file.
    pub(crate) fn source_include_spans(&mut self, uri: &str) -> Vec<crate::span::Span> {
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
    ) -> Vec<crate::macro_expand::SourceLayoutGap> {
        let Some(key) = self.ensure_loaded(uri) else {
            return Vec::new();
        };
        self.spliced_source(&key).gaps.clone()
    }

    /// Written line of each spliced piece, for save-text line numbers.
    pub(crate) fn source_line_segments(&mut self, uri: &str) -> Vec<(crate::span::Span, u32)> {
        let Some(key) = self.ensure_loaded(uri) else {
            return Vec::new();
        };
        self.spliced_source(&key)
            .segments
            .iter()
            .map(|segment| (segment.spliced, segment.line))
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
    ) -> (
        String,
        Vec<SpliceSegment>,
        Vec<crate::macro_expand::SourceLayoutGap>,
    ) {
        if stack.iter().any(|file| file == key) {
            return (String::new(), Vec::new(), Vec::new());
        }
        let Some(source) = self.docs.get(key).map(|document| document.source.clone()) else {
            return (String::new(), Vec::new(), Vec::new());
        };
        stack.push(key.to_string());
        let mut replacements = Vec::new();
        let mut seen_spans = HashSet::new();
        let mut claimed_loops: Vec<Span> = Vec::new();
        let mut series = Vec::new();
        for (file, span) in sites {
            if file != key {
                continue;
            }
            let Some(plan) = targets.get(&(key.to_string(), *span)) else {
                continue;
            };
            if plan.unresolved || plan.hits.is_empty() {
                continue;
            }
            if plan.hits.len() > 1 {
                series.push(*span);
            }
        }
        let mut grouped: Vec<Span> = Vec::new();
        for &span in &series {
            if grouped.contains(&span) {
                continue;
            }
            if claimed_loops
                .iter()
                .any(|claimed| claimed.start <= span.start && span.end <= claimed.end)
            {
                continue;
            }
            let loops = crate::macro_expand::loops_containing(&source, span);
            let Some(outer) = loops.first() else {
                let site = (key.to_string(), span);
                let Some(plan) = targets.get(&site) else {
                    continue;
                };
                let hits = plan.hits.clone();
                let (body, segments, gaps) = self.concat_include_hits(&hits, sites, targets, stack);
                replacements.push((span, body, segments, gaps));
                seen_spans.insert(span);
                grouped.push(span);
                continue;
            };
            let for_span = Span::new(outer.header.start as usize, outer.closer.end as usize);
            let group: Vec<Span> = series
                .iter()
                .copied()
                .filter(|site| for_span.start <= site.start && site.end <= for_span.end)
                .collect();
            let (body, segments, gaps) =
                self.unroll_loop_includes(key, &source, &loops, &group, sites, targets, stack);
            replacements.push((for_span, body, segments, gaps));
            claimed_loops.push(for_span);
            seen_spans.insert(for_span);
            for site in group {
                seen_spans.insert(site);
                grouped.push(site);
            }
        }
        let stable_sites: Vec<Span> = sites
            .iter()
            .filter_map(|(file, span)| (file == key).then_some(*span))
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
            let Some(plan) = targets.get(&site) else {
                continue;
            };
            if plan.unresolved || plan.hits.len() != 1 {
                continue;
            }
            let (body, segments, gaps) =
                self.splice_with_map(&plan.hits[0].path, sites, targets, stack);
            replacements.push((span, body, segments, gaps));
        }
        stack.pop();
        apply_replacements_mapped(&source, &replacements, Some(key.to_string()))
    }

    fn concat_include_hits(
        &self,
        hits: &[IncludeHit],
        sites: &HashSet<(String, Span)>,
        targets: &IncludeTargets,
        stack: &mut Vec<String>,
    ) -> (
        String,
        Vec<SpliceSegment>,
        Vec<crate::macro_expand::SourceLayoutGap>,
    ) {
        let mut text = String::new();
        let mut segments = Vec::new();
        let mut gaps = Vec::new();
        for hit in hits {
            let (body, nested, nested_gaps) =
                self.splice_with_map(&hit.path, sites, targets, stack);
            let offset = text.len() as u32;
            for mut segment in nested {
                segment.spliced.start += offset;
                segment.spliced.end += offset;
                if segment.spliced.start < segment.spliced.end {
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
        (text, segments, gaps)
    }

    #[allow(clippy::too_many_arguments)]
    fn unroll_loop_includes(
        &self,
        key: &str,
        source: &str,
        loops: &[crate::macro_expand::ContainingLoop],
        group: &[Span],
        sites: &HashSet<(String, Span)>,
        targets: &IncludeTargets,
        stack: &mut Vec<String>,
    ) -> (
        String,
        Vec<SpliceSegment>,
        Vec<crate::macro_expand::SourceLayoutGap>,
    ) {
        let Some(outer) = loops.first() else {
            return (String::new(), Vec::new(), Vec::new());
        };
        let keys = iteration_keys(key, &outer.variables, group, targets);
        let mut text = String::new();
        let mut segments = Vec::new();
        let mut gaps = Vec::new();
        for iteration in keys {
            let bindings = iteration
                .iter()
                .zip(outer.variables.iter())
                .map(|(value, name)| (name.clone(), value.clone()))
                .collect::<Vec<_>>();
            push_generated(
                &mut text,
                &mut segments,
                &one_shot_header(&outer.variables, &bindings),
                key,
                outer.header,
                source,
            );
            self.copy_loop_body(
                key,
                source,
                outer.body_start,
                outer.body_end,
                &outer.variables,
                &iteration,
                &loops[1..],
                &bindings,
                sites,
                targets,
                stack,
                &mut text,
                &mut segments,
                &mut gaps,
            );
            push_generated(
                &mut text,
                &mut segments,
                "@#endfor\n",
                key,
                outer.closer,
                source,
            );
        }
        (text, segments, gaps)
    }

    #[allow(clippy::too_many_arguments)]
    fn copy_loop_body(
        &self,
        key: &str,
        source: &str,
        from: u32,
        to: u32,
        outer_vars: &[String],
        iteration: &[String],
        inner_loops: &[crate::macro_expand::ContainingLoop],
        bindings: &[(String, String)],
        sites: &HashSet<(String, Span)>,
        targets: &IncludeTargets,
        stack: &mut Vec<String>,
        text: &mut String,
        segments: &mut Vec<SpliceSegment>,
        gaps: &mut Vec<crate::macro_expand::SourceLayoutGap>,
    ) {
        let mut cuts: Vec<(u32, u32, BodyCut)> = Vec::new();
        for loop_dir in inner_loops {
            if loop_dir.header.start >= from && loop_dir.header.end <= to {
                cuts.push((
                    loop_dir.header.start,
                    loop_dir.header.end,
                    BodyCut::Header(one_shot_header(&loop_dir.variables, bindings)),
                ));
            }
        }
        for (file, span) in sites {
            if file != key || span.start < from || span.end > to {
                continue;
            }
            let Some(plan) = targets.get(&(key.to_string(), *span)) else {
                continue;
            };
            if plan.unresolved {
                continue;
            }
            let matched: Vec<_> = plan
                .hits
                .iter()
                .filter(|hit| hit_key(outer_vars, &hit.bindings) == iteration)
                .collect();
            let path = if matched.len() == 1 {
                Some(matched[0].path.clone())
            } else if plan.hits.len() == 1 {
                Some(plan.hits[0].path.clone())
            } else {
                None
            };
            let Some(path) = path else {
                continue;
            };
            cuts.push((span.start, span.end, BodyCut::Child(path)));
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
            );
            match cut {
                BodyCut::Header(header) => {
                    push_generated(text, segments, &header, key, Span { start, end }, source);
                }
                BodyCut::Child(path) => {
                    let (body, nested, nested_gaps) =
                        self.splice_with_map(&path, sites, targets, stack);
                    let offset = text.len() as u32;
                    for mut segment in nested {
                        segment.spliced.start += offset;
                        segment.spliced.end += offset;
                        if segment.spliced.start < segment.spliced.end {
                            segments.push(segment);
                        }
                    }
                    for gap in nested_gaps {
                        gaps.push(gap.shift(offset));
                    }
                    text.push_str(&body);
                    if (end as usize) > (start as usize)
                        && source.as_bytes().get((end as usize) - 1) == Some(&b'\n')
                        && !body.ends_with('\n')
                        && !body.ends_with('\r')
                    {
                        let at = text.len() as u32;
                        text.push('\n');
                        gaps.push(crate::macro_expand::SourceLayoutGap::SyntheticNewline(
                            crate::span::Span {
                                start: at,
                                end: at + 1,
                            },
                        ));
                    }
                }
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
        );
    }
}

enum BodyCut {
    Header(String),
    Child(String),
}

/// Values of the loop variables on one executed iteration, in variable order.
fn hit_key(variables: &[String], bindings: &[(String, String)]) -> Vec<String> {
    variables
        .iter()
        .map(|name| {
            bindings
                .iter()
                .find(|(bound, _)| bound == name)
                .map(|(_, value)| value.clone())
                .unwrap_or_default()
        })
        .collect()
}

/// Loop iterations in execution order. The site that ran most often supplies
/// the order; a site skipped by `@#if` only adds iterations it actually ran.
fn iteration_keys(
    file: &str,
    variables: &[String],
    group: &[Span],
    targets: &IncludeTargets,
) -> Vec<Vec<String>> {
    let mut best: Option<Span> = None;
    let mut best_len = 0usize;
    for span in group {
        let len = targets
            .get(&(file.to_string(), *span))
            .map(|plan| plan.hits.len())
            .unwrap_or(0);
        if len > best_len {
            best_len = len;
            best = Some(*span);
        }
    }
    let mut keys = Vec::new();
    let mut push_span = |span: Span| {
        let Some(plan) = targets.get(&(file.to_string(), span)) else {
            return;
        };
        for hit in &plan.hits {
            let key = hit_key(variables, &hit.bindings);
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    };
    if let Some(span) = best {
        push_span(span);
    }
    for span in group {
        push_span(*span);
    }
    keys
}

fn one_shot_header(variables: &[String], bindings: &[(String, String)]) -> String {
    let value_of = |name: &str| {
        bindings
            .iter()
            .find(|(bound, _)| bound == name)
            .map(|(_, value)| value.as_str())
            .unwrap_or("0")
    };
    if variables.len() == 1 {
        format!("@#for {} in [{}]\n", variables[0], value_of(&variables[0]))
    } else if variables.is_empty() {
        "@#for _i in [0]\n".to_string()
    } else {
        let names = variables.join(", ");
        let values = variables
            .iter()
            .map(|name| value_of(name))
            .collect::<Vec<_>>()
            .join(", ");
        format!("@#for ({names}) in [({values})]\n")
    }
}

fn push_generated(
    out: &mut String,
    segments: &mut Vec<SpliceSegment>,
    piece: &str,
    file: &str,
    origin: Span,
    source: &str,
) {
    if piece.is_empty() {
        return;
    }
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
    });
}

fn apply_replacements_mapped(
    source: &str,
    replacements: &[(
        Span,
        String,
        Vec<SpliceSegment>,
        Vec<crate::macro_expand::SourceLayoutGap>,
    )],
    file: Option<String>,
) -> (
    String,
    Vec<SpliceSegment>,
    Vec<crate::macro_expand::SourceLayoutGap>,
) {
    let mut out = String::with_capacity(source.len());
    let mut last = 0usize;
    let mut ordered = replacements.to_vec();
    ordered.sort_by_key(|(span, _, _, _)| span.start);
    let mut segments = Vec::new();
    let mut gaps = Vec::new();
    // Directive trailing newlines land at these spliced offsets once copied.
    let mut newline_marks: Vec<(u32, bool)> = Vec::new();
    for (span, body, nested, nested_gaps) in ordered {
        let start = span.start as usize;
        let end = span.end as usize;
        if start < last || end > source.len() || start > source.len() {
            continue;
        }
        let line = crate::macro_expand::directive_line_extent(source, span);
        push_root_piece(
            &mut out,
            &mut segments,
            source,
            last,
            start,
            file.as_deref(),
        );
        let indent_from = (line.start as usize).max(last);
        if indent_from < start {
            let indent_len = (start - indent_from) as u32;
            let indent_end = out.len() as u32;
            gaps.push(crate::macro_expand::SourceLayoutGap::Omit(Span {
                start: indent_end - indent_len,
                end: indent_end,
            }));
        }
        let offset = out.len() as u32;
        for mut seg in nested {
            seg.spliced.start += offset;
            seg.spliced.end += offset;
            if seg.spliced.start < seg.spliced.end {
                segments.push(seg);
            }
        }
        for gap in nested_gaps {
            gaps.push(gap.shift(offset));
        }
        out.push_str(&body);
        let consumed_newline = end > start && source.as_bytes()[end - 1] == b'\n';
        let following_newline = end < source.len() && source.as_bytes()[end] == b'\n';
        if following_newline {
            let synthetic = !body.is_empty() && !body.ends_with('\n') && !body.ends_with('\r');
            newline_marks.push((out.len() as u32, synthetic));
        } else if consumed_newline && !body.ends_with('\n') && !body.ends_with('\r') {
            // The directive span owns the line break. Put it back so the next
            // written line stays on its own line, including an empty include.
            let at = out.len() as u32;
            out.push('\n');
            newline_marks.push((at, !body.is_empty()));
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
    );
    for (at, synthetic) in newline_marks {
        if (at as usize) < out.len() && out.as_bytes()[at as usize] == b'\n' {
            let span = Span {
                start: at,
                end: at + 1,
            };
            gaps.push(if synthetic {
                crate::macro_expand::SourceLayoutGap::SyntheticNewline(span)
            } else {
                crate::macro_expand::SourceLayoutGap::Omit(span)
            });
        }
    }
    gaps.sort_by_key(|gap| gap.span().start);
    (out, segments, gaps)
}

fn push_root_piece(
    out: &mut String,
    segments: &mut Vec<SpliceSegment>,
    source: &str,
    from: usize,
    to: usize,
    file: Option<&str>,
) {
    if from >= to {
        return;
    }
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
    });
    out.push_str(&source[from..to]);
}

/// Overlay key: `\` becomes `/`, trailing slashes drop. No disk and no `..` fold.
fn overlay_key(key: &str) -> String {
    key.replace('\\', "/").trim_end_matches('/').to_string()
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
        .map(|idx| normalized[..idx].to_string())
}

fn overlay_join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
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
    // Directory lookup may collapse `.` without changing supplied map keys or
    // folding `..`, which remains a distinct map path.
    let mut lookup = joined;
    while lookup.starts_with("./") {
        lookup.drain(..2);
    }
    while lookup.contains("/./") {
        lookup = lookup.replace("/./", "/");
    }
    if lookup == "." {
        lookup.clear();
    } else if lookup.ends_with("/.") {
        lookup.truncate(lookup.len() - 2);
    }
    lookup
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

fn read_text(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    match String::from_utf8(bytes.clone()) {
        Ok(s) => Some(s),
        Err(_) => Some(bytes.iter().map(|&b| b as char).collect()),
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
