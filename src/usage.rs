//! Private Check usage tree. The written arena and its references stay intact.
//!
//! Dynare 7.2 DataTree constructors simplify while parsing, before collection.
//! Intern this small parallel graph once; then visit each live node once. Local
//! definitions are collection edges, not replacements of written local symbols.

use std::collections::{HashMap, HashSet};

use crate::constructor::{ConstructorContext, ConstructorTree, DataTreeScope, Node, NodeId};
use crate::expr::{ExprId, ExprKind};
use crate::intern::Name;
use crate::model::{
    Equation, Model, PacTargetComponentRow, PacTargetInfoRow, SemiStructuralKind,
    SemiStructuralValue,
};

pub(crate) struct Usage<'a> {
    model: &'a Model,
    tree: ConstructorTree,
    normalized: HashMap<ExprId, NodeId>,
    locals: HashMap<(DataTreeScope, Name), ExprId>,
}

impl<'a> Usage<'a> {
    pub(crate) fn new(model: &'a Model) -> Self {
        // AddLocalVariable belongs to one DataTree. A heterogeneous body may
        // define the same local name as the aggregate model with another RHS.
        let mut locals = HashMap::new();
        let mut contexts = HashMap::new();
        let trees = std::iter::once((DataTreeScope::Dynamic, model.equations.as_slice())).chain(
            model.heterogeneous_models.iter().map(|block| {
                (
                    DataTreeScope::Heterogeneous(block.dimension),
                    block.equations.as_slice(),
                )
            }),
        );
        for (context, equations) in trees {
            for eq in equations {
                if eq.is_local && model.model_local_action_attempted(eq) {
                    if let (Some(lhs), Some(rhs)) = (eq.lhs_expr, eq.rhs_expr) {
                        if let ExprKind::Ident { name, .. } = model.exprs.get(lhs).kind {
                            locals.entry((context, name)).or_insert(rhs);
                        }
                    }
                }
                let mut pending: Vec<_> =
                    [eq.lhs_expr, eq.rhs_expr].into_iter().flatten().collect();
                while let Some(id) = pending.pop() {
                    if contexts.insert(id, context).is_some() {
                        continue;
                    }
                    match &model.exprs.get(id).kind {
                        ExprKind::Unary { arg, .. }
                        | ExprKind::SteadyState { arg }
                        | ExprKind::Expectation { arg, .. } => pending.push(*arg),
                        ExprKind::Binary { lhs, rhs, .. } => pending.extend([*lhs, *rhs]),
                        ExprKind::Call { args, .. } => pending.extend(args),
                        _ => {}
                    }
                }
            }
        }
        let mut pending: Vec<_> = model
            .steady_state_equations
            .iter()
            .filter_map(|equation| {
                equation
                    .rhs_expr
                    .map(|expr| (DataTreeScope::SteadyState, expr))
            })
            .chain(
                model
                    .planner_objective_expr
                    .map(|expr| (DataTreeScope::Planner(0), expr)),
            )
            .collect();
        while let Some((context, id)) = pending.pop() {
            if contexts.insert(id, context).is_some() {
                continue;
            }
            match &model.exprs.get(id).kind {
                ExprKind::Unary { arg, .. }
                | ExprKind::SteadyState { arg }
                | ExprKind::Expectation { arg, .. } => pending.push((context, *arg)),
                ExprKind::Binary { lhs, rhs, .. } => {
                    pending.extend([(context, *lhs), (context, *rhs)])
                }
                ExprKind::Call { args, .. } => {
                    pending.extend(args.iter().map(|arg| (context, *arg)))
                }
                _ => {}
            }
        }
        let mut usage = Self {
            model,
            tree: ConstructorTree::default(),
            normalized: HashMap::new(),
            locals,
        };
        // The arena allocates children first. No recursive AST walk is needed,
        // and normalization work is bounded by the written arena size.
        let mut pending: Vec<_> = model
            .steady_state_equations
            .iter()
            .filter_map(|equation| equation.lhs_expr)
            .chain(
                model
                    .written_equations
                    .iter()
                    .filter(|row| row.equation.is_local)
                    .filter_map(|row| row.equation.lhs_expr),
            )
            .collect();
        let mut binding_targets = HashSet::new();
        while let Some(id) = pending.pop() {
            if !binding_targets.insert(id) {
                continue;
            }
            match &model.exprs.get(id).kind {
                ExprKind::Unary { arg, .. }
                | ExprKind::SteadyState { arg }
                | ExprKind::Expectation { arg, .. } => pending.push(*arg),
                ExprKind::Binary { lhs, rhs, .. } => pending.extend([*lhs, *rhs]),
                ExprKind::Call { args, .. } => pending.extend(args),
                _ => {}
            }
        }
        for (id, expr) in model.exprs.iter() {
            // Path values have a separate persistent DataTree and never form
            // model usage roots. Parse already owns their constructor facts.
            if model.constructor_scopes.get(&id) == Some(&DataTreeScope::ShockPaths) {
                continue;
            }
            // Targets are written navigation nodes, not constructor reads.
            if binding_targets.contains(&id) {
                continue;
            }
            let literal = matches!(expr.kind, ExprKind::Number).then(|| {
                let text = model
                    .numeric_literal_texts
                    .get(&id)
                    .map(String::as_str)
                    .unwrap_or_else(|| {
                        &model.source[expr.span.start as usize..expr.span.end as usize]
                    });
                (
                    text,
                    model
                        .numeric_literals
                        .get(&id)
                        .copied()
                        .or_else(|| text.parse().ok()),
                )
            });
            let context = model
                .constructor_scopes
                .get(&id)
                .copied()
                .or_else(|| contexts.get(&id).copied())
                .unwrap_or(DataTreeScope::General);
            let ident_value = if let ExprKind::Ident { name, .. } = expr.kind {
                usage
                    .locals
                    .get(&(context, name))
                    .and_then(|rhs| usage.normalized.get(rhs))
                    .and_then(|n| usage.tree.value(*n))
            } else {
                None
            };
            let normalized = usage.tree.construct(
                id,
                &expr.kind,
                &usage.normalized,
                literal,
                ConstructorContext {
                    scope: context,
                    ident_value,
                },
                |name| model.name(name).to_string(),
            );
            usage.normalized.insert(id, normalized);
        }
        usage
    }

    pub(crate) fn model_names(&self) -> HashSet<Name> {
        self.names(model_roots(self.model))
    }

    pub(crate) fn exogenous_exemptions(&self) -> HashSet<Name> {
        let command_growth = self
            .model
            .semi_structural_commands
            .iter()
            .filter(|command| command.kind == SemiStructuralKind::PacModel)
            .flat_map(|command| &command.options)
            .filter_map(|option| match &option.value {
                SemiStructuralValue::Expression(expression)
                    if option.name.eq_ignore_ascii_case("growth") =>
                {
                    expression.expr
                }
                _ => None,
            });
        let component_growth = self
            .model
            .pac_target_info
            .iter()
            .flat_map(|block| &block.rows)
            .filter_map(|row| match row {
                PacTargetInfoRow::Component(component) => Some(component),
                _ => None,
            })
            .flat_map(|component| &component.rows)
            .filter_map(|row| match row {
                PacTargetComponentRow::Growth(expression) => expression.expr,
                _ => None,
            });
        let mut names = self.names(command_growth.chain(component_growth));
        names.extend(self.model.varexobs.iter().map(|observed| observed.name));
        names
    }

    pub(crate) fn parameter_names(&self) -> HashSet<Name> {
        let mut names = self.names(
            model_roots(self.model).chain(
                self.model
                    .steady_state_equations
                    .iter()
                    .filter_map(|eq| eq.rhs_expr),
            ),
        );
        for equation in &self.model.steady_state_equations {
            names.extend(
                equation
                    .steady_state_targets
                    .iter()
                    .map(|target| target.name),
            );
        }
        names
    }

    pub(crate) fn names(&self, roots: impl IntoIterator<Item = ExprId>) -> HashSet<Name> {
        let mut names = HashSet::new();
        let mut seen = HashSet::new();
        let mut pending: Vec<_> = roots
            .into_iter()
            .filter_map(|id| self.normalized.get(&id).copied())
            .collect();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            match self.tree.node(id) {
                Node::Ident(name, _, context) => {
                    names.insert(*name);
                    if let Some(definition) = self
                        .locals
                        .get(&(*context, *name))
                        .and_then(|expr| self.normalized.get(expr))
                    {
                        pending.push(*definition);
                    }
                }
                Node::Neg(arg) | Node::SteadyState(arg) | Node::Expectation(_, arg) => {
                    pending.push(*arg)
                }
                Node::Binary(_, lhs, rhs) => pending.extend([*lhs, *rhs]),
                Node::Builtin(_, args) | Node::Call(_, args) => pending.extend(args),
                Node::Number(_) | Node::Opaque(_) | Node::PathNamespace(_) => {}
            }
        }
        names
    }
}

fn model_roots(model: &Model) -> impl Iterator<Item = ExprId> + '_ {
    model
        .equations
        .iter()
        .chain(model.heterogeneous_models.iter().flat_map(|b| &b.equations))
        .filter(|eq| !eq.is_local)
        .flat_map(|eq: &Equation| [eq.lhs_expr, eq.rhs_expr].into_iter().flatten())
}
