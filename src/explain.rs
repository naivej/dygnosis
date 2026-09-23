//! Diagnostic code documentation.
//!
//! Mechanical port of `python_dynare_lsp/explain.py` `_ENTRIES` for the 70
//! codes shipped through 0.5.0, then kind `shared` / `skipped` / `added`.
//! 297 keys = 238 shared + 27 added + 32 skipped (S016 shipped as E335-E337). Catalog **0.5.1** D-clash and
//! D-check Errors are shared; the **0.5.2** D-walk, D-block, D-open, and D-extfun
//! rows add shared keys and drop the `S###` keys they replace. An
//! `S###` key is a placeholder for a row a named version will later drop and
//! replace with an emitted `E` / `W` code; a row no version owns keeps its real
//! letter from the start (`E186`-`E191` / `W186`-`W187`).
//! `I050` and `W042` use the recorded surface rewrites in
//! `dev_logs/0.1/0.1.0/22-c-explain.md` (do not advertise Compute Steady State).

use std::collections::HashMap;
use std::sync::OnceLock;

/// How a documented code relates to Dynare.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExplainKind {
    /// Official refuse or warn we also report.
    Shared,
    /// Catalog skip; listed and never reported.
    Skipped,
    /// Extra diagnostic Dynare never reports.
    Added,
}

impl ExplainKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Skipped => "skipped",
            Self::Added => "added",
        }
    }
}

/// Title, markdown body, and shared/skipped/added kind for one diagnostic code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExplainEntry {
    pub title: &'static str,
    pub body: &'static str,
    pub kind: ExplainKind,
}

// 306 keys: 249 shared + 27 added + 30 skipped.
static ENTRIES: &[(&str, ExplainEntry)] = &[
    ("E001", ExplainEntry {
        title: "Parse error",
        body: "The Dynare parser could not interpret the source. The diagnostic range points at the offending token or the nearest recoverable position. Dynare refuses with a generic bison `ERROR` at a location, or with `character unrecognized by lexer` when the source holds a double-quoted string.\n\n**Warrant**\n\nThe editor names the missing construct and points at a usable range; Dynare's bison location is often the next token, and its lexer note names no range at all. When a reserved preprocessor symbol (`dsge_prior_weight`) is used outside a declaration, the message names the symbol and the positions that accept it, instead of Dynare's bare `unexpected DSGE_PRIOR_WEIGHT`.\n\n**Common causes**\n\n- Missing semicolon at the end of a declaration or equation\n- Unbalanced parentheses, braces, or block keywords\n- Malformed time subscript such as `y(1)` where `y(+1)` was meant\n- A reserved keyword used as an identifier\n- A reserved preprocessor symbol used where an expression is expected\n\n**Fix**\n\nInspect the line cited and the line immediately preceding it. Dynare's preprocessor frequently flags the *next* line after a missing semicolon.",
        kind: ExplainKind::Shared,
    }),
    ("W013", ExplainEntry {
        title: "Equation count does not match endogenous variable count",
        body: "The number of equations inside the `model` block does not equal the number of endogenous variables declared in the `var` block.\n\nWhen ramsey_model, ramsey_policy, or discretionary_policy is present and instruments= lists N unique names, the expected gap is −N, not equality; a square file still warns.\n\n**Fix**\n\n- Add a missing equation, or remove a duplicate one\n- Declare the missing endogenous variable in `var`, or remove an   extra declaration\n- Check whether a commented-out equation was intended to be   active\n- For ramsey_model / ramsey_policy / discretionary_policy with N instruments, the intended gap is −N; do not add equations only to make the file square",
        kind: ExplainKind::Added,
    }),
    ("E020", ExplainEntry {
        title: "Undeclared identifier in model block",
        body: "An identifier appears in the `model` block but is not declared as a `var`, `varexo`, or `parameters` symbol. Dynare refuses: `Unknown symbol: alpph`.\n\n**Warrant**\n\nThe editor sentence names the undeclared identifier in the equation and may include a Did-you-mean suggestion; Dynare's string is the generic `Unknown symbol`.\n\n**Fix**\n\n- Add the identifier to the appropriate declaration block\n- Correct a typo (a close-match suggestion may appear)\n- If the symbol is a local helper, define it in the parameter   section before use",
        kind: ExplainKind::Shared,
    }),
    ("E023", ExplainEntry {
        title: "Predetermined variable not declared endogenous",
        body: "A name listed in `predetermined_variables` must also be declared as an endogenous variable in the `var` block. Dynare refuses: `Unknown symbol: e`.\n\n**Warrant**\n\nThe editor names the `predetermined_variables` role; Dynare's parse string is the generic `Unknown symbol`.\n\n**Fix**\n\n- Add the variable to the `var` declaration, or\n- Remove it from `predetermined_variables` if it is not actually   endogenous",
        kind: ExplainKind::Shared,
    }),
    ("E024", ExplainEntry {
        title: "Deterministic exogenous with a lead or lag",
        body: "A deterministic exogenous variable (``varexo_det``) is used with a lead or lag. Dynare refuses: `Exogenous deterministic variable tau cannot be given a lead or a lag.` Parameter leads/lags are accepted because Dynare treats them as fixed scalars.\n\n**Fix**\n\n- Remove the time subscript\n- If the dated quantity is state-dependent, model it as an endogenous variable instead",
        kind: ExplainKind::Shared,
    }),
    ("E025", ExplainEntry {
        title: "Invalid model-local (`#`) variable",
        body: "A model-local variable defined with `#` either reuses a declared `var`, `varexo`, or `parameters` name, or is used in an equation before its `#` definition. Dynare refuses: `… has wrong type or was already used on the right-hand side. You cannot use it on the left-hand side of a pound ('#') expression`.\n\n**Fix**\n\n- Rename the model-local helper so it does not clash with a declared symbol\n- Move the `#` definition above its first use\n- Or remove the declaration if the name was meant to be only a model-local helper",
        kind: ExplainKind::Shared,
    }),
    ("E026", ExplainEntry {
        title: "varexo_det with a perfect-foresight solver",
        body: "A ``varexo_det`` declaration cannot appear with ``simul``, ``perfect_foresight_solver``, or ``perfect_foresight_with_expectation_errors_solver``. Dynare refuses: `A .mod file cannot contain both one of {perfect_foresight_solver, simul, perfect_foresight_with_expectation_errors_solver} and varexo_det declaration (all exogenous variables are deterministic in this case)`.\n\n**Fix**\n\nRemove the ``varexo_det`` declaration (use ``varexo``), or drop the perfect-foresight solver.",
        kind: ExplainKind::Shared,
    }),
    ("E027", ExplainEntry {
        title: "varexo_det with Ramsey",
        body: "``ramsey_model`` and ``ramsey_policy`` cannot be used with deterministic exogenous variables. Dynare refuses: `ramsey_model and ramsey_policy are incompatible with deterministic exogenous variables`.\n\n**Fix**\n\nRemove the ``varexo_det`` declaration, or drop the Ramsey command.",
        kind: ExplainKind::Shared,
    }),
    ("E028", ExplainEntry {
        title: "varexo_det with identification",
        body: "The ``identification`` command, or the ``sensitivity(identification=1)`` option, cannot be used with deterministic exogenous variables. Dynare refuses: `identification is incompatible with deterministic exogenous variables`.\n\n**Fix**\n\nRemove the ``varexo_det`` declaration, or drop ``identification`` (or the ``identification=1`` option on ``sensitivity``).",
        kind: ExplainKind::Shared,
    }),
    ("E030", ExplainEntry {
        title: "Duplicate declaration across types or `#` twice",
        body: "The same identifier is declared in two different blocks (for example both `var` and `varexo`), in two different trend forms (`trend_var` and `log_trend_var`), or both in a declaration and in the `epilogue` block or on an `external_function` statement (`name=`, `first_deriv_provided`, `second_deriv_provided`). A model-local `#` name defined twice is the same code. Dynare refuses: `Symbol y declared twice with different types!` and `Local model variable foo declared twice.`\n\n**Fix**\n\nRemove the extra declaration. If you intended two related but distinct symbols, rename one.",
        kind: ExplainKind::Shared,
    }),
    ("W031", ExplainEntry {
        title: "Symbol declared twice with the same type",
        body: "The same identifier is declared more than once with the same type: in `var`, `varexo`, `varexo_det`, or `parameters`, as the same trend form twice, twice in the `epilogue` block, or as a function name repeated on `external_function` statements (`name=`, `first_deriv_provided`, `second_deriv_provided`). Dynare accepts and warns `Symbol y declared twice.`\n\n**Fix**\n\nRemove the redundant declaration.",
        kind: ExplainKind::Shared,
    }),
    ("W054", ExplainEntry {
        title: "Duplicate equation",
        body: "Two equations inside the `model` block are textually identical. The diagnostic cites the line where the duplicate appears and the line where the first occurrence was found. A duplicate can also show up as the equation-count Warning `W013`.\n\n**Fix**\n\nRemove the duplicate.",
        kind: ExplainKind::Added,
    }),
    ("W055", ExplainEntry {
        title: "Contradictory equation (always false)",
        body: "An equation reduces to a tautological falsehood, for example `0 = 1`.\n\n**Fix**\n\nRemove the equation, or restore a variable reference that was accidentally simplified away.",
        kind: ExplainKind::Added,
    }),
    ("W056", ExplainEntry {
        title: "Duplicate parameter assignment",
        body: "The same parameter is assigned a value more than once in the parameter section. The last assignment wins.\n\n**Fix**\n\nRemove one of the assignments, or rename if two distinct parameters were intended.",
        kind: ExplainKind::Added,
    }),
    ("W057", ExplainEntry {
        title: "Stray equation outside model block",
        body: "A line that looks like a model equation appears outside the `model` ... `end;` block.\n\n**Fix**\n\nMove the equation inside the `model` block, or convert it to a parameter assignment if it belongs at the top level.",
        kind: ExplainKind::Added,
    }),
    ("W062", ExplainEntry {
        title: "Circular @#include detected",
        body: "Two or more files include each other in a loop. The message names the chain, for example `a.mod -> b.mod -> a.mod`.\n\n**Common causes**\n\n- A submodel was refactored and now includes its parent\n- Two helper files cross-include each other for shared\n  parameters or steady-state definitions\n- A copy-paste mistake duplicated the include in the wrong\n  direction\n\n**Fix**\n\nBreak the cycle by removing one `@#include` along the chain. If both files genuinely need a shared block, extract that block into a third file and have both parents include it.",
        kind: ExplainKind::Added,
    }),
    ("E061", ExplainEntry {
        title: "Could not open @#include target",
        body: "An `@#include` directive names a file that could not be opened. Dynare refuses: `Could not open F. The following directories were searched` (then the directories actually searched).\n\n**Common causes**\n\n- A typo in the filename\n- The included file lives in a directory that isn't on the search paths\n- The file was renamed or moved without updating the   directive\n\n**Fix**\n\nCorrect the filename, add the missing file, or extend the search paths so the directory containing the include is visible.",
        kind: ExplainKind::Shared,
    }),
    ("E062", ExplainEntry {
        title: "Unmatched macro block",
        body: "A Dynare macro `@#if` block has no matching `@#endif`, or a `@#for` block has no matching `@#endfor`. Dynare refuses with a generic bison syntax `ERROR`.\n\n**Warrant**\n\nThe editor names the unmatched opener or stray closer and points at that directive; Dynare's bison location does not.\n\n**Common causes**\n\n- A copy-paste deleted the closing directive\n- Mismatched closers — `@#endif` accidentally written for   a `@#for`, or vice versa\n- A nested block missing its inner closer\n\n**Fix**\n\nAdd the missing `@#endif` or `@#endfor` at the appropriate scope, or remove the stray closer. Each `@#if` needs its own `@#endif`; each `@#for` its own `@#endfor`.",
        kind: ExplainKind::Shared,
    }),
    ("E063", ExplainEntry {
        title: "Undefined macro interpolation",
        body: "An active line still contains an unresolved `@{NAME}` macro interpolation. Dynare refuses: `Unknown variable N`.\n\n**Fix**\n\nDefine the macro with `@#define NAME = value` before the line that uses it, correct the macro name, or remove the interpolation.",
        kind: ExplainKind::Shared,
    }),
    ("E064", ExplainEntry {
        title: "Macro error directive",
        body: "An active Dynare macro `@#error` directive was reached. Dynare refuses: `Macro-processing error` plus the user message from the directive.\n\n**Fix**\n\nRemove the `@#error` directive, or guard it behind a macro condition that is false for this model variant.",
        kind: ExplainKind::Shared,
    }),
    ("E065", ExplainEntry {
        title: "Invalid steady_state operand",
        body: "The `steady_state(...)` operator must not contain exogenous shocks. Dynare refuses: `Exogenous variables are not allowed in the context of the STEADY_STATE() operator.`\n\n**Fix**\n\nRemove the exogenous variable from `steady_state(...)`, replace it with the intended endogenous or parameter expression, or rewrite the equation so the shock enters outside the operator.",
        kind: ExplainKind::Shared,
    }),
    ("E999", ExplainEntry {
        title: "Additional errors truncated",
        body: "More diagnostics were produced than are shown at once. Fix the visible errors first; the next analysis pass will surface anything that was previously hidden.",
        kind: ExplainKind::Added,
    }),
    ("I050", ExplainEntry {
        title: "No initval or steady_state_model block",
        body: "The file declares variables and equations but does not include an `initval` or `steady_state_model` block. This check is presence only; this tool does not compute a numerical steady state. A sibling FILENAME_steadystate.m also counts as presence.\n\n**Fix**\n\nAdd an `initval` block with initial guesses, or a `steady_state_model` block with closed-form assignments. For a numerical solve, use Dynare (for example `steady;` in MATLAB/Octave).",
        kind: ExplainKind::Added,
    }),
    ("W010", ExplainEntry {
        title: "Parameter declared but never assigned",
        body: "A name appears in the `parameters` block but no assignment was found in the parameter section or in `steady_state_model`. At runtime, the parameter will be undefined and most computations will fail.\n\n**Fix**\n\nAssign a numerical value, or remove the declaration if the parameter is no longer used.",
        kind: ExplainKind::Added,
    }),
    ("W011", ExplainEntry {
        title: "Parameter assignment cannot be evaluated",
        body: "An assignment like `phi = 1/(1-beta)` could not be evaluated because one or more right-hand-side symbols are not yet defined. The parameter falls back to undefined.\n\n**Fix**\n\nReorder the parameter section so that dependencies appear before dependents.",
        kind: ExplainKind::Added,
    }),
    ("W012", ExplainEntry {
        title: "Undeclared helper variable in parameter section",
        body: "An identifier appears on the right-hand side of a parameter assignment but is not declared as a parameter or known helper variable.\n\n**Fix**\n\nAdd a declaration, or replace the helper with an explicit numeric value.",
        kind: ExplainKind::Added,
    }),
    ("W020", ExplainEntry {
        title: "Endogenous variable never referenced in model",
        body: "An endogenous variable is declared in `var` but does not appear in any equation. Either remove the declaration or add the missing equation that uses the variable.",
        kind: ExplainKind::Added,
    }),
    ("E021", ExplainEntry {
        title: "Exogenous variable never referenced in model",
        body: "A shock declared in `varexo` does not appear in any equation. Dynare refuses: `unused_exo not used in model block. To bypass this error, use the nostrict option. This may lead to crashes or unexpected behavior.` A `varexo_det` declaration does not count — 7.1 accepts one that is never used.",
        kind: ExplainKind::Shared,
    }),
    ("W022", ExplainEntry {
        title: "Parameter declared but never referenced in model equations",
        body: "A parameter is declared and assigned but does not appear in any model equation. Dynare warns: `Parameter(s) unused_p not used in the model`.",
        kind: ExplainKind::Shared,
    }),
    ("W042", ExplainEntry {
        title: "Endogenous variable missing from steady_state_model",
        body: "The `steady_state_model` block does not assign a value for every endogenous variable. Dynare warns: `variable 'c' is not assigned a value`. Dynare will fall back to the `initval` value (or zero).\n\n**Fix**\n\nAdd the missing assignments. For a numerical solve, use Dynare (for example `steady;` in MATLAB/Octave).",
        kind: ExplainKind::Shared,
    }),
    ("E058", ExplainEntry {
        title: "Undeclared variable in a block that names symbols",
        body: "A block that names symbols refers to one that is not declared at all: an `initval` / `endval` entry, a `histval` lag, a `filter_initial_state` entry, an `init2shocks` pair, a `homotopy_setup` row, a `shock_groups` member, an `svar_identification` body row, a `conditional_forecast_paths` `var` row, a `std(…)` / `corr(…)` prior head name, or a name slot of a `moment_calibration` / `irf_calibration` row or of a `matched_irfs` / `matched_irfs_weights` row. Dynare refuses: `Unknown symbol: undeclared_zzz`.\n\n**Warrant**\n\nThe editor names the undeclared entry and its block; Dynare's string is the generic `Unknown symbol`.\n\n**Fix**\n\nDeclare the variable, or remove the stray entry.",
        kind: ExplainKind::Shared,
    }),
    ("W051", ExplainEntry {
        title: "Exogenous variable set in initval",
        body: "Setting an exogenous variable in `initval` has no effect on the steady-state computation. Shocks are zero at the deterministic steady state by construction.",
        kind: ExplainKind::Added,
    }),
    ("W052", ExplainEntry {
        title: "Endogenous variable missing from initval",
        body: "The `initval` block does not provide an initial guess for every endogenous variable. The solver will start from zero for the missing entries, which may slow or prevent convergence on nonlinear models.",
        kind: ExplainKind::Added,
    }),
    ("E059", ExplainEntry {
        title: "Name in initval/endval or a std/corr prior head is neither endogenous or exogenous",
        body: "An `initval` or `endval` entry, a `histval` entry, or a `std(…)` / `corr(…)` prior head names a symbol that is not endogenous or exogenous (for example a parameter). Dynare refuses: `… is neither endogenous or exogenous.`\n\n**Fix**\n\nAssign parameters before the model block, or inside `steady_state_model`. Use `initval` / `endval` only for endogenous or exogenous variables, and a plain `name.prior(…)` head for a parameter.",
        kind: ExplainKind::Shared,
    }),
    ("W060", ExplainEntry {
        title: "Exogenous variables declared but no shocks block",
        body: "One or more exogenous variables are declared in `varexo` but the file contains no `shocks` block specifying their variance-covariance structure. The model is then deterministic.\n\n**Fix**\n\nAdd a `shocks` block to define the shock processes, or remove the unused `varexo` declarations.",
        kind: ExplainKind::Added,
    }),
    ("W061", ExplainEntry {
        title: "Ambiguous include parent context",
        body: "The active include file is reachable from more than one parent model, so the editor cannot tell which parent's declarations apply.\n\n**Fix**\n\nOpen the intended parent `.mod` file.",
        kind: ExplainKind::Added,
    }),
    ("W070", ExplainEntry {
        title: "Parameter outside its conventional range",
        body: "A parameter assignment falls outside the theoretically admissible range for its standard interpretation. The conventional-range table is opinionated but conservative: it flags values that violate the *theoretical* admissible range under the parameter's conventional meaning, not values that simply look unusual.\n\n**Common causes**\n\n- Unit error: e.g. `beta = 99` when 0.99 was meant\n- Sign error on a quantity that must be non-negative   (variance, standard deviation, depreciation rate)\n- Gross-vs-net confusion on a rate parameter\n\n**Fix**\n\nCorrect the value, or — if the calibration is intentional — ignore the warning.",
        kind: ExplainKind::Added,
    }),
    ("E090", ExplainEntry {
        title: "Observed variable is not a declared endogenous variable",
        body: "A name listed in ``varobs`` is not a declared endogenous variable. Dynare refuses: `e is not endogenous.`\n\n**Warrant**\n\nThe editor names the ``varobs`` role and, when the name is already declared as something else, says so; Dynare's string is only `N is not endogenous.`\n\n**Fix**\n\nDeclare the variable in ``var``, or remove it from ``varobs`` if it was a typo or an exogenous/parameter name.",
        kind: ExplainKind::Shared,
    }),
    ("W091", ExplainEntry {
        title: "Duplicate observed variable",
        body: "A variable is listed more than once in ``varobs``. Each observed variable should appear exactly once.\n\n**Fix**\n\nRemove the duplicate entry.",
        kind: ExplainKind::Added,
    }),
    ("W092", ExplainEntry {
        title: "Stochastic singularity",
        body: "There are more observed variables (``varobs``) than shocks (structural shocks plus measurement errors). The likelihood is then stochastically singular and estimation cannot proceed: the model cannot generate enough independent variation to match the observed series.\n\n**Fix**\n\nAdd structural shocks, add measurement errors on the observed variables (an ``stderr`` on an observed variable), or reduce the number of observed variables so that observables ≤ shocks.",
        kind: ExplainKind::Added,
    }),
    ("E093", ExplainEntry {
        title: "estimated_params references an undeclared symbol",
        body: "An ``estimated_params`` entry names a symbol that is not declared with the expected role: a plain entry must name a parameter, an ``stderr`` or ``skew`` entry must name a shock or observed variable, and a ``corr`` entry must name two declared shocks or variables. Dynare refuses: `Unknown symbol: not_a_param` (unknown ``skew`` is `in `estimated_params' block, unknown symbol: {name}`).\n\n**Warrant**\n\nThe editor names the ``estimated_params`` role (parameter, stderr, corr, or skew); Dynare's string is the generic `Unknown symbol` or the estimated-params unknown-symbol line.\n\n**Fix**\n\nDeclare the symbol, or correct the name / entry type.",
        kind: ExplainKind::Shared,
    }),
    ("W094", ExplainEntry {
        title: "estimated_params bound or initial-value inconsistency",
        body: "An ``estimated_params`` entry has a lower bound that is not below its upper bound, or an initial value that lies outside the ``[lower, upper]`` interval.\n\n**Fix**\n\nOrder the bounds so that lower < upper and place the initial value inside them.",
        kind: ExplainKind::Added,
    }),
    ("E095", ExplainEntry {
        title: "observation_trends variable not in varobs",
        body: "A variable given a trend in ``observation_trends`` is not listed in ``varobs``. Dynare refuses: `variable y in observation_trends block is not an observed variable`.\n\n**Fix**\n\nAdd the variable to ``varobs`` or remove its trend specification.",
        kind: ExplainKind::Shared,
    }),
    ("E100", ExplainEntry {
        title: "planner_objective and optimal-policy commands go together",
        body: "``planner_objective`` must appear with ``ramsey_model``, ``ramsey_policy``, ``osr``, or ``discretionary_policy``, and those commands (except ``osr``) need ``planner_objective``. Dynare refuses either missing direction: `A planner_objective statement must be used with a ramsey_model, a ramsey_policy, osr, or a discretionary_policy statement and vice versa.`\n\n**Fix**\n\nAdd the missing ``planner_objective <expression>;``, or add a matching policy command.",
        kind: ExplainKind::Shared,
    }),
    ("E101", ExplainEntry {
        title: "Policy instrument is not a declared endogenous variable",
        body: "An ``instruments=(...)`` entry names a symbol that is not a declared endogenous variable. Dynare refuses: `Unknown symbol: not_endo`.\n\n**Warrant**\n\nThe editor names the policy instrument; Dynare's string is the generic `Unknown symbol`.\n\n**Fix**\n\nDeclare the instrument in ``var``, or correct the instrument name.",
        kind: ExplainKind::Shared,
    }),
    ("W102", ExplainEntry {
        title: "planner_discount is not a valid discount factor",
        body: "``planner_discount`` must be a discount factor in the interval (0, 1]. A value outside this range is almost certainly a mistake (for example, entering a discount rate instead of a factor).\n\n**Fix**\n\nSet ``planner_discount`` to a value such as 0.99.",
        kind: ExplainKind::Added,
    }),
    ("E103", ExplainEntry {
        title: "osr is missing osr_params or optim_weights",
        body: "Optimal simple rules (``osr``) need an ``osr_params`` statement (the parameters to optimise). Dynare refuses when it is missing: `The osr statement requires the osr_params statement.` Dynare also refuses when neither ``optim_weights`` nor ``planner_objective`` is present: `The osr statement requires either an optim_weights block or a planner_objective.`\n\n**Fix**\n\nAdd the missing ``osr_params`` statement, and either an ``optim_weights`` block or a ``planner_objective``.",
        kind: ExplainKind::Shared,
    }),
    ("E104", ExplainEntry {
        title: "More than one planner_objective with Ramsey",
        body: "With ``ramsey_model`` or ``ramsey_policy``, only one ``planner_objective`` statement is allowed. Dynare refuses: `there can only be one planner_objective statement`.\n\n**Fix**\n\nKeep a single ``planner_objective``.",
        kind: ExplainKind::Shared,
    }),
    ("W110", ExplainEntry {
        title: "Shock correlation outside [-1, 1]",
        body: "A ``corr`` entry in the shocks block sets a correlation whose magnitude exceeds one. A correlation coefficient must lie in [-1, 1], and the implied covariance matrix would not be positive semidefinite.\n\n**Fix**\n\nSet the correlation to a value in [-1, 1].",
        kind: ExplainKind::Added,
    }),
    ("E111", ExplainEntry {
        title: "Shock variance or correlation specified more than once",
        body: "A shock's variance / standard error, or a correlation pair, is specified more than once in one shocks block. Dynare refuses: `shocks: variance or stderr of shock on e declared twice` and `shocks: covariance or correlation shock on variable pair (e, u) declared twice`.\n\n**Fix**\n\nKeep one specification per shock variance and per correlation pair in the block.",
        kind: ExplainKind::Shared,
    }),
    ("E113", ExplainEntry {
        title: "shock_paths with shocks, mshocks, endval, or controlled paths",
        body: "A ``shock_paths`` block cannot appear with ``shocks``, ``mshocks``, ``endval``, or ``perfect_foresight_controlled_paths``. Dynare refuses: `the 'shock_paths' block cannot be used in conjunction with either 'shocks', 'mshocks', 'endval' or 'perfect_foresight_controlled_paths' blocks.`.\n\n**Fix**\n\nKeep ``shock_paths`` and drop the other block, or drop ``shock_paths``.",
        kind: ExplainKind::Shared,
    }),
    ("W112", ExplainEntry {
        title: "Negative shock variance",
        body: "A shocks-block ``var e = ...`` entry sets a variance that folds to a negative constant. A variance is a squared quantity and should be non-negative.\n\n(The ``stderr`` form is not flagged: a negative standard error is squared, so the variance is still positive.)\n\n**Fix**\n\nUse a non-negative value. Recall the ``var`` form sets the variance, i.e. the standard error *squared* (e.g. ``var e = 0.01^2;``).",
        kind: ExplainKind::Added,
    }),
    ("W120", ExplainEntry {
        title: "Stochastic command with no stochastic exogenous variable",
        body: "``stoch_simul`` / ``estimation`` drive the model with stochastic shocks, but the model declares no stochastic ``varexo``. ``varexo_det`` declarations are deterministic and do not count as stochastic shocks.\n\n**Fix**\n\nDeclare at least one stochastic exogenous variable (a dummy ``varexo`` plus a shocks-block entry is enough if the model is otherwise deterministic).",
        kind: ExplainKind::Added,
    }),
    ("W121", ExplainEntry {
        title: "Parameter used with a lead or lag",
        body: "A declared parameter appears with a time subscript such as ``beta(+1)`` or ``rho(-1)`` in the model block. Dynare warns: `The following parameter(s) are used with a lead or a lag: betta`.\n\n**Fix**\n\nRemove the time index, or declare the symbol with ``var`` / ``varexo`` if it really is a variable.",
        kind: ExplainKind::Shared,
    }),
    ("W122", ExplainEntry {
        title: "Deep parameter assigned a non-finite value",
        body: "A parameter that is used in the model equations is assigned a non-finite value (``NaN`` or ``Inf``) while a run command (``steady``, ``stoch_simul``, ``perfect_foresight_*``, ``estimation``, ...) is present.\n\n**Fix**\n\nAssign a finite numeric value before the run command.",
        kind: ExplainKind::Added,
    }),
    ("E130", ExplainEntry {
        title: "Variable used before assignment in steady_state_model",
        body: "The ``steady_state_model`` block is evaluated top to bottom as a sequence of assignments, so every variable on a right-hand side must already have been assigned above. Dynare refuses: `variable 'n' is undefined in the declaration of variable 'log_n'`.\n\n**Fix**\n\nReorder the assignments so each variable is computed before it is used.",
        kind: ExplainKind::Shared,
    }),
    ("E170", ExplainEntry {
        title: "Multiple occbin_constraints blocks",
        body: "The file has more than one ``occbin_constraints`` block. Dynare refuses: `Multiple 'occbin_constraints' blocks are not allowed`.\n\n**Fix**\n\nKeep a single ``occbin_constraints`` block.",
        kind: ExplainKind::Shared,
    }),
    ("E171", ExplainEntry {
        title: "Too many constraints in occbin_constraints",
        body: "An ``occbin_constraints`` block lists more than two named constraints. Dynare refuses: `only up to two constraints are supported in 'occbin_constraints' block`.\n\n**Fix**\n\nRemove the extra constraint. OccBin supports at most two.",
        kind: ExplainKind::Shared,
    }),
    ("E172", ExplainEntry {
        title: "OccBin regime is not defined",
        body: "A model equation with ``bind`` / ``relax`` tags is missing a copy for one combination of those constraints. Dynare refuses: `for equation 'NAME', the regime corresponding to bind='…' and relax='…' is not defined`.\n\n**Fix**\n\nAdd the missing bind or relax equation for that ``name``.",
        kind: ExplainKind::Shared,
    }),
    ("E173", ExplainEntry {
        title: "bind or relax tag without a name tag",
        body: "A model equation has a ``bind`` or ``relax`` tag but no ``name`` tag. Dynare refuses: `An equation with a 'bind' or 'relax' tag must have a 'name' tag`.\n\n**Fix**\n\nAdd ``[name='…']`` on that equation.",
        kind: ExplainKind::Shared,
    }),
    ("E174", ExplainEntry {
        title: "Missing bind expression",
        body: "A named OccBin constraint has no ``bind`` inequality (a non-comparison such as ``bind i;`` counts as missing). Dynare refuses: `The 'bind' expression is missing in constraint 'NAME'`.\n\n**Fix**\n\nAdd a ``bind`` inequality on that constraint.",
        kind: ExplainKind::Shared,
    }),
    ("E175", ExplainEntry {
        title: "No equation for an OccBin constraint",
        body: "An ``occbin_constraints`` name is never mentioned in a ``bind`` or ``relax`` equation tag. Dynare refuses: `No equation has been declared for constraint 'NAME'`.\n\n**Fix**\n\nAdd a model equation tagged ``bind`` or ``relax`` with that constraint name.",
        kind: ExplainKind::Shared,
    }),
    ("E176", ExplainEntry {
        title: "Constraint listed in both bind and relax",
        body: "The same constraint name appears in both the ``bind`` and ``relax`` tags on one equation. Dynare refuses: `The constraint 'C' is both in the 'bind' and 'relax' tags`.\n\n**Fix**\n\nKeep the name in ``bind`` or ``relax``, not both.",
        kind: ExplainKind::Shared,
    }),
    ("E177", ExplainEntry {
        title: "Duplicate OccBin regime",
        body: "Two equations with the same ``name`` tag declare the same bind/relax combination. Dynare refuses: `The regime corresponding to bind='…' has already been declared for this equation`.\n\n**Fix**\n\nRemove the duplicate regime equation.",
        kind: ExplainKind::Shared,
    }),
    ("E178", ExplainEntry {
        title: "shocks(surprise) without occbin_constraints",
        body: "A ``shocks(surprise)`` block requires an ``occbin_constraints`` block. Dynare refuses: `the 'shocks(surprise)' block can only be used in conjunction with the 'occbin_constraints' block.`.\n\n**Fix**\n\nAdd ``occbin_constraints``, or drop the ``surprise`` option.",
        kind: ExplainKind::Shared,
    }),
    ("E179", ExplainEntry {
        title: "occbin_constraints with an incompatible command",
        body: "An ``occbin_constraints`` block can only be used with ``estimation``, ``stoch_simul``, and ``calib_smoother``. Dynare refuses: `the 'occbin_constraints' block is not compatible with commands other than 'estimation', 'stoch_simul', and 'calib_smoother'.`.\n\n**Fix**\n\nRemove the incompatible command, or drop ``occbin_constraints``.",
        kind: ExplainKind::Shared,
    }),
    ("E180", ExplainEntry {
        title: "mcp tag and perpendicular together",
        body: "One equation has both an ``mcp`` tag and a complementarity condition after ``⟂`` / ``_|_``. Dynare refuses: `Can't have both an 'mcp' tag and a complementarity condition after the perpendicular symbol`.\n\n**Fix**\n\nKeep one form.",
        kind: ExplainKind::Shared,
    }),
    ("E181", ExplainEntry {
        title: "bind or relax is not an inequality",
        body: "The ``bind`` or ``relax`` expression is a comparison other than ``<``, ``>``, ``<=``, or ``>=`` (for example ``==``). Dynare refuses: `The 'bind' expression must be an inequality constraint` / `The 'relax' expression must be an inequality constraint`. A non-binary ``bind i;`` is a missing bind (E174), not this code.\n\n**Fix**\n\nWrite an inequality.",
        kind: ExplainKind::Shared,
    }),
    ("E182", ExplainEntry {
        title: "Forbidden expression in occbin_constraints",
        body: "An ``occbin_constraints`` expression uses a lead/lag, a model-local, an exogenous variable, ``EXPECTATION``, ``var_expectation``, ``pac_expectation``, ``pac_target_nonstationary``, or ``SUM()``. Dynare refuses, for example: `Leads and lags on variables are forbidden in 'occbin_constraints'. Note that you can achieve the same effect by introducing an auxiliary variable in the model.`; `Model local variable z cannot be used in 'occbin_constraints'.`; `Exogenous variable e cannot be used in 'occbin_constraints'.`; `The 'expectation' operator is forbidden in 'occbin_constraints'.`; `The SUM() operator is forbidden in occbin_constraints block`. ``STEADY_STATE`` itself is allowed.\n\n**Fix**\n\nUse contemporaneous endogenous variables (and parameters). Introduce an auxiliary variable for leads, lags, or exogenous terms.",
        kind: ExplainKind::Shared,
    }),
    ("E183", ExplainEntry {
        title: "Complementarity condition has an incorrect form",
        body: "The condition after ``⟂`` / ``_|_`` is not an inequality on a contemporaneous endogenous variable with constant bounds. Dynare refuses: `Complementarity condition has an incorrect form` (an extra ``: …`` detail is appended when that form can be named).\n\n**Fix**\n\nWrite an inequality such as ``i >= 0`` or ``0 <= i <= 1``.",
        kind: ExplainKind::Shared,
    }),
    ("E184", ExplainEntry {
        title: "Duplicate clause in an OccBin constraint",
        body: "A named constraint repeats ``bind``, ``relax``, ``error_bind``, or ``error_relax``. Dynare refuses: `The '{}' clause is declared multiple times`. The last copy is what the parser keeps.\n\n**Fix**\n\nKeep a single copy of that clause.",
        kind: ExplainKind::Shared,
    }),
    ("E185", ExplainEntry {
        title: "Invalid or reused OccBin constraint name",
        body: "A ``bind`` / ``relax`` tag piece or an ``occbin_constraints`` name is not a letter-or-underscore identifier, or ``occbin_NAME_bind`` is already declared as a variable. Dynare refuses: `The string '{}' is not a valid Occbin constraint name (contains unauthorized characters)` and `The name '{}' is already used. Please use another name for Occbin constraint '{}'`.\n\n**Fix**\n\nUse letters, digits, and underscores, and pick a name that does not collide with ``occbin_NAME_bind``.",
        kind: ExplainKind::Shared,
    }),
    ("W131", ExplainEntry {
        title: "Variable silently overwritten in steady_state_model",
        body: "A variable is assigned more than once in the ``steady_state_model`` block and the later assignment does not use the earlier value. Dynare warns: `in the 'steady_state_model' block, variable 'n' is declared twice`. (An in-place transformation that reuses the value, such as the ``A = log(A)`` log-model idiom, is intentional and is not flagged.)\n\n**Fix**\n\nRemove the redundant assignment, or fold the two into one.",
        kind: ExplainKind::Shared,
    }),
    ("W140", ExplainEntry {
        title: "Nonlinear operator in a linear model",
        body: "The model is declared ``linear`` (``model(linear);``) but an equation applies an operator Dynare still accepts at check, such as ``log``, ``exp``, products, division, or powers, to a variable.\n\n**Fix**\n\nRemove the ``linear`` option, or rewrite the equation without the nonlinear operator.",
        kind: ExplainKind::Added,
    }),
    ("W150", ExplainEntry {
        title: "Deprecated command or option",
        body: "A deprecated command or option is used. Dynare warns: `The 'simul' statement is deprecated. Please use 'perfect_foresight_setup' and 'perfect_foresight_solver' instead.`; `The 'ramsey_policy' statement is deprecated. Please use 'ramsey_model', 'stoch_simul', and 'evaluate_planner_objective' instead.`; `The 'aim_solver' option is deprecated. It has been superseded by the 'dr=aim' option.`; `the 'bytecode' option is deprecated and will be removed in a future release of Dynare.`\n\n**Fix**\n\nSwitch to the modern command or option form.",
        kind: ExplainKind::Shared,
    }),
    ("W160", ExplainEntry {
        title: "Named companion file was not found",
        body: "A named option or quoted path points at a companion file that was not found next to the `.mod` or on the search paths. Missing convention files (`FILENAME_steadystate.m`, `FILENAME_prior_restrictions.m`, `run_FILENAME.m`) and a missing identifier helper are not this warning.\n\n**Fix**\n\nAdd the file next to this `.mod`, correct the path, or add its directory to the search paths.",
        kind: ExplainKind::Added,
    }),
    ("W170", ExplainEntry {
        title: "Obsolete mcp complementarity tag",
        body: "A complementarity condition is written with the ``mcp`` tag and no ``⟂`` / ``_|_`` after the equation. Dynare accepts and warns: `Specifying complementarity conditions with the 'mcp' tag is obsolete. Please consider switching to the new syntax using the perpendicular symbol.` An equation that has both forms is an Error (E180), not this Warning.\n\n**Warrant**\n\nThe editor keeps the shorter `Use ⟂ or _|_ after the equation` line instead of Dynare's longer `Please consider switching…` sentence.\n\n**Fix**\n\nWrite the condition after ``⟂`` or ``_|_`` instead of ``[mcp=…]``.",
        kind: ExplainKind::Shared,
    }),
    ("E200", ExplainEntry {
        title: "write_latex_steady_state_model without steady_state_model",
        body: "Dynare refuses: `You cannot have a write_latex_steady_state_model statement without a steady_state_model block.`\n\n**Fix**\n\nAdd a ``steady_state_model`` block, or drop ``write_latex_steady_state_model``.",
        kind: ExplainKind::Shared,
    }),
    ("E201", ExplainEntry {
        title: "No model equation with a run command",
        body: "Dynare refuses: `At least one model equation must be declared!` when the file has no non-``#`` model equation and a ``check``, perfect-foresight solver, PFEE solver, or stochastic command is present.\n\n**Fix**\n\nAdd at least one model equation, or drop the run command.",
        kind: ExplainKind::Shared,
    }),
    ("E202", ExplainEntry {
        title: "discretionary_policy with Ramsey",
        body: "Dynare refuses: `You cannot use the discretionary_policy command when you use either ramsey_model or ramsey_policy and vice versa`.\n\n**Fix**\n\nKeep either discretionary policy or Ramsey, not both.",
        kind: ExplainKind::Shared,
    }),
    ("E203", ExplainEntry {
        title: "ramsey_constraints without Ramsey",
        body: "Dynare refuses: `A ramsey_constraints block requires the presence of a ramsey_model or ramsey_policy statement`.\n\n**Fix**\n\nAdd ``ramsey_model`` or ``ramsey_policy``, or drop ``ramsey_constraints``.",
        kind: ExplainKind::Shared,
    }),
    ("E204", ExplainEntry {
        title: "osr has both optim_weights and planner_objective",
        body: "Dynare refuses: `The osr statement cannot have both optim_weights and a planner_objective; they are mutually exclusive.`\n\n**Fix**\n\nKeep either ``optim_weights`` or ``planner_objective``, not both.",
        kind: ExplainKind::Shared,
    }),
    ("E205", ExplainEntry {
        title: "Perfect-foresight and stochastic commands in the same file",
        body: "Dynare refuses: `A .mod file cannot contain both one of {perfect_foresight_solver, simul, perfect_foresight_with_expectation_errors_solver} and one of {stoch_simul, estimation, osr, ramsey_policy, discretionary_policy}. This is not possible: one cannot mix perfect foresight context with stochastic context in the same file.`\n\n**Fix**\n\nKeep either the perfect-foresight solver or the stochastic command, not both.",
        kind: ExplainKind::Shared,
    }),
    ("E206", ExplainEntry {
        title: "model use_dll with bytecode",
        body: "Dynare refuses: `In 'model' block, 'use_dll' option is not compatible with 'bytecode'`.\n\n**Fix**\n\nDrop ``use_dll`` or drop ``bytecode``.",
        kind: ExplainKind::Shared,
    }),
    ("E207", ExplainEntry {
        title: "no_static with a stochastic, steady, or check command",
        body: "Dynare refuses: `no_static option is incompatible with stoch_simul, estimation, osr, ramsey_policy, discretionary_policy, steady and check commands`.\n\n**Fix**\n\nDrop ``no_static``, or drop the incompatible command.",
        kind: ExplainKind::Shared,
    }),
    ("E208", ExplainEntry {
        title: "[static] and [dynamic] equation counts differ",
        body: "Dynare refuses: `the number of equations marked [static] must be equal to the number of equations marked [dynamic]`.\n\n**Fix**\n\nGive each ``[static]`` equation a matching ``[dynamic]`` equation, or drop the tags.",
        kind: ExplainKind::Shared,
    }),
    ("E209", ExplainEntry {
        title: "[static]/[dynamic] tags with Ramsey or discretionary policy",
        body: "Dynare refuses: `marking equations as [static] or [dynamic] is not possible with ramsey_model, ramsey_policy or discretionary_policy`.\n\n**Fix**\n\nDrop the tags, or drop the Ramsey / discretionary command.",
        kind: ExplainKind::Shared,
    }),
    ("W200", ExplainEntry {
        title: "Nonsmooth operator in a stochastic context",
        body: "Dynare warns: `you are using a function (max, min, abs, sign) or an operator (<, >, <=, >=, ==, !=) which is unsuitable for a stochastic context; see the reference manual, section about \"Expressions\", for more details.`\n\n**Fix**\n\nRewrite without those operators, or drop the stochastic command if the file is meant to be perfect foresight.",
        kind: ExplainKind::Shared,
    }),
    ("E210", ExplainEntry {
        title: "Nonsmooth operator on an endogenous in a linear model",
        body: "Dynare refuses: `you have declared your model 'linear' but you are using a function (max, min, abs, sign) or an operator (<, >, <=, >=, ==, !=) on an endogenous variable.`\n\n**Fix**\n\nDrop ``linear``, or rewrite without that operator on endogenous variables.",
        kind: ExplainKind::Shared,
    }),
    ("E211", ExplainEntry {
        title: "Nonsmooth operator on an exogenous in a linear non-PF model",
        body: "Dynare refuses: `you have declared your model 'linear' but you are using a function (max, min, abs, sign) or an operator (<, >, <=, >=, ==, !=) on an exogenous variable in a non-perfect-foresight context.`\n\n**Fix**\n\nDrop ``linear``, add a perfect-foresight solver, or rewrite without that operator on exogenous variables.",
        kind: ExplainKind::Shared,
    }),
    ("E212", ExplainEntry {
        title: "Estimated parameter used in a shock expression",
        body: "Dynare refuses: `some estimated parameters (…) also appear in the expressions defining the variance/covariance matrix of shocks; this is not allowed.`\n\n**Fix**\n\nUse a calibrated parameter in the shocks block, or drop that name from ``estimated_params``.",
        kind: ExplainKind::Shared,
    }),
    ("E213", ExplainEntry {
        title: "perfect_foresight_solver before setup",
        body: "Dynare refuses: `A 'perfect_foresight_setup' command must come before 'perfect_foresight_solver'`.\n\n**Fix**\n\nPut ``perfect_foresight_setup;`` before ``perfect_foresight_solver;``.",
        kind: ExplainKind::Shared,
    }),
    ("E214", ExplainEntry {
        title: "PFEE solver before PFEE setup",
        body: "Dynare refuses: `A 'perfect_foresight_with_expectation_errors_setup' command must come before 'perfect_foresight_with_expectation_errors_solver'`.\n\n**Fix**\n\nPut the PFEE setup command before the PFEE solver.",
        kind: ExplainKind::Shared,
    }),
    ("E215", ExplainEntry {
        title: "discretionary_policy without instruments",
        body: "Dynare refuses: `discretionary_policy: the instruments option is required.`\n\n**Fix**\n\nAdd ``instruments=(…)`` on ``discretionary_policy``.",
        kind: ExplainKind::Shared,
    }),
    ("E216", ExplainEntry {
        title: "extended_path without periods",
        body: "Dynare refuses: `the 'periods' option of 'extended_path' is mandatory`.\n\n**Fix**\n\nWrite ``extended_path(periods=…);``.",
        kind: ExplainKind::Shared,
    }),
    ("E217", ExplainEntry {
        title: "initval after endval",
        body: "Dynare refuses: `an 'initval' block cannot appear after an 'endval' block`.\n\n**Fix**\n\nMove ``initval`` before ``endval``, or drop one of the blocks.",
        kind: ExplainKind::Shared,
    }),
    ("E218", ExplainEntry {
        title: "initval/endval all_values_required is incomplete",
        body: "``initval`` or ``endval`` was opened with ``all_values_required`` but some variables have no assignment. Dynare refuses: `You have not set the following endogenous variables in initval:`; `You have not set the following exogenous variables in initval:`; `You have not set the following endogenous variables in endval:`; `You have not set the following exogenous variables in endval:`.\n\n**Fix**\n\nAssign every endogenous and exogenous in that block, or drop ``all_values_required``.",
        kind: ExplainKind::Shared,
    }),
    ("E219", ExplainEntry {
        title: "dsge_prior_weight declared with dsge_var",
        body: "``estimation`` passes the ``dsge_var`` option while ``dsge_prior_weight`` is already declared as a model variable or parameter. Dynare refuses: `dsge_prior_weight should not be declared as a model variable / parameter when the dsge_var option is passed to the estimation statement.`\n\n**Fix**\n\nRemove the ``dsge_prior_weight`` declaration; the ``dsge_var`` option declares it implicitly.",
        kind: ExplainKind::Shared,
    }),
    ("E220", ExplainEntry {
        title: "DSGE-VAR bayesian_irf shock count",
        body: "``estimation`` estimates a DSGE-VAR and passes ``bayesian_irf``, but the number of shocks differs from the number of observed variables. Dynare refuses: `When estimating a DSGE-Var and the bayesian_irf option is passed to the estimation statement, the number of shocks must equal the number of observed variables.`\n\n**Fix**\n\nAdjust the ``varexo`` or ``varobs`` lists so the two counts are equal.",
        kind: ExplainKind::Shared,
    }),
    ("E221", ExplainEntry {
        title: "DSGE-VAR fewer shocks than observed variables",
        body: "``estimation`` estimates a DSGE-VAR without ``bayesian_irf``, and the number of shocks is smaller than the number of observed variables. Dynare refuses: `When estimating a DSGE-Var, the number of shocks must be greater than or equal to the number of observed variables.`\n\n**Fix**\n\nAdd shocks (``varexo``) until the shock count is at least the ``varobs`` count.",
        kind: ExplainKind::Shared,
    }),
    ("E222", ExplainEntry {
        title: "dsge_prior_weight missing from estimated_params",
        body: "``estimation`` estimates the DSGE-VAR prior weight (bare ``dsge_var``) but ``dsge_prior_weight`` is not listed in ``estimated_params``. Dynare refuses: `When estimating a DSGE-VAR model and estimating the weight of the prior, dsge_prior_weight must be referenced in the estimated_params block.`\n\n**Fix**\n\nAdd ``dsge_prior_weight`` to ``estimated_params``, or calibrate the weight with ``dsge_var=…``.",
        kind: ExplainKind::Shared,
    }),
    ("E223", ExplainEntry {
        title: "dsge_prior_weight estimated with calibrated dsge_var",
        body: "``dsge_prior_weight`` is in ``estimated_params`` while ``estimation`` calibrates the weight with ``dsge_var=…``. Dynare refuses: `If dsge_prior_weight is in the estimated_params block, the prior weight cannot be calibrated via the dsge_var option in the estimation statement.`\n\n**Fix**\n\nKeep one form: estimate the weight with a bare ``dsge_var``, or remove ``dsge_prior_weight`` from ``estimated_params``.",
        kind: ExplainKind::Shared,
    }),
    ("E224", ExplainEntry {
        title: "dsge_prior_weight estimated without dsge_var",
        body: "``dsge_prior_weight`` is in ``estimated_params`` but no ``estimation`` statement passes ``dsge_var``. Dynare refuses: `If dsge_prior_weight is in the estimated_params block, the dsge_var option must be passed to the estimation statement.`\n\n**Fix**\n\nAdd ``dsge_var`` to the estimation statement, or remove ``dsge_prior_weight`` from ``estimated_params``.",
        kind: ExplainKind::Shared,
    }),
    ("E225", ExplainEntry {
        title: "dsge_varlag without dsge_var",
        body: "``estimation`` passes ``dsge_varlag`` without ``dsge_var``. Dynare refuses: `The estimation statement requires a dsge_var option to be passed if the dsge_varlag option is passed.`\n\n**Fix**\n\nAdd ``dsge_var`` (or ``dsge_var=…``), or drop ``dsge_varlag``.",
        kind: ExplainKind::Shared,
    }),
    ("E226", ExplainEntry {
        title: "More than one dsge_var across estimation statements",
        body: "One ``estimation`` statement estimates the DSGE-VAR prior weight (bare ``dsge_var``) and another calibrates it (``dsge_var=…``). Dynare refuses: `An estimation statement cannot take more than one dsge_var option.`\n\n**Fix**\n\nUse the same form of ``dsge_var`` everywhere: estimate the weight, or calibrate it once.",
        kind: ExplainKind::Shared,
    }),
    ("E227", ExplainEntry {
        title: "estimation without a data file",
        body: "``estimation`` has neither a ``datafile`` option nor a ``data`` statement. Dynare refuses: `The estimation statement requires a data file to be supplied via the datafile option.`\n\n**Fix**\n\nAdd ``datafile='…'``, or supply the observations with a ``data`` statement.",
        kind: ExplainKind::Shared,
    }),
    ("E228", ExplainEntry {
        title: "mode_file with estimated_params_init(use_calibration)",
        body: "``estimation`` passes ``mode_file`` while ``estimated_params_init`` was opened with ``use_calibration``. Dynare refuses: `The mode_file option of the estimation statement is incompatible with the use_calibration option of the estimated_params_init block.`\n\n**Fix**\n\nDrop the ``mode_file`` option, or drop ``use_calibration`` from the ``estimated_params_init`` block.",
        kind: ExplainKind::Shared,
    }),
    ("E229", ExplainEntry {
        title: "mh_tune_jscale with mh_jscale",
        body: "``estimation`` passes both ``mh_tune_jscale`` and ``mh_jscale``. Dynare refuses: `The mh_tune_jscale and mh_jscale options of the estimation statement are incompatible.`\n\n**Fix**\n\nKeep one of the two: tune the scale, or set it.",
        kind: ExplainKind::Shared,
    }),
    ("E230", ExplainEntry {
        title: "mh_tune_guess without mh_tune_jscale",
        body: "``estimation`` passes ``mh_tune_guess`` without ``mh_tune_jscale``. Dynare refuses: `The option mh_tune_guess in estimation statement cannot be used without option mh_tune_jscale.`\n\n**Fix**\n\nAdd ``mh_tune_jscale``, or drop ``mh_tune_guess``.",
        kind: ExplainKind::Shared,
    }),
    ("E231", ExplainEntry {
        title: "filter_algorithm=gmf with proposal_approximation=montecarlo",
        body: "``estimation`` passes ``filter_algorithm=gmf`` together with ``proposal_approximation=montecarlo``. Dynare refuses: `The filter_algorithm=gmf option is incompatible with proposal_approximation=montecarlo in the estimation statement.`\n\n**Fix**\n\nChange the filter algorithm, or the proposal approximation.",
        kind: ExplainKind::Shared,
    }),
    ("E232", ExplainEntry {
        title: "filter_algorithm=gmf with distribution_approximation=montecarlo",
        body: "``estimation`` passes ``filter_algorithm=gmf`` together with ``distribution_approximation=montecarlo``. Dynare refuses: `The filter_algorithm=gmf option is incompatible with distribution_approximation=montecarlo in the estimation statement.`\n\n**Fix**\n\nChange the filter algorithm, or the distribution approximation.",
        kind: ExplainKind::Shared,
    }),
    ("E233", ExplainEntry {
        title: "Estimated parameter in planner_discount",
        body: "A parameter listed in ``estimated_params`` appears in the ``planner_discount`` expression. Dynare refuses: `It is not possible to estimate a parameter ({name}) that appears in the discount factor of the planner (i.e. in the 'planner_discount' option).`\n\n**Fix**\n\nCalibrate that parameter, or use a discount factor that does not contain it.",
        kind: ExplainKind::Shared,
    }),
    ("E234", ExplainEntry {
        title: "prior_function or posterior_function without function",
        body: "``prior_function`` or ``posterior_function`` was given an option list without a ``function`` option. Dynare refuses: `both the 'prior_function' and 'posterior_function' commands require the 'function' option`\n\n**Fix**\n\nAdd ``function=…`` to the command, or write the command without an option list to use the default function.",
        kind: ExplainKind::Shared,
    }),
    ("E235", ExplainEntry {
        title: "discretionary_policy order greater than 1",
        body: "``discretionary_policy`` was given an order greater than 1. Dynare refuses: `discretionary_policy: order > 1 is not yet implemented`\n\n**Fix**\n\nUse ``order=1``, or drop the option.",
        kind: ExplainKind::Shared,
    }),
    ("E236", ExplainEntry {
        title: "identification order not in 1..3",
        body: "``identification`` was given an order outside 1 to 3. Dynare refuses: `the order option of identification command must be between 1 and 3`\n\n**Fix**\n\nSet ``order`` to 1, 2, or 3.",
        kind: ExplainKind::Shared,
    }),
    ("E237", ExplainEntry {
        title: "identification max_dim_cova_group is 0",
        body: "``identification`` was given ``max_dim_cova_group=0``. Dynare refuses: `The max_dim_cova_group option to identification only accepts integers > 0.`\n\n**Fix**\n\nUse a positive integer.",
        kind: ExplainKind::Shared,
    }),
    ("E238", ExplainEntry {
        title: "Multiple HP or bandpass filters",
        body: "``stoch_simul`` used more than one of ``hp_filter``, ``one_sided_hp_filter``, and ``bandpass_filter``. Dynare refuses: `stoch_simul: can only use one of HP, one-sided HP, and bandpass filters`\n\n**Fix**\n\nKeep a single filter option.",
        kind: ExplainKind::Shared,
    }),
    ("E239", ExplainEntry {
        title: "Undeclared name in a command symbol list",
        body: "A trailing symbol list names a symbol that is not declared. Every command that takes such a list is covered: ``stoch_simul``, ``estimation``, ``calib_smoother``, ``ramsey_policy``, ``discretionary_policy``, ``osr``, ``ms_irf``, ``plot_conditional_forecast``, ``forecast``, ``rplot``, ``dynasave``, ``dynatype``, ``shock_decomposition``, ``realtime_shock_decomposition``, ``initial_condition_decomposition``, ``plot_shock_decomposition``, ``squeeze_shock_decomposition``, and the ``osr_params`` statement. Dynare refuses: `{cmd}: Variable {name} was not declared.`\n\n**Fix**\n\nDeclare the symbol, or remove it from the list.",
        kind: ExplainKind::Shared,
    }),
    ("E240", ExplainEntry {
        title: "Wrong type in a command symbol list",
        body: "A trailing symbol list names a declared symbol of the wrong type. Dynare refuses with one of four lists, each naming the types that command accepts: `{cmd}: Variable {name} is not one of {endogenous}`, `{endogenous, exogenous}` (``rplot``, ``dynasave``, ``dynatype``), `{endogenous, epilogue}` (``plot_shock_decomposition``), or `{parameter}` (``osr_params``, whose sentence prints `osr: `).\n\n**Fix**\n\nUse a symbol of the type the command accepts, or remove it from the list.",
        kind: ExplainKind::Shared,
    }),
    ("W201", ExplainEntry {
        title: "restriction_fname is deprecated",
        body: "The ``restriction_fname`` option is used. Dynare accepts and warns: `restriction_fname is now deprecated, and may be removed in a future version of Dynare. Use svar_identification instead.`\n\n**Fix**\n\nUse ``svar_identification`` instead of ``restriction_fname``.",
        kind: ExplainKind::Shared,
    }),
    ("W202", ExplainEntry {
        title: "Symbol listed twice in a stoch_simul list",
        body: "``stoch_simul``'s symbol list names the same variable more than once; Dynare keeps the first occurrence. Dynare accepts and warns: `In stoch_simul: {name} found more than once in symbol list. Removing all but first occurrence.`\n\n**Fix**\n\nRemove the duplicate entry.",
        kind: ExplainKind::Shared,
    }),
    ("E241", ExplainEntry {
        title: "histval completeness",
        body: "``histval(all_values_required)`` is missing an assignment for an endogenous or exogenous variable. Dynare refuses: `You have not set the following endogenous variables in histval: {names}` / `You have not set the following exogenous variables in endval: {names}` (their exo line says ``endval``).\n\n**Fix**\n\nGive every endogenous and exogenous variable a ``histval`` assignment, or drop ``all_values_required``.",
        kind: ExplainKind::Shared,
    }),
    ("E242", ExplainEntry {
        title: "histval lag greater than zero",
        body: "A ``histval`` assignment uses a positive lag. Dynare refuses: `histval: the lag on {name} should be less than or equal to 0`.\n\n**Fix**\n\nUse a lag of 0 or less.",
        kind: ExplainKind::Shared,
    }),
    ("E243", ExplainEntry {
        title: "histval pair declared twice",
        body: "The same ``(name, lag)`` pair appears twice in one ``histval`` block. Dynare refuses: `histval: {name}({lag}) declared twice`.\n\n**Fix**\n\nKeep one assignment for that pair in the block.",
        kind: ExplainKind::Shared,
    }),
    ("E244", ExplainEntry {
        title: "Duplicate symbol in estimated_params",
        body: "A plain parameter is listed twice in one ``estimated_params`` / ``estimated_params_init`` / ``estimated_params_bounds`` block. Dynare refuses: `in `{block}' block, the symbol {name} is declared twice.`\n\n**Fix**\n\nKeep one entry for that symbol in the block.",
        kind: ExplainKind::Shared,
    }),
    ("E245", ExplainEntry {
        title: "Duplicate stderr in estimated_params",
        body: "The same ``stderr`` name is listed twice in one estimated-params block. Dynare refuses: `in `{block}' block, the stderr of {name} is declared twice.`\n\n**Fix**\n\nKeep one ``stderr`` entry for that name.",
        kind: ExplainKind::Shared,
    }),
    ("E246", ExplainEntry {
        title: "Duplicate correlation in estimated_params",
        body: "The same correlation pair is listed twice in one estimated-params block (order-insensitive). Dynare refuses: `in `{block}' block, the correlation between {a} and {b} is declared twice.`\n\n**Fix**\n\nKeep one ``corr`` entry for that pair.",
        kind: ExplainKind::Shared,
    }),
    ("E247", ExplainEntry {
        title: "Duplicate skewness in estimated_params",
        body: "The same ``skew`` name is listed twice in one estimated-params block. Dynare refuses: `in `{block}' block, the skewness of {name} is declared twice.`\n\n**Fix**\n\nKeep one ``skew`` entry for that name.",
        kind: ExplainKind::Shared,
    }),
    ("E248", ExplainEntry {
        title: "Estimated parameter value used in the same block",
        body: "A parameter declared in the same estimated-params block appears in another entry's init, bounds, or prior expressions. Dynare refuses: `in `{block}' block, the value of estimated parameter {used} is used in the declaration for {target}. This behaviour is undefined.`\n\n**Fix**\n\nDo not refer to another estimated parameter's value in the same block.",
        kind: ExplainKind::Shared,
    }),
    ("E249", ExplainEntry {
        title: "Skewness on a non-exogenous name",
        body: "``estimated_params`` lists ``skew`` on a name that is not exogenous. Dynare refuses: `in `estimated_params' block, skewness can only be specified for exogenous variables, not for '{name}'.`\n\n**Fix**\n\nUse ``skew`` only on a ``varexo`` name.",
        kind: ExplainKind::Shared,
    }),
    ("E250", ExplainEntry {
        title: "Beta prior with mean and std 0.5",
        body: "An ``estimated_params`` beta prior has mean and standard deviation both 0.5. Dynare refuses: `The prior density is not defined for the beta distribution when the mean = standard deviation = 0.5.`\n\n**Fix**\n\nChange the mean or the standard deviation so they are not both 0.5.",
        kind: ExplainKind::Shared,
    }),
    ("E251", ExplainEntry {
        title: "Exogenous variable in planner_objective",
        body: "``planner_objective`` uses an exogenous or undeclared name. Dynare refuses: `You cannot include exogenous variables (or variables of undeclared type) in the planner objective. Please define an auxiliary endogenous variable like eps_aux=epsilon and use it instead of the varexo.`\n\n**Fix**\n\nReplace the exogenous name with an auxiliary endogenous variable.",
        kind: ExplainKind::Shared,
    }),
    ("E252", ExplainEntry {
        title: "Lead or lag in planner_objective",
        body: "``planner_objective`` uses a lead or lag. Dynare refuses: `Leads and lags on variables are forbidden in 'planner_objective'.`\n\n**Fix**\n\nWrite the objective in contemporaneous variables.",
        kind: ExplainKind::Shared,
    }),
    ("E253", ExplainEntry {
        title: "Model-local in planner_objective",
        body: "``planner_objective`` uses a ``#`` model-local name. Dynare refuses: `Model local variable {name} cannot be used in 'planner_objective'.`\n\n**Fix**\n\nUse a declared endogenous variable instead of the model-local.",
        kind: ExplainKind::Shared,
    }),
    ("W203", ExplainEntry {
        title: "Several osr_params statements",
        body: "The file has more than one ``osr_params`` statement. Dynare accepts and warns: `You have more than one osr_params statement in the .mod file.`\n\n**Fix**\n\nKeep a single ``osr_params`` statement.",
        kind: ExplainKind::Shared,
    }),
    ("E254", ExplainEntry {
        title: "osr_params_bounds before osr_params",
        body: "``osr_params_bounds`` appears and there is no earlier ``osr_params`` statement. Dynare refuses: `you must have an osr_params statement before the osr_params_bounds block.`\n\n**Fix**\n\nPut ``osr_params`` above ``osr_params_bounds``.",
        kind: ExplainKind::Shared,
    }),
    ("E255", ExplainEntry {
        title: "osr_params_bounds name is not a parameter",
        body: "A name in ``osr_params_bounds`` is not a parameter. Dynare refuses: `{name} must be a parameter to be used in the osr_bounds block`.\n\n**Fix**\n\nList only declared parameters.",
        kind: ExplainKind::Shared,
    }),
    ("E256", ExplainEntry {
        title: "Equation tag used twice",
        body: "The same tag key appears twice on one equation. Dynare refuses: `Tag '{key}' cannot be used twice for the same equation`.\n\n**Fix**\n\nKeep one copy of that tag.",
        kind: ExplainKind::Shared,
    }),
    ("E257", ExplainEntry {
        title: "Default equation tag collides with existing name",
        body: "An equation has no ``name`` tag, and both the LHS identifier and the 1-based equation index are already used as ``name`` values. Dynare refuses: `Error creating default equation tag: cannot assign default tag to equation number {n} because it is already in use`.\n\n**Fix**\n\nGive the equation an explicit ``[name=…]`` that does not collide.",
        kind: ExplainKind::Shared,
    }),
    ("E258", ExplainEntry {
        title: "Several varobs statements",
        body: "The file has more than one ``varobs`` statement. Dynare refuses: `varobs: you cannot have several 'varobs' statements in the same MOD file`.\n\n**Fix**\n\nKeep a single ``varobs`` statement.",
        kind: ExplainKind::Shared,
    }),
    ("E259", ExplainEntry {
        title: "Several varexobs statements",
        body: "The file has more than one ``varexobs`` statement. Dynare refuses: `varexobs: you cannot have several 'varexobs' statements in the same MOD file`.\n\n**Fix**\n\nKeep a single ``varexobs`` statement.",
        kind: ExplainKind::Shared,
    }),
    ("E260", ExplainEntry {
        title: "varexobs name is not exogenous",
        body: "A ``varexobs`` name is not an exogenous variable. Dynare refuses: `varexobs: {name} is not an exogenous variable`.\n\n**Fix**\n\nList only ``varexo`` names, or declare the name as exogenous.",
        kind: ExplainKind::Shared,
    }),
    ("E261", ExplainEntry {
        title: "observation_trends name declared twice",
        body: "The same leading name appears twice in one ``observation_trends`` block. Dynare refuses: `observation_trends: {name} declared twice`.\n\n**Fix**\n\nKeep one trend line for that name in the block.",
        kind: ExplainKind::Shared,
    }),
    ("E262", ExplainEntry {
        title: "mcp left-hand side is not a variable",
        body: "The ``mcp`` tag's left-hand side is not a variable. Dynare refuses: `Left-hand side of expression in 'mcp' tag is not a variable`.\n\n**Fix**\n\nWrite an inequality whose left-hand side is a declared variable.",
        kind: ExplainKind::Shared,
    }),
    ("E263", ExplainEntry {
        title: "mcp left-hand side is not endogenous",
        body: "The ``mcp`` tag's left-hand side is a declared name that is not endogenous. Dynare refuses: `Left-hand side of expression in 'mcp' tag is not an endogenous variable`.\n\n**Fix**\n\nUse an endogenous variable on the left-hand side.",
        kind: ExplainKind::Shared,
    }),
    ("E264", ExplainEntry {
        title: "mcp right-hand side is not a constant",
        body: "The ``mcp`` tag's right-hand side is not a numeric constant. Dynare refuses: `Right-hand side of expression in 'mcp' tag should be a constant`.\n\n**Fix**\n\nUse a number on the right-hand side.",
        kind: ExplainKind::Shared,
    }),
    ("E265", ExplainEntry {
        title: "mcp tag has no inequality",
        body: "The ``mcp`` tag has no ``<`` or ``>``. Dynare refuses: `'mcp' tag does not contain an inequality`.\n\n**Fix**\n\nWrite an inequality such as ``y > 0``.",
        kind: ExplainKind::Shared,
    }),
    ("E266", ExplainEntry {
        title: "shocks variance on the wrong type",
        body: "``shocks`` sets a variance on a name that is neither exogenous nor observed endogenous. Dynare refuses: `shocks: setting a variance on '{name}' is not allowed, because it is neither an exogenous variable nor an observed endogenous variable`.\n\n**Fix**\n\nSet variances on ``varexo`` or ``varobs`` names.",
        kind: ExplainKind::Shared,
    }),
    ("E267", ExplainEntry {
        title: "shocks standard error on the wrong type",
        body: "``shocks`` sets a standard error on a name that is neither exogenous nor observed endogenous. Dynare refuses: `shocks: setting a standard error on '{name}' is not allowed, because it is neither an exogenous variable nor an observed endogenous variable`.\n\n**Fix**\n\nSet standard errors on ``varexo`` or ``varobs`` names.",
        kind: ExplainKind::Shared,
    }),
    ("E268", ExplainEntry {
        title: "shocks covariance on mixed types",
        body: "``shocks`` sets a covariance on a pair that is not both exogenous or both observed endogenous. Dynare refuses: `shocks: setting a covariance between '{a}' and '{b}'is not allowed; covariances can only be specified for exogenous or observed endogenous variables of same type`.\n\n**Fix**\n\nUse two exogenous names or two observed endogenous names.",
        kind: ExplainKind::Shared,
    }),
    ("E269", ExplainEntry {
        title: "shocks correlation on mixed types",
        body: "``shocks`` sets a correlation on a pair that is not both exogenous or both observed endogenous. Dynare refuses: `shocks: setting a correlation between '{a}' and '{b}'is not allowed; correlations can only be specified for exogenous or observed endogenous variables of same type`.\n\n**Fix**\n\nUse two exogenous names or two observed endogenous names.",
        kind: ExplainKind::Shared,
    }),
    ("E270", ExplainEntry {
        title: "shocks skewness on a non-exogenous name",
        body: "``shocks`` sets skewness on a name that is not exogenous. Dynare refuses: `shocks: setting skewness for '{a}', '{b}', '{c}' is not allowed; skewness can only be specified for exogenous variables`.\n\n**Fix**\n\nUse ``skew`` only on ``varexo`` names.",
        kind: ExplainKind::Shared,
    }),
    ("E271", ExplainEntry {
        title: "Option declared twice in one list",
        body: "The same option identifier appears twice in one ``(…)`` list. Dynare refuses: `option {name} declared twice`.\n\n**Fix**\n\nKeep one copy of that option in the list.",
        kind: ExplainKind::Shared,
    }),
    ("E272", ExplainEntry {
        title: "static equation with dynamics",
        body: "An equation tagged ``[static]`` contains a lead, lag, ``EXPECTATION``, ``diff``, or ``STEADY_STATE``. Dynare refuses: `An equation tagged [static] cannot contain leads, lags, expectations, diff or STEADY_STATE operators`.\n\n**Fix**\n\nRemove the dynamics, or drop the ``[static]`` tag.",
        kind: ExplainKind::Shared,
    }),
    ("E273", ExplainEntry {
        title: "generate_irfs element name repeated",
        body: "A ``generate_irfs`` element name is used more than once in one block. Dynare refuses: `Names in the generate_irfs block must be unique but you entered '{name}' more than once.`\n\n**Fix**\n\nGive each element a distinct name in the block.",
        kind: ExplainKind::Shared,
    }),
    ("E274", ExplainEntry {
        title: "generate_irfs exogenous set twice",
        body: "The same exogenous name is set twice inside one ``generate_irfs`` element. Dynare refuses: `You have set the exogenous variable {name} twice.`\n\n**Fix**\n\nKeep one setting for that exogenous name.",
        kind: ExplainKind::Shared,
    }),
    ("E275", ExplainEntry {
        title: "Namespace-qualified symbol",
        body: "An expression uses ``ident.ident`` outside a skipped ``shock_paths`` body. Dynare refuses: `Namespace-qualified symbol {ns}.{name} not allowed in this context`.\n\n**Fix**\n\nUse a declared symbol without a namespace prefix.",
        kind: ExplainKind::Shared,
    }),
    ("E276", ExplainEntry {
        title: "log of interned numeric zero",
        body: "``log`` or ``ln`` is applied to interned numeric 0 while the expression is built. Dynare refuses: `log(0) not defined!`.\n\n**Fix**\n\nDo not take the log of a literal zero.",
        kind: ExplainKind::Shared,
    }),
    ("E277", ExplainEntry {
        title: "log10 of interned numeric zero",
        body: "``log10`` is applied to interned numeric 0 while the expression is built. Dynare refuses: `log10(0) not defined!`.\n\n**Fix**\n\nDo not take log10 of a literal zero.",
        kind: ExplainKind::Shared,
    }),
    ("E278", ExplainEntry {
        title: "Division by interned numeric zero",
        body: "A division denominator folds to interned numeric 0 while the expression is built. Dynare refuses: `Division by zero when forming ({num})/({den}); denominator simplified to 0 (possibly after substituting a variable set to 0).`\n\n**Fix**\n\nChange the denominator so it is not zero.",
        kind: ExplainKind::Shared,
    }),
    ("E279", ExplainEntry {
        title: "external_function name used as a variable outside model",
        body: "A name declared with ``external_function(name=…)`` is used as a bare variable outside ``model``. Dynare refuses: `Symbol '{name}' is the name of a MATLAB/Octave function, and cannot be used as a variable.`\n\n**Fix**\n\nCall the function with arguments, or use a different name.",
        kind: ExplainKind::Shared,
    }),
    ("E280", ExplainEntry {
        title: "external_function name used as a variable inside model",
        body: "A name declared with ``external_function(name=…)`` is used as a bare variable inside ``model``. Dynare refuses: `Symbol {name} is a function name external to Dynare. It cannot be used like a variable without input argument inside model.`\n\n**Fix**\n\nCall the function with arguments, or use a different name.",
        kind: ExplainKind::Shared,
    }),
    ("E281", ExplainEntry {
        title: "Mod-file local used inside model",
        body: "A name auto-declared outside ``model`` is used inside ``model``. Dynare refuses: `Variable {name} not allowed inside model declaration. Its scope is only outside model.`\n\n**Fix**\n\nDeclare the name as ``var``, ``varexo``, or ``parameters`` if it belongs in the model.",
        kind: ExplainKind::Shared,
    }),
    ("E282", ExplainEntry {
        title: "Model-local used outside model",
        body: "A ``#`` model-local name is used outside ``model`` (not in initval/endval/histval). Dynare refuses: `Variable {name} not allowed outside model declaration. Its scope is only inside model.`\n\n**Fix**\n\nKeep the name inside ``model``, or declare it as a parameter.",
        kind: ExplainKind::Shared,
    }),
    ("E283", ExplainEntry {
        title: "@#if condition is not bool or number",
        body: "An ``@#if`` condition is not a boolean or a number. Dynare refuses: `The condition must evaluate to a boolean or a double`.\n\n**Fix**\n\nUse a boolean or numeric condition.",
        kind: ExplainKind::Shared,
    }),
    ("E284", ExplainEntry {
        title: "@#for tuple arity mismatch",
        body: "An ``@#for`` tuple has a different size from the index list. Dynare refuses: `Encountered tuple of size {n} but only have {m} index variables`.\n\n**Fix**\n\nMatch the number of index names to the tuple size.",
        kind: ExplainKind::Shared,
    }),
    ("E285", ExplainEntry {
        title: "Macro + operand type mismatch",
        body: "A macro ``+`` combines operands of incompatible types. Dynare refuses: `Type mismatch for operands of + operator`.\n\n**Fix**\n\nAdd numbers to numbers, or change the operands.",
        kind: ExplainKind::Shared,
    }),
    ("E286", ExplainEntry {
        title: "with_epilogue without an epilogue block",
        body: "A ``shock_decomposition``, ``realtime_shock_decomposition``, or ``initial_condition_decomposition`` statement has the ``with_epilogue`` option, but the file has no ``epilogue`` block. Dynare refuses: `the 'with_epilogue' option cannot be specified when there is no 'epilogue' block`.\n\n**Fix**\n\nAdd an ``epilogue;`` … ``end;`` block, or drop the ``with_epilogue`` option.",
        kind: ExplainKind::Shared,
    }),
    ("E287", ExplainEntry {
        title: "Duplicate name in the epilogue block",
        body: "The ``epilogue`` block assigns the same name twice. Dynare refuses: `in the 'epilogue' block, variable 'foo' is declared twice`.\n\n**Fix**\n\nKeep one assignment per name.",
        kind: ExplainKind::Shared,
    }),
    ("E288", ExplainEntry {
        title: "Undeclared name in the epilogue block",
        body: "An expression in the ``epilogue`` block uses a name that is not declared before it. Dynare refuses: `Variable bar used in the epilogue block but was not declared.`\n\n**Fix**\n\nDeclare the symbol, or correct the name.",
        kind: ExplainKind::Shared,
    }),
    ("E289", ExplainEntry {
        title: "Exogenous variable in the epilogue block",
        body: "The ``epilogue`` block uses an exogenous variable. Dynare refuses: `Symbol 'e' cannot be used inside the epilogue block, because it is an exogenous variable.`\n\n**Fix**\n\nUse endogenous variables and epilogue names there.",
        kind: ExplainKind::Shared,
    }),
    ("E290", ExplainEntry {
        title: "Deterministic exogenous variable in the epilogue block",
        body: "The ``epilogue`` block uses a ``varexo_det`` variable. Dynare refuses: `Symbol 'ed' cannot be used inside the epilogue block, because it is an exogenous deterministic variable.`\n\n**Fix**\n\nUse endogenous variables and epilogue names there.",
        kind: ExplainKind::Shared,
    }),
    ("E291", ExplainEntry {
        title: "EXPECTATION in the epilogue block",
        body: "The ``epilogue`` block uses the ``EXPECTATION`` operator. Dynare refuses: `The 'expectation' operator is forbidden in 'epilogue'.`\n\n**Fix**\n\nRemove the operator from the epilogue expression.",
        kind: ExplainKind::Shared,
    }),
    ("E292", ExplainEntry {
        title: "STEADY_STATE in the epilogue block",
        body: "The ``epilogue`` block uses the ``STEADY_STATE`` operator. Dynare refuses: `The STEADY_STATE() operator is forbidden in epilogue block`.\n\n**Fix**\n\nRemove the operator from the epilogue expression.",
        kind: ExplainKind::Shared,
    }),
    ("E293", ExplainEntry {
        title: "SUM in the epilogue block",
        body: "The ``epilogue`` block uses the ``SUM`` operator. Dynare refuses: `The SUM() operator is forbidden in epilogue block`.\n\n**Fix**\n\nRemove the operator from the epilogue expression.",
        kind: ExplainKind::Shared,
    }),
    ("E294", ExplainEntry {
        title: "Epilogue name outside the epilogue block",
        body: "A name declared in the ``epilogue`` block is used in the model or in another statement. Dynare refuses: `Symbol 'foo' cannot be used outside the epilogue block.`\n\n**Fix**\n\nUse the name only inside the ``epilogue`` block.",
        kind: ExplainKind::Shared,
    }),
    ("E295", ExplainEntry {
        title: "Unknown name in change_type",
        body: "A ``change_type`` statement lists a name that is not declared before it. Dynare refuses: `Unknown variable zzz`.\n\n**Fix**\n\nDeclare the symbol, or correct the name.",
        kind: ExplainKind::Shared,
    }),
    ("E296", ExplainEntry {
        title: "change_type after the symbol was used",
        body: "A ``change_type`` statement changes a symbol that an earlier expression already used. Dynare refuses: `You cannot modify the type of symbol y after having used it in an expression`.\n\n**Fix**\n\nMove the ``change_type`` statement above the first use.",
        kind: ExplainKind::Shared,
    }),
    ("E297", ExplainEntry {
        title: "Several ramsey_model statements",
        body: "The file has more than one ``ramsey_model`` statement. Dynare refuses: `Several 'ramsey_model' statements cannot appear in a given .mod file.`\n\n**Fix**\n\nKeep a single ``ramsey_model`` statement.",
        kind: ExplainKind::Shared,
    }),
    ("E298", ExplainEntry {
        title: "ramsey_model after ramsey_policy",
        body: "A ``ramsey_model`` statement follows a ``ramsey_policy`` statement. Dynare refuses: `A 'ramsey_model' statement cannot follow a 'ramsey_policy' statement.`\n\n**Fix**\n\nKeep one of the two, in the intended order.",
        kind: ExplainKind::Shared,
    }),
    ("E299", ExplainEntry {
        title: "ramsey_policy after ramsey_model",
        body: "A ``ramsey_policy`` statement follows a ``ramsey_model`` statement. Dynare refuses: `A 'ramsey_policy' statement cannot follow a 'ramsey_model' statement.`\n\n**Fix**\n\nKeep one of the two, in the intended order.",
        kind: ExplainKind::Shared,
    }),
    ("E300", ExplainEntry {
        title: "Several ramsey_policy statements",
        body: "The file has more than one ``ramsey_policy`` statement. Dynare refuses: `Several 'ramsey_policy' statements cannot appear in a given .mod file.`\n\n**Fix**\n\nKeep a single ``ramsey_policy`` statement.",
        kind: ExplainKind::Shared,
    }),
    ("E301", ExplainEntry {
        title: "planner_discount with a declared optimal_policy_discount_factor",
        body: "``ramsey_model(planner_discount=…)`` is used while ``optimal_policy_discount_factor`` is already declared as a parameter. Dynare refuses: `ramsey_model: the 'planner_discount' option cannot be used when the 'optimal_policy_discount_factor' parameter is explicitly declared.`\n\n**Fix**\n\nDrop the ``planner_discount`` option, or remove the parameter declaration.",
        kind: ExplainKind::Shared,
    }),
    ("E302", ExplainEntry {
        title: "planner_discount on ramsey_policy with a declared optimal_policy_discount_factor",
        body: "``ramsey_policy(planner_discount=…)`` is used while ``optimal_policy_discount_factor`` is already declared as a parameter. Dynare refuses: `ramsey_policy: the 'planner_discount' option cannot be used when the 'optimal_policy_discount_factor' parameter is explicitly declared.`\n\n**Fix**\n\nDrop the ``planner_discount`` option, or remove the parameter declaration.",
        kind: ExplainKind::Shared,
    }),
    ("E303", ExplainEntry {
        title: "dsge_prior_weight declared as a parameter",
        body: "The ``parameters`` declaration lists ``dsge_prior_weight``. Dynare refuses: `dsge_prior_weight cannot be declared as a parameter. Use the dsge_var option in the estimation statement instead.`\n\n**Fix**\n\nRemove it from ``parameters`` and pass ``dsge_var`` to the ``estimation`` statement.",
        kind: ExplainKind::Shared,
    }),
    ("E304", ExplainEntry {
        title: "@#includepath is not a directory",
        body: "The ``@#includepath`` argument does not resolve to an existing directory (a relative path resolves against the file that contains the directive). Dynare refuses: `missing_dir does not evaluate to a valid directory`.\n\n**Warrant**\n\nDynare prints that line inside a `Macro-processing error: backtrace…` block that also repeats the directive and its location; the editor reports the single line.\n\n**Fix**\n\nPoint the directive at an existing directory, or create it.",
        kind: ExplainKind::Shared,
    }),
    ("E305", ExplainEntry {
        title: "@#includepath argument is not a string",
        body: "The ``@#includepath`` argument does not evaluate to a string. Dynare refuses: `File name does not evaluate to a string`.\n\n**Fix**\n\nQuote the directory: ``@#includepath \"mydir\"``.",
        kind: ExplainKind::Shared,
    }),
    ("E306", ExplainEntry {
        title: "load_params_and_steady_state cannot open its file",
        body: "The ``load_params_and_steady_state`` file is not found next to the ``.mod``. Dynare refuses: `Can't open nope.txt`.\n\n**Fix**\n\nPut the file next to the ``.mod``, or correct its name.",
        kind: ExplainKind::Shared,
    }),
    ("E307", ExplainEntry {
        title: "Trend variable declared twice",
        body: "A ``trend_var`` or ``log_trend_var`` name is declared twice. Dynare refuses: `Trend variable A was declared twice.`\n\n**Fix**\n\nDeclare each trend variable once.",
        kind: ExplainKind::Shared,
    }),
    ("E308", ExplainEntry {
        title: "Endogenous variable listed twice as following a trend",
        body: "A ``var(deflator=…)`` list holds the same endogenous variable twice (also across statements). Dynare refuses: `Variable y was listed more than once as following a trend.`\n\n**Fix**\n\nList the variable once.",
        kind: ExplainKind::Shared,
    }),
    ("E309", ExplainEntry {
        title: "Deflator contains a non-stationary endogenous variable",
        body: "The deflator expression of a ``var(deflator=…)`` statement uses a variable that is itself declared with a deflator. Dynare refuses: `The deflator contains a non-stationary endogenous variable. This is not allowed. Please use only stationary endogenous and/or {log_}trend_vars.`\n\n**Fix**\n\nUse a stationary variable or a trend variable as the deflator.",
        kind: ExplainKind::Shared,
    }),
    ("E310", ExplainEntry {
        title: "Trend variable used outside the model",
        body: "A ``trend_var`` / ``log_trend_var`` name appears outside the ``model`` block (for example in a parameter assignment). Dynare refuses: `Variable A not allowed outside model declaration, because it is a trend variable.`\n\n**Fix**\n\nUse the trend variable in the ``model`` block or as a deflator.",
        kind: ExplainKind::Shared,
    }),
    ("E311", ExplainEntry {
        title: "filter_initial_state name is not endogenous or exogenous",
        body: "A ``filter_initial_state`` entry names a symbol that is neither endogenous nor exogenous (for example a parameter). Dynare refuses: `filter_initial_state: rho should be an endogenous or exogenous variable`.\n\n**Fix**\n\nList an endogenous or exogenous variable.",
        kind: ExplainKind::Shared,
    }),
    ("E312", ExplainEntry {
        title: "filter_initial_state exogenous variable without a lag",
        body: "A ``filter_initial_state`` entry gives an exogenous variable the lag 0. Dynare refuses: `filter_initial_state: exogenous variable e must be provided with a lag`.\n\n**Fix**\n\nGive the exogenous variable a negative lag.",
        kind: ExplainKind::Shared,
    }),
    ("E313", ExplainEntry {
        title: "filter_initial_state entry declared twice",
        body: "The same ``(name, lag)`` pair appears twice in one ``filter_initial_state`` block. Dynare refuses: `filter_initial_state: (y, 0) declared twice`.\n\n**Fix**\n\nKeep one assignment per ``(name, lag)`` pair in the block.",
        kind: ExplainKind::Shared,
    }),
    ("E314", ExplainEntry {
        title: "filter_initial_state lag not used by the model",
        body: "A ``filter_initial_state`` entry uses a lag that the model does not carry: the entry's ``lag`` stands for the model lag ``lag - 1``. Dynare refuses: `filter_initial_state: variable y does not appear in the model with the lag -3 (see the reference manual for the timing convention in 'filter_initial_state')`.\n\n**Fix**\n\nMatch an entry's lag to the model: for ``y(-3)`` in the model, write ``y(-2)`` here.",
        kind: ExplainKind::Shared,
    }),
    ("E315", ExplainEntry {
        title: "optim_weights variable declared twice",
        body: "One ``optim_weights`` block gives the same variable two weights. Dynare refuses: `optim_weights: y declared twice`.\n\n**Fix**\n\nKeep one weight per variable in the block.",
        kind: ExplainKind::Shared,
    }),
    ("E316", ExplainEntry {
        title: "optim_weights pair declared twice",
        body: "One ``optim_weights`` block gives the same pair of variables two weights. Dynare refuses: `optim_weights: pair of variables (y, z) declared twice`.\n\n**Fix**\n\nKeep one weight per pair in the block (the order of the two names matters).",
        kind: ExplainKind::Shared,
    }),
    ("E317", ExplainEntry {
        title: "Name is not endogenous, or is an exogenous deterministic",
        body: "A name that must be endogenous is not. Dynare refuses `N is not endogenous.`, and a ``varexo_det`` name on a ``std(…)`` / ``corr(…)`` prior head or in the shock slot of a ``matched_irfs`` / ``matched_irfs_weights`` row gets their other sentence, `N is an exogenous deterministic.` The surfaces are the ``optim_weights`` weights, a ``conditional_forecast_paths`` `var` row, a prior head, both names of a ``moment_calibration`` row, the endogenous of an ``irf_calibration`` row, and the endogenous slot of a ``matched_irfs`` / ``matched_irfs_weights`` row.\n\n**Fix**\n\nName an endogenous variable.",
        kind: ExplainKind::Shared,
    }),
    ("E318", ExplainEntry {
        title: "Two ramsey_constraints for one variable",
        body: "``ramsey_constraints`` constrains the same endogenous variable twice (also across blocks). Dynare refuses: `The ramsey_constraints block contains two constraints for variable y`.\n\n**Fix**\n\nMerge the bounds into one constraint, such as ``0 < y < 1``.",
        kind: ExplainKind::Shared,
    }),
    ("E319", ExplainEntry {
        title: "Ramsey constraint is not an inequality",
        body: "A ``ramsey_constraints`` entry is not an inequality. Dynare refuses: `Ramsey constraint has an incorrect form: This expression is not an inequality`.\n\n**Fix**\n\nWrite ``y > bound``, ``y < bound``, or ``lower < y < upper``.",
        kind: ExplainKind::Shared,
    }),
    ("E320", ExplainEntry {
        title: "Ramsey constraint bound is not constant",
        body: "A ``ramsey_constraints`` bound holds an endogenous or exogenous variable. Dynare refuses: `Ramsey constraint has an incorrect form: Bounds must not contain any endogenous or exogenous variable`.\n\n**Fix**\n\nUse constants (parameters are allowed) for the bounds.",
        kind: ExplainKind::Shared,
    }),
    ("E321", ExplainEntry {
        title: "Ramsey constraint does not match the complementarity form",
        body: "A ``ramsey_constraints`` entry is an inequality but not one of the accepted shapes, and Dynare reports it without a detail. Dynare refuses: `Ramsey constraint has an incorrect form:`.\n\n**Fix**\n\nWrite ``y > bound``, ``y < bound``, or ``lower < y < upper`` around a single contemporaneous endogenous variable.",
        kind: ExplainKind::Shared,
    }),
    ("E322", ExplainEntry {
        title: "external_function without a name option",
        body: "An ``external_function`` statement has no ``name`` option. Dynare refuses: `The 'name' option must be passed to external_function().`\n\n**Fix**\n\nAdd ``name='myfunc'``.",
        kind: ExplainKind::Shared,
    }),
    ("E323", ExplainEntry {
        title: "external_function name option without an argument",
        body: "The ``name`` option of ``external_function`` is empty. Dynare refuses: `An argument must be passed to the 'name' option of the external_function() statement.`\n\n**Fix**\n\nPass the function name: ``name='myfunc'``.",
        kind: ExplainKind::Shared,
    }),
    ("E324", ExplainEntry {
        title: "external_function second derivative without the first",
        body: "``second_deriv_provided=…`` names a derivative function while the statement does not provide a first derivative. Dynare refuses: `If the second derivative is provided to the external_function command, the first derivative must also be provided.`\n\n**Fix**\n\nAdd ``first_deriv_provided=…``, or drop the second derivative.",
        kind: ExplainKind::Shared,
    }),
    ("E325", ExplainEntry {
        title: "external_function bare second derivative without a bare first",
        body: "A bare ``second_deriv_provided`` asks the top-level function for the second derivative while the first derivative comes from somewhere else. Dynare refuses: `If the second derivative is provided in the top-level function, the first derivative must also be provided in that function.`\n\n**Fix**\n\nAdd a bare ``first_deriv_provided``, or name the second-derivative function.",
        kind: ExplainKind::Shared,
    }),
    ("E326", ExplainEntry {
        title: "external_function nargs differs from an earlier statement",
        body: "A second ``external_function`` statement for the same function passes a different number of arguments. Dynare refuses: `The number of arguments passed to the external_function() statement do not match the number of arguments passed to a previous call or declaration of the top-level function.`\n\n**Fix**\n\nGive the statements the same ``nargs``.",
        kind: ExplainKind::Shared,
    }),
    ("E327", ExplainEntry {
        title: "external_function first derivative differs from an earlier statement",
        body: "A second ``external_function`` statement for the same function names a different first-derivative function. Dynare refuses: `The first derivative function passed to the external_function() statement does not match the first derivative function passed to a previous call or declaration of the top-level function.`\n\n**Fix**\n\nRepeat the earlier ``first_deriv_provided`` form.",
        kind: ExplainKind::Shared,
    }),
    ("E328", ExplainEntry {
        title: "external_function second derivative from another function",
        body: "The first derivative comes from the top-level function while ``second_deriv_provided`` names a different external function. Dynare refuses: `If the first derivative is provided by the top-level function, the second derivative cannot be provided by any other external function.`\n\n**Fix**\n\nProvide the second derivative in the same function (a bare ``second_deriv_provided``), or name a first-derivative function.",
        kind: ExplainKind::Shared,
    }),
    ("E329", ExplainEntry {
        title: "init2shocks duplicate endogenous variable",
        body: "An ``init2shocks`` block pairs the same endogenous variable twice. Dynare refuses: `Init2shocks(default): enogenous variable 'y' appears more than once in the init2shocks statement` (their spelling).\n\n**Fix**\n\nKeep one pair per endogenous variable.",
        kind: ExplainKind::Shared,
    }),
    ("E330", ExplainEntry {
        title: "init2shocks first name is not endogenous",
        body: "The first name of an ``init2shocks`` pair is not an endogenous variable. Dynare refuses: `init2shocks: rho should be an endogenous variable`.\n\n**Fix**\n\nPut an endogenous variable first.",
        kind: ExplainKind::Shared,
    }),
    ("E331", ExplainEntry {
        title: "init2shocks second name is not exogenous",
        body: "The second name of an ``init2shocks`` pair is not a ``varexo`` variable; ``varexo_det`` does not count here. Dynare refuses: `init2shocks: rho should be an exogenous variable`.\n\n**Fix**\n\nPut a ``varexo`` variable second.",
        kind: ExplainKind::Shared,
    }),
    ("E332", ExplainEntry {
        title: "homotopy_setup name is not a parameter or exogenous variable",
        body: "A ``homotopy_setup`` row names a symbol that is neither a parameter, a ``varexo``, nor a ``varexo_det`` variable. Dynare refuses: `homotopy_val: y should be a parameter or exogenous variable`.\n\n**Fix**\n\nUse a parameter or an exogenous variable.",
        kind: ExplainKind::Shared,
    }),
    ("E333", ExplainEntry {
        title: "shock_groups member is not exogenous",
        body: "A ``shock_groups`` member is not a ``varexo`` variable; ``varexo_det`` does not count here. Dynare refuses: `shock_groups: rho should be an exogenous variable`.\n\n**Fix**\n\nList ``varexo`` variables.",
        kind: ExplainKind::Shared,
    }),
    ("E334", ExplainEntry {
        title: "external_function Jacobian and Hessian from the same non-top-level function",
        body: "The Jacobian and the Hessian are provided by the same external function, but that function is not the statement's top-level function. Dynare refuses: `If the Jacobian and Hessian are provided by the same function, that function must be the top-level function.`\n\n**Fix**\n\nProvide both derivatives in the top-level function (bare ``first_deriv_provided`` / ``second_deriv_provided``), or name two different external functions.",
        kind: ExplainKind::Shared,
    }),
    ("W204", ExplainEntry {
        title: "Unknown symbol in a load_params_and_steady_state file",
        body: "The data file of ``load_params_and_steady_state`` holds a name that is not declared at that point in the ``.mod``. Dynare accepts and warns: `Unknown symbol zzz in w204_params.txt`. A name that *is* declared but sits in an unsupported slot — an ``epilogue`` helper, an ``external_function`` name, a trend variable — errors **E380** instead; this warning keeps only the genuinely unknown name.\n\n**Fix**\n\nRemove the entry from the data file, or declare the symbol above the ``load_params_and_steady_state`` statement.",
        kind: ExplainKind::Shared,
    }),
    ("W205", ExplainEntry {
        title: "shock_groups label reused",
        body: "Two rows of the same ``shock_groups`` block reuse a label. The comparison is within one block only: each block is its own statement, so two separate ``shock_groups`` blocks may share a label silently. Dynare accepts the file and warns only when writing the MATLAB files, once per row that has a later twin: `shock group label 'g1' has been reused. Only using the last definition.`\n\n**Fix**\n\nGive each row of the block its own label.",
        kind: ExplainKind::Shared,
    }),
    ("E335", ExplainEntry {
        title: "Equation-surgery tag matched no equation",
        body: "A ``model_remove`` / ``model_replace`` tag set names no equation of the model. Dynare refuses: `model_remove/model_replace/exclude_eqs/include_eqs: The equations specified by <tag list> were not found.`\n\n**Fix**\n\nCheck the tag value against the ``[name='...']`` tags of the model block, or drop the tag.",
        kind: ExplainKind::Shared,
    }),
    ("E336", ExplainEntry {
        title: "Excluded equation has no single endogenous on its left side",
        body: "``model_remove`` removed an equation that carries no ``endogenous`` tag and whose left side is not one endogenous variable. Dynare refuses: `Equation N has been excluded but it does not have a single variable on its left-hand side or an `endogenous` tag`. ``model_replace`` does not gate on this.\n\n**Fix**\n\nName the variable with an ``endogenous='name'`` tag, or put a single endogenous variable on the left side.",
        kind: ExplainKind::Shared,
    }),
    ("E337", ExplainEntry {
        title: "Same endogenous excluded twice by one statement",
        body: "One ``model_remove`` / ``model_replace`` statement excluded two equations that name the same endogenous. Dynare refuses: `Variable c was excluded twice via a model_remove or model_replace statement, or via the include_eqs or exclude_eqs option`.\n\n**Warrant**\n\nDynare looks the printed name up at the loop index, so it can name a symbol that was not excluded at all; the editor names the variable that was excluded twice.\n\n**Fix**\n\nRemove the equation once, or drop one of the tags.",
        kind: ExplainKind::Shared,
    }),
    ("E338", ExplainEntry {
        title: "data statement without file or series",
        body: "The ``data`` statement carries neither ``file`` nor ``series``. Dynare refuses: `The file or series option must be passed to the data statement.`

**Fix**

Pass exactly one of the two: ``data(file='x.csv');`` or ``data(series=y);``.",
        kind: ExplainKind::Shared,
    }),
    ("E339", ExplainEntry {
        title: "data statement with both file and series",
        body: "The ``data`` statement carries both ``file`` and ``series``. Dynare refuses: `The file and series options cannot be used simultaneously in the data statement.`

**Fix**

Keep one of the two.",
        kind: ExplainKind::Shared,
    }),
    ("E340", ExplainEntry {
        title: "data statement nobs is not positive",
        body: "The ``nobs`` option of the ``data`` statement is ``0``. Dynare refuses: `The nobs option of the data statement only accepts positive integers.` A negative value is a syntax error before this check runs.

**Fix**

Pass a positive number of observations, or drop ``nobs``.",
        kind: ExplainKind::Shared,
    }),
    ("E341", ExplainEntry {
        title: "ms_estimation without no_create_init, datafile and initial_year",
        body: "``ms_estimation`` was written without ``no_create_init`` and without one of ``datafile`` / ``initial_year``. Dynare refuses: `If you do not pass no_create_init to ms_estimation, you must pass the datafile and initial_year options.` One sentence covers every missing combination.

**Fix**

Add ``no_create_init``, or pass both ``datafile`` and ``initial_year``.",
        kind: ExplainKind::Shared,
    }),
    ("E342", ExplainEntry {
        title: "conditional_forecast without parameter_set",
        body: "``conditional_forecast`` was written without the ``parameter_set`` option. Dynare refuses: ``You must pass the `parameter_set` option to conditional_forecast``. The option takes one of ``prior_mode``, ``prior_mean``, ``posterior_mean``, ``posterior_mode``, ``posterior_median``, ``mle_mode`` or ``calibration``.

**Fix**

Pass ``parameter_set=calibration`` (or the prior or posterior set you intend).",
        kind: ExplainKind::Shared,
    }),
    ("E343", ExplainEntry {
        title: "conditional_forecast_paths periods and values counts differ",
        body: "A ``var`` row of ``conditional_forecast_paths`` lists a different number of ``periods`` and ``values`` entries. Dynare refuses: `shocks/conditional_forecast_paths: variable Pie: number of periods is different from number of shock values`. A range such as ``1:4`` counts as one entry.

**Fix**

Give one value per period entry.",
        kind: ExplainKind::Shared,
    }),
    ("E344", ExplainEntry {
        title: "conditional_forecast_paths variable declared twice",
        body: "One ``var`` name appears twice in a single ``conditional_forecast_paths`` block. Dynare refuses: `shocks/conditional_forecast_paths: variable Pie declared twice`.

**Fix**

Merge the two rows into one, or remove the duplicate.",
        kind: ExplainKind::Shared,
    }),
    ("E345", ExplainEntry {
        title: "markov_switching statement missing a required option",
        body: "``markov_switching`` was written without one of ``chain``, ``number_of_regimes`` or ``duration``. Dynare refuses: `A 'chain' option must be passed to the 'markov_switching' statement.` with the missing option named.

**Fix**

Pass the named option.",
        kind: ExplainKind::Shared,
    }),
    ("E346", ExplainEntry {
        title: "markov_switching chain is zero",
        body: "The ``chain`` option of ``markov_switching`` is ``0``. Dynare refuses: `The value passed to the chain option must be greater than zero.` A negative value is a syntax error before this check runs.

**Fix**

Chains are numbered from 1, in file order.",
        kind: ExplainKind::Shared,
    }),
    ("E347", ExplainEntry {
        title: "markov_switching number_of_regimes is zero",
        body: "The ``number_of_regimes`` option of ``markov_switching`` is ``0``. Dynare refuses: `The value passed to the number_of_regimes option must be greater than zero.` A negative value is a syntax error before this check runs.

**Fix**

Pass a positive number of regimes.",
        kind: ExplainKind::Shared,
    }),
    ("E348", ExplainEntry {
        title: "markov_switching chain is not the next consecutive integer",
        body: "The ``chain`` options of the file's ``markov_switching`` statements must be ``1``, ``2``, ``3``, … in file order. Dynare refuses: `The markov_switching chain option takes consecutive integers beginning at 1.`

**Fix**

Number the statements from 1 with no gaps, in the order they appear.",
        kind: ExplainKind::Shared,
    }),
    ("E349", ExplainEntry {
        title: "markov_switching parameters names are not parameters",
        body: "A name in the ``parameters=[…]`` option of ``markov_switching`` is not a declared parameter. Dynare refuses: `Variables passed to the parameters option of the markov_switching statement must be parameters. Caused by: Y`.

**Fix**

List only ``parameters`` names, or declare the name as a parameter.",
        kind: ExplainKind::Shared,
    }),
    ("E350", ExplainEntry {
        title: "markov_switching restrictions row is not three entries",
        body: "A row of the ``restrictions`` option of ``markov_switching`` does not hold exactly three entries. Dynare refuses: `restrictions in the subsample statement must be specified in the form [current_period_regime, next_period_regime, transition_probability]`.

**Fix**

Write each row as ``[from_regime, to_regime, probability]``.",
        kind: ExplainKind::Shared,
    }),
    ("E351", ExplainEntry {
        title: "markov_switching restriction regime above number_of_regimes",
        body: "A regime number in the ``restrictions`` option is larger than ``number_of_regimes``. Dynare refuses: `the regimes specified in the restrictions option must be <= the number of regimes specified in the number_of_regimes option`.

**Fix**

Correct the regime numbers, or raise ``number_of_regimes``.",
        kind: ExplainKind::Shared,
    }),
    ("E352", ExplainEntry {
        title: "markov_switching restriction given twice for one regime pair",
        body: "Two rows of the ``restrictions`` option name the same ``[from, to]`` regime pair. Dynare refuses: `two restrictions were given for: 1, 2`.

**Fix**

Keep one row per regime pair.",
        kind: ExplainKind::Shared,
    }),
    ("E353", ExplainEntry {
        title: "markov_switching transition probability above 1",
        body: "A transition probability in the ``restrictions`` option is greater than 1. Dynare refuses: `the transition probability, 1.5 must be less than 1`.

**Fix**

Pass a probability of at most 1.",
        kind: ExplainKind::Shared,
    }),
    ("E354", ExplainEntry {
        title: "markov_switching row or column of transition probabilities does not sum to 1",
        body: "Every transition out of one regime (or into it) was given, and the probabilities do not sum to 1. Dynare refuses: `When all transitions probabilities are specified for a certain regime, they must sum to 1`. One sentence covers both the row sum and the column sum.

**Fix**

Adjust the probabilities so the complete row and the complete column each sum to 1.",
        kind: ExplainKind::Shared,
    }),
    ("E355", ExplainEntry {
        title: "markov_switching partial transition probabilities sum to 1 or more",
        body: "Only some transitions out of one regime (or into it) were given, and their sum is 1 or more. Dynare refuses: `When transition probabilites are not specified for every regime, their sum must be < 1`.

**Fix**

Lower the probabilities, or give the whole row and column.",
        kind: ExplainKind::Shared,
    }),
    ("E356", ExplainEntry {
        title: "more than one svar_identification block",
        body: "The file holds a second ``svar_identification;`` … ``end;`` block. Dynare refuses: `You may only have one svar_identification block in your .mod file.`

**Fix**

Merge the two blocks into one.",
        kind: ExplainKind::Shared,
    }),
    ("E357", ExplainEntry {
        title: "svar_identification with both choleskys",
        body: "One ``svar_identification`` block holds both ``upper_cholesky;`` and ``lower_cholesky;``. Dynare refuses: `Within the svar_identification statement, you may only have one of upper_cholesky and lower_cholesky.`

**Fix**

Keep one of the two.",
        kind: ExplainKind::Shared,
    }),
    ("E358", ExplainEntry {
        title: "svar_identification exclusion lag used more than once",
        body: "The same ``exclusion lag N;`` opens two elements of one ``svar_identification`` block. Dynare refuses: `lag 0 used more than once.`

**Fix**

Put every equation of that lag under one element.",
        kind: ExplainKind::Shared,
    }),
    ("E359", ExplainEntry {
        title: "svar_identification equation number repeated under one lag",
        body: "Two ``equation N, …;`` rows under one ``exclusion lag`` carry the same equation number. Dynare refuses: `equation number 1 referenced more than once under a single lag.`

**Fix**

Name each equation once per lag.",
        kind: ExplainKind::Shared,
    }),
    ("E360", ExplainEntry {
        title: "svar_identification equation number below 1",
        body: "An ``equation`` row of ``svar_identification`` carries the number ``0``. Dynare refuses: `equation numbers must be greater than or equal to 1.` A negative number is a syntax error before this check runs, and the ``restriction equation N, …`` spelling is not range-checked.

**Fix**

Number equations from 1.",
        kind: ExplainKind::Shared,
    }),
    ("E361", ExplainEntry {
        title: "svar_identification name added twice in one equation row",
        body: "One ``equation N, name…;`` row lists the same name twice. Dynare refuses: `Py restriction added twice.`

**Fix**

List the name once.",
        kind: ExplainKind::Shared,
    }),
    ("E362", ExplainEntry {
        title: "svar_identification restriction mixes Qi and Ri",
        body: "One ``restriction equation N, …;`` uses both a ``coeff(name,0)`` term (the contemporaneous Qi matrix) and a ``coeff(name,k)`` term with ``k > 0`` (the lagged Ri matrix). Dynare refuses: `SVAR_IDENTIFICATION: a single restrictions must affect either Qi or Ri, but not both`.

**Fix**

Split the restriction into one statement per matrix.",
        kind: ExplainKind::Shared,
    }),
    ("E363", ExplainEntry {
        title: "svar without coefficients, variances or constants",
        body: "``svar`` was written without any of ``coefficients``, ``variances`` or ``constants``. Dynare refuses: `You must pass one of 'coefficients', 'variances', or 'constants'.` The third name is unreachable at this pin: ``constants`` is a token the grammar does not take, so spelling it is a syntax error.

**Fix**

Pass ``coefficients`` or ``variances``.",
        kind: ExplainKind::Shared,
    }),
    ("E364", ExplainEntry {
        title: "svar with two of coefficients, variances and constants",
        body: "``svar`` was written with two of ``coefficients``, ``variances`` and ``constants``. Dynare refuses: `You may only pass one of 'coefficients', 'variances', or 'constants'.` The third name is unreachable at this pin: ``constants`` is a token the grammar does not take, so spelling it is a syntax error.

**Fix**

Keep one of the two.",
        kind: ExplainKind::Shared,
    }),
    ("E365", ExplainEntry {
        title: "svar without the chain option",
        body: "``svar`` was written without the ``chain`` option. Dynare refuses: `A 'chain' option must be passed to the 'svar' statement.`

**Fix**

Pass ``chain=N``; the number must name the matching ``markov_switching`` chain.",
        kind: ExplainKind::Shared,
    }),
    ("E366", ExplainEntry {
        title: "svar chain is zero",
        body: "The ``chain`` option of ``svar`` is ``0``. Dynare refuses: `The value passed to the 'chain' option must be greater than zero.` This is a different sentence from the one ``markov_switching`` prints for its own ``chain``. A negative or fractional value is a syntax error before this check runs.

**Fix**

Number the chain from 1.",
        kind: ExplainKind::Shared,
    }),
    ("E367", ExplainEntry {
        title: "svar equations option holds a non-positive number",
        body: "The ``equations=[…]`` option of ``svar`` holds a number that is ``0`` or less. Dynare refuses: `The value(s) passed to the 'equations' option must be greater than zero.` An empty list and a negative value are syntax errors before this check runs.

**Fix**

Number the equations from 1.",
        kind: ExplainKind::Shared,
    }),
    ("E368", ExplainEntry {
        title: "ms_compute_probabilities with both probability options",
        body: "``ms_compute_probabilities`` was written with both ``real_time_smoothed`` and ``filtered_probabilities``. Dynare refuses: `You may only pass one of real_time_smoothed and filtered_probabilities to ms_compute_probabilities.`

**Fix**

Keep one of the two.",
        kind: ExplainKind::Shared,
    }),
    ("E369", ExplainEntry {
        title: "ms_irf with more than one regime or probability option",
        body: "``ms_irf`` was written with more than one of ``regime``, ``regimes`` and ``filtered_probabilities``. Dynare refuses: `You may only pass one of regime, regimes and filtered_probabilities to ms_irf`. No full stop.

**Fix**

Keep one of the three.",
        kind: ExplainKind::Shared,
    }),
    ("E370", ExplainEntry {
        title: "ms_forecast with both regime and regimes",
        body: "``ms_forecast`` was written with both ``regime`` and ``regimes``. Dynare refuses: `You may only pass one of regime and regimes to ms_forecast`. No full stop. ``ms_forecast`` has no ``filtered_probabilities`` option.

**Fix**

Keep one of the two.",
        kind: ExplainKind::Shared,
    }),
    ("E371", ExplainEntry {
        title: "ms_variance_decomposition with more than one regime or probability option",
        body: "``ms_variance_decomposition`` was written with more than one of ``regime``, ``regimes`` and ``filtered_probabilities``. Dynare refuses: `You may only pass one of regime, regimes and filtered_probabilities to ms_variance_decomposition`. No full stop.

**Fix**

Keep one of the three.",
        kind: ExplainKind::Shared,
    }),
    ("E372", ExplainEntry {
        title: "prior statement without the shape option",
        body: "A dotted ``prior`` statement carries no ``shape``. Dynare refuses: `You must pass the shape option to the prior statement.` The sentence is the same for the plain, ``std(…)``, ``corr(…)`` and joint forms.

**Fix**

Pass ``shape=beta`` (or the distribution you intend).",
        kind: ExplainKind::Shared,
    }),
    ("E373", ExplainEntry {
        title: "prior statement without mean or mode",
        body: "A dotted ``prior`` statement carries neither ``mean`` nor ``mode``. Dynare refuses: `You must pass at least one of mean and mode to the prior statement.` Passing both is accepted.

**Fix**

Pass ``mean=`` or ``mode=``.",
        kind: ExplainKind::Shared,
    }),
    ("E374", ExplainEntry {
        title: "prior statement without exactly one of stdev and variance",
        body: "A dotted ``prior`` statement carries neither ``stdev`` nor ``variance``, or both. Dynare refuses: `You must pass exactly one of stdev and variance to the prior statement.` One sentence covers both cases; the joint ``[a, b]`` form is not checked.

**Fix**

Pass exactly one of the two.",
        kind: ExplainKind::Shared,
    }),
    ("E375", ExplainEntry {
        title: "prior statement domain does not hold two values",
        body: "The ``domain`` option of a single ``prior`` statement does not hold exactly two values. Dynare refuses: `You must pass exactly two values to the domain option.` An empty list is a syntax error before this check runs.

**Fix**

Pass two values, such as ``domain=[0, 1]``.",
        kind: ExplainKind::Shared,
    }),
    ("E376", ExplainEntry {
        title: "joint prior statement domain does not hold four values",
        body: "The ``domain`` option of a joint ``[a, b].prior(…)`` statement does not hold exactly four values. Dynare refuses: `You must pass exactly four values to the domain option.`

**Fix**

Pass four values, such as ``domain=[0.1 0.2 0.3 0.4]``.",
        kind: ExplainKind::Shared,
    }),
    ("E377", ExplainEntry {
        title: "joint prior statement with fewer than two names",
        body: "A joint prior statement names fewer than two parameters. Dynare refuses: `you must pass at least two parameters to the joint prior statement`. The lowercase ``you`` is theirs.

**Fix**

Name two or more parameters, or use the single ``name.prior(…)`` form.",
        kind: ExplainKind::Shared,
    }),
    ("E378", ExplainEntry {
        title: "assignment or prior head is not a parameter",
        body: "A top-level `symbol = …;` assignment, or the head of a plain or bracketed ``prior`` statement, names a symbol that is not a parameter. Dynare refuses both while parsing, with the same sentence: `y is not a parameter`. The ``std(…)`` and ``corr(…)`` heads print their own sentence instead, and a line whose head the file never declares is native MATLAB text that 7.1 accepts.

**Fix**

Name a ``parameters`` symbol, or move the statement to the surface that takes this kind.",
        kind: ExplainKind::Shared,
    }),
    ("E379", ExplainEntry {
        title: "corr prior mixes an endogenous and an exogenous name",
        body: "The two names of a ``corr(A,B).prior(…)`` statement are of different types. Dynare refuses: `In the corr(A,B).prior statement, A and B must be of the same type. In your case, Pie and eps are of different types.`

**Fix**

Name two endogenous variables or two exogenous ones.",
        kind: ExplainKind::Shared,
    }),
    ("E380", ExplainEntry {
        title: "load_params_and_steady_state names an unsupported variable type",
        body: "The data file of ``load_params_and_steady_state`` holds a name that is declared, but not as a parameter, an endogenous variable, a ``varexo``, or a ``varexo_det``: an ``epilogue`` helper, an ``external_function`` name, or a trend variable. Dynare accepts the file through its checks and refuses only when writing the MATLAB files: `Unsupported variable type for A in load_params_and_steady_state`.

**Fix**

Remove the entry from the data file, or name one of the four allowed kinds.",
        kind: ExplainKind::Shared,
    }),
    ("E381", ExplainEntry {
        title: "steady_state operator contains an external function",
        body: "The expression inside a ``steady_state(…)`` operator calls an ``external_function``. Dynare accepts the file through its checks and refuses only when writing the MATLAB files: `The expression inside a steady_state operator cannot contain external functions`.

**Fix**

Move the call out of the ``steady_state(…)`` operator.",
        kind: ExplainKind::Shared,
    }),
    ("E382", ExplainEntry {
        title: "method_of_moments without a method",
        body: "A ``method_of_moments`` statement carries no ``mom_method`` option, so Dynare cannot tell GMM, SMM, and IRF matching apart. Dynare refuses: `The 'method_of_moments' statement requires a method to be supplied via the 'mom_method' option. Possible values are 'GMM', 'SMM', or 'IRF_MATCHING'.`\n\nThe option value must be one of those three bare words; a quoted or other word is a syntax error, reported as **E001**.\n\n**Fix**\n\nAdd ``mom_method=GMM``, ``mom_method=SMM``, or ``mom_method=IRF_MATCHING``.",
        kind: ExplainKind::Shared,
    }),
    ("E383", ExplainEntry {
        title: "method_of_moments without a data file",
        body: "A ``method_of_moments`` statement asks for ``GMM`` or ``SMM`` but names no ``datafile``. Both methods match moments against data, so the file is required. Dynare refuses: `The 'method_of_moments' statement requires a data file to be supplied via the 'datafile' option.`\n\n``IRF_MATCHING`` does not need one. The file itself need not exist yet at this step; a named one that is missing is **W160**.\n\n**Fix**\n\nAdd ``datafile='your_data.csv'``.",
        kind: ExplainKind::Shared,
    }),
    ("E384", ExplainEntry {
        title: "analytic option without GMM",
        body: "``analytic_standard_errors`` or ``analytic_jacobian`` is a bare flag on a ``method_of_moments`` statement whose ``mom_method`` is ``SMM`` or ``IRF_MATCHING``. Only GMM has the analytic derivatives these options ask for. Dynare refuses: `The analytic_standard_errors statement requires the GMM option.` (or ``analytic_jacobian`` in place of the first name).\n\n**Fix**\n\nDrop the option, or switch ``mom_method`` to ``GMM``.",
        kind: ExplainKind::Shared,
    }),
    ("E385", ExplainEntry {
        title: "method_of_moments with more than one filter",
        body: "A ``method_of_moments`` statement asks for more than one of ``hp_filter``, ``one_sided_hp_filter``, and ``bandpass_filter``. Dynare refuses: `method_of_moments: can only use one of HP, one-sided HP, and bandpass filters`\n\nThis is the ``method_of_moments`` sentence; ``stoch_simul`` has its own (**E238**).\n\n**Fix**\n\nKeep one filter.",
        kind: ExplainKind::Shared,
    }),
    ("E386", ExplainEntry {
        title: "Matched moment is not a supported shape",
        body: "A ``matched_moments`` row is not a shape the moment matcher accepts. Dynare refuses: `Matched moment expression has incorrect format: {reason}`, where the reason is one of:\n\n- `Variable {name} is not an endogenous`\n- `Unsupported binary operator`\n- `First argument of power expression must be a variable`\n- `Second argument of power expression must be a positive integer`\n- `Unsupported expression`\n\nA moment is a product of endogenous variables, each optionally raised to a positive whole-number power, each optionally with a lead or a lag. So ``y``, ``c*y``, ``y^2``, ``y*y(-1)`` and ``y(1)`` are accepted, while ``y+c``, ``log(y)``, ``1``, and ``y^c`` are not. The reason names the first problem found.\n\n**Fix**\n\nRewrite the row as a product of endogenous variables with positive integer powers.",
        kind: ExplainKind::Shared,
    }),
    ("E387", ExplainEntry {
        title: "Name is not exogenous",
        body: "A name in a shock slot is declared, but is neither a ``varexo`` nor a ``varexo_det``: an endogenous variable or a parameter. Dynare refuses: `{name} is not exogenous.` The slots are the ``varexo`` row of ``matched_irfs`` and of ``matched_irfs_weights``.\n\nFor the ``irf_calibration`` shock, Dynare's own sentence names the row's endogenous instead: `Variable {endo} is not an exogenous.` That sentence is this code as well.\n\n**Fix**\n\nName a ``varexo`` variable.",
        kind: ExplainKind::Shared,
    }),
    ("E388", ExplainEntry {
        title: "matched_irfs pair repeated",
        body: "One ``matched_irfs`` block lists the same endogenous/shock pair twice. Dynare refuses: `matched_irfs: the pair endogenous {y} with exogenous {e} appears two times`. The periods may differ; the pair is the key. Two separate blocks may repeat a pair silently.\n\n**Fix**\n\nMerge the two rows into one, or drop the second.",
        kind: ExplainKind::Shared,
    }),
    ("E389", ExplainEntry {
        title: "matched_irfs_weights tuple repeated",
        body: "One ``matched_irfs_weights`` block lists the same six-part tuple twice: both endogenous names with their periods, and both shock names. The weight is not part of the key, so two rows differing only in the weight are refused. Dynare refuses: `matched_irfs: the tuple ({y}({1}),{e},{c}({2}),{e}) appears two times`, with no spaces around the parts.\n\n**Fix**\n\nMerge the two rows, or drop the second.",
        kind: ExplainKind::Shared,
    }),
    ("E390", ExplainEntry {
        title: "matched_irfs periods and values counts differ",
        body: "In one ``matched_irfs`` row the ``periods`` and ``values`` lists hold different numbers of elements. Dynare refuses: `matched_irfs: the 'periods' and 'values' keywords are not followed by the same number of elements`. A range such as ``1:2`` counts as one period, and a parenthesised value such as ``(xx)`` as one value.\n\n**Fix**\n\nGive one value per period.",
        kind: ExplainKind::Shared,
    }),
    ("E391", ExplainEntry {
        title: "matched_irfs periods and weights counts differ",
        body: "In one ``matched_irfs`` row the ``weights`` list holds more than one element and does not match the number of periods. Dynare refuses: `matched_irfs: the 'periods' and 'weights' keywords are not followed by the same number of elements`. A single weight is accepted and copied across the periods.\n\n**Fix**\n\nGive one weight per period, or a single weight for the whole row.",
        kind: ExplainKind::Shared,
    }),
    ("E392", ExplainEntry {
        title: "matched_irfs periods holds a date",
        body: "A ``periods`` entry of a ``matched_irfs`` row is written as a date such as ``2000Q1``. The row counts periods after the start of the simulation, so it takes integers and integer ranges only. Dynare refuses: `matched_irfs: dates are not allowed in the 'periods' keyword`\n\n**Fix**\n\nWrite the horizon as an integer, or as an ``a:b`` range of integers.",
        kind: ExplainKind::Shared,
    }),
    ("E186", ExplainEntry {
        title: "Unused endogenous after substitution",
        body: "Dynare refuses: `Error: <name> not used in the model block`. Catching step: transform (rewrite). Owner: skip-rewrite E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("E187", ExplainEntry {
        title: "Ramsey FOCs leave Lagrange unused",
        body: "Dynare refuses: `the following Lagrange multiplier(s) do(es) not appear in first-order conditions`. Catching step: transform (rewrite). Owner: skip-rewrite E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("E188", ExplainEntry {
        title: "Equation count after AUX, Ramsey, log, or leads",
        body: "Dynare refuses: `There are <n> equations but <m> endogenous variables!`. Catching step: transform (rewrite). Owner: skip-rewrite E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("E189", ExplainEntry {
        title: "Division by zero substituting constants",
        body: "Dynare refuses: `Division by zero when substituting constants…`. Catching step: transform (rewrite). Owner: skip-rewrite E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("E190", ExplainEntry {
        title: "Partial information EXPECTATION(0) is not a single variable",
        body: "Dynare refuses: `EXPECTATION(0)(X) can only be used when X is a single variable`. Catching step: transform (rewrite). Owner: skip-rewrite E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("E191", ExplainEntry {
        title: "Excluded name still assigned in initval or endval",
        body: "Dynare refuses: `Variable … was excluded but found in an initval or endval statement`. Catching step: writer. Owner: skip-writer E. Their probe crashes with no message at this pin. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("W186", ExplainEntry {
        title: "Possible auxiliary name in a symbol list",
        body: "Dynare warns: `WARNING: symbol_list variable … possible auxiliary variable name`. Catching step: check. Owner: skip-rewrite W. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("W187", ExplainEntry {
        title: "Generated .m nests more than 32 parentheses",
        body: "Dynare warns: `A .m file created by Dynare will have more than 32 nested parenthesis…`. Catching step: writer. Owner: skip-writer W. The trigger is the nesting depth of their generated text, not of the file. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S002", ExplainEntry {
        title: "shocks(learnt_in) without PFEE setup and solver",
        body: "Dynare refuses: `'shocks(learnt_in=…)' block can only be used in conjunction with…`. Catching step: transform (written clash). Owner: skip 0.7 E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S003", ExplainEntry {
        title: "endval(learnt_in) without PFEE setup and solver",
        body: "Dynare refuses: `'endval(learnt_in=…)' block can only be used…`. Catching step: transform (written clash). Owner: skip 0.7 E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S004", ExplainEntry {
        title: "perfect_foresight_controlled_paths(learnt_in) without PFEE",
        body: "Dynare refuses: `'perfect_foresight_controlled_paths(learnt_in=…)'…`. Catching step: transform (written clash). Owner: skip 0.7 E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S005", ExplainEntry {
        title: "shock_paths(learnt_in) without PFEE setup and solver",
        body: "Dynare refuses: `'shock_paths(learnt_in=…)'…`. Catching step: transform (written clash). Owner: skip 0.7 E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S008", ExplainEntry {
        title: "Heterogeneous model equation count after AUX",
        body: "Dynare refuses: `There are <n> equations but <m> endogenous variables in the model for heterogeneity dimension`. Catching step: transform (rewrite). Owner: skip-rewrite E (0.9). This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S009", ExplainEntry {
        title: "var(log) endogenous in a VAR, TCM, or PAC equation",
        body: "Dynare refuses: `the following variables are declared with var(log) and therefore cannot appear in a VAR/TCM/PAC equation`. Catching step: transform (written clash). Owner: skip 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S010", ExplainEntry {
        title: "var_model or TCM equation name does not exist",
        body: "Dynare refuses: `no equation is named '<eqtag>' / looking for equation tag … failed`. Catching step: transform (written clash). Owner: skip 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S011", ExplainEntry {
        title: "VAR or TCM equation shape",
        body: "Dynare refuses: `in Equation <tag>. A VAR/trend component model may only…`. Catching step: transform (written clash). Owner: skip 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S012", ExplainEntry {
        title: "TCM or PAC rewrite",
        body: "Dynare refuses: `PAC/TCM rewrite messages`. Catching step: transform (rewrite). Owner: skip-rewrite 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S013", ExplainEntry {
        title: "var_expectation_model auxiliary, name, or linear form",
        body: "Dynare refuses: `var_expectation_model <name> refers to nonexistent… / not expected form / name used several times`. Catching step: transform (written clash). Owner: skip 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S014", ExplainEntry {
        title: "Remaining pac_expectation after substitution",
        body: "Dynare refuses: `unknown pac_model / no matching pac_target_info`. Catching step: transform (rewrite). Owner: skip-rewrite 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S020", ExplainEntry {
        title: "Heterogeneity with an unsupported command or option",
        body: "Dynare refuses: `'<cmd>' … is not supported for heterogeneous models`. Catching step: check. Owner: skip 0.9 E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S035", ExplainEntry {
        title: "Heterogeneous shocks with a bad type",
        body: "Dynare refuses: `not a heterogeneous exogenous variable`. Catching step: check. Owner: skip 0.9 E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S037", ExplainEntry {
        title: "shock_paths self reference without a lag",
        body: "Dynare refuses: `the use of 'self.…' without a lag is not allowed, since it is a circular reference`. Catching step: check. Owner: skip 0.7 E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S038", ExplainEntry {
        title: "PAC growth, auxname, or kind vs pac_target_info",
        body: "Dynare refuses: `PAC checkPass messages`. Catching step: check. Owner: skip 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S040", ExplainEntry {
        title: "Heterogeneous lead or lag bound",
        body: "Dynare refuses: `In model(heterogeneity=…), equation N: …`. Catching step: check. Owner: skip 0.9 E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S041", ExplainEntry {
        title: "Subsample lookup and the options / subsamples bodies",
        body: "Dynare refuses: `A subsample statement has not been issued for alpha`, and the `options` / `subsamples` statement bodies are not read. Catching step: parse. Owner: skip 0.7 E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S052", ExplainEntry {
        title: "shock_paths body, DATE, and related parse",
        body: "Dynare refuses: `various`. Catching step: parse. Owner: skip 0.7 E (DATE / `set_time` / `database` / `shock_paths` body / `mshocks` add-multiply / surprise `stderr` / `heteroskedastic_shocks`). This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S053", ExplainEntry {
        title: "PAC, var_model, TCM, or var_expectation parse",
        body: "Dynare refuses: `various`. Catching step: parse. Owner: skip 0.8 E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S054", ExplainEntry {
        title: "Heterogeneity dimension unknown or twice",
        body: "Dynare refuses: `various`. Catching step: parse. Owner: skip 0.9 E. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S062", ExplainEntry {
        title: "deterministic_trends body names a non-variable symbol",
        body: "Dynare warns: `WARNING: Non-variable symbol used in deterministic_trends: …`. Catching step: check. Owner: skip 0.8 W. This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
    ("S063", ExplainEntry {
        title: "PAC / heterogeneous operator inside epilogue",
        body: "Dynare refuses: `… forbidden in epilogue` / `… because it is heterogeneous`. Catching step: check. Owner: skip 0.8 E (var_expectation / pac_expectation / pac_target_nonstationary) and skip 0.9 E (heterogeneous). This code is never emitted.",
        kind: ExplainKind::Skipped,
    }),
];

fn entry_map() -> &'static HashMap<&'static str, ExplainEntry> {
    static MAP: OnceLock<HashMap<&'static str, ExplainEntry>> = OnceLock::new();
    MAP.get_or_init(|| ENTRIES.iter().copied().collect())
}

/// Return `{title, body}` for a diagnostic code, or `None`.
///
/// Lookup is case-insensitive.
pub fn explain(code: &str) -> Option<ExplainEntry> {
    let key = code.to_ascii_uppercase();
    entry_map().get(key.as_str()).copied()
}

/// Render the explanation as a single markdown string, or `None`.
///
/// Heading uses the caller's `code` string.
pub fn render_markdown(code: &str) -> Option<String> {
    let entry = explain(code)?;
    Some(format!("### {code}: {}\n\n{}\n", entry.title, entry.body))
}

/// Sorted catalog keys that have explanations.
pub fn known_codes() -> Vec<&'static str> {
    let mut codes: Vec<&'static str> = ENTRIES.iter().map(|(code, _)| *code).collect();
    codes.sort_unstable();
    codes
}
