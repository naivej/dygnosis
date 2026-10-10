//! Typed comparison detail shared by editor and agent transports.
//!
//! Producers compare domain values; locations and parser IDs only identify
//! occurrences. Legacy rows retain their original JSON pointers.

mod budget;
pub mod equations;
mod expression_values;
mod families;
mod fields;
pub(crate) mod occurrences;
mod priors;
mod schema;
mod source;
mod surfaces;
#[cfg(test)]
mod tests;

pub use budget::{alignment_availability, enforce_output_budget};
pub use schema::*;
pub use source::{
    populate_captured_sources, CaptureBoundary, CapturedSourceInput, SourceFilePair,
    SourceIdentityProof, SourceInputError,
};

use crate::model::Model;
use crate::model_diff::ModelDiff;

pub(crate) fn populate(before: &Model, after: &Model, diff: &mut ModelDiff) {
    fields::populate(before, after, diff);
    equations::populate(before, after, diff);
    let mut claims = occurrences::TokenClaims::with_expression_budget(
        diff.semantic.budgets.serialized_output_bytes,
    );
    surfaces::populate(before, after, diff, &mut claims);
    priors::populate(before, after, diff, &mut claims);
    families::populate(before, after, diff, &mut claims);
    surfaces::populate_commands(before, after, diff, &claims);
    occurrences::populate_block_context(before, after, diff);
}
