//! Typed comparison detail shared by editor and agent transports.
//!
//! Producers compare domain values; locations and parser IDs only identify
//! occurrences. Legacy rows retain their original JSON pointers.

mod budget;
pub mod equations;
mod fields;
mod schema;
mod source;
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
}
