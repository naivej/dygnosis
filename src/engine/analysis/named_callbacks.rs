//! Successful named Parse actions over retained grammar/diagnostic receipts.
//! Queries and completeness use this one gate; recovery rows remain unchanged.
use std::collections::HashSet;

use crate::engine::diagnostics::parse_preflight::ParseRefusal;
use crate::engine::language::model::{Model, NamedSlotOwner};

#[derive(Default)]
pub(crate) struct Accepted {
    pub slots: HashSet<usize>,
    pub dotted: HashSet<usize>,
    pub subsamples: HashSet<usize>,
    pub complete: bool,
    pub refusal: Option<ParseRefusal>,
}

pub(crate) fn collect(model: &Model) -> Accepted {
    let pac = crate::engine::diagnostics::check_d_pac::named_callbacks(model);
    let dotted = crate::engine::diagnostics::check_d_ms::named_callbacks(model);
    let mut frontier = pac.frontier.into_iter().chain(dotted.frontier).min();
    let mut refusal = pac.frontier.map(|order| ParseRefusal {
        order,
        span: pac.cause_span.unwrap_or_default(),
        cause: pac
            .cause
            .clone()
            .unwrap_or_else(|| "A named model Parse callback did not complete.".into()),
    });
    if let Some(dotted) = dotted.refusal.clone()
        && refusal
            .as_ref()
            .is_none_or(|prior| dotted.order < prior.order)
    {
        refusal = Some(dotted);
    }
    if let Some(ordinary) = crate::engine::diagnostics::parse_preflight::ordinary_refusal(model)
        && refusal
            .as_ref()
            .is_none_or(|prior| ordinary.order < prior.order)
    {
        refusal = Some(ordinary);
    }
    if let Some(stop) = &refusal {
        frontier = Some(frontier.map_or(stop.order, |prior| prior.min(stop.order)));
    }
    let to_order = |order| {
        model
            .expanded_token_origins
            .iter()
            .position(|range| range.start <= order && order < range.end)
            .unwrap_or(model.expanded_tokens.len())
    };
    for (issue, order) in &model.parse_issue_callbacks {
        if matches!(
            issue.kind,
            crate::engine::language::model::ParseIssueKind::KeywordTypo { .. }
        ) {
            continue;
        }
        frontier = Some(frontier.map_or(*order, |prior| prior.min(*order)));
        if refusal.as_ref().is_none_or(|prior| *order < prior.order)
            && let Some(diagnostic) =
                crate::engine::diagnostics::check_parse::recorded_refusal(model, issue)
        {
            refusal = Some(ParseRefusal {
                order: *order,
                cause: diagnostic.message,
                span: issue.span,
            });
        }
    }
    for shape in crate::engine::diagnostics::check_d_ms::shape_refuses(model) {
        let order = shape
            .parse_order
            .or_else(|| shape.parse_execution.as_ref().map(|owner| owner.start))
            .map(to_order)
            .unwrap_or(0);
        frontier = Some(frontier.map_or(order, |prior| prior.min(order)));
        if refusal.as_ref().is_none_or(|prior| order < prior.order) {
            refusal = Some(ParseRefusal {
                order,
                span: shape.span,
                cause: crate::engine::diagnostics::check_d_ms::shape_refuse_message(shape),
            });
        }
    }
    let mut accepted = Accepted {
        complete: pac.complete && dotted.complete && refusal.is_none(),
        refusal,
        dotted: dotted.dotted,
        subsamples: dotted.subsamples,
        ..Default::default()
    };
    let mut dimensions = HashSet::new();
    let mut databases = HashSet::new();
    let mut refused_dimension_lists = HashSet::new();
    let mut refused_database_lists = HashSet::new();
    let mut slots: Vec<_> = model.named_slots.iter().enumerate().collect();
    slots.sort_by_key(|(_, slot)| (slot.callback_order, slot.order));
    for (index, slot) in slots {
        if crate::engine::control::cancelled() {
            return Default::default();
        }
        let succeeded =
            match slot.owner {
                NamedSlotOwner::DimensionDeclaration => {
                    if refused_dimension_lists.contains(&slot.callback_order) {
                        false
                    } else if dimensions.insert(slot.name) {
                        true
                    } else {
                        refused_dimension_lists.insert(slot.callback_order);
                        false
                    }
                }
                NamedSlotOwner::DatabaseDeclaration => {
                    if refused_database_lists.contains(&slot.callback_order) {
                        false
                    } else if databases.insert(slot.name) {
                        true
                    } else {
                        refused_database_lists.insert(slot.callback_order);
                        false
                    }
                }
                NamedSlotOwner::DimensionLookup => dimensions.contains(&slot.name),
                NamedSlotOwner::DatabaseReference => databases.contains(&slot.name),
                NamedSlotOwner::HeterogeneousShock(owner) => dimensions.contains(&slot.name)
                    && model.shock_blocks.get(owner).is_some_and(|block| {
                        !crate::engine::diagnostics::check_d_hank::heterogeneous_shock_row_unknown(
                            model, block,
                        )
                    }),
                NamedSlotOwner::ModelCommand(owner) => model
                    .semi_structural_commands
                    .get(owner)
                    .is_some_and(|command| pac.commands.contains(&command.parse_order)),
                NamedSlotOwner::ModelOperator(owner) => pac.operators.contains(&owner),
                // begin_pac_target_info only selects its name and the dynamic tree.
                // Field checks and named model lookup occur in later stages.
                NamedSlotOwner::PacTargetInfo => true,
            };
        if succeeded {
            accepted.slots.insert(index);
        } else {
            accepted.complete = false;
            let order = match slot.owner {
                NamedSlotOwner::ModelCommand(owner) => {
                    model.semi_structural_commands[owner].active_tokens.end
                }
                _ => slot.callback_order,
            };
            frontier = Some(frontier.map_or(order, |prior| prior.min(order)));
            if accepted
                .refusal
                .as_ref()
                .is_none_or(|prior| order < prior.order)
            {
                let label = model.name(slot.name);
                let cause = match slot.owner {
                    NamedSlotOwner::DimensionDeclaration => format!("Heterogeneity dimension '{label}' already declared"),
                    NamedSlotOwner::DatabaseDeclaration => format!("Database '{label}' already declared"),
                    NamedSlotOwner::DimensionLookup | NamedSlotOwner::HeterogeneousShock(_) => format!("The heterogeneity dimension '{label}' lookup did not complete its Parse callback."),
                    NamedSlotOwner::DatabaseReference => format!("The database '{label}' is not declared before this reference."),
                    _ => "A named Parse callback did not complete.".into(),
                };
                accepted.refusal = Some(ParseRefusal {
                    order,
                    cause,
                    span: slot.span,
                });
            }
        }
    }
    if let Some(frontier) = frontier {
        accepted.slots.retain(|index| {
            let slot = &model.named_slots[*index];
            let order = match slot.owner {
                NamedSlotOwner::ModelCommand(owner) => {
                    model.semi_structural_commands[owner].active_tokens.end
                }
                _ => slot.callback_order,
            };
            order <= frontier
        });
        accepted
            .dotted
            .retain(|index| model.dotted_statements[*index].active_tokens.end <= frontier);
        accepted.subsamples.retain(|index| {
            model
                .fact_receipts
                .get("subsamples")
                .and_then(|rows| rows.get(*index))
                .is_some_and(|receipt| {
                    model.dotted_statements.iter().any(|statement| {
                        statement.kind == crate::engine::language::model::DottedKind::Subsamples
                            && statement.active_tokens.start == receipt.parse_order
                            && statement.active_tokens.end <= frontier
                    })
                })
        });
    }
    accepted
}
