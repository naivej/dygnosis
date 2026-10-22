//! Written folding is a projection of the shared file-local source inventory.

use crate::engine::analysis::source_inventory::{
    SourceFold, SourceInventory, inert_source_from_tokens,
};
use crate::engine::language::macro_expand::scan_macro_tokens;
use crate::engine::language::model::MacroDirective;

/// Compatibility mask for original-source formatting and syntax recovery.
pub(crate) fn inert_source(text: &str) -> String {
    inert_source_from_tokens(text, &scan_macro_tokens(text))
}

/// Uncached compatibility entry point. Workspace consumers use their retained
/// SourceInventory, so folding and other source features share the same parse.
#[allow(dead_code)]
pub(crate) fn source_folds(text: &str, _directives: &[MacroDirective]) -> Vec<SourceFold> {
    SourceInventory::build(text, Default::default(), None)
        .map(|inventory| inventory.folds)
        .unwrap_or_default()
}
