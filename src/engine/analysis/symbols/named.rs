//! Typed names outside the ordinary symbol table. Sites are parser receipts,
//! not a text search; subsample copies keep their source definition relation.
use super::*;
use crate::engine::language::model::{
    DottedHead, NamedSlotKind, SemiStructuralKind, SubsampleHead, SubsampleInstruction,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Site {
    pub order: usize,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Fact {
    pub kind: &'static str,
    pub name: String,
    pub namespace: Option<String>,
    pub declarations: Vec<Site>,
    pub uses: Vec<Site>,
    /// Subsample copies share a source label; other facts own their origin.
    pub origin: usize,
}

fn model_kind(kind: SemiStructuralKind) -> &'static str {
    match kind {
        SemiStructuralKind::VarModel => "var_model",
        SemiStructuralKind::TrendComponentModel => "trend_component_model",
        SemiStructuralKind::VarExpectationModel => "var_expectation_model",
        SemiStructuralKind::PacModel => "pac_model",
    }
}

fn fact(facts: &mut Vec<Fact>, kind: &'static str, name: &str) -> usize {
    facts
        .iter()
        .position(|row| row.kind == kind && row.name == name && row.namespace.is_none())
        .unwrap_or_else(|| {
            let id = facts.len();
            facts.push(Fact {
                kind,
                name: name.into(),
                namespace: None,
                declarations: Vec::new(),
                uses: Vec::new(),
                origin: id,
            });
            id
        })
}

pub(super) fn collect(model: &Model) -> Vec<Fact> {
    let mut facts = Vec::new();
    let accepted = crate::engine::analysis::named_callbacks::collect(model);
    // Auxiliary model references are resolved at Transform, against the final
    // definition tables; the two owning commands have different priorities.
    for (_, slot) in model
        .named_slots
        .iter()
        .enumerate()
        .filter(|(index, slot)| slot.declaration && accepted.slots.contains(index))
    {
        if crate::engine::control::cancelled() {
            return facts;
        }
        let kind = match slot.kind {
            NamedSlotKind::Dimension => "heterogeneity_dimension",
            NamedSlotKind::Database => "database",
            NamedSlotKind::Model(kind) => model_kind(kind),
            NamedSlotKind::AuxiliaryModel(_) => unreachable!(),
        };
        let id = fact(&mut facts, kind, model.name(slot.name));
        facts[id].declarations.push(Site {
            order: slot.order,
            span: slot.span,
        });
    }
    for (_, slot) in model
        .named_slots
        .iter()
        .enumerate()
        .filter(|(index, _)| accepted.slots.contains(index))
    {
        if crate::engine::control::cancelled() {
            return facts;
        }
        if slot.declaration {
            continue;
        }
        if !matches!(slot.kind, NamedSlotKind::AuxiliaryModel(_)) {
            let kind = match slot.kind {
                NamedSlotKind::Dimension => "heterogeneity_dimension",
                NamedSlotKind::Database => "database",
                NamedSlotKind::Model(kind) => model_kind(kind),
                NamedSlotKind::AuxiliaryModel(_) => unreachable!(),
            };
            let name = model.name(slot.name);
            let temporal = matches!(
                slot.kind,
                NamedSlotKind::Dimension | NamedSlotKind::Database
            );
            let existing = facts.iter().position(|row| {
                row.kind == kind
                    && row.name == name
                    && row.namespace.is_none()
                    && (!temporal
                        || row
                            .declarations
                            .iter()
                            .any(|declaration| declaration.order < slot.order))
            });
            let id = existing.unwrap_or_else(|| {
                let id = facts.len();
                facts.push(Fact {
                    kind,
                    name: name.into(),
                    namespace: None,
                    declarations: Vec::new(),
                    uses: Vec::new(),
                    origin: id,
                });
                id
            });
            facts[id].uses.push(Site {
                order: slot.order,
                span: slot.span,
            });
            continue;
        }
        let NamedSlotKind::AuxiliaryModel(owner) = slot.kind else {
            continue;
        };
        let (preferred, fallback) = if owner == SemiStructuralKind::PacModel {
            ("trend_component_model", "var_model")
        } else {
            ("var_model", "trend_component_model")
        };
        let name = model.name(slot.name);
        let kind = if facts
            .iter()
            .any(|row| row.kind == preferred && row.name == name && !row.declarations.is_empty())
        {
            preferred
        } else {
            fallback
        };
        let id = fact(&mut facts, kind, name);
        facts[id].uses.push(Site {
            order: slot.order,
            span: slot.span,
        });
    }
    collect_subsamples(model, &mut facts, &accepted);
    facts
}

fn head(model: &Model, head: &SubsampleHead) -> Vec<String> {
    match head {
        SubsampleHead::Symbol(name, _) | SubsampleHead::Std(name, _) => {
            vec![model.name(*name).into()]
        }
        SubsampleHead::Corr(first, _, second, _) => {
            vec![model.name(*first).into(), model.name(*second).into()]
        }
    }
}

fn dotted_head(model: &Model, head: &DottedHead) -> Option<Vec<String>> {
    match head {
        DottedHead::Param { first, .. } | DottedHead::Std { first, .. } => {
            Some(vec![model.name(*first).into()])
        }
        DottedHead::Corr { first, second, .. } => {
            Some(vec![model.name(*first).into(), model.name(*second).into()])
        }
        DottedHead::Vec { .. } => None,
    }
}

fn lookup<'a>(
    active: &'a HashMap<Vec<String>, HashMap<String, usize>>,
    key: &[String],
) -> Option<&'a HashMap<String, usize>> {
    active.get(key).or_else(|| {
        (key.len() == 2)
            .then(|| active.get(&[key[1].clone(), key[0].clone()][..]))
            .flatten()
    })
}

fn collect_subsamples(
    model: &Model,
    facts: &mut Vec<Fact>,
    accepted: &crate::engine::analysis::named_callbacks::Accepted,
) {
    enum Event<'a> {
        Statement(&'a SubsampleInstruction, usize),
        Use(&'a crate::engine::language::model::SubsampleUse),
    }
    let mut events = Vec::new();
    for (index, row) in model.subsamples.iter().enumerate() {
        if !accepted.subsamples.contains(&index) {
            continue;
        }
        if let Some(receipt) = model
            .fact_receipts
            .get("subsamples")
            .and_then(|rows| rows.get(index))
        {
            events.push((receipt.parse_order, Event::Statement(row, index)));
        }
    }
    events.extend(
        model
            .subsample_uses
            .iter()
            .filter(|row| accepted.dotted.contains(&row.statement))
            .map(|row| (row.order, Event::Use(row))),
    );
    events.sort_by_key(|(order, _)| *order);
    let mut active: HashMap<Vec<String>, HashMap<String, usize>> = HashMap::new();
    for (_, event) in events {
        if crate::engine::control::cancelled() {
            return;
        }
        match event {
            Event::Statement(
                SubsampleInstruction::Declare {
                    head: owner,
                    ranges,
                    ..
                },
                index,
            ) => {
                let key = head(model, owner);
                let namespace = format!("subsamples({})", key.join(","));
                let _ = index;
                let mut labels = HashMap::new();
                for range in ranges {
                    let name = model.name(range.name);
                    let id = facts.len();
                    facts.push(Fact {
                        kind: "named_subsample",
                        name: name.into(),
                        namespace: Some(namespace.clone()),
                        declarations: vec![Site {
                            order: range.name_order,
                            span: range.name_span,
                        }],
                        uses: Vec::new(),
                        origin: id,
                    });
                    labels.insert(name.into(), id);
                }
                active.insert(key, labels);
            }
            Event::Statement(SubsampleInstruction::Copy { target, source, .. }, _) => {
                let target = head(model, target);
                // copy_subsamples requires the written source order. A later
                // prior/options lookup separately permits the reversed pair.
                let Some(source) = active.get(&head(model, source)).cloned() else {
                    continue;
                };
                let namespace = format!("subsamples({})", target.join(","));
                let mut labels = HashMap::new();
                let mut source: Vec<_> = source.into_iter().collect();
                source.sort_by_key(|(_, original)| *original);
                for (name, original) in source {
                    let id = facts.len();
                    let mut copy = facts[original].clone();
                    copy.namespace = Some(namespace.clone());
                    copy.uses.clear();
                    facts.push(copy);
                    labels.insert(name, id);
                }
                active.insert(target, labels);
            }
            Event::Use(row) => {
                let Some(key) = dotted_head(model, &row.head) else {
                    continue;
                };
                let name = model.name(row.name);
                if let Some(id) = lookup(&active, &key)
                    .and_then(|labels| labels.get(name))
                    .copied()
                {
                    facts[id].uses.push(Site {
                        order: row.order,
                        span: row.span,
                    });
                } else if !row.checked {
                    // The pin does not resolve a prior/options copy's labels.
                    // Keep that operand distinct from any later declaration.
                    let id = facts.len();
                    facts.push(Fact {
                        kind: "named_subsample",
                        name: name.into(),
                        namespace: Some(format!("subsamples({})", key.join(","))),
                        declarations: Vec::new(),
                        uses: vec![Site {
                            order: row.order,
                            span: row.span,
                        }],
                        origin: id,
                    });
                }
            }
        }
    }
}

pub(super) fn binding(view: &SemanticModel, id: usize) -> Binding {
    let row = &view.named[id];
    Binding {
        id: format!(
            "{}:{}:{}:{:?}:{}:{id}",
            view.revision, view.root, row.kind, row.namespace, row.name
        ),
        name: row.name.clone(),
        kind: row.kind.into(),
        namespace: row.namespace.clone(),
        dimension: None,
        declaration: row
            .declarations
            .first()
            .and_then(|site| view.written(site.span)),
        definition: None,
        identity: Identity::Named(id),
    }
}

pub(super) fn references(
    view: &SemanticModel,
    id: usize,
    include_declaration: bool,
) -> Vec<Reference> {
    let row = &view.named[id];
    let mut result = Vec::new();
    for (sites, declaration) in [(&row.declarations, true), (&row.uses, false)] {
        if declaration && !include_declaration {
            continue;
        }
        for site in sites {
            if crate::engine::control::cancelled() {
                return Vec::new();
            }
            result.push(Reference {
                order: site.order,
                span: site.span,
                written: view.written(site.span),
                role: if declaration { "declaration" } else { "use" },
                dimension: None,
                declaration,
            });
        }
    }
    result.sort_by_key(|site| site.order);
    result
}

pub(super) fn owns_target(view: &SemanticModel, id: usize, file: &str, byte: u32) -> bool {
    let row = &view.named[id];
    row.uses
        .iter()
        .chain(
            (row.origin == id)
                .then_some(&row.declarations)
                .into_iter()
                .flatten(),
        )
        .any(|site| {
            view.written(site.span)
                .is_some_and(|(owner, span)| owner == file && span.start <= byte && byte < span.end)
        })
}

pub(super) fn rename(
    view: &SemanticModel,
    id: usize,
    replacement: &str,
) -> Result<Vec<(String, Span)>, Refusal> {
    check_cancelled()?;
    if !view.complete {
        return Err(INCOMPLETE);
    }
    let row = &view.named[id];
    if row.kind == "database" {
        return Err(Refusal::new("EXTERNAL_NAMESPACE", "A database name identifies a MATLAB workspace value. Rename cannot update that external value; no edits were proposed."));
    }
    if row.declarations.is_empty() {
        return Err(if row.kind == "named_subsample" {
            Refusal::new("UNRESOLVED_SUBSAMPLE", "This copy operand has no prior subsample declaration in its namespace. Rename cannot verify the runtime label; no edits were proposed.")
        } else {
            Refusal::new("UNRESOLVED_NAME", "This name has no accepted declaration in its namespace at the required stage. Rename cannot verify its binding; no edits were proposed.")
        });
    }
    if !is_legal_ident(replacement) {
        return Err(IDENTIFIER);
    }
    let affected = |other: &Fact| other.origin == row.origin;
    let namespaces: HashSet<_> = view
        .named
        .iter()
        .filter(|other| affected(other))
        .map(|other| other.namespace.clone())
        .collect();
    if replacement != row.name
        && view.named.iter().any(|other| {
            !affected(other)
                && other.name == replacement
                && other.kind == row.kind
                && namespaces.contains(&other.namespace)
        })
    {
        return Err(COLLISION);
    }
    let mut orders = HashSet::new();
    let mut targets = Vec::new();
    for (other_id, other) in view
        .named
        .iter()
        .enumerate()
        .filter(|(_, other)| affected(other))
    {
        if other.declarations.len() != 1 {
            return Err(BINDING);
        }
        for site in references(view, other_id, true) {
            let target = site.written.ok_or(MAPPING)?;
            view.literal_identifier(&target, &row.name)?;
            orders.insert(site.order);
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
    }
    if targets.is_empty() {
        return Err(MAPPING);
    }
    let mut tokens = view.model.expanded_tokens.clone();
    for (order, token) in view.model.expanded_tokens.iter().enumerate() {
        if crate::engine::control::cancelled() {
            return Err(CANCELLED);
        }
        if view
            .written(token.span)
            .is_some_and(|target| targets.contains(&target))
        {
            if !orders.contains(&order) {
                return Err(BINDING);
            }
            tokens[order].lexeme = Some(replacement.into());
        }
    }
    let (renamed, _) = crate::engine::language::parser::parse_expanded_controlled(
        &view.model.source,
        tokens,
        None,
        view.model.macro_work_limit,
    );
    if !crate::engine::workspace::model_map::parser_complete(&renamed) {
        return Err(BINDING);
    }
    let expected: Vec<_> = view
        .named
        .iter()
        .cloned()
        .map(|mut fact| {
            if affected(&fact) {
                fact.name = replacement.into();
            }
            fact
        })
        .collect();
    if expected != collect(&renamed) {
        return Err(Refusal::new("NAMESPACE_CAPTURE", "The new name would change another named reference or copy relation in an affected namespace; no edits were proposed."));
    }
    if view.model.symbol_type_events.len() != renamed.symbol_type_events.len()
        || view
            .model
            .symbol_type_events
            .iter()
            .zip(&renamed.symbol_type_events)
            .any(|(before, after)| {
                view.model.name(before.name) != renamed.name(after.name)
                    || before.kind != after.kind
                    || before.changed != after.changed
            })
        || view.model.statements.len() != renamed.statements.len()
        || view
            .model
            .statements
            .iter()
            .zip(&renamed.statements)
            .any(|(before, after)| {
                before.kind != after.kind
                    || before.native != after.native
                    || before.token_range != after.token_range
            })
    {
        return Err(BINDING);
    }
    check_cancelled()?;
    Ok(targets)
}
