//! Original-source masking preserves offsets for formatting and syntax recovery.

use crate::engine::analysis::source_inventory::inert_source_from_tokens;
use crate::engine::language::macro_expand::scan_macro_tokens;

/// Mask macro directives while preserving source offsets for formatting and syntax recovery.
pub(crate) fn inert_source(text: &str) -> String {
    inert_source_from_tokens(text, &scan_macro_tokens(text))
}
