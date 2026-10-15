//! Accepted symbol bindings and complete source edits, independent of transports.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use super::model_locals::ModelLocals;
use crate::engine::editing::refs::{ident_at, is_legal_ident};
use crate::engine::language::intern::Name;
use crate::engine::language::lexer::{tokenize, TokenKind};
use crate::engine::language::model::Model;
use crate::engine::language::span::Span;
use crate::engine::workspace::Workspace;

mod ordinary;
mod tags;

pub(crate) const SOURCE: Refusal = Refusal::new(
    "SOURCE_UNAVAILABLE",
    "Rename source is unavailable. Open the model root and its required includes.",
);
pub(crate) const MAPPING: Refusal = Refusal::new("UNVERIFIED_TARGET", "Rename cannot verify a literal written target for every occurrence. Resolve generated names or incomplete source locations.");
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
pub(crate) const BINDING_CAPTURE: Refusal = Refusal::new("BINDING_CAPTURE", "The new name would bind existing unresolved uses in an affected scope. Choose a name that has no unresolved uses there.");

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
    Local(LocalIdentity),
    Builtin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Binding {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub dimension: Option<String>,
    pub declaration: Option<(String, Span)>,
    pub definition: Option<(String, Span)>,
    identity: Identity,
}

impl Binding {
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
    facts: ModelLocals,
    mappings: HashMap<Span, (String, Span)>,
    tags: Vec<tags::Reference>,
    bindings: OnceLock<Vec<Binding>>,
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
        let facts = ModelLocals::collect(&model);
        check_cancelled()?;
        spans.extend(facts.declarations.iter().map(|row| row.span));
        spans.extend(facts.definitions.iter().map(|row| row.target_span));
        spans.extend(facts.uses.iter().map(|row| row.span));
        let mappings: HashMap<_, _> = spans
            .into_iter()
            .take_while(|_| !crate::engine::control::cancelled())
            .filter_map(|span| {
                workspace
                    .map_effective_origin(root, span)
                    .map(|written| (span, written))
            })
            .collect();
        let mut files: HashSet<_> = mappings.values().map(|(file, _)| file.clone()).collect();
        files.extend(workspace.input_stamps(root).into_keys());
        let sources = files
            .into_iter()
            .take_while(|_| !crate::engine::control::cancelled())
            .filter_map(|file| {
                workspace
                    .source_for_normalized_key(&file)
                    .map(|text| (file, text.to_owned()))
            })
            .collect();
        let mut result = Self {
            root: root.into(),
            revision,
            model,
            complete,
            sources,
            facts,
            mappings,
            tags: Vec::new(),
            bindings: OnceLock::new(),
        };
        result.tags = tags::collect(&result, &result.model);
        check_cancelled()?;
        Ok(result)
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
        if crate::engine::control::cancelled() {
            return Vec::new();
        }
        self.bindings
            .get_or_init(|| self.collect_bindings())
            .clone()
    }

    fn collect_bindings(&self) -> Vec<Binding> {
        let mut result = Vec::new();
        let mut names = HashSet::new();
        for event in &self.model.symbol_type_events {
            if crate::engine::control::cancelled() {
                break;
            }
            let name = self.model.name(event.name);
            if !names.insert(name.to_owned())
                || self.model.final_symbol_kind(event.name) == Some("model_local_variable")
            {
                continue;
            }
            let kind = self
                .model
                .final_symbol_kind(event.name)
                .unwrap_or(event.kind.as_str());
            let declaration = ordinary::declaration(self, name)
                .ok()
                .map(|(target, _)| target);
            result.push(Binding {
                id: format!("{}:{}:ordinary:{name}", self.revision, self.root),
                name: name.into(),
                kind: kind.into(),
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
        for (_, expr) in self.model.exprs.iter() {
            if crate::engine::control::cancelled() {
                break;
            }
            let name = match expr.kind {
                crate::engine::language::expr::ExprKind::Call { callee, .. }
                    if self.model.final_symbol_kind(callee).is_none() =>
                {
                    Some(self.model.name(callee))
                }
                crate::engine::language::expr::ExprKind::Expectation { .. } => Some("EXPECTATION"),
                crate::engine::language::expr::ExprKind::SteadyState { .. } => Some("STEADY_STATE"),
                _ => None,
            };
            if let Some(name) = name {
                if names.insert(name.into()) {
                    result.push(Binding {
                        id: format!("{}:{}:builtin:{name}", self.revision, self.root),
                        name: name.into(),
                        kind: "builtin_function".into(),
                        dimension: None,
                        declaration: None,
                        definition: None,
                        identity: Identity::Builtin,
                    });
                }
            }
        }
        result
    }

    pub fn select(&self, selector: Selector<'_>) -> Result<Option<Binding>, SelectionError> {
        let target_name = match &selector {
            Selector::Target { file, byte } => self
                .tags
                .iter()
                .find(|tag| {
                    tag.written.as_ref().is_some_and(|(target, span)| {
                        target == file && span.start <= *byte && *byte < span.end
                    })
                })
                .map(|tag| tag.name.clone())
                .or_else(|| {
                    self.sources
                        .get(*file)
                        .and_then(|source| ident_at(source, *byte))
                        .map(|(name, _)| name)
                }),
            _ => None,
        };
        let candidates: Vec<_> = self
            .bindings()
            .into_iter()
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
                            && !(binding.identity == Identity::Builtin
                                && binding.name.eq_ignore_ascii_case(name))
                    }) {
                        return false;
                    }
                    self.references(binding, true).iter().any(|site| {
                        site.written.as_ref().is_some_and(|(target, span)| {
                            target == file && span.start <= *byte && *byte < span.end
                        })
                    })
                }
            })
            .collect();
        match candidates.len() {
            0 => Ok(None),
            1 => Ok(candidates.into_iter().next()),
            _ => Err(SelectionError { reason: Refusal::new("AMBIGUOUS_SYMBOL", "The selected name or written source has more than one symbol binding. Select a candidate binding or a source position in one scope."), candidates }),
        }
    }

    pub fn references(&self, binding: &Binding, include_declaration: bool) -> Vec<Reference> {
        if crate::engine::control::cancelled() {
            return Vec::new();
        }
        match binding.identity {
            Identity::Local(key) => self.local_sites(key, include_declaration),
            Identity::Ordinary => {
                let sites = ordinary::binding_sites(&self.model, &binding.name);
                let native = ordinary::native_orders(&self.model);
                let declarations: HashSet<_> = self
                    .model
                    .symbol_type_events
                    .iter()
                    .filter(|event| self.model.name(event.name) == binding.name && !event.changed)
                    .map(|event| event.span)
                    .collect();
                let mut result: Vec<_> = self
                    .model
                    .expanded_tokens
                    .iter()
                    .take_while(|_| !crate::engine::control::cancelled())
                    .enumerate()
                    .filter(|(order, token)| {
                        token.kind == TokenKind::Ident
                            && token.text(&self.model.source) == binding.name
                            && sites.contains(&token.span)
                            && !native.contains(order)
                    })
                    .filter_map(|(order, token)| {
                        let declaration = declarations.contains(&token.span);
                        (include_declaration || !declaration).then(|| Reference {
                            order,
                            span: token.span,
                            written: self.written(token.span),
                            role: if declaration { "declaration" } else { "use" },
                            dimension: super::model_locals::scope_at_order(&self.model, order)
                                .flatten()
                                .map(|name| self.model.name(name).into()),
                            declaration,
                        })
                    })
                    .collect();
                result.extend(
                    self.tags
                        .iter()
                        .take_while(|_| !crate::engine::control::cancelled())
                        .filter(|tag| {
                            tag.name == binding.name && (include_declaration || !tag.declaration)
                        })
                        .map(|tag| Reference {
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
                        }),
                );
                result.extend(self.call_sites(binding));
                result.sort_by_key(|site| site.order);
                result
            }
            Identity::Builtin => self.call_sites(binding),
        }
    }

    fn call_sites(&self, binding: &Binding) -> Vec<Reference> {
        let starts: HashSet<_> = self
            .model
            .exprs
            .iter()
            .filter_map(|(_, expr)| {
                use crate::engine::language::expr::ExprKind;
                let matches = match &expr.kind {
                    ExprKind::Call { callee, .. } => self.model.name(*callee) == binding.name,
                    ExprKind::Expectation { .. } => binding.name == "EXPECTATION",
                    ExprKind::SteadyState { .. } => binding.name == "STEADY_STATE",
                    _ => false,
                };
                matches.then_some(expr.span.start)
            })
            .collect();
        let native = ordinary::native_orders(&self.model);
        self.model
            .expanded_tokens
            .iter()
            .take_while(|_| !crate::engine::control::cancelled())
            .enumerate()
            .filter(|(order, token)| {
                token.kind == TokenKind::Ident
                    && (token.text(&self.model.source) == binding.name
                        || binding.identity == Identity::Builtin
                            && token
                                .text(&self.model.source)
                                .eq_ignore_ascii_case(&binding.name))
                    && starts.contains(&token.span.start)
                    && !native.contains(order)
            })
            .map(|(order, token)| Reference {
                order,
                span: token.span,
                written: self.written(token.span),
                role: "call",
                dimension: super::model_locals::scope_at_order(&self.model, order)
                    .flatten()
                    .map(|name| self.model.name(name).into()),
                declaration: false,
            })
            .collect()
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
        if source.get(target.1.start as usize..target.1.end as usize) != Some(name)
            || !tokenize(source)
                .iter()
                .any(|token| token.kind == TokenKind::Ident && token.span == target.1)
        {
            return Err(MAPPING);
        }
        Ok(())
    }
}

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
        .bindings()
        .iter()
        .any(|current| current.id == binding.id)
    {
        return Err(Refusal::new(
            "INPUT_CHANGED",
            "The selected binding does not belong to the current source snapshot.",
        ));
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
        Identity::Builtin => {
            return Err(Refusal::new(
                "BUILTIN_SYMBOL",
                "Built-in function names cannot be renamed.",
            ))
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
            let owner_targets = if binding.is_local() {
                let compatible: Vec<_> = owner
                    .bindings()
                    .into_iter()
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
                let targets = owner.local_rename(&other, replacement)?;
                affected_bindings.insert(index, other);
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
