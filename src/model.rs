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

/// One name a `model_remove` took out of the model, with that statement's position and
/// which way it left.
#[derive(Clone, Copy, Debug)]
pub struct SurgeryExit {
    pub name: Name,
    /// Span of the removal statement.
    pub statement: Span,
    pub kind: SurgeryKind,
}

/// Which way a `model_remove` moved an endogenous out of the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurgeryKind {
    /// Still used somewhere: 7.1's exogenous.
    Exogenous,
    /// Nowhere else: 7.1's `excludedVariable`.
    Dropped,
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
    /// The row's label, quotes stripped (`'g1'` and `g1` are the same label).
    pub label: String,
    /// The label token's span (inside the quotes for a quoted label).
    pub label_span: Span,
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

/// A written date expression, including a possible `+ INTEGER` offset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DateExpr {
    pub text: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeriodPoint {
    Integer(i32),
    Date(DateExpr),
    End,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeriodRange {
    pub first: PeriodPoint,
    /// `None` for one period; `End` is legal only in exogenous `shock_paths`.
    pub last: Option<PeriodPoint>,
    pub span: Span,
}

/// The written value of a shock or path instruction. Expressions are never
/// evaluated by the parser; the optional tree supports later name checks.
#[derive(Clone, Debug)]
pub struct WrittenValue {
    pub text: String,
    pub span: Span,
    pub expr: Option<ExprId>,
    /// Names and qualified references in a `shock_paths` value. Empty on
    /// ordinary shock values. The raw text above remains authoritative.
    pub path_refs: Vec<PathReference>,
}

#[derive(Clone, Debug)]
pub struct PathReference {
    pub namespace: Option<String>,
    pub name: Name,
    pub span: Span,
    pub lag: Option<String>,
    pub learnt_in: Option<PeriodPoint>,
    pub call: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShockOperation {
    Values,
    Add,
    Multiply,
    Scales,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShockBlockKind {
    Regular,
    Surprise,
    LearntIn,
    Multiplicative,
    Heteroskedastic,
    /// Parsed and kept apart from ordinary stochastic checks; diagnostics for
    /// this 7.2 form belong to 0.9.
    Heterogeneous,
}

#[derive(Clone, Debug, Default)]
pub struct ShockOptions {
    pub overwrite: bool,
    pub learnt_in: Option<PeriodPoint>,
    pub learnt_in_span: Option<Span>,
    pub relative_to_initval: bool,
    pub heterogeneity: Option<(Name, Span)>,
    /// Option names and spans in written order, including repeated options.
    pub written: Vec<(String, Span)>,
}

#[derive(Clone, Debug)]
pub struct ScheduledShock {
    pub name: Name,
    pub name_span: Span,
    pub periods: Vec<PeriodRange>,
    pub values: Vec<WrittenValue>,
    pub operation: ShockOperation,
    pub span: Span,
}

/// One written shocks-family block, including its otherwise empty body.
#[derive(Clone, Debug)]
pub struct ShockBlock {
    pub kind: ShockBlockKind,
    pub options: ShockOptions,
    pub stochastic: Vec<ShockStmt>,
    pub scheduled: Vec<ScheduledShock>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum PathTarget {
    Exogenous {
        name: Name,
        span: Span,
    },
    Controlled {
        exogenize: Name,
        exogenize_span: Span,
        endogenize: Name,
        endogenize_span: Span,
    },
}

#[derive(Clone, Debug)]
pub struct PathStanza {
    pub target: PathTarget,
    pub periods: Vec<PeriodRange>,
    pub values: Vec<WrittenValue>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct PathBlock {
    pub options: ShockOptions,
    pub stanzas: Vec<PathStanza>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct EndvalEntry {
    pub name: Name,
    pub name_span: Span,
    pub value: WrittenValue,
    pub operation: ShockOperation,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct EndvalInstruction {
    pub learnt_in: Option<PeriodPoint>,
    pub learnt_in_span: Option<Span>,
    pub entries: Vec<EndvalEntry>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct DatabaseDeclaration {
    pub names: Vec<(Name, Span)>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SetTimeStatement {
    pub value: DateExpr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubsampleHead {
    Symbol(Name, Span),
    Std(Name, Span),
    Corr(Name, Span, Name, Span),
}

#[derive(Clone, Debug)]
pub struct SubsampleRange {
    pub name: Name,
    pub name_span: Span,
    pub first: DateExpr,
    pub last: DateExpr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum SubsampleInstruction {
    Declare {
        head: SubsampleHead,
        ranges: Vec<SubsampleRange>,
        span: Span,
    },
    Copy {
        target: SubsampleHead,
        source: SubsampleHead,
        span: Span,
    },
}

#[derive(Clone, Debug)]
pub struct DateOption {
    pub command: String,
    pub name: String,
    pub value: DateExpr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct StochSimulRequest {
    pub span: Span,
    /// Explicit `irf=INTEGER`, including zero. `None` means no explicit value.
    pub irf: Option<(i32, Span)>,
    /// An explicit `irf_shocks=(...)` list; empty is different from absent.
    pub irf_shocks: Option<Vec<(Name, Span)>>,
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
    /// Endogenous a `model_remove` dropped from the model (7.1's `excludedVariable`
    /// type). The symbol is no longer in `endogenous`, but it was declared before the
    /// removal: `filter_initial_state` refuses it with the timing message, not with
    /// the undeclared one.
    pub excluded_endogenous: Vec<Decl>,
    /// Every name a `model_remove` took out of the model, file order. A check that reads
    /// a name's type reads it as of the statement it is looking at: 7.1 validated that
    /// statement while the name was still endogenous.
    pub surgery_exits: Vec<SurgeryExit>,
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
    /// Flat-vector index where each parsed `shocks` block begins, including empty blocks.
    pub shock_stmt_block_starts: Vec<usize>,
    /// Written 7.2 shock blocks in file order. `shock_stmts` above remains the
    /// flat stochastic view used by diagnostics shipped before 0.7.
    pub shock_blocks: Vec<ShockBlock>,
    pub shock_paths: Vec<PathBlock>,
    pub controlled_paths: Vec<PathBlock>,
    pub endval_instructions: Vec<EndvalInstruction>,
    pub databases: Vec<DatabaseDeclaration>,
    pub subsamples: Vec<SubsampleInstruction>,
    pub set_time: Vec<SetTimeStatement>,
    pub date_options: Vec<DateOption>,
    pub stoch_simul_requests: Vec<StochSimulRequest>,
    /// `varobs` names in declaration order, including repeats.
    pub varobs: Vec<ObservedVar>,
    /// First `varobs …;` statement.
    pub varobs_span: Option<Span>,
    pub estimated_params: Vec<EstimatedParam>,
    /// Flat-vector index where each `estimated_params` block begins.
    pub estimated_params_block_starts: Vec<usize>,
    pub estimated_params_span: Option<Span>,
    /// First occurrence of each leading name within each `observation_trends` block.
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
    /// Every parsed MS-SBVAR family statement, file order.
    pub ms_statements: Vec<MsStatement>,
    /// Statement spans the parser recognised as a family shape but deliberately did
    /// not interpret: a dotted head the pin's lexer would send to native MATLAB, and
    /// a family keyword in a shape 7.1's grammar has no production for. 7.1 makes no
    /// language claim on those lines, so neither may we.
    pub ms_unparsed_spans: Vec<Span>,
    /// Every parsed dotted `prior` / `options` / `subsamples` statement, file order.
    pub dotted_statements: Vec<DottedStatement>,
    /// One row per statement whose shape 7.1's grammar has no production for, in
    /// file order: a family command with a malformed option list or block body, a
    /// pin keyword used in a shape the grammar does not take, or a dotted head the
    /// grammar cannot put a body on. 7.1 refuses each with a parse-stage
    /// `syntax error, unexpected …`, so the row carries the token to report and the
    /// command or head to name.
    pub shape_refuses: Vec<ShapeRefuse>,
    /// Every parsed `data` statement, file order (the estimation / MS-SBVAR one).
    pub data_statements: Vec<DataStatement>,
    /// `svar_identification;` … `end;` bodies, file order.
    pub svar_identifications: Vec<SvarIdentification>,
    /// `conditional_forecast_paths;` … `end;` bodies, file order.
    pub conditional_forecast_paths: Vec<ConditionalForecastPaths>,
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
    /// Every `estimation` statement, file order: its identifier span and whether
    /// that statement carried `datafile=`. The **E227** gate reads the order
    /// against `data_statements`, because 7.1's flag is per statement and set in
    /// file order.
    pub estimation_statements: Vec<EstimationStatement>,
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
    /// Flat-vector index where each `histval` block begins.
    pub histval_block_starts: Vec<usize>,
    /// Parsed `estimated_params_init` entries (not `estimated_params`).
    pub estimated_params_init: Vec<EstimatedParam>,
    pub estimated_params_init_block_starts: Vec<usize>,
    pub estimated_params_init_span: Option<Span>,
    /// Parsed `estimated_params_bounds` entries.
    pub estimated_params_bounds: Vec<EstimatedParam>,
    pub estimated_params_bounds_block_starts: Vec<usize>,
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
    /// Later leading names that repeat an earlier one in the same block.
    pub observation_trends_dups: Vec<(Name, Span)>,
    pub generate_irfs: Vec<GenerateIrfsElement>,
    pub generate_irfs_block_starts: Vec<usize>,
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
    pub filter_initial_state_block_starts: Vec<usize>,
    /// Parsed `optim_weights` rows (every block concatenated).
    pub optim_weights: Vec<OptimWeight>,
    pub optim_weights_block_starts: Vec<usize>,
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
    /// Flat-vec index where each `shock_groups` block's rows begin. Their reuse
    /// warning compares labels within one block only (each block is its own
    /// statement; probed on 7.1), so the check needs the boundaries.
    pub shock_group_block_starts: Vec<usize>,
    /// True iff a Sims `bvar_density` / `bvar_forecast` / `bvar_irf` statement is present.
    pub bvar_present: bool,
    /// Every `method_of_moments` statement, file order.
    pub mom_statements: Vec<MomStatement>,
    /// Every `matched_moments` row, every block concatenated in source order.
    pub matched_moments: Vec<MatchedMoment>,
    /// One span per `matched_moments;` … `end;`.
    pub matched_moments_blocks: Vec<Span>,
    /// Every `matched_irfs` block, file order.
    pub matched_irfs: Vec<MatchedIrfsBlock>,
    /// Every `matched_irfs_weights` block, file order.
    pub matched_irfs_weights: Vec<MatchedIrfsWeightsBlock>,
    /// Every `matched_irfs_weights` row, every block concatenated in source order.
    pub matched_irfs_weight_rows: Vec<MatchedIrfsWeight>,
    /// Every `moment_calibration` block, file order.
    pub moment_calibration: Vec<MomentCalibrationBlock>,
    /// Every `irf_calibration` block, file order.
    pub irf_calibration: Vec<IrfCalibrationBlock>,
    /// A token on one of the five moment blocks that 7.1 refuses while reading
    /// the file, with the sentence it prints. Syntax errors, and the two
    /// sentences no code owns.
    pub mom_syntax: Vec<MomSyntax>,
}

/// One parse-stage sentence on a moment or calibration block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MomSyntax {
    pub span: Span,
    /// The sentence 7.1 prints for this token.
    pub message: String,
}

/// `DATE` as 7.1's lexer reads it: an optional sign, digits, and one unit suffix
/// (`y`, `a`, `m1`–`m12`, `q1`–`q4`, `s1`/`s2`, `h1`/`h2`), case-insensitive.
pub(crate) fn dynare_date(text: &str) -> bool {
    let body = text.strip_prefix('-').unwrap_or(text);
    let digits = body.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits == body.len() {
        return false;
    }
    let suffix = body.split_at(digits).1.to_ascii_lowercase();
    if suffix == "y" || suffix == "a" {
        return true;
    }
    let (unit, number) = suffix.split_at(1);
    let Ok(number) = number.parse::<u32>() else {
        return false;
    };
    match unit {
        "m" => (1..=12).contains(&number),
        "q" => (1..=4).contains(&number),
        "s" | "h" => (1..=2).contains(&number),
        _ => false,
    }
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

/// One parsed MS-SBVAR family statement (`ms_*`, `sbvar`, `svar`, `markov_switching`,
/// `conditional_forecast`, `svar_global_identification_check`).
#[derive(Clone, Debug)]
pub struct MsStatement {
    /// Command name as written.
    pub command: String,
    /// Whole statement: opener through the terminating `;`.
    pub span: Span,
    /// Parsed option rows; a bare command with no `(…)` has none.
    pub options: Vec<FamilyOption>,
}

/// One parsed `data(file=…)` statement (the estimation / MS-SBVAR data statement).
#[derive(Clone, Debug)]
pub struct DataStatement {
    /// Opener through the terminating `;`.
    pub span: Span,
    pub options: Vec<FamilyOption>,
}

impl DataStatement {
    /// The `file` or `series` option the estimation gate looks for.
    pub fn has_file_or_series(&self) -> bool {
        self.options.iter().any(|opt| {
            opt.has_value
                && (opt.name.eq_ignore_ascii_case("file")
                    || opt.name.eq_ignore_ascii_case("series"))
        })
    }
}

/// One statement the pin's grammar has no production for, recorded where the
/// parser meets it. The pinned preprocessor refuses these at parse with
/// `syntax error, unexpected …`. E001 uses that text when `official_message`
/// is set, and otherwise names `subject` with our generic shape wording.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShapeRefuse {
    /// The token the pinned preprocessor stops on.
    pub span: Span,
    /// The command, keyword or head the message names, as written.
    pub subject: String,
    /// What the grammar takes there for a generic hint. Empty when the whole
    /// statement has no form or `official_message` supplies the exact text.
    pub expected: &'static str,
    /// Exact official syntax text for a known token-level refusal, when available.
    pub official_message: Option<&'static str>,
}

impl ShapeRefuse {
    pub fn new(span: Span, subject: impl Into<String>, expected: &'static str) -> Self {
        Self {
            span,
            subject: subject.into(),
            expected,
            official_message: None,
        }
    }

    pub fn official(span: Span, subject: impl Into<String>, message: &'static str) -> Self {
        Self {
            span,
            subject: subject.into(),
            expected: "",
            official_message: Some(message),
        }
    }
}

/// Our wording for a shape with generic bison text: short, one
/// problem, naming the command or the option. The hint names what the grammar
/// takes there. Shared by the MS-SBVAR family (**E001** from a recorded
/// `ShapeRefuse`) and the moment family (whose handed-over shapes are not).
pub fn shape_refuse_wording(subject: &str, expected: &str) -> String {
    format!("Unexpected token in '{subject}'. The grammar takes {expected} here.")
}

/// **E280**: an `external_function` name used as a bare variable inside a model
/// expression. One wording, so the several surfaces that can meet it agree.
pub fn external_function_in_model_message(name: &str) -> String {
    format!(
        "Symbol {name} is a function name external to Dynare. It cannot be used like a variable without input argument inside model."
    )
}

/// **E281**: a name first written outside `model` (or in another statement's
/// expression) used inside a model expression.
pub fn mod_file_local_in_model_message(name: &str) -> String {
    format!(
        "Variable {name} not allowed inside model declaration. Its scope is only outside model."
    )
}

/// The dotted `….prior(…)` / `….options(…)` / `….subsamples(…)` statement family.
#[derive(Clone, Debug)]
pub struct DottedStatement {
    pub kind: DottedKind,
    /// The head the statement is keyed on.
    pub head: DottedHead,
    /// Head through the terminating `;`.
    pub span: Span,
    /// Parsed option rows. Empty for `options` / `subsamples`, whose bodies are
    /// recognised but not read.
    pub options: Vec<FamilyOption>,
}

/// Which dotted statement this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DottedKind {
    Prior,
    Options,
    Subsamples,
}

/// The head of a dotted statement. 7.1 keys the statement on a declared symbol,
/// so each name here was declared before the statement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DottedHead {
    /// `alpha.prior(…)`, or `alpha.beta.prior(…)`.
    Param { first: Name, second: Option<Name> },
    /// `std(e).prior(…)`, or `std(e).beta.prior(…)`.
    Std {
        first: Name,
        first_span: Span,
        second: Option<Name>,
    },
    /// `corr(y, c).prior(…)`, or `corr(y, c).beta.prior(…)`.
    Corr {
        first: Name,
        first_span: Span,
        second: Name,
        second_span: Span,
        third: Option<Name>,
    },
    /// `[alpha, beta].prior(…)`.
    Vec { names: Vec<(Name, Span)> },
}

/// One option row of a parsed family statement.
#[derive(Clone, Debug)]
pub struct FamilyOption {
    /// Option name as written.
    pub name: String,
    /// Option-name span.
    pub span: Span,
    /// `true` when the option was written `name=value`.
    pub has_value: bool,
    pub value_kind: FamilyValueKind,
    /// Value span; the option-name span when there is no value.
    pub value_span: Span,
    /// Value text with whitespace collapsed.
    pub value_text: String,
    /// Names for a name list; the declaration order as written.
    pub names: Vec<(Name, Span)>,
}

/// The shape of one option's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FamilyValueKind {
    /// A bare flag: `coefficients`, `filtered_probabilities`.
    Flag,
    /// One number, bare name, or quoted string.
    Scalar,
    /// `[a, b, c]` of names.
    NameList,
    /// `[1 2 3]`, `[1, 2]`.
    Vector,
    /// `[[1, 2, 0.5], [1, 2, 0.3]]`.
    Matrix,
    /// A date such as `1959Q1` or `1959M4`.
    Date,
    /// A cell range such as `A1:B10`; 7.1 reads its two halves as one value.
    Range,
}

/// One parsed `svar_identification;` … `end;` block.
#[derive(Clone, Debug)]
pub struct SvarIdentification {
    /// Opener through `end;`.
    pub span: Span,
    pub elements: Vec<SvarIdentificationElement>,
    /// Rows the grammar has no production for, in file order: an `equation` row
    /// written before any `exclusion lag`, and a lag whose `equation` list never
    /// came. A body with none of these is one 7.1 parses.
    pub shape_refuses: Vec<ShapeRefuse>,
}

/// One element of an `svar_identification` body.
#[derive(Clone, Debug)]
pub enum SvarIdentificationElement {
    /// `exclusion lag N;` and the `equation` rows that follow it.
    ExclusionLag {
        lag: Option<i32>,
        span: Span,
        equations: Vec<SvarEquation>,
    },
    /// `exclusion constants;`
    ExclusionConstants {
        span: Span,
    },
    UpperCholesky {
        span: Span,
    },
    LowerCholesky {
        span: Span,
    },
    /// `restriction equation N, EXPR = EXPR;`
    Restriction {
        number: Option<i32>,
        span: Span,
        /// The restriction expression's span.
        expr_span: Span,
    },
}

/// One `equation N, name…;` row under an `exclusion lag` row.
#[derive(Clone, Debug)]
pub struct SvarEquation {
    pub number: Option<i32>,
    pub names: Vec<(Name, Span)>,
    pub span: Span,
}

/// One parsed `conditional_forecast_paths;` … `end;` block.
#[derive(Clone, Debug)]
pub struct ConditionalForecastPaths {
    /// Opener through `end;`.
    pub span: Span,
    pub rows: Vec<ConditionalForecastPath>,
    /// Rows the grammar has no production for, in file order: `exogenize` /
    /// `endogenize` instead of `var`, a row that stops before its `values`, and an
    /// empty `periods` / `values` row.
    pub shape_refuses: Vec<ShapeRefuse>,
}

/// One `var name; periods …; values …;` row.
#[derive(Clone, Debug)]
pub struct ConditionalForecastPath {
    pub name: Name,
    pub name_span: Span,
    /// One entry per `periods` element; `1:4` counts as one entry.
    pub periods: Vec<Span>,
    pub periods_span: Span,
    /// One entry per `values` element.
    pub values: Vec<Span>,
    pub values_span: Span,
    /// The `var` row span.
    pub span: Span,
    /// The row carried a `periods` keyword at all.
    pub has_periods: bool,
    /// The row carried a `values` keyword at all.
    pub has_values: bool,
}

/// One `method_of_moments` statement: its keyword through `;`, and its option rows.
#[derive(Clone, Debug)]
pub struct MomStatement {
    /// Keyword through `;`.
    pub span: Span,
    /// The statement wrote `(…)` at all. `method_of_moments()` is a syntax refuse
    /// while a bare `method_of_moments;` reaches the missing-method sentence, and
    /// both store an empty option list.
    pub has_option_list: bool,
    /// Empty when the statement has no `(…)`.
    pub options: Vec<FamilyOption>,
}

/// One `matched_moments` row: a model expression through `;`.
#[derive(Clone, Debug)]
pub struct MatchedMoment {
    /// `join_lexemes` of the expression.
    pub text: String,
    pub span: Span,
    /// `parse_expr`; `None` when the row was empty.
    pub expr: Option<ExprId>,
}

/// One `matched_irfs` block.
#[derive(Clone, Debug)]
pub struct MatchedIrfsBlock {
    /// Opener through `end;`.
    pub span: Span,
    /// The block wrote `(overwrite)`.
    pub overwrite: bool,
    pub rows: Vec<MatchedIrfsRow>,
}

/// One `matched_irfs_weights` block.
#[derive(Clone, Debug)]
pub struct MatchedIrfsWeightsBlock {
    /// Opener through `end;`.
    pub span: Span,
    /// The block wrote `(overwrite)`.
    pub overwrite: bool,
    pub rows: Vec<MatchedIrfsWeight>,
}

/// One `var ENDO; varexo EXO; periods …; values …; weights …;` row.
#[derive(Clone, Debug)]
pub struct MatchedIrfsRow {
    pub endogenous: Name,
    pub endogenous_span: Span,
    pub exogenous: Name,
    pub exogenous_span: Span,
    /// One entry per `period_list` item; `1:4` is one.
    pub periods: Vec<Span>,
    /// One entry per value; `(xx)` is one.
    pub values: Vec<Span>,
    /// Expressions inside `values` parentheses. A bare number has no expression.
    pub value_exprs: Vec<ExprId>,
    /// Empty when the row has no `weights` keyword.
    pub weights: Vec<Span>,
    /// Expressions inside `weights` parentheses.
    pub weight_exprs: Vec<ExprId>,
    pub span: Span,
}

/// One `name(periods), exo, name(periods), exo, expression;` row.
#[derive(Clone, Debug)]
pub struct MatchedIrfsWeight {
    pub left_endo: Name,
    pub left_endo_span: Span,
    /// `1` or `1:2`, as written.
    pub left_periods: String,
    pub left_periods_span: Span,
    pub left_exo: Name,
    pub left_exo_span: Span,
    pub right_endo: Name,
    pub right_endo_span: Span,
    pub right_periods: String,
    pub right_periods_span: Span,
    pub right_exo: Name,
    pub right_exo_span: Span,
    pub weight_text: String,
    pub weight_span: Span,
    /// The weight expression, when one was read.
    pub weight_expr: Option<ExprId>,
    pub span: Span,
}

/// One `moment_calibration;` … `end;` block.
#[derive(Clone, Debug)]
pub struct MomentCalibrationBlock {
    pub span: Span,
    pub rows: Vec<MomentCalibrationRow>,
}

/// One `name, name(lags), range;` row.
#[derive(Clone, Debug)]
pub struct MomentCalibrationRow {
    pub first: Name,
    pub first_span: Span,
    pub second: Name,
    pub second_span: Span,
    /// `None` when there is no `(…)`; 7.1 defaults this to `0`.
    pub lags: Option<String>,
    pub lags_span: Option<Span>,
    pub range: CalibrationRange,
    pub span: Span,
}

/// One `irf_calibration;` … `end;` block.
#[derive(Clone, Debug)]
pub struct IrfCalibrationBlock {
    pub span: Span,
    /// The block wrote `(relative_irf)`.
    pub relative_irf: bool,
    pub rows: Vec<IrfCalibrationRow>,
}

/// One `name(periods), exo, range;` row.
#[derive(Clone, Debug)]
pub struct IrfCalibrationRow {
    pub endogenous: Name,
    pub endogenous_span: Span,
    /// `None` when there is no `(…)`; 7.1 defaults this to `1`.
    pub periods: Option<String>,
    pub periods_span: Option<Span>,
    pub exogenous: Name,
    pub exogenous_span: Span,
    pub range: CalibrationRange,
    pub span: Span,
}

/// The third column of a `moment_calibration` / `irf_calibration` row.
#[derive(Clone, Debug)]
pub enum CalibrationRange {
    /// `[0.5, 1.2]`, `[0, Inf]`.
    Bracket {
        lower: String,
        upper: String,
        span: Span,
    },
    /// `+`
    Plus { span: Span },
    /// `-`
    Minus { span: Span },
}

/// `dsge_var` forms on one `estimation` statement.
#[derive(Clone, Copy, Debug, Default)]
pub struct EstimationDsgeVarStmt {
    pub estimated: Option<Span>,
    pub calibrated: Option<Span>,
}

/// One `estimation` statement, in file order.
#[derive(Clone, Copy, Debug)]
pub struct EstimationStatement {
    /// The command identifier's span. The **E227** row points here.
    pub span: Span,
    /// This statement carried `datafile=`.
    pub has_datafile: bool,
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
    /// A double-quoted string. 7.1's lexer accepts only single quotes in the grammar,
    /// so this is lexer junk wherever it appears (verbatim blocks pass raw text through).
    DoubleQuotedString,
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
    /// At statement head an `<INITIAL>` keyword followed by `=` is that keyword,
    /// not an assignment. 7.1: `syntax error, unexpected EQUAL, expecting ';' or '('`.
    UnexpectedEqual,
    /// `end = 0;` inside an assignment block. `end` is the closer, so this is
    /// not a name. 7.1: `syntax error, unexpected IDENTIFIER, expecting ';'`.
    UnexpectedEndAssign,
    /// `end` used as a name inside an equation. The block rule returns `END`.
    /// 7.1: `syntax error, unexpected END`.
    UnexpectedEnd,
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

    /// The spans of every statement this parser read on purpose, so the text-level
    /// passes (invalid identifiers, missing semicolons) can leave their contents
    /// alone. Every parsed surface must be listed here, or the passes will read its
    /// option values as declarations or as parameter assignments.
    pub fn statement_spans(&self) -> Vec<Span> {
        let mut spans: Vec<Span> = self
            .shock_blocks
            .iter()
            .map(|block| block.span)
            .chain(self.shock_paths.iter().map(|block| block.span))
            .chain(self.controlled_paths.iter().map(|block| block.span))
            .chain(self.databases.iter().map(|decl| decl.span))
            .chain(self.subsamples.iter().map(|item| match item {
                SubsampleInstruction::Declare { span, .. }
                | SubsampleInstruction::Copy { span, .. } => *span,
            }))
            .chain(self.set_time.iter().map(|stmt| stmt.span))
            .chain(self.ms_statements.iter().map(|stmt| stmt.span))
            .chain(self.data_statements.iter().map(|stmt| stmt.span))
            .chain(self.dotted_statements.iter().map(|stmt| stmt.span))
            .chain(self.ms_unparsed_spans.iter().copied())
            .chain(self.svar_identifications.iter().map(|block| block.span))
            .chain(
                self.conditional_forecast_paths
                    .iter()
                    .map(|block| block.span),
            )
            .chain(self.mom_statements.iter().map(|stmt| stmt.span))
            .chain(self.matched_moments_blocks.iter().copied())
            .chain(self.matched_irfs.iter().map(|block| block.span))
            .chain(self.matched_irfs_weights.iter().map(|block| block.span))
            .chain(self.moment_calibration.iter().map(|block| block.span))
            .chain(self.irf_calibration.iter().map(|block| block.span))
            .collect();
        spans.sort_by_key(|span| (span.start, span.end));
        spans
    }

    /// True when a `model_remove` took `name` out of the model **after** byte `at`: the
    /// statement at `at` was written while the name was still endogenous.
    pub fn surgery_exit_after(&self, name: Name, at: u32) -> bool {
        self.surgery_exits
            .iter()
            .any(|exit| exit.name == name && exit.statement.start > at)
    }

    /// True when a `model_remove` **dropped** `name` (excluded, not exogenous) after `at`.
    pub fn dropped_by_surgery_after(&self, name: Name, at: u32) -> bool {
        self.surgery_exits.iter().any(|exit| {
            exit.name == name && exit.statement.start > at && exit.kind == SurgeryKind::Dropped
        })
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
