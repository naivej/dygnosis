//! Named PAC/VAR context required by an extracted equation fragment.
//!
//! Accepted parser receipts select definitions and expanded tokens. A bounded
//! work list closes named references without treating model names as variables.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Range;

use super::{EqRef, Model, OmittedContext, eq_at, is_pac_name};
use crate::engine::language::expr::{ExprId, ExprKind};
use crate::engine::language::intern::Name;
use crate::engine::language::model::{
    NamedModelOperatorKind, PacTargetComponentRow, PacTargetInfoRow, SemiStructuralCommand,
    SemiStructuralKind, SemiStructuralValue, Statement, WrittenExpression,
};
use crate::engine::language::parser::join_lexemes;

const MAX_VISITS: usize = 1_000_000;
const MAX_CONTEXT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Default)]
struct NamedIndex {
    commands: HashMap<(SemiStructuralKind, Name), Vec<usize>>,
    targets: HashMap<Name, Vec<usize>>,
    tags: HashMap<String, Vec<EqRef>>,
    direct_lhs: HashMap<Name, Vec<EqRef>>,
    aggregate: Vec<EqRef>,
}

struct AcceptedParents {
    // usize::MAX marks overlapping parser parents; it is never an index.
    owners: Vec<Option<usize>>,
    accepted: Vec<bool>,
}

fn refuse_owner(parents: &mut AcceptedParents, order: usize) {
    if let Some(Some(index)) = parents.owners.get(order)
        && let Some(accepted) = parents.accepted.get_mut(*index)
    {
        *accepted = false;
    }
}

#[derive(Default)]
pub(super) struct NamedContext {
    commands: BTreeSet<usize>,
    targets: BTreeSet<usize>,
    pub equations: BTreeSet<EqRef>,
    pub expressions: Vec<(ExprId, usize)>,
    pub symbols: BTreeSet<String>,
    pub symbol_reads: Vec<(Name, usize)>,
    visited_expressions: HashSet<ExprId>,
    index: Option<NamedIndex>,
    pac_equations: Option<HashMap<Name, BTreeSet<EqRef>>>,
    parents: Option<AcceptedParents>,
    visits: usize,
    rendered_bytes: usize,
    whole_aggregate_target: bool,
    whole_aggregate_pac: bool,
}

impl NamedContext {
    pub(super) fn charge(&mut self) -> Result<(), String> {
        if crate::engine::control::cancelled() {
            return Err("extraction was cancelled while closing named model dependencies".into());
        }
        self.visits += 1;
        if self.visits > MAX_VISITS {
            return Err(format!(
                "named model dependency closure exceeded {MAX_VISITS} visits; no fragment was returned"
            ));
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty() && self.targets.is_empty()
    }

    pub(super) fn text_work(&mut self, text: &str) -> Result<(), String> {
        for _ in text.as_bytes().chunks(64) {
            self.charge()?;
        }
        Ok(())
    }

    pub(super) fn rendered(&mut self, text: &str) -> Result<(), String> {
        self.text_work(text)?;
        self.rendered_bytes = self.rendered_bytes.saturating_add(text.len() + 2);
        if self.rendered_bytes > MAX_CONTEXT_BYTES {
            return Err(format!(
                "named model fragment exceeds {MAX_CONTEXT_BYTES} bytes; no fragment was returned"
            ));
        }
        Ok(())
    }

    fn ensure_parents(&mut self, model: &Model) -> Result<(), String> {
        if self.parents.is_some() {
            return Ok(());
        }
        let mut parents = AcceptedParents {
            owners: Vec::new(),
            accepted: Vec::new(),
        };
        // Build one token-order index. Repeated macro spans cannot choose a parent.
        for _ in &model.expanded_tokens {
            self.charge()?;
            parents.owners.push(None);
        }
        for (index, statement) in model.statements.iter().enumerate() {
            self.charge()?;
            parents.accepted.push(
                statement.complete
                    && !statement.native
                    && !model.constructor_refused_statements.contains(&statement.id),
            );
            for order in statement.token_range.clone() {
                self.charge()?;
                let owner = parents.owners.get_mut(order).ok_or_else(|| {
                    "a parser parent has an invalid expanded token range".to_string()
                })?;
                *owner = Some(match *owner {
                    None => index,
                    Some(_) => usize::MAX,
                });
            }
        }
        let mut issue_orders: HashMap<_, Vec<_>> = HashMap::new();
        for &(span, order) in &model.parse_issue_orders {
            self.charge()?;
            issue_orders.entry(span).or_default().push(order);
        }
        for issue in &model.parse_issues {
            self.charge()?;
            if let Some(orders) = issue_orders.get(&issue.span) {
                for &order in orders {
                    self.charge()?;
                    refuse_owner(&mut parents, order);
                }
            } else {
                // Old diagnostics without an execution receipt retain a conservative span guard.
                for (index, statement) in model.statements.iter().enumerate() {
                    self.charge()?;
                    if super::overlaps(statement.span, issue.span) {
                        parents.accepted[index] = false;
                    }
                }
            }
        }
        for refusal in &model.shape_refuses {
            self.charge()?;
            if let Some(range) = &refusal.parse_execution {
                for order in range.clone() {
                    self.charge()?;
                    refuse_owner(&mut parents, order);
                }
            }
            if let Some(order) = refusal.parse_order {
                refuse_owner(&mut parents, order);
            }
        }
        for &(span, _, _) in &model.const_fold_errors {
            self.charge()?;
            for (index, statement) in model.statements.iter().enumerate() {
                self.charge()?;
                if statement.span.start <= span.start && span.start < statement.span.end {
                    parents.accepted[index] = false;
                }
            }
        }
        self.parents = Some(parents);
        Ok(())
    }

    /// Cached source proof shares the closure/render allowance and cancellation.
    pub(super) fn accepted_parent<'a>(
        &mut self,
        model: &'a Model,
        order: usize,
        range: &Range<usize>,
    ) -> Result<&'a Statement, String> {
        self.charge()?;
        self.ensure_parents(model)?;
        let parents = self.parents.as_ref().unwrap();
        let index = parents
            .owners
            .get(order)
            .copied()
            .flatten()
            .filter(|&index| index != usize::MAX)
            .ok_or_else(|| {
                "a required named-model dependency has no unique accepted parser parent".to_string()
            })?;
        let statement = &model.statements[index];
        if statement.token_range.start > range.start || range.end > statement.token_range.end {
            return Err(
                "a required named-model dependency has no unique accepted parser parent".into(),
            );
        }
        if !parents.accepted[index] {
            return Err(format!(
                "required {} context was not completely accepted; no fragment was returned",
                statement.name
            ));
        }
        Ok(statement)
    }

    fn ensure_index(&mut self, model: &Model) -> Result<(), String> {
        if self.index.is_some() {
            return Ok(());
        }
        let mut index = NamedIndex::default();
        for (position, command) in model.semi_structural_commands.iter().enumerate() {
            self.charge()?;
            // Count the option lookup too, including malformed declarations.
            for _ in &command.options {
                self.charge()?;
            }
            if let Some(name) = symbol_option(command, "model_name") {
                index
                    .commands
                    .entry((command.kind, name))
                    .or_default()
                    .push(position);
            }
        }
        for (position, target) in model.pac_target_info.iter().enumerate() {
            self.charge()?;
            index.targets.entry(target.name).or_default().push(position);
        }
        for (position, equation) in model.equations.iter().enumerate() {
            self.charge()?;
            if equation.is_local {
                continue;
            }
            let place = EqRef {
                block: None,
                index: position,
            };
            index.aggregate.push(place);
            if let Some(tag) = equation.tag_map.get("name") {
                index.tags.entry(tag.clone()).or_default().push(place);
            }
            if let Some(ExprKind::Ident {
                name, timing: 0, ..
            }) = equation.lhs_expr.map(|id| &model.exprs.get(id).kind)
            {
                index.direct_lhs.entry(*name).or_default().push(place);
            }
        }
        self.index = Some(index);
        Ok(())
    }

    fn include_equation_set(&mut self, equations: &[EqRef]) -> Result<(), String> {
        for &place in equations {
            self.charge()?;
            self.equations.insert(place);
        }
        Ok(())
    }

    /// Grow from the selected equations and their ordinary model-local closure.
    /// A later call examines only new expression nodes or new named definitions.
    pub fn grow(&mut self, model: &Model, retained: &[EqRef]) -> Result<bool, String> {
        let before = (
            self.commands.len(),
            self.targets.len(),
            self.equations.len(),
            self.expressions.len(),
            self.symbols.len(),
        );
        let mut pending = Vec::new();
        for &place in retained {
            self.charge()?;
            let equation = eq_at(model, place);
            pending.extend(equation.lhs_expr);
            pending.extend(equation.rhs_expr);
        }
        pending.extend(self.expressions.iter().map(|(id, _)| *id));
        while let Some(id) = pending.pop() {
            self.charge()?;
            if !self.visited_expressions.insert(id) {
                continue;
            }
            match &model.exprs.get(id).kind {
                ExprKind::Unary { arg, .. }
                | ExprKind::SteadyState { arg }
                | ExprKind::Expectation { arg, .. } => pending.push(*arg),
                ExprKind::Binary { lhs, rhs, .. } => pending.extend([*lhs, *rhs]),
                ExprKind::Call { callee, args } => {
                    pending.extend(args.iter().copied());
                    if !is_pac_name(model.name(*callee)) {
                        continue;
                    }
                    let operator = model
                        .named_model_operator_exprs
                        .get(&id)
                        .and_then(|index| model.named_model_operators.get(*index))
                        .ok_or_else(|| {
                            "a retained PAC or VAR operator has no accepted named-model receipt"
                                .to_string()
                        })?;
                    let kind = match operator.kind {
                        NamedModelOperatorKind::VarExpectation => {
                            SemiStructuralKind::VarExpectationModel
                        }
                        NamedModelOperatorKind::PacExpectation
                        | NamedModelOperatorKind::PacTargetNonstationary => {
                            SemiStructuralKind::PacModel
                        }
                    };
                    self.include_model(model, kind, operator.name, &mut pending)?;
                    if operator.kind == NamedModelOperatorKind::PacTargetNonstationary
                        && !self
                            .index
                            .as_ref()
                            .unwrap()
                            .targets
                            .contains_key(&operator.name)
                    {
                        return Err(format!(
                            "PAC model `{}` has no pac_target_info context for pac_target_nonstationary",
                            model.name(operator.name)
                        ));
                    }
                }
                ExprKind::Number
                | ExprKind::String
                | ExprKind::Ident { .. }
                | ExprKind::PathNamespace { .. } => {}
                ExprKind::Error => {}
            }
        }
        Ok(before
            != (
                self.commands.len(),
                self.targets.len(),
                self.equations.len(),
                self.expressions.len(),
                self.symbols.len(),
            ))
    }

    fn include_model(
        &mut self,
        model: &Model,
        kind: SemiStructuralKind,
        name: Name,
        pending: &mut Vec<ExprId>,
    ) -> Result<(), String> {
        self.charge()?;
        self.ensure_index(model)?;
        let indices = self
            .index
            .as_ref()
            .unwrap()
            .commands
            .get(&(kind, name))
            .map(Vec::as_slice)
            .unwrap_or_default();
        let [index] = indices else {
            let cause = if indices.is_empty() {
                "has no definition"
            } else {
                "has more than one definition"
            };
            return Err(format!(
                "PAC or VAR model `{}` {cause}; its required context cannot be selected",
                model.name(name)
            ));
        };
        if !self.commands.insert(*index) {
            return Ok(());
        }
        let command = &model.semi_structural_commands[*index];
        self.accepted_parent(model, command.parse_order, &command.active_tokens)?;
        for required in match kind {
            SemiStructuralKind::VarModel => &["model_name", "eqtags"][..],
            SemiStructuralKind::TrendComponentModel => &["model_name", "eqtags", "targets"][..],
            SemiStructuralKind::VarExpectationModel => {
                &["model_name", "auxiliary_model_name", "horizon"][..]
            }
            SemiStructuralKind::PacModel => &["model_name", "discount"][..],
        } {
            if !command
                .options
                .iter()
                .any(|option| option.name.eq_ignore_ascii_case(required))
            {
                return Err(format!(
                    "named model `{}` has no required `{required}` option",
                    model.name(name)
                ));
            }
        }
        let mut keys = HashSet::new();
        for option in &command.options {
            self.charge()?;
            let key = option.name.to_ascii_lowercase();
            let repeated = match kind {
                SemiStructuralKind::VarExpectationModel => {
                    matches!(key.as_str(), "expression" | "discount")
                }
                SemiStructuralKind::PacModel => {
                    matches!(key.as_str(), "growth" | "auxname" | "kind")
                }
                _ => false,
            };
            if !keys.insert(key.clone()) && !repeated {
                return Err(format!(
                    "named model `{}` repeats the `{key}` option",
                    model.name(name)
                ));
            }
            match &option.value {
                SemiStructuralValue::Expression(expression) => {
                    self.include_expression(expression, command.parse_order, pending)?
                }
                SemiStructuralValue::Symbol { name, .. }
                    if matches!(key.as_str(), "variable" | "discount") =>
                {
                    self.symbols.insert(model.name(*name).to_string());
                    self.symbol_reads.push((*name, command.parse_order));
                }
                SemiStructuralValue::Tags(tags) => {
                    for (tag, _) in tags {
                        self.charge()?;
                        let matches = self
                            .index
                            .as_ref()
                            .unwrap()
                            .tags
                            .get(tag)
                            .cloned()
                            .unwrap_or_default();
                        if matches.is_empty() {
                            return Err(format!(
                                "named model `{}` requires equation tag `{tag}`, which has no retained aggregate equation",
                                model.name(name)
                            ));
                        }
                        self.include_equation_set(&matches)?;
                    }
                }
                _ => {}
            }
        }
        if kind == SemiStructuralKind::VarExpectationModel {
            let variable = symbol_option(command, "variable").is_some();
            let expression = command
                .options
                .iter()
                .any(|option| option.name.eq_ignore_ascii_case("expression"));
            if variable == expression {
                return Err(format!(
                    "VAR expectation model `{}` requires exactly one variable or expression",
                    model.name(name)
                ));
            }
        }
        if let Some(auxiliary) = symbol_option(command, "auxiliary_model_name") {
            // The pin gives these two callers different namespace precedence.
            let preferences = if kind == SemiStructuralKind::PacModel {
                [
                    SemiStructuralKind::TrendComponentModel,
                    SemiStructuralKind::VarModel,
                ]
            } else {
                [
                    SemiStructuralKind::VarModel,
                    SemiStructuralKind::TrendComponentModel,
                ]
            };
            let auxiliary_kind = preferences.into_iter().find(|candidate| self.index.as_ref().unwrap().commands.contains_key(&(*candidate, auxiliary)))
                .ok_or_else(|| format!("named model `{}` requires auxiliary model `{}`, which has no retained definition", model.name(name), model.name(auxiliary)))?;
            self.include_model(model, auxiliary_kind, auxiliary, pending)?;
        }
        if kind == SemiStructuralKind::PacModel {
            let targets = self
                .index
                .as_ref()
                .unwrap()
                .targets
                .get(&name)
                .cloned()
                .unwrap_or_default();
            for index in targets {
                self.include_target(model, index, pending)?;
            }
            self.include_pac_equations(model, name)?;
        }
        Ok(())
    }

    fn include_expression(
        &mut self,
        expression: &WrittenExpression,
        order: usize,
        pending: &mut Vec<ExprId>,
    ) -> Result<(), String> {
        self.charge()?;
        let id = expression
            .expr
            .ok_or_else(|| "a required named-model expression has no retained tree".to_string())?;
        self.expressions.push((id, order));
        pending.push(id);
        Ok(())
    }

    fn include_target(
        &mut self,
        model: &Model,
        index: usize,
        pending: &mut Vec<ExprId>,
    ) -> Result<(), String> {
        self.charge()?;
        if !self.targets.insert(index) {
            return Ok(());
        }
        let target = &model.pac_target_info[index];
        self.accepted_parent(model, target.parse_order, &target.active_tokens)?;
        for row in &target.rows {
            self.charge()?;
            match row {
                PacTargetInfoRow::Target(expression) => {
                    self.include_expression(expression, target.parse_order, pending)?;
                    // A direct written LHS proves a small closure. Otherwise keep
                    // the full aggregate context used by Dynare's transformations.
                    let direct = expression
                        .expr
                        .and_then(|id| match model.exprs.get(id).kind {
                            ExprKind::Ident {
                                name, timing: 0, ..
                            } => Some(name),
                            _ => None,
                        });
                    let matches = direct
                        .and_then(|name| {
                            self.index.as_ref().unwrap().direct_lhs.get(&name).cloned()
                        })
                        .unwrap_or_default();
                    if matches.is_empty() {
                        self.whole_aggregate_target = true;
                        let aggregate = self.index.as_ref().unwrap().aggregate.clone();
                        self.include_equation_set(&aggregate)?;
                    } else {
                        self.include_equation_set(&matches)?;
                    }
                }
                PacTargetInfoRow::Component(component) => {
                    self.include_expression(&component.component, target.parse_order, pending)?;
                    for row in &component.rows {
                        if let PacTargetComponentRow::Growth(expression) = row {
                            self.include_expression(expression, target.parse_order, pending)?;
                        }
                    }
                }
                PacTargetInfoRow::AuxnameTargetNonstationary { .. } => {}
            }
        }
        Ok(())
    }

    fn include_pac_equations(&mut self, model: &Model, name: Name) -> Result<(), String> {
        if self.pac_equations.is_none() {
            self.index_pac_equations(model)?;
        }
        let matches = self
            .pac_equations
            .as_ref()
            .unwrap()
            .get(&name)
            .cloned()
            .unwrap_or_default();
        if matches.is_empty() {
            // A PAC call held in a local can acquire its containing equation
            // only after inlining. Keeping the aggregate context preserves it.
            self.whole_aggregate_pac = true;
            let aggregate = self.index.as_ref().unwrap().aggregate.clone();
            self.include_equation_set(&aggregate)?;
        } else {
            for place in matches {
                self.charge()?;
                self.equations.insert(place);
            }
        }
        Ok(())
    }

    fn index_pac_equations(&mut self, model: &Model) -> Result<(), String> {
        let mut equations: HashMap<Name, BTreeSet<EqRef>> = HashMap::new();
        for (index, equation) in model
            .equations
            .iter()
            .enumerate()
            .filter(|(_, equation)| !equation.is_local)
        {
            self.charge()?;
            let mut pending: Vec<_> = equation
                .lhs_expr
                .into_iter()
                .chain(equation.rhs_expr)
                .collect();
            let mut seen = HashSet::new();
            while let Some(id) = pending.pop() {
                self.charge()?;
                if !seen.insert(id) {
                    continue;
                }
                if let Some(operator) = model
                    .named_model_operator_exprs
                    .get(&id)
                    .and_then(|index| model.named_model_operators.get(*index))
                    .filter(|operator| operator.kind == NamedModelOperatorKind::PacExpectation)
                {
                    equations
                        .entry(operator.name)
                        .or_default()
                        .insert(EqRef { block: None, index });
                }
                match &model.exprs.get(id).kind {
                    ExprKind::Unary { arg, .. }
                    | ExprKind::SteadyState { arg }
                    | ExprKind::Expectation { arg, .. } => pending.push(*arg),
                    ExprKind::Binary { lhs, rhs, .. } => pending.extend([*lhs, *rhs]),
                    ExprKind::Call { args, .. } => pending.extend(args.iter().copied()),
                    _ => {}
                }
            }
        }
        self.pac_equations = Some(equations);
        Ok(())
    }

    pub fn pieces(&mut self, model: &Model) -> Result<Vec<(usize, String)>, String> {
        let mut pieces = Vec::new();
        let mut bytes = 0usize;
        let ranges: Vec<_> = self
            .commands
            .iter()
            .map(|&index| {
                let command = &model.semi_structural_commands[index];
                (command.parse_order, command.active_tokens.clone())
            })
            .chain(self.targets.iter().map(|&index| {
                let target = &model.pac_target_info[index];
                (target.parse_order, target.active_tokens.clone())
            }))
            .collect();
        for (order, range) in ranges {
            self.accepted_parent(model, order, &range)?;
            let tokens = model.expanded_tokens.get(range).ok_or_else(|| {
                "a required named model has no complete expanded token range".to_string()
            })?;
            for token in tokens {
                self.charge()?;
                let text = token.text(&model.source);
                bytes = bytes.saturating_add(text.len() + 1);
                if bytes > MAX_CONTEXT_BYTES {
                    return Err(format!(
                        "named model context exceeds {MAX_CONTEXT_BYTES} bytes; no fragment was returned"
                    ));
                }
                if super::contains_macro(text) {
                    return Err(
                        "required named model context still contains unexpanded macro text".into(),
                    );
                }
            }
            pieces.push((order, join_lexemes(&model.source, tokens)));
        }
        pieces.sort_by_key(|(order, _)| *order);
        Ok(pieces)
    }

    pub fn notes(&self, model: &Model) -> Vec<OmittedContext> {
        let mut notes = Vec::new();
        if !self.is_empty()
            && model.execution_steps.iter().any(|step| {
                matches!(
                    step,
                    crate::engine::language::model::ExecutionStep::Opaque(_)
                )
            })
        {
            notes.push(OmittedContext { kind: "native".into(), detail: "native MATLAB/Octave statements, including initialization or update calls, are not in the fragment".into() });
        }
        if self.commands.len() < model.semi_structural_commands.len()
            || self.targets.len() < model.pac_target_info.len()
        {
            notes.push(OmittedContext { kind: "named_models".into(), detail: "named model definitions and PAC target blocks unrelated to the retained dependency closure are not in the fragment".into() });
        }
        if self.whole_aggregate_target {
            notes.push(OmittedContext { kind: "retained_context".into(), detail: "all aggregate equations are retained because a composite PAC target needs the original model context for Dynare's target matching".into() });
        }
        if self.whole_aggregate_pac {
            notes.push(OmittedContext { kind: "retained_context".into(), detail: "all aggregate equations are retained because the PAC equation association requires the original model-local context".into() });
        }
        notes
    }
}

fn symbol_option(command: &SemiStructuralCommand, name: &str) -> Option<Name> {
    command.options.iter().rev().find_map(|option| {
        (option.name.eq_ignore_ascii_case(name))
            .then_some(&option.value)
            .and_then(|value| match value {
                SemiStructuralValue::Symbol { name, .. } => Some(*name),
                _ => None,
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_dependency_work_stops_at_the_limit_or_cancellation() {
        // Guards continuing a dependency walk after its finite allowance or cancellation.
        let model = crate::engine::language::parser::parse(
            "var y; model; [name='eq'] y=var_expectation(missing); end;",
        );
        let retained = [EqRef {
            block: None,
            index: 0,
        }];
        let mut context = NamedContext {
            visits: MAX_VISITS,
            ..Default::default()
        };
        assert!(
            context
                .grow(&model, &retained)
                .unwrap_err()
                .contains("exceeded")
        );
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let result = crate::engine::control::run(Some(flag.clone()), || {
            crate::engine::control::cancel_after(&flag, 0);
            NamedContext::default().grow(&model, &retained)
        });
        assert!(result.is_err());
        assert!(flag.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn parent_proof_build_and_cached_lookup_share_the_remaining_allowance() {
        // Guards an uncharged statement/refusal scan or a cache lookup ignoring cancellation.
        let model = crate::engine::language::parser::parse(
            "var y; parameters b; pac_model(model_name=pm,discount=b); model; y=pac_expectation(pm); end;",
        );
        let command = &model.semi_structural_commands[0];
        let mut context = NamedContext {
            visits: MAX_VISITS - 2,
            ..Default::default()
        };
        assert!(
            context
                .accepted_parent(&model, command.parse_order, &command.active_tokens)
                .unwrap_err()
                .contains("exceeded")
        );
        assert!(context.parents.is_none());

        let mut context = NamedContext::default();
        context
            .accepted_parent(&model, command.parse_order, &command.active_tokens)
            .unwrap();
        // The built index is reused, but a lookup must still use the same budget.
        context.visits = MAX_VISITS;
        assert!(
            context
                .accepted_parent(&model, command.parse_order, &command.active_tokens)
                .unwrap_err()
                .contains("exceeded")
        );
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let result = crate::engine::control::run(Some(flag.clone()), || {
            crate::engine::control::cancel_after(&flag, 3);
            NamedContext::default().accepted_parent(
                &model,
                command.parse_order,
                &command.active_tokens,
            )
        });
        assert!(result.is_err());
        assert!(flag.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn named_rendering_preserves_closure_work_and_cancellation() {
        // Guards resetting the allowance for rendering or returning a fragment after mid-render cancellation.
        let model = crate::engine::language::parser::parse(
            "var y; parameters b; pac_model(model_name=pm,discount=b); model; y=pac_expectation(pm); end;",
        );
        let retained = [EqRef {
            block: None,
            index: 0,
        }];
        let mut context = NamedContext::default();
        context.grow(&model, &retained).unwrap();
        let closure = super::super::close(&model, &retained, &[], &context);
        let pieces = context.pieces(&model).unwrap();
        context.visits = MAX_VISITS - 5;
        let error = super::super::render_named(
            &model,
            &retained,
            &closure,
            None,
            pieces.clone(),
            &mut context,
        )
        .unwrap_err();
        assert!(error.contains("exceeded"), "{error}");
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let result = crate::engine::control::run(Some(flag.clone()), || {
            context.visits = 0;
            crate::engine::control::cancel_after(&flag, 5);
            super::super::render_named(&model, &retained, &closure, None, pieces, &mut context)
        });
        assert!(result.is_err());
        assert!(flag.load(std::sync::atomic::Ordering::Relaxed));
    }
}
