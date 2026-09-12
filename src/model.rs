//! Parsed `.mod` model. This is the seam diagnostic families and transports share.

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
    pub span: Span,
}

/// One `var` / `corr` statement inside a `shocks` block (`stderr` is omitted).
#[derive(Clone, Debug)]
pub struct ShockStmt {
    pub kind: ShockKind,
    /// Folded RHS (`var name = expr` / `corr a, b = expr`). `None` if missing or unevaluable.
    pub rhs: Option<f64>,
    /// From `var`/`corr` through the statement `;`.
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ShockKind {
    /// `var name` (optional `= variance`).
    Var(Name),
    /// `var n1, n2, … = covariance` (two or more names, source order).
    Cov(Vec<Name>),
    /// `corr a, b = expr`.
    Corr { a: Name, b: Name },
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
    /// Whole `planner_objective …;` statement if present.
    pub planner_objective_span: Option<Span>,
    /// Unique, first-seen, from `instruments=(…)` on any policy command.
    pub instruments: Vec<Name>,
    /// First `planner_discount` option that folds; later options do not overwrite.
    pub planner_discount: Option<f64>,
    /// Names from `osr_params …;`.
    pub osr_params: Vec<Name>,
    /// True iff an `optim_weights;` … `end;` block is present.
    pub has_optim_weights: bool,
    /// Identifier span of each top-level `simul` command (not `stoch_simul`).
    pub simul_spans: Vec<Span>,
    /// Identifier span of the first `ramsey_policy`.
    pub ramsey_policy_span: Option<Span>,
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
