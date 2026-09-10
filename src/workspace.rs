//! Workspace index for `@#include` resolution, cycles, and the effective model.
//!
//! Overlay (editor/MCP in-memory text) beats disk. Include records (paths,
//! spans, cycle chains, unresolved names) are stored here for slice 12; this
//! module does not emit diagnostic codes.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::include_resolver::{
    normalize_separators, normalize_uri, path_key, resolve_include_path, uri_to_path,
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
    records: HashMap<String, IncludeRecords>,
}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
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
        self.effective.remove(&key);
        self.records.remove(&key);
    }

    /// Drop this document so a later load can read disk again.
    ///
    /// Clears every cached effective model and include-record map: other
    /// roots may have spliced this file.
    pub fn remove_document(&mut self, uri: &str) {
        let key = normalize_uri(uri);
        self.docs.remove(&key);
        self.effective.clear();
        self.records.clear();
    }

    /// Read `path` from disk (utf-8, then latin-1) unless an overlay exists.
    pub fn load_from_disk(&mut self, path: &Path) -> Option<&Model> {
        let key = path_key(path);
        if self.docs.get(&key).is_some_and(|d| d.overlay) {
            return self.docs.get(&key).map(|d| &d.model);
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
        self.effective.remove(&key);
        self.records.remove(&key);
        self.docs.get(&key).map(|d| &d.model)
    }

    pub fn add_search_path(&mut self, path: PathBuf) {
        if !self.search_paths.iter().any(|p| p == &path) {
            self.search_paths.push(path);
        }
        self.effective.clear();
        self.records.clear();
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
        self.records.clear();
    }

    pub fn get_model(&self, uri: &str) -> Option<&Model> {
        self.docs.get(&normalize_uri(uri)).map(|d| &d.model)
    }

    pub fn get_source(&self, uri: &str) -> Option<&str> {
        self.docs
            .get(&normalize_uri(uri))
            .map(|d| d.source.as_str())
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
            let spliced = self.splice_key(&key, &mut Vec::new(), &[]);
            let model = parse(&spliced);
            self.effective.insert(key.clone(), model);
        }
        self.effective.get(&key)
    }

    /// Transitively included files (root excluded).
    pub fn resolve_all_includes(&mut self, uri: &str) -> HashMap<String, Model> {
        let records = self.include_records(uri).cloned().unwrap_or_default();
        let mut out = HashMap::new();
        let mut seen = HashSet::new();
        for resolved in &records.resolved {
            let key = path_key(&resolved.path);
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

    fn ensure_loaded(&mut self, uri: &str) -> Option<String> {
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
        let mut paths = self.configured_search(active_search);
        if let Some(doc) = self.docs.get(including_key) {
            paths = append_unique(&paths, &doc.includepath_dirs);
        }
        let known = self.known_keys();
        resolve_include_path(filename, including_key, &paths, Some(&known))
    }

    fn walk_graph(&mut self, root_key: &str) -> IncludeRecords {
        let mut records = IncludeRecords::default();
        let mut seen_cycles: HashSet<Vec<String>> = HashSet::new();
        self.dfs_graph(
            root_key,
            &mut vec![root_key.to_string()],
            &[],
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
        inherited_search: &[PathBuf],
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
        let mut side_paths: Vec<PathBuf> = Vec::new();
        if let Some(doc) = self.docs.get(current_key) {
            side_paths = doc.includepath_dirs.clone();
        }
        for event in events {
            match event {
                IncludeEvent::IncludePath(dir) => {
                    let added = includepath_paths(current_key, &dir);
                    side_paths = append_unique(&side_paths, &added);
                }
                IncludeEvent::Include(dir) => {
                    let effective_paths = append_unique(inherited_search, &side_paths);
                    let resolved =
                        self.resolve_filename(current_key, &dir.filename, &effective_paths);
                    match resolved {
                        None => {
                            records.unresolved.push(UnresolvedInclude {
                                filename: dir.filename,
                                span: root_span.unwrap_or(dir.span),
                                included_from: root_span.map(|_| file_basename(current_key)),
                            });
                        }
                        Some(path) => {
                            let resolved_key = path_key(&path);
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
                                &effective_paths,
                                nested_root,
                                records,
                                seen_cycles,
                            );
                            stack.pop();
                            if let Some(nested_doc) = self.docs.get(&resolved_key) {
                                side_paths =
                                    append_unique(&side_paths, &nested_doc.includepath_dirs);
                            }
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
        inherited_search: &[PathBuf],
    ) -> String {
        if stack.iter().any(|k| k == key) {
            return String::new();
        }
        let Some(source) = self.source_for_key(key) else {
            return String::new();
        };
        let mut side_paths: Vec<PathBuf> = self
            .docs
            .get(key)
            .map(|d| d.includepath_dirs.clone())
            .unwrap_or_default();
        let mut all: Vec<SpliceEvent> = {
            let Some(doc) = self.docs.get(key) else {
                return source;
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
        let mut replacements: Vec<(Span, String)> = Vec::new();
        for event in all {
            match event {
                SpliceEvent::IncludePath(dir) => {
                    let added = includepath_paths(key, &dir);
                    side_paths = append_unique(&side_paths, &added);
                }
                SpliceEvent::Include(dir) => {
                    let effective_paths = append_unique(inherited_search, &side_paths);
                    let resolved = self.resolve_filename(key, &dir.filename, &effective_paths);
                    let body = match resolved {
                        None => String::new(),
                        Some(path) => {
                            let resolved_key = path_key(&path);
                            if stack.iter().any(|k| k == &resolved_key) {
                                String::new()
                            } else {
                                stack.push(key.to_string());
                                let nested =
                                    self.splice_key(&resolved_key, stack, &effective_paths);
                                stack.pop();
                                if let Some(nested_doc) = self.docs.get(&resolved_key) {
                                    side_paths =
                                        append_unique(&side_paths, &nested_doc.includepath_dirs);
                                }
                                nested
                            }
                        }
                    };
                    replacements.push((dir.span, body));
                }
            }
        }
        apply_replacements(&source, &replacements)
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

fn apply_replacements(source: &str, replacements: &[(Span, String)]) -> String {
    let mut out = String::with_capacity(source.len());
    let mut last = 0usize;
    let mut ordered = replacements.to_vec();
    ordered.sort_by_key(|(span, _)| span.start);
    for (span, body) in ordered {
        let start = span.start as usize;
        let end = span.end as usize;
        if start < last || end > source.len() || start > source.len() {
            continue;
        }
        out.push_str(&source[last..start]);
        out.push_str(&body);
        last = end;
    }
    out.push_str(&source[last..]);
    out
}

fn includepath_dirs_for(key: &str, model: &Model) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for dir in &model.includepaths {
        for p in includepath_paths(key, dir) {
            if !paths.iter().any(|e| e == &p) {
                paths.push(p);
            }
        }
    }
    paths
}

fn includepath_paths(key: &str, directive: &IncludePathDirective) -> Vec<PathBuf> {
    let including_dir = Path::new(key).parent().map(Path::to_path_buf);
    let mut paths = Vec::new();
    for raw in split_includepath_argument(&directive.argument) {
        let path = PathBuf::from(normalize_separators(&raw));
        let path = if path.is_absolute() {
            path
        } else if let Some(dir) = &including_dir {
            dir.join(path)
        } else {
            path
        };
        let path = std::fs::canonicalize(&path)
            .map(strip_verbatim)
            .unwrap_or(path);
        if !paths.iter().any(|p| p == &path) {
            paths.push(path);
        }
    }
    paths
}

/// Colon-split that does not split Windows `C:/` drive prefixes.
pub fn split_includepath_argument(argument: &str) -> Vec<String> {
    let mut raw = argument.trim();
    if (raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2)
        || (raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2)
    {
        raw = &raw[1..raw.len() - 1];
    }
    if raw.is_empty() {
        return Vec::new();
    }
    let bytes = raw.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0;
    for i in 0..bytes.len() {
        if bytes[i] != b':' {
            continue;
        }
        if i == start + 1
            && bytes[start].is_ascii_alphabetic()
            && i + 1 < bytes.len()
            && (bytes[i + 1] == b'/' || bytes[i + 1] == b'\\')
        {
            continue;
        }
        parts.push(raw[start..i].trim().to_string());
        start = i + 1;
    }
    parts.push(raw[start..].trim().to_string());
    parts.into_iter().filter(|p| !p.is_empty()).collect()
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
