//! Proven model-local declarations, definitions, and bound reads.
//!
//! Identities use expanded execution order. A written span alone cannot identify
//! a repeated macro execution. Source origins remain in the written model map.

use std::collections::{HashMap, HashSet};

use crate::expr::{ExprId, ExprKind};
use crate::intern::Name;
use crate::model::Model;
use crate::span::Span;

#[derive(Clone, Debug)]
pub struct LocalDeclaration {
    pub name: Name,
    pub span: Span,
    pub parse_order: usize,
    /// Index in `Model::written_declarations`, for verified source mapping.
    pub declaration_index: usize,
    pub tex_name: Option<String>,
}

#[derive(Clone, Debug)]
pub struct LocalDefinition {
    pub name: Name,
    pub target_span: Span,
    pub parse_order: usize,
    /// First execution token after the completed definition.
    pub end_order: usize,
    pub dimension: Option<Name>,
    /// Index in `Model::written_equations`, including removed written rows.
    pub equation_index: usize,
    pub rhs_expr: ExprId,
    /// Index in this inventory's declaration list.
    pub declaration: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct LocalUse {
    pub name: Name,
    pub span: Span,
    pub full_span: Span,
    pub timing: i32,
    pub parse_order: usize,
    pub dimension: Option<Name>,
    pub equation_index: usize,
    /// Index in this inventory's definitions; absent for a declaration alone
    /// or the pinned quiet cross-tree case.
    pub definition: Option<usize>,
    pub declaration: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalBinding {
    pub name: Name,
    pub declaration: Option<usize>,
    pub definition: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct ModelLocals {
    pub declarations: Vec<LocalDeclaration>,
    pub definitions: Vec<LocalDefinition>,
    pub uses: Vec<LocalUse>,
}

impl ModelLocals {
    pub fn collect(model: &Model) -> Self {
        let mut facts = Self::default();
        for (declaration_index, row) in model.written_declarations.iter().enumerate() {
            if row.written_kind != "model_local_variable" {
                continue;
            }
            let decl = &row.declaration;
            // A refused different-type declaration is not a local binding.
            let earlier = model.symbol_type_events
                [..decl.symbol_type_context.index().saturating_sub(1)]
                .iter()
                .rev()
                .find(|event| event.name == decl.name);
            if earlier.is_some_and(|event| event.kind.as_str() != "model_local_variable") {
                continue;
            }
            facts.declarations.push(LocalDeclaration {
                name: decl.name,
                span: decl.span,
                parse_order: decl.parse_order,
                declaration_index,
                tex_name: decl.tex_name.clone(),
            });
        }
        let mut definitions = HashMap::new();
        for (equation_index, row) in model.written_equations.iter().enumerate() {
            let eq = &row.equation;
            let Some(lhs) = eq
                .lhs_expr
                .filter(|lhs| model.valid_model_local_targets.contains(lhs))
            else {
                continue;
            };
            let ExprKind::Ident {
                name, ident_span, ..
            } = model.exprs.get(lhs).kind
            else {
                continue;
            };
            let Some(rhs_expr) = eq.rhs_expr else {
                continue;
            };
            let index = facts.definitions.len();
            definitions.insert((row.dimension, name), index);
            facts.definitions.push(LocalDefinition {
                name,
                target_span: ident_span,
                parse_order: eq.parse_order,
                end_order: eq.active_tokens.end + 1,
                dimension: row.dimension,
                equation_index,
                rhs_expr,
                declaration: facts.declarations.iter().position(|decl| decl.name == name),
            });
        }
        let mut reads: Vec<_> = model.model_expression_uses.iter().collect();
        reads.sort_by_key(|read| read.parse_order);
        for (equation_index, row) in model.written_equations.iter().enumerate() {
            let eq = &row.equation;
            let roots = if eq.is_local {
                vec![eq.rhs_expr]
            } else {
                vec![eq.lhs_expr, eq.rhs_expr]
            };
            let names: HashSet<_> = roots
                .into_iter()
                .flatten()
                .flat_map(|expr| {
                    model
                        .exprs
                        .walk_idents(expr)
                        .map(|read| (read.name, read.span))
                })
                .collect();
            let first = reads.partition_point(|read| read.parse_order < eq.active_tokens.start);
            for read in reads[first..]
                .iter()
                .take_while(|read| read.parse_order < eq.active_tokens.end)
                .filter(|read| {
                    names.contains(&(read.name, read.span))
                        && model.symbol_kind_in_context(read.name, read.context)
                            == Some("model_local_variable")
                })
            {
                let definition = definitions.get(&(row.dimension, read.name)).copied();
                // A definition already establishes this binding. Later explicit
                // metadata belongs to the same identity without making a bare
                // declaration available before its execution.
                let declaration = definition
                    .and_then(|index| facts.definitions[index].declaration)
                    .or_else(|| facts.declaration_before(read.name, read.parse_order));
                facts.uses.push(LocalUse {
                    name: read.name,
                    span: read.span,
                    full_span: read.full_span,
                    timing: read.timing,
                    parse_order: read.parse_order,
                    dimension: row.dimension,
                    equation_index,
                    definition,
                    declaration,
                });
            }
        }
        facts.uses.sort_by_key(|read| read.parse_order);
        facts
    }

    fn declaration_before(&self, name: Name, order: usize) -> Option<usize> {
        self.declarations
            .iter()
            .enumerate()
            .find(|(_, decl)| decl.name == name && decl.parse_order <= order)
            .map(|(index, _)| index)
    }

    /// Names available at a model-expression token. Explicit declarations are
    /// global; completed definitions are available only in their model scope.
    pub fn available(&self, dimension: Option<Name>, parse_order: usize) -> Vec<LocalBinding> {
        let mut names = HashSet::new();
        let mut bindings = Vec::new();
        for (index, decl) in self.declarations.iter().enumerate() {
            if decl.parse_order > parse_order || !names.insert(decl.name) {
                continue;
            }
            bindings.push(LocalBinding {
                name: decl.name,
                declaration: Some(index),
                definition: self.definitions.iter().position(|definition| {
                    definition.dimension == dimension && definition.name == decl.name
                }),
            });
        }
        for (index, definition) in self.definitions.iter().enumerate() {
            if definition.dimension != dimension
                || definition.end_order > parse_order
                || !names.insert(definition.name)
            {
                continue;
            }
            bindings.push(LocalBinding {
                name: definition.name,
                declaration: definition.declaration,
                definition: Some(index),
            });
        }
        bindings
    }
}

/// Whether a name can be used for a local declaration and definition target.
/// The pin's symbol production permits some keyword tokens; reserved functions,
/// expression operators, and non-symbol statement/block tokens remain excluded.
pub fn is_local_name(name: &str) -> bool {
    crate::parser::is_model_local_name(name)
}

/// Current aggregate or dimension model body, including incomplete input.
/// `None` means outside a model body; `Some(None)` is the aggregate model.
pub fn scope_at_order(model: &Model, order: usize) -> Option<Option<Name>> {
    let statement = model.statements.iter().find(|statement| {
        if !matches!(statement.name.as_str(), "model" | "model_replace") {
            return false;
        }
        let body_end = statement
            .token_range
            .clone()
            .rev()
            .find(|&index| {
                model.expanded_tokens.get(index).is_some_and(|token| {
                    token.kind == crate::lexer::TokenKind::Ident
                        && token.text(&model.source).eq_ignore_ascii_case("end")
                })
            })
            .unwrap_or_else(|| {
                statement.token_range.end
                    + usize::from(
                        !statement.complete
                            && model
                                .expanded_tokens
                                .get(statement.token_range.end)
                                .is_some_and(|token| token.kind == crate::lexer::TokenKind::Eof),
                    )
            });
        order >= statement.opener_range.end && order < body_end
    })?;
    Some(statement.dimension.as_ref().and_then(|name| {
        model
            .heterogeneous_models
            .iter()
            .find(|block| model.name(block.dimension) == name)
            .map(|block| block.dimension)
    }))
}
