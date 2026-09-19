//! Parsed `.mod` model. This is the seam diagnostic families and transports share.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::expr::{ExprArena, ExprId, IdentRef};
use crate::intern::{Interner, Name};
use crate::span::Span;

#[derive(Clone, Debug)]
pub struct Decl {
    pub name: Name,
    pub span: Span,
    pub long_name: Option<String>,
    pub log_transform: bool,
}

#[derive(Clone, Debug)]
pub struct Equation {
    pub text: String,
    pub name: String,
    pub span: Span,
    pub lhs: String,
    pub rhs: String,
    pub lhs_expr: Option<ExprId>,
    pub rhs_expr: Option<ExprId>,
    /// `#name = expr` model-local (parser bumped `Hash`).
    pub is_local: bool,
    /// Same Hash-token flag; duplicate-`#` E030 walks this field.
    pub model_local: bool,
    /// Leading `[...]` contained a bare `static` token.
    pub static_tag: bool,
    /// Leading `[...]` contained a bare `dynamic` token.
    pub dynamic_tag: bool,
    /// Lowercased `static` / `dynamic` from leading `[…]` tags (E050 tag class).
    pub tags: Vec<String>,
    /// Every `[key]` / `[key=value]` on this equation. Flag tags store `""`.
    pub tag_map: BTreeMap<String, String>,
    /// Tag keys that appeared twice on this equation (`[name='a', name='b']`).
    pub tag_twice: Vec<(String, Span)>,
    pub complementarity: Option<Complementarity>,
}

/// One `model_remove` / `model_replace` statement and what it matched.
#[derive(Clone, Debug)]
pub struct EquationSurgery {
    /// Keyword span of the statement.
    pub span: Span,
    /// `true` for a `model_replace` block.
    pub replace: bool,
    /// The tag sets the statement listed; a set matches when every pair matches.
    pub tag_sets: Vec<Vec<(String, String)>>,
    /// Tag sets that matched no equation.
    pub unmatched: Vec<Vec<(String, String)>>,
    /// Keys listed twice inside one bracketed set.
    pub tag_twice: Vec<(String, Span)>,
    /// The equations it removed, in file order.
    pub removed: Vec<RemovedEquation>,
}

/// An equation a surgery statement removed.
#[derive(Clone, Debug)]
pub struct RemovedEquation {
    /// The equation as parsed, with its tags and spans.
    pub equation: Equation,
    /// 1-based position in the equation list before the statement ran.
    pub number: usize,
    /// The endogenous the equation names: its `endogenous` tag value, or the
    /// single endogenous symbol on its left side. `None` when it names none.
    pub endogenous: Option<String>,
}

#[derive(Clone, Debug)]
pub struct OccbinExpr {
    pub text: String,
    pub span: Span,
    pub expr: Option<ExprId>,
}

#[derive(Clone, Debug)]
pub struct OccbinConstraint {
    pub name: String,
    pub name_span: Span,
    pub bind: Option<OccbinExpr>,
    pub relax: Option<OccbinExpr>,
    pub error_bind: Option<OccbinExpr>,
    pub error_relax: Option<OccbinExpr>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Complementarity {
    pub text: String,
    pub span: Span,
    pub matched: Option<ComplementarityTriple>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComplementarityTriple {
    pub variable: String,
    pub lower_bound: Option<String>,
    pub upper_bound: Option<String>,
}

/// A name listed in `varobs`, with the identifier's span.
#[derive(Clone, Copy, Debug)]
pub struct ObservedVar {
    pub name: Name,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EstimatedParamKind {
    Param,
    Stderr,
    Corr,
    Skew,
}

/// File-level optimal-policy command (`ramsey_model` / `ramsey_policy` /
/// `discretionary_policy` / `osr`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyCommand {
    RamseyModel,
    RamseyPolicy,
    DiscretionaryPolicy,
    Osr,
}

/// One `ramsey_model` / `ramsey_policy` / … statement, file order.
#[derive(Clone, Copy, Debug)]
pub struct PolicyCommandStatement {
    pub command: PolicyCommand,
    /// Command identifier through the statement `;`.
    pub span: Span,
    /// `planner_discount=` on this statement (`ramsey_model` / `ramsey_policy` only).
    pub planner_discount: Option<Span>,
}

/// Target type of a `change_type(…)` statement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeTypeKind {
    Parameters,
    Var,
    Varexo,
    VarexoDet,
}

/// One `change_type(type) name_list;` statement.
#[derive(Clone, Debug)]
pub struct ChangeTypeStmt {
    pub new_type: ChangeTypeKind,
    /// Listed names with their identifier spans, source order.
    pub names: Vec<(Name, Span)>,
    /// Statement keyword through `;`.
    pub span: Span,
}

/// One `trend_var` / `log_trend_var` name.
#[derive(Clone, Debug)]
pub struct TrendVar {
    pub name: Name,
    pub span: Span,
    pub log_trend: bool,
    pub growth: Option<ExprId>,
}

/// One `var(deflator=…)` / `var(log_deflator=…)` / `var(log, deflator=…)` statement.
#[derive(Clone, Debug)]
pub struct NonstationaryVar {
    pub name: Name,
    pub span: Span,
    pub log_deflator: bool,
    pub log_option: bool,
    pub deflator: Option<ExprId>,
}

/// One `optim_weights` row: `symbol expr;` or `symbol, symbol expr;`.
#[derive(Clone, Debug)]
pub struct OptimWeight {
    pub first: Name,
    pub first_span: Span,
    pub second: Option<Name>,
    pub second_span: Option<Span>,
    pub expr: Option<ExprId>,
    /// Whole row through `;`.
    pub span: Span,
}

/// One `ramsey_constraints` entry (one expression through `;`).
#[derive(Clone, Debug)]
pub struct RamseyConstraint {
    pub expr: Option<ExprId>,
    pub span: Span,
}

/// `first_deriv_provided` / `second_deriv_provided` value on `external_function`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DerivSpec {
    /// Bare option: the derivative is provided by the top-level function.
    Bare(Span),
    /// `=<name>`: another external function provides it.
    Named(Name, Span),
}

/// One `external_function(…)` statement.
#[derive(Clone, Debug)]
pub struct ExternalFunctionStmt {
    /// `name=` value and its span.
    pub name: Option<(Name, Span)>,
    /// `nargs=` value.
    pub nargs: Option<i32>,
    pub first_deriv: Option<DerivSpec>,
    pub second_deriv: Option<DerivSpec>,
    /// Statement keyword through `;`.
    pub span: Span,
}

/// One `symbol symbol;` row of an `init2shocks` block.
#[derive(Clone, Debug)]
pub struct Init2ShocksRow {
    pub endo: Name,
    pub endo_span: Span,
    pub exo: Name,
    pub exo_span: Span,
    pub span: Span,
}

/// One `init2shocks(name=group);` … `end;` block.
#[derive(Clone, Debug)]
pub struct Init2ShocksBlock {
    pub group: String,
    pub rows: Vec<Init2ShocksRow>,
}

/// One `name, expr, expr;` row of a `homotopy_setup` block.
#[derive(Clone, Debug)]
pub struct HomotopyRow {
    pub name: Name,
    pub span: Span,
}

/// One `'group' = name_list;` row of a `shock_groups` block.
#[derive(Clone, Debug)]
pub struct ShockGroup {
    pub members: Vec<(Name, Span)>,
}

/// Deprecated command / model option recorded from an option-list identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeprecatedOption {
    AimSolver,
    Bytecode,
}

impl PolicyCommand {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RamseyModel => "ramsey_model",
            Self::RamseyPolicy => "ramsey_policy",
            Self::DiscretionaryPolicy => "discretionary_policy",
            Self::Osr => "osr",
        }
    }

    pub fn is_planner(self) -> bool {
        matches!(
            self,
            Self::RamseyModel | Self::RamseyPolicy | Self::DiscretionaryPolicy
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EstimatedParam {
    pub name: Name,
    pub kind: EstimatedParamKind,
    pub corr_with: Option<Name>,
    pub init: Option<f64>,
    pub lower: Option<f64>,
    pub upper: Option<f64>,
    pub init_expr: Option<ExprId>,
    pub lower_expr: Option<ExprId>,
    pub upper_expr: Option<ExprId>,
    pub mean_expr: Option<ExprId>,
    pub std_expr: Option<ExprId>,
    pub prior_beta: bool,
    pub span: Span,
}

/// One `var` / `corr` / `stderr` / `skew` statement inside a `shocks` block.
#[derive(Clone, Debug)]
pub struct ShockStmt {
    pub kind: ShockKind,
    /// Folded RHS (`var name = expr` / `corr a, b = expr`). `None` if missing or unevaluable.
    pub rhs: Option<f64>,
    /// Parsed RHS expression (`var`/`corr`/`stderr`/`skew`), including unevaluable names.
    pub rhs_expr: Option<ExprId>,
    /// From `var`/`corr`/`skew` through the statement `;` (includes a following `stderr`).
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ShockKind {
    /// `var name = expr` (variance).
    Var(Name),
    /// `var name; stderr expr`.
    Stderr(Name),
    /// `var n1, n2, … = covariance` (two or more names, source order).
    Cov(Vec<Name>),
    /// `corr a, b = expr`.
    Corr { a: Name, b: Name },
    /// `skew a = expr` or `skew a, b, c = expr`.
    Skew(Vec<Name>),
}

#[derive(Clone, Debug)]
pub struct Assignment {
    pub name: Name,
    pub expression: String,
    pub span: Span,
    /// P-expr tree of the RHS. `None` when the statement was recovered
    /// from a joined string (endval) rather than `parse_expr`.
    pub expr: Option<ExprId>,
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    pub source: String,
    pub intern: Interner,
    pub endogenous: Vec<Decl>,
    pub exogenous: Vec<Decl>,
    pub deterministic_exogenous: Vec<Decl>,
    pub parameters: Vec<Decl>,
    pub predetermined: Vec<Decl>,
    pub param_assignments: Vec<Assignment>,
    pub helper_assignments: Vec<Assignment>,
    pub equations: Vec<Equation>,
    /// `model_remove` / `model_replace` statements, file order, with what each removed.
    pub equation_surgery: Vec<EquationSurgery>,
    pub steady_state_equations: Vec<Equation>,
    pub initval: Vec<Assignment>,
    pub endval: Vec<Assignment>,
    pub is_linear: bool,
    pub model_block: Option<Span>,
    pub ss_block: Option<Span>,
    pub initval_block: Option<Span>,
    pub endval_block: Option<Span>,
    pub shocks_block: Option<Span>,
    /// Unique interned names from `var` / `corr` lists in `shocks` / `mshocks`.
    pub shocks_vars: Vec<Name>,
    /// `var` / `corr` statements from `shocks` blocks only (not `mshocks`).
    pub shock_stmts: Vec<ShockStmt>,
    /// `varobs` names in declaration order, including repeats.
    pub varobs: Vec<ObservedVar>,
    /// First `varobs …;` statement.
    pub varobs_span: Option<Span>,
    pub estimated_params: Vec<EstimatedParam>,
    pub estimated_params_span: Option<Span>,
    /// First occurrence of each `observation_trends` leading name.
    pub observation_trends: Vec<(Name, Span)>,
    pub observation_trends_span: Option<Span>,
    /// `ramsey_model` / `ramsey_policy` / `discretionary_policy` / `osr` in file order.
    pub policy_commands: Vec<PolicyCommand>,
    /// Identifier span of the first policy command (not the `(options)`).
    pub policy_command_span: Option<Span>,
    /// Whole `planner_objective …;` statement if present (first).
    pub planner_objective_span: Option<Span>,
    /// Every `planner_objective …;` statement, file order.
    pub planner_objective_spans: Vec<Span>,
    /// Unique, first-seen, from `instruments=(…)` on any policy command.
    pub instruments: Vec<Name>,
    /// First `planner_discount` option that folds; later options do not overwrite.
    pub planner_discount: Option<f64>,
    /// First `planner_discount` expression (first-wins; beside the folded float).
    pub planner_discount_expr: Option<ExprId>,
    /// Names from `osr_params …;`.
    pub osr_params: Vec<Name>,
    /// True iff an `optim_weights;` … `end;` block is present.
    pub has_optim_weights: bool,
    /// Identifier span of each top-level `simul` command (not `stoch_simul`).
    pub simul_spans: Vec<Span>,
    /// Identifier span of the first `ramsey_policy`.
    pub ramsey_policy_span: Option<Span>,
    /// Identifier span of the first `discretionary_policy`.
    pub discretionary_policy_span: Option<Span>,
    /// `aim_solver` / `bytecode` identifier spans in `model(…)` and command `(…)` option lists.
    pub deprecated_option_spans: Vec<(DeprecatedOption, Span)>,
    pub exprs: ExprArena,
    /// Recovery notes from the parser (byte spans). Messages are formatted in
    /// `check_parse` via `LineIndex`.
    pub parse_issues: Vec<ParseIssue>,
    /// Literal `@#include` directives (quoted or bare path). Identifier-only
    /// arguments (`@#include FOO`) are not recorded.
    pub includes: Vec<IncludeDirective>,
    /// `@#includepath` directives (raw argument; workspace splits/resolves).
    pub includepaths: Vec<IncludePathDirective>,
    /// Pre-expand `@#` directives other than `@#include` (file-text order).
    pub macro_directives: Vec<MacroDirective>,
    /// Pre-expand `@{…}` interpolations (file-text order).
    pub macro_interps: Vec<MacroInterp>,
    /// `occbin_constraints` regimes in source order (every block concatenated).
    pub occbin_constraints: Vec<OccbinConstraint>,
    /// One span per `occbin_constraints;` … `end;`.
    pub occbin_constraints_blocks: Vec<Span>,
    /// Sticky: true if any `shocks(…surprise…)` opener was seen.
    pub shocks_surprise: bool,
    /// First `surprise` option identifier on `shocks(…)`.
    pub shocks_surprise_span: Option<Span>,
    /// First `shock_paths` opener.
    pub shock_paths_span: Option<Span>,
    /// First `perfect_foresight_controlled_paths` opener.
    pub perfect_foresight_controlled_paths_span: Option<Span>,
    /// First `identification` command identifier.
    pub identification_span: Option<Span>,
    /// First `perfect_foresight_solver` identifier.
    pub perfect_foresight_solver_span: Option<Span>,
    /// First `perfect_foresight_with_expectation_errors_solver` identifier.
    pub pfee_solver_span: Option<Span>,
    /// First `extended_path` identifier.
    pub extended_path_span: Option<Span>,
    /// First `method_of_moments` identifier.
    pub method_of_moments_span: Option<Span>,
    /// First `sensitivity` command identifier.
    pub sensitivity_span: Option<Span>,
    /// `use_dll` identifier in `model(…)`.
    pub use_dll_span: Option<Span>,
    /// `no_static` identifier in `model(…)`.
    pub no_static_span: Option<Span>,
    /// First `check` command identifier.
    pub check_span: Option<Span>,
    /// First `steady` command identifier.
    pub steady_span: Option<Span>,
    /// First `stoch_simul` command identifier.
    pub stoch_simul_span: Option<Span>,
    /// First `estimation` command identifier.
    pub estimation_span: Option<Span>,
    /// First `calib_smoother` command identifier.
    pub calib_smoother_span: Option<Span>,
    /// First `perfect_foresight_setup` identifier.
    pub perfect_foresight_setup_span: Option<Span>,
    /// First `perfect_foresight_with_expectation_errors_setup` identifier.
    pub pfee_setup_span: Option<Span>,
    /// First `write_latex_steady_state_model` identifier.
    pub write_latex_steady_state_model_span: Option<Span>,
    /// `periods` appears in the first `extended_path(…)` option list.
    pub extended_path_has_periods: bool,
    /// First `ramsey_constraints` opener.
    pub ramsey_constraints_span: Option<Span>,
    /// `initval(all_values_required)`.
    pub initval_all_values_required: bool,
    /// `endval(all_values_required)`.
    pub endval_all_values_required: bool,
    /// First `initval` opener that follows an `endval` block.
    pub initval_after_endval_span: Option<Span>,
    /// `instruments=` was present on a `discretionary_policy` (list may be empty).
    pub discretionary_has_instruments_option: bool,
    /// First `data` command identifier (not `database`).
    pub data_span: Option<Span>,
    /// First `prior_function` command identifier.
    pub prior_function_span: Option<Span>,
    /// First `posterior_function` command identifier.
    pub posterior_function_span: Option<Span>,
    /// `function=` seen on `prior_function` / `posterior_function` (sticky OR).
    pub prior_function_has_function: bool,
    /// `(…)` seen on `prior_function` / `posterior_function` (sticky OR).
    pub prior_function_has_parens: bool,
    /// `use_calibration` on `estimated_params_init`.
    pub estimated_params_init_use_calibration: Option<Span>,
    /// First bare `dsge_var` on `estimation`.
    pub dsge_var_estimated: Option<Span>,
    /// First `dsge_var=` on `estimation`.
    pub dsge_var_calibrated: Option<Span>,
    /// Per `estimation` statement that listed a `dsge_var` form.
    pub estimation_dsge_var_stmts: Vec<EstimationDsgeVarStmt>,
    /// First `dsge_varlag` on `estimation`.
    pub dsge_varlag_span: Option<Span>,
    /// First `bayesian_irf` on `estimation`.
    pub bayesian_irf_span: Option<Span>,
    /// First `datafile=` on `estimation`.
    pub estimation_datafile_span: Option<Span>,
    /// First `dataseries=` on `estimation` (recorded; not an E227 gate).
    pub estimation_dataseries_span: Option<Span>,
    /// First `mode_file=` on `estimation`.
    pub estimation_mode_file_span: Option<Span>,
    /// First `mh_tune_jscale` on `estimation` (bare or `=`).
    pub mh_tune_jscale_span: Option<Span>,
    /// First `mh_jscale=` on `estimation`.
    pub mh_jscale_span: Option<Span>,
    /// First `mh_tune_guess=` on `estimation`.
    pub mh_tune_guess_span: Option<Span>,
    /// First `filter_algorithm=gmf` on `estimation`.
    pub filter_algorithm_gmf_span: Option<Span>,
    /// First `proposal_approximation=montecarlo` on `estimation`.
    pub proposal_approximation_montecarlo_span: Option<Span>,
    /// First `distribution_approximation=montecarlo` on `estimation`.
    pub distribution_approximation_montecarlo_span: Option<Span>,
    /// `sensitivity(identification=1)` option span (does not set `identification_span`).
    pub sensitivity_identification_eq_1: Option<Span>,
    /// `identification(order=N)` number and its span.
    pub identification_order: Option<(i32, Span)>,
    /// `identification(max_dim_cova_group=N)` number and its span.
    pub max_dim_cova_group: Option<(i32, Span)>,
    /// `discretionary_policy(order=N)` number and its span.
    pub discretionary_order: Option<(i32, Span)>,
    /// First `hp_filter` on `stoch_simul`.
    pub stoch_simul_hp_filter: Option<Span>,
    /// First `one_sided_hp_filter` on `stoch_simul`.
    pub stoch_simul_one_sided_hp_filter: Option<Span>,
    /// First `bandpass_filter` on `stoch_simul`.
    pub stoch_simul_bandpass_filter: Option<Span>,
    /// First `restriction_fname` option identifier.
    pub restriction_fname_span: Option<Span>,
    /// Trailing / `osr_params` symbol-list names.
    pub command_symbols: Vec<CommandSymbol>,
    /// `histval;` … `end;` (first block).
    pub histval_block: Option<Span>,
    /// `histval(all_values_required)`.
    pub histval_all_values_required: bool,
    pub histval: Vec<HistvalEntry>,
    /// Parsed `estimated_params_init` entries (not `estimated_params`).
    pub estimated_params_init: Vec<EstimatedParam>,
    pub estimated_params_init_span: Option<Span>,
    /// Parsed `estimated_params_bounds` entries.
    pub estimated_params_bounds: Vec<EstimatedParam>,
    pub estimated_params_bounds_span: Option<Span>,
    pub osr_params_bounds: Vec<OsrBound>,
    /// Opener span of the first `osr_params_bounds` block.
    pub osr_params_bounds_span: Option<Span>,
    /// First `osr_params` statement (whole statement).
    pub osr_params_span: Option<Span>,
    pub osr_params_second_span: Option<Span>,
    pub osr_params_statement_count: u32,
    /// First `planner_objective` expression (first-wins).
    pub planner_objective_expr: Option<ExprId>,
    pub varexobs: Vec<ObservedVar>,
    pub varexobs_span: Option<Span>,
    pub varexobs_statement_count: u32,
    pub varexobs_second_span: Option<Span>,
    pub varobs_statement_count: u32,
    pub varobs_second_span: Option<Span>,
    /// Later `observation_trends` leading names that repeat an earlier one.
    pub observation_trends_dups: Vec<(Name, Span)>,
    pub generate_irfs: Vec<GenerateIrfsElement>,
    pub generate_irfs_span: Option<Span>,
    /// Second occurrence of an option ident in one `(…)` list.
    pub option_twice: Vec<(String, Span)>,
    /// `ident.ident` in a parsed expression (`"self.y"`).
    pub namespace_qualified: Vec<(String, Span)>,
    /// Interned-0 fold errors while building (span, code, 7.1 message).
    pub const_fold_errors: Vec<(Span, &'static str, String)>,
    /// `external_function(name=…)` identifiers (command body otherwise skipped).
    pub external_function_names: Vec<Name>,
    /// Auto-declared names from non-model expressions.
    pub mod_file_locals: Vec<Name>,
    /// Macro type errors from expansion (`@#if` not bool, `@#for` tuple, `+` mismatch).
    pub macro_type_errors: Vec<(Span, &'static str, String)>,
    /// `epilogue;` … `end;` (first block).
    pub epilogue_block: Option<Span>,
    /// `epilogue` assignments `name = expr;`, source order.
    pub epilogue: Vec<Assignment>,
    /// First `with_epilogue` option on `shock_decomposition` /
    /// `realtime_shock_decomposition` / `initial_condition_decomposition`.
    pub with_epilogue_span: Option<Span>,
    /// Every `ramsey_model` / `ramsey_policy` / `discretionary_policy` / `osr`
    /// statement with its span, file order.
    pub policy_command_statements: Vec<PolicyCommandStatement>,
    /// Every `change_type(…)` statement, file order.
    pub change_type_statements: Vec<ChangeTypeStmt>,
    /// `dsge_prior_weight` inside a `parameters` declaration.
    pub dsge_prior_weight_param: Option<Span>,
    /// `load_params_and_steady_state(…)` filename (quotes stripped) and statement span.
    pub load_params_file: Option<(String, Span)>,
    /// Every `trend_var` / `log_trend_var` entry, file order.
    pub trend_vars: Vec<TrendVar>,
    /// Every `var(deflator=…)` / `var(log_deflator=…)` name, file order.
    pub nonstationary_vars: Vec<NonstationaryVar>,
    /// `filter_initial_state;` … `end;` (first block).
    pub filter_initial_state_block: Option<Span>,
    /// Parsed `filter_initial_state` entries (own vec; not `histval`).
    pub filter_initial_state: Vec<HistvalEntry>,
    /// Parsed `optim_weights` rows (every block concatenated).
    pub optim_weights: Vec<OptimWeight>,
    /// Parsed `ramsey_constraints` expressions (every block concatenated).
    pub ramsey_constraints: Vec<RamseyConstraint>,
    /// Every `external_function(…)` statement, file order.
    pub external_functions: Vec<ExternalFunctionStmt>,
    /// Every `init2shocks` block, file order.
    pub init2shocks_blocks: Vec<Init2ShocksBlock>,
    /// Every `homotopy_setup` row, file order.
    pub homotopy_rows: Vec<HomotopyRow>,
    /// Members of every `shock_groups` block, file order.
    pub shock_groups: Vec<ShockGroup>,
    /// True iff a Sims `bvar_density` / `bvar_forecast` / `bvar_irf` statement is present.
    pub bvar_present: bool,
}

/// `histval` assignment `name(lag) = expr`.
#[derive(Clone, Debug)]
pub struct HistvalEntry {
    pub name: Name,
    pub lag: i32,
    pub span: Span,
    pub expr: Option<ExprId>,
}

/// One `NAME, lower, upper;` in `osr_params_bounds`.
#[derive(Clone, Debug)]
pub struct OsrBound {
    pub name: Name,
    pub span: Span,
    pub lower: Option<ExprId>,
    pub upper: Option<ExprId>,
}

/// One `NAME, exo = number, …;` in `generate_irfs`.
#[derive(Clone, Debug)]
pub struct GenerateIrfsElement {
    pub name: Name,
    pub span: Span,
    pub exos: Vec<(Name, Span)>,
}

/// One name in a trailing command list or `osr_params`.
#[derive(Clone, Debug)]
pub struct CommandSymbol {
    pub command: String,
    pub name: Name,
    pub span: Span,
    /// Statement this name was listed on. Duplicate detection is per statement:
    /// Dynare calls `removeDuplicates` on one statement's list, so the same name
    /// on two `stoch_simul` statements is not a duplicate.
    pub list_id: u32,
}

/// `dsge_var` forms on one `estimation` statement.
#[derive(Clone, Copy, Debug, Default)]
pub struct EstimationDsgeVarStmt {
    pub estimated: Option<Span>,
    pub calibrated: Option<Span>,
}

/// A literal `@#include` filename plus the directive's byte span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncludeDirective {
    pub filename: String,
    pub span: Span,
}

/// An `@#includepath` argument plus the directive's byte span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncludePathDirective {
    pub argument: String,
    pub span: Span,
}

/// A pre-expand `@#` directive other than `@#include`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacroDirective {
    /// Directive name, lowercased (`if`, `for`, `error`, …).
    pub kind: String,
    /// Rest of the directive after the name (quotes kept). `None` if empty.
    pub argument: Option<String>,
    pub span: Span,
}

/// A pre-expand `@{…}` interpolation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacroInterp {
    /// Text between `@{` and `}`.
    pub inner: String,
    pub span: Span,
}

/// Parser recovery recorded on `Model`. Not a formatted diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseIssue {
    pub kind: ParseIssueKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseIssueKind {
    MissingEnd {
        keyword: String,
        last_stmt_semi: Option<u32>,
        next_block_label: Option<String>,
        insert_offset: u32,
    },
    MissingDeclSemi {
        keyword: String,
        next_is_assign: bool,
        /// `None` when the declaration runs to EOF with no following `;`.
        next_span: Option<Span>,
    },
    MissingAssignSemi {
        name: String,
    },
    MissingFinalSemi {
        keyword: String,
        body_code_end: u32,
    },
    /// `model_remove;` / `model_remove();` / `model_remove([]);`
    MissingSurgeryTag {
        keyword: String,
    },
    /// `model_remove("tag");` — 7.1's lexer refuses the double quote.
    SurgeryTagDoubleQuoted {
        keyword: String,
    },
    /// `model_remove([name=e1]);` — 7.1 wants the value in single quotes.
    SurgeryTagUnquoted {
        keyword: String,
    },
    /// `model_replace('tag'); end;` — the grammar wants an equation list.
    EmptyReplaceBody,
    KeywordTypo {
        found: String,
        correct: String,
    },
    MissingShocksSemi {
        family: ShocksSemiFamily,
        /// Next keyword, `var` name, or `stderr`/`corr` depending on `family`.
        label: String,
        fix_start: u32,
        fix_end: u32,
    },
}

/// Message/fix family for a missing `;` inside `shocks`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShocksSemiFamily {
    BeforeKeyword,
    AfterVar,
    EndOfStmt,
}

impl Model {
    pub fn name(&self, name: Name) -> &str {
        self.intern.get(name)
    }

    /// `ModFileStructure::isStochasticContext`, plus `ramsey_policy` (sets `stoch_simul_present`).
    pub fn is_stochastic_context(&self) -> bool {
        self.stoch_simul_span.is_some()
            || self.estimation_span.is_some()
            || self.policy_commands.contains(&PolicyCommand::Osr)
            || self
                .policy_commands
                .contains(&PolicyCommand::DiscretionaryPolicy)
            || self.calib_smoother_span.is_some()
            || self.identification_span.is_some()
            || self.method_of_moments_span.is_some()
            || self.sensitivity_span.is_some()
            || self.extended_path_span.is_some()
            || self.ramsey_policy_span.is_some()
    }

    /// PF/PFEE **solver** context (`simul` counts). Setup alone does not.
    pub fn is_pf_solver_context(&self) -> bool {
        self.perfect_foresight_solver_span.is_some()
            || self.pfee_solver_span.is_some()
            || !self.simul_spans.is_empty()
    }

    /// Non-`#` model equations, including `[static]`.
    pub fn non_local_equation_count(&self) -> usize {
        self.equations.iter().filter(|eq| !eq.is_local).count()
    }

    /// Identifier refs in `eq`, walking `lhs_expr` then `rhs_expr`.
    pub fn ident_refs(&self, eq: &Equation) -> Vec<IdentRef> {
        let mut out = Vec::new();
        if let Some(id) = eq.lhs_expr {
            out.extend(self.exprs.walk_idents(id));
        }
        if let Some(id) = eq.rhs_expr {
            out.extend(self.exprs.walk_idents(id));
        }
        out
    }

    pub fn summary(&self) -> ParseSummary {
        ParseSummary {
            endogenous: self
                .endogenous
                .iter()
                .map(|d| self.name(d.name).to_string())
                .collect(),
            exogenous: self
                .exogenous
                .iter()
                .map(|d| self.name(d.name).to_string())
                .collect(),
            parameters: self
                .parameters
                .iter()
                .map(|d| self.name(d.name).to_string())
                .collect(),
            n_model_equations: self.equations.len(),
            n_steady_state_equations: self.steady_state_equations.len(),
            n_initval_entries: self.initval.len(),
            is_linear: self.is_linear,
            has_model_block: self.model_block.is_some(),
            has_steady_state_model_block: self.ss_block.is_some(),
            has_initval_block: self.initval_block.is_some(),
            has_shocks_block: self.shocks_block.is_some(),
        }
    }
}

/// Library `ParseSummary` snapshot (names, counts, and block flags).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseSummary {
    pub endogenous: Vec<String>,
    pub exogenous: Vec<String>,
    pub parameters: Vec<String>,
    pub n_model_equations: usize,
    pub n_steady_state_equations: usize,
    pub n_initval_entries: usize,
    pub is_linear: bool,
    pub has_model_block: bool,
    pub has_steady_state_model_block: bool,
    pub has_initval_block: bool,
    pub has_shocks_block: bool,
}
