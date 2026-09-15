//! W160 named unresolved companion files, and I050 quiet when SS resolves.

use crate::companion::{CompanionKind, CompanionRecord};
use crate::diagnostic::{Diagnostic, Severity};

/// One Warning per companion record whose path did not resolve.
pub fn check_w160(records: &[CompanionRecord]) -> Vec<Diagnostic> {
    records
        .iter()
        .filter(|r| r.path.is_none())
        .map(|r| {
            Diagnostic::new(
                r.named_in,
                Severity::Warning,
                "W160",
                format!(
                    "Named companion '{}' ({}) was not found. Fix: add the file next to this .mod, correct the path, or add its directory to the search paths.",
                    r.name,
                    r.kind.as_str()
                ),
            )
        })
        .collect()
}

/// Drop I050 when a `steady_state_file` companion resolved on disk.
pub fn quiet_i050(diags: &mut Vec<Diagnostic>, records: &[CompanionRecord]) {
    if records
        .iter()
        .any(|r| r.kind == CompanionKind::SteadyStateFile && r.path.is_some())
    {
        diags.retain(|d| d.code != "I050");
    }
}
