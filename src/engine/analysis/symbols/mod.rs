//! Accepted symbol bindings and complete source edits, independent of transports.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use super::model_locals::ModelLocals;
use super::source_inventory::SourceInventory;
use crate::engine::analysis_configuration::SourceAvailability;
use crate::engine::editing::refs::is_legal_ident;
use crate::engine::language::expr::ExprId;
use crate::engine::language::intern::Name;
use crate::engine::language::lexer::{Token, TokenKind, tokenize};
use crate::engine::language::model::Model;
use crate::engine::language::span::Span;
use crate::engine::workspace::{SourceDocument, Workspace};

mod named;
mod ordinary;
mod tags;

#[cfg(test)]
thread_local! {
    static REFERENCE_LOOKUPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static ACCEPTED_CALL_COLLECTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) const SOURCE: Refusal = Refusal::new(
    "SOURCE_UNAVAILABLE",
    "Rename source is unavailable. Open the model root and its required includes.",
);
pub(crate) const MAPPING: Refusal = Refusal::new(
    "UNVERIFIED_TARGET",
    "Rename cannot verify a literal written target for every occurrence. Resolve generated names or incomplete source locations.",
);
pub(crate) const INCOMPLETE: Refusal = Refusal::new(
    "INCOMPLETE_INPUT",
    "Rename was withheld because required includes, parsing, or macro expansion are incomplete.",
);
pub(crate) const COLLISION: Refusal = Refusal::new(
    "NAME_COLLISION",
    "The new name is already declared in an affected model.",
);
pub(crate) const BINDING: Refusal = Refusal::new(
    "INCOMPATIBLE_BINDING",
    "Rename cannot verify one compatible symbol binding across all affected model owners.",
);
pub(crate) const IDENTIFIER: Refusal = Refusal::new(
    "INVALID_IDENTIFIER",
    "Rename requires a legal, non-reserved Dynare identifier.",
);
pub(crate) const BINDING_CAPTURE: Refusal = Refusal::new(
    "BINDING_CAPTURE",
    "The new name would bind existing unresolved uses in an affected scope. Choose a name that has no unresolved uses there.",
);
pub(crate) const ANALYSIS_PREVIEW: Refusal = Refusal::new(
    "ANALYSIS_PREVIEW_EDIT_UNSUPPORTED",
    "Rename is unavailable while a definition, branch or equation-selection analysis preview is selected. Reset Analysis Configuration before renaming. No edits were proposed.",
);
pub(crate) const CONDITIONAL_EDIT: Refusal = Refusal::new(
    "CONDITIONAL_EDIT_UNPROVEN",
    "Rename cannot verify whether an inactive or uncertain source occurrence is the same symbol or a conflict with the new name. No edits were proposed.",
);
const CONDITIONAL_INCLUDE: Refusal = Refusal::new(
    "CONDITIONAL_EDIT_UNPROVEN",
    "Rename cannot verify uses or name conflicts inside an include skipped by the selected macro conditions. That file was not read for this edit. No edits were proposed.",
);
const CONDITIONAL_GENERATED: Refusal = Refusal::new(
    "CONDITIONAL_EDIT_UNPROVEN",
    "Rename cannot verify the names produced by an inactive or uncertain macro substitution. No edits were proposed.",
);
const CONDITIONAL_SOURCE: Refusal = Refusal::new(
    "CONDITIONAL_EDIT_UNPROVEN",
    "Rename cannot verify conditional source because written-source inspection is incomplete. No edits were proposed.",
);

const CANCELLED: Refusal = Refusal::new(
    "CANCELLED",
    "Symbol analysis was cancelled; no symbol result or rename edits were returned.",
);

fn check_cancelled() -> Result<(), Refusal> {
    if crate::engine::control::cancelled() {
        Err(CANCELLED)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub code: &'static str,
    pub message: &'static str,
}

impl Refusal {
    const fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LocalIdentity {
    name: Name,
    dimension: Option<Name>,
    declaration: Option<usize>,
    definition: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Identity {
    Ordinary,
    Named(usize),
    Local(LocalIdentity),
    Builtin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Binding {
    pub id: String,
    pub name: String,
    pub kind: String,
    /// The owner of a head-specific name, such as `subsamples(alpha)`.
    pub namespace: Option<String>,
    pub dimension: Option<String>,
    pub declaration: Option<(String, Span)>,
    pub definition: Option<(String, Span)>,
    identity: Identity,
}

impl Binding {
    pub fn is_ordinary(&self) -> bool {
        self.identity == Identity::Ordinary && ordinary::ordinary(&self.kind)
    }

    pub fn is_local(&self) -> bool {
        matches!(self.identity, Identity::Local(_))
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Selector<'a> {
    Name(&'a str),
    Target { file: &'a str, byte: u32 },
    Id(&'a str),
}

#[derive(Clone, Debug)]
pub(crate) struct SelectionError {
    pub reason: Refusal,
    pub candidates: Vec<Binding>,
}

#[derive(Clone, Debug)]
pub(crate) struct Reference {
    /// Expanded token order identifies repeated executions of one source site.
    pub order: usize,
    pub span: Span,
    pub written: Option<(String, Span)>,
    pub role: &'static str,
    pub dimension: Option<String>,
    pub declaration: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Edit {
    pub file: String,
    pub span: Span,
    pub expected: String,
    pub replacement: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Rename {
    pub edits: Vec<Edit>,
    pub roots: Vec<String>,
}

/// One analyzed owner and its captured source identities. Acquisition,
/// coordinate conversion and validation of current inputs stay with callers.
pub(crate) struct SemanticModel {
    pub root: String,
    pub revision: String,
    pub model: Model,
    pub complete: bool,
    pub sources: HashMap<String, String>,
    source_documents: BTreeMap<String, SourceDocument>,
    source_tokens: HashMap<String, OnceLock<SourceTokens>>,
    facts: ModelLocals,
    named: Vec<named::Fact>,
    mappings: HashMap<Span, (String, Span)>,
    tags: Vec<tags::Reference>,
    bindings: OnceLock<Vec<Binding>>,
    ordinary_sites: OnceLock<HashMap<Name, Vec<Reference>>>,
    accepted_calls: HashMap<ExprId, (usize, Span)>,
    call_sites: OnceLock<HashMap<String, Vec<Reference>>>,
    parse_stop: Option<usize>,
    accepted_events: HashSet<usize>,
}

enum SourceTokens {
    Inventory {
        inventory: Arc<SourceInventory>,
        raw_targets: Option<OnceLock<Vec<Token>>>,
        context_may_change_from: Option<u32>,
    },
    Lexed {
        tokens: Vec<Token>,
        context_may_change_from: Option<u32>,
    },
}

impl SourceTokens {
    fn tokens(&self) -> Option<&[Token]> {
        match self {
            Self::Inventory { inventory, .. } => inventory
                .structure_complete()
                .then_some(inventory.lexical_tokens.as_slice()),
            Self::Lexed { tokens, .. } => Some(tokens),
        }
    }

    fn macro_may_change_context(&self, byte: u32) -> bool {
        let context_may_change_from = match self {
            Self::Inventory {
                context_may_change_from,
                ..
            }
            | Self::Lexed {
                context_may_change_from,
                ..
            } => context_may_change_from,
        };
        context_may_change_from.is_some_and(|start| start <= byte)
    }

    fn target_tokens(&self, source: &str) -> Option<&[Token]> {
        if let Self::Inventory {
            raw_targets: Some(cache),
            ..
        } = self
        {
            if cache.get().is_none() {
                let tokens = tokenize(source);
                if crate::engine::control::cancelled() {
                    return None;
                }
                let _ = cache.set(tokens);
            }
            return cache.get().map(Vec::as_slice);
        }
        self.tokens()
    }
}

impl SemanticModel {
    pub fn capture(
        workspace: &mut Workspace,
        root: &str,
        revision: String,
    ) -> Result<Self, Refusal> {
        check_cancelled()?;
        let model = workspace.get_effective_model(root).ok_or(SOURCE)?.clone();
        let includes_complete = workspace.includes_complete(root);
        let report = workspace.expand_report(root).ok_or(SOURCE)?;
        let complete = includes_complete && report.complete && report.model_map.complete;
        let mut spans: HashSet<_> = model
            .expanded_tokens
            .iter()
            .take_while(|_| !crate::engine::control::cancelled())
            .map(|token| token.span)
            .collect();
        let mut emitted_sources = HashSet::new();
        for token in &model.expanded_tokens {
            if crate::engine::control::cancelled() {
                break;
            }
            if let Some(emitted) = &token.emitted
                && emitted_sources.insert(std::sync::Arc::as_ptr(&emitted.source) as usize)
            {
                spans.extend(
                    emitted
                        .source
                        .origins
                        .iter()
                        .take_while(|_| !crate::engine::control::cancelled())
                        .map(|origin| origin.written),
                );
            }
        }
        spans.extend(model.symbol_type_events.iter().map(|event| event.span));
        let parse_stop = crate::engine::diagnostics::parse_preflight::first_refusal(&model)
            .map(|stop| stop.order);
        let facts = ModelLocals::collect_before(&model, parse_stop);
        let accepted = crate::engine::analysis::named_callbacks::collect(&model);
        let named = named::collect(&model, &accepted);
        let accepted_calls = Self::collect_accepted_calls(&model, &accepted);
        check_cancelled()?;
        spans.extend(facts.declarations.iter().map(|row| row.span));
        spans.extend(facts.definitions.iter().map(|row| row.target_span));
        spans.extend(facts.uses.iter().map(|row| row.span));
        let origins = workspace.effective_origins(root).ok_or(SOURCE)?;
        let mappings: HashMap<_, _> = spans
            .into_iter()
            .take_while(|_| !crate::engine::control::cancelled())
            .filter_map(|span| origins.map(span).map(|written| (span, written)))
            .collect();
        let mut files: HashSet<_> = mappings.values().map(|(file, _)| file.clone()).collect();
        files.extend(workspace.input_stamps(root).into_keys());
        let source_documents = files
            .iter()
            .take_while(|_| !crate::engine::control::cancelled())
            .filter_map(|file| {
                workspace
                    .source_document(root, file)
                    .map(|document| (file.clone(), document))
            })
            .collect();
        let sources: HashMap<_, _> = files
            .into_iter()
            .take_while(|_| !crate::engine::control::cancelled())
            .filter_map(|file| {
                workspace
                    .source_for_normalized_key(&file)
                    .map(|text| (file, text.to_owned()))
            })
            .collect();
        let source_tokens = sources
            .keys()
            .map(|file| (file.clone(), OnceLock::new()))
            .collect();
        let accepted_events =
            crate::engine::diagnostics::parse_preflight::accepted_symbol_events(&model, parse_stop);
        let mut result = Self {
            root: root.into(),
            revision,
            model,
            complete,
            sources,
            source_documents,
            source_tokens,
            facts,
            named,
            mappings,
            tags: Vec::new(),
            bindings: OnceLock::new(),
            ordinary_sites: OnceLock::new(),
            accepted_calls,
            call_sites: OnceLock::new(),
            parse_stop,
            accepted_events,
        };
        result.tags = tags::collect(&result, &result.model);
        check_cancelled()?;
        Ok(result)
    }

    pub(crate) fn local_facts(&self) -> &ModelLocals {
        &self.facts
    }

    /// Original-source lexer facts for this captured file. Inert inventories
    /// are borrowed only when their tokens match original lexical boundaries.
    pub(crate) fn raw_source_tokens(&self, file: &str) -> Option<&[Token]> {
        self.source_tokens(file)?
            .target_tokens(self.sources.get(file)?)
    }

    pub fn written(&self, span: Span) -> Option<(String, Span)> {
        if let Some(written) = self.mappings.get(&span) {
            return Some(written.clone());
        }
        let (origin, (file, written)) = self
            .mappings
            .iter()
            .take_while(|_| !crate::engine::control::cancelled())
            .filter(|(origin, _)| origin.start <= span.start && span.end <= origin.end)
            .min_by_key(|(origin, _)| origin.end - origin.start)?;
        Some((
            file.clone(),
            Span {
                start: written.start + span.start - origin.start,
                end: written.start + span.end - origin.start,
            },
        ))
    }

    fn local_binding(&self, key: LocalIdentity) -> Binding {
        Binding {
            id: format!(
                "{}:{}:local:{}:{:?}:{:?}:{:?}",
                self.revision,
                self.root,
                self.model.name(key.name),
                key.dimension,
                key.declaration,
                key.definition
            ),
            name: self.model.name(key.name).into(),
            kind: "model_local_variable".into(),
            namespace: None,
            dimension: key.dimension.map(|name| self.model.name(name).into()),
            declaration: key
                .declaration
                .and_then(|id| self.written(self.facts.declarations[id].span)),
            definition: key
                .definition
                .and_then(|id| self.written(self.facts.definitions[id].target_span)),
            identity: Identity::Local(key),
        }
    }

    pub fn local_expression(&self, binding: &Binding) -> Option<&str> {
        let Identity::Local(key) = binding.identity else {
            return None;
        };
        let definition = self.facts.definitions.get(key.definition?)?;
        Some(
            self.model
                .written_equations
                .get(definition.equation_index)?
                .equation
                .rhs
                .as_str(),
        )
    }

    pub fn bindings(&self) -> Vec<Binding> {
        self.binding_inventory().to_vec()
    }

    pub(crate) fn binding_inventory(&self) -> &[Binding] {
        if crate::engine::control::cancelled() {
            return &[];
        }
        if self.bindings.get().is_none() {
            let bindings = self.collect_bindings();
            if crate::engine::control::cancelled() {
                return &[];
            }
            let _ = self.bindings.set(bindings);
        }
        self.bindings.get().unwrap()
    }

    fn collect_bindings(&self) -> Vec<Binding> {
        let mut result = Vec::new();
        let mut names = HashSet::new();
        let kinds: HashMap<_, _> = self
            .model
            .symbol_type_events
            .iter()
            .enumerate()
            .filter(|(index, _)| self.accepted_events.contains(index))
            .map(|(_, event)| (event.name, event.kind))
            .collect();
        for (index, event) in self.model.symbol_type_events.iter().enumerate() {
            if !self.accepted_events.contains(&index) {
                continue;
            }
            if crate::engine::control::cancelled() {
                break;
            }
            let name = self.model.name(event.name);
            if !names.insert(name.to_owned())
                || kinds
                    .get(&event.name)
                    .is_some_and(|kind| kind.as_str() == "model_local_variable")
            {
                continue;
            }
            let kind = kinds.get(&event.name).unwrap_or(&event.kind).as_str();
            let declaration = ordinary::declaration(self, name)
                .ok()
                .map(|(target, _)| target)
                .filter(|(file, span)| {
                    self.sources
                        .get(file)
                        .and_then(|source| source.get(span.start as usize..span.end as usize))
                        == Some(name)
                });
            result.push(Binding {
                id: format!("{}:{}:ordinary:{name}", self.revision, self.root),
                name: name.into(),
                kind: kind.into(),
                namespace: None,
                dimension: self
                    .model
                    .written_declarations
                    .iter()
                    .find(|row| row.declaration.name == event.name)
                    .and_then(|row| row.declaration.heterogeneity.as_ref())
                    .map(|(dimension, _)| self.model.name(*dimension).into()),
                declaration,
                definition: None,
                identity: Identity::Ordinary,
            });
        }
        for tag in &self.tags {
            if crate::engine::control::cancelled() {
                break;
            }
            if tag.declaration && names.insert(tag.name.clone()) {
                result.push(Binding {
                    id: format!("{}:{}:ordinary:{}", self.revision, self.root, tag.name),
                    name: tag.name.clone(),
                    kind: "var".into(),
                    namespace: None,
                    dimension: None,
                    declaration: tag.written.clone(),
                    definition: None,
                    identity: Identity::Ordinary,
                });
            }
        }
        for (id, definition) in self.facts.definitions.iter().enumerate() {
            if crate::engine::control::cancelled() {
                break;
            }
            result.push(self.local_binding(LocalIdentity {
                name: definition.name,
                dimension: definition.dimension,
                declaration: definition.declaration,
                definition: Some(id),
            }));
        }
        for (id, declaration) in self.facts.declarations.iter().enumerate() {
            if crate::engine::control::cancelled() {
                break;
            }
            if !self
                .facts
                .definitions
                .iter()
                .any(|definition| definition.name == declaration.name)
            {
                result.push(self.local_binding(LocalIdentity {
                    name: declaration.name,
                    dimension: None,
                    declaration: Some(id),
                    definition: None,
                }));
            }
        }
        for usage in &self.facts.uses {
            if crate::engine::control::cancelled() {
                break;
            }
            let binding = self.local_binding(LocalIdentity {
                name: usage.name,
                dimension: usage.dimension,
                declaration: usage.declaration,
                definition: usage.definition,
            });
            if !result.contains(&binding) {
                result.push(binding);
            }
        }
        // Calls are retained expression facts, including valid built-in names.
        let accepted_calls = &self.accepted_calls;
        let mut builtin_names = HashSet::new();
        for (id, expr) in self.model.exprs.iter() {
            if crate::engine::control::cancelled() {
                break;
            }
            if matches!(
                expr.kind,
                crate::engine::language::expr::ExprKind::Call { .. }
                    | crate::engine::language::expr::ExprKind::Expectation { .. }
                    | crate::engine::language::expr::ExprKind::SteadyState { .. }
            ) && !accepted_calls.contains_key(&id)
            {
                continue;
            }
            let name = match expr.kind {
                crate::engine::language::expr::ExprKind::Call { callee, .. }
                    if !kinds.contains_key(&callee) =>
                {
                    Some(self.model.name(callee))
                }
                crate::engine::language::expr::ExprKind::Expectation { .. } => Some("EXPECTATION"),
                crate::engine::language::expr::ExprKind::SteadyState { .. } => Some("STEADY_STATE"),
                _ => None,
            };
            if let Some(name) = name {
                let name = match name.to_ascii_lowercase().as_str() {
                    "expectation" => "EXPECTATION".to_string(),
                    "steady_state" => "STEADY_STATE".to_string(),
                    "sum" => "SUM".to_string(),
                    _ => name.to_ascii_lowercase(),
                };
                if !builtin_names.insert(name.clone()) {
                    continue;
                }
                result.push(Binding {
                    id: format!("{}:{}:builtin:{name}", self.revision, self.root),
                    name,
                    kind: "builtin_function".into(),
                    namespace: None,
                    dimension: None,
                    declaration: None,
                    definition: None,
                    identity: Identity::Builtin,
                });
            }
        }
        result.extend((0..self.named.len()).map(|id| named::binding(self, id)));
        result
    }

    pub fn select(&self, selector: Selector<'_>) -> Result<Option<Binding>, SelectionError> {
        let target_name = match &selector {
            Selector::Target { file, byte } => {
                let name = self.target_name(file, *byte);
                if name.is_none()
                    && self
                        .source_tokens(file)
                        .is_some_and(|tokens| !tokens.macro_may_change_context(*byte))
                {
                    return Ok(None);
                }
                name
            }
            _ => None,
        };
        let candidates: Vec<_> = self
            .binding_inventory()
            .iter()
            .filter(|binding| match &selector {
                Selector::Name(name) => {
                    binding.name == *name
                        || binding.identity == Identity::Builtin
                            && binding.name.eq_ignore_ascii_case(name)
                }
                Selector::Id(id) => binding.id == *id,
                Selector::Target { file, byte } => {
                    if target_name.as_ref().is_some_and(|name| {
                        name != &binding.name
                            && binding.kind != "external_function"
                            && !(binding.identity == Identity::Builtin
                                && binding.name.eq_ignore_ascii_case(name))
                    }) {
                        return false;
                    }
                    if let Identity::Named(id) = binding.identity {
                        return named::owns_target(self, id, file, *byte);
                    }
                    self.references(binding, true).iter().any(|site| {
                        site.written.as_ref().is_some_and(|(target, span)| {
                            target == file && span.start <= *byte && *byte < span.end
                        })
                    })
                }
            })
            .cloned()
            .collect();
        match candidates.len() {
            0 => Ok(None),
            1 => Ok(candidates.into_iter().next()),
            _ => Err(SelectionError {
                reason: Refusal::new(
                    "AMBIGUOUS_SYMBOL",
                    "The selected name or written source has more than one symbol binding. Select a candidate binding or a source position in one scope.",
                ),
                candidates,
            }),
        }
    }

    fn target_name(&self, file: &str, byte: u32) -> Option<String> {
        if let Some(tag) = self.tags.iter().find(|tag| {
            tag.written.as_ref().is_some_and(|(target, span)| {
                target == file && span.start <= byte && byte < span.end
            })
        }) {
            return Some(tag.name.clone());
        }
        let source = self.sources.get(file)?;
        let tokens = self.raw_source_tokens(file)?;
        let token = tokens.get(tokens.partition_point(|token| token.span.end <= byte))?;
        if token.span.start > byte {
            return None;
        }
        if token.kind == TokenKind::Ident {
            return Some(token.text(source).into());
        }
        if token.kind != TokenKind::String {
            return None;
        }
        // External implementation declarations can be quoted. Only their
        // accepted inner name, not arbitrary quoted text, is a symbol target.
        self.model
            .symbol_type_events
            .iter()
            .enumerate()
            .take_while(|_| !crate::engine::control::cancelled())
            .find(|(index, event)| {
                self.accepted_events.contains(index)
                    && !event.changed
                    && event.kind.as_str() == "external_function"
                    && self
                        .written(self.external_name_span(event.span))
                        .is_some_and(|(target, span)| {
                            target == file && span.start <= byte && byte < span.end
                        })
            })
            .map(|(_, event)| self.model.name(event.name).into())
    }

    pub fn references(&self, binding: &Binding, include_declaration: bool) -> Vec<Reference> {
        #[cfg(test)]
        REFERENCE_LOOKUPS.with(|count| count.set(count.get() + 1));
        if crate::engine::control::cancelled() {
            return Vec::new();
        }
        match binding.identity {
            Identity::Local(key) => self.local_sites(key, include_declaration),
            Identity::Named(id) => named::references(self, id, include_declaration),
            Identity::Ordinary => {
                if binding.kind == "external_function" {
                    return self.external_sites(binding, include_declaration);
                }
                let mut result = self
                    .model
                    .intern
                    .lookup(&binding.name)
                    .and_then(|name| self.ordinary_site_inventory().get(&name))
                    .into_iter()
                    .flatten()
                    .filter(|site| include_declaration || !site.declaration)
                    .cloned()
                    .collect::<Vec<_>>();
                result.extend(self.call_sites(binding));
                result.sort_by_key(|site| site.order);
                result
            }
            Identity::Builtin => self.call_sites(binding),
        }
    }

    /// All parser-owned ordinary-family sites in one cached traversal. Names
    /// are keys for joining binding metadata; equal text is never site proof.
    pub fn accepted_ordinary_sites(&self) -> impl Iterator<Item = (&str, &Reference)> {
        self.ordinary_site_inventory()
            .iter()
            .flat_map(move |(name, sites)| {
                sites.iter().map(move |site| (self.model.name(*name), site))
            })
            .take_while(|_| !crate::engine::control::cancelled())
    }

    fn ordinary_site_inventory(&self) -> &HashMap<Name, Vec<Reference>> {
        self.ordinary_sites.get_or_init(|| {
            let sites = ordinary::binding_site_inventory(&self.model);
            let native = ordinary::native_orders(&self.model);
            let excluded_nonordinary = ordinary::excluded_nonordinary_orders(&self.model);
            let refused_events: HashSet<_> = self
                .model
                .type_event_occurrences
                .iter()
                .filter(|(index, _)| !self.accepted_events.contains(index))
                .flat_map(|(_, range)| range.clone())
                .collect();
            let mut declarations = HashMap::<Name, HashSet<usize>>::new();
            for (index, range) in &self.model.type_event_occurrences {
                let event = &self.model.symbol_type_events[*index];
                if self.accepted_events.contains(index) && !event.changed {
                    declarations
                        .entry(event.name)
                        .or_default()
                        .insert(range.start);
                }
            }
            let mut result = HashMap::<Name, Vec<Reference>>::new();
            for (order, token) in self.model.expanded_tokens.iter().enumerate() {
                if crate::engine::control::cancelled() {
                    return HashMap::new();
                }
                if self.parse_stop.is_some_and(|stop| order >= stop)
                    || token.kind != TokenKind::Ident
                    || native.contains(&order)
                    || excluded_nonordinary.contains(&order)
                    || refused_events.contains(&order)
                {
                    continue;
                }
                let Some(name) = self.model.intern.lookup(token.text(&self.model.source)) else {
                    continue;
                };
                if !sites
                    .get(&name)
                    .is_some_and(|sites| sites.contains(&token.span))
                {
                    continue;
                }
                let declaration = declarations
                    .get(&name)
                    .is_some_and(|sites| sites.contains(&order));
                result.entry(name).or_default().push(Reference {
                    order,
                    span: token.span,
                    written: self.written(token.span),
                    role: if declaration { "declaration" } else { "use" },
                    dimension: super::model_locals::scope_at_order(&self.model, order)
                        .flatten()
                        .map(|name| self.model.name(name).into()),
                    declaration,
                });
            }
            for tag in &self.tags {
                if self
                    .parse_stop
                    .is_some_and(|stop| tag.declaration_order > stop)
                {
                    continue;
                }
                if crate::engine::control::cancelled() {
                    return HashMap::new();
                }
                let Some(name) = self.model.intern.lookup(&tag.name) else {
                    continue;
                };
                result.entry(name).or_default().push(Reference {
                    order: tag.order.unwrap_or(tag.declaration_order),
                    span: tag
                        .order
                        .map(|order| self.model.expanded_tokens[order].span)
                        .unwrap_or_default(),
                    written: tag.written.clone(),
                    role: if tag.declaration {
                        "declaration"
                    } else {
                        "tag_reference"
                    },
                    dimension: None,
                    declaration: tag.declaration,
                });
            }
            for sites in result.values_mut() {
                sites.sort_by_key(|site| site.order);
            }
            result
        })
    }

    fn collect_accepted_calls(
        model: &Model,
        accepted: &crate::engine::analysis::named_callbacks::Accepted,
    ) -> HashMap<ExprId, (usize, Span)> {
        #[cfg(test)]
        ACCEPTED_CALL_COLLECTIONS.with(|count| count.set(count.get() + 1));
        let owners: HashSet<_> = accepted
            .slots
            .iter()
            .take_while(|_| !crate::engine::control::cancelled())
            .filter_map(|index| match model.named_slots[*index].owner {
                crate::engine::language::model::NamedSlotOwner::ModelOperator(owner) => Some(owner),
                _ => None,
            })
            .collect();
        let mut result: HashMap<_, _> = model
            .function_calls
            .iter()
            .take_while(|_| !crate::engine::control::cancelled())
            .filter(|call| {
                accepted
                    .refusal
                    .as_ref()
                    .is_none_or(|stop| call.callback_order <= stop.order)
            })
            .map(|call| (call.expr, (call.order, call.span)))
            .collect();
        result.extend(
            model
                .named_model_operator_exprs
                .iter()
                .take_while(|_| !crate::engine::control::cancelled())
                .filter(|(_, owner)| owners.contains(owner))
                .map(|(id, owner)| {
                    let row = &model.named_model_operators[*owner];
                    (*id, (row.keyword_order, row.operator_span))
                }),
        );
        result
    }

    fn call_sites(&self, binding: &Binding) -> Vec<Reference> {
        let Some(inventory) = self.call_site_inventory() else {
            return Vec::new();
        };
        if binding.identity != Identity::Builtin {
            return inventory.get(&binding.name).cloned().unwrap_or_default();
        }
        let mut result: Vec<_> = inventory
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case(&binding.name))
            .flat_map(|(_, sites)| sites.iter().cloned())
            .collect();
        result.sort_by_key(|site| site.order);
        result
    }

    fn call_site_inventory(&self) -> Option<&HashMap<String, Vec<Reference>>> {
        if crate::engine::control::cancelled() {
            return None;
        }
        if self.call_sites.get().is_none() {
            let mut result = HashMap::<String, Vec<Reference>>::new();
            for (id, &(order, span)) in &self.accepted_calls {
                if crate::engine::control::cancelled() {
                    return None;
                }
                use crate::engine::language::expr::ExprKind;
                let name = match self.model.exprs.get(*id).kind {
                    ExprKind::Call { callee, .. } => self.model.name(callee),
                    ExprKind::Expectation { .. } => "EXPECTATION",
                    ExprKind::SteadyState { .. } => "STEADY_STATE",
                    _ => continue,
                };
                result.entry(name.into()).or_default().push(Reference {
                    order,
                    span,
                    written: self.written(span),
                    role: "call",
                    dimension: super::model_locals::scope_at_order(&self.model, order)
                        .flatten()
                        .map(|name| self.model.name(name).into()),
                    declaration: false,
                });
            }
            for sites in result.values_mut() {
                sites.sort_by_key(|site| site.order);
            }
            if crate::engine::control::cancelled() {
                return None;
            }
            let _ = self.call_sites.set(result);
        }
        self.call_sites.get()
    }

    fn external_sites(&self, binding: &Binding, include_declaration: bool) -> Vec<Reference> {
        let mut result = self.call_sites(binding);
        if include_declaration {
            result.extend(
                self.model
                    .symbol_type_events
                    .iter()
                    .enumerate()
                    .take_while(|_| !crate::engine::control::cancelled())
                    .filter(|(index, event)| {
                        self.accepted_events.contains(index)
                            && !event.changed
                            && event.kind.as_str() == "external_function"
                            && self.model.name(event.name) == binding.name
                    })
                    .filter_map(|(index, event)| {
                        let order = self
                            .model
                            .type_event_occurrences
                            .iter()
                            .find(|(id, _)| *id == index)?
                            .1
                            .start;
                        let span = self.external_name_span(event.span);
                        Some(Reference {
                            order,
                            span,
                            written: self.written(span),
                            role: "declaration",
                            dimension: None,
                            declaration: true,
                        })
                    }),
            );
        }
        result.sort_by_key(|site| site.order);
        result
    }

    fn external_name_span(&self, span: Span) -> Span {
        let Some(token) = self
            .model
            .expanded_tokens
            .iter()
            .find(|token| token.span == span && token.kind == TokenKind::String)
        else {
            return span;
        };
        let text = token.text(&self.model.source);
        if text.len() >= 2
            && matches!(text.as_bytes()[0], b'\'' | b'"')
            && text.as_bytes().first() == text.as_bytes().last()
        {
            Span {
                start: span.start + 1,
                end: span.end - 1,
            }
        } else {
            span
        }
    }

    fn local_sites(&self, key: LocalIdentity, include_declaration: bool) -> Vec<Reference> {
        let mut sites = Vec::new();
        if include_declaration {
            if let Some(id) = key.declaration {
                let row = &self.facts.declarations[id];
                sites.push((
                    row.span,
                    row.parse_order.saturating_sub(1),
                    "declaration",
                    true,
                ));
            }
            if let Some(id) = key.definition {
                let row = &self.facts.definitions[id];
                sites.push((row.target_span, row.parse_order, "definition", true));
            }
        }
        sites.extend(
            self.facts
                .uses
                .iter()
                .filter(|usage| {
                    usage.name == key.name
                        && usage.dimension == key.dimension
                        && usage.definition == key.definition
                        && usage.declaration == key.declaration
                })
                .map(|usage| (usage.span, usage.parse_order, "use", false)),
        );
        sites
            .into_iter()
            .map(|(span, order, role, declaration)| Reference {
                order,
                span,
                written: self.written(span),
                role,
                dimension: key.dimension.map(|name| self.model.name(name).into()),
                declaration,
            })
            .collect()
    }

    fn local_rename(
        &self,
        binding: &Binding,
        replacement: &str,
    ) -> Result<Vec<(String, Span)>, Refusal> {
        check_cancelled()?;
        let Identity::Local(key) = binding.identity else {
            return Err(BINDING);
        };
        if !self.complete {
            return Err(INCOMPLETE);
        }
        if !super::model_locals::is_local_name(replacement) {
            return Err(IDENTIFIER);
        }
        if replacement != binding.name
            && (self
                .model
                .symbol_type_events
                .iter()
                .any(|event| self.model.name(event.name) == replacement)
                || self.facts.definitions.iter().any(|definition| {
                    self.model.name(definition.name) == replacement
                        && definition.dimension == key.dimension
                }))
        {
            return Err(COLLISION);
        }
        let available_order = key
            .declaration
            .map(|id| self.facts.declarations[id].parse_order)
            .into_iter()
            .chain(
                key.definition
                    .map(|id| self.facts.definitions[id].end_order),
            )
            .min()
            .unwrap_or(0);
        if replacement != binding.name
            && self.model.model_expression_uses.iter().any(|usage| {
                self.model.name(usage.name) == replacement
                    && usage.parse_order >= available_order
                    && super::model_locals::scope_at_order(&self.model, usage.parse_order)
                        == Some(key.dimension)
                    && self
                        .model
                        .symbol_kind_in_context(usage.name, usage.context)
                        .is_none()
            })
        {
            return Err(BINDING_CAPTURE);
        }
        if self
            .facts
            .declarations
            .iter()
            .filter(|decl| decl.name == key.name)
            .count()
            > 1
            || key.declaration.is_some()
                && self.facts.definitions.iter().any(|definition| {
                    definition.name == key.name && definition.dimension != key.dimension
                })
        {
            return Err(BINDING);
        }
        let sites = self.local_sites(key, true);
        let mut targets = Vec::new();
        for site in &sites {
            if crate::engine::control::cancelled() {
                break;
            }
            let target = site.written.clone().ok_or(MAPPING)?;
            self.literal_identifier(&target, &binding.name)?;
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
        for (order, token) in self.model.expanded_tokens.iter().enumerate() {
            if crate::engine::control::cancelled() {
                break;
            }
            if self
                .written(token.span)
                .is_some_and(|target| targets.contains(&target))
                && !self.owns_local_execution(key, order)
            {
                return Err(BINDING);
            }
        }
        if targets.is_empty() {
            return Err(MAPPING);
        }
        check_cancelled()?;
        Ok(targets)
    }

    fn owns_local_execution(&self, key: LocalIdentity, order: usize) -> bool {
        key.declaration
            .is_some_and(|id| self.facts.declarations[id].parse_order == order + 1)
            || key.definition.is_some_and(|id| {
                let row = &self.facts.definitions[id];
                self.model.written_equations[row.equation_index]
                    .equation
                    .active_tokens
                    .contains(&order)
                    && self.model.expanded_tokens[order].span == row.target_span
            })
            || self.facts.uses.iter().any(|usage| {
                usage.parse_order == order
                    && usage.name == key.name
                    && usage.dimension == key.dimension
                    && usage.definition == key.definition
                    && usage.declaration == key.declaration
            })
    }

    fn literal_identifier(&self, target: &(String, Span), name: &str) -> Result<(), Refusal> {
        let source = self.sources.get(&target.0).ok_or(SOURCE)?;
        let tokens = self.written_tokens(&target.0).ok_or(MAPPING)?;
        let index = tokens.partition_point(|token| token.span.start < target.1.start);
        if source.get(target.1.start as usize..target.1.end as usize) != Some(name)
            || !tokens
                .get(index)
                .is_some_and(|token| token.kind == TokenKind::Ident && token.span == target.1)
        {
            return Err(MAPPING);
        }
        Ok(())
    }

    /// Reuse source syntax if a source feature already built it. Ordinary
    /// symbol reads and edits do not request an inert parse just for this proof.
    fn written_tokens(&self, file: &str) -> Option<&[Token]> {
        self.source_tokens(file)?.tokens()
    }

    fn source_tokens(&self, file: &str) -> Option<&SourceTokens> {
        let source = self.sources.get(file)?;
        let cached = self.source_tokens.get(file)?;
        if let Some(tokens) = cached.get() {
            return Some(tokens);
        }
        // Macro execution precedes the lexer and can change a later quote or
        // comment boundary or map a generated name to its own source anchor.
        // Retain this fact once; trivia at or after it needs semantic selection.
        let context_may_change_from = if file != self.root
            && file != crate::engine::workspace::include_resolver::normalize_uri(&self.root)
        {
            // Included fragments can inherit a quote or comment from another
            // file, even when this file contains no written macro token.
            Some(0)
        } else {
            self.source_documents
                .get(file)
                .and_then(SourceDocument::macro_source)
                .map(|source| {
                    source
                        .tokens
                        .iter()
                        .find(|token| {
                            matches!(
                                token.kind,
                                TokenKind::MacroDir
                                    | TokenKind::MacroInterp
                                    | TokenKind::MacroPrefix
                            )
                        })
                        .map(|token| token.span.start)
                })
                .unwrap_or(Some(0))
        };
        let tokens = self
            .source_documents
            .get(file)
            .and_then(SourceDocument::cached_inventory)
            .map(|inventory| {
                let raw_targets = (!inventory.structure_complete()
                    || inventory.macro_tokens.iter().any(|token| {
                        matches!(
                            token.kind,
                            TokenKind::MacroDir | TokenKind::MacroInterp | TokenKind::MacroPrefix
                        )
                    })
                    || source.contains('\r'))
                .then(OnceLock::new);
                SourceTokens::Inventory {
                    inventory,
                    raw_targets,
                    context_may_change_from,
                }
            })
            .unwrap_or_else(|| SourceTokens::Lexed {
                tokens: tokenize(source),
                context_may_change_from,
            });
        if crate::engine::control::cancelled() {
            return None;
        }
        let _ = cached.set(tokens);
        cached.get()
    }

    fn prove_conditional_edit(&self, name: &str, replacement: &str) -> Result<(), Refusal> {
        if !self.complete {
            // The binding-specific proof reports the existing incomplete cause.
            return Ok(());
        }
        for document in self.source_documents.values() {
            check_cancelled()?;
            if !document.has_conditional_source() {
                continue;
            }
            let inventory = document.inventory().ok_or(CONDITIONAL_SOURCE)?;
            check_cancelled()?;
            if !inventory.structure_complete() {
                return Err(CONDITIONAL_SOURCE);
            }
            for token in inventory.literal_identifiers() {
                check_cancelled()?;
                let written = token.text(document.source());
                if (written == name || written == replacement)
                    && document.availability(token.span) != SourceAvailability::Active
                {
                    return Err(CONDITIONAL_EDIT);
                }
            }
            // A dormant include or substitution can hide any name. Do not
            // acquire dormant files or execute alternatives to guess its uses.
            for token in inventory.macro_tokens.iter() {
                check_cancelled()?;
                let possible_names = token.kind == TokenKind::MacroInterp && !token.comment_context
                    || token.kind == TokenKind::MacroDir
                        && crate::engine::language::macro_expand::directive_name(
                            token.text(document.source()),
                        ) == "include";
                if possible_names
                    && !inventory.is_proven_native(token.span)
                    && document.availability(token.span) != SourceAvailability::Active
                {
                    return Err(if token.kind == TokenKind::MacroDir {
                        CONDITIONAL_INCLUDE
                    } else {
                        CONDITIONAL_GENERATED
                    });
                }
            }
            // These two quoted operands have a symbol role. Other strings,
            // including equation labels, do not become possible name uses.
            for tokens in inventory.lexical_tokens.windows(3) {
                check_cancelled()?;
                if tokens[0].kind != TokenKind::Ident
                    || tokens[1].kind != TokenKind::Eq
                    || tokens[2].kind != TokenKind::String
                    || inventory.is_proven_native(tokens[2].span)
                {
                    continue;
                }
                let key = tokens[0].text(document.source()).to_ascii_lowercase();
                if let Some(value) = tags::content(tokens[2].text(document.source()))
                    && let Some((_, written)) = tags::semantic_name(&key, value)
                    && (written == name || written == replacement)
                    && document.availability(tokens[2].span) != SourceAvailability::Active
                {
                    return Err(CONDITIONAL_EDIT);
                }
            }
        }
        check_cancelled()
    }
}

#[cfg(test)]
mod named_tests;
#[cfg(test)]
mod tests;

/// The caller supplies exactly the owners it checked. This closes over edited
/// sources within that set; equal spelling in an unrelated owner adds no edit.
pub(crate) fn rename(
    owners: &[SemanticModel],
    selected_owner: usize,
    binding: &Binding,
    replacement: &str,
) -> Result<Rename, Refusal> {
    check_cancelled()?;
    let selected = owners.get(selected_owner).ok_or(SOURCE)?;
    if !selected
        .binding_inventory()
        .iter()
        .any(|current| current.id == binding.id)
    {
        return Err(Refusal::new(
            "INPUT_CHANGED",
            "The selected binding does not belong to the current source snapshot.",
        ));
    }
    if selected.model.is_analysis_preview() {
        return Err(ANALYSIS_PREVIEW);
    }
    if binding.identity != Identity::Builtin {
        selected.prove_conditional_edit(&binding.name, replacement)?;
    }
    let (mut targets, declaration, kind) = match binding.identity {
        Identity::Local(_) => (
            selected.local_rename(binding, replacement)?,
            binding
                .declaration
                .clone()
                .or_else(|| binding.definition.clone()),
            binding.kind.clone(),
        ),
        Identity::Ordinary => {
            let decision = ordinary::decision(selected, &binding.name, replacement)?;
            (
                decision.targets,
                Some(decision.declaration),
                decision.kind.into(),
            )
        }
        Identity::Named(id) => (
            named::rename(selected, id, replacement)?,
            binding.declaration.clone(),
            binding.kind.clone(),
        ),
        Identity::Builtin => {
            return Err(Refusal::new(
                "BUILTIN_SYMBOL",
                "Built-in function names cannot be renamed.",
            ));
        }
    };
    let mut affected = vec![selected_owner];
    let mut affected_bindings = HashMap::from([(selected_owner, binding.clone())]);
    loop {
        let mut added = false;
        for (index, owner) in owners.iter().enumerate() {
            if crate::engine::control::cancelled() {
                break;
            }
            if affected.contains(&index)
                || !targets
                    .iter()
                    .any(|(file, _)| owner.sources.contains_key(file))
            {
                continue;
            }
            if owner.model.is_analysis_preview() {
                return Err(ANALYSIS_PREVIEW);
            }
            owner.prove_conditional_edit(&binding.name, replacement)?;
            let owner_targets = if binding.is_local() {
                let compatible: Vec<_> = owner
                    .binding_inventory()
                    .iter()
                    .filter(|other| {
                        other.is_local()
                            && other.name == binding.name
                            && other.dimension == binding.dimension
                            && other.declaration == binding.declaration
                            && other.definition == binding.definition
                            && (other.declaration.is_some() || other.definition.is_some())
                    })
                    .collect();
                if compatible.len() != 1 {
                    return Err(BINDING);
                }
                let other = compatible.into_iter().next().unwrap();
                let targets = owner.local_rename(other, replacement)?;
                affected_bindings.insert(index, other.clone());
                targets
            } else if matches!(binding.identity, Identity::Named(_)) {
                let mut compatible: Vec<_> = owner
                    .binding_inventory()
                    .iter()
                    .filter(|other| {
                        matches!(other.identity, Identity::Named(_))
                            && other.name == binding.name
                            && other.kind == kind
                            && other.declaration == declaration
                    })
                    .collect();
                if compatible.len() > 1 {
                    compatible.retain(|other| matches!(other.identity, Identity::Named(id) if owner.named[id].origin == id));
                }
                if compatible.len() != 1 {
                    return Err(BINDING);
                }
                let other = compatible.pop().unwrap();
                let Identity::Named(id) = other.identity else {
                    unreachable!()
                };
                let targets = named::rename(owner, id, replacement)?;
                affected_bindings.insert(index, other.clone());
                targets
            } else {
                let decision = ordinary::decision(owner, &binding.name, replacement)?;
                if declaration.as_ref() != Some(&decision.declaration) || kind != decision.kind {
                    return Err(BINDING);
                }
                decision.targets
            };
            for target in owner_targets {
                if crate::engine::control::cancelled() {
                    break;
                }
                if !targets.contains(&target) {
                    targets.push(target);
                }
            }
            affected.push(index);
            added = true;
        }
        if !added {
            break;
        }
    }
    let target_set: HashSet<_> = targets.iter().cloned().collect();
    for index in &affected {
        if crate::engine::control::cancelled() {
            break;
        }
        let owner = &owners[*index];
        if let Some(other) = affected_bindings.get(index)
            && let Identity::Local(key) = other.identity
            && owner
                .model
                .expanded_tokens
                .iter()
                .enumerate()
                .any(|(order, token)| {
                    owner
                        .written(token.span)
                        .is_some_and(|target| target_set.contains(&target))
                        && !owner.owns_local_execution(key, order)
                })
        {
            return Err(BINDING);
        }
        if !tags::compatible(owner, &owner.model, &owner.tags, &target_set, &binding.name) {
            return Err(BINDING);
        }
    }
    targets.sort_by(|(left_file, left_span), (right_file, right_span)| {
        left_file
            .cmp(right_file)
            .then(left_span.start.cmp(&right_span.start))
            .then(left_span.end.cmp(&right_span.end))
    });
    if targets
        .windows(2)
        .any(|pair| pair[0].0 == pair[1].0 && pair[0].1.end > pair[1].1.start)
    {
        return Err(MAPPING);
    }
    let edits = targets
        .into_iter()
        .map(|(file, span)| Edit {
            file,
            span,
            expected: binding.name.clone(),
            replacement: replacement.into(),
        })
        .collect();
    check_cancelled()?;
    Ok(Rename {
        edits,
        roots: affected
            .into_iter()
            .map(|index| owners[index].root.clone())
            .collect(),
    })
}
