//! Parsed `.mod` model. This is the seam diagnostic families and transports share.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::expr::{ExprArena, ExprId, IdentRef};
use crate::intern::{Interner, Name};
use crate::span::Span;

#[derive(Clone, Debug)]
pub struct Decl {
    /// Position in the expanded token stream; original source spans may repeat.
    pub parse_order: usize,
    /// Length of type history when this name slot was parsed, before later
    /// macro iterations or directives could change its type.
    pub symbol_type_context: SymbolContext,
    pub name: Name,
    pub span: Span,
    pub long_name: Option<String>,
    /// Text inside a written `$…$` after this name. Absent when none was written.
    pub tex_name: Option<String>,
    pub log_transform: bool,
    /// `heterogeneity=<symbol>` on the declaration: the dimension name and the
    /// value identifier's span.
    pub heterogeneity: Option<(Name, Span)>,
}

/// Presentation records of existing parser surfaces. Order is execution order,
/// never a sort by written spans (which macro copies can share).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatementKind {
    Declaration,
    Dimension,
    Assignment,
    Command,
    Block,
}

impl StatementKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Declaration => "declaration",
            Self::Dimension => "dimension",
            Self::Assignment => "assignment",
            Self::Command => "command",
            Self::Block => "block",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignmentIndex {
    Parameter(usize),
    Helper(usize),
}

#[derive(Clone, Debug)]
pub struct Statement {
    pub id: usize,
    pub kind: StatementKind,
    pub name: String,
    pub token_range: Range<usize>,
    pub opener_range: Range<usize>,
    /// First parsed token, retained separately from the full construct span.
    /// Synthesized macro text still needs a written-source mapping proof.
    pub keyword_span: Span,
    pub span: Span,
    pub complete: bool,
    pub category: Option<String>,
    pub subtype: Option<String>,
    pub dimension: Option<String>,
    pub assignment: Option<AssignmentIndex>,
    /// A retained scalar MATLAB helper assignment, not a Dynare command.
    pub native: bool,
}

/// Opaque native text is an execution barrier, not a Dynare command.
#[derive(Clone, Debug)]
pub enum ExecutionStep {
    Statement(usize),
    Opaque(Span),
}

#[derive(Clone, Debug)]
pub struct WrittenDeclaration {
    pub statement_id: usize,
    pub written_kind: String,
    pub declaration: Decl,
    pub token_range: Range<usize>,
}

/// Includes rows later removed/replaced, for file-local written structure.
#[derive(Clone, Debug)]
pub struct WrittenEquation {
    pub statement_id: usize,
    pub equation: Equation,
    pub token_range: Range<usize>,
    pub dimension: Option<Name>,
}

/// An already parsed declaration/assignment target, kept by execution token
/// position so macro copies cannot be recovered by matching written spans.
#[derive(Clone, Debug)]
pub struct WrittenWrite {
    pub name: Name,
    pub token_range: Range<usize>,
}

/// One name of a `heterogeneity_dimension` statement, file order (one record
/// per name, each carrying the whole statement's span).
#[derive(Clone, Debug)]
pub struct HeterogeneityDimension {
    pub parse_order: usize,
    pub name: Name,
    pub name_span: Span,
    /// Statement keyword through `;`.
    pub span: Span,
}

/// One `model(heterogeneity=d); … end;` block. The written equations live
/// here; `Model::equations` stays the aggregate-written list.
#[derive(Clone, Debug)]
pub struct HeterogeneousModelBlock {
    /// The dimension name and its identifier span in the opener.
    pub dimension: Name,
    pub dimension_span: Span,
    /// Opener through `end;`.
    pub span: Span,
    /// The body's written equations, in file order, tags and `#` locals included.
    pub equations: Vec<Equation>,
}

/// Which `heterogeneity_*` command a record is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeterogeneityCommandKind {
    LoadSteadyState,
    ComputeSteadyState,
    Solve,
    Simulate,
}

/// One `heterogeneity_load_steady_state` / `heterogeneity_compute_steady_state` /
/// `heterogeneity_solve` / `heterogeneity_simulate` statement.
#[derive(Clone, Debug)]
pub struct HeterogeneityCommand {
    pub kind: HeterogeneityCommandKind,
    /// Command spelling as written.
    pub command: String,
    /// Command keyword through the terminating `;`.
    pub span: Span,
    /// Written options with their name/value spans.
    pub options: Vec<HeterogeneityOption>,
    /// `heterogeneity_simulate`'s trailing symbol list, source order.
    pub simulate_names: Vec<(Name, Span)>,
}

/// One written option of a `heterogeneity_*` command.
#[derive(Clone, Debug)]
pub struct HeterogeneityOption {
    /// Option name as written, including its case.
    pub name: String,
    pub name_span: Span,
    /// `name=value`: the raw value text and its span. `None` for a bare flag.
    pub value: Option<(String, Span)>,
}

#[derive(Clone, Debug)]
pub struct SteadyStateTarget {
    pub name: Name,
    pub span: Span,
    /// Type table after the RHS and before registering this output name.
    pub symbol_type_context: SymbolContext,
    /// The RHS and delimiter completed before this output's driver action.
    pub(crate) action_attempted: bool,
}

#[derive(Clone, Debug)]
pub struct Equation {
    /// Expanded token position, retained through removal and replacement.
    pub parse_order: usize,
    pub text: String,
    pub name: String,
    pub span: Span,
    pub lhs: String,
    pub rhs: String,
    pub lhs_expr: Option<ExprId>,
    pub rhs_expr: Option<ExprId>,
    /// Scalar or bracketed outputs of a steady-state assignment, in written order.
    /// Other equation domains leave this empty. Multiple outputs have no scalar LHS.
    pub steady_state_targets: Vec<SteadyStateTarget>,
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
    /// Expanded-token indexes of this equation execution, excluding the eaten
    /// semicolon. Empty means the execution was not recorded. A raw written
    /// span is not a substitute: it can cover a discarded macro branch.
    pub active_tokens: Range<usize>,
}

/// The four top-level semi-structural model commands. Their option sets differ
/// in Dynare's grammar, so a command keeps its kind as well as its written rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SemiStructuralKind {
    VarModel,
    TrendComponentModel,
    VarExpectationModel,
    PacModel,
}

#[derive(Clone, Debug)]
pub struct SemiStructuralCommand {
    pub symbol_type_context: SymbolContext,
    pub kind: SemiStructuralKind,
    pub span: Span,
    pub options: Vec<SemiStructuralOption>,
}

#[derive(Clone, Debug)]
pub struct SemiStructuralOption {
    /// Option word as written, including its case.
    pub name: String,
    pub span: Span,
    pub value: SemiStructuralValue,
}

#[derive(Clone, Debug)]
pub enum SemiStructuralValue {
    Flag,
    Symbol {
        name: Name,
        span: Span,
    },
    /// `eqtags` and `targets` are lists of quoted equation-tag strings.
    Tags(Vec<(String, Span)>),
    Expression(WrittenExpression),
    Integer {
        text: String,
        span: Span,
    },
    Horizon {
        first: String,
        last: String,
        span: Span,
    },
    Kind {
        text: String,
        span: Span,
    },
}

#[derive(Clone, Debug)]
pub struct WrittenExpression {
    pub symbol_type_context: SymbolContext,
    pub text: String,
    pub span: Span,
    pub expr: Option<ExprId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedModelOperatorKind {
    VarExpectation,
    PacExpectation,
    PacTargetNonstationary,
}

#[derive(Clone, Debug)]
pub struct NamedModelOperator {
    pub kind: NamedModelOperatorKind,
    pub name: Name,
    pub operator_span: Span,
    pub name_span: Span,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct PacTargetInfoBlock {
    pub name: Name,
    pub name_span: Span,
    pub span: Span,
    pub rows: Vec<PacTargetInfoRow>,
}

#[derive(Clone, Debug)]
pub enum PacTargetInfoRow {
    Target(WrittenExpression),
    AuxnameTargetNonstationary { name: Name, span: Span },
    Component(PacTargetComponent),
}

#[derive(Clone, Debug)]
pub struct PacTargetComponent {
    /// Retained `component` token; the full span remains the analysis range.
    pub keyword_span: Span,
    pub component: WrittenExpression,
    pub rows: Vec<PacTargetComponentRow>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum PacTargetComponentRow {
    Growth(WrittenExpression),
    Auxname { name: Name, span: Span },
    Kind { text: String, span: Span },
}

#[derive(Clone, Debug)]
pub struct DeterministicTrendsBlock {
    pub span: Span,
    pub rows: Vec<DeterministicTrendRow>,
}

#[derive(Clone, Debug)]
pub struct DeterministicTrendRow {
    pub name: Name,
    pub name_span: Span,
    pub expression: WrittenExpression,
    pub span: Span,
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

/// An initval/endval assignment that model_remove pruned after excluding its name.
#[derive(Clone, Copy, Debug)]
pub struct PrunedInitialization {
    pub name: Name,
    pub span: Span,
    pub block: &'static str,
    pub removal_kind: &'static str,
    /// Index of the type change in parser execution order. Source spans can
    /// repeat when a macro loop expands one written line more than once.
    pub removal_event: usize,
}

/// A symbol's type in Dynare's parser history. Written keyword spellings are
/// converted once on entry; compatibility queries keep their canonical spellings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Var,
    Varexo,
    VarexoDet,
    Parameters,
    Epilogue,
    TrendVar,
    LogTrendVar,
    ExternalFunction,
    ModFileLocal,
    DatabaseVariable,
    ModelLocalVariable,
    Excluded,
}

impl SymbolKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Var => "var",
            Self::Varexo => "varexo",
            Self::VarexoDet => "varexo_det",
            Self::Parameters => "parameters",
            Self::Epilogue => "epilogue",
            Self::TrendVar => "trend_var",
            Self::LogTrendVar => "log_trend_var",
            Self::ExternalFunction => "external_function",
            Self::ModFileLocal => "mod_file_local",
            Self::DatabaseVariable => "database_variable",
            Self::ModelLocalVariable => "model_local_variable",
            Self::Excluded => "excluded",
        }
    }

    pub(crate) fn declaration_keyword(keyword: &str) -> Self {
        match keyword {
            "var" => Self::Var,
            "varexo" => Self::Varexo,
            "varexo_det" => Self::VarexoDet,
            "parameters" => Self::Parameters,
            "model_local_variable" => Self::ModelLocalVariable,
            _ => unreachable!("not a declaration keyword: {keyword}"),
        }
    }
}

/// A position in symbol history, captured during parser execution. This is
/// distinct from a written byte offset, which macro copies can share.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SymbolContext(usize);

/// An identifier read by the MODEL_EXPRESSION grammar, with the symbol table
/// at that read. Later declarations and repeated macro spans cannot change it.
#[derive(Clone, Copy, Debug)]
pub struct ModelExpressionUse {
    /// Effective statement execution owning this read; macro spans can repeat.
    pub(crate) statement_id: usize,
    pub name: Name,
    pub span: Span,
    pub full_span: Span,
    pub timing: i32,
    pub context: SymbolContext,
    pub command: &'static str,
}

impl SymbolContext {
    pub(crate) fn index(self) -> usize {
        self.0
    }
}

/// One declaration or type change in expanded parser execution order.
#[derive(Clone, Copy, Debug)]
pub struct SymbolTypeEvent {
    pub name: Name,
    pub span: Span,
    pub kind: SymbolKind,
    pub changed: bool,
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
    /// Type history when this row was parsed, including previous macro iterations.
    pub symbol_type_context: SymbolContext,
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

/// Symbol role seen when an estimated-parameter removal row is parsed.
/// Source spans can repeat under macro expansion, so this is captured in parse order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EstimatedNameRole {
    Unknown,
    Endogenous,
    Exogenous,
    Parameter,
    Other,
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
    /// The discretionary command can initialize its discount parameter.
    pub discount_parameter_valid: bool,
    /// Whether the discount symbol already existed after reading these options.
    pub discount_symbol_existed: bool,
}

/// An instrument occurrence with the type it had when its command was parsed.
#[derive(Clone, Copy, Debug)]
pub struct PolicyInstrumentUse {
    pub name: Name,
    pub span: Span,
    pub command_span: Span,
    pub kind: Option<&'static str>,
    /// Statement identity in parser order; written spans may repeat in macros.
    pub command_index: usize,
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
    pub parse_order: usize,
    /// Names already declared in effective parser order, including generated names.
    pub known_names: Vec<Name>,
    /// Names already used before this type-change statement.
    pub used_names: Vec<Name>,
    pub new_type: ChangeTypeKind,
    /// Listed names with their identifier spans, source order.
    pub names: Vec<(Name, Span)>,
    /// Statement keyword through `;`.
    pub span: Span,
}

/// One `trend_var` / `log_trend_var` name.
#[derive(Clone, Debug)]
pub struct TrendVar {
    pub parse_order: usize,
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
    /// Types captured after the row's expression, in effective parser order.
    pub first_kind: Option<&'static str>,
    pub second_kind: Option<&'static str>,
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
    pub parse_order: usize,
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
    /// Type history when this row was parsed, including previous macro iterations.
    pub symbol_type_context: SymbolContext,
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
    pub group_span: Option<Span>,
    pub rows: Vec<Init2ShocksRow>,
    pub span: Span,
}

/// One `name, expr, expr;` row of a `homotopy_setup` block.
#[derive(Clone, Debug)]
pub struct HomotopyRow {
    /// Type history when this row was parsed, including previous macro iterations.
    pub symbol_type_context: SymbolContext,
    pub name: Name,
    pub span: Span,
}

/// One `'group' = name_list;` row of a `shock_groups` block.
#[derive(Clone, Debug)]
pub struct ShockGroup {
    /// Type history when this row was parsed, including previous macro iterations.
    pub symbol_type_context: SymbolContext,
    /// The row's label, quotes stripped (`'g1'` and `g1` are the same label).
    pub label: String,
    /// The label token's span (inside the quotes for a quoted label).
    pub label_span: Span,
    pub members: Vec<(Name, Span)>,
    pub span: Span,
}

/// One named or default `shock_groups` block over the existing flat row list.
#[derive(Clone, Debug)]
pub struct ShockGroupBlock {
    pub group: String,
    pub group_span: Option<Span>,
    pub row_start: usize,
    pub row_end: usize,
    pub span: Span,
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
    /// Type history when this row was parsed, including previous macro iterations.
    pub symbol_type_context: SymbolContext,
    pub name: Name,
    pub name_span: Span,
    pub name_role_at_remove: EstimatedNameRole,
    pub kind: EstimatedParamKind,
    pub corr_with: Option<Name>,
    pub corr_with_span: Option<Span>,
    pub corr_role_at_remove: EstimatedNameRole,
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
    pub symbol_type_context: SymbolContext,
    pub(crate) parse_order: usize,
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
    /// DATE and offset token spellings after macro expansion.
    pub(crate) constructor_text: String,
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

/// The written value and expression tree of a shock or path instruction.
#[derive(Clone, Debug)]
pub struct WrittenValue {
    pub text: String,
    pub span: Span,
    pub expr: Option<ExprId>,
    /// The complete expression and required separator reached their action.
    pub(crate) completed: bool,
    /// Names and qualified references in a `shock_paths` value. Empty on
    /// ordinary shock values. The raw text above remains authoritative.
    pub path_refs: Vec<PathReference>,
}

#[derive(Clone, Debug)]
pub struct PathReference {
    pub symbol_type_context: SymbolContext,
    pub namespace: Option<String>,
    pub name: Name,
    pub span: Span,
    pub ident_span: Span,
    pub lag_span: Option<Span>,
    pub(crate) constructed_lag: Option<i32>,
    pub lag: Option<String>,
    /// Whether a parenthesized lag was written, including an empty `()`.
    pub lag_call: bool,
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
    pub symbol_type_context: SymbolContext,
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
    pub symbol_type_context: SymbolContext,
    pub target: PathTarget,
    pub periods: Vec<PeriodRange>,
    pub values: Vec<WrittenValue>,
    pub(crate) callback_completed: bool,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct PathBlock {
    pub options: ShockOptions,
    pub stanzas: Vec<PathStanza>,
    pub(crate) completed: bool,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct EndvalEntry {
    pub symbol_type_context: SymbolContext,
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

/// One name whose type `var_remove` changed while the file was parsed.
#[derive(Clone, Debug)]
pub struct VarRemovedName {
    pub symbol_type_context: SymbolContext,
    pub(crate) parse_order: usize,
    pub name: Name,
    pub name_span: Span,
    pub statement: Span,
}

#[derive(Clone, Debug)]
pub enum SubsampleInstruction {
    Declare {
        symbol_type_context: SymbolContext,
        head: SubsampleHead,
        ranges: Vec<SubsampleRange>,
        span: Span,
    },
    Copy {
        symbol_type_context: SymbolContext,
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

/// The `irf_shocks` option uses the same name/type rule on `stoch_simul` and
/// `estimation`. Names retain their written spans and command order.
#[derive(Clone, Debug)]
pub struct IrfShocksOption {
    pub symbol_type_context: SymbolContext,
    pub command: String,
    pub span: Span,
    pub names: Vec<(Name, Span)>,
}

#[derive(Clone, Debug)]
pub struct Assignment {
    pub symbol_type_context: SymbolContext,
    pub name: Name,
    pub expression: String,
    pub span: Span,
    /// P-expr tree of the RHS. `None` when the statement was recovered
    /// from a joined string (endval) rather than `parse_expr`.
    pub expr: Option<ExprId>,
    /// Retained native MATLAB text, whose RHS declares no Dynare symbols.
    pub native: bool,
    /// Expanded-token indexes of this assignment execution. Empty means the
    /// execution was not recorded. A raw written span is not a substitute.
    pub active_tokens: Range<usize>,
}

/// Proof for the new assignment trace; legacy numeric readers ignore it.
#[derive(Clone, Debug)]
pub struct AssignmentSyntax {
    pub full_rhs: bool,
    pub written_plain_number: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    pub source: String,
    /// Tokens after macro expansion. Equation and assignment `active_tokens`
    /// index this vector. Empty when the model was not produced by the parser.
    pub expanded_tokens: Vec<crate::lexer::Token>,
    pub intern: Interner,
    pub statements: Vec<Statement>,
    pub execution_steps: Vec<ExecutionStep>,
    pub assignment_syntax: HashMap<ExprId, AssignmentSyntax>,
    pub numeric_literals: HashMap<ExprId, f64>,
    /// Exact Parse constant spelling, including macro-produced numeric tokens.
    pub(crate) numeric_literal_texts: HashMap<ExprId, String>,
    pub opaque_tokens: HashMap<usize, Vec<crate::lexer::Token>>,
    pub written_declarations: Vec<WrittenDeclaration>,
    pub written_equations: Vec<WrittenEquation>,
    pub type_event_occurrences: Vec<(usize, Range<usize>)>,
    pub write_targets: Vec<WrittenWrite>,
    pub endogenous: Vec<Decl>,
    pub exogenous: Vec<Decl>,
    pub deterministic_exogenous: Vec<Decl>,
    /// Written parameter declarations. For types use final_parameters or a captured parser context.
    pub parameters: Vec<Decl>,
    pub predetermined: Vec<Decl>,
    pub param_assignments: Vec<Assignment>,
    pub helper_assignments: Vec<Assignment>,
    pub equations: Vec<Equation>,
    /// SUM argument role when the call was parsed; later removal cannot invalidate that earlier role check.
    pub sum_argument_roles: HashMap<ExprId, bool>,
    pub semi_structural_commands: Vec<SemiStructuralCommand>,
    pub named_model_operators: Vec<NamedModelOperator>,
    pub pac_target_info: Vec<PacTargetInfoBlock>,
    pub deterministic_trends: Vec<DeterministicTrendsBlock>,
    /// A repeated leading name within one independent `deterministic_trends` block.
    pub deterministic_trends_dups: Vec<(Name, Span)>,
    /// `model_remove` / `model_replace` statements, file order, with what each removed.
    pub equation_surgery: Vec<EquationSurgery>,
    /// Endogenous a `model_remove` dropped from the model (7.1's `excludedVariable`
    /// type). The symbol is no longer in `endogenous`, but it was declared before the
    /// removal: `filter_initial_state` refuses it with the timing message, not with
    /// the undeclared one.
    pub excluded_endogenous: Vec<Decl>,
    /// `var_remove` changes these names to the excluded type. Command lists
    /// read the final type, whereas expressions are checked in source order.
    pub var_removed: Vec<VarRemovedName>,
    /// Names declared with `model_local_variable`, separate from `#` rows.
    pub model_local_variables: Vec<Decl>,
    /// Uses captured while the name is excluded in a model-expression context.
    pub var_removed_model_uses: Vec<(Name, Span, usize)>,
    /// Every name a `model_remove` took out of the model, file order. A check that reads
    /// a name's type reads it as of the statement it is looking at: 7.1 validated that
    /// statement while the name was still endogenous.
    pub surgery_exits: Vec<SurgeryExit>,
    /// Source-order type history, including repeated macro-origin spans.
    pub symbol_type_events: Vec<SymbolTypeEvent>,
    /// Preserve the written assignment even though the live model drops it.
    pub pruned_initializations: Vec<PrunedInitialization>,
    pub steady_state_equations: Vec<Equation>,
    /// Bare RHS uses with their Parse-time symbol history. Macro copies may
    /// share written spans, and later type changes must not alter these roles.
    pub steady_state_rhs_uses: Vec<(Name, Span, SymbolContext)>,
    /// Ordinary expression uses in effective Parse order, excluding native text.
    pub outside_expression_uses: Vec<(Name, Span, SymbolContext)>,
    /// The private official DataTree used by each completed expression allocation.
    pub(crate) constructor_scopes: HashMap<ExprId, crate::constructor::DataTreeScope>,
    /// Facts collected from completed shock-path constructor roots. Written
    /// references remain separate for comparison and source ownership.
    pub(crate) path_value_facts: HashMap<ExprId, crate::constructor::PathValueFacts>,
    /// Constructor failure before the owning reader can flush deferred names.
    pub(crate) constructor_refused_statements: HashSet<usize>,
    /// Pound callbacks not reached because the written RHS was refused.
    pub(crate) unattempted_model_local_targets: HashSet<ExprId>,
    /// Symbol history after the RHS, before a pound callback checks its target.
    pub(crate) model_local_target_contexts: HashMap<ExprId, SymbolContext>,
    pub model_expression_uses: Vec<ModelExpressionUse>,
    pub initval: Vec<Assignment>,
    pub endval: Vec<Assignment>,
    pub is_linear: bool,
    /// `model(differentiate_forward_vars)` was written. A comment that mentions
    /// the name is not this option. The count check withholds E188 when it is set,
    /// because the rewrite may add helper variables.
    pub differentiate_forward_vars: bool,
    /// Identifier span of `block` inside an aggregate `model(…)` option list.
    pub model_block_option: Option<Span>,
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
    /// A written partial_information option on a command that sets the model flag.
    pub partial_information: bool,
    pub irf_shocks_options: Vec<IrfShocksOption>,
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
    /// Every instrument occurrence in effective parser order, before later retypes.
    pub instrument_uses: Vec<PolicyInstrumentUse>,
    /// First `planner_discount` option that folds; later options do not overwrite.
    pub planner_discount: Option<f64>,
    /// First `planner_discount` expression (first-wins; beside the folded float).
    pub planner_discount_expr: Option<ExprId>,
    /// Names from `osr_params …;`.
    pub osr_params: Vec<Name>,
    /// True iff an `optim_weights;` … `end;` block is present.
    pub has_optim_weights: bool,
    /// Keyword span of the first `optim_weights` block.
    pub optim_weights_span: Option<Span>,
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
    /// Proven positions of parser refusals in the original expanded stream.
    /// Written spans can repeat or run backwards through a macro loop.
    pub(crate) parse_issue_orders: Vec<(Span, usize)>,
    pub(crate) expanded_token_origins: Vec<Range<usize>>,
    /// Reserved-symbol expression uses captured while reading Dynare blocks.
    pub reserved_block_symbol_uses: Vec<Span>,
    /// Names that were still trend variables when an ordinary expression used them.
    pub trend_outside_uses: Vec<(Name, Span)>,
    /// Unknown function calls read inside epilogue before later declarations.
    pub epilogue_undeclared_calls: Vec<(Name, Span)>,
    /// Literal `@#include` directives (quoted or bare path). Identifier-only
    /// arguments (`@#include FOO`) are not recorded.
    pub includes: Vec<IncludeDirective>,
    /// `@#includepath` directives (raw argument; workspace resolves one path).
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
    /// Parsed `estimated_params_remove` rows, checked at the same parse surface.
    pub estimated_params_remove: Vec<EstimatedParam>,
    pub estimated_params_remove_block_starts: Vec<usize>,
    pub estimated_params_remove_span: Option<Span>,
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
    /// Refused bare namespace expression nodes. Identity distinguishes macro
    /// occurrences that have the same written span.
    pub namespace_qualified_exprs: HashSet<ExprId>,
    /// Interned-0 fold errors while building (span, code, 7.1 message).
    pub const_fold_errors: Vec<(Span, &'static str, String)>,
    /// Completed shock_paths Parse actions, in parser execution order.
    pub(crate) path_parse_errors: Vec<(Span, &'static str, String)>,
    /// `external_function(name=…)` identifiers (command body otherwise skipped).
    pub external_function_names: Vec<Name>,
    /// Auto-declared names from non-model expressions.
    pub mod_file_locals: Vec<Name>,
    /// Macro type errors from expansion (`@#if` not bool, `@#for` tuple, `+` mismatch).
    pub macro_type_errors: Vec<(Span, &'static str, String)>,
    /// First unsupported macro directive/interpolation in the written file.
    /// Present when valid macro text remains unexpanded and parsed rows are incomplete.
    pub macro_incomplete_span: Option<Span>,
    /// Named incomplete-expansion failures (I211), source order, deduplicated.
    pub incomplete_reasons: Vec<crate::macro_expand::IncompleteReason>,
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
    /// Declaration records for non-ordinary symbols successfully changed to an
    /// ordinary type. Original source lists remain intact; the historical field
    /// name is retained for the Rust interface.
    pub retyped_trend_decls: Vec<Decl>,
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
    /// Named/default block identities and spans for compare. Diagnostics keep
    /// using `shock_group_block_starts` and the flat rows above.
    pub shock_group_blocks: Vec<ShockGroupBlock>,
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
    /// Every `heterogeneity_dimension` name, file order (one record per name).
    pub heterogeneity_dimensions: Vec<HeterogeneityDimension>,
    /// `model(heterogeneity=d); … end;` blocks, file order. `equations` above
    /// stays written-aggregate so `n_model_equations` is unchanged.
    pub heterogeneous_models: Vec<HeterogeneousModelBlock>,
    /// Every `heterogeneity_*` command statement, file order.
    pub heterogeneity_commands: Vec<HeterogeneityCommand>,
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
    pub symbol_type_context: SymbolContext,
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
    /// Type history when this row was parsed, including previous macro iterations.
    pub symbol_type_context: SymbolContext,
    pub(crate) parse_order: usize,
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
    /// Proven position of this execution in the original expanded token stream.
    pub(crate) parse_order: Option<usize>,
    /// Executed statement bounds when a producer token is not independently
    /// identifiable. Earlier statements still precede this definite refusal.
    pub(crate) parse_execution: Option<Range<usize>>,
    /// The token the pinned preprocessor stops on.
    pub span: Span,
    /// The command, keyword or head the message names, as written.
    pub subject: String,
    /// What the grammar takes there for a generic hint. Empty when the whole
    /// statement has no form or `official_message` supplies the exact text.
    pub expected: &'static str,
    /// Exact official parser text for a known refusal, when available.
    pub official_message: Option<Cow<'static, str>>,
}

impl ShapeRefuse {
    pub(crate) fn with_parse_order(mut self, order: usize) -> Self {
        self.parse_order = Some(order);
        self
    }

    pub fn new(span: Span, subject: impl Into<String>, expected: &'static str) -> Self {
        Self {
            parse_order: None,
            parse_execution: None,
            span,
            subject: subject.into(),
            expected,
            official_message: None,
        }
    }

    pub fn official(
        span: Span,
        subject: impl Into<String>,
        message: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            parse_order: None,
            parse_execution: None,
            span,
            subject: subject.into(),
            expected: "",
            official_message: Some(message.into()),
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
    pub symbol_type_context: SymbolContext,
    /// Completion position of this execution in the original expanded token stream.
    pub(crate) parse_order: usize,
    pub kind: DottedKind,
    /// Retained `prior`, `options`, or `subsamples` token after the head.
    pub keyword_span: Span,
    /// The head the statement is keyed on.
    pub head: DottedHead,
    /// Head through the terminating `;`.
    pub span: Span,
    /// A parenthesized body, as opposed to a `prior` / `options` copy.
    pub has_body: bool,
    /// Source head of a `prior` / `options` copy. Copy forms check name types
    /// but do not look up named subsample declarations.
    pub copy_source: Option<DottedHead>,
    /// Parsed option rows for `prior` and `options` bodies. Empty for
    /// `subsamples` declarations and dotted copy forms.
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
    /// Producer positions in the original expanded token stream. Several
    /// emitted tokens can share one interpolation's written span.
    pub(crate) parse_order: usize,
    pub(crate) value_parse_order: usize,
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
    /// Completed element actions in executed token order, parallel to `elements`.
    pub(crate) element_parse_orders: Vec<Option<usize>>,
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
    pub(crate) parse_order: Option<usize>,
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
    pub(crate) parse_order: Option<usize>,
    pub(crate) shape_parse_order: usize,
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
    /// MODEL_EXPRESSION uses allocated for this row in parser execution order.
    pub(crate) expression_uses: Range<usize>,
    /// The moment walk reads symbol types when its block reaches `end;`.
    pub(crate) walk_context: SymbolContext,
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
#[derive(Clone, Debug)]
pub struct EstimationStatement {
    /// The command identifier's span. The **E227** row points here.
    pub span: Span,
    /// This statement carried `datafile=`.
    pub has_datafile: bool,
    /// Only the written options that locate heteroskedastic observations.
    pub data_options: Vec<FamilyOption>,
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
    /// Pinned Bison syntax sentence while reading a new surface.
    BisonSyntax(String),
    /// A complete command lacks its final `;`. The refusing token still owns
    /// the official message and range; active tokens prove the edit separately.
    BisonMissingSemi {
        message: String,
        active_tokens: std::ops::Range<usize>,
    },
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
    pub(crate) fn model_local_action_attempted(&self, equation: &Equation) -> bool {
        !equation
            .lhs_expr
            .is_some_and(|target| self.unattempted_model_local_targets.contains(&target))
    }

    /// Roles that ParsingDriver allows in an ordinary expression outside model.
    pub(crate) fn outside_expression_symbol_is_valid(
        &self,
        name: Name,
        context: SymbolContext,
    ) -> bool {
        !self.heterogeneous_in_context(name, context)
            && !matches!(
                self.symbol_kind_in_context(name, context),
                Some(
                    "model_local_variable"
                        | "external_function"
                        | "epilogue"
                        | "trend_var"
                        | "log_trend_var"
                        | "excluded"
                )
            )
    }

    /// ParsingDriver validates steady-state outputs after reading their RHS.
    pub(crate) fn steady_state_target_is_valid(&self, target: &SteadyStateTarget) -> bool {
        matches!(
            self.symbol_kind_in_context(target.name, target.symbol_type_context),
            None | Some("var" | "parameters" | "mod_file_local")
        ) && !self.heterogeneous_in_context(target.name, target.symbol_type_context)
    }

    /// A rejected model row makes uses and counts from that body incomplete.
    pub(crate) fn model_rows_rejected(&self) -> bool {
        self.parse_issues.iter().any(|issue| {
            self.model_block
                .into_iter()
                .chain(self.heterogeneous_models.iter().map(|block| block.span))
                .any(|block| block.start <= issue.span.start && issue.span.start <= block.end)
        })
    }

    pub fn name(&self, name: Name) -> &str {
        self.intern.get(name)
    }

    /// The spans of every statement this parser read on purpose, so the text-level
    /// passes (invalid identifiers, missing semicolons) can leave their contents
    /// alone. Every parsed surface must be listed here, or the passes will read its
    /// option values as declarations or as parameter assignments.
    pub fn statement_spans(&self) -> Vec<Span> {
        let mut spans: Vec<Span> = self
            .semi_structural_commands
            .iter()
            .map(|command| command.span)
            .chain(self.pac_target_info.iter().map(|block| block.span))
            .chain(self.deterministic_trends.iter().map(|block| block.span))
            .chain(self.shock_blocks.iter().map(|block| block.span))
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
            .chain(self.heterogeneity_dimensions.iter().map(|dim| dim.span))
            .chain(self.heterogeneous_models.iter().map(|block| block.span))
            .chain(self.heterogeneity_commands.iter().map(|stmt| stmt.span))
            .collect();
        spans.sort_by_key(|span| (span.start, span.end));
        spans
    }

    pub fn macro_incomplete(&self) -> bool {
        self.macro_incomplete_span.is_some()
    }

    /// Last type recorded for `name`, in parser execution order.
    ///
    /// `var_remove` leaves the declaration on its original list and appends an
    /// `excluded` event. A later `change_type` appends the restored type. Callers
    /// that care about the symbol's final type read this, not the raw removal log.
    pub fn final_symbol_kind(&self, name: Name) -> Option<&'static str> {
        self.symbol_type_events
            .iter()
            .rev()
            .find(|event| event.name == name)
            .map(|event| event.kind.as_str())
    }

    /// Capture the current parser position; never derive this from a source span.
    pub(crate) fn symbol_context(&self) -> SymbolContext {
        SymbolContext(self.symbol_type_events.len())
    }

    /// Compatibility name for the excluded-name fallback. Internal readers use
    /// the explicit name so that they cannot mistake it for the final table type.
    pub fn final_kind(&self, name: Name) -> Option<&'static str> {
        self.final_kind_or_written_if_excluded(name)
    }

    /// Type at a captured parser position, independent of written source order.
    pub(crate) fn symbol_kind_in_context(
        &self,
        name: Name,
        context: SymbolContext,
    ) -> Option<&'static str> {
        self.symbol_type_events[..context.index()]
            .iter()
            .rev()
            .find(|event| event.name == name)
            .map(|event| event.kind.as_str())
    }

    pub(crate) fn parameter_in_context(&self, name: Name, context: SymbolContext) -> bool {
        self.symbol_kind_in_context(name, context) == Some("parameters")
    }

    /// Whether a name still has its declared dimension at a parser position.
    /// A successful type change makes it ordinary, even if the keyword stays
    /// `var` or `parameters`. Macro copies can share spans, so use event order.
    pub(crate) fn heterogeneous_in_context(&self, name: Name, context: SymbolContext) -> bool {
        if self.symbol_type_events[..context.index()]
            .iter()
            .any(|event| event.name == name && event.changed)
        {
            return false;
        }
        self.endogenous
            .iter()
            .chain(&self.exogenous)
            .chain(&self.parameters)
            .any(|decl| {
                decl.name == name
                    && decl.symbol_type_context.index() <= context.index()
                    && decl.heterogeneity.is_some()
            })
    }

    /// An ordinary declaration view at a recorded symbol's first written site.
    /// Implicit locals have a type-event occurrence rather than a declaration
    /// keyword. Preserve its execution order and context without inventing one.
    pub(crate) fn retyped_declaration(&self, name: Name) -> Option<Decl> {
        if let Some(written) = self
            .written_declarations
            .iter()
            .find(|row| row.declaration.name == name)
        {
            return Some(written.declaration.clone());
        }
        let (event_index, event) = self
            .symbol_type_events
            .iter()
            .enumerate()
            .find(|(_, event)| event.name == name && !event.changed)?;
        let (_, occurrence) = self
            .type_event_occurrences
            .iter()
            .find(|(index, _)| *index == event_index)?;
        Some(Decl {
            parse_order: occurrence.end,
            symbol_type_context: SymbolContext(event_index + 1),
            name,
            span: event.span,
            long_name: None,
            tex_name: None,
            log_transform: false,
            heterogeneity: None,
        })
    }

    /// Final type of `name` for a check that asks what the name is.
    ///
    /// This is [`Self::final_symbol_kind`], except an `excluded` name uses the
    /// list it was written on. Removal checks already read that list.
    pub fn final_kind_or_written_if_excluded(&self, name: Name) -> Option<&'static str> {
        match self.final_symbol_kind(name) {
            Some("excluded") | None => self.written_type(name),
            Some(kind) => Some(kind),
        }
    }

    fn written_type(&self, name: Name) -> Option<&'static str> {
        if self.endogenous.iter().any(|decl| decl.name == name) {
            Some("var")
        } else if self
            .deterministic_exogenous
            .iter()
            .any(|decl| decl.name == name)
        {
            Some("varexo_det")
        } else if self.exogenous.iter().any(|decl| decl.name == name) {
            Some("varexo")
        } else if self.parameters.iter().any(|decl| decl.name == name) {
            Some("parameters")
        } else {
            None
        }
    }

    /// Type `name` has at byte `at`: the last declaration or type change written
    /// before it. This source-position reader does not recover macro execution
    /// order; internal parse-time checks use their captured parser context.
    pub fn symbol_kind_before(&self, name: Name, at: u32) -> Option<&'static str> {
        self.symbol_type_events
            .iter()
            .rev()
            .find(|event| event.name == name && event.span.start < at)
            .map(|event| event.kind.as_str())
    }

    /// `name` is a parameter at byte `at`, falling back to the written list.
    pub fn parameter_at(&self, name: Name, at: u32) -> bool {
        match self.symbol_kind_before(name, at) {
            Some(kind) => kind == "parameters",
            None => self.parameters.iter().any(|decl| decl.name == name),
        }
    }

    /// Aggregate endogenous declarations by final type, first declaration per
    /// name. Declarations stay on their written list, so a `change_type(var)`
    /// parameter or exogenous name is endogenous here and a retyped `var` is not.
    pub fn final_endogenous(&self) -> Vec<&Decl> {
        self.final_decls(&["var"])
            .into_iter()
            .filter(|decl| self.final_heterogeneity(decl).is_none())
            .collect()
    }

    /// A successful type change makes a heterogeneous declaration ordinary.
    /// Keep the written dimension on `Decl` for source-oriented reads.
    pub(crate) fn final_heterogeneity(&self, decl: &Decl) -> Option<Name> {
        let changed = self
            .symbol_type_events
            .iter()
            .any(|event| event.name == decl.name && event.changed);
        if changed {
            None
        } else {
            decl.heterogeneity.map(|(name, _)| name)
        }
    }

    /// Parameter declarations by final type, first declaration per name, the
    /// same way `final_endogenous` reads endogenous ones.
    pub fn final_parameters(&self) -> Vec<&Decl> {
        self.final_decls(&["parameters"])
    }

    /// Heterogeneous endogenous among the types recorded so far. Parsing SUM
    /// captures this answer before later directives can change the symbol.
    pub(crate) fn is_heterogeneous_endogenous(&self, name: Name) -> bool {
        self.final_kind_or_written_if_excluded(name) == Some("var")
            && self
                .endogenous
                .iter()
                .any(|decl| decl.name == name && self.final_heterogeneity(decl).is_some())
    }

    /// First declaration per name whose final type is one of `kinds`.
    pub(crate) fn final_decls(&self, kinds: &[&str]) -> Vec<&Decl> {
        let mut final_kind = HashMap::new();
        for event in &self.symbol_type_events {
            final_kind.insert(event.name, event.kind.as_str());
        }
        let lists = [
            (&self.endogenous, "var"),
            (&self.exogenous, "varexo"),
            (&self.deterministic_exogenous, "varexo_det"),
            (&self.parameters, "parameters"),
        ];
        let (written, other): (Vec<_>, Vec<_>) = lists
            .into_iter()
            .partition(|(_, written_kind)| kinds.contains(written_kind));
        let mut seen = HashSet::new();
        written
            .into_iter()
            .chain(other)
            .flat_map(|(list, written_kind)| list.iter().map(move |decl| (decl, written_kind)))
            .chain(
                self.retyped_trend_decls
                    .iter()
                    .map(|decl| (decl, "trend_var")),
            )
            .filter(|(decl, written_kind)| {
                kinds.contains(&final_kind.get(&decl.name).copied().unwrap_or(written_kind))
            })
            .map(|(decl, _)| decl)
            .filter(|decl| seen.insert(decl.name))
            .collect()
    }

    /// Final type is `varexo` or `varexo_det`, falling back to the written lists.
    pub fn final_exogenous(&self, name: Name) -> bool {
        match self.final_symbol_kind(name) {
            Some(kind) => matches!(kind, "varexo" | "varexo_det"),
            None => self.exogenous.iter().any(|decl| decl.name == name),
        }
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
        if !eq.steady_state_targets.is_empty() {
            out.extend(eq.steady_state_targets.iter().map(|target| IdentRef {
                name: target.name,
                span: target.span,
                timing: 0,
                timing_span: None,
            }));
        } else if let Some(id) = eq.lhs_expr {
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
