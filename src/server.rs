//! LSP server (stdio). Wave a: document loop. Wave b: navigation / edit. Wave c: intel / format / commands.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[path = "server_project.rs"]
mod project;

#[path = "server_ordering.rs"]
mod ordering;

#[cfg(test)]
#[path = "server_ordering_tests.rs"]
mod ordering_tests;

use serde_json::{json, Value};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, ClientSocket, LanguageServer, LspService, Server};

use crate::catalog::{
    command_options, family_help, option_doc, FAMILY_COMMAND_HELP, FAMILY_OPERATOR_HELP,
    HETEROGENEITY_OPTION, HET_SHOCKS_OVERWRITE,
};
use crate::diagnostic::{check_file, check_in_workspace_with_origins, DiagnosticSet};
use crate::equation_names::{equation_name_plan, long_name_plan, metadata_completion};
use crate::expand::{EquationOrigin, OriginFrame};
use crate::explain;
use crate::format::{format_range, format_text};
use crate::lexer::{tokenize, TokenKind};
use crate::model::{Decl, Equation, Model};
use crate::model_diff::{compare_models_with_sources, CompareSource};
use crate::model_info::{
    assigned_number, classify_variable_timing, format_timing_line, TimingClass,
};
use crate::refs::{
    enclosing_paren_has_ident, ident_at, is_legal_ident, occurrences, option_command_at,
    option_owner_at,
};
use crate::server_model_map::{common_leaves, WrittenView};
use crate::server_names::{NameRole, NameSites, SemanticMapping};
use crate::server_settings::{
    PresentationSettings, ResourceSettings, SettingsStore, CONFIGURATION_SCHEMA_VERSION,
    MODEL_INFO_SCHEMA_VERSION,
};
use crate::span::{LineIndex, Span};
use crate::workspace::Workspace;
use crate::{Severity, VERSION};

const DYNARE_KEYWORDS: &[(&str, &str)] = &[
    ("var", "Declare endogenous variables"),
    ("varexo", "Declare exogenous variables"),
    ("parameters", "Declare parameters"),
    ("model", "Begin model equation block"),
    ("model_remove", "Remove equations by tag"),
    ("model_replace", "Replace equations by tag"),
    ("end", "End a block"),
    ("steady_state_model", "Define steady state computation"),
    ("initval", "Set initial values for steady state computation"),
    ("endval", "Set terminal values"),
    ("shocks", "Define shock processes"),
    ("steady", "Compute the steady state"),
    ("check", "Check Blanchard-Kahn conditions"),
    ("stoch_simul", "Compute stochastic simulation"),
    ("simul", "Compute deterministic simulation"),
    ("resid", "Compute residuals of model equations"),
    ("estimated_params", "Define parameters to estimate"),
    ("estimation", "Run Bayesian or ML estimation"),
    ("varobs", "Declare observed variables"),
    ("calib_smoother", "Run the calibrated smoother"),
    ("forecast", "Compute forecasts"),
    ("osr", "Optimal simple rules"),
    ("ramsey_model", "Ramsey optimal policy model"),
    ("planner_objective", "Define planner's objective for Ramsey"),
    ("identification", "Run identification analysis"),
    ("sensitivity", "Run sensitivity analysis"),
    ("sbvar", "Estimate a Markov-switching SBVAR model"),
    (
        "svar_identification",
        "Describe the SVAR identification restrictions",
    ),
    (
        "svar_global_identification_check",
        "Check the SVAR identification globally",
    ),
    (
        "conditional_forecast_paths",
        "Constrain an endogenous path before conditional_forecast",
    ),
    (
        "plot_conditional_forecast",
        "Plot the conditional and unconditional forecasts",
    ),
    ("prior", "Prior distribution for a parameter"),
    ("method_of_moments", "Run method of moments estimation"),
    (
        "matched_moments",
        "Specify the product moments used in estimation",
    ),
    (
        "matched_irfs",
        "Specify the empirical IRFs matched in estimation",
    ),
    (
        "matched_irfs_weights",
        "Specify the weighting matrix used for IRF matching",
    ),
    ("irf_calibration", "Define IRF calibration criteria"),
    ("moment_calibration", "Define moment calibration criteria"),
];

const BUILTIN_FNS: &[(&str, &str)] = &[
    ("exp", "Exponential function exp(x)"),
    ("log", "Natural logarithm log(x)"),
    ("ln", "Natural logarithm ln(x)"),
    ("sqrt", "Square root sqrt(x)"),
    ("abs", "Absolute value abs(x)"),
    ("sin", "Sine function"),
    ("cos", "Cosine function"),
];

const OUT_CODES: &[&str] = &[
    "E040", "W040", "W041", "I041", "W071", "I070", "I071", "W080", "W081", "DYNR",
];

struct OpenDoc {
    text: String,
    version: i32,
    diagnostics: Vec<Diagnostic>,
    library: Vec<crate::Diagnostic>,
}

struct RootReport {
    root: Url,
    routes: HashMap<Url, Vec<RoutedDiagnostic>>,
    revision: String,
    errors: usize,
    warnings: usize,
}

#[derive(Clone)]
struct RoutedDiagnostic {
    diagnostic: crate::Diagnostic,
    lsp_diagnostic: Diagnostic,
    text: std::sync::Arc<str>,
    root: Url,
    revision: String,
}

struct ValueHintSite {
    value: Option<f64>,
    range: Option<Range>,
    plain: bool,
    statement_id: usize,
}

/// Source URI is the outer grouping key. An occurrence ordinal keeps repeated
/// diagnostics from one compilation unit while merging identical other roots.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct DiagnosticPresentationKey {
    start_line: u32,
    start_character: u32,
    end_line: u32,
    end_character: u32,
    code: String,
    severity: String,
    message: String,
    writing_root: Option<String>,
    related: String,
}

fn diagnostic_presentation_key(item: &Diagnostic) -> DiagnosticPresentationKey {
    DiagnosticPresentationKey {
        start_line: item.range.start.line,
        start_character: item.range.start.character,
        end_line: item.range.end.line,
        end_character: item.range.end.character,
        code: format!("{:?}", item.code),
        severity: format!("{:?}", item.severity),
        message: item.message.clone(),
        writing_root: item
            .data
            .as_ref()
            .filter(|_| matches!(&item.code, Some(NumberOrString::String(code)) if crate::check_writing::is_writing_code(code)))
            .and_then(|data| data.get("root"))
            .and_then(Value::as_str)
            .map(str::to_string),
        related: format!(
            "{:?}|{:?}",
            item.related_information,
            item.data
                .as_ref()
                .and_then(|data| data.get("related_context"))
        ),
    }
}

struct Inner {
    docs: HashMap<Url, OpenDoc>,
    published: HashMap<Url, Vec<Diagnostic>>,
    routed_library: HashMap<Url, Vec<RoutedDiagnostic>>,
    workspace: Workspace,
    settings: SettingsStore,
    tracked_roots: HashMap<Url, String>,
    model_info_notifications: bool,
    token_refresh: bool,
    hint_refresh: bool,
    semantic_mapping: SemanticMapping,
    completion_label_details: bool,
    completion_snippets: bool,
    publication_versions: HashMap<Url, Option<i32>>,
    reports: HashMap<Url, Arc<RootReport>>,
    project: project::ProjectState,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            docs: HashMap::new(),
            published: HashMap::new(),
            routed_library: HashMap::new(),
            workspace: Workspace::new(),
            settings: SettingsStore::default(),
            tracked_roots: HashMap::new(),
            model_info_notifications: false,
            token_refresh: false,
            hint_refresh: false,
            semantic_mapping: SemanticMapping::default(),
            completion_label_details: false,
            completion_snippets: false,
            publication_versions: HashMap::new(),
            reports: HashMap::new(),
            project: project::ProjectState::default(),
        }
    }
}

impl Inner {
    fn document(&self, uri: &Url) -> Option<&OpenDoc> {
        self.docs.get(uri).or_else(|| {
            let key = crate::include_resolver::normalize_uri(uri.as_str());
            self.docs
                .iter()
                .find(|(candidate, _)| {
                    crate::include_resolver::normalize_uri(candidate.as_str()) == key
                })
                .map(|(_, doc)| doc)
        })
    }
    fn name_views(&mut self, uri: &Url) -> Option<Vec<(Model, crate::model_map::WrittenModelMap)>> {
        let mut roots = self.known_owner_roots(uri);
        if roots.is_empty() && is_model_root(uri) {
            roots.push(uri.clone());
        }
        if roots.is_empty() {
            let model = self.workspace.get_model(uri.as_str())?.clone();
            let text = self.workspace.get_source(uri.as_str())?;
            return Some(vec![(model, crate::expand::expand_report(text).model_map)]);
        }
        let mut views = Vec::new();
        for root in roots {
            self.prepare_root(&root);
            let model = self.workspace.get_effective_model(root.as_str())?.clone();
            let map = self
                .workspace
                .expand_report(root.as_str())?
                .model_map
                .clone();
            views.push((model, map));
        }
        Some(views)
    }
    fn prepare_root(&mut self, uri: &Url) -> ResourceSettings {
        let settings = self.settings.resolve(uri);
        self.workspace
            .set_root_search_paths(uri.as_str(), settings.search_paths.clone());
        settings
    }

    fn presentation_for(&self, uri: &Url) -> PresentationSettings {
        self.settings.resolve(uri).presentation
    }

    fn known_owner_roots(&self, uri: &Url) -> Vec<Url> {
        let owners = self.workspace.owner_roots(uri.as_str());
        let mut roots: Vec<_> = self
            .tracked_roots
            .keys()
            .filter(|root| owners.contains(&self.project.observed_identity(root)))
            .cloned()
            .collect();
        roots.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        roots.extend(self.project.owners(uri));
        roots.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        let mut seen = HashSet::new();
        roots.retain(|root| seen.insert(self.project.observed_identity(root)));
        roots
    }

    fn root_revision(&mut self, uri: &Url) -> Option<String> {
        self.remember_request_root(uri);
        let settings = self.prepare_root(uri);
        let inputs = self.workspace.input_revision(uri.as_str())?;
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        settings.hash(&mut hash);
        inputs.hash(&mut hash);
        let revision = format!("{:016x}", hash.finish());
        self.tracked_roots
            .entry(uri.clone())
            .or_insert_with(|| revision.clone());
        Some(revision)
    }

    fn changed_model_info(&mut self) -> Vec<Value> {
        let mut roots: Vec<_> = self.tracked_roots.keys().cloned().collect();
        roots.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        let mut changes = Vec::new();
        for root in roots {
            let previous = self.tracked_roots.get(&root).cloned();
            let revision = self.root_revision(&root);
            if previous.as_ref() != revision.as_ref() {
                if let Some(revision) = &revision {
                    self.tracked_roots.insert(root.clone(), revision.clone());
                } else {
                    self.tracked_roots.remove(&root);
                }
                let input_revision = self.workspace.input_revision(root.as_str());
                changes.push(json!({"schema_version": MODEL_INFO_SCHEMA_VERSION, "root_uri": root, "revision": revision, "input_revision": input_revision}));
            }
        }
        changes
    }

    /// Recheck each open compilation unit, then route writing summaries to their source file.
    /// Publishing the union with the previous routes clears notes whose owner changed.
    fn refresh_diagnostics(
        &mut self,
        first: Option<&Url>,
    ) -> Vec<(Url, Option<i32>, Vec<Diagnostic>)> {
        let mut roots: Vec<Url> = self.docs.keys().cloned().collect();
        roots.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        for uri in roots {
            if !is_model_root(&uri) && !self.known_owner_roots(&uri).is_empty() {
                self.reports.remove(&uri);
                continue;
            }
            if is_model_root(&uri) {
                self.root_revision(&uri);
            } else {
                self.prepare_root(&uri);
            }
            let set = check_in_workspace_with_origins(&mut self.workspace, uri.as_str());
            let revision = self
                .workspace
                .input_revision(uri.as_str())
                .unwrap_or_default();
            let text = self
                .document(&uri)
                .map(|doc| Arc::from(doc.text.as_str()))
                .unwrap_or_default();
            let report = prepare_root_report(&uri, set, text, revision);
            self.reports.insert(uri, Arc::new(report));
        }
        self.reports
            .retain(|root, _| self.docs.contains_key(root) || self.project.is_selected(root));
        self.merge_diagnostics(first)
    }

    /// Route the one report per compilation unit. This never computes a model.
    fn merge_diagnostics(
        &mut self,
        first: Option<&Url>,
    ) -> Vec<(Url, Option<i32>, Vec<Diagnostic>)> {
        let mut checks: Vec<_> = self
            .reports
            .iter()
            .map(|(root, report)| (root.clone(), Arc::clone(report)))
            .collect();
        checks.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
        let mut routed: HashMap<Url, Vec<Diagnostic>> = self
            .docs
            .keys()
            .chain(self.reports.keys())
            .cloned()
            .map(|uri| (uri, Vec::new()))
            .collect();
        let mut routed_from: HashMap<Url, Vec<Url>> = HashMap::new();
        let mut library_routed: HashMap<Url, Vec<crate::Diagnostic>> = HashMap::new();
        let mut routed_library: HashMap<Url, Vec<RoutedDiagnostic>> = HashMap::new();
        // Resolve each URI once for this merge. The next merge observes the
        // filesystem again, including aliases whose target has changed.
        let mut identities = HashMap::new();
        let mut identity = |uri: &Url| {
            identities
                .entry(uri.clone())
                .or_insert_with(|| crate::include_resolver::normalize_uri(uri.as_str()))
                .clone()
        };
        let mut source_identities: HashMap<_, _> = self
            .reports
            .keys()
            .map(|uri| (identity(uri), uri.clone()))
            .collect();
        source_identities.extend(self.docs.keys().map(|uri| (identity(uri), uri.clone())));
        for (root, report) in &checks {
            for (source, rows) in &report.routes {
                let key = identity(source);
                let uri = source_identities
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| source.clone());
                for row in rows {
                    routed_library
                        .entry(uri.clone())
                        .or_default()
                        .push(row.clone());
                    routed
                        .entry(uri.clone())
                        .or_default()
                        .push(row.lsp_diagnostic.clone());
                    routed_from
                        .entry(uri.clone())
                        .or_default()
                        .push(root.clone());
                    if self.docs.contains_key(&uri) {
                        library_routed
                            .entry(uri.clone())
                            .or_default()
                            .push(row.diagnostic.clone());
                    }
                }
            }
        }
        for (uri, items) in &mut routed {
            let mut unique = Vec::new();
            let mut per_root: HashMap<(Url, DiagnosticPresentationKey), usize> = HashMap::new();
            let mut retained: HashSet<(DiagnosticPresentationKey, usize)> = HashSet::new();
            let contexts = routed_from.remove(uri).unwrap_or_default();
            for (source_root, item) in contexts.into_iter().zip(std::mem::take(items)) {
                let key = diagnostic_presentation_key(&item);
                let occurrence = per_root.entry((source_root, key.clone())).or_default();
                let ordinal = *occurrence;
                *occurrence += 1;
                if retained.insert((key, ordinal)) {
                    unique.push(item);
                }
            }
            *items = unique;
        }
        self.routed_library = routed_library;
        for (uri, doc) in &mut self.docs {
            let mut unique: Vec<crate::Diagnostic> = Vec::new();
            for diag in library_routed.remove(uri).unwrap_or_default() {
                if !unique.iter().any(|prior| {
                    prior.span == diag.span
                        && prior.code == diag.code
                        && prior.severity == diag.severity
                        && prior.message == diag.message
                        && prior.fix == diag.fix
                        && prior.related == diag.related
                }) {
                    unique.push(diag);
                }
            }
            doc.library = unique;
        }
        for (uri, doc) in &mut self.docs {
            doc.diagnostics = routed.get(uri).cloned().unwrap_or_default();
        }
        let mut publish: Vec<Url> = self
            .published
            .keys()
            .chain(routed.keys())
            .cloned()
            .collect();
        publish.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        publish.dedup();
        if let Some(first) = first {
            if let Some(at) = publish.iter().position(|uri| uri == first) {
                let first = publish.remove(at);
                let key = identity(&first);
                let mut aliases: Vec<_> = publish
                    .iter()
                    .filter(|uri| !routed.contains_key(*uri) && identity(uri) == key)
                    .cloned()
                    .collect();
                publish.retain(|uri| !aliases.contains(uri));
                // Clear a superseded URI before its native-equivalent current
                // report, including clients whose collections ignore case.
                aliases.push(first);
                aliases.append(&mut publish);
                publish = aliases;
            }
        }
        let mut open_versions = HashMap::new();
        for (uri, doc) in &self.docs {
            // Match document(): an exact URI wins, otherwise the first native
            // identity match in the open-document iteration supplies its version.
            open_versions.entry(identity(uri)).or_insert(doc.version);
        }
        let current_versions: HashMap<_, _> = publish
            .iter()
            .map(|uri| {
                let version = self
                    .docs
                    .get(uri)
                    .map(|doc| doc.version)
                    .or_else(|| open_versions.get(&identity(uri)).copied());
                (uri.clone(), version)
            })
            .collect();
        let output = publish
            .into_iter()
            .filter(|uri| {
                Some(uri) == first
                    || self.published.get(uri) != routed.get(uri)
                    || self.publication_versions.get(uri).copied() != Some(current_versions[uri])
            })
            .map(|uri| {
                let version = routed
                    .contains_key(&uri)
                    .then(|| current_versions[&uri])
                    .flatten();
                let diagnostics = routed.get(&uri).cloned().unwrap_or_default();
                (uri, version, diagnostics)
            })
            .collect();
        self.publication_versions = routed
            .keys()
            .map(|uri| (uri.clone(), current_versions[uri]))
            .collect();
        self.published = routed;
        output
    }
}

/// Language server backend (document overlay + diagnostics).
pub struct Backend {
    client: Client,
    inner: Arc<Mutex<Inner>>,
    project_wake: Arc<tokio::sync::Notify>,
    output_gate: Arc<tokio::sync::Mutex<()>>,
}

impl std::fmt::Debug for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Backend").finish_non_exhaustive()
    }
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            inner: Arc::new(Mutex::new(Inner::default())),
            project_wake: Arc::new(tokio::sync::Notify::new()),
            output_gate: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    fn lock_inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn presentation_settings(&self, uri: &Url) -> PresentationSettings {
        self.lock_inner().presentation_for(uri)
    }

    pub fn model_input_revision(&self, root: &Url) -> Option<String> {
        self.lock_inner().root_revision(root)
    }

    pub fn known_model_roots(&self, document: &Url) -> Vec<Url> {
        self.lock_inner().known_owner_roots(document)
    }

    async fn refresh_presentation(&self, settings_changed: bool) {
        let (changes, notify, tokens, hints) = {
            let mut inner = self.lock_inner();
            let changes = inner.changed_model_info();
            let refresh = settings_changed || !changes.is_empty();
            (
                changes,
                inner.model_info_notifications,
                refresh && inner.token_refresh,
                refresh && inner.hint_refresh,
            )
        };
        if notify {
            for change in changes {
                self.client
                    .send_notification::<ModelInfoChanged>(change)
                    .await;
            }
        }
        if tokens {
            let _ = self.client.semantic_tokens_refresh().await;
        }
        if hints {
            let _ = self.client.inlay_hint_refresh().await;
        }
    }

    fn upsert(
        &self,
        uri: Url,
        text: String,
        version: i32,
    ) -> Vec<(Url, Option<i32>, Vec<Diagnostic>)> {
        let mut inner = self.lock_inner();
        let key = crate::include_resolver::normalize_uri(uri.as_str());
        inner.docs.retain(|candidate, _| {
            candidate == &uri || crate::include_resolver::normalize_uri(candidate.as_str()) != key
        });
        inner.align_project_root(&uri);
        inner.project_changed(&uri, false);
        if is_model_root(&uri) {
            inner.project.active = Some(uri.clone());
        }
        inner.workspace.update_document(uri.as_str(), &text);
        inner.docs.insert(
            uri.clone(),
            OpenDoc {
                text,
                version,
                diagnostics: Vec::new(),
                library: Vec::new(),
            },
        );
        inner.refresh_diagnostics(Some(&uri))
    }

    fn pull_items(&self, uri: &Url) -> Vec<Diagnostic> {
        let inner = self.lock_inner();
        inner
            .published
            .get(uri)
            .or_else(|| {
                let key = crate::include_resolver::normalize_uri(uri.as_str());
                inner
                    .published
                    .iter()
                    .find(|(candidate, _)| {
                        crate::include_resolver::normalize_uri(candidate.as_str()) == key
                    })
                    .map(|(_, items)| items)
            })
            .cloned()
            .unwrap_or_default()
    }

    fn hover_at(&self, pos: &TextDocumentPositionParams) -> Option<Hover> {
        let mut inner = self.lock_inner();
        if is_model_root(&pos.text_document.uri) {
            inner.project.active = Some(pos.text_document.uri.clone());
        }
        let text = inner.document(&pos.text_document.uri)?.text.clone();
        let normalized = crate::parser::normalize_newlines(&text);
        let index = LineIndex::new(&normalized);
        let byte = index.offset_utf16(&text, span_pos(pos.position));
        let (word, span) = ident_at(&text, byte)?;
        let range = Some(span_range(&index, &text, span));
        if let Some(cmd) = option_command_at(&text, byte) {
            if let Some((name, command_doc)) = command_options(&cmd)
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(&word))
            {
                let mut md = format!("**`{cmd}` option**: `{word}`");
                let description = shocks_overwrite_doc(&text, byte, &cmd, name, command_doc);
                if !description.is_empty() {
                    md.push_str("\n\n");
                    md.push_str(description);
                }
                return Some(markdown_hover(md, range));
            }
        }
        if word.eq_ignore_ascii_case("heterogeneity") {
            if let Some(head) = heterogeneity_declaration_head(&text, byte) {
                let md = format!("**`{head}` option**: `heterogeneity`\n\n{HETEROGENEITY_OPTION}");
                return Some(markdown_hover(md, range));
            }
        }
        if let Some(help) = family_help(&word) {
            return Some(markdown_hover(format!("**`{word}`**\n\n{help}"), range));
        }
        let preferences = inner.presentation_for(&pos.text_document.uri);
        let views = inner.name_views(&pos.text_document.uri)?;
        let mut descriptions = views
            .iter()
            .map(|(model, _)| decl_hover_markdown(model, &word, &preferences));
        let md = descriptions.next()??;
        if !descriptions.all(|description| description.as_ref() == Some(&md)) {
            return None;
        }
        Some(markdown_hover(md, range))
    }

    fn doc_symbols(&self, uri: &Url) -> Option<Vec<DocumentSymbol>> {
        let mut inner = self.lock_inner();
        let text = inner.document(uri)?.text.clone();
        let preferences = inner.presentation_for(uri);
        let owners = inner.known_owner_roots(uri);
        let roots = if owners.is_empty() && is_model_root(uri) {
            vec![uri.clone()]
        } else {
            owners
        };
        let mut views = Vec::new();
        for root in &roots {
            inner.root_revision(root);
            let model = inner.workspace.get_effective_model(root.as_str())?.clone();
            let report = inner.workspace.expand_report(root.as_str())?.clone();
            let authoritative = roots.len() == 1
                && report.complete
                && report.model_map.complete
                && inner.workspace.includes_complete(root.as_str());
            let view = WrittenView::new(
                uri,
                &text,
                &model,
                &report.model_map,
                Some(&inner.workspace),
                authoritative,
                roots.len() == 1,
            )
            .with_known_uris(inner.docs.keys().chain(std::iter::once(root)));
            views.push(view.symbols(&preferences));
        }
        if roots.len() > 1 {
            return Some(common_leaves(views));
        }
        if let Some(view) = views.pop().filter(|view| !view.is_empty()) {
            return Some(view);
        }
        let model = inner.workspace.get_model(uri.as_str())?;
        let report = crate::expand::expand_report(&text);
        Some(
            WrittenView::new(uri, &text, model, &report.model_map, None, false, false)
                .symbols(&preferences),
        )
    }

    fn workspace_symbols(&self, query: &str) -> Vec<SymbolInformation> {
        let q = query.to_ascii_lowercase();
        let mut out = Vec::new();
        let uris: Vec<_> = self.lock_inner().docs.keys().cloned().collect();
        for uri in uris {
            let Some(nested) = self.doc_symbols(&uri) else {
                continue;
            };
            flatten_workspace_symbols(&uri, &nested, None, &q, &mut out);
        }
        out
    }

    fn value_hints(&self, params: &InlayHintParams) -> Vec<InlayHint> {
        let uri = &params.text_document.uri;
        let mut inner = self.lock_inner();
        if !inner.presentation_for(uri).parameter_value_hints {
            return Vec::new();
        }
        let owners = inner.known_owner_roots(uri);
        let root = if owners.len() == 1 {
            owners[0].clone()
        } else if owners.is_empty() && is_model_root(uri) {
            uri.clone()
        } else {
            return Vec::new();
        };
        let Some(revision) = inner.root_revision(&root) else {
            return Vec::new();
        };
        let Some(model) = inner.workspace.get_effective_model(root.as_str()).cloned() else {
            return Vec::new();
        };
        let Some(report) = inner.workspace.expand_report(root.as_str()).cloned() else {
            return Vec::new();
        };
        if !report.complete
            || !report.model_map.complete
            || !inner.workspace.includes_complete(root.as_str())
        {
            return Vec::new();
        }
        let key = crate::include_resolver::normalize_uri(uri.as_str());
        let Some(text) = inner.workspace.get_source(uri.as_str()) else {
            return Vec::new();
        };
        // All executions at an anchor take part, including unknown values.
        let mut sites: std::collections::BTreeMap<(u32, u32), ValueHintSite> =
            std::collections::BTreeMap::new();
        for value in crate::assignment_values::assignment_values(&model) {
            let source = &report.model_map.statements[value.statement_id];
            let Some(anchor) = source
                .anchor
                .as_ref()
                .filter(|anchor| anchor.file.as_deref() == Some(key.as_str()))
            else {
                continue;
            };
            let range = if source.segments.len() == 1
                && source.segments[0].file.as_deref() == Some(key.as_str())
            {
                crate::server_model_map::written_range(text, source.segments[0].span)
            } else {
                None
            };
            let proof = value.value.filter(|_| range.is_some());
            sites
                .entry((anchor.span.start, anchor.span.end))
                .and_modify(|site| {
                    if site.value.map(f64::to_bits) != proof.map(f64::to_bits)
                        || site.range != range
                    {
                        site.value = None;
                    }
                    site.plain |= value.written_plain_number;
                })
                .or_insert(ValueHintSite {
                    value: proof,
                    range,
                    plain: value.written_plain_number,
                    statement_id: value.statement_id,
                });
        }
        sites.into_values().filter_map(|ValueHintSite {value, range, plain, statement_id:id}| {
            let value = value?;
            let position = range?.end;
            if plain || !pos_in_range(position, params.range) { return None; }
            Some(InlayHint {
                position, label: InlayHintLabel::String(format!("= {value}")), kind: None,
                text_edits: None, tooltip: Some(InlayHintTooltip::String("Value from this expression and earlier assignments.".into())),
                padding_left: Some(true), padding_right: Some(false),
                data: Some(json!({"root_uri":root,"revision":revision,"statement_id":format!("s{id}")})),
            })
        }).collect()
    }

    fn decl_location(&self, pos: &TextDocumentPositionParams) -> Option<Location> {
        let inner = self.lock_inner();
        let doc = inner.document(&pos.text_document.uri)?;
        let index = LineIndex::new(&doc.text);
        let byte = index.offset_utf16(&doc.text, span_pos(pos.position));
        let (word, _) = ident_at(&doc.text, byte)?;
        let model = inner.workspace.get_model(pos.text_document.uri.as_str())?;
        let decl = find_decl(model, &word)?;
        Some(Location::new(
            pos.text_document.uri.clone(),
            span_range(&index, &doc.text, decl.span),
        ))
    }

    fn definition_at(&self, pos: &TextDocumentPositionParams) -> Option<GotoDefinitionResponse> {
        let mut inner = self.lock_inner();
        let uri = &pos.text_document.uri;
        let doc = inner.document(uri)?;
        let text = doc.text.clone();
        let index = LineIndex::new(&text);
        let byte = index.offset_utf16(&text, span_pos(pos.position));
        let covering = inner
            .workspace
            .companion_records(uri.as_str())
            .map(|recs| {
                recs.iter()
                    .filter(|r| r.named_in.start <= byte && byte < r.named_in.end)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !covering.is_empty() {
            let locs: Vec<Location> = covering
                .iter()
                .filter_map(|r| {
                    let path = r.path.as_ref()?;
                    let target = Url::from_file_path(path).ok()?;
                    Some(Location::new(
                        target,
                        Range::new(Position::new(0, 0), Position::new(0, 0)),
                    ))
                })
                .collect();
            return match locs.len() {
                0 => None,
                1 => Some(GotoDefinitionResponse::Scalar(locs.into_iter().next()?)),
                _ => Some(GotoDefinitionResponse::Array(locs)),
            };
        }
        let (word, _) = ident_at(&text, byte)?;
        let model = inner.workspace.get_model(uri.as_str())?;
        let decl = find_decl(model, &word)?;
        Some(GotoDefinitionResponse::Scalar(Location::new(
            uri.clone(),
            span_range(&index, &text, decl.span),
        )))
    }

    fn ident_locations(&self, pos: &TextDocumentPositionParams) -> Option<Vec<Location>> {
        let mut inner = self.lock_inner();
        let doc = inner.document(&pos.text_document.uri)?;
        let index = LineIndex::new(&doc.text);
        let byte = index.offset_utf16(&doc.text, span_pos(pos.position));
        let (word, _) = ident_at(&doc.text, byte)?;
        let active_key = crate::include_resolver::normalize_uri(pos.text_document.uri.as_str());
        let mut roots = vec![pos.text_document.uri.clone()];
        let open: Vec<Url> = inner.docs.keys().cloned().collect();
        for uri in open {
            if uri == pos.text_document.uri {
                continue;
            }
            if inner
                .workspace
                .resolve_all_includes(uri.as_str())
                .contains_key(&active_key)
            {
                roots.push(uri);
            }
        }
        let mut scope = HashSet::new();
        for root in roots {
            scope.insert(crate::include_resolver::normalize_uri(root.as_str()));
            scope.extend(
                inner
                    .workspace
                    .resolve_all_includes(root.as_str())
                    .into_keys(),
            );
        }
        let mut scope: Vec<String> = scope.into_iter().collect();
        scope.sort();
        let mut locs = Vec::new();
        for file in scope {
            let Some(text) = inner.workspace.get_source(&file) else {
                continue;
            };
            let uri = inner
                .docs
                .keys()
                .find(|uri| crate::include_resolver::normalize_uri(uri.as_str()) == file)
                .cloned()
                .or_else(|| file_url_from_path_key(&file));
            let Some(uri) = uri else { continue };
            let index = LineIndex::new(text);
            locs.extend(
                occurrences(text, &word)
                    .into_iter()
                    .map(|span| Location::new(uri.clone(), span_range(&index, text, span))),
            );
        }
        if locs.is_empty() {
            None
        } else {
            Some(locs)
        }
    }

    fn ident_highlights(&self, pos: &TextDocumentPositionParams) -> Option<Vec<DocumentHighlight>> {
        let mut inner = self.lock_inner();
        let text = inner.document(&pos.text_document.uri)?.text.clone();
        let normalized = crate::parser::normalize_newlines(&text);
        let index = LineIndex::new(&normalized);
        let byte = index.offset_utf16(&text, span_pos(pos.position));
        let (word, _) = ident_at(&text, byte)?;
        let views = inner.name_views(&pos.text_document.uri)?;
        let sites: Vec<_> = views
            .iter()
            .map(|(model, map)| NameSites::new(model, map, &pos.text_document.uri, &text))
            .collect();
        let hits = occurrences(&text, &word)
            .into_iter()
            .map(|span| DocumentHighlight {
                range: span_range(&index, &text, span),
                kind: Some(if sites.iter().all(|sites| sites.is_write(span)) {
                    DocumentHighlightKind::WRITE
                } else {
                    DocumentHighlightKind::READ
                }),
            })
            .collect::<Vec<_>>();
        if hits.is_empty() {
            None
        } else {
            Some(hits)
        }
    }

    fn complete(&self, pos: &TextDocumentPositionParams) -> Option<CompletionResponse> {
        let mut inner = self.lock_inner();
        let text = inner.document(&pos.text_document.uri)?.text.clone();
        let normalized = crate::parser::normalize_newlines(&text);
        let index = LineIndex::new(&normalized);
        let byte = index.offset_utf16(&text, span_pos(pos.position));
        if let Some(item) = metadata_completion_item(&mut inner, &pos.text_document.uri, byte) {
            return Some(CompletionResponse::Array(vec![item]));
        }
        if let Some(cmd) = option_command_at(&text, byte) {
            let items = command_options(&cmd)
                .iter()
                .map(|(name, doc_str)| {
                    let documentation = shocks_overwrite_doc(&text, byte, &cmd, name, doc_str);
                    CompletionItem {
                        label: (*name).into(),
                        kind: Some(CompletionItemKind::PROPERTY),
                        detail: Some(format!("{cmd} option")),
                        documentation: Some(Documentation::String(documentation.into())),
                        ..CompletionItem::default()
                    }
                })
                .collect::<Vec<_>>();
            if items.is_empty() {
                return None;
            }
            return Some(CompletionResponse::Array(items));
        }
        if let Some(head) = heterogeneity_declaration_head(&text, byte) {
            return Some(CompletionResponse::Array(vec![CompletionItem {
                label: "heterogeneity".into(),
                kind: Some(CompletionItemKind::PROPERTY),
                detail: Some(format!("{head} option")),
                documentation: Some(Documentation::String(HETEROGENEITY_OPTION.into())),
                ..CompletionItem::default()
            }]));
        }
        let preferences = inner.presentation_for(&pos.text_document.uri);
        let views = inner.name_views(&pos.text_document.uri)?;
        let mut groups = views.iter().map(|(model, _)| {
            default_completions(
                model,
                &preferences,
                inner.completion_label_details,
                inner.completion_snippets,
            )
        });
        let mut items = groups.next()?;
        for group in groups {
            items.retain(|item| group.contains(item));
        }
        Some(CompletionResponse::Array(items))
    }

    fn signature_at(&self, pos: &TextDocumentPositionParams) -> Option<SignatureHelp> {
        let inner = self.lock_inner();
        let doc = inner.document(&pos.text_document.uri)?;
        let index = LineIndex::new(&doc.text);
        let byte = index.offset_utf16(&doc.text, span_pos(pos.position));
        crate::signature_help::signature_help(&doc.text, byte)
    }

    fn prepare_rename_at(&self, pos: &TextDocumentPositionParams) -> Option<Range> {
        let inner = self.lock_inner();
        let doc = inner.document(&pos.text_document.uri)?;
        let index = LineIndex::new(&doc.text);
        let byte = index.offset_utf16(&doc.text, span_pos(pos.position));
        let (word, span) = ident_at(&doc.text, byte)?;
        if !is_legal_ident(&word) {
            return None;
        }
        if !is_declared_in_open(&inner, &word) {
            return None;
        }
        Some(span_range(&index, &doc.text, span))
    }

    fn rename_at(&self, pos: &TextDocumentPositionParams, new_name: &str) -> Option<WorkspaceEdit> {
        if !is_legal_ident(new_name) {
            return None;
        }
        let inner = self.lock_inner();
        let doc = inner.document(&pos.text_document.uri)?;
        let index = LineIndex::new(&doc.text);
        let byte = index.offset_utf16(&doc.text, span_pos(pos.position));
        let (word, _) = ident_at(&doc.text, byte)?;
        if !is_legal_ident(&word) || !is_declared_in_open(&inner, &word) {
            return None;
        }
        let mut changes = HashMap::new();
        for (uri, open) in &inner.docs {
            if !uri_is_mod_or_inc(uri) {
                continue;
            }
            let idx = LineIndex::new(&open.text);
            let mut spans = occurrences(&open.text, &word);
            if spans.is_empty() {
                continue;
            }
            spans.sort_by_key(|s| std::cmp::Reverse(s.start));
            let edits = spans
                .into_iter()
                .map(|span| TextEdit {
                    range: span_range(&idx, &open.text, span),
                    new_text: new_name.to_string(),
                })
                .collect::<Vec<_>>();
            if !edits.is_empty() {
                changes.insert(uri.clone(), edits);
            }
        }
        if changes.is_empty() {
            None
        } else {
            Some(WorkspaceEdit {
                changes: Some(changes),
                ..WorkspaceEdit::default()
            })
        }
    }

    fn quick_fixes(&self, params: &CodeActionParams) -> Option<CodeActionResponse> {
        let mut inner = self.lock_inner();
        let mut actions: Vec<_> = naming_code_actions(&mut inner, params)
            .into_iter()
            .map(CodeActionOrCommand::CodeAction)
            .collect();
        if !action_kind_requested(&params.context.only, &CodeActionKind::QUICKFIX) {
            return (!actions.is_empty()).then_some(actions);
        }
        let uri = &params.text_document.uri;
        let rows = inner.routed_library.get(uri).cloned().unwrap_or_default();
        let mut seen_fixes = HashSet::new();
        for row in rows {
            let lib = &row.diagnostic;
            let Some(fix) = &lib.fix else { continue };
            inner.prepare_root(&row.root);
            if inner.workspace.input_revision(row.root.as_str()).as_deref() != Some(&row.revision) {
                continue;
            }
            let current = inner
                .document(uri)
                .map(|doc| doc.text.as_str())
                .or_else(|| inner.workspace.get_source(uri.as_str()));
            if current != Some(row.text.as_ref()) {
                continue;
            }
            let index = LineIndex::new(&row.text);
            let diag_range = span_range(&index, &row.text, lib.span);
            if !ranges_overlap(diag_range, params.range) {
                continue;
            }
            if !params.context.diagnostics.is_empty()
                && !context_owns_stored_fix(&params.context, lib, diag_range, &row)
            {
                continue;
            }
            let seen_key = (
                row.root.to_string(),
                row.revision.clone(),
                lib.code.clone(),
                lib.span.start,
                lib.span.end,
                fix.new_text.clone(),
                fix.start_line,
                fix.start_char,
                fix.end_line,
                fix.end_char,
            );
            if !seen_fixes.insert(seen_key) {
                continue;
            }
            let edit = TextEdit {
                range: Range::new(
                    lsp_pos_from_scalar(&index, &row.text, fix.start_line, fix.start_char),
                    lsp_pos_from_scalar(&index, &row.text, fix.end_line, fix.end_char),
                ),
                new_text: fix.new_text.clone(),
            };
            let action = CodeActionOrCommand::CodeAction(CodeAction {
                title: fix_title(lib),
                kind: Some(CodeActionKind::QUICKFIX),
                edit: Some(versioned_workspace_edit(
                    &inner,
                    HashMap::from([(uri.clone(), vec![edit])]),
                )),
                is_preferred: Some(true),
                diagnostics: Some(vec![row.lsp_diagnostic.clone()]),
                ..CodeAction::default()
            });
            actions.push(action);
        }
        (!actions.is_empty()).then_some(actions)
    }
    fn shock_templates(&self, params: &CodeActionParams) -> Vec<CodeAction> {
        let kind = CodeActionKind::REFACTOR;
        if !action_kind_requested(&params.context.only, &kind) {
            return Vec::new();
        }
        let uri = &params.text_document.uri;
        let mut inner = self.lock_inner();
        let Some(text) = inner.document(uri).map(|doc| doc.text.clone()) else {
            return Vec::new();
        };
        let (kinds, stochastic_names, deterministic_names) = {
            let Some(model) = inner.workspace.get_effective_model(uri.as_str()) else {
                return Vec::new();
            };
            if crate::shock_template::has_ordinary_shocks(model) {
                return Vec::new();
            }
            (
                crate::shock_template::available_kinds(model),
                crate::shock_template::aggregate_varexo_names(model),
                crate::shock_template::aggregate_deterministic_names(model),
            )
        };
        let index = LineIndex::new(&text);
        kinds
            .into_iter()
            .filter_map(|template_kind| {
                let names = match template_kind {
                    crate::shock_template::TemplateKind::Stochastic => &stochastic_names,
                    crate::shock_template::TemplateKind::Deterministic => &deterministic_names,
                };
                let edit = crate::shock_template::insertion(&text, names, template_kind)?;
                let pos = index.position_utf16(&text, edit.byte);
                let position = Position::new(pos.line, pos.character);
                let mut changes = HashMap::new();
                changes.insert(
                    uri.clone(),
                    vec![TextEdit {
                        range: Range::new(position, position),
                        new_text: edit.new_text,
                    }],
                );
                Some(CodeAction {
                    title: template_kind.title().into(),
                    kind: Some(kind.clone()),
                    diagnostics: None,
                    edit: Some(versioned_workspace_edit(&inner, changes)),
                    command: None,
                    is_preferred: Some(false),
                    disabled: None,
                    data: None,
                })
            })
            .collect()
    }

    fn linked_ranges(&self, pos: &TextDocumentPositionParams) -> Option<LinkedEditingRanges> {
        let inner = self.lock_inner();
        let doc = inner.document(&pos.text_document.uri)?;
        let index = LineIndex::new(&doc.text);
        let byte = index.offset_utf16(&doc.text, span_pos(pos.position));
        let (word, _) = ident_at(&doc.text, byte)?;
        if !is_declared_in_open(&inner, &word) {
            return None;
        }
        let spans = occurrences(&doc.text, &word);
        if spans.len() < 2 {
            return None;
        }
        Some(LinkedEditingRanges {
            ranges: spans
                .into_iter()
                .map(|span| span_range(&index, &doc.text, span))
                .collect(),
            word_pattern: None,
        })
    }

    fn apply_settings(&self, settings: &Value) -> Vec<String> {
        let mut inner = self.lock_inner();
        let explanations = inner.settings.apply(settings);
        inner.project_reconfigure(true);
        explanations
    }

    fn folding_ranges(&self, uri: &Url) -> Option<Vec<FoldingRange>> {
        let inner = self.lock_inner();
        let doc = inner.document(uri)?;
        let model = inner.workspace.get_model(uri.as_str())?;
        let normalized = crate::parser::normalize_newlines(&doc.text);
        let index = LineIndex::new(&normalized);
        let report = crate::expand::expand_report(&doc.text);
        let mut ranges =
            WrittenView::new(uri, &doc.text, model, &report.model_map, None, false, false).folds();
        let mut stack: Vec<&crate::model::MacroDirective> = Vec::new();
        const COND: &[&str] = &["if", "ifdef", "ifndef"];
        for directive in &model.macro_directives {
            let kind = directive.kind.as_str();
            if COND.contains(&kind) || kind == "for" {
                stack.push(directive);
            } else if kind == "endif"
                && stack
                    .last()
                    .is_some_and(|d| COND.contains(&d.kind.as_str()))
            {
                let opener = stack.pop().unwrap();
                let start = index.position_utf16(&doc.text, opener.span.start);
                let end = index.position_utf16(&doc.text, directive.span.end);
                if start.line < end.line {
                    ranges.push(FoldingRange {
                        start_line: start.line,
                        start_character: None,
                        end_line: end.line,
                        end_character: None,
                        kind: Some(FoldingRangeKind::Region),
                        collapsed_text: Some(format!("@#{} ... @#endif", opener.kind)),
                    });
                }
            } else if kind == "endfor" && stack.last().is_some_and(|d| d.kind == "for") {
                let opener = stack.pop().unwrap();
                let start = index.position_utf16(&doc.text, opener.span.start);
                let end = index.position_utf16(&doc.text, directive.span.end);
                if start.line < end.line {
                    ranges.push(FoldingRange {
                        start_line: start.line,
                        start_character: None,
                        end_line: end.line,
                        end_character: None,
                        kind: Some(FoldingRangeKind::Region),
                        collapsed_text: Some("@#for ... @#endfor".into()),
                    });
                }
            }
        }
        for (start_line, end_line) in block_comment_folds(&doc.text) {
            ranges.push(FoldingRange {
                start_line,
                start_character: None,
                end_line,
                end_character: None,
                kind: Some(FoldingRangeKind::Comment),
                collapsed_text: None,
            });
        }
        if ranges.is_empty() {
            None
        } else {
            Some(ranges)
        }
    }

    fn selection_ranges(&self, uri: &Url, positions: &[Position]) -> Option<Vec<SelectionRange>> {
        let inner = self.lock_inner();
        let doc = inner.document(uri)?;
        let model = inner.workspace.get_model(uri.as_str())?;
        let index = LineIndex::new(&doc.text);
        let file_range = full_document_range(&doc.text);
        let mut out = Vec::new();
        for &pos in positions {
            let mut chain = Vec::new();
            let byte = index.offset_utf16(&doc.text, span_pos(pos));
            if let Some((_, span)) = ident_at(&doc.text, byte) {
                chain.push(span_range(&index, &doc.text, span));
            }
            if let Some(eq) = equation_at(model, &index, &doc.text, pos) {
                chain.push(span_range(&index, &doc.text, eq.span));
            }
            for span in [
                model.model_block,
                model.ss_block,
                model.initval_block,
                model.endval_block,
                model.shocks_block,
            ]
            .into_iter()
            .flatten()
            {
                let rng = span_range(&index, &doc.text, span);
                if range_has_pos(rng, pos) {
                    chain.push(rng);
                }
            }
            chain.push(file_range);
            chain.sort_by(|a, b| {
                (b.start.line, b.start.character, a.end.line, a.end.character).cmp(&(
                    a.start.line,
                    a.start.character,
                    b.end.line,
                    b.end.character,
                ))
            });
            let mut nested: Vec<Range> = Vec::new();
            for rng in chain {
                if nested.is_empty() || range_strictly_contains(rng, nested[nested.len() - 1]) {
                    nested.push(rng);
                }
            }
            let mut node: Option<SelectionRange> = None;
            for rng in nested.into_iter().rev() {
                node = Some(SelectionRange {
                    range: rng,
                    parent: node.map(Box::new),
                });
            }
            out.push(node.unwrap_or(SelectionRange {
                range: Range::new(pos, pos),
                parent: None,
            }));
        }
        Some(out)
    }

    fn document_links(&self, uri: &Url) -> Option<Vec<DocumentLink>> {
        let mut inner = self.lock_inner();
        let text = inner.document(uri)?.text.clone();
        let includes = inner
            .workspace
            .get_model(uri.as_str())
            .map(|m| m.includes.clone())
            .unwrap_or_default();
        let index = LineIndex::new(&text);
        let records = inner
            .workspace
            .include_records(uri.as_str())
            .cloned()
            .unwrap_or_default();
        let companions = inner
            .workspace
            .companion_records(uri.as_str())
            .map(|c| c.to_vec())
            .unwrap_or_default();
        let mut links = Vec::new();
        for inc in &includes {
            let resolved = records.resolved.iter().find(|r| {
                (r.span.start == inc.span.start && r.span.end == inc.span.end)
                    || r.filename == inc.filename
            });
            let Some(resolved) = resolved else {
                continue;
            };
            let Ok(target) = Url::from_file_path(&resolved.path) else {
                continue;
            };
            links.push(DocumentLink {
                range: span_range(&index, &text, inc.span),
                target: Some(target),
                tooltip: Some(format!("Open {}", inc.filename)),
                data: None,
            });
        }
        for rec in &companions {
            let Some(path) = rec.path.as_ref() else {
                continue;
            };
            let Ok(target) = Url::from_file_path(path) else {
                continue;
            };
            let basename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(rec.name.as_str());
            links.push(DocumentLink {
                range: span_range(&index, &text, rec.named_in),
                target: Some(target),
                tooltip: Some(format!("{} {basename}", rec.kind.as_str())),
                data: None,
            });
        }
        if links.is_empty() {
            None
        } else {
            Some(links)
        }
    }

    fn semantic_tokens(&self, uri: &Url, range: Option<Range>) -> Option<SemanticTokens> {
        let mut inner = self.lock_inner();
        let text = inner.document(uri)?.text.clone();
        let views = inner.name_views(uri)?;
        let sites: Vec<_> = views
            .iter()
            .map(|(model, map)| {
                (
                    NameSites::new(model, map, uri, &text),
                    classify_variable_timing(model),
                )
            })
            .collect();
        let mapping = inner.semantic_mapping.clone();
        let normalized = crate::parser::normalize_newlines(&text);
        let index = LineIndex::new(&normalized);
        let mut raw = Vec::new();
        for tok in tokenize(&text) {
            if tok.kind != TokenKind::Ident {
                continue;
            }
            let name = tok.text(&text);
            let start = index.position_utf16(&text, tok.span.start);
            if let Some(range) = range {
                if !pos_in_range_half_open(Position::new(start.line, start.character), range) {
                    continue;
                }
            }
            let role = sites.first()?.0.role(name, tok.span);
            if !sites
                .iter()
                .all(|(sites, _)| sites.role(name, tok.span) == role)
            {
                continue;
            }
            let Some(role) = role else {
                continue;
            };
            let Some(ttype) = mapping.token_type(role) else {
                continue;
            };
            let mods = sites
                .iter()
                .map(|(sites, timing)| {
                    let mut mods = 0u32;
                    if sites.is_declaration(tok.span) {
                        mods |= mapping.declaration;
                    }
                    if role == NameRole::Endogenous {
                        if let Some(info) = timing.get(name) {
                            match info.class {
                                TimingClass::ForwardLooking | TimingClass::Mixed => {
                                    mods |= mapping.forward
                                }
                                _ => {}
                            }
                            match info.class {
                                TimingClass::Predetermined | TimingClass::Mixed => {
                                    mods |= mapping.predetermined
                                }
                                _ => {}
                            }
                        }
                    }
                    mods
                })
                .reduce(|common, modifiers| common & modifiers)
                .unwrap_or(0);
            let end = index.position_utf16(&text, tok.span.end);
            let length = if start.line == end.line {
                end.character.saturating_sub(start.character)
            } else {
                tok.text(&text).encode_utf16().count() as u32
            };
            raw.push((start.line, start.character, length, ttype, mods));
        }
        raw.sort_by_key(|t| (t.0, t.1));
        let mut data = Vec::new();
        let mut prev_line = 0u32;
        let mut prev_col = 0u32;
        for (line, col, length, ttype, mods) in raw {
            let delta_line = line - prev_line;
            let delta_start = if delta_line == 0 { col - prev_col } else { col };
            data.push(SemanticToken {
                delta_line,
                delta_start,
                length,
                token_type: ttype,
                token_modifiers_bitset: mods,
            });
            prev_line = line;
            prev_col = col;
        }
        Some(SemanticTokens {
            result_id: None,
            data,
        })
    }

    fn format_document(&self, uri: &Url) -> Option<Vec<TextEdit>> {
        let inner = self.lock_inner();
        let doc = inner.document(uri)?;
        let settings = inner.settings.resolve(uri);
        let formatted = format_text(&doc.text, &settings.format_indent_unit)?;
        Some(vec![full_document_edit(&doc.text, formatted)])
    }

    fn format_line_range(&self, uri: &Url, range: Range) -> Option<Vec<TextEdit>> {
        let inner = self.lock_inner();
        let doc = inner.document(uri)?;
        let mut end_line = range.end.line;
        if range.end.character == 0 && end_line > range.start.line {
            end_line -= 1;
        }
        let settings = inner.settings.resolve(uri);
        let (start_line, end_line, replacement) = format_range(
            &doc.text,
            range.start.line,
            end_line,
            &settings.format_indent_unit,
        )?;
        Some(vec![line_range_edit(
            &doc.text,
            start_line,
            end_line,
            replacement,
        )])
    }

    fn execute(&self, command: &str, arguments: &[Value]) -> Value {
        if let Some(root) = arguments
            .first()
            .and_then(|value| value.get("root_uri"))
            .and_then(Value::as_str)
            .and_then(|uri| Url::parse(uri).ok())
            .filter(is_model_root)
        {
            self.lock_inner().project.active = Some(root);
            self.project_wake.notify_one();
        }
        let result = match command {
            "dynare/explainDiagnostic" => explain_command(arguments),
            "dynare/compareModels" => self.compare_command(arguments),
            "dynare/showEffectiveModel" => self.show_effective_model_command(arguments),
            "dynare/modelInfo" => self.model_info_command(arguments),
            _ => json!({"error": format!("unknown command {command}"), "code": "UNKNOWN_COMMAND"}),
        };
        self.lock_inner().bound_workspace_cache();
        result
    }

    fn model_info_command(&self, arguments: &[Value]) -> Value {
        let Some(argument) = arguments.first().and_then(Value::as_object) else {
            return json!({"error":"dynare/modelInfo requires a root_uri argument", "code":"INVALID_ARGUMENTS"});
        };
        let Some(root) = argument
            .get("root_uri")
            .and_then(Value::as_str)
            .and_then(|uri| Url::parse(uri).ok())
        else {
            return json!({"error":"dynare/modelInfo requires a valid root_uri", "code":"INVALID_ARGUMENTS"});
        };
        let document = match argument.get("document_uri") {
            None => root.clone(),
            Some(value) => match value.as_str().and_then(|uri| Url::parse(uri).ok()) {
                Some(uri) => uri,
                None => {
                    return json!({"error":"dynare/modelInfo requires a valid document_uri", "code":"INVALID_ARGUMENTS"})
                }
            },
        };
        let mut inner = self.lock_inner();
        if !is_model_root(&root) {
            return json!({"error":"Choose a .mod or .dyn owner root for model information", "code":"ROOT_REQUIRED", "owner_roots":inner.known_owner_roots(&root)});
        }
        if !["file", "untitled"].contains(&root.scheme()) {
            return json!({"error":"Model information requires a file or untitled root URI", "code":"UNSUPPORTED_URI"});
        }
        let Some(revision) = inner.root_revision(&root) else {
            return json!({"error":"The model root is unavailable", "code":"ROOT_NOT_FOUND"});
        };
        if crate::include_resolver::normalize_uri(document.as_str())
            != crate::include_resolver::normalize_uri(root.as_str())
            && !inner
                .workspace
                .owner_roots(document.as_str())
                .contains(&crate::include_resolver::normalize_uri(root.as_str()))
        {
            return json!({"error":"The displayed document does not belong to the chosen model root", "code":"DOCUMENT_NOT_OWNED"});
        }
        let Some(model) = inner.workspace.get_effective_model(root.as_str()).cloned() else {
            return json!({"error":"The model root is unavailable", "code":"ROOT_NOT_FOUND"});
        };
        let Some(report) = inner.workspace.expand_report(root.as_str()).cloned() else {
            return json!({"error":"The model root is unavailable", "code":"ROOT_NOT_FOUND"});
        };
        let complete = report.complete
            && report.model_map.complete
            && inner.workspace.includes_complete(root.as_str());
        let includes = inner
            .workspace
            .include_records(root.as_str())
            .cloned()
            .unwrap_or_default();
        let mut result = if complete {
            crate::model_info::model_info_json(&model)
        } else {
            let reasons = crate::model_info::incomplete_reason_records(&model, Some(&includes));
            let reason_json: Vec<Value> = reasons
                .iter()
                .map(|(span, code, message)| {
                    let location =
                        incomplete_reason_location(&mut inner, &root, *span, *code == "E061");
                    json!({
                        "code": code,
                        "message": message,
                        "location": location,
                    })
                })
                .collect();
            let mut reason_json = reason_json;
            reason_json.sort_by_key(|reason| {
                (
                    reason["location"]["uri"].as_str().unwrap_or("").to_string(),
                    reason["location"]["range"]["start"]["line"]
                        .as_u64()
                        .unwrap_or(0),
                    reason["location"]["range"]["start"]["character"]
                        .as_u64()
                        .unwrap_or(0),
                )
            });
            crate::model_info::model_incomplete_status_with_reasons(&reason_json)
        };
        let Some(text) = inner.workspace.get_source(document.as_str()) else {
            return json!({"error":"The displayed source is unavailable", "code":"DOCUMENT_NOT_FOUND"});
        };
        let view = WrittenView::new(
            &document,
            text,
            &model,
            &report.model_map,
            Some(&inner.workspace),
            complete,
            true,
        )
        .with_known_uris(inner.docs.keys().chain(std::iter::once(&root)));
        let facts = view.facts_json();
        result
            .as_object_mut()
            .unwrap()
            .extend(facts.as_object().unwrap().clone());
        result["schema_version"] = json!(MODEL_INFO_SCHEMA_VERSION);
        result["root_uri"] = json!(root);
        result["document_uri"] = json!(document);
        result["document_version"] = json!(inner.document(&document).map(|doc| doc.version));
        result["revision"] = json!(revision);
        result["complete"] = json!(complete);
        result["owner_roots"] = json!(inner.known_owner_roots(&document));
        result["dependency_candidates"] = json!(inner
            .workspace
            .input_candidate_paths(root.as_str())
            .iter()
            .filter_map(|path| Url::from_file_path(path).ok())
            .collect::<Vec<_>>());
        result["block_categories"] = json!(crate::model_map::BLOCK_CATEGORIES
            .iter()
            .map(|(category, default)| json!({"category":category,"default":default}))
            .collect::<Vec<_>>());
        let companions = inner
            .workspace
            .companion_records(root.as_str())
            .map(<[_]>::to_vec)
            .unwrap_or_default();
        result["related_files"] =
            crate::model_info::related_files_json(&includes, &companions, |path| {
                path.to_string_lossy().into_owned()
            });
        result
    }

    fn compare_command(&self, arguments: &[Value]) -> Value {
        let (uri_a, uri_b) = match parse_compare_args(arguments) {
            Ok(pair) => pair,
            Err(v) => return v,
        };
        let mut inner = self.lock_inner();
        let revision_a = Url::parse(&uri_a)
            .ok()
            .and_then(|uri| inner.root_revision(&uri));
        let revision_b = Url::parse(&uri_b)
            .ok()
            .and_then(|uri| inner.root_revision(&uri));
        let Some(model_a) = inner.workspace.get_effective_model(uri_a.as_str()).cloned() else {
            return json!({"error": format!("No parsed model for uri_a: {uri_a}"), "code": "URI_A_NOT_FOUND"});
        };
        let Some(model_b) = inner.workspace.get_effective_model(uri_b.as_str()).cloned() else {
            return json!({"error": format!("No parsed model for uri_b: {uri_b}"), "code": "URI_B_NOT_FOUND"});
        };
        if let Some(status) = crate::mcp::incomplete_model_status(
            &model_a,
            inner.workspace.includes_complete(uri_a.as_str()),
        )
        .or_else(|| {
            crate::mcp::incomplete_model_status(
                &model_b,
                inner.workspace.includes_complete(uri_b.as_str()),
            )
        }) {
            return status;
        }
        let diff = compare_models_with_sources(
            &model_a,
            &model_b,
            inner
                .workspace
                .get_source(uri_a.as_str())
                .map(|text| CompareSource {
                    text,
                    origin_uri: Some(uri_a.as_str()),
                }),
            inner
                .workspace
                .get_source(uri_b.as_str())
                .map(|text| CompareSource {
                    text,
                    origin_uri: Some(uri_b.as_str()),
                }),
        );
        let before = crate::compare_navigation::ComparisonInput::capture(
            &mut inner.workspace,
            &uri_a,
            Some(&uri_a),
            revision_a,
            &model_a,
            diff.shock_setup_changes
                .iter()
                .map(|change| change.before.as_ref()),
        );
        let after = crate::compare_navigation::ComparisonInput::capture(
            &mut inner.workspace,
            &uri_b,
            Some(&uri_b),
            revision_b,
            &model_b,
            diff.shock_setup_changes
                .iter()
                .map(|change| change.after.as_ref()),
        );
        if !inner.workspace.input_snapshot_is_current(&uri_a)
            || !inner.workspace.input_snapshot_is_current(&uri_b)
        {
            return json!({"error": "Comparison inputs changed while reading them; refresh the comparison", "code": "INPUT_CHANGED"});
        }
        let mut result = diff.to_json();
        result["navigation"] = crate::compare_navigation::navigation_json(
            &diff,
            &before,
            &after,
            crate::compare_navigation::Coordinates::Lsp,
        );
        result
    }

    fn show_effective_model_command(&self, arguments: &[Value]) -> Value {
        let Some(uri) = arguments
            .first()
            .and_then(|arg| arg.get("root_uri"))
            .and_then(Value::as_str)
            .and_then(|uri| Url::parse(uri).ok())
            .or_else(|| extract_command_uri(arguments))
        else {
            return json!({"success": false, "message": "Missing or invalid URI argument"});
        };
        let mut inner = self.lock_inner();
        if !is_model_root(&uri) {
            return json!({"success": false, "code":"ROOT_REQUIRED", "message":"Choose a .mod or .dyn owner root for the effective model", "owner_roots":inner.known_owner_roots(&uri)});
        }
        let Some(revision) = inner.root_revision(&uri) else {
            return json!({"success": false, "message": "Document not available"});
        };
        let Some(report) = inner.workspace.expand_report(uri.as_str()).cloned() else {
            return json!({"success": false, "message": "Document not available"});
        };
        let requested = arguments
            .first()
            .and_then(|arg| arg.get("layout"))
            .and_then(Value::as_str);
        let source_gaps = matches!(requested, Some("source"))
            .then(|| inner.workspace.source_layout_gaps(uri.as_str()))
            .unwrap_or_default();
        let source_lines = matches!(requested, Some("source"))
            .then(|| inner.workspace.source_line_segments(uri.as_str()))
            .unwrap_or_default();
        let model = matches!(requested, Some("readable") | Some("source"))
            .then(|| inner.workspace.get_effective_model(uri.as_str()))
            .flatten();
        enum DisplayLayout {
            Readable(crate::preview_layout::PreviewLayout),
            Source(crate::preview_source::SourcePreview),
        }
        let layout = model.and_then(|model| match requested {
            Some("readable") => {
                crate::preview_layout::readable(&report, model).map(DisplayLayout::Readable)
            }
            Some("source") => {
                crate::preview_source::source(&report, model, &source_gaps, &source_lines)
                    .map(DisplayLayout::Source)
            }
            _ => None,
        });
        let source_unproven = matches!(
            &layout,
            Some(DisplayLayout::Source(preview)) if !preview.proven
        );
        let effective_text = match &layout {
            Some(DisplayLayout::Readable(preview)) => preview.text.as_str(),
            Some(DisplayLayout::Source(preview)) => preview.text.as_str(),
            None if requested == Some("source") => "",
            None => report.effective_text.as_str(),
        };
        let complete = report.navigation_complete
            && inner.workspace.includes_complete(uri.as_str())
            && !source_unproven
            && !(requested == Some("source") && layout.is_none());
        let has_heterogeneous = !report.heterogeneous_origins.is_empty();
        let origins: Vec<Value> = report
            .origins
            .iter()
            .map(|origin| equation_origin_json(&inner.workspace, origin, has_heterogeneous))
            .collect();
        let mut result = json!({
            "uri": uri.as_str(),
            "root_uri": uri,
            "revision": revision,
            "document_version": inner.document(&uri).map(|doc| doc.version),
            "complete": complete,
            "navigation_schema_version": crate::preview_navigation::NAVIGATION_SCHEMA_VERSION,
            "dependency_candidates": inner.workspace.input_candidate_paths(uri.as_str())
                .iter().filter_map(|path| Url::from_file_path(path).ok()).collect::<Vec<_>>(),
            "effective_text": effective_text,
            "origins": origins,
            "macro_messages": lsp_macro_messages(&inner.workspace, uri.as_str(), &report),
        });
        result["navigation"] = if complete {
            let index = LineIndex::new(effective_text);
            let mut rows = 0;
            crate::preview_navigation::navigation_json(
                &report,
                |span| {
                    let display_span = match &layout {
                        // Source clears proven when ranges are short; complete then skips this index.
                        Some(DisplayLayout::Readable(preview)) => preview.ranges[rows],
                        Some(DisplayLayout::Source(preview)) => preview.ranges[rows],
                        None => span,
                    };
                    rows += 1;
                    json!(span_range(&index, effective_text, display_span))
                },
                |segment| preview_written_location(&inner, &uri, segment),
            )
        } else {
            json!([])
        };
        if matches!(requested, Some("source")) {
            result["source_navigation_schema_version"] =
                json!(crate::source_navigation::SOURCE_NAVIGATION_SCHEMA_VERSION);
            result["source_navigation"] = if complete {
                if let Some(DisplayLayout::Source(preview)) = &layout {
                    let index = LineIndex::new(effective_text);
                    let file_cuts = inner.workspace.source_file_cuts(uri.as_str());
                    crate::source_navigation::source_navigation_json(
                        &preview.text,
                        &preview.fragments,
                        &file_cuts,
                        |span| {
                            let segments =
                                inner.workspace.map_effective_segments(uri.as_str(), span);
                            (segments.len() == 1)
                                .then(|| preview_written_location(&inner, &uri, &segments[0]))
                                .flatten()
                        },
                        |span| json!(span_range(&index, effective_text, span)),
                    )
                } else {
                    json!([])
                }
            } else {
                json!([])
            };
            result["macro_ranges"] = if complete {
                if let Some(DisplayLayout::Source(preview)) = &layout {
                    let index = LineIndex::new(effective_text);
                    let file_cuts = inner.workspace.source_file_cuts(uri.as_str());
                    let include_spans = inner.workspace.source_include_spans(uri.as_str());
                    crate::source_navigation::macro_ranges_json(
                        &preview.text,
                        &preview.fragments,
                        &file_cuts,
                        &include_spans,
                        |span| json!(span_range(&index, effective_text, span)),
                    )
                } else {
                    json!([])
                }
            } else {
                json!([])
            };
        }
        if !complete {
            result["status"] = json!("incomplete");
        }
        if !inner.workspace.input_snapshot_is_current(uri.as_str()) {
            return json!({"success":false,"code":"INPUT_CHANGED","message":"The model inputs changed; refresh the effective model"});
        }
        result
    }

    fn prepare_hierarchy(
        &self,
        pos: &TextDocumentPositionParams,
    ) -> Option<Vec<CallHierarchyItem>> {
        let inner = self.lock_inner();
        let doc = inner.document(&pos.text_document.uri)?;
        let model = inner.workspace.get_model(pos.text_document.uri.as_str())?;
        let index = LineIndex::new(&doc.text);
        let byte = index.offset_utf16(&doc.text, span_pos(pos.position));
        let mut items = Vec::new();
        if let Some((word, _)) = ident_at(&doc.text, byte) {
            if find_decl(model, &word).is_some() {
                items.push(ch_variable_item(
                    model,
                    &index,
                    &doc.text,
                    &pos.text_document.uri,
                    &word,
                ));
            }
        }
        if let Some(eq) = equation_at(model, &index, &doc.text, pos.position) {
            items.push(ch_equation_item(
                model,
                &index,
                &doc.text,
                &pos.text_document.uri,
                eq,
            ));
        }
        if items.is_empty() {
            None
        } else {
            Some(items)
        }
    }

    fn incoming(&self, item: &CallHierarchyItem) -> Option<Vec<CallHierarchyIncomingCall>> {
        let data = item.data.as_ref()?;
        if data.get("dynare").and_then(|v| v.as_str()) != Some("variable") {
            return Some(Vec::new());
        }
        let name = data.get("name")?.as_str()?.to_string();
        let inner = self.lock_inner();
        let doc = inner.document(&item.uri)?;
        let model = inner.workspace.get_model(item.uri.as_str())?;
        let index = LineIndex::new(&doc.text);
        let mut calls = Vec::new();
        for eq in &model.equations {
            if eq.is_local || eq.model_local {
                continue;
            }
            let in_eq: Vec<Span> = model
                .ident_refs(eq)
                .into_iter()
                .filter(|r| model.name(r.name) == name)
                .map(|r| r.span)
                .collect();
            if in_eq.is_empty() {
                continue;
            }
            calls.push(CallHierarchyIncomingCall {
                from: ch_equation_item(model, &index, &doc.text, &item.uri, eq),
                from_ranges: in_eq
                    .into_iter()
                    .map(|s| span_range(&index, &doc.text, s))
                    .collect(),
            });
        }
        Some(calls)
    }

    fn outgoing(&self, item: &CallHierarchyItem) -> Option<Vec<CallHierarchyOutgoingCall>> {
        let data = item.data.as_ref()?;
        if data.get("dynare").and_then(|v| v.as_str()) != Some("equation") {
            return Some(Vec::new());
        }
        let start = data.get("start")?.as_u64()? as u32;
        let inner = self.lock_inner();
        let doc = inner.document(&item.uri)?;
        let model = inner.workspace.get_model(item.uri.as_str())?;
        let eq = model.equations.iter().find(|e| e.span.start == start)?;
        let index = LineIndex::new(&doc.text);
        let mut by_name: HashMap<String, Vec<Span>> = HashMap::new();
        for r in model.ident_refs(eq) {
            let n = model.name(r.name).to_string();
            if find_decl(model, &n).is_none() {
                continue;
            }
            by_name.entry(n).or_default().push(r.span);
        }
        let mut names: Vec<String> = by_name.keys().cloned().collect();
        names.sort();
        let mut calls = Vec::new();
        for name in names {
            let occs = by_name.remove(&name).unwrap_or_default();
            calls.push(CallHierarchyOutgoingCall {
                to: ch_variable_item(model, &index, &doc.text, &item.uri, &name),
                from_ranges: occs
                    .into_iter()
                    .map(|s| span_range(&index, &doc.text, s))
                    .collect(),
            });
        }
        Some(calls)
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        ordering::ready().await;
        Ok(Some(self.value_hints(&params)))
    }
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        ordering::ready().await;
        {
            let mut inner = self.lock_inner();
            let folders = params.workspace_folders.unwrap_or_else(|| {
                #[allow(deprecated)]
                params
                    .root_uri
                    .clone()
                    .map(|uri| {
                        vec![WorkspaceFolder {
                            uri,
                            name: String::new(),
                        }]
                    })
                    .unwrap_or_default()
            });
            inner.settings.set_folders(folders);
            let document = params.capabilities.text_document.as_ref();
            inner.semantic_mapping = SemanticMapping::negotiate(
                document.and_then(|document| document.semantic_tokens.as_ref()),
            );
            let completion = document
                .and_then(|document| document.completion.as_ref())
                .and_then(|completion| completion.completion_item.as_ref());
            inner.completion_label_details = completion
                .and_then(|completion| completion.label_details_support)
                .unwrap_or(false);
            inner.completion_snippets = completion
                .and_then(|completion| completion.snippet_support)
                .unwrap_or(false);
            inner.model_info_notifications = params
                .capabilities
                .experimental
                .as_ref()
                .and_then(|value| value.get("dygnosis"))
                .and_then(|value| value.get("modelInfoChanged"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            inner.project.notifications = params
                .capabilities
                .experimental
                .as_ref()
                .and_then(|value| value.get("dygnosis"))
                .and_then(|value| value.get("projectStatusChanged"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let workspace = params.capabilities.workspace.as_ref();
            inner.token_refresh = workspace
                .and_then(|cap| cap.semantic_tokens.as_ref())
                .and_then(|cap| cap.refresh_support)
                .unwrap_or(false);
            inner.hint_refresh = workspace
                .and_then(|cap| cap.inlay_hint.as_ref())
                .and_then(|cap| cap.refresh_support)
                .unwrap_or(false);
        }
        let explanations = if let Some(opts) = params.initialization_options {
            self.apply_settings(&opts)
        } else {
            self.lock_inner().project_reconfigure(true);
            Vec::new()
        };
        ordering::committed();
        for explanation in explanations {
            self.client
                .log_message(MessageType::WARNING, explanation)
                .await;
        }
        let mut result = initialize_result();
        if let Some(SemanticTokensServerCapabilities::SemanticTokensOptions(options)) =
            &mut result.capabilities.semantic_tokens_provider
        {
            options.legend = self.lock_inner().semantic_mapping.legend.clone();
        }
        Ok(result)
    }

    async fn initialized(&self, _: InitializedParams) {
        ordering::ready().await;
        self.lock_inner().project.initialized = true;
        ordering::committed();
        self.client
            .log_message(MessageType::INFO, "dygnosis initialized")
            .await;
        self.kick_project();
    }

    async fn shutdown(&self) -> Result<()> {
        ordering::ready().await;
        let _output = self.output_gate.lock().await;
        let mut inner = self.lock_inner();
        inner.project.shutdown = true;
        inner.project.epoch += 1;
        ordering::committed();
        self.project_wake.notify_one();
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        ordering::ready().await;
        let output = self.output_gate.lock().await;
        let doc = params.text_document;
        let to_publish = self.upsert(doc.uri, doc.text, doc.version);
        ordering::committed();
        for (uri, version, diagnostics) in to_publish {
            self.client
                .publish_diagnostics(uri, diagnostics, version)
                .await;
        }
        drop(output);
        self.refresh_presentation(false).await;
        self.kick_project();
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        ordering::ready().await;
        let output = self.output_gate.lock().await;
        if self
            .lock_inner()
            .document(&params.text_document.uri)
            .is_some_and(|doc| params.text_document.version < doc.version)
        {
            return;
        }
        let Some(change) = params.content_changes.last() else {
            return;
        };
        if change.range.is_some() {
            return;
        }
        let to_publish = self.upsert(
            params.text_document.uri,
            change.text.clone(),
            params.text_document.version,
        );
        ordering::committed();
        for (uri, version, diagnostics) in to_publish {
            self.client
                .publish_diagnostics(uri, diagnostics, version)
                .await;
        }
        drop(output);
        self.refresh_presentation(false).await;
        self.kick_project();
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        ordering::ready().await;
        let output = self.output_gate.lock().await;
        let uri = params.text_document.uri;
        let snapshot = {
            let inner = self.lock_inner();
            inner.document(&uri).map(|d| (d.text.clone(), d.version))
        };
        let (text, version) = match (params.text, snapshot) {
            (Some(text), Some((_, version))) => (text, version),
            (Some(text), None) => (text, 0),
            (None, Some((text, version))) => (text, version),
            (None, None) => return,
        };
        let to_publish = self.upsert(uri, text, version);
        ordering::committed();
        for (uri, version, diagnostics) in to_publish {
            self.client
                .publish_diagnostics(uri, diagnostics, version)
                .await;
        }
        drop(output);
        self.refresh_presentation(false).await;
        self.kick_project();
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        ordering::ready().await;
        let output = self.output_gate.lock().await;
        let uri = params.text_document.uri;
        let to_publish = {
            let mut inner = self.lock_inner();
            let key = crate::include_resolver::normalize_uri(uri.as_str());
            inner.docs.retain(|candidate, _| {
                crate::include_resolver::normalize_uri(candidate.as_str()) != key
            });
            inner.workspace.remove_document(uri.as_str());
            inner.project_changed(&uri, false);
            inner.refresh_diagnostics(Some(&uri))
        };
        ordering::committed();
        for (uri, version, diagnostics) in to_publish {
            self.client
                .publish_diagnostics(uri, diagnostics, version)
                .await;
        }
        drop(output);
        self.refresh_presentation(false).await;
        self.kick_project();
    }

    async fn diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> Result<DocumentDiagnosticReportResult> {
        ordering::ready().await;
        let items = self.pull_items(&params.text_document.uri);
        Ok(DocumentDiagnosticReportResult::Report(
            DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                related_documents: None,
                full_document_diagnostic_report: FullDocumentDiagnosticReport {
                    result_id: None,
                    items,
                },
            }),
        ))
    }

    async fn workspace_diagnostic(
        &self,
        params: WorkspaceDiagnosticParams,
    ) -> Result<WorkspaceDiagnosticReportResult> {
        ordering::ready().await;
        let inner = self.lock_inner();
        let mut items: Vec<_> = inner
            .published
            .iter()
            .map(|(uri, diagnostics)| {
                WorkspaceDocumentDiagnosticReport::Full(WorkspaceFullDocumentDiagnosticReport {
                    uri: uri.clone(),
                    version: inner.document(uri).map(|doc| i64::from(doc.version)),
                    full_document_diagnostic_report: FullDocumentDiagnosticReport {
                        result_id: None,
                        items: diagnostics.clone(),
                    },
                })
            })
            .collect();
        let mut removed = Vec::new();
        for previous in params.previous_result_ids {
            if !inner.published.contains_key(&previous.uri) {
                removed.push(WorkspaceDocumentDiagnosticReport::Full(
                    WorkspaceFullDocumentDiagnosticReport {
                        uri: previous.uri,
                        version: None,
                        full_document_diagnostic_report: FullDocumentDiagnosticReport {
                            result_id: None,
                            items: Vec::new(),
                        },
                    },
                ));
            }
        }
        removed.append(&mut items);
        let items = removed;
        Ok(WorkspaceDiagnosticReportResult::Report(
            WorkspaceDiagnosticReport { items },
        ))
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        ordering::ready().await;
        let output = self.output_gate.lock().await;
        let to_publish = {
            let mut inner = self.lock_inner();
            for event in &params.changes {
                inner.project_changed(&event.uri, event.typ == FileChangeType::DELETED);
                inner.workspace.remove_document(event.uri.as_str());
            }
            let snapshots: Vec<(Url, String, i32)> = inner
                .docs
                .iter()
                .map(|(uri, doc)| (uri.clone(), doc.text.clone(), doc.version))
                .collect();
            for (uri, text, version) in snapshots {
                inner.workspace.update_document(uri.as_str(), &text);
                if let Some(doc) = inner.docs.get_mut(&uri) {
                    doc.version = version;
                }
            }
            inner.refresh_diagnostics(None)
        };
        ordering::committed();
        for (uri, version, diagnostics) in to_publish {
            self.client
                .publish_diagnostics(uri, diagnostics, version)
                .await;
        }
        drop(output);
        self.refresh_presentation(false).await;
        self.kick_project();
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        ordering::ready().await;
        Ok(self.hover_at(&params.text_document_position_params))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        ordering::ready().await;
        Ok(self
            .doc_symbols(&params.text_document.uri)
            .map(DocumentSymbolResponse::Nested))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        ordering::ready().await;
        Ok(Some(self.workspace_symbols(&params.query)))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        ordering::ready().await;
        Ok(self.definition_at(&params.text_document_position_params))
    }

    async fn goto_declaration(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        ordering::ready().await;
        Ok(self
            .decl_location(&params.text_document_position_params)
            .map(GotoDefinitionResponse::Scalar))
    }

    async fn goto_type_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        ordering::ready().await;
        Ok(self
            .decl_location(&params.text_document_position_params)
            .map(GotoDefinitionResponse::Scalar))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        ordering::ready().await;
        Ok(self.ident_locations(&params.text_document_position))
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        ordering::ready().await;
        Ok(self.ident_highlights(&params.text_document_position_params))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        ordering::ready().await;
        Ok(self.complete(&params.text_document_position))
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        ordering::ready().await;
        Ok(self.signature_at(&params.text_document_position_params))
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        ordering::ready().await;
        Ok(self
            .prepare_rename_at(&params)
            .map(PrepareRenameResponse::Range))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        ordering::ready().await;
        Ok(self.rename_at(&params.text_document_position, &params.new_name))
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        ordering::ready().await;
        let mut actions = self.quick_fixes(&params).unwrap_or_default();
        for action in self.shock_templates(&params) {
            actions.push(CodeActionOrCommand::CodeAction(action));
        }
        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }

    async fn linked_editing_range(
        &self,
        params: LinkedEditingRangeParams,
    ) -> Result<Option<LinkedEditingRanges>> {
        ordering::ready().await;
        Ok(self.linked_ranges(&params.text_document_position_params))
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        ordering::ready().await;
        Ok(self.folding_ranges(&params.text_document.uri))
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>> {
        ordering::ready().await;
        Ok(self.selection_ranges(&params.text_document.uri, &params.positions))
    }

    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
        ordering::ready().await;
        Ok(self.document_links(&params.text_document.uri))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        ordering::ready().await;
        Ok(self
            .semantic_tokens(&params.text_document.uri, None)
            .map(SemanticTokensResult::Tokens))
    }

    async fn semantic_tokens_range(
        &self,
        params: SemanticTokensRangeParams,
    ) -> Result<Option<SemanticTokensRangeResult>> {
        ordering::ready().await;
        Ok(self
            .semantic_tokens(&params.text_document.uri, Some(params.range))
            .map(SemanticTokensRangeResult::Tokens))
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        ordering::ready().await;
        Ok(self.format_document(&params.text_document.uri))
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        ordering::ready().await;
        Ok(self.format_line_range(&params.text_document.uri, params.range))
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> Result<Option<Value>> {
        ordering::ready().await;
        if matches!(
            params.command.as_str(),
            "dynare/projectStatus" | "dynare/recheckProject" | "dynare/cancelProject"
        ) {
            return Ok(Some(self.project_command(&params.command).await));
        }
        Ok(Some(self.execute(&params.command, &params.arguments)))
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        ordering::ready().await;
        let output = self.output_gate.lock().await;
        let explanations = self.apply_settings(&params.settings);
        let to_publish = self.lock_inner().refresh_diagnostics(None);
        ordering::committed();
        for explanation in explanations {
            self.client
                .log_message(MessageType::WARNING, explanation)
                .await;
        }
        for (uri, version, diagnostics) in to_publish {
            self.client
                .publish_diagnostics(uri, diagnostics, version)
                .await;
        }
        drop(output);
        self.refresh_presentation(true).await;
        self.kick_project();
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        ordering::ready().await;
        let output = self.output_gate.lock().await;
        let to_publish = {
            let mut inner = self.lock_inner();
            inner
                .settings
                .change_folders(params.event.added, params.event.removed);
            inner.project_reconfigure(true);
            inner.refresh_diagnostics(None)
        };
        ordering::committed();
        for (uri, version, diagnostics) in to_publish {
            self.client
                .publish_diagnostics(uri, diagnostics, version)
                .await;
        }
        drop(output);
        self.refresh_presentation(true).await;
        self.kick_project();
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        ordering::ready().await;
        Ok(self.prepare_hierarchy(&params.text_document_position_params))
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
        ordering::ready().await;
        Ok(self.incoming(&params.item))
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        ordering::ready().await;
        Ok(self.outgoing(&params.item))
    }
}

/// Wave c initialize payload: FULL sync + diagnostic pull + navigation + intel / format / commands.
pub fn initialize_result() -> InitializeResult {
    InitializeResult {
        capabilities: ServerCapabilities {
            text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
            diagnostic_provider: Some(DiagnosticServerCapabilities::Options(DiagnosticOptions {
                identifier: None,
                inter_file_dependencies: true,
                workspace_diagnostics: true,
                work_done_progress_options: WorkDoneProgressOptions::default(),
            })),
            hover_provider: Some(HoverProviderCapability::Simple(true)),
            inlay_hint_provider: Some(OneOf::Left(true)),
            document_symbol_provider: Some(OneOf::Left(true)),
            workspace_symbol_provider: Some(OneOf::Left(true)),
            definition_provider: Some(OneOf::Left(true)),
            declaration_provider: Some(DeclarationCapability::Simple(true)),
            type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
            references_provider: Some(OneOf::Left(true)),
            document_highlight_provider: Some(OneOf::Left(true)),
            completion_provider: Some(CompletionOptions {
                trigger_characters: Some(vec!["(".into(), ",".into()]),
                ..CompletionOptions::default()
            }),
            signature_help_provider: Some(SignatureHelpOptions {
                trigger_characters: Some(vec!["(".into(), ",".into(), "=".into()]),
                ..SignatureHelpOptions::default()
            }),
            rename_provider: Some(OneOf::Right(RenameOptions {
                prepare_provider: Some(true),
                work_done_progress_options: WorkDoneProgressOptions::default(),
            })),
            code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
            linked_editing_range_provider: Some(LinkedEditingRangeServerCapabilities::Simple(true)),
            folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
            selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
            document_link_provider: Some(DocumentLinkOptions {
                resolve_provider: Some(false),
                work_done_progress_options: WorkDoneProgressOptions::default(),
            }),
            semantic_tokens_provider: Some(
                SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions {
                    legend: SemanticMapping::default().legend,
                    range: Some(true),
                    full: Some(SemanticTokensFullOptions::Bool(true)),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                }),
            ),
            document_formatting_provider: Some(OneOf::Left(true)),
            document_range_formatting_provider: Some(OneOf::Left(true)),
            execute_command_provider: Some(ExecuteCommandOptions {
                commands: vec![
                    "dynare/explainDiagnostic".into(),
                    "dynare/compareModels".into(),
                    "dynare/showEffectiveModel".into(),
                    "dynare/modelInfo".into(),
                    "dynare/projectStatus".into(),
                    "dynare/recheckProject".into(),
                    "dynare/cancelProject".into(),
                ],
                work_done_progress_options: WorkDoneProgressOptions::default(),
            }),
            call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
            position_encoding: Some(PositionEncodingKind::UTF16),
            workspace: Some(WorkspaceServerCapabilities {
                workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                    supported: Some(true),
                    change_notifications: Some(OneOf::Left(true)),
                }),
                ..WorkspaceServerCapabilities::default()
            }),
            experimental: Some(json!({"dygnosis": {
                "modelInfo": {"command": "dynare/modelInfo", "schema_version": MODEL_INFO_SCHEMA_VERSION, "dependency_candidates": true},
                "modelInfoChanged": true,
                "compareModels": {"command": "dynare/compareModels", "navigation_schema_version": 1},
                "effectivePreview": {"command":"dynare/showEffectiveModel", "navigation_schema_version":crate::preview_navigation::NAVIGATION_SCHEMA_VERSION, "source_navigation_schema_version":crate::source_navigation::SOURCE_NAVIGATION_SCHEMA_VERSION, "dependency_candidates":true, "readable_layout":true, "source_layout":true, "macro_ranges":true, "macro_messages":true},
                "configuration": {"schema_version": CONFIGURATION_SCHEMA_VERSION}
                ,"projectDiagnostics": {"schema_version":project::SCHEMA_VERSION,"status_command":"dynare/projectStatus","recheck_command":"dynare/recheckProject","cancel_command":"dynare/cancelProject","active_model_notification":"dynare/activeModelChanged","status_notification":"dynare/projectStatusChanged","typing_pause_ms":250}
            }})),
            ..ServerCapabilities::default()
        },
        server_info: Some(ServerInfo {
            name: "dygnosis".into(),
            version: Some(VERSION.into()),
        }),
    }
}

/// Library diagnostics as LSP items. Uses [`check_file`] (same families as CLI).
pub fn diagnostics_for(uri: &str, text: &str) -> Vec<Diagnostic> {
    library_to_lsp(text, &check_file(text, uri))
}

fn library_to_lsp(text: &str, diags: &[crate::Diagnostic]) -> Vec<Diagnostic> {
    let index = LineIndex::new(text);
    diags
        .iter()
        .filter(|d| !is_dropped_code(&d.code))
        .map(|d| {
            let start = index.position_utf16(text, d.span.start);
            let end = index.position_utf16(text, d.span.end);
            Diagnostic {
                range: Range::new(
                    Position::new(start.line, start.character),
                    Position::new(end.line, end.character),
                ),
                severity: Some(lsp_severity(d.severity)),
                code: Some(NumberOrString::String(d.code.clone())),
                source: Some("dygnosis".into()),
                message: d.message.clone(),
                tags: lsp_tags(&d.tags),
                related_information: diagnostic_related_information(d),
                data: diagnostic_context(d),
                ..Diagnostic::default()
            }
        })
        .collect()
}

/// Convert one root once, outside the background commit lock. Each written
/// file has one shared source snapshot and one line-index construction.
fn prepare_root_report(
    root: &Url,
    set: DiagnosticSet,
    text: Arc<str>,
    revision: String,
) -> RootReport {
    let errors = set
        .diagnostics
        .iter()
        .filter(|diag| diag.severity == Severity::Error)
        .count();
    let warnings = set
        .diagnostics
        .iter()
        .filter(|diag| diag.severity == Severity::Warning)
        .count();
    let mut files: HashMap<Url, (Arc<str>, Vec<crate::Diagnostic>)> = HashMap::new();
    for (index, mut diagnostic) in set.diagnostics.into_iter().enumerate() {
        if is_dropped_code(&diagnostic.code) {
            continue;
        }
        let (uri, source) = if let Some(origin) = set.origins.get(index).and_then(Option::as_ref) {
            let Some(uri) = (if origin.file == crate::include_resolver::normalize_uri(root.as_str())
            {
                Some(root.clone())
            } else {
                file_url_from_path_key(&origin.file)
            }) else {
                continue;
            };
            diagnostic.span = origin.span;
            (uri, Arc::clone(&origin.text))
        } else {
            (root.clone(), Arc::clone(&text))
        };
        files
            .entry(uri)
            .or_insert_with(|| (source, Vec::new()))
            .1
            .push(diagnostic);
    }
    let routes = files
        .into_iter()
        .map(|(uri, (source, diagnostics))| {
            let converted = library_to_lsp(&source, &diagnostics);
            let rows = diagnostics
                .into_iter()
                .zip(converted)
                .map(|(diagnostic, mut item)| {
                    // Every row needs provenance for replay after a root
                    // revision notification, including ordinary checks.
                    let data = item
                        .data
                        .get_or_insert_with(|| json!({}))
                        .as_object_mut()
                        .unwrap();
                    data.insert("root".into(), json!(root));
                    data.insert("input_revision".into(), json!(revision));
                    RoutedDiagnostic {
                        diagnostic,
                        lsp_diagnostic: item,
                        text: Arc::clone(&source),
                        root: root.clone(),
                        revision: revision.clone(),
                    }
                })
                .collect();
            (uri, rows)
        })
        .collect();
    RootReport {
        root: root.clone(),
        routes,
        revision,
        errors,
        warnings,
    }
}

fn related_location(site: &crate::diagnostic::DiagnosticOrigin) -> Option<Location> {
    site.text
        .get(site.span.start as usize..site.span.end as usize)?;
    let uri = if crate::include_resolver::is_virtual_uri(&site.file) {
        Url::parse(&site.file).ok()?
    } else {
        file_url_from_path_key(&site.file)?
    };
    let normalized = crate::parser::normalize_newlines(&site.text);
    let index = LineIndex::new(&normalized);
    Some(Location::new(
        uri,
        span_range(&index, &site.text, site.span),
    ))
}

fn diagnostic_related_information(
    diagnostic: &crate::Diagnostic,
) -> Option<Vec<DiagnosticRelatedInformation>> {
    let rows: Vec<_> = diagnostic
        .related
        .iter()
        .flat_map(|related| {
            related.locations.iter().filter_map(|site| {
                Some(DiagnosticRelatedInformation {
                    location: related_location(site)?,
                    message: related.message.clone(),
                })
            })
        })
        .collect();
    (!rows.is_empty()).then_some(rows)
}

fn related_context(diagnostic: &crate::Diagnostic) -> Option<Value> {
    let rows: Vec<_> = diagnostic.related.iter().filter(|related| !related.origin_frames.is_empty()).map(|related| json!({
        "message": related.message,
        "origin_frames": related.origin_frames.iter().map(|frame| json!({
            "kind": frame.kind, "variable": frame.variable, "value": frame.value,
            "locations": frame.locations.iter().filter_map(related_location).collect::<Vec<_>>()
        })).collect::<Vec<_>>()
    })).collect();
    (!rows.is_empty()).then(|| json!({"related_context": rows}))
}

fn diagnostic_context(diagnostic: &crate::Diagnostic) -> Option<Value> {
    let mut data = related_context(diagnostic).unwrap_or_else(|| json!({}));
    if let Some(context) = &diagnostic.writing {
        data["writing_context"] = json!(context);
        data["root"] = json!(context.root);
        data["input_revision"] = json!(context.input_revision);
    }
    (!data.as_object()?.is_empty()).then_some(data)
}

fn naming_code_actions(inner: &mut Inner, params: &CodeActionParams) -> Vec<CodeAction> {
    if !action_kind_requested(&params.context.only, &CodeActionKind::QUICKFIX) {
        return Vec::new();
    }
    let requested = &params.text_document.uri;
    let mut notes: Vec<(Url, Diagnostic)> = inner
        .published
        .get(requested)
        .into_iter()
        .flatten()
        .filter(|diag| {
            matches!(&diag.code, Some(NumberOrString::String(code)) if matches!(code.as_str(), "I208" | "I209"))
                && ranges_overlap(diag.range, params.range)
        })
        .filter_map(|diag| {
            let root = writing_root(diag)?;
            inner
                .reports
                .contains_key(&root)
                .then(|| (root, diag.clone()))
        })
        .collect();
    notes.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
    notes.dedup_by(|a, b| a.0 == b.0 && a.1.data == b.1.data);
    let shared_site = notes
        .iter()
        .map(|(root, _)| root)
        .collect::<HashSet<_>>()
        .len()
        > 1;
    if !params.context.diagnostics.is_empty() {
        notes.retain(|(_, note)| {
            params.context.diagnostics.iter().any(|supplied| {
                supplied.code == note.code
                    && supplied.range == note.range
                    && supplied.data.is_some()
                    && supplied.data == note.data
            })
        });
    }
    notes
        .into_iter()
        .filter_map(|(root, note)| naming_action_for_root(inner, params, &root, note, shared_site))
        .collect()
}

fn writing_root(diag: &Diagnostic) -> Option<Url> {
    if !matches!(&diag.code, Some(NumberOrString::String(code)) if matches!(code.as_str(), "I208" | "I209"))
    {
        return None;
    }
    Url::parse(diag.data.as_ref()?.get("root")?.as_str()?).ok()
}

fn naming_action_for_root(
    inner: &mut Inner,
    params: &CodeActionParams,
    root: &Url,
    note: Diagnostic,
    shared_site: bool,
) -> Option<CodeAction> {
    inner.prepare_root(root);
    let revision = note.data.as_ref()?.get("input_revision")?.as_str()?;
    if inner.workspace.input_revision(root.as_str()).as_deref() != Some(revision) {
        return None;
    }
    if params
        .context
        .diagnostics
        .iter()
        .filter(|diag| writing_root(diag).as_ref() == Some(root))
        .any(|diag| {
            diag.data
                .as_ref()
                .and_then(|data| data.get("input_revision"))
                .and_then(Value::as_str)
                .is_some_and(|supplied| supplied != revision)
        })
    {
        return None;
    }
    let context = serde_json::from_value::<crate::diagnostic::WritingContext>(
        note.data.as_ref()?.get("writing_context")?.clone(),
    )
    .ok()?;
    let plan = match &note.code {
        Some(NumberOrString::String(code)) if code == "I208" => {
            equation_name_plan(&mut inner.workspace, root.as_str(), &context)?
        }
        Some(NumberOrString::String(code)) if code == "I209" => {
            long_name_plan(&mut inner.workspace, root.as_str(), &context)?
        }
        _ => return None,
    };
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    for edit in plan.edits {
        let url = naming_edit_url(inner, &params.text_document.uri, &edit.file)?;
        let text = inner.workspace.get_source(&edit.file)?;
        let index = LineIndex::new(text);
        changes.entry(url).or_default().push(TextEdit {
            range: span_range(&index, text, edit.span),
            new_text: edit.new_text,
        });
    }
    if changes.is_empty() {
        return None;
    }
    let title = if shared_site {
        let path = root
            .to_file_path()
            .ok()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.to_string());
        format!("{} in {path}", plan.title)
    } else {
        plan.title
    };
    Some(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![note]),
        edit: Some(versioned_workspace_edit(inner, changes)),
        command: None,
        is_preferred: Some(true),
        disabled: None,
        data: None,
    })
}

fn versioned_workspace_edit(inner: &Inner, changes: HashMap<Url, Vec<TextEdit>>) -> WorkspaceEdit {
    let mut changes: Vec<_> = changes.into_iter().collect();
    changes.sort_by(|left, right| left.0.as_str().cmp(right.0.as_str()));
    WorkspaceEdit {
        document_changes: Some(DocumentChanges::Edits(
            changes
                .into_iter()
                .map(|(uri, edits)| {
                    let version = inner.document(&uri).map(|doc| doc.version);
                    TextDocumentEdit {
                        text_document: OptionalVersionedTextDocumentIdentifier { uri, version },
                        edits: edits.into_iter().map(OneOf::Left).collect(),
                    }
                })
                .collect(),
        )),
        ..WorkspaceEdit::default()
    }
}

fn naming_edit_url(inner: &Inner, open: &Url, file_key: &str) -> Option<Url> {
    if crate::include_resolver::normalize_uri(open.as_str()) == file_key {
        return Some(open.clone());
    }
    if let Some(url) = inner
        .docs
        .keys()
        .find(|url| crate::include_resolver::normalize_uri(url.as_str()) == file_key)
    {
        return Some(url.clone());
    }
    file_url_from_path_key(file_key)
}

fn span_pos(pos: Position) -> crate::span::Position {
    crate::span::Position {
        line: pos.line,
        character: pos.character,
    }
}

/// A stored fix uses scalar columns. The edit sent to the editor is UTF-16.
fn lsp_pos_from_scalar(index: &LineIndex, text: &str, line: u32, character: u32) -> Position {
    let byte = index.offset(text, crate::span::Position { line, character });
    let pos = index.position_utf16(text, byte);
    Position::new(pos.line, pos.character)
}

fn lsp_macro_messages(
    workspace: &crate::workspace::Workspace,
    root: &str,
    report: &crate::expand::ExpandReport,
) -> Vec<serde_json::Value> {
    report
        .macro_messages
        .iter()
        .map(|message| {
            let source = message
                .file
                .as_deref()
                .and_then(|file| workspace.get_source(file))
                .or_else(|| workspace.get_source(root));
            let mut row = serde_json::json!({
                "kind": message.kind,
                "message": message.message,
            });
            if let Some(source) = source {
                let index = LineIndex::new(source);
                row["range"] = serde_json::json!(span_range(&index, source, message.span));
            }
            if let Some(file) = &message.file {
                if let Some(uri) = file_url_from_path_key(file) {
                    row["uri"] = serde_json::json!(uri);
                }
            }
            row
        })
        .collect()
}

fn span_range(index: &LineIndex, text: &str, span: Span) -> Range {
    let start = index.position_utf16(text, span.start);
    let end = index.position_utf16(text, span.end);
    Range::new(
        Position::new(start.line, start.character),
        Position::new(end.line, end.character),
    )
}

fn file_url_from_path_key(path_key: &str) -> Option<Url> {
    if crate::include_resolver::is_virtual_uri(path_key) {
        return Url::parse(path_key).ok();
    }
    let path = Path::new(path_key);
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    Url::from_file_path(path).ok()
}

fn origin_lsp_range(workspace: &Workspace, origin_uri: Option<&str>, span: Span) -> Range {
    let text = origin_uri
        .and_then(|u| workspace.get_source(u))
        .unwrap_or("");
    let index = LineIndex::new(text);
    span_range(&index, text, span)
}

fn preview_written_location(
    inner: &Inner,
    root: &Url,
    segment: &crate::model_map::WrittenSegment,
) -> Option<Value> {
    let key = segment.file.as_deref()?;
    let text = inner.workspace.get_source(key)?;
    if segment.span.is_empty() {
        return None;
    }
    let range = crate::server_model_map::written_range(text, segment.span)?;
    let uri = inner
        .docs
        .keys()
        .chain(std::iter::once(root))
        .find(|uri| crate::include_resolver::normalize_uri(uri.as_str()) == key)
        .cloned()
        .or_else(|| file_url_from_path_key(key))?;
    if !["file", "untitled"].contains(&uri.scheme()) {
        return None;
    }
    Some(
        json!({"uri":uri,"range":range,"document_version":inner.document(&uri).map(|doc|doc.version)}),
    )
}

fn incomplete_reason_location(
    inner: &mut Inner,
    root: &Url,
    span: crate::span::Span,
    root_written: bool,
) -> Value {
    // E061 records already anchor at the written root include site. Macro
    // failures instead carry offsets in the include-spliced effective source.
    let origin = if root_written {
        Some((crate::include_resolver::normalize_uri(root.as_str()), span))
    } else {
        inner.workspace.map_effective_origin(root.as_str(), span)
    };
    let Some((file, written)) = origin else {
        return Value::Null;
    };
    let Some(text) = inner.workspace.get_source(&file) else {
        return Value::Null;
    };
    let Some(range) = crate::server_model_map::written_range(text, written) else {
        return Value::Null;
    };
    let Some(uri) = inner
        .docs
        .keys()
        .chain(std::iter::once(root))
        .find(|uri| crate::include_resolver::normalize_uri(uri.as_str()) == file)
        .cloned()
        .or_else(|| file_url_from_path_key(&file))
    else {
        return Value::Null;
    };
    if !["file", "untitled"].contains(&uri.scheme()) {
        return Value::Null;
    }
    json!({
        "uri": uri,
        "range": range,
        "document_version": inner.document(&uri).map(|doc| doc.version),
    })
}

fn origin_uri_json(path_key: Option<&str>) -> Option<String> {
    path_key
        .and_then(file_url_from_path_key)
        .map(|u| u.to_string())
}

fn equation_origin_json(
    workspace: &Workspace,
    origin: &EquationOrigin,
    has_heterogeneous: bool,
) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("index".into(), json!(origin.index));
    if has_heterogeneous {
        obj.insert("scope_index".into(), json!(origin.scope_index));
        if let Some(dimension) = &origin.dimension {
            obj.insert("scope".into(), json!("heterogeneous"));
            obj.insert("dimension".into(), json!(dimension));
            obj.insert("block_index".into(), json!(origin.block_index));
        } else {
            obj.insert("scope".into(), json!("aggregate"));
        }
    }
    if let Some(uri) = origin_uri_json(origin.origin_uri.as_deref()) {
        obj.insert("origin_uri".into(), json!(uri));
    }
    obj.insert(
        "range".into(),
        json!(origin_lsp_range(
            workspace,
            origin.origin_uri.as_deref(),
            origin.origin_span,
        )),
    );
    if !origin.origin_frames.is_empty() {
        let frames: Vec<Value> = origin
            .origin_frames
            .iter()
            .map(|frame| origin_frame_json(workspace, frame))
            .collect();
        obj.insert("origin_frames".into(), Value::Array(frames));
    }
    Value::Object(obj)
}

fn origin_frame_json(workspace: &Workspace, frame: &OriginFrame) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("kind".into(), json!(frame.kind));
    if let Some(variable) = &frame.variable {
        obj.insert("variable".into(), json!(variable));
    }
    if let Some(value) = &frame.value {
        obj.insert("value".into(), json!(value));
    }
    if let Some(uri) = origin_uri_json(frame.origin_uri.as_deref()) {
        obj.insert("origin_uri".into(), json!(uri));
    }
    obj.insert(
        "range".into(),
        json!(origin_lsp_range(
            workspace,
            frame.origin_uri.as_deref(),
            frame.origin_span,
        )),
    );
    Value::Object(obj)
}

fn shocks_overwrite_doc(
    src: &str,
    byte: u32,
    cmd: &str,
    name: &str,
    command_doc: &'static str,
) -> &'static str {
    if cmd.eq_ignore_ascii_case("shocks")
        && name.eq_ignore_ascii_case("overwrite")
        && enclosing_paren_has_ident(src, byte, "heterogeneity")
    {
        return HET_SHOCKS_OVERWRITE;
    }
    if command_doc.is_empty() {
        option_doc(name)
    } else {
        command_doc
    }
}

fn heterogeneity_declaration_head(src: &str, byte: u32) -> Option<String> {
    let owner = option_owner_at(src, byte)?;
    if ["var", "varexo", "parameters", "model"]
        .iter()
        .any(|head| owner.eq_ignore_ascii_case(head))
    {
        Some(owner.to_ascii_lowercase())
    } else {
        None
    }
}

fn markdown_hover(value: String, range: Option<Range>) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range,
    }
}

fn decl_hover_markdown(
    model: &Model,
    word: &str,
    preferences: &PresentationSettings,
) -> Option<String> {
    let name = model.intern.lookup(word)?;
    let kind = model
        .final_symbol_kind(name)
        .or_else(|| model.final_kind_or_written_if_excluded(name));
    if kind == Some("var") {
        let mut parts = vec![format!("**Endogenous variable**: `{word}`")];
        if let Some(info) = classify_variable_timing(model).get(word) {
            parts.push(format_timing_line(info));
        }
        append_name_metadata(&mut parts, model, word, preferences);
        return Some(parts.join("\n\n"));
    }
    if kind == Some("varexo_det") {
        let mut parts = vec![format!("**Deterministic exogenous variable**: `{word}`")];
        append_name_metadata(&mut parts, model, word, preferences);
        return Some(parts.join("\n\n"));
    }
    if kind == Some("varexo") {
        let mut parts = vec![format!("**Exogenous variable**: `{word}`")];
        append_name_metadata(&mut parts, model, word, preferences);
        return Some(parts.join("\n\n"));
    }
    if kind == Some("parameters") {
        let mut parts = vec![format!("**Parameter**: `{word}`")];
        match assigned_number(model, word) {
            Some(n) => parts.push(format!("Value: `{n}`")),
            None => parts.push("Value: *not available to Dygnosis*".into()),
        }
        append_name_metadata(&mut parts, model, word, preferences);
        return Some(parts.join("\n\n"));
    }
    None
}

fn append_name_metadata(
    parts: &mut Vec<String>,
    model: &Model,
    word: &str,
    preferences: &PresentationSettings,
) {
    let Some(declaration) = model
        .written_declarations
        .iter()
        .find(|written| model.name(written.declaration.name) == word)
        .map(|written| &written.declaration)
    else {
        return;
    };
    if preferences.name_details.long_name {
        if let Some(long) = &declaration.long_name {
            parts.push(crate::server_names::literal(long));
        }
    }
    if preferences.name_details.tex {
        if let Some(tex) = &declaration.tex_name {
            parts.push(format!("TeX: {}", crate::server_names::code(tex)));
        }
    }
}

fn find_named<'a>(decls: &'a [Decl], model: &Model, word: &str) -> Option<&'a Decl> {
    decls.iter().find(|d| model.name(d.name) == word)
}

fn find_decl<'a>(model: &'a Model, word: &str) -> Option<&'a Decl> {
    find_named(&model.endogenous, model, word)
        .or_else(|| find_named(&model.exogenous, model, word))
        .or_else(|| find_named(&model.parameters, model, word))
        .or_else(|| find_named(&model.retyped_trend_decls, model, word))
}

fn is_declared_in_open(inner: &Inner, word: &str) -> bool {
    inner.docs.keys().any(|uri| {
        inner
            .workspace
            .get_model(uri.as_str())
            .and_then(|m| find_decl(m, word))
            .is_some()
    })
}

fn uri_is_mod_or_inc(uri: &Url) -> bool {
    let ext = uri
        .to_file_path()
        .ok()
        .and_then(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_ascii_lowercase())
        })
        .or_else(|| {
            Path::new(uri.path())
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_ascii_lowercase())
        });
    matches!(ext.as_deref(), Some("mod" | "inc"))
}

fn fix_title(d: &crate::Diagnostic) -> String {
    if let Some(rest) = d.message.split_once("Fix:") {
        let suffix = rest.1.trim();
        if !suffix.is_empty() {
            if suffix.len() > 120 {
                return format!("{}...", suffix.chars().take(117).collect::<String>().trim());
            }
            return suffix.to_string();
        }
    }
    format!("Apply fix for {}", d.code)
}

fn context_owns_stored_fix(
    context: &CodeActionContext,
    diagnostic: &crate::Diagnostic,
    diag_range: Range,
    row: &RoutedDiagnostic,
) -> bool {
    context.diagnostics.iter().any(|supplied| {
        let Some(NumberOrString::String(code)) = &supplied.code else {
            return false;
        };
        let Some(NumberOrString::String(published_code)) = &row.lsp_diagnostic.code else {
            return false;
        };
        // Two stored rows can share code, range, root, and revision. The
        // published message, source, and severity are what the client selected.
        if code != &diagnostic.code
            || code != published_code
            || supplied.range != diag_range
            || supplied.range != row.lsp_diagnostic.range
            || supplied.message != row.lsp_diagnostic.message
            || supplied.source != row.lsp_diagnostic.source
            || supplied.severity != row.lsp_diagnostic.severity
        {
            return false;
        }
        let Some(data) = supplied.data.as_ref().and_then(Value::as_object) else {
            return false;
        };
        let Some(root) = data.get("root").and_then(Value::as_str) else {
            return false;
        };
        let Some(revision) = data.get("input_revision").and_then(Value::as_str) else {
            return false;
        };
        if root != row.root.as_str() || revision != row.revision {
            return false;
        }
        match (data.get("writing_context"), diagnostic.writing.as_ref()) {
            (None, None) => true,
            (Some(supplied_writing), Some(owner)) => {
                serde_json::from_value::<crate::diagnostic::WritingContext>(
                    supplied_writing.clone(),
                )
                .ok()
                .as_ref()
                    == Some(owner)
            }
            _ => false,
        }
    })
}

fn action_kind_requested(only: &Option<Vec<CodeActionKind>>, kind: &CodeActionKind) -> bool {
    match only {
        None => true,
        Some(kinds) => {
            let got = kind.as_str();
            kinds.iter().any(|want| {
                let want = want.as_str();
                got == want || got.starts_with(&format!("{want}."))
            })
        }
    }
}

fn ranges_overlap(left: Range, right: Range) -> bool {
    let left_start = (left.start.line, left.start.character);
    let left_end = (left.end.line, left.end.character);
    let right_start = (right.start.line, right.start.character);
    let right_end = (right.end.line, right.end.character);
    if right_start == right_end {
        left_start <= right_start && right_start <= left_end
    } else {
        left_start < right_end && right_start < left_end
    }
}

fn metadata_completion_item(inner: &mut Inner, uri: &Url, byte: u32) -> Option<CompletionItem> {
    let mut roots = inner.known_owner_roots(uri);
    if roots.is_empty() && is_model_root(uri) {
        roots.push(uri.clone());
    }
    let mut agreed = None;
    for root in roots {
        inner.prepare_root(&root);
        let completion =
            metadata_completion(&mut inner.workspace, root.as_str(), uri.as_str(), byte)?;
        if agreed
            .as_ref()
            .is_some_and(|previous| previous != &completion)
        {
            return None;
        }
        agreed = Some(completion);
    }
    let completion = agreed?;
    let text = inner.document(uri)?.text.as_str();
    let index = LineIndex::new(text);
    let replacement = &completion.edit.new_text;
    let new_text = if inner.completion_snippets {
        let literal = |text: &str| {
            text.replace('\\', "\\\\")
                .replace('$', "\\$")
                .replace('}', "\\}")
        };
        format!(
            "{}${{1:{}}}{}",
            literal(&replacement[..completion.value.start]),
            &replacement[completion.value.clone()],
            literal(&replacement[completion.value.end..])
        )
    } else {
        replacement.clone()
    };
    Some(CompletionItem {
        label: completion.label.into(),
        kind: Some(CompletionItemKind::PROPERTY),
        detail: Some(
            if completion.label == "name" {
                "Equation name tag"
            } else {
                "Symbol long name"
            }
            .into(),
        ),
        insert_text_format: Some(if inner.completion_snippets {
            InsertTextFormat::SNIPPET
        } else {
            InsertTextFormat::PLAIN_TEXT
        }),
        text_edit: Some(CompletionTextEdit::Edit(TextEdit {
            range: span_range(&index, text, completion.edit.span),
            new_text,
        })),
        ..CompletionItem::default()
    })
}

fn default_completions(
    model: &Model,
    preferences: &PresentationSettings,
    label_details: bool,
    snippets: bool,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    for (kw, doc) in DYNARE_KEYWORDS.iter().chain(FAMILY_COMMAND_HELP) {
        items.push(CompletionItem {
            label: (*kw).into(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("Dynare keyword".into()),
            documentation: Some(Documentation::String((*doc).into())),
            ..CompletionItem::default()
        });
    }
    for d in model.final_decls(&["var"]) {
        let name = model.name(d.name);
        items.push(name_completion(
            d,
            name,
            "endogenous variable",
            CompletionItemKind::VARIABLE,
            preferences,
            label_details,
        ));
    }
    for d in model.final_decls(&["varexo", "varexo_det"]) {
        let name = model.name(d.name);
        let role = if model.final_symbol_kind(d.name) == Some("varexo_det") {
            "deterministic exogenous variable"
        } else {
            "exogenous variable"
        };
        items.push(name_completion(
            d,
            name,
            role,
            CompletionItemKind::EVENT,
            preferences,
            label_details,
        ));
    }
    for d in model.final_parameters() {
        let name = model.name(d.name);
        items.push(name_completion(
            d,
            name,
            "parameter",
            CompletionItemKind::CONSTANT,
            preferences,
            label_details,
        ));
    }
    for (name, doc) in BUILTIN_FNS.iter().chain(FAMILY_OPERATOR_HELP) {
        items.push(CompletionItem {
            label: (*name).into(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some("built-in function".into()),
            documentation: Some(Documentation::String((*doc).into())),
            ..CompletionItem::default()
        });
    }
    for block in ["model", "steady_state_model", "initval", "endval", "shocks"] {
        items.push(CompletionItem {
            label: block.into(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some(format!("{block} block")),
            insert_text: Some(format!(
                "{block};\n{}\nend;",
                if snippets { "$0" } else { "" }
            )),
            insert_text_format: Some(if snippets {
                InsertTextFormat::SNIPPET
            } else {
                InsertTextFormat::PLAIN_TEXT
            }),
            ..CompletionItem::default()
        });
    }
    items
}

fn name_completion(
    declaration: &Decl,
    name: &str,
    role: &str,
    kind: CompletionItemKind,
    preferences: &PresentationSettings,
    supports_details: bool,
) -> CompletionItem {
    let long = declaration
        .long_name
        .as_ref()
        .filter(|_| preferences.name_details.long_name);
    let detail = if !supports_details {
        long.map(|long| format!("{role} · {long}"))
            .unwrap_or_else(|| role.to_string())
    } else {
        role.to_string()
    };
    let label_details = if supports_details {
        long.map(|long| CompletionItemLabelDetails {
            detail: None,
            description: Some(long.clone()),
        })
    } else {
        None
    };
    let documentation = declaration
        .tex_name
        .as_ref()
        .filter(|_| preferences.name_details.tex)
        .map(|tex| {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!("TeX: {}", crate::server_names::code(tex)),
            })
        });
    CompletionItem {
        label: name.to_string(),
        insert_text: Some(name.to_string()),
        kind: Some(kind),
        detail: Some(detail),
        label_details,
        documentation,
        ..CompletionItem::default()
    }
}

#[allow(deprecated)]
fn flatten_workspace_symbols(
    uri: &Url,
    symbols: &[DocumentSymbol],
    container: Option<&str>,
    query: &str,
    out: &mut Vec<SymbolInformation>,
) {
    for sym in symbols {
        let matches = query.is_empty() || sym.name.to_ascii_lowercase().contains(query);
        if matches {
            out.push(SymbolInformation {
                name: sym.name.clone(),
                kind: sym.kind,
                tags: None,
                deprecated: None,
                location: Location::new(uri.clone(), sym.range),
                container_name: container.map(str::to_string),
            });
        }
        if let Some(children) = &sym.children {
            flatten_workspace_symbols(uri, children, Some(&sym.name), query, out);
        }
    }
}

fn lsp_severity(severity: Severity) -> DiagnosticSeverity {
    match severity {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
        Severity::Information => DiagnosticSeverity::INFORMATION,
        Severity::Hint => DiagnosticSeverity::HINT,
    }
}

fn lsp_tags(tags: &[i32]) -> Option<Vec<DiagnosticTag>> {
    let result: Vec<_> = [
        (1, DiagnosticTag::UNNECESSARY),
        (2, DiagnosticTag::DEPRECATED),
    ]
    .into_iter()
    .filter_map(|(value, tag)| tags.contains(&value).then_some(tag))
    .collect();
    (!result.is_empty()).then_some(result)
}

fn is_dropped_code(code: &str) -> bool {
    OUT_CODES.contains(&code)
}

fn extract_command_uri(arguments: &[Value]) -> Option<Url> {
    let first = arguments.first()?;
    let obj = if first.is_array() {
        first.as_array()?.first()?
    } else {
        first
    };
    let s = if let Some(s) = obj.as_str() {
        s.to_string()
    } else {
        obj.get("uri")?.as_str()?.to_string()
    };
    Url::parse(&s).ok().or_else(|| Url::from_file_path(&s).ok())
}

/// In-process server for tests. Stdio entry is [`run_stdio`].
pub fn new_service() -> (LspService<Backend>, ClientSocket) {
    LspService::build(Backend::new)
        .custom_method("dynare/activeModelChanged", Backend::active_model_changed)
        .finish()
}

/// Run the language server over stdin/stdout.
pub async fn run_stdio() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = new_service();
    Server::new(stdin, stdout, socket)
        .serve(ordering::OrderedService::new(service))
        .await;
}

/// Debug TCP listener (one accept). Not the editor ship path.
pub async fn run_tcp(host: &str, port: u16) {
    let listener = tokio::net::TcpListener::bind((host, port))
        .await
        .unwrap_or_else(|e| panic!("TCP bind {host}:{port}: {e}"));
    let (stream, _) = listener
        .accept()
        .await
        .unwrap_or_else(|e| panic!("TCP accept: {e}"));
    let (read, write) = tokio::io::split(stream);
    let (service, socket) = new_service();
    Server::new(read, write, socket)
        .serve(ordering::OrderedService::new(service))
        .await;
}

fn pos_in_range(pos: Position, range: Range) -> bool {
    let p = (pos.line, pos.character);
    p >= (range.start.line, range.start.character) && p <= (range.end.line, range.end.character)
}

fn pos_in_range_half_open(pos: Position, range: Range) -> bool {
    let p = (pos.line, pos.character);
    p >= (range.start.line, range.start.character) && p < (range.end.line, range.end.character)
}

fn range_has_pos(range: Range, pos: Position) -> bool {
    pos_in_range(pos, range)
}

fn range_strictly_contains(outer: Range, inner: Range) -> bool {
    if outer.start == inner.start && outer.end == inner.end {
        return false;
    }
    pos_in_range(inner.start, outer) && pos_in_range(inner.end, outer)
}

fn line_end_utf16(text: &str, line: u32) -> u32 {
    let index = LineIndex::new(text);
    let last = index.position_utf16(text, text.len() as u32).line;
    if line > last {
        return 0;
    }
    let byte = index.offset_utf16(
        text,
        crate::span::Position {
            line,
            character: u32::MAX,
        },
    );
    index.position_utf16(text, byte).character
}

fn full_document_range(text: &str) -> Range {
    let index = LineIndex::new(text);
    let line = index.position_utf16(text, text.len() as u32).line;
    Range::new(
        Position::new(0, 0),
        Position::new(line, line_end_utf16(text, line)),
    )
}

fn full_document_edit(old_text: &str, new_text: String) -> TextEdit {
    TextEdit {
        range: full_document_range(old_text),
        new_text,
    }
}

fn line_range_edit(text: &str, start_line: u32, end_line: u32, replacement: String) -> TextEdit {
    let end = line_end_utf16(text, end_line);
    TextEdit {
        range: Range::new(Position::new(start_line, 0), Position::new(end_line, end)),
        new_text: replacement,
    }
}

fn block_comment_folds(text: &str) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < bytes.len() {
                i += 2;
                let start_line = text[..start].bytes().filter(|&b| b == b'\n').count() as u32;
                let end_line = text[..i].bytes().filter(|&b| b == b'\n').count() as u32;
                if start_line < end_line {
                    out.push((start_line, end_line));
                }
            }
            continue;
        }
        i += 1;
    }
    out
}

fn equation_at<'a>(
    model: &'a Model,
    index: &LineIndex,
    text: &str,
    pos: Position,
) -> Option<&'a Equation> {
    for eq in model
        .equations
        .iter()
        .chain(model.steady_state_equations.iter())
    {
        let rng = span_range(index, text, eq.span);
        if range_has_pos(rng, pos) {
            return Some(eq);
        }
    }
    None
}

fn is_model_root(uri: &Url) -> bool {
    Path::new(uri.path())
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("mod") || ext.eq_ignore_ascii_case("dyn"))
}

enum ModelInfoChanged {}

impl notification::Notification for ModelInfoChanged {
    type Params = Value;
    const METHOD: &'static str = "dynare/modelInfoChanged";
}

fn explain_command(arguments: &[Value]) -> Value {
    let code = extract_explain_code(arguments);
    let Some(code) = code else {
        return Value::Null;
    };
    match explain::render_markdown(&code) {
        Some(md) => Value::String(md),
        None => {
            let known = explain::known_codes().join(", ");
            Value::String(format!(
                "Diagnostic `{code}` is not documented. Known codes: {known}"
            ))
        }
    }
}

fn extract_explain_code(arguments: &[Value]) -> Option<String> {
    let first = arguments.first()?;
    let obj = if first.is_array() {
        first.as_array()?.first()?
    } else {
        first
    };
    if let Some(s) = obj.as_str() {
        return Some(s.to_string());
    }
    obj.get("code")?.as_str().map(|s| s.to_string())
}

fn parse_compare_args(arguments: &[Value]) -> std::result::Result<(String, String), Value> {
    let values: &[Value] = if arguments.len() == 1 && arguments[0].is_array() {
        arguments[0]
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or(arguments)
    } else {
        arguments
    };
    if values.len() >= 2 {
        if let (Some(a), Some(b)) = (values[0].as_str(), values[1].as_str()) {
            return Ok((a.to_string(), b.to_string()));
        }
    }
    if let Some(obj) = values.first().and_then(|v| v.as_object()) {
        let a = obj
            .get("uri_a")
            .or_else(|| obj.get("uriA"))
            .and_then(|v| v.as_str());
        let b = obj
            .get("uri_b")
            .or_else(|| obj.get("uriB"))
            .and_then(|v| v.as_str());
        if let (Some(a), Some(b)) = (a, b) {
            return Ok((a.to_string(), b.to_string()));
        }
    }
    Err(json!({
        "error": "compareModels requires uri_a and uri_b arguments",
        "code": "BAD_ARGS",
    }))
}

fn ch_variable_item(
    model: &Model,
    index: &LineIndex,
    text: &str,
    uri: &Url,
    name: &str,
) -> CallHierarchyItem {
    let rng = find_decl(model, name)
        .map(|d| span_range(index, text, d.span))
        .unwrap_or_else(|| Range::new(Position::new(0, 0), Position::new(0, 0)));
    CallHierarchyItem {
        name: name.to_string(),
        kind: SymbolKind::VARIABLE,
        tags: None,
        detail: Some("variable".into()),
        uri: uri.clone(),
        range: rng,
        selection_range: rng,
        data: Some(json!({"dynare": "variable", "name": name})),
    }
}

fn ch_equation_item(
    _model: &Model,
    index: &LineIndex,
    text: &str,
    uri: &Url,
    eq: &Equation,
) -> CallHierarchyItem {
    let rng = span_range(index, text, eq.span);
    let name = if !eq.name.is_empty() {
        eq.name.clone()
    } else if !eq.lhs.trim().is_empty() {
        format!("{} = ...", eq.lhs.trim())
    } else {
        eq.text
            .lines()
            .next()
            .unwrap_or("equation")
            .chars()
            .take(48)
            .collect()
    };
    CallHierarchyItem {
        name,
        kind: SymbolKind::FUNCTION,
        tags: None,
        detail: Some("equation".into()),
        uri: uri.clone(),
        range: rng,
        selection_range: rng,
        data: Some(json!({"dynare": "equation", "start": eq.span.start})),
    }
}

#[cfg(test)]
mod diagnostic_revision_tests {
    use super::*;

    #[test]
    fn model_change_announces_the_same_input_token_as_diagnostics() {
        let root = Url::parse("file:///C:/dygnosis-preview/revision-notification.mod").unwrap();
        let mut inner = Inner::default();
        inner
            .workspace
            .update_document(root.as_str(), "var y; model; y=1; end;");
        inner.root_revision(&root).unwrap();
        inner
            .workspace
            .update_document(root.as_str(), "var y; model; y=2; end;");
        let input_revision = inner.workspace.input_revision(root.as_str()).unwrap();
        let changes = inner.changed_model_info();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0]["input_revision"], input_revision);
        assert_eq!(changes[0]["revision"], inner.root_revision(&root).unwrap());
    }
}
