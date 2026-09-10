//! Path resolution for Dynare `@#include` directives.
//!
//! Search order matches Python `resolve_include_path` (not a line-for-line port):
//! absolute path; including-file directory; configured `search_paths`; then
//! virtual `known_paths` (exact, then unique suffix).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Normalize a directive filename's separators (`\` → `/`).
pub fn normalize_separators(filename: &str) -> String {
    filename.replace('\\', "/")
}

/// Coerce a `file://` URI or plain path to a [`PathBuf`].
pub fn uri_to_path(uri_or_path: &str) -> PathBuf {
    if let Some(after) = uri_or_path.strip_prefix("file://") {
        let decoded = percent_decode(after);
        let path = strip_file_netloc(&decoded);
        let path = strip_windows_drive_slash(&path);
        return PathBuf::from(path);
    }
    PathBuf::from(uri_or_path)
}

/// Canonical dictionary key for a workspace path (`normcase` on Windows).
pub fn path_key(path: &Path) -> String {
    let absolute = make_absolute(path);
    normcase(&absolute)
}

/// Normalize a URI or path string to a [`path_key`].
pub fn normalize_uri(uri_or_path: &str) -> String {
    path_key(&uri_to_path(uri_or_path))
}

/// Walk up from `start_path` to the nearest directory containing `.git`.
///
/// If none is found, the filesystem root is returned.
pub fn find_workspace_root(start_path: &Path) -> PathBuf {
    let mut current = make_absolute(start_path);
    if current.is_file() {
        if let Some(parent) = current.parent() {
            current = parent.to_path_buf();
        }
    }
    loop {
        if current.join(".git").exists() {
            return current;
        }
        match current.parent() {
            Some(parent) if parent != current => current = parent.to_path_buf(),
            _ => return current,
        }
    }
}

/// Resolve a literal `@#include` filename to an absolute path, or `None`.
pub fn resolve_include_path(
    directive_filename: &str,
    including_file_uri_or_path: &str,
    search_paths: &[PathBuf],
    known_paths: Option<&HashSet<String>>,
) -> Option<PathBuf> {
    if directive_filename.is_empty() {
        return None;
    }
    let known = known_paths.cloned().unwrap_or_default();
    let normalized = normalize_separators(directive_filename);
    let candidate_rel = PathBuf::from(&normalized);

    if is_abs_path(&candidate_rel) {
        return if matches_candidate(&candidate_rel, &known) {
            Some(resolve_or_abs(&candidate_rel))
        } else {
            None
        };
    }

    let including_path = uri_to_path(including_file_uri_or_path);
    let including_dir = if including_path.is_file() || has_suffix(&including_path) {
        including_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or(including_path)
    } else {
        including_path
    };

    let sibling = including_dir.join(&candidate_rel);
    if matches_candidate(&sibling, &known) {
        return Some(resolve_or_abs(&sibling));
    }

    for sp in search_paths {
        let candidate = sp.join(&candidate_rel);
        if matches_candidate(&candidate, &known) {
            return Some(resolve_or_abs(&candidate));
        }
    }

    if matches_known(&candidate_rel, &known) {
        return Some(resolve_or_abs(&candidate_rel));
    }
    match_unique_known_relative_path(&candidate_rel, &known)
}

fn matches_candidate(p: &Path, known: &HashSet<String>) -> bool {
    // A directory with the right name is not a Dynare include target.
    if p.is_file() {
        return true;
    }
    known_contains(known, p)
}

fn matches_known(p: &Path, known: &HashSet<String>) -> bool {
    known_contains(known, p)
}

fn known_contains(known: &HashSet<String>, p: &Path) -> bool {
    let key = path_key(p);
    if known.contains(&key) {
        return true;
    }
    known.contains(&normcase(p))
}

fn match_unique_known_relative_path(p: &Path, known: &HashSet<String>) -> Option<PathBuf> {
    let target_parts = path_parts(p);
    if target_parts.is_empty() {
        return None;
    }
    let mut matches: Vec<PathBuf> = Vec::new();
    let mut seen = HashSet::new();
    for entry in known {
        let known_path = uri_to_path(entry);
        if known_path.is_file() {
            continue;
        }
        let known_parts = path_parts(&known_path);
        if known_parts.len() < target_parts.len() {
            continue;
        }
        let suffix = &known_parts[known_parts.len() - target_parts.len()..];
        if !parts_equal(suffix, &target_parts) {
            continue;
        }
        let resolved = resolve_or_abs(&known_path);
        let key = path_key(&resolved);
        if seen.insert(key) {
            matches.push(resolved);
        }
    }
    if matches.len() == 1 {
        matches.pop()
    } else {
        None
    }
}

fn parts_equal(left: &[String], right: &[String]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    if cfg!(windows) {
        left.iter()
            .zip(right)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
    } else {
        left == right
    }
}

fn path_parts(p: &Path) -> Vec<String> {
    p.iter().map(|s| s.to_string_lossy().into_owned()).collect()
}

fn is_abs_path(p: &Path) -> bool {
    if p.is_absolute() {
        return true;
    }
    let s = p.to_string_lossy();
    let b = s.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'/' || b[2] == b'\\')
}

fn has_suffix(p: &Path) -> bool {
    p.extension().is_some()
}

fn make_absolute(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return strip_verbatim(canonical);
    }
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

fn resolve_or_abs(p: &Path) -> PathBuf {
    std::fs::canonicalize(p)
        .map(strip_verbatim)
        .unwrap_or_else(|_| make_absolute(p))
}

fn strip_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        p
    }
}

fn normcase(path: &Path) -> String {
    let s = path.to_string_lossy();
    if cfg!(windows) {
        s.replace('/', "\\").to_lowercase()
    } else {
        s.into_owned()
    }
}

fn strip_file_netloc(after_scheme: &str) -> String {
    if let Some(rest) = after_scheme.strip_prefix("//") {
        if rest.starts_with('/') {
            return rest.to_string();
        }
        if let Some(slash) = rest.find('/') {
            let (netloc, path) = rest.split_at(slash);
            if netloc.is_empty() || netloc.eq_ignore_ascii_case("localhost") {
                return path.to_string();
            }
            return format!("//{netloc}{path}");
        }
        return rest.to_string();
    }
    after_scheme.to_string()
}

fn strip_windows_drive_slash(path: &str) -> String {
    let b = path.as_bytes();
    if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
        path[1..].to_string()
    } else {
        path.to_string()
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
