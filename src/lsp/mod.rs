//! LSP server (stdio). Wave a: document loop. Wave b: navigation / edit. Wave c: intel / format / commands.

pub(crate) mod model_map;
pub(crate) mod names;
pub mod settings;
pub(crate) mod signature_help;
mod syntax_intel;
mod typed_names;

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

mod analysis_configuration;
mod comparison_inputs;
mod document_analysis;
mod document_context;
mod source_activity;
mod source_intelligence;
mod source_projection;
mod workspace_checks;
mod written_requests;

mod ordering;
mod parameter_hints;
mod presentation_refresh;

mod locals;

mod rename;

#[cfg(test)]
thread_local! {
    static SYNTAX_CONTEXT_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod ordering_tests;

#[cfg(test)]
mod edit_analysis_tests;

use serde_json::{Value, json};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, ClientSocket, LanguageServer, LspService, Server};

use crate::engine::analysis::model_info::{
    TimingClass, assigned_number, classify_variable_timing, format_timing_line,
};
use crate::engine::catalog::{HET_SHOCKS_OVERWRITE, HETEROGENEITY_OPTION, option_doc};
use crate::engine::diagnostics::explain;
use crate::engine::diagnostics::{DiagnosticSet, check_file, check_in_workspace_with_origins};
use crate::engine::editing::equation_names::{
    equation_name_plan, long_name_plan, metadata_completion,
};
use crate::engine::editing::format::{
    FormatOutcome, RangeFormatOutcome, format_outcome_with_limit, format_range_outcome_with_limit,
};
use crate::engine::editing::refs::{
    enclosing_paren_has_ident, ident_at, is_legal_ident, option_command_at, option_owner_at,
};
use crate::engine::language::expand::{EquationOrigin, OriginFrame};
use crate::engine::language::lexer::{TokenKind, tokenize};
use crate::engine::language::model::{Decl, Equation, Model};
use crate::engine::language::span::{LineIndex, Span};
use crate::engine::workspace::{SourceDocument, Workspace};
use crate::lsp::model_map::{WrittenView, common_leaves};
use crate::lsp::names::{NameRole, NameSites, SemanticMapping};
use crate::lsp::settings::{
    CONFIGURATION_SCHEMA_VERSION, MODEL_INFO_SCHEMA_VERSION, PresentationSettings,
    ResourceSettings, SettingsStore,
};
use crate::{Severity, VERSION};

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
    incomplete_cause: Option<String>,
    reusable: bool,
    stamps: crate::engine::workspace::InputStamps,
    owner_files: HashSet<String>,
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

/// Request-local handles to Workspace's existing model and written map.
type NameView = (
    Arc<Model>,
    Arc<crate::engine::language::expand::ExpandReport>,
);

#[cfg(test)]
thread_local! {
    static VALUE_HINT_SOURCE_INDEXES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn value_hint_source_index(source: &crate::engine::workspace::SourceDocument) -> Arc<LineIndex> {
    #[cfg(test)]
    VALUE_HINT_SOURCE_INDEXES.with(|indexes| indexes.set(indexes.get() + 1));
    source.source_index()
}

fn value_hint_range(index: &LineIndex, text: &str, span: Span) -> Option<Range> {
    text.get(span.start as usize..span.end as usize)?;
    Some(span_range(index, text, span))
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
            .filter(|_| matches!(&item.code, Some(NumberOrString::String(code)) if crate::engine::diagnostics::check_writing::is_writing_code(code)))
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
    semantic_views: HashMap<Url, Arc<crate::engine::analysis::symbols::SemanticModel>>,
    parameter_hints: HashMap<Url, Arc<parameter_hints::Entry>>,
    source_inspection_notices: HashMap<Url, String>,
    settings: SettingsStore,
    tracked_roots: HashMap<Url, String>,
    model_info_notifications: bool,
    document_context_enabled: bool,
    document_contexts: HashMap<String, Url>,
    analysis_configuration: analysis_configuration::AnalysisState,
    token_refresh: bool,
    hint_refresh: bool,
    semantic_mapping: SemanticMapping,
    completion_label_details: bool,
    completion_snippets: bool,
    publication_versions: HashMap<Url, Option<i32>>,
    reports: HashMap<Url, Arc<RootReport>>,
    workspace_checks: workspace_checks::WorkspaceCheckState,
    document_analysis: document_analysis::DocumentAnalysis,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            docs: HashMap::new(),
            published: HashMap::new(),
            routed_library: HashMap::new(),
            workspace: Workspace::new(),
            semantic_views: HashMap::new(),
            parameter_hints: HashMap::new(),
            source_inspection_notices: HashMap::new(),
            settings: SettingsStore::default(),
            tracked_roots: HashMap::new(),
            model_info_notifications: false,
            document_context_enabled: false,
            document_contexts: HashMap::new(),
            analysis_configuration: analysis_configuration::AnalysisState::default(),
            token_refresh: false,
            hint_refresh: false,
            semantic_mapping: SemanticMapping::default(),
            completion_label_details: false,
            completion_snippets: false,
            publication_versions: HashMap::new(),
            reports: HashMap::new(),
            workspace_checks: workspace_checks::WorkspaceCheckState::default(),
            document_analysis: document_analysis::DocumentAnalysis::default(),
        }
    }
}

impl Inner {
    /// One explanation per sustained source-inspection limitation. Editing a
    /// limited file must not produce the same notice on every source request.
    fn source_inspection_notice(&mut self, uri: &Url) -> Option<String> {
        let inventory = self.workspace.cached_source_inventory(uri.as_str())?;
        let Some(cause) = &inventory.coverage_cause else {
            self.source_inspection_notices.remove(uri);
            return None;
        };
        if self.source_inspection_notices.get(uri) == Some(cause) {
            return None;
        }
        self.source_inspection_notices
            .insert(uri.clone(), cause.clone());
        let file = uri
            .path_segments()
            .and_then(Iterator::last)
            .unwrap_or("this file");
        Some(format!(
            "Source information for {file} is incomplete. {cause} Available Outline entries and folds are retained; source names and syntax help are limited to proven source context."
        ))
    }

    fn source_document(&self, uri: &Url) -> Option<SourceDocument> {
        self.document(uri)?;
        let file = crate::engine::workspace::include_resolver::normalize_uri(uri.as_str());
        self.workspace.source_document(uri.as_str(), &file)
    }

    fn document(&self, uri: &Url) -> Option<&OpenDoc> {
        self.docs.get(uri).or_else(|| {
            let key = crate::engine::workspace::include_resolver::normalize_uri(uri.as_str());
            self.docs
                .iter()
                .find(|(candidate, _)| {
                    crate::engine::workspace::include_resolver::normalize_uri(candidate.as_str())
                        == key
                })
                .map(|(_, doc)| doc)
        })
    }
    fn name_views(&mut self, uri: &Url) -> Option<Vec<NameView>> {
        let roots = self.context_roots(uri);
        if self.context_ambiguous(uri) {
            return None;
        }
        if roots.is_empty() {
            let model = self.workspace.model_snapshot(uri.as_str())?;
            let text = self.workspace.get_source(uri.as_str())?;
            return Some(vec![(
                model,
                Arc::new(crate::engine::language::expand::expand_report(text)),
            )]);
        }
        let mut views = Vec::new();
        for root in roots {
            self.prepare_root(&root);
            let model = self.workspace.effective_model_snapshot(root.as_str())?;
            let map = self.workspace.expand_report_snapshot(root.as_str())?;
            views.push((model, map));
        }
        Some(views)
    }
    fn prepare_root(&mut self, uri: &Url) -> ResourceSettings {
        let mut settings = self.settings.resolve(uri);
        if let Some(paths) = self.analysis_configuration.paths_for(uri) {
            settings.search_paths = paths.to_vec();
        }
        self.workspace
            .set_root_search_paths(uri.as_str(), settings.search_paths.clone());
        self.workspace
            .set_root_macro_work_limit(uri.as_str(), settings.macro_work_limit);
        settings
    }

    fn analysis_configuration_committed(&mut self, root: &Url) {
        self.semantic_views.remove(root);
        self.workspace_analysis_changed(root);
        self.reports.remove(root);
    }

    fn syntax_context(
        &mut self,
        uri: &Url,
        text: &str,
        byte: u32,
        signature: bool,
    ) -> Option<syntax_intel::Context> {
        let inventory = self.workspace.source_inventory(uri.as_str())?;
        let semantic_source = self.semantic_completion_source(uri, byte, &inventory);
        self.syntax_context_with_source(uri, text, byte, signature, &inventory, semantic_source)
    }

    /// Source paths need execution evidence before they can use configured
    /// names. Existing occurrence anchors provide that evidence without a
    /// second macro execution or a scan of enclosing statement text.
    fn semantic_completion_source(
        &mut self,
        uri: &Url,
        byte: u32,
        inventory: &crate::engine::analysis::source_inventory::SourceInventory,
    ) -> bool {
        let macro_token = inventory
            .macro_tokens
            .partition_point(|token| token.span.end < byte);
        if inventory
            .macro_tokens
            .get(macro_token)
            .is_some_and(|token| {
                matches!(token.kind, TokenKind::MacroDir | TokenKind::MacroInterp)
                    && token.span.start <= byte
                    && byte <= token.span.end
            })
        {
            return false;
        }
        if self.context_ambiguous(uri)
            || inventory.coverage_cause.is_some()
            || inventory.macro_path(byte).iter().any(|step| step.loop_body)
        {
            return false;
        }
        let roots = self.context_roots(uri);
        if roots.is_empty() {
            return inventory.macro_path(byte).is_empty();
        }
        let file = crate::engine::workspace::include_resolver::normalize_uri(uri.as_str());
        for root in roots {
            self.prepare_root(&root);
            if self
                .workspace
                .effective_positions(root.as_str(), &file, byte)
                .len()
                != 1
            {
                return false;
            }
            if self
                .workspace
                .get_effective_model(root.as_str())
                .is_none_or(|model| model.macro_incomplete())
                || !self.workspace.includes_complete(root.as_str())
            {
                return false;
            }
            if inventory.macro_path(byte).is_empty() {
                continue;
            }
            let Some(origins) = self.workspace.effective_origins(root.as_str()) else {
                return false;
            };
            let Some(model) = self.workspace.get_effective_model(root.as_str()) else {
                return false;
            };
            if !model.expanded_tokens.iter().any(|token| {
                origins.map(token.span).is_some_and(|(owner, anchor)| {
                    owner == file && inventory.literal_anchor_on_path(anchor, byte)
                })
            }) {
                return false;
            }
        }
        true
    }

    fn syntax_context_with_source(
        &mut self,
        uri: &Url,
        text: &str,
        byte: u32,
        signature: bool,
        inventory: &crate::engine::analysis::source_inventory::SourceInventory,
        semantic_source: bool,
    ) -> Option<syntax_intel::Context> {
        #[cfg(test)]
        SYNTAX_CONTEXT_READS.with(|reads| reads.set(reads.get() + 1));
        let read = if signature {
            syntax_intel::signature_context
        } else {
            syntax_intel::context
        };
        let local = syntax_intel::source_context(inventory, text, byte, signature);
        // Macro and literal/comment checks concern the displayed source. Native
        // fragments can acquire a Dynare context only from a proven include.
        if local
            .as_ref()
            .is_some_and(|context| context.language == "macro_expression")
        {
            return local;
        }
        if self.context_ambiguous(uri) {
            return None;
        }
        if !semantic_source {
            return local;
        }
        let roots = self.context_roots(uri);
        if roots.is_empty() {
            return local;
        }
        let file = crate::engine::workspace::include_resolver::normalize_uri(uri.as_str());
        let mut contexts = Vec::new();
        for root in roots {
            self.prepare_root(&root);
            let positions = self
                .workspace
                .effective_positions(root.as_str(), &file, byte);
            let model = self.workspace.get_effective_model(root.as_str())?;
            for position in positions {
                let context = read(model, &model.source, position)?;
                // Spliced source positions also cover discarded text. A root
                // with no accepted statement here cannot replace a proven
                // written block context with its generic top-level fallback.
                contexts.push(
                    if context.language == "top_level" && context.parent.is_none() {
                        local.clone().unwrap_or(context)
                    } else {
                        context
                    },
                );
            }
        }
        let Some(first) = contexts.first() else {
            return local;
        };
        contexts
            .iter()
            .all(|context| context == first)
            .then(|| first.clone())
    }

    fn presentation_for(&self, uri: &Url) -> PresentationSettings {
        self.settings.resolve(uri).presentation
    }

    fn known_owner_roots(&self, uri: &Url) -> Vec<Url> {
        let owners = self.workspace.owner_roots(uri.as_str());
        let mut roots: Vec<_> = self
            .tracked_roots
            .keys()
            .filter(|root| owners.contains(&self.workspace_checks.observed_identity(root)))
            .cloned()
            .collect();
        roots.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        roots.extend(self.workspace_checks.owners(uri));
        roots.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        let mut seen = HashSet::new();
        roots.retain(|root| seen.insert(self.workspace_checks.observed_identity(root)));
        roots
    }

    fn root_revision(&mut self, uri: &Url) -> Option<String> {
        self.root_revision_with_inputs(uri)
            .map(|(revision, _)| revision)
    }

    fn root_revision_with_inputs(&mut self, uri: &Url) -> Option<(String, String)> {
        self.remember_request_root(uri);
        self.prepare_root(uri);
        let inputs = self.workspace.input_revision(uri.as_str())?;
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        inputs.hash(&mut hash);
        let revision = format!("{:016x}", hash.finish());
        self.tracked_roots
            .entry(uri.clone())
            .or_insert_with(|| revision.clone());
        Some((revision, inputs))
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
        let documents: Vec<Url> = self.docs.keys().cloned().collect();
        // Discover every open model before resolving a helper's context. Its
        // filename sort order must not decide whether it has an owner.
        let mut discover_roots: HashSet<_> = documents
            .iter()
            .filter(|uri| is_model_root(uri))
            .cloned()
            .collect();
        for document in &documents {
            discover_roots.extend(self.known_owner_roots(document));
        }
        discover_roots.extend(self.document_contexts.values().cloned());
        let mut input_revisions = HashMap::new();
        for uri in discover_roots {
            if let Some((_, inputs)) = self.root_revision_with_inputs(&uri) {
                input_revisions.insert(uri, inputs);
            }
        }
        self.revalidate_document_contexts();
        let mut roots = HashSet::new();
        for document in &documents {
            let contexts = self.context_roots(document);
            if contexts.is_empty() && !self.context_ambiguous(document) {
                roots.insert(document.clone());
            } else {
                roots.extend(contexts);
            }
        }
        let mut roots: Vec<_> = roots.into_iter().collect();
        roots.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        for uri in &roots {
            let input_revision = if is_model_root(uri) {
                input_revisions.remove(uri).or_else(|| {
                    self.root_revision_with_inputs(uri)
                        .map(|(_, inputs)| inputs)
                })
            } else {
                self.prepare_root(uri);
                self.workspace.input_revision(uri.as_str())
            };
            if input_revision.as_ref().is_some_and(|revision| {
                self.reports
                    .get(uri)
                    .is_some_and(|report| report.revision == *revision)
            }) {
                continue;
            }
            let set = check_in_workspace_with_origins(&mut self.workspace, uri.as_str());
            let revision = self
                .workspace
                .input_revision(uri.as_str())
                .unwrap_or_default();
            let text = self
                .document(uri)
                .map(|doc| Arc::from(doc.text.as_str()))
                .or_else(|| self.workspace.get_source(uri.as_str()).map(Arc::from))
                .unwrap_or_default();
            let choices = self.workspace.root_analysis_configuration(uri.as_str());
            let mut report = prepare_root_report(uri, set, text, revision, &choices);
            report.reusable = self.workspace.includes_complete(uri.as_str())
                && self
                    .workspace
                    .expand_report(uri.as_str())
                    .is_some_and(|report| report.complete);
            report.stamps = self.workspace.input_stamps(uri.as_str());
            report.owner_files = self.workspace.known_include_files(uri.as_str());
            self.reports.insert(uri.clone(), Arc::new(report));
        }
        self.reports
            .retain(|root, _| roots.contains(root) || self.workspace_checks.is_selected(root));
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
                .or_insert_with(|| {
                    crate::engine::workspace::include_resolver::normalize_uri(uri.as_str())
                })
                .clone()
        };
        let mut source_identities: HashMap<_, _> = self
            .reports
            .keys()
            .map(|uri| (identity(uri), uri.clone()))
            .collect();
        source_identities.extend(self.docs.keys().map(|uri| (identity(uri), uri.clone())));
        let contexts: HashMap<_, _> = self
            .docs
            .keys()
            .map(|document| {
                (
                    identity(document),
                    self.context_roots(document)
                        .iter()
                        .map(&mut identity)
                        .collect::<HashSet<_>>(),
                )
            })
            .collect();
        for (root, report) in &checks {
            for (source, rows) in &report.routes {
                let key = identity(source);
                let uri = source_identities
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| source.clone());
                if let Some(owners) = contexts.get(&key) {
                    let owner = identity(root);
                    // Unknown non-model fragments retain their written check.
                    // Ambiguous native contexts withhold owner-specific notes.
                    if !owners.contains(&owner)
                        && (!owners.is_empty() || self.context_ambiguous(&uri))
                    {
                        continue;
                    }
                }
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
        // Another root can finish while this document's checked report is
        // absent. Keep its last publication until its current owners finish;
        // an empty route here is not a completed check of the edited version.
        let pending: HashSet<_> = self
            .published
            .keys()
            .chain(routed.keys())
            .filter(|uri| self.document_analysis_pending(uri))
            .cloned()
            .collect();
        for uri in &pending {
            if let Some(previous) = self.published.get(uri) {
                routed.insert(uri.clone(), previous.clone());
            } else {
                routed.remove(uri);
            }
            if let Some(previous) = self.routed_library.get(uri) {
                routed_library.insert(uri.clone(), previous.clone());
            } else {
                routed_library.remove(uri);
            }
        }
        self.routed_library = routed_library;
        for (uri, doc) in &mut self.docs {
            if pending.contains(uri) {
                continue;
            }
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
            if !pending.contains(uri) {
                doc.diagnostics = routed.get(uri).cloned().unwrap_or_default();
            }
        }
        let mut publish: Vec<Url> = self
            .published
            .keys()
            .chain(routed.keys())
            .cloned()
            .collect();
        publish.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        publish.dedup();
        if let Some(first) = first
            && let Some(at) = publish.iter().position(|uri| uri == first)
        {
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
        let mut open_versions = HashMap::new();
        for (uri, doc) in &self.docs {
            // Match document(): an exact URI wins, otherwise the first native
            // identity match in the open-document iteration supplies its version.
            open_versions.entry(identity(uri)).or_insert(doc.version);
        }
        let current_versions: HashMap<_, _> = publish
            .iter()
            .map(|uri| {
                let version = if pending.contains(uri) {
                    self.publication_versions.get(uri).copied().flatten()
                } else {
                    self.docs
                        .get(uri)
                        .map(|doc| doc.version)
                        .or_else(|| open_versions.get(&identity(uri)).copied())
                };
                (uri.clone(), version)
            })
            .collect();
        let output = publish
            .into_iter()
            .filter(|uri| {
                !pending.contains(uri)
                    && (self.published.get(uri) != routed.get(uri)
                        || self.publication_versions.get(uri).copied()
                            != Some(current_versions[uri]))
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
#[derive(Clone)]
pub struct Backend {
    client: Client,
    inner: Arc<Mutex<Inner>>,
    written_sources: Arc<Mutex<HashMap<Url, crate::engine::workspace::SourceDocument>>>,
    workspace_wake: Arc<tokio::sync::Notify>,
    document_analysis_done: Arc<tokio::sync::Notify>,
    output_gate: Arc<tokio::sync::Mutex<()>>,
    presentation_refresh: Arc<presentation_refresh::PresentationRefresh>,
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
            written_sources: Arc::new(Mutex::new(HashMap::new())),
            workspace_wake: Arc::new(tokio::sync::Notify::new()),
            document_analysis_done: Arc::new(tokio::sync::Notify::new()),
            output_gate: Arc::new(tokio::sync::Mutex::new(())),
            presentation_refresh: Arc::new(presentation_refresh::PresentationRefresh::default()),
        }
    }

    fn lock_inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    async fn report_source_inspection(&self, uri: &Url) {
        let notice = self.lock_inner().source_inspection_notice(uri);
        if let Some(notice) = notice {
            self.client.show_message(MessageType::INFO, notice).await;
        }
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
        self.presentation_refresh
            .request(&self.client, tokens, hints);
    }

    /// Prepare a checked fixture synchronously for focused presentation tests.
    #[cfg(test)]
    fn upsert(
        &self,
        uri: Url,
        text: String,
        version: i32,
    ) -> Vec<(Url, Option<i32>, Vec<Diagnostic>)> {
        self.store_document(uri.clone(), text, version);
        self.lock_inner().refresh_diagnostics(Some(&uri))
    }

    fn store_document(&self, uri: Url, text: String, version: i32) {
        let mut inner = self.lock_inner();
        inner.document_analysis.retire_aliases(&uri);
        let key = crate::engine::workspace::include_resolver::normalize_uri(uri.as_str());
        let mut sources = self.written_sources.lock().unwrap();
        inner.docs.retain(|candidate, _| {
            let keep = candidate == &uri
                || crate::engine::workspace::include_resolver::normalize_uri(candidate.as_str())
                    != key;
            if !keep {
                sources.remove(candidate);
            }
            keep
        });
        inner.parameter_hints.retain(|candidate, _| {
            candidate != &uri
                && crate::engine::workspace::include_resolver::normalize_uri(candidate.as_str())
                    != key
        });
        inner.align_workspace_root(&uri);
        inner.workspace_changed(&uri, false);
        if is_model_root(&uri) {
            inner.workspace_checks.active = Some(uri.clone());
        }
        inner.prepare_root(&uri);
        inner.workspace.update_document(uri.as_str(), &text);
        if let Some(source) = inner.workspace.source_document(uri.as_str(), &key) {
            sources.insert(uri.clone(), source);
        }
        inner.docs.insert(
            uri.clone(),
            OpenDoc {
                text,
                version,
                diagnostics: Vec::new(),
                library: Vec::new(),
            },
        );
    }

    fn pull_items(&self, uri: &Url) -> Vec<Diagnostic> {
        let inner = self.lock_inner();
        inner
            .published
            .get(uri)
            .or_else(|| {
                let key = crate::engine::workspace::include_resolver::normalize_uri(uri.as_str());
                inner
                    .published
                    .iter()
                    .find(|(candidate, _)| {
                        crate::engine::workspace::include_resolver::normalize_uri(
                            candidate.as_str(),
                        ) == key
                    })
                    .map(|(_, items)| items)
            })
            .cloned()
            .unwrap_or_default()
    }

    /// Written source may prove that no model hover result can exist.
    fn written_hover_at(&self, pos: &TextDocumentPositionParams) -> Option<Option<Hover>> {
        let source = self.written_source(&pos.text_document.uri)?;
        let result = written_requests::hover(&source, pos.position);
        let sources = self.written_sources.lock().unwrap();
        if sources
            .get(&pos.text_document.uri)
            .is_some_and(|current| current.same_source(&source))
        {
            result
        } else {
            None
        }
    }

    #[cfg(test)]
    fn syntax_hover_at(&self, pos: &TextDocumentPositionParams) -> Option<Hover> {
        self.written_hover_at(pos).flatten()
    }

    #[cfg(test)]
    fn hover_at(&self, pos: &TextDocumentPositionParams) -> Option<Hover> {
        self.written_hover_at(pos)
            .unwrap_or_else(|| self.model_hover_at(pos))
    }

    #[cfg(test)]
    fn hover_may_need_model(&self, pos: &TextDocumentPositionParams) -> bool {
        let inner = self.lock_inner();
        let Some(document) = inner.document(&pos.text_document.uri) else {
            return false;
        };
        let Some(inventory) = inner
            .workspace
            .source_inventory(pos.text_document.uri.as_str())
        else {
            return true;
        };
        let index = LineIndex::new(&crate::engine::language::parser::normalize_newlines(
            &document.text,
        ));
        let byte = index.offset_utf16(&document.text, span_pos(pos.position));
        syntax_intel::hover_may_need_model(&inventory, &document.text, byte)
    }

    fn model_hover_at(&self, pos: &TextDocumentPositionParams) -> Option<Hover> {
        let mut inner = self.lock_inner();
        // Every provider reads one validated input snapshot. Retry once if a
        // disk write or lazy acquisition changes it during projection.
        for _ in 0..2 {
            let read = inner.workspace.begin_input_read();
            let hover = Self::model_hover_from_inputs(&mut inner, pos);
            if read.is_current(&inner.workspace) {
                return hover;
            }
        }
        None
    }

    fn model_hover_from_inputs(
        inner: &mut Inner,
        pos: &TextDocumentPositionParams,
    ) -> Option<Hover> {
        if let Some(document) = inner.document(&pos.text_document.uri) {
            let index = inner
                .workspace
                .source_document(
                    pos.text_document.uri.as_str(),
                    &crate::engine::workspace::include_resolver::normalize_uri(
                        pos.text_document.uri.as_str(),
                    ),
                )
                .map(|source| source.source_index())
                .unwrap_or_else(|| {
                    Arc::new(LineIndex::new(
                        &crate::engine::language::parser::normalize_newlines(&document.text),
                    ))
                });
            let byte = index.offset_utf16(&document.text, span_pos(pos.position));
            if let Some(hover) = source_intelligence::hover(inner, &pos.text_document.uri, byte) {
                return Some(hover);
            }
        }
        if let Some(hover) = locals::hover(inner, pos) {
            return hover.map(|mut hover| {
                source_intelligence::label(inner, &pos.text_document.uri, &mut hover);
                hover
            });
        }
        if is_model_root(&pos.text_document.uri) {
            inner.workspace_checks.active = Some(pos.text_document.uri.clone());
        }
        let text = inner.document(&pos.text_document.uri)?.text.clone();
        let normalized = crate::engine::language::parser::normalize_newlines(&text);
        let index = LineIndex::new(&normalized);
        let byte = index.offset_utf16(&text, span_pos(pos.position));
        if let Some(hover) = typed_names::hover(inner, pos) {
            return hover.map(|mut hover| {
                source_intelligence::label(inner, &pos.text_document.uri, &mut hover);
                hover
            });
        }
        let (word, span) = syntax_intel::word_at(&text, byte)?;
        let range = Some(span_range(&index, &text, span));
        if macro_directive_name_at(&normalized, byte).is_some() {
            let result = crate::engine::catalog::syntax::query(
                &crate::engine::catalog::syntax::SyntaxQuery {
                    name: Some(word.clone()),
                    kind: Some("macro_directive".into()),
                    ..Default::default()
                },
            );
            if let Some(record) = result.records.first() {
                return Some(markdown_hover(syntax_intel::documentation(record), range));
            }
        }
        if let Some(cmd) = option_command_at(&text, byte)
            && let Some(option) =
                syntax_intel::options(&cmd).find(|option| option.name.eq_ignore_ascii_case(&word))
        {
            let mut md = format!("**`{cmd}` option**: `{word}`");
            let description =
                shocks_overwrite_doc(&text, byte, &cmd, &option.name, &option.summary);
            if !description.is_empty() {
                md.push_str("\n\n");
                md.push_str(description);
            }
            return Some(markdown_hover(md, range));
        }
        if word.eq_ignore_ascii_case("heterogeneity")
            && let Some(head) = heterogeneity_declaration_head(&text, byte)
        {
            let md = format!("**`{head}` option**: `heterogeneity`\n\n{HETEROGENEITY_OPTION}");
            return Some(markdown_hover(md, range));
        }
        if let Some(context) = inner.syntax_context(&pos.text_document.uri, &text, byte, false) {
            let matches: Vec<_> = syntax_intel::records(&context)
                .into_iter()
                .filter(|record| {
                    record.name.eq_ignore_ascii_case(&word)
                        || record
                            .aliases
                            .iter()
                            .any(|alias| alias.eq_ignore_ascii_case(&word))
                })
                .map(syntax_intel::documentation)
                .collect();
            if !matches.is_empty() {
                return Some(markdown_hover(
                    format!("**`{word}`**\n\n{}", matches.join("\n\n")),
                    range,
                ));
            }
        }
        let preferences = inner.presentation_for(&pos.text_document.uri);
        let roots = inner.context_roots(&pos.text_document.uri);
        let md = if !roots.is_empty() && !inner.context_ambiguous(&pos.text_document.uri) {
            // Local and typed hover validated these same immutable snapshots
            // above while this request held the lock. Borrow their model facts
            // instead of cloning every expression and written-source map.
            let views: Option<Vec<_>> = roots
                .iter()
                .map(|root| inner.semantic_views.get(root))
                .collect();
            let views = views?;
            let mut descriptions = views
                .iter()
                .map(|view| decl_hover_markdown(&view.model, &word, &preferences));
            let first = descriptions.next()??;
            if !descriptions.all(|description| description.as_ref() == Some(&first)) {
                return None;
            }
            first
        } else {
            let views = inner.name_views(&pos.text_document.uri)?;
            let mut descriptions = views
                .iter()
                .map(|(model, _)| decl_hover_markdown(model, &word, &preferences));
            let first = descriptions.next()??;
            if !descriptions.all(|description| description.as_ref() == Some(&first)) {
                return None;
            }
            first
        };
        let mut hover = markdown_hover(md, range);
        source_intelligence::label(inner, &pos.text_document.uri, &mut hover);
        Some(hover)
    }

    fn doc_symbols(&self, uri: &Url) -> Option<Vec<DocumentSymbol>> {
        use crate::engine::analysis::source_inventory::SourceItemKind;

        let mut inner = self.lock_inner();
        let source = inner.source_document(uri)?;
        let text = source.source();
        let preferences = inner.presentation_for(uri);
        let inventory = inner.workspace.source_inventory(uri.as_str())?;
        let roots = inner.context_roots(uri);
        let projected = source_intelligence::file(&mut inner, uri);
        let availability: HashMap<_, _> = projected
            .as_ref()
            .into_iter()
            .flat_map(|file| file.availability.iter())
            .map(|item| (item.source_id.as_str(), item))
            .collect();
        if inner.context_ambiguous(uri) {
            let mut rows = Vec::new();
            model_map::add_source_symbols(&mut rows, &inventory, text, &preferences, |_| {
                crate::engine::analysis_configuration::SourceAvailability::Unknown
            });
            return Some(rows);
        }
        let mut views = Vec::new();
        for root in &roots {
            inner.root_revision(root);
            let Some(model) = inner.workspace.effective_model_snapshot(root.as_str()) else {
                continue;
            };
            let Some(report) = inner.workspace.expand_report_snapshot(root.as_str()) else {
                continue;
            };
            let authoritative = roots.len() == 1
                && report.complete
                && report.model_map.complete
                && inner.workspace.includes_complete(root.as_str());
            let view = WrittenView::new(
                uri,
                text,
                &model,
                &report.model_map,
                Some(&inner.workspace),
                authoritative,
                roots.len() == 1,
            )
            .with_known_uris(inner.docs.keys().chain(std::iter::once(root)));
            let mut rows = view.symbols(&preferences);
            if let Some(semantic) = typed_names::root_view(&mut inner, root) {
                model_map::add_binding_symbols(&mut rows, &semantic, uri, text, &preferences);
            }
            views.push(rows);
        }
        let mut rows = if roots.len() > 1 {
            common_leaves(views)
        } else if let Some(view) = views.pop() {
            view
        } else if roots.is_empty() {
            let model = inner.workspace.get_model(uri.as_str())?;
            let report = crate::engine::language::expand::expand_report(text);
            WrittenView::new(uri, text, model, &report.model_map, None, false, false)
                .symbols(&preferences)
        } else {
            Vec::new()
        };
        model_map::add_source_symbols(&mut rows, &inventory, text, &preferences, |item| {
            availability
                .get(item.source_id.as_str())
                .map(|state| {
                    if matches!(
                        item.kind,
                        SourceItemKind::Conditional | SourceItemKind::Branch | SourceItemKind::Loop
                    ) {
                        state.control_availability
                    } else {
                        state.availability
                    }
                })
                .unwrap_or_default()
        });
        Some(rows)
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

    fn decl_location(&self, pos: &TextDocumentPositionParams) -> Option<Location> {
        rename::definition(&mut self.lock_inner(), pos)
    }

    fn definition_at(&self, pos: &TextDocumentPositionParams) -> Option<GotoDefinitionResponse> {
        let mut inner = self.lock_inner();
        if let Some(target) = locals::definition(&mut inner, pos, false) {
            return target;
        }
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
        rename::definition(&mut inner, pos).map(GotoDefinitionResponse::Scalar)
    }

    fn ident_highlights(&self, pos: &TextDocumentPositionParams) -> Option<Vec<DocumentHighlight>> {
        let mut inner = self.lock_inner();
        if let Some(highlights) = locals::highlights(&mut inner, pos) {
            return highlights;
        }
        let text = inner.document(&pos.text_document.uri)?.text.clone();
        let file = crate::engine::workspace::include_resolver::normalize_uri(
            pos.text_document.uri.as_str(),
        );
        let index = inner
            .workspace
            .source_document(pos.text_document.uri.as_str(), &file)
            .map(|source| source.source_index())
            .unwrap_or_else(|| {
                Arc::new(LineIndex::new(
                    &crate::engine::language::parser::normalize_newlines(&text),
                ))
            });
        let references = rename::references(&mut inner, pos, true)?;
        let typed = typed_names::sites(&mut inner, &pos.text_document.uri);
        let views = inner.name_views(&pos.text_document.uri)?;
        let sites: Vec<_> = views
            .iter()
            .map(|(model, report)| {
                NameSites::new(model, &report.model_map, &pos.text_document.uri, &text)
            })
            .collect();
        let mut source_uris = HashMap::new();
        let hits = references
            .into_iter()
            .filter(|location| {
                if location.uri == pos.text_document.uri {
                    return true;
                }
                if let Some(own) = source_uris.get(&location.uri) {
                    return *own;
                }
                let own = crate::engine::workspace::include_resolver::normalize_uri(
                    location.uri.as_str(),
                ) == file;
                source_uris.insert(location.uri.clone(), own);
                own
            })
            .map(|location| {
                let span = Span {
                    start: index.offset_utf16(&text, span_pos(location.range.start)),
                    end: index.offset_utf16(&text, span_pos(location.range.end)),
                };
                DocumentHighlight {
                    range: location.range,
                    kind: Some(
                        if typed
                            .as_ref()
                            .and_then(|sites| sites.at(span))
                            .is_some_and(|(_, declaration)| declaration)
                            || sites.iter().all(|sites| sites.is_write(span))
                        {
                            DocumentHighlightKind::WRITE
                        } else {
                            DocumentHighlightKind::READ
                        },
                    ),
                }
            })
            .collect::<Vec<_>>();
        if hits.is_empty() { None } else { Some(hits) }
    }

    fn complete(
        &self,
        pos: &TextDocumentPositionParams,
        context: Option<&CompletionContext>,
    ) -> Option<CompletionResponse> {
        let mut inner = self.lock_inner();
        let text = inner.document(&pos.text_document.uri)?.text.clone();
        let normalized = crate::engine::language::parser::normalize_newlines(&text);
        let index = LineIndex::new(&normalized);
        let byte = index.offset_utf16(&text, span_pos(pos.position));
        if let Some(name) = macro_directive_name_at(&normalized, byte) {
            let edit_start = name.start.min(byte);
            return Some(CompletionResponse::Array(macro_directive_completions(
                span_range(
                    &index,
                    &normalized,
                    Span::new(edit_start as usize, name.end as usize),
                ),
                &normalized[edit_start as usize..name.start as usize],
            )));
        }
        if context.is_some_and(|context| {
            context.trigger_kind == CompletionTriggerKind::TRIGGER_CHARACTER
                && context.trigger_character.as_deref() == Some("#")
        }) {
            return Some(CompletionResponse::Array(Vec::new()));
        }
        let inventory = inner
            .workspace
            .source_inventory(pos.text_document.uri.as_str())?;
        let semantic_source =
            inner.semantic_completion_source(&pos.text_document.uri, byte, &inventory);
        if semantic_source
            && let Some(item) = metadata_completion_item(&mut inner, &pos.text_document.uri, byte)
        {
            return Some(CompletionResponse::Array(vec![item]));
        }
        let mut syntax_context = inner.syntax_context_with_source(
            &pos.text_document.uri,
            &text,
            byte,
            false,
            &inventory,
            semantic_source,
        );
        let Some(mut syntax_context) = syntax_context.take() else {
            return Some(CompletionResponse::Array(Vec::new()));
        };
        if syntax_context.language == "macro_expression" {
            let mut items = syntax_intel::completions(&syntax_context);
            items.extend(syntax_intel::macro_definitions(&text).into_iter().map(
                |(name, _, _, function)| CompletionItem {
                    label: name,
                    kind: Some(if function {
                        CompletionItemKind::FUNCTION
                    } else {
                        CompletionItemKind::VARIABLE
                    }),
                    detail: Some("written macro definition".into()),
                    ..CompletionItem::default()
                },
            ));
            return Some(CompletionResponse::Array(items));
        }
        // Option-value completion can change the display context below. Typed
        // names keep the original source context resolved for this request.
        let source_context = syntax_context.clone();
        if semantic_source
            && let Some(items) =
                typed_names::completions(&mut inner, &pos.text_document.uri, byte, &source_context)
        {
            return Some(CompletionResponse::Array(items));
        }
        let mut value_items = Vec::new();
        if let Some((cmd, option)) = crate::lsp::signature_help::option_context(&text, byte) {
            if let Some(option) = option {
                syntax_context.language = "expression";
                value_items = syntax_intel::option_values(&cmd, &option);
            } else {
                let items = syntax_intel::options(&cmd)
                    .map(|option| {
                        let name = option.name.as_str();
                        let documentation =
                            shocks_overwrite_doc(&text, byte, &cmd, name, &option.summary);
                        CompletionItem {
                            label: name.into(),
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
        let mut items = if semantic_source {
            let views = inner.name_views(&pos.text_document.uri)?;
            let mut groups = views.iter().map(|(model, _)| {
                default_completions(
                    model,
                    &preferences,
                    inner.completion_label_details,
                    inner.completion_snippets,
                    &syntax_context,
                )
            });
            let mut items = groups.next()?;
            for group in groups {
                items.retain(|item| group.contains(item));
            }
            items
        } else {
            syntax_intel::completions(&syntax_context)
        };
        items.extend(value_items);
        if semantic_source {
            items.extend(locals::completions(
                &mut inner,
                &pos.text_document.uri,
                byte,
                &preferences,
            ));
            items.extend(typed_names::expression_completions(
                &mut inner,
                &pos.text_document.uri,
                &source_context,
            ));
        }
        {
            let mut names: HashSet<_> = items.iter().map(|item| item.label.clone()).collect();
            for candidate in inventory.compatible_candidates(byte) {
                let Some(name) = &candidate.name else {
                    continue;
                };
                if names.insert(name.clone()) {
                    items.push(CompletionItem {
                        label: name.clone(),
                        kind: Some(CompletionItemKind::TEXT),
                        detail: Some("source candidate · unverified".into()),
                        ..CompletionItem::default()
                    });
                }
            }
        }
        Some(CompletionResponse::Array(items))
    }

    fn signature_at(&self, pos: &TextDocumentPositionParams) -> Option<SignatureHelp> {
        let mut inner = self.lock_inner();
        let source = inner.source_document(&pos.text_document.uri)?;
        let text = source.source();
        let index = source.source_index();
        let byte = index.offset_utf16(text, span_pos(pos.position));
        let inventory = source.inventory()?;
        let semantic_source =
            inner.semantic_completion_source(&pos.text_document.uri, byte, &inventory);
        let context = inner.syntax_context_with_source(
            &pos.text_document.uri,
            text,
            byte,
            true,
            &inventory,
            semantic_source,
        )?;
        crate::lsp::signature_help::combined_signature_help(
            text,
            byte,
            context.language,
            &inventory.macro_tokens,
        )
    }

    fn prepare_rename_at(&self, pos: &TextDocumentPositionParams) -> Result<Range> {
        let mut inner = self.lock_inner();
        rename::decide(&mut inner, pos, None)
            .map(|decision| decision.range)
            .map_err(tower_lsp::jsonrpc::Error::invalid_params)
    }

    fn rename_at(&self, pos: &TextDocumentPositionParams, new_name: &str) -> Result<WorkspaceEdit> {
        if !is_legal_ident(new_name) {
            return Err(tower_lsp::jsonrpc::Error::invalid_params(
                rename::IDENTIFIER,
            ));
        }
        let mut inner = self.lock_inner();
        rename::decide(&mut inner, pos, Some(new_name))
            .map(|decision| decision.edit)
            .map_err(tower_lsp::jsonrpc::Error::invalid_params)
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
            // Reject unrelated stored fixes before checking their root inputs.
            // Every returned fix still needs the original revision and bytes.
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
            if inner
                .workspace
                .root_analysis_configuration(row.root.as_str())
                .is_preview()
            {
                actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: fix_title(lib),
                    kind: Some(CodeActionKind::QUICKFIX),
                    disabled: Some(CodeActionDisabled {
                        reason: inner
                            .workspace
                            .root_analysis_configuration(row.root.as_str())
                            .preview_edit_cause("Diagnostic fixes"),
                    }),
                    diagnostics: Some(vec![row.lsp_diagnostic.clone()]),
                    ..CodeAction::default()
                }));
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
            if crate::engine::editing::shock_template::has_ordinary_shocks(model) {
                return Vec::new();
            }
            (
                crate::engine::editing::shock_template::available_kinds(model),
                crate::engine::editing::shock_template::aggregate_varexo_names(model),
                crate::engine::editing::shock_template::aggregate_deterministic_names(model),
            )
        };
        let index = LineIndex::new(&text);
        kinds
            .into_iter()
            .filter_map(|template_kind| {
                let names = match template_kind {
                    crate::engine::editing::shock_template::TemplateKind::Stochastic => {
                        &stochastic_names
                    }
                    crate::engine::editing::shock_template::TemplateKind::Deterministic => {
                        &deterministic_names
                    }
                };
                let edit =
                    crate::engine::editing::shock_template::insertion(&text, names, template_kind)?;
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
        let mut inner = self.lock_inner();
        let decision = rename::decide(&mut inner, pos, None).ok()?;
        let DocumentChanges::Edits(documents) = decision.edit.document_changes? else {
            return None;
        };
        // Linked editing can update one document only. The Rename provider owns
        // complete changes which also require another source file.
        if documents
            .iter()
            .any(|document| document.text_document.uri != pos.text_document.uri)
        {
            return None;
        }
        let ranges: Vec<_> = documents
            .into_iter()
            .flat_map(|document| document.edits)
            .filter_map(|edit| match edit {
                OneOf::Left(edit) => Some(edit.range),
                OneOf::Right(_) => None,
            })
            .collect();
        if ranges.len() < 2 {
            return None;
        }
        Some(LinkedEditingRanges {
            ranges,
            word_pattern: None,
        })
    }

    fn apply_settings(&self, settings: &Value) -> (Vec<String>, settings::SettingsChange) {
        let mut inner = self.lock_inner();
        let previous = inner.settings.clone();
        let explanations = inner.settings.apply(settings);
        let hints = inner.docs.keys().any(|uri| {
            inner
                .settings
                .resolve(uri)
                .presentation
                .parameter_value_hints
                != previous.resolve(uri).presentation.parameter_value_hints
        });
        let analysis = inner.workspace_settings_changed(&previous);
        if analysis {
            inner.document_analysis.cancel();
            self.refresh_written_sources(&inner);
        }
        (explanations, settings::SettingsChange { analysis, hints })
    }

    fn folding_ranges(&self, uri: &Url) -> Option<Vec<FoldingRange>> {
        let inner = self.lock_inner();
        let source = inner.source_document(uri)?;
        let text = source.source();
        let inventory = source.inventory()?;
        let index = source.source_index();
        let mut ranges: Vec<_> = inventory
            .folds
            .iter()
            .filter_map(|fold| {
                let start_line = index.position_utf16(text, fold.span.start).line;
                let end_line = index
                    .position_utf16(text, fold.span.end.saturating_sub(1))
                    .line;
                (start_line < end_line).then_some(FoldingRange {
                    start_line,
                    end_line,
                    start_character: None,
                    end_character: None,
                    kind: Some(if fold.comment {
                        FoldingRangeKind::Comment
                    } else {
                        FoldingRangeKind::Region
                    }),
                    collapsed_text: None,
                })
            })
            .collect();
        // VS Code exposes one fold per starting line and accepts only nested
        // or disjoint ranges. Prefer the enclosing construct at a shared line.
        ranges.sort_by_key(|range| (range.start_line, std::cmp::Reverse(range.end_line)));
        ranges.dedup_by_key(|range| range.start_line);
        let mut ends = Vec::new();
        ranges.retain(|range| {
            while ends.last().is_some_and(|end| *end < range.start_line) {
                ends.pop();
            }
            if ends.last().is_some_and(|end| *end < range.end_line) {
                return false;
            }
            ends.push(range.end_line);
            true
        });
        (!ranges.is_empty()).then_some(ranges)
    }

    fn selection_ranges(&self, uri: &Url, positions: &[Position]) -> Option<Vec<SelectionRange>> {
        let inner = self.lock_inner();
        let doc = inner.document(uri)?;
        let model = inner.workspace.get_model(uri.as_str())?;
        let index = inner.source_document(uri)?.source_index();
        let tokens = source_projection::selection_tokens(&doc.text);
        let file_range = full_document_range(&doc.text);
        let mut out = Vec::new();
        for &pos in positions {
            let mut chain = Vec::new();
            let byte = index.offset_utf16(&doc.text, span_pos(pos));
            if let Some(span) = crate::engine::editing::refs::ident_span_at(&tokens, byte) {
                chain.push(span_range(&index, &doc.text, span));
            }
            if let Some(eq) = equation_at_byte(model, byte) {
                let range = span_range(&index, &doc.text, eq.span);
                if range_has_pos(range, pos) {
                    chain.push(range);
                }
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
                if span.start <= byte && byte <= span.end {
                    let range = span_range(&index, &doc.text, span);
                    if range_has_pos(range, pos) {
                        chain.push(range);
                    }
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
        let source = inner.source_document(uri)?;
        let text = source.source();
        let includes = inner
            .workspace
            .get_model(uri.as_str())
            .map(|m| m.includes.clone())
            .unwrap_or_default();
        let index = source.source_index();
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
        let roots = inner.context_roots(uri);
        let source_root = (roots.len() == 1).then(|| roots[0].clone());
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
                range: span_range(&index, text, inc.span),
                target: Some(target),
                tooltip: Some(format!("Open {}", inc.filename)),
                data: source_root
                    .as_ref()
                    .map(|root| json!({"dygnosis_source_include":true,"root_uri":root})),
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
                range: span_range(&index, text, rec.named_in),
                target: Some(target),
                tooltip: Some(format!("{} {basename}", rec.kind.as_str())),
                data: None,
            });
        }
        for link in source_intelligence::literal_links(&mut inner, uri, text, &index) {
            if let Some(existing) = links
                .iter_mut()
                .find(|existing| existing.range == link.range)
            {
                *existing = link;
            } else {
                links.push(link);
            }
        }
        if links.is_empty() { None } else { Some(links) }
    }

    fn semantic_tokens(&self, uri: &Url, range: Option<Range>) -> Option<SemanticTokens> {
        let mut inner = self.lock_inner();
        let source = inner.source_document(uri)?;
        let text = source.source();
        let index = source.source_index();
        let byte_range = range.map(|range| Span {
            start: index.offset_utf16(text, span_pos(range.start)),
            end: index.offset_utf16(text, span_pos(range.end)),
        });
        let typed = typed_names::sites_in_range(&mut inner, uri, byte_range);
        let file = crate::engine::workspace::include_resolver::normalize_uri(uri.as_str());
        let tokens = source_projection::color_tokens(
            text,
            typed
                .as_ref()
                .and_then(|sites| sites.raw_source_tokens(&file)),
        );
        let views = inner.name_views(uri)?;
        let mapping = inner.semantic_mapping.clone();
        let sites: Vec<_> = views
            .iter()
            .map(|(model, report)| {
                (
                    NameSites::for_range(model, &report.model_map, uri, text, byte_range),
                    source_projection::color_timing(model, &mapping),
                )
            })
            .collect();
        let mut raw = Vec::new();
        let first = range.map_or(0, |range| {
            let byte = index.offset_utf16(text, span_pos(range.start));
            tokens.partition_point(|token| token.span.start < byte)
        });
        for tok in &tokens[first..] {
            if tok.kind != TokenKind::Ident {
                continue;
            }
            let name = tok.text(text);
            let start = index.position_utf16(text, tok.span.start);
            if range.is_some_and(|range| Position::new(start.line, start.character) >= range.end) {
                break;
            }
            if let Some(range) = range
                && !pos_in_range_half_open(Position::new(start.line, start.character), range)
            {
                continue;
            }
            let typed = typed.as_ref().and_then(|sites| sites.at(tok.span));
            let role = typed.map(|(role, _)| role).or_else(|| {
                sites
                    .first()?
                    .0
                    .role(name, tok.span)
                    .filter(|role| *role == NameRole::ModelLocal)
            });
            if typed.is_none()
                && !sites
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
                    if typed.map_or_else(
                        || sites.is_declaration(tok.span),
                        |(_, declaration)| declaration,
                    ) {
                        mods |= mapping.declaration;
                    }
                    if role == NameRole::Endogenous
                        && let Some(info) = timing.get(name)
                    {
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
                    mods
                })
                .reduce(|common, modifiers| common & modifiers)
                .unwrap_or(0);
            let end = index.position_utf16(text, tok.span.end);
            let length = if start.line == end.line {
                end.character.saturating_sub(start.character)
            } else {
                tok.text(text).encode_utf16().count() as u32
            };
            raw.push((start.line, start.character, length, ttype, mods));
        }
        if let Some(typed) = &typed {
            let mut positions: HashSet<_> = raw.iter().map(|token| (token.0, token.1)).collect();
            for (span, role, declaration) in typed.iter() {
                let start = index.position_utf16(text, span.start);
                if range.is_some_and(|range| {
                    !pos_in_range_half_open(Position::new(start.line, start.character), range)
                }) {
                    continue;
                }
                let Some(token_type) = mapping.token_type(role) else {
                    continue;
                };
                let end = index.position_utf16(text, span.end);
                if start.line == end.line && positions.insert((start.line, start.character)) {
                    raw.push((
                        start.line,
                        start.character,
                        end.character - start.character,
                        token_type,
                        if declaration { mapping.declaration } else { 0 },
                    ));
                }
            }
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

    fn format_document(&self, uri: &Url) -> Result<Option<Vec<TextEdit>>> {
        let inner = self.lock_inner();
        let doc = inner.document(uri).ok_or_else(|| {
            tower_lsp::jsonrpc::Error::invalid_params(
                "The formatting document is unavailable. Open it and try again.",
            )
        })?;
        let settings = inner.settings.resolve(uri);
        match format_outcome_with_limit(
            &doc.text,
            &settings.format_indent_unit,
            settings.macro_work_limit,
        ) {
            FormatOutcome::Changed(formatted) => {
                Ok(Some(vec![full_document_edit(&doc.text, formatted)]))
            }
            FormatOutcome::Unchanged => Ok(None),
            FormatOutcome::Unsupported(reason) => {
                Err(tower_lsp::jsonrpc::Error::invalid_params(reason))
            }
        }
    }

    fn format_line_range(&self, uri: &Url, range: Range) -> Result<Option<Vec<TextEdit>>> {
        let inner = self.lock_inner();
        let doc = inner.document(uri).ok_or_else(|| {
            tower_lsp::jsonrpc::Error::invalid_params(
                "The formatting document is unavailable. Open it and try again.",
            )
        })?;
        let mut end_line = range.end.line;
        if range.end.character == 0 && end_line > range.start.line {
            end_line -= 1;
        }
        let settings = inner.settings.resolve(uri);
        let (start_line, end_line, replacement) = match format_range_outcome_with_limit(
            &doc.text,
            range.start.line,
            end_line,
            &settings.format_indent_unit,
            settings.macro_work_limit,
        ) {
            RangeFormatOutcome::Changed(start, end, replacement) => (start, end, replacement),
            RangeFormatOutcome::Unchanged => return Ok(None),
            RangeFormatOutcome::Unsupported(reason) => {
                return Err(tower_lsp::jsonrpc::Error::invalid_params(reason));
            }
        };
        Ok(Some(vec![line_range_edit(
            &doc.text,
            start_line,
            end_line,
            replacement,
        )]))
    }

    fn execute(&self, command: &str, arguments: &[Value]) -> Value {
        if let Some(root) = arguments
            .first()
            .and_then(|value| value.get("root_uri"))
            .and_then(Value::as_str)
            .and_then(|uri| Url::parse(uri).ok())
            .filter(is_model_root)
        {
            self.lock_inner().workspace_checks.active = Some(root);
            self.workspace_wake.notify_one();
        }
        let result = match command {
            "dynare/explainDiagnostic" => explain_command(arguments),
            "dynare/compareModelSnapshots" => self.compare_snapshot_command(arguments),
            "dynare/validateComparisonInputs" => {
                comparison_inputs::validate(&self.lock_inner(), arguments)
            }
            "dynare/showEffectiveModel" => self.show_effective_model_command(arguments),
            "dynare/captureRename" => rename::capture_command(&mut self.lock_inner(), arguments)
                .unwrap_or_else(rename::refusal_json),
            "dynare/modelInfo" => self.model_info_command(arguments),
            "dynare/analysisContext" => self.analysis_context_command(arguments),
            "dynare/sourceCandidates" => {
                source_intelligence::candidates(&mut self.lock_inner(), arguments)
            }
            _ => json!({"error": format!("unknown command {command}"), "code": "UNKNOWN_COMMAND"}),
        };
        self.lock_inner().bound_workspace_cache();
        result
    }

    fn model_info_command(&self, arguments: &[Value]) -> Value {
        let Some(argument) = arguments.first().and_then(Value::as_object) else {
            return json!({"error":"dynare/modelInfo requires a root_uri argument", "code":"INVALID_ARGUMENTS", "inactive_ranges":[]});
        };
        let Some(root) = argument
            .get("root_uri")
            .and_then(Value::as_str)
            .and_then(|uri| Url::parse(uri).ok())
        else {
            return json!({"error":"dynare/modelInfo requires a valid root_uri", "code":"INVALID_ARGUMENTS", "inactive_ranges":[]});
        };
        let document = match argument.get("document_uri") {
            None => root.clone(),
            Some(value) => match value.as_str().and_then(|uri| Url::parse(uri).ok()) {
                Some(uri) => uri,
                None => {
                    return json!({"error":"dynare/modelInfo requires a valid document_uri", "code":"INVALID_ARGUMENTS", "inactive_ranges":[]});
                }
            },
        };
        let mut inner = self.lock_inner();
        if !is_model_root(&root) {
            return json!({"error":"Choose a .mod or .dyn owner root for model information", "code":"ROOT_REQUIRED", "owner_roots":inner.known_owner_roots(&root), "inactive_ranges":[]});
        }
        if !["file", "untitled"].contains(&root.scheme()) {
            return json!({"error":"Model information requires a file or untitled root URI", "code":"UNSUPPORTED_URI", "inactive_ranges":[]});
        }
        let revision = inner.root_revision(&root);
        let analysis = analysis_configuration::metadata(&mut inner, &root);
        let document_key =
            crate::engine::workspace::include_resolver::normalize_uri(document.as_str());
        if document_key != crate::engine::workspace::include_resolver::normalize_uri(root.as_str())
            && inner.document_contexts.get(&document_key) != Some(&root)
            && !inner.source_browsing_relation(&root, &document)
        {
            return json!({"error":"The displayed document does not belong to the chosen model root", "code":"DOCUMENT_NOT_OWNED", "inactive_ranges":[]});
        }
        if analysis["state"] == "stale" {
            let Some(revision) = analysis_configuration::context_revision(&mut inner, &root) else {
                return json!({"error":"The model root is unavailable", "code":"ROOT_NOT_FOUND", "inactive_ranges":[]});
            };
            let cause = &analysis["cause"];
            return json!({"schema_version":MODEL_INFO_SCHEMA_VERSION,"root_uri":root,"document_uri":document,
                "document_version":inner.document(&document).map(|doc|doc.version),"revision":revision,"complete":false,
                "analysis":analysis,"status":"stale","message":cause["message"],"incomplete_reasons":[cause],
                "owner_roots":inner.known_owner_roots(&document),"browsing_documents":inner.browsing_documents(&root),
                "statements":[],"declarations":[],"equations":[],"related_files":[],"block_categories":[],"first_model_anchor":null,
                "inactive_ranges":[]});
        }
        let Some(revision) = revision else {
            return json!({"error":"The model root is unavailable", "code":"ROOT_NOT_FOUND", "inactive_ranges":[]});
        };
        let Some(model) = inner.workspace.effective_model_snapshot(root.as_str()) else {
            return json!({"error":"The model root is unavailable", "code":"ROOT_NOT_FOUND", "inactive_ranges":[]});
        };
        let Some(report) = inner.workspace.expand_report_snapshot(root.as_str()) else {
            return json!({"error":"The model root is unavailable", "code":"ROOT_NOT_FOUND", "inactive_ranges":[]});
        };
        let complete = report.complete
            && report.model_map.complete
            && inner.workspace.includes_complete(root.as_str());
        let includes = inner
            .workspace
            .include_records(root.as_str())
            .cloned()
            .unwrap_or_default();
        let semantic = complete
            .then(|| inner.semantic_views.get(&root))
            .flatten()
            .filter(|view| view.complete && view.revision == revision)
            .cloned();
        let local_owned = (complete && semantic.is_none())
            .then(|| crate::engine::analysis::model_info::accepted_model_locals(&model));
        let local_facts = semantic
            .as_ref()
            .map(|view| view.local_facts())
            .or(local_owned.as_ref());
        let timing = complete.then(|| crate::engine::analysis::timing::TimingAnalysis::new(&model));
        let mut result = if let Some(local_facts) = local_facts {
            crate::engine::analysis::model_info::model_info_json_with_locals_and_timing(
                &model,
                Some(local_facts),
                timing.as_ref().unwrap(),
            )
        } else {
            let reasons = self.model_info_reasons(&mut inner, &root);
            crate::engine::analysis::model_info::model_incomplete_status_with_reasons(
                reasons.as_array().unwrap(),
            )
        };
        let inactive_ranges = source_activity::requested_document_ranges(&inner, &root, &document);
        let Some(text) = inner.workspace.get_source(document.as_str()) else {
            return json!({"error":"The displayed source is unavailable", "code":"DOCUMENT_NOT_FOUND", "inactive_ranges":[]});
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
        let facts = view.facts_json(timing.as_ref());
        if let Some(local_facts) = local_facts {
            locals::info_origins(&view, local_facts, &report.model_map, &mut result);
        }
        if let Value::Object(facts) = facts {
            result.as_object_mut().unwrap().extend(facts);
        }
        result["schema_version"] = json!(MODEL_INFO_SCHEMA_VERSION);
        result["root_uri"] = json!(root);
        result["document_uri"] = json!(document);
        result["inactive_ranges"] = json!(inactive_ranges);
        result["document_version"] = json!(inner.document(&document).map(|doc| doc.version));
        result["revision"] = json!(revision);
        result["complete"] = json!(complete);
        result["analysis"] = analysis;
        result["browsing_documents"] = json!(inner.browsing_documents(&root));
        result["source_browsing"] = json!(
            document != root
                && !inner.workspace.owner_roots(document.as_str()).contains(
                    &crate::engine::workspace::include_resolver::normalize_uri(root.as_str())
                )
        );
        result["owner_roots"] = json!(inner.known_owner_roots(&document));
        result["dependency_candidates"] = json!(
            inner
                .workspace
                .input_candidate_paths(root.as_str())
                .iter()
                .filter_map(|path| Url::from_file_path(path).ok())
                .collect::<Vec<_>>()
        );
        result["block_categories"] = json!(
            crate::engine::workspace::model_map::BLOCK_CATEGORIES
                .iter()
                .map(|(category, default)| json!({"category":category,"default":default}))
                .collect::<Vec<_>>()
        );
        let companions = inner
            .workspace
            .companion_records(root.as_str())
            .map(<[_]>::to_vec)
            .unwrap_or_default();
        result["related_files"] = crate::engine::analysis::model_info::related_files_json(
            &includes,
            &companions,
            |path| path.to_string_lossy().into_owned(),
        );
        result
    }

    fn capture_snapshot_input(
        &self,
        input: &crate::engine::comparison::compare_snapshots::SnapshotInput,
        previous: Option<&crate::engine::comparison::compare_snapshots::CapturedSnapshot>,
    ) -> crate::engine::comparison::compare_snapshots::SnapshotCapture {
        use crate::engine::comparison::compare_snapshots::{
            SnapshotCapture, SnapshotFailure, SnapshotInput, capture_git_snapshot_with_limit,
            capture_working_snapshot_configured,
        };
        match input {
            SnapshotInput::Git(input) => {
                let macro_work_limit =
                    Url::parse(&format!("{}/", input.repository_uri.trim_end_matches('/')))
                        .ok()
                        .and_then(|repository| repository.join(&input.root_file).ok())
                        .map(|root| self.lock_inner().settings.resolve(&root).macro_work_limit)
                        .unwrap_or_default();
                capture_git_snapshot_with_limit(input, None, macro_work_limit)
            }
            SnapshotInput::Working {
                input_id,
                root_uri,
                expected_revision,
                search_paths,
                analysis,
            } => {
                let Some(uri) = Url::parse(root_uri).ok().filter(|uri| {
                    is_model_root(uri) && matches!(uri.scheme(), "file" | "untitled")
                }) else {
                    return SnapshotCapture::Failure(SnapshotFailure::new(
                        "INVALID_ARGUMENTS",
                        "Working snapshot needs a file or untitled model root",
                        Some(root_uri.clone()),
                    ));
                };
                if input_id.is_empty() || expected_revision.is_empty() {
                    return SnapshotCapture::Failure(SnapshotFailure::new(
                        "INVALID_ARGUMENTS",
                        "Working snapshot needs an input ID and expected revision",
                        None,
                    ));
                }
                let mut workspace = {
                    let mut inner = self.lock_inner();
                    let Some(current) = inner
                        .root_revision(&uri)
                        .or_else(|| analysis_configuration::context_revision(&mut inner, &uri))
                    else {
                        return SnapshotCapture::Failure(SnapshotFailure::new(
                            "ROOT_NOT_FOUND",
                            "Working model root is unavailable",
                            Some(root_uri.clone()),
                        ));
                    };
                    if &current != expected_revision {
                        return SnapshotCapture::Failure(SnapshotFailure::new(
                            "INPUT_CHANGED",
                            "Working model changed before capture; refresh the comparison",
                            Some(root_uri.clone()),
                        ));
                    }
                    match search_paths {
                        Some(paths) => inner.workspace.snapshot_for_root(
                            root_uri,
                            paths.iter().map(std::path::PathBuf::from).collect(),
                        ),
                        None => inner.workspace.snapshot_with_root_settings(root_uri),
                    }
                };
                if let Some(previous) = previous {
                    previous.reuse_sources_for(&mut workspace);
                }
                let mut capture = capture_working_snapshot_configured(
                    workspace,
                    root_uri,
                    input_id,
                    "editor_buffers_and_disk",
                    analysis.clone(),
                );
                if let SnapshotCapture::Ready(input) = &mut capture {
                    input.inputs["expected_revision"] = json!(expected_revision);
                }
                capture
            }
        }
    }

    fn compare_snapshot_command(&self, arguments: &[Value]) -> Value {
        use crate::engine::comparison::compare_snapshots::{
            SNAPSHOT_SCHEMA_VERSION, SnapshotCapture, SnapshotCompareRequest, SnapshotCoordinates,
            SnapshotFailure, SnapshotInput, compare_captured_snapshots_ref, validate_working_pair,
        };
        fn summarized(
            mut result: Value,
            old: &SnapshotCapture,
            new: &SnapshotCapture,
            request: &SnapshotCompareRequest,
        ) -> Value {
            let source = crate::engine::analysis_configuration::AnalysisConfiguration::default();
            let requested = |input: &SnapshotInput| {
                input
                    .analysis()
                    .cloned()
                    .or_else(|| matches!(input, SnapshotInput::Git(_)).then(|| source.clone()))
            };
            let before = requested(&request.before);
            let after = requested(&request.after);
            let mut summaries = json!({"before":old.analysis_summary(before.as_ref()),"after":new.analysis_summary(after.as_ref())});
            if matches!(result["code"].as_str(), Some("INPUT_CHANGED" | "CANCELLED")) {
                for side in ["before", "after"] {
                    summaries[side]["complete"] = Value::Null;
                    summaries[side]["counts"] = Value::Null;
                    summaries[side]["counts_cause"] = result["message"].clone();
                }
            }
            result["analysis_summaries"] = summaries;
            if let Some(result) = result.as_object_mut() {
                result.remove("analysis_summary");
            }
            result
        }
        let request = arguments
            .first()
            .cloned()
            .and_then(|value| serde_json::from_value::<SnapshotCompareRequest>(value).ok());
        let Some(request) =
            request.filter(|request| request.schema_version == SNAPSHOT_SCHEMA_VERSION)
        else {
            return SnapshotFailure::new(
                "INVALID_ARGUMENTS",
                "compareModelSnapshots requires snapshot schema 1 with Before and After inputs",
                None,
            )
            .response("before");
        };
        let mut old = self.capture_snapshot_input(&request.before, None);
        let mut new = self.capture_snapshot_input(
            &request.after,
            match &old {
                SnapshotCapture::Ready(input) => Some(input),
                _ => None,
            },
        );
        let mut needs = Vec::new();
        for (side, input, captured) in [
            ("before", &request.before, &old),
            ("after", &request.after, &new),
        ] {
            match captured {
                SnapshotCapture::Failure(failure) => {
                    return summarized(failure.response(side), &old, &new, &request);
                }
                SnapshotCapture::NeedsSources(keys) => {
                    let SnapshotInput::Git(input) = input else {
                        unreachable!("only historical sources are deferred")
                    };
                    needs.push(json!({"side":side,"input_id":input.input_id,"file_keys":keys}));
                }
                _ => {}
            }
        }
        // A Working side remains pinned while the host acquires historical
        // sources. Check it after every round, including needs_sources rounds.
        for (side, input) in [("before", &request.before), ("after", &request.after)] {
            if let SnapshotInput::Working {
                root_uri,
                expected_revision,
                ..
            } = input
            {
                let uri = Url::parse(root_uri).expect("validated Working root");
                let mut inner = self.lock_inner();
                if inner
                    .root_revision(&uri)
                    .or_else(|| analysis_configuration::context_revision(&mut inner, &uri))
                    .as_ref()
                    != Some(expected_revision)
                {
                    return summarized(
                        SnapshotFailure::new(
                            "INPUT_CHANGED",
                            "Working model changed during capture; refresh the comparison",
                            Some(root_uri.clone()),
                        )
                        .response(side),
                        &old,
                        &new,
                        &request,
                    );
                }
            }
        }
        if !needs.is_empty() {
            return json!({"state":"needs_sources","requests":needs});
        }
        let (SnapshotCapture::Ready(old_input), SnapshotCapture::Ready(new_input)) =
            (&mut old, &mut new)
        else {
            unreachable!("finished captures")
        };
        let result = compare_captured_snapshots_ref(old_input, new_input, SnapshotCoordinates::Lsp);
        let mut inner = self.lock_inner();
        if let Err(failure) = validate_working_pair(old_input, new_input, Some(&inner.workspace)) {
            return summarized(failure.response("after"), &old, &new, &request);
        }
        for (side, input) in [("before", &request.before), ("after", &request.after)] {
            if let SnapshotInput::Working {
                root_uri,
                expected_revision,
                ..
            } = input
            {
                let uri = Url::parse(root_uri).expect("validated Working root");
                if inner
                    .root_revision(&uri)
                    .or_else(|| analysis_configuration::context_revision(&mut inner, &uri))
                    .as_ref()
                    != Some(expected_revision)
                {
                    return summarized(
                        SnapshotFailure::new(
                            "INPUT_CHANGED",
                            "Working model changed during comparison; refresh the comparison",
                            Some(root_uri.clone()),
                        )
                        .response(side),
                        &old,
                        &new,
                        &request,
                    );
                }
            }
        }
        if result["state"] == "failure" {
            summarized(result, &old, &new, &request)
        } else {
            result
        }
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
            let analysis = analysis_configuration::metadata(&mut inner, &uri);
            return json!({"success": false, "code":analysis["cause"]["code"].as_str().unwrap_or("ROOT_NOT_FOUND"),
                "message":analysis["cause"]["message"].as_str().unwrap_or("The model root is unavailable. Open the root before expanding it."), "analysis":analysis});
        };
        let analysis = analysis_configuration::metadata(&mut inner, &uri);
        let Some(report) = inner.workspace.expand_report_snapshot(uri.as_str()) else {
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
        let source_evaluations = matches!(requested, Some("source"))
            .then(|| inner.workspace.source_evaluations(uri.as_str()))
            .unwrap_or_default();
        let work_limit = inner.workspace.root_macro_work_limit(uri.as_str());
        let preferences = inner.presentation_for(&uri);
        let includes_complete = inner.workspace.includes_complete(uri.as_str());
        let model = matches!(requested, Some("readable") | Some("source"))
            .then(|| inner.workspace.get_effective_model(uri.as_str()))
            .flatten();
        enum DisplayLayout {
            Readable(crate::engine::preview::preview_layout::PreviewLayout),
            Source(crate::engine::preview::preview_source::SourcePreview),
        }
        let mut source_reasons = Vec::new();
        let layout = model.and_then(|model| match requested {
            Some("readable") => crate::engine::preview::preview_layout::readable(&report, model)
                .map(DisplayLayout::Readable),
            Some("source") => crate::engine::preview::preview_source::source_with_evaluations(
                &report,
                model,
                &source_gaps,
                &source_lines,
                &source_evaluations,
                work_limit,
            )
            .map(|preview| preview.map(DisplayLayout::Source))
            .unwrap_or_else(|reasons| {
                source_reasons = reasons;
                None
            }),
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
            && includes_complete
            && !source_unproven
            && !(requested == Some("source") && layout.is_none());
        let document_symbols = model
            .map(|model| {
                let token_spans = match &layout {
                    Some(DisplayLayout::Readable(preview)) => Some(preview.token_spans.as_slice()),
                    Some(DisplayLayout::Source(preview)) if preview.proven => {
                        Some(preview.token_spans.as_slice())
                    }
                    _ => None,
                };
                token_spans
                    .map(|spans| {
                        model_map::preview_symbols(
                            &uri,
                            effective_text,
                            model,
                            &report.model_map,
                            spans,
                            &preferences,
                            complete && report.complete && report.model_map.complete,
                        )
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let has_heterogeneous = !report.heterogeneous_origins.is_empty();
        let origins: Vec<Value> = report
            .origins
            .iter()
            .map(|origin| equation_origin_json(&inner.workspace, origin, has_heterogeneous))
            .collect();
        let mut result = json!({
            "uri": uri.as_str(),
            "root_uri": uri,
            "analysis": analysis,
            "revision": revision,
            "document_version": inner.document(&uri).map(|doc| doc.version),
            "complete": complete,
            "navigation_schema_version": crate::engine::preview::preview_navigation::NAVIGATION_SCHEMA_VERSION,
            "dependency_candidates": inner.workspace.input_candidate_paths(uri.as_str())
                .iter().filter_map(|path| Url::from_file_path(path).ok()).collect::<Vec<_>>(),
            "effective_text": effective_text,
            "origins": origins,
            "macro_messages": lsp_macro_messages(&inner.workspace, uri.as_str(), &report),
        });
        if matches!(requested, Some("source") | Some("readable")) {
            result["document_symbols"] = json!(document_symbols);
        }
        result["navigation"] = if complete {
            let index = LineIndex::new(effective_text);
            let mut rows = 0;
            crate::engine::preview::preview_navigation::navigation_json(
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
                json!(crate::engine::preview::source_navigation::SOURCE_NAVIGATION_SCHEMA_VERSION);
            result["source_navigation"] = if complete {
                if let Some(DisplayLayout::Source(preview)) = &layout {
                    let index = LineIndex::new(effective_text);
                    let file_cuts = inner.workspace.source_file_cuts(uri.as_str());
                    crate::engine::preview::source_navigation::source_navigation_json(
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
                    crate::engine::preview::source_navigation::macro_ranges_json(
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
            let mut info = self.model_info_reasons(&mut inner, &uri);
            if !source_reasons.is_empty() {
                let mut causes = Vec::new();
                let mut limits = Vec::new();
                for reason in &source_reasons {
                    let origin = inner
                        .workspace
                        .mapped_incomplete_origin(uri.as_str(), reason.span);
                    let mut limit = crate::engine::analysis::model_info::macro_limit_record(
                        reason,
                        Some(uri.as_str()),
                    );
                    if let Some(limit) = &mut limit {
                        limit["affected"] = json!("source-layout preview");
                        limit["omitted"] = json!("source-layout text and navigation");
                        limits.push(limit.clone());
                    }
                    let cause = crate::engine::analysis::model_info::IncompleteCause {
                        code: reason.code,
                        message: source_preview_stop_message(reason),
                        origin,
                        include_chain: Vec::new(),
                        limit,
                    };
                    let mut value = incomplete_lsp_reason(&mut inner, &uri, &cause);
                    value["available"] = json!(if report.complete {
                        "complete compact macro expansion"
                    } else {
                        "verified emitted prefix"
                    });
                    value["omitted"] = json!("source-layout text and navigation");
                    causes.push(value);
                }
                if report.complete {
                    info = json!([]);
                }
                info.as_array_mut()
                    .expect("incomplete causes")
                    .extend(causes);
                result["limits"] = json!(limits);
            }
            result["incomplete_reasons"] = info;
            result["message"] = json!(if let Some(reason) = source_reasons.first() {
                format!(
                    "{} Source-layout text and navigation were withheld.",
                    source_preview_stop_message(reason)
                )
            } else if source_unproven || requested == Some("source") && layout.is_none() {
                "Expansion is partial because the requested source layout could not be verified; source navigation is unavailable.".to_owned()
            } else {
                "Expansion is partial; unresolved includes, parsing, or macro expansion prevent complete counts and source navigation.".to_owned()
            });
        }
        if !inner.workspace.input_snapshot_is_current(uri.as_str()) {
            return json!({"success":false,"code":"INPUT_CHANGED","message":"The model inputs changed; refresh the effective model"});
        }
        result
    }

    fn model_info_reasons(&self, inner: &mut Inner, root: &Url) -> Value {
        let model = inner.workspace.effective_model_snapshot(root.as_str());
        let mut reasons = Vec::new();
        if let Some(model) = model {
            for cause in crate::engine::analysis::model_info::incomplete_causes(
                &mut inner.workspace,
                root.as_str(),
                &model,
            ) {
                reasons.push(incomplete_lsp_reason(inner, root, &cause));
            }
        }
        if reasons.is_empty() {
            reasons.push(json!({"message":"The complete expanded source or its written locations could not be verified."}));
        }
        json!(reasons)
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
        if let Some((word, _)) = ident_at(&doc.text, byte)
            && find_decl(model, &word).is_some()
        {
            items.push(ch_variable_item(
                model,
                &index,
                &doc.text,
                &pos.text_document.uri,
                &word,
            ));
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
        if items.is_empty() { None } else { Some(items) }
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
        self.wait_document_analysis(Some(&params.text_document.uri))
            .await;
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
            inner.document_context_enabled = params
                .capabilities
                .experimental
                .as_ref()
                .and_then(|value| value.get("dygnosis"))
                .and_then(|value| value.get("documentContext"))
                .and_then(|value| value.get("schema_version"))
                .and_then(Value::as_u64)
                == Some(1);
            inner.analysis_configuration.enabled = params
                .capabilities
                .experimental
                .as_ref()
                .and_then(|value| value.get("dygnosis"))
                .and_then(|value| value.get("analysisConfiguration"))
                .and_then(|value| value.get("schema_version"))
                .and_then(Value::as_u64)
                == Some(1);
            inner.workspace_checks.notifications = params
                .capabilities
                .experimental
                .as_ref()
                .and_then(|value| value.get("dygnosis"))
                .and_then(|value| value.get("workspaceStatusChanged"))
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
            self.apply_settings(&opts).0
        } else {
            self.lock_inner().workspace_reconfigure(true);
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
        self.lock_inner().workspace_checks.initialized = true;
        ordering::committed();
        self.client
            .log_message(MessageType::INFO, "dygnosis initialized")
            .await;
        self.kick_workspace();
    }

    async fn shutdown(&self) -> Result<()> {
        ordering::ready().await;
        let _output = self.output_gate.lock().await;
        self.presentation_refresh.stop();
        let mut inner = self.lock_inner();
        inner.document_analysis.cancel();
        inner.workspace_checks.shutdown = true;
        inner.workspace_checks.cancel_workers();
        inner.workspace_checks.epoch += 1;
        ordering::committed();
        self.workspace_wake.notify_one();
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        ordering::ready().await;
        let output = self.output_gate.lock().await;
        let doc = params.text_document;
        let uri = doc.uri.clone();
        self.store_document(doc.uri, doc.text, doc.version);
        self.lock_inner().queue_document_analysis(&uri);
        ordering::committed();
        drop(output);
        self.kick_document_analysis();
        self.kick_workspace();
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
        let uri = params.text_document.uri.clone();
        self.store_document(
            params.text_document.uri,
            change.text.clone(),
            params.text_document.version,
        );
        self.lock_inner().queue_document_analysis(&uri);
        ordering::committed();
        drop(output);
        self.kick_document_analysis();
        self.kick_workspace();
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
        self.store_document(uri.clone(), text, version);
        self.lock_inner().queue_document_analysis(&uri);
        ordering::committed();
        drop(output);
        self.kick_document_analysis();
        self.kick_workspace();
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        ordering::ready().await;
        let output = self.output_gate.lock().await;
        let uri = params.text_document.uri;
        let to_publish = {
            let mut inner = self.lock_inner();
            inner.document_analysis.cancel();
            let key = crate::engine::workspace::include_resolver::normalize_uri(uri.as_str());
            let mut sources = self.written_sources.lock().unwrap();
            inner.docs.retain(|candidate, _| {
                let keep =
                    crate::engine::workspace::include_resolver::normalize_uri(candidate.as_str())
                        != key;
                if !keep {
                    sources.remove(candidate);
                }
                keep
            });
            drop(sources);
            inner.parameter_hints.retain(|candidate, _| {
                crate::engine::workspace::include_resolver::normalize_uri(candidate.as_str()) != key
            });
            inner.document_contexts.remove(&key);
            inner.source_inspection_notices.retain(|candidate, _| {
                crate::engine::workspace::include_resolver::normalize_uri(candidate.as_str()) != key
            });
            inner.workspace.remove_document(uri.as_str());
            inner.workspace_changed(&uri, false);
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
        self.kick_workspace();
    }

    async fn diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> Result<DocumentDiagnosticReportResult> {
        ordering::ready().await;
        self.wait_document_analysis(Some(&params.text_document.uri))
            .await;
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
        self.wait_document_analysis(None).await;
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
            inner.document_analysis.cancel();
            for event in &params.changes {
                inner.workspace_changed(&event.uri, event.typ == FileChangeType::DELETED);
                inner.workspace.remove_document(event.uri.as_str());
            }
            let snapshots: Vec<(Url, String, i32)> = inner
                .docs
                .iter()
                .map(|(uri, doc)| (uri.clone(), doc.text.clone(), doc.version))
                .collect();
            for (uri, text, version) in snapshots {
                inner.prepare_root(&uri);
                inner.workspace.update_document(uri.as_str(), &text);
                let key = crate::engine::workspace::include_resolver::normalize_uri(uri.as_str());
                if let Some(source) = inner.workspace.source_document(uri.as_str(), &key) {
                    self.written_sources
                        .lock()
                        .unwrap()
                        .insert(uri.clone(), source);
                }
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
        self.kick_workspace();
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        ordering::ready().await;
        if let Some(hover) = self.written_hover_at(&params.text_document_position_params) {
            return Ok(hover);
        }
        self.wait_document_analysis(Some(
            &params.text_document_position_params.text_document.uri,
        ))
        .await;
        Ok(self.model_hover_at(&params.text_document_position_params))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        ordering::ready().await;
        self.wait_document_analysis(Some(&params.text_document.uri))
            .await;
        let result = self
            .doc_symbols(&params.text_document.uri)
            .map(DocumentSymbolResponse::Nested);
        self.report_source_inspection(&params.text_document.uri)
            .await;
        Ok(result)
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        ordering::ready().await;
        self.wait_document_analysis(None).await;
        let result = self.workspace_symbols(&params.query);
        let uris: Vec<_> = self.lock_inner().docs.keys().cloned().collect();
        for uri in uris {
            self.report_source_inspection(&uri).await;
        }
        Ok(Some(result))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        ordering::ready().await;
        self.wait_document_analysis(Some(
            &params.text_document_position_params.text_document.uri,
        ))
        .await;
        Ok(self.definition_at(&params.text_document_position_params))
    }

    async fn goto_declaration(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        ordering::ready().await;
        self.wait_document_analysis(Some(
            &params.text_document_position_params.text_document.uri,
        ))
        .await;
        {
            let mut inner = self.lock_inner();
            if let Some(target) =
                locals::definition(&mut inner, &params.text_document_position_params, true)
            {
                return Ok(target);
            }
        }
        Ok(self
            .decl_location(&params.text_document_position_params)
            .map(GotoDefinitionResponse::Scalar))
    }

    async fn goto_type_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        ordering::ready().await;
        self.wait_document_analysis(Some(
            &params.text_document_position_params.text_document.uri,
        ))
        .await;
        Ok(self
            .decl_location(&params.text_document_position_params)
            .map(GotoDefinitionResponse::Scalar))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        ordering::ready().await;
        self.wait_document_analysis(Some(&params.text_document_position.text_document.uri))
            .await;
        {
            let mut inner = self.lock_inner();
            if let Some(locations) = locals::references(
                &mut inner,
                &params.text_document_position,
                params.context.include_declaration,
            ) {
                return Ok(locations);
            }
        }
        Ok(rename::references(
            &mut self.lock_inner(),
            &params.text_document_position,
            params.context.include_declaration,
        ))
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        ordering::ready().await;
        if !self.written_binding_may_exist(&params.text_document_position_params) {
            return Ok(None);
        }
        self.wait_document_analysis(Some(
            &params.text_document_position_params.text_document.uri,
        ))
        .await;
        Ok(self.ident_highlights(&params.text_document_position_params))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        ordering::ready().await;
        self.wait_document_analysis(Some(&params.text_document_position.text_document.uri))
            .await;
        let result = self.complete(&params.text_document_position, params.context.as_ref());
        self.report_source_inspection(&params.text_document_position.text_document.uri)
            .await;
        Ok(result)
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        ordering::ready().await;
        self.wait_document_analysis(Some(
            &params.text_document_position_params.text_document.uri,
        ))
        .await;
        let result = self.signature_at(&params.text_document_position_params);
        self.report_source_inspection(&params.text_document_position_params.text_document.uri)
            .await;
        Ok(result)
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        ordering::ready().await;
        self.wait_document_analysis(Some(&params.text_document.uri))
            .await;
        Ok(self
            .prepare_rename_at(&params)
            .map(PrepareRenameResponse::Range)
            .map(Some)?)
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        ordering::ready().await;
        self.wait_document_analysis(Some(&params.text_document_position.text_document.uri))
            .await;
        self.rename_at(&params.text_document_position, &params.new_name)
            .map(Some)
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        ordering::ready().await;
        // Stored fixes need current model inputs, including automatic requests.
        // Share the pending worker before validation; the lightbulb timer still
        // omits shock templates that only an explicit request needs.
        self.wait_document_analysis(Some(&params.text_document.uri))
            .await;
        let automatic = params.context.trigger_kind == Some(CodeActionTriggerKind::AUTOMATIC);
        let mut actions = self.quick_fixes(&params).unwrap_or_default();
        if !automatic {
            for action in self.shock_templates(&params) {
                actions.push(CodeActionOrCommand::CodeAction(action));
            }
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
        self.wait_document_analysis(Some(
            &params.text_document_position_params.text_document.uri,
        ))
        .await;
        Ok(self.linked_ranges(&params.text_document_position_params))
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        ordering::ready().await;
        let result = self.folding_ranges(&params.text_document.uri);
        self.report_source_inspection(&params.text_document.uri)
            .await;
        Ok(result)
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>> {
        ordering::ready().await;
        self.wait_document_analysis(Some(&params.text_document.uri))
            .await;
        Ok(self.selection_ranges(&params.text_document.uri, &params.positions))
    }

    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
        ordering::ready().await;
        self.wait_document_analysis(Some(&params.text_document.uri))
            .await;
        Ok(self.document_links(&params.text_document.uri))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        ordering::ready().await;
        self.wait_document_analysis(Some(&params.text_document.uri))
            .await;
        Ok(self
            .semantic_tokens(&params.text_document.uri, None)
            .map(SemanticTokensResult::Tokens))
    }

    async fn semantic_tokens_range(
        &self,
        params: SemanticTokensRangeParams,
    ) -> Result<Option<SemanticTokensRangeResult>> {
        ordering::ready().await;
        if self.written_colors_empty(&params.text_document.uri, params.range) {
            return Ok(Some(SemanticTokensRangeResult::Tokens(SemanticTokens {
                result_id: None,
                data: Vec::new(),
            })));
        }
        self.wait_document_analysis(Some(&params.text_document.uri))
            .await;
        Ok(self
            .semantic_tokens(&params.text_document.uri, Some(params.range))
            .map(SemanticTokensRangeResult::Tokens))
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        ordering::ready().await;
        self.wait_document_analysis(Some(&params.text_document.uri))
            .await;
        self.format_document(&params.text_document.uri)
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        ordering::ready().await;
        self.wait_document_analysis(Some(&params.text_document.uri))
            .await;
        self.format_line_range(&params.text_document.uri, params.range)
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> Result<Option<Value>> {
        ordering::ready().await;
        if params.command == "dynare/setDocumentContext" {
            return Ok(Some(self.set_document_context(&params.arguments).await));
        }
        if params.command == "dynare/setAnalysisConfiguration" {
            return Ok(Some(
                self.set_analysis_configuration(&params.arguments).await,
            ));
        }
        if matches!(
            params.command.as_str(),
            "dynare/workspaceStatus" | "dynare/recheckWorkspace" | "dynare/cancelWorkspace"
        ) {
            return Ok(Some(self.workspace_command(&params.command).await));
        }
        let requested_root = params
            .arguments
            .first()
            .and_then(|argument| argument.get("root_uri"))
            .and_then(Value::as_str)
            .and_then(|uri| Url::parse(uri).ok());
        self.wait_document_analysis(requested_root.as_ref()).await;
        Ok(Some(self.execute(&params.command, &params.arguments)))
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        ordering::ready().await;
        let output = self.output_gate.lock().await;
        let (explanations, changes) = self.apply_settings(&params.settings);
        let to_publish = if changes.analysis {
            self.lock_inner().refresh_diagnostics(None)
        } else {
            Vec::new()
        };
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
        if changes.analysis {
            self.refresh_presentation(true).await;
        } else {
            let hints = changes.hints && self.lock_inner().hint_refresh;
            self.presentation_refresh
                .request(&self.client, false, hints);
        }
        self.kick_workspace();
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        ordering::ready().await;
        let output = self.output_gate.lock().await;
        let to_publish = {
            let mut inner = self.lock_inner();
            let previous = inner.settings.clone();
            let removed: Vec<_> = params
                .event
                .removed
                .iter()
                .map(|folder| folder.uri.clone())
                .collect();
            let reset = inner.analysis_configuration.reset_removed(&removed);
            inner
                .settings
                .change_folders(params.event.added, params.event.removed);
            for root in reset {
                let paths = inner.settings.resolve(&root).search_paths;
                let _ = inner.workspace.set_root_analysis_configuration_with_paths(
                    root.as_str(),
                    Default::default(),
                    Some(paths),
                );
                inner.analysis_configuration_committed(&root);
            }
            inner.document_contexts.clear();
            inner.workspace_settings_changed(&previous);
            inner.document_analysis.cancel();
            self.refresh_written_sources(&inner);
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
        self.kick_workspace();
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        ordering::ready().await;
        self.wait_document_analysis(Some(
            &params.text_document_position_params.text_document.uri,
        ))
        .await;
        Ok(self.prepare_hierarchy(&params.text_document_position_params))
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
        ordering::ready().await;
        self.wait_document_analysis(Some(&params.item.uri)).await;
        Ok(self.incoming(&params.item))
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        ordering::ready().await;
        self.wait_document_analysis(Some(&params.item.uri)).await;
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
                trigger_characters: Some(vec!["(".into(), ",".into(), "#".into()]),
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
                    "dynare/compareModelSnapshots".into(),
                    "dynare/validateComparisonInputs".into(),
                    "dynare/showEffectiveModel".into(),
                    "dynare/captureRename".into(),
                    "dynare/modelInfo".into(),
                    "dynare/analysisContext".into(),
                    "dynare/setAnalysisConfiguration".into(),
                    "dynare/sourceCandidates".into(),
                    "dynare/setDocumentContext".into(),
                    "dynare/workspaceStatus".into(),
                    "dynare/recheckWorkspace".into(),
                    "dynare/cancelWorkspace".into(),
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
                "modelInfo": {"command": "dynare/modelInfo", "schema_version": MODEL_INFO_SCHEMA_VERSION, "dependency_candidates": true, "model_locals": true},
                "modelInfoChanged": true,
                "documentContext": {"command":"dynare/setDocumentContext", "schema_version":1},
                "analysisConfiguration": {"schema_version":1,"context_command":"dynare/analysisContext","set_command":"dynare/setAnalysisConfiguration","source_candidates_command":"dynare/sourceCandidates"},
                "rename": {"command":"dynare/captureRename", "schema_version":1, "max_source_bytes":crate::engine::comparison::compare_snapshots::MAX_SNAPSHOT_SOURCE_BYTES},
                "compareModelSnapshots": {"command":"dynare/compareModelSnapshots", "schema_version":crate::engine::comparison::compare_snapshots::SNAPSHOT_SCHEMA_VERSION, "navigation_schema_version":crate::engine::comparison::compare_snapshots::SNAPSHOT_NAVIGATION_SCHEMA_VERSION, "needs_sources":true, "max_manifest_files":crate::engine::comparison::compare_snapshots::MAX_SNAPSHOT_FILES, "max_source_bytes":crate::engine::comparison::compare_snapshots::MAX_SNAPSHOT_SOURCE_BYTES,"semantic_schema_version":1,"source_changes_schema_version":1,"coverage_schema_version":1},
                "effectivePreview": {"command":"dynare/showEffectiveModel", "navigation_schema_version":crate::engine::preview::preview_navigation::NAVIGATION_SCHEMA_VERSION, "source_navigation_schema_version":crate::engine::preview::source_navigation::SOURCE_NAVIGATION_SCHEMA_VERSION, "dependency_candidates":true, "readable_layout":true, "source_layout":true, "macro_ranges":true, "macro_messages":true, "document_symbols":true},
                "configuration": {"schema_version": CONFIGURATION_SCHEMA_VERSION}
                ,"workspaceDiagnostics": {"schema_version":workspace_checks::SCHEMA_VERSION,"status_command":"dynare/workspaceStatus","recheck_command":"dynare/recheckWorkspace","cancel_command":"dynare/cancelWorkspace","active_model_notification":"dynare/activeModelChanged","status_notification":"dynare/workspaceStatusChanged","typing_pause_ms":250}
            }})),
            ..ServerCapabilities::default()
        },
        server_info: Some(ServerInfo {
            name: "dygnosis".into(),
            version: Some(VERSION.into()),
        }),
    }
}

/// Library diagnostics as LSP items, using the shared [`check_file`] checks.
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
    analysis: &crate::engine::analysis_configuration::AnalysisConfiguration,
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
            let Some(uri) = (if origin.file
                == crate::engine::workspace::include_resolver::normalize_uri(root.as_str())
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
    let mut incomplete_cause: Option<(u8, String)> = None;
    let configuration_label = analysis.display_label();
    let routes = files
        .into_iter()
        .map(|(uri, (source, diagnostics))| {
            let converted = library_to_lsp(&source, &diagnostics);
            let rows = diagnostics
                .into_iter()
                .zip(converted)
                .map(|(diagnostic, mut item)| {
                    let rank = match diagnostic.code.as_str() {
                        "E061" => Some(0),
                        "E001" => Some(1),
                        "I211" => Some(2),
                        _ => None,
                    };
                    if let Some(rank) = rank
                        && incomplete_cause
                            .as_ref()
                            .is_none_or(|(prior, _)| rank < *prior)
                    {
                        incomplete_cause = Some((
                            rank,
                            format!(
                                "{}:{} — {}",
                                uri.path().rsplit('/').next().unwrap_or("model"),
                                item.range.start.line + 1,
                                item.message
                            )
                            .chars()
                            .take(512)
                            .collect(),
                        ));
                    }
                    if !analysis.is_source() {
                        item.source = Some(format!("Dygnosis · {configuration_label}"));
                        item.message = format!("[{configuration_label}] {}", item.message);
                    }
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
        incomplete_cause: incomplete_cause.map(|(_, cause)| cause),
        reusable: false,
        stamps: crate::engine::workspace::InputStamps::new(),
        owner_files: HashSet::new(),
    }
}

fn related_location(site: &crate::engine::diagnostics::DiagnosticOrigin) -> Option<Location> {
    site.text
        .get(site.span.start as usize..site.span.end as usize)?;
    let uri = if crate::engine::workspace::include_resolver::is_virtual_uri(&site.file) {
        Url::parse(&site.file).ok()?
    } else {
        file_url_from_path_key(&site.file)?
    };
    let normalized = crate::engine::language::parser::normalize_newlines(&site.text);
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
    if inner
        .workspace
        .root_analysis_configuration(root.as_str())
        .is_preview()
    {
        let title = match &note.code {
            Some(NumberOrString::String(code)) if code == "I208" => "Add equation tags",
            Some(NumberOrString::String(code)) if code == "I209" => "Add long names",
            _ => return None,
        };
        return Some(CodeAction {
            title: title.into(),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![note]),
            disabled: Some(CodeActionDisabled {
                reason: inner
                    .workspace
                    .root_analysis_configuration(root.as_str())
                    .preview_edit_cause(title),
            }),
            ..CodeAction::default()
        });
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
    let context = serde_json::from_value::<crate::engine::diagnostics::WritingContext>(
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
    if crate::engine::workspace::include_resolver::normalize_uri(open.as_str()) == file_key {
        return Some(open.clone());
    }
    if let Some(url) = inner.docs.keys().find(|url| {
        crate::engine::workspace::include_resolver::normalize_uri(url.as_str()) == file_key
    }) {
        return Some(url.clone());
    }
    // An unopened include keeps an open file's directory spelling. Canonicalize
    // alone differs for a Windows 8.3 path and for a directory link.
    client_spelled_file_url(inner, open, file_key).or_else(|| file_url_from_path_key(file_key))
}

fn client_spelled_file_url(inner: &Inner, open: &Url, file_key: &str) -> Option<Url> {
    if crate::engine::workspace::include_resolver::is_virtual_uri(file_key) {
        return None;
    }
    let target = crate::engine::workspace::include_resolver::make_absolute(Path::new(file_key));
    let mut bases = Vec::new();
    if open.scheme() == "file" {
        bases.push(open);
    }
    for url in inner.docs.keys() {
        if url.scheme() == "file" && !bases.contains(&url) {
            bases.push(url);
        }
    }
    let mut best: Option<(usize, Url)> = None;
    for base in bases {
        let Some((spelled, depth)) = spelled_include_path(base, &target) else {
            continue;
        };
        let Ok(url) = Url::from_file_path(&spelled) else {
            continue;
        };
        if crate::engine::workspace::include_resolver::normalize_uri(url.as_str()) != file_key {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(best_depth, _)| depth > *best_depth)
        {
            best = Some((depth, url));
        }
    }
    best.map(|(_, url)| url)
}

fn spelled_include_path(base: &Url, target: &Path) -> Option<(PathBuf, usize)> {
    let base_dir = base.to_file_path().ok()?.parent()?.to_path_buf();
    let canonical_dir = crate::engine::workspace::include_resolver::make_absolute(&base_dir);
    let relative = target.strip_prefix(&canonical_dir).ok()?;
    if relative.as_os_str().is_empty()
        || relative.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return None;
    }
    Some((base_dir.join(relative), canonical_dir.components().count()))
}

fn span_pos(pos: Position) -> crate::engine::language::span::Position {
    crate::engine::language::span::Position {
        line: pos.line,
        character: pos.character,
    }
}

/// A stored fix uses scalar columns. The edit sent to the editor is UTF-16.
fn lsp_pos_from_scalar(index: &LineIndex, text: &str, line: u32, character: u32) -> Position {
    let byte = index.offset(
        text,
        crate::engine::language::span::Position { line, character },
    );
    let pos = index.position_utf16(text, byte);
    Position::new(pos.line, pos.character)
}

fn lsp_macro_messages(
    workspace: &crate::engine::workspace::Workspace,
    root: &str,
    report: &crate::engine::language::expand::ExpandReport,
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
            if let Some(file) = &message.file
                && let Some(uri) = file_url_from_path_key(file)
            {
                row["uri"] = serde_json::json!(uri);
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
    if crate::engine::workspace::include_resolver::is_virtual_uri(path_key) {
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
    segment: &crate::engine::workspace::model_map::WrittenSegment,
) -> Option<Value> {
    let key = segment.file.as_deref()?;
    let text = inner.workspace.get_source(key)?;
    if segment.span.is_empty() {
        return None;
    }
    let range = crate::lsp::model_map::written_range(text, segment.span)?;
    let uri = inner
        .docs
        .keys()
        .chain(std::iter::once(root))
        .find(|uri| crate::engine::workspace::include_resolver::normalize_uri(uri.as_str()) == key)
        .cloned()
        .or_else(|| file_url_from_path_key(key))?;
    if !["file", "untitled"].contains(&uri.scheme()) {
        return None;
    }
    Some(
        json!({"uri":uri,"range":range,"document_version":inner.document(&uri).map(|doc|doc.version)}),
    )
}

fn source_preview_stop_message(
    reason: &crate::engine::language::macro_expand::IncompleteReason,
) -> String {
    reason
        .limit_name
        .map(|name| format!("Source preview stopped at the {name} limit."))
        .unwrap_or_else(|| reason.message.clone())
}

fn incomplete_lsp_reason(
    inner: &mut Inner,
    root: &Url,
    cause: &crate::engine::analysis::model_info::IncompleteCause,
) -> Value {
    let mut value = cause.outcome_json();
    value["location"] = cause
        .origin
        .as_ref()
        .map(|site| incomplete_written_location(inner, root, &site.file, site.span))
        .unwrap_or(Value::Null);
    let chain: Vec<_> = cause
        .include_chain
        .iter()
        .map(|site| incomplete_written_location(inner, root, &site.file, site.span))
        .collect();
    if !chain.is_empty() {
        value["include_chain"] = json!(chain);
    }
    value
}

fn incomplete_written_location(
    inner: &mut Inner,
    root: &Url,
    file: &str,
    written: crate::engine::language::span::Span,
) -> Value {
    let Some(text) = inner.workspace.get_source(file) else {
        return Value::Null;
    };
    let Some(range) = crate::lsp::model_map::written_range(text, written) else {
        return Value::Null;
    };
    let Some(uri) = inner
        .docs
        .keys()
        .chain(std::iter::once(root))
        .find(|uri| crate::engine::workspace::include_resolver::normalize_uri(uri.as_str()) == file)
        .cloned()
        .or_else(|| file_url_from_path_key(file))
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
        let mut parts = vec![names::declaration_heading(word, "var", false)?];
        if let Some(info) = classify_variable_timing(model).get(word) {
            parts.push(format_timing_line(info));
        }
        append_name_metadata(&mut parts, model, word, preferences);
        return Some(parts.join("\n\n"));
    }
    if kind == Some("varexo_det") {
        let mut parts = vec![names::declaration_heading(word, "varexo_det", false)?];
        append_name_metadata(&mut parts, model, word, preferences);
        return Some(parts.join("\n\n"));
    }
    if kind == Some("varexo") {
        let mut parts = vec![names::declaration_heading(word, "varexo", false)?];
        append_name_metadata(&mut parts, model, word, preferences);
        return Some(parts.join("\n\n"));
    }
    if kind == Some("parameters") {
        let mut parts = vec![names::declaration_heading(word, "parameters", false)?];
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
    names::append_name_metadata(
        parts,
        declaration.long_name.as_deref(),
        declaration.tex_name.as_deref(),
        &preferences.name_details,
    );
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
                serde_json::from_value::<crate::engine::diagnostics::WritingContext>(
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
    let roots = inner.context_roots(uri);
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

/// The macro scanner owns which written markers are directives. Completion
/// reads only the name on that token's first line, without expanding the file.
fn macro_directive_name_at(text: &str, byte: u32) -> Option<Span> {
    macro_directive_name_with_tokens(
        text,
        byte,
        &crate::engine::language::macro_expand::scan_macro_tokens(text),
    )
}

fn macro_directive_name_with_tokens(
    text: &str,
    byte: u32,
    tokens: &[crate::engine::language::lexer::Token],
) -> Option<Span> {
    tokens
        .iter()
        .filter(|token| token.kind == TokenKind::MacroDir)
        .find_map(|token| {
            let start = token.span.start as usize + 2;
            let end = token.span.end as usize;
            let bytes = text.as_bytes();
            let mut name_start = start;
            while name_start < end && matches!(bytes[name_start], b' ' | b'\t') {
                name_start += 1;
            }
            let mut name_end = name_start;
            if name_end < end && (bytes[name_end].is_ascii_alphabetic() || bytes[name_end] == b'_')
            {
                name_end += 1;
                while name_end < end
                    && (bytes[name_end].is_ascii_alphanumeric() || bytes[name_end] == b'_')
                {
                    name_end += 1;
                }
            }
            (start <= byte as usize && byte as usize <= name_end)
                .then(|| Span::new(name_start, name_end))
        })
}

fn macro_directive_completions(range: Range, prefix: &str) -> Vec<CompletionItem> {
    crate::engine::catalog::syntax::in_context("macro_directive", Some("macro_directive"), None)
        .enumerate()
        .map(|(order, record)| {
            let name = record.name.trim_start_matches("@#");
            CompletionItem {
                label: name.into(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("macro directive".into()),
                documentation: Some(Documentation::String(record.summary.clone())),
                sort_text: Some(format!("{order:02}")),
                filter_text: Some(name.into()),
                insert_text: Some(name.into()),
                insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: format!("{prefix}{name}"),
                })),
                ..CompletionItem::default()
            }
        })
        .collect()
}

fn default_completions(
    model: &Model,
    preferences: &PresentationSettings,
    label_details: bool,
    snippets: bool,
    context: &syntax_intel::Context,
) -> Vec<CompletionItem> {
    let mut items = syntax_intel::completions(context);
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
    for block in ["model", "steady_state_model", "initval", "endval", "shocks"]
        .into_iter()
        .filter(|_| context.language == "top_level")
    {
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
                value: format!("TeX: {}", crate::lsp::names::code(tex)),
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
        crate::engine::language::span::Position {
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

fn equation_at_byte(model: &Model, byte: u32) -> Option<&Equation> {
    model
        .equations
        .iter()
        .chain(&model.steady_state_equations)
        .find(|equation| equation.span.start <= byte && byte <= equation.span.end)
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
    crate::engine::workspace::check_walk::is_model_path(Path::new(uri.path()))
}

enum ModelInfoChanged {}

impl notification::Notification for ModelInfoChanged {
    type Params = Value;
    const METHOD: &'static str = "dynare/modelInfoChanged";
}

fn explain_command(arguments: &[Value]) -> Value {
    let code = extract_explain_code(arguments);
    let Some(code) = code else {
        return json!({"error":"Explain Diagnostic requires a diagnostic code, for example W010.", "code":"INVALID_ARGUMENTS"});
    };
    match explain::render_user_markdown(&code) {
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
    fn presentation_settings_preserve_model_and_analysis_context_revisions() {
        // Guards a view change refusing an action whose source proof is still
        // current. Analysis-setting changes must invalidate both tokens.
        let root = Url::parse("file:///report-reuse/revision.mod").unwrap();
        let mut inner = Inner::default();
        inner
            .workspace
            .update_document(root.as_str(), "var y; model; y=0; end;");
        let revision = inner.root_revision(&root).unwrap();
        let context = analysis_configuration::context_revision(&mut inner, &root).unwrap();
        inner
            .settings
            .apply(&json!({"outline":{"activity":"inactive"},"formatIndent":2}));
        assert_eq!(inner.root_revision(&root).unwrap(), revision);
        assert_eq!(
            analysis_configuration::context_revision(&mut inner, &root).unwrap(),
            context
        );
        inner.settings.apply(&json!({"macroWorkLimit":1000000}));
        assert_ne!(inner.root_revision(&root).unwrap(), revision);
        assert_ne!(
            analysis_configuration::context_revision(&mut inner, &root).unwrap(),
            context
        );
    }

    #[tokio::test]
    async fn unrelated_open_model_keeps_its_checked_report_after_an_edit() {
        // Guards rebuilding every open root's diagnostics after one model
        // changes. A changed root must still obtain a current report.
        let (service, _socket) = new_service();
        let server = service.inner();
        let first = Url::parse("file:///report-reuse/first.mod").unwrap();
        let second = Url::parse("file:///report-reuse/second.mod").unwrap();
        server.upsert(first.clone(), "var y; model; y=0; end;".to_owned(), 1);
        server.upsert(second.clone(), "var x; model; x=0; end;".to_owned(), 1);
        let (first_report, second_report) = {
            let inner = server.lock_inner();
            (
                Arc::clone(&inner.reports[&first]),
                Arc::clone(&inner.reports[&second]),
            )
        };
        server.upsert(second.clone(), "var x; model; x=1; end;".to_owned(), 2);
        let inner = server.lock_inner();
        assert!(Arc::ptr_eq(&first_report, &inner.reports[&first]));
        assert!(!Arc::ptr_eq(&second_report, &inner.reports[&second]));
        assert_ne!(second_report.revision, inner.reports[&second].revision);
    }

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

#[cfg(test)]
mod value_hint_projection_tests {
    use super::*;

    fn position(text: &str, byte: usize) -> Position {
        let prefix = &text[..byte];
        Position::new(
            prefix.bytes().filter(|byte| *byte == b'\n').count() as u32,
            prefix.rsplit('\n').next().unwrap().encode_utf16().count() as u32,
        )
    }

    #[tokio::test]
    async fn inlay_hints_share_one_source_index_without_changing_native_values_or_ranges() {
        // Guards indexing the whole document for every calibration assignment,
        // while preserving written UTF-16 positions, ordering and native shadows.
        let mut text = String::from("parameters p q r;\r\n");
        for value in 1..=300 {
            text.push_str(&format!("p={value};\r\n"));
        }
        text.push_str("/* 🚀 */ q=p+1;\r\nhelper=2;\r\n/* 😀 */ r=q+helper;\r\nexp=2;\r\nq=exp(p);\r\nr=p+1;\r\np=2;\r\nq=p+1;\r\n");
        let expected = [
            (
                position(
                    &text,
                    text.find("/* 🚀 */ q=p+1;").unwrap() + "/* 🚀 */ q=p+1;".len(),
                ),
                "= 301",
            ),
            (
                position(
                    &text,
                    text.find("/* 😀 */ r=q+helper;").unwrap() + "/* 😀 */ r=q+helper;".len(),
                ),
                "= 303",
            ),
            (
                position(&text, text.rfind("q=p+1;").unwrap() + "q=p+1;".len()),
                "= 3",
            ),
        ];
        let (service, _socket) = new_service();
        let server = service.inner();
        let uri = Url::parse("file:///inlay-projection.mod").unwrap();
        server.upsert(uri.clone(), text, 1);
        for (request, range) in [
            Range::new(Position::new(0, 0), Position::new(u32::MAX, u32::MAX)),
            Range::new(Position::new(expected[2].0.line, 0), expected[2].0),
        ]
        .into_iter()
        .enumerate()
        {
            VALUE_HINT_SOURCE_INDEXES.with(|indexes| indexes.set(0));
            let rows = server
                .inlay_hint(InlayHintParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    range,
                    work_done_progress_params: Default::default(),
                })
                .await
                .unwrap()
                .unwrap();
            let actual: Vec<_> = rows
                .iter()
                .map(|row| {
                    let InlayHintLabel::String(label) = &row.label else {
                        panic!("string hint required")
                    };
                    (row.position, label.as_str())
                })
                .collect();
            let expected: Vec<_> = expected
                .iter()
                .copied()
                .filter(|(position, _)| range.start <= *position && *position <= range.end)
                .collect();
            assert_eq!(actual, expected);
            assert_eq!(
                VALUE_HINT_SOURCE_INDEXES.with(std::cell::Cell::get),
                usize::from(request == 0),
                "inlay hints must project once per current source and owner"
            );
        }
    }
}
