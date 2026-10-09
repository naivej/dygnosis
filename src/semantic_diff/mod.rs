//! Typed comparison detail shared by editor and agent transports.
//!
//! Producers compare domain values; locations and parser IDs only identify
//! occurrences. Legacy rows retain their original JSON pointers.

mod budget;
mod fields;
mod schema;
#[cfg(test)]
mod tests;

pub use budget::{alignment_availability, enforce_output_budget};
pub use schema::*;

use crate::model::Model;
use crate::model_diff::ModelDiff;

pub(crate) fn populate_foundation(before: &Model, after: &Model, diff: &mut ModelDiff) {
    fields::populate(before, after, diff);
}
