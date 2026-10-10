//! Isolated comparison inputs shared by LSP and MCP. Source adapters acquire
//! bytes; this module and Workspace decide which includes execute.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::compare_navigation::{navigation_json, ComparisonInput, Coordinates};
use crate::model::Model;
use crate::model_diff::{compare_models_with_sources, CompareSource};
use crate::semantic_diff::{CaptureBoundary, SourceFilePair, SourceIdentityProof};
use crate::workspace::Workspace;

pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;
pub const SNAPSHOT_NAVIGATION_SCHEMA_VERSION: u32 = 2;
pub const MAX_SNAPSHOT_FILES: usize = 100_000;
pub const MAX_SNAPSHOT_SOURCE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, Hash)]
#[serde(deny_unknown_fields)]
pub struct ManifestEntry {
    pub mode: String,
    pub object_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Hash)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceFact {
    Text { text: String },
    Failure { code: String, message: String },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitSnapshotInput {
    pub input_id: String,
    pub root_file: String,
    pub repository_uri: String,
    pub commit: String,
    #[serde(default)]
    pub requested_ref: Option<String>,
    #[serde(default)]
    pub search_paths: Vec<String>,
    pub manifest: BTreeMap<String, ManifestEntry>,
    #[serde(default)]
    pub sources: BTreeMap<String, SourceFact>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SnapshotInput {
    Git(GitSnapshotInput),
    Working {
        input_id: String,
        root_uri: String,
        expected_revision: String,
        #[serde(default)]
        search_paths: Option<Vec<String>>,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotCompareRequest {
    pub schema_version: u32,
    pub before: SnapshotInput,
    pub after: SnapshotInput,
}

#[derive(Clone, Debug, Serialize)]
pub struct SnapshotFailure {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_key: Option<String>,
}

impl SnapshotFailure {
    pub fn new(code: &str, message: impl Into<String>, file_key: Option<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
            file_key,
        }
    }

    pub fn response(&self, side: &str) -> Value {
        let mut value = serde_json::to_value(self).expect("snapshot failure");
        value["state"] = json!("failure");
        value["side"] = json!(side);
        value
    }
}

/// Historical lookup facts are separate from loaded texts. A regular manifest
/// entry wins candidate selection even when its body has not been acquired.
#[derive(Default)]
pub(crate) struct SnapshotLookup {
    pub repository_path: String,
    pub manifest: BTreeMap<String, ManifestEntry>,
    pub facts: BTreeMap<String, SourceFact>,
    pub requested: BTreeSet<String>,
    pub failure: Option<SnapshotFailure>,
}

impl SnapshotLookup {
    pub fn path(&self, base: &str, name: &str) -> Option<String> {
        let absolute = |path: &str| path.starts_with('/') || path.as_bytes().get(1) == Some(&b':');
        let relative = |path: &str| {
            let prefix = format!("{}/", self.repository_path.trim_end_matches('/'));
            let matches = path.get(..prefix.len()).is_some_and(|value| {
                if cfg!(windows) || self.repository_path.as_bytes().get(1) == Some(&b':') {
                    value.eq_ignore_ascii_case(&prefix)
                } else {
                    value == prefix
                }
            });
            if path == self.repository_path {
                Some(String::new())
            } else if matches {
                Some(path[prefix.len()..].to_owned())
            } else {
                None
            }
        };
        let name = name.replace('\\', "/");
        let base = base.replace('\\', "/");
        if absolute(&name) {
            return tree_path("", &relative(&name)?);
        }
        let base = if absolute(&base) {
            relative(&base)?
        } else {
            base
        };
        tree_path(&base, &name)
    }

    pub fn contains(&mut self, key: &str) -> bool {
        if let Some(entry) = self.manifest.get(key) {
            if !matches!(entry.mode.as_str(), "100644" | "100755") {
                self.refuse(
                    key,
                    if entry.mode == "120000" {
                        "Historical symlink sources are unsupported"
                    } else {
                        "Historical gitlink sources are unsupported"
                    },
                );
            }
            return true;
        }
        if self.manifest.iter().any(|(path, entry)| {
            matches!(entry.mode.as_str(), "120000" | "160000")
                && key.starts_with(&format!("{path}/"))
        }) {
            self.refuse(key, "Historical include crosses a symlink or gitlink");
            return true;
        }
        false
    }

    pub fn refuse(&mut self, key: &str, message: &str) {
        if self.failure.is_none() {
            self.failure = Some(SnapshotFailure::new(
                "UNSUPPORTED_SOURCE",
                message,
                Some(key.to_owned()),
            ));
        }
    }

    pub fn request(&mut self, key: &str) {
        if self.failure.is_some() {
            return;
        }
        match self.facts.get(key) {
            Some(SourceFact::Failure { code, message }) => {
                self.failure = Some(SnapshotFailure::new(code, message, Some(key.to_owned())));
            }
            Some(SourceFact::Text { .. }) => {}
            None => {
                self.requested.insert(key.to_owned());
            }
        }
    }

    pub fn directory_exists(&self, directory: &str) -> bool {
        let prefix = if directory.is_empty() {
            String::new()
        } else {
            format!("{directory}/")
        };
        self.manifest.keys().any(|key| key.starts_with(&prefix))
    }
}

/// Resolve tree paths without consulting the host filesystem or changing case.
/// `None` means an absolute path or traversal outside the selected repository.
pub fn tree_path(base: &str, name: &str) -> Option<String> {
    let base = base.replace('\\', "/");
    let name = name.replace('\\', "/");
    if base.starts_with('/')
        || base.contains(':')
        || base.contains('\0')
        || name.starts_with('/')
        || name.contains(':')
        || name.contains('\0')
    {
        return None;
    }
    let mut parts = Vec::new();
    for part in base.split('/').chain(name.split('/')) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            value => parts.push(value),
        }
    }
    Some(parts.join("/"))
}

pub(crate) fn tree_parent(key: &str) -> &str {
    key.rsplit_once('/').map(|(parent, _)| parent).unwrap_or("")
}

pub enum SnapshotCapture {
    Ready(Box<CapturedSnapshot>),
    NeedsSources(Vec<String>),
    Failure(SnapshotFailure),
}

/// This input owns its workspace. Historical text never enters the editor's
/// diagnostics, owners, scans, or ordinary analysis caches.
pub struct CapturedSnapshot {
    workspace: Workspace,
    root: String,
    model: Model,
    input_id: String,
    revision: String,
    pub inputs: Value,
    pub sources: BTreeMap<String, String>,
    historical: bool,
}

pub fn capture_git_snapshot(input: &GitSnapshotInput) -> SnapshotCapture {
    let invalid = |message| {
        SnapshotCapture::Failure(SnapshotFailure::new("INVALID_ARGUMENTS", message, None))
    };
    if input.input_id.is_empty()
        || input.repository_uri.is_empty()
        || !matches!(input.commit.len(), 40 | 64)
        || !input.commit.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return invalid("Git snapshot needs an input ID, repository identity, and full commit ID");
    }
    if input.manifest.len() > MAX_SNAPSHOT_FILES
        || input
            .sources
            .values()
            .map(|fact| match fact {
                SourceFact::Text { text } => text.len(),
                SourceFact::Failure { .. } => 0,
            })
            .sum::<usize>()
            > MAX_SNAPSHOT_SOURCE_BYTES
    {
        return SnapshotCapture::Failure(SnapshotFailure::new(
            "CAPTURE_LIMIT",
            "Snapshot source limit reached",
            None,
        ));
    }
    if tree_path("", &input.root_file).as_deref() != Some(input.root_file.as_str())
        || input.root_file.is_empty()
        || !std::path::Path::new(&input.root_file)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("mod") || extension.eq_ignore_ascii_case("dyn")
            })
        || !input.manifest.iter().all(|(key, entry)| {
            !key.is_empty()
                && tree_path("", key).as_deref() == Some(key.as_str())
                && matches!(
                    entry.mode.as_str(),
                    "100644" | "100755" | "120000" | "160000"
                )
                && matches!(entry.object_id.len(), 40 | 64)
                && entry.object_id.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    {
        return invalid("Snapshot manifest and source keys must be exact repository-relative paths with valid Git modes and object IDs");
    }
    let Some(root_entry) = input.manifest.get(&input.root_file) else {
        return SnapshotCapture::Failure(SnapshotFailure::new(
            "ROOT_NOT_FOUND",
            "Model root is absent from the selected commit",
            Some(input.root_file.clone()),
        ));
    };
    if !matches!(root_entry.mode.as_str(), "100644" | "100755") {
        return SnapshotCapture::Failure(SnapshotFailure::new(
            "UNSUPPORTED_SOURCE",
            "Historical model root is a symlink or gitlink",
            Some(input.root_file.clone()),
        ));
    }
    if !input
        .sources
        .keys()
        .all(|key| input.manifest.contains_key(key))
    {
        return invalid("Snapshot source keys must belong to the manifest");
    }
    match input.sources.get(&input.root_file) {
        None => return SnapshotCapture::NeedsSources(vec![input.root_file.clone()]),
        Some(SourceFact::Failure { code, message }) => {
            return SnapshotCapture::Failure(SnapshotFailure::new(
                code,
                message,
                Some(input.root_file.clone()),
            ))
        }
        _ => {}
    }
    let mut workspace = Workspace::snapshot_documents(input);
    let Some(model) = workspace.get_effective_model(&input.root_file).cloned() else {
        return invalid("Snapshot root could not be read");
    };
    let lookup = workspace
        .snapshot_lookup
        .as_ref()
        .expect("historical lookup");
    if let Some(failure) = &lookup.failure {
        return SnapshotCapture::Failure(failure.clone());
    }
    if !lookup.requested.is_empty() {
        return SnapshotCapture::NeedsSources(lookup.requested.iter().cloned().collect());
    }
    if let Some(failure) = incomplete(&mut workspace, &input.root_file, &model) {
        return SnapshotCapture::Failure(failure);
    }
    let sources = workspace.snapshot_sources(&input.root_file);
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    input.repository_uri.hash(&mut hash);
    input.commit.hash(&mut hash);
    input.root_file.hash(&mut hash);
    input.search_paths.hash(&mut hash);
    sources.hash(&mut hash);
    for key in sources.keys() {
        input.manifest.get(key).hash(&mut hash);
    }
    let revision = format!("{:016x}", hash.finish());
    let inputs = json!({"kind":"git", "input_id":input.input_id, "root_file":input.root_file,
        "repository_uri":input.repository_uri, "commit":input.commit, "requested_ref":input.requested_ref,
        "revision":revision, "search_paths":input.search_paths, "complete":true,
        "source_policy":"git_tree", "file_keys":sources.keys().collect::<Vec<_>>(), "dependency_candidates":[]});
    SnapshotCapture::Ready(Box::new(CapturedSnapshot {
        workspace,
        root: input.root_file.clone(),
        model,
        input_id: input.input_id.clone(),
        revision,
        inputs,
        sources,
        historical: true,
    }))
}

/// Capture a caller-owned Working workspace. MCP supplies saved files; LSP
/// supplies an isolated overlay/disk workspace and checks its live revision.
pub fn capture_working_snapshot(
    mut workspace: Workspace,
    root_uri: &str,
    input_id: &str,
    source_policy: &str,
) -> SnapshotCapture {
    let Some(revision) = workspace.input_revision(root_uri) else {
        if !crate::include_resolver::is_virtual_uri(root_uri)
            && crate::include_resolver::uri_to_path(root_uri).is_file()
        {
            return SnapshotCapture::Failure(SnapshotFailure::new(
                "SOURCE_READ_FAILED",
                "Working model root could not be read",
                Some(root_uri.to_owned()),
            ));
        }
        return SnapshotCapture::Failure(SnapshotFailure::new(
            "ROOT_NOT_FOUND",
            "Working model root is unavailable",
            Some(root_uri.to_owned()),
        ));
    };
    let Some(model) = workspace.get_effective_model(root_uri).cloned() else {
        return SnapshotCapture::Failure(SnapshotFailure::new(
            "ROOT_NOT_FOUND",
            "Working model root could not be read",
            Some(root_uri.to_owned()),
        ));
    };
    if let Some(failure) = incomplete(&mut workspace, root_uri, &model) {
        return SnapshotCapture::Failure(failure);
    }
    if !workspace.input_snapshot_is_current(root_uri) {
        return SnapshotCapture::Failure(input_changed());
    }
    let sources = workspace.snapshot_sources(root_uri);
    let inputs = json!({"kind":"working", "input_id":input_id, "root_uri":root_uri,
        "root_file":crate::include_resolver::normalize_uri(root_uri), "revision":revision,
        "search_paths":workspace.snapshot_search_paths(root_uri), "complete":true, "source_policy":source_policy,
        "file_keys":sources.keys().collect::<Vec<_>>(),
        "dependency_candidates":workspace.input_candidate_paths(root_uri).iter().filter_map(|path| tower_lsp::lsp_types::Url::from_file_path(path).ok()).collect::<Vec<_>>()});
    SnapshotCapture::Ready(Box::new(CapturedSnapshot {
        workspace,
        root: root_uri.to_owned(),
        model,
        input_id: input_id.to_owned(),
        revision,
        inputs,
        sources,
        historical: false,
    }))
}

fn incomplete(workspace: &mut Workspace, root: &str, model: &Model) -> Option<SnapshotFailure> {
    crate::mcp::incomplete_model_status(model, workspace.includes_complete(root))?;
    let miss = workspace.find_unresolved_includes(root).first().cloned();
    Some(SnapshotFailure::new(
        "INCOMPLETE_INPUT",
        match &miss {
            Some(miss) => format!("Active include {} is absent or unreadable", miss.filename),
            None => "Model parsing or macro expansion is incomplete".to_owned(),
        },
        miss.map(|miss| miss.filename),
    ))
}

fn input_changed() -> SnapshotFailure {
    SnapshotFailure::new(
        "INPUT_CHANGED",
        "Comparison inputs changed while reading them; refresh the comparison",
        None,
    )
}

#[derive(Clone, Copy)]
pub enum SnapshotCoordinates {
    Lsp,
    Mcp,
}

/// Captured identities alone establish these aliases. Historical paths must
/// never be canonicalized through the current host's filesystem.
fn source_pairs<'a>(
    before: &'a CapturedSnapshot,
    after: &'a CapturedSnapshot,
) -> Vec<SourceFilePair<'a>> {
    let old_root = before.inputs["root_file"].as_str().unwrap_or(&before.root);
    let new_root = after.inputs["root_file"].as_str().unwrap_or(&after.root);
    let includes = |snapshot: &'a CapturedSnapshot, root: &str| {
        snapshot
            .sources
            .keys()
            .filter(move |key| key.as_str() != root)
            .collect::<Vec<_>>()
    };
    let old = includes(before, old_root);
    let new = includes(after, new_root);
    if before.historical == after.historical {
        if before.historical && before.inputs["repository_uri"] != after.inputs["repository_uri"] {
            return Vec::new();
        }
        let new: BTreeSet<_> = new.into_iter().collect();
        return old
            .into_iter()
            .filter(|key| new.contains(key))
            .map(|key| SourceFilePair {
                before_key: key,
                after_key: key,
                proof: if before.historical {
                    SourceIdentityProof::SameRepositoryKey
                } else {
                    SourceIdentityProof::SameWrittenFileIdentity
                },
            })
            .collect();
    }
    let aliases = |snapshot: &'a CapturedSnapshot, keys: Vec<&'a String>| {
        let mut aliases: BTreeMap<String, Vec<&'a String>> = BTreeMap::new();
        for key in keys {
            let path = if let Some(lookup) = &snapshot.workspace.snapshot_lookup {
                format!("{}/{}", lookup.repository_path.trim_end_matches('/'), key)
            } else {
                key.clone()
            };
            if let Some(alias) = lexical_captured_path(&path) {
                aliases.entry(alias).or_default().push(key);
            }
        }
        aliases
    };
    // Roots participate in alias uniqueness, although root correspondence is
    // selected explicitly and must never be emitted as an include pair.
    let old = aliases(before, before.sources.keys().collect());
    let new = aliases(after, after.sources.keys().collect());
    old.iter()
        .filter_map(|(alias, keys)| {
            let candidates = new.get(alias)?;
            (keys.len() == 1
                && candidates.len() == 1
                && keys[0] != old_root
                && candidates[0] != new_root)
                .then_some(SourceFilePair {
                    before_key: keys[0],
                    after_key: candidates[0],
                    proof: SourceIdentityProof::SameWrittenFileIdentity,
                })
        })
        .collect()
}

fn lexical_captured_path(path: &str) -> Option<String> {
    if crate::include_resolver::is_virtual_uri(path) || path.contains('\0') {
        return None;
    }
    let path = crate::include_resolver::uri_to_path(path)
        .to_string_lossy()
        .replace('\\', "/");
    let drive = path.as_bytes().get(1) == Some(&b':');
    if !drive && !path.starts_with('/') {
        return None;
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.len() > usize::from(drive) => {
                parts.pop();
            }
            ".." => return None,
            value => parts.push(value),
        }
    }
    let prefix = if path.starts_with("//") {
        "//"
    } else if path.starts_with('/') {
        "/"
    } else {
        ""
    };
    let identity = format!("{prefix}{}", parts.join("/"));
    Some(if cfg!(windows) {
        identity.to_lowercase()
    } else {
        identity
    })
}

pub fn compare_captured_snapshots(
    mut before: CapturedSnapshot,
    mut after: CapturedSnapshot,
    coordinates: SnapshotCoordinates,
) -> Value {
    if before.input_id == after.input_id {
        return SnapshotFailure::new(
            "INVALID_ARGUMENTS",
            "Before and After need distinct input IDs",
            None,
        )
        .response("before");
    }
    let mut diff = compare_models_with_sources(
        &before.model,
        &after.model,
        before
            .workspace
            .get_source(&before.root)
            .map(|text| CompareSource {
                text,
                origin_uri: Some(&before.root),
            }),
        after
            .workspace
            .get_source(&after.root)
            .map(|text| CompareSource {
                text,
                origin_uri: Some(&after.root),
            }),
    );
    let old = ComparisonInput::capture(
        &mut before.workspace,
        &before.root,
        Some(&before.root),
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
        crate::semantic_diff::Side::Before,
    )
    .with_snapshot_identity(&before.input_id, before.inputs["commit"].as_str());
    let new = ComparisonInput::capture(
        &mut after.workspace,
        &after.root,
        Some(&after.root),
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
        crate::semantic_diff::Side::After,
    )
    .with_snapshot_identity(&after.input_id, after.inputs["commit"].as_str());
    let include_pairs = source_pairs(&before, &after);
    crate::compare_navigation::populate_written_statements(&mut diff, &old, &new);
    let boundary = CaptureBoundary::RootAndExecutedIncludes;
    let _ = crate::semantic_diff::populate_captured_sources(
        &mut diff,
        old.source_input(boundary),
        new.source_input(boundary),
        &include_pairs,
    );
    crate::semantic_diff::enforce_output_budget(&mut diff);
    for (side, input) in [("before", &before), ("after", &after)] {
        if !input.historical && !input.workspace.input_snapshot_is_current(&input.root) {
            return input_changed().response(side);
        }
    }
    json!({"state":"result", "inputs":{"schema_version":SNAPSHOT_SCHEMA_VERSION, "before":before.inputs, "after":after.inputs},
        "diff":diff.to_json(), "navigation":navigation_json(&diff, &old, &new, match coordinates { SnapshotCoordinates::Lsp => Coordinates::SnapshotLsp, SnapshotCoordinates::Mcp => Coordinates::SnapshotMcp }),
        "sources":{"before":before.sources, "after":after.sources}})
}

#[cfg(test)]
mod source_identity_tests {
    use super::*;

    fn captured(historical: bool, repository: &str, root: &str, keys: &[&str]) -> CapturedSnapshot {
        let mut workspace = Workspace::new();
        if historical {
            workspace.snapshot_lookup = Some(SnapshotLookup {
                repository_path: repository.into(),
                ..Default::default()
            });
        }
        CapturedSnapshot {
            workspace,
            root: root.into(),
            model: crate::parse(""),
            input_id: if historical { "old" } else { "new" }.into(),
            revision: String::new(),
            inputs: json!({"root_file":root,"repository_uri":repository}),
            sources: keys
                .iter()
                .chain(std::iter::once(&root))
                .map(|key| ((*key).into(), String::new()))
                .collect(),
            historical,
        }
    }

    #[test]
    fn alias_proof_is_unique_and_uses_lexical_paths_only() {
        let old = captured(true, "/repo", "root.mod", &["shared/a", "shared/./a"]);
        let new = captured(false, "", "/repo/root.mod", &["/repo/shared/a"]);
        assert!(
            source_pairs(&old, &new).is_empty(),
            "two captured aliases cannot select one occurrence"
        );
        let old = captured(true, "/repo", "root.mod", &["shared/a"]);
        let new = captured(
            false,
            "",
            "/repo/root.mod",
            &["/repo/shared/a", "/repo/shared/./a"],
        );
        assert!(
            source_pairs(&old, &new).is_empty(),
            "one alias cannot select among two captured occurrences"
        );
        let new = captured(
            false,
            "",
            "/repo/root.mod",
            &["/repo/shared/../shared/a", "/other/shared/a"],
        );
        let pairs = source_pairs(&old, &new);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].after_key, "/repo/shared/../shared/a");
        assert_eq!(
            lexical_captured_path("/repo/../repo/shared/a"),
            lexical_captured_path("/repo/shared/a")
        );
        assert!(lexical_captured_path("/../repo/shared/a").is_none());
        assert!(lexical_captured_path("shared/a").is_none());
    }

    #[cfg(windows)]
    #[test]
    fn root_alias_collision_prevents_include_correspondence() {
        let old = captured(true, "C:/repo", "Root.mod", &["root.mod"]);
        let new = captured(false, "", "C:/repo/other.mod", &["C:/repo/root.mod"]);
        assert!(source_pairs(&old, &new).is_empty());
        assert!(source_pairs(&new, &old).is_empty());
    }
}
