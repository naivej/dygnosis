//! Dynare analysis library.
//!
//! Shared analysis of written `.mod` source for LSP and MCP.
//! Dynare compute (steady state, BK, identification, MATLAB) stays out.

mod engine;
pub mod lsp;
pub mod mcp;
mod test_support;

// Preserve the public Rust module paths while implementation follows its owner.
pub use engine::analysis::assignment_values;
pub use engine::analysis::equations;
pub use engine::analysis::model_info;
pub use engine::analysis::model_locals;
pub use engine::analysis::timing;
pub use engine::analysis_configuration;
pub use engine::catalog;
pub use engine::comparison as model_diff;
pub use engine::comparison::compare_snapshots;
pub use engine::comparison::semantic_diff;
pub use engine::diagnostics as diagnostic;
pub use engine::diagnostics::check_clash;
pub use engine::diagnostics::check_context;
pub use engine::diagnostics::check_d_block;
pub use engine::diagnostics::check_d_hank;
pub use engine::diagnostics::check_d_ms;
pub use engine::diagnostics::check_d_open;
pub use engine::diagnostics::check_d_pac;
pub use engine::diagnostics::check_d_shocks;
pub use engine::diagnostics::check_d_surgery;
pub use engine::diagnostics::check_e020;
pub use engine::diagnostics::check_e030;
pub use engine::diagnostics::check_e060;
pub use engine::diagnostics::check_estimated_params;
pub use engine::diagnostics::check_estimation;
pub use engine::diagnostics::check_mom;
pub use engine::diagnostics::check_occbin;
pub use engine::diagnostics::check_parse;
pub use engine::diagnostics::check_symbol_list;
pub use engine::diagnostics::check_w010;
pub use engine::diagnostics::check_w090;
pub use engine::diagnostics::check_w100;
pub use engine::diagnostics::check_w110;
pub use engine::diagnostics::check_w120;
pub use engine::diagnostics::check_w130;
pub use engine::diagnostics::check_w160;
pub use engine::diagnostics::e010;
pub use engine::diagnostics::explain;
pub use engine::editing::auto_fix;
pub use engine::editing::format;
pub use engine::editing::refs;
pub use engine::language::expand;
pub use engine::language::expr;
pub use engine::language::intern;
pub use engine::language::lexer;
pub use engine::language::macro_expand;
pub use engine::language::model;
pub use engine::language::parser;
pub use engine::language::span;
pub use engine::macro_work::MacroWorkLimit;
pub use engine::workspace;
pub use engine::workspace::check_walk;
pub use engine::workspace::companion;
pub use engine::workspace::include_resolver;
pub use engine::workspace::model_map;
pub use lsp as server;
pub use lsp::settings as server_settings;
pub use test_support::preprocessor;

pub use auto_fix::{apply_fix, auto_fix};
pub use catalog::{
    ListOptions, NamedOption, command_options, is_known_command, list_options, option_doc,
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
    check_e060_family, check_e060_family_on_model, check_e061, check_e062, check_e063, check_e064,
    check_e065, check_w061,
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
pub use check_w090::check_w090;
pub use check_w100::check_w100;
pub use check_w110::check_w110;
pub use check_w120::{check_w120, check_w120_family, check_w121, check_w122};
pub use check_w130::check_w130;
pub use check_w160::{check_w160, quiet_i050};
pub use companion::{CompanionKind, CompanionRecord};
pub use diagnostic::{
    Diagnostic, DiagnosticSet, Severity, TextEdit, WritingContext, WritingRows, analyze,
    check_file, check_file_with_origins,
};
pub use e010::check_e010;
pub use equations::{
    CountGap, EquationIdent, EquationRow, IdentClass, count_gap, equations, explain_equation,
};
pub use expand::{EquationOrigin, ExpandReport, OriginFrame, expand_report};
pub use expr::{BinOp, Expr, ExprArena, ExprId, ExprKind, IdentRef, UnOp};
pub use format::{format_range, format_text};
pub use include_resolver::{find_workspace_root, resolve_companion_path, resolve_include_path};
pub use mcp::{
    DiagnosticCodeItem, McpDiagnostic, McpReference, McpWorkspaceReference, dynare_auto_fix,
    dynare_compare_models, dynare_diagnose, dynare_equations, dynare_expand, dynare_explain,
    dynare_extract, dynare_find_references, dynare_format, dynare_list_diagnostic_codes,
    dynare_list_options, dynare_model_info, dynare_related_files, dynare_rename,
    dynare_workspace_diagnose, registered_tool_names, tools_list_json,
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
    CompareSource, EquationChange, IndexedEquation, ModelDiff, ParameterChange, ShockSetting,
    ShockSetupChange, SourceLocation, SymbolChange, SymbolSide, UnmatchedSameName, WrittenPeriod,
    compare_models, compare_models_with_sources,
};
pub use model_info::{
    StructureSummary, TimingClass, TimingInfo, assigned_number, classify_variable_timing,
    format_timing_line, structure_summary,
};
pub use parser::parse;

/// Every Dynare block opener the language has, in table order. The pin's lexer
/// scopes these rules to `INITIAL`, so a spelling the file declares is an ordinary
/// symbol inside a body.
pub fn block_openers() -> &'static [&'static str] {
    parser::BLOCK_OPENERS
}
#[doc(hidden)]
pub use preprocessor::{JsonStage, PreprocessorResult, find_preprocessor, run_preprocessor};
pub use refs::{
    ident_at, is_legal_ident, occurrences, option_command_at, option_owner_at, rename_in_text,
};
pub use workspace::{IncludeRecords, ResolvedInclude, UnresolvedInclude, Workspace};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
