//! Workspace index for `@#include` resolution, companion records, and the
//! effective model.
//!
//! Overlay (editor/MCP in-memory text) beats disk. Include and companion
//! records are stored here. This module does not emit diagnostic codes.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::companion::{self, CompanionKind, CompanionRecord};
use crate::expand::{expand_report_from_spliced, ExpandReport, SpliceSegment};
use crate::include_resolver::{
    normalize_separators, normalize_uri, path_key, resolve_companion_path, resolve_include_path,
    uri_to_path,
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

/// URI/path-keyed document index plus include graph walks.
#[derive(Default)]
pub struct Workspace {
    docs: HashMap<String, Doc>,
    search_paths: Vec<PathBuf>,
    effective: HashMap<String, Model>,
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
        self.expand.clear();
        self.records.remove(key);
        self.companions.remove(key);
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
        self.expand.clear();
        self.records.remove(&key);
        self.companions.remove(&key);
    }

    /// Drop this document so a later load can read disk again.
    ///
    /// Clears every cached effective model, include-record map, and companion
    /// cache: other roots may have spliced this file.
    pub fn remove_document(&mut self, uri: &str) {
        let key = normalize_uri(uri);
        self.docs.remove(&key);
        self.effective.clear();
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
        self.expand.clear();
        self.records.remove(&key);
        self.companions.remove(&key);
        self.docs.get(&key).map(|d| &d.model)
    }

    pub fn add_search_path(&mut self, path: PathBuf) {
        if !self.search_paths.iter().any(|p| p == &path) {
            self.search_paths.push(path);
        }
        self.effective.clear();
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
        self.expand.clear();
        self.records.clear();
        self.companions.clear();
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

    /// Whether this workspace represents only caller-supplied map entries.
    pub(crate) fn is_overlay_only(&self) -> bool {
        self.overlay_only
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
            let spliced = self.splice_key(&key, &mut Vec::new(), &mut Vec::new());
            let model = parse(&spliced);
            self.effective.insert(key.clone(), model);
        }
        self.effective.get(&key)
    }

    /// Map one span in the include-spliced model back to the active root file.
    /// Included-file spans have no location in the root and return `None`.
    pub(crate) fn map_effective_span_to_root(&mut self, uri: &str, span: Span) -> Option<Span> {
        let key = self.ensure_loaded(uri)?;
        let (_, segments) = self.splice_with_map(&key, &mut Vec::new(), &mut Vec::new());
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
        let (_, segments) = self.splice_with_map(&key, &mut Vec::new(), &mut Vec::new());
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
            let (spliced, map) = self.splice_with_map(&key, &mut Vec::new(), &mut Vec::new());
            let report = expand_report_from_spliced(&spliced, &map);
            self.expand.insert(key.clone(), report);
        }
        self.expand.get(&key)
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

    fn configured_search(&self, extra: &[PathBuf]) -> Vec<PathBuf> {
        append_unique(&self.search_paths, extra)
    }

    fn resolve_filename(
        &self,
        including_key: &str,
        filename: &str,
        active_search: &[PathBuf],
    ) -> Option<PathBuf> {
        if self.overlay_only {
            return self
                .overlay_include_key(including_key, filename, active_search)
                .map(PathBuf::from);
        }
        let paths = self.configured_search(active_search);
        let known = self.known_keys();
        resolve_include_path(filename, including_key, &paths, Some(&known))
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
        &self,
        root_key: &str,
        name: &str,
        extra_suffixes: &[&str],
    ) -> Option<PathBuf> {
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
        let mut paths = self.search_paths.clone();
        if let Some(doc) = self.docs.get(root_key) {
            paths = append_unique(&paths, &doc.includepath_dirs);
        }
        let known = self.known_keys();
        resolve_companion_path(name, root_key, &paths, Some(&known), extra_suffixes)
    }

    fn build_companions(&self, root_key: &str) -> Vec<CompanionRecord> {
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
        let mut records = IncludeRecords::default();
        let mut seen_cycles: HashSet<Vec<String>> = HashSet::new();
        self.dfs_graph(
            root_key,
            &mut vec![root_key.to_string()],
            &mut Vec::new(),
            None,
            &mut records,
            &mut seen_cycles,
        );
        records
    }

    fn dfs_graph(
        &mut self,
        current_key: &str,
        stack: &mut Vec<String>,
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
                    let added = self.directive_search_paths(&stack[0], &dir);
                    *active_search = append_unique(active_search, &added);
                }
                IncludeEvent::Include(dir) => {
                    let resolved = self.resolve_filename(current_key, &dir.filename, active_search);
                    match resolved {
                        None => {
                            let mut searched = Vec::new();
                            if let Some(parent) = Path::new(current_key).parent() {
                                searched.push(parent.display().to_string());
                            }
                            for p in active_search.iter() {
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
                            if let Some(idx) = stack.iter().position(|k| k == &resolved_key) {
                                let mut cycle: Vec<String> = stack[idx..].to_vec();
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
                            stack.push(resolved_key.clone());
                            self.dfs_graph(
                                &resolved_key,
                                stack,
                                active_search,
                                nested_root,
                                records,
                                seen_cycles,
                            );
                            stack.pop();
                        }
                    }
                }
            }
        }
    }

    fn splice_key(
        &mut self,
        key: &str,
        stack: &mut Vec<String>,
        active_search: &mut Vec<PathBuf>,
    ) -> String {
        self.splice_with_map(key, stack, active_search).0
    }

    fn splice_with_map(
        &mut self,
        key: &str,
        stack: &mut Vec<String>,
        active_search: &mut Vec<PathBuf>,
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
                    let resolved = self.resolve_filename(key, &dir.filename, active_search);
                    let (body, nested_map) = match resolved {
                        None => (String::new(), Vec::new()),
                        Some(path) => {
                            let resolved_key = self.include_key(&path);
                            if stack.iter().any(|k| k == &resolved_key) {
                                (String::new(), Vec::new())
                            } else {
                                stack.push(key.to_string());
                                let nested =
                                    self.splice_with_map(&resolved_key, stack, active_search);
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
