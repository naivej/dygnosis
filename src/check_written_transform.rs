//! Transform refusals whose cause is provable in the written model.
//!
//! These are deliberately narrower than Dynare's rewritten-tree checks. The
//! first result follows the pinned transformPass order.

use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, Severity};
use crate::expr::{BinOp, ExprId, ExprKind};
use crate::intern::Name;
use crate::model::{Equation, Model};
use crate::span::Span;

pub(crate) fn check_early(model: &Model) -> Vec<Diagnostic> {
    if crate::check_writing::model_structure_incomplete(model) {
        return Vec::new();
    }
    if let Some(zero) = direct_constant_denominator(model) {
        return vec![zero];
    }
    direct_unused_endogenous(model)
}

pub(crate) fn check_late(model: &Model) -> Vec<Diagnostic> {
    if crate::check_writing::model_structure_incomplete(model) {
        return Vec::new();
    }
    if let Some(expectation) = direct_partial_information_expectation(model) {
        return vec![expectation];
    }
    if let Some(count) = plain_aggregate_count(model) {
        return vec![count];
    }
    plain_heterogeneous_count(model).into_iter().collect()
}

fn direct_unused_endogenous(model: &Model) -> Vec<Diagnostic> {
    // The official pass substitutes # locals before this check and excludes
    // aggregate names used by heterogeneous bodies. Read those written uses,
    // and keep unresolved rewrites, surgery, and standalone BVAR out.
    if (model.model_block.is_none() && !model.heterogeneous_models.is_empty())
        || model.bvar_present
        || !model.policy_commands.is_empty()
        || !model.planner_objective_spans.is_empty()
        || !model.ramsey_constraints.is_empty()
        || !model.model_local_variables.is_empty()
        || !model.equation_surgery.is_empty()
        || !model.var_removed.is_empty()
        || !model
            .equations
            .iter()
            .chain(
                model
                    .heterogeneous_models
                    .iter()
                    .flat_map(|block| block.equations.iter()),
            )
            .all(|eq| plain_equation(model, eq))
    {
        return Vec::new();
    }
    let used: HashSet<Name> = model
        .equations
        .iter()
        .chain(
            model
                .heterogeneous_models
                .iter()
                .flat_map(|block| block.equations.iter()),
        )
        .flat_map(|eq| model.ident_refs(eq))
        .map(|reference| reference.name)
        .collect();
    model
        .final_endogenous()
        .into_iter()
        .filter(|decl| !used.contains(&decl.name))
        .map(|decl| {
            Diagnostic::new(
                decl.span,
                Severity::Error,
                "E186",
                format!("{} not used in the model block", model.name(decl.name)),
            )
        })
        .collect()
}

/// A bounded branch of `DynamicModel::simplifyEquations`: one unique direct
/// finite literal `x = C` and a whole left- or right-hand side `N/(x-C)`.
/// `N` is a nonzero finite literal or a single name. Earlier divisions that
/// are not this form withhold E189.
fn direct_constant_denominator(model: &Model) -> Option<Diagnostic> {
    if !model.equation_surgery.is_empty()
        || !model.var_removed.is_empty()
        || model.equations.iter().any(|eq| {
            eq.is_local
                || eq.static_tag
                || eq.complementarity.is_some()
                || eq.tag_map.contains_key("bind")
                || eq.tag_map.contains_key("relax")
        })
    {
        return None;
    }
    let mut lhs_counts = HashMap::<Name, usize>::new();
    for eq in &model.equations {
        if let Some(lhs) = eq.lhs_expr {
            if let ExprKind::Ident {
                name, timing: 0, ..
            } = model.exprs.get(lhs).kind
            {
                *lhs_counts.entry(name).or_default() += 1;
            }
        }
    }
    let mut constants = HashMap::<Name, f64>::new();
    for eq in &model.equations {
        let (Some(lhs), Some(rhs)) = (eq.lhs_expr, eq.rhs_expr) else {
            continue;
        };
        let ExprKind::Ident {
            name, timing: 0, ..
        } = model.exprs.get(lhs).kind
        else {
            continue;
        };
        if lhs_counts.get(&name) == Some(&1)
            && model.endogenous.iter().any(|decl| decl.name == name)
        {
            if let Some(value) = finite_number_token(model, rhs) {
                constants.insert(name, value);
            }
        }
    }
    for (index, eq) in model.equations.iter().enumerate() {
        for side in [eq.lhs_expr, eq.rhs_expr].into_iter().flatten() {
            if let Some(span) = direct_zero_denominator(model, side, &constants) {
                return Some(Diagnostic::new(
                    span,
                    Severity::Error,
                    "E189",
                    format!(
                        "Division by zero when substituting constants in equation {}",
                        index + 1
                    ),
                ));
            }
            if contains_division(model, side) {
                return None;
            }
        }
    }
    None
}

/// A direct nonnegative numeric token, not unary minus or folded arithmetic.
/// The pin does not treat `x=-1` as a constant equation for this pass.
fn finite_number_token(model: &Model, id: ExprId) -> Option<f64> {
    let expr = model.exprs.get(id);
    if !matches!(expr.kind, ExprKind::Number) {
        return None;
    }
    expr.interned.filter(|value| value.is_finite())
}

/// Nonzero finite literal, or one written name. A literal zero is folded by
/// the pin before the denominator is tested, so it is outside this proof.
fn direct_nonzero_numerator(model: &Model, id: ExprId) -> bool {
    match &model.exprs.get(id).kind {
        ExprKind::Number => finite_number_token(model, id).is_some_and(|value| value != 0.0),
        ExprKind::Ident { .. } => true,
        _ => false,
    }
}

fn direct_zero_denominator(
    model: &Model,
    id: ExprId,
    constants: &HashMap<Name, f64>,
) -> Option<Span> {
    let ExprKind::Binary {
        op: BinOp::Div,
        lhs,
        rhs,
    } = model.exprs.get(id).kind
    else {
        return None;
    };
    if !direct_nonzero_numerator(model, lhs) {
        return None;
    }
    let denominator = model.exprs.get(rhs);
    let ExprKind::Binary {
        op: BinOp::Sub,
        lhs,
        rhs: written_constant,
    } = denominator.kind
    else {
        return None;
    };
    let value = finite_number_token(model, written_constant)?;
    let ExprKind::Ident {
        name, timing: 0, ..
    } = model.exprs.get(lhs).kind
    else {
        return None;
    };
    (constants.get(&name) == Some(&value)).then_some(denominator.span)
}

fn contains_division(model: &Model, id: ExprId) -> bool {
    match &model.exprs.get(id).kind {
        ExprKind::Binary { op: BinOp::Div, .. } => true,
        ExprKind::Binary { lhs, rhs, .. } => {
            contains_division(model, *lhs) || contains_division(model, *rhs)
        }
        ExprKind::Unary { arg, .. }
        | ExprKind::SteadyState { arg }
        | ExprKind::Expectation { arg, .. } => contains_division(model, *arg),
        ExprKind::Call { args, .. } => args.iter().any(|arg| contains_division(model, *arg)),
        ExprKind::Ident { .. } | ExprKind::Number | ExprKind::String | ExprKind::Error => false,
    }
}

fn direct_partial_information_expectation(model: &Model) -> Option<Diagnostic> {
    if !model.partial_information
        || !model.model_local_variables.is_empty()
        || model.equations.iter().any(|eq| eq.is_local)
    {
        return None;
    }
    for (equation_index, eq) in model.equations.iter().enumerate() {
        for root in [eq.lhs_expr, eq.rhs_expr].into_iter().flatten() {
            if let Some(span) = bad_expectation(model, root, equation_index) {
                return Some(Diagnostic::new(
                    span,
                    Severity::Error,
                    "E190",
                    "In Partial Information models, EXPECTATION(0)(X) can only be used when X is a single variable.",
                ));
            }
        }
    }
    None
}

fn bad_expectation(model: &Model, id: ExprId, equation_index: usize) -> Option<Span> {
    let node = model.exprs.get(id);
    match &node.kind {
        ExprKind::Expectation { shift, arg } => {
            if *shift == 0 {
                if let Some((left, right)) = direct_nonvariable_argument(model, *arg) {
                    let other_definition = model.equations.iter().enumerate().any(|(i, eq)| {
                        i != equation_index
                            && eq.lhs_expr.is_some_and(|lhs| {
                                matches!(&model.exprs.get(lhs).kind,
                                    ExprKind::Ident { name, timing: 0, .. }
                                    if *name == left || *name == right)
                            })
                    });
                    if !other_definition {
                        return Some(node.span);
                    }
                }
            }
            bad_expectation(model, *arg, equation_index)
        }
        ExprKind::Unary { arg, .. } | ExprKind::SteadyState { arg } => {
            bad_expectation(model, *arg, equation_index)
        }
        ExprKind::Binary { lhs, rhs, .. } => bad_expectation(model, *lhs, equation_index)
            .or_else(|| bad_expectation(model, *rhs, equation_index)),
        ExprKind::Call { args, .. } => args
            .iter()
            .find_map(|arg| bad_expectation(model, *arg, equation_index)),
        ExprKind::Ident { .. } | ExprKind::Number | ExprKind::String | ExprKind::Error => None,
    }
}

/// Two distinct written variables in one binary node cannot be reduced to a
/// single VariableNode by the pinned constant/identity simplifications.
fn direct_nonvariable_argument(model: &Model, id: ExprId) -> Option<(Name, Name)> {
    let ExprKind::Binary {
        op: BinOp::Add,
        lhs,
        rhs,
    } = &model.exprs.get(id).kind
    else {
        return None;
    };
    let ExprKind::Ident { name: left, .. } = &model.exprs.get(*lhs).kind else {
        return None;
    };
    let ExprKind::Ident { name: right, .. } = &model.exprs.get(*rhs).kind else {
        return None;
    };
    (*left != *right).then_some((*left, *right))
}

fn plain_aggregate_count(model: &Model) -> Option<Diagnostic> {
    if model.model_block.is_none()
        || model.bvar_present
        || !model.policy_commands.is_empty()
        || !model.heterogeneous_models.is_empty()
        || !plain_count_inputs(model)
    {
        return None;
    }
    let n_eq = model.equations.len();
    let n_endo = model.final_endogenous().len();
    (n_eq != n_endo).then(|| {
        Diagnostic::new(
            model.model_block.unwrap(),
            Severity::Error,
            "E188",
            format!("There are {n_eq} equations but {n_endo} endogenous variables!"),
        )
    })
}

fn plain_heterogeneous_count(model: &Model) -> Option<Diagnostic> {
    if model.heterogeneous_models.is_empty() || !plain_count_inputs(model) {
        return None;
    }
    let mut counts = HashMap::<Name, (usize, Span)>::new();
    for block in &model.heterogeneous_models {
        let row = counts.entry(block.dimension).or_insert((0, block.span));
        row.0 += block.equations.len();
    }
    let mut seen = HashSet::new();
    for block in &model.heterogeneous_models {
        let dimension = block.dimension;
        if !seen.insert(dimension) {
            continue;
        }
        let n_endo = model
            .endogenous
            .iter()
            .filter(|decl| decl.heterogeneity.is_some_and(|(dim, _)| dim == dimension))
            .map(|decl| decl.name)
            .collect::<HashSet<_>>()
            .len();
        let (n_eq, span) = counts[&dimension];
        if n_eq != n_endo {
            let name = model.name(dimension);
            return Some(Diagnostic::new(
                span,
                Severity::Error,
                "E192",
                format!("There are {n_eq} equations but {n_endo} endogenous variables in the model for heterogeneity dimension '{name}'!"),
            ).with_model_dimension(name));
        }
    }
    None
}

fn plain_count_inputs(model: &Model) -> bool {
    if !model.equation_surgery.is_empty()
        || !model.var_removed.is_empty()
        || !model.model_local_variables.is_empty()
        || !model.semi_structural_commands.is_empty()
        || !model.named_model_operators.is_empty()
        || !model.trend_vars.is_empty()
        || !model.nonstationary_vars.is_empty()
        || model.endogenous.iter().any(|decl| decl.log_transform)
        || model.differentiate_forward_vars
    {
        return false;
    }
    model
        .equations
        .iter()
        .chain(
            model
                .heterogeneous_models
                .iter()
                .flat_map(|block| block.equations.iter()),
        )
        .all(|eq| plain_equation(model, eq))
}

fn plain_equation(model: &Model, eq: &Equation) -> bool {
    if eq.is_local
        || eq.static_tag
        || eq.dynamic_tag
        || eq.complementarity.is_some()
        || eq.tag_map.contains_key("bind")
        || eq.tag_map.contains_key("relax")
        || eq.lhs_expr.is_none()
        || (eq.rhs_expr.is_none() && !eq.rhs.trim().is_empty())
    {
        return false;
    }
    [eq.lhs_expr, eq.rhs_expr]
        .into_iter()
        .flatten()
        .all(|id| plain_expr(model, id))
}

fn plain_expr(model: &Model, id: ExprId) -> bool {
    match &model.exprs.get(id).kind {
        ExprKind::Number => true,
        ExprKind::Ident { name, timing, .. } => {
            if model.final_exogenous(*name) {
                *timing == 0
            } else {
                (-1..=1).contains(timing)
            }
        }
        ExprKind::Unary { arg, .. } => plain_expr(model, *arg),
        ExprKind::Binary { lhs, rhs, .. } => plain_expr(model, *lhs) && plain_expr(model, *rhs),
        ExprKind::String
        | ExprKind::Call { .. }
        | ExprKind::SteadyState { .. }
        | ExprKind::Expectation { .. }
        | ExprKind::Error => false,
    }
}
