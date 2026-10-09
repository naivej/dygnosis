//! Negotiated semantic roles and literal written-name presentation.

use crate::expr::ExprKind;
use crate::model::Model;
use crate::model_map::{SourceOccurrence, WrittenModelMap};
use crate::span::Span;
use std::collections::{HashMap, HashSet};
use tower_lsp::lsp_types::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NameRole {
    Endogenous,
    Exogenous,
    Parameter,
    ModelLocal,
}

impl NameRole {
    pub fn index(self) -> usize {
        match self {
            Self::Endogenous => 0,
            Self::Exogenous => 1,
            Self::Parameter => 2,
            Self::ModelLocal => 3,
        }
    }
}

const ROLES: [&str; 4] = [
    "dynareEndogenous",
    "dynareExogenous",
    "dynareParameter",
    "dynareModelLocal",
];

#[derive(Clone, Debug)]
pub(crate) struct SemanticMapping {
    pub legend: SemanticTokensLegend,
    types: [Option<u32>; 4],
    pub declaration: u32,
    pub forward: u32,
    pub predetermined: u32,
}

impl Default for SemanticMapping {
    fn default() -> Self {
        Self {
            legend: SemanticTokensLegend {
                token_types: vec![SemanticTokenType::VARIABLE],
                token_modifiers: Vec::new(),
            },
            types: [Some(0); 4],
            declaration: 0,
            forward: 0,
            predetermined: 0,
        }
    }
}

impl SemanticMapping {
    pub fn negotiate(capability: Option<&SemanticTokensClientCapabilities>) -> Self {
        let mut mapping = Self {
            legend: SemanticTokensLegend {
                token_types: Vec::new(),
                token_modifiers: Vec::new(),
            },
            types: [None; 4],
            declaration: 0,
            forward: 0,
            predetermined: 0,
        };
        let Some(capability) = capability else {
            return mapping;
        };
        for (role, custom) in ROLES.iter().enumerate() {
            let selected = if capability
                .token_types
                .iter()
                .any(|token| token.as_str() == *custom)
            {
                Some(SemanticTokenType::new(custom))
            } else if capability
                .token_types
                .contains(&SemanticTokenType::VARIABLE)
            {
                Some(SemanticTokenType::VARIABLE)
            } else {
                None
            };
            if let Some(selected) = selected {
                let index = mapping
                    .legend
                    .token_types
                    .iter()
                    .position(|token| token == &selected)
                    .unwrap_or_else(|| {
                        mapping.legend.token_types.push(selected);
                        mapping.legend.token_types.len() - 1
                    });
                mapping.types[role] = Some(index as u32);
            }
        }
        for modifier in [
            SemanticTokenModifier::DECLARATION,
            SemanticTokenModifier::new("forwardLooking"),
            SemanticTokenModifier::new("predetermined"),
        ] {
            if !capability.token_modifiers.contains(&modifier) {
                continue;
            }
            let bit = 1 << mapping.legend.token_modifiers.len();
            match modifier.as_str() {
                "declaration" => mapping.declaration = bit,
                "forwardLooking" => mapping.forward = bit,
                "predetermined" => mapping.predetermined = bit,
                _ => {}
            }
            mapping.legend.token_modifiers.push(modifier);
        }
        mapping
    }

    pub fn token_type(&self, role: NameRole) -> Option<u32> {
        self.types[role.index()]
    }
}

pub(crate) fn code(value: &str) -> String {
    let longest = value
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(longest + 1);
    if value.starts_with('`') || value.ends_with('`') {
        format!("{fence} {value} {fence}")
    } else {
        format!("{fence}{value}{fence}")
    }
}

pub(crate) fn literal(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| {
            if character.is_ascii_punctuation() {
                vec!['\\', character]
            } else {
                vec![character]
            }
        })
        .collect()
}

fn role(kind: &str) -> Option<NameRole> {
    match kind {
        "var" => Some(NameRole::Endogenous),
        "varexo" | "varexo_det" => Some(NameRole::Exogenous),
        "parameters" => Some(NameRole::Parameter),
        "model_local_variable" => Some(NameRole::ModelLocal),
        _ => None,
    }
}

fn contains(region: Span, token: Span) -> bool {
    region.start <= token.start && token.end <= region.end
}

fn spans(source: &SourceOccurrence, key: &str, text: &str) -> Vec<Span> {
    source
        .segments
        .iter()
        .filter(|segment| segment.file.as_ref().is_none_or(|file| file == key))
        .filter_map(|segment| {
            text.get(segment.span.start as usize..segment.span.end as usize)
                .map(|_| segment.span)
        })
        .collect()
}

struct LocalScope {
    names: HashSet<String>,
    regions: Vec<Span>,
}

pub(crate) struct NameSites {
    roles: HashMap<String, NameRole>,
    known_names: HashSet<String>,
    declarations: Vec<(Span, Option<NameRole>)>,
    writes: Vec<Span>,
    local_scopes: Vec<LocalScope>,
}

impl NameSites {
    pub fn new(model: &Model, map: &WrittenModelMap, uri: &Url, text: &str) -> Self {
        let key = crate::include_resolver::normalize_uri(uri.as_str());
        // Implicit pound definitions now have parser type history, but their
        // editor role still belongs only to the model scope below. Explicit
        // model_local_variable declarations and later ordinary retypes retain
        // their existing global projection.
        let implicit_locals: HashSet<_> = model
            .symbol_type_events
            .iter()
            .filter(|event| {
                model.final_symbol_kind(event.name) == Some("model_local_variable")
                    && !model
                        .model_local_variables
                        .iter()
                        .any(|decl| decl.name == event.name)
            })
            .map(|event| event.name)
            .collect();
        let roles = model
            .symbol_type_events
            .iter()
            .filter(|event| !implicit_locals.contains(&event.name))
            .filter_map(|event| {
                model
                    .final_symbol_kind(event.name)
                    .and_then(role)
                    .map(|role| (model.name(event.name).to_string(), role))
            })
            .collect();
        let known_names = model
            .symbol_type_events
            .iter()
            .filter(|event| !implicit_locals.contains(&event.name))
            .map(|event| model.name(event.name).to_string())
            .collect();
        let mut declarations = Vec::new();
        let mut writes = Vec::new();
        for (declaration, source) in model.written_declarations.iter().zip(&map.declarations) {
            if declaration.written_kind == "predetermined_variables" {
                continue;
            }
            let role = model
                .final_symbol_kind(declaration.declaration.name)
                .and_then(role);
            for span in spans(source, &key, text) {
                declarations.push((span, role));
                writes.push(span);
            }
        }
        for source in &map.writes {
            writes.extend(spans(source, &key, text));
        }
        let mut scopes: HashMap<Option<String>, LocalScope> = HashMap::new();
        for (row, occurrence) in model.written_equations.iter().zip(&map.equations) {
            let scope = scopes
                .entry(occurrence.dimension.clone())
                .or_insert(LocalScope {
                    names: HashSet::new(),
                    regions: Vec::new(),
                });
            scope.regions.extend(spans(&occurrence.source, &key, text));
            if row.equation.is_local
                && let Some(ExprKind::Ident {
                    name,
                    timing: 0,
                    timing_span: None,
                    ..
                }) = row.equation.lhs_expr.map(|id| &model.exprs.get(id).kind)
            {
                scope.names.insert(model.name(*name).to_string());
            }
        }
        Self {
            roles,
            known_names,
            declarations,
            writes,
            local_scopes: scopes.into_values().collect(),
        }
    }

    pub fn role(&self, name: &str, span: Span) -> Option<NameRole> {
        let mut projected = self
            .declarations
            .iter()
            .filter(|(site, _)| contains(*site, span))
            .map(|(_, role)| *role);
        if let Some(role) = projected.next() {
            return projected
                .all(|other| other == role)
                .then_some(role)
                .flatten();
        }
        let fallback = self.roles.get(name).copied();
        // A rejected # shadow does not retype an ordinary symbol. Likewise an
        // excluded or incompatible known name cannot acquire a local role.
        if fallback.is_some() || self.known_names.contains(name) {
            return fallback;
        }
        let scopes: Vec<_> = self
            .local_scopes
            .iter()
            .filter(|scope| scope.regions.iter().any(|region| contains(*region, span)))
            .collect();
        if scopes.is_empty() {
            return fallback;
        }
        let local = scopes
            .iter()
            .map(|scope| {
                if scope.names.contains(name) {
                    Some(NameRole::ModelLocal)
                } else {
                    fallback
                }
            })
            .collect::<Vec<_>>();
        (local.iter().all(|role| *role == local[0]))
            .then_some(local[0])
            .flatten()
    }

    pub fn is_declaration(&self, span: Span) -> bool {
        self.declarations
            .iter()
            .any(|(site, _)| contains(*site, span))
    }
    pub fn is_write(&self, span: Span) -> bool {
        self.writes.iter().any(|site| contains(*site, span))
    }
}
