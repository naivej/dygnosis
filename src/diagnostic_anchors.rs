//! Display ranges derived only after diagnostic selection and suppression.
//! Full parser construct spans remain available to every analysis reader.

use crate::diagnostic::Diagnostic;
use crate::model::{Model, Statement};
use crate::span::Span;

pub(crate) fn apply(
    model: &Model,
    diagnostics: &mut [Diagnostic],
    mut safely_mapped: impl FnMut(Span, &str) -> bool,
) {
    for diagnostic in diagnostics {
        let mut owners = model
            .statements
            .iter()
            .filter(|statement| owns_summary(model, diagnostic, statement));
        if let Some(owner) = owners.find(|owner| {
            written_keyword(model, owner) && safely_mapped(owner.keyword_span, &owner.name)
        }) {
            diagnostic.span = owner.keyword_span;
        }
    }
}

fn owns_summary(model: &Model, diagnostic: &Diagnostic, statement: &Statement) -> bool {
    if matches!(diagnostic.code.as_str(), "E188" | "W013" | "E208") {
        return statement.name == "model" && statement.dimension.is_none();
    }
    if matches!(diagnostic.code.as_str(), "E026" | "E027") {
        // Use the same first written declaration as the clash producer. Its
        // token execution position identifies the statement across includes
        // and repeated macro spans; the child's source span does not.
        return model
            .deterministic_exogenous
            .first()
            .and_then(|declaration| {
                model.written_declarations.iter().find(|written| {
                    written.written_kind == "varexo_det"
                        && written.declaration.parse_order == declaration.parse_order
                })
            })
            .is_some_and(|written| {
                statement.id == written.statement_id && statement.name == "varexo_det"
            });
    }
    if diagnostic.code == "E171" {
        return model.occbin_constraints_blocks.len() == 1
            && statement.name == "occbin_constraints"
            && model.occbin_constraints_blocks[0].start == statement.span.start;
    }
    if matches!(diagnostic.code.as_str(), "E192" | "W208") {
        // The producer retains the checked dimension independently of the
        // written span, which can be shared by several macro executions.
        // A declaration fallback with no model opener remains unchanged.
        return diagnostic
            .model_dimension
            .as_deref()
            .is_some_and(|dimension| {
                statement.name == "model" && statement.dimension.as_deref() == Some(dimension)
            });
    }
    // Some legacy block spans include following whitespace. Its start and
    // parsed construct kind identify the owner without changing that span.
    if statement.span.start != diagnostic.span.start {
        return false;
    }
    match (diagnostic.code.as_str(), statement.name.as_str()) {
        ("I050", "model")
        | ("W042", "steady_state_model")
        | ("W052" | "E217", "initval")
        | ("E218", "initval" | "endval")
        | ("E241", "histval")
        | ("W092" | "E258", "varobs")
        | ("E259", "varexobs")
        | ("W203", "osr_params")
        | ("E100" | "E104", "planner_objective")
        | ("E203", "ramsey_constraints")
        | ("E170", "occbin_constraints")
        | ("E212", "estimated_params" | "shocks")
        | ("E254", "osr_params_bounds")
        | ("E297" | "E298" | "E299" | "E300", "ramsey_model" | "ramsey_policy")
        | ("E356" | "E357", "svar_identification")
        | ("E438", "pac_target_info")
        | ("E474", "optim_weights")
        | ("E322", "external_function")
        | ("E338", "data")
        | ("E341", "ms_estimation")
        | ("E342", "conditional_forecast")
        | ("E345", "markov_switching")
        | ("E363" | "E365", "svar")
        | ("E382" | "E383", "method_of_moments")
        | ("E439", "var_model" | "trend_component_model" | "var_expectation_model" | "pac_model") => {
            true
        }
        ("E001", _) => statement.complete && empty_moment_block(model, statement),
        ("E441", "var_expectation_model") => model
            .semi_structural_commands
            .iter()
            .find(|command| command.span == diagnostic.span)
            .is_some_and(|command| {
                !command
                    .options
                    .iter()
                    .any(|option| matches!(option.name.as_str(), "variable" | "expression"))
            }),
        _ => false,
    }
}

fn empty_moment_block(model: &Model, statement: &Statement) -> bool {
    match statement.name.as_str() {
        "matched_moments" => {
            model.matched_moments_blocks.contains(&statement.span)
                && !model.matched_moments.iter().any(|row| {
                    statement.span.start <= row.span.start && row.span.end <= statement.span.end
                })
        }
        "matched_irfs" => model
            .matched_irfs
            .iter()
            .any(|block| block.span == statement.span && block.rows.is_empty()),
        "matched_irfs_weights" => model
            .matched_irfs_weights
            .iter()
            .any(|block| block.span == statement.span && block.rows.is_empty()),
        "moment_calibration" => model
            .moment_calibration
            .iter()
            .any(|block| block.span == statement.span && block.rows.is_empty()),
        "irf_calibration" => model
            .irf_calibration
            .iter()
            .any(|block| block.span == statement.span && block.rows.is_empty()),
        _ => false,
    }
}

fn written_keyword(model: &Model, statement: &Statement) -> bool {
    model
        .source
        .get(statement.keyword_span.start as usize..statement.keyword_span.end as usize)
        .is_some_and(|text| text.eq_ignore_ascii_case(&statement.name))
}
