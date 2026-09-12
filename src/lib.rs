//! Thin Dynare analysis library.
//!
//! Scope follows the 0.1.1 cut line in `dev_logs/0.1/0.1.1/masterplan.md`.
//! Dynare compute (steady state, BK, identification, MATLAB) stays out.

pub mod auto_fix;
pub mod catalog;
pub mod check_e020;
pub mod check_e030;
pub mod check_e060;
pub mod check_parse;
pub mod check_w010;
pub mod check_w070;
pub mod check_w090;
pub mod check_w100;
pub mod check_w110;
pub mod check_w120;
pub mod check_w130;
mod diag_shape;
pub mod diagnostic;
pub mod e010;
pub mod explain;
pub mod expr;
pub mod format;
pub mod include_resolver;
pub mod intern;
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
pub mod span;
pub mod workspace;

pub use auto_fix::{apply_fix, auto_fix};
pub use catalog::{
    command_options, is_known_command, list_options, option_doc, ListOptions, NamedOption,
};
pub use check_e020::check_e020;
pub use check_e030::check_e030;
pub use check_e060::{
    check_e060, check_e060_family, check_e060_family_on_model, check_e061, check_e062, check_e063,
    check_e064, check_e065, check_w061,
};
pub use check_parse::{check_parse, has_structural_error};
pub use check_w010::{
    check_w010, check_w010_family, check_w011, check_w012, check_w020, check_w021, check_w022,
};
pub use check_w070::check_w070;
pub use check_w090::check_w090;
pub use check_w100::check_w100;
pub use check_w110::check_w110;
pub use check_w120::{check_w120, check_w120_family, check_w121, check_w122};
pub use check_w130::check_w130;
pub use diagnostic::{analyze, check_file, format_check_lines, Diagnostic, Severity, TextEdit};
pub use e010::check_e010;
pub use expr::{BinOp, Expr, ExprArena, ExprId, ExprKind, IdentRef, UnOp};
pub use format::{format_range, format_text};
pub use include_resolver::{find_workspace_root, resolve_include_path};
pub use mcp::{
    dynare_auto_fix, dynare_compare_models, dynare_diagnose, dynare_explain,
    dynare_find_references, dynare_list_diagnostic_codes, dynare_list_options, dynare_model_info,
    dynare_rename, registered_tool_names, tools_list_json, DiagnosticCodeItem, McpDiagnostic,
    McpReference, McpWorkspaceReference,
};
pub use model::{
    DeprecatedOption, EstimatedParam, EstimatedParamKind, IncludeDirective, IncludePathDirective,
    MacroDirective, MacroInterp, Model, ObservedVar, ParseIssue, ParseIssueKind, ParseSummary,
    PolicyCommand, ShockKind, ShockStmt, ShocksSemiFamily,
};
pub use model_diff::{compare_models, EquationChange, ModelDiff, ParameterChange};
pub use model_info::{
    assigned_number, classify_variable_timing, format_structure_lens, format_timing_line,
    structure_summary, StructureSummary, TimingClass, TimingInfo,
};
pub use parser::parse;
pub use preprocessor::{
    find_preprocessor, maybe_run_and_reconcile, reconcile_diagnostics, run_preprocessor,
    run_preprocessor_structured, PreprocessorResult,
};
pub use refs::{ident_at, is_legal_ident, occurrences, option_command_at, rename_in_text};
pub use workspace::{CycleRecord, IncludeRecords, ResolvedInclude, UnresolvedInclude, Workspace};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
