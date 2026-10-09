//! Conservative comparison of accepted facts within proven execution contexts.
//! Producer token claims prevent supporting Commands from owning a field twice.

use std::collections::{BTreeMap, VecDeque};
use std::ops::Range;

use crate::model::{ExecutionStep, Model, Statement};
use crate::model_diff::ModelDiff;

use super::*;

#[derive(Clone, Debug)]
pub struct FactField {
    pub name: String,
    pub label: String,
    pub value: FieldState,
    pub facet: ChangeFacet,
    pub is_expression: bool,
    /// False only when optional comparison work exhausted its budget. This is
    /// distinct from a real retained model value whose state is Unknown.
    pub comparison_available: bool,
}

impl FactField {
    pub fn new(name: &str, label: &str, value: FieldState, facet: ChangeFacet) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            value,
            facet,
            is_expression: false,
            comparison_available: true,
        }
    }
    pub fn expression(mut self) -> Self {
        self.is_expression = true;
        self
    }
    pub fn unavailable(mut self) -> Self {
        self.comparison_available = false;
        self
    }
}

#[derive(Clone, Debug)]
pub struct CapturedFact {
    pub family: SemanticFamily,
    pub role: String,
    /// Resolved, ordered target components. No source offset or parser index.
    pub key: Vec<String>,
    pub side: RowSide,
    pub fields: Vec<FactField>,
    /// Only ranges already owned by a named producer. They may include target
    /// tokens, but must not cover options whose values the producer omitted.
    pub claims: Vec<Range<usize>>,
    pub limits: Vec<ComparisonLimit>,
    pub count_unit: CountUnit,
}

impl CapturedFact {
    pub fn new(family: SemanticFamily, role: &str, key: Vec<String>, side: RowSide) -> Self {
        Self {
            family,
            role: role.into(),
            key,
            side,
            fields: Vec::new(),
            claims: Vec::new(),
            limits: Vec::new(),
            count_unit: CountUnit::AcceptedOccurrence,
        }
    }
}

#[derive(Default)]
pub struct TokenClaims {
    /// Shared optional retained-expression traversal/materialization account.
    pub expression_work: super::expression_values::RetainedExpressionWork,
    before: BTreeMap<usize, Vec<Range<usize>>>,
    after: BTreeMap<usize, Vec<Range<usize>>>,
}

impl TokenClaims {
    pub fn with_expression_budget(detail_bytes: usize) -> Self {
        Self {
            expression_work: super::expression_values::RetainedExpressionWork::new(detail_bytes),
            ..Self::default()
        }
    }
    pub fn claim(&mut self, side: Side, statement_id: usize, range: Range<usize>) {
        let map = match side {
            Side::Before => &mut self.before,
            Side::After => &mut self.after,
        };
        if !range.is_empty() {
            map.entry(statement_id).or_default().push(range);
        }
    }
    pub fn contains(&self, side: Side, statement_id: usize, token: usize) -> bool {
        let map = match side {
            Side::Before => &self.before,
            Side::After => &self.after,
        };
        map.get(&statement_id)
            .is_some_and(|ranges| ranges.iter().any(|range| range.contains(&token)))
    }
}

pub fn statement_text(model: &Model, range: Range<usize>) -> Option<String> {
    let tokens = model.expanded_tokens.get(range)?;
    Some(crate::parser::join_lexemes(&model.source, tokens))
}

pub fn statement_side(model: &Model, id: usize, label: &str, scope: ComparisonScope) -> RowSide {
    let mut side = RowSide::named(label, scope.clone());
    if let Some(statement) = model
        .statements
        .get(id)
        .filter(|statement| statement.id == id)
    {
        side.occurrence = Some(statement.token_range.start);
        side.context = Some(StatementContext {
            kind: statement.kind.as_str().into(),
            name: statement.name.clone(),
            execution_order: execution_order(model, id),
            scope,
            pointer: None,
        });
        side.provenance = Some(OccurrenceProvenance {
            span: statement.keyword_span,
            parse_order: Some(statement.token_range.start),
            equation_id: None,
            statement_id: Some(id),
        });
    }
    side
}

pub fn execution_order(model: &Model, id: usize) -> usize {
    model
        .execution_steps
        .iter()
        .position(|step| matches!(step, ExecutionStep::Statement(value) if *value == id))
        .unwrap_or(id)
}

pub fn parent_at_order(model: &Model, order: usize) -> Option<usize> {
    let mut parents = model
        .statements
        .iter()
        .filter(|statement| statement.token_range.contains(&order));
    let parent = parents.next()?;
    parents.next().is_none().then_some(parent.id)
}

/// Presentation records can survive parser recovery. Withhold correspondence
/// and command claims for those records; accepted child facts stay available.
pub fn accepted_statement(model: &Model, statement: &Statement) -> bool {
    statement.complete
        && !statement.native
        && statement.name != "verbatim"
        && !model.constructor_refused_statements.contains(&statement.id)
        && !model.shape_refuses.iter().any(|refusal| {
            refusal.parse_execution.as_ref().is_some_and(|range| {
                range.start < statement.token_range.end && statement.token_range.start < range.end
            }) || refusal
                .parse_order
                .is_some_and(|order| statement.token_range.contains(&order))
        })
        && !model.const_fold_errors.iter().any(|(span, _, _)| {
            statement.span.start <= span.start && span.start < statement.span.end
        })
        && !model.parse_issues.iter().any(|issue| {
            let orders: Vec<_> = model
                .parse_issue_orders
                .iter()
                .filter(|(span, _)| *span == issue.span)
                .map(|(_, order)| *order)
                .collect();
            if orders.is_empty() {
                statement.span.start <= issue.span.start && issue.span.start < statement.span.end
            } else {
                orders
                    .into_iter()
                    .any(|order| statement.token_range.contains(&order))
            }
        })
        && !model
            .written_equations
            .iter()
            .filter(|row| row.statement_id == statement.id && row.equation.is_local)
            .any(|row| {
                row.equation
                    .lhs_expr
                    .is_none_or(|target| !model.valid_model_local_targets.contains(&target))
            })
        && !model
            .steady_state_equations
            .iter()
            .filter(|equation| statement.token_range.contains(&equation.parse_order))
            .any(|equation| {
                equation.steady_state_targets.iter().any(|target| {
                    !target.action_attempted || !model.steady_state_target_is_valid(target)
                })
            })
        && !model
            .histval
            .iter()
            .chain(&model.filter_initial_state)
            .any(|entry| {
                statement.token_range.contains(&entry.active_tokens.start)
                    && !entry.accepted_assignment
            })
}

type Signature = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);
fn signature(statement: &Statement) -> Signature {
    (
        statement.kind.as_str().into(),
        statement.name.clone(),
        statement.category.clone(),
        statement.subtype.clone(),
        statement.dimension.clone(),
    )
}

/// Paired IDs are private capture-local proof. Full unchanged execution text
/// supplies anchors, including native barriers. Unique signatures pair only
/// inside the same surrounding anchors; repeated changed contexts do not pair.
pub struct ParentPairs {
    pub before_to_after: BTreeMap<usize, usize>,
}

impl ParentPairs {
    pub fn new(before: &Model, after: &Model) -> Self {
        fn steps(model: &Model) -> Vec<(Option<usize>, String)> {
            model
                .execution_steps
                .iter()
                .map(|step| match step {
                    ExecutionStep::Statement(id) => {
                        let statement = &model.statements[*id];
                        let text = statement_text(model, statement.token_range.clone())
                            .unwrap_or_default();
                        (
                            accepted_statement(model, statement).then_some(*id),
                            format!("{:?}:{text}", signature(statement)),
                        )
                    }
                    ExecutionStep::Opaque(span) => (
                        None,
                        format!(
                            "native:{}",
                            model
                                .source
                                .get(span.start as usize..span.end as usize)
                                .unwrap_or_default()
                        ),
                    ),
                })
                .collect()
        }
        let old = steps(before);
        let new = steps(after);
        let mut counts: BTreeMap<&str, [Vec<usize>; 2]> = BTreeMap::new();
        for (list, side) in [(&old, 0), (&new, 1)] {
            for (index, (_, text)) in list.iter().enumerate() {
                counts.entry(text).or_default()[side].push(index);
            }
        }
        let mut anchors: Vec<_> = counts
            .values()
            .filter_map(|indices| match (&indices[0][..], &indices[1][..]) {
                ([a], [b]) => Some((*a, *b)),
                _ => None,
            })
            .collect();
        anchors.sort_unstable();
        // Crossing anchors represent order edits. They cannot delimit a proof
        // region, so retain only anchors whose order is unchanged on both sides.
        let mut suffix_min = vec![usize::MAX; anchors.len() + 1];
        for i in (0..anchors.len()).rev() {
            suffix_min[i] = suffix_min[i + 1].min(anchors[i].1);
        }
        let mut prefix_max = None;
        let anchors: Vec<_> = anchors
            .iter()
            .enumerate()
            .filter_map(|(i, &(a, b))| {
                let keep = prefix_max.is_none_or(|max| max < b) && b < suffix_min[i + 1];
                prefix_max = Some(prefix_max.map_or(b, |max: usize| max.max(b)));
                keep.then_some((a, b))
            })
            .collect();
        let mut pairs = BTreeMap::new();
        let mut starts = (0, 0);
        for (end_old, end_new) in anchors
            .iter()
            .copied()
            .chain(std::iter::once((old.len(), new.len())))
        {
            let left = &old[starts.0..end_old];
            let right = &new[starts.1..end_new];
            if left
                .iter()
                .map(|(_, text)| text)
                .eq(right.iter().map(|(_, text)| text))
            {
                for ((a, _), (b, _)) in left.iter().zip(right) {
                    if let (Some(a), Some(b)) = (a, b) {
                        pairs.insert(*a, *b);
                    }
                }
            } else {
                let mut groups: BTreeMap<Signature, [Vec<usize>; 2]> = BTreeMap::new();
                for (model, list, side) in [(before, left, 0), (after, right, 1)] {
                    for (id, _) in list {
                        if let Some(id) = id {
                            groups.entry(signature(&model.statements[*id])).or_default()[side]
                                .push(*id);
                        }
                    }
                }
                for group in groups.values() {
                    if let ([a], [b]) = (&group[0][..], &group[1][..]) {
                        pairs.insert(*a, *b);
                    }
                }
            }
            if let (Some((Some(a), _)), Some((Some(b), _))) = (old.get(end_old), new.get(end_new)) {
                pairs.insert(*a, *b);
            }
            starts = (end_old.saturating_add(1), end_new.saturating_add(1));
        }
        Self {
            before_to_after: pairs,
        }
    }
}

fn parent(fact: &CapturedFact) -> Option<usize> {
    fact.side.provenance.as_ref()?.statement_id
}
type FactKey = (
    String,
    String,
    Vec<String>,
    String,
    Option<String>,
    Option<String>,
);
fn fact_key(fact: &CapturedFact) -> FactKey {
    (
        format!("{:?}", fact.family),
        fact.role.clone(),
        fact.key.clone(),
        fact.side.scope.domain.clone(),
        fact.side.scope.dimension.clone(),
        fact.side.scope.block.clone(),
    )
}
fn equal_facts(before: &CapturedFact, after: &CapturedFact) -> bool {
    before.fields.len() == after.fields.len()
        && before.fields.iter().zip(&after.fields).all(|(a, b)| {
            a.name == b.name
                && (!a.comparison_available || !b.comparison_available || a.value == b.value)
        })
}

/// Every named fact, including unchanged facts, claims its accepted producer
/// tokens. Locations, occurrence IDs and execution positions are not values.
pub fn compare_facts(
    before: &Model,
    after: &Model,
    mut old: Vec<CapturedFact>,
    mut new: Vec<CapturedFact>,
    diff: &mut ModelDiff,
    claims: &mut TokenClaims,
) {
    let first_row = diff.semantic.rows.len();
    for fact in old.iter().chain(&new) {
        for limit in &fact.limits {
            record_limit(diff, fact.family, limit.clone());
        }
    }
    for (facts, side, model) in [(&old, Side::Before, before), (&new, Side::After, after)] {
        for fact in facts {
            if let Some(id) = parent(fact) {
                if let Some(statement) = model.statements.get(id) {
                    for range in &fact.claims {
                        if statement.token_range.start <= range.start
                            && range.end <= statement.token_range.end
                        {
                            claims.claim(side, id, range.clone());
                        }
                    }
                }
            }
        }
    }
    let pairs = ParentPairs::new(before, after);
    let reverse: BTreeMap<_, _> = pairs
        .before_to_after
        .iter()
        .map(|(a, b)| (*b, *a))
        .collect();
    let mut groups: BTreeMap<(Option<usize>, FactKey), [Vec<usize>; 2]> = BTreeMap::new();
    let mut unproven = [Vec::new(), Vec::new()];
    for (facts, side) in [(&old, 0), (&new, 1)] {
        for (index, fact) in facts.iter().enumerate() {
            let id = (fact.count_unit != CountUnit::FinalFact)
                .then(|| parent(fact))
                .flatten()
                .and_then(|id| {
                    if side == 0 {
                        pairs.before_to_after.contains_key(&id).then_some(id)
                    } else {
                        reverse.get(&id).copied()
                    }
                });
            if id.is_some() || fact.count_unit == CountUnit::FinalFact {
                groups.entry((id, fact_key(fact))).or_default()[side].push(index);
            } else {
                unproven[side].push(index);
            }
        }
    }
    // Relative order among common unique facts is meaningful. Added/removed
    // siblings cannot manufacture an order edit by shifting display positions.
    let mut matched = Vec::new();
    for group in groups.values() {
        if let ([a], [b]) = (&group[0][..], &group[1][..]) {
            matched.push((*a, *b));
        }
    }
    let mut orders: BTreeMap<usize, [Vec<(usize, usize)>; 2]> = BTreeMap::new();
    for &(a, b) in &matched {
        if let Some(id) = parent(&old[a]) {
            let row = orders.entry(id).or_default();
            row[0].push((a, b));
            row[1].push((a, b));
        }
    }
    let mut changed_order = BTreeMap::new();
    for order in orders.values_mut() {
        order[0].sort_by_key(|(a, _)| *a);
        order[1].sort_by_key(|(_, b)| *b);
        let new_ranks: BTreeMap<_, _> = order[1]
            .iter()
            .enumerate()
            .map(|(rank, &pair)| (pair, rank))
            .collect();
        for (rank, &pair) in order[0].iter().enumerate() {
            let new_rank = new_ranks[&pair];
            if new_rank != rank {
                changed_order.insert(pair, (rank, new_rank));
            }
        }
    }
    for group in groups.values() {
        mask_budget_fields(&mut old, &mut new, group);
        if let ([a], [b]) = (&group[0][..], &group[1][..]) {
            emit(
                Some(&old[*a]),
                Some(&new[*b]),
                false,
                changed_order.get(&(*a, *b)).copied(),
                diff,
            );
        } else {
            let unchanged = group[0].len() == group[1].len()
                && group[0]
                    .iter()
                    .zip(&group[1])
                    .all(|(a, b)| equal_facts(&old[*a], &new[*b]));
            if !unchanged {
                emit_candidates(&old, &new, group, true, diff);
            }
        }
    }
    // Entire unchanged repeated sequences need no rows, without asserting a
    // pair for any changed occurrence. No ordinal pairing across changed runs.
    let mut unproven_groups: BTreeMap<FactKey, [Vec<usize>; 2]> = BTreeMap::new();
    for (facts, indices, side) in [(&old, &unproven[0], 0), (&new, &unproven[1], 1)] {
        for &index in indices {
            unproven_groups.entry(fact_key(&facts[index])).or_default()[side].push(index);
        }
    }
    for group in unproven_groups.values() {
        mask_budget_fields(&mut old, &mut new, group);
        let unchanged = group[0].len() == group[1].len()
            && group[0]
                .iter()
                .zip(&group[1])
                .all(|(a, b)| equal_facts(&old[*a], &new[*b]));
        if unchanged {
            continue;
        }
        emit_candidates(&old, &new, group, false, diff);
    }
    diff.semantic.rows[first_row..].sort_by_key(|row| {
        let side = row.after.as_ref().or(row.before.as_ref());
        (
            side.and_then(|side| side.context.as_ref().map(|context| context.execution_order))
                .unwrap_or(usize::MAX),
            side.and_then(|side| side.occurrence).unwrap_or(usize::MAX),
        )
    });
    for (index, row) in diff.semantic.rows.iter_mut().enumerate().skip(first_row) {
        row.pointer = format!("/semantic/rows/{index}");
    }
}

fn mask_budget_fields(old: &mut [CapturedFact], new: &mut [CapturedFact], group: &[Vec<usize>; 2]) {
    let names: std::collections::BTreeSet<_> = group[0]
        .iter()
        .map(|&index| &old[index])
        .chain(group[1].iter().map(|&index| &new[index]))
        .flat_map(|fact| &fact.fields)
        .filter(|field| !field.comparison_available)
        .map(|field| field.name.clone())
        .collect();
    for (facts, indices) in [(old, &group[0]), (new, &group[1])] {
        for &index in indices {
            for field in &mut facts[index].fields {
                if names.contains(&field.name) {
                    field.comparison_available = false;
                }
            }
        }
    }
}

fn emit_candidates(
    old: &[CapturedFact],
    new: &[CapturedFact],
    group: &[Vec<usize>; 2],
    proven_parent: bool,
    diff: &mut ModelDiff,
) {
    // Cancelling equal retained values establishes no occurrence pair. This
    // keeps unchanged controls out of an ambiguous changed candidate group.
    let fingerprint = |fact: &CapturedFact| {
        serde_json::to_vec(
            &fact
                .fields
                .iter()
                .filter(|field| field.comparison_available)
                .map(|field| (&field.name, &field.value))
                .collect::<Vec<_>>(),
        )
        .expect("typed finite comparison fields")
    };
    let mut candidates: BTreeMap<Vec<u8>, VecDeque<usize>> = BTreeMap::new();
    for &b in &group[1] {
        candidates
            .entry(fingerprint(&new[b]))
            .or_default()
            .push_back(b);
    }
    let mut remaining_old = Vec::new();
    for &a in &group[0] {
        if candidates
            .get_mut(&fingerprint(&old[a]))
            .and_then(VecDeque::pop_front)
            .is_none()
        {
            remaining_old.push(a);
        }
    }
    let mut remaining_new: Vec<_> = candidates.into_values().flatten().collect();
    remaining_new.sort_unstable();
    if remaining_old.is_empty() && remaining_new.is_empty() && proven_parent {
        // The paired parent proves the ordered sequence changed, while no
        // repeated target identifies an occurrence pair. Keep side occurrences
        // and an Order facet instead of inventing a paired field transition.
        for &a in &group[0] {
            emit(Some(&old[a]), None, true, None, diff);
            if let Some(row) = diff.semantic.rows.last_mut() {
                row.facets.push(ChangeFacet::Order);
            }
        }
        for &b in &group[1] {
            emit(None, Some(&new[b]), true, None, diff);
            if let Some(row) = diff.semantic.rows.last_mut() {
                row.facets.push(ChangeFacet::Order);
            }
        }
        return;
    }
    for a in remaining_old {
        emit(Some(&old[a]), None, !group[1].is_empty(), None, diff);
    }
    for b in remaining_new {
        emit(None, Some(&new[b]), !group[0].is_empty(), None, diff);
    }
}

fn emit(
    before: Option<&CapturedFact>,
    after: Option<&CapturedFact>,
    unpaired: bool,
    order_changed: Option<(usize, usize)>,
    diff: &mut ModelDiff,
) {
    if order_changed.is_none()
        && before
            .zip(after)
            .is_some_and(|(before, after)| equal_facts(before, after))
    {
        return;
    }
    let fact = after.or(before).expect("one fact side");
    let mut row = SemanticRow::new(
        fact.family,
        if unpaired {
            ChangeKind::Unpaired
        } else {
            match (before, after) {
                (Some(_), Some(_)) => ChangeKind::Changed,
                (Some(_), None) => ChangeKind::Removed,
                _ => ChangeKind::Added,
            }
        },
        &fact.side.name,
    );
    row.count_unit = fact.count_unit;
    row.before = before.map(|fact| fact.side.clone());
    row.after = after.map(|fact| fact.side.clone());
    row.fields.push(FieldChange::new(
        "role",
        "Role",
        before
            .map(|fact| FieldState::text(&fact.role))
            .unwrap_or_else(FieldState::absent),
        after
            .map(|fact| FieldState::text(&fact.role))
            .unwrap_or_else(FieldState::absent),
    ));
    let mut names = std::collections::BTreeSet::new();
    for field in before
        .into_iter()
        .chain(after)
        .flat_map(|fact| &fact.fields)
    {
        names.insert(field.name.clone());
    }
    for name in names {
        let a = before.and_then(|fact| fact.fields.iter().find(|field| field.name == name));
        let b = after.and_then(|fact| fact.fields.iter().find(|field| field.name == name));
        let field = b.or(a).expect("field on one side");
        let mut change = FieldChange::new(
            &name,
            &field.label,
            a.map(|field| field.value.clone())
                .unwrap_or_else(FieldState::absent),
            b.map(|field| field.value.clone())
                .unwrap_or_else(FieldState::absent),
        );
        let available = a
            .into_iter()
            .chain(b)
            .all(|field| field.comparison_available);
        if !available {
            change.changed = false;
            change.comparison_availability = Availability::LimitExceeded;
        }
        if change.changed && !row.facets.contains(&field.facet) {
            row.facets.push(field.facet);
        }
        if available && field.is_expression && (field_text(a).is_some() || field_text(b).is_some())
        {
            row.expressions.push(if unpaired {
                ExpressionDetail {
                    field: name.clone(),
                    before: field_text(a).map(ExpressionSide::plain),
                    after: field_text(b).map(ExpressionSide::plain),
                    highlight_basis: HighlightBasis::None,
                    availability: Availability::Complete,
                    reason: Some("Occurrence correspondence is not proven.".into()),
                }
            } else {
                super::equations::expression_detail(
                    &mut diff.semantic,
                    &name,
                    field_text(a),
                    field_text(b),
                )
            });
        }
        row.fields.push(change);
    }
    if let Some((before, after)) = order_changed {
        row.facets.push(ChangeFacet::Order);
        row.fields.push(FieldChange::new(
            "order",
            "Relative assignment order",
            FieldState::present(FieldValue::Integer(before as i64)),
            FieldState::present(FieldValue::Integer(after as i64)),
        ));
    }
    for limit in before
        .into_iter()
        .chain(after)
        .flat_map(|fact| &fact.limits)
    {
        if !row.limits.contains(limit) {
            row.limits.push(limit.clone());
        }
    }
    if unpaired {
        row.limits.push(ComparisonLimit::new("occurrence_correspondence_unpaired","Repeated targets or execution contexts have no proven cross-side correspondence; each occurrence remains a side fact.","semantic_occurrences"));
    }
    if before.is_some()
        && after.is_some()
        && order_changed.is_none()
        && row.fields.iter().all(|field| !field.changed)
    {
        return;
    }
    if row
        .expressions
        .iter()
        .any(|detail| detail.availability == Availability::LimitExceeded)
    {
        row.limits.push(ComparisonLimit::new("occurrence_token_alignment_limit","Token highlights for a retained field exceeded the shared comparison budget; exact text remains available.","semantic_occurrences"));
    }
    for limit in &row.limits {
        record_limit(diff, row.family, limit.clone());
    }
    diff.semantic.push_row(row);
}

pub(super) fn record_limit(diff: &mut ModelDiff, family: SemanticFamily, limit: ComparisonLimit) {
    let index = diff
        .coverage
        .families
        .iter()
        .position(|coverage| coverage.family == family)
        .unwrap_or_else(|| {
            diff.coverage.families.push(FamilyCoverage {
                family,
                availability: Availability::Complete,
                fields: Vec::new(),
                limits: Vec::new(),
            });
            diff.coverage.families.len() - 1
        });
    let coverage = &mut diff.coverage.families[index];
    coverage.availability = Availability::Partial;
    if !coverage
        .limits
        .iter()
        .any(|existing| existing.code == limit.code && existing.owner == limit.owner)
    {
        coverage.limits.push(limit.clone());
    }
    if !diff
        .semantic
        .limits
        .iter()
        .any(|existing| existing.code == limit.code && existing.owner == limit.owner)
    {
        diff.semantic.limits.push(limit.clone());
    }
    if !diff
        .coverage
        .limits
        .iter()
        .any(|existing| existing.code == limit.code && existing.owner == limit.owner)
    {
        diff.coverage.limits.push(limit);
    }
    diff.semantic.availability = Availability::Partial;
    diff.coverage.availability = Availability::Partial;
}

fn field_text(field: Option<&FactField>) -> Option<&str> {
    field.and_then(|field| match field.value.value.as_ref() {
        Some(FieldValue::Text(text)) => Some(text.as_str()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fact(value: FieldState, available: bool, flag: bool) -> CapturedFact {
        let mut fact = CapturedFact::new(
            SemanticFamily::Operations,
            "budget_control",
            vec!["control".into()],
            RowSide::named("control", ComparisonScope::global()),
        );
        fact.count_unit = CountUnit::FinalFact;
        let value =
            FactField::new("expression", "Expression", value, ChangeFacet::Expression).expression();
        fact.fields.push(if available {
            value
        } else {
            value.unavailable()
        });
        fact.fields.push(FactField::new(
            "flag",
            "Flag",
            FieldState::present(FieldValue::Boolean(flag)),
            ChangeFacet::Options,
        ));
        if !available {
            fact.limits.push(ComparisonLimit::new(
                "expression_tree_materialization_limit",
                "Expression comparison exceeded the test budget.",
                "semantic_family_expression_budget",
            ));
        }
        fact
    }
    #[test]
    fn budget_unavailable_value_is_not_a_model_edit() {
        let model = Model::default();
        let mut diff = crate::compare_models(&model, &model);
        compare_facts(
            &model,
            &model,
            vec![fact(FieldState::text("x+1"), true, false)],
            vec![fact(FieldState::unknown(), false, false)],
            &mut diff,
            &mut TokenClaims::default(),
        );
        assert!(diff.semantic.rows.is_empty());
        assert!(diff
            .coverage
            .limits
            .iter()
            .any(|limit| limit.code == "expression_tree_materialization_limit"));
    }
    #[test]
    fn budget_mask_preserves_other_field_changes_and_wire_availability() {
        let model = Model::default();
        let mut diff = crate::compare_models(&model, &model);
        compare_facts(
            &model,
            &model,
            vec![fact(FieldState::text("x+1"), true, false)],
            vec![fact(FieldState::unknown(), false, true)],
            &mut diff,
            &mut TokenClaims::default(),
        );
        assert_eq!(diff.semantic.rows.len(), 1);
        let row = &diff.semantic.rows[0];
        assert_eq!(row.facets, vec![ChangeFacet::Options]);
        assert!(row.expressions.is_empty());
        let expression = row
            .fields
            .iter()
            .find(|field| field.name == "expression")
            .unwrap();
        assert!(!expression.changed);
        assert_eq!(
            expression.comparison_availability,
            Availability::LimitExceeded
        );
        assert!(serde_json::to_string(expression)
            .unwrap()
            .contains("limit_exceeded"));
    }
    #[test]
    fn real_known_to_unknown_value_keeps_its_change() {
        let model = Model::default();
        let mut diff = crate::compare_models(&model, &model);
        compare_facts(
            &model,
            &model,
            vec![fact(FieldState::text("x+1"), true, false)],
            vec![fact(FieldState::unknown(), true, false)],
            &mut diff,
            &mut TokenClaims::default(),
        );
        assert_eq!(diff.semantic.rows.len(), 1);
        assert_eq!(diff.semantic.rows[0].facets, vec![ChangeFacet::Expression]);
        assert!(diff.semantic.rows[0]
            .fields
            .iter()
            .any(|field| field.name == "expression"
                && field.changed
                && field.comparison_availability == Availability::Complete));
    }
}
