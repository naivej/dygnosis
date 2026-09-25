//! Thin Dynare analysis library.
//!
//! Scope follows the 0.6.0 cut line in `dev_logs/0.6/0.6.0/masterplan.md`
//! (method of moments, matched moments and IRFs, and IRF and moment calibration).
//! Dynare compute (steady state, BK, identification, MATLAB) stays out.

pub mod auto_fix;
pub mod catalog;
pub mod check_clash;
pub mod check_context;
pub mod check_d_block;
pub mod check_d_hank;
pub mod check_d_ms;
pub mod check_d_open;
pub mod check_d_pac;
pub mod check_d_shocks;
pub mod check_d_surgery;
pub mod check_e020;
pub mod check_e030;
pub mod check_e060;
pub mod check_estimated_params;
pub mod check_estimation;
pub mod check_mom;
pub mod check_occbin;
pub mod check_parse;
pub mod check_symbol_list;
pub mod check_w010;
pub mod check_w070;
pub mod check_w090;
pub mod check_w100;
pub mod check_w110;
pub mod check_w120;
pub mod check_w130;
pub mod check_w160;
mod command_skip;
pub mod companion;
mod diag_shape;
pub mod diagnostic;
pub mod e010;
pub mod equations;
pub mod expand;
pub mod explain;
pub mod expr;
pub(crate) mod extract;
pub mod format;
pub mod include_resolver;
pub mod intern;
mod lag_fold;
pub mod lexer;
pub mod macro_expand;
pub mod mcp;
pub mod model;
pub mod model_diff;
pub mod model_info;
pub mod parser;
pub mod preprocessor;
pub mod refs;
pub mod server;
mod shape_gate;
pub mod span;
mod suppress;
pub mod workspace;

pub use auto_fix::{apply_fix, auto_fix};
pub use catalog::{
    command_options, is_known_command, list_options, option_doc, ListOptions, NamedOption,
};
pub use check_clash::check_clash;
pub use check_context::check_context;
pub use check_d_block::check_d_block;
pub use check_d_ms::check_d_ms;
pub use check_d_open::{check_d_open, check_workspace_d_open};
pub use check_d_surgery::check_d_surgery;
pub use check_e020::check_e020;
pub use check_e030::check_e030;
pub use check_e060::{
    check_e060, check_e060_family, check_e060_family_on_model, check_e061, check_e062, check_e063,
    check_e064, check_e065, check_w061,
};
pub use check_estimated_params::check_estimated_params;
pub use check_estimation::check_estimation;
pub use check_mom::check_mom;
pub use check_occbin::check_occbin;
pub use check_parse::{check_parse, has_structural_error};
pub use check_symbol_list::check_symbol_list;
pub use check_w010::{
    check_w010, check_w010_family, check_w011, check_w012, check_w020, check_w021, check_w022,
};
pub use check_w070::check_w070;
pub use check_w090::check_w090;
pub use check_w100::check_w100;
pub use check_w110::check_w110;
pub use check_w120::{check_w120, check_w120_family, check_w121, check_w122};
pub use check_w130::check_w130;
pub use check_w160::{check_w160, quiet_i050};
pub use companion::{CompanionKind, CompanionRecord};
pub use diagnostic::{analyze, check_file, format_check_lines, Diagnostic, Severity, TextEdit};
pub use e010::check_e010;
pub use equations::{
    count_gap, equations, explain_equation, CountGap, EquationIdent, EquationRow, IdentClass,
};
pub use expand::{expand_report, EquationOrigin, ExpandReport, OriginFrame};
pub use expr::{BinOp, Expr, ExprArena, ExprId, ExprKind, IdentRef, UnOp};
pub use format::{format_range, format_text};
pub use include_resolver::{find_workspace_root, resolve_companion_path, resolve_include_path};
pub use mcp::{
    dynare_auto_fix, dynare_compare_models, dynare_diagnose, dynare_equations, dynare_expand,
    dynare_explain, dynare_find_references, dynare_list_diagnostic_codes, dynare_list_options,
    dynare_model_info, dynare_related_files, dynare_rename, registered_tool_names, tools_list_json,
    DiagnosticCodeItem, McpDiagnostic, McpReference, McpWorkspaceReference,
};
pub use model::{
    CalibrationRange, Complementarity, ComplementarityTriple, DeprecatedOption, EstimatedParam,
    EstimatedParamKind, IncludeDirective, IncludePathDirective, IrfCalibrationBlock,
    IrfCalibrationRow, MacroDirective, MacroInterp, MatchedIrfsBlock, MatchedIrfsRow,
    MatchedIrfsWeight, MatchedIrfsWeightsBlock, MatchedMoment, Model, MomStatement,
    MomentCalibrationBlock, MomentCalibrationRow, ObservedVar, OccbinConstraint, OccbinExpr,
    ParseIssue, ParseIssueKind, ParseSummary, PolicyCommand, ShockKind, ShockStmt,
    ShocksSemiFamily,
};
pub use model_diff::{
    compare_models, compare_models_with_sources, CompareSource, EquationChange, IndexedEquation,
    ModelDiff, ParameterChange, ShockSetting, ShockSetupChange, SourceLocation, UnmatchedSameName,
    WrittenPeriod,
};
pub use model_info::{
    assigned_number, classify_variable_timing, format_structure_lens, format_timing_line,
    structure_summary, StructureSummary, TimingClass, TimingInfo,
};
pub use parser::parse;

/// Every Dynare block opener the language has, in table order. The pin's lexer
/// scopes these rules to `INITIAL`, so a spelling the file declares is an ordinary
/// symbol inside a body.
pub fn block_openers() -> &'static [&'static str] {
    parser::BLOCK_OPENERS
}
#[doc(hidden)]
pub use preprocessor::{find_preprocessor, run_preprocessor, JsonStage, PreprocessorResult};
pub use refs::{
    ident_at, is_legal_ident, occurrences, option_command_at, option_owner_at, rename_in_text,
};
pub use workspace::{CycleRecord, IncludeRecords, ResolvedInclude, UnresolvedInclude, Workspace};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
