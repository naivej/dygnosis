//! Added Warning W211: a lead on an ordinary aggregate exogenous variable.
//!
//! Always enabled. Intentional leads stay legal; this is a timing review, not
//! a Dynare refusal, and it has no automatic fix.

use crate::diagnostic::{Diagnostic, Severity};
use crate::intern::Name;
use crate::model::Model;
use crate::span::Span;

pub(crate) fn exogenous_leads(model: &Model) -> Vec<Diagnostic> {
    if crate::check_writing::model_structure_incomplete(model)
        || !stochastic_without_perfect_foresight(model)
    {
        return Vec::new();
    }
    let mut out = Vec::new();
    for eq in &model.equations {
        for ident in model.ident_refs(eq) {
            if ident.timing <= 0 || !ordinary_aggregate_varexo(model, ident.name) {
                continue;
            }
            let end = ident
                .timing_span
                .map(|span| span.end)
                .unwrap_or(ident.span.end);
            let name = model.name(ident.name);
            out.push(Diagnostic::new(
                Span {
                    start: ident.span.start,
                    end,
                },
                Severity::Warning,
                "W211",
                format!(
                    "Exogenous variable '{name}' is used with a lead. Check the intended shock timing."
                ),
            ));
        }
    }
    out
}

fn stochastic_without_perfect_foresight(model: &Model) -> bool {
    let stochastic = model.stoch_simul_span.is_some() || model.estimation_span.is_some();
    stochastic && !perfect_foresight_command(model)
}

fn perfect_foresight_command(model: &Model) -> bool {
    !model.simul_spans.is_empty()
        || model.perfect_foresight_solver_span.is_some()
        || model.pfee_solver_span.is_some()
        || model.perfect_foresight_setup_span.is_some()
        || model.pfee_setup_span.is_some()
        || model.perfect_foresight_controlled_paths_span.is_some()
}

fn ordinary_aggregate_varexo(model: &Model, name: Name) -> bool {
    if model
        .deterministic_exogenous
        .iter()
        .any(|decl| decl.name == name)
    {
        return false;
    }
    model
        .exogenous
        .iter()
        .any(|decl| decl.name == name && decl.heterogeneity.is_none())
}
