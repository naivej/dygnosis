//! Read server-host Git objects and saved Working files for MCP comparison.
//! Git bodies are acquired only when the shared snapshot engine requests them.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::compare_snapshots::{
    capture_git_snapshot, capture_working_snapshot, compare_captured_snapshots, tree_path,
    CapturedSnapshot, GitSnapshotInput, ManifestEntry, SnapshotCapture, SnapshotCoordinates,
    SnapshotFailure, SourceFact, MAX_SNAPSHOT_FILES, MAX_SNAPSHOT_SOURCE_BYTES,
};
use crate::workspace::Workspace;

const MAX_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
const MAX_BLOB_BYTES: usize = 8 * 1024 * 1024;
const MAX_CAPTURE_ROUNDS: usize = 256;
const MAX_SOURCE_READS: usize = 512;
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(60);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum RepositorySelector {
    Git {
        /// Repository-relative .mod or .dyn root at this revision.
        root_file: String,
        /// Locally available commit, branch, tag, or revision expression. Resolved once before reading any model text. No automatic fetch.
        #[serde(rename = "ref")]
        requested_ref: String,
    },
    Working {
        /// Repository-relative .mod or .dyn root. Reads saved files on this server host, including saved active includes and untracked files.
        root_file: String,
    },
}

impl RepositorySelector {
    pub(crate) fn root_file(&self) -> &str {
        match self {
            Self::Git { root_file, .. } | Self::Working { root_file } => root_file,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        let root = self.root_file();
        if root.is_empty()
            || tree_path("", root).as_deref() != Some(root)
            || !matches!(
                Path::new(root).extension().and_then(|s| s.to_str()),
                Some("mod" | "dyn")
            )
        {
            return Err(
                "Each root_file must be an exact repository-relative .mod or .dyn path".into(),
            );
        }
        if let Self::Git { requested_ref, .. } = self {
            if requested_ref.is_empty() || requested_ref.contains('\0') {
                return Err(
                    "A Git selector requires a nonempty local ref without NUL characters".into(),
                );
            }
        }
        Ok(())
    }
}

pub(crate) struct RepositoryComparison {
    pub repository_path: String,
    pub before: RepositorySelector,
    pub after: RepositorySelector,
    pub search_paths: Vec<String>,
}

struct Acquisition<'a> {
    repository: PathBuf,
    cancelled: &'a (dyn Fn() -> bool + Sync),
    started: Instant,
    reads: usize,
    bytes: usize,
}

impl Acquisition<'_> {
    fn check(&self) -> Result<(), SnapshotFailure> {
        if (self.cancelled)() {
            Err(SnapshotFailure::new(
                "CANCELLED",
                "Repository comparison was cancelled",
                None,
            ))
        } else if self.started.elapsed() >= CAPTURE_TIMEOUT {
            Err(SnapshotFailure::new(
                "CAPTURE_LIMIT",
                "Repository comparison exceeded its capture time limit",
                None,
            ))
        } else {
            Ok(())
        }
    }

    fn git(&self, args: &[&str], limit: usize) -> Result<Vec<u8>, SnapshotFailure> {
        self.check()?;
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&self.repository)
            .args(args)
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // A server can have its own Git environment. Explicit repository_path
        // must still select the repository on disk rather than that environment.
        for name in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_COMMON_DIR",
        ] {
            command.env_remove(name);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().map_err(|error| {
            SnapshotFailure::new(
                "GIT_UNAVAILABLE",
                format!("Cannot start Git on this server host: {error}"),
                None,
            )
        })?;
        let overflow = Arc::new(AtomicBool::new(false));
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let out_flag = Arc::clone(&overflow);
        let err_flag = Arc::clone(&overflow);
        let out = std::thread::spawn(move || bounded_read(stdout, limit, out_flag));
        let err = std::thread::spawn(move || bounded_read(stderr, 64 * 1024, err_flag));
        let started = Instant::now();
        let status = loop {
            let failure = self.check().err().or_else(|| {
                if overflow.load(Ordering::Relaxed) {
                    Some(SnapshotFailure::new(
                        "CAPTURE_LIMIT",
                        "Git output exceeded its acquisition limit",
                        None,
                    ))
                } else if started.elapsed() >= COMMAND_TIMEOUT {
                    Some(SnapshotFailure::new(
                        "CAPTURE_LIMIT",
                        "Git object read exceeded its time limit",
                        None,
                    ))
                } else {
                    None
                }
            });
            if let Some(failure) = failure {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out.join();
                let _ = err.join();
                return Err(failure);
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = out.join();
                    let _ = err.join();
                    return Err(SnapshotFailure::new(
                        "GIT_READ_FAILED",
                        format!("Cannot wait for Git: {error}"),
                        None,
                    ));
                }
            }
        };
        let bytes = out
            .join()
            .map_err(|_| SnapshotFailure::new("GIT_READ_FAILED", "Git output reader failed", None))?
            .map_err(|e| {
                SnapshotFailure::new(
                    "GIT_READ_FAILED",
                    format!("Cannot read Git output: {e}"),
                    None,
                )
            })?;
        let stderr = err
            .join()
            .map_err(|_| SnapshotFailure::new("GIT_READ_FAILED", "Git error reader failed", None))?
            .map_err(|e| {
                SnapshotFailure::new(
                    "GIT_READ_FAILED",
                    format!("Cannot read Git errors: {e}"),
                    None,
                )
            })?;
        self.check()?;
        if overflow.load(Ordering::Relaxed) {
            return Err(SnapshotFailure::new(
                "CAPTURE_LIMIT",
                "Git output exceeded its acquisition limit",
                None,
            ));
        }
        if !status.success() {
            return Err(SnapshotFailure::new(
                "GIT_OBJECT_UNAVAILABLE",
                format!(
                    "Git cannot read this local revision or object: {}",
                    String::from_utf8_lossy(&stderr).trim()
                ),
                None,
            ));
        }
        Ok(bytes)
    }

    fn resolve(&self, selector: &RepositorySelector) -> Result<Option<String>, SnapshotFailure> {
        let RepositorySelector::Git { requested_ref, .. } = selector else {
            return Ok(None);
        };
        let expression = format!("{requested_ref}^{{commit}}");
        let bytes = self.git(
            &["rev-parse", "--verify", "--end-of-options", &expression],
            1024,
        )?;
        let commit = String::from_utf8_lossy(&bytes).trim().to_owned();
        if !valid_oid(&commit) {
            return Err(SnapshotFailure::new(
                "GIT_OBJECT_UNAVAILABLE",
                "Git did not resolve this ref to a full commit ID",
                None,
            ));
        }
        Ok(Some(commit))
    }

    fn capture(
        &mut self,
        selector: &RepositorySelector,
        commit: Option<String>,
        side: &str,
        search_paths: &[String],
    ) -> Result<CapturedSnapshot, SnapshotFailure> {
        self.check()?;
        let root_file = selector.root_file();
        let repository_uri = tower_lsp::lsp_types::Url::from_directory_path(&self.repository)
            .map_err(|_| {
                SnapshotFailure::new(
                    "REPOSITORY_UNAVAILABLE",
                    "Repository path cannot form a file URI",
                    None,
                )
            })?
            .to_string();
        let captured = if let Some(commit) = commit {
            let manifest = parse_manifest(&self.git(
                &["ls-tree", "-r", "-z", "--full-tree", &commit],
                MAX_MANIFEST_BYTES,
            )?)?;
            let mut input = GitSnapshotInput {
                input_id: side.to_owned(),
                root_file: root_file.to_owned(),
                repository_uri,
                commit,
                requested_ref: match selector {
                    RepositorySelector::Git { requested_ref, .. } => Some(requested_ref.clone()),
                    _ => None,
                },
                search_paths: search_paths
                    .iter()
                    .map(|path| historical_search_path(&self.repository, path))
                    .collect(),
                manifest,
                sources: BTreeMap::new(),
            };
            let mut ready = None;
            for _ in 0..MAX_CAPTURE_ROUNDS {
                self.check()?;
                match capture_git_snapshot(&input) {
                    SnapshotCapture::Ready(value) => {
                        ready = Some(*value);
                        break;
                    }
                    SnapshotCapture::Failure(failure) => return Err(failure),
                    SnapshotCapture::NeedsSources(keys) => {
                        let mut progress = false;
                        for key in keys {
                            if input.sources.contains_key(&key) {
                                continue;
                            }
                            let entry = input.manifest.get(&key).ok_or_else(|| {
                                SnapshotFailure::new(
                                    "GIT_READ_FAILED",
                                    "Engine requested a source outside the selected tree",
                                    Some(key.clone()),
                                )
                            })?;
                            self.reads += 1;
                            if self.reads > MAX_SOURCE_READS {
                                return Err(SnapshotFailure::new(
                                    "CAPTURE_LIMIT",
                                    "Repository source-read limit reached",
                                    Some(key),
                                ));
                            }
                            let fact = match self
                                .git(&["cat-file", "blob", &entry.object_id], MAX_BLOB_BYTES)
                            {
                                Ok(bytes) => {
                                    self.bytes += bytes.len();
                                    if self.bytes > MAX_SNAPSHOT_SOURCE_BYTES {
                                        return Err(SnapshotFailure::new(
                                            "CAPTURE_LIMIT",
                                            "Repository source-byte limit reached",
                                            Some(key),
                                        ));
                                    }
                                    SourceFact::Text {
                                        text: decode_source(bytes),
                                    }
                                }
                                Err(failure)
                                    if matches!(
                                        failure.code.as_str(),
                                        "CANCELLED" | "CAPTURE_LIMIT"
                                    ) =>
                                {
                                    return Err(failure)
                                }
                                Err(failure) => SourceFact::Failure {
                                    code: failure.code,
                                    message: failure.message,
                                },
                            };
                            input.sources.insert(key, fact);
                            progress = true;
                        }
                        if !progress {
                            return Err(SnapshotFailure::new(
                                "CAPTURE_NO_PROGRESS",
                                "Repository source capture made no progress",
                                None,
                            ));
                        }
                    }
                }
            }
            ready.ok_or_else(|| {
                SnapshotFailure::new(
                    "CAPTURE_LIMIT",
                    "Repository capture-round limit reached",
                    None,
                )
            })?
        } else {
            let root = self.repository.join(root_file);
            if !root.is_file() {
                return Err(SnapshotFailure::new(
                    "ROOT_NOT_FOUND",
                    "Saved model root is absent on this server host",
                    Some(root_file.to_owned()),
                ));
            }
            let metadata = std::fs::metadata(&root).map_err(|error| {
                SnapshotFailure::new(
                    "SOURCE_READ_FAILED",
                    format!("Cannot inspect saved model root: {error}"),
                    Some(root_file.to_owned()),
                )
            })?;
            if metadata.len() > MAX_BLOB_BYTES as u64 {
                return Err(SnapshotFailure::new(
                    "CAPTURE_LIMIT",
                    "Saved model root exceeds the source-byte limit",
                    Some(root_file.to_owned()),
                ));
            }
            let root_uri = tower_lsp::lsp_types::Url::from_file_path(&root)
                .map_err(|_| {
                    SnapshotFailure::new(
                        "ROOT_NOT_FOUND",
                        "Model root cannot form a file URI",
                        Some(root_file.to_owned()),
                    )
                })?
                .to_string();
            let workspace = Workspace::with_search_paths(
                search_paths
                    .iter()
                    .map(|path| self.repository.join(path))
                    .collect(),
            );
            match capture_working_snapshot(workspace, &root_uri, side, "saved_files") {
                SnapshotCapture::Ready(value) => *value,
                SnapshotCapture::Failure(failure) => return Err(failure),
                SnapshotCapture::NeedsSources(_) => {
                    return Err(SnapshotFailure::new(
                        "SOURCE_READ_FAILED",
                        "Saved model capture requested deferred sources",
                        None,
                    ))
                }
            }
        };
        self.check()?;
        let mut captured = captured;
        captured.inputs["repository_uri"] = json!(repository_uri_for(&self.repository));
        captured.inputs["repository_path"] = json!(self.repository.to_string_lossy());
        captured.inputs["root_file"] = json!(root_file);
        captured.inputs["search_paths"] = json!(search_paths);
        captured.inputs["resolved_search_paths"] = json!(search_paths
            .iter()
            .map(|path| self.repository.join(path).to_string_lossy().into_owned())
            .collect::<Vec<_>>());
        Ok(captured)
    }
}

fn repository_uri_for(path: &Path) -> String {
    tower_lsp::lsp_types::Url::from_directory_path(path)
        .map(|uri| uri.to_string())
        .unwrap_or_default()
}

fn historical_search_path(repository: &Path, path: &str) -> String {
    if let Some(relative) = tree_path("", path) {
        return relative;
    }
    // Working folders can lie outside the repository. Normalize the absolute
    // candidate lexically, so historical lookup can refuse that folder only
    // when an executed include reaches it, without consulting current disk.
    let repository = crate::include_resolver::uri_to_path(&repository_uri_for(repository));
    let host_path = repository.join(path);
    let mut normalized = PathBuf::new();
    for component in host_path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            value => normalized.push(value.as_os_str()),
        }
    }
    let lookup = crate::compare_snapshots::SnapshotLookup {
        repository_path: repository.to_string_lossy().replace('\\', "/"),
        ..Default::default()
    };
    let absolute = normalized.to_string_lossy().replace('\\', "/");
    lookup.path("", &absolute).unwrap_or(absolute)
}

fn bounded_read(
    mut reader: impl Read,
    limit: usize,
    overflow: Arc<AtomicBool>,
) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let keep = count.min(limit.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&buffer[..keep]);
        if keep != count {
            overflow.store(true, Ordering::Relaxed);
        }
    }
    Ok(bytes)
}

fn valid_oid(oid: &str) -> bool {
    matches!(oid.len(), 40 | 64) && oid.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn parse_manifest(bytes: &[u8]) -> Result<BTreeMap<String, ManifestEntry>, SnapshotFailure> {
    let invalid = || {
        SnapshotFailure::new(
            "GIT_READ_FAILED",
            "Git tree manifest has an unsupported path or entry",
            None,
        )
    };
    let mut manifest = BTreeMap::new();
    if !bytes.is_empty() && bytes.last() != Some(&0) {
        return Err(invalid());
    }
    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(invalid)?;
        let header = std::str::from_utf8(&record[..tab]).map_err(|_| invalid())?;
        let key = std::str::from_utf8(&record[tab + 1..]).map_err(|_| invalid())?;
        let fields: Vec<_> = header.split(' ').collect();
        if fields.len() != 3
            || !valid_oid(fields[2])
            || tree_path("", key).as_deref() != Some(key)
            || key.is_empty()
            || !matches!(
                (fields[0], fields[1]),
                ("100644" | "100755" | "120000", "blob") | ("160000", "commit")
            )
        {
            return Err(invalid());
        }
        if manifest
            .insert(
                key.to_owned(),
                ManifestEntry {
                    mode: fields[0].to_owned(),
                    object_id: fields[2].to_owned(),
                },
            )
            .is_some()
        {
            return Err(invalid());
        }
        if manifest.len() > MAX_SNAPSHOT_FILES {
            return Err(SnapshotFailure::new(
                "CAPTURE_LIMIT",
                "Repository manifest-entry limit reached",
                None,
            ));
        }
    }
    Ok(manifest)
}

// Keep the existing Workspace decoding rule for non-UTF-8 source files.
fn decode_source(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_else(|error| {
        error
            .into_bytes()
            .iter()
            .map(|byte| *byte as char)
            .collect()
    })
}

pub(crate) fn compare_repository(
    params: RepositoryComparison,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Value {
    let identify = |side: &str, selector: &RepositorySelector| {
        let mut identity = json!({"input_id":side, "root_file":selector.root_file(),
            "repository_path":params.repository_path, "repository_uri":repository_uri_for(Path::new(&params.repository_path)),
            "search_paths":params.search_paths, "resolved_search_paths":params.search_paths.iter().map(|path| Path::new(&params.repository_path).join(path).to_string_lossy().into_owned()).collect::<Vec<_>>(),
            "complete":false, "revision":null});
        match selector {
            RepositorySelector::Git { requested_ref, .. } => {
                identity["kind"] = json!("git");
                identity["source_policy"] = json!("git_tree");
                identity["requested_ref"] = json!(requested_ref);
                identity["commit"] = Value::Null;
            }
            RepositorySelector::Working { .. } => {
                identity["kind"] = json!("working");
                identity["source_policy"] = json!("saved_files");
            }
        }
        identity
    };
    let mut inputs = json!({"schema_version":crate::compare_snapshots::SNAPSHOT_SCHEMA_VERSION,
        "before":identify("before", &params.before), "after":identify("after", &params.after)});
    let mut result = compare_repository_inner(params, cancelled, &mut inputs);
    if result["state"] != "result" {
        if let Some(side) = result["side"].as_str() {
            inputs[side]["complete"] = json!(false);
        }
        result["inputs"] = inputs;
    }
    result
}

fn compare_repository_inner(
    params: RepositoryComparison,
    cancelled: &(dyn Fn() -> bool + Sync),
    inputs: &mut Value,
) -> Value {
    let repository = match std::fs::canonicalize(&params.repository_path) {
        Ok(path) if path.is_dir() => path,
        _ => {
            return SnapshotFailure::new(
                "REPOSITORY_UNAVAILABLE",
                "repository_path is not an accessible directory on this server host",
                None,
            )
            .response("before")
        }
    };
    let mut acquisition = Acquisition {
        repository,
        cancelled,
        started: Instant::now(),
        reads: 0,
        bytes: 0,
    };
    let resolved_root = match acquisition.git(&["rev-parse", "--show-toplevel"], 32 * 1024) {
        Ok(bytes) => PathBuf::from(String::from_utf8_lossy(&bytes).trim()),
        Err(failure) => return failure.response("before"),
    };
    match std::fs::canonicalize(resolved_root) {
        Ok(path) if path == acquisition.repository => {}
        _ => return SnapshotFailure::new("REPOSITORY_UNAVAILABLE", "repository_path must name the repository root; use root_file for a model in a subfolder", None).response("before"),
    }
    // Pin every revision before acquiring the first root or include body.
    let before_commit = match acquisition.resolve(&params.before) {
        Ok(commit) => commit,
        Err(failure) => return failure.response("before"),
    };
    if let Some(commit) = &before_commit {
        inputs["before"]["commit"] = json!(commit);
    }
    let after_commit = match acquisition.resolve(&params.after) {
        Ok(commit) => commit,
        Err(failure) => return failure.response("after"),
    };
    if let Some(commit) = &after_commit {
        inputs["after"]["commit"] = json!(commit);
    }
    let before = match acquisition.capture(
        &params.before,
        before_commit,
        "before",
        &params.search_paths,
    ) {
        Ok(value) => value,
        Err(failure) => return failure.response("before"),
    };
    inputs["before"] = before.inputs.clone();
    let after =
        match acquisition.capture(&params.after, after_commit, "after", &params.search_paths) {
            Ok(value) => value,
            Err(failure) => return failure.response("after"),
        };
    inputs["after"] = after.inputs.clone();
    if let Err(failure) = acquisition.check() {
        return failure.response("after");
    }
    let result = compare_captured_snapshots(before, after, SnapshotCoordinates::Mcp);
    if let Err(failure) = acquisition.check() {
        return failure.response("after");
    }
    // MCP retains its established structural fields at the result's top level.
    if result["state"] == "result" {
        let mut output = result["diff"].clone();
        output["state"] = json!("result");
        output["inputs"] = result["inputs"].clone();
        output["navigation"] = result["navigation"].clone();
        output["sources"] = result["sources"].clone();
        output
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    static NEXT_REPO: AtomicU64 = AtomicU64::new(0);

    struct TestRepository(PathBuf);

    impl TestRepository {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "dygnosis mcp history {} {}",
                std::process::id(),
                NEXT_REPO.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).expect("unique temporary repository");
            let repo = Self(path);
            repo.git(&["init", "--quiet"]);
            repo.git(&["config", "user.name", "Dygnosis test"]);
            repo.git(&["config", "user.email", "test@example.invalid"]);
            repo.git(&["config", "core.autocrlf", "false"]);
            repo
        }

        fn git(&self, args: &[&str]) -> String {
            let output = Command::new("git")
                .arg("-C")
                .arg(&self.0)
                .args(args)
                .output()
                .expect("Git installed for repository test");
            assert!(
                output.status.success(),
                "{:?}: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        }

        fn write(&self, key: &str, text: &str) {
            let path = self.0.join(key);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }

        fn commit(&self) -> String {
            self.git(&["add", "."]);
            self.git(&["commit", "--quiet", "-m", "model"]);
            self.git(&["rev-parse", "HEAD"])
        }

        fn compare(
            &self,
            before: RepositorySelector,
            after: RepositorySelector,
            search_paths: &[&str],
        ) -> Value {
            compare_repository(
                RepositoryComparison {
                    repository_path: self.0.to_string_lossy().into_owned(),
                    before,
                    after,
                    search_paths: search_paths.iter().map(|path| (*path).to_owned()).collect(),
                },
                &|| false,
            )
        }
    }

    impl Drop for TestRepository {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn git(root: &str, reference: &str) -> RepositorySelector {
        RepositorySelector::Git {
            root_file: root.into(),
            requested_ref: reference.into(),
        }
    }

    fn working(root: &str) -> RepositorySelector {
        RepositorySelector::Working {
            root_file: root.into(),
        }
    }

    const ROOT: &str = "var y;\nmodel;\n@#include \"equations.inc\"\nend;\n";

    #[test]
    fn history_reads_each_commits_executed_include_and_mcp_source_identity() {
        let repo = TestRepository::new();
        repo.write("main.mod", ROOT);
        repo.write("equations.inc", "[name='eq'] y = 1;\n");
        let first = repo.commit();
        repo.write("equations.inc", "[name='eq'] y = 2;\n");
        let second = repo.commit();
        repo.write("equations.inc", "[name='eq'] y = 9;\n");
        let result = repo.compare(git("main.mod", &first), git("main.mod", &second), &[]);
        assert_eq!(result["state"], "result", "{result}");
        assert_eq!(
            result["changed_equations"].as_array().unwrap().len(),
            1,
            "{result}"
        );
        assert_eq!(result["inputs"]["before"]["commit"], first);
        assert_eq!(result["inputs"]["after"]["commit"], second);
        assert_eq!(result["inputs"]["after"]["source_policy"], "git_tree");
        assert_eq!(result["inputs"]["after"]["search_paths"], json!([]));
        assert_eq!(result["navigation"]["schema_version"], 2, "{result}");
        let result_text = result.to_string();
        assert!(result_text.contains("equations.inc"), "{result}");
        assert!(!result_text.contains("y = 9"), "{result}");
        let saved = repo.compare(git("main.mod", "HEAD"), working("main.mod"), &[]);
        assert_eq!(saved["state"], "result", "{saved}");
        assert_eq!(saved["inputs"]["after"]["source_policy"], "saved_files");
        assert!(saved.to_string().contains("y = 9"), "{saved}");
    }

    #[test]
    fn historical_missing_include_never_uses_saved_or_untracked_files() {
        let repo = TestRepository::new();
        repo.write("main.mod", ROOT);
        let missing = repo.commit();
        repo.write("equations.inc", "[name='eq'] y = 9;\n");
        let result = repo.compare(git("main.mod", &missing), working("main.mod"), &[]);
        assert_eq!(result["state"], "failure", "{result}");
        assert_eq!(result["side"], "before");
        assert_eq!(result["code"], "INCOMPLETE_INPUT");
        assert!(result.get("changed_equations").is_none());
        let saved = repo.compare(working("main.mod"), working("main.mod"), &[]);
        assert_eq!(saved["state"], "result", "{saved}");
    }

    #[test]
    fn renamed_roots_refs_and_configured_search_paths_use_selected_trees() {
        let repo = TestRepository::new();
        repo.write("old.mod", ROOT);
        repo.write("common/equations.inc", "[name='eq'] y = 1;\n");
        let first = repo.commit();
        repo.git(&["tag", "baseline"]);
        repo.git(&["mv", "old.mod", "new.dyn"]);
        repo.write("common/equations.inc", "[name='eq'] y = 2;\n");
        let second = repo.commit();
        let result = repo.compare(
            git("old.mod", "baseline"),
            git("new.dyn", "HEAD"),
            &["common"],
        );
        assert_eq!(result["state"], "result", "{result}");
        assert_eq!(result["inputs"]["before"]["commit"], first);
        assert_eq!(result["inputs"]["after"]["commit"], second);
        assert_eq!(result["inputs"]["before"]["requested_ref"], "baseline");
        assert_eq!(
            result["inputs"]["before"]["search_paths"],
            json!(["common"])
        );
        let missing = repo.compare(
            git("new.dyn", "baseline"),
            git("new.dyn", "HEAD"),
            &["common"],
        );
        assert_eq!(missing["code"], "ROOT_NOT_FOUND");
        assert_eq!(missing["side"], "before");
        let mut acquisition = Acquisition {
            repository: std::fs::canonicalize(&repo.0).unwrap(),
            cancelled: &|| false,
            started: Instant::now(),
            reads: 0,
            bytes: 0,
        };
        let selector = git("old.mod", "baseline");
        let pinned = acquisition.resolve(&selector).unwrap();
        repo.git(&["tag", "--force", "baseline", "HEAD"]);
        let captured = acquisition
            .capture(&selector, pinned, "before", &["common".into()])
            .unwrap_or_else(|failure| panic!("{}", failure.message));
        assert_eq!(
            captured.inputs["commit"], first,
            "Moving a ref cannot replace its pinned commit"
        );
    }

    #[test]
    fn missing_git_blob_and_oversized_sources_do_not_use_disk_or_publish_rows() {
        let repo = TestRepository::new();
        repo.write("main.mod", "var y; model; [name='eq'] y=1; end;\n");
        let commit = repo.commit();
        let blob = repo.git(&["rev-parse", "HEAD:main.mod"]);
        std::fs::remove_file(
            repo.0
                .join(".git/objects")
                .join(&blob[..2])
                .join(&blob[2..]),
        )
        .unwrap();
        let missing = repo.compare(git("main.mod", &commit), working("main.mod"), &[]);
        assert_eq!(missing["code"], "GIT_OBJECT_UNAVAILABLE", "{missing}");
        assert_eq!(missing["inputs"]["before"]["commit"], commit);
        assert_eq!(missing["inputs"]["before"]["complete"], false);
        assert!(missing.get("changed_equations").is_none());
        repo.write("main.mod", &"x".repeat(MAX_BLOB_BYTES + 1));
        let oversized = repo.compare(working("main.mod"), working("main.mod"), &[]);
        assert_eq!(oversized["code"], "CAPTURE_LIMIT", "{oversized}");
        assert!(oversized.get("changed_equations").is_none());
    }

    #[test]
    fn acquisition_reads_active_blobs_only_and_saved_changes_reject_publish() {
        let repo = TestRepository::new();
        repo.write(
            "main.mod",
            "var y;\nmodel;\n@#if 0\n@#include \"inactive.inc\"\n@#endif\n[name='eq'] y=1;\nend;\n",
        );
        repo.write("inactive.inc", &"x".repeat(MAX_BLOB_BYTES + 1));
        let commit = repo.commit();
        let mut acquisition = Acquisition {
            repository: std::fs::canonicalize(&repo.0).unwrap(),
            cancelled: &|| false,
            started: Instant::now(),
            reads: 0,
            bytes: 0,
        };
        let before = acquisition
            .capture(&git("main.mod", &commit), Some(commit), "before", &[])
            .unwrap_or_else(|f| panic!("{}", f.message));
        assert_eq!(
            acquisition.reads, 1,
            "Inactive include bodies must not be read"
        );
        let after = acquisition
            .capture(&working("main.mod"), None, "after", &[])
            .unwrap_or_else(|f| panic!("{}", f.message));
        repo.write("main.mod", "var y;\nmodel;\n[name='eq'] y=2;\nend;\n");
        let result = compare_captured_snapshots(before, after, SnapshotCoordinates::Mcp);
        assert_eq!(result["code"], "INPUT_CHANGED", "{result}");
        assert_eq!(result["side"], "after");
        assert!(result.get("diff").is_none());
    }

    #[test]
    fn absolute_in_repository_folders_and_written_includes_use_historical_sources() {
        let repo = TestRepository::new();
        let folder = repo.0.join("common").to_string_lossy().replace('\\', "/");
        repo.write("main.mod", ROOT);
        repo.write("common/equations.inc", "[name='eq'] y=1;\n");
        let first = repo.commit();
        repo.write("common/equations.inc", "[name='eq'] y=9;\n");
        let result = repo.compare(git("main.mod", &first), working("main.mod"), &[&folder]);
        assert_eq!(result["state"], "result", "{result}");
        assert_eq!(result["changed_equations"].as_array().unwrap().len(), 1);
        repo.write(
            "absolute.mod",
            &format!("var y;\nmodel;\n@#include \"{folder}/equations.inc\"\nend;\n"),
        );
        let absolute = repo.commit();
        let written = repo.compare(
            git("absolute.mod", &absolute),
            git("absolute.mod", &absolute),
            &[],
        );
        assert_eq!(written["state"], "result", "{written}");
        let outside = repo
            .0
            .parent()
            .unwrap()
            .join("outside.inc")
            .to_string_lossy()
            .replace('\\', "/");
        repo.write(
            "external.mod",
            &format!("var y;\nmodel;\n@#include \"{outside}\"\nend;\n"),
        );
        let external = repo.commit();
        let failure = repo.compare(git("external.mod", &external), working("main.mod"), &[]);
        assert_eq!(failure["code"], "UNSUPPORTED_SOURCE", "{failure}");
        assert_eq!(failure["side"], "before");
        assert!(failure.get("changed_equations").is_none());
    }

    #[test]
    fn sibling_search_folders_work_for_saved_inputs_and_fail_only_when_history_reaches_them() {
        let container = TestRepository::new();
        let repo = TestRepository(container.0.join("project"));
        std::fs::create_dir(&repo.0).unwrap();
        repo.git(&["init", "--quiet"]);
        repo.git(&["config", "user.name", "Dygnosis test"]);
        repo.git(&["config", "user.email", "test@example.invalid"]);
        container.write("common/equations.inc", "[name='eq'] y=1;\n");
        repo.write("main.mod", ROOT);
        repo.write(
            "other.mod",
            "var y;\nmodel;\n@#include \"other.inc\"\nend;\n",
        );
        repo.write("other.inc", "[name='eq'] y=2;\n");
        repo.write("inactive.mod", "var y;\nmodel;\n@#if 0\n@#include \"equations.inc\"\n@#endif\n[name='eq'] y=1;\nend;\n");
        repo.commit();
        let saved = repo.compare(working("main.mod"), working("other.mod"), &["../common"]);
        assert_eq!(saved["state"], "result", "{saved}");
        assert_eq!(saved["changed_equations"].as_array().unwrap().len(), 1);
        assert_eq!(
            saved["inputs"]["before"]["search_paths"],
            json!(["../common"])
        );
        let failure = repo.compare(
            git("main.mod", "HEAD"),
            working("other.mod"),
            &["../common"],
        );
        assert_eq!(failure["code"], "UNSUPPORTED_SOURCE", "{failure}");
        assert_eq!(failure["side"], "before");
        assert!(failure.get("changed_equations").is_none());
        let local = repo.compare(
            git("other.mod", "HEAD"),
            working("other.mod"),
            &["../common"],
        );
        assert_eq!(
            local["state"], "result",
            "An unused external search folder must not reject history: {local}"
        );
        let inactive = repo.compare(
            git("inactive.mod", "HEAD"),
            git("inactive.mod", "HEAD"),
            &["../common"],
        );
        assert_eq!(
            inactive["state"], "result",
            "An inactive include must not consult external folders: {inactive}"
        );
    }

    #[test]
    fn both_refs_resolve_before_sources_and_capture_honors_cancellation() {
        let repo = TestRepository::new();
        repo.write("main.mod", "var y; model; y=1; end;\n");
        repo.commit();
        let invalid_after = repo.compare(
            git("absent.mod", "HEAD"),
            git("main.mod", "not-a-local-ref"),
            &[],
        );
        assert_eq!(
            invalid_after["side"], "after",
            "Ref failure must precede body acquisition: {invalid_after}"
        );
        assert_eq!(invalid_after["code"], "GIT_OBJECT_UNAVAILABLE");
        let result = compare_repository(
            RepositoryComparison {
                repository_path: repo.0.to_string_lossy().into_owned(),
                before: git("main.mod", "HEAD"),
                after: working("main.mod"),
                search_paths: Vec::new(),
            },
            &|| true,
        );
        assert_eq!(result["code"], "CANCELLED");
        assert!(result.get("changed_equations").is_none());
    }

    #[test]
    fn historical_symlink_and_gitlink_dependencies_are_explicit_failures() {
        let repo = TestRepository::new();
        repo.write("main.mod", ROOT);
        let oid = repo.git(&["hash-object", "-w", "--stdin"]);
        repo.git(&["add", "main.mod"]);
        repo.git(&[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("120000,{oid},equations.inc"),
        ]);
        repo.git(&["commit", "--quiet", "-m", "symlink"]);
        let result = repo.compare(git("main.mod", "HEAD"), git("main.mod", "HEAD"), &[]);
        assert_eq!(result["code"], "UNSUPPORTED_SOURCE", "{result}");
        assert_eq!(result["side"], "before");
        let commit = repo.git(&["rev-parse", "HEAD"]);
        repo.git(&[
            "update-index",
            "--cacheinfo",
            &format!("160000,{commit},equations.inc"),
        ]);
        repo.git(&["commit", "--quiet", "-m", "gitlink"]);
        let result = repo.compare(git("main.mod", "HEAD"), git("main.mod", "HEAD"), &[]);
        assert_eq!(result["code"], "UNSUPPORTED_SOURCE", "{result}");
    }

    #[test]
    fn nul_manifest_keeps_tabs_spaces_and_case_and_refuses_invalid_entries() {
        let oid = "a".repeat(40);
        let bytes = format!("100644 blob {oid}\tFolder/a\tb c.inc\0");
        let manifest = parse_manifest(bytes.as_bytes()).unwrap();
        assert!(manifest.contains_key("Folder/a\tb c.inc"));
        assert!(!manifest.contains_key("folder/a\tb c.inc"));
        for invalid in [
            format!("100644 blob {oid}\t../outside.mod\0"),
            "100644 blob short\tmain.mod\0".to_owned(),
            format!("100644 blob {oid}\tmain.mod"),
        ] {
            assert!(parse_manifest(invalid.as_bytes()).is_err());
        }
    }
}
