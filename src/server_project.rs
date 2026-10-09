//! Folder discovery and one background compilation unit at a time.
//! Root-file loading and parsing run on the background worker.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tower_lsp::lsp_types::{notification, Diagnostic, Url};
use tower_lsp::Client;

use super::{
    file_url_from_path_key, is_model_root, prepare_root_report, Backend, Inner, RootReport,
};
use crate::diagnostic::check_in_workspace_with_origins;
use crate::include_resolver::{normalize_uri, path_key};
use crate::server_settings::SettingsStore;
use crate::workspace::InputStamps;
use crate::Workspace;

pub(super) const SCHEMA_VERSION: u32 = 1;
const TYPING_PAUSE: Duration = Duration::from_millis(250);
const RETAINED_REQUEST_ROOTS: usize = 8;

#[derive(Default)]
pub(super) struct ProjectState {
    roots: BTreeMap<Url, RootEntry>,
    pub notifications: bool,
    pub worker_running: bool,
    pub(super) initialized: bool,
    pub shutdown: bool,
    pub cancelled: bool,
    pub discovery_requested: bool,
    discovery: &'static str,
    discovery_failures: Vec<Value>,
    pub(super) epoch: u64,
    pass_revision: u64,
    pub(super) active: Option<Url>,
    explicit_active: Option<Url>,
    started: Option<Instant>,
    discovery_ms: f64,
    analysis_ms: f64,
    completed_jobs: u64,
    reused_jobs: u64,
    request_roots: Vec<Url>,
}

struct RootEntry {
    selected: bool,
    state: &'static str,
    generation: u64,
    due: Option<Instant>,
    dependencies: HashSet<String>,
    /// Native candidate identities observed for the last committed report.
    dependency_uris: BTreeSet<Url>,
    owner_files: HashSet<String>,
    proven: bool,
    /// Path identity from discovery, a committed check, or an edit of this
    /// root. Foreground scans compare the string. Merge still resolves
    /// routing identities from the filesystem once per operation.
    identity: String,
    revision: Option<String>,
    errors: usize,
    warnings: usize,
    failure: Option<String>,
    stamps: InputStamps,
    force: bool,
    cached_report: Option<Arc<RootReport>>,
}

impl RootEntry {
    fn pending() -> Self {
        Self {
            selected: true,
            state: "pending",
            generation: 0,
            due: Some(Instant::now()),
            dependencies: HashSet::new(),
            dependency_uris: BTreeSet::new(),
            owner_files: HashSet::new(),
            proven: false,
            identity: String::new(),
            revision: None,
            errors: 0,
            warnings: 0,
            failure: None,
            stamps: InputStamps::new(),
            force: true,
            cached_report: None,
        }
    }

    fn queue(&mut self, delay: Duration) {
        self.generation += 1;
        self.state = "pending";
        self.due = Some(Instant::now() + delay);
        self.failure = None;
        self.revision = None;
        self.errors = 0;
        self.warnings = 0;
    }
}

impl ProjectState {
    pub fn is_selected(&self, root: &Url) -> bool {
        if let Some(entry) = self.roots.get(root) {
            return entry.selected;
        }
        let key = normalize_uri(root.as_str());
        self.roots
            .values()
            .any(|entry| entry.selected && entry.identity == key)
    }

    /// Stored root identity. One fresh resolution when this URI is absent or the stored string is empty.
    pub(super) fn observed_identity(&self, uri: &Url) -> String {
        self.roots
            .get(uri)
            .map(|entry| entry.identity.clone())
            .filter(|identity| !identity.is_empty())
            .unwrap_or_else(|| normalize_uri(uri.as_str()))
    }

    pub fn owners(&self, uri: &Url) -> Vec<Url> {
        let key = normalize_uri(uri.as_str());
        self.roots
            .iter()
            .filter(|(_, entry)| {
                entry.identity != key && entry.selected && entry.owner_files.contains(&key)
            })
            .map(|(root, _)| root.clone())
            .collect()
    }

    fn begin_pass(&mut self) {
        self.pass_revision += 1;
        self.cancelled = false;
        self.started = Some(Instant::now());
        self.analysis_ms = 0.0;
        self.completed_jobs = 0;
        self.reused_jobs = 0;
    }

    fn status(&self, enabled: bool) -> Value {
        let mut counts = BTreeMap::from([
            ("pending", 0),
            ("checking", 0),
            ("checked", 0),
            ("incomplete", 0),
            ("failed", 0),
            ("excluded", 0),
        ]);
        let roots: Vec<_> = self.roots.iter().map(|(uri, entry)| {
            *counts.entry(entry.state).or_default() += 1;
            json!({"root_uri": uri, "state": entry.state, "revision": entry.revision, "errors": entry.errors, "warnings": entry.warnings, "failure": entry.failure,
                "dependency_candidates": entry.dependency_uris})
        }).collect();
        let complete = enabled
            && !self.cancelled
            && self.discovery == "complete"
            && counts["pending"] == 0
            && counts["checking"] == 0;
        json!({"schema_version": SCHEMA_VERSION, "pass_revision": self.pass_revision, "enabled": enabled, "discovery": if enabled { self.discovery } else { "disabled" },
            "cancelled": self.cancelled, "complete": complete, "coverage_complete": complete && self.discovery_failures.is_empty() && counts["failed"] == 0 && counts["incomplete"] == 0,
            "counts": counts, "roots": roots, "discovery_failures": self.discovery_failures,
            "metrics": {"discovery_ms": self.discovery_ms, "analysis_ms": self.analysis_ms, "completed_jobs": self.completed_jobs, "reused_jobs": self.reused_jobs, "elapsed_ms": self.started.map(|at| at.elapsed().as_secs_f64() * 1000.0)}})
    }

    fn finished(&self) -> bool {
        self.discovery == "complete"
            && !self
                .roots
                .values()
                .any(|entry| entry.due.is_some() || entry.state == "checking")
    }
}

impl Inner {
    pub(super) fn align_project_root(&mut self, uri: &Url) {
        if !is_model_root(uri) {
            return;
        }
        // One fresh identity for the opened URI. Other roots keep the identity
        // stored at discovery, check, or their own edit.
        let key = normalize_uri(uri.as_str());
        let alias = self
            .project
            .roots
            .iter()
            .find(|(root, entry)| *root != uri && entry.identity == key)
            .map(|(root, _)| root.clone());
        if let Some(alias) = alias {
            let mut entry = self.project.roots.remove(&alias).unwrap();
            if entry.selected {
                entry.queue(TYPING_PAUSE);
            }
            entry.identity = key.clone();
            self.project.roots.insert(uri.clone(), entry);
        } else if let Some(entry) = self.project.roots.get_mut(uri) {
            entry.identity = key.clone();
        }
        let report_roots: Vec<Url> = self.reports.keys().cloned().collect();
        for root in report_roots {
            if root == *uri {
                continue;
            }
            let same = self
                .project
                .roots
                .get(&root)
                .map(|entry| entry.identity == key)
                .unwrap_or_else(|| normalize_uri(root.as_str()) == key);
            if same {
                self.reports.remove(&root);
            }
        }
    }

    pub(super) fn project_reconfigure(&mut self, force: bool) {
        self.project.epoch += 1;
        self.project.begin_pass();
        self.project.discovery_requested = self.settings.project_diagnostics;
        self.project.discovery = if self.settings.project_diagnostics {
            "pending"
        } else {
            "disabled"
        };
        self.project.discovery_failures.clear();
        let settings = &self.settings;
        for (root, entry) in &mut self.project.roots {
            let selected = settings.project_diagnostics && selection(settings, root) == Some(true);
            entry.selected = selected;
            if selected {
                if force || entry.state == "checking" || entry.state == "pending" {
                    entry.force = force;
                    entry.proven = false;
                    entry.queue(Duration::ZERO);
                    self.reports.remove(root);
                }
            } else {
                entry.generation += 1;
                entry.due = None;
                entry.state = "excluded";
                entry.cached_report = None;
            }
        }
        if !settings.project_diagnostics {
            self.project.roots.clear();
        }
        self.reports
            .retain(|root, _| self.docs.contains_key(root) || self.project.is_selected(root));
        self.bound_workspace_cache();
    }

    pub(super) fn project_changed(&mut self, changed: &Url, deleted: bool) {
        if !self.settings.project_diagnostics || self.project.shutdown {
            return;
        }
        let resuming = self.project.cancelled;
        if resuming || self.project.finished() {
            self.project.begin_pass();
        }
        if resuming && self.project.discovery == "cancelled" {
            self.project.discovery_requested = true;
            self.project.discovery = "pending";
        }
        let key = normalize_uri(changed.as_str());
        if !deleted && let Some(entry) = self.project.roots.get_mut(changed) {
            entry.identity = key.clone();
        }
        let roots: Vec<Url> = self.project.roots.keys().cloned().collect();
        for root in roots {
            // Drop the root borrow before touching reports. A delete
            // canonicalizes only after the stored identity misses. Typing
            // compares the stored string and does not stat every root.
            let open = self.docs.contains_key(&root);
            let remove_report = {
                let Some(entry) = self.project.roots.get_mut(&root) else {
                    continue;
                };
                if !entry.selected {
                    continue;
                }
                let same_root = root == *changed
                    || entry.identity == key
                    || (deleted && normalize_uri(root.as_str()) == key);
                if same_root && deleted {
                    entry.selected = false;
                    entry.generation += 1;
                    entry.due = None;
                    entry.state = "excluded";
                    entry.cached_report = None;
                    !open
                } else if same_root
                    || !entry.proven
                    || entry.dependencies.contains(&key)
                    || (resuming && entry.state == "pending")
                {
                    entry.queue(TYPING_PAUSE);
                    entry.proven = false;
                    true
                } else {
                    false
                }
            };
            if remove_report {
                self.reports.remove(&root);
            }
        }
        if Path::new(changed.path())
            .extension()
            .is_some_and(|ext| ext == "mod")
            && (deleted
                || !self
                    .project
                    .roots
                    .values()
                    .any(|entry| entry.identity == key))
        {
            self.project.discovery_requested = true;
            self.project.discovery = "pending";
        }
        // A cancellation is lifted by an edit, even when dependency provenance
        // proves that none of the already checked roots needs another check.
        self.project.cancelled = false;
    }

    pub(super) fn bound_workspace_cache(&mut self) {
        self.project
            .request_roots
            .retain(|root| self.tracked_roots.contains_key(root));
        let mut roots: HashSet<_> = self
            .docs
            .keys()
            .map(|uri| normalize_uri(uri.as_str()))
            .collect();
        let retained: Vec<Url> = self
            .project
            .request_roots
            .iter()
            .rev()
            .take(RETAINED_REQUEST_ROOTS)
            .cloned()
            .collect();
        for root in retained {
            roots.insert(self.project.observed_identity(&root));
        }
        self.workspace.retain_analysis_roots(&roots);
        // Project dependency provenance lives in ProjectState, independently
        // of this bounded cache and model-info subscription bookkeeping.
    }

    pub(super) fn remember_request_root(&mut self, root: &Url) {
        self.project.request_roots.retain(|uri| uri != root);
        self.project.request_roots.push(root.clone());
        if self.project.request_roots.len() > RETAINED_REQUEST_ROOTS {
            self.project.request_roots.remove(0);
        }
    }
}

impl Backend {
    pub fn project_status(&self) -> Value {
        let inner = self.lock_inner();
        inner.project.status(inner.settings.project_diagnostics)
    }

    pub async fn active_model_changed(&self, params: Value) {
        super::ordering::ready().await;
        let uri = match params.get("root_uri") {
            Some(Value::Null) => None,
            Some(Value::String(raw)) => {
                let Some(uri) = Url::parse(raw).ok().filter(|uri| {
                    is_model_root(uri) && matches!(uri.scheme(), "file" | "untitled")
                }) else {
                    return;
                };
                Some(uri)
            }
            _ => return,
        };
        self.lock_inner().project.explicit_active = uri;
        super::ordering::committed();
        self.kick_project();
    }

    pub(super) fn kick_project(&self) {
        self.project_wake.notify_one();
        let mut inner = self.lock_inner();
        if !inner.project.initialized
            || inner.project.worker_running
            || inner.project.shutdown
            || !inner.settings.project_diagnostics
            || inner.project.cancelled
        {
            return;
        }
        if !inner.project.discovery_requested
            && !inner
                .project
                .roots
                .values()
                .any(|entry| entry.selected && entry.due.is_some())
        {
            return;
        }
        inner.project.worker_running = true;
        let client = self.client.clone();
        let state = Arc::clone(&self.inner);
        let wake = Arc::clone(&self.project_wake);
        let output_gate = Arc::clone(&self.output_gate);
        tokio::spawn(async move {
            run_worker(client, state, wake, output_gate).await;
        });
    }

    pub(super) async fn project_command(&self, command: &str) -> Value {
        if command == "dynare/projectStatus" {
            return self.project_status();
        }
        let _output = self.output_gate.lock().await;
        let publications = {
            let mut inner = self.lock_inner();
            if command == "dynare/recheckProject" {
                inner.project_reconfigure(true);
            } else if command == "dynare/cancelProject" {
                inner.project.epoch += 1;
                inner.project.cancelled = true;
                inner.project.discovery_requested = false;
                if inner.project.discovery == "pending" || inner.project.discovery == "discovering"
                {
                    inner.project.discovery = "cancelled";
                }
                for entry in inner.project.roots.values_mut() {
                    entry.generation += 1;
                    entry.due = None;
                    if entry.state == "checking" {
                        entry.state = "pending";
                    }
                }
            }
            inner.merge_diagnostics(None)
        };
        super::ordering::committed();
        publish(&self.client, publications).await;
        self.kick_project();
        send_status(&self.client, &self.inner).await;
        self.project_status()
    }
}

enum Work {
    Discover {
        epoch: u64,
        settings: SettingsStore,
    },
    Check {
        epoch: u64,
        generation: u64,
        root: Url,
        workspace: Box<Workspace>,
        overlays: HashMap<String, i32>,
        prior: Option<CachedReport>,
    },
    Delay(Duration),
    Stop,
}

async fn run_worker(
    client: Client,
    state: Arc<Mutex<Inner>>,
    wake: Arc<tokio::sync::Notify>,
    output_gate: Arc<tokio::sync::Mutex<()>>,
) {
    loop {
        let work = {
            let mut inner = state.lock().unwrap_or_else(|e| e.into_inner());
            if !inner.settings.project_diagnostics
                || inner.project.cancelled
                || inner.project.shutdown
            {
                Work::Stop
            } else if inner.project.discovery_requested {
                inner.project.discovery_requested = false;
                inner.project.discovery = "discovering";
                Work::Discover {
                    epoch: inner.project.epoch,
                    settings: inner.settings.clone(),
                }
            } else {
                let now = Instant::now();
                let active_uri = inner
                    .project
                    .explicit_active
                    .clone()
                    .or_else(|| inner.project.active.clone());
                let active = active_uri
                    .as_ref()
                    .map(|root| inner.project.observed_identity(root));
                let next = inner
                    .project
                    .roots
                    .iter()
                    .filter(|(_, entry)| entry.selected && entry.due.is_some_and(|due| due <= now))
                    .min_by_key(|(_, entry)| {
                        (
                            Some(entry.identity.as_str()) != active.as_deref(),
                            entry.due,
                        )
                    })
                    .map(|(root, _)| root.clone());
                if let Some(root) = next {
                    let settings = inner.settings.resolve(&root);
                    let epoch = inner.project.epoch;
                    let prior = inner
                        .project
                        .roots
                        .get(&root)
                        .filter(|entry| !entry.force)
                        .and_then(|entry| {
                            entry.cached_report.as_ref().map(|report| CachedReport {
                                report: Arc::clone(report),
                                stamps: entry.stamps.clone(),
                                owner_files: entry.owner_files.clone(),
                            })
                        });
                    let entry = inner.project.roots.get_mut(&root).unwrap();
                    entry.due = None;
                    entry.state = "checking";
                    let generation = entry.generation;
                    let overlays = inner
                        .docs
                        .iter()
                        .map(|(uri, doc)| (normalize_uri(uri.as_str()), doc.version))
                        .collect();
                    let workspace = Box::new(
                        inner
                            .workspace
                            .snapshot_for_root(root.as_str(), settings.search_paths),
                    );
                    Work::Check {
                        epoch,
                        generation,
                        root,
                        workspace,
                        overlays,
                        prior,
                    }
                } else if let Some(due) = inner
                    .project
                    .roots
                    .values()
                    .filter(|entry| entry.selected)
                    .filter_map(|entry| entry.due)
                    .min()
                {
                    Work::Delay(due.saturating_duration_since(now))
                } else {
                    Work::Stop
                }
            }
        };
        if matches!(work, Work::Stop) {
            // Decide and release worker ownership in one critical section, so
            // an event cannot queue work between the decision and this release.
            let runnable = {
                let mut inner = state.lock().unwrap_or_else(|e| e.into_inner());
                let runnable = inner.settings.project_diagnostics
                    && !inner.project.cancelled
                    && !inner.project.shutdown
                    && (inner.project.discovery_requested
                        || inner
                            .project
                            .roots
                            .values()
                            .any(|entry| entry.selected && entry.due.is_some()));
                if !runnable {
                    inner.project.worker_running = false;
                }
                runnable
            };
            if runnable {
                continue;
            }
            send_status(&client, &state).await;
            return;
        }
        send_status(&client, &state).await;
        match work {
            Work::Discover { epoch, settings } => {
                let started = Instant::now();
                let result = tokio::task::spawn_blocking(move || discover(&settings)).await;
                let _output = output_gate.lock().await;
                let publications = {
                    let mut inner = state.lock().unwrap_or_else(|e| e.into_inner());
                    if inner.project.epoch != epoch
                        || !inner.settings.project_diagnostics
                        || inner.project.cancelled
                        || inner.project.shutdown
                    {
                        continue;
                    }
                    inner.project.discovery_ms = started.elapsed().as_secs_f64() * 1000.0;
                    inner.project.discovery = "complete";
                    match result {
                        Ok((found, failures)) => {
                            // Resolve each open document once. A stored root
                            // identity that is not in this walk is resolved
                            // once, so a changed canonical target still binds
                            // to the existing root.
                            let mut open_identities = HashMap::new();
                            for uri in inner.docs.keys() {
                                open_identities
                                    .entry(normalize_uri(uri.as_str()))
                                    .or_insert_with(|| uri.clone());
                            }
                            let roots = bind_discovered_roots(
                                &inner.project.roots,
                                &open_identities,
                                found,
                            );
                            inner.project.discovery_failures = failures;
                            // A failed folder walk cannot prove prior roots gone.
                            // Keep them pending/incomplete rather than clear coverage.
                            let failed_folders: Vec<_> = inner
                                .project
                                .discovery_failures
                                .iter()
                                .filter_map(|item| {
                                    item["folder_uri"].as_str().and_then(|s| Url::parse(s).ok())
                                })
                                .collect();
                            inner.project.roots.retain(|root, _| {
                                roots.contains_key(root)
                                    || failed_folders.iter().any(|folder| in_folder(root, folder))
                            });
                            for (root, (key, selected)) in roots {
                                let entry = inner
                                    .project
                                    .roots
                                    .entry(root)
                                    .or_insert_with(RootEntry::pending);
                                let previous = std::mem::replace(&mut entry.identity, key);
                                if !selected {
                                    entry.selected = false;
                                    entry.state = "excluded";
                                    entry.due = None;
                                    entry.generation += 1;
                                } else if !entry.selected {
                                    entry.selected = true;
                                    entry.queue(Duration::ZERO);
                                } else if !previous.is_empty() && previous != entry.identity {
                                    entry.proven = false;
                                    entry.queue(Duration::ZERO);
                                }
                            }
                        }
                        Err(error) => {
                            inner.project.discovery_failures = vec![
                                json!({"failure": format!("Discovery worker failed: {error}")}),
                            ]
                        }
                    }
                    let docs: HashSet<_> = inner.docs.keys().cloned().collect();
                    let selected: HashSet<_> = inner
                        .project
                        .roots
                        .iter()
                        .filter(|(_, entry)| entry.selected)
                        .map(|(root, _)| root.clone())
                        .collect();
                    inner
                        .reports
                        .retain(|root, _| docs.contains(root) || selected.contains(root));
                    inner.merge_diagnostics(None)
                };
                publish(&client, publications).await;
            }
            Work::Check {
                epoch,
                generation,
                root,
                workspace,
                overlays,
                prior,
            } => {
                let check_root = root.clone();
                let result =
                    tokio::task::spawn_blocking(move || compute(&check_root, *workspace, prior))
                        .await;
                let _output = output_gate.lock().await;
                let publications = {
                    let mut inner = state.lock().unwrap_or_else(|e| e.into_inner());
                    if inner.project.epoch != epoch
                        || inner.project.cancelled
                        || inner.project.shutdown
                        || !inner.settings.project_diagnostics
                        || !inner
                            .project
                            .roots
                            .get(&root)
                            .is_some_and(|entry| entry.selected && entry.generation == generation)
                    {
                        continue;
                    }
                    match result {
                        Ok(Ok(checked)) => {
                            let mut open_versions = HashMap::new();
                            for (uri, doc) in &inner.docs {
                                open_versions
                                    .entry(normalize_uri(uri.as_str()))
                                    .or_insert(doc.version);
                            }
                            let current = checked.dependencies.iter().all(|key| {
                                open_versions.get(key).copied() == overlays.get(key).copied()
                            });
                            if !checked.current || !current {
                                inner
                                    .project
                                    .roots
                                    .get_mut(&root)
                                    .unwrap()
                                    .queue(TYPING_PAUSE);
                                continue;
                            }
                            inner.project.analysis_ms += checked.elapsed_ms;
                            inner.project.completed_jobs += 1;
                            inner.project.reused_jobs += u64::from(checked.reused);
                            let entry = inner.project.roots.get_mut(&root).unwrap();
                            entry.identity = normalize_uri(root.as_str());
                            entry.state = if checked.complete {
                                "checked"
                            } else {
                                "incomplete"
                            };
                            entry.dependencies = checked.dependencies;
                            entry.dependency_uris = checked.dependency_uris;
                            entry.owner_files = checked.owner_files;
                            entry.stamps = checked.stamps;
                            entry.force = false;
                            entry.proven = checked.complete;
                            entry.revision = Some(checked.report.revision.clone());
                            entry.errors = checked.report.errors;
                            entry.warnings = checked.report.warnings;
                            entry.cached_report =
                                checked.complete.then(|| Arc::clone(&checked.report));
                            inner.reports.insert(root.clone(), checked.report);
                            // A fragment opened before discovery no longer owns
                            // an independent compilation-unit report once known.
                            let fragments: Vec<_> = inner
                                .docs
                                .keys()
                                .filter(|uri| {
                                    !is_model_root(uri) && !inner.known_owner_roots(uri).is_empty()
                                })
                                .cloned()
                                .collect();
                            for fragment in fragments {
                                inner.reports.remove(&fragment);
                            }
                        }
                        failure => {
                            let message = match failure {
                                Ok(Err(message)) => message,
                                Err(error) => format!("Check worker failed: {error}"),
                                _ => unreachable!(),
                            };
                            let entry = inner.project.roots.get_mut(&root).unwrap();
                            entry.state = "failed";
                            entry.failure = Some(message);
                            entry.proven = false;
                            entry.cached_report = None;
                            if !inner.docs.contains_key(&root) {
                                inner.reports.remove(&root);
                            }
                        }
                    }
                    inner.merge_diagnostics(Some(&root))
                };
                publish(&client, publications).await;
            }
            Work::Delay(delay) => {
                tokio::select! { _ = tokio::time::sleep(delay) => {}, _ = wake.notified() => {} }
            }
            Work::Stop => unreachable!(),
        }
        send_status(&client, &state).await;
    }
}

struct Checked {
    report: Arc<RootReport>,
    dependencies: HashSet<String>,
    dependency_uris: BTreeSet<Url>,
    owner_files: HashSet<String>,
    current: bool,
    complete: bool,
    elapsed_ms: f64,
    stamps: InputStamps,
    reused: bool,
}

struct CachedReport {
    report: Arc<RootReport>,
    stamps: InputStamps,
    owner_files: HashSet<String>,
}

fn compute(
    root: &Url,
    mut workspace: Workspace,
    prior: Option<CachedReport>,
) -> std::result::Result<Checked, String> {
    let started = Instant::now();
    if let Some(prior) =
        prior.filter(|prior| prior.report.root == *root && workspace.inputs_match(&prior.stamps))
    {
        let dependencies: HashSet<_> = prior.stamps.keys().cloned().collect();
        let dependency_uris = dependencies
            .iter()
            .filter_map(|path| file_url_from_path_key(path))
            .collect();
        return Ok(Checked {
            report: prior.report,
            dependencies,
            dependency_uris,
            owner_files: prior.owner_files,
            stamps: prior.stamps,
            reused: true,
            current: true,
            complete: true,
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        });
    }
    let revision = workspace
        .input_revision(root.as_str())
        .ok_or_else(|| format!("Cannot read root model: {root}"))?;
    let set = check_in_workspace_with_origins(&mut workspace, root.as_str());
    let complete = workspace.includes_complete(root.as_str())
        && workspace
            .expand_report(root.as_str())
            .is_some_and(|report| report.complete);
    let text = Arc::from(
        workspace
            .get_source(root.as_str())
            .ok_or_else(|| format!("Cannot read root model: {root}"))?,
    );
    let current = workspace.input_revision(root.as_str()).as_deref() == Some(&revision)
        && workspace.input_snapshot_is_current(root.as_str());
    let dependencies: HashSet<_> = workspace
        .input_candidate_paths(root.as_str())
        .iter()
        .map(|path| path_key(path))
        .collect();
    // Resolve native identities once on the worker. Status reads describe
    // these committed inputs and must not inspect a later filesystem state.
    let dependency_uris = dependencies
        .iter()
        .filter_map(|path| file_url_from_path_key(path))
        .collect();
    let owner_files = workspace
        .include_records(root.as_str())
        .into_iter()
        .flat_map(|records| records.resolved.iter())
        .map(|record| path_key(&record.path))
        .collect();
    Ok(Checked {
        report: Arc::new(prepare_root_report(root, set, text, revision)),
        dependencies,
        dependency_uris,
        owner_files,
        current,
        complete,
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        stamps: workspace.input_stamps(root.as_str()),
        reused: false,
    })
}

/// Bind walked roots to open documents or existing roots.
/// A stored identity that is already in this walk is reused. Any other
/// existing root is resolved once and kept when that resolution is in the walk.
fn bind_discovered_roots(
    existing: &BTreeMap<Url, RootEntry>,
    open_identities: &HashMap<String, Url>,
    found: Vec<(Url, String, bool)>,
) -> BTreeMap<Url, (String, bool)> {
    let fresh_keys: HashSet<String> = found.iter().map(|(_, key, _)| key.clone()).collect();
    let mut stored_identities = HashMap::new();
    for (uri, entry) in existing {
        if !entry.identity.is_empty() && fresh_keys.contains(&entry.identity) {
            stored_identities
                .entry(entry.identity.clone())
                .or_insert_with(|| uri.clone());
        } else if let Some(key) = fresh_canonical(uri, &fresh_keys) {
            stored_identities.entry(key).or_insert_with(|| uri.clone());
        }
    }
    found
        .into_iter()
        .map(|(root, key, selected)| {
            let identity = open_identities
                .get(&key)
                .cloned()
                .or_else(|| stored_identities.get(&key).cloned())
                .unwrap_or(root);
            (identity, (key, selected))
        })
        .collect()
}

fn fresh_canonical(uri: &Url, fresh_keys: &HashSet<String>) -> Option<String> {
    let key = normalize_uri(uri.as_str());
    fresh_keys.contains(&key).then_some(key)
}

fn discover(settings: &SettingsStore) -> (Vec<(Url, String, bool)>, Vec<Value>) {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    let mut failures = Vec::new();
    for folder in &settings.folders {
        let Ok(path) = folder.uri.to_file_path() else {
            continue;
        };
        match crate::check_walk::collect_mod_files(&path) {
            Ok(paths) => {
                for path in paths {
                    if let Ok(uri) = Url::from_file_path(path) {
                        let key = normalize_uri(uri.as_str());
                        if seen.insert(key.clone()) {
                            roots.push((uri.clone(), key, selection(settings, &uri) == Some(true)));
                        }
                    }
                }
            }
            Err(error) => failures.push(
                json!({"folder_uri": folder.uri, "failure": format!("Cannot discover root models: {error}")}),
            ),
        }
    }
    (roots, failures)
}

fn in_folder(root: &Url, folder: &Url) -> bool {
    root.to_file_path()
        .ok()
        .zip(folder.to_file_path().ok())
        .is_some_and(|(root, folder)| Path::new(&path_key(&root)).starts_with(path_key(&folder)))
}

fn selection(settings: &SettingsStore, root: &Url) -> Option<bool> {
    let folder = settings
        .folders
        .iter()
        .filter(|folder| in_folder(root, &folder.uri))
        .max_by_key(|folder| folder.uri.path().len())?;
    let root_path = root.to_file_path().ok()?;
    let folder_path = folder.uri.to_file_path().ok()?;
    let root_key = path_key(&root_path);
    let folder_key = path_key(&folder_path);
    let relative = Path::new(&root_key)
        .strip_prefix(&folder_key)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    Some(
        !settings
            .resolve(root)
            .project_exclude_paths
            .iter()
            .any(|pattern| {
                if cfg!(windows) {
                    glob_matches(&pattern.to_lowercase(), &relative)
                } else {
                    glob_matches(pattern, &relative)
                }
            }),
    )
}

/// Folder-relative globs: `*`/`?` stay in one component and `**` crosses it.
/// A trailing slash denotes the directory and its descendants.
fn glob_matches(pattern: &str, relative: &str) -> bool {
    let mut pattern = pattern.replace('\\', "/");
    if pattern.ends_with('/') {
        pattern.push_str("**");
    }
    let pattern: Vec<_> = pattern.chars().collect();
    let relative: Vec<_> = relative.chars().collect();
    fn matches(
        pattern: &[char],
        path: &[char],
        i: usize,
        j: usize,
        memo: &mut HashMap<(usize, usize), bool>,
    ) -> bool {
        if let Some(found) = memo.get(&(i, j)) {
            return *found;
        }
        let found = match pattern.get(i) {
            None => j == path.len(),
            Some('*') if pattern.get(i + 1) == Some(&'*') => {
                if pattern.get(i + 2) == Some(&'/') {
                    matches(pattern, path, i + 3, j, memo)
                        || (j..path.len()).any(|end| {
                            path[end] == '/' && matches(pattern, path, i + 3, end + 1, memo)
                        })
                } else {
                    matches(pattern, path, i + 2, j, memo)
                        || (j < path.len() && matches(pattern, path, i, j + 1, memo))
                }
            }
            Some('*') => {
                matches(pattern, path, i + 1, j, memo)
                    || (path.get(j).is_some_and(|ch| *ch != '/')
                        && matches(pattern, path, i, j + 1, memo))
            }
            Some('?') => {
                path.get(j).is_some_and(|ch| *ch != '/')
                    && matches(pattern, path, i + 1, j + 1, memo)
            }
            Some(ch) => path.get(j) == Some(ch) && matches(pattern, path, i + 1, j + 1, memo),
        };
        memo.insert((i, j), found);
        found
    }
    matches(&pattern, &relative, 0, 0, &mut HashMap::new())
}

async fn publish(client: &Client, publications: Vec<(Url, Option<i32>, Vec<Diagnostic>)>) {
    for (uri, version, diagnostics) in publications {
        client.publish_diagnostics(uri, diagnostics, version).await;
    }
}

enum ProjectStatusChanged {}
impl notification::Notification for ProjectStatusChanged {
    type Params = Value;
    const METHOD: &'static str = "dynare/projectStatusChanged";
}

async fn send_status(client: &Client, state: &Mutex<Inner>) {
    let status = {
        let inner = state.lock().unwrap_or_else(|e| e.into_inner());
        inner
            .project
            .notifications
            .then(|| inner.project.status(inner.settings.project_diagnostics))
    };
    if let Some(status) = status {
        client
            .send_notification::<ProjectStatusChanged>(status)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folder_relative_globs() {
        assert!(glob_matches("**/*.mod", "root.mod"));
        assert!(glob_matches("**/*.mod", "a/b/root.mod"));
        assert!(glob_matches("generated/", "generated/sub/root.mod"));
        assert!(glob_matches("a/?oo.mod", "a/foo.mod"));
        assert!(!glob_matches("*.mod", "a/root.mod"));
        assert!(!glob_matches("archive/**", "archives/root.mod"));
        assert!(!glob_matches("**/foo.mod", "notfoo.mod"));
        assert!(glob_matches("a/?oo.mod", "a/žoo.mod"));
    }

    #[test]
    fn proven_dependency_selection_and_unknown_fallback_survive_cache_clear() {
        let mut inner = Inner::default();
        inner.project.discovery = "complete";
        let changed = Url::from_file_path(std::env::temp_dir().join("dependency.any")).unwrap();
        let dependent = Url::from_file_path(std::env::temp_dir().join("dependent.mod")).unwrap();
        let unrelated = Url::from_file_path(std::env::temp_dir().join("unrelated.mod")).unwrap();
        let unknown = Url::from_file_path(std::env::temp_dir().join("unknown.mod")).unwrap();
        for root in [&dependent, &unrelated, &unknown] {
            let mut entry = RootEntry::pending();
            entry.due = None;
            entry.state = "checked";
            entry.proven = *root != unknown;
            inner.project.roots.insert(root.clone(), entry);
        }
        inner
            .project
            .roots
            .get_mut(&dependent)
            .unwrap()
            .dependencies
            .insert(normalize_uri(changed.as_str()));
        inner.bound_workspace_cache();
        inner.project_changed(&changed, false);
        assert!(inner.project.roots[&dependent].due.is_some());
        assert!(inner.project.roots[&unknown].due.is_some());
        assert!(inner.project.roots[&unrelated].due.is_none());
        assert!(
            inner.project.owners(&changed).is_empty(),
            "search candidates do not imply include ownership"
        );
    }

    #[test]
    fn an_edit_matches_the_stored_root_identity() {
        let mut inner = Inner::default();
        inner.project.discovery = "complete";
        let edited = Url::from_file_path(std::env::temp_dir().join("edited-root.mod")).unwrap();
        let other = Url::from_file_path(std::env::temp_dir().join("quiet-root.mod")).unwrap();
        for root in [&edited, &other] {
            let mut entry = RootEntry::pending();
            entry.due = None;
            entry.state = "checked";
            entry.proven = true;
            entry.identity = normalize_uri(root.as_str());
            inner.project.roots.insert(root.clone(), entry);
        }
        inner.project_changed(&edited, false);
        assert!(inner.project.roots[&edited].due.is_some());
        assert!(inner.project.roots[&other].due.is_none());
        assert_eq!(
            inner.project.roots[&edited].identity,
            normalize_uri(edited.as_str())
        );
    }

    #[test]
    fn stored_identity_selects_include_owners_and_skips_the_file_itself() {
        let mut inner = Inner::default();
        let owner = Url::from_file_path(std::env::temp_dir().join("owner-root.mod")).unwrap();
        let include = Url::from_file_path(std::env::temp_dir().join("shared.inc")).unwrap();
        let key = normalize_uri(include.as_str());
        let mut owner_entry = RootEntry::pending();
        owner_entry.due = None;
        owner_entry.state = "checked";
        owner_entry.identity = normalize_uri(owner.as_str());
        owner_entry.owner_files.insert(key.clone());
        let mut self_entry = RootEntry::pending();
        self_entry.due = None;
        self_entry.state = "checked";
        self_entry.identity = key;
        self_entry
            .owner_files
            .insert(normalize_uri(include.as_str()));
        inner.project.roots.insert(owner.clone(), owner_entry);
        inner.project.roots.insert(include.clone(), self_entry);
        assert_eq!(inner.project.owners(&include), vec![owner]);
    }

    #[test]
    fn a_matching_stored_identity_keeps_the_existing_root() {
        let root = Url::from_file_path(std::env::temp_dir().join("kept-root.mod")).unwrap();
        let walked = Url::from_file_path(std::env::temp_dir().join("walked-root.mod")).unwrap();
        let mut entry = RootEntry::pending();
        entry.identity = "stored-key".to_string();
        let mut existing = BTreeMap::new();
        existing.insert(root.clone(), entry);
        let bound = bind_discovered_roots(
            &existing,
            &HashMap::new(),
            vec![(walked.clone(), "stored-key".to_string(), true)],
        );
        assert_eq!(bound.len(), 1);
        assert!(bound.contains_key(&root));
        assert_eq!(bound[&root].0, "stored-key");
    }

    #[cfg(windows)]
    #[test]
    fn a_stale_root_identity_stays_bound_to_its_case_alias() {
        let path =
            std::env::temp_dir().join(format!("dygnosis-stale-{}-root.mod", std::process::id()));
        let alias_path =
            path.with_file_name(format!("dygnosis-stale-{}-ROOT.mod", std::process::id()));
        let root = Url::from_file_path(&path).unwrap();
        let alias = Url::from_file_path(&alias_path).unwrap();
        let key = normalize_uri(root.as_str());
        assert_eq!(key, normalize_uri(alias.as_str()));
        let mut entry = RootEntry::pending();
        entry.identity = "stale-root-identity".to_string();
        entry.proven = true;
        entry.due = None;
        entry.state = "checked";
        let mut existing = BTreeMap::new();
        existing.insert(alias.clone(), entry);
        let bound =
            bind_discovered_roots(&existing, &HashMap::new(), vec![(root, key.clone(), true)]);
        assert_eq!(bound.len(), 1);
        assert!(bound.contains_key(&alias));
        assert_eq!(bound[&alias].0, key);
    }

    #[cfg(windows)]
    #[test]
    fn a_case_alias_reuses_the_stored_root_identity() {
        let mut inner = Inner::default();
        let path =
            std::env::temp_dir().join(format!("dygnosis-identity-{}-root.mod", std::process::id()));
        let alias_path =
            path.with_file_name(format!("dygnosis-identity-{}-ROOT.mod", std::process::id()));
        let root = Url::from_file_path(&path).unwrap();
        let alias = Url::from_file_path(&alias_path).unwrap();
        let key = normalize_uri(root.as_str());
        assert_eq!(key, normalize_uri(alias.as_str()));
        let mut entry = RootEntry::pending();
        entry.due = None;
        entry.state = "checked";
        entry.proven = true;
        entry.identity = key.clone();
        inner.project.roots.insert(root.clone(), entry);
        inner.reports.insert(
            root.clone(),
            Arc::new(RootReport {
                root: root.clone(),
                routes: HashMap::new(),
                revision: String::new(),
                errors: 0,
                warnings: 0,
            }),
        );
        inner.align_project_root(&alias);
        assert_eq!(inner.project.roots.len(), 1);
        assert!(inner.project.roots.contains_key(&alias));
        assert!(!inner.reports.contains_key(&root));
        assert!(inner.project.is_selected(&alias));
        assert!(inner.project.is_selected(&root));
        assert!(inner.project.roots[&alias].due.is_some());
        inner.project.roots.get_mut(&alias).unwrap().due = None;
        inner.project.roots.get_mut(&alias).unwrap().proven = true;
        inner.project.roots.get_mut(&alias).unwrap().state = "checked";
        inner.project_changed(&alias, false);
        assert!(inner.project.roots[&alias].due.is_some());
    }

    #[test]
    fn a_missing_root_is_an_infrastructure_failure() {
        let uri = Url::from_file_path(
            std::env::temp_dir().join(format!("dygnosis-absent-{}-root.mod", std::process::id())),
        )
        .unwrap();
        assert!(compute(&uri, Workspace::new(), None)
            .err()
            .expect("failure")
            .contains("Cannot read root model"));
    }
}
