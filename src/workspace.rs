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
use crate::model::{IncludeDirective, IncludePathDirective, Model};
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

struct IncludeStack {
    files: Vec<String>,
    edges: Vec<IncludeSite>,
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
    includepath_dirs: Vec<PathBuf>,
}

/// One joined include source and its written-file map. The model, expand view,
/// and diagnostic locations all read this same snapshot until invalidation.
struct SplicedSource {
    text: String,
    segments: Vec<SpliceSegment>,
    includes_complete: bool,
    navigation: NavigationSource,
}

type NavigationTargets = HashMap<(String, Span), Option<String>>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct InputFile {
    overlay: bool,
    bytes: Option<Vec<u8>>,
}

/// URI/path-keyed document index plus include graph walks.
#[derive(Default)]
pub struct Workspace {
    docs: HashMap<String, Doc>,
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
        let includepath_dirs = overlay_includepath_dirs_for(key, &model);
        self.docs.insert(
            key.to_string(),
            Doc {
                source,
                model,
                overlay: true,
                includepath_dirs,
            },
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
        let includepath_dirs = includepath_dirs_for(&key, &model);
        self.docs.insert(
            key.clone(),
            Doc {
                source,
                model,
                overlay: true,
                includepath_dirs,
            },
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
        let includepath_dirs = includepath_dirs_for(&key, &model);
        self.docs.insert(
            key.clone(),
            Doc {
                source,
                model,
                overlay: false,
                includepath_dirs,
            },
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

    fn input_file(&self, key: &str) -> InputFile {
        if let Some(doc) = self.docs.get(key).filter(|doc| doc.overlay) {
            InputFile {
                overlay: true,
                bytes: Some(doc.source.as_bytes().to_vec()),
            }
        } else {
            InputFile {
                overlay: false,
                bytes: if is_virtual_uri(key) {
                    None
                } else {
                    std::fs::read(key).ok()
                },
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

    /// Parse of the spliced source: resolved include bodies in place of
    /// directives; unresolved and cyclic edges left empty.
    pub fn get_effective_model(&mut self, uri: &str) -> Option<&Model> {
        let key = self.ensure_loaded(uri)?;
        if !self.effective.contains_key(&key) {
            let spliced = self.spliced_source(&key);
            let model = parse(&spliced.text);
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
            let report = expand_report_from_spliced(
                &spliced.text,
                &spliced.segments,
                Some(&spliced.navigation),
            );
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

    /// Include records for slice 12 (spans, resolved path or unresolved, cycles).
    pub fn include_records(&mut self, uri: &str) -> Option<&IncludeRecords> {
        let key = self.ensure_loaded(uri)?;
        if !self.records.contains_key(&key) {
            let records = self.walk_graph(&key);
            self.records.insert(key.clone(), records);
        }
        self.records.get(&key)
    }

    /// Whether the required include expansion is proven complete. Raw graph
    /// diagnostics remain unchanged, including inactive written include sites.
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
        if self.overlay_only {
            let paths = self
                .docs
                .get(root_key)
                .map(|doc| doc.includepath_dirs.as_slice())
                .unwrap_or_default();
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
        if let Some(doc) = self.docs.get(root_key) {
            paths = append_unique(&paths, &doc.includepath_dirs);
        }
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
        let Some(doc) = self.docs.get(root_key) else {
            return Vec::new();
        };
        let source = doc.model.source.clone();
        let model = doc.model.clone();
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

    fn walk_graph(&mut self, root_key: &str) -> IncludeRecords {
        self.dependency_candidates.remove(root_key);
        let mut records = IncludeRecords::default();
        let mut seen_cycles: HashSet<Vec<String>> = HashSet::new();
        self.dfs_graph(
            root_key,
            &mut IncludeStack {
                files: vec![root_key.to_string()],
                edges: Vec::new(),
            },
            &mut Vec::new(),
            None,
            &mut records,
            &mut seen_cycles,
        );
        let included = records
            .resolved
            .iter()
            .map(|record| self.include_key(&record.path))
            .collect();
        self.include_owners.insert(root_key.to_owned(), included);
        records
    }

    fn dfs_graph(
        &mut self,
        current_key: &str,
        stack: &mut IncludeStack,
        active_search: &mut Vec<PathBuf>,
        root_span: Option<Span>,
        records: &mut IncludeRecords,
        seen_cycles: &mut HashSet<Vec<String>>,
    ) {
        if self.model_for_key(current_key).is_none() {
            return;
        }
        let events = {
            let model = self.docs.get(current_key).unwrap();
            ordered_events(&model.model)
        };
        for event in events {
            match event {
                IncludeEvent::IncludePath(dir) => {
                    // The first entry remains the root invocation directory.
                    let added = self.directive_search_paths(&stack.files[0], &dir);
                    *active_search = append_unique(active_search, &added);
                }
                IncludeEvent::Include(dir) => {
                    let resolved = self.resolve_filename(
                        &stack.files[0],
                        current_key,
                        &dir.filename,
                        active_search,
                    );
                    match resolved {
                        None => {
                            let mut searched = Vec::new();
                            if let Some(parent) = Path::new(current_key).parent() {
                                searched.push(parent.display().to_string());
                            }
                            for p in self.configured_search(&stack.files[0], active_search) {
                                let s = p.display().to_string();
                                if !searched.iter().any(|d| d == &s) {
                                    searched.push(s);
                                }
                            }
                            records.unresolved.push(UnresolvedInclude {
                                filename: dir.filename,
                                span: root_span.unwrap_or(dir.span),
                                included_from: root_span.map(|_| file_basename(current_key)),
                                searched,
                            });
                        }
                        Some(path) => {
                            let resolved_key = self.include_key(&path);
                            if let Some(idx) = stack.files.iter().position(|k| k == &resolved_key) {
                                let mut cycle: Vec<String> = stack.files[idx..].to_vec();
                                cycle.push(resolved_key);
                                let rotation: Vec<String> = cycle[..cycle.len() - 1].to_vec();
                                if let Some(min_idx) = rotation
                                    .iter()
                                    .enumerate()
                                    .min_by_key(|(_, k)| *k)
                                    .map(|(i, _)| i)
                                {
                                    let mut canonical = rotation[min_idx..].to_vec();
                                    canonical.extend(rotation[..min_idx].iter().cloned());
                                    if seen_cycles.insert(canonical) {
                                        records.cycles.push(CycleRecord {
                                            chain: cycle,
                                            span: root_span.unwrap_or(dir.span),
                                            earlier: stack
                                                .edges
                                                .get(idx.saturating_sub(1))
                                                .cloned()
                                                .unwrap_or_else(|| IncludeSite {
                                                    file: current_key.to_string(),
                                                    span: dir.span,
                                                }),
                                            closing: IncludeSite {
                                                file: current_key.to_string(),
                                                span: dir.span,
                                            },
                                        });
                                    }
                                }
                                continue;
                            }
                            records.resolved.push(ResolvedInclude {
                                filename: dir.filename.clone(),
                                span: dir.span,
                                path: path.clone(),
                            });
                            let nested_root = root_span.or(Some(dir.span));
                            stack.files.push(resolved_key.clone());
                            stack.edges.push(IncludeSite {
                                file: current_key.to_string(),
                                span: dir.span,
                            });
                            self.dfs_graph(
                                &resolved_key,
                                stack,
                                active_search,
                                nested_root,
                                records,
                                seen_cycles,
                            );
                            stack.files.pop();
                            stack.edges.pop();
                        }
                    }
                }
            }
        }
    }

    fn spliced_source(&mut self, key: &str) -> Arc<SplicedSource> {
        if let Some(source) = self.spliced.get(key) {
            return Arc::clone(source);
        }
        // Resolving includes may load documents and invalidate other cached
        // roots. Insert only after the full walk has finished.
        let raw_complete = self
            .include_records(key)
            .is_some_and(|records| records.unresolved.is_empty() && records.cycles.is_empty());
        let mut include_targets = HashMap::new();
        let (text, segments) = self.splice_with_map(
            key,
            &mut Vec::new(),
            &mut Vec::new(),
            false,
            &mut include_targets,
        );
        // Reuse the exact targets already resolved by the legacy splice. Only
        // executed includes load a raw file in this separate navigation proof.
        let navigation_proof = self.docs.get(key).map(|document| {
            crate::macro_expand::navigation_macros_complete(key, &document.source, |file, span| {
                let target = include_targets.get(&(file.to_string(), span))?.as_ref()?;
                let source = self.docs.get(target)?.source.clone();
                Some((target.clone(), source))
            })
        });
        let navigation = navigation_proof
            .filter(|proof| proof.complete)
            .and_then(|proof| {
                self.navigation_splice(key, &proof.sites, &include_targets, &mut Vec::new())
            })
            .map(|(text, segments)| NavigationSource::Mapped { text, segments })
            .unwrap_or(NavigationSource::Unavailable);
        let includes_complete =
            if raw_complete && !crate::macro_expand::has_include_directives(&text) {
                true
            } else {
                // Only this metadata pass retains failed directives, so the macro
                // visitor can distinguish required sites from known false branches.
                let (activity, _) = self.splice_with_map(
                    key,
                    &mut Vec::new(),
                    &mut Vec::new(),
                    true,
                    &mut HashMap::new(),
                );
                crate::macro_expand::required_includes_complete(&activity)
            };
        let source = Arc::new(SplicedSource {
            text,
            segments,
            includes_complete,
            navigation,
        });
        self.spliced.insert(key.to_string(), Arc::clone(&source));
        source
    }

    fn splice_with_map(
        &mut self,
        key: &str,
        stack: &mut Vec<String>,
        active_search: &mut Vec<PathBuf>,
        keep_unresolved: bool,
        include_targets: &mut NavigationTargets,
    ) -> (String, Vec<SpliceSegment>) {
        if stack.iter().any(|k| k == key) {
            return (String::new(), Vec::new());
        }
        let Some(source) = self.source_for_key(key) else {
            return (String::new(), Vec::new());
        };
        let root_key = stack.first().map(String::as_str).unwrap_or(key).to_string();
        let mut all: Vec<SpliceEvent> = {
            let Some(doc) = self.docs.get(key) else {
                return identity_splice(&source, Some(key.to_string()));
            };
            doc.model
                .includes
                .iter()
                .cloned()
                .map(SpliceEvent::Include)
                .chain(
                    doc.model
                        .includepaths
                        .iter()
                        .cloned()
                        .map(SpliceEvent::IncludePath),
                )
                .collect()
        };
        all.sort_by_key(|e| match e {
            SpliceEvent::Include(d) => d.span.start,
            SpliceEvent::IncludePath(d) => d.span.start,
        });
        let mut replacements: Vec<(Span, String, Vec<SpliceSegment>)> = Vec::new();
        for event in all {
            match event {
                SpliceEvent::IncludePath(dir) => {
                    let added = self.directive_search_paths(&root_key, &dir);
                    *active_search = append_unique(active_search, &added);
                }
                SpliceEvent::Include(dir) => {
                    let resolved =
                        self.resolve_filename(&root_key, key, &dir.filename, active_search);
                    let (body, nested_map) = match resolved {
                        None if keep_unresolved => identity_splice(
                            &source[dir.span.start as usize..dir.span.end as usize],
                            None,
                        ),
                        None => (String::new(), Vec::new()),
                        Some(path) => {
                            let resolved_key = self.include_key(&path);
                            include_targets
                                .entry((key.to_string(), dir.span))
                                .and_modify(|target| {
                                    if target.as_deref() != Some(resolved_key.as_str()) {
                                        *target = None;
                                    }
                                })
                                .or_insert_with(|| Some(resolved_key.clone()));
                            if stack.iter().any(|k| k == &resolved_key) || resolved_key == key {
                                if keep_unresolved {
                                    identity_splice(
                                        &source[dir.span.start as usize..dir.span.end as usize],
                                        None,
                                    )
                                } else {
                                    (String::new(), Vec::new())
                                }
                            } else {
                                stack.push(key.to_string());
                                let nested = self.splice_with_map(
                                    &resolved_key,
                                    stack,
                                    active_search,
                                    keep_unresolved,
                                    include_targets,
                                );
                                stack.pop();
                                nested
                            }
                        }
                    };
                    replacements.push((dir.span, body, nested_map));
                }
            }
        }
        apply_replacements_mapped(&source, &replacements, Some(key.to_string()))
    }

    /// Metadata projection over already resolved targets. Dormant directives
    /// stay in their caller's source; their file bodies cannot affect the proof.
    fn navigation_splice(
        &self,
        key: &str,
        sites: &HashSet<(String, Span)>,
        targets: &NavigationTargets,
        stack: &mut Vec<String>,
    ) -> Option<(String, Vec<SpliceSegment>)> {
        if stack.iter().any(|file| file == key) {
            return None;
        }
        let document = self.docs.get(key)?;
        stack.push(key.to_string());
        let mut replacements = Vec::new();
        for directive in &document.model.includes {
            let site = (key.to_string(), directive.span);
            if sites.contains(&site) {
                let target = targets.get(&site)?.as_deref()?;
                let (body, segments) = self.navigation_splice(target, sites, targets, stack)?;
                replacements.push((directive.span, body, segments));
            }
        }
        stack.pop();
        Some(apply_replacements_mapped(
            &document.source,
            &replacements,
            Some(key.to_string()),
        ))
    }

    fn directive_search_paths(&self, key: &str, directive: &IncludePathDirective) -> Vec<PathBuf> {
        if self.overlay_only {
            overlay_includepath_paths(key, directive)
        } else {
            includepath_paths(key, directive)
        }
    }
}

enum IncludeEvent {
    Include(IncludeDirective),
    IncludePath(IncludePathDirective),
}

enum SpliceEvent {
    Include(IncludeDirective),
    IncludePath(IncludePathDirective),
}

fn ordered_events(model: &Model) -> Vec<IncludeEvent> {
    let mut events: Vec<IncludeEvent> = model
        .includes
        .iter()
        .cloned()
        .map(IncludeEvent::Include)
        .chain(
            model
                .includepaths
                .iter()
                .cloned()
                .map(IncludeEvent::IncludePath),
        )
        .collect();
    events.sort_by_key(|e| match e {
        IncludeEvent::Include(d) => d.span.start,
        IncludeEvent::IncludePath(d) => d.span.start,
    });
    events
}

fn identity_splice(source: &str, file: Option<String>) -> (String, Vec<SpliceSegment>) {
    let segs = if source.is_empty() {
        Vec::new()
    } else {
        vec![SpliceSegment {
            spliced: Span::new(0, source.len()),
            file,
            origin: Span::new(0, source.len()),
        }]
    };
    (source.to_string(), segs)
}

fn apply_replacements_mapped(
    source: &str,
    replacements: &[(Span, String, Vec<SpliceSegment>)],
    file: Option<String>,
) -> (String, Vec<SpliceSegment>) {
    let mut out = String::with_capacity(source.len());
    let mut last = 0usize;
    let mut ordered = replacements.to_vec();
    ordered.sort_by_key(|(span, _, _)| span.start);
    let mut segments = Vec::new();
    for (span, body, nested) in ordered {
        let start = span.start as usize;
        let end = span.end as usize;
        if start < last || end > source.len() || start > source.len() {
            continue;
        }
        push_root_piece(
            &mut out,
            &mut segments,
            source,
            last,
            start,
            file.as_deref(),
        );
        let offset = out.len() as u32;
        for mut seg in nested {
            seg.spliced.start += offset;
            seg.spliced.end += offset;
            if seg.spliced.start < seg.spliced.end {
                segments.push(seg);
            }
        }
        out.push_str(&body);
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
    (out, segments)
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
    segments.push(SpliceSegment {
        spliced: Span {
            start: offset,
            end: offset + len,
        },
        file: file.map(str::to_string),
        origin: Span::new(from, to),
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

fn includepath_dirs_for(key: &str, model: &Model) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for dir in &model.includepaths {
        for p in includepath_paths(key, dir) {
            // A resolved path that is not a directory never helps an include.
            if !p.is_dir() {
                continue;
            }
            if !paths.iter().any(|e| e == &p) {
                paths.push(p);
            }
        }
    }
    paths
}

fn overlay_includepath_dirs_for(key: &str, model: &Model) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for directive in &model.includepaths {
        paths = append_unique(&paths, &overlay_includepath_paths(key, directive));
    }
    paths
}

fn overlay_includepath_paths(key: &str, directive: &IncludePathDirective) -> Vec<PathBuf> {
    includepath_literal(&directive.argument)
        .into_iter()
        .map(|raw| PathBuf::from(overlay_includepath_key(key, &raw)))
        .collect()
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

fn includepath_paths(key: &str, directive: &IncludePathDirective) -> Vec<PathBuf> {
    includepath_literal(&directive.argument)
        .map(|raw| resolve_includepath(key, &raw))
        .into_iter()
        .collect()
}

/// Resolve one `@#includepath` argument against the invocation root's parent.
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
