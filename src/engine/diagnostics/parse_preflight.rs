//! Parse refusal receipts shared by accepted facts and coverage explanations.
//! This pass does not call full diagnostics or parser completeness.
use crate::engine::language::model::Model;
use crate::engine::language::span::Span;
use std::ops::Range;

pub(crate) fn callback_order(model: &Model, key: &'static str, index: usize) -> usize {
    model
        .fact_receipts
        .get(key)
        .and_then(|rows| rows.get(index))
        .map_or(0, |receipt| receipt.parse_order)
}

pub(crate) fn statement_callback_order(model: &Model, token_index: usize) -> usize {
    model
        .statements
        .iter()
        .find(|statement| statement.token_range.contains(&token_index))
        .map_or(0, |statement| statement.token_range.end)
}

/// Locate a diagnostic action only inside its retained parser execution owner.
/// The owner range is required because macro copies can share written spans.
pub(crate) fn within(model: &Model, owner: Range<usize>, span: Span, after: bool) -> Option<usize> {
    let tokens = model.expanded_tokens.get(owner.clone())?;
    let mut matching = tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| token.span.start < span.end && span.start < token.span.end);
    if after {
        matching
            .next_back()
            .map(|(index, _)| owner.start + index + 1)
    } else {
        matching.next().map(|(index, _)| owner.start + index)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ParseRefusal {
    /// Retained expanded-token execution index, never a written byte offset.
    pub order: usize,
    pub cause: String,
    pub span: crate::engine::language::span::Span,
}

pub(crate) fn first_refusal(model: &Model) -> Option<ParseRefusal> {
    crate::engine::analysis::named_callbacks::collect(model).refusal
}

/// Ordinary Parse owners supply exact action orders beside their diagnostics.
pub(crate) fn ordinary_refusal(model: &Model) -> Option<ParseRefusal> {
    let to_order = |order| {
        model
            .expanded_token_origins
            .iter()
            .position(|range| range.start <= order && order < range.end)
            .unwrap_or(model.expanded_tokens.len())
    };
    let mut rows: Vec<_> = super::check_e020::parse_refusals(model)
        .into_iter()
        .filter(|(diagnostic, _)| diagnostic.severity == super::Severity::Error)
        .map(|(diagnostic, order)| ParseRefusal {
            order,
            cause: diagnostic.message,
            span: diagnostic.span,
        })
        .collect();
    rows.extend(
        super::check_e030::parse_receipts(model)
            .into_iter()
            .chain(super::check_d_surgery::parse_receipts(model))
            .filter(|(diagnostic, _)| diagnostic.severity == super::Severity::Error)
            .map(|(diagnostic, order)| ParseRefusal {
                order,
                cause: diagnostic.message,
                span: diagnostic.span,
            }),
    );
    if let Some((diagnostic, order)) = super::check_d_ms::parse_refusal(model) {
        rows.push(ParseRefusal {
            order: to_order(order),
            cause: diagnostic.message,
            span: diagnostic.span,
        });
    }
    rows.extend(
        super::check_d_hank::parse_refusals(model)
            .into_iter()
            .chain(super::check_e060::parse_refusals(model))
            .chain(super::check_d_open::parse_refusals(model))
            .chain(super::check_d_block::parse_refusals(model))
            .chain(super::check_occbin::parse_refusals(model))
            .chain(super::check_w090::parse_refusals(model))
            .chain(super::check_w130::parse_refusals(model))
            .chain(super::check_w100::parse_refusals(model))
            .chain(super::check_context::parse_refusals(model))
            .chain(super::check_d_shocks::parse_refusals(model))
            .chain(super::check_mom::parse_refusals(model))
            .map(|(diagnostic, order)| ParseRefusal {
                order,
                cause: diagnostic.message,
                span: diagnostic.span,
            }),
    );
    rows.extend(
        super::check_w110::completed_parse_duplicates(model)
            .into_iter()
            .map(|(diagnostic, order)| ParseRefusal {
                order: to_order(order),
                cause: diagnostic.message,
                span: diagnostic.span,
            }),
    );
    rows.into_iter().min_by_key(|row| row.order)
}

/// Symbol declarations execute sequentially inside a vector declaration. An
/// earlier accepted name remains live when a later name refuses with E030.
pub(crate) fn accepted_symbol_events(
    model: &Model,
    stop: Option<usize>,
) -> std::collections::HashSet<usize> {
    let conflicting = super::check_e030::conflicting_declaration_events(model);
    let event_stop = stop.and_then(|stop| {
        conflicting
            .iter()
            .filter(|(_, order)| *order == stop)
            .map(|(event, _)| *event)
            .min()
    });
    model
        .symbol_type_events
        .iter()
        .enumerate()
        .filter_map(|(index, _)| {
            if event_stop.is_some_and(|refused| index >= refused) {
                return None;
            }
            let order = super::check_e030::declaration_callback_order(model, index);
            stop.is_none_or(|stop| {
                order < stop
                    || order == stop
                        && (model.early_type_event_actions.contains(&index)
                            || event_stop.is_some_and(|refused| index < refused))
            })
            .then_some(index)
        })
        .collect()
}
