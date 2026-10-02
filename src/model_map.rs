//! Written model structure. Presentation metadata does not widen the parser.

use crate::span::Span;

/// Complete recovered Dynare structure. Native MATLAB lines do not require
/// Dynare's statement semicolon; their arithmetic proof is a separate concern.
pub fn parser_complete(model: &crate::model::Model) -> bool {
    model.parse_issues.is_empty()
        && model
            .statements
            .iter()
            .all(|statement| statement.native || statement.complete)
}

/// A verified portion in one written file. Separate include segments never
/// become one range merely because they share a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WrittenSegment {
    pub file: Option<String>,
    pub span: Span,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceOccurrence {
    pub segments: Vec<WrittenSegment>,
    pub anchor: Option<WrittenSegment>,
    pub origin_frames: Vec<SourceFrame>,
}

/// Macro context with independently clipped written portions. The legacy
/// equation-origin envelope remains unchanged in `expand`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFrame {
    pub kind: String,
    pub variable: Option<String>,
    pub value: Option<String>,
    pub segments: Vec<WrittenSegment>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EquationOccurrence {
    /// Expanded token position, unique in this revision even at repeated sites.
    pub id: usize,
    pub statement_id: usize,
    pub name: String,
    pub dimension: Option<String>,
    pub number: Option<usize>,
    pub active: bool,
    pub local: bool,
    pub static_only: bool,
    pub source: SourceOccurrence,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WrittenModelMap {
    /// Recovered structure remains available when counts are not authoritative.
    pub complete: bool,
    pub statements: Vec<SourceOccurrence>,
    pub declarations: Vec<SourceOccurrence>,
    pub equations: Vec<EquationOccurrence>,
    /// Captured declaration type-event indices with proven token occurrences.
    pub type_events: Vec<(usize, SourceOccurrence)>,
}

/// Complete inventory of block branches the parser already recognizes.
pub const SUPPORTED_BLOCKS: &[&str] = &[
    "model",
    "model_replace",
    "steady_state_model",
    "initval",
    "endval",
    "histval",
    "filter_initial_state",
    "shocks",
    "mshocks",
    "heteroskedastic_shocks",
    "shock_paths",
    "perfect_foresight_controlled_paths",
    "conditional_forecast_paths",
    "estimated_params",
    "estimated_params_init",
    "estimated_params_bounds",
    "estimated_params_remove",
    "observation_trends",
    "deterministic_trends",
    "matched_moments",
    "matched_irfs",
    "matched_irfs_weights",
    "generate_irfs",
    "irf_calibration",
    "moment_calibration",
    "optim_weights",
    "osr_params_bounds",
    "ramsey_constraints",
    "homotopy_setup",
    "occbin_constraints",
    "svar_identification",
    "shock_groups",
    "init2shocks",
    "pac_target_info",
    "epilogue",
    "verbatim",
    "priors",
];

/// Category keys consumed by clients, with the approved tint defaults.
pub const BLOCK_CATEGORIES: &[(&str, &str)] = &[
    ("model.aggregate", "model"),
    ("model.heterogeneous", "model"),
    ("model_replace", "subtle"),
    ("steady_state_model", "subtle"),
    ("initval", "subtle"),
    ("endval.standard", "subtle"),
    ("endval.learnt_in", "subtle"),
    ("histval", "subtle"),
    ("filter_initial_state", "subtle"),
    ("shocks.standard", "subtle"),
    ("shocks.surprise", "subtle"),
    ("shocks.learnt_in", "subtle"),
    ("shocks.heterogeneous", "subtle"),
    ("mshocks.standard", "subtle"),
    ("mshocks.learnt_in", "subtle"),
    ("heteroskedastic_shocks", "subtle"),
    ("shock_paths.standard", "subtle"),
    ("shock_paths.learnt_in", "subtle"),
    ("perfect_foresight_controlled_paths.standard", "subtle"),
    ("perfect_foresight_controlled_paths.learnt_in", "subtle"),
    ("conditional_forecast_paths", "subtle"),
    ("estimated_params", "subtle"),
    ("estimated_params_init", "subtle"),
    ("estimated_params_bounds", "subtle"),
    ("estimated_params_remove", "subtle"),
    ("observation_trends", "subtle"),
    ("deterministic_trends", "subtle"),
    ("matched_moments", "subtle"),
    ("matched_irfs", "subtle"),
    ("matched_irfs_weights", "subtle"),
    ("generate_irfs", "subtle"),
    ("irf_calibration", "subtle"),
    ("moment_calibration", "subtle"),
    ("optim_weights", "subtle"),
    ("osr_params_bounds", "subtle"),
    ("ramsey_constraints", "subtle"),
    ("homotopy_setup", "subtle"),
    ("occbin_constraints", "subtle"),
    ("svar_identification", "subtle"),
    ("shock_groups", "subtle"),
    ("init2shocks", "subtle"),
    ("pac_target_info", "subtle"),
    ("epilogue", "subtle"),
    ("verbatim", "subtle"),
    ("priors", "subtle"),
];

pub(crate) fn is_supported_block(name: &str) -> bool {
    SUPPORTED_BLOCKS.contains(&name)
}

pub(crate) fn block_category(name: &str, subtype: Option<&str>) -> Option<&'static str> {
    let category = match name {
        "model" => {
            if subtype == Some("heterogeneous") {
                "model.heterogeneous"
            } else {
                "model.aggregate"
            }
        }
        "endval" => {
            if subtype == Some("learnt_in") {
                "endval.learnt_in"
            } else {
                "endval.standard"
            }
        }
        "shocks" => match subtype {
            Some("heterogeneous") => "shocks.heterogeneous",
            Some("learnt_in") => "shocks.learnt_in",
            Some("surprise") => "shocks.surprise",
            _ => "shocks.standard",
        },
        "mshocks" => {
            if subtype == Some("learnt_in") {
                "mshocks.learnt_in"
            } else {
                "mshocks.standard"
            }
        }
        "shock_paths" => {
            if subtype == Some("learnt_in") {
                "shock_paths.learnt_in"
            } else {
                "shock_paths.standard"
            }
        }
        "perfect_foresight_controlled_paths" => {
            if subtype == Some("learnt_in") {
                "perfect_foresight_controlled_paths.learnt_in"
            } else {
                "perfect_foresight_controlled_paths.standard"
            }
        }
        other => other,
    };
    BLOCK_CATEGORIES
        .iter()
        .find_map(|&(key, _)| (key == category).then_some(key))
}
